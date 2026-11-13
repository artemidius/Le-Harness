use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::runtime::{ToolHandler, ToolInvocation, ToolResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_working_directory")]
    pub working_directory: PathBuf,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub network: bool,
}

fn default_working_directory() -> PathBuf {
    PathBuf::from(".")
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutionLimits {
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_output_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionOutput {
    pub backend: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("invalid working directory")]
    InvalidWorkingDirectory,
    #[error("working directory escapes the repository")]
    OutsideRepository,
    #[error("environment variable is not allowed: {0}")]
    EnvironmentDenied(String),
    #[error("network access is disabled for this backend")]
    NetworkDenied,
    #[error("command timed out after {0:?}")]
    Timeout(Duration),
    #[error("combined command output exceeded {0} bytes")]
    OutputLimit(usize),
    #[error("command I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("command arguments are invalid: {0}")]
    Arguments(serde_json::Error),
}

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    async fn execute(&self, spec: &CommandSpec) -> Result<ExecutionOutput, ExecutionError>;
}

#[derive(Debug, Clone)]
pub struct NativeExecution {
    root: PathBuf,
    limits: ExecutionLimits,
    allowed_environment: BTreeSet<String>,
}

impl NativeExecution {
    pub fn new(root: impl AsRef<Path>, limits: ExecutionLimits) -> Result<Self, ExecutionError> {
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            limits,
            allowed_environment: BTreeSet::from(["PATH".to_owned()]),
        })
    }

    pub fn allow_environment(mut self, name: impl Into<String>) -> Self {
        self.allowed_environment.insert(name.into());
        self
    }
}

#[async_trait]
impl ExecutionBackend for NativeExecution {
    async fn execute(&self, spec: &CommandSpec) -> Result<ExecutionOutput, ExecutionError> {
        if spec.network {
            return Err(ExecutionError::NetworkDenied);
        }
        let working_directory = confined_directory(&self.root, &spec.working_directory)?;
        let environment = checked_environment(&spec.environment, &self.allowed_environment)?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(working_directory)
            .env_clear();
        for name in &self.allowed_environment {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.envs(environment);
        run_command(command, "native", self.limits).await
    }
}

#[derive(Debug, Clone)]
pub struct DockerExecution {
    root: PathBuf,
    image: String,
    limits: ExecutionLimits,
    allowed_environment: BTreeSet<String>,
    allow_network: bool,
}

impl DockerExecution {
    pub fn new(
        root: impl AsRef<Path>,
        image: impl Into<String>,
        limits: ExecutionLimits,
    ) -> Result<Self, ExecutionError> {
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            image: image.into(),
            limits,
            allowed_environment: BTreeSet::new(),
            allow_network: false,
        })
    }

    pub fn allow_environment(mut self, name: impl Into<String>) -> Self {
        self.allowed_environment.insert(name.into());
        self
    }

    pub fn allow_network(mut self, allow: bool) -> Self {
        self.allow_network = allow;
        self
    }
}

#[async_trait]
impl ExecutionBackend for DockerExecution {
    async fn execute(&self, spec: &CommandSpec) -> Result<ExecutionOutput, ExecutionError> {
        if spec.network && !self.allow_network {
            return Err(ExecutionError::NetworkDenied);
        }
        let working_directory = confined_directory(&self.root, &spec.working_directory)?;
        let relative = working_directory
            .strip_prefix(&self.root)
            .map_err(|_| ExecutionError::OutsideRepository)?;
        let environment = checked_environment(&spec.environment, &self.allowed_environment)?;
        let container_workdir = if relative.as_os_str().is_empty() {
            "/workspace".to_owned()
        } else {
            format!("/workspace/{}", relative.to_string_lossy())
        };
        let mut command = Command::new("docker");
        command.args([
            "run",
            "--rm",
            "--init",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--pids-limit=256",
            "--memory=1g",
            "--cpus=2",
        ]);
        if !spec.network {
            command.args(["--network", "none"]);
        }
        command.args([
            "--mount",
            &format!("type=bind,src={},dst=/workspace", self.root.display()),
        ]);
        command.args(["--workdir", &container_workdir]);
        for (name, value) in environment {
            command.args(["--env", &format!("{name}={value}")]);
        }
        command.arg(&self.image).arg(&spec.program).args(&spec.args);
        command.current_dir(&self.root).env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        run_command(command, "docker", self.limits).await
    }
}

fn checked_environment(
    requested: &BTreeMap<String, String>,
    allowed: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, ExecutionError> {
    for name in requested.keys() {
        if !allowed.contains(name) {
            return Err(ExecutionError::EnvironmentDenied(name.clone()));
        }
    }
    Ok(requested.clone())
}

fn confined_directory(root: &Path, relative: &Path) -> Result<PathBuf, ExecutionError> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ExecutionError::InvalidWorkingDirectory);
    }
    let directory = root.join(relative).canonicalize()?;
    if !directory.starts_with(root) {
        return Err(ExecutionError::OutsideRepository);
    }
    if !directory.is_dir() {
        return Err(ExecutionError::InvalidWorkingDirectory);
    }
    Ok(directory)
}

async fn run_command(
    mut command: Command,
    backend: &str,
    limits: ExecutionLimits,
) -> Result<ExecutionOutput, ExecutionError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let started = Instant::now();
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("piped stdout is present");
    let stderr = child.stderr.take().expect("piped stderr is present");
    let counter = Arc::new(Mutex::new(0usize));
    let operation = collect_child(&mut child, stdout, stderr, counter, limits.max_output_bytes);
    let (status, stdout, stderr) = match tokio::time::timeout(limits.timeout, operation).await {
        Ok(result) => result?,
        Err(_) => {
            child.kill().await?;
            let _ = child.wait().await;
            return Err(ExecutionError::Timeout(limits.timeout));
        }
    };
    Ok(ExecutionOutput {
        backend: backend.into(),
        exit_code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        duration_ms: started.elapsed().as_millis(),
    })
}

