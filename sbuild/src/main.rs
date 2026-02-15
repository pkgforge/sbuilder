mod commands;

use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use sbuild::types::SoarEnv;
use tracing::Level;

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum LogLevel {
    #[default]
    Info,
    Verbose,
    Debug,
}

impl From<LogLevel> for u8 {
    fn from(level: LogLevel) -> u8 {
        match level {
            LogLevel::Info => 1,
            LogLevel::Verbose => 2,
            LogLevel::Debug => 3,
        }
    }
}

impl From<LogLevel> for Level {
    fn from(level: LogLevel) -> Level {
        match level {
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Verbose => Level::DEBUG,
            LogLevel::Info => Level::INFO,
        }
    }
}

#[derive(Parser)]
#[command(name = "sbuild")]
#[command(about = "Toolchain for building, linting, and managing SBUILD packages", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Build(commands::build::BuildArgs),
    Info(commands::info::InfoArgs),
    Cache(commands::cache::CacheArgs),
    Lint(commands::lint::LintArgs),
    Meta(commands::meta::MetaArgs),
}

fn get_soar_env() -> Option<SoarEnv> {
    use std::process::Command;

    let cmd = Command::new("soar").arg("env").output();
    let mut soar_env = SoarEnv::default();

    if let Ok(cmd_output) = cmd {
        if cmd_output.status.success() && cmd_output.stderr.is_empty() {
            let output_str = String::from_utf8_lossy(&cmd_output.stdout);
            for line in output_str.lines() {
                if let Some(value) = line.strip_prefix("SOAR_CACHE=") {
                    soar_env.cache_path = value.to_string();
                }
                if let Some(value) = line.strip_prefix("SOAR_BIN=") {
                    soar_env.bin_path = value.to_string();
                }
            }
            return Some(soar_env);
        }
    }
    None
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Build(args) => commands::build::run(args, get_soar_env()).await,
        Commands::Info(args) => commands::info::run(args).await,
        Commands::Cache(args) => {
            if let Err(e) = commands::cache::run(args) {
                eprintln!("{}: {}", "Error".bright_red(), e);
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::Lint(args) => {
            commands::lint::run(args);
            Ok(())
        }
        Commands::Meta(args) => commands::meta::run(args).await.map_err(|e| e.to_string()),
    };

    if let Err(e) = result {
        eprintln!("{}: {}", "Error".bright_red(), e);
        std::process::exit(1);
    }
}