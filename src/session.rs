use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::EVENT_SCHEMA_VERSION;
use crate::event::RuntimeEvent;
use crate::runtime::{AgentRun, AgentStatus};

pub const SESSION_SCHEMA_VERSION: u16 = 1;
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHeader {
    pub schema_version: u16,
    pub id: String,
    pub parent_session_id: Option<String>,
    pub created_unix_ms: u128,
    pub prompt: String,
    pub repository_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionFooter {
    pub status: AgentStatus,
    pub answer: String,
    pub model_requests: u32,
    pub tool_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSnapshot {
    pub header: SessionHeader,
    pub events: Vec<RuntimeEvent>,
    pub footer: SessionFooter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ResumeState {
    Ready {
        session_id: String,
        prior_answer: String,
        prior_status: AgentStatus,
    },
    ReconciliationRequired {
        session_id: String,
        pending_tool_ids: Vec<String>,
    },
}

impl ResumeState {
    pub fn continuation_prompt(&self, task: &str) -> Result<String, SessionError> {
        match self {
            Self::Ready {
                session_id,
                prior_answer,
                prior_status,
            } => Ok(format!(
                "Continue session {session_id}. The previous run ended with status {}. Its terminal answer follows:\n\n{}\n\nNew task:\n{}",
                status_name(prior_status),
                prior_answer,
                task
            )),
            Self::ReconciliationRequired {
                pending_tool_ids, ..
            } => Err(SessionError::UnsafeResume(pending_tool_ids.join(", "))),
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session {0} was not found")]
    NotFound(String),
    #[error("invalid session identifier")]
    InvalidId,
    #[error("session journal is invalid: {0}")]
    InvalidJournal(String),
    #[error("session has unreconciled tool calls: {0}")]
    UnsafeResume(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "record", content = "data", rename_all = "snake_case")]
enum JournalRecord {
    Header(SessionHeader),
    Event(RuntimeEvent),
    Footer(SessionFooter),
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save(
        &self,
        prompt: impl Into<String>,
        repository_root: impl Into<PathBuf>,
        parent_session_id: Option<String>,
        run: &AgentRun,
    ) -> Result<SessionSnapshot, SessionError> {
        fs::create_dir_all(&self.root)?;
        let created_unix_ms = unix_millis()?;
        let sequence = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let id = format!("{created_unix_ms:x}-{:x}-{sequence:x}", std::process::id());
        let snapshot = SessionSnapshot {
            header: SessionHeader {
                schema_version: SESSION_SCHEMA_VERSION,
                id: id.clone(),
                parent_session_id,
                created_unix_ms,
                prompt: prompt.into(),
                repository_root: repository_root.into(),
            },
            events: run.events.clone(),
            footer: SessionFooter {
                status: run.status.clone(),
                answer: run.output.clone(),
                model_requests: run.model_requests,
                tool_calls: run.tool_calls,
            },
        };
        self.write_atomic(&snapshot)?;
        Ok(snapshot)
    }

    pub fn load(&self, id: &str) -> Result<SessionSnapshot, SessionError> {
        let path = self.path_for(id)?;
        let file = File::open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SessionError::NotFound(id.to_owned())
            } else {
                SessionError::Io(error)
            }
        })?;
        let mut header = None;
        let mut footer = None;
        let mut events = Vec::new();
        for line in BufReader::new(file).lines() {
            let record: JournalRecord = serde_json::from_str(&line?)?;
            match record {
                JournalRecord::Header(value) if header.is_none() && events.is_empty() => {
                    header = Some(value)
                }
                JournalRecord::Event(value) if footer.is_none() => events.push(value),
                JournalRecord::Footer(value) if footer.is_none() => footer = Some(value),
                _ => {
                    return Err(SessionError::InvalidJournal(
                        "records are duplicated or out of order".into(),
                    ));
                }
            }
        }
        let snapshot = SessionSnapshot {
            header: header.ok_or_else(|| SessionError::InvalidJournal("missing header".into()))?,
            events,
            footer: footer.ok_or_else(|| SessionError::InvalidJournal("missing footer".into()))?,
        };
        validate(&snapshot, id)?;
        Ok(snapshot)
    }

    pub fn list(&self) -> Result<Vec<SessionHeader>, SessionError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            sessions.push(self.load(id)?.header);
        }
        sessions.sort_by_key(|session| std::cmp::Reverse(session.created_unix_ms));
        Ok(sessions)
    }

    pub fn resume_state(&self, id: &str) -> Result<ResumeState, SessionError> {
        let snapshot = self.load(id)?;
        let pending_tool_ids = pending_tool_ids(&snapshot.events)?;
        if pending_tool_ids.is_empty() {
            Ok(ResumeState::Ready {
                session_id: id.to_owned(),
                prior_answer: snapshot.footer.answer,
                prior_status: snapshot.footer.status,
            })
        } else {
            Ok(ResumeState::ReconciliationRequired {
                session_id: id.to_owned(),
                pending_tool_ids,
            })
        }
    }

    fn write_atomic(&self, snapshot: &SessionSnapshot) -> Result<(), SessionError> {
        let path = self.path_for(&snapshot.header.id)?;
        let temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        let mut writer = BufWriter::new(temporary);
        write_record(&mut writer, &JournalRecord::Header(snapshot.header.clone()))?;
        for event in &snapshot.events {
            write_record(&mut writer, &JournalRecord::Event(event.clone()))?;
        }
        write_record(&mut writer, &JournalRecord::Footer(snapshot.footer.clone()))?;
        writer.flush()?;
        writer.get_ref().as_file().sync_all()?;
        writer
            .into_inner()
            .map_err(|error| error.into_error())?
            .persist(path)
            .map_err(|error| error.error)?;
        Ok(())
    }

    fn path_for(&self, id: &str) -> Result<PathBuf, SessionError> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(SessionError::InvalidId);
        }
        Ok(self.root.join(format!("{id}.jsonl")))
    }
}

