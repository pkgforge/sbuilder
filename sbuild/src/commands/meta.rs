use std::path::PathBuf;

use clap::{Parser, Subcommand};
use log::{debug, info, warn};
use sbuild_meta::{
    hash::{compute_recipe_hash, compute_recipe_hash_excluding_version},
    manifest::OciManifest,
    metadata::PackageMetadata,
    recipe::{filter_by_arch, filter_enabled, scan_recipes, SBuildRecipe},
    registry::RegistryClient,
    Error, Result,
};

#[derive(Parser)]
#[command(about = "Metadata generator for SBUILD packages")]
pub struct MetaArgs {
    #[command(subcommand)]
    command: MetaCommands,
}

#[derive(Subcommand)]
enum MetaCommands {
    Generate {
        #[arg(short, long)]
        arch: String,

        #[arg(short, long, num_args = 1..)]
        recipes: Vec<PathBuf>,

        #[arg(short, long)]
        output: Option<PathBuf>,

        #[arg(short, long)]
        cache: Option<PathBuf>,

        #[arg(short, long, default_value = "4")]
        parallel: usize,

        #[arg(long, env = "GITHUB_TOKEN")]
        github_token: Option<String>,

        #[arg(long, default_value = "pkgforge")]
        ghcr_owner: String,
    },

    ShouldRebuild {
        #[arg(short, long)]
        recipe: PathBuf,

        #[arg(short, long)]
        cache: Option<PathBuf>,

        #[arg(short, long)]
        force: bool,
    },

    CheckUpdates {
        #[arg(short, long, num_args = 1..)]
        recipes: Vec<PathBuf>,

        #[arg(short, long)]
        cache: Option<PathBuf>,

        #[arg(short, long)]
        output: PathBuf,

        #[arg(short, long, default_value = "10")]
        parallel: usize,

        #[arg(long, default_value = "30")]
        timeout: u64,
    },

    Inspect {
        recipe: PathBuf,

        #[arg(short, long, default_value = "x86_64-linux")]
        arch: String,

        #[arg(long, default_value = "pkgforge")]
        ghcr_owner: String,

        /// Fetch live manifest data from GHCR
        #[arg(long)]
        live: bool,
    },

    Hash {
        recipe: PathBuf,

        #[arg(long)]
        exclude_version: bool,
    },

    FetchManifest {
        #[arg(short, long)]
        repository: String,

        #[arg(short, long)]
        tag: Option<String>,

        #[arg(short, long, default_value = "x86_64-linux")]
        arch: String,

        #[arg(long, env = "GITHUB_TOKEN")]
        github_token: Option<String>,
    },
}

pub async fn run(args: MetaArgs) -> Result<()> {
    setup_logging();

    match args.command {
        MetaCommands::Generate {
            arch,
            recipes,
            output,
            cache,
            parallel,
            github_token,
            ghcr_owner,
        } => {
            cmd_generate(
                arch,
                recipes,
                output,
                cache,
                parallel,
                github_token,
                ghcr_owner,
            )
            .await
        }

        MetaCommands::ShouldRebuild {
            recipe,
            cache,
            force,
        } => cmd_should_rebuild(recipe, cache, force).await,

        MetaCommands::CheckUpdates {
            recipes,
            cache,
            output,
            parallel,
            timeout,
        } => cmd_check_updates(recipes, cache, output, parallel, timeout).await,

        MetaCommands::Inspect {
            recipe,
            arch,
            ghcr_owner,
            live,
        } => cmd_inspect(recipe, arch, ghcr_owner, live).await,

        MetaCommands::Hash {
            recipe,
            exclude_version,
        } => cmd_hash(recipe, exclude_version),

        MetaCommands::FetchManifest {
            repository,
            tag,
            arch,
            github_token,
        } => cmd_fetch_manifest(repository, tag, arch, github_token).await,
    }
}

fn setup_logging() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .format_target(false)
        .format_timestamp(None)
        .init();
}

