use std::fmt;

use thiserror::Error;

#[derive(Clone)]
pub struct MistralSettings {
    pub api_key: String,
    pub chat_model: String,
    pub fim_model: String,
    pub utility_model: String,
}

impl fmt::Debug for MistralSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MistralSettings")
            .field("api_key", &"[REDACTED]")
            .field("chat_model", &self.chat_model)
            .field("fim_model", &self.fim_model)
            .field("utility_model", &self.utility_model)
            .finish()
    }
}

impl MistralSettings {
    pub fn from_env() -> Result<Self, SettingsError> {
        let chat_model = required("LE_HARNESS_CHAT_MODEL")?;
        Ok(Self {
            api_key: required("MISTRAL_API_KEY")?,
            utility_model: std::env::var("LE_HARNESS_UTILITY_MODEL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| chat_model.clone()),
            chat_model,
            fim_model: required("LE_HARNESS_FIM_MODEL")?,
        })
    }

    pub fn diagnostic() -> SettingsDiagnostic {
        SettingsDiagnostic {
            api_key: present("MISTRAL_API_KEY"),
            chat_model: std::env::var("LE_HARNESS_CHAT_MODEL").ok(),
            fim_model: std::env::var("LE_HARNESS_FIM_MODEL").ok(),
            utility_model: std::env::var("LE_HARNESS_UTILITY_MODEL").ok(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsDiagnostic {
    pub api_key: bool,
    pub chat_model: Option<String>,
    pub fim_model: Option<String>,
    pub utility_model: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("required environment variable {0} is missing or empty")]
    Missing(&'static str),
}

fn required(name: &'static str) -> Result<String, SettingsError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(SettingsError::Missing(name))
}

fn present(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_api_key() {
        let settings = MistralSettings {
            api_key: "secret-value".into(),
            chat_model: "chat".into(),
            fim_model: "fim".into(),
            utility_model: "utility".into(),
        };
        let rendered = format!("{settings:?}");
        assert!(!rendered.contains("secret-value"));
        assert!(rendered.contains("chat"));
    }
}
