//! sbuild-meta CLI
//!
//! Command-line interface for metadata generation and version checking.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{debug, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use sbuild_meta::{
    hash::{compute_recipe_hash, compute_recipe_hash_excluding_version},
    manifest::OciManifest,
    metadata::PackageMetadata,
    recipe::{filter_by_arch, filter_enabled, scan_recipes, SBuildRecipe},
    registry::RegistryClient,
    Error, Result,
};

#[derive(Parser)]
#[command(name = "sbuild-meta")]
#[command(about = "Metadata generator for SBUILD packages", long_about = None)]
#[command(version)]
struct Cli {
    /// Log level (error, warn, info, debug, trace)
    #[arg(long, default_value = "trace")]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate metadata for packages
    Generate {
        /// Target architecture (x86_64-Linux, aarch64-Linux, riscv64-Linux)
        #[arg(short, long)]
        arch: String,

        /// Recipe directories to scan
        #[arg(short, long, num_args = 1..)]
        recipes: Vec<PathBuf>,

        /// Output directory for JSON files (creates {cache_type}/{arch}.json)
        #[arg(short, long)]
        output: PathBuf,

        /// Cache type to generate (bincache, pkgcache, or all)
        #[arg(long, default_value = "all")]
        cache_type: String,

        /// Historical cache database (optional)
        #[arg(short, long)]
        cache: Option<PathBuf>,

        /// Number of parallel workers
        #[arg(short, long, default_value = "4")]
        parallel: usize,

        /// GitHub token for registry access
        #[arg(long, env = "GITHUB_TOKEN")]
        github_token: Option<String>,

        /// GHCR owner/organization (default: pkgforge)
        #[arg(long, default_value = "pkgforge")]
        ghcr_owner: String,
    },

    /// Check if a recipe should be rebuilt
    ShouldRebuild {
        /// Path to SBUILD recipe
        #[arg(short, long)]
        recipe: PathBuf,

        /// Path to cache database
        #[arg(short, long)]
        cache: Option<PathBuf>,

        /// Force rebuild regardless of status
        #[arg(short, long)]
        force: bool,
    },

    /// Check for upstream updates
    CheckUpdates {
        /// Recipe directories to scan
        #[arg(short, long, num_args = 1..)]
        recipes: Vec<PathBuf>,

        /// Path to cache database
        #[arg(short, long)]
        cache: Option<PathBuf>,

        /// Output JSON file with outdated packages
        #[arg(short, long)]
        output: PathBuf,

        /// Number of parallel workers
        #[arg(short, long, default_value = "10")]
        parallel: usize,

        /// Timeout for pkgver script execution (in seconds)
        #[arg(long, default_value = "30")]
        timeout: u64,
    },

    /// Compute hash of a recipe
    Hash {
        /// Path to SBUILD recipe
        recipe: PathBuf,

        /// Exclude version field from hash
        #[arg(long)]
        exclude_version: bool,
    },

    /// Fetch and display manifest for a package
    FetchManifest {
        /// Package repository (e.g., pkgforge/bincache/bat)
        #[arg(short, long)]
        repository: String,

        /// Tag to fetch (optional, uses latest arch-specific if not provided)
        #[arg(short, long)]
        tag: Option<String>,

        /// Target architecture
        #[arg(short, long, default_value = "x86_64-Linux")]
        arch: String,

        /// GitHub token for registry access
        #[arg(long, env = "GITHUB_TOKEN")]
        github_token: Option<String>,
    },
}

