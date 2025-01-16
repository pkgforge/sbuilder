use std::{
    collections::HashMap,
    env::{
        self,
        consts::{ARCH, OS},
    },
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{self, Arc},
    thread,
    time::Duration,
};

use sbuild_linter::{
    build_config::BuildConfig, license::License, logger::TaskLogger, BuildAsset, Linter,
};
use squishy::{
    appimage::{get_offset, AppImage},
    EntryKind,
};

use crate::{
    cleanup::Finalize,
    constant::{
        APPIMAGE_MAGIC_BYTES, ELF_MAGIC_BYTES, FLATIMAGE_MAGIC_BYTES, PNG_MAGIC_BYTES,
        SVG_MAGIC_BYTES, XML_MAGIC_BYTES,
    },
    types::{OutputStream, PackageType, SoarEnv},
    utils::{
        calc_magic_bytes, download, extract_filename, is_static_elf, pack_appimage, temp_file,
    },
};

pub struct BuildContext {
    pkg: String,
    pkg_id: String,
    pkg_type: Option<String>,
    sbuild_pkg: String,
    outdir: PathBuf,
    tmpdir: PathBuf,
    version: String,
}

impl BuildContext {
    fn new<P: AsRef<Path>>(
        build_config: &BuildConfig,
        cache_path: P,
        version: String,
        outdir: Option<String>,
    ) -> Self {
        let sbuild_pkg = build_config
            .pkg_type
            .as_ref()
            .map(|t| format!("{}.{}", build_config.pkg, t))
            .unwrap_or(build_config.pkg.clone());

        let outdir = outdir
            .map(|dir| {
                let path = Path::new(&dir);
                if path.is_absolute() {
                    path.to_owned()
                } else {
                    let current_dir = env::current_dir().expect("Failed to get current directory");
                    current_dir.join(dir).join(&build_config.pkg_id)
                }
            })
            .unwrap_or_else(|| {
                cache_path
                    .as_ref()
                    .join("sbuild")
                    .join(&build_config.pkg_id)
            });
        let tmpdir = outdir.join("SBUILD_TEMP");

        Self {
            pkg: build_config.pkg.clone(),
            pkg_id: build_config.pkg_id.clone(),
            pkg_type: build_config.pkg_type.clone(),
            sbuild_pkg,
            outdir,
            tmpdir,
            version,
        }
    }

    fn env_vars(&self, soar_bin: &str) -> Vec<(String, String)> {
        let paths = env::var("PATH").unwrap_or_default();

        let inherit_keys = [
            "DEBIAN_FRONTEND",
            "EGET_TIMEOUT",
            "GIT_ASKPASS",
            "GIT_TERMINAL_PROMPT",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "GITLAB_TOKEN",
            "GL_TOKEN",
            "HF_TOKEN",
            "HOST_TRIPLET",
            "NIXPKGS_ALLOW_BROKEN",
            "NIXPKGS_ALLOW_UNFREE",
            "NIXPKGS_ALLOW_UNSUPPORTED_SYSTEM",
            "SYSTMP",
            "TERM",
            "USER_AGENT",
        ];

        let get_env_var =
            |key: &str| -> (String, Option<String>) { (key.to_string(), env::var(key).ok()) };

        let existing_envs: Vec<(String, Option<String>)> =
            inherit_keys.iter().map(|key| get_env_var(key)).collect();

        let paths = format!("{}:{}", soar_bin, paths);
        let mut vars: Vec<(String, String)> = [
            ("pkg", self.pkg.clone()),
            ("pkg_id", self.pkg_id.clone()),
            ("pkg_type", self.pkg_type.clone().unwrap_or_default()),
            ("sbuild_pkg", self.sbuild_pkg.clone()),
            ("sbuild_pkgver", self.version.clone()),
            ("sbuild_outdir", self.outdir.to_string_lossy().to_string()),
            ("sbuild_tmpdir", self.tmpdir.to_string_lossy().to_string()),
            ("pkg_ver", self.version.clone()),
            ("pkgver", self.version.clone()),
        ]
        .into_iter()
        .flat_map(|(key, value)| {
            let value = match key {
                "sbuild_outdir" | "sbuild_tmpdir" => value,
                _ => value.replace(|c: char| c.is_whitespace(), ""),
            };
            vec![
                (key.to_string(), value.clone()),
                (key.to_uppercase(), value),
            ]
        })
        .chain(std::iter::once(("PATH".to_string(), paths)))
        .collect();

        vars.extend(
            existing_envs
                .into_iter()
                .filter_map(|(key, value)| value.map(|val| (key, val))),
        );
        vars
    }
}

