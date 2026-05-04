use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::acp::AcpPromptTiming;
use crate::runtime_event::{OutputEvent, RuntimeEvent};
use crate::utils::now_ts;

const RUNNING_EXECUTION_TTL_SECS: i64 = 60 * 60;
const METRICS_SAMPLE_LIMIT: usize = 10_000;

#[derive(Clone)]
pub struct EventStore {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub session_id: String,
    pub backend: String,
    pub request_hash: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub fencing_token: i64,
    pub process_spawn_ms: Option<u64>,
    pub init_ms: Option<u64>,
    pub session_new_ms: Option<u64>,
    pub prompt_ms: Option<u64>,
    pub total_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ObservabilitySummary {
    pub total_executions: u64,
    pub completed_executions: u64,
    pub failed_executions: u64,
    pub running_executions: u64,
    pub avg_total_ms: Option<f64>,
    pub avg_prompt_ms: Option<f64>,
    pub p95_total_ms: Option<u64>,
    pub token_usage: TokenUsageSummary,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub active_sessions: u64,
    pub queued_prompts: u64,
    pub latest: Vec<ExecutionRecord>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TokenUsageSummary {
    pub events: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PrometheusMetrics {
    pub execution_attempts: u64,
    pub execution_completed: u64,
    pub execution_failed: u64,
    pub execution_running: u64,
    pub avg_total_ms: Option<f64>,
    pub avg_prompt_ms: Option<f64>,
    pub p95_total_ms: Option<u64>,
    pub prompt_latency_ms: Vec<u64>,
    pub init_latency_ms: Vec<u64>,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub active_sessions: u64,
    pub queued_prompts: u64,
    pub token_usage: TokenUsageSummary,
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
            "UPDATE executions SET status = 'failed', finished_at = ?3
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running' AND started_at < ?4",
            params![backend, request_hash, now, stale_before],
        )?;
        if let Some(existing) = tx
            .query_row(
                "SELECT request_hash FROM executions WHERE execution_id = ?1",
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
                "SELECT COALESCE(MAX(fencing_token), 0) + 1 FROM executions",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);
        tx.execute(
            "INSERT INTO executions (execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token) VALUES (?1, ?2, ?3, ?4, 'running', ?5, NULL, ?6)",
            params![&execution_id, session_id, backend, request_hash, now, fencing_token],
        )?;
        tx.commit()?;
        Ok(execution_id)
    }

    pub fn append_event(&self, execution_id: &str, event: &RuntimeEvent) -> Result<i64> {
        let event_json =
            serde_json::to_string(event).context("Failed to serialize runtime event")?;
        let mut conn = crate::utils::lock_or_recover(&self.conn);
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let next_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE execution_id = ?1",
                params![execution_id],
                |row| row.get(0),
            )
            .context("Failed to allocate event seq")?;
        tx.execute(
            "INSERT INTO events (execution_id, seq, event_type, event_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![execution_id, next_seq, event.event_type(), event_json, now_ts()],
        )?;
        tx.commit()?;
        Ok(next_seq)
    }

