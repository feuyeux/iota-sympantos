#![allow(dead_code)]

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::runtime_event::{
    ApprovalDecisionEvent, ApprovalRequestEvent, ErrorEvent, OutputEvent, RuntimeEvent, ToolCallEvent,
};

#[derive(Clone)]
pub struct EventStore {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub session_id: String,
    pub backend: String,
    pub request_hash: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub fencing_token: i64,
}

impl EventStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open event store {}", path.display()))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init()?;
        Ok(store)
    }

    pub fn default_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(home.join(".i6").join("context").join("events.sqlite"))
    }

    pub fn begin_execution(
        &self,
        backend: &str,
        session_id: &str,
        request_hash: &str,
    ) -> Result<String> {
        self.begin_execution_with_id(backend, session_id, request_hash, None)
    }

    pub fn begin_execution_with_id(
        &self,
        backend: &str,
        session_id: &str,
        request_hash: &str,
        execution_id: Option<&str>,
    ) -> Result<String> {
        let execution_id = execution_id
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = now_ts();
        let conn = self.conn.lock().expect("event store mutex poisoned");
        if let Some(existing) = conn
            .query_row(
                "SELECT request_hash FROM executions WHERE execution_id = ?1",
                params![execution_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            if existing == request_hash {
                return Ok(execution_id);
            }
            bail!("execution_id conflict: request_hash differs");
        }
        let fencing_token: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(fencing_token), 0) + 1 FROM executions",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);
        conn.execute(
            "INSERT INTO executions (execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token) VALUES (?1, ?2, ?3, ?4, 'running', ?5, NULL, ?6)",
            params![execution_id, session_id, backend, request_hash, now, fencing_token],
        )?;
        Ok(execution_id)
    }

    pub fn append_event(&self, execution_id: &str, event: &RuntimeEvent) -> Result<i64> {
        let event_json =
            serde_json::to_string(event).context("Failed to serialize runtime event")?;
        let conn = self.conn.lock().expect("event store mutex poisoned");
        let next_seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE execution_id = ?1",
                params![execution_id],
                |row| row.get(0),
            )
            .context("Failed to allocate event seq")?;
        conn.execute(
            "INSERT INTO events (execution_id, seq, event_type, event_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![execution_id, next_seq, event.event_type(), event_json, now_ts()],
        )?;
        Ok(next_seq)
    }

    pub fn finish_execution(&self, execution_id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().expect("event store mutex poisoned");
        conn.execute(
            "UPDATE executions SET status = ?2, finished_at = ?3 WHERE execution_id = ?1",
            params![execution_id, status, now_ts()],
        )?;
        Ok(())
    }

    pub fn find_completed_by_request_hash(
        &self,
        backend: &str,
        request_hash: &str,
    ) -> Result<Option<ExecutionRecord>> {
        let conn = self.conn.lock().expect("event store mutex poisoned");
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token FROM executions\n             WHERE backend = ?1 AND request_hash = ?2 AND status = 'completed'\n             ORDER BY finished_at DESC, fencing_token DESC, started_at DESC LIMIT 1",
            params![backend, request_hash],
            |row| {
                Ok(ExecutionRecord {
                    execution_id: row.get(0)?,
                    session_id: row.get(1)?,
                    backend: row.get(2)?,
                    request_hash: row.get(3)?,
                    status: row.get(4)?,
                    started_at: row.get(5)?,
                    finished_at: row.get(6)?,
                    fencing_token: row.get(7)?,
                })
            },
        )
        .optional()
        .context("Failed to find completed execution")
    }

    pub fn output_text(&self, execution_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("event store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT event_json FROM events WHERE execution_id = ?1 AND event_type = 'output' ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![execution_id], |row| row.get::<_, String>(0))?;
        let mut output = String::new();
        for row in rows {
            let event_json = row?;
            if let Ok(RuntimeEvent::Output(OutputEvent { text, .. })) =
                serde_json::from_str::<RuntimeEvent>(&event_json)
            {
                output.push_str(&text);
            }
        }
        if output.is_empty() {
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }

    pub fn find_running_by_request_hash(
        &self,
        backend: &str,
        request_hash: &str,
    ) -> Result<Option<ExecutionRecord>> {
        let conn = self.conn.lock().expect("event store mutex poisoned");
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token FROM executions
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running'
             ORDER BY started_at ASC LIMIT 1",
            params![backend, request_hash],
            |row| {
                Ok(ExecutionRecord {
                    execution_id: row.get(0)?,
                    session_id: row.get(1)?,
                    backend: row.get(2)?,
                    request_hash: row.get(3)?,
                    status: row.get(4)?,
                    started_at: row.get(5)?,
                    finished_at: row.get(6)?,
                    fencing_token: row.get(7)?,
                })
            },
        )
        .optional()
        .context("Failed to find running execution")
    }

    pub fn events_since(
        &self,
        execution_id: &str,
        after_seq: i64,
    ) -> Result<Vec<(i64, RuntimeEvent)>> {
        let conn = self.conn.lock().expect("event store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT seq, event_json FROM events WHERE execution_id = ?1 AND seq > ?2 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![execution_id, after_seq], |row| {
            let seq: i64 = row.get(0)?;
            let event_json: String = row.get(1)?;
            Ok((seq, event_json))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (seq, event_json) = row?;
            if let Ok(event) = serde_json::from_str::<RuntimeEvent>(&event_json) {
                events.push((seq, event));
            }
        }
        Ok(events)
    }

    pub fn get_execution(&self, execution_id: &str) -> Result<Option<ExecutionRecord>> {
        let conn = self.conn.lock().expect("event store mutex poisoned");
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token FROM executions WHERE execution_id = ?1",
            params![execution_id],
            |row| {
                Ok(ExecutionRecord {
                    execution_id: row.get(0)?,
                    session_id: row.get(1)?,
                    backend: row.get(2)?,
                    request_hash: row.get(3)?,
                    status: row.get(4)?,
                    started_at: row.get(5)?,
                    finished_at: row.get(6)?,
                    fencing_token: row.get(7)?,
                })
            },
        )
        .optional()
        .context("Failed to read execution")
    }

    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().expect("event store mutex poisoned");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (\n  execution_id TEXT NOT NULL,\n  seq INTEGER NOT NULL,\n  event_type TEXT NOT NULL,\n  event_json TEXT NOT NULL,\n  created_at INTEGER NOT NULL,\n  PRIMARY KEY (execution_id, seq)\n);\n\nCREATE TABLE IF NOT EXISTS executions (\n  execution_id TEXT PRIMARY KEY,\n  session_id TEXT NOT NULL,\n  backend TEXT NOT NULL,\n  request_hash TEXT NOT NULL,\n  status TEXT NOT NULL,\n  started_at INTEGER NOT NULL,\n  finished_at INTEGER,\n  fencing_token INTEGER NOT NULL DEFAULT 0\n);",
        )?;
        let _ = conn.execute(
            "ALTER TABLE executions ADD COLUMN fencing_token INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_executions_running_lock ON executions(backend, request_hash) WHERE status = 'running'",
            [],
        );
        Ok(())
    }
}

