use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use futures_util::{StreamExt, stream};
use le_rust::chat::{
    ChatRequest, CompletionTextError, Content, FunctionDefinition, Message, Tool, ToolArguments,
    ToolChoice,
};
use le_rust::fim::FimRequest;
use le_rust::models::ListModelsOptions;
use le_rust::{Client, RequestOptions};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::settings::{ApiKey, MistralSettings};

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send + 'static>>;

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub role: ModelRole,
}

impl ChatTurn {
    pub fn user(prompt: impl Into<String>) -> Self {
        Self {
            messages: vec![Message::user(prompt.into())],
            tools: coding_tools(),
            role: ModelRole::Planner,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Planner,
    Utility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelEvent {
    Text {
        choice: usize,
        text: String,
    },
    ToolCallDelta {
        choice: usize,
        index: Option<usize>,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    Finished {
        choice: usize,
        reason: String,
    },
    Usage {
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        total_tokens: Option<u64>,
    },
    Routed {
        role: String,
        backend: String,
        fallback: bool,
    },
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("Mistral request failed: {0}")]
    Mistral(#[from] le_rust::Error),
    #[error("Mistral completion did not contain usable text: {0}")]
    CompletionText(#[from] CompletionTextError),
    #[error("scripted model has no response for this request")]
    ScriptExhausted,
    #[error("model route budget exhausted for {0}")]
    RouteBudget(&'static str),
    #[error("configured route lacks required capability: {0}")]
    MissingCapability(&'static str),
}

#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn list_models(&self) -> Result<Vec<String>, ModelError>;
    async fn stream_chat(&self, turn: ChatTurn) -> Result<ModelEventStream, ModelError>;
    async fn complete_fim(&self, prompt: String, suffix: String) -> Result<String, ModelError>;
}

#[async_trait]
impl<T: ModelAdapter + ?Sized> ModelAdapter for Arc<T> {
    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        (**self).list_models().await
    }

    async fn stream_chat(&self, turn: ChatTurn) -> Result<ModelEventStream, ModelError> {
        (**self).stream_chat(turn).await
    }

    async fn complete_fim(&self, prompt: String, suffix: String) -> Result<String, ModelError> {
        (**self).complete_fim(prompt, suffix).await
    }
}

#[derive(Clone)]
pub struct MistralAdapter {
    client: Client,
    chat_model: String,
    fim_model: String,
}

impl MistralAdapter {
    pub fn from_settings(settings: &MistralSettings) -> Result<Self, ModelError> {
        Self::from_credentials(
            &settings.api_key,
            settings.chat_model.clone(),
            settings.fim_model.clone(),
            None,
        )
    }

    pub fn from_credentials(
        api_key: &ApiKey,
        chat_model: impl Into<String>,
        fim_model: impl Into<String>,
        base_url: Option<&str>,
    ) -> Result<Self, ModelError> {
        let mut builder = Client::builder().api_key(api_key.expose());
        if let Some(base_url) = base_url {
            builder = builder.base_url(base_url).allow_insecure_loopback(true);
        }
        let client = builder.build()?;
        Ok(Self {
            client,
            chat_model: chat_model.into(),
            fim_model: fim_model.into(),
        })
    }

    pub fn from_client(
        client: Client,
        chat_model: impl Into<String>,
        fim_model: impl Into<String>,
    ) -> Self {
        Self {
            client,
            chat_model: chat_model.into(),
            fim_model: fim_model.into(),
        }
    }

    pub fn from_settings_with_chat(
        settings: &MistralSettings,
        chat_model: impl Into<String>,
    ) -> Result<Self, ModelError> {
        let mut adapter = Self::from_settings(settings)?;
        adapter.chat_model = chat_model.into();
        Ok(adapter)
    }
}

#[async_trait]
impl ModelAdapter for MistralAdapter {
    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        let response = self
            .client
            .models()
            .list(ListModelsOptions::default())
            .await?;
        Ok(response
            .data
            .data
            .into_iter()
            .map(|model| model.id)
            .collect())
    }

    async fn stream_chat(&self, turn: ChatTurn) -> Result<ModelEventStream, ModelError> {
        let completion = self
            .client
            .chat()
            .stream(
                ChatRequest::new(&self.chat_model, turn.messages)
                    .tools(turn.tools)
                    .tool_choice(ToolChoice::Auto)
                    .parallel_tool_calls(false),
                RequestOptions::default(),
            )
            .await?;
        let mapped = completion
            .map(|chunk| chunk.map_err(ModelError::from).map(chunk_events))
            .flat_map(|result| match result {
                Ok(events) => stream::iter(events.into_iter().map(Ok).collect::<Vec<_>>()),
                Err(error) => stream::iter(vec![Err(error)]),
            });
        Ok(Box::pin(mapped))
    }

    async fn complete_fim(&self, prompt: String, suffix: String) -> Result<String, ModelError> {
        let response = self
            .client
            .fim()
            .complete(
                FimRequest::new(&self.fim_model, prompt).suffix(suffix),
                RequestOptions::default(),
            )
            .await?;
        Ok(response.data.first_text()?.to_owned())
    }
}

pub fn coding_tools() -> Vec<Tool> {
    vec![
        tool(
            "list_files",
            "List repository files",
            serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool(
            "read_file",
            "Read a UTF-8 repository file",
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        ),
        tool(
            "search",
            "Search literal text in repository files",
            serde_json::json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
        ),
        tool(
            "replace_span",
            "Replace an exact UTF-8 byte span using a snapshot hash",
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"expected_sha256":{"type":"string"},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"replacement":{"type":"string"}},"required":["path","expected_sha256","start","end","replacement"],"additionalProperties":false}),
        ),
        tool(
            "fim_replace_span",
            "Fill a verified code span using Codestral FIM",
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"expected_sha256":{"type":"string"},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"fallback_replacement":{"type":"string"}},"required":["path","expected_sha256","start","end"],"additionalProperties":false}),
        ),
        tool(
            "run_command",
            "Run a bounded command using an executable and argument vector",
            serde_json::json!({"type":"object","properties":{"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"working_directory":{"type":"string"},"environment":{"type":"object","additionalProperties":{"type":"string"}},"network":{"type":"boolean"}},"required":["program"],"additionalProperties":false}),
        ),
    ]
}

fn tool(name: &str, description: &str, parameters: serde_json::Value) -> Tool {
    Tool::function(
        FunctionDefinition::new(name, parameters)
            .description(description)
            .strict(true),
    )
}

fn chunk_events(chunk: le_rust::chat::CompletionChunk) -> Vec<ModelEvent> {
    let mut events = Vec::new();
    for choice in chunk.choices {
        if let Some(Content::Text(text)) = choice.delta.content {
            events.push(ModelEvent::Text {
                choice: choice.index,
                text,
            });
        }
        for tool in choice.delta.tool_calls {
            let function = tool.function;
            events.push(ModelEvent::ToolCallDelta {
                choice: choice.index,
                index: tool.index,
                id: tool.id,
                name: function.as_ref().and_then(|value| value.name.clone()),
                arguments: function
                    .and_then(|value| value.arguments)
                    .map(arguments_text),
            });
        }
        if let Some(reason) = choice.finish_reason {
            events.push(ModelEvent::Finished {
                choice: choice.index,
                reason: reason.as_str().to_owned(),
            });
        }
    }
    if let Some(usage) = chunk.usage {
        events.push(ModelEvent::Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        });
    }
    events
}

fn arguments_text(arguments: ToolArguments) -> String {
    match arguments {
        ToolArguments::String(value) => value,
        ToolArguments::Json(value) => value.to_string(),
    }
}

#[derive(Clone, Default)]
pub struct ScriptedModelAdapter {
    models: Vec<String>,
    chat: Arc<Mutex<VecDeque<Vec<ModelEvent>>>>,
    fim: Arc<Mutex<VecDeque<String>>>,
}

impl ScriptedModelAdapter {
    pub fn new(models: Vec<String>) -> Self {
        Self {
            models,
            ..Self::default()
        }
    }

