use std::sync::Arc;

use async_trait::async_trait;

use crate::edit::{
    CreateFileRequest, DeleteFileRequest, ReplaceFileRequest, ReplaceTextRequest, SpanEdit,
    WorkspaceEditor,
};
use crate::execution::{CommandSpec, ExecutionBackend};
use crate::fim::{FimEditRequest, FimService};
use crate::mcp::McpManager;
use crate::repository::RepositoryToolHandler;
use crate::runtime::{ToolHandler, ToolInvocation, ToolResult};

pub struct CodingTools {
    repository: RepositoryToolHandler,
    editor: WorkspaceEditor,
    execution: Arc<dyn ExecutionBackend>,
    fim: Option<Arc<dyn FimService>>,
    mcp: Option<Arc<McpManager>>,
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
            fim: None,
            mcp: None,
        }
    }

    pub fn with_mcp(mut self, mcp: Arc<McpManager>) -> Self {
        self.mcp = Some(mcp);
        self
    }

    pub fn with_fim(mut self, fim: Arc<dyn FimService>) -> Self {
        self.fim = Some(fim);
        self
    }
}

#[async_trait]
impl ToolHandler for CodingTools {
    async fn approval_preview(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<Option<String>, String> {
        let preview = match invocation.name.as_str() {
            "create_file" => {
                let request: CreateFileRequest = parse(&invocation.arguments)?;
                self.editor
                    .preview_create_summary(&request)
                    .map_err(|error| error.to_string())?
            }
            "replace_text" => {
                let request: ReplaceTextRequest = parse(&invocation.arguments)?;
                bounded(
                    self.editor
                        .preview_text(request)
                        .map_err(|error| error.to_string())?
                        .diff,
                )
            }
            "replace_file" => {
                let request: ReplaceFileRequest = parse(&invocation.arguments)?;
                bounded(
                    self.editor
                        .preview_file(request)
                        .map_err(|error| error.to_string())?
                        .diff,
                )
            }
            "replace_span" => {
                let request: SpanEdit = parse(&invocation.arguments)?;
                bounded(
                    self.editor
                        .preview(request)
                        .map_err(|error| error.to_string())?
                        .diff,
                )
            }
            "delete_file" => {
                let request: DeleteFileRequest = parse(&invocation.arguments)?;
                self.editor
                    .preview_delete_summary(&request)
                    .map_err(|error| error.to_string())?
            }
            "fim_replace_span" => {
                let request: FimEditRequest = parse(&invocation.arguments)?;
                format!(
                    "Generate and apply a FIM replacement in {} at bytes {}..{}. The file must still match sha256 {}.",
                    request.path, request.start, request.end, request.expected_sha256
                )
            }
            _ => return Ok(None),
        };
        Ok(Some(preview))
    }

    async fn execute(&self, invocation: &ToolInvocation) -> ToolResult {
        match invocation.name.as_str() {
            "list_files" | "read_file" | "file_metadata" | "search" => {
                self.repository.execute(invocation).await
            }
            "create_file" => {
                let request: CreateFileRequest = match serde_json::from_str(&invocation.arguments) {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid file arguments: {error}"));
                    }
                };
                match self.editor.create(request) {
                    Ok(created) => match serde_json::to_string(&created) {
                        Ok(output) => ToolResult::success_with_metadata(
                            output,
                            serde_json::json!({
                                "type":"create",
                                "path":&created.path,
                                "sha256":&created.sha256,
                                "created_directories":&created.created_directories
                            }),
                        ),
                        Err(error) => ToolResult::error(error.to_string()),
                    },
                    Err(error) => ToolResult::error(error.to_string()),
                }
            }
            "replace_text" => {
                let request: ReplaceTextRequest = match serde_json::from_str(&invocation.arguments)
                {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid edit arguments: {error}"));
                    }
                };
                apply_preview(&self.editor, self.editor.preview_text(request))
            }
            "replace_file" => {
                let request: ReplaceFileRequest = match serde_json::from_str(&invocation.arguments)
                {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid edit arguments: {error}"));
                    }
                };
                apply_preview(&self.editor, self.editor.preview_file(request))
            }
            "replace_span" => {
                let request: SpanEdit = match serde_json::from_str(&invocation.arguments) {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid edit arguments: {error}"));
                    }
                };
                let preview = self.editor.preview(request);
                apply_preview(&self.editor, preview)
            }
            "delete_file" => {
                let request: DeleteFileRequest = match serde_json::from_str(&invocation.arguments) {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid delete arguments: {error}"));
                    }
                };
                match self.editor.delete(request) {
                    Ok(deleted) => match serde_json::to_string(&deleted) {
                        Ok(output) => ToolResult::success_with_metadata(
                            output,
                            serde_json::json!({
                                "type":"delete",
                                "path":&deleted.path,
                                "previous_sha256":&deleted.previous_sha256,
                                "size":deleted.size
                            }),
                        ),
                        Err(error) => ToolResult::error(error.to_string()),
                    },
                    Err(error) => ToolResult::error(error.to_string()),
                }
            }
            "fim_replace_span" => {
                let Some(fim) = &self.fim else {
                    return ToolResult::error("FIM editing is not configured");
                };
                let request: FimEditRequest = match serde_json::from_str(&invocation.arguments) {
                    Ok(request) => request,
                    Err(error) => {
                        return ToolResult::error(format!("invalid FIM arguments: {error}"));
                    }
                };
                match fim.apply_fim(request).await {
                    Ok(outcome) => match serde_json::to_string(&outcome) {
                        Ok(output) => ToolResult::success_with_metadata(
                            output,
                            serde_json::json!({
                                "type":"fim_replace",
                                "path":&outcome.applied.path,
                                "previous_sha256":&outcome.applied.previous_sha256,
                                "sha256":&outcome.applied.sha256
                            }),
                        ),
                        Err(error) => ToolResult::error(error.to_string()),
                    },
                    Err(error) => ToolResult::error(error),
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
            name if name.starts_with("mcp__") => {
                let Some(mcp) = &self.mcp else {
                    return ToolResult::error("MCP is not configured");
                };
                match mcp.call(name, &invocation.arguments).await {
                    Ok(output) => ToolResult::success(output),
                    Err(error) => ToolResult::error(error.to_string()),
                }
            }
            _ => ToolResult::error(format!("unknown tool: {}", invocation.name)),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(arguments: &str) -> Result<T, String> {
    serde_json::from_str(arguments).map_err(|error| format!("invalid tool arguments: {error}"))
}

fn bounded(value: String) -> String {
    const LIMIT: usize = 16 * 1024;
    if value.len() <= LIMIT {
        return value;
    }
    let mut end = LIMIT;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... preview truncated ...", &value[..end])
}

fn apply_preview(
    editor: &WorkspaceEditor,
    preview: Result<crate::edit::EditPreview, crate::edit::EditError>,
) -> ToolResult {
    match preview.and_then(|preview| {
        let diff = preview.diff.clone();
        editor.apply(&preview).map(|applied| (applied, diff))
    }) {
        Ok((applied, diff)) => {
            let metadata = serde_json::json!({
                "type":"replace",
                "path":&applied.path,
                "previous_sha256":&applied.previous_sha256,
                "sha256":&applied.sha256
            });
            ToolResult::success_with_metadata(
                serde_json::json!({"applied":applied,"diff":diff}).to_string(),
                metadata,
            )
        }
        Err(error) => ToolResult::error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{ExecutionLimits, NativeExecution};
    use crate::policy::{
        ApprovalStore, Effect, PermissionMode, PermissionPolicy, PolicyToolHandler, ToolEffects,
    };
    use crate::repository::{Repository, RepositoryToolLimits};

    #[tokio::test]
    async fn create_file_is_dispatched_and_subject_to_write_policy() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let tools = CodingTools::new(
            RepositoryToolHandler::new(
                Repository::open(root).unwrap(),
                RepositoryToolLimits::default(),
            ),
            WorkspaceEditor::open(root).unwrap(),
            Arc::new(NativeExecution::new(root, ExecutionLimits::default()).unwrap()),
        );
        let invocation = ToolInvocation {
            id: "create-1".into(),
            name: "create_file".into(),
            arguments: serde_json::json!({"path":"sort.py","content":"def sort(items):\n    return sorted(items)\n"}).to_string(),
        };
        let guarded = PolicyToolHandler::new(
            tools,
            ToolEffects::default().register("create_file", Effect::Write),
            PermissionPolicy::new(PermissionMode::Inspect, ApprovalStore::default()),
        );
        let denied = guarded.execute(&invocation).await;
        assert!(denied.is_error);
        assert!(!root.join("sort.py").exists());

        let allowed = CodingTools::new(
            RepositoryToolHandler::new(
                Repository::open(root).unwrap(),
                RepositoryToolLimits::default(),
            ),
            WorkspaceEditor::open(root).unwrap(),
            Arc::new(NativeExecution::new(root, ExecutionLimits::default()).unwrap()),
        )
        .execute(&invocation)
        .await;
        assert!(!allowed.is_error, "{}", allowed.content);
        assert_eq!(
            std::fs::read_to_string(root.join("sort.py")).unwrap(),
            "def sort(items):\n    return sorted(items)\n"
        );
    }

    #[tokio::test]
    async fn create_read_replace_and_delete_form_a_complete_file_lifecycle() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let tools = CodingTools::new(
            RepositoryToolHandler::new(
                Repository::open(root).unwrap(),
                RepositoryToolLimits::default(),
            ),
            WorkspaceEditor::open(root).unwrap(),
            Arc::new(NativeExecution::new(root, ExecutionLimits::default()).unwrap()),
        );
        let create = ToolInvocation {
            id: "1".into(),
            name: "create_file".into(),
            arguments: serde_json::json!({
                "path":"src/sort.py",
                "content":"def sort(items):\n    return items\n"
            })
            .to_string(),
        };
        let preview = tools.approval_preview(&create).await.unwrap().unwrap();
        assert!(preview.contains("src/sort.py"));
        assert!(!tools.execute(&create).await.is_error);

        let read = tools
            .execute(&ToolInvocation {
                id: "2".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path":"src/sort.py"}).to_string(),
            })
            .await;
        let snapshot: serde_json::Value = serde_json::from_str(&read.content).unwrap();
        let replace = ToolInvocation {
            id: "3".into(),
            name: "replace_text".into(),
            arguments: serde_json::json!({
                "path":"src/sort.py",
                "expected_sha256":snapshot["sha256"],
                "old_text":"return items",
                "new_text":"return sorted(items)"
            })
            .to_string(),
        };
        assert!(
            tools
                .approval_preview(&replace)
                .await
                .unwrap()
                .unwrap()
                .contains("sorted")
        );
        let replaced = tools.execute(&replace).await;
        assert!(!replaced.is_error, "{}", replaced.content);
        assert_eq!(replaced.metadata.as_ref().unwrap()["type"], "replace");

        let read = tools
            .execute(&ToolInvocation {
                id: "4".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path":"src/sort.py"}).to_string(),
            })
            .await;
        let snapshot: serde_json::Value = serde_json::from_str(&read.content).unwrap();
        let delete = ToolInvocation {
            id: "5".into(),
            name: "delete_file".into(),
            arguments: serde_json::json!({
                "path":"src/sort.py",
                "expected_sha256":snapshot["sha256"]
            })
            .to_string(),
        };
        assert!(
            tools
                .approval_preview(&delete)
                .await
                .unwrap()
                .unwrap()
                .contains("/dev/null")
        );
        let deleted = tools.execute(&delete).await;
        assert!(!deleted.is_error, "{}", deleted.content);
        assert_eq!(deleted.metadata.as_ref().unwrap()["type"], "delete");
        assert!(!root.join("src/sort.py").exists());
    }
}