pub struct Builder {
    logger: TaskLogger,
    soar_env: SoarEnv,
    external: bool,
    desktop: HashMap<String, bool>,
    icon: HashMap<String, bool>,
    appstream: HashMap<String, bool>,
    pkg_type: PackageType,
    log_level: u8,
    keep: bool,
    timeout: Duration,
}

impl Builder {
    pub fn new(
        logger: TaskLogger,
        soar_env: SoarEnv,
        external: bool,
        log_level: u8,
        keep: bool,
        timeout: Duration,
    ) -> Self {
        Builder {
            logger,
            soar_env,
            external,
            desktop: HashMap::new(),
            icon: HashMap::new(),
            appstream: HashMap::new(),
            pkg_type: PackageType::Unknown,
            log_level,
            keep,
            timeout,
        }
    }

    pub async fn download_build_assets(&mut self, build_assets: &[BuildAsset]) {
        for asset in build_assets {
            self.logger
                .info(format!("Downloading build asset from {}", asset.url));

            let out_path = format!("SBUILD_TEMP/{}", asset.out);
            if download(&asset.url, out_path).await.is_err() {
                self.logger
                    .error(format!("Failed to download build asset from {}", asset.url));
                std::process::exit(1);
            };
        }
    }

    pub async fn handle_license(&mut self, licenses: &[License]) {
        for license in licenses {
            match license {
                License::Complex(license_complex) => {
                    if let Some(ref file) = license_complex.file {
                        let file_path = Path::new(file.trim_start_matches('/'));
                        if file_path.exists() {
                            self.logger
                                .info(format!("Copying license from {} to LICENSE", file));
                            fs::copy(&file_path, "LICENSE").unwrap();
                            fs::remove_file(&file_path).unwrap();
                            return;
                        }
                    } else if let Some(ref url) = license_complex.url {
                        self.logger
                            .info(format!("Downloading license from {} to LICENSE", url));
                        if download(url, "LICENSE").await.is_err() {
                            self.logger
                                .warn(format!("Failed to download license from {}", url));
                        };
                    }
                }
                _ => {}
            }
        }
    }

