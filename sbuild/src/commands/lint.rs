use std::{
    collections::HashSet,
    env,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{
        self,
        atomic::{AtomicUsize, Ordering},
        Arc, LazyLock,
    },
    thread,
    time::{Duration, Instant},
};

use clap::Parser;
use colored::Colorize;
use sbuild_linter::{
    logger::{LogManager, LogMessage},
    semaphore::Semaphore,
    Linter,
};

static CHECK_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "✔".bright_green().bold());
static CROSS_MARK: LazyLock<colored::ColoredString> = LazyLock::new(|| "〤".bright_red().bold());
static WARN: LazyLock<colored::ColoredString> = LazyLock::new(|| "⚠️".bright_yellow().bold());

#[derive(Parser)]
#[command(about = "Linter for SBUILD package files")]
pub struct LintArgs {
    /// Files to lint
    #[arg(required = true)]
    files: Vec<String>,

    /// Enable pkgver mode
    #[arg(short = 'P', long)]
    pkgver: bool,

    /// Disable shellcheck
    #[arg(long)]
    no_shellcheck: bool,

    /// Run N jobs in parallel
    #[arg(short, long, default_value = "4")]
    parallel: usize,

    /// Replace the original file on success
    #[arg(short, long)]
    inplace: bool,

    /// File to store successful packages list
    #[arg(long)]
    success: Option<PathBuf>,

    /// File to store failed packages list
    #[arg(long)]
    fail: Option<PathBuf>,

    /// Timeout duration in seconds
    #[arg(long, default_value = "30")]
    timeout: u64,
}

pub fn run(args: LintArgs) {
    let files: HashSet<String> = args.files.iter().cloned().collect();

    if files.is_empty() {
        eprintln!("No files specified");
        std::process::exit(1);
    }

    if !args.no_shellcheck && which::which("shellcheck").is_err() {
        eprintln!("[{}] shellcheck not found. Please install.", &*CROSS_MARK);
        std::process::exit(1);
    }

    println!("sbuild lint v{}", env!("CARGO_PKG_VERSION"));

    let now = Instant::now();
    let success = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicUsize::new(0));

    let (tx, rx) = sync::mpsc::channel();
    let log_manager = LogManager::new(tx.clone());

    let fail_store = if let Some(ref fail_path) = args.fail {
        match OpenOptions::new().create(true).append(true).open(fail_path) {
            Ok(f) => Some(Arc::new(f)),
            Err(err) => {
                eprintln!("{}", err);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let success_store = if let Some(ref success_path) = args.success {
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(success_path)
        {
            Ok(f) => Some(Arc::new(f)),
            Err(err) => {
                eprintln!("{}", err);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let parallel = args.parallel;
    let logger_handle = thread::spawn(move || {
        let show_log = parallel == 1;
        while let Ok(log) = rx.recv() {
            match log {
                LogMessage::Info(msg) if show_log => println!("{}", msg),
                LogMessage::Error(msg) if show_log => eprintln!("[{}] {}", &*CROSS_MARK, msg),
                LogMessage::Warn(msg) if show_log => eprintln!("[{}] {}", &*WARN, msg),
                LogMessage::Success(msg) if show_log => println!("[{}] {}", &*CHECK_MARK, msg),
                LogMessage::CustomError(msg) if show_log => eprintln!("{}", msg),
                LogMessage::Done => break,
                _ => {}
            }
        }
    });

    let semaphore = Arc::new(Semaphore::new(args.parallel));
    let mut handles = Vec::new();

    for file_path in &files {
        let file_path = file_path.clone();
        let semaphore = Arc::clone(&semaphore);
        let success = Arc::clone(&success);
        let logger = log_manager.create_logger::<PathBuf>(None);
        let fail = Arc::clone(&fail);
        let success_store = success_store.clone();
        let fail_store = fail_store.clone();
        let inplace = args.inplace;
        let no_shellcheck = args.no_shellcheck;
        let pkgver = args.pkgver;
        let timeout = args.timeout;

        semaphore.acquire();
        let handle = thread::spawn(move || {
            let linter = Linter::new(logger, Duration::from_secs(timeout));
            if linter
                .lint(&file_path, inplace, no_shellcheck, pkgver)
                .is_some()
            {
                if let Some(mut success_store) = success_store {
                    let fp = format!("{}\n", file_path);
                    let _ = success_store.write_all(fp.as_bytes());
                }
                success.fetch_add(1, Ordering::SeqCst);
            } else {
                if let Some(mut fail_store) = fail_store {
                    let fp = format!("{}\n", file_path);
                    let _ = fail_store.write_all(fp.as_bytes());
                }
                fail.fetch_add(1, Ordering::SeqCst);
            }
            semaphore.release();
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    log_manager.done();
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
