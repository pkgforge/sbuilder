//! sbuild CLI - Builder for SBUILD packages
//!
//! A Rust-based builder for SBUILD package recipes that replaces the shell-based approach.

use std::{
    env,
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
            };

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

            // Collect files to push
            let files: Vec<PathBuf> = std::fs::read_dir(outdir)
                .map_err(|e| format!("Failed to read output directory: {}", e))?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();

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

            // Build GHCR repo path: base_repo/pkg_family/recipe_name
            // e.g., pkgforge/bincache/hello/static or pkgforge/pkgcache/cat/appimage.cat.stable
            let (full_repo, pkg) = if let Some(url) = recipe_url {
                if let Some((pkg_family, recipe_name)) = parse_ghcr_path(url) {
                    let full_path = format!("{}/{}/{}", base_repo, pkg_family, recipe_name);
                    info!("GHCR path: {}", full_path);
                    (full_path, recipe_name)
                } else {
                    warn!("Could not parse GHCR path from recipe URL, using base repo");
                    (base_repo.clone(), pkg_name.unwrap_or("unknown").to_string())
                }
            } else {
                (base_repo.clone(), pkg_name.unwrap_or("unknown").to_string())
            };

            let annotations = PackageAnnotations {
                pkg: pkg.clone(),
                pkg_id: "unknown".to_string(),
                pkg_type: None,
                version: version.clone(),
                description: None,
                homepage: None,
                license: None,
                build_date: chrono::Utc::now().to_rfc3339(),
                build_id: env::var("GITHUB_RUN_ID").ok(),
                build_gha: env::var("GITHUB_RUN_ID")
                    .ok()
                    .map(|id| format!("https://github.com/{}/actions/runs/{}",
                        env::var("GITHUB_REPOSITORY").unwrap_or_default(), id)),
                build_script: recipe_url.map(|s| s.to_string()),
            };

            let tag = format!("{}-{}", version, arch.to_lowercase());

            match client.push(&files, &full_repo, &tag, &annotations) {
                Ok(target) => {
                    info!("Pushed to {}", target);
                    if cli.ci {
                        write_github_env("GHCRPKG_URL", &target);
                        write_github_env("PUSH_SUCCESSFUL", "YES");
                    }
                }
                Err(e) => {
                    if cli.ci {
                        write_github_env("PUSH_SUCCESSFUL", "NO");
                    }
                    return Err(format!("GHCR push failed: {}", e));
                }
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
