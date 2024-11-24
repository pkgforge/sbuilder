use std::{
    collections::HashSet,
    env,
    fmt::Display,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::PathBuf,
    process::{Command, ExitStatus},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, LazyLock, Mutex,
    },
    thread,
    time::Instant,
};

use build_config::{visitor::BuildConfigVisitor, BuildConfig};
use colored::Colorize;
use comments::Comments;
use nanoid::nanoid;
use serde::{Deserialize, Deserializer};

mod build_config;
pub mod comments;
mod distro_pkg;
mod error;
mod log;
mod validator;
mod xexec;

#[derive(Debug, Deserialize, Clone)]
struct BuildAsset {
    url: String,
    out: String,
}

const VALID_PKG_TYPES: [&str; 9] = [
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
const VALID_CATEGORIES: &str = include_str!("categories");

const CHECK_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "✔".bright_green().bold());
const CROSS_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "〤".bright_red().bold());
const WARN: LazyLock<colored::ColoredString> = LazyLock::new(|| "⚠️".bright_yellow().bold());

pub struct CliConfig {
    pub parallel: Option<usize>,
}

pub static CONFIG: LazyLock<Mutex<CliConfig>> =
    LazyLock::new(|| Mutex::new(CliConfig { parallel: None }));

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

fn deserialize_yaml(yaml_str: &str) -> Result<BuildConfig, serde_yml::Error> {
    let deserializer = serde_yml::Deserializer::from_str(yaml_str);
    let visitor = BuildConfigVisitor {
        sbuild_str: yaml_str.to_string(),
        visited: HashSet::new(),
        errors: Vec::new(),
    };
    deserializer.deserialize_map(visitor)
}

enum FileError {
    InvalidFile(String),
    NotFound(String),
}

impl Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::InvalidFile(fp) => writeln!(
                f,
                "[{}] Invalid file {}. Please provide a valid YAML file.",
                &*CROSS_MARK, fp
            ),
            FileError::NotFound(fp) => writeln!(f, "[{}] File {} not found.", &*CROSS_MARK, fp),
        }
    }
}

fn read_yaml(file_path: &str) -> Result<String, FileError> {
    let Ok(file) = File::open(file_path) else {
        return Err(FileError::NotFound(file_path.into()));
    };
    let reader = BufReader::new(file);

    let mut yaml_content = String::new();
    let mut lines = reader.lines();

    if let Some(line) = lines.next() {
        let line = line.map_err(|_| FileError::InvalidFile(file_path.into()))?;
        if !line.trim_start().starts_with("#!/SBUILD") {
            info!("[{}] File doesn't start with '#!/SBUILD'", &*WARN);
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

fn run_shellcheck(script: &str, severity: &str) -> std::io::Result<ExitStatus> {
    let tmp = temp_file(&script);

    let out = Command::new("shellcheck")
        .arg(format!("--severity={}", severity))
        .arg(&tmp)
        .status();

    fs::remove_file(tmp).expect("Failed to delete temporary script file");
    out
}

fn shellcheck(script: &str) -> std::io::Result<()> {
    if !run_shellcheck(script, "error")?.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Shellcheck emitted errors.",
        ));
    }

    let _ = run_shellcheck(script, "warning");

    Ok(())
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

fn is_pkgver_success(config: &BuildConfig, pkgver_path: &str) -> bool {
    let x_exec = &config.x_exec;
    let mut success = true;

    match config.pkgver {
        Some(ref pkgver) => {
            info!("[{}] Using hard-coded pkgver", "+".bright_blue().bold());
            let file = File::create(pkgver_path).unwrap();
            let mut writer = BufWriter::new(file);
            let _ = writer.write_all(&pkgver.as_bytes());

            info!(
                "[{}] Version ({}) from pkgver written to {}",
                &*CHECK_MARK,
                pkgver,
                pkgver_path.bright_cyan()
            );
        }
        None => {
            if let Some(ref pkgver) = x_exec.pkgver {
                let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, pkgver);
                let tmp = temp_file(&script);
                let cmd = Command::new(&tmp).output();
                fs::remove_file(tmp).expect("Failed to delete temporary script file");
                if let Ok(cmd) = cmd {
                    if cmd.status.success() {
                        if !cmd.stderr.is_empty() {
                            einfo!("[{}] x.exec.pkgver script produced error.", &*CROSS_MARK);
                            einfo!("{}", String::from_utf8_lossy(&cmd.stderr));
                            success = false;
                        } else {
                            let out = cmd.stdout;
                            let output_str = String::from_utf8_lossy(&out);
                            let output_str = output_str.trim();
                            if output_str.is_empty() {
                                einfo!(
                                    "[{}] x_exec.pkgver produced empty result. Skipping...",
                                    &*WARN
                                );
                            } else {
                                if output_str.lines().count() > 1 {
                                    einfo!(
                                        "[{}] x_exec.pkgver should only produce one output",
                                        &*CROSS_MARK
                                    );
                                    output_str.lines().for_each(|line| {
                                        info!("-> {}", line.trim());
                                    });
                                    success = false;
                                } else {
                                    let file = File::create(pkgver_path).unwrap();
                                    let mut writer = BufWriter::new(file);
                                    let _ = writer.write_all(&output_str.as_bytes());

                                    info!(
                                        "[{}] Fetched version ({}) using x_exec.pkgver written to {}",
                                        &*CHECK_MARK,
                                        &output_str,
                                        pkgver_path.bright_cyan()
                                    );
                                }
                            }
                        }
                    } else {
                        einfo!(
                            "[{}] {} -> Failed to read output from pkgver script. Please make sure the script is valid.",
                            &*CROSS_MARK,
                            "x_exec.pkgver".bold()
                        );
                        success = false;
                        if !cmd.stderr.is_empty() {
                            einfo!("{}", String::from_utf8_lossy(&cmd.stderr));
                        }
                    }
                } else {
                    einfo!(
                        "[{}] {} -> pkgver script failed to execute.",
                        &*CROSS_MARK,
                        "x_exec.pkgver".bold()
                    );
                    success = false;
                }
            }
        }
    }
    success
}

