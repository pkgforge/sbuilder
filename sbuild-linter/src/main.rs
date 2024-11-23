use std::{
    collections::HashSet,
    env,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    process::{Command, ExitStatus, Stdio},
    sync::LazyLock,
};

use build_config::{visitor::BuildConfigVisitor, BuildConfig};
use colored::Colorize;
use serde::{Deserialize, Deserializer};
use xexec::XExec;

mod build_config;
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

fn read_yaml_with_header(
    file_path: &str,
) -> Result<(Vec<String>, String), Box<dyn std::error::Error>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut header_lines = Vec::new();
    let mut yaml_content = String::new();
    let mut lines = reader.lines();

    if let Some(line) = lines.next() {
        let mut line = line?;
        if !line.trim_start().starts_with("#!/SBUILD") {
            println!("[{}] File doesn't start with '#!/SBUILD'", &*WARN);
            if line.starts_with("#") {
                header_lines.push(line);
            }
            line = "#!/SBUILD".to_string();
        }
        header_lines.push(line);
    } else {
        return Err("Invalid file".into());
    }

    if let Some(line) = lines.next() {
        let line = line?;
        if line.trim_start().starts_with('#') {
            header_lines.push(line);
        } else {
            yaml_content.push_str(&line);
            yaml_content.push('\n');
        }
    }
    header_lines.push("\n".into());

    for line in lines {
        let line = line?;
        yaml_content.push_str(&line);
        yaml_content.push('\n');
    }

    Ok((header_lines, yaml_content))
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

fn is_shellcheck_success(x_exec: &XExec) -> bool {
    let mut success = true;
    if let Some(ref pkgver) = x_exec.pkgver {
        let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, pkgver);
        if shellcheck(&script).is_err() {
            eprintln!(
                "[{}] {} -> Shellcheck verification failed.",
                &*CROSS_MARK,
                "x_exec.pkgver".bold()
            );
            success = false;
        };
    }

    let script = format!("#!/usr/bin/env {}\n{}", x_exec.shell, x_exec.run);
    if shellcheck(&script).is_err() {
        eprintln!(
            "[{}] {} -> Shellcheck verification failed.",
            &*CROSS_MARK,
            "x_exec.shell".bold()
        );
        success = false;
    };
    success
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <file-path>", args[0]);
        std::process::exit(1);
    }

    let file_path = &args[1];
    let (headers, yaml_str) = read_yaml_with_header(&file_path).expect("Invalid file.");

    println!(
        "[{}] Deserializing and validating SBUILD...",
        "-".bright_blue().bold()
    );
    match deserialize_yaml(&yaml_str) {
        Ok(config) => {
            println!("[{}] SBUILD validation successful.", &*CHECK_MARK);
            if !config.x_exec.disable_shellcheck.map_or(false, |v| v) {
                println!("[{}] Performing shellcheck", "-".bright_blue().bold());
                if !is_shellcheck_success(&config.x_exec) {
                    std::process::exit(1);
                }
            }
            let output_path = format!("{}.validated", file_path);
            let file = File::create(&output_path).unwrap();
            let mut writer = BufWriter::new(file);

            for line in headers {
                writeln!(writer, "{}", line).unwrap();
            }
            config.write_yaml(&mut writer, 0).unwrap();
            println!(
                "[{}] Validated YAML has been written to {}",
                &*CHECK_MARK, output_path
            );
        }
        Err(e) => {
            eprintln!("{}", e.to_string());
            eprintln!("[{}] SBUILD validation faild.", &*CROSS_MARK);
        }
    };
}
