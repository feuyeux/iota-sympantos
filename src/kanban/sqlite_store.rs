use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::state_machine::validate_transition;
use super::store::KanbanStore;
use super::types::*;
use crate::utils::{lock_or_recover, now_ts};

// --- Schema -------------------------------------------------------------------

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS boards (
    id          INTEGER PRIMARY KEY,
    slug        TEXT    UNIQUE NOT NULL,
    name        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    id              INTEGER PRIMARY KEY,
    board_id        INTEGER NOT NULL REFERENCES boards(id),
    title           TEXT    NOT NULL,
    body            TEXT,
    status          TEXT    NOT NULL DEFAULT 'triage',
    assignee        TEXT,
    priority        INTEGER NOT NULL DEFAULT 0,
    tags            TEXT    NOT NULL DEFAULT '[]',
    workspace_kind  TEXT,
    workspace_path  TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    claimed_at      INTEGER,
    claim_ttl_secs  INTEGER NOT NULL DEFAULT 900
);
CREATE TABLE IF NOT EXISTS links (
    from_id INTEGER NOT NULL,
    to_id   INTEGER NOT NULL,
    kind    TEXT    NOT NULL,
    PRIMARY KEY (from_id, to_id, kind)
);
CREATE TABLE IF NOT EXISTS comments (
    id          INTEGER PRIMARY KEY,
    task_id     INTEGER NOT NULL,
    author      TEXT    NOT NULL,
    body        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
    id              TEXT    PRIMARY KEY,
    task_id         INTEGER NOT NULL,
    profile         TEXT    NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'running',
    started_at      INTEGER NOT NULL,
    finished_at     INTEGER,
    last_heartbeat  INTEGER NOT NULL,
    exit_code       INTEGER,
    output_summary  TEXT
);
CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY,
    event_type  TEXT    NOT NULL,
    payload     TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_board_status ON tasks(board_id, status);
CREATE INDEX IF NOT EXISTS idx_tasks_assignee     ON tasks(assignee);
CREATE INDEX IF NOT EXISTS idx_runs_task          ON runs(task_id);
CREATE INDEX IF NOT EXISTS idx_comments_task      ON comments(task_id);
";

// --- Struct -------------------------------------------------------------------

#[derive(Clone)]
pub struct SqliteKanbanStore {
    conn: Arc<Mutex<Connection>>,
    event_tx: broadcast::Sender<KanbanUiEvent>,
}

