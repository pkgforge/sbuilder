//! sbuild CLI - Builder for SBUILD packages
//!
//! A Rust-based builder for SBUILD package recipes that replaces the shell-based approach.

use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        self,
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use sbuild::{
    builder::Builder,
    checksum,
    ghcr::{GhcrClient, PackageAnnotations},
    signing::Signer,
    types::SoarEnv,
};
use sbuild_linter::logger::{LogManager, LogMessage};
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

/// Extract package family and recipe name from recipe URL or path
///
/// Returns (pkg_family, recipe_name) tuple
/// Example: `binaries/hello/static.yaml` -> `("hello", "static")`
/// Example: `packages/cat/appimage.cat.stable.yaml` -> `("cat", "appimage.cat.stable")`
fn parse_ghcr_path(recipe_path: &str) -> Option<(String, String)> {
    // Extract the relevant path part (after binaries/ or packages/)
    let path_part = if recipe_path.contains("/binaries/") {
        recipe_path.split("/binaries/").last()
    } else if recipe_path.contains("/packages/") {
        recipe_path.split("/packages/").last()
    } else if recipe_path.starts_with("binaries/") {
        recipe_path.strip_prefix("binaries/")
    } else if recipe_path.starts_with("packages/") {
        recipe_path.strip_prefix("packages/")
    } else {
        return None;
    };

    let path_part = path_part?;

    // Split into directory and filename: "hello/static.yaml" or "cat/appimage.cat.stable.yaml"
    let parts: Vec<&str> = path_part.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let pkg_family = parts[0].to_string();
    let filename = parts[1];

    // Remove .yaml/.yml extension
    let recipe_name = filename
        .strip_suffix(".yaml")
        .or_else(|| filename.strip_suffix(".yml"))
        .unwrap_or(filename)
        .to_string();

    Some((pkg_family, recipe_name))
}

/// Format file size in human-readable format
fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

/// Sanitize a name to be OCI repository name compliant
/// OCI repository names must be lowercase and only contain [a-z0-9._-]
fn sanitize_oci_name(name: &str) -> String {
    // First replace ++ with pp (common convention: c++ -> cpp, g++ -> gpp)
    let name = name.replace("++", "pp");

    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-' // Replace other invalid chars with -
            }
        })
        .collect::<String>()
        // Remove consecutive dashes
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Update JSON metadata file with correct GHCR URLs before pushing
fn update_json_metadata(
    json_path: &Path,
    pkg_name: &str,
    ghcr_repo: &str,
    tag: &str,
    bsum: Option<&str>,
    shasum: Option<&str>,
    binary_size: Option<u64>,
    ghcr_total_size: Option<u64>,
) -> Result<(), String> {
    // Read existing JSON
    let content = fs::read_to_string(json_path)
        .map_err(|e| format!("Failed to read JSON: {}", e))?;

    let mut json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    if let Some(obj) = json.as_object_mut() {
        // Update pkg_name
        obj.insert("pkg_name".to_string(), serde_json::json!(pkg_name));

        // Update GHCR URLs
        // ghcr_pkg: ghcr.io/{repo}:{tag}
        obj.insert("ghcr_pkg".to_string(), serde_json::json!(format!("ghcr.io/{}:{}", ghcr_repo, tag)));

        // ghcr_url: https://ghcr.io/{repo}
        obj.insert("ghcr_url".to_string(), serde_json::json!(format!("https://ghcr.io/{}", ghcr_repo)));

        // download_url: https://api.ghcr.pkgforge.dev/{repo}?tag={tag}&download={pkg_name}
        obj.insert("download_url".to_string(), serde_json::json!(format!(
            "https://api.ghcr.pkgforge.dev/{}?tag={}&download={}",
            ghcr_repo, tag, pkg_name
        )));

        // Update checksums if provided
        if let Some(b) = bsum {
            obj.insert("bsum".to_string(), serde_json::json!(b));
        }
        if let Some(s) = shasum {
            obj.insert("shasum".to_string(), serde_json::json!(s));
        }

        // Update binary size
        if let Some(s) = binary_size {
            obj.insert("size".to_string(), serde_json::json!(format_size(s)));
            obj.insert("size_raw".to_string(), serde_json::json!(s));
        }

        // Update GHCR total size (all files in package)
        if let Some(s) = ghcr_total_size {
            obj.insert("ghcr_size".to_string(), serde_json::json!(format_size(s)));
            obj.insert("ghcr_size_raw".to_string(), serde_json::json!(s));
        }
    }

    // Write back
    let updated = serde_json::to_string_pretty(&json)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))?;

    fs::write(json_path, updated)
        .map_err(|e| format!("Failed to write JSON: {}", e))?;

    Ok(())
}

