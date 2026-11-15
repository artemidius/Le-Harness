use std::fs::{self, File};
use std::io::{self, Read, Write};
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
    #[error("file already exists: {0}")]
    AlreadyExists(String),
    #[error("file operation exceeds the {limit}-byte limit")]
    TooLarge { limit: usize },
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
    #[error("replacement text was not found in {0}")]
    TextNotFound(String),
    #[error("replacement text occurs more than once in {0}")]
    AmbiguousText(String),
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
pub struct CreateFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedFile {
    pub path: String,
    pub sha256: String,
    pub created_directories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceTextRequest {
    pub path: String,
    pub expected_sha256: String,
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceFileRequest {
    pub path: String,
    pub expected_sha256: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFileRequest {
    pub path: String,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletedFile {
    pub path: String,
    pub previous_sha256: String,
    pub size: u64,
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

    pub fn preview_create_summary(&self, request: &CreateFileRequest) -> Result<String, EditError> {
        if request.content.len() > 1024 * 1024 {
            return Err(EditError::TooLarge { limit: 1024 * 1024 });
        }
        let relative = validate_relative(Path::new(&request.path))?;
        let destination = self.root.join(relative);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(EditError::AlreadyExists(request.path.clone()));
        }
        reject_non_directory_creation_components(&self.root, relative)?;
        let diff = TextDiff::from_lines("", &request.content)
            .unified_diff()
            .header("/dev/null", &format!("b/{}", path_text(relative)))
            .to_string();
        Ok(bounded_preview(&diff))
    }

    pub fn preview_delete_summary(&self, request: &DeleteFileRequest) -> Result<String, EditError> {
        const MAX_DELETE_BYTES: usize = 64 * 1024 * 1024;
        let (path, display) = self.resolve(Path::new(&request.path))?;
        let bytes = read_bounded_file(&path, MAX_DELETE_BYTES)?;
        let actual = hash(&bytes);
        ensure_hash(&display, &request.expected_sha256, &actual)?;
        match String::from_utf8(bytes) {
            Ok(content) => {
                let empty = String::new();
                let diff = TextDiff::from_lines(&content, &empty)
                    .unified_diff()
                    .header(&format!("a/{display}"), "/dev/null")
                    .to_string();
                Ok(format!(
                    "Deletion is permanent unless the file is recoverable from version control.\n{}",
                    bounded_preview(&diff)
                ))
            }
            Err(error) => Ok(format!(
                "Delete binary file {display} ({} bytes, sha256 {actual}). Recovery depends on version control.",
                error.as_bytes().len()
            )),
        }
    }

    /// Publish a complete new file without replacing an existing file or symlink.
    pub fn create(&self, request: CreateFileRequest) -> Result<CreatedFile, EditError> {
        const MAX_CREATE_BYTES: usize = 1024 * 1024;
        if request.content.len() > MAX_CREATE_BYTES {
            return Err(EditError::TooLarge {
                limit: MAX_CREATE_BYTES,
            });
        }
        let relative = validate_relative(Path::new(&request.path))?;
        let name = relative.file_name().ok_or(EditError::InvalidPath)?;
        let (parent, created_directories) =
            self.create_parent_directories(relative.parent().ok_or(EditError::InvalidPath)?)?;
        let destination = parent.join(name);
        if fs::symlink_metadata(&destination).is_ok() {
            rollback_directories(&created_directories);
            return Err(EditError::AlreadyExists(request.path));
        }
        let publish = (|| -> Result<(), EditError> {
            let mut temporary = NamedTempFile::new_in(&parent)?;
            temporary.write_all(request.content.as_bytes())?;
            temporary.as_file_mut().sync_all()?;
            temporary.persist_noclobber(&destination).map_err(|error| {
                if error.error.kind() == io::ErrorKind::AlreadyExists {
                    EditError::AlreadyExists(request.path.clone())
                } else {
                    EditError::Io(error.error)
                }
            })?;
            sync_directory(&parent)?;
            Ok(())
        })();
        if let Err(error) = publish {
            rollback_directories(&created_directories);
            return Err(error);
        }
        Ok(CreatedFile {
            path: request.path.replace('\\', "/"),
            sha256: hash(request.content.as_bytes()),
            created_directories: created_directories
                .iter()
                .filter_map(|path| path.strip_prefix(&self.root).ok())
                .map(path_text)
                .collect(),
        })
    }

    pub fn preview_text(&self, request: ReplaceTextRequest) -> Result<EditPreview, EditError> {
        if request.old_text.is_empty() {
            return Err(EditError::AmbiguousText(request.path));
        }
        let snapshot = self.snapshot(&request.path)?;
        ensure_hash(&request.path, &request.expected_sha256, &snapshot.sha256)?;
        let mut occurrences = snapshot.content.match_indices(&request.old_text);
        let (start, _) = occurrences
            .next()
            .ok_or_else(|| EditError::TextNotFound(request.path.clone()))?;
        if occurrences.next().is_some() {
            return Err(EditError::AmbiguousText(request.path));
        }
        self.preview(SpanEdit {
            path: snapshot.path,
            expected_sha256: snapshot.sha256,
            start,
            end: start + request.old_text.len(),
            replacement: request.new_text,
        })
    }

    pub fn preview_file(&self, request: ReplaceFileRequest) -> Result<EditPreview, EditError> {
        const MAX_REPLACE_BYTES: usize = 1024 * 1024;
        if request.content.len() > MAX_REPLACE_BYTES {
            return Err(EditError::TooLarge {
                limit: MAX_REPLACE_BYTES,
            });
        }
        let snapshot = self.snapshot(&request.path)?;
        self.preview(SpanEdit {
            path: snapshot.path,
            expected_sha256: request.expected_sha256,
            start: 0,
            end: snapshot.content.len(),
            replacement: request.content,
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
        let latest = hash(&fs::read(&path)?);
        ensure_hash(&display, &preview.request.expected_sha256, &latest)?;
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

    pub fn delete(&self, request: DeleteFileRequest) -> Result<DeletedFile, EditError> {
        const MAX_DELETE_BYTES: usize = 64 * 1024 * 1024;
        let (path, display) = self.resolve(Path::new(&request.path))?;
        let bytes = read_bounded_file(&path, MAX_DELETE_BYTES)?;
        let actual = hash(&bytes);
        ensure_hash(&display, &request.expected_sha256, &actual)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() {
            return Err(EditError::NotFile);
        }
        let latest = hash(&read_bounded_file(&path, MAX_DELETE_BYTES)?);
        ensure_hash(&display, &request.expected_sha256, &latest)?;
        fs::remove_file(&path)?;
        sync_directory(path.parent().ok_or(EditError::InvalidPath)?)?;
        Ok(DeletedFile {
            path: display,
            previous_sha256: actual,
            size: bytes.len() as u64,
        })
    }

    fn create_parent_directories(
        &self,
        relative: &Path,
    ) -> Result<(PathBuf, Vec<PathBuf>), EditError> {
        let mut current = self.root.clone();
        let mut created = Vec::new();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                rollback_directories(&created);
                return Err(EditError::InvalidPath);
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    rollback_directories(&created);
                    return Err(EditError::OutsideRepository);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if let Err(error) = fs::create_dir(&current) {
                        rollback_directories(&created);
                        return Err(EditError::Io(error));
                    }
                    created.push(current.clone());
                }
                Err(error) => {
                    rollback_directories(&created);
                    return Err(EditError::Io(error));
                }
            }
        }
        let canonical = current.canonicalize().map_err(EditError::Io)?;
        if !canonical.starts_with(&self.root) {
            rollback_directories(&created);
            return Err(EditError::OutsideRepository);
        }
        Ok((canonical, created))
    }

    fn resolve(&self, relative: &Path) -> Result<(PathBuf, String), EditError> {
        let relative = validate_relative(relative)?;
        let joined = self.root.join(relative);
        reject_symlink_components(&self.root, relative)?;
        let path = joined.canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(EditError::OutsideRepository);
        }
        if !path.is_file() {
            return Err(EditError::NotFile);
        }
        Ok((path, relative.to_string_lossy().replace('\\', "/")))
    }
}

fn validate_relative(path: &Path) -> Result<&Path, EditError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(EditError::InvalidPath);
    }
    Ok(path)
}

