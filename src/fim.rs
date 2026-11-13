use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::edit::{AppliedEdit, EditError, SpanEdit, WorkspaceEditor};
use crate::model::ModelAdapter;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FimEditRequest {
    pub path: String,
    pub expected_sha256: String,
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub fallback_replacement: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditStrategy {
    Fim,
    ConventionalFallback,
}

#[derive(Debug, Clone, Serialize)]
pub struct FimEditOutcome {
    pub strategy: EditStrategy,
    pub applied: AppliedEdit,
    pub diff: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

#[async_trait]
pub trait FimService: Send + Sync {
    async fn apply_fim(&self, request: FimEditRequest) -> Result<FimEditOutcome, String>;
}

pub struct FimEditEngine<A> {
    model: A,
    editor: WorkspaceEditor,
    max_generated_bytes: usize,
}

impl<A> FimEditEngine<A> {
    pub fn new(model: A, editor: WorkspaceEditor, max_generated_bytes: usize) -> Self {
        Self {
            model,
            editor,
            max_generated_bytes,
        }
    }
}

#[async_trait]
impl<A: ModelAdapter> FimService for FimEditEngine<A> {
    async fn apply_fim(&self, request: FimEditRequest) -> Result<FimEditOutcome, String> {
        let snapshot = self
            .editor
            .snapshot(&request.path)
            .map_err(|error| error.to_string())?;
        validate_span(&snapshot.content, request.start, request.end)
            .map_err(|error| error.to_string())?;
        if snapshot.sha256 != request.expected_sha256 {
            return Err(EditError::Conflict {
                path: request.path,
                expected: request.expected_sha256,
                actual: snapshot.sha256,
            }
            .to_string());
        }
        let prefix = snapshot.content[..request.start].to_owned();
        let suffix = snapshot.content[request.end..].to_owned();
        let generated = self.model.complete_fim(prefix, suffix).await;
        let (replacement, strategy, fallback_reason) = match generated {
            Ok(generated) => match validate_generated(generated, self.max_generated_bytes) {
                Ok(generated) => (generated, EditStrategy::Fim, None),
                Err(reason) => fallback(&request, reason)?,
            },
            Err(error) => fallback(&request, error.to_string())?,
        };
        let preview = self
            .editor
            .preview(SpanEdit {
                path: snapshot.path,
                expected_sha256: snapshot.sha256,
                start: request.start,
                end: request.end,
                replacement,
            })
            .map_err(|error| error.to_string())?;
        let diff = preview.diff.clone();
        let applied = self
            .editor
            .apply(&preview)
            .map_err(|error| error.to_string())?;
        Ok(FimEditOutcome {
            strategy,
            applied,
            diff,
            fallback_reason,
        })
    }
}

fn validate_span(content: &str, start: usize, end: usize) -> Result<(), EditError> {
    if start > end || end > content.len() {
        return Err(EditError::InvalidSpan {
            start,
            end,
            length: content.len(),
        });
    }
    if !content.is_char_boundary(start) || !content.is_char_boundary(end) {
        return Err(EditError::InvalidCharacterBoundary);
    }
    Ok(())
}

fn validate_generated(generated: String, max_bytes: usize) -> Result<String, String> {
    if generated.len() > max_bytes {
        return Err(format!(
            "FIM middle exceeded the {max_bytes}-byte generation limit"
        ));
    }
    if generated.contains('\0') {
        return Err("FIM middle contained a NUL byte".into());
    }
    if generated.trim_start().starts_with("```") {
        return Err("FIM middle was wrapped in a Markdown code fence".into());
    }
    Ok(generated)
}

fn fallback(
    request: &FimEditRequest,
    reason: String,
) -> Result<(String, EditStrategy, Option<String>), String> {
    match &request.fallback_replacement {
        Some(replacement) => Ok((
            replacement.clone(),
            EditStrategy::ConventionalFallback,
            Some(reason),
        )),
        None => Err(format!(
            "FIM edit failed and no conventional fallback was supplied: {reason}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScriptedModelAdapter;
    use std::fs;
    use tempfile::tempdir;

    fn request(path: &str, hash: String) -> FimEditRequest {
        FimEditRequest {
            path: path.into(),
            expected_sha256: hash,
            start: 3,
            end: 6,
            fallback_replacement: None,
        }
    }

    #[tokio::test]
    async fn applies_fim_middle_through_transaction_engine() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("code.rs"), "fn old() {}\n").unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        let snapshot = editor.snapshot("code.rs").unwrap();
        let model = ScriptedModelAdapter::default();
        model.push_fim("new").await;
        let outcome = FimEditEngine::new(model, editor, 100)
            .apply_fim(request("code.rs", snapshot.sha256))
            .await
            .unwrap();
        assert_eq!(outcome.strategy, EditStrategy::Fim);
        assert_eq!(
            fs::read_to_string(directory.path().join("code.rs")).unwrap(),
            "fn new() {}\n"
        );
    }

    #[tokio::test]
    async fn records_conventional_fallback_reason() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("code.rs"), "fn old() {}\n").unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        let snapshot = editor.snapshot("code.rs").unwrap();
        let mut edit = request("code.rs", snapshot.sha256);
        edit.fallback_replacement = Some("safe".into());
        let outcome = FimEditEngine::new(ScriptedModelAdapter::default(), editor, 100)
            .apply_fim(edit)
            .await
            .unwrap();
        assert_eq!(outcome.strategy, EditStrategy::ConventionalFallback);
        assert!(outcome.fallback_reason.is_some());
    }

    #[tokio::test]
    async fn rejects_stale_snapshot_before_requesting_fim() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("code.rs"), "fn old() {}\n").unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        let error = FimEditEngine::new(ScriptedModelAdapter::default(), editor, 100)
            .apply_fim(request("code.rs", "stale".into()))
            .await
            .unwrap_err();
        assert!(error.contains("snapshot conflict"));
    }
}
