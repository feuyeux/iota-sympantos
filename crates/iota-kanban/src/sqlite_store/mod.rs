use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use super::event_sourcing::*;
use super::store::KanbanStore;
use super::types::*;
use crate::utils::{lock_or_recover, now_ts};

mod boards;
mod comments;
mod events;
mod links;
mod runs;
mod tasks;

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
CREATE TABLE IF NOT EXISTS event_sync_cursors (
    source      TEXT    PRIMARY KEY,
    cursor      INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
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
        if path != Path::new(":memory:")
            && let Some(parent) = path.parent()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating kanban db dir {}", parent.display()))?;
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

    /// Record a structured event directly to the events table.
    /// Called after _impl methods have released the mutex, so re-locking is safe.
    fn record_event_internal(&self, event_type: &str, payload: &str) {
        let conn = self.lock_conn();
        let now = now_ts();
        let _ = conn.execute(
            "INSERT INTO events (event_type, payload, created_at) VALUES (?1, ?2, ?3)",
            params![event_type, payload, now],
        );
    }

    pub fn sync_cursor(&self, source: &str) -> Result<EventId> {
        let conn = self.lock_conn();
        let cursor = conn
            .query_row(
                "SELECT cursor FROM event_sync_cursors WHERE source = ?1",
                params![source],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);
        Ok(cursor as EventId)
    }

    pub fn set_sync_cursor(&self, source: &str, cursor: EventId) -> Result<()> {
        let now = now_ts();
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO event_sync_cursors (source, cursor, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(source) DO UPDATE SET
                cursor = MAX(event_sync_cursors.cursor, excluded.cursor),
                updated_at = excluded.updated_at",
            params![source, cursor as i64, now],
        )?;
        Ok(())
    }

    /// Replay a sequence of events against this store, rebuilding state.
    /// Used for syncing a remote node's events into this store.
    /// Returns the number of applied events. A failed event aborts the replay so
    /// callers never acknowledge a cursor that contains an unapplied event.
    #[allow(dead_code)]
    pub fn replay_events(&self, events: &[KanbanEvent]) -> Result<usize> {
        let mut applied = 0;
        for event in events {
            self.apply_event(event).with_context(|| {
                format!("failed to replay event {} ({})", event.id, event.event_type)
            })?;
            applied += 1;
        }
        Ok(applied)
    }

    fn apply_event(&self, event: &KanbanEvent) -> Result<()> {
        match event.event_type.as_str() {
            EVT_BOARD_CREATED => {
                let p: BoardCreatedPayload = serde_json::from_str(&event.payload)?;
                let conn = self.lock_conn();
                conn.execute(
                    "INSERT OR IGNORE INTO boards (id, slug, name, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![p.board_id as i64, p.slug, p.name, event.created_at],
                )?;
                Ok(())
            }
            EVT_TASK_CREATED => {
                let p: TaskCreatedPayload = serde_json::from_str(&event.payload)?;
                let tags_json = serde_json::to_string(&p.tags)?;
                let conn = self.lock_conn();
                conn.execute(
                    "INSERT OR IGNORE INTO tasks
                     (id, board_id, title, body, status, assignee, priority, tags,
                      created_at, updated_at, claim_ttl_secs)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, 900)",
                    params![
                        p.task_id as i64,
                        p.board_id as i64,
                        p.title,
                        p.body,
                        p.status,
                        p.assignee,
                        p.priority,
                        tags_json,
                        event.created_at,
                    ],
                )?;
                Ok(())
            }
            EVT_TASK_UPDATED => {
                let p: TaskUpdatedPayload = serde_json::from_str(&event.payload)?;
                let patch = TaskPatch {
                    title: p.patch.title,
                    body: p.patch.body,
                    status: p.patch.status.and_then(|s| s.parse().ok()),
                    assignee: p.patch.assignee,
                    priority: p.patch.priority,
                    tags: p.patch.tags,
                    workspace_kind: None,
                    workspace_path: None,
                };
                self.update_task_impl(p.task_id, patch)
            }
            EVT_TASK_DELETED => {
                let p: TaskDeletedPayload = serde_json::from_str(&event.payload)?;
                self.delete_task_impl(p.task_id)
            }
            EVT_TASK_TRANSITIONED => {
                let p: TaskTransitionedPayload = serde_json::from_str(&event.payload)?;
                let to: Status = p.to.parse()?;
                self.transition_impl(p.task_id, to)
            }
            EVT_LINK_CREATED => {
                let p: LinkCreatedPayload = serde_json::from_str(&event.payload)?;
                let kind: LinkKind = p.kind.parse()?;
                self.create_link_impl(p.from_id, p.to_id, kind)
            }
            EVT_LINK_REMOVED => {
                let p: LinkRemovedPayload = serde_json::from_str(&event.payload)?;
                let kind: LinkKind = p.kind.parse()?;
                self.remove_link_impl(p.from_id, p.to_id, kind)
            }
            EVT_COMMENT_ADDED => {
                let p: CommentAddedPayload = serde_json::from_str(&event.payload)?;
                let conn = self.lock_conn();
                conn.execute(
                    "INSERT OR IGNORE INTO comments (id, task_id, author, body, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        p.comment_id as i64,
                        p.task_id as i64,
                        p.author,
                        p.body,
                        event.created_at,
                    ],
                )?;
                Ok(())
            }
            EVT_RUN_STARTED => {
                let p: RunStartedPayload = serde_json::from_str(&event.payload)?;
                let conn = self.lock_conn();
                conn.execute(
                    "INSERT OR IGNORE INTO runs
                     (id, task_id, profile, status, started_at, last_heartbeat)
                     VALUES (?1, ?2, ?3, 'running', ?4, ?4)",
                    params![p.run_id, p.task_id as i64, p.profile, event.created_at],
                )?;
                Ok(())
            }
            EVT_RUN_COMPLETED => {
                let p: RunCompletedPayload = serde_json::from_str(&event.payload)?;
                let status: RunStatus = p.status.parse()?;
                self.complete_run_impl(&p.run_id, status, p.exit_code)
            }
            _ => Ok(()), // Unknown event types are silently skipped
        }
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
}

