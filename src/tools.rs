use std::sync::Arc;

use async_trait::async_trait;

use crate::edit::{SpanEdit, WorkspaceEditor};
use crate::execution::{CommandSpec, ExecutionBackend};
use crate::repository::RepositoryToolHandler;
use crate::runtime::{ToolHandler, ToolInvocation, ToolResult};

pub struct CodingTools {
    repository: RepositoryToolHandler,
    editor: WorkspaceEditor,
    execution: Arc<dyn ExecutionBackend>,
}

impl CodingTools {
    pub fn new(
        repository: RepositoryToolHandler,
        editor: WorkspaceEditor,
        execution: Arc<dyn ExecutionBackend>,
    ) -> Self {
        Self {
            repository,
            editor,
            execution,
        }
    }
}

#[async_trait]
impl ToolHandler for CodingTools {
    async fn execute(&self, invocation: &ToolInvocation) -> ToolResult {
        match invocation.name.as_str() {
            "list_files" | "read_file" | "search" => self.repository.execute(invocation).await,
            "replace_span" => {
                let request: SpanEdit = match serde_json::from_str(&invocation.arguments) {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid edit arguments: {error}"));
                    }
                };
                match self.editor.preview(request).and_then(|preview| {
                    let diff = preview.diff.clone();
                    self.editor.apply(&preview).map(|applied| (applied, diff))
                }) {
                    Ok((applied, diff)) => ToolResult::success(
                        serde_json::json!({"applied":applied,"diff":diff}).to_string(),
                    ),
                    Err(error) => ToolResult::error(error.to_string()),
                }
            }
            "run_command" => {
                let request: CommandSpec = match serde_json::from_str(&invocation.arguments) {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid command arguments: {error}"));
                    }
                };
                match self.execution.execute(&request).await {
                    Ok(output) => match serde_json::to_string(&output) {
                        Ok(output) => ToolResult::success(output),
                        Err(error) => ToolResult::error(error.to_string()),
                    },
                    Err(error) => ToolResult::error(error.to_string()),
                }
            }
            _ => ToolResult::error(format!("unknown tool: {}", invocation.name)),
        }
    }
}
