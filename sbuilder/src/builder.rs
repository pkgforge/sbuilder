use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{self, Arc},
    thread,
};

use futures::StreamExt;
use reqwest::header::USER_AGENT;
use sbuild_linter::{build_config::BuildConfig, logger::Logger, BuildAsset, Linter};
use squishy::{appimage::AppImage, EntryKind};

use crate::{
    cleanup::FileCleanup,
    constant::{APPIMAGE_MAGIC_BYTES, ELF_MAGIC_BYTES, FLATIMAGE_MAGIC_BYTES, PNG_MAGIC_BYTES},
    types::{OutputStream, SoarEnv},
    utils::{calc_magic_bytes, extract_filename, temp_file},
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
        let paths = std::env::var("PATH").unwrap_or_default();
        let paths = format!("{}:{}", soar_bin, paths);
        [
            ("pkg", self.pkg.clone()),
            ("pkg_id", self.pkg_id.clone()),
            ("pkg_type", self.pkg_type.clone().unwrap_or_default()),
            ("sbuild_pkg", self.sbuild_pkg.clone()),
            ("sbuild_outdir", self.outdir.clone()),
            ("sbuild_tmpdir", self.tmpdir.clone()),
            ("user_agent", "pkgforge/soar".to_string()),
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
        .collect()
    }
}

pub struct Builder {
    logger: Logger,
    soar_env: SoarEnv,
    external: bool,
}

impl Builder {
    pub fn new(logger: Logger, soar_env: SoarEnv, external: bool) -> Self {
        Builder {
            logger,
            soar_env,
            external,
        }
    }

