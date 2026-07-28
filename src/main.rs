mod commands;

use clap::{Parser, Subcommand};
use colored::Colorize;

#[derive(Parser)]
#[command(name = "sbuild")]
#[command(about = "Tooling for the soarpkgs package format", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(flatten)]
    Port(commands::port::PortCommands),
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Port(args) => commands::port::run(args).await,
    };

    if let Err(e) = result {
        eprintln!("{}: {}", "Error".bright_red(), e);
        std::process::exit(1);
    }
}