    pub fn finish_execution(&self, execution_id: &str, status: &str) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "UPDATE executions SET status = ?2, finished_at = ?3 WHERE execution_id = ?1",
            params![execution_id, status, now_ts()],
        )?;
        Ok(())
    }

    pub fn record_timing(&self, execution_id: &str, timing: &AcpPromptTiming) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "UPDATE executions SET process_spawn_ms = ?2, init_ms = ?3, session_new_ms = ?4, prompt_ms = ?5, total_ms = ?6 WHERE execution_id = ?1",
            params![
                execution_id,
                opt_u64_to_i64(timing.process_spawn_ms),
                opt_u64_to_i64(timing.init_ms),
                opt_u64_to_i64(timing.session_new_ms),
                Some(u64_to_i64(timing.prompt_ms)),
                Some(u64_to_i64(timing.total_ms)),
            ],
        )?;
        Ok(())
    }

    pub fn record_cache_hit(&self) -> Result<()> {
        self.increment_observability_counter("cache_hit", 1)
    }

    pub fn record_cache_miss(&self) -> Result<()> {
        self.increment_observability_counter("cache_miss", 1)
    }

    pub fn set_active_sessions(&self, value: u64) -> Result<()> {
        self.set_observability_gauge("active_sessions", value)
    }

    pub fn set_queued_prompts(&self, value: u64) -> Result<()> {
        self.set_observability_gauge("queued_prompts", value)
    }

    pub fn find_completed_by_request_hash(
        &self,
        backend: &str,
        request_hash: &str,
    ) -> Result<Option<ExecutionRecord>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token, process_spawn_ms, init_ms, session_new_ms, prompt_ms, total_ms FROM executions\n             WHERE backend = ?1 AND request_hash = ?2 AND status = 'completed'\n             ORDER BY finished_at DESC, fencing_token DESC, started_at DESC LIMIT 1",
            params![backend, request_hash],
            row_to_execution_record,
        )
        .optional()
        .context("Failed to find completed execution")
    }

    pub fn output_text(&self, execution_id: &str) -> Result<Option<String>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
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
        let conn = crate::utils::lock_or_recover(&self.conn);
        let now = now_ts();
        let stale_before = now - RUNNING_EXECUTION_TTL_SECS;
        conn.execute(
            "UPDATE executions SET status = 'failed', finished_at = ?3
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running' AND started_at < ?4",
            params![backend, request_hash, now, stale_before],
        )?;
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token, process_spawn_ms, init_ms, session_new_ms, prompt_ms, total_ms FROM executions
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running'
             ORDER BY started_at ASC LIMIT 1",
            params![backend, request_hash],
            row_to_execution_record,
        )
        .optional()
        .context("Failed to find running execution")
    }

    pub fn get_execution(&self, execution_id: &str) -> Result<Option<ExecutionRecord>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token, process_spawn_ms, init_ms, session_new_ms, prompt_ms, total_ms FROM executions WHERE execution_id = ?1",
            params![execution_id],
            row_to_execution_record,
        )
        .optional()
        .context("Failed to read execution")
    }

    pub fn executions_by_status(&self, status: &str, limit: usize) -> Result<Vec<ExecutionRecord>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX).max(0);
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token, process_spawn_ms, init_ms, session_new_ms, prompt_ms, total_ms FROM executions WHERE status = ?1 ORDER BY started_at DESC, fencing_token DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![status, limit], row_to_execution_record)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn execution_events(&self, execution_id: &str) -> Result<Vec<(i64, String, RuntimeEvent)>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT seq, event_type, event_json FROM events WHERE execution_id = ?1 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![execution_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (seq, event_type, event_json) = row?;
            if let Ok(event) = serde_json::from_str::<RuntimeEvent>(&event_json) {
                events.push((seq, event_type, event));
            }
        }
        Ok(events)
    }

    pub fn events_since(
        &self,
        execution_id: &str,
        after_seq: i64,
    ) -> Result<Vec<(i64, RuntimeEvent)>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT seq, event_json FROM events WHERE execution_id = ?1 AND seq > ?2 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![execution_id, after_seq], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
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

    pub fn slowest_executions(&self, limit: usize) -> Result<Vec<ExecutionRecord>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX).max(0);
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token, process_spawn_ms, init_ms, session_new_ms, prompt_ms, total_ms FROM executions WHERE total_ms IS NOT NULL ORDER BY total_ms DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_execution_record)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn recent_executions(&self, limit: usize) -> Result<Vec<ExecutionRecord>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX).max(0);
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token, process_spawn_ms, init_ms, session_new_ms, prompt_ms, total_ms FROM executions ORDER BY started_at DESC, fencing_token DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_execution_record)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn observability_summary(&self, limit: usize) -> Result<ObservabilitySummary> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let total_executions = count_status(&conn, None)?;
        let completed_executions = count_status(&conn, Some("completed"))?;
        let failed_executions = count_status(&conn, Some("failed"))?;
        let running_executions = count_status(&conn, Some("running"))?;
        let avg_total_ms = avg_column(&conn, TimingColumn::TotalMs)?;
        let avg_prompt_ms = avg_column(&conn, TimingColumn::PromptMs)?;
        let p95_total_ms = percentile_total_ms(&conn, 0.95)?;
        let token_usage = token_usage_summary(&conn)?;
        let cache_hits = counter_value(&conn, "cache_hit")?;
        let cache_misses = counter_value(&conn, "cache_miss")?;
        let active_sessions = gauge_value(&conn, "active_sessions")?;
        let queued_prompts = gauge_value(&conn, "queued_prompts")?;
        drop(conn);
        Ok(ObservabilitySummary {
            total_executions,
            completed_executions,
            failed_executions,
            running_executions,
            avg_total_ms,
            avg_prompt_ms,
            p95_total_ms,
            token_usage,
            cache_hits,
            cache_misses,
            active_sessions,
            queued_prompts,
            latest: self.recent_executions(limit)?,
        })
    }

    pub fn prometheus_metrics(&self) -> Result<PrometheusMetrics> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        Ok(PrometheusMetrics {
            execution_attempts: count_status(&conn, None)?,
            execution_completed: count_status(&conn, Some("completed"))?,
            execution_failed: count_status(&conn, Some("failed"))?,
            execution_running: count_status(&conn, Some("running"))?,
            avg_total_ms: avg_column(&conn, TimingColumn::TotalMs)?,
            avg_prompt_ms: avg_column(&conn, TimingColumn::PromptMs)?,
            p95_total_ms: percentile_total_ms_limited(&conn, 0.95, METRICS_SAMPLE_LIMIT)?,
            prompt_latency_ms: latency_values_limited(
                &conn,
                TimingColumn::PromptMs,
                METRICS_SAMPLE_LIMIT,
            )?,
            init_latency_ms: latency_values_limited(
                &conn,
                TimingColumn::InitMs,
                METRICS_SAMPLE_LIMIT,
            )?,
            cache_hits: counter_value(&conn, "cache_hit")?,
            cache_misses: counter_value(&conn, "cache_miss")?,
            active_sessions: gauge_value(&conn, "active_sessions")?,
            queued_prompts: gauge_value(&conn, "queued_prompts")?,
            token_usage: token_usage_summary(&conn)?,
        })
    }

    fn increment_observability_counter(&self, name: &str, by: u64) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "INSERT INTO observability_counters (name, value) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value = value + excluded.value",
            params![name, u64_to_i64(by)],
        )?;
        Ok(())
    }

    fn set_observability_gauge(&self, name: &str, value: u64) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "INSERT INTO observability_gauges (name, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![name, u64_to_i64(value), now_ts()],
        )?;
        Ok(())
    }

    fn init(&self) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (\n  execution_id TEXT NOT NULL,\n  seq INTEGER NOT NULL,\n  event_type TEXT NOT NULL,\n  event_json TEXT NOT NULL,\n  created_at INTEGER NOT NULL,\n  PRIMARY KEY (execution_id, seq)\n);\n\nCREATE TABLE IF NOT EXISTS executions (\n  execution_id TEXT PRIMARY KEY,\n  session_id TEXT NOT NULL,\n  backend TEXT NOT NULL,\n  request_hash TEXT NOT NULL,\n  status TEXT NOT NULL,\n  started_at INTEGER NOT NULL,\n  finished_at INTEGER,\n  fencing_token INTEGER NOT NULL DEFAULT 0,\n  process_spawn_ms INTEGER,\n  init_ms INTEGER,\n  session_new_ms INTEGER,\n  prompt_ms INTEGER,\n  total_ms INTEGER\n);",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS observability_counters (
  name TEXT PRIMARY KEY,
  value INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS observability_gauges (
  name TEXT PRIMARY KEY,
  value INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);",
        )?;
        let _ = conn.execute(
            "ALTER TABLE executions ADD COLUMN fencing_token INTEGER NOT NULL DEFAULT 0",
            [],
        );
        for column in [
            "process_spawn_ms",
            "init_ms",
            "session_new_ms",
            "prompt_ms",
            "total_ms",
        ] {
            let _ = conn.execute(
                &format!("ALTER TABLE executions ADD COLUMN {} INTEGER", column),
                [],
            );
        }
        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_executions_running_lock ON executions(backend, request_hash) WHERE status = 'running'",
            [],
        );
        // Purge records older than 30 days to bound database growth.
        self.purge_old_records(&conn);
        Ok(())
    }

    /// Deletes completed/failed executions (and their events) that finished
    /// more than `retention_days` days ago.
    fn purge_old_records(&self, conn: &Connection) {
        const RETENTION_DAYS: i64 = 30;
        let cutoff = now_ts() - RETENTION_DAYS * 86_400;
        // Remove stale event rows first (FK reference from events → executions).
        let _ = conn.execute(
            "DELETE FROM events WHERE execution_id IN (
               SELECT execution_id FROM executions
               WHERE status IN ('completed', 'failed')
               AND finished_at IS NOT NULL
               AND finished_at < ?1
             )",
            params![cutoff],
        );
        let _ = conn.execute(
            "DELETE FROM executions
             WHERE status IN ('completed', 'failed')
             AND finished_at IS NOT NULL
             AND finished_at < ?1",
            params![cutoff],
        );
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