impl SqliteKanbanStore {
    pub fn open(path: &Path) -> Result<Self> {
        if path != Path::new(":memory:") {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating kanban db dir {}", parent.display()))?;
            }
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening kanban db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        let (event_tx, _) = broadcast::channel(64);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            event_tx,
        })
    }

    fn lock_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        lock_or_recover(&self.conn)
    }

    /// Subscribe to real-time UI events emitted when store mutations occur.
    pub fn subscribe(&self) -> broadcast::Receiver<KanbanUiEvent> {
        self.event_tx.subscribe()
    }

    /// Look up the task_id associated with a run.
    fn task_id_for_run(&self, run_id: &str) -> Option<TaskId> {
        let conn = self.lock_conn();
        conn.query_row(
            "SELECT task_id FROM runs WHERE id = ?1",
            params![run_id],
            |row| row.get::<_, i64>(0),
        )
        .ok()
        .map(|v| v as TaskId)
    }

    // -- Board -----------------------------------------------------------------

    fn create_board_impl(&self, slug: &str, name: &str) -> Result<BoardId> {
        let now = now_ts();
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO boards (slug, name, created_at) VALUES (?1, ?2, ?3)",
            params![slug, name, now],
        )?;
        Ok(conn.last_insert_rowid() as u64)
    }

    fn list_boards_impl(&self) -> Result<Vec<Board>> {
        let conn = self.lock_conn();
        let mut stmt =
            conn.prepare("SELECT id, slug, name, created_at FROM boards ORDER BY slug")?;
        let rows = stmt.query_map([], row_to_board)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn get_board_impl(&self, slug: &str) -> Result<Board> {
        let conn = self.lock_conn();
        conn.query_row(
            "SELECT id, slug, name, created_at FROM boards WHERE slug = ?1",
            params![slug],
            row_to_board,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("board '{}' not found", slug),
            other => other.into(),
        })
    }

    // -- Task ------------------------------------------------------------------

    fn create_task_impl(&self, req: CreateTaskRequest) -> Result<TaskId> {
        let now = now_ts();
        let status = req.status.unwrap_or(Status::Triage).as_str();
        let priority = req.priority.unwrap_or(0);
        let tags_json = serde_json::to_string(&req.tags)?;
        let workspace_path = req
            .workspace_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO tasks
             (board_id, title, body, status, assignee, priority, tags,
              workspace_kind, workspace_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                req.board_id as i64,
                req.title,
                req.body,
                status,
                req.assignee,
                priority as i64,
                tags_json,
                req.workspace_kind,
                workspace_path,
                now,
            ],
        )?;
        Ok(conn.last_insert_rowid() as u64)
    }

    fn get_task_impl(&self, id: TaskId) -> Result<Task> {
        let conn = self.lock_conn();
        conn.query_row(
            "SELECT id, board_id, title, body, status, assignee, priority, tags,
                    workspace_kind, workspace_path, created_at, updated_at,
                    claimed_at, claim_ttl_secs
             FROM tasks WHERE id = ?1",
            params![id as i64],
            row_to_task,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("task {} not found", id),
            other => other.into(),
        })
    }

    fn update_task_impl(&self, id: TaskId, patch: TaskPatch) -> Result<()> {
        use rusqlite::types::Value;

        let mut parts: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let status_patch = patch.status;
        let now = now_ts();

        if let Some(v) = patch.title {
            parts.push(format!("title = ?{}", values.len() + 1));
            values.push(Value::Text(v));
        }
        if let Some(v) = patch.body {
            parts.push(format!("body = ?{}", values.len() + 1));
            values.push(v.map(Value::Text).unwrap_or(Value::Null));
        }
        if let Some(v) = status_patch {
            parts.push(format!("status = ?{}", values.len() + 1));
            values.push(Value::Text(v.as_str().to_owned()));
            if v == Status::Running {
                parts.push(format!("claimed_at = ?{}", values.len() + 1));
                values.push(Value::Integer(now));
            }
        }
        if let Some(v) = patch.assignee {
            parts.push(format!("assignee = ?{}", values.len() + 1));
            values.push(v.map(Value::Text).unwrap_or(Value::Null));
        }
        if let Some(v) = patch.priority {
            parts.push(format!("priority = ?{}", values.len() + 1));
            values.push(Value::Integer(v as i64));
        }
        if let Some(v) = patch.tags {
            parts.push(format!("tags = ?{}", values.len() + 1));
            values.push(Value::Text(serde_json::to_string(&v)?));
        }
        if let Some(v) = patch.workspace_kind {
            parts.push(format!("workspace_kind = ?{}", values.len() + 1));
            values.push(v.map(Value::Text).unwrap_or(Value::Null));
        }
        if let Some(v) = patch.workspace_path {
            parts.push(format!("workspace_path = ?{}", values.len() + 1));
            values.push(
                v.map(|p| Value::Text(p.to_string_lossy().into_owned()))
                    .unwrap_or(Value::Null),
            );
        }

        if parts.is_empty() {
            return Ok(());
        }

        parts.push(format!("updated_at = ?{}", values.len() + 1));
        values.push(Value::Integer(now));
        let id_param = values.len() + 1;
        let sql = format!(
            "UPDATE tasks SET {} WHERE id = ?{id_param}",
            parts.join(", ")
        );
        values.push(Value::Integer(id as i64));

        let conn = self.lock_conn();
        if let Some(to) = status_patch {
            let current_str: String = conn
                .query_row(
                    "SELECT status FROM tasks WHERE id = ?1",
                    params![id as i64],
                    |row| row.get(0),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        anyhow::anyhow!("task {} not found", id)
                    }
                    other => anyhow::Error::from(other),
                })?;
            let from: Status = current_str.parse()?;
            validate_transition(from, to)?;
        }
        let rows = conn.execute(&sql, rusqlite::params_from_iter(values))?;
        if rows == 0 {
            bail!("task {} not found", id);
        }
        Ok(())
    }

    fn list_tasks_impl(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        use rusqlite::types::Value;

        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();

        if let Some(board_id) = filter.board_id {
            conditions.push(format!("board_id = ?{}", values.len() + 1));
            values.push(Value::Integer(board_id as i64));
        }
        if let Some(status) = filter.status {
            conditions.push(format!("status = ?{}", values.len() + 1));
            values.push(Value::Text(status.as_str().to_owned()));
        }
        if let Some(assignee) = filter.assignee {
            conditions.push(format!("assignee = ?{}", values.len() + 1));
            values.push(Value::Text(assignee));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let limit_clause = match filter.limit {
            Some(n) => format!("LIMIT {n}"),
            None => String::new(),
        };
        let sql = format!(
            "SELECT id, board_id, title, body, status, assignee, priority, tags,
                    workspace_kind, workspace_path, created_at, updated_at,
                    claimed_at, claim_ttl_secs
             FROM tasks {where_clause}
             ORDER BY priority DESC, created_at ASC
             {limit_clause}"
        );

        let conn = self.lock_conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), row_to_task)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn delete_task_impl(&self, id: TaskId) -> Result<()> {
        let conn = self.lock_conn();
        let rows = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id as i64])?;
        if rows == 0 {
            bail!("task {} not found", id);
        }
        Ok(())
    }

    fn transition_impl(&self, id: TaskId, to: Status) -> Result<()> {
        let conn = self.lock_conn();
        let current_str: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![id as i64],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("task {} not found", id),
                other => anyhow::Error::from(other),
            })?;
        let from: Status = current_str.parse()?;
        validate_transition(from, to)?;
        let now = now_ts();
        if to == Status::Running {
            conn.execute(
                "UPDATE tasks SET status = ?1, claimed_at = ?2, updated_at = ?2 WHERE id = ?3",
                params![to.as_str(), now, id as i64],
            )?;
        } else {
            conn.execute(
                "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![to.as_str(), now, id as i64],
            )?;
        }
        Ok(())
    }

    // -- Links -----------------------------------------------------------------

    fn create_link_impl(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        let conn = self.lock_conn();
        conn.execute(
            "INSERT OR IGNORE INTO links (from_id, to_id, kind) VALUES (?1, ?2, ?3)",
            params![from as i64, to as i64, kind.as_str()],
        )?;
        Ok(())
    }

    fn remove_link_impl(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        let conn = self.lock_conn();
        conn.execute(
            "DELETE FROM links WHERE from_id = ?1 AND to_id = ?2 AND kind = ?3",
            params![from as i64, to as i64, kind.as_str()],
        )?;
        Ok(())
    }

    fn get_links_impl(&self, id: TaskId) -> Result<Vec<Link>> {
        let conn = self.lock_conn();
        let mut stmt = conn
            .prepare("SELECT from_id, to_id, kind FROM links WHERE from_id = ?1 OR to_id = ?1")?;
        let rows = stmt.query_map(params![id as i64], row_to_link)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // -- Comments --------------------------------------------------------------

    fn add_comment_impl(&self, task_id: TaskId, author: &str, body: &str) -> Result<CommentId> {
        let now = now_ts();
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO comments (task_id, author, body, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![task_id as i64, author, body, now],
        )?;
        Ok(conn.last_insert_rowid() as u64)
    }

    fn list_comments_impl(&self, task_id: TaskId) -> Result<Vec<Comment>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, author, body, created_at
             FROM comments WHERE task_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![task_id as i64], row_to_comment)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // -- Runs ------------------------------------------------------------------

    fn create_run_impl(&self, task_id: TaskId, profile: &str) -> Result<RunId> {
        let id = Uuid::new_v4().to_string();
        let now = now_ts();
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO runs (id, task_id, profile, status, started_at, last_heartbeat)
             VALUES (?1, ?2, ?3, 'running', ?4, ?4)",
            params![id, task_id as i64, profile, now],
        )?;
        Ok(id)
    }

    fn complete_run_impl(
        &self,
        run_id: &str,
        status: RunStatus,
        exit_code: Option<i32>,
    ) -> Result<()> {
        let now = now_ts();
        let conn = self.lock_conn();
        let rows = conn.execute(
            "UPDATE runs SET status = ?1, finished_at = ?2, exit_code = ?3 WHERE id = ?4",
            params![status.as_str(), now, exit_code, run_id],
        )?;
        if rows == 0 {
            bail!("run '{}' not found", run_id);
        }
        Ok(())
    }

    fn heartbeat_impl(&self, run_id: &str) -> Result<()> {
        let now = now_ts();
        let conn = self.lock_conn();
        let rows = conn.execute(
            "UPDATE runs SET last_heartbeat = ?1 WHERE id = ?2 AND status = 'running'",
            params![now, run_id],
        )?;
        if rows == 0 {
            bail!("run '{}' not found or already finished", run_id);
        }
        Ok(())
    }

    fn get_runs_impl(&self, task_id: TaskId) -> Result<Vec<Run>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, profile, status, started_at, finished_at,
                    last_heartbeat, exit_code, output_summary
             FROM runs WHERE task_id = ?1 ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map(params![task_id as i64], row_to_run)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // -- Events ----------------------------------------------------------------

    fn append_event_impl(&self, event_type: &str, payload: &str) -> Result<EventId> {
        let now = now_ts();
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO events (event_type, payload, created_at) VALUES (?1, ?2, ?3)",
            params![event_type, payload, now],
        )?;
        Ok(conn.last_insert_rowid() as u64)
    }

    fn events_since_impl(&self, cursor: EventId) -> Result<Vec<KanbanEvent>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, event_type, payload, created_at
             FROM events WHERE id > ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![cursor as i64], row_to_event)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

