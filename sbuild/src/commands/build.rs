use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{
        self,
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use clap::Parser;
use colored::Colorize;
use log::{error, info, warn, LevelFilter};
use sbuild::{
    builder::Builder,
    checksum, fetch_recipe,
    ghcr::{sanitize_oci_tag, GhcrClient, PackageAnnotations},
    read_recipe_metadata,
    signing::Signer,
    types::SoarEnv,
    update_json_metadata,
};
use sbuild_linter::logger::{LogManager, LogMessage};
use sbuild_meta::sanitize_oci_name;

#[derive(Parser)]
#[command(about = "Build packages from SBUILD recipes")]
pub struct BuildArgs {
    #[arg(required = true)]
    pub recipes: Vec<String>,

    #[arg(short, long)]
    pub outdir: Option<PathBuf>,

    #[arg(short, long)]
    pub keep: bool,

    #[arg(long, default_value = "3600")]
    pub timeout: u64,

    #[arg(long, default_value = "30")]
    pub timeout_linter: u64,

    #[arg(long, value_enum, default_value = "info")]
    pub log_level: LogLevel,

    #[arg(long)]
    pub ci: bool,

    #[arg(long)]
    pub force: bool,

    #[arg(long)]
    pub skip_existing: bool,

    #[arg(long, env = "GITHUB_TOKEN")]
    pub github_token: Option<String>,

    #[arg(long, env = "GHCR_TOKEN")]
    pub ghcr_token: Option<String>,

    #[arg(long)]
    pub ghcr_repo: Option<String>,

    #[arg(long)]
    pub push: bool,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub sign: bool,

    #[arg(long, env = "MINISIGN_KEY")]
    pub minisign_key: Option<String>,

    #[arg(long, env = "MINISIGN_PASSWORD")]
    pub minisign_password: Option<String>,

    #[arg(long, default_value = "true")]
    pub checksums: bool,

    #[arg(long)]
    pub cache: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum, Default)]
pub enum LogLevel {
    #[default]
    Info,
    Verbose,
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

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> LevelFilter {
        match level {
            LogLevel::Debug | LogLevel::Verbose => LevelFilter::Debug,
            LogLevel::Info => LevelFilter::Info,
        }
    }
}

pub async fn run(args: BuildArgs, soar_env: Option<SoarEnv>) -> Result<(), String> {
    init_logging(args.ci, args.log_level);

    println!(
        "{} v{}",
        "sbuild".bright_cyan().bold(),
        env!("CARGO_PKG_VERSION")
    );

    let soar_env = soar_env.unwrap_or_default();

    let now = Instant::now();
    let success = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));

    let (tx, rx) = sync::mpsc::channel();
    let log_manager = LogManager::new(tx.clone());

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
        let (recipe_path, recipe_url) =
            if recipe_input.starts_with("http://") || recipe_input.starts_with("https://") {
                match fetch_recipe(recipe_input).await {
                    Ok(content) => {
                        let temp_path =
                            std::env::temp_dir().join(format!("sbuild-{}.yaml", uuid_simple()));
                        if let Err(e) = std::fs::write(&temp_path, &content) {
                            error!("Failed to write temp recipe: {}", e);
                            fail.fetch_add(1, Ordering::SeqCst);
                            continue;
                        }
                        (
                            temp_path.to_string_lossy().to_string(),
                            Some(recipe_input.as_str()),
                        )
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
            true,
            args.log_level.into(),
            args.keep,
            Duration::from_secs(args.timeout),
        );

        info!("Building: {}", recipe_input);

        let outdir_str = args
            .outdir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        if let Some(build_outdir) = builder
            .build(
                &recipe_path,
                outdir_str.clone(),
                Duration::from_secs(args.timeout_linter),
                args.skip_existing,
            )
            .await
        {
            success.fetch_add(1, Ordering::SeqCst);

            if args.ci {
                write_github_env("SBUILD_SUCCESSFUL", "YES");
            }

            let pkg_name = build_outdir
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            if let Err(e) =
                post_build_processing(&build_outdir, &args, recipe_url, pkg_name.as_deref()).await
            {
                warn!("Post-build processing failed: {}", e);
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
            fail_count
        );
    }

    println!("[{}] Completed in {:.2?}", "⏱".bright_blue(), now.elapsed());

    if args.ci {
        write_github_output("success_count", &success_count.to_string());
        write_github_output("fail_count", &fail_count.to_string());
    }

    if fail_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn init_logging(_ci_mode: bool, log_level: LogLevel) {
    env_logger::Builder::new()
        .filter_level(log_level.into())
        .format_target(false)
        .format_timestamp(None)
        .init();
}

fn write_github_env(key: &str, value: &str) {
    if let Ok(env_file) = env::var("GITHUB_ENV") {
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&env_file) {
            use std::io::Write;
            writeln!(file, "{}={}", key, value).ok();
        }
    }
}

fn write_github_output(key: &str, value: &str) {
    if let Ok(output_file) = env::var("GITHUB_OUTPUT") {
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&output_file) {
            use std::io::Write;
            writeln!(file, "{}={}", key, value).ok();
        }
    }
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", duration.as_secs(), duration.subsec_nanos())
}

