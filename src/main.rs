#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use clap::{Parser, Subcommand, ValueEnum};
use le_harness::edit::WorkspaceEditor;
use le_harness::execution::{DockerExecution, ExecutionBackend, ExecutionLimits, NativeExecution};
use le_harness::fim::FimEditEngine;
use le_harness::model::{MistralAdapter, ModelAdapter};
use le_harness::policy::{
    ApprovalBinding, ApprovalRequest, ApprovalStore, Approver, Effect,
    InteractivePolicyToolHandler, PermissionMode, PermissionPolicy, ToolEffects,
};
use le_harness::repository::{Repository, RepositoryToolHandler, RepositoryToolLimits};
use le_harness::runtime::{AgentController, AgentLimits};
use le_harness::settings::MistralSettings;
use le_harness::tools::CodingTools;
use serde::Serialize;

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
    /// Run one bounded coding-agent session.
    Run {
        /// Task for the agent.
        prompt: String,
        /// Repository root.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Permission policy.
        #[arg(long, value_enum, default_value = "assisted")]
        mode: CliMode,
        /// Command execution backend.
        #[arg(long, value_enum, default_value = "native")]
        execution: CliExecution,
        /// Docker image used by the Docker backend.
        #[arg(long, default_value = "rust:1.88-slim")]
        image: String,
        /// Emit one JSON result instead of human-readable output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliMode {
    Inspect,
    Assisted,
    AutomatedSandbox,
}

impl From<CliMode> for PermissionMode {
    fn from(value: CliMode) -> Self {
        match value {
            CliMode::Inspect => Self::Inspect,
            CliMode::Assisted => Self::Assisted,
            CliMode::AutomatedSandbox => Self::AutomatedSandbox,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliExecution {
    Native,
    Docker,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command {
        Some(Command::Doctor { live }) => doctor(live).await?,
        Some(Command::Run {
            prompt,
            root,
            mode,
            execution,
            image,
            json,
        }) => run(prompt, root, mode, execution, image, json).await?,
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

async fn run(
    prompt: String,
    root: PathBuf,
    mode: CliMode,
    execution: CliExecution,
    image: String,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(mode, CliMode::AutomatedSandbox) && matches!(execution, CliExecution::Native) {
        return Err("automated-sandbox mode requires --execution docker".into());
    }
    let root = root.canonicalize()?;
    let before = git_evidence(&root);
    let repository = Repository::open(&root)?;
    let repository_tools = RepositoryToolHandler::new(repository, RepositoryToolLimits::default());
    let editor = WorkspaceEditor::open(&root)?;
    let limits = ExecutionLimits::default();
    let backend: Arc<dyn ExecutionBackend> = match execution {
        CliExecution::Native => {
            if !json {
                eprintln!(
                    "Note: native command execution is bounded but is not a security sandbox."
                );
            }
            Arc::new(NativeExecution::new(&root, limits)?)
        }
        CliExecution::Docker => Arc::new(DockerExecution::new(&root, image, limits)?),
    };
    let settings = MistralSettings::from_env()?;
    let model = MistralAdapter::from_settings(&settings)?;
    let fim = Arc::new(FimEditEngine::new(
        model.clone(),
        editor.clone(),
        512 * 1024,
    ));
    let tools = CodingTools::new(repository_tools, editor, backend).with_fim(fim);
    let effects = ToolEffects::default()
        .register("list_files", Effect::Read)
        .register("read_file", Effect::Read)
        .register("search", Effect::Read)
        .register("replace_span", Effect::Write)
        .register("fim_replace_span", Effect::Write)
        .register("run_command", Effect::Execute);
    let approvals = ApprovalStore::default();
    let policy = PermissionPolicy::new(mode.into(), approvals.clone());
    let tools = InteractivePolicyToolHandler::new(
        tools,
        effects,
        policy,
        approvals,
        ConsoleApprover { enabled: !json },
    );
    let controller = AgentController::new(model, tools, AgentLimits::default());
    let cancellation = controller.cancellation_token();
    let operation = controller.run(prompt);
    tokio::pin!(operation);
    let result = tokio::select! {
        result = &mut operation => result,
        signal = tokio::signal::ctrl_c() => {
            signal?;
            cancellation.cancel();
            operation.await
        }
    };
    let after = git_evidence(&root);
    let output = RunOutput {
        status: result.status,
        answer: result.output,
        model_requests: result.model_requests,
        tool_calls: result.tool_calls,
        events: result.events,
        git: GitTransition { before, after },
    };
    if json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("{}", output.answer);
        println!(
            "\nstatus: {} | model requests: {} | tool calls: {}",
            serde_json::to_value(&output.status)?["state"]
                .as_str()
                .unwrap_or("unknown"),
            output.model_requests,
            output.tool_calls
        );
        if output.git.before.status != output.git.after.status {
            println!("repository changes:\n{}", output.git.after.status);
        }
    }
    Ok(())
}

struct ConsoleApprover {
    enabled: bool,
}

#[async_trait]
impl Approver for ConsoleApprover {
    async fn approve(&self, request: &ApprovalRequest) -> Option<ApprovalBinding> {
        if !self.enabled {
            return None;
        }
        let request = request.clone();
        tokio::task::spawn_blocking(move || {
            eprintln!(
                "Approve {:?} tool {} with arguments {}? [y/N]",
                request.effect, request.tool, request.arguments
            );
            eprint!("> ");
            io::stderr().flush().ok()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer).ok()?;
            matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
                .then(|| request.binding.clone())
        })
        .await
        .ok()
        .flatten()
    }
}

#[derive(Serialize)]
struct RunOutput {
    status: le_harness::runtime::AgentStatus,
    answer: String,
    model_requests: u32,
    tool_calls: u32,
    events: Vec<le_harness::event::RuntimeEvent>,
    git: GitTransition,
}

#[derive(Serialize)]
struct GitTransition {
    before: GitEvidence,
    after: GitEvidence,
}

#[derive(Serialize)]
struct GitEvidence {
    status: String,
    diff_stat: String,
}

fn git_evidence(root: &Path) -> GitEvidence {
    GitEvidence {
        status: git_output(root, &["status", "--short"]),
        diff_stat: git_output(root, &["diff", "--stat"]),
    }
}

fn git_output(root: &Path, args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_machine_readable_inspect_run() {
        let cli = Cli::try_parse_from([
            "le-harness",
            "run",
            "inspect this repository",
            "--mode",
            "inspect",
            "--json",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Run {
                mode: CliMode::Inspect,
                json: true,
                ..
            })
        ));
    }
}
