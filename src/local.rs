use std::net::IpAddr;

use async_trait::async_trait;
use le_rust::Client;
use url::Url;

use crate::model::{ChatTurn, MistralAdapter, ModelAdapter, ModelError, ModelEventStream};

#[derive(Debug, Clone)]
pub struct LocalModelConfig {
    pub base_url: String,
    pub chat_model: String,
    pub fim_model: Option<String>,
    pub api_key: Option<String>,
    pub tool_calling: bool,
}

#[derive(Clone)]
pub struct LocalOpenAiAdapter {
    inner: MistralAdapter,
    tool_calling: bool,
    fim: bool,
}

impl LocalOpenAiAdapter {
    pub fn new(config: LocalModelConfig) -> Result<Self, ModelError> {
        validate_loopback(&config.base_url)?;
        if config.chat_model.trim().is_empty() {
            return Err(ModelError::MissingCapability("local chat model identifier"));
        }
        let fim = config.fim_model.is_some();
        let fim_model = config
            .fim_model
            .clone()
            .unwrap_or_else(|| config.chat_model.clone());
        let client = Client::builder()
            .api_key(config.api_key.unwrap_or_else(|| "local-only".into()))
            .base_url(&config.base_url)
            .allow_insecure_loopback(true)
            .build()?;
        Ok(Self {
            inner: MistralAdapter::from_client(client, config.chat_model, fim_model),
            tool_calling: config.tool_calling,
            fim,
        })
    }
}

#[async_trait]
impl ModelAdapter for LocalOpenAiAdapter {
    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        self.inner.list_models().await
    }

    async fn stream_chat(&self, mut turn: ChatTurn) -> Result<ModelEventStream, ModelError> {
        if !self.tool_calling && !turn.tools.is_empty() {
            turn.tools.clear();
        }
        self.inner.stream_chat(turn).await
    }

    async fn complete_fim(&self, prompt: String, suffix: String) -> Result<String, ModelError> {
        if !self.fim {
            return Err(ModelError::MissingCapability("local fill in the middle"));
        }
        self.inner.complete_fim(prompt, suffix).await
    }
}

fn validate_loopback(base_url: &str) -> Result<(), ModelError> {
    let url = Url::parse(base_url).map_err(|_| ModelError::MissingCapability("valid local URL"))?;
    let host = url
        .host_str()
        .ok_or(ModelError::MissingCapability("local URL host"))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !loopback {
        return Err(ModelError::MissingCapability(
            "loopback-only local endpoint",
        ));
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ModelError::MissingCapability("HTTP local endpoint"));
    }
    if !url.path().ends_with("/v1/") {
        return Err(ModelError::MissingCapability(
            "local base URL ending in /v1/",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback_and_rejects_remote_hosts() {
        assert!(validate_loopback("http://127.0.0.1:11434/v1/").is_ok());
        assert!(validate_loopback("http://[::1]:8000/v1/").is_ok());
        assert!(validate_loopback("https://localhost/v1/").is_ok());
        assert!(validate_loopback("https://models.example.com/v1/").is_err());
        assert!(validate_loopback("http://127.0.0.1:11434/").is_err());
    }
}