async fn collect_child<R1, R2>(
    child: &mut Child,
    stdout: R1,
    stderr: R2,
    counter: Arc<Mutex<usize>>,
    limit: usize,
) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>), ExecutionError>
where
    R1: AsyncRead + Unpin,
    R2: AsyncRead + Unpin,
{
    let stdout_read = read_bounded(stdout, Arc::clone(&counter), limit);
    let stderr_read = read_bounded(stderr, counter, limit);
    let (status, stdout, stderr) = tokio::join!(child.wait(), stdout_read, stderr_read);
    Ok((status?, stdout?, stderr?))
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    counter: Arc<Mutex<usize>>,
    limit: usize,
) -> Result<Vec<u8>, ExecutionError> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return if exceeded {
                Err(ExecutionError::OutputLimit(limit))
            } else {
                Ok(output)
            };
        }
        let mut total = counter.lock().await;
        if total.saturating_add(read) > limit {
            exceeded = true;
            *total = limit.saturating_add(1);
        } else {
            *total += read;
        }
        drop(total);
        if !exceeded {
            output.extend_from_slice(&buffer[..read]);
        }
    }
}

pub struct CommandToolHandler<B> {
    backend: B,
}

impl<B> CommandToolHandler<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl<B: ExecutionBackend> ToolHandler for CommandToolHandler<B> {
    async fn execute(&self, invocation: &ToolInvocation) -> ToolResult {
        if invocation.name != "run_command" {
            return ToolResult::error(format!("unknown tool: {}", invocation.name));
        }
        let spec: CommandSpec = match serde_json::from_str(&invocation.arguments) {
            Ok(spec) => spec,
            Err(error) => return ToolResult::error(ExecutionError::Arguments(error).to_string()),
        };
        match self.backend.execute(&spec).await {
            Ok(output) => match serde_json::to_string(&output) {
                Ok(output) => ToolResult::success(output),
                Err(error) => ToolResult::error(error.to_string()),
            },
            Err(error) => ToolResult::error(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn spec(script: &str) -> CommandSpec {
        CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            working_directory: PathBuf::from("."),
            environment: BTreeMap::new(),
            network: false,
        }
    }

    #[tokio::test]
    async fn native_execution_captures_both_streams() {
        let directory = tempdir().unwrap();
        let backend = NativeExecution::new(directory.path(), ExecutionLimits::default()).unwrap();
        let output = backend
            .execute(&spec("printf out; printf err >&2"))
            .await
            .unwrap();
        assert_eq!(output.stdout, "out");
        assert_eq!(output.stderr, "err");
        assert_eq!(output.exit_code, Some(0));
    }

    #[tokio::test]
    async fn timeout_is_terminal() {
        let directory = tempdir().unwrap();
        let backend = NativeExecution::new(
            directory.path(),
            ExecutionLimits {
                timeout: Duration::from_millis(25),
                max_output_bytes: 100,
            },
        )
        .unwrap();
        assert!(matches!(
            backend.execute(&spec("sleep 1")).await,
            Err(ExecutionError::Timeout(_))
        ));
    }

    #[tokio::test]
    async fn output_is_bounded_across_streams() {
        let directory = tempdir().unwrap();
        let backend = NativeExecution::new(
            directory.path(),
            ExecutionLimits {
                timeout: Duration::from_secs(2),
                max_output_bytes: 8,
            },
        )
        .unwrap();
        assert!(matches!(
            backend
                .execute(&spec("printf 12345; printf 67890 >&2"))
                .await,
            Err(ExecutionError::OutputLimit(8))
        ));
    }

    #[tokio::test]
    async fn environment_is_allowlisted_and_cleared() {
        let directory = tempdir().unwrap();
        let backend = NativeExecution::new(directory.path(), ExecutionLimits::default())
            .unwrap()
            .allow_environment("HARNESS_TEST");
        let mut command = spec("printf '%s:%s' \"$HARNESS_TEST\" \"${HOME-unset}\"");
        command
            .environment
            .insert("HARNESS_TEST".into(), "yes".into());
        let output = backend.execute(&command).await.unwrap();
        assert_eq!(output.stdout, "yes:unset");
    }

    #[tokio::test]
    async fn rejects_escape_and_network_requests() {
        let directory = tempdir().unwrap();
        let backend = NativeExecution::new(directory.path(), ExecutionLimits::default()).unwrap();
        let mut escaping = spec("true");
        escaping.working_directory = PathBuf::from("..");
        assert!(matches!(
            backend.execute(&escaping).await,
            Err(ExecutionError::InvalidWorkingDirectory)
        ));
        let mut networked = spec("true");
        networked.network = true;
        assert!(matches!(
            backend.execute(&networked).await,
            Err(ExecutionError::NetworkDenied)
        ));
    }

    #[tokio::test]
    async fn docker_backend_runs_when_daemon_and_fixture_image_are_available() {
        let available = std::process::Command::new("docker")
            .args(["image", "inspect", "alpine:3.22"])
            .output()
            .is_ok_and(|output| output.status.success());
        if !available {
            return;
        }
        let directory = tempdir().unwrap();
        let backend =
            DockerExecution::new(directory.path(), "alpine:3.22", ExecutionLimits::default())
                .unwrap();
        let output = backend.execute(&spec("printf isolated")).await.unwrap();
        assert_eq!(output.backend, "docker");
        assert_eq!(output.stdout, "isolated");
    }
}
