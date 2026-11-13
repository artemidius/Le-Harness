#![allow(clippy::print_stdout)]

use clap::{Parser, Subcommand};
use le_harness::model::{MistralAdapter, ModelAdapter};
use le_harness::settings::MistralSettings;

#[derive(Debug, Parser)]
#[command(name = "le-harness", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print build and runtime information.
    Doctor {
        /// Query Mistral and verify the configured model identifiers.
        #[arg(long)]
        live: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Command::Doctor { live }) => doctor(live).await?,
        None => println!("Run `le-harness --help` to get started."),
    }
    Ok(())
}

async fn doctor(live: bool) -> Result<(), Box<dyn std::error::Error>> {
    let diagnostic = MistralSettings::diagnostic();
    println!("Le Harness {}", env!("CARGO_PKG_VERSION"));
    println!(
        "MISTRAL_API_KEY: {}",
        if diagnostic.api_key { "set" } else { "missing" }
    );
    println!(
        "chat model: {}",
        diagnostic.chat_model.as_deref().unwrap_or("missing")
    );
    println!(
        "FIM model: {}",
        diagnostic.fim_model.as_deref().unwrap_or("missing")
    );
    let docker = std::process::Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output();
    match docker {
        Ok(output) if output.status.success() => println!(
            "Docker isolation: available (server {})",
            String::from_utf8_lossy(&output.stdout).trim()
        ),
        _ => println!("Docker isolation: unavailable (native execution is not a sandbox)"),
    }
    if live {
        let settings = MistralSettings::from_env()?;
        let adapter = MistralAdapter::from_settings(&settings)?;
        let models = adapter.list_models().await?;
        for (role, model) in [("chat", settings.chat_model), ("FIM", settings.fim_model)] {
            println!(
                "{role} model {model}: {}",
                if models.contains(&model) {
                    "available"
                } else {
                    "not returned by model discovery"
                }
            );
        }
    }
    Ok(())
}
