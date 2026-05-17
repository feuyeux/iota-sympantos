#![allow(dead_code)]

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::utils::now_ts;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApprovalDimension {
    Shell,
    FileOutsideWorkspace,
    Network,
    McpExternal,
    PrivilegeEscalation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicyDecision {
    pub approved: bool,
    pub reason: String,
}

#[derive(Clone)]
pub struct ApprovalStore {
    conn: Arc<Mutex<Connection>>,
}

impl ApprovalStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open approval store {}", path.display()))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init()?;
        Ok(store)
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::config::paths::StorePaths::resolve()?.approvals_db())
    }

    pub fn open_default() -> Result<Self> {
        Self::open(&Self::default_path()?)
    }

    pub fn record_request(
        &self,
        execution_id: Option<&str>,
        backend: &str,
        tool_name: &str,
        payload: &Value,
    ) -> Result<String> {
        let request_id = Uuid::new_v4().to_string();
        let payload_json = serde_json::to_string(payload)?;
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "INSERT INTO approval_requests (request_id, execution_id, backend, tool_name, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![request_id, execution_id, backend, tool_name, payload_json, now_ts()],
        )?;
        Ok(request_id)
    }

    pub fn record_decision(&self, request_id: &str, approved: bool, reason: &str) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute(
            "INSERT INTO approval_decisions (request_id, approved, reason, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![request_id, approved, reason, now_ts()],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_pending_requests(&self) -> Result<Vec<(String, String, String)>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT r.request_id, r.backend, r.tool_name FROM approval_requests r
             LEFT JOIN approval_decisions d ON r.request_id = d.request_id
             WHERE d.request_id IS NULL
             ORDER BY r.created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    #[allow(dead_code)]
    pub fn get_decision_history(
        &self,
        execution_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, String, bool)>> {
        let conn = crate::utils::lock_or_recover(&self.conn);

        let mut all_rows: Vec<(String, String, bool)> = Vec::new();

        if let Some(exec_id) = execution_id {
            let mut stmt = conn.prepare(
                "SELECT r.request_id, r.tool_name, d.approved FROM approval_requests r
                 JOIN approval_decisions d ON r.request_id = d.request_id
                 WHERE r.execution_id = ?1
                 ORDER BY d.created_at DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![exec_id, limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)? != 0,
                ))
            })?;
            for row in rows {
                all_rows.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT r.request_id, r.tool_name, d.approved FROM approval_requests r
                 JOIN approval_decisions d ON r.request_id = d.request_id
                 ORDER BY d.created_at DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)? != 0,
                ))
            })?;
            for row in rows {
                all_rows.push(row?);
            }
        }

        Ok(all_rows)
    }

    fn init(&self) -> Result<()> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS approval_requests (
  request_id TEXT PRIMARY KEY,
  execution_id TEXT,
  backend TEXT NOT NULL,
  tool_name TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS approval_decisions (
  request_id TEXT NOT NULL,
  approved INTEGER NOT NULL,
  reason TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(request_id) REFERENCES approval_requests(request_id)
);
CREATE INDEX IF NOT EXISTS idx_approval_decisions_order ON approval_decisions(request_id, created_at);
CREATE INDEX IF NOT EXISTS idx_approval_requests_execution ON approval_requests(execution_id, created_at);",
        )?;
        let _ = conn.execute(
            "ALTER TABLE approval_requests ADD COLUMN execution_id TEXT",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_approval_requests_execution ON approval_requests(execution_id, created_at)",
            [],
        );
        Ok(())
    }
}

pub fn classify_operation(tool_name: &str, payload: &serde_json::Value) -> Vec<ApprovalDimension> {
    let mut dimensions = Vec::new();
    let normalized = tool_name.to_lowercase();

    // ── Tool-name-based checks ────────────────────────────────────────────────
    if normalized.contains("shell") || normalized.contains("bash") || normalized.contains("exec") {
        dimensions.push(ApprovalDimension::Shell);
    }
    if normalized.starts_with("mcp") || normalized.contains("external") {
        dimensions.push(ApprovalDimension::McpExternal);
    }
    if normalized.contains("network") {
        dimensions.push(ApprovalDimension::Network);
    }

    // ── Payload field-based checks (structured, not full-JSON string match) ───
    // Collect only the specific string fields that carry path / command / URL
    // values to avoid false positives from unrelated text in the payload.
    let mut field_values: Vec<String> = Vec::new();
    for key in &[
        "command",
        "cmd",
        "path",
        "file",
        "url",
        "uri",
        "target",
        "destination",
    ] {
        if let Some(v) = payload.get(key).and_then(|v| v.as_str()) {
            field_values.push(v.to_lowercase());
        }
    }
    // Also check top-level string payload if the whole value is a string.
    if let Some(v) = payload.as_str() {
        field_values.push(v.to_lowercase());
    }

    for value in &field_values {
        if (value.contains("http://") || value.contains("https://") || value.contains("ftp://"))
            && !dimensions.contains(&ApprovalDimension::Network)
        {
            dimensions.push(ApprovalDimension::Network);
        }
        // Path traversal / system directory access
        if (value.contains("../")
            || value.contains("..\\")
            || value.starts_with("/etc/")
            || value.starts_with("/root/")
            || value.contains("c:\\windows")
            || value.contains("c:/windows"))
            && !dimensions.contains(&ApprovalDimension::FileOutsideWorkspace)
        {
            dimensions.push(ApprovalDimension::FileOutsideWorkspace);
        }
        // Privilege escalation indicators
        if (value.starts_with("sudo ")
            || value.contains(" sudo ")
            || value.contains("runas")
            || value.contains("administrator")
            || value.contains("privilege"))
            && !dimensions.contains(&ApprovalDimension::PrivilegeEscalation)
        {
            dimensions.push(ApprovalDimension::PrivilegeEscalation);
        }
    }

    dimensions
}

pub fn default_decision(dimensions: &[ApprovalDimension]) -> ApprovalPolicyDecision {
    if dimensions.is_empty() {
        return ApprovalPolicyDecision {
            approved: false,
            reason: "manual approval required".to_string(),
        };
    }
    ApprovalPolicyDecision {
        approved: false,
        reason: format!("manual approval required for {:?}", dimensions),
    }
}

#[cfg(test)]
#[path = "approvals_tests.rs"]
mod tests;
