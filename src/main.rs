#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use async_trait::async_trait;
use clap::{Parser, Subcommand, ValueEnum};
use le_harness::credentials::{
    CredentialStore, EphemeralCredentialStore, OsCredentialStore, resolve_persisted_api_key,
};
use le_harness::edit::WorkspaceEditor;
use le_harness::eval::{EvalManifest, EvalMode};
use le_harness::execution::{DockerExecution, ExecutionBackend, ExecutionLimits, NativeExecution};
use le_harness::fim::FimEditEngine;
use le_harness::local::{LocalModelConfig, LocalOpenAiAdapter};
use le_harness::mcp::{McpConfig, McpManager};
use le_harness::model::{
    MistralAdapter, MistralModelInfo, ModelAdapter, SwappableModel, coding_tools,
};
use le_harness::model_store::{ModelSelection, ModelStore};
use le_harness::policy::{
    ApprovalBinding, ApprovalRequest, ApprovalStore, Approver, Effect,
    InteractivePolicyToolHandler, PermissionMode, PermissionPolicy, ToolEffects,
};
use le_harness::repository::{Repository, RepositoryToolHandler, RepositoryToolLimits};
use le_harness::router::{HybridRouter, RouteBudgets, RouteCapabilities};
use le_harness::runtime::{AgentController, AgentLimits};
use le_harness::session::SessionStore;
use le_harness::settings::{
    ApiKey, CredentialPrompt, InteractionPolicy, MistralModelConfig, MistralSettings, ModelPrompt,
    ModelSelectionRole, SettingsError, resolve_api_key, resolve_model,
};
use le_harness::tools::CodingTools;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "le-harness", version, about)]
struct Cli {
    /// Disable all interactive prompts.
    #[arg(long, global = true)]
    no_input: bool,
    /// Do not read or save API keys in the operating system credential store.
    #[arg(long, global = true)]
    no_credential_store: bool,
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
        /// Probe a loopback OpenAI-compatible endpoint.
        #[arg(long, requires = "local_model")]
        local_base_url: Option<String>,
        /// Exact local model identifier to verify.
        #[arg(long, requires = "local_base_url")]
        local_model: Option<String>,
    },
    /// Manage the Mistral API key in the operating system credential store.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Start a terminal chat, optionally with an initial task.
    Run {
        /// Optional first message. Required for non-interactive use.
        prompt: Option<String>,
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
        /// JSON configuration for local stdio MCP servers.
        #[arg(long)]
        mcp_config: Option<PathBuf>,
        /// Loopback OpenAI-compatible base URL ending in `/v1/`.
        #[arg(long, requires = "local_model")]
        local_base_url: Option<String>,
        /// Exact local chat model identifier.
        #[arg(long, requires = "local_base_url")]
        local_model: Option<String>,
        /// Exact local FIM model identifier; omission disables local FIM.
        #[arg(long, requires = "local_base_url")]
        local_fim_model: Option<String>,
        /// Use no cloud clients or cloud fallbacks.
        #[arg(long, requires = "local_base_url")]
        local_only: bool,
        /// Declare that the local endpoint does not support tool calling.
        #[arg(long, requires = "local_base_url")]
        local_without_tools: bool,
        /// Persist the run under this state directory.
        #[arg(long)]
        session_dir: Option<PathBuf>,
        /// Continue from a terminal session without replaying its tool calls.
        #[arg(long)]
        resume: Option<String>,
    },
    /// Run a repeatable evaluation manifest.
    Eval {
        /// Versioned JSON task manifest.
        manifest: PathBuf,
        /// Candidate strategy to measure.
        #[arg(long, value_enum)]
        mode: CliEvalMode,
        /// Runs per case.
        #[arg(long, default_value_t = 5)]
        repetitions: u32,
        /// Report format.
        #[arg(long, value_enum, default_value = "json")]
        format: ReportFormat,
    },
    /// Inspect persisted JSONL session journals.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Explore a persisted execution trace in an interactive terminal UI.
    Tui {
        /// Session identifier returned by `run` or `session list`.
        id: String,
        /// Exact journal directory.
        #[arg(long)]
        session_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Prompt for and save a Mistral API key securely.
    Login,
    /// Remove the saved Mistral API key.
    Logout,
    /// Report credential availability without exposing the key.
    Status,
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    /// List sessions, newest first.
    List {
        #[arg(long)]
        session_dir: Option<PathBuf>,
    },
    /// Print a validated session journal as JSON.
    Inspect {
        id: String,
        #[arg(long)]
        session_dir: Option<PathBuf>,
    },
    /// Report whether a session can be continued safely.
    Resume {
        id: String,
        #[arg(long)]
        session_dir: Option<PathBuf>,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliEvalMode {
    Conventional,
    Fim,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReportFormat {
    Json,
    Markdown,
}

struct LocalCliConfig {
    base_url: Option<String>,
    model: Option<String>,
    fim_model: Option<String>,
    only: bool,
    tool_calling: bool,
}

struct RunConfig {
    prompt: Option<String>,
    root: PathBuf,
    mode: CliMode,
    execution: CliExecution,
    image: String,
    json: bool,
    no_input: bool,
    no_credential_store: bool,
    mcp_config: Option<PathBuf>,
    local: LocalCliConfig,
    session_dir: Option<PathBuf>,
    resume: Option<String>,
}

struct RoutedModels {
    planner: Arc<dyn ModelAdapter>,
    fim: Arc<dyn ModelAdapter>,
    utility: Arc<dyn ModelAdapter>,
    labels: [String; 3],
}

#[tokio::main]
async fn main() -> ExitCode {
    match run_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let no_input = cli.no_input;
    let no_credential_store = cli.no_credential_store;
    match cli.command {
        Some(Command::Doctor {
            live,
            local_base_url,
            local_model,
        }) => {
            doctor(
                live,
                local_base_url,
                local_model,
                no_input,
                no_credential_store,
            )
            .await?
        }
        Some(Command::Auth { command }) => {
            if no_credential_store {
                return Err("auth commands require the OS credential store".into());
            }
            auth_command(command, no_input)?
        }
        Some(Command::Run {
            prompt,
            root,
            mode,
            execution,
            image,
            json,
            mcp_config,
            local_base_url,
            local_model,
            local_fim_model,
            local_only,
            local_without_tools,
            session_dir,
            resume,
        }) => {
            run(RunConfig {
                prompt,
                root,
                mode,
                execution,
                image,
                json,
                no_input,
                no_credential_store,
                mcp_config,
                local: LocalCliConfig {
                    base_url: local_base_url,
                    model: local_model,
                    fim_model: local_fim_model,
                    only: local_only,
                    tool_calling: !local_without_tools,
                },
                session_dir,
                resume,
            })
            .await?
        }
        Some(Command::Eval {
            manifest,
            mode,
            repetitions,
            format,
        }) => evaluate(&manifest, mode, repetitions, format)?,
        Some(Command::Session { command }) => session_command(command)?,
        Some(Command::Tui { id, session_dir }) => {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                return Err("TUI requires an interactive terminal".into());
            }
            le_harness::tui::run(session_store(session_dir)?.load(&id)?)?;
        }
        None => {
            run(RunConfig {
                prompt: None,
                root: PathBuf::from("."),
                mode: CliMode::Assisted,
                execution: CliExecution::Native,
                image: "rust:1.88-slim".into(),
                json: false,
                no_input,
                no_credential_store,
                mcp_config: None,
                local: LocalCliConfig {
                    base_url: None,
                    model: None,
                    fim_model: None,
                    only: false,
                    tool_calling: true,
                },
                session_dir: None,
                resume: None,
            })
            .await?
        }
    }
    Ok(())
}

fn evaluate(
    path: &Path,
    mode: CliEvalMode,
    repetitions: u32,
    format: ReportFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest: EvalManifest = serde_json::from_slice(&std::fs::read(path)?)?;
    let mode = match mode {
        CliEvalMode::Conventional => EvalMode::Conventional,
        CliEvalMode::Fim => EvalMode::Fim,
    };
    let report = manifest.run(mode, repetitions)?;
    match format {
        ReportFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        ReportFormat::Markdown => print!("{}", report.markdown()),
    }
    Ok(())
}

fn session_command(command: SessionCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        SessionCommand::List { session_dir } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&session_store(session_dir)?.list()?)?
            );
        }
        SessionCommand::Inspect { id, session_dir } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&session_store(session_dir)?.load(&id)?)?
            );
        }
        SessionCommand::Resume { id, session_dir } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&session_store(session_dir)?.resume_state(&id)?)?
            );
        }
    }
    Ok(())
}

