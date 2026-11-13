use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use futures_util::StreamExt;
use le_rust::chat::{Message, ToolCall};
use serde::{Deserialize, Serialize};

use crate::event::RuntimeEvent;
use crate::model::coding_tools;
use crate::model::{ChatTurn, ModelAdapter, ModelEvent};

#[derive(Debug, Clone, Copy)]
pub struct AgentLimits {
    pub max_model_requests: u32,
    pub max_tool_calls: u32,
    pub max_model_events: u32,
    pub max_output_bytes: usize,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_model_requests: 12,
            max_tool_calls: 32,
            max_model_events: 10_000,
            max_output_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    async fn execute(&self, invocation: &ToolInvocation) -> ToolResult;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentStatus {
    Completed { finish_reason: String },
    Cancelled,
    BudgetExhausted { resource: String },
    Failed { message: String },
}

#[derive(Debug, Clone)]
pub struct AgentRun {
    pub status: AgentStatus,
    pub output: String,
    pub events: Vec<RuntimeEvent>,
    pub model_requests: u32,
    pub tool_calls: u32,
}

pub struct AgentController<A, T> {
    model: A,
    tools: T,
    limits: AgentLimits,
    cancellation: CancellationToken,
}

impl<A, T> AgentController<A, T>
where
    A: ModelAdapter,
    T: ToolHandler,
{
    pub fn new(model: A, tools: T, limits: AgentLimits) -> Self {
        Self {
            model,
            tools,
            limits,
            cancellation: CancellationToken::default(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub async fn run(&self, prompt: impl Into<String>) -> AgentRun {
        let mut state = RunState::new(Message::user(prompt.into()));
        loop {
            if self.cancellation.is_cancelled() {
                return state.finish(AgentStatus::Cancelled);
            }
            if state.model_requests >= self.limits.max_model_requests {
                return state.finish(AgentStatus::BudgetExhausted {
                    resource: "model requests".into(),
                });
            }
            state.model_requests += 1;
            state.record(
                "model.requested",
                serde_json::json!({"number": state.model_requests}),
            );
            let response = self
                .model
                .stream_chat(ChatTurn {
                    messages: state.messages.clone(),
                    tools: coding_tools(),
                })
                .await;
            let mut stream = match response {
                Ok(stream) => stream,
                Err(error) => {
                    return state.finish(AgentStatus::Failed {
                        message: error.to_string(),
                    });
                }
            };
            let mut turn = TurnAccumulator::default();
            while let Some(event) = stream.next().await {
                if self.cancellation.is_cancelled() {
                    return state.finish(AgentStatus::Cancelled);
                }
                state.model_events += 1;
                if state.model_events > self.limits.max_model_events {
                    return state.finish(AgentStatus::BudgetExhausted {
                        resource: "model events".into(),
                    });
                }
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        return state.finish(AgentStatus::Failed {
                            message: error.to_string(),
                        });
                    }
                };
                if let Err(message) = turn.push(event, self.limits.max_output_bytes) {
                    return state.finish(AgentStatus::Failed { message });
                }
            }

            let completed = match turn.finish() {
                Ok(completed) => completed,
                Err(message) => return state.finish(AgentStatus::Failed { message }),
            };
            state.output.push_str(&completed.text);
            if state.output.len() > self.limits.max_output_bytes {
                return state.finish(AgentStatus::BudgetExhausted {
                    resource: "output bytes".into(),
                });
            }
            if completed.tools.is_empty() {
                state.record(
                    "model.finished",
                    serde_json::json!({"reason": &completed.finish_reason}),
                );
                return state.finish(AgentStatus::Completed {
                    finish_reason: completed.finish_reason,
                });
            }
            if !completed.text.is_empty() {
                return state.finish(AgentStatus::Failed {
                    message:
                        "a tool-call turn also contained text that cannot be replayed losslessly"
                            .into(),
                });
            }
            let native_calls = completed
                .tools
                .iter()
                .map(|tool| ToolCall::function(&tool.id, &tool.name, tool.arguments.clone()))
                .collect();
            state
                .messages
                .push(Message::assistant_tool_calls(native_calls));
            for tool in completed.tools {
                if state.tool_calls >= self.limits.max_tool_calls {
                    return state.finish(AgentStatus::BudgetExhausted {
                        resource: "tool calls".into(),
                    });
                }
                state.tool_calls += 1;
                state.record(
                    "tool.requested",
                    serde_json::json!({"id":&tool.id,"name":&tool.name}),
                );
                let result = self.tools.execute(&tool).await;
                state.record(
                    "tool.completed",
                    serde_json::json!({"id":&tool.id,"is_error":result.is_error}),
                );
                state.messages.push(Message::tool(tool.id, result.content));
            }
        }
    }
}

struct RunState {
    messages: Vec<Message>,
    output: String,
    events: Vec<RuntimeEvent>,
    sequence: u64,
    model_requests: u32,
    model_events: u32,
    tool_calls: u32,
}

impl RunState {
    fn new(message: Message) -> Self {
        let mut state = Self {
            messages: vec![message],
            output: String::new(),
            events: Vec::new(),
            sequence: 0,
            model_requests: 0,
            model_events: 0,
            tool_calls: 0,
        };
        state.record("session.started", serde_json::json!({}));
        state
    }

    fn record(&mut self, kind: &str, payload: serde_json::Value) {
        self.sequence += 1;
        self.events
            .push(RuntimeEvent::new(self.sequence, kind, payload));
    }

    fn finish(mut self, status: AgentStatus) -> AgentRun {
        self.record("session.finished", serde_json::json!({"status":&status}));
        AgentRun {
            status,
            output: self.output,
            events: self.events,
            model_requests: self.model_requests,
            tool_calls: self.tool_calls,
        }
    }
}

#[derive(Default)]
struct TurnAccumulator {
    text: String,
    tools: BTreeMap<usize, ToolParts>,
    finish_reason: Option<String>,
}

#[derive(Default)]
struct ToolParts {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl TurnAccumulator {
    fn push(&mut self, event: ModelEvent, byte_limit: usize) -> Result<(), String> {
        match event {
            ModelEvent::Text { choice, text } => {
                require_primary(choice)?;
                self.text.push_str(&text);
                if self.text.len() > byte_limit {
                    return Err("model turn exceeded the output byte limit".into());
                }
            }
            ModelEvent::ToolCallDelta {
                choice,
                index,
                id,
                name,
                arguments,
            } => {
                require_primary(choice)?;
                let index = index.ok_or_else(|| "tool-call delta omitted its index".to_owned())?;
                let target = self.tools.entry(index).or_default();
                set_consistent(&mut target.id, id, "tool call ID")?;
                set_consistent(&mut target.name, name, "tool name")?;
                if let Some(arguments) = arguments {
                    target.arguments.push_str(&arguments);
                }
                if target.arguments.len() > byte_limit {
                    return Err("tool arguments exceeded the output byte limit".into());
                }
            }
            ModelEvent::Finished { choice, reason } => {
                require_primary(choice)?;
                set_consistent(&mut self.finish_reason, Some(reason), "finish reason")?;
            }
            ModelEvent::Usage { .. } => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<CompletedTurn, String> {
        let finish_reason = self
            .finish_reason
            .ok_or_else(|| "model stream ended without a finish reason".to_owned())?;
        let mut tools = Vec::with_capacity(self.tools.len());
        for (_, parts) in self.tools {
            tools.push(ToolInvocation {
                id: parts
                    .id
                    .ok_or_else(|| "tool call omitted its ID".to_owned())?,
                name: parts
                    .name
                    .ok_or_else(|| "tool call omitted its name".to_owned())?,
                arguments: parts.arguments,
            });
        }
        Ok(CompletedTurn {
            text: self.text,
            tools,
            finish_reason,
        })
    }
}

struct CompletedTurn {
    text: String,
    tools: Vec<ToolInvocation>,
    finish_reason: String,
}

fn require_primary(choice: usize) -> Result<(), String> {
    if choice == 0 {
        Ok(())
    } else {
        Err("agent loop only accepts primary choice index 0".into())
    }
}

fn set_consistent(
    target: &mut Option<String>,
    value: Option<String>,
    name: &str,
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    if target.as_ref().is_some_and(|current| current != &value) {
        return Err(format!("conflicting {name} fragments"));
    }
    *target = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScriptedModelAdapter;

    #[derive(Default)]
    struct EchoTool;

    #[async_trait]
    impl ToolHandler for EchoTool {
        async fn execute(&self, invocation: &ToolInvocation) -> ToolResult {
            ToolResult::success(format!("{}:{}", invocation.name, invocation.arguments))
        }
    }

    fn text(text: &str) -> Vec<ModelEvent> {
        vec![
            ModelEvent::Text {
                choice: 0,
                text: text.into(),
            },
            ModelEvent::Finished {
                choice: 0,
                reason: "stop".into(),
            },
        ]
    }

    #[tokio::test]
    async fn completes_an_ordinary_turn() {
        let model = ScriptedModelAdapter::default();
        model.push_chat(text("done")).await;
        let run = AgentController::new(model, EchoTool, AgentLimits::default())
            .run("task")
            .await;
        assert_eq!(run.output, "done");
        assert_eq!(
            run.status,
            AgentStatus::Completed {
                finish_reason: "stop".into()
            }
        );
    }

    #[tokio::test]
    async fn executes_a_tool_then_continues() {
        let model = ScriptedModelAdapter::default();
        model
            .push_chat(vec![
                ModelEvent::ToolCallDelta {
                    choice: 0,
                    index: Some(0),
                    id: Some("call-1".into()),
                    name: Some("read".into()),
                    arguments: Some("{\"path\":".into()),
                },
                ModelEvent::ToolCallDelta {
                    choice: 0,
                    index: Some(0),
                    id: None,
                    name: None,
                    arguments: Some("\"README.md\"}".into()),
                },
                ModelEvent::Finished {
                    choice: 0,
                    reason: "tool_calls".into(),
                },
            ])
            .await;
        model.push_chat(text("answer")).await;
        let run = AgentController::new(model, EchoTool, AgentLimits::default())
            .run("task")
            .await;
        assert_eq!(
            run.status,
            AgentStatus::Completed {
                finish_reason: "stop".into()
            }
        );
        assert_eq!(run.tool_calls, 1);
        assert_eq!(run.model_requests, 2);
    }

    #[tokio::test]
    async fn rejects_an_unindexed_tool_call() {
        let model = ScriptedModelAdapter::default();
        model
            .push_chat(vec![ModelEvent::ToolCallDelta {
                choice: 0,
                index: None,
                id: Some("call-1".into()),
                name: Some("read".into()),
                arguments: Some("{}".into()),
            }])
            .await;
        let run = AgentController::new(model, EchoTool, AgentLimits::default())
            .run("task")
            .await;
        assert!(matches!(run.status, AgentStatus::Failed { .. }));
    }

    #[tokio::test]
    async fn cancellation_is_terminal() {
        let model = ScriptedModelAdapter::default();
        let controller = AgentController::new(model, EchoTool, AgentLimits::default());
        controller.cancellation_token().cancel();
        let run = controller.run("task").await;
        assert_eq!(run.status, AgentStatus::Cancelled);
        assert_eq!(run.model_requests, 0);
    }

    #[tokio::test]
    async fn provider_failure_is_terminal() {
        let run = AgentController::new(
            ScriptedModelAdapter::default(),
            EchoTool,
            AgentLimits::default(),
        )
        .run("task")
        .await;
        assert!(matches!(run.status, AgentStatus::Failed { .. }));
    }

    #[tokio::test]
    async fn request_budget_stops_a_tool_loop() {
        let model = ScriptedModelAdapter::default();
        model
            .push_chat(vec![
                ModelEvent::ToolCallDelta {
                    choice: 0,
                    index: Some(0),
                    id: Some("call-1".into()),
                    name: Some("read".into()),
                    arguments: Some("{}".into()),
                },
                ModelEvent::Finished {
                    choice: 0,
                    reason: "tool_calls".into(),
                },
            ])
            .await;
        let limits = AgentLimits {
            max_model_requests: 1,
            ..AgentLimits::default()
        };
        let run = AgentController::new(model, EchoTool, limits)
            .run("task")
            .await;
        assert_eq!(
            run.status,
            AgentStatus::BudgetExhausted {
                resource: "model requests".into()
            }
        );
    }
}
