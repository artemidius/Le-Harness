#![allow(clippy::print_stdout)]

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "le-harness", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print build and runtime information.
    Doctor,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Command::Doctor) => println!("Le Harness {}", env!("CARGO_PKG_VERSION")),
        None => println!("Run `le-harness --help` to get started."),
    }
}
