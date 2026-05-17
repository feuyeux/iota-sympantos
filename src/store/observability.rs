use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::runtime_event::TokenUsageEvent;
use crate::utils::now_ts;

#[derive(Clone)]
pub struct ObservabilityStore {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokenUsage {
    pub id: String,
    pub ts: i64,
    pub execution_id: Option<String>,
    pub session_id: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub source: String,
    pub input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub thinking_tokens: Option<u64>,
    pub tool_use_prompt_tokens: Option<u64>,
    pub provider_reported_total_tokens: Option<u64>,
    pub normalized_total_tokens: Option<u64>,
    pub raw_payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageSummary {
    pub backend: String,
    pub count: u64,
    pub input_tokens_mean: Option<f64>,
    pub cache_read_input_tokens_mean: Option<f64>,
    pub cache_creation_input_tokens_mean: Option<f64>,
    pub output_tokens_mean: Option<f64>,
    pub thinking_tokens_mean: Option<f64>,
    pub provider_reported_total_mean: Option<f64>,
    pub normalized_total_mean: Option<f64>,
}

impl ObservabilityStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open observability store {}", path.display()))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init()?;
        Ok(store)
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::config::paths::StorePaths::resolve()?.events_db())
    }

    pub fn record_token_usage(
        &self,
        execution_id: Option<&str>,
        session_id: Option<&str>,
        backend: &str,
        usage: &TokenUsageEvent,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let raw_payload_json = serde_json::to_string(&usage.raw_payload)?;
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "INSERT INTO token_usage_events (
                id, ts, execution_id, session_id, backend, model, provider, source,
                input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                output_tokens, thinking_tokens, tool_use_prompt_tokens,
                provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                &id,
                now_ts(),
                execution_id,
                session_id.or(usage.session_id.as_deref()),
                backend,
                usage.model.as_deref(),
                usage.provider.as_deref(),
                usage.source.as_deref().unwrap_or("unknown"),
                opt_i64(usage.input_tokens),
                opt_i64(usage.cache_read_input_tokens),
                opt_i64(usage.cache_creation_input_tokens),
                opt_i64(usage.output_tokens),
                opt_i64(usage.thinking_tokens),
                opt_i64(usage.tool_use_prompt_tokens),
                opt_i64(usage.provider_reported_total_tokens),
                opt_i64(usage.normalized_total_tokens),
                raw_payload_json,
            ],
        )?;
        Ok(id)
    }

    pub fn recent_token_usage(&self, limit: usize) -> Result<Vec<StoredTokenUsage>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, ts, execution_id, session_id, backend, model, provider, source,
                    input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                    output_tokens, thinking_tokens, tool_use_prompt_tokens,
                    provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             FROM token_usage_events
             ORDER BY ts DESC, id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_token_usage)?;
        collect_rows(rows)
    }

    pub fn recent_token_executions(&self, limit: usize) -> Result<Vec<StoredTokenUsage>> {
        let events = self.recent_token_usage(limit.saturating_mul(20).max(limit))?;
        let mut best_by_execution: BTreeMap<String, StoredTokenUsage> = BTreeMap::new();
        for event in events {
            let key = event
                .execution_id
                .clone()
                .unwrap_or_else(|| event.id.clone());
            match best_by_execution.get(&key) {
                Some(existing) if token_event_score(existing) >= token_event_score(&event) => {}
                _ => {
                    best_by_execution.insert(key, event);
                }
            }
        }
        let mut records = best_by_execution.into_values().collect::<Vec<_>>();
        records.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.id.cmp(&a.id)));
        records.truncate(limit);
        Ok(records)
    }

    pub fn token_usage_for_execution(&self, execution_id: &str) -> Result<Vec<StoredTokenUsage>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, ts, execution_id, session_id, backend, model, provider, source,
                    input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                    output_tokens, thinking_tokens, tool_use_prompt_tokens,
                    provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             FROM token_usage_events
             WHERE execution_id = ?1
             ORDER BY ts ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![execution_id], row_to_token_usage)?;
        collect_rows(rows)
    }

    pub fn token_summary_since(&self, since_ts: i64) -> Result<Vec<TokenUsageSummary>> {
        let events = self.token_usage_since(since_ts)?;
        let mut best_by_execution: BTreeMap<String, StoredTokenUsage> = BTreeMap::new();
        for event in events {
            let key = event
                .execution_id
                .clone()
                .unwrap_or_else(|| event.id.clone());
            match best_by_execution.get(&key) {
                Some(existing) if token_event_score(existing) >= token_event_score(&event) => {}
                _ => {
                    best_by_execution.insert(key, event);
                }
            }
        }
        let mut by_backend: BTreeMap<String, SummaryAccumulator> = BTreeMap::new();
        for event in best_by_execution.values() {
            by_backend
                .entry(event.backend.clone())
                .or_default()
                .add(event);
        }
        return Ok(by_backend
            .into_iter()
            .map(|(backend, acc)| acc.finish(backend))
            .collect());
    }

    fn token_usage_since(&self, since_ts: i64) -> Result<Vec<StoredTokenUsage>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, ts, execution_id, session_id, backend, model, provider, source,
                    input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                    output_tokens, thinking_tokens, tool_use_prompt_tokens,
                    provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             FROM token_usage_events
             WHERE ts >= ?1
             ORDER BY ts DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![since_ts], row_to_token_usage)?;
        collect_rows(rows)
    }

    fn init(&self) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS token_usage_events (
              id TEXT PRIMARY KEY,
              ts INTEGER NOT NULL,
              execution_id TEXT,
              session_id TEXT,
              backend TEXT NOT NULL,
              model TEXT,
              provider TEXT,
              source TEXT NOT NULL,
              input_tokens INTEGER,
              cache_read_input_tokens INTEGER,
              cache_creation_input_tokens INTEGER,
              output_tokens INTEGER,
              thinking_tokens INTEGER,
              tool_use_prompt_tokens INTEGER,
              provider_reported_total_tokens INTEGER,
              normalized_total_tokens INTEGER,
              raw_payload_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_token_usage_execution ON token_usage_events(execution_id, ts);
            CREATE INDEX IF NOT EXISTS idx_token_usage_backend ON token_usage_events(backend, ts);",
        )?;
        Ok(())
    }
}