/// Metadata extracted from SBUILD file for GHCR annotations
#[derive(Debug, Default)]
struct SbuildMetadata {
    pkg_id: String,
    pkg_type: Option<String>,
    description: Option<String>,
    homepage: Option<String>,
    license: Option<String>,
    ghcr_pkg: Option<String>,
    provides: Vec<String>,
}

/// Read metadata from SBUILD file in output directory
fn read_sbuild_metadata(outdir: &Path) -> Option<SbuildMetadata> {
    let sbuild_path = outdir.join("SBUILD");
    let content = fs::read_to_string(&sbuild_path).ok()?;

    // Parse YAML content
    let yaml: serde_yml::Value = serde_yml::from_str(&content).ok()?;
    let map = yaml.as_mapping()?;

    let pkg_id = map
        .get("pkg_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let pkg_type = map.get("pkg_type").and_then(|v| v.as_str()).map(String::from);

    // Description can be a string or a map with short/long
    let description = map.get("description").and_then(|v| {
        if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else if let Some(m) = v.as_mapping() {
            m.get("short").and_then(|s| s.as_str()).map(String::from)
        } else {
            None
        }
    });

    // Homepage is an array, take the first one
    let homepage = map.get("homepage").and_then(|v| {
        if let Some(arr) = v.as_sequence() {
            arr.first().and_then(|s| s.as_str()).map(String::from)
        } else {
            v.as_str().map(String::from)
        }
    });

    // License is an array of strings or complex objects
    let license = map.get("license").and_then(|v| {
        if let Some(arr) = v.as_sequence() {
            let licenses: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    if let Some(s) = item.as_str() {
                        Some(s.to_string())
                    } else if let Some(m) = item.as_mapping() {
                        m.get("id").and_then(|id| id.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
                .collect();
            if licenses.is_empty() {
                None
            } else {
                Some(licenses.join(", "))
            }
        } else {
            v.as_str().map(String::from)
        }
    });

    // ghcr_pkg is a string for custom GHCR path
    let ghcr_pkg = map
        .get("ghcr_pkg")
        .and_then(|v| v.as_str())
        .map(String::from);

    // provides is an array of package names (with possible annotations)
    // Parse to extract unique package names: "prog:alias" -> "prog", "prog==sym" -> "prog", "prog=>rename" -> "prog"
    let provides: Vec<String> = map
        .get("provides")
        .and_then(|v| v.as_sequence())
        .map(|arr| {
            let mut seen = std::collections::HashSet::new();
            arr.iter()
                .filter_map(|item| item.as_str())
                .filter_map(|s| {
                    // Extract base package name before any annotation
                    let pkg_name = s
                        .split(|c| c == ':' || c == '=')
                        .next()
                        .unwrap_or(s)
                        .to_string();
                    // Deduplicate
                    if seen.insert(pkg_name.clone()) {
                        Some(pkg_name)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Some(SbuildMetadata {
        pkg_id,
        pkg_type,
        description,
        homepage,
        license,
        ghcr_pkg,
        provides,
    })
}

/// Log level for build output
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum LogLevel {
    /// Minimal output
    #[default]
    Info,
    /// Verbose output
    Verbose,
    /// Debug output with maximum detail
    Debug,
}

impl From<LogLevel> for u8 {
    fn from(level: LogLevel) -> u8 {
        match level {
            LogLevel::Info => 1,
            LogLevel::Verbose => 2,
            LogLevel::Debug => 3,
        }
    }
}

/// sbuild - Builder for SBUILD packages
#[derive(Parser)]
#[command(name = "sbuild")]
#[command(about = "Build packages from SBUILD recipes", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build packages from SBUILD recipes
    Build(BuildArgs),

    /// Get information about an SBUILD recipe
    Info(InfoArgs),
}

#[derive(Parser)]
struct BuildArgs {
    /// SBUILD recipe files or URLs to build
    #[arg(required = true)]
    recipes: Vec<String>,

    /// Output directory for build artifacts
    #[arg(short, long)]
    outdir: Option<PathBuf>,

    /// Keep temporary build directory after completion
    #[arg(short, long)]
    keep: bool,

    /// Build timeout in seconds
    #[arg(long, default_value = "3600")]
    timeout: u64,

    /// Linter timeout in seconds
    #[arg(long, default_value = "30")]
    timeout_linter: u64,

    /// Log level for build output
    #[arg(long, value_enum, default_value = "info")]
    log_level: LogLevel,

    /// CI mode - output GitHub Actions environment variables
    #[arg(long)]
    ci: bool,

    /// Force rebuild even if package exists
    #[arg(long)]
    force: bool,

    /// GitHub token for authenticated requests
    #[arg(long, env = "GITHUB_TOKEN")]
    github_token: Option<String>,

    /// GHCR token for pushing packages
    #[arg(long, env = "GHCR_TOKEN")]
    ghcr_token: Option<String>,

    /// GHCR repository base (e.g., pkgforge/bincache)
    #[arg(long)]
    ghcr_repo: Option<String>,

    /// Push packages to GHCR after build
    #[arg(long)]
    push: bool,

    /// Sign packages with minisign
    #[arg(long)]
    sign: bool,

    /// Minisign private key (or path to key file)
    #[arg(long, env = "MINISIGN_KEY")]
    minisign_key: Option<String>,

    /// Minisign private key password
    #[arg(long, env = "MINISIGN_PASSWORD")]
    minisign_password: Option<String>,

    /// Generate checksums for built artifacts
    #[arg(long, default_value = "true")]
    checksums: bool,
}

#[derive(Parser)]
struct InfoArgs {
    /// SBUILD recipe file or URL
    #[arg(required = true)]
    recipe: String,

    /// Check if recipe supports this host (e.g., x86_64-Linux)
    #[arg(long)]
    check_host: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value = "text")]
    format: OutputFormat,

    /// Output specific field (pkg, pkg_id, version, hosts, etc.)
    #[arg(long)]
    field: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum OutputFormat {
    #[default]
    Text,
    Json,
}

fn init_logging(ci_mode: bool, log_level: LogLevel) {
    let level = match log_level {
        LogLevel::Debug => Level::DEBUG,
        LogLevel::Verbose => Level::DEBUG,
        LogLevel::Info => {
            if ci_mode {
                Level::INFO
            } else {
                Level::INFO
            }
        }
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .with_thread_ids(false)
        .without_time()
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();
}

fn get_soar_env() -> Option<SoarEnv> {
    let cmd = Command::new("soar").arg("env").output();
    let mut soar_env = SoarEnv::default();

    if let Ok(cmd_output) = cmd {
        if cmd_output.status.success() && cmd_output.stderr.is_empty() {
            let output_str = String::from_utf8_lossy(&cmd_output.stdout);
            for line in output_str.lines() {
                if let Some(value) = line.strip_prefix("SOAR_CACHE=") {
                    soar_env.cache_path = value.to_string();
                }
                if let Some(value) = line.strip_prefix("SOAR_BIN=") {
                    soar_env.bin_path = value.to_string();
                }
            }
            return Some(soar_env);
        }
    }
    None
}

/// Fetch a recipe from a URL
async fn fetch_recipe(url: &str) -> Result<String, String> {
    debug!("Fetching recipe from {}", url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch recipe: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error {}: {}", response.status(), url));
    }

    response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))
}

/// Write to GitHub Actions environment file
fn write_github_env(key: &str, value: &str) {
    if let Ok(env_file) = env::var("GITHUB_ENV") {
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&env_file) {
            use std::io::Write;
            writeln!(file, "{}={}", key, value).ok();
        }
    }
}

/// Write GitHub Actions output
fn write_github_output(key: &str, value: &str) {
    if let Ok(output_file) = env::var("GITHUB_OUTPUT") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .open(&output_file)
        {
            use std::io::Write;
            writeln!(file, "{}={}", key, value).ok();
        }
    }
}

/// Post-build processing: checksums, signing, push
async fn post_build_processing(
    outdir: &Path,
    cli: &BuildArgs,
    recipe_url: Option<&str>,
    pkg_name: Option<&str>,
) -> Result<(), String> {
    // Generate checksums
    if cli.checksums {
        info!("Generating checksums...");
        match checksum::generate_checksum_file(outdir) {
            Ok(_) => info!("Checksums generated"),
            Err(e) => warn!("Failed to generate checksums: {}", e),
        }
    }

    // Sign artifacts
    if cli.sign {
        if let Some(ref key) = cli.minisign_key {
            info!("Signing artifacts...");

            // Check if key is a file path or key data
            let signer = if Path::new(key).exists() {
                Signer::with_key_file(key)
            } else {
                Signer::with_key_data(key.clone())
            }
            .with_password(cli.minisign_password.clone());

            if let Err(e) = Signer::check_minisign() {
                return Err(format!("Signing failed: {}", e));
            }

            match signer.sign_directory(outdir) {
                Ok(signed) => info!("Signed {} files", signed.len()),
                Err(e) => return Err(format!("Signing failed: {}", e)),
            }
        } else {
            warn!("--sign specified but no --minisign-key provided");
        }
    }

    // Push to GHCR
    if cli.push {
        if let (Some(ref token), Some(ref base_repo)) = (&cli.ghcr_token, &cli.ghcr_repo) {
            info!("Pushing to GHCR...");

            if let Err(e) = GhcrClient::check_oras() {
                return Err(format!("GHCR push failed: {}", e));
            }

            let client = GhcrClient::new(token.clone());

            if let Err(e) = client.login() {
                return Err(format!("GHCR login failed: {}", e));
            }

            // Read version from .version file
            let version = std::fs::read_dir(outdir)
                .ok()
                .and_then(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .find(|e| e.path().extension().map(|ext| ext == "version").unwrap_or(false))
                        .and_then(|e| std::fs::read_to_string(e.path()).ok())
                })
                .unwrap_or_else(|| "latest".to_string())
                .trim()
                .to_string();

            // Get architecture
            let arch = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
            let tag = format!("{}-{}", version, arch.to_lowercase());

            // Get pkg_family and recipe_name from recipe URL
            let (pkg_family, recipe_name) = recipe_url
                .and_then(|url| parse_ghcr_path(url))
                .unwrap_or_else(|| {
                    let default = pkg_name.unwrap_or("unknown").to_string();
                    (default.clone(), default)
                });

            // Read metadata from SBUILD file
            let metadata = read_sbuild_metadata(outdir).unwrap_or_default();

            let mut push_success = true;
            let mut pushed_urls = Vec::new();

            // Check for soar-packages/ directory (explicit multi-package structure)
            let soar_packages_dir = outdir.join("soar-packages");

            if soar_packages_dir.is_dir() {
                // Explicit multi-package mode
                info!("Found soar-packages/ directory, using explicit package structure");

                // Collect shared files from root (everything except soar-packages/)
                let shared_files: Vec<PathBuf> = std::fs::read_dir(outdir)
                    .map_err(|e| format!("Failed to read output directory: {}", e))?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect();

                // Each subdirectory in soar-packages/ is a package
                let package_dirs: Vec<PathBuf> = std::fs::read_dir(&soar_packages_dir)
                    .map_err(|e| format!("Failed to read soar-packages directory: {}", e))?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();

                if package_dirs.is_empty() {
                    warn!("soar-packages/ directory is empty");
                    return Ok(());
                }

                for pkg_dir in &package_dirs {
                    let pkg_name_dir = pkg_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");

                    // Build GHCR repo path for this package
                    // Sanitize package name for OCI compatibility (e.g., c++filt -> c-filt)
                    let sanitized_pkg_name = sanitize_oci_name(pkg_name_dir);
                    // Use ghcr_pkg if specified, otherwise use auto-generated path
                    let full_repo = if let Some(ref custom_base) = metadata.ghcr_pkg {
                        format!("{}/{}", custom_base, sanitized_pkg_name)
                    } else {
                        format!("{}/{}/{}/{}", base_repo, pkg_family, recipe_name, sanitized_pkg_name)
                    };
                    info!("Pushing package {} to {}", pkg_name_dir, full_repo);

                    // Collect files: package-specific files + shared files
                    let mut files_to_push: Vec<PathBuf> = std::fs::read_dir(pkg_dir)
                        .map_err(|e| format!("Failed to read package directory: {}", e))?
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.is_file())
                        .collect();

                    // Add shared files from root
                    files_to_push.extend(shared_files.clone());

                    // Find the main binary (same name as directory)
                    let main_binary = files_to_push.iter().find(|f| {
                        f.file_name().and_then(|n| n.to_str()) == Some(pkg_name_dir)
                    });

                    // Compute checksums and size for the main binary
                    let (bsum, shasum, binary_size) = if let Some(binary_path) = main_binary {
                        let size = fs::metadata(binary_path).ok().map(|m| m.len());
                        (
                            checksum::b3sum(binary_path).ok(),
                            checksum::sha256sum(binary_path).ok(),
                            size,
                        )
                    } else {
                        (None, None, None)
                    };

                    // Calculate total GHCR size (all files being pushed)
                    let ghcr_total_size: u64 = files_to_push
                        .iter()
                        .filter_map(|f| fs::metadata(f).ok().map(|m| m.len()))
                        .sum();

                    // Update JSON metadata with correct GHCR URLs
                    let json_path = pkg_dir.join(format!("{}.json", pkg_name_dir));
                    if json_path.exists() {
                        if let Err(e) = update_json_metadata(
                            &json_path,
                            pkg_name_dir,
                            &full_repo,
                            &tag,
                            bsum.as_deref(),
                            shasum.as_deref(),
                            binary_size,
                            Some(ghcr_total_size),
                        ) {
                            warn!("Failed to update JSON metadata: {}", e);
                        }
                    }

                    let annotations = PackageAnnotations {
                        pkg: pkg_name_dir.to_string(),
                        pkg_id: metadata.pkg_id.clone(),
                        pkg_type: metadata.pkg_type.clone(),
                        version: version.clone(),
                        description: metadata.description.clone(),
                        homepage: metadata.homepage.clone(),
                        license: metadata.license.clone(),
                        build_date: chrono::Utc::now().to_rfc3339(),
                        build_id: env::var("GITHUB_RUN_ID").ok(),
                        build_gha: env::var("GITHUB_RUN_ID")
                            .ok()
                            .map(|id| format!("https://github.com/{}/actions/runs/{}",
                                env::var("GITHUB_REPOSITORY").unwrap_or_default(), id)),
                        build_script: recipe_url.map(|s| s.to_string()),
                        bsum,
                        shasum,
                    };

                    match client.push(&files_to_push, &full_repo, &tag, &annotations) {
                        Ok(target) => {
                            info!("Pushed {} to {}", pkg_name_dir, target);
                            pushed_urls.push(target);
                        }
                        Err(e) => {
                            error!("Failed to push {}: {}", pkg_name_dir, e);
                            push_success = false;
                        }
                    }
                }
            } else {
                // Use provides field to identify which binaries to push
                info!("Using provides field for package detection");

                // Collect all files
                let all_files: Vec<PathBuf> = std::fs::read_dir(outdir)
                    .map_err(|e| format!("Failed to read output directory: {}", e))?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && !p.is_symlink())
                    .collect();

                // Get binary files that match the provides list
                let binary_files: Vec<&PathBuf> = if !metadata.provides.is_empty() {
                    all_files
                        .iter()
                        .filter(|p| {
                            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            metadata.provides.contains(&name.to_string())
                        })
                        .collect()
                } else {
                    // Fallback: if no provides, use heuristic detection
                    warn!("No provides field found, falling back to heuristic detection");
                    all_files
                        .iter()
                        .filter(|p| {
                            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            // Exclude metadata files and known non-binary files
                            !matches!(ext, "json" | "log" | "version" | "sig" | "minisig" | "txt" | "yaml" | "yml" | "png" | "svg")
                                && !matches!(name, "CHECKSUM" | "SBUILD" | "LICENSE" | "README" | "CHANGELOG")
                        })
                        .collect()
                };

                if binary_files.is_empty() {
                    warn!("No binary files found to push");
                    return Ok(());
                }

                // Collect shared files (files that don't match any binary name pattern)
                let shared_files: Vec<&PathBuf> = all_files
                    .iter()
                    .filter(|p| {
                        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        // Shared if no binary has this stem as its name
                        !binary_files.iter().any(|b| {
                            b.file_name().and_then(|n| n.to_str()) == Some(stem)
                        })
                    })
                    .filter(|p| {
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                        // Only include certain file types as shared
                        matches!(ext, "log" | "version" | "txt")
                    })
                    .collect();

                // Push each binary separately to {base_repo}/{pkg_family}/{recipe_name}/{binary_name}
                for binary_path in &binary_files {
                    let binary_name = binary_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");

                    // Build GHCR repo path for this binary
                    // Sanitize binary name for OCI compatibility (e.g., c++filt -> c-filt)
                    let sanitized_binary_name = sanitize_oci_name(binary_name);
                    // Use ghcr_pkg if specified, otherwise use auto-generated path
                    let full_repo = if let Some(ref custom_base) = metadata.ghcr_pkg {
                        format!("{}/{}", custom_base, sanitized_binary_name)
                    } else {
                        format!("{}/{}/{}/{}", base_repo, pkg_family, recipe_name, sanitized_binary_name)
                    };
                    info!("Pushing {} to {}", binary_name, full_repo);

                    // Collect files for this binary: binary + associated metadata + shared files
                    let mut files_to_push: Vec<PathBuf> = vec![(*binary_path).clone()];

                    // Add associated files (json, sig, png, svg, log)
                    for ext in &["json", "sig", "png", "svg", "log"] {
                        let assoc_file = outdir.join(format!("{}.{}", binary_name, ext));
                        if assoc_file.exists() {
                            files_to_push.push(assoc_file);
                        }
                    }

                    // Add shared files
                    for shared in &shared_files {
                        files_to_push.push((*shared).clone());
                    }

                    // Compute checksums and size for this binary
                    let bsum = checksum::b3sum(binary_path).ok();
                    let shasum = checksum::sha256sum(binary_path).ok();
                    let binary_size = fs::metadata(binary_path).ok().map(|m| m.len());

                    // Calculate total GHCR size (all files being pushed)
                    let ghcr_total_size: u64 = files_to_push
                        .iter()
                        .filter_map(|f| fs::metadata(f).ok().map(|m| m.len()))
                        .sum();

                    // Update JSON metadata with correct GHCR URLs
                    let json_path = outdir.join(format!("{}.json", binary_name));
                    if json_path.exists() {
                        if let Err(e) = update_json_metadata(
                            &json_path,
                            binary_name,
                            &full_repo,
                            &tag,
                            bsum.as_deref(),
                            shasum.as_deref(),
                            binary_size,
                            Some(ghcr_total_size),
                        ) {
                            warn!("Failed to update JSON metadata: {}", e);
                        }
                    }

                    let annotations = PackageAnnotations {
                        pkg: binary_name.to_string(),
                        pkg_id: metadata.pkg_id.clone(),
                        pkg_type: metadata.pkg_type.clone(),
                        version: version.clone(),
                        description: metadata.description.clone(),
                        homepage: metadata.homepage.clone(),
                        license: metadata.license.clone(),
                        build_date: chrono::Utc::now().to_rfc3339(),
                        build_id: env::var("GITHUB_RUN_ID").ok(),
                        build_gha: env::var("GITHUB_RUN_ID")
                            .ok()
                            .map(|id| format!("https://github.com/{}/actions/runs/{}",
                                env::var("GITHUB_REPOSITORY").unwrap_or_default(), id)),
                        build_script: recipe_url.map(|s| s.to_string()),
                        bsum,
                        shasum,
                    };

                    match client.push(&files_to_push, &full_repo, &tag, &annotations) {
                        Ok(target) => {
                            info!("Pushed {} to {}", binary_name, target);
                            pushed_urls.push(target);
                        }
                        Err(e) => {
                            error!("Failed to push {}: {}", binary_name, e);
                            push_success = false;
                        }
                    }
                }
            }

            if cli.ci {
                if push_success && !pushed_urls.is_empty() {
                    write_github_env("GHCRPKG_URL", &pushed_urls.join(","));
                    write_github_env("PUSH_SUCCESSFUL", "YES");
                } else {
                    write_github_env("PUSH_SUCCESSFUL", "NO");
                }
            }

            if !push_success {
                return Err("One or more GHCR pushes failed".to_string());
            }
        } else {
            warn!("--push specified but --ghcr-token or --ghcr-repo not provided");
        }
    }

    Ok(())
}