// --- Trait impl --------------------------------------------------------------

impl KanbanStore for SqliteKanbanStore {
    fn create_board(&self, slug: &str, name: &str) -> Result<BoardId> {
        self.create_board_impl(slug, name)
    }
    fn list_boards(&self) -> Result<Vec<Board>> {
        self.list_boards_impl()
    }
    fn get_board(&self, slug: &str) -> Result<Board> {
        self.get_board_impl(slug)
    }
    fn create_task(&self, req: CreateTaskRequest) -> Result<TaskId> {
        let title = req.title.clone();
        let id = self.create_task_impl(req)?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskCreated { id, title });
        Ok(id)
    }
    fn get_task(&self, id: TaskId) -> Result<Task> {
        self.get_task_impl(id)
    }
    fn update_task(&self, id: TaskId, patch: TaskPatch) -> Result<()> {
        self.update_task_impl(id, patch)?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskUpdated { id });
        Ok(())
    }
    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        self.list_tasks_impl(filter)
    }
    fn delete_task(&self, id: TaskId) -> Result<()> {
        self.delete_task_impl(id)?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskDeleted { id });
        Ok(())
    }
    fn transition(&self, id: TaskId, to: Status) -> Result<()> {
        let from = self.get_task(id)?.status;
        self.transition_impl(id, to)?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskStatusChanged { id, from, to });
        Ok(())
    }
    fn create_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.create_link_impl(from, to, kind)
    }
    fn remove_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.remove_link_impl(from, to, kind)
    }
    fn get_links(&self, id: TaskId) -> Result<Vec<Link>> {
        self.get_links_impl(id)
    }
    fn add_comment(&self, task_id: TaskId, author: &str, body: &str) -> Result<CommentId> {
        let comment_id = self.add_comment_impl(task_id, author, body)?;
        let _ = self.event_tx.send(KanbanUiEvent::CommentAdded { task_id, comment_id });
        Ok(comment_id)
    }
    fn list_comments(&self, task_id: TaskId) -> Result<Vec<Comment>> {
        self.list_comments_impl(task_id)
    }
    fn create_run(&self, task_id: TaskId, profile: &str) -> Result<RunId> {
        let run_id = self.create_run_impl(task_id, profile)?;
        let _ = self.event_tx.send(KanbanUiEvent::RunStarted { task_id, run_id: run_id.clone() });
        Ok(run_id)
    }
    fn complete_run(&self, run_id: &str, status: RunStatus, exit_code: Option<i32>) -> Result<()> {
        let task_id = self.task_id_for_run(run_id).unwrap_or(0);
        self.complete_run_impl(run_id, status, exit_code)?;
        let _ = self.event_tx.send(KanbanUiEvent::RunCompleted {
            task_id,
            run_id: run_id.to_string(),
            status,
        });
        Ok(())
    }
    fn heartbeat(&self, run_id: &str) -> Result<()> {
        self.heartbeat_impl(run_id)
    }
    fn get_runs(&self, task_id: TaskId) -> Result<Vec<Run>> {
        self.get_runs_impl(task_id)
    }
    fn append_event(&self, event_type: &str, payload: &str) -> Result<EventId> {
        self.append_event_impl(event_type, payload)
    }
    fn events_since(&self, cursor: EventId) -> Result<Vec<KanbanEvent>> {
        self.events_since_impl(cursor)
    }
}

