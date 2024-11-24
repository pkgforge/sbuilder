use std::{
    collections::HashSet,
    env,
    fmt::Display,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    process::{Command, ExitStatus, Stdio},
    sync::LazyLock,
    time::Instant,
};

use build_config::{visitor::BuildConfigVisitor, BuildConfig};
use colored::Colorize;
use comments::Comments;
use serde::{Deserialize, Deserializer};

mod build_config;
pub mod comments;
mod distro_pkg;
mod error;
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
            println!("[{}] File doesn't start with '#!/SBUILD'", &*WARN);
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
    Command::new("shellcheck")
        .arg(format!("--severity={}", severity))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .and_then(|mut child| {
            child.stdin.as_mut().unwrap().write_all(script.as_bytes())?;
            child.wait()
        })
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

fn is_pkgver_success(config: &BuildConfig, pkgver_path: &str) -> bool {
    let x_exec = &config.x_exec;
    let mut success = true;

    match config.pkgver {
        Some(ref pkgver) => {
            println!("[{}] Using hard-coded pkgver", "+".bright_blue().bold());
            let file = File::create(pkgver_path).unwrap();
            let mut writer = BufWriter::new(file);
            let _ = writer.write_all(&pkgver.as_bytes());

            println!(
                "[{}] Version ({}) from pkgver written to {}",
                &*CHECK_MARK,
                pkgver,
                pkgver_path.bright_cyan()
            );
        }
        None => {
            if let Some(ref pkgver) = x_exec.pkgver {
                let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, pkgver);
                let cmd = Command::new("sh").args(["-c", &script]).output();
                if let Ok(cmd) = cmd {
                    if cmd.status.success() {
                        if !cmd.stderr.is_empty() {
                            eprintln!("[{}] x.exec.pkgver script produced error.", &*CROSS_MARK);
                            eprintln!("{}", String::from_utf8_lossy(&cmd.stderr));
                            success = false;
                        } else {
                            let out = cmd.stdout;
                            let output_str = String::from_utf8_lossy(&out);
                            let output_str = output_str.trim();
                            if output_str.is_empty() {
                                eprintln!(
                                    "[{}] x_exec.pkgver produced empty result. Skipping...",
                                    &*WARN
                                );
                            } else {
                                if output_str.lines().count() > 1 {
                                    eprintln!(
                                        "[{}] x_exec.pkgver should only produce one output",
                                        &*CROSS_MARK
                                    );
                                    output_str.lines().for_each(|line| {
                                        println!("-> {}", line.trim());
                                    });
                                    success = false;
                                } else {
                                    let file = File::create(pkgver_path).unwrap();
                                    let mut writer = BufWriter::new(file);
                                    let _ = writer.write_all(&output_str.as_bytes());

                                    println!(
                                        "[{}] Fetched version ({}) using x_exec.pkgver written to {}",
                                        &*CHECK_MARK,
                                        &output_str,
                                        pkgver_path.bright_cyan()
                                    );
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[{}] {} -> Failed to read output from pkgver script. Please make sure the script is valid.",
                            &*CROSS_MARK,
                            "x_exec.pkgver".bold()
                        );
                        success = false;
                        if !cmd.stderr.is_empty() {
                            eprintln!("{}", String::from_utf8_lossy(&cmd.stderr));
                        }
                    }
                } else {
                    eprintln!(
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
        eprintln!(
            "[{}] {} -> Shellcheck verification failed.",
            &*CROSS_MARK,
            "x_exec.run".bold()
        );
        success = false;
    };

    if let Some(ref pkgver) = x_exec.pkgver {
        let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, pkgver);
        if shellcheck(&script).is_err() {
            eprintln!(
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
    println!(
        "[{}] Build Docs: https://github.com/pkgforge/soarpkgs/blob/main/SBUILD.md",
        "-".bright_blue().bold()
    );
    println!(
        "[{}] Spec Docs: https://github.com/pkgforge/soarpkgs/blob/main/SBUILD_SPEC.md",
        "-".bright_blue().bold()
    );
}

fn usage() -> String {
    format!(
        "Usage: sbuild-linter [OPTIONS] [FILES]\n\n\
         Options:\n\
         --pkgver              Enable pkgver mode\n\
         --no-shellcheck       Disable shellcheck\n\
         --help, -h            Show this help message\n\n\
         Files:\n\
         Specify one or more files to process."
    )
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut pkgver = false;
    let mut disable_shellcheck = false;
    let mut files: Vec<String> = Vec::new();

    for arg in args.iter().skip(1).into_iter() {
        match arg.as_str() {
            "--pkgver" | "-p" => {
                pkgver = true;
            }
            "--no-shellcheck" => {
                disable_shellcheck = true;
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
    for file_path in &files {
        let yaml_str = match read_yaml(file_path) {
            Ok(y) => y,
            Err(err) => {
                eprintln!("{}", err);
                continue;
            }
        };

        println!("\n[{}] Linting {}", "-".bright_blue().bold(), file_path);
        match deserialize_yaml(&yaml_str) {
            Ok(config) => {
                if disable_shellcheck {
                    println!("[{}] Skipping shellcheck", "-".bright_blue().bold())
                } else {
                    println!("[{}] Performing shellcheck", "-".bright_blue().bold());
                    if !is_shellcheck_success(&config) {
                        continue;
                    }
                    println!("[{}] Shellcheck passed", &*CHECK_MARK);
                }
                if let Some(pkgver_path) = pkgver.then(|| format!("{}.pkgver", file_path)) {
                    if !is_pkgver_success(&config, &pkgver_path) {
                        continue;
                    }
                };

                let output_path = format!("{}.validated", file_path);
                let file = File::create(&output_path).unwrap();
                let mut writer = BufWriter::new(file);

                let mut comments = Comments::new();
                comments.parse_comments(file_path).unwrap();
                config.write_yaml(&mut writer, 0, comments).unwrap();
                println!("[{}] SBUILD validation successful.", &*CHECK_MARK);
                println!(
                    "[{}] Validated YAML has been written to {}",
                    &*CHECK_MARK, output_path
                );
            }
            Err(e) => {
                eprintln!("{}", e.to_string());
                eprintln!("[{}] SBUILD validation faild.", &*CROSS_MARK);
                print_build_docs();
            }
        };
    }
    println!(
        "\n[{}] Evaluated {} file(s) in {:#?}",
        "+".bright_blue().bold(),
        files.len(),
        now.elapsed()
    );
}
