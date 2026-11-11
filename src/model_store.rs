use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::settings::{MistralModelConfig, SettingsError};

const SCHEMA_VERSION: u16 = 1;

/// Non-secret cloud model choices, stored separately from the API credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub chat: String,
    pub fim: String,
}

#[derive(Debug, Clone)]
pub struct ModelStore {
    path: PathBuf,
}

#[derive(Debug, Error)]
pub enum ModelStoreError {
    #[error("model preference I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("model preference JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported model preference schema version {0}")]
    Schema(u16),
    #[error("saved model choice is invalid: {0}")]
    Model(#[from] SettingsError),
}

#[derive(Serialize, Deserialize)]
struct StoredModels {
    schema_version: u16,
    chat: String,
    fim: String,
}

impl ModelStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Option<PathBuf> {
        if let Some(root) = std::env::var_os("LE_HARNESS_CONFIG_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            return Some(root.join("models.json"));
        }
        if let Some(root) = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            return Some(root.join("le-harness/models.json"));
        }
        if cfg!(windows) {
            if let Some(root) = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
            {
                return Some(root.join("le-harness/models.json"));
            }
        }
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(".config/le-harness/models.json"))
    }

    pub fn load(&self) -> Result<Option<ModelSelection>, ModelStoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let saved: StoredModels = serde_json::from_slice(&bytes)?;
        if saved.schema_version != SCHEMA_VERSION {
            return Err(ModelStoreError::Schema(saved.schema_version));
        }
        MistralModelConfig::from_values(&saved.chat, &saved.fim, None)?;
        Ok(Some(ModelSelection {
            chat: saved.chat,
            fim: saved.fim,
        }))
    }

    pub fn save(&self, selection: &ModelSelection) -> Result<(), ModelStoreError> {
        MistralModelConfig::from_values(&selection.chat, &selection.fim, None)?;
        let parent = self.path.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "model store has no parent",
            )
        })?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(
            &mut temporary,
            &StoredModels {
                schema_version: SCHEMA_VERSION,
                chat: selection.chat.clone(),
                fim: selection.fim.clone(),
            },
        )?;
        temporary.as_file_mut().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_reloads_both_models() {
        let directory = tempfile::tempdir().unwrap();
        let store = ModelStore::new(directory.path().join("models.json"));
        assert!(store.load().unwrap().is_none());
        let selection = ModelSelection {
            chat: "codestral-latest".into(),
            fim: "mistral-code-fim-latest".into(),
        };
        store.save(&selection).unwrap();
        assert_eq!(store.load().unwrap(), Some(selection));
    }

    #[test]
    fn invalid_saved_model_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        fs::write(
            &path,
            r#"{"schema_version":1,"chat":"bad\nmodel","fim":"codestral"}"#,
        )
        .unwrap();
        assert!(matches!(
            ModelStore::new(path).load(),
            Err(ModelStoreError::Model(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        ModelStore::new(path.clone())
            .save(&ModelSelection {
                chat: "a".into(),
                fim: "b".into(),
            })
            .unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
