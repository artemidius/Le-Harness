use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::EVENT_SCHEMA_VERSION;

/// Stable envelope for persisted and machine-readable runtime events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub schema_version: u16,
    pub sequence: u64,
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

impl RuntimeEvent {
    pub fn new(sequence: u64, kind: impl Into<String>, payload: Value) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            sequence,
            kind: kind.into(),
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_envelope_round_trips() {
        let event = RuntimeEvent::new(7, "session.started", serde_json::json!({"mode":"inspect"}));
        let encoded = serde_json::to_string(&event).unwrap();
        assert_eq!(
            serde_json::from_str::<RuntimeEvent>(&encoded).unwrap(),
            event
        );
    }
}
