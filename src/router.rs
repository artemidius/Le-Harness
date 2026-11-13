use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use futures_util::{StreamExt, stream};

use crate::model::{ChatTurn, ModelAdapter, ModelError, ModelEvent, ModelEventStream, ModelRole};

#[derive(Debug, Clone, Copy)]
pub struct RouteBudgets {
    pub planner_requests: u32,
    pub fim_requests: u32,
    pub utility_requests: u32,
}

impl Default for RouteBudgets {
    fn default() -> Self {
        Self {
            planner_requests: 12,
            fim_requests: 24,
            utility_requests: 24,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RouteCapabilities {
    pub planner_tools: bool,
    pub fim: bool,
    pub utility_chat: bool,
    pub utility_tools: bool,
}

impl Default for RouteCapabilities {
    fn default() -> Self {
        Self {
            planner_tools: true,
            fim: true,
            utility_chat: true,
            utility_tools: true,
        }
    }
}

pub struct HybridRouter<P, F, U> {
    planner: P,
    fim: F,
    utility: U,
    planner_label: String,
    fim_label: String,
    utility_label: String,
    budgets: RouteBudgets,
    capabilities: RouteCapabilities,
    fallback_planner_to_utility: bool,
    planner_used: AtomicU32,
    fim_used: AtomicU32,
    utility_used: AtomicU32,
}

impl<P, F, U> HybridRouter<P, F, U> {
    pub fn new(planner: P, fim: F, utility: U, labels: [String; 3], budgets: RouteBudgets) -> Self {
        let [planner_label, fim_label, utility_label] = labels;
        Self {
            planner,
            fim,
            utility,
            planner_label,
            fim_label,
            utility_label,
            budgets,
            capabilities: RouteCapabilities::default(),
            fallback_planner_to_utility: false,
            planner_used: AtomicU32::new(0),
            fim_used: AtomicU32::new(0),
            utility_used: AtomicU32::new(0),
        }
    }

    pub fn capabilities(mut self, capabilities: RouteCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn fallback_planner_to_utility(mut self, enabled: bool) -> Self {
        self.fallback_planner_to_utility = enabled;
        self
    }

    pub fn fim_label(&self) -> &str {
        &self.fim_label
    }

    fn consume(counter: &AtomicU32, limit: u32, role: &'static str) -> Result<(), ModelError> {
        let previous = counter.fetch_add(1, Ordering::AcqRel);
        if previous >= limit {
            counter.fetch_sub(1, Ordering::AcqRel);
            Err(ModelError::RouteBudget(role))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl<P: ModelAdapter, F: ModelAdapter, U: ModelAdapter> ModelAdapter for HybridRouter<P, F, U> {
    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        self.planner.list_models().await
    }

    async fn stream_chat(&self, turn: ChatTurn) -> Result<ModelEventStream, ModelError> {
        match turn.role {
            ModelRole::Planner => {
                if !self.capabilities.planner_tools && !turn.tools.is_empty() {
                    return Err(ModelError::MissingCapability("planner tool calling"));
                }
                Self::consume(&self.planner_used, self.budgets.planner_requests, "planner")?;
                match self.planner.stream_chat(turn.clone()).await {
                    Ok(response) => Ok(routed(response, "planner", &self.planner_label, false)),
                    Err(error)
                        if self.fallback_planner_to_utility
                            && self.capabilities.utility_chat
                            && (turn.tools.is_empty() || self.capabilities.utility_tools) =>
                    {
                        Self::consume(
                            &self.utility_used,
                            self.budgets.utility_requests,
                            "utility",
                        )?;
                        let response = self.utility.stream_chat(turn).await.map_err(|_| error)?;
                        Ok(routed(response, "planner", &self.utility_label, true))
                    }
                    Err(error) => Err(error),
                }
            }
            ModelRole::Utility => {
                if !self.capabilities.utility_chat {
                    return Err(ModelError::MissingCapability("utility chat"));
                }
                if !turn.tools.is_empty() && !self.capabilities.utility_tools {
                    return Err(ModelError::MissingCapability("utility tool calling"));
                }
                Self::consume(&self.utility_used, self.budgets.utility_requests, "utility")?;
                let response = self.utility.stream_chat(turn).await?;
                Ok(routed(response, "utility", &self.utility_label, false))
            }
        }
    }

    async fn complete_fim(&self, prompt: String, suffix: String) -> Result<String, ModelError> {
        if !self.capabilities.fim {
            return Err(ModelError::MissingCapability("fill in the middle"));
        }
        Self::consume(&self.fim_used, self.budgets.fim_requests, "fim")?;
        self.fim.complete_fim(prompt, suffix).await
    }
}

fn routed(
    response: ModelEventStream,
    role: &str,
    backend: &str,
    fallback: bool,
) -> ModelEventStream {
    let event = ModelEvent::Routed {
        role: role.into(),
        backend: backend.into(),
        fallback,
    };
    Box::pin(stream::once(async { Ok(event) }).chain(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChatTurn, ScriptedModelAdapter};

    fn done() -> Vec<ModelEvent> {
        vec![ModelEvent::Finished {
            choice: 0,
            reason: "stop".into(),
        }]
    }

    #[tokio::test]
    async fn records_planner_to_utility_fallback() {
        let utility = ScriptedModelAdapter::default();
        utility.push_chat(done()).await;
        let router = HybridRouter::new(
            ScriptedModelAdapter::default(),
            ScriptedModelAdapter::default(),
            utility,
            ["large".into(), "codestral".into(), "small".into()],
            RouteBudgets::default(),
        )
        .fallback_planner_to_utility(true);
        let mut stream = router.stream_chat(ChatTurn::user("task")).await.unwrap();
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            ModelEvent::Routed { fallback: true, .. }
        ));
    }

    #[tokio::test]
    async fn enforces_route_budget_and_capability() {
        let planner = ScriptedModelAdapter::default();
        planner.push_chat(done()).await;
        let router = HybridRouter::new(
            planner,
            ScriptedModelAdapter::default(),
            ScriptedModelAdapter::default(),
            ["p".into(), "f".into(), "u".into()],
            RouteBudgets {
                planner_requests: 1,
                ..RouteBudgets::default()
            },
        );
        let mut first = router.stream_chat(ChatTurn::user("one")).await.unwrap();
        assert!(first.next().await.is_some());
        assert!(matches!(
            router.stream_chat(ChatTurn::user("two")).await,
            Err(ModelError::RouteBudget("planner"))
        ));

        let disabled = HybridRouter::new(
            ScriptedModelAdapter::default(),
            ScriptedModelAdapter::default(),
            ScriptedModelAdapter::default(),
            ["p".into(), "f".into(), "u".into()],
            RouteBudgets::default(),
        )
        .capabilities(RouteCapabilities {
            fim: false,
            ..RouteCapabilities::default()
        });
        assert!(matches!(
            disabled.complete_fim("a".into(), "b".into()).await,
            Err(ModelError::MissingCapability(_))
        ));
    }
}
