use std::fmt;

use thiserror::Error;

/// A validated Mistral API key whose debug representation never exposes the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SettingsError> {
        let value = value.into();
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            return Err(SettingsError::InvalidApiKey);
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// Whether credential resolution may ask the user for missing input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionPolicy {
    Allow,
    Deny,
}

/// Boundary implemented by the CLI so settings logic remains terminal-agnostic.
pub trait CredentialPrompt {
    fn prompt_api_key(&mut self) -> Result<String, SettingsError>;
}

/// Resolves a session-scoped Mistral API key.
///
/// A non-empty environment value always wins. The prompt is invoked at most once and only when
/// interaction is explicitly allowed.
pub fn resolve_api_key(
    environment_value: Option<String>,
    interaction: InteractionPolicy,
    prompt: &mut dyn CredentialPrompt,
) -> Result<ApiKey, SettingsError> {
    if let Some(value) = environment_value.filter(|value| !value.trim().is_empty()) {
        return ApiKey::new(value);
    }

    if interaction == InteractionPolicy::Deny {
        return Err(SettingsError::PromptDisabled);
    }

    ApiKey::new(prompt.prompt_api_key()?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MistralModelConfig {
    pub chat_model: String,
    pub fim_model: String,
    pub utility_model: String,
}

impl MistralModelConfig {
    pub fn from_env() -> Result<Self, SettingsError> {
        let chat_model = required("LE_HARNESS_CHAT_MODEL")?;
        Ok(Self {
            utility_model: std::env::var("LE_HARNESS_UTILITY_MODEL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| chat_model.clone()),
            chat_model,
            fim_model: required("LE_HARNESS_FIM_MODEL")?,
        })
    }
}

#[derive(Clone)]
pub struct MistralSettings {
    pub api_key: ApiKey,
    pub chat_model: String,
    pub fim_model: String,
    pub utility_model: String,
}

impl fmt::Debug for MistralSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MistralSettings")
            .field("api_key", &self.api_key)
            .field("chat_model", &self.chat_model)
            .field("fim_model", &self.fim_model)
            .field("utility_model", &self.utility_model)
            .finish()
    }
}

impl MistralSettings {
    pub fn from_env() -> Result<Self, SettingsError> {
        let api_key = ApiKey::new(required("MISTRAL_API_KEY")?)?;
        Self::from_env_with_api_key(api_key)
    }

    pub fn from_env_with_api_key(api_key: ApiKey) -> Result<Self, SettingsError> {
        Ok(Self::from_parts(api_key, MistralModelConfig::from_env()?))
    }

    pub fn from_parts(api_key: ApiKey, models: MistralModelConfig) -> Self {
        Self {
            api_key,
            chat_model: models.chat_model,
            fim_model: models.fim_model,
            utility_model: models.utility_model,
        }
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

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("required environment variable {0} is missing or empty")]
    Missing(&'static str),
    #[error("Mistral API key must not be empty or contain control characters")]
    InvalidApiKey,
    #[error(
        "MISTRAL_API_KEY is missing and interactive input is unavailable; set the environment variable or run in a terminal without --no-input/--json"
    )]
    PromptDisabled,
    #[error("Mistral API key input was cancelled")]
    InputCancelled,
    #[error("could not read Mistral API key: {0}")]
    PromptFailed(String),
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

    struct StubPrompt {
        value: Result<String, SettingsError>,
        calls: usize,
    }

    impl StubPrompt {
        fn returning(value: &str) -> Self {
            Self {
                value: Ok(value.into()),
                calls: 0,
            }
        }
    }

    impl CredentialPrompt for StubPrompt {
        fn prompt_api_key(&mut self) -> Result<String, SettingsError> {
            self.calls += 1;
            self.value.clone()
        }
    }

    #[test]
    fn environment_key_takes_precedence_without_prompting() {
        let mut prompt = StubPrompt::returning("prompt-key");
        let key = resolve_api_key(
            Some("environment-key".into()),
            InteractionPolicy::Allow,
            &mut prompt,
        )
        .unwrap();

        assert_eq!(key.expose(), "environment-key");
        assert_eq!(prompt.calls, 0);
    }

    #[test]
    fn blank_environment_key_prompts_once() {
        let mut prompt = StubPrompt::returning("prompt-key");
        let key =
            resolve_api_key(Some("  ".into()), InteractionPolicy::Allow, &mut prompt).unwrap();

        assert_eq!(key.expose(), "prompt-key");
        assert_eq!(prompt.calls, 1);
    }

    #[test]
    fn disabled_interaction_fails_without_prompting() {
        let mut prompt = StubPrompt::returning("prompt-key");
        let error = resolve_api_key(None, InteractionPolicy::Deny, &mut prompt).unwrap_err();

        assert_eq!(error, SettingsError::PromptDisabled);
        assert_eq!(prompt.calls, 0);
    }

    #[test]
    fn invalid_prompt_value_is_rejected() {
        let mut prompt = StubPrompt::returning(" \n ");
        let error = resolve_api_key(None, InteractionPolicy::Allow, &mut prompt).unwrap_err();

        assert_eq!(error, SettingsError::InvalidApiKey);
        assert_eq!(prompt.calls, 1);
    }

    #[test]
    fn prompt_error_is_preserved() {
        let expected = SettingsError::InputCancelled;
        let mut prompt = StubPrompt {
            value: Err(expected.clone()),
            calls: 0,
        };
        let error = resolve_api_key(None, InteractionPolicy::Allow, &mut prompt).unwrap_err();

        assert_eq!(error, expected);
        assert_eq!(prompt.calls, 1);
    }

    #[test]
    fn debug_redacts_api_key() {
        let settings = MistralSettings {
            api_key: ApiKey::new("secret-value").unwrap(),
            chat_model: "chat".into(),
            fim_model: "fim".into(),
            utility_model: "utility".into(),
        };
        let rendered = format!("{settings:?}");
        assert!(!rendered.contains("secret-value"));
        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("chat"));
    }

    #[test]
    fn api_key_debug_is_redacted() {
        let key = ApiKey::new("secret-value").unwrap();
        assert_eq!(format!("{key:?}"), "[REDACTED]");
    }
}