fn row_to_execution_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExecutionRecord> {
    Ok(ExecutionRecord {
        execution_id: row.get(0)?,
        session_id: row.get(1)?,
        backend: row.get(2)?,
        request_hash: row.get(3)?,
        status: row.get(4)?,
        started_at: row.get(5)?,
        finished_at: row.get(6)?,
        fencing_token: row.get(7)?,
        process_spawn_ms: opt_i64_to_u64(row.get(8)?),
        init_ms: opt_i64_to_u64(row.get(9)?),
        session_new_ms: opt_i64_to_u64(row.get(10)?),
        prompt_ms: opt_i64_to_u64(row.get(11)?),
        total_ms: opt_i64_to_u64(row.get(12)?),
    })
}

fn count_status(conn: &Connection, status: Option<&str>) -> Result<u64> {
    let count: i64 = if let Some(status) = status {
        conn.query_row(
            "SELECT COUNT(*) FROM executions WHERE status = ?1",
            params![status],
            |row| row.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM executions", [], |row| row.get(0))?
    };
    Ok(count.try_into().unwrap_or(0))
}

/// A whitelist of column names that are safe to interpolate into SQL for
/// aggregation queries.  Never pass user-supplied strings to these functions.
#[derive(Clone, Copy)]
enum TimingColumn {
    TotalMs,
    PromptMs,
    InitMs,
}

impl TimingColumn {
    fn as_str(self) -> &'static str {
        match self {
            Self::TotalMs => "total_ms",
            Self::PromptMs => "prompt_ms",
            Self::InitMs => "init_ms",
        }
    }
}