    async fn download<P: AsRef<Path>>(&self, url: &str, out: P) -> Result<(), String> {
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .header(USER_AGENT, "pkgforge/soar")
            .send()
            .await
            .unwrap();

        if !response.status().is_success() {
            return Err(format!("Error download build asset from {}", url));
        }

        let output_path = out.as_ref();
        if let Some(output_dir) = output_path.parent() {
            if !output_dir.exists() {
                fs::create_dir_all(output_dir).unwrap();
            }
        }

        let temp_path = format!("{}.part", output_path.display());
        let mut stream = response.bytes_stream();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&temp_path)
            .unwrap();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            file.write_all(&chunk).unwrap();
        }

        fs::rename(&temp_path, output_path).unwrap();

        Ok(())
    }

    pub async fn download_build_assets(&self, build_assets: &[BuildAsset]) {
        for asset in build_assets {
            self.logger
                .info(&format!("Downloading build asset from {}", asset.url));

            let out_path = format!("SBUILD_TEMP/{}", asset.out);
            self.download(&asset.url, out_path).await.unwrap();
        }
    }

    async fn prepare_assets(&self, build_config: &BuildConfig) -> Result<(), String> {
        if let Some(ref build_assets) = build_config.build_asset {
            self.download_build_assets(build_assets).await;
        }

        if let Some(ref desktop) = build_config.desktop {
            let out_path = extract_filename(desktop);
            self.download(desktop, &out_path).await?;
            let final_path = format!("{}.desktop", build_config.pkg);
            fs::rename(out_path, final_path).unwrap();
        }

        if let Some(ref icon) = build_config.icon {
            let out_path = extract_filename(icon);
            self.download(icon, &out_path).await?;
            let magic_bytes = calc_magic_bytes(&out_path, 8);
            let extension = if magic_bytes == PNG_MAGIC_BYTES {
                "png"
            } else {
                "svg"
            };
            let final_path = format!("{}.{}", build_config.pkg, extension);
            fs::rename(out_path, final_path).unwrap();
        }

        Ok(())
    }

    fn setup_output_handlers(&self) -> (sync::mpsc::Sender<OutputStream>, thread::JoinHandle<()>) {
        let (tx, rx) = sync::mpsc::channel();
        let logger = Arc::new(self.logger.clone());

        let log_file = File::create("build.log").unwrap();
        let mut writer = BufWriter::new(log_file);

        let output_handle = thread::spawn(move || {
            while let Ok(output) = rx.recv() {
                match output {
                    OutputStream::Stdout(msg) => {
                        logger.info(&msg);
                        writeln!(writer, "{}", msg).unwrap();
                    }
                    OutputStream::Stderr(msg) => {
                        logger.custom_error(&msg);
                        writeln!(writer, "{}", msg).unwrap();
                    }
                }
            }
        });

        (tx, output_handle)
    }

    pub async fn exec(
        &self,
        context: &BuildContext,
        build_config: BuildConfig,
        exec_file: String,
    ) -> bool {
        env::set_current_dir(&context.outdir).unwrap();

        // if the builder is invoked from soar, need to find a better way to install
        // build utils
        if self.external {
            let build_utils = build_config.build_util.clone().unwrap_or_default();
            let mut child = Command::new("soar")
                .args(["add".to_string()].iter().chain(build_utils.iter()))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();

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

            let status = child.wait().unwrap();
            if !status.success() {
                self.logger.error("Failed to install build utils");
                return false;
            }
        }

        if let Err(e) = self.prepare_assets(&build_config).await {
            self.logger.error(&e);
            return false;
        }

        let mut child = Command::new(exec_file)
            .envs(context.env_vars(&self.soar_env.bin_path))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

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

        let cleanup = FileCleanup::new(context.sbuild_pkg.clone(), &context.outdir);
        if let Err(e) = cleanup.cleanup() {
            self.logger
                .error(&format!("Failed to cleanup files: {}", e));
            return false;
        }
        true
    }

    pub async fn build(&self, file_path: &str) -> bool {
        let logger = &self.logger;
        let linter = Linter::new(logger.clone());

        let pwd = env::current_dir().unwrap();
        let mut success = false;

        if let Some(build_config) = linter.lint(file_path, false, false, true) {
            if build_config._disabled {
                logger.error(&format!("{} -> Disabled package. Skipping...", file_path));
            } else {
                let version_file = format!("{}.pkgver", file_path);
                let version = fs::read_to_string(&version_file).ok();

                if version.is_none() {
                    return false;
                }

                let version = version.unwrap();
                let x_exec = &build_config.x_exec;
                let app_id = build_config.app_id.clone().unwrap();
                let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, x_exec.run);
                let tmp = temp_file(&app_id, &script);

                let context = BuildContext::new(&build_config, &self.soar_env.cache_path, version);
                fs::create_dir_all(&context.outdir).unwrap();
                let final_version_file =
                    format!("{}/{}.version", context.outdir, context.sbuild_pkg);
                fs::rename(version_file, final_version_file).unwrap();

                success = self
                    .exec(&context, build_config, tmp.to_string_lossy().to_string())
                    .await;
            }
        } else {
            success = false;
        }

        env::set_current_dir(pwd).unwrap();
        success
    }

    pub fn do_work<P: AsRef<Path>>(&self, file_path: P, pkg_name: &str) {
        let magic_bytes = calc_magic_bytes(&file_path, 12);
        if magic_bytes[0..4] != ELF_MAGIC_BYTES {
            self.logger.error("not an ELF");
            std::process::exit(1);
        }

        if magic_bytes[8..] == APPIMAGE_MAGIC_BYTES {
            let appimage = AppImage::new(None, &file_path, None).unwrap();
            let squashfs = &appimage.squashfs;

            if let Some(entry) = appimage.find_icon() {
                if let EntryKind::File(basic_file) = entry.kind {
                    let dest = format!("{}.DirIcon", pkg_name);
                    let _ = squashfs.write_file(basic_file, &dest);
                    self.logger
                        .info(&format!("Extracted {} to {}", entry.path.display(), dest));
                }
            }
            if let Some(entry) = appimage.find_desktop() {
                if let EntryKind::File(basic_file) = entry.kind {
                    let dest = format!("{}.desktop", pkg_name);
                    let _ = squashfs.write_file(basic_file, &dest);
                    self.logger
                        .info(&format!("Extracted {} to {}", entry.path.display(), dest));
                }
            };
            if let Some(entry) = appimage.find_appstream() {
                if let EntryKind::File(basic_file) = entry.kind {
                    let dest = format!(
                        "{}.{}",
                        pkg_name,
                        entry.path.file_name().unwrap().to_string_lossy()
                    );
                    let _ = squashfs.write_file(basic_file, &dest);
                    self.logger
                        .info(&format!("Extracted {} to {}", entry.path.display(), dest));
                }
            };
        } else if magic_bytes[4..8] == FLATIMAGE_MAGIC_BYTES {
            // TODO
        };
    }
}
