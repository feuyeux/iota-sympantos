//! Minimal SQLite store for execution cache / replay / dedupe.
//!
//! This is intentionally a stripped-down version of [`super::events::EventStore`]
//! that preserves only the cache/replay/deduplication functionality.
//! All observability features (metrics, Prometheus, token usage, timings) have
//! been removed.  The full EventStore will be deleted in a later migration step.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::runtime_event::{OutputEvent, RuntimeEvent};
use crate::utils::now_ts;

const RUNNING_EXECUTION_TTL_SECS: i64 = 60 * 60;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CacheStore {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStatus {
    Running,
    Completed,
    Failed,
    Unknown(String),
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Unknown(value) => value.as_str(),
        }
    }
}

impl From<&str> for ExecutionStatus {
    fn from(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            other => {
                tracing::warn!(status = other, "unknown execution status read from cache store");
                Self::Unknown(other.to_string())
            }
        }
    }
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for ExecutionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedExecution {
    pub execution_id: String,
    pub session_id: String,
    pub backend: String,
    pub request_hash: String,
    pub status: ExecutionStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub fencing_token: i64,
}

// ---------------------------------------------------------------------------
// CacheStore implementation
// ---------------------------------------------------------------------------

impl CacheStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open cache store {}", path.display()))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init()?;
        Ok(store)
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::config::paths::StorePaths::resolve()?.events_db())
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
        let mut conn = crate::utils::lock_or_recover(&self.conn);
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stale_before = now - RUNNING_EXECUTION_TTL_SECS;
        tx.execute(
            "UPDATE cache_executions SET status = 'failed', finished_at = ?3
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running' AND started_at < ?4",
            params![backend, request_hash, now, stale_before],
        )?;
        if let Some(existing) = tx
            .query_row(
                "SELECT request_hash FROM cache_executions WHERE execution_id = ?1",
                params![&execution_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            if existing == request_hash {
                tx.commit()?;
                return Ok(execution_id);
            }
            bail!("execution_id conflict: request_hash differs");
        }
        let fencing_token: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(fencing_token), 0) + 1 FROM cache_executions",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);
        tx.execute(
            "INSERT INTO cache_executions \
             (execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token) \
             VALUES (?1, ?2, ?3, ?4, 'running', ?5, NULL, ?6)",
            params![&execution_id, session_id, backend, request_hash, now, fencing_token],
        )?;
        tx.commit()?;
        Ok(execution_id)
    }

    /// Store only `Output` events for later replay.
    pub fn append_output(&self, execution_id: &str, event: &RuntimeEvent) -> Result<()> {
        if !matches!(event, RuntimeEvent::Output(_)) {
            return Ok(());
        }
        let event_json =
            serde_json::to_string(event).context("Failed to serialize runtime event")?;
        let mut conn = crate::utils::lock_or_recover(&self.conn);
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let next_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM cache_outputs WHERE execution_id = ?1",
                params![execution_id],
                |row| row.get(0),
            )
            .context("Failed to allocate output seq")?;
        tx.execute(
            "INSERT INTO cache_outputs (execution_id, seq, event_json, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![execution_id, next_seq, event_json, now_ts()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn finish_execution(&self, execution_id: &str, status: ExecutionStatus) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "UPDATE cache_executions SET status = ?2, finished_at = ?3 WHERE execution_id = ?1",
            params![execution_id, status.as_str(), now_ts()],
        )?;
        Ok(())
    }

    pub fn find_completed_by_request_hash(
        &self,
        backend: &str,
        request_hash: &str,
    ) -> Result<Option<CachedExecution>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, \
                    started_at, finished_at, fencing_token \
             FROM cache_executions \
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'completed' \
             ORDER BY finished_at DESC, fencing_token DESC, started_at DESC LIMIT 1",
            params![backend, request_hash],
            row_to_cached_execution,
        )
        .optional()
        .context("Failed to find completed cached execution")
    }

    pub fn find_running_by_request_hash(
        &self,
        backend: &str,
        request_hash: &str,
    ) -> Result<Option<CachedExecution>> {
        let now = now_ts();
        let stale_before = now - RUNNING_EXECUTION_TTL_SECS;
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "UPDATE cache_executions SET status = 'failed', finished_at = ?3
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running' AND started_at < ?4",
            params![backend, request_hash, now, stale_before],
        )?;
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, \
                    started_at, finished_at, fencing_token \
             FROM cache_executions \
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running' \
             ORDER BY started_at ASC LIMIT 1",
            params![backend, request_hash],
            row_to_cached_execution,
        )
        .optional()
        .context("Failed to find running cached execution")
    }

    pub fn get_execution(&self, execution_id: &str) -> Result<Option<CachedExecution>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, \
                    started_at, finished_at, fencing_token \
             FROM cache_executions WHERE execution_id = ?1",
            params![execution_id],
            row_to_cached_execution,
        )
        .optional()
        .context("Failed to read cached execution")
    }

    /// Replay all stored Output events for the given execution, concatenated.
    pub fn output_text(&self, execution_id: &str) -> Result<Option<String>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT event_json FROM cache_outputs \
             WHERE execution_id = ?1 ORDER BY seq ASC",
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

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn init(&self) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cache_executions (
    execution_id  TEXT PRIMARY KEY,
    session_id    TEXT NOT NULL,
    backend       TEXT NOT NULL,
    request_hash  TEXT NOT NULL,
    status        TEXT NOT NULL,
    started_at    INTEGER NOT NULL,
    finished_at   INTEGER,
    fencing_token INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS cache_outputs (
    execution_id TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    event_json   TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (execution_id, seq)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cache_running_lock
    ON cache_executions(backend, request_hash) WHERE status = 'running';",
        )?;
        // Purge records older than 30 days to bound database growth.
        purge_old_records(&conn);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// SHA-256 hash that uniquely identifies a (backend, cwd, prompt) triple.
pub fn request_hash(backend: &str, cwd: &Path, prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(backend.as_bytes());
    hasher.update(b"\0");
    hasher.update(cwd.as_os_str().to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(prompt.as_bytes());
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn row_to_cached_execution(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedExecution> {
    let status = row.get::<_, String>(4)?;
    Ok(CachedExecution {
        execution_id: row.get(0)?,
        session_id: row.get(1)?,
        backend: row.get(2)?,
        request_hash: row.get(3)?,
        status: ExecutionStatus::from(status.as_str()),
        started_at: row.get(5)?,
        finished_at: row.get(6)?,
        fencing_token: row.get(7)?,
    })
}

fn purge_old_records(conn: &Connection) {
    const RETENTION_DAYS: i64 = 30;
    let cutoff = now_ts() - RETENTION_DAYS * 86_400;
    let _ = conn.execute(
        "DELETE FROM cache_outputs WHERE execution_id IN (
           SELECT execution_id FROM cache_executions
           WHERE status IN ('completed', 'failed')
           AND finished_at IS NOT NULL
           AND finished_at < ?1
         )",
        params![cutoff],
    );
    let _ = conn.execute(
        "DELETE FROM cache_executions
         WHERE status IN ('completed', 'failed')
         AND finished_at IS NOT NULL
         AND finished_at < ?1",
        params![cutoff],
    );
}