fn avg_column(conn: &Connection, column: TimingColumn) -> Result<Option<f64>> {
    let col = column.as_str();
    let sql = format!("SELECT AVG({col}) FROM executions WHERE {col} IS NOT NULL");
    conn.query_row(&sql, [], |row| row.get(0))
        .context("Failed to calculate execution average")
}

fn latency_values_limited(
    conn: &Connection,
    column: TimingColumn,
    limit: usize,
) -> Result<Vec<u64>> {
    let col = column.as_str();
    let sql = format!(
        "SELECT {col} FROM executions WHERE {col} IS NOT NULL ORDER BY started_at DESC LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, i64>(0))?;
    let mut values = Vec::new();
    for row in rows {
        if let Some(value) = opt_i64_to_u64(Some(row?)) {
            values.push(value);
        }
    }
    Ok(values)
}

fn percentile_total_ms(conn: &Connection, percentile: f64) -> Result<Option<u64>> {
    percentile_total_ms_limited(conn, percentile, METRICS_SAMPLE_LIMIT)
}

fn percentile_total_ms_limited(
    conn: &Connection,
    percentile: f64,
    limit: usize,
) -> Result<Option<u64>> {
    let mut stmt = conn.prepare(
        "SELECT total_ms FROM (
           SELECT total_ms FROM executions WHERE total_ms IS NOT NULL ORDER BY started_at DESC LIMIT ?1
         ) ORDER BY total_ms ASC",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, i64>(0))?;
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    if values.is_empty() {
        return Ok(None);
    }
    let index = ((values.len() as f64 * percentile).ceil() as usize).saturating_sub(1);
    Ok(opt_i64_to_u64(Some(values[index.min(values.len() - 1)])))
}

fn token_usage_summary(conn: &Connection) -> Result<TokenUsageSummary> {
    let mut stmt = conn.prepare(
        "SELECT event_json FROM events WHERE event_type = 'token_usage' ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![METRICS_SAMPLE_LIMIT as i64], |row| {
        row.get::<_, String>(0)
    })?;
    let mut summary = TokenUsageSummary::default();
    for row in rows {
        let event_json = row?;
        let Ok(RuntimeEvent::TokenUsage(usage)) = serde_json::from_str(&event_json) else {
            continue;
        };
        summary.events += 1;
        summary.input_tokens += usage.input_tokens.unwrap_or(0);
        summary.output_tokens += usage.output_tokens.unwrap_or(0);
        summary.total_tokens += usage
            .total_tokens
            .unwrap_or_else(|| usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0));
    }
    Ok(summary)
}