fn sign_file(signer: &Signer, file_path: &Path) -> Option<PathBuf> {
    match signer.sign(file_path) {
        Ok(_) => {
            let sig_path = PathBuf::from(format!("{}.sig", file_path.display()));
            if sig_path.exists() {
                info!("Signed: {}", file_path.display());
                Some(sig_path)
            } else {
                warn!("Signature file not created for: {}", file_path.display());
                None
            }
        }
        Err(e) => {
            warn!("Failed to sign {}: {}", file_path.display(), e);
            None
        }
    }
}

async fn post_build_processing(
    outdir: &Path,
    cli: &BuildArgs,
    recipe_url: Option<&str>,
    pkg_name: Option<&str>,
) -> Result<(), String> {
    use sbuild::parse_ghcr_path;

    if cli.checksums {
        info!("Generating checksums...");
        match checksum::generate_checksum_file(outdir) {
            Ok(_) => info!("Checksums generated"),
            Err(e) => warn!("Failed to generate checksums: {}", e),
        }
    }

    let signer = if cli.sign {
        if let Some(ref key) = cli.minisign_key {
            if let Err(e) = Signer::check_minisign() {
                return Err(format!("Signing failed: {}", e));
            }

            let s = if Path::new(key).exists() {
                Signer::with_key_file(key)
            } else {
                Signer::with_key_data(key.clone())
            }
            .with_password(cli.minisign_password.clone());

            Some(s)
        } else {
            warn!("--sign specified but no --minisign-key provided");
            None
        }
    } else {
        None
    };

    if cli.push {
        if let (Some(ref token), Some(ref base_repo)) = (&cli.ghcr_token, &cli.ghcr_repo) {
            if cli.dry_run {
                info!("[DRY-RUN] Simulating GHCR push...");
            } else {
                info!("Pushing to GHCR...");
            }

            if !cli.dry_run {
                if let Err(e) = GhcrClient::check_oras() {
                    return Err(format!("GHCR push failed: {}", e));
                }
            }

            let client = if !cli.dry_run {
                let c = GhcrClient::new(token.clone());
                if let Err(e) = c.login() {
                    return Err(format!("GHCR login failed: {}", e));
                }
                Some(c)
            } else {
                None
            };

            let base_version = std::fs::read_dir(outdir)
                .ok()
                .and_then(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .find(|e| {
                            e.path()
                                .extension()
                                .map(|ext| ext == "version")
                                .unwrap_or(false)
                        })
                        .and_then(|e| std::fs::read_to_string(e.path()).ok())
                        .map(|s| s.lines().next().unwrap_or("").to_string())
                })
                .unwrap_or_else(|| "latest".to_string())
                .trim()
                .to_string();

            let arch = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);

            let (version, revision) = if let Some(ref cache_path) = cli.cache {
                match sbuild_cache::CacheDatabase::open(cache_path) {
                    Ok(cache_db) => {
                        let meta = read_recipe_metadata(outdir);
                        let cache_pkg_id = meta
                            .as_ref()
                            .map(|m| m.pkg_id.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| pkg_name.unwrap_or("unknown"));
                        let host = arch.to_lowercase();
                        match cache_db.get_revision(cache_pkg_id, &host, &base_version) {
                            Ok(rev) if rev > 0 => {
                                let versioned = format!("{}-r{}", base_version, rev);
                                info!(
                                    "Revision {}: version {} -> {}",
                                    rev, base_version, versioned
                                );
                                (versioned, rev)
                            }
                            Ok(_) => (base_version.clone(), 0),
                            Err(e) => {
                                warn!("Failed to get revision from cache: {}", e);
                                (base_version.clone(), 0)
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to open build cache: {}", e);
                        (base_version.clone(), 0)
                    }
                }
            } else {
                (base_version.clone(), 0)
            };

            let tag = format!("{}-{}", sanitize_oci_tag(&version), arch.to_lowercase());

            let (pkg_family, recipe_name) =
                recipe_url.and_then(parse_ghcr_path).unwrap_or_else(|| {
                    let default = pkg_name.unwrap_or("unknown").to_string();
                    (default.clone(), default)
                });

            let metadata = read_recipe_metadata(outdir);

            let mut push_success = true;
            let mut pushed_urls = Vec::new();

            let packages_dir = outdir.join("packages");

            if packages_dir.is_dir() {
                info!("Found packages/ directory, using explicit package structure");

                let shared_files: Vec<PathBuf> = std::fs::read_dir(outdir)
                    .map_err(|e| format!("Failed to read output directory: {}", e))?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect();

                // Use recipe packages field if available, otherwise discover from directories
                let package_names: Vec<String> = if let Some(ref meta) = metadata {
                    if meta.has_packages() {
                        meta.get_provided_packages()
                    } else {
                        std::fs::read_dir(&packages_dir)
                            .map_err(|e| format!("Failed to read packages directory: {}", e))?
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().is_dir())
                            .filter_map(|e| e.path().file_name()?.to_str().map(|s| s.to_string()))
                            .collect()
                    }
                } else {
                    std::fs::read_dir(&packages_dir)
                        .map_err(|e| format!("Failed to read packages directory: {}", e))?
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().is_dir())
                        .filter_map(|e| e.path().file_name()?.to_str().map(|s| s.to_string()))
                        .collect()
                };

                if package_names.is_empty() {
                    warn!("packages/ directory is empty");
                    return Ok(());
                }

                for pkg_name_dir in &package_names {
                    let pkg_dir = packages_dir.join(pkg_name_dir);

                    let sanitized_pkg_name = sanitize_oci_name(pkg_name_dir);
                    let owner = base_repo.split('/').next().unwrap_or(base_repo);
                    let full_repo = if let Some(ref custom_base) =
                        metadata.as_ref().and_then(|m| m.ghcr_pkg.as_ref())
                    {
                        format!("{}/{}/{}", owner, custom_base, sanitized_pkg_name)
                    } else {
                        format!(
                            "{}/{}/{}/{}",
                            base_repo, pkg_family, recipe_name, sanitized_pkg_name
                        )
                    };
                    info!("Pushing package {} to {}", pkg_name_dir, full_repo);

                    // Collect files from the package directory
                    let mut files_to_push: Vec<PathBuf> = if pkg_dir.is_dir() {
                        std::fs::read_dir(&pkg_dir)
                            .map_err(|e| format!("Failed to read package directory: {}", e))?
                            .filter_map(|e| e.ok())
                            .map(|e| e.path())
                            .filter(|p| p.is_file())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    // Also check for binaries in the root outdir (per-package provides)
                    if let Some(provides) = metadata
                        .as_ref()
                        .and_then(|m| m.get_package_provides(pkg_name_dir))
                    {
                        for provide in provides {
                            let root_path = outdir.join(provide);
                            if root_path.exists()
                                && root_path.is_file()
                                && !files_to_push.contains(&root_path)
                            {
                                files_to_push.push(root_path);
                            }
                        }
                    }

                    files_to_push.extend(shared_files.clone());

                    let main_binary = files_to_push
                        .iter()
                        .find(|f| {
                            f.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| n == pkg_name_dir.as_str())
                                .unwrap_or(false)
                        })
                        .cloned();

                    // All non-shared files in the package are binaries to sign
                    let binaries_to_sign: Vec<PathBuf> = files_to_push
                        .iter()
                        .filter(|f| !shared_files.contains(f))
                        .cloned()
                        .collect();

                    if let Some(ref s) = signer {
                        for binary_path in &binaries_to_sign {
                            if let Some(sig_path) = sign_file(s, binary_path) {
                                files_to_push.push(sig_path);
                            }
                        }
                    }

                    let (bsum, shasum, binary_size) = if let Some(ref binary_path) = main_binary {
                        let size = fs::metadata(binary_path).ok().map(|m| m.len());
                        (
                            checksum::b3sum(binary_path).ok(),
                            checksum::sha256sum(binary_path).ok(),
                            size,
                        )
                    } else {
                        (None, None, None)
                    };

                    let checksum_bsum = {
                        let checksum_path = pkg_dir.join("CHECKSUM");
                        if checksum_path.exists() {
                            checksum::b3sum(&checksum_path).ok()
                        } else {
                            None
                        }
                    };

                    let ghcr_total_size: u64 = files_to_push
                        .iter()
                        .filter_map(|f| fs::metadata(f).ok().map(|m| m.len()))
                        .sum();

                    let json_path = pkg_dir.join(format!("{}.json", pkg_name_dir));
                    if json_path.exists() {
                        if let Err(e) = update_json_metadata(
                            &json_path,
                            pkg_name_dir,
                            &full_repo,
                            &tag,
                            bsum.as_deref(),
                            shasum.as_deref(),
                            checksum_bsum.as_deref(),
                            binary_size,
                            Some(ghcr_total_size),
                        ) {
                            warn!("Failed to update JSON metadata: {}", e);
                        }
                    }

                    let annotations = PackageAnnotations {
                        pkg: pkg_name_dir.to_string(),
                        pkg_id: metadata
                            .as_ref()
                            .map(|m| m.pkg_id.clone())
                            .unwrap_or_default(),
                        pkg_type: metadata.as_ref().and_then(|m| m.pkg_type.clone()),
                        version: version.clone(),
                        description: metadata
                            .as_ref()
                            .map(|m| m.description.clone())
                            .filter(|s| !s.is_empty()),
                        homepage: metadata.as_ref().and_then(|m| m.homepage.first().cloned()),
                        license: metadata
                            .as_ref()
                            .map(|m| m.license.join(", "))
                            .filter(|s| !s.is_empty()),
                        build_date: chrono::Utc::now().to_rfc3339(),
                        build_id: env::var("GITHUB_RUN_ID").ok(),
                        build_gha: env::var("GITHUB_RUN_ID").ok().map(|id| {
                            format!(
                                "https://github.com/{}/actions/runs/{}",
                                env::var("GITHUB_REPOSITORY").unwrap_or_default(),
                                id
                            )
                        }),
                        build_script: recipe_url.map(|s| s.to_string()),
                        bsum,
                        shasum,
                        checksum_bsum,
                    };

                    if cli.dry_run {
                        let target = format!("ghcr.io/{}:{}", full_repo, tag);
                        info!(
                            "[DRY-RUN] Would push {} files to {}",
                            files_to_push.len(),
                            target
                        );
                        for f in &files_to_push {
                            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                            let size = fs::metadata(f).ok().map(|m| m.len()).unwrap_or(0);
                            info!("  - {} ({} bytes)", name, size);
                        }
                        pushed_urls.push(target);
                    } else {
                        match client.as_ref().unwrap().push(
                            &files_to_push,
                            &full_repo,
                            &tag,
                            &annotations,
                        ) {
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
                }
            } else {
                let all_files: Vec<PathBuf> = std::fs::read_dir(outdir)
                    .map_err(|e| format!("Failed to read output directory: {}", e))?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && !p.is_symlink())
                    .collect();

                if all_files.is_empty() {
                    warn!("No files found to push");
                    return Ok(());
                }

                let default_pkg_name = pkg_name.unwrap_or(&pkg_family).to_string();
                let binaries_to_push: Vec<String> = {
                    let packages = metadata
                        .as_ref()
                        .map(|m| m.get_provided_packages())
                        .unwrap_or_default();
                    if !packages.is_empty() && packages != vec![default_pkg_name.clone()] {
                        info!("Using packages from provides: {:?}", packages);
                        packages
                    } else {
                        info!(
                            "No packages from provides, using pkg name: {}",
                            default_pkg_name
                        );
                        vec![default_pkg_name.clone()]
                    }
                };

                for binary_name in &binaries_to_push {
                    let sanitized_binary_name = sanitize_oci_name(binary_name);
                    let owner = base_repo.split('/').next().unwrap_or(base_repo);
                    let full_repo = if let Some(ref custom_base) =
                        metadata.as_ref().and_then(|m| m.ghcr_pkg.as_ref())
                    {
                        format!("{}/{}/{}", owner, custom_base, sanitized_binary_name)
                    } else {
                        format!(
                            "{}/{}/{}/{}",
                            base_repo, pkg_family, recipe_name, sanitized_binary_name
                        )
                    };
                    info!("Pushing {} to {}", binary_name, full_repo);

                    let mut files_to_push: Vec<PathBuf> = Vec::new();
                    let mut main_binary_path: Option<PathBuf> = None;

                    let packages = metadata
                        .as_ref()
                        .map(|m| m.get_provided_packages())
                        .unwrap_or_default();
                    if packages.len() <= 1
                        && packages.first().map_or(true, |p| p == &default_pkg_name)
                    {
                        files_to_push = all_files.clone();
                        main_binary_path = all_files
                            .iter()
                            .find(|p| {
                                p.file_name().and_then(|n| n.to_str()) == Some(binary_name.as_ref())
                            })
                            .cloned();
                    } else {
                        let binary_path = outdir.join(binary_name);
                        if binary_path.exists() {
                            files_to_push.push(binary_path.clone());
                            main_binary_path = Some(binary_path);
                        }

                        for ext in &[
                            "json",
                            "png",
                            "svg",
                            "desktop",
                            "appdata.xml",
                            "metainfo.xml",
                        ] {
                            let assoc_file = outdir.join(format!("{}.{}", binary_name, ext));
                            if assoc_file.exists() {
                                files_to_push.push(assoc_file);
                            }
                        }

                        for file in &all_files {
                            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if matches!(name, "CHECKSUM" | "SBUILD" | "LICENSE")
                                || matches!(ext, "log" | "version")
                            {
                                if !files_to_push.contains(file) {
                                    files_to_push.push(file.clone());
                                }
                            }
                        }
                    }

                    if files_to_push.is_empty() {
                        warn!("No files found for {}", binary_name);
                        continue;
                    }

                    let mut binaries_to_sign: Vec<PathBuf> = Vec::new();
                    if let Some(ref bin_path) = main_binary_path {
                        binaries_to_sign.push(bin_path.clone());
                    }

                    for extra_bin in metadata
                        .as_ref()
                        .map(|m| m.get_binaries())
                        .unwrap_or_default()
                    {
                        let extra_path = outdir.join(&extra_bin);
                        if extra_path.exists() && !binaries_to_sign.contains(&extra_path) {
                            binaries_to_sign.push(extra_path.clone());
                            if !files_to_push.contains(&extra_path) {
                                files_to_push.push(extra_path);
                            }
                        }
                    }

                    if let Some(ref s) = signer {
                        for bin_path in &binaries_to_sign {
                            if let Some(sig_path) = sign_file(s, bin_path) {
                                files_to_push.push(sig_path);
                            }
                        }
                    }

                    let (bsum, shasum, binary_size) = if let Some(ref bin_path) = main_binary_path {
                        (
                            checksum::b3sum(bin_path).ok(),
                            checksum::sha256sum(bin_path).ok(),
                            fs::metadata(bin_path).ok().map(|m| m.len()),
                        )
                    } else {
                        (None, None, None)
                    };

                    let checksum_bsum = {
                        let checksum_path = outdir.join("CHECKSUM");
                        if checksum_path.exists() {
                            checksum::b3sum(&checksum_path).ok()
                        } else {
                            None
                        }
                    };

                    let ghcr_total_size: u64 = files_to_push
                        .iter()
                        .filter_map(|f| fs::metadata(f).ok().map(|m| m.len()))
                        .sum();

                    let json_path = outdir.join(format!("{}.json", binary_name));
                    if json_path.exists() {
                        if let Err(e) = update_json_metadata(
                            &json_path,
                            binary_name,
                            &full_repo,
                            &tag,
                            bsum.as_deref(),
                            shasum.as_deref(),
                            checksum_bsum.as_deref(),
                            binary_size,
                            Some(ghcr_total_size),
                        ) {
                            warn!("Failed to update JSON metadata: {}", e);
                        }
                    }

                    let annotations = PackageAnnotations {
                        pkg: binary_name.to_string(),
                        pkg_id: metadata
                            .as_ref()
                            .map(|m| m.pkg_id.clone())
                            .unwrap_or_default(),
                        pkg_type: metadata.as_ref().and_then(|m| m.pkg_type.clone()),
                        version: version.clone(),
                        description: metadata
                            .as_ref()
                            .map(|m| m.description.clone())
                            .filter(|s| !s.is_empty()),
                        homepage: metadata.as_ref().and_then(|m| m.homepage.first().cloned()),
                        license: metadata
                            .as_ref()
                            .map(|m| m.license.join(", "))
                            .filter(|s| !s.is_empty()),
                        build_date: chrono::Utc::now().to_rfc3339(),
                        build_id: env::var("GITHUB_RUN_ID").ok(),
                        build_gha: env::var("GITHUB_RUN_ID").ok().map(|id| {
                            format!(
                                "https://github.com/{}/actions/runs/{}",
                                env::var("GITHUB_REPOSITORY").unwrap_or_default(),
                                id
                            )
                        }),
                        build_script: recipe_url.map(|s| s.to_string()),
                        bsum,
                        shasum,
                        checksum_bsum,
                    };

                    if cli.dry_run {
                        let target = format!("ghcr.io/{}:{}", full_repo, tag);
                        info!(
                            "[DRY-RUN] Would push {} files to {}",
                            files_to_push.len(),
                            target
                        );
                        for f in &files_to_push {
                            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                            let size = fs::metadata(f).ok().map(|m| m.len()).unwrap_or(0);
                            info!("  - {} ({} bytes)", name, size);
                        }
                        pushed_urls.push(target);
                    } else {
                        match client.as_ref().unwrap().push(
                            &files_to_push,
                            &full_repo,
                            &tag,
                            &annotations,
                        ) {
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
            }

            if cli.ci {
                if push_success && !pushed_urls.is_empty() {
                    write_github_env("GHCRPKG_URL", &pushed_urls.join(","));
                    write_github_env("PUSH_SUCCESSFUL", "YES");
                } else {
                    write_github_env("PUSH_SUCCESSFUL", "NO");
                }
            }

            if !cli.dry_run && push_success && !pushed_urls.is_empty() {
                if let Some(ref cache_path) = cli.cache {
                    if let Ok(cache_db) = sbuild_cache::CacheDatabase::open(cache_path) {
                        let meta = read_recipe_metadata(outdir);
                        let cache_pkg_id = meta
                            .as_ref()
                            .map(|m| m.pkg_id.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| pkg_name.unwrap_or("unknown"));
                        let host = arch.to_lowercase();
                        let build_id = env::var("GITHUB_RUN_ID").ok();

                        let cache_pkg_name = meta
                            .as_ref()
                            .map(|m| m.pkg.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or(cache_pkg_id);
                        let _ = cache_db.get_or_create_package(cache_pkg_id, cache_pkg_name, &host);

                        if let Err(e) = cache_db.update_build_result(
                            cache_pkg_id,
                            &host,
                            &version,
                            sbuild_cache::BuildStatus::Success,
                            build_id.as_deref(),
                            Some(&tag),
                            None,
                            Some(&base_version),
                            revision,
                        ) {
                            warn!("Failed to update build cache: {}", e);
                        } else {
                            info!("Updated build cache for {} on {}", cache_pkg_id, host);
                        }
                    }
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
