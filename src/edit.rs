use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::TextDiff;
use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EditError {
    #[error("edit path is invalid")]
    InvalidPath,
    #[error("edit path escapes the repository")]
    OutsideRepository,
    #[error("edit target is not a regular file")]
    NotFile,
    #[error("edit target is not UTF-8 text")]
    Binary,
    #[error("edit span {start}..{end} is invalid for a {length}-byte file")]
    InvalidSpan {
        start: usize,
        end: usize,
        length: usize,
    },
    #[error("edit span is not on UTF-8 character boundaries")]
    InvalidCharacterBoundary,
    #[error("snapshot conflict for {path}: expected {expected}, found {actual}")]
    Conflict {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("edit I/O failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub sha256: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanEdit {
    pub path: String,
    pub expected_sha256: String,
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditPreview {
    pub request: SpanEdit,
    pub candidate_sha256: String,
    pub candidate: String,
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedEdit {
    pub path: String,
    pub previous_sha256: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSetOutcome {
    pub applied: Vec<AppliedEdit>,
    pub failure: Option<ChangeFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeFailure {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEditor {
    root: PathBuf,
}

impl WorkspaceEditor {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, EditError> {
        let root = root.as_ref().canonicalize()?;
        if !root.is_dir() {
            return Err(EditError::InvalidPath);
        }
        Ok(Self { root })
    }

    pub fn snapshot(&self, path: impl AsRef<Path>) -> Result<FileSnapshot, EditError> {
        let (resolved, display) = self.resolve(path.as_ref())?;
        let bytes = fs::read(resolved)?;
        let content = String::from_utf8(bytes).map_err(|_| EditError::Binary)?;
        Ok(FileSnapshot {
            path: display,
            sha256: hash(content.as_bytes()),
            content,
        })
    }

    pub fn preview(&self, request: SpanEdit) -> Result<EditPreview, EditError> {
        let snapshot = self.snapshot(&request.path)?;
        ensure_hash(&request.path, &request.expected_sha256, &snapshot.sha256)?;
        if request.start > request.end || request.end > snapshot.content.len() {
            return Err(EditError::InvalidSpan {
                start: request.start,
                end: request.end,
                length: snapshot.content.len(),
            });
        }
        if !snapshot.content.is_char_boundary(request.start)
            || !snapshot.content.is_char_boundary(request.end)
        {
            return Err(EditError::InvalidCharacterBoundary);
        }
        let mut candidate = String::with_capacity(
            snapshot.content.len() - (request.end - request.start) + request.replacement.len(),
        );
        candidate.push_str(&snapshot.content[..request.start]);
        candidate.push_str(&request.replacement);
        candidate.push_str(&snapshot.content[request.end..]);
        let diff = TextDiff::from_lines(&snapshot.content, &candidate)
            .unified_diff()
            .header(
                &format!("a/{}", snapshot.path),
                &format!("b/{}", snapshot.path),
            )
            .to_string();
        Ok(EditPreview {
            candidate_sha256: hash(candidate.as_bytes()),
            request,
            candidate,
            diff,
        })
    }

    pub fn apply(&self, preview: &EditPreview) -> Result<AppliedEdit, EditError> {
        let (path, display) = self.resolve(Path::new(&preview.request.path))?;
        let current = fs::read(&path)?;
        let actual = hash(&current);
        ensure_hash(&display, &preview.request.expected_sha256, &actual)?;
        let permissions = fs::metadata(&path)?.permissions();
        let parent = path.parent().ok_or(EditError::InvalidPath)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.as_file_mut().set_permissions(permissions)?;
        temporary.write_all(preview.candidate.as_bytes())?;
        temporary.as_file_mut().sync_all()?;
        temporary.persist(&path).map_err(|error| error.error)?;
        sync_directory(parent)?;
        Ok(AppliedEdit {
            path: display,
            previous_sha256: actual,
            sha256: preview.candidate_sha256.clone(),
        })
    }

    pub fn apply_change_set(&self, previews: &[EditPreview]) -> ChangeSetOutcome {
        let mut applied = Vec::new();
        for preview in previews {
            match self.apply(preview) {
                Ok(edit) => applied.push(edit),
                Err(error) => {
                    return ChangeSetOutcome {
                        applied,
                        failure: Some(ChangeFailure {
                            path: preview.request.path.clone(),
                            message: error.to_string(),
                        }),
                    };
                }
            }
        }
        ChangeSetOutcome {
            applied,
            failure: None,
        }
    }

    fn resolve(&self, relative: &Path) -> Result<(PathBuf, String), EditError> {
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(EditError::InvalidPath);
        }
        let path = self.root.join(relative).canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(EditError::OutsideRepository);
        }
        if !path.is_file() {
            return Err(EditError::NotFile);
        }
        Ok((path, relative.to_string_lossy().replace('\\', "/")))
    }
}

fn ensure_hash(path: &str, expected: &str, actual: &str) -> Result<(), EditError> {
    if expected == actual {
        Ok(())
    } else {
        Err(EditError::Conflict {
            path: path.into(),
            expected: expected.into(),
            actual: actual.into(),
        })
    }
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), io::Error> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup(name: &str, content: &str) -> (tempfile::TempDir, WorkspaceEditor, FileSnapshot) {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(name), content).unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        let snapshot = editor.snapshot(name).unwrap();
        (directory, editor, snapshot)
    }

    #[test]
    fn previews_and_applies_an_exact_span() {
        let (directory, editor, snapshot) = setup("hello.rs", "fn old() {}\n");
        let preview = editor
            .preview(SpanEdit {
                path: snapshot.path,
                expected_sha256: snapshot.sha256,
                start: 3,
                end: 6,
                replacement: "new".into(),
            })
            .unwrap();
        assert!(preview.diff.contains("-fn old()"));
        assert!(preview.diff.contains("+fn new()"));
        editor.apply(&preview).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("hello.rs")).unwrap(),
            "fn new() {}\n"
        );
    }

    #[test]
    fn preserves_crlf_outside_the_span() {
        let (directory, editor, snapshot) = setup("file.txt", "one\r\ntwo\r\n");
        let preview = editor
            .preview(SpanEdit {
                path: snapshot.path,
                expected_sha256: snapshot.sha256,
                start: 5,
                end: 8,
                replacement: "TWO".into(),
            })
            .unwrap();
        editor.apply(&preview).unwrap();
        assert_eq!(
            fs::read(directory.path().join("file.txt")).unwrap(),
            b"one\r\nTWO\r\n"
        );
    }

    #[test]
    fn rejects_stale_snapshots_without_overwriting() {
        let (directory, editor, snapshot) = setup("file.txt", "first");
        let preview = editor
            .preview(SpanEdit {
                path: snapshot.path,
                expected_sha256: snapshot.sha256,
                start: 0,
                end: 5,
                replacement: "second".into(),
            })
            .unwrap();
        fs::write(directory.path().join("file.txt"), "human").unwrap();
        assert!(matches!(
            editor.apply(&preview),
            Err(EditError::Conflict { .. })
        ));
        assert_eq!(
            fs::read_to_string(directory.path().join("file.txt")).unwrap(),
            "human"
        );
    }

    #[test]
    fn rejects_non_character_boundaries() {
        let (_directory, editor, snapshot) = setup("utf8.txt", "é");
        assert!(matches!(
            editor.preview(SpanEdit {
                path: snapshot.path,
                expected_sha256: snapshot.sha256,
                start: 1,
                end: 2,
                replacement: "x".into(),
            }),
            Err(EditError::InvalidCharacterBoundary)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn preserves_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (directory, editor, snapshot) = setup("script", "old");
        fs::set_permissions(
            directory.path().join("script"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let preview = editor
            .preview(SpanEdit {
                path: snapshot.path,
                expected_sha256: snapshot.sha256,
                start: 0,
                end: 3,
                replacement: "new".into(),
            })
            .unwrap();
        editor.apply(&preview).unwrap();
        assert_eq!(
            fs::metadata(directory.path().join("script"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }

    #[test]
    fn change_set_reports_visible_partial_application() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("a"), "A").unwrap();
        fs::write(directory.path().join("b"), "B").unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        let a = editor.snapshot("a").unwrap();
        let b = editor.snapshot("b").unwrap();
        let first = editor
            .preview(SpanEdit {
                path: a.path,
                expected_sha256: a.sha256,
                start: 0,
                end: 1,
                replacement: "1".into(),
            })
            .unwrap();
        let second = editor
            .preview(SpanEdit {
                path: b.path,
                expected_sha256: b.sha256,
                start: 0,
                end: 1,
                replacement: "2".into(),
            })
            .unwrap();
        fs::write(directory.path().join("b"), "human").unwrap();
        let outcome = editor.apply_change_set(&[first, second]);
        assert_eq!(outcome.applied.len(), 1);
        assert_eq!(outcome.failure.unwrap().path, "b");
        assert_eq!(fs::read_to_string(directory.path().join("a")).unwrap(), "1");
        assert_eq!(
            fs::read_to_string(directory.path().join("b")).unwrap(),
            "human"
        );
    }
}
