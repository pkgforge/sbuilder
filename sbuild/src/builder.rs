use std::{
    env,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{self, Arc},
    thread,
    time::Duration,
};

use goblin::elf::Elf;
use memmap2::Mmap;
use sbuild_linter::{build_config::BuildConfig, logger::TaskLogger, BuildAsset, Linter};
use squishy::{appimage::AppImage, EntryKind};

use crate::{
    cleanup::Finalize,
    constant::{
        APPIMAGE_MAGIC_BYTES, ELF_MAGIC_BYTES, FLATIMAGE_MAGIC_BYTES, PNG_MAGIC_BYTES,
        SVG_MAGIC_BYTES, XML_MAGIC_BYTES,
    },
    types::{OutputStream, PackageType, SoarEnv},
    utils::{calc_magic_bytes, download, extract_filename, temp_file},
};

pub struct BuildContext {
    pkg: String,
    pkg_id: String,
    pkg_type: Option<String>,
    sbuild_pkg: String,
    outdir: String,
    tmpdir: String,
    version: String,
}

impl BuildContext {
    fn new<P: AsRef<Path>>(build_config: &BuildConfig, cache_path: P, version: String) -> Self {
        let sbuild_pkg = build_config
            .pkg_type
            .as_ref()
            .map(|t| format!("{}.{}", build_config.pkg, t))
            .unwrap_or(build_config.pkg.clone());

        let outdir = format!(
            "{}/sbuild/{}",
            cache_path.as_ref().display(),
            build_config.pkg_id
        );
        let tmpdir = format!("{}/SBUILD_TEMP", outdir);

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

        let existing_envs = [
            ("USER_AGENT", env::var("USER_AGENT").ok()),
            ("GITLAB_TOKEN", env::var("GITLAB_TOKEN").ok()),
            ("GL_TOKEN", env::var("GL_TOKEN").ok()),
            ("GITHUB_TOKEN", env::var("GITHUB_TOKEN").ok()),
            ("GH_TOKEN", env::var("GH_TOKEN").ok()),
            ("TERM", env::var("TERM").ok()),
        ];

        let paths = format!("{}:{}", soar_bin, paths);
        let mut vars: Vec<(String, String)> = [
            ("pkg", self.pkg.clone()),
            ("pkg_id", self.pkg_id.clone()),
            ("pkg_type", self.pkg_type.clone().unwrap_or_default()),
            ("sbuild_pkg", self.sbuild_pkg.clone()),
            ("sbuild_outdir", self.outdir.clone()),
            ("sbuild_tmpdir", self.tmpdir.clone()),
            ("pkg_ver", self.version.clone()),
        ]
        .into_iter()
        .flat_map(|(key, value)| {
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
                .filter_map(|(key, value)| value.map(|val| (key.to_string(), val))),
        );
        vars
    }
}

pub struct Builder {
    logger: TaskLogger,
    soar_env: SoarEnv,
    external: bool,
    desktop: bool,
    icon: bool,
    appstream: bool,
    pkg_type: PackageType,
}

impl Builder {
    pub fn new(logger: TaskLogger, soar_env: SoarEnv, external: bool) -> Self {
        Builder {
            logger,
            soar_env,
            external,
            desktop: false,
            icon: false,
            appstream: false,
            pkg_type: PackageType::Unknown,
        }
    }