fn reject_symlink_components(root: &Path, relative: &Path) -> Result<(), EditError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(EditError::InvalidPath);
        };
        current.push(name);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(EditError::OutsideRepository);
        }
    }
    Ok(())
}

fn reject_non_directory_creation_components(root: &Path, relative: &Path) -> Result<(), EditError> {
    let mut current = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(EditError::InvalidPath);
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    || (components.peek().is_some() && !metadata.is_dir()) =>
            {
                return Err(EditError::OutsideRepository);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(EditError::Io(error)),
        }
    }
    Ok(())
}

fn rollback_directories(created: &[PathBuf]) {
    for directory in created.iter().rev() {
        let _ = fs::remove_dir(directory);
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn bounded_preview(value: &str) -> String {
    const LIMIT: usize = 16 * 1024;
    if value.len() <= LIMIT {
        return value.to_owned();
    }
    let mut end = LIMIT;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... preview truncated ...", &value[..end])
}

fn read_bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>, EditError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > limit as u64 {
        return Err(EditError::TooLarge { limit });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(EditError::TooLarge { limit });
    }
    Ok(bytes)
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
    fn creates_a_new_file_without_replacing_existing_content() {
        let directory = tempdir().unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        let request = CreateFileRequest {
            path: "src/algorithms/sort.py".into(),
            content: "def sort(items):\n    return items\n".into(),
        };
        let created = editor.create(request.clone()).unwrap();
        assert_eq!(created.path, request.path);
        assert_eq!(created.sha256, hash(request.content.as_bytes()));
        assert_eq!(created.created_directories, ["src", "src/algorithms"]);
        assert_eq!(
            fs::read_to_string(directory.path().join("src/algorithms/sort.py")).unwrap(),
            request.content
        );
        assert!(matches!(
            editor.create(CreateFileRequest {
                content: "replacement".into(),
                ..request
            }),
            Err(EditError::AlreadyExists(_))
        ));
        assert_eq!(
            fs::read_to_string(directory.path().join("src/algorithms/sort.py")).unwrap(),
            "def sort(items):\n    return items\n"
        );
    }

    #[test]
    fn replaces_unique_text_and_rejects_ambiguous_or_stale_requests() {
        let (directory, editor, snapshot) = setup("file.txt", "alpha beta gamma\n");
        let preview = editor
            .preview_text(ReplaceTextRequest {
                path: snapshot.path.clone(),
                expected_sha256: snapshot.sha256.clone(),
                old_text: "beta".into(),
                new_text: "BETA".into(),
            })
            .unwrap();
        editor.apply(&preview).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("file.txt")).unwrap(),
            "alpha BETA gamma\n"
        );

        fs::write(directory.path().join("file.txt"), "same same").unwrap();
        let current = editor.snapshot("file.txt").unwrap();
        assert!(matches!(
            editor.preview_text(ReplaceTextRequest {
                path: current.path.clone(),
                expected_sha256: current.sha256,
                old_text: "same".into(),
                new_text: "new".into(),
            }),
            Err(EditError::AmbiguousText(_))
        ));
        assert!(matches!(
            editor.preview_file(ReplaceFileRequest {
                path: current.path,
                expected_sha256: snapshot.sha256,
                content: "replacement".into(),
            }),
            Err(EditError::Conflict { .. })
        ));
    }

    #[test]
    fn deletes_text_and_binary_files_only_with_the_current_hash() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("binary.dat"), [0, 1, 2, 3]).unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        let expected = hash(&[0, 1, 2, 3]);
        assert!(matches!(
            editor.delete(DeleteFileRequest {
                path: "binary.dat".into(),
                expected_sha256: "stale".into(),
            }),
            Err(EditError::Conflict { .. })
        ));
        let deleted = editor
            .delete(DeleteFileRequest {
                path: "binary.dat".into(),
                expected_sha256: expected.clone(),
            })
            .unwrap();
        assert_eq!(deleted.previous_sha256, expected);
        assert_eq!(deleted.size, 4);
        assert!(!directory.path().join("binary.dat").exists());
    }

    #[test]
    fn create_rejects_outside_paths_and_symlinked_parents() {
        let directory = tempdir().unwrap();
        #[cfg(unix)]
        let outside = tempdir().unwrap();
        let editor = WorkspaceEditor::open(directory.path()).unwrap();
        for path in ["../escape.py", "/tmp/escape.py"] {
            assert!(matches!(
                editor.create(CreateFileRequest {
                    path: path.into(),
                    content: "x".into()
                }),
                Err(EditError::InvalidPath)
            ));
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), directory.path().join("link")).unwrap();
            assert!(matches!(
                editor.create(CreateFileRequest {
                    path: "link/escape.py".into(),
                    content: "x".into()
                }),
                Err(EditError::OutsideRepository)
            ));
            assert!(!outside.path().join("escape.py").exists());

            let outside_file = outside.path().join("keep.txt");
            fs::write(&outside_file, "keep").unwrap();
            std::os::unix::fs::symlink(&outside_file, directory.path().join("linked-file"))
                .unwrap();
            assert!(matches!(
                editor.delete(DeleteFileRequest {
                    path: "linked-file".into(),
                    expected_sha256: hash(b"keep"),
                }),
                Err(EditError::OutsideRepository)
            ));
            assert_eq!(fs::read_to_string(outside_file).unwrap(), "keep");

            let oversized_name = "x".repeat(300);
            assert!(
                editor
                    .create(CreateFileRequest {
                        path: format!("temporary/parents/{oversized_name}"),
                        content: "x".into(),
                    })
                    .is_err()
            );
            assert!(!directory.path().join("temporary").exists());
        }
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
