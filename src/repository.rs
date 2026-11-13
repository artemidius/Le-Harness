use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::runtime::{ToolHandler, ToolInvocation, ToolResult};

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("repository root is unavailable: {0}")]
    Root(io::Error),
    #[error("path must be repository-relative and cannot contain parent traversal")]
    InvalidPath,
    #[error("path escapes the repository root")]
    OutsideRoot,
    #[error("path is not a regular file")]
    NotFile,
    #[error("file exceeds the {limit}-byte read limit")]
    TooLarge { limit: usize },
    #[error("file is binary or is not valid UTF-8")]
    Binary,
    #[error("repository I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("repository walk failed: {0}")]
    Walk(#[from] ignore::Error),
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub preview: String,
}

impl Repository {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RepositoryError> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(RepositoryError::Root)?;
        if !root.is_dir() {
            return Err(RepositoryError::Root(io::Error::new(
                io::ErrorKind::NotADirectory,
                "root is not a directory",
            )));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list_files(&self, max_files: usize) -> Result<Vec<String>, RepositoryError> {
        if max_files == 0 {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(false)
            .follow_links(false)
            .build();
        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&self.root)
                .map_err(|_| RepositoryError::OutsideRoot)?;
            paths.push(path_text(relative));
            if paths.len() >= max_files {
                break;
            }
        }
        paths.sort();
        Ok(paths)
    }

    pub fn read_text(
        &self,
        relative: impl AsRef<Path>,
        max_bytes: usize,
    ) -> Result<String, RepositoryError> {
        let path = self.resolve_file(relative.as_ref())?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > max_bytes as u64 {
            return Err(RepositoryError::TooLarge { limit: max_bytes });
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)?
            .take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > max_bytes {
            return Err(RepositoryError::TooLarge { limit: max_bytes });
        }
        if bytes.contains(&0) {
            return Err(RepositoryError::Binary);
        }
        String::from_utf8(bytes).map_err(|_| RepositoryError::Binary)
    }

    pub fn search_text(
        &self,
        query: &str,
        max_results: usize,
        max_file_bytes: usize,
    ) -> Result<Vec<SearchMatch>, RepositoryError> {
        if query.is_empty() || max_results == 0 {
            return Ok(Vec::new());
        }
        let mut matches = Vec::new();
        for path in self.list_files(100_000)? {
            let content = match self.read_text(&path, max_file_bytes) {
                Ok(content) => content,
                Err(RepositoryError::Binary | RepositoryError::TooLarge { .. }) => continue,
                Err(error) => return Err(error),
            };
            for (line_index, line) in content.lines().enumerate() {
                for (byte_index, _) in line.match_indices(query) {
                    matches.push(SearchMatch {
                        path: path.clone(),
                        line: line_index + 1,
                        column: line[..byte_index].chars().count() + 1,
                        preview: line.chars().take(240).collect(),
                    });
                    if matches.len() >= max_results {
                        return Ok(matches);
                    }
                }
            }
        }
        Ok(matches)
    }

    fn resolve_file(&self, relative: &Path) -> Result<PathBuf, RepositoryError> {
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(RepositoryError::InvalidPath);
        }
        let path = self.root.join(relative).canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(RepositoryError::OutsideRoot);
        }
        if !path.is_file() {
            return Err(RepositoryError::NotFile);
        }
        Ok(path)
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Clone)]
pub struct RepositoryToolHandler {
    repository: Repository,
    limits: RepositoryToolLimits,
}

#[derive(Debug, Clone, Copy)]
pub struct RepositoryToolLimits {
    pub max_files: usize,
    pub max_read_bytes: usize,
    pub max_search_results: usize,
}

impl Default for RepositoryToolLimits {
    fn default() -> Self {
        Self {
            max_files: 10_000,
            max_read_bytes: 1024 * 1024,
            max_search_results: 200,
        }
    }
}