    pub async fn push_chat(&self, events: Vec<ModelEvent>) {
        self.chat.lock().await.push_back(events);
    }

    pub async fn push_fim(&self, text: impl Into<String>) {
        self.fim.lock().await.push_back(text.into());
    }
}

#[async_trait]
impl ModelAdapter for ScriptedModelAdapter {
    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        Ok(self.models.clone())
    }

    async fn stream_chat(&self, _turn: ChatTurn) -> Result<ModelEventStream, ModelError> {
        let events = self
            .chat
            .lock()
            .await
            .pop_front()
            .ok_or(ModelError::ScriptExhausted)?;
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }

    async fn complete_fim(&self, _prompt: String, _suffix: String) -> Result<String, ModelError> {
        self.fim
            .lock()
            .await
            .pop_front()
            .ok_or(ModelError::ScriptExhausted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scripted_adapter_is_deterministic() {
        let adapter = ScriptedModelAdapter::new(vec!["chat".into()]);
        adapter
            .push_chat(vec![ModelEvent::Text {
                choice: 0,
                text: "hello".into(),
            }])
            .await;
        let mut response = adapter.stream_chat(ChatTurn::user("hi")).await.unwrap();
        assert_eq!(
            response.next().await.unwrap().unwrap(),
            ModelEvent::Text {
                choice: 0,
                text: "hello".into()
            }
        );
        assert!(response.next().await.is_none());
        assert_eq!(adapter.list_models().await.unwrap(), vec!["chat"]);
    }
}