fn counter_value(conn: &Connection, name: &str) -> Result<u64> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT value FROM observability_counters WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(opt_i64_to_u64(value).unwrap_or(0))
}

fn gauge_value(conn: &Connection, name: &str) -> Result<u64> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT value FROM observability_gauges WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(opt_i64_to_u64(value).unwrap_or(0))
}

fn opt_u64_to_i64(value: Option<u64>) -> Option<i64> {
    value.map(u64_to_i64)
}

fn u64_to_i64(value: u64) -> i64 {
    value.try_into().unwrap_or(i64::MAX)
}

fn opt_i64_to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| value.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_event::{
        ApprovalDecisionEvent, ApprovalRequestEvent, ErrorEvent, ToolCallEvent,
    };

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
        assert_eq!(
            stored.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert!(matches!(stored[0].1, RuntimeEvent::Output(_)));
        assert!(matches!(stored[1].1, RuntimeEvent::ToolCall(_)));
        assert!(matches!(stored[2].1, RuntimeEvent::ApprovalRequest(_)));
        assert!(matches!(stored[3].1, RuntimeEvent::ApprovalDecision(_)));
        assert!(matches!(stored[4].1, RuntimeEvent::Error(_)));
        assert_eq!(
            store.output_text(&execution_id).unwrap().as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn persists_execution_timing_and_summarizes() {
        let store = EventStore::open(Path::new(":memory:")).unwrap();
        let execution_id = store
            .begin_execution_with_id("codex", "session", "hash-a", Some("exec-timing"))
            .unwrap();
        store
            .record_timing(
                &execution_id,
                &AcpPromptTiming {
                    client_started: true,
                    process_spawned: true,
                    process_spawn_ms: Some(10),
                    init_ms: Some(20),
                    session_reused: false,
                    session_new_ms: Some(30),
                    prompt_ms: 40,
                    total_ms: 100,
                },
            )
            .unwrap();
        store.finish_execution(&execution_id, "completed").unwrap();

        let record = store.get_execution(&execution_id).unwrap().unwrap();
        assert_eq!(record.process_spawn_ms, Some(10));
        assert_eq!(record.init_ms, Some(20));
        assert_eq!(record.session_new_ms, Some(30));
        assert_eq!(record.prompt_ms, Some(40));
        assert_eq!(record.total_ms, Some(100));

        let summary = store.observability_summary(5).unwrap();
        assert_eq!(summary.total_executions, 1);
        assert_eq!(summary.completed_executions, 1);
        assert_eq!(summary.avg_total_ms, Some(100.0));
        assert_eq!(summary.p95_total_ms, Some(100));
        assert_eq!(summary.latest.len(), 1);
    }
}
