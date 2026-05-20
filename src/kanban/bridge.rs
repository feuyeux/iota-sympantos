use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
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

    #[cfg(test)]
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

        let result = (|| -> Result<Output> {
            let mut command = Command::new(&self.hermes_bin);
            command
                .args(["kanban", "specify", &task_id.to_string(), "--json"])
                .env("HERMES_KANBAN_DB", &shadow.path)
                .env("HERMES_KANBAN_BOARD", &board.slug);
            run_with_timeout(&mut command, self.timeout)
                .with_context(|| "failed to run hermes kanban specify")
        })();
        let cleanup_result = self.materializer.cleanup(task_id);
        let output = result?;
        cleanup_result?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("hermes kanban specify failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Try to parse as JSON, fall back to raw text as the spec body
        let spec_body = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stdout) {
            parsed
                .get("spec")
                .and_then(|v| v.as_str())
                .or_else(|| parsed.get("body").and_then(|v| v.as_str()))
                .unwrap_or(&stdout)
                .to_string()
        } else {
            stdout.trim().to_string()
        };

        // Write spec back to task body
        store.update_task(
            task_id,
            super::types::TaskPatch {
                body: Some(Some(spec_body.clone())),
                ..Default::default()
            },
        )?;

        Ok(SpecifyResult { task_id, spec_body })
    }

    /// Decompose a large task into subtasks via LLM
    pub fn decompose(&self, task_id: TaskId, store: &dyn KanbanStore) -> Result<DecomposeResult> {
        let task = store.get_task(task_id)?;
        let board = self.get_board_for_task(&task, store)?;
        let shadow = self.materializer.materialize(&task, &board, store)?;

        let existing_shadow_task_ids = read_shadow_task_ids(&shadow.path)?;

        let result = (|| -> Result<Vec<ShadowTaskRow>> {
            let mut command = Command::new(&self.hermes_bin);
            command
                .args(["kanban", "decompose", &task_id.to_string(), "--json"])
                .env("HERMES_KANBAN_DB", &shadow.path)
                .env("HERMES_KANBAN_BOARD", &board.slug);
            let output = run_with_timeout(&mut command, self.timeout)
                .with_context(|| "failed to run hermes kanban decompose")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("hermes kanban decompose failed: {}", stderr.trim());
            }

            read_new_shadow_tasks(&shadow.path, &existing_shadow_task_ids)
        })();
        let cleanup_result = self.materializer.cleanup(task_id);
        let new_tasks = result?;
        cleanup_result?;

        // Import new tasks into iota store
        let mut child_ids = Vec::new();
        for shadow_task in new_tasks {
            let new_id = store.create_task(CreateTaskRequest {
                board_id: task.board_id,
                title: shadow_task.title,
                body: shadow_task.body,
                status: None, // triage
                assignee: shadow_task.assignee,
                priority: Some(task.priority),
                tags: task.tags.clone(),
                workspace_kind: task.workspace_kind.clone(),
                workspace_path: task.workspace_path.clone(),
            })?;
            store.create_link(task_id, new_id, LinkKind::Parent)?;
            child_ids.push(new_id);
        }

        Ok(DecomposeResult {
            parent_id: task_id,
            child_ids,
        })
    }

    fn get_board_for_task(&self, task: &Task, store: &dyn KanbanStore) -> Result<Board> {
        store
            .list_boards()?
            .into_iter()
            .find(|b| b.id == task.board_id)
            .ok_or_else(|| anyhow::anyhow!("board not found for task {}", task.id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShadowTaskRow {
    title: String,
    body: Option<String>,
    assignee: Option<String>,
}

fn read_shadow_task_ids(db_path: &Path) -> Result<Vec<String>> {
    let conn = rusqlite::Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT id FROM tasks")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn read_new_shadow_tasks(db_path: &Path, existing_ids: &[String]) -> Result<Vec<ShadowTaskRow>> {
    let existing_ids: HashSet<&str> = existing_ids.iter().map(|s| s.as_str()).collect();
    let conn = rusqlite::Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT id, title, body, assignee FROM tasks ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ShadowTaskRow {
                title: row.get(1)?,
                body: row.get(2)?,
                assignee: row.get(3)?,
            },
        ))
    })?;

    let mut tasks = Vec::new();
    for row in rows {
        let (id, task) = row?;
        if !existing_ids.contains(id.as_str()) {
            tasks.push(task);
        }
    }
    Ok(tasks)
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output> {
    configure_process_tree_root(command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = std::time::Instant::now();

    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map_err(Into::into);
        }
        if started.elapsed() >= timeout {
            kill_child_tree(&mut child);
            let _ = child.wait_with_output();
            bail!("hermes command timed out after {}ms", timeout.as_millis());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn configure_process_tree_root(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

fn kill_child_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let pid = child.id().to_string();
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", pid)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
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
        let bridge =
            AdvancedBridge::new(PathBuf::from("/nonexistent/hermes-binary-xyz"), tmp.clone());
        assert!(!bridge.is_available());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn specify_fails_gracefully_when_hermes_missing() {
        let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let board_id = store.create_board("b", "B").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Vague task".into(),
                body: Some("do stuff".into()),
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();

        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bridge = AdvancedBridge::new(PathBuf::from("/nonexistent/hermes-xyz"), tmp.clone());

        let result = bridge.specify(task_id, &store);
        assert!(result.is_err());
        assert!(
            !tmp.join(task_id.to_string()).exists(),
            "shadow directory should be cleaned after specify spawn failure"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn decompose_fails_gracefully_when_hermes_missing() {
        let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let board_id = store.create_board("b", "B").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Big task".into(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();

        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bridge = AdvancedBridge::new(PathBuf::from("/nonexistent/hermes-xyz"), tmp.clone());

        let result = bridge.decompose(task_id, &store);
        assert!(result.is_err());
        assert!(
            !tmp.join(task_id.to_string()).exists(),
            "shadow directory should be cleaned after decompose spawn failure"
        );
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

    #[test]
    fn specify_respects_timeout() {
        let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let board_id = store.create_board("b", "B").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Slow task".into(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();

        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let hermes_path = if cfg!(windows) {
            let path = tmp.join("slow-hermes.cmd");
            std::fs::write(
                &path,
                "@echo off\r\npowershell -NoProfile -WindowStyle Hidden -Command Start-Sleep -Seconds 2\r\n",
            )
            .unwrap();
            path
        } else {
            let path = tmp.join("slow-hermes.sh");
            std::fs::write(&path, "#!/bin/sh\nsleep 2\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&path).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&path, perms).unwrap();
            }
            path
        };

        let bridge = AdvancedBridge::new(hermes_path, tmp.join("shadows"))
            .with_timeout(Duration::from_millis(50));
        let started = std::time::Instant::now();
        let result = bridge.specify(task_id, &store);

        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timeout should stop the command promptly"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn read_new_shadow_tasks_excludes_existing_materialized_tasks() {
        let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("kanban.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                body TEXT,
                assignee TEXT
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, title, body, assignee) VALUES ('1', 'parent', NULL, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, title, body, assignee) VALUES ('2', 'linked', NULL, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, title, body, assignee) VALUES ('3', 'new child', 'body', 'alice')",
            [],
        )
        .unwrap();
        drop(conn);

        let tasks = read_new_shadow_tasks(&db_path, &["1".to_string(), "2".to_string()]).unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "new child");
        assert_eq!(tasks[0].body.as_deref(), Some("body"));
        assert_eq!(tasks[0].assignee.as_deref(), Some("alice"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
