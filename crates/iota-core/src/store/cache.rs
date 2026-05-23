//! Minimal SQLite store for execution lifecycle records.
//!
//! This store preserves only execution identity, status, and timing functionality.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::utils::now_ts;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CacheStore {
    conn: Arc<Mutex<Connection>>,
    running_ttl_secs: i64,
    retention_days: i64,
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
                tracing::warn!(
                    status = other,
                    "unknown execution status read from cache store"
                );
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
        let cfg = crate::config::store_config();
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            running_ttl_secs: cfg.cache_running_ttl_secs,
            retention_days: cfg.cache_retention_days,
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
        let mut conn = self.lock_conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stale_before = now - self.running_ttl_secs;
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

    pub fn finish_execution(&self, execution_id: &str, status: ExecutionStatus) -> Result<()> {
        let conn = self.lock_conn();
        conn.execute(
            "UPDATE cache_executions SET status = ?2, finished_at = ?3 WHERE execution_id = ?1",
            params![execution_id, status.as_str(), now_ts()],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_execution_statuses(
        &self,
        execution_ids: &[&str],
    ) -> Result<Vec<(String, ExecutionStatus)>> {
        if execution_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.lock_conn();
        let placeholders = (1..=execution_ids.len())
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            "SELECT execution_id, status FROM cache_executions WHERE execution_id IN ({})",
            placeholders
        );
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(execution_ids.iter().copied()),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ExecutionStatus::from(row.get::<_, String>(1)?.as_str()),
                ))
            },
        )?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn lock_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| {
            tracing::error!("cache store connection mutex poisoned, recovering");
            poisoned.into_inner()
        })
    }

    fn init(&self) -> Result<()> {
        let conn = self.lock_conn();
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_cache_running_lock
    ON cache_executions(backend, request_hash) WHERE status = 'running';",
        )?;
        // Clean up stale running executions globally on startup.
        cleanup_stale_running(&conn, self.running_ttl_secs);
        // Purge records older than retention_days to bound database growth.
        purge_old_records(&conn, self.retention_days);
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

fn cleanup_stale_running(conn: &Connection, running_ttl_secs: i64) {
    let now = now_ts();
    let stale_before = now - running_ttl_secs;
    let _ = conn.execute(
        "UPDATE cache_executions SET status = 'failed', finished_at = ?1
         WHERE status = 'running' AND started_at < ?2",
        params![now, stale_before],
    );
}

fn purge_old_records(conn: &Connection, retention_days: i64) {
    let cutoff = now_ts() - retention_days * 86_400;
    let _ = conn.execute(
        "DELETE FROM cache_executions
         WHERE status IN ('completed', 'failed')
         AND finished_at IS NOT NULL
         AND finished_at < ?1",
        params![cutoff],
    );
}

/// Standalone deduplication query helper (e.g. for observability cross-checks).
#[allow(dead_code)]
pub fn get_execution_status(
    conn: &Connection,
    execution_id: &str,
) -> Result<Option<ExecutionStatus>> {
    conn.query_row(
        "SELECT status FROM cache_executions WHERE execution_id = ?1",
        params![execution_id],
        |row| Ok(ExecutionStatus::from(row.get::<_, String>(0)?.as_str())),
    )
    .optional()
    .context("Failed to query execution status")
}