#[derive(Default)]
struct SummaryAccumulator {
    count: u64,
    input: MeanAccumulator,
    cache_read: MeanAccumulator,
    cache_creation: MeanAccumulator,
    output: MeanAccumulator,
    thinking: MeanAccumulator,
    provider_total: MeanAccumulator,
    normalized_total: MeanAccumulator,
}

impl SummaryAccumulator {
    fn add(&mut self, event: &StoredTokenUsage) {
        self.count += 1;
        self.input.add(event.input_tokens);
        self.cache_read.add(event.cache_read_input_tokens);
        self.cache_creation.add(event.cache_creation_input_tokens);
        self.output.add(event.output_tokens);
        self.thinking.add(event.thinking_tokens);
        self.provider_total
            .add(event.provider_reported_total_tokens);
        self.normalized_total.add(event.normalized_total_tokens);
    }

    fn finish(self, backend: String) -> TokenUsageSummary {
        TokenUsageSummary {
            backend,
            count: self.count,
            input_tokens_mean: self.input.mean(),
            cache_read_input_tokens_mean: self.cache_read.mean(),
            cache_creation_input_tokens_mean: self.cache_creation.mean(),
            output_tokens_mean: self.output.mean(),
            thinking_tokens_mean: self.thinking.mean(),
            provider_reported_total_mean: self.provider_total.mean(),
            normalized_total_mean: self.normalized_total.mean(),
        }
    }
}

#[derive(Default)]
struct MeanAccumulator {
    sum: u64,
    count: u64,
}

impl MeanAccumulator {
    fn add(&mut self, value: Option<u64>) {
        if let Some(value) = value {
            self.sum += value;
            self.count += 1;
        }
    }

    fn mean(&self) -> Option<f64> {
        (self.count > 0).then(|| self.sum as f64 / self.count as f64)
    }
}

fn token_event_score(event: &StoredTokenUsage) -> u8 {
    let mut score = 0;
    // Prefer official backend-reported totals over computed
    if event.provider_reported_total_tokens.is_some() {
        score += 5;
    }
    // Normalized/computed totals are more authoritative
    if event.normalized_total_tokens.is_some() {
        score += 4;
    }
    // Prefer sources other than partial session updates
    if event.source != "session_update.usage_update" {
        score += 2;
    }
    // Individual token counts
    if event.input_tokens.is_some() {
        score += 1;
    }
    if event.output_tokens.is_some() {
        score += 1;
    }
    score
}

/// Validate token counts: provider total should be >= (input + output + thinking).
/// Returns error reason if validation fails.
fn validate_token_counts(event: &StoredTokenUsage) -> Option<String> {
    let provider_total = event.provider_reported_total_tokens.unwrap_or(0);
    let computed = event
        .input_tokens
        .unwrap_or(0)
        .saturating_add(event.output_tokens.unwrap_or(0))
        .saturating_add(event.thinking_tokens.unwrap_or(0));
    if provider_total > 0 && computed > 0 && computed > provider_total {
        return Some(format!(
            "computed tokens ({}) exceed provider total ({})",
            computed, provider_total
        ));
    }
    None
}

fn opt_i64(value: Option<u64>) -> Option<i64> {
    value.and_then(|value| value.try_into().ok())
}

fn opt_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| value.try_into().ok())
}

fn row_to_token_usage(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredTokenUsage> {
    let raw_payload_json: String = row.get(16)?;
    let raw_payload = serde_json::from_str(&raw_payload_json).unwrap_or(Value::Null);
    Ok(StoredTokenUsage {
        id: row.get(0)?,
        ts: row.get(1)?,
        execution_id: row.get(2)?,
        session_id: row.get(3)?,
        backend: row.get(4)?,
        model: row.get(5)?,
        provider: row.get(6)?,
        source: row.get(7)?,
        input_tokens: opt_u64(row.get(8)?),
        cache_read_input_tokens: opt_u64(row.get(9)?),
        cache_creation_input_tokens: opt_u64(row.get(10)?),
        output_tokens: opt_u64(row.get(11)?),
        thinking_tokens: opt_u64(row.get(12)?),
        tool_use_prompt_tokens: opt_u64(row.get(13)?),
        provider_reported_total_tokens: opt_u64(row.get(14)?),
        normalized_total_tokens: opt_u64(row.get(15)?),
        raw_payload,
    })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>> {
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}
