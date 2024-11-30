use std::{
    env,
    sync::{
        self,
        atomic::{AtomicUsize, Ordering},
        Arc, LazyLock,
    },
    thread,
    time::Instant,
};

use colored::Colorize;
use sbuild_linter::{
    logger::{LogMessage, Logger},
    semaphore::Semaphore,
    Linter,
};

static CHECK_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "✔".bright_green().bold());
static CROSS_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "〤".bright_red().bold());
static WARN: LazyLock<colored::ColoredString> = LazyLock::new(|| "⚠️".bright_yellow().bold());

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

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut pkgver = false;
    let mut disable_shellcheck = false;
    let mut files: Vec<String> = Vec::new();
    let mut parallel = None;

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
                        Ok(count) => parallel = Some(count),
                        Err(_) => {
                            eprintln!("Invalid number of parallel jobs: '{}'", next);
                            eprintln!("{}", usage());
                            std::process::exit(1);
                        }
                    }
                } else {
                    eprintln!("Number of parallel jobs not provided. Setting 4.");
                    parallel = Some(4);
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

    if !disable_shellcheck && which::which("shellcheck").is_err() {
        eprintln!("[{}] shellcheck not found. Please install.", &*CROSS_MARK);
        std::process::exit(1);
    }

    println!("sbuild-linter v{}", env!("CARGO_PKG_VERSION"));

    let now = Instant::now();
    let success = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));

    let (tx, rx) = sync::mpsc::channel();
    let logger = Logger::new(tx.clone());

    let logger_handle = thread::spawn(move || {
        let show_log = parallel.is_none();
        while let Ok(log) = rx.recv() {
            match log {
                LogMessage::Info(msg) if show_log => {
                    println!("{}", msg);
                }
                LogMessage::Error(msg) if show_log => {
                    eprintln!("[{}] {}", &*CROSS_MARK, msg);
                }
                LogMessage::Warn(msg) if show_log => {
                    eprintln!("[{}] {}", &*WARN, msg);
                }
                LogMessage::Success(msg) if show_log => {
                    println!("[{}] {}", &*CHECK_MARK, msg);
                }
                LogMessage::CustomError(msg) if show_log => {
                    eprintln!("{}", msg);
                }
                LogMessage::Done => break,
                _ => {}
            }
        }
    });

    if let Some(par) = parallel {
        let semaphore = Arc::new(Semaphore::new(par));
        let mut handles = Vec::new();
        let files = files.clone();

        for file_path in files {
            let semaphore = Arc::clone(&semaphore);
            let success = Arc::clone(&success);
            let logger = logger.clone();
            let fail = Arc::clone(&fail);

            semaphore.acquire();
            let handle = thread::spawn(move || {
                let linter = Linter::new(logger);
                if linter.lint(&file_path, disable_shellcheck, pkgver) {
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
            let linter = Linter::new(logger.clone());
            if linter.lint(file_path, disable_shellcheck, pkgver) {
                success.fetch_add(1, Ordering::SeqCst);
            } else {
                fail.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    logger_handle.join().unwrap();

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