    pub async fn download_build_assets(&mut self, build_assets: &[BuildAsset]) {
        for asset in build_assets {
            self.logger
                .info(&format!("Downloading build asset from {}", asset.url));

            let out_path = format!("SBUILD_TEMP/{}", asset.out);
            download(&asset.url, out_path).await.unwrap();
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
                let out_path = format!("{}/{}.desktop", dir, context.sbuild_pkg);
                self.logger
                    .info(&format!("Using local file from {}", out_path));
                out_path
            } else {
                let url = &desktop.url.clone().unwrap();
                let out_path = extract_filename(url);
                self.logger
                    .info(&format!("Downloading desktop file from {}", url));
                download(url, &out_path).await?;
                out_path
            };

            let out_path = Path::new(&out_path);
            if out_path.exists() {
                let final_path = format!("{}.desktop", context.sbuild_pkg);
                fs::rename(out_path, final_path).unwrap();
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
                self.logger.info(&format!("Downloading icon from {}", url));
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
                    let final_path = format!("{}.{}", context.sbuild_pkg, extension);
                    self.logger.info(&format!("Renamed icon to {}", final_path));
                    fs::rename(out_path, final_path).unwrap();
                } else {
                    let tmp_path = format!("{}/{}", context.tmpdir, out_path.display());
                    fs::rename(&out_path, &tmp_path).unwrap();
                    self.logger
                        .warn(&format!("Unsupported icon. Moved to {}", tmp_path));
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

        let mut child = Command::new(exec_file)
            .env_clear()
            .envs(context.env_vars(&self.soar_env.bin_path))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();

        if let Err(e) = self.prepare_resources(&build_config, context).await {
            self.logger.error(&e);
            return false;
        }

        self.setup_cmd_logging(&mut child);

        let status = child.wait().unwrap();
        if !status.success() {
            self.logger
                .error(&format!("Build failed with status: {}", status));
            return false;
        }

        let bin_path = Path::new(&context.sbuild_pkg);
        if !bin_path.exists() {
            self.logger.error(&format!(
                "{} should exist in {} but doesn't.",
                context.sbuild_pkg, context.outdir
            ));
            return false;
        }

        self.do_work(bin_path, &context.sbuild_pkg);

        let cleanup = Finalize::new(
            context.sbuild_pkg.clone(),
            &context.outdir,
            build_config,
            self.pkg_type.clone(),
        );
        if let Err(e) = cleanup.cleanup().await {
            self.logger
                .error(&format!("Failed to cleanup files: {}", e));
            return false;
        }
        true
    }

    pub fn validate_files(&mut self, context: &BuildContext) {
        let files = env::current_dir().unwrap().read_dir().unwrap();

        for entry in files {
            let Ok(entry) = entry else { continue };
            if let Some(file_name) = entry.file_name().to_str() {
                if file_name == format!("{}.png", context.sbuild_pkg)
                    || file_name == format!("{}.svg", context.sbuild_pkg)
                {
                    self.icon = true;
                }
                if file_name == format!("{}.appdata.xml", context.sbuild_pkg)
                    || file_name == format!("{}.metainfo.xml", context.sbuild_pkg)
                {
                    self.appstream = true;
                }
                if file_name == format!("{}.desktop", context.sbuild_pkg) {
                    self.desktop = true;
                }
            }
        }
    }

    pub async fn build(&mut self, file_path: &str) -> bool {
        let logger = self.logger.clone();
        let linter = Linter::new(logger.clone(), Duration::from_secs(120));

        let pwd = env::current_dir().unwrap();
        let mut success = false;

        let validated_file = format!("{}.validated", file_path);
        let version_file = format!("{}.pkgver", file_path);

        if let Some(build_config) = linter.lint(file_path, false, false, true) {
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
                let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, x_exec.run);
                let tmp = temp_file(pkg_id, &script);

                let context = BuildContext::new(&build_config, &self.soar_env.cache_path, version);
                let _ = fs::remove_dir_all(&context.outdir);
                fs::create_dir_all(&context.outdir).unwrap();
                let final_version_file =
                    format!("{}/{}.version", context.outdir, context.sbuild_pkg);
                let final_validated_file =
                    format!("{}/{}.validated", context.outdir, context.sbuild_pkg);
                fs::copy(&version_file, &final_version_file).unwrap();
                fs::copy(&version_file, &final_validated_file).unwrap();

                let log_path = format!("{}/build.log", context.outdir);
                logger.move_log_file(log_path).unwrap();
                success = self
                    .exec(&context, build_config, tmp.to_string_lossy().to_string())
                    .await;
                logger.success(&format!(
                    "Successfully built the package at {}",
                    context.outdir
                ));
            }
        } else {
            success = false;
        }

        env::set_current_dir(pwd).unwrap();

        let _ = fs::remove_file(validated_file);
        let _ = fs::remove_file(version_file);
        success
    }

    pub fn do_work<P: AsRef<Path>>(&mut self, file_path: P, pkg_name: &str) {
        let magic_bytes = calc_magic_bytes(&file_path, 12);

        if magic_bytes[8..] == APPIMAGE_MAGIC_BYTES {
            self.pkg_type = PackageType::AppImage;
            let appimage = AppImage::new(None, &file_path, None).unwrap();
            let squashfs = &appimage.squashfs;

            if !self.icon {
                if let Some(entry) = appimage.find_icon() {
                    if let EntryKind::File(basic_file) = entry.kind {
                        let dest = format!("{}.DirIcon", pkg_name);
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
                        let final_path = format!("{}.{}", pkg_name, extension);
                        fs::rename(&dest, &final_path).unwrap();

                        self.logger
                            .info(&format!("Renamed {} to {}", dest, final_path));
                        self.icon = true;
                    }
                }
            }
            if !self.desktop {
                if let Some(entry) = appimage.find_desktop() {
                    if let EntryKind::File(basic_file) = entry.kind {
                        let dest = format!("{}.desktop", pkg_name);
                        let _ = squashfs.write_file(basic_file, &dest);
                        self.logger.info(&format!(
                            "Extracted {} to {}",
                            entry.path.display(),
                            dest
                        ));
                        self.desktop = true;
                    }
                };
            }
            if !self.appstream {
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
                        let dest = format!("{}.{}.xml", pkg_name, file_name);
                        let _ = squashfs.write_file(basic_file, &dest);
                        self.logger.info(&format!(
                            "Extracted {} to {}",
                            entry.path.display(),
                            dest
                        ));
                        self.appstream = true;
                    }
                };
            }
        } else if magic_bytes[4..8] == FLATIMAGE_MAGIC_BYTES {
            self.pkg_type = PackageType::FlatImage
        } else if magic_bytes[..4] == ELF_MAGIC_BYTES {
            let file = File::open(file_path).unwrap();
            let mmap = unsafe { Mmap::map(&file).unwrap() };
            let elf = Elf::parse(&mmap).unwrap();

            self.pkg_type = if elf.interpreter.is_some() {
                PackageType::Dynamic
            } else {
                PackageType::Static
            };
        };

        if self.pkg_type == PackageType::Unknown {
            self.logger.error("Unsupported binary file. Aborting.");
            std::process::exit(1);
        }
    }
}