fn is_shellcheck_success(config: &BuildConfig) -> bool {
    let x_exec = &config.x_exec;
    let mut success = true;

    let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, x_exec.run);
    if shellcheck(&script).is_err() {
        einfo!(
            "[{}] {} -> Shellcheck verification failed.",
            &*CROSS_MARK,
            "x_exec.run".bold()
        );
        success = false;
    };

    if let Some(ref pkgver) = x_exec.pkgver {
        let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, pkgver);
        if shellcheck(&script).is_err() {
            einfo!(
                "[{}] {} -> Shellcheck verification failed.",
                &*CROSS_MARK,
                "x_exec.pkgver".bold()
            );
            success = false;
        }
    }

    success
}

fn print_build_docs() {
    info!(
        "[{}] Build Docs: https://github.com/pkgforge/soarpkgs/blob/main/SBUILD.md",
        "-".bright_blue().bold()
    );
    info!(
        "[{}] Spec Docs: https://github.com/pkgforge/soarpkgs/blob/main/SBUILD_SPEC.md",
        "-".bright_blue().bold()
    );
}

fn usage() -> String {
    r#"Usage: sbuild-linter [OPTIONS] [FILES]

A linter for SBUILD package files.

Options:
   --pkgver, -p          Enable pkgver mode
   --no-shellcheck       Disable shellcheck
   --parallel <N>        Run N jobs in parallel (default: 4)
   --help, -h            Show this help message

Arguments:
   FILE...               One or more package files to validate"#
        .to_string()
}

fn lint(file_path: &str, disable_shellcheck: bool, pkgver: bool) -> bool {
    let yaml_str = match read_yaml(file_path) {
        Ok(y) => y,
        Err(err) => {
            eprintln!("{}", err);
            return false;
        }
    };

    println!("\n[{}] Linting {}", "-".bright_blue().bold(), file_path);
    match deserialize_yaml(&yaml_str) {
        Ok(config) => {
            if disable_shellcheck {
                info!("[{}] Skipping shellcheck", "-".bright_blue().bold())
            } else {
                info!("[{}] Performing shellcheck", "-".bright_blue().bold());
                if !is_shellcheck_success(&config) {
                    return false;
                }
                info!("[{}] Shellcheck passed", &*CHECK_MARK);
            }
            if let Some(pkgver_path) = pkgver.then(|| format!("{}.pkgver", file_path)) {
                if !is_pkgver_success(&config, &pkgver_path) {
                    return false;
                }
            };

            let output_path = format!("{}.validated", file_path);
            let file = File::create(&output_path).unwrap();
            let mut writer = BufWriter::new(file);

            let mut comments = Comments::new();
            comments.parse_comments(file_path).unwrap();
            config.write_yaml(&mut writer, 0, comments).unwrap();
            info!("[{}] SBUILD validation successful.", &*CHECK_MARK);
            info!(
                "[{}] Validated YAML has been written to {}",
                &*CHECK_MARK, output_path
            );
            return true;
        }
        Err(e) => {
            einfo!("{}", e.to_string());
            einfo!("[{}] SBUILD validation faild.", &*CROSS_MARK);
            print_build_docs();
        }
    };
    false
}