fn setup_logging(level: &str) {
    let level = match level.to_lowercase().as_str() {
        "error" => Level::ERROR,
        "warn" => Level::WARN,
        "info" => Level::INFO,
        "debug" => Level::DEBUG,
        "trace" => Level::TRACE,
        _ => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .compact()
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    setup_logging(&cli.log_level);

    match cli.command {
        Commands::Generate {
            arch,
            recipes,
            output,
            cache_type,
            cache,
            parallel,
            github_token,
            ghcr_owner,
        } => {
            cmd_generate(
                arch,
                recipes,
                output,
                cache_type,
                cache,
                parallel,
                github_token,
                ghcr_owner,
            )
            .await
        }

        Commands::ShouldRebuild {
            recipe,
            cache,
            force,
        } => cmd_should_rebuild(recipe, cache, force).await,

        Commands::CheckUpdates {
            recipes,
            cache,
            output,
            parallel,
            timeout,
        } => cmd_check_updates(recipes, cache, output, parallel, timeout).await,

        Commands::Hash {
            recipe,
            exclude_version,
        } => cmd_hash(recipe, exclude_version),

        Commands::FetchManifest {
            repository,
            tag,
            arch,
            github_token,
        } => cmd_fetch_manifest(repository, tag, arch, github_token).await,
    }
}

async fn cmd_generate(
    arch: String,
    recipe_dirs: Vec<PathBuf>,
    output_dir: PathBuf,
    cache_type_filter: String,
    _cache: Option<PathBuf>,
    _parallel: usize,
    github_token: Option<String>,
    ghcr_owner: String,
) -> Result<()> {
    info!(
        "Generating metadata for {} (cache: {})",
        arch, cache_type_filter
    );

    // Create registry client (uses anonymous auth)
    let _ = github_token; // Token not used for public registry access
    let client = RegistryClient::new();

    // Scan all recipe directories
    let mut all_recipes = Vec::new();
    for dir in &recipe_dirs {
        info!("Scanning recipes in {:?}", dir);
        let recipes = scan_recipes(dir)?;
        all_recipes.extend(recipes);
    }

    info!("Found {} total recipes", all_recipes.len());

    // Filter by arch and enabled status
    let recipes = filter_enabled(filter_by_arch(all_recipes, &arch));
    info!("After filtering: {} recipes for {}", recipes.len(), arch);

    // Separate metadata by cache type
    let mut bincache_metadata: Vec<PackageMetadata> = Vec::new();
    let mut pkgcache_metadata: Vec<PackageMetadata> = Vec::new();

    for (path, recipe) in recipes {
        // Get all GHCR packages for this recipe (handles multiple binaries)
        let ghcr_packages = recipe.ghcr_packages_from_path(&path, &ghcr_owner);

        if ghcr_packages.is_empty() {
            warn!("No GHCR packages found for {:?}", path);
            continue;
        }

        for ghcr_info in &ghcr_packages {
            // Filter by cache type if specified
            if cache_type_filter != "all" && ghcr_info.cache_type != cache_type_filter {
                continue;
            }

            info!(
                "Processing: {} -> {} ({:?})",
                recipe.pkg, ghcr_info.pkg_name, path
            );

            // Start with recipe-based metadata
            let mut metadata = PackageMetadata::from_recipe(&recipe);

            // Set fields from GHCR info
            metadata.pkg_name = ghcr_info.pkg_name.clone();
            metadata.pkg_family = Some(ghcr_info.pkg_family.clone());

            // Extract pkg_type from recipe_name (first part before dot)
            let pkg_type = ghcr_info.recipe_name.split('.').next().unwrap_or("static");
            metadata.pkg_type = Some(pkg_type.to_string());
            metadata.pkg = format!("{}.{}", ghcr_info.pkg_name, pkg_type);

            // Set GHCR URL
            metadata.ghcr_url = Some(ghcr_info.ghcr_url());
            metadata.pkg_webpage = Some(ghcr_info.pkg_webpage(&arch));

            // Build script URL
            let recipe_path_str = path.to_string_lossy();
            metadata.build_script = Some(format!(
                "https://github.com/pkgforge/soarpkgs/blob/main/{}",
                recipe_path_str
            ));

            // Try to fetch manifest from registry
            match client.list_tags(&ghcr_info.ghcr_path).await {
                Ok(tag_list) => {
                    if let Some(tag) = RegistryClient::get_latest_arch_tag(&tag_list.tags, &arch) {
                        match client.fetch_manifest(&ghcr_info.ghcr_path, tag).await {
                            Ok(manifest_str) => {
                                if let Ok(manifest) = OciManifest::from_json(&manifest_str) {
                                    metadata.enrich_from_manifest(
                                        &manifest,
                                        &ghcr_info.ghcr_path,
                                        &arch,
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to fetch manifest for {}: {}",
                                    ghcr_info.ghcr_path, e
                                );
                            }
                        }
                    } else {
                        debug!("No matching tag for {} on {}", arch, ghcr_info.ghcr_path);
                    }
                }
                Err(e) => {
                    warn!("Failed to list tags for {}: {}", ghcr_info.ghcr_path, e);
                }
            }

            metadata.parse_note_flags();

            // Only add packages that have valid metadata (requires download_url from GHCR)
            if metadata.is_valid() {
                if ghcr_info.cache_type == "bincache" {
                    bincache_metadata.push(metadata);
                } else {
                    pkgcache_metadata.push(metadata);
                }
            } else {
                debug!(
                    "Skipping {}: not in GHCR or invalid metadata",
                    ghcr_info.ghcr_path
                );
            }
        }
    }

    // Process and write output for each cache type
    let write_cache_metadata =
        |cache_type: &str, mut metadata_list: Vec<PackageMetadata>| -> Result<()> {
            if metadata_list.is_empty() {
                info!("No {} packages to write", cache_type);
                return Ok(());
            }

            // Sort alphabetically by package name
            metadata_list.sort_by(|a, b| a.pkg.cmp(&b.pkg));

            // Create output directory if needed
            let cache_dir = output_dir.join(cache_type);
            std::fs::create_dir_all(&cache_dir)?;

            // Write output file
            let output_file = cache_dir.join(format!("{}.json", arch));
            let json = serde_json::to_string_pretty(&metadata_list)?;
            std::fs::write(&output_file, json)?;

            info!(
                "Generated {} metadata for {} packages -> {:?}",
                cache_type,
                metadata_list.len(),
                output_file
            );
            Ok(())
        };

    // Write outputs based on filter
    if cache_type_filter == "all" || cache_type_filter == "bincache" {
        write_cache_metadata("bincache", bincache_metadata)?;
    }
    if cache_type_filter == "all" || cache_type_filter == "pkgcache" {
        write_cache_metadata("pkgcache", pkgcache_metadata)?;
    }

    Ok(())
}

async fn cmd_should_rebuild(
    recipe_path: PathBuf,
    cache: Option<PathBuf>,
    force: bool,
) -> Result<()> {
    if force {
        info!("Force rebuild requested");
        std::process::exit(0); // Exit 0 = should rebuild
    }

    let recipe = SBuildRecipe::from_file(&recipe_path)?;

    if recipe.is_disabled() {
        info!("Recipe is disabled, skipping");
        std::process::exit(1); // Exit 1 = should NOT rebuild
    }

    // Check if version field exists
    if recipe.pkgver.is_none() {
        info!("No version field in recipe, should rebuild (new package)");
        std::process::exit(0);
    }

    // If we have a cache, check the recipe hash
    if let Some(cache_path) = cache {
        if cache_path.exists() {
            // TODO: Implement cache lookup
            // For now, compute hash and print it
            let content = std::fs::read_to_string(&recipe_path)?;
            let hash = compute_recipe_hash_excluding_version(&content);
            info!("Recipe hash (excluding version): {}", hash);
            // Would compare with cached hash here
        }
    }

    // Default: don't rebuild
    info!("No rebuild needed");
    std::process::exit(1);
}

async fn cmd_check_updates(
    recipe_dirs: Vec<PathBuf>,
    _cache: Option<PathBuf>,
    output: PathBuf,
    _parallel: usize,
    timeout: u64,
) -> Result<()> {
    info!("Checking for upstream updates (timeout: {}s)", timeout);

    // Scan all recipe directories
    let mut all_recipes = Vec::new();
    for dir in &recipe_dirs {
        let recipes = scan_recipes(dir)?;
        all_recipes.extend(recipes);
    }

    let enabled_recipes = filter_enabled(all_recipes);
    info!("Found {} enabled recipes", enabled_recipes.len());

    #[derive(serde::Serialize)]
    struct UpdateInfo {
        pkg: String,
        pkg_id: String,
        recipe_path: String,
        current_version: String,
        upstream_version: String,
    }

    let mut updates: Vec<UpdateInfo> = Vec::new();

    for (path, recipe) in enabled_recipes {
        // Only check recipes that have both version and pkgver
        let current_version = match &recipe.pkgver {
            Some(v) => v.clone(),
            None => continue, // Skip recipes without explicit version
        };

        let pkgver_script = match recipe.pkgver_script() {
            Some(s) => s,
            None => continue, // Skip recipes without pkgver script
        };

        info!("Checking {} (current: {})", recipe.pkg, current_version);

        // Execute pkgver script
        match execute_pkgver(pkgver_script, timeout).await {
            Ok(upstream_version) => {
                let upstream_version = upstream_version.trim().to_string();
                if upstream_version != current_version {
                    info!(
                        "  Update available: {} -> {}",
                        current_version, upstream_version
                    );
                    updates.push(UpdateInfo {
                        pkg: recipe.pkg.clone(),
                        pkg_id: recipe.pkg_id.clone(),
                        recipe_path: path.to_string_lossy().to_string(),
                        current_version,
                        upstream_version,
                    });
                }
            }
            Err(e) => {
                warn!("  Failed to check {}: {}", recipe.pkg, e);
            }
        }
    }

    // Write output
    let json = serde_json::to_string_pretty(&updates)?;
    std::fs::write(&output, json)?;

    info!("Found {} updates -> {:?}", updates.len(), output);
    Ok(())
}

async fn execute_pkgver(script: &str, timeout_secs: u64) -> Result<String> {
    use tokio::process::Command;
    use tokio::time::{timeout, Duration};

    let result = timeout(
        Duration::from_secs(timeout_secs),
        Command::new("bash").arg("-c").arg(script).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(Error::PkgverFailed(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        }
        Ok(Err(e)) => Err(Error::PkgverFailed(e.to_string())),
        Err(_) => Err(Error::PkgverFailed("Timeout".to_string())),
    }
}

fn cmd_hash(recipe_path: PathBuf, exclude_version: bool) -> Result<()> {
    let content = std::fs::read_to_string(&recipe_path)?;

    let hash = if exclude_version {
        compute_recipe_hash_excluding_version(&content)
    } else {
        compute_recipe_hash(&content)
    };

    println!("{}", hash);
    Ok(())
}

async fn cmd_fetch_manifest(
    repository: String,
    tag: Option<String>,
    arch: String,
    github_token: Option<String>,
) -> Result<()> {
    let _ = github_token; // Token not used for public registry access
    let client = RegistryClient::new();

    // Get tag if not specified
    let tag = match tag {
        Some(t) => t,
        None => {
            info!("Fetching tag list for {}", repository);
            let tag_list = client.list_tags(&repository).await?;
            debug!("Found {} tags: {:?}", tag_list.tags.len(), tag_list.tags);
            RegistryClient::get_latest_arch_tag(&tag_list.tags, &arch)
                .ok_or_else(|| Error::ManifestNotFound(format!("No tag found for {}", arch)))?
                .clone()
        }
    };

    info!("Fetching manifest for {}:{}", repository, tag);
    let manifest_str = client.fetch_manifest(&repository, &tag).await?;
    let manifest = OciManifest::from_json(&manifest_str)?;

    println!("Repository: {}", repository);
    println!("Tag: {}", tag);
    println!("Schema Version: {}", manifest.schema_version);
    println!("Total Size: {}", manifest.total_size_human());
    println!("Files: {:?}", manifest.filenames());

    if let Some(ghcr_pkg) = manifest.ghcr_pkg() {
        println!("GHCR Package: {}", ghcr_pkg);
    }

    if let Some(build_id) = manifest.build_id() {
        println!("Build ID: {}", build_id);
    }

    if let Ok(Some(pkg_json)) = manifest.get_package_json() {
        println!("\nEmbedded Package JSON:");
        println!("{}", serde_json::to_string_pretty(&pkg_json)?);
    }

    Ok(())
}