pub fn request_hash(backend: &str, cwd: &Path, prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(backend.as_bytes());
    hasher.update(b"\0");
    hasher.update(cwd.as_os_str().to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(prompt.as_bytes());
    hex::encode(hasher.finalize())
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_id_conflict_is_rejected() {
        let store = EventStore::open(Path::new(":memory:")).unwrap();
        let id = store
            .begin_execution_with_id("codex", "session", "hash-a", Some("exec-1"))
            .unwrap();
        assert_eq!(id, "exec-1");
        let same = store
            .begin_execution_with_id("codex", "session", "hash-a", Some("exec-1"))
            .unwrap();
        assert_eq!(same, "exec-1");
        let conflict = store.begin_execution_with_id("codex", "session", "hash-b", Some("exec-1"));
        assert!(conflict.is_err());
    }

    #[test]
    fn persists_runtime_events_in_sequence() {
        let store = EventStore::open(Path::new(":memory:")).unwrap();
        let execution_id = store
            .begin_execution_with_id("codex", "session", "hash-a", Some("exec-events"))
            .unwrap();
        let events = [
            RuntimeEvent::Output(OutputEvent {
                text: "hello".to_string(),
                role: Some("assistant".to_string()),
            }),
            RuntimeEvent::ToolCall(ToolCallEvent {
                id: "tool-1".to_string(),
                name: "iota_memory_search".to_string(),
                arguments: serde_json::json!({"query":"hello"}),
            }),
            RuntimeEvent::ApprovalRequest(ApprovalRequestEvent {
                id: "approval-1".to_string(),
                tool_name: "shell".to_string(),
                payload: serde_json::json!({"command":"echo hello"}),
            }),
            RuntimeEvent::ApprovalDecision(ApprovalDecisionEvent {
                request_id: "approval-1".to_string(),
                approved: true,
                reason: Some("test".to_string()),
            }),
            RuntimeEvent::Error(ErrorEvent {
                message: "boom".to_string(),
                code: Some(1),
                data: None,
            }),
        ];

        for (index, event) in events.iter().enumerate() {
            let seq = store.append_event(&execution_id, event).unwrap();
            assert_eq!(seq, index as i64 + 1);
        }

        let stored = store.events_since(&execution_id, 0).unwrap();
        assert_eq!(stored.len(), events.len());
        assert_eq!(stored.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
        assert!(matches!(stored[0].1, RuntimeEvent::Output(_)));
        assert!(matches!(stored[1].1, RuntimeEvent::ToolCall(_)));
        assert!(matches!(stored[2].1, RuntimeEvent::ApprovalRequest(_)));
        assert!(matches!(stored[3].1, RuntimeEvent::ApprovalDecision(_)));
        assert!(matches!(stored[4].1, RuntimeEvent::Error(_)));
        assert_eq!(store.output_text(&execution_id).unwrap().as_deref(), Some("hello"));
    }
}
