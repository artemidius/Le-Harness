use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use futures_util::{StreamExt, stream};
use le_rust::chat::{ChatRequest, CompletionTextError, Content, Message, ToolArguments};
use le_rust::fim::FimRequest;
use le_rust::models::ListModelsOptions;
use le_rust::{Client, RequestOptions};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::settings::MistralSettings;

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send + 'static>>;

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub messages: Vec<Message>,
}

impl ChatTurn {
    pub fn user(prompt: impl Into<String>) -> Self {
        Self {
            messages: vec![Message::user(prompt.into())],
        }
    }
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
        index: usize,
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
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("Mistral request failed: {0}")]
    Mistral(#[from] le_rust::Error),
    #[error("Mistral completion did not contain usable text: {0}")]
    CompletionText(#[from] CompletionTextError),
    #[error("scripted model has no response for this request")]
    ScriptExhausted,
}

#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn list_models(&self) -> Result<Vec<String>, ModelError>;
    async fn stream_chat(&self, turn: ChatTurn) -> Result<ModelEventStream, ModelError>;
    async fn complete_fim(&self, prompt: String, suffix: String) -> Result<String, ModelError>;
}

#[derive(Clone)]
pub struct MistralAdapter {
    client: Client,
    chat_model: String,
    fim_model: String,
}

impl MistralAdapter {
    pub fn from_settings(settings: &MistralSettings) -> Result<Self, ModelError> {
        let client = Client::builder().api_key(&settings.api_key).build()?;
        Ok(Self {
            client,
            chat_model: settings.chat_model.clone(),
            fim_model: settings.fim_model.clone(),
        })
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
                ChatRequest::new(&self.chat_model, turn.messages),
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
                index: tool.index.unwrap_or(0),
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