impl RepositoryToolHandler {
    pub fn new(repository: Repository, limits: RepositoryToolLimits) -> Self {
        Self { repository, limits }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArguments {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    query: String,
}

#[async_trait]
impl ToolHandler for RepositoryToolHandler {
    async fn execute(&self, invocation: &ToolInvocation) -> ToolResult {
        let value = match invocation.name.as_str() {
            "list_files" => self
                .repository
                .list_files(self.limits.max_files)
                .and_then(json_value),
            "read_file" => {
                parse_arguments::<ReadArguments>(&invocation.arguments).and_then(|args| {
                    self.repository
                        .read_text(args.path, self.limits.max_read_bytes)
                        .and_then(json_value)
                })
            }
            "search" => {
                parse_arguments::<SearchArguments>(&invocation.arguments).and_then(|args| {
                    self.repository
                        .search_text(
                            &args.query,
                            self.limits.max_search_results,
                            self.limits.max_read_bytes,
                        )
                        .and_then(json_value)
                })
            }
            _ => return ToolResult::error(format!("unknown tool: {}", invocation.name)),
        };
        match value {
            Ok(value) => ToolResult::success(value),
            Err(error) => ToolResult::error(error.to_string()),
        }
    }
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(input: &str) -> Result<T, RepositoryError> {
    serde_json::from_str(input).map_err(RepositoryError::InvalidArguments)
}

fn json_value<T: Serialize>(value: T) -> Result<String, RepositoryError> {
    serde_json::to_string(&value)
        .map_err(|error| RepositoryError::Io(io::Error::new(io::ErrorKind::InvalidData, error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn fixture() -> (tempfile::TempDir, Repository) {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("keep.rs"), "fn main() {}\n").unwrap();
        fs::write(directory.path().join("ignored.log"), "secret\n").unwrap();
        fs::write(directory.path().join(".gitignore"), "*.log\n").unwrap();
        let repository = Repository::open(directory.path()).unwrap();
        (directory, repository)
    }

    #[test]
    fn lists_files_in_stable_order_and_honors_gitignore() {
        let (_directory, repository) = fixture();
        assert_eq!(
            repository.list_files(10).unwrap(),
            vec![".gitignore", "keep.rs"]
        );
    }

    #[test]
    fn rejects_parent_traversal() {
        let (_directory, repository) = fixture();
        assert!(matches!(
            repository.read_text("../outside", 100),
            Err(RepositoryError::InvalidPath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_that_escape_the_root() {
        use std::os::unix::fs::symlink;

        let (directory, repository) = fixture();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("outside.txt"), "nope").unwrap();
        symlink(
            outside.path().join("outside.txt"),
            directory.path().join("escape"),
        )
        .unwrap();
        assert!(matches!(
            repository.read_text("escape", 100),
            Err(RepositoryError::OutsideRoot)
        ));
    }

    #[test]
    fn enforces_size_and_text_limits() {
        let (directory, repository) = fixture();
        fs::write(directory.path().join("large.txt"), "12345").unwrap();
        fs::write(directory.path().join("binary"), [0, 1, 2]).unwrap();
        assert!(matches!(
            repository.read_text("large.txt", 4),
            Err(RepositoryError::TooLarge { .. })
        ));
        assert!(matches!(
            repository.read_text("binary", 4),
            Err(RepositoryError::Binary)
        ));
    }

    #[test]
    fn searches_literal_text_with_character_columns() {
        let (directory, repository) = fixture();
        fs::write(directory.path().join("unicode.txt"), "é needle needle\n").unwrap();
        let matches = repository.search_text("needle", 10, 100).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].column, 3);
        assert_eq!(matches[1].column, 10);
    }

    #[tokio::test]
    async fn typed_tool_handler_rejects_unknown_arguments() {
        let (_directory, repository) = fixture();
        let handler = RepositoryToolHandler::new(repository, RepositoryToolLimits::default());
        let result = handler
            .execute(&ToolInvocation {
                id: "1".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"keep.rs","extra":true}"#.into(),
            })
            .await;
        assert!(result.is_error);
    }
}