async fn cmd_generate(
    arch: String,
    recipe_dirs: Vec<PathBuf>,
    output: Option<PathBuf>,
    _cache: Option<PathBuf>,
    _parallel: usize,
    github_token: Option<String>,
    ghcr_owner: String,
) -> Result<()> {
    let arch = arch.to_lowercase();
    info!("Generating metadata for {}", arch);

    let _ = github_token;
    let client = RegistryClient::new();

    let mut all_recipes = Vec::new();
    for dir in &recipe_dirs {
        info!("Scanning recipes in {:?}", dir);
        let recipes = scan_recipes(dir)?;
        all_recipes.extend(recipes);
    }

    info!("Found {} total recipes", all_recipes.len());

    let recipes = filter_enabled(filter_by_arch(all_recipes, &arch));
    info!("After filtering: {} recipes for {}", recipes.len(), arch);

    let mut metadata: Vec<PackageMetadata> = Vec::new();

    for (path, recipe) in recipes {
        let ghcr_packages = recipe.ghcr_packages_from_path(&path, &ghcr_owner);

        if ghcr_packages.is_empty() {
            warn!("No GHCR packages found for {:?}", path);
            continue;
        }

        for ghcr_info in &ghcr_packages {
            info!(
                "Processing: {} -> {} ({:?})",
                recipe.pkg, ghcr_info.pkg_name, path
            );

            let mut pkg_metadata = PackageMetadata::from_recipe(&recipe);

            pkg_metadata.pkg_name = ghcr_info.pkg_name.clone();
            pkg_metadata.pkg_family = Some(ghcr_info.pkg_family.clone());

            let pkg_type = ghcr_info.recipe_name.split('.').next().unwrap_or("static");
            pkg_metadata.pkg_type = Some(pkg_type.to_string());
            pkg_metadata.pkg = format!("{}.{}", ghcr_info.pkg_name, pkg_type);

            pkg_metadata.ghcr_url = Some(ghcr_info.ghcr_url());
            pkg_metadata.pkg_webpage = Some(ghcr_info.pkg_webpage(&arch));

            let recipe_path_str = path.to_string_lossy();
            pkg_metadata.build_script = Some(format!(
                "https://github.com/pkgforge/soarpkgs/blob/main/{}",
                recipe_path_str
            ));

            match client.list_tags(&ghcr_info.ghcr_path).await {
                Ok(tag_list) => {
                    if let Some(tag) = RegistryClient::get_latest_arch_tag(&tag_list.tags, &arch) {
                        match client.fetch_manifest(&ghcr_info.ghcr_path, tag).await {
                            Ok(manifest_str) => {
                                if let Ok(manifest) = OciManifest::from_json(&manifest_str) {
                                    pkg_metadata.enrich_from_manifest(
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

            pkg_metadata.parse_note_flags();

            if pkg_metadata.is_valid() {
                metadata.push(pkg_metadata);
            } else {
                debug!(
                    "Skipping {}: not in GHCR or invalid metadata",
                    ghcr_info.ghcr_path
                );
            }
        }
    }

    if metadata.is_empty() {
        info!("No packages to write");
        return Ok(());
    }

    metadata.sort_by(|a, b| a.pkg.cmp(&b.pkg));

    let output_path = output.unwrap_or_else(|| PathBuf::from(format!("{}.json", arch)));

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(&metadata)?;
    std::fs::write(&output_path, json)?;

    info!(
        "Generated metadata for {} packages -> {:?}",
        metadata.len(),
        output_path
    );

    Ok(())
}

async fn cmd_should_rebuild(
    recipe_path: PathBuf,
    _cache: Option<PathBuf>,
    force: bool,
) -> Result<()> {
    if force {
        info!("Force rebuild requested");
        std::process::exit(0);
    }

    let recipe = SBuildRecipe::from_file(&recipe_path)?;

    if recipe.is_disabled() {
        info!("Recipe is disabled, skipping");
        std::process::exit(1);
    }

    if recipe.pkgver.is_none() {
        info!("No version field in recipe, should rebuild (new package)");
        std::process::exit(0);
    }

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
        upstream_remote_version: Option<String>,
    }

    let mut updates: Vec<UpdateInfo> = Vec::new();

    for (path, recipe) in enabled_recipes {
        let current_pkgver = match &recipe.pkgver {
            Some(v) => v.clone(),
            None => continue,
        };

        let current_remote_version = match &recipe.remote_pkgver {
            Some(v) => v.clone(),
            None => current_pkgver.clone(),
        };

        let pkgver_script = match recipe.pkgver_script() {
            Some(s) => s,
            None => continue,
        };

        info!(
            "Checking {} (current: {})",
            recipe.pkg, current_remote_version
        );

        match execute_pkgver(pkgver_script, timeout).await {
            Ok((upstream_version, upstream_remote_version)) => {
                let upstream_version = upstream_version.trim().to_string();
                let upstream_for_comparison = upstream_remote_version
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| upstream_version.clone());

                if upstream_for_comparison != current_remote_version {
                    info!(
                        "  Update available: {} -> {}",
                        current_remote_version, upstream_for_comparison
                    );
                    updates.push(UpdateInfo {
                        pkg: recipe.pkg.clone(),
                        pkg_id: recipe.pkg_id.clone(),
                        recipe_path: path.to_string_lossy().to_string(),
                        current_version: current_pkgver,
                        upstream_version,
                        upstream_remote_version,
                    });
                }
            }
            Err(e) => {
                warn!("  Failed to check {}: {}", recipe.pkg, e);
            }
        }
    }

    let json = serde_json::to_string_pretty(&updates)?;
    std::fs::write(&output, json)?;

    info!("Found {} updates -> {:?}", updates.len(), output);
    Ok(())
}

async fn execute_pkgver(script: &str, timeout_secs: u64) -> Result<(String, Option<String>)> {
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
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let lines: Vec<&str> = stdout.lines().collect();

                if lines.is_empty() {
                    Ok((String::new(), None))
                } else {
                    let pkgver = lines[0].trim().to_string();
                    let remote_pkgver = if lines.len() >= 2 {
                        Some(lines[1].trim().to_string())
                    } else {
                        None
                    };
                    Ok((pkgver, remote_pkgver))
                }
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

async fn cmd_inspect(
    recipe_path: PathBuf,
    arch: String,
    ghcr_owner: String,
    live: bool,
) -> Result<()> {
    let arch = arch.to_lowercase();
    let recipe = SBuildRecipe::from_file(&recipe_path)?;
    let ghcr_packages = recipe.ghcr_packages_from_path(&recipe_path, &ghcr_owner);

    if ghcr_packages.is_empty() {
        info!("No GHCR packages found for {:?}", recipe_path);
        return Ok(());
    }

    let client = if live {
        Some(RegistryClient::new())
    } else {
        None
    };

    let mut all_metadata = Vec::new();

    for ghcr_info in &ghcr_packages {
        let mut pkg_metadata = PackageMetadata::from_recipe(&recipe);

        pkg_metadata.pkg_name = ghcr_info.pkg_name.clone();
        pkg_metadata.pkg_family = Some(ghcr_info.pkg_family.clone());

        let pkg_type = ghcr_info.recipe_name.split('.').next().unwrap_or("static");
        pkg_metadata.pkg_type = Some(pkg_type.to_string());
        pkg_metadata.pkg = format!("{}.{}", ghcr_info.pkg_name, pkg_type);

        pkg_metadata.ghcr_url = Some(ghcr_info.ghcr_url());
        pkg_metadata.pkg_webpage = Some(ghcr_info.pkg_webpage(&arch));

        if let Some(pkg_provides) = recipe.get_package_provides(&ghcr_info.pkg_name) {
            pkg_metadata.provides = Some(pkg_provides.to_vec());
        }

        let recipe_path_str = recipe_path.to_string_lossy();
        pkg_metadata.build_script = Some(format!(
            "https://github.com/pkgforge/soarpkgs/blob/main/{}",
            recipe_path_str
        ));

        if let Some(ref client) = client {
            match client.list_tags(&ghcr_info.ghcr_path).await {
                Ok(tag_list) => {
                    if let Some(tag) = RegistryClient::get_latest_arch_tag(&tag_list.tags, &arch) {
                        match client.fetch_manifest(&ghcr_info.ghcr_path, tag).await {
                            Ok(manifest_str) => {
                                if let Ok(manifest) = OciManifest::from_json(&manifest_str) {
                                    pkg_metadata.enrich_from_manifest(
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
                    }
                }
                Err(e) => {
                    warn!("Failed to list tags for {}: {}", ghcr_info.ghcr_path, e);
                }
            }
        }

        pkg_metadata.parse_note_flags();
        all_metadata.push(pkg_metadata);
    }

    let json = serde_json::to_string_pretty(&all_metadata)?;
    println!("{}", json);
    Ok(())
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
    _github_token: Option<String>,
) -> Result<()> {
    let arch = arch.to_lowercase();
    let client = RegistryClient::new();

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