    async fn prepare_resources(
        &mut self,
        build_config: &BuildConfig,
        context: &BuildContext,
    ) -> Result<(), String> {
        if let Some(ref desktop) = build_config.desktop {
            let out_path = if let Some(ref file) = desktop.file {
                self.logger.info(&format!("Using local file from {}", file));
                extract_filename(file)
            } else if let Some(ref dir) = desktop.dir {
                let out_path = format!("{}/{}.desktop", dir, build_config.pkg);
                self.logger
                    .info(&format!("Using local file from {}", out_path));
                out_path
            } else {
                let url = &desktop.url.clone().unwrap();
                let out_path = extract_filename(url);
                self.logger.info(&format!(
                    "Downloading desktop file from {} to {}",
                    url, out_path
                ));
                download(url, &out_path).await?;
                out_path
            };

            let out_path = Path::new(&out_path);
            if out_path.exists() {
                let final_path = format!("{}.desktop", build_config.pkg);
                fs::rename(out_path, final_path).unwrap();
                self.desktop.insert(context.pkg.clone(), true);
            } else {
                self.logger.warn(&format!(
                    "Desktop file not found in {}. Skipping...",
                    out_path.display()
                ));
            }
        }

        if let Some(ref icon) = build_config.icon {
            let out_path = if let Some(ref file) = icon.file {
                self.logger.info(&format!("Using local file from {}", file));
                extract_filename(file)
            } else if let Some(ref dir) = icon.dir {
                let dir_path = Path::new(dir);

                let find_diricon = |dir_path: &Path| -> Result<Option<String>, String> {
                    for entry in fs::read_dir(dir_path)
                        .map_err(|err| format!("Unable to search dir {}: {:#?}", dir, err))?
                    {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            if path.is_file() {
                                if path.file_name() == Some(".DirIcon".as_ref()) {
                                    return Ok(Some(path.to_string_lossy().into_owned()));
                                }
                            }
                        }
                    }
                    Ok(None)
                };

                let found_path = find_diricon(dir_path)?.or_else(|| {
                    for extension in ["png", "svg"] {
                        for entry in fs::read_dir(dir_path).unwrap() {
                            if let Ok(entry) = entry {
                                let path = entry.path();
                                if path.is_file() {
                                    if let Some(ext) = path
                                        .extension()
                                        .and_then(|ext| ext.to_str())
                                        .map(|s| s.to_lowercase())
                                    {
                                        if ext == extension {
                                            return Some(path.to_string_lossy().into_owned());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None
                });

                if let Some(found) = found_path {
                    self.logger
                        .info(&format!("Using local file from {}", found));
                    found
                } else {
                    format!("{}/.DirIcon", dir)
                }
            } else {
                let url = &icon.url.clone().unwrap();
                let out_path = extract_filename(url);
                self.logger
                    .info(&format!("Downloading icon from {} to {}", url, out_path));
                download(url, &out_path).await?;
                out_path
            };

            let out_path = Path::new(&out_path);
            if out_path.exists() {
                let magic_bytes = calc_magic_bytes(&out_path, 8);

                if let Some(extension) = if magic_bytes == PNG_MAGIC_BYTES {
                    Some("png")
                } else if magic_bytes[..4] == SVG_MAGIC_BYTES || magic_bytes[..5] == XML_MAGIC_BYTES
                {
                    Some("svg")
                } else {
                    None
                } {
                    let final_path = format!("{}.{}", build_config.pkg, extension);
                    self.logger.info(&format!("Renamed icon to {}", final_path));
                    fs::rename(out_path, final_path).unwrap();
                    self.icon.insert(context.pkg.clone(), true);
                } else {
                    let tmp_path = context.tmpdir.join(out_path);
                    fs::rename(&out_path, &tmp_path).unwrap();
                    self.logger.warn(&format!(
                        "Unsupported icon. Moved to {}",
                        tmp_path.display()
                    ));
                }
            } else {
                self.logger.warn(&format!(
                    "Icon not found in {}. Skipping...",
                    out_path.display()
                ));
            }
        }

        Ok(())
    }

    fn setup_output_handlers(&self) -> (sync::mpsc::Sender<OutputStream>, thread::JoinHandle<()>) {
        let (tx, rx) = sync::mpsc::channel();
        let logger = Arc::new(self.logger.clone());

        let output_handle = thread::spawn(move || {
            while let Ok(output) = rx.recv() {
                match output {
                    OutputStream::Stdout(msg) => {
                        logger.info(&msg);
                    }
                    OutputStream::Stderr(msg) => {
                        logger.custom_error(&msg);
                    }
                }
            }
        });

        (tx, output_handle)
    }

    fn setup_cmd_logging(&self, child: &mut Child) {
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let (tx, output_handle) = self.setup_output_handlers();
        let tx_stderr = tx.clone();

        let stdout_handle = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            reader.lines().for_each(|line| {
                if let Ok(line) = line {
                    let _ = tx.send(OutputStream::Stdout(line));
                }
            })
        });

        let stderr_handle = std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            reader.lines().for_each(|line| {
                if let Ok(line) = line {
                    let _ = tx_stderr.send(OutputStream::Stderr(line));
                }
            })
        });

        stdout_handle.join().unwrap();
        stderr_handle.join().unwrap();
        output_handle.join().unwrap();
    }

    pub async fn exec(
        &mut self,
        context: &BuildContext,
        build_config: BuildConfig,
        exec_file: String,
    ) -> bool {
        env::set_current_dir(&context.outdir).unwrap();

        fs::create_dir_all(&context.tmpdir).unwrap();

        // if the builder is invoked from soar, need to find a better way to install
        // build utils
        if self.external {
            if let Some(build_utils) = build_config.build_util.clone() {
                let mut child = Command::new("soar")
                    .env_clear()
                    .envs(context.env_vars(&self.soar_env.bin_path))
                    .args(["add".to_string()].iter().chain(build_utils.iter()))
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap();
                self.setup_cmd_logging(&mut child);
                let status = child.wait().unwrap();
                if !status.success() {
                    self.logger.error("Failed to install build utils");
                    return false;
                }
            };
        }

        if let Some(ref build_assets) = build_config.build_asset {
            self.download_build_assets(build_assets).await;
        }

        if let Some(ref licenses) = build_config.license {
            self.handle_license(licenses).await;
        }

        let mut child = Command::new(exec_file)
            .env_clear()
            .envs(context.env_vars(&self.soar_env.bin_path))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();

        if let Err(err) = self.prepare_resources(&build_config, context).await {
            self.logger.warn(&err);
        }

        self.setup_cmd_logging(&mut child);

        let _ = child.wait().unwrap();

        if let Some(entrypoint) = build_config
            .x_exec
            .entrypoint
            .as_ref()
            .map(|e| e.trim_start_matches('/').to_string())
        {
            let entry_path = Path::new(&entrypoint);
            if entry_path.exists() {
                symlink(entrypoint, build_config.pkg.clone()).unwrap();
            } else {
                self.logger.error(format!(
                    "Entrypoint {} should exist in {} but doesn't.",
                    entrypoint,
                    context.outdir.display()
                ));
                return false;
            }
        }

        self.handle_provides(&context, &build_config);

        let mut finalize = Finalize::new(
            &context.outdir,
            build_config,
            self.pkg_type.clone(),
            self.keep,
        );
        if let Err(e) = finalize.update().await {
            self.logger
                .error(format!("Failed to finalize build: {}", e));
            return false;
        }
        true
    }

    pub async fn build(
        &mut self,
        file_path: &str,
        outdir: Option<String>,
        timeout: Duration,
    ) -> bool {
        let logger = self.logger.clone();
        let linter = Linter::new(logger.clone(), timeout);

        let pwd = env::current_dir().unwrap();
        let mut success = false;

        let validated_file = format!("{}.validated", file_path);
        let version_file = format!("{}.pkgver", file_path);

        if let Some(build_config) = linter.lint(file_path, false, false, true) {
            logger.info(format!("{}", fs::read_to_string(&validated_file).unwrap()));
            if build_config._disabled {
                logger.error(format!("{} -> Disabled package. Skipping...", file_path));
                if let Some(reason) = build_config._disabled_reason {
                    logger.error(format!("{} -> {:#?}", file_path, reason));
                }
            } else {
                let version = fs::read_to_string(&version_file).ok();

                if version.is_none() {
                    return false;
                }

                let version = version.unwrap();
                let x_exec = &build_config.x_exec;
                let pkg_id = &build_config.pkg_id;
                let script = format!(
                    "#!/usr/bin/env {}\n{}\n{}",
                    x_exec.shell,
                    match self.log_level {
                        2 => "set -x",
                        3 => "set -xv",
                        _ => "",
                    },
                    x_exec.run
                );
                let tmp = temp_file(pkg_id, &script);

                let context =
                    BuildContext::new(&build_config, &self.soar_env.cache_path, version, outdir);
                let _ = fs::remove_dir_all(&context.outdir);
                fs::create_dir_all(&context.outdir).unwrap();
                let final_version_file =
                    format!("{}/{}.version", context.outdir.display(), context.pkg);
                let final_validated_file = format!("{}/SBUILD", context.outdir.display());
                fs::copy(&version_file, &final_version_file).unwrap();
                fs::copy(&validated_file, &final_validated_file).unwrap();

                let log_path = context.outdir.join("BUILD.log");
                logger.move_log_file(log_path).unwrap();

                if let Some(ref arch) = x_exec.arch {
                    if !arch.contains(&ARCH.to_string()) {
                        logger.error(format!("Unsupported architecture. Aborting..."));
                        return false;
                    }
                }

                if let Some(ref arch) = x_exec.os {
                    if !arch.contains(&OS.to_string()) {
                        logger.error(format!("Unsupported OS. Aborting..."));
                        return false;
                    }
                }

                if let Some(ref host) = x_exec.host {
                    let current_host = format!("{ARCH}-{OS}");
                    if !host.contains(&current_host) {
                        logger.error(format!("Unsupported HOST. Aborting..."));
                        return false;
                    }
                }

                success = self
                    .exec(&context, build_config, tmp.to_string_lossy().to_string())
                    .await;
                if success {
                    logger.success(format!(
                        "Successfully built the package at {}",
                        context.outdir.display()
                    ));
                } else {
                    logger.success(&format!("Failed to build the package: {}", context.pkg));
                }
            }
        } else {
            success = false;
        }

        env::set_current_dir(pwd).unwrap();

        let _ = fs::remove_file(validated_file);
        let _ = fs::remove_file(version_file);
        success
    }

    pub fn handle_provides(&mut self, context: &BuildContext, build_config: &BuildConfig) {
        let pkg_name = &build_config.pkg;
        let pkg_type = &build_config.pkg_type;

        let provides = build_config.provides.clone();
        let provides = if build_config.x_exec.entrypoint.is_some() {
            vec![pkg_name.clone()]
        } else {
            provides.unwrap_or_else(|| vec![pkg_name.clone()])
        };

        let mut exists_any = false;

        for provide in provides {
            let cmd = provide
                .split_once(|c| c == ':' || c == '=')
                .map(|(p1, _)| p1.to_string())
                .unwrap_or_else(|| provide.to_string());
            let provide_path = Path::new(&cmd);

            if !provide_path.exists() {
                self.logger
                    .warn(format!("Provide '{}' does not exist.", provide));
                continue;
            }

            exists_any = true;

            let magic_bytes = calc_magic_bytes(&provide_path, 12);

            if magic_bytes[4] != 2 {
                self.logger
                    .error("32-bit binary is not supported. Aborting...");
                std::process::exit(1);
            }

            if magic_bytes[8..] == APPIMAGE_MAGIC_BYTES {
                let filter = if *pkg_type == Some("nixappimage".into()) {
                    self.pkg_type = PackageType::NixAppImage;
                    Some(pkg_name.as_str())
                } else {
                    self.pkg_type = PackageType::AppImage;
                    None
                };

                let offset = get_offset(&provide_path).unwrap();

                if !is_static_elf(&provide_path) {
                    self.logger.info(format!(
                        "{} -> Dynamic AppImage. Attempting to convert it to static.",
                        &provide_path.display()
                    ));
                    let tmp_path = "SBUILD_TEMP/squashfs_tmp/";
                    let file_path = &provide_path.to_string_lossy().to_string();
                    let env_vars = context.env_vars(&self.soar_env.bin_path);

                    let Ok(usqfs) = which::which("unsquashfs") else {
                        self.logger
                            .warn("unsquashfs not found. Skipping conversion.");
                        continue;
                    };

                    let mut child = Command::new(usqfs)
                        .env_clear()
                        .envs(env_vars.clone())
                        .args([
                            "-offset",
                            &offset.to_string(),
                            "-force",
                            "-dest",
                            tmp_path,
                            file_path,
                        ])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .stdin(Stdio::null())
                        .spawn()
                        .unwrap();

                    let _ = child.wait().unwrap();
                    if !Path::new(tmp_path).exists() {
                        self.logger.warn("Failed to unpack appimage");
                    }
                    if pack_appimage(env_vars, tmp_path, &file_path, &self.logger) {
                        self.logger.info(format!(
                            "{} -> Successfully converted to static AppImage.",
                            &provide_path.display()
                        ));
                    };
                }

                let Ok(appimage) = AppImage::new(filter, &provide_path, None) else {
                    self.logger.warn(format!(
                        "Tried reading {} as AppImage but couldn't.",
                        provide_path.display()
                    ));
                    continue;
                };
                let squashfs = &appimage.squashfs;

                if self.icon.get(&provide).is_none() {
                    if let Some(entry) = appimage.find_icon() {
                        if let EntryKind::File(basic_file) = entry.kind {
                            let dest = format!("{}.DirIcon", cmd);
                            let _ = squashfs.write_file(basic_file, &dest);
                            self.logger.info(&format!(
                                "Extracted {} to {}",
                                entry.path.display(),
                                dest
                            ));

                            let magic_bytes = calc_magic_bytes(&dest, 8);
                            let extension = if magic_bytes == PNG_MAGIC_BYTES {
                                "png"
                            } else {
                                "svg"
                            };
                            let final_path = format!("{}.{}", cmd, extension);
                            fs::rename(&dest, &final_path).unwrap();

                            self.logger
                                .info(&format!("Renamed {} to {}", dest, final_path));
                            self.icon.insert(provide.clone(), true);
                        }
                    }
                }
                if self.desktop.get(&provide).is_none() {
                    if let Some(entry) = appimage.find_desktop() {
                        if let EntryKind::File(basic_file) = entry.kind {
                            let dest = format!("{}.desktop", cmd);
                            let _ = squashfs.write_file(basic_file, &dest);
                            self.logger.info(&format!(
                                "Extracted {} to {}",
                                entry.path.display(),
                                dest
                            ));
                            self.desktop.insert(provide.clone(), true);
                        }
                    };
                }
                if self.appstream.get(&provide).is_none() {
                    if let Some(entry) = appimage.find_appstream() {
                        if let EntryKind::File(basic_file) = entry.kind {
                            let file_name = if entry
                                .path
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .contains("appdata")
                            {
                                "appdata"
                            } else {
                                "metainfo"
                            };
                            let dest = format!("{}.{}.xml", cmd, file_name);
                            let _ = squashfs.write_file(basic_file, &dest);
                            self.logger.info(&format!(
                                "Extracted {} to {}",
                                entry.path.display(),
                                dest
                            ));
                            self.appstream.insert(provide.clone(), true);
                        }
                    };
                }
            } else if magic_bytes[4..8] == FLATIMAGE_MAGIC_BYTES {
                self.pkg_type = PackageType::FlatImage
            } else if magic_bytes[..4] == ELF_MAGIC_BYTES {
                self.pkg_type = if is_static_elf(&provide_path) {
                    PackageType::Static
                } else {
                    PackageType::Dynamic
                };
            };

            if self.pkg_type == PackageType::Unknown {
                self.logger
                    .error(format!("Unsupported binary file {}. Aborting.", cmd));
                std::process::exit(1);
            }
        }

        if !exists_any {
            self.logger.error("None of the provides exist. Aborting.");
            std::process::exit(1);
        }
    }
}