async fn doctor(
    live: bool,
    local_base_url: Option<String>,
    local_model: Option<String>,
    no_input: bool,
    no_credential_store: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let diagnostic = MistralSettings::diagnostic();
    println!("Le Harness {}", env!("CARGO_PKG_VERSION"));
    println!(
        "MISTRAL_API_KEY environment override: {}",
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
    println!(
        "utility model: {}",
        diagnostic
            .utility_model
            .as_deref()
            .unwrap_or("same as chat model")
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
        let api_key = resolve_mistral_key(no_input, false, no_credential_store)?;
        let adapter = MistralAdapter::from_credentials(
            &api_key,
            "discovery-only",
            "discovery-only",
            configured_mistral_base_url().as_deref(),
        )?;
        let mut models = adapter.list_models().await?;
        models.sort();
        println!("available Mistral models:");
        for model in &models {
            println!("  {model}");
        }
        let configured = [
            ("chat", diagnostic.chat_model.clone()),
            ("FIM", diagnostic.fim_model.clone()),
            (
                "utility",
                diagnostic
                    .utility_model
                    .clone()
                    .or_else(|| diagnostic.chat_model.clone()),
            ),
        ];
        for (role, model) in configured
            .into_iter()
            .filter_map(|(role, model)| model.map(|model| (role, model)))
        {
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
    if let (Some(base_url), Some(model)) = (local_base_url, local_model) {
        let adapter = LocalOpenAiAdapter::new(LocalModelConfig {
            base_url,
            chat_model: model.clone(),
            fim_model: None,
            api_key: std::env::var("LE_HARNESS_LOCAL_API_KEY").ok(),
            tool_calling: true,
        })?;
        let models = adapter.list_models().await?;
        println!(
            "local model {model}: {}",
            if models.contains(&model) {
                "available"
            } else {
                "not returned by model discovery"
            }
        );
    }
    Ok(())
}

async fn run(config: RunConfig) -> Result<(), Box<dyn std::error::Error>> {
    let RunConfig {
        prompt,
        root,
        mode,
        execution,
        image,
        json,
        no_input,
        no_credential_store,
        mcp_config,
        local,
        session_dir,
        resume,
    } = config;
    let interactive = !json && !no_input && io::stdin().is_terminal() && io::stdout().is_terminal();
    if !interactive && prompt.is_none() {
        return Err("a prompt is required without an interactive terminal".into());
    }
    if matches!(mode, CliMode::AutomatedSandbox) && matches!(execution, CliExecution::Native) {
        return Err("automated-sandbox mode requires --execution docker".into());
    }
    let root = root.canonicalize()?;
    let sessions = session_store(session_dir)?;
    if interactive {
        println!("Le Harness · {}", root.display());
        println!("Type /help for commands or /exit to leave.\n");
    }
    let mut next_prompt = match prompt {
        Some(prompt) => prompt,
        None => match read_chat_message()? {
            Some(prompt) => prompt,
            None => return Ok(()),
        },
    };
    let mut initial_resume = if let Some(id) = resume.as_deref() {
        let snapshot = sessions.load(id)?;
        if snapshot.header.repository_root.canonicalize()? != root {
            return Err("resume session belongs to a different repository".into());
        }
        Some(sessions.resume_state(id)?)
    } else {
        None
    };
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
    let local_adapter = match (&local.base_url, &local.model) {
        (Some(base_url), Some(model)) => Some(LocalOpenAiAdapter::new(LocalModelConfig {
            base_url: base_url.clone(),
            chat_model: model.clone(),
            fim_model: local.fim_model.clone(),
            api_key: std::env::var("LE_HARNESS_LOCAL_API_KEY").ok(),
            tool_calling: local.tool_calling,
        })?),
        (None, None) => None,
        _ => return Err("both --local-base-url and --local-model are required".into()),
    };
    let model_store = ModelStore::default_path().map(ModelStore::new);
    let (cloud_settings, prompted_on_start) = if local.only {
        (None, false)
    } else {
        let (settings, prompted) =
            resolve_mistral_settings(no_input, json, no_credential_store, model_store.as_ref())
                .await?;
        (Some(settings), prompted)
    };
    let model = Arc::new(SwappableModel::new(build_routed_model(
        cloud_settings.as_ref(),
        local_adapter.as_ref(),
        &local,
    )?));
    if interactive && let Some(settings) = cloud_settings.as_ref() {
        println!(
            "le> Using {} (FIM: {}). Enter /model to change.",
            settings.chat_model, settings.fim_model
        );
    }
    let fim = Arc::new(FimEditEngine::new(
        model.clone(),
        editor.clone(),
        512 * 1024,
    ));
    let mut tools = CodingTools::new(repository_tools, editor, backend).with_fim(fim);
    let mut catalog = if local.only && !local.tool_calling {
        Vec::new()
    } else {
        coding_tools()
    };
    let mut effects = ToolEffects::default()
        .register("list_files", Effect::Read)
        .register("read_file", Effect::Read)
        .register("file_metadata", Effect::Read)
        .register("search", Effect::Read)
        .register("create_file", Effect::Write)
        .register("replace_text", Effect::Write)
        .register("replace_file", Effect::Write)
        .register("replace_span", Effect::Write)
        .register("fim_replace_span", Effect::Write)
        .register("delete_file", Effect::Write)
        .register("run_command", Effect::Execute);
    if let Some(path) = mcp_config {
        let config: McpConfig = serde_json::from_slice(&std::fs::read(path)?)?;
        let manager = Arc::new(McpManager::new(
            config,
            std::time::Duration::from_secs(30),
            2 * 1024 * 1024,
        ));
        for discovered in manager.discover().await? {
            effects = effects.register(&discovered.qualified_name, discovered.effect);
            catalog.push(discovered.model_tool());
        }
        tools = tools.with_mcp(manager);
    }
    let approvals = ApprovalStore::default();
    let policy = PermissionPolicy::new(mode.into(), approvals.clone());
    let tools = InteractivePolicyToolHandler::new(
        tools,
        effects,
        policy,
        approvals,
        ConsoleApprover {
            enabled: !json && !no_input,
        },
    );
    let controller = AgentController::new(model.clone(), tools, AgentLimits::default())
        .with_tool_catalog(catalog);
    let cancellation = controller.cancellation_token();
    let mut cloud_settings = cloud_settings;
    if prompted_on_start && next_prompt == "/model" {
        next_prompt = match read_chat_message()? {
            Some(prompt) => prompt,
            None => return Ok(()),
        };
    }
    let mut history = Vec::new();
    let mut parent_session = resume;
    loop {
        if next_prompt == "/model" {
            if let Some(current) = cloud_settings.as_ref() {
                match select_mistral_models(&current.api_key).await {
                    Ok(models) => {
                        let selected = MistralSettings::from_parts(current.api_key.clone(), models);
                        match build_routed_model(Some(&selected), local_adapter.as_ref(), &local) {
                            Ok(route) => {
                                model.replace(route).await;
                                println!(
                                    "le> Using {} (FIM: {}).",
                                    selected.chat_model, selected.fim_model
                                );
                                if let Some(store) = model_store.as_ref() {
                                    let choice = ModelSelection {
                                        chat: selected.chat_model.clone(),
                                        fim: selected.fim_model.clone(),
                                    };
                                    if let Err(error) = store.save(&choice) {
                                        eprintln!("le> Could not save model choice: {error}");
                                    }
                                }
                                if nonblank_environment("LE_HARNESS_CHAT_MODEL").is_some()
                                    || nonblank_environment("LE_HARNESS_FIM_MODEL").is_some()
                                {
                                    eprintln!(
                                        "le> Environment model overrides will take precedence next time."
                                    );
                                }
                                cloud_settings = Some(selected);
                            }
                            Err(error) => eprintln!("le> Could not switch model: {error}"),
                        }
                    }
                    Err(error) => eprintln!("le> Could not open model picker: {error}"),
                }
            } else {
                eprintln!("le> Local-only model is set by --local-model when starting this chat.");
            }
            next_prompt = match read_chat_message()? {
                Some(prompt) => prompt,
                None => return Ok(()),
            };
            continue;
        }
        let original_prompt = next_prompt;
        let effective_prompt = if let Some(state) = initial_resume.take() {
            state.continuation_prompt(&original_prompt)?
        } else {
            original_prompt.clone()
        };
        if interactive {
            println!("le> Working...");
        }
        let before = git_evidence(&root);
        let result = {
            let operation = controller.run_with_history(effective_prompt, &history);
            tokio::pin!(operation);
            tokio::select! {
                result = &mut operation => result,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    cancellation.cancel();
                    operation.await
                }
            }
        };
        cancellation.reset();
        let after = git_evidence(&root);
        let saved = sessions.save(original_prompt, &root, parent_session, &result)?;
        let output = RunOutput {
            session_id: saved.header.id.clone(),
            status: result.status.clone(),
            answer: result.output.clone(),
            model_requests: result.model_requests,
            tool_calls: result.tool_calls,
            events: result.events,
            git: GitTransition { before, after },
        };
        let failure = match &output.status {
            le_harness::runtime::AgentStatus::Failed { message } => Some(message.clone()),
            _ => None,
        };
        if json {
            println!("{}", serde_json::to_string(&output)?);
        } else if interactive {
            if !output.answer.is_empty() {
                println!("\nle> {}", output.answer);
            }
            if let Some(message) = &failure {
                eprintln!("le> Error: {message}");
            } else if matches!(output.status, le_harness::runtime::AgentStatus::Cancelled) {
                eprintln!("le> Cancelled.");
            }
            if output.git.before.status != output.git.after.status {
                println!("repository changes:\n{}", output.git.after.status);
            }
        } else {
            println!("{}", output.answer);
            println!(
                "\nstatus: {} | model requests: {} | tool calls: {} | session: {}",
                serde_json::to_value(&output.status)?["state"]
                    .as_str()
                    .unwrap_or("unknown"),
                output.model_requests,
                output.tool_calls,
                output.session_id
            );
            if output.git.before.status != output.git.after.status {
                println!("repository changes:\n{}", output.git.after.status);
            }
        }
        if !interactive {
            if let Some(message) = failure {
                return Err(format!("agent run failed: {message}").into());
            }
            return Ok(());
        }
        if matches!(
            result.status,
            le_harness::runtime::AgentStatus::Completed { .. }
        ) {
            history = result.messages;
        }
        parent_session = Some(saved.header.id);
        next_prompt = match read_chat_message()? {
            Some(prompt) => prompt,
            None => return Ok(()),
        };
    }
}

fn build_routed_model(
    settings: Option<&MistralSettings>,
    local_adapter: Option<&LocalOpenAiAdapter>,
    local: &LocalCliConfig,
) -> Result<Arc<dyn ModelAdapter>, Box<dyn std::error::Error>> {
    let routed = if local.only {
        let adapter = local_adapter.ok_or("--local-only requires a configured local endpoint")?;
        let label = local.model.clone().expect("validated local model");
        RoutedModels {
            planner: Arc::new((*adapter).clone()),
            fim: Arc::new((*adapter).clone()),
            utility: Arc::new((*adapter).clone()),
            labels: [
                label.clone(),
                local.fim_model.clone().unwrap_or_else(|| "disabled".into()),
                label,
            ],
        }
    } else {
        let settings = settings.ok_or("cloud settings are missing")?;
        let utility_label = local
            .model
            .clone()
            .unwrap_or_else(|| settings.utility_model.clone());
        let utility: Arc<dyn ModelAdapter> = match local_adapter {
            Some(adapter) => Arc::new((*adapter).clone()),
            None => Arc::new(mistral_adapter(settings, &settings.utility_model)?),
        };
        RoutedModels {
            planner: Arc::new(mistral_adapter(settings, &settings.chat_model)?),
            fim: Arc::new(mistral_adapter(settings, &settings.chat_model)?),
            utility,
            labels: [
                settings.chat_model.clone(),
                settings.fim_model.clone(),
                utility_label,
            ],
        }
    };
    Ok(Arc::new(
        HybridRouter::new(
            routed.planner,
            routed.fim,
            routed.utility,
            routed.labels,
            RouteBudgets::default(),
        )
        .capabilities(RouteCapabilities {
            planner_tools: !local.only || local.tool_calling,
            fim: !local.only || local.fim_model.is_some(),
            utility_chat: true,
            utility_tools: local.base_url.is_none() || local.tool_calling,
        }),
    ))
}

fn read_chat_message() -> io::Result<Option<String>> {
    loop {
        print!("\nyou> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            println!();
            return Ok(None);
        }
        let line = line.trim();
        match line {
            "" => continue,
            "/exit" | "/quit" => return Ok(None),
            "/help" => println!(
                "Enter a task or follow-up. /model changes cloud models; /exit leaves the chat; Ctrl-C cancels the current turn."
            ),
            _ => return Ok(Some(line.to_owned())),
        }
    }
}

struct TerminalCredentialPrompt;

impl CredentialPrompt for TerminalCredentialPrompt {
    fn prompt_api_key(&mut self) -> Result<String, SettingsError> {
        rpassword::prompt_password("le> Mistral API key (hidden; saved in OS credential store): ")
            .map_err(|error| match error.kind() {
                io::ErrorKind::Interrupted | io::ErrorKind::UnexpectedEof => {
                    SettingsError::InputCancelled
                }
                _ => SettingsError::PromptFailed(error.to_string()),
            })
    }
}

struct TerminalModelPrompt;

impl ModelPrompt for TerminalModelPrompt {
    fn prompt_model(
        &mut self,
        role: ModelSelectionRole,
        choices: &[String],
    ) -> Result<String, SettingsError> {
        eprintln!("le> Select a Mistral {role} model for this chat:");
        for (index, model) in choices.iter().enumerate() {
            eprintln!("  {}) {model}", index + 1);
        }
        loop {
            eprint!("you> Choice [1-{}]: ", choices.len());
            io::stderr()
                .flush()
                .map_err(|error| SettingsError::ModelPromptFailed(error.to_string()))?;
            let mut answer = String::new();
            let read = io::stdin()
                .read_line(&mut answer)
                .map_err(|error| match error.kind() {
                    io::ErrorKind::Interrupted | io::ErrorKind::UnexpectedEof => {
                        SettingsError::ModelInputCancelled
                    }
                    _ => SettingsError::ModelPromptFailed(error.to_string()),
                })?;
            if read == 0 {
                return Err(SettingsError::ModelInputCancelled);
            }
            let answer = answer.trim();
            if let Ok(index) = answer.parse::<usize>()
                && let Some(model) = index.checked_sub(1).and_then(|index| choices.get(index))
            {
                return Ok(model.clone());
            }
            if let Some(model) = choices.iter().find(|model| model.as_str() == answer) {
                return Ok(model.clone());
            }
            eprintln!(
                "Enter a number from 1 to {} or an exact model ID.",
                choices.len()
            );
        }
    }
}

fn resolve_mistral_key(
    no_input: bool,
    json: bool,
    no_credential_store: bool,
) -> Result<ApiKey, SettingsError> {
    resolve_mistral_key_with_policy(
        credential_interaction_policy(
            no_input,
            json,
            ci_environment(),
            io::stdin().is_terminal(),
            io::stderr().is_terminal(),
        ),
        no_credential_store,
    )
}

fn resolve_mistral_key_with_policy(
    policy: InteractionPolicy,
    no_credential_store: bool,
) -> Result<ApiKey, SettingsError> {
    let persistent = OsCredentialStore;
    let ephemeral = EphemeralCredentialStore;
    let store: &dyn CredentialStore = if no_credential_store {
        &ephemeral
    } else {
        &persistent
    };
    let resolved = resolve_persisted_api_key(
        std::env::var("MISTRAL_API_KEY").ok(),
        policy,
        &mut TerminalCredentialPrompt,
        store,
    )?;
    if let Some(warning) = resolved.store_warning {
        eprintln!("Warning: {warning}");
    }
    Ok(resolved.api_key)
}

fn auth_command(command: AuthCommand, no_input: bool) -> Result<(), Box<dyn std::error::Error>> {
    let store = OsCredentialStore;
    match command {
        AuthCommand::Login => {
            let policy = credential_interaction_policy(
                no_input,
                false,
                ci_environment(),
                io::stdin().is_terminal(),
                io::stderr().is_terminal(),
            );
            let key = resolve_api_key(None, policy, &mut TerminalCredentialPrompt)?;
            store
                .save(key.expose())
                .map_err(|error| format!("could not save key in OS credential store: {error}"))?;
            println!("Mistral API key saved in the OS credential store.");
        }
        AuthCommand::Logout => {
            if store
                .delete()
                .map_err(|error| format!("could not access OS credential store: {error}"))?
            {
                println!("Saved Mistral API key removed.");
            } else {
                println!("No saved Mistral API key was present.");
            }
        }
        AuthCommand::Status => {
            let status = match store.load() {
                Ok(Some(secret)) if ApiKey::new(secret.clone()).is_ok() => "stored",
                Ok(Some(_)) => "invalid",
                Ok(None) => "missing",
                Err(error) => {
                    return Err(format!("could not access OS credential store: {error}").into());
                }
            };
            println!("Mistral API key: {status}");
            if std::env::var("MISTRAL_API_KEY").is_ok_and(|value| !value.trim().is_empty()) {
                println!("MISTRAL_API_KEY environment override: set");
            }
        }
    }
    Ok(())
}

async fn resolve_mistral_settings(
    no_input: bool,
    json: bool,
    no_credential_store: bool,
    store: Option<&ModelStore>,
) -> Result<(MistralSettings, bool), Box<dyn std::error::Error>> {
    let policy = credential_interaction_policy(
        no_input,
        json,
        ci_environment(),
        io::stdin().is_terminal(),
        io::stderr().is_terminal(),
    );
    let api_key = resolve_mistral_key_with_policy(policy, no_credential_store)?;
    let chat_environment = nonblank_environment("LE_HARNESS_CHAT_MODEL");
    let fim_environment = nonblank_environment("LE_HARNESS_FIM_MODEL");
    let saved = match store.map(ModelStore::load) {
        Some(Ok(saved)) => saved,
        Some(Err(error)) => {
            eprintln!("le> Could not read saved model choice: {error}");
            None
        }
        None => None,
    };
    let chat_choice = chat_environment
        .clone()
        .or_else(|| saved.as_ref().map(|value| value.chat.clone()));
    let fim_choice = fim_environment
        .clone()
        .or_else(|| saved.as_ref().map(|value| value.fim.clone()));

    let (chat_choices, fim_choices) = if chat_choice.is_none() || fim_choice.is_none() {
        if policy == InteractionPolicy::Deny {
            (Vec::new(), Vec::new())
        } else {
            let discovery = MistralAdapter::from_credentials(
                &api_key,
                "discovery-only",
                "discovery-only",
                configured_mistral_base_url().as_deref(),
            )?;
            model_choices(discovery.discover_models().await?)
        }
    } else {
        (Vec::new(), Vec::new())
    };

    let mut prompt = TerminalModelPrompt;
    let chat_model = resolve_model(
        chat_choice.clone(),
        "LE_HARNESS_CHAT_MODEL",
        ModelSelectionRole::Chat,
        policy,
        &chat_choices,
        &mut prompt,
    )?;
    let fim_model = resolve_model(
        fim_choice.clone(),
        "LE_HARNESS_FIM_MODEL",
        ModelSelectionRole::Fim,
        policy,
        &fim_choices,
        &mut prompt,
    )?;
    let models = MistralModelConfig::from_values(
        chat_model,
        fim_model,
        nonblank_environment("LE_HARNESS_UTILITY_MODEL"),
    )?;
    let prompted = chat_choice.is_none() || fim_choice.is_none();
    if prompted && let Some(store) = store {
        let selection = ModelSelection {
            chat: models.chat_model.clone(),
            fim: models.fim_model.clone(),
        };
        if let Err(error) = store.save(&selection) {
            eprintln!("le> Could not save model choice: {error}");
        }
    }
    Ok((MistralSettings::from_parts(api_key, models), prompted))
}

async fn select_mistral_models(
    api_key: &ApiKey,
) -> Result<MistralModelConfig, Box<dyn std::error::Error>> {
    let discovery = MistralAdapter::from_credentials(
        api_key,
        "discovery-only",
        "discovery-only",
        configured_mistral_base_url().as_deref(),
    )?;
    let (chat_choices, fim_choices) = model_choices(discovery.discover_models().await?);
    let mut prompt = TerminalModelPrompt;
    let chat = resolve_model(
        None,
        "LE_HARNESS_CHAT_MODEL",
        ModelSelectionRole::Chat,
        InteractionPolicy::Allow,
        &chat_choices,
        &mut prompt,
    )?;
    let fim = resolve_model(
        None,
        "LE_HARNESS_FIM_MODEL",
        ModelSelectionRole::Fim,
        InteractionPolicy::Allow,
        &fim_choices,
        &mut prompt,
    )?;
    Ok(MistralModelConfig::from_values(
        chat,
        fim,
        nonblank_environment("LE_HARNESS_UTILITY_MODEL"),
    )?)
}

fn nonblank_environment(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn model_choices(models: Vec<MistralModelInfo>) -> (Vec<String>, Vec<String>) {
    let usable: Vec<_> = models
        .into_iter()
        .filter(|model| {
            model.archived != Some(true)
                && !model.id.trim().is_empty()
                && !model.id.chars().any(char::is_control)
        })
        .collect();

    let mut chat = model_ids(&usable, |model| {
        model.chat == Some(true) && model.function_calling != Some(false)
    });
    if chat.is_empty() {
        chat = model_ids(&usable, |model| {
            model.chat != Some(false) && model.function_calling != Some(false)
        });
    }

    let mut fim = model_ids(&usable, |model| model.fim == Some(true));
    if fim.is_empty() {
        fim = model_ids(&usable, |model| {
            model.fim != Some(false) && model.id.to_ascii_lowercase().contains("codestral")
        });
    }
    if fim.is_empty() {
        fim = model_ids(&usable, |model| model.fim != Some(false));
    }
    (chat, fim)
}

fn model_ids(
    models: &[MistralModelInfo],
    predicate: impl Fn(&MistralModelInfo) -> bool,
) -> Vec<String> {
    let mut ids: Vec<_> = models
        .iter()
        .filter(|model| predicate(model))
        .map(|model| model.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn credential_interaction_policy(
    no_input: bool,
    json: bool,
    ci: bool,
    input_terminal: bool,
    diagnostic_terminal: bool,
) -> InteractionPolicy {
    if no_input || json || ci || !input_terminal || !diagnostic_terminal {
        InteractionPolicy::Deny
    } else {
        InteractionPolicy::Allow
    }
}

fn ci_environment() -> bool {
    std::env::var("CI").is_ok_and(|value| {
        !value.trim().is_empty()
            && !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
    })
}

fn configured_mistral_base_url() -> Option<String> {
    std::env::var("LE_HARNESS_MISTRAL_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn mistral_adapter(
    settings: &MistralSettings,
    chat_model: &str,
) -> Result<MistralAdapter, le_harness::model::ModelError> {
    MistralAdapter::from_credentials(
        &settings.api_key,
        chat_model,
        &settings.fim_model,
        configured_mistral_base_url().as_deref(),
    )
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
            eprintln!("le> Proposed {:?} action: {}", request.effect, request.tool);
            if let Some(preview) = &request.preview {
                eprintln!("{preview}");
            } else {
                eprintln!("arguments: {}", request.arguments);
            }
            eprintln!("le> Allow this exact action? [y/N]");
            eprint!("you> ");
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
    session_id: String,
    status: le_harness::runtime::AgentStatus,
    answer: String,
    model_requests: u32,
    tool_calls: u32,
    events: Vec<le_harness::event::RuntimeEvent>,
    git: GitTransition,
}

fn session_store(configured: Option<PathBuf>) -> Result<SessionStore, Box<dyn std::error::Error>> {
    if let Some(path) = configured {
        return Ok(SessionStore::new(path));
    }
    if let Some(path) = std::env::var_os("LE_HARNESS_STATE_DIR").map(PathBuf::from) {
        return Ok(SessionStore::new(path.join("sessions")));
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/state"))
        })
        .ok_or("cannot determine state directory; set LE_HARNESS_STATE_DIR")?;
    Ok(SessionStore::new(base.join("le-harness/sessions")))
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

    #[test]
    fn interactive_credentials_require_an_explicitly_interactive_context() {
        assert_eq!(
            credential_interaction_policy(false, false, false, true, true),
            InteractionPolicy::Allow
        );
        for policy in [
            credential_interaction_policy(true, false, false, true, true),
            credential_interaction_policy(false, true, false, true, true),
            credential_interaction_policy(false, false, true, true, true),
            credential_interaction_policy(false, false, false, false, true),
            credential_interaction_policy(false, false, false, true, false),
        ] {
            assert_eq!(policy, InteractionPolicy::Deny);
        }
    }

    #[test]
    fn no_input_is_a_global_flag() {
        let cli = Cli::try_parse_from(["le-harness", "doctor", "--live", "--no-input"]).unwrap();
        assert!(cli.no_input);
        assert!(matches!(
            cli.command,
            Some(Command::Doctor { live: true, .. })
        ));
    }

    #[test]
    fn model_picker_uses_capabilities_and_excludes_archived_models() {
        let models = vec![
            MistralModelInfo {
                id: "chat-b".into(),
                chat: Some(true),
                fim: Some(false),
                function_calling: Some(true),
                archived: None,
            },
            MistralModelInfo {
                id: "fim-a".into(),
                chat: Some(false),
                fim: Some(true),
                function_calling: Some(false),
                archived: None,
            },
            MistralModelInfo {
                id: "archived-chat".into(),
                chat: Some(true),
                fim: Some(false),
                function_calling: Some(true),
                archived: Some(true),
            },
        ];

        let (chat, fim) = model_choices(models);
        assert_eq!(chat, vec!["chat-b"]);
        assert_eq!(fim, vec!["fim-a"]);
    }

    #[test]
    fn model_picker_supports_providers_without_capability_metadata() {
        let models = vec![
            MistralModelInfo {
                id: "legacy-b".into(),
                chat: None,
                fim: None,
                function_calling: None,
                archived: None,
            },
            MistralModelInfo {
                id: "legacy-a".into(),
                chat: None,
                fim: None,
                function_calling: None,
                archived: None,
            },
        ];

        let (chat, fim) = model_choices(models);
        assert_eq!(chat, vec!["legacy-a", "legacy-b"]);
        assert_eq!(fim, vec!["legacy-a", "legacy-b"]);
    }
}
