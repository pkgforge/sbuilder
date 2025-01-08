use std::{
    env,
    process::Command,
    sync::{
        self,
        atomic::{AtomicUsize, Ordering},
        Arc, LazyLock,
    },
    thread,
    time::Instant,
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
   --help, -h            Show this help message

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
                    "Error: Command produced errors: {}",
                    String::from_utf8_lossy(&cmd_output.stderr)
                );
            }
        } else {
            eprintln!("Error: Command exited with a non-zero status.");
        }
    } else {
        eprintln!("Error: Failed to execute command.");
    }
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let mut files = Vec::new();

    let iter = args.iter().skip(1);
    for arg in iter {
        match arg.as_str() {
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
        let mut builder = Builder::new(logger.clone(), soar_env.clone(), true);
        if builder.build(file_path).await {
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
