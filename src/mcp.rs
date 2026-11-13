use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use le_rust::chat::{FunctionDefinition, Tool as ModelTool};
use rmcp::model::{CallToolRequestParams, PaginatedRequestParams};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::RwLock;

use crate::policy::Effect;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    pub tool_effects: BTreeMap<String, Effect>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredMcpTool {
    pub qualified_name: String,
    pub server: String,
    pub native_name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    pub effect: Effect,
}

impl DiscoveredMcpTool {
    pub fn model_tool(&self) -> ModelTool {
        ModelTool::function(
            FunctionDefinition::new(&self.qualified_name, self.input_schema.clone()).description(
                self.description.clone().unwrap_or_else(|| {
                    format!("MCP tool {} from {}", self.native_name, self.server)
                }),
            ),
        )
    }
}

#[derive(Debug, Error)]
pub enum McpError {
    #[error("MCP configuration is invalid: {0}")]
    Configuration(String),
    #[error("MCP operation timed out for server {0}")]
    Timeout(String),
    #[error("MCP transport failed for server {server}: {message}")]
    Transport { server: String, message: String },
    #[error("MCP tool result exceeded {0} bytes")]
    OutputLimit(usize),
    #[error("unknown MCP tool {0}")]
    UnknownTool(String),
    #[error("MCP arguments must be a JSON object")]
    InvalidArguments,
}

#[derive(Debug, Clone)]
struct RegisteredTool {
    server: String,
    native_name: String,
}

pub struct McpManager {
    config: McpConfig,
    timeout: Duration,
    max_output_bytes: usize,
    registered: RwLock<HashMap<String, RegisteredTool>>,
}

impl McpManager {
    pub fn new(config: McpConfig, timeout: Duration, max_output_bytes: usize) -> Self {
        Self {
            config,
            timeout,
            max_output_bytes,
            registered: RwLock::new(HashMap::new()),
        }
    }

    pub async fn discover(&self) -> Result<Vec<DiscoveredMcpTool>, McpError> {
        let mut discovered = Vec::new();
        let mut registered = HashMap::new();
        for (server_name, config) in &self.config.servers {
            let mut client = self.connect(server_name, config).await?;
            let mut cursor = None;
            loop {
                let params = cursor
                    .clone()
                    .map(|cursor| PaginatedRequestParams::default().with_cursor(Some(cursor)));
                let page = self.timed(server_name, client.list_tools(params)).await?;
                for tool in page.tools {
                    let native_name = tool.name.to_string();
                    let effect =
                        config
                            .tool_effects
                            .get(&native_name)
                            .copied()
                            .ok_or_else(|| {
                                McpError::Configuration(format!(
                                    "server {server_name} tool {native_name} has no explicit effect"
                                ))
                            })?;
                    let qualified_name = format!(
                        "mcp__{}__{}",
                        safe_name(server_name),
                        safe_name(&native_name)
                    );
                    if registered.contains_key(&qualified_name) {
                        return Err(McpError::Configuration(format!(
                            "qualified tool name collision: {qualified_name}"
                        )));
                    }
                    registered.insert(
                        qualified_name.clone(),
                        RegisteredTool {
                            server: server_name.clone(),
                            native_name: native_name.clone(),
                        },
                    );
                    discovered.push(DiscoveredMcpTool {
                        qualified_name,
                        server: server_name.clone(),
                        native_name,
                        description: tool.description.map(|value| value.into_owned()),
                        input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
                        effect,
                    });
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            let _ = client.close_with_timeout(self.timeout).await;
        }
        *self.registered.write().await = registered;
        discovered.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        Ok(discovered)
    }

    pub async fn call(&self, qualified_name: &str, arguments: &str) -> Result<String, McpError> {
        let registered = self
            .registered
            .read()
            .await
            .get(qualified_name)
            .cloned()
            .ok_or_else(|| McpError::UnknownTool(qualified_name.into()))?;
        let config = self
            .config
            .servers
            .get(&registered.server)
            .ok_or_else(|| McpError::UnknownTool(qualified_name.into()))?;
        let value: serde_json::Value =
            serde_json::from_str(arguments).map_err(|_| McpError::InvalidArguments)?;
        let object = value
            .as_object()
            .cloned()
            .ok_or(McpError::InvalidArguments)?;
        let mut client = self.connect(&registered.server, config).await?;
        let result = self
            .timed(
                &registered.server,
                client.call_tool(
                    CallToolRequestParams::new(Cow::Owned(registered.native_name))
                        .with_arguments(object),
                ),
            )
            .await?;
        let output = serde_json::to_string(&result).map_err(|error| McpError::Transport {
            server: registered.server.clone(),
            message: error.to_string(),
        })?;
        let _ = client.close_with_timeout(self.timeout).await;
        if output.len() > self.max_output_bytes {
            return Err(McpError::OutputLimit(self.max_output_bytes));
        }
        Ok(output)
    }

    async fn connect(
        &self,
        name: &str,
        config: &McpServerConfig,
    ) -> Result<RunningService<RoleClient, ()>, McpError> {
        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .env_clear()
            .envs(&config.environment);
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        if let Some(directory) = &config.working_directory {
            command.current_dir(directory);
        }
        let transport = TokioChildProcess::new(command).map_err(|error| McpError::Transport {
            server: name.into(),
            message: error.to_string(),
        })?;
        self.timed(name, ().serve(transport)).await
    }

    async fn timed<T, E>(
        &self,
        server: &str,
        operation: impl std::future::Future<Output = Result<T, E>>,
    ) -> Result<T, McpError>
    where
        E: std::fmt::Display,
    {
        match tokio::time::timeout(self.timeout, operation).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(McpError::Transport {
                server: server.into(),
                message: error.to_string(),
            }),
            Err(_) => Err(McpError::Timeout(server.into())),
        }
    }
}

fn safe_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_names_are_provider_safe() {
        assert_eq!(safe_name("git-tools/v1"), "git_tools_v1");
    }
}