/// Handle the info subcommand
async fn handle_info(args: InfoArgs) -> Result<(), String> {
    // Fetch recipe content
    let content = if args.recipe.starts_with("http://") || args.recipe.starts_with("https://") {
        fetch_recipe(&args.recipe).await?
    } else {
        std::fs::read_to_string(&args.recipe)
            .map_err(|e| format!("Failed to read recipe: {}", e))?
    };

    // Parse YAML
    let yaml: serde_yml::Value = serde_yml::from_str(&content)
        .map_err(|e| format!("Failed to parse YAML: {}", e))?;

    // Check host compatibility if requested
    if let Some(ref check_host) = args.check_host {
        let hosts = yaml
            .get("x_exec")
            .and_then(|x| x.get("host"))
            .and_then(|h| h.as_sequence());

        if let Some(host_list) = hosts {
            let supported: Vec<&str> = host_list
                .iter()
                .filter_map(|h| h.as_str())
                .collect();

            let is_supported = supported.iter().any(|h| {
                h.eq_ignore_ascii_case(check_host)
            });

            if !is_supported {
                eprintln!("Recipe does not support host: {}", check_host);
                eprintln!("Supported hosts: {:?}", supported);
                std::process::exit(1);
            }

            println!("Host {} is supported", check_host);
            return Ok(());
        } else {
            // No host restriction means all hosts are supported
            println!("Host {} is supported (no restrictions)", check_host);
            return Ok(());
        }
    }

    // Output specific field if requested
    if let Some(ref field) = args.field {
        let value = match field.as_str() {
            "pkg" => yaml.get("pkg").and_then(|v| v.as_str()).map(|s| s.to_string()),
            "pkg_id" => yaml.get("pkg_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            "pkg_name" => yaml.get("pkg_name").and_then(|v| v.as_str()).map(|s| s.to_string()),
            "pkg_type" => yaml.get("pkg_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
            "description" => yaml.get("description").and_then(|v| v.as_str()).map(|s| s.to_string()),
            "version" => yaml.get("version").and_then(|v| v.as_str()).map(|s| s.to_string()),
            "hosts" => {
                yaml.get("x_exec")
                    .and_then(|x| x.get("host"))
                    .and_then(|h| h.as_sequence())
                    .map(|hosts| {
                        hosts.iter()
                            .filter_map(|h| h.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
            }
            _ => {
                // Try to get arbitrary field
                yaml.get(field).map(|v| {
                    match v {
                        serde_yml::Value::String(s) => s.clone(),
                        serde_yml::Value::Bool(b) => b.to_string(),
                        serde_yml::Value::Number(n) => n.to_string(),
                        _ => serde_yml::to_string(v).unwrap_or_default(),
                    }
                })
            }
        };

        match value {
            Some(v) => {
                println!("{}", v);
                Ok(())
            }
            None => {
                eprintln!("Field '{}' not found", field);
                std::process::exit(1);
            }
        }
    } else {
        // Output full info
        match args.format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&yaml)
                    .map_err(|e| format!("Failed to convert to JSON: {}", e))?;
                println!("{}", json);
            }
            OutputFormat::Text => {
                println!("{}: {}", "pkg".bright_cyan(), yaml.get("pkg").and_then(|v| v.as_str()).unwrap_or("N/A"));
                println!("{}: {}", "pkg_id".bright_cyan(), yaml.get("pkg_id").and_then(|v| v.as_str()).unwrap_or("N/A"));
                println!("{}: {}", "pkg_name".bright_cyan(), yaml.get("pkg_name").and_then(|v| v.as_str()).unwrap_or("N/A"));
                println!("{}: {}", "pkg_type".bright_cyan(), yaml.get("pkg_type").and_then(|v| v.as_str()).unwrap_or("N/A"));
                println!("{}: {}", "description".bright_cyan(), yaml.get("description").and_then(|v| v.as_str()).unwrap_or("N/A"));

                if let Some(hosts) = yaml.get("x_exec").and_then(|x| x.get("host")).and_then(|h| h.as_sequence()) {
                    let host_list: Vec<&str> = hosts.iter().filter_map(|h| h.as_str()).collect();
                    println!("{}: {}", "hosts".bright_cyan(), host_list.join(", "));
                } else {
                    println!("{}: {}", "hosts".bright_cyan(), "all (no restrictions)");
                }
            }
        }
        Ok(())
    }
}

/// Handle the build subcommand
async fn handle_build(args: BuildArgs) {
    init_logging(args.ci, args.log_level);

    println!(
        "{} v{}",
        "sbuild".bright_cyan().bold(),
        env!("CARGO_PKG_VERSION")
    );

    // Get soar environment (optional - we can work without it)
    let soar_env = get_soar_env().unwrap_or_else(|| {
        debug!("soar not available, using default paths");
        SoarEnv::default()
    });

    let now = Instant::now();
    let success = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));

    let (tx, rx) = sync::mpsc::channel();
    let log_manager = LogManager::new(tx.clone());

    // Logger thread for build output
    let logger_handle = thread::spawn(move || {
        let check = "✔".bright_green().bold();
        let cross = "✗".bright_red().bold();
        let warning = "⚠".bright_yellow().bold();

        while let Ok(log) = rx.recv() {
            match log {
                LogMessage::Info(msg) => println!("{}", msg),
                LogMessage::Error(msg) => eprintln!("[{}] {}", cross, msg),
                LogMessage::Warn(msg) => eprintln!("[{}] {}", warning, msg),
                LogMessage::Success(msg) => println!("[{}] {}", check, msg),
                LogMessage::CustomError(msg) => eprintln!("{}", msg),
                LogMessage::Done => break,
            }
        }
    });

    for recipe_input in &args.recipes {
        // Determine if input is URL or local file
        let (recipe_path, recipe_url) = if recipe_input.starts_with("http://")
            || recipe_input.starts_with("https://")
        {
            match fetch_recipe(recipe_input).await {
                Ok(content) => {
                    // Write to temp file for builder
                    let temp_path =
                        std::env::temp_dir().join(format!("sbuild-{}.yaml", uuid_simple()));
                    if let Err(e) = std::fs::write(&temp_path, &content) {
                        error!("Failed to write temp recipe: {}", e);
                        fail.fetch_add(1, Ordering::SeqCst);
                        continue;
                    }
                    (temp_path.to_string_lossy().to_string(), Some(recipe_input.as_str()))
                }
                Err(e) => {
                    error!("Failed to fetch recipe {}: {}", recipe_input, e);
                    fail.fetch_add(1, Ordering::SeqCst);
                    continue;
                }
            }
        } else {
            (recipe_input.clone(), None)
        };

        let named_temp_file = tempfile::Builder::new()
            .prefix("sbuild-log-")
            .rand_bytes(8)
            .tempfile()
            .expect("Failed to create temp file");
        let tmp_file_path = named_temp_file.path().to_path_buf();
        let logger = log_manager.create_logger(Some(tmp_file_path));

        let now_time = chrono::Utc::now();
        logger.write_to_file(format!(
            "sbuild v{} [{}]",
            env!("CARGO_PKG_VERSION"),
            now_time.format("%A, %B %d, %Y %H:%M:%S UTC")
        ));

        let mut builder = Builder::new(
            logger.clone(),
            soar_env.clone(),
            true, // external
            args.log_level.into(),
            args.keep,
            Duration::from_secs(args.timeout),
        );

        info!("Building: {}", recipe_input);

        let outdir_str = args.outdir.as_ref().map(|p| p.to_string_lossy().to_string());

        if builder
            .build(
                &recipe_path,
                outdir_str.clone(),
                Duration::from_secs(args.timeout_linter),
            )
            .await
        {
            success.fetch_add(1, Ordering::SeqCst);

            if args.ci {
                write_github_env("SBUILD_SUCCESSFUL", "YES");
            }

            // Post-build processing if outdir is specified
            if let Some(ref outdir) = args.outdir {
                // Find the actual output directory (it includes pkg_id subdirectory)
                if let Ok(entries) = std::fs::read_dir(outdir) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_dir() {
                            // Get pkg name from directory name (it's the pkg or pkg_id)
                            let pkg_name = path.file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string());
                            if let Err(e) = post_build_processing(
                                &path,
                                &args,
                                recipe_url,
                                pkg_name.as_deref(),
                            ).await {
                                warn!("Post-build processing failed: {}", e);
                            }
                        }
                    }
                }
            }
        } else {
            fail.fetch_add(1, Ordering::SeqCst);

            if args.ci {
                write_github_env("SBUILD_SUCCESSFUL", "NO");
                write_github_env("GHA_BUILD_FAILED", "YES");
            }
        }
    }

    log_manager.done();
    logger_handle.join().unwrap();

    // Summary
    println!();
    let success_count = success.load(Ordering::SeqCst);
    let fail_count = fail.load(Ordering::SeqCst);
    let total = success_count + fail_count;

    println!(
        "[{}] {} of {} packages built successfully",
        "+".bright_blue().bold(),
        success_count,
        total,
    );

    if fail_count > 0 {
        println!(
            "[{}] {} packages failed",
            "-".bright_red().bold(),
            fail_count,
        );
    }

    println!(
        "[{}] Completed in {:.2?}",
        "⏱".bright_blue(),
        now.elapsed()
    );

    if args.ci {
        write_github_output("success_count", &success_count.to_string());
        write_github_output("fail_count", &fail_count.to_string());
    }

    // Exit with error if any builds failed
    if fail_count > 0 {
        std::process::exit(1);
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build(args) => handle_build(args).await,
        Commands::Info(args) => {
            if let Err(e) = handle_info(args).await {
                eprintln!("{}: {}", "Error".bright_red(), e);
                std::process::exit(1);
            }
        }
    }
}

/// Generate a simple UUID-like string for temp files
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", duration.as_secs(), duration.subsec_nanos())
}
