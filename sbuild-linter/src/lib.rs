use std::{
    collections::HashSet,
    env,
    fmt::Display,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::PathBuf,
    process::{Command, ExitStatus},
    sync, thread,
    time::Duration,
};

use build_config::{visitor::BuildConfigVisitor, BuildConfig};
use colored::Colorize;
use comments::Comments;
use logger::Logger;
use nanoid::nanoid;
use serde::{Deserialize, Deserializer};

pub mod build_config;
pub mod comments;
pub mod description;
pub mod distro_pkg;
pub mod error;
pub mod license;
pub mod logger;
pub mod resource;
pub mod semaphore;
pub mod validator;
pub mod xexec;

pub const VALID_PKG_TYPES: [&str; 9] = [
    "appbundle",
    "appimage",
    "archive",
    "dynamic",
    "flatimage",
    "gameimage",
    "nixappimage",
    "runimage",
    "static",
];
pub const VALID_CATEGORIES: &str = include_str!("categories");
pub const VALID_ARCH: [&str; 4] = ["aarch64", "loongarch64", "riscv64", "x86_64"];
pub const VALID_OS: [&str; 6] = ["freebsd", "illumos", "linux", "netbsd", "openbsd", "redox"];

#[derive(Debug, Deserialize, Clone)]
pub struct BuildAsset {
    pub url: String,
    pub out: String,
}

pub struct Linter {
    logger: Logger,
    timeout: Duration,
}

impl Linter {
    pub fn new(logger: Logger, timeout: Duration) -> Self {
        Linter { logger, timeout }
    }

    pub fn lint(
        &self,
        file_path: &str,
        inplace: bool,
        disable_shellcheck: bool,
        pkgver: bool,
    ) -> Option<BuildConfig> {
        let logger = &self.logger;
        let yaml_str = match self.read_yaml(file_path) {
            Ok(y) => y,
            Err(err) => {
                eprintln!("{}", err);
                return None;
            }
        };

        logger.info(&format!("Linting {}", file_path));
        match self.deserialize_yaml(&yaml_str) {
            Ok(config) => {
                if disable_shellcheck {
                    logger.info("Skipping shellcheck");
                } else {
                    logger.info("Performing shellcheck");
                    if !self.is_shellcheck_success(&config) {
                        return None;
                    }
                    logger.success("Shellcheck passed");
                }
                if let Some(pkgver_path) = pkgver.then(|| format!("{}.pkgver", file_path)) {
                    if !self.generate_pkgver(&config, &pkgver_path) {
                        return None;
                    }
                };

                let mut comments = Comments::new();
                comments.parse_comments(file_path).unwrap();

                let output_path = inplace
                    .then_some(file_path.to_string())
                    .unwrap_or_else(|| format!("{}.validated", file_path));
                let file = File::create(&output_path).unwrap();
                let mut writer = BufWriter::new(file);

                config.write_yaml(&mut writer, 0, comments).unwrap();
                logger.info("SBUILD validation successful.");
                logger.info(&format!(
                    "Validated YAML has been written to {}",
                    output_path
                ));
                return Some(config);
            }
            Err(_) => {
                logger.error("SBUILD validation faild.");
            }
        };
        None
    }

    fn deserialize_yaml(&self, yaml_str: &str) -> Result<BuildConfig, serde_yml::Error> {
        let deserializer = serde_yml::Deserializer::from_str(yaml_str);
        let visitor = BuildConfigVisitor {
            sbuild_str: yaml_str.to_string(),
            visited: HashSet::new(),
            errors: Vec::new(),
            logger: self.logger.clone(),
        };
        deserializer.deserialize_map(visitor)
    }

    fn read_yaml(&self, file_path: &str) -> Result<String, FileError> {
        let logger = &self.logger;
        let Ok(file) = File::open(file_path) else {
            return Err(FileError::NotFound(file_path.into()));
        };
        let reader = BufReader::new(file);

        let mut yaml_content = String::new();
        let mut lines = reader.lines();

        if let Some(line) = lines.next() {
            let line = line.map_err(|_| FileError::InvalidFile(file_path.into()))?;
            if !line.trim_start().starts_with("#!/SBUILD") {
                logger.warn("File doesn't start with '#!/SBUILD'");
            }
        } else {
            return Err(FileError::InvalidFile(file_path.into()));
        }

        for line in lines {
            let line = line.map_err(|_| FileError::InvalidFile(file_path.into()))?;
            yaml_content.push_str(&line);
            yaml_content.push('\n');
        }

        Ok(yaml_content)
    }

    fn run_shellcheck(&self, script: &str, severity: &str) -> std::io::Result<ExitStatus> {
        let tmp = temp_file(script);

        let out = Command::new("shellcheck")
            .arg(format!("--severity={}", severity))
            .arg(&tmp)
            .status();

        fs::remove_file(tmp).expect("Failed to delete temporary script file");
        out
    }