fn write_record(writer: &mut impl Write, record: &JournalRecord) -> Result<(), SessionError> {
    serde_json::to_writer(&mut *writer, record)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn validate(snapshot: &SessionSnapshot, expected_id: &str) -> Result<(), SessionError> {
    if snapshot.header.schema_version != SESSION_SCHEMA_VERSION || snapshot.header.id != expected_id
    {
        return Err(SessionError::InvalidJournal(
            "unsupported schema or mismatched identifier".into(),
        ));
    }
    for (index, event) in snapshot.events.iter().enumerate() {
        if event.schema_version != EVENT_SCHEMA_VERSION || event.sequence != index as u64 + 1 {
            return Err(SessionError::InvalidJournal(
                "event schema or sequence mismatch".into(),
            ));
        }
    }
    if snapshot.events.first().map(|event| event.kind.as_str()) != Some("session.started")
        || snapshot.events.last().map(|event| event.kind.as_str()) != Some("session.finished")
    {
        return Err(SessionError::InvalidJournal(
            "terminal event markers are missing".into(),
        ));
    }
    Ok(())
}

fn pending_tool_ids(events: &[RuntimeEvent]) -> Result<Vec<String>, SessionError> {
    let mut pending = BTreeSet::new();
    for event in events {
        if matches!(event.kind.as_str(), "tool.requested" | "tool.completed") {
            let id = event
                .payload
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| SessionError::InvalidJournal("tool event has no ID".into()))?;
            if event.kind == "tool.requested" {
                if !pending.insert(id.to_owned()) {
                    return Err(SessionError::InvalidJournal(
                        "tool call was requested twice".into(),
                    ));
                }
            } else if !pending.remove(id) {
                return Err(SessionError::InvalidJournal(
                    "tool completion has no matching request".into(),
                ));
            }
        }
    }
    Ok(pending.into_iter().collect())
}

fn unix_millis() -> Result<u128, SessionError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| SessionError::InvalidJournal("system clock precedes Unix epoch".into()))
}

fn status_name(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Completed { .. } => "completed",
        AgentStatus::Cancelled => "cancelled",
        AgentStatus::BudgetExhausted { .. } => "budget_exhausted",
        AgentStatus::Failed { .. } => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> AgentRun {
        AgentRun {
            status: AgentStatus::Completed {
                finish_reason: "stop".into(),
            },
            output: "done".into(),
            events: vec![
                RuntimeEvent::new(1, "session.started", serde_json::json!({})),
                RuntimeEvent::new(2, "tool.requested", serde_json::json!({"id":"call-1"})),
                RuntimeEvent::new(3, "tool.completed", serde_json::json!({"id":"call-1"})),
                RuntimeEvent::new(
                    4,
                    "session.finished",
                    serde_json::json!({"status":{"state":"completed","finish_reason":"stop"}}),
                ),
            ],
            model_requests: 2,
            tool_calls: 1,
        }
    }

    #[test]
    fn atomically_round_trips_and_lists_journals() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::new(directory.path());
        let saved = store
            .save("task", "/repo", Some("parent".into()), &run())
            .unwrap();
        assert_eq!(store.load(&saved.header.id).unwrap(), saved);
        assert_eq!(store.list().unwrap(), vec![saved.header]);
        assert!(
            directory.path().read_dir().unwrap().all(|entry| entry
                .unwrap()
                .path()
                .extension()
                .unwrap()
                == "jsonl")
        );
    }

    #[test]
    fn resume_context_never_replays_completed_tools() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::new(directory.path());
        let saved = store.save("task", "/repo", None, &run()).unwrap();
        let state = store.resume_state(&saved.header.id).unwrap();
        let prompt = state.continuation_prompt("next").unwrap();
        assert!(prompt.contains("done"));
        assert!(!prompt.contains("call-1"));
    }

    #[test]
    fn invalid_identifiers_and_event_sequences_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::new(directory.path());
        assert!(matches!(
            store.load("../escape"),
            Err(SessionError::InvalidId)
        ));
        let mut run = run();
        run.events[1].sequence = 9;
        let saved = store.save("task", "/repo", None, &run).unwrap();
        assert!(matches!(
            store.load(&saved.header.id),
            Err(SessionError::InvalidJournal(_))
        ));
    }

    #[test]
    fn unmatched_tool_request_requires_reconciliation() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::new(directory.path());
        let mut run = run();
        run.events.remove(2);
        run.events[2].sequence = 3;
        let saved = store.save("task", "/repo", None, &run).unwrap();
        assert!(matches!(
            store.resume_state(&saved.header.id).unwrap(),
            ResumeState::ReconciliationRequired { pending_tool_ids, .. }
                if pending_tool_ids == vec!["call-1"]
        ));
    }
}
