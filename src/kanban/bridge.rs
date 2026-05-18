use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::shadow::ShadowMaterializer;
use super::store::KanbanStore;
use super::types::*;

pub struct AdvancedBridge {
    hermes_bin: PathBuf,
    timeout: Duration,
    materializer: ShadowMaterializer,
}

pub struct SpecifyResult {
    pub task_id: TaskId,
    pub spec_body: String,
}

pub struct DecomposeResult {
    pub parent_id: TaskId,
    pub child_ids: Vec<TaskId>,
}

impl AdvancedBridge {
    pub fn new(hermes_bin: PathBuf, shadows_dir: PathBuf) -> Self {
        Self {
            hermes_bin,
            timeout: Duration::from_secs(120),
            materializer: ShadowMaterializer::new(shadows_dir),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if hermes binary is available
    pub fn is_available(&self) -> bool {
        Command::new(&self.hermes_bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Expand a vague task description into a structured spec via LLM
    pub fn specify(&self, task_id: TaskId, store: &dyn KanbanStore) -> Result<SpecifyResult> {
        let task = store.get_task(task_id)?;
        let board = self.get_board_for_task(&task, store)?;
        let shadow = self.materializer.materialize(&task, &board, store)?;

        let output = Command::new(&self.hermes_bin)
            .args(["kanban", "specify", &task_id.to_string(), "--json"])
            .env("HERMES_KANBAN_DB", &shadow.path)
            .env("HERMES_KANBAN_BOARD", &board.slug)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| "failed to run hermes kanban specify")?;

        self.materializer.cleanup(task_id)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("hermes kanban specify failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Try to parse as JSON, fall back to raw text as the spec body
        let spec_body = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stdout) {
            parsed.get("spec").and_then(|v| v.as_str())
                .or_else(|| parsed.get("body").and_then(|v| v.as_str()))
                .unwrap_or(&stdout)
                .to_string()
        } else {
            stdout.trim().to_string()
        };

        // Write spec back to task body
        store.update_task(task_id, super::types::TaskPatch {
            body: Some(Some(spec_body.clone())),
            ..Default::default()
        })?;

        Ok(SpecifyResult { task_id, spec_body })
    }

    /// Decompose a large task into subtasks via LLM
    pub fn decompose(&self, task_id: TaskId, store: &dyn KanbanStore) -> Result<DecomposeResult> {
        let task = store.get_task(task_id)?;
        let board = self.get_board_for_task(&task, store)?;
        let shadow = self.materializer.materialize(&task, &board, store)?;

        let output = Command::new(&self.hermes_bin)
            .args(["kanban", "decompose", &task_id.to_string(), "--json"])
            .env("HERMES_KANBAN_DB", &shadow.path)
            .env("HERMES_KANBAN_BOARD", &board.slug)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| "failed to run hermes kanban decompose")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.materializer.cleanup(task_id)?;
            bail!("hermes kanban decompose failed: {}", stderr.trim());
        }

        // Read new tasks from shadow DB (hermes creates them there)
        let shadow_conn = rusqlite::Connection::open(&shadow.path)?;
        let mut stmt = shadow_conn.prepare(
            "SELECT id, title, body, assignee FROM tasks WHERE id != ?1"
        )?;
        let new_tasks: Vec<(i64, String, Option<String>, Option<String>)> = stmt
            .query_map(rusqlite::params![task_id as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.materializer.cleanup(task_id)?;

        // Import new tasks into iota store
        let mut child_ids = Vec::new();
        for (_shadow_id, title, body, assignee) in new_tasks {
            let new_id = store.create_task(CreateTaskRequest {
                board_id: task.board_id,
                title,
                body,
                status: None, // triage
                assignee,
                priority: Some(task.priority),
                tags: task.tags.clone(),
                workspace_kind: task.workspace_kind.clone(),
                workspace_path: task.workspace_path.clone(),
            })?;
            store.create_link(task_id, new_id, LinkKind::Parent)?;
            child_ids.push(new_id);
        }

        Ok(DecomposeResult { parent_id: task_id, child_ids })
    }

    fn get_board_for_task(&self, task: &Task, store: &dyn KanbanStore) -> Result<Board> {
        store.list_boards()?
            .into_iter()
            .find(|b| b.id == task.board_id)
            .ok_or_else(|| anyhow::anyhow!("board not found for task {}", task.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kanban::sqlite_store::SqliteKanbanStore;
    use crate::kanban::store::KanbanStore;
    use std::path::Path;

    #[test]
    fn bridge_is_available_false_for_missing_binary() {
        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bridge = AdvancedBridge::new(
            PathBuf::from("/nonexistent/hermes-binary-xyz"),
            tmp.clone(),
        );
        assert!(!bridge.is_available());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn specify_fails_gracefully_when_hermes_missing() {
        let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let board_id = store.create_board("b", "B").unwrap();
        let task_id = store.create_task(CreateTaskRequest {
            board_id, title: "Vague task".into(), body: Some("do stuff".into()),
            status: None, assignee: None, priority: None, tags: vec![],
            workspace_kind: None, workspace_path: None,
        }).unwrap();

        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bridge = AdvancedBridge::new(
            PathBuf::from("/nonexistent/hermes-xyz"),
            tmp.clone(),
        );

        let result = bridge.specify(task_id, &store);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn decompose_fails_gracefully_when_hermes_missing() {
        let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let board_id = store.create_board("b", "B").unwrap();
        let task_id = store.create_task(CreateTaskRequest {
            board_id, title: "Big task".into(), body: None,
            status: None, assignee: None, priority: None, tags: vec![],
            workspace_kind: None, workspace_path: None,
        }).unwrap();

        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bridge = AdvancedBridge::new(
            PathBuf::from("/nonexistent/hermes-xyz"),
            tmp.clone(),
        );

        let result = bridge.decompose(task_id, &store);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn with_timeout_configures_bridge() {
        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bridge = AdvancedBridge::new(PathBuf::from("hermes"), tmp.clone())
            .with_timeout(Duration::from_secs(60));
        assert_eq!(bridge.timeout, Duration::from_secs(60));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
