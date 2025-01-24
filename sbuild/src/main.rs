use std::{
    env,
    process::Command,
    sync::{
        self,
        atomic::{AtomicUsize, Ordering},
        Arc, LazyLock,
    },
    thread,
    time::{Duration, Instant},
};

use colored::Colorize;
use sbuild::{builder::Builder, types::SoarEnv};
use sbuild_linter::logger::{LogManager, LogMessage};

static CHECK_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "✔".bright_green().bold());
static CROSS_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "〤".bright_red().bold());
static WARN: LazyLock<colored::ColoredString> = LazyLock::new(|| "⚠️".bright_yellow().bold());

fn usage() -> String {
    r#"Usage: sbuild [OPTIONS] [FILES]

A builder for SBUILD package files.

Options:
   --help, -h                   Show this help message
   --log-level                  Log level for build script: info/1 (default), verbose/2, debug/3
   --keep, -k                   Whether to keep sbuild temp directory
   --outdir, -o <PATH>          Directory to store the build files in
   --timeout-linter <DURATION>  Timeout duration after which the linter exists

Arguments:
   FILE...               One or more package files to build"#
        .to_string()
}

fn get_soar_env() -> SoarEnv {
    let cmd = Command::new("soar").arg("env").output();
    let mut soar_env = SoarEnv::default();

    if let Ok(cmd_output) = cmd {
        if cmd_output.status.success() {
            if cmd_output.stderr.is_empty() {
                let output_str = String::from_utf8_lossy(&cmd_output.stdout);
                for line in output_str.lines() {
                    if let Some(value) = line.strip_prefix("SOAR_CACHE=") {
                        soar_env.cache_path = value.to_string();
                    }
                    if let Some(value) = line.strip_prefix("SOAR_BIN=") {
                        soar_env.bin_path = value.to_string();
                    }
                }
                return soar_env;
            } else {
                eprintln!(
                    "Error: `soar env` produced errors: {}",
                    String::from_utf8_lossy(&cmd_output.stderr)
                );
            }
        } else {
            eprintln!("Error: `soar env` exited with a non-zero status.");
        }
    } else {
        eprintln!("Error: Failed to execute command `soar env`.");
    }
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let mut files = Vec::new();
    let mut outdir = None;
    let mut timeout = 120;
    let mut lint_timeout = 30;
    let mut keep_temp = false;

    let mut log_level = 1;

    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--keep" | "-k" => {
                keep_temp = true;
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return;
            }
            "--outdir" | "-o" => {
                if let Some(next) = iter.next() {
                    if next.starts_with("-") {
                        eprintln!("Expected dir path. Got flag instead.");
                        std::process::exit(1);
                    }
                    outdir = Some(next);
                } else {
                    eprintln!("outdir path is not provided.");
                    eprintln!("{}", usage());
                    std::process::exit(1);
                }
            }
            "--timeout" => {
                if let Some(next) = iter.next() {
                    match next.parse::<usize>() {
                        Ok(duration) => timeout = duration,
                        Err(_) => {
                            eprintln!("Invalid timeout duration: '{}'", next);
                            eprintln!("{}", usage());
                            std::process::exit(1);
                        }
                    };
                }
            }
            "--timeout-linter" => {
                if let Some(next) = iter.next() {
                    match next.parse::<usize>() {
                        Ok(duration) => lint_timeout = duration,
                        Err(_) => {
                            eprintln!("Invalid timeout duration: '{}'", next);
                            eprintln!("{}", usage());
                            std::process::exit(1);
                        }
                    };
                }
            }
            "--log-level" => {
                if let Some(next) = iter.next() {
                    log_level = match next.to_lowercase().trim() {
                        "info" | "1" => 1,
                        "verbose" | "2" => 2,
                        "debug" | "3" => 3,
                        other => {
                            eprintln!("Invalid log level: '{}'", other);
                            eprintln!("{}", usage());
                            std::process::exit(1);
                        }
                    }
                }
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

    println!("sbuild v{}", env!("CARGO_PKG_VERSION"));

    if which::which("soar").is_err() {
        eprintln!("soar is unavailable. Please install soar to continue.");
        std::process::exit(1);
    }

    let soar_env = get_soar_env();

    let now = Instant::now();
    let success = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));

    let (tx, rx) = sync::mpsc::channel();
    let log_manager = LogManager::new(tx.clone());

    let logger_handle = thread::spawn(move || {
        while let Ok(log) = rx.recv() {
            match log {
                LogMessage::Info(msg) => {
                    println!("{}", msg);
                }
                LogMessage::Error(msg) => {
                    eprintln!("[{}] {}", &*CROSS_MARK, msg);
                }
                LogMessage::Warn(msg) => {
                    eprintln!("[{}] {}", &*WARN, msg);
                }
                LogMessage::Success(msg) => {
                    println!("[{}] {}", &*CHECK_MARK, msg);
                }
                LogMessage::CustomError(msg) => {
                    eprintln!("{}", msg);
                }
                LogMessage::Done => break,
            }
        }
    });

    for file_path in &files {
        let named_temp_file = tempfile::Builder::new()
            .prefix("sbuild-")
            .rand_bytes(8)
            .tempfile()
            .expect("Failed to create temp file");
        let tmp_file_path = named_temp_file.path().to_path_buf();
        let logger = log_manager.create_logger(Some(tmp_file_path));

        let now = chrono::Utc::now();
        logger.write_to_file(format!(
            "sbuild v{} [{}]",
            env!("CARGO_PKG_VERSION"),
            now.format("%A, %B %d, %Y %H:%M:%S")
        ));

        let mut builder = Builder::new(
            logger.clone(),
            soar_env.clone(),
            true,
            log_level,
            keep_temp,
            Duration::from_secs(timeout as u64),
        );

        if builder
            .build(
                file_path,
                outdir.cloned(),
                Duration::from_secs(lint_timeout as u64),
            )
            .await
        {
            success.fetch_add(1, Ordering::SeqCst);
        } else {
            fail.fetch_add(1, Ordering::SeqCst);
        }
    }

    log_manager.done();
    logger_handle.join().unwrap();

    println!();
    println!(
        "[{}] {} files built successfully",
        "+".bright_blue().bold(),
        success.load(Ordering::SeqCst),
    );
    println!(
        "[{}] {} files failed to build",
        "+".bright_blue().bold(),
        fail.load(Ordering::SeqCst),
    );
    let total_evaluated = fail.load(Ordering::SeqCst) + success.load(Ordering::SeqCst);
    println!(
        "[{}] Processed {}/{} file(s) in {:#?}",
        "+".bright_blue().bold(),
        total_evaluated,
        files.len(),
        now.elapsed()
    );
}