// --- Row helpers -------------------------------------------------------------

fn row_to_board(row: &rusqlite::Row<'_>) -> rusqlite::Result<Board> {
    Ok(Board {
        id: row.get::<_, i64>(0)? as u64,
        slug: row.get(1)?,
        name: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let status_str: String = row.get(4)?;
    let status = status_str
        .parse::<Status>()
        .map_err(|e| parse_err(4, e.to_string()))?;

    let tags_json: String = row.get(7)?;
    let tags: Vec<String> =
        serde_json::from_str(&tags_json).map_err(|e| parse_err(7, e.to_string()))?;

    let workspace_path: Option<String> = row.get(9)?;

    Ok(Task {
        id: row.get::<_, i64>(0)? as u64,
        board_id: row.get::<_, i64>(1)? as u64,
        title: row.get(2)?,
        body: row.get(3)?,
        status,
        assignee: row.get(5)?,
        priority: row.get::<_, i64>(6)? as i32,
        tags,
        workspace_kind: row.get(8)?,
        workspace_path: workspace_path.map(PathBuf::from),
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        claimed_at: row.get(12)?,
        claim_ttl_secs: row.get(13)?,
    })
}

fn row_to_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<Link> {
    let kind_str: String = row.get(2)?;
    let kind = kind_str
        .parse::<LinkKind>()
        .map_err(|e| parse_err(2, e.to_string()))?;
    Ok(Link {
        from_id: row.get::<_, i64>(0)? as u64,
        to_id: row.get::<_, i64>(1)? as u64,
        kind,
    })
}

fn row_to_comment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: row.get::<_, i64>(0)? as u64,
        task_id: row.get::<_, i64>(1)? as u64,
        author: row.get(2)?,
        body: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let status_str: String = row.get(3)?;
    let status = status_str
        .parse::<RunStatus>()
        .map_err(|e| parse_err(3, e.to_string()))?;
    Ok(Run {
        id: row.get(0)?,
        task_id: row.get::<_, i64>(1)? as u64,
        profile: row.get(2)?,
        status,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        last_heartbeat: row.get(6)?,
        exit_code: row.get(7)?,
        output_summary: row.get(8)?,
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<KanbanEvent> {
    Ok(KanbanEvent {
        id: row.get::<_, i64>(0)? as u64,
        event_type: row.get(1)?,
        payload: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn parse_err(col: usize, msg: String) -> rusqlite::Error {
    #[derive(Debug)]
    struct E(String);
    impl std::fmt::Display for E {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for E {}
    rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(E(msg)))
}