// --- Trait impl --------------------------------------------------------------

impl KanbanStore for SqliteKanbanStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn create_board(&self, slug: &str, name: &str) -> Result<BoardId> {
        let id = self.create_board_impl(slug, name)?;
        self.record_event_internal(
            EVT_BOARD_CREATED,
            &serde_json::to_string(&BoardCreatedPayload {
                board_id: id,
                slug: slug.to_string(),
                name: name.to_string(),
            })
            .unwrap_or_default(),
        );
        Ok(id)
    }
    fn list_boards(&self) -> Result<Vec<Board>> {
        self.list_boards_impl()
    }
    fn get_board(&self, slug: &str) -> Result<Board> {
        self.get_board_impl(slug)
    }
    fn create_task(&self, req: CreateTaskRequest) -> Result<TaskId> {
        let title = req.title.clone();
        let body = req.body.clone();
        let board_id = req.board_id;
        let status = req.status.unwrap_or(Status::Triage).as_str().to_string();
        let assignee = req.assignee.clone();
        let priority = req.priority.unwrap_or(0);
        let tags = req.tags.clone();
        let id = self.create_task_impl(req)?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskCreated {
            id,
            title: title.clone(),
        });
        self.record_event_internal(
            EVT_TASK_CREATED,
            &serde_json::to_string(&TaskCreatedPayload {
                task_id: id,
                board_id,
                title,
                body,
                status,
                assignee,
                priority,
                tags,
            })
            .unwrap_or_default(),
        );
        Ok(id)
    }
    fn get_task(&self, id: TaskId) -> Result<Task> {
        self.get_task_impl(id)
    }
    fn update_task(&self, id: TaskId, patch: TaskPatch) -> Result<()> {
        let patch_payload = TaskPatchPayload {
            title: patch.title.clone(),
            body: patch.body.clone(),
            status: patch.status.map(|s| s.as_str().to_string()),
            assignee: patch.assignee.clone(),
            priority: patch.priority,
            tags: patch.tags.clone(),
        };
        self.update_task_impl(id, patch)?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskUpdated { id });
        self.record_event_internal(
            EVT_TASK_UPDATED,
            &serde_json::to_string(&TaskUpdatedPayload {
                task_id: id,
                patch: patch_payload,
            })
            .unwrap_or_default(),
        );
        Ok(())
    }
    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        self.list_tasks_impl(filter)
    }
    fn delete_task(&self, id: TaskId) -> Result<()> {
        self.delete_task_impl(id)?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskDeleted { id });
        self.record_event_internal(
            EVT_TASK_DELETED,
            &serde_json::to_string(&TaskDeletedPayload { task_id: id }).unwrap_or_default(),
        );
        Ok(())
    }
    fn transition(&self, id: TaskId, to: Status) -> Result<()> {
        let from = self.get_task(id)?.status;
        self.transition_impl(id, to)?;
        let _ = self
            .event_tx
            .send(KanbanUiEvent::TaskStatusChanged { id, from, to });
        self.record_event_internal(
            EVT_TASK_TRANSITIONED,
            &serde_json::to_string(&TaskTransitionedPayload {
                task_id: id,
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            })
            .unwrap_or_default(),
        );
        Ok(())
    }
    fn create_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.create_link_impl(from, to, kind)?;
        self.record_event_internal(
            EVT_LINK_CREATED,
            &serde_json::to_string(&LinkCreatedPayload {
                from_id: from,
                to_id: to,
                kind: kind.as_str().to_string(),
            })
            .unwrap_or_default(),
        );
        Ok(())
    }
    fn remove_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.remove_link_impl(from, to, kind)?;
        self.record_event_internal(
            EVT_LINK_REMOVED,
            &serde_json::to_string(&LinkRemovedPayload {
                from_id: from,
                to_id: to,
                kind: kind.as_str().to_string(),
            })
            .unwrap_or_default(),
        );
        Ok(())
    }
    fn get_links(&self, id: TaskId) -> Result<Vec<Link>> {
        self.get_links_impl(id)
    }
    fn add_comment(&self, task_id: TaskId, author: &str, body: &str) -> Result<CommentId> {
        let comment_id = self.add_comment_impl(task_id, author, body)?;
        let _ = self.event_tx.send(KanbanUiEvent::CommentAdded {
            task_id,
            comment_id,
        });
        self.record_event_internal(
            EVT_COMMENT_ADDED,
            &serde_json::to_string(&CommentAddedPayload {
                comment_id,
                task_id,
                author: author.to_string(),
                body: body.to_string(),
            })
            .unwrap_or_default(),
        );
        Ok(comment_id)
    }
    fn list_comments(&self, task_id: TaskId) -> Result<Vec<Comment>> {
        self.list_comments_impl(task_id)
    }
    fn create_run(&self, task_id: TaskId, profile: &str) -> Result<RunId> {
        let run_id = self.create_run_impl(task_id, profile)?;
        let _ = self.event_tx.send(KanbanUiEvent::RunStarted {
            task_id,
            run_id: run_id.clone(),
        });
        self.record_event_internal(
            EVT_RUN_STARTED,
            &serde_json::to_string(&RunStartedPayload {
                run_id: run_id.clone(),
                task_id,
                profile: profile.to_string(),
            })
            .unwrap_or_default(),
        );
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
        self.record_event_internal(
            EVT_RUN_COMPLETED,
            &serde_json::to_string(&RunCompletedPayload {
                run_id: run_id.to_string(),
                task_id,
                status: status.as_str().to_string(),
                exit_code,
            })
            .unwrap_or_default(),
        );
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

#[cfg(test)]
#[path = "../sqlite_store_tests.rs"]
mod sqlite_store_tests;