pub struct Semaphore {
    permits: Mutex<usize>,
    condvar: Condvar,
}

impl Semaphore {
    pub fn new(count: usize) -> Self {
        Semaphore {
            permits: Mutex::new(count),
            condvar: Condvar::new(),
        }
    }

    pub fn acquire(&self) {
        let mut permits = self.permits.lock().unwrap();
        while *permits == 0 {
            permits = self.condvar.wait(permits).unwrap();
        }
        *permits -= 1;
    }

    pub fn release(&self) {
        let mut permits = self.permits.lock().unwrap();
        *permits += 1;
        self.condvar.notify_one();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut pkgver = false;
    let mut disable_shellcheck = false;
    let mut files: Vec<String> = Vec::new();

    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--pkgver" | "-p" => {
                pkgver = true;
            }
            "--no-shellcheck" => {
                disable_shellcheck = true;
            }
            "--parallel" => {
                if let Some(next) = iter.next() {
                    match next.parse::<usize>() {
                        Ok(count) => CONFIG.lock().unwrap().parallel = Some(count),
                        Err(_) => {
                            eprintln!("Invalid number of parallel jobs: '{}'", next);
                            eprintln!("{}", usage());
                            std::process::exit(1);
                        }
                    }
                } else {
                    eprintln!("Number of parallel jobs not provided. Setting 4.");
                    CONFIG.lock().unwrap().parallel = Some(4);
                }
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return;
            }
            arg => {
                if arg.starts_with("--") {
                    eprintln!("Unknown argument '{}'", arg);
                    eprintln!("{}", usage());
                    std::process::exit(1);
                } else {
                    files.push(arg.to_string());
                }
            }
        }
    }

    if files.is_empty() {
        eprintln!("{}", usage());
        std::process::exit(1);
    }

    if !disable_shellcheck {
        if which::which("shellcheck").is_err() {
            eprintln!("[{}] shellcheck not found. Please install.", &*CROSS_MARK);
            std::process::exit(1);
        }
    }

    println!("sbuild-linter v{}", env!("CARGO_PKG_VERSION"));

    let now = Instant::now();
    let success = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));

    let parallel = CONFIG.lock().unwrap().parallel;
    if let Some(par) = parallel {
        let semaphore = Arc::new(Semaphore::new(par));
        let mut handles = Vec::new();
        let files = files.clone();

        for file_path in files {
            let semaphore = Arc::clone(&semaphore);
            let success = Arc::clone(&success);
            let fail = Arc::clone(&fail);

            semaphore.acquire();
            let handle = thread::spawn(move || {
                if lint(&file_path, disable_shellcheck, pkgver) {
                    success.fetch_add(1, Ordering::SeqCst);
                } else {
                    fail.fetch_add(1, Ordering::SeqCst);
                }

                semaphore.release();
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    } else {
        for file_path in &files {
            if lint(file_path, disable_shellcheck, pkgver) {
                success.fetch_add(1, Ordering::SeqCst);
            } else {
                fail.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    println!();
    println!(
        "[{}] {} files validated successfully",
        "+".bright_blue().bold(),
        success.load(Ordering::SeqCst),
    );
    println!(
        "[{}] {} files failed to pass validation",
        "+".bright_blue().bold(),
        fail.load(Ordering::SeqCst),
    );
    let total_evaluated = fail.load(Ordering::SeqCst) + success.load(Ordering::SeqCst);
    println!(
        "[{}] Evaluated {}/{} file(s) in {:#?}",
        "+".bright_blue().bold(),
        total_evaluated,
        files.len(),
        now.elapsed()
    );
}