    fn shellcheck(&self, script: &str) -> std::io::Result<()> {
        if !self.run_shellcheck(script, "error")?.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Shellcheck emitted errors.",
            ));
        }

        let _ = self.run_shellcheck(script, "warning");

        Ok(())
    }

    pub fn generate_pkgver(&self, config: &BuildConfig, pkgver_path: &str) -> bool {
        let logger = &self.logger;
        let x_exec = &config.x_exec;
        let mut success = false;

        match config.pkgver {
            Some(ref pkgver) => {
                logger.info("Using hard-coded pkgver");
                let file = File::create(pkgver_path).unwrap();
                let mut writer = BufWriter::new(file);
                let _ = writer.write_all(pkgver.as_bytes());

                logger.success(&format!(
                    "Version ({}) from pkgver written to {}",
                    pkgver,
                    pkgver_path.bright_cyan()
                ));
                success = true;
            }
            None => {
                if let Some(ref pkgver) = x_exec.pkgver {
                    let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, pkgver);
                    let tmp = temp_file(&script);

                    let (tx, rx) = sync::mpsc::channel();
                    let tmp_clone = tmp.clone();
                    thread::spawn(move || {
                        let cmd = Command::new(&tmp_clone).output();
                        let _ = tx.send(cmd);
                    });

                    match rx.recv_timeout(self.timeout) {
                        Ok(cmd_result) => {
                            match cmd_result {
                                Ok(cmd) => {
                                    if cmd.status.success() {
                                        if !cmd.stderr.is_empty() {
                                            logger.error("x.exec.pkgver script produced error.");
                                            logger.error(&String::from_utf8_lossy(&cmd.stderr));
                                        } else {
                                            let out = cmd.stdout;
                                            let output_str = String::from_utf8_lossy(&out);
                                            let output_str = output_str.trim();
                                            if output_str.is_empty() {
                                                logger.warn("x_exec.pkgver produced empty result. Skipping...");
                                            } else if output_str.lines().count() > 1 {
                                                logger.error(
                                                    "x_exec.pkgver should only produce one output",
                                                );
                                                output_str.lines().for_each(|line| {
                                                    logger.info(&format!("-> {}", line.trim()));
                                                });
                                            } else {
                                                let file = File::create(pkgver_path).unwrap();
                                                let mut writer = BufWriter::new(file);
                                                let _ = writer.write_all(output_str.as_bytes());

                                                logger.success(&format!("Fetched version ({}) using x_exec.pkgver written to {}", &output_str, pkgver_path.bright_cyan()));
                                                success = true;
                                            }
                                        }
                                    } else {
                                        logger.error(&format!("{} -> Failed to read output from pkgver script. Please make sure the script is valid.", "x_exec.pkgver".bold()));
                                        if !cmd.stderr.is_empty() {
                                            logger.error(&String::from_utf8_lossy(&cmd.stderr));
                                        }
                                    }
                                }
                                Err(err) => {
                                    logger.error(&format!(
                                        "{} -> pkgver script failed to execute. {}",
                                        "x_exec.pkgver".bold(),
                                        err
                                    ));
                                }
                            }
                        }
                        Err(_) => {
                            logger.error(&format!(
                                "{} -> pkgver script timed out after 15 seconds",
                                "x_exec.pkgver".bold()
                            ));
                        }
                    }

                    fs::remove_file(tmp).expect("Failed to delete temporary script file");
                } else {
                    // we don't care if the pkgver is not set
                    success = true;
                }
            }
        }
        success
    }

    fn is_shellcheck_success(&self, config: &BuildConfig) -> bool {
        let logger = &self.logger;
        let x_exec = &config.x_exec;
        let mut success = true;

        let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, x_exec.run);
        if self.shellcheck(&script).is_err() {
            logger.error(&format!(
                "{} -> Shellcheck verification failed.",
                "x_exec.run".bold()
            ));
            success = false;
        };

        if let Some(ref pkgver) = x_exec.pkgver {
            let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, pkgver);
            if self.shellcheck(&script).is_err() {
                logger.error(&format!(
                    "{} -> Shellcheck verification failed.",
                    "x_exec.pkgver".bold()
                ));
                success = false;
            }
        }

        success
    }
}

enum FileError {
    InvalidFile(String),
    NotFound(String),
}

impl Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::InvalidFile(fp) => {
                writeln!(f, "Invalid file {}. Please provide a valid YAML file.", fp)
            }
            FileError::NotFound(fp) => writeln!(f, "File {} not found.", fp),
        }
    }
}

fn get_line_number_for_key(yaml_str: &str, key: &str) -> usize {
    let mut line_number = 0;
    for (index, line) in yaml_str.lines().enumerate() {
        if line.contains(key) {
            line_number = index + 1;
            break;
        }
    }
    line_number
}

fn get_pkg_id(src: &str) -> String {
    let (_, url) = src.split_once("://").unwrap();
    let (url, _) = url.split_once('?').unwrap_or((url, ""));
    url.replace('/', ".").trim_matches('.').to_string()
}

fn temp_file(script: &str) -> PathBuf {
    let tmp_dir = env::temp_dir();
    let tmp_file_path = tmp_dir.join(format!("sbuild-{}", nanoid!()));
    {
        let mut tmp_file =
            File::create(&tmp_file_path).expect("Failed to create temporary script file");
        tmp_file
            .write_all(script.as_bytes())
            .expect("Failed to write to temporary script file");

        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp_file_path)
            .expect("Failed to read file metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp_file_path, perms).expect("Failed to set executable permissions");
    }
    tmp_file_path
}
