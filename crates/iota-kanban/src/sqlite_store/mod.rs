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
mod security;
mod tasks;
mod txn;

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
        security::prepare_sqlite_path(path)?;
        let conn = Connection::open(path)
            .with_context(|| format!("opening kanban db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        security::secure_sqlite_files(path)?;
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
        self.with_transaction(|conn| {
            let mut applied = 0;
            for event in events {
                Self::apply_event_on_conn(conn, event).with_context(|| {
                    format!("failed to replay event {} ({})", event.id, event.event_type)
                })?;
                applied += 1;
            }
            Ok(applied)
        })
    }

    /// Atomically imports a remote event bundle: replays every new event,
    /// appends each to this store's own event log, and advances the sync
    /// cursor — all inside a single SQLite transaction.
    ///
    /// Previously (result.md S-04) these three steps ran as independent
    /// `KanbanStore` trait calls, each opening and releasing the store's
    /// lock separately. A crash or error partway through could replay some
    /// events but not append them (or vice versa), or advance the cursor
    /// past events that were never actually applied — silently losing or
    /// duplicating data on the next sync. Running all of it under one
    /// `with_transaction` means the whole import either fully commits or
    /// fully rolls back.
    pub fn import_event_bundle_atomic(
        &self,
        new_events: &[KanbanEvent],
        source: &str,
        cursor: EventId,
    ) -> Result<usize> {
        self.with_transaction(|conn| {
            let mut applied = 0;
            for event in new_events {
                Self::apply_event_on_conn(conn, event).with_context(|| {
                    format!(
                        "failed to replay event {} ({}) during bundle import",
                        event.id, event.event_type
                    )
                })?;
                Self::append_event_on_conn(conn, &event.event_type, &event.payload)?;
                applied += 1;
            }
            Self::set_sync_cursor_on_conn(conn, source, cursor)?;
            Ok(applied)
        })
    }

    fn set_sync_cursor_on_conn(conn: &Connection, source: &str, cursor: EventId) -> Result<()> {
        let now = now_ts();
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

    fn apply_event_on_conn(conn: &Connection, event: &KanbanEvent) -> Result<()> {
        match event.event_type.as_str() {
            EVT_BOARD_CREATED => {
                let p: BoardCreatedPayload = serde_json::from_str(&event.payload)?;
                conn.execute(
                    "INSERT OR IGNORE INTO boards (id, slug, name, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![p.board_id as i64, p.slug, p.name, event.created_at],
                )?;
                Ok(())
            }
            EVT_TASK_CREATED => {
                let p: TaskCreatedPayload = serde_json::from_str(&event.payload)?;
                let tags_json = serde_json::to_string(&p.tags)?;
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
                Self::update_task_on_conn(conn, p.task_id, patch)
            }
            EVT_TASK_DELETED => {
                let p: TaskDeletedPayload = serde_json::from_str(&event.payload)?;
                Self::delete_task_on_conn(conn, p.task_id)
            }
            EVT_TASK_TRANSITIONED => {
                let p: TaskTransitionedPayload = serde_json::from_str(&event.payload)?;
                let to: Status = p.to.parse()?;
                Self::transition_on_conn(conn, p.task_id, to)
            }
            EVT_LINK_CREATED => {
                let p: LinkCreatedPayload = serde_json::from_str(&event.payload)?;
                let kind: LinkKind = p.kind.parse()?;
                Self::create_link_on_conn(conn, p.from_id, p.to_id, kind)
            }
            EVT_LINK_REMOVED => {
                let p: LinkRemovedPayload = serde_json::from_str(&event.payload)?;
                let kind: LinkKind = p.kind.parse()?;
                Self::remove_link_on_conn(conn, p.from_id, p.to_id, kind)
            }
            EVT_COMMENT_ADDED => {
                let p: CommentAddedPayload = serde_json::from_str(&event.payload)?;
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
                Self::complete_run_on_conn(conn, &p.run_id, status, p.exit_code)
            }
            _ => Ok(()), // Unknown event types are silently skipped
        }
    }
}

// --- Trait impl --------------------------------------------------------------

impl KanbanStore for SqliteKanbanStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn create_board(&self, slug: &str, name: &str) -> Result<BoardId> {
        self.with_transaction(|conn| {
            let now = now_ts();
            conn.execute(
                "INSERT INTO boards (slug, name, created_at) VALUES (?1, ?2, ?3)",
                params![slug, name, now],
            )?;
            let id = conn.last_insert_rowid() as u64;
            let payload = serde_json::to_string(&BoardCreatedPayload {
                board_id: id,
                slug: slug.to_string(),
                name: name.to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_BOARD_CREATED, &payload)?;
            Ok(id)
        })
    }
    fn list_boards(&self) -> Result<Vec<Board>> {
        self.list_boards_impl()
    }
    fn get_board(&self, slug: &str) -> Result<Board> {
        self.get_board_impl(slug)
    }
    fn create_task(&self, req: CreateTaskRequest) -> Result<TaskId> {
        // Capture the fields the event payload needs before `req` is moved
        // into `create_task_on_conn`.
        let title = req.title.clone();
        let body = req.body.clone();
        let board_id = req.board_id;
        let status = req.status.unwrap_or(Status::Triage).as_str().to_string();
        let assignee = req.assignee.clone();
        let priority = req.priority.unwrap_or(0);
        let tags = req.tags.clone();
        // SECURITY/CORRECTNESS (result.md S-04): the domain write (INSERT
        // INTO tasks) and the event-log append (INSERT INTO events) must
        // be atomic — a crash between them previously could leave a task
        // with no corresponding event (breaking sync/replay) or vice
        // versa. `with_transaction` runs both under one BEGIN/COMMIT and
        // one held lock.
        let id = self.with_transaction(|conn| {
            let id = Self::create_task_on_conn(conn, req)?;
            let payload = serde_json::to_string(&TaskCreatedPayload {
                task_id: id,
                board_id,
                title: title.clone(),
                body,
                status,
                assignee,
                priority,
                tags,
            })
            ?;
            Self::append_event_on_conn(conn, EVT_TASK_CREATED, &payload)?;
            Ok(id)
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskCreated { id, title });
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
        self.with_transaction(|conn| {
            Self::update_task_on_conn(conn, id, patch)?;
            let payload = serde_json::to_string(&TaskUpdatedPayload {
                task_id: id,
                patch: patch_payload,
            })
            ?;
            Self::append_event_on_conn(conn, EVT_TASK_UPDATED, &payload)?;
            Ok(())
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskUpdated { id });
        Ok(())
    }
    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        self.list_tasks_impl(filter)
    }
    fn delete_task(&self, id: TaskId) -> Result<()> {
        self.with_transaction(|conn| {
            Self::delete_task_on_conn(conn, id)?;
            let payload =
                serde_json::to_string(&TaskDeletedPayload { task_id: id })?;
            Self::append_event_on_conn(conn, EVT_TASK_DELETED, &payload)?;
            Ok(())
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskDeleted { id });
        Ok(())
    }
    fn transition(&self, id: TaskId, to: Status) -> Result<()> {
        let from = self.with_transaction(|conn| {
            let from_text: String = conn.query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![id as i64],
                |row| row.get(0),
            )?;
            let from: Status = from_text.parse()?;
            Self::transition_on_conn(conn, id, to)?;
            let payload = serde_json::to_string(&TaskTransitionedPayload {
                task_id: id,
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_TASK_TRANSITIONED, &payload)?;
            Ok(from)
        })?;
        let _ = self
            .event_tx
            .send(KanbanUiEvent::TaskStatusChanged { id, from, to });
        Ok(())
    }
    fn create_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.with_transaction(|conn| {
            Self::create_link_on_conn(conn, from, to, kind)?;
            let payload = serde_json::to_string(&LinkCreatedPayload {
                from_id: from,
                to_id: to,
                kind: kind.as_str().to_string(),
            })
            ?;
            Self::append_event_on_conn(conn, EVT_LINK_CREATED, &payload)?;
            Ok(())
        })
    }
    fn remove_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.with_transaction(|conn| {
            Self::remove_link_on_conn(conn, from, to, kind)?;
            let payload = serde_json::to_string(&LinkRemovedPayload {
                from_id: from,
                to_id: to,
                kind: kind.as_str().to_string(),
            })
            ?;
            Self::append_event_on_conn(conn, EVT_LINK_REMOVED, &payload)?;
            Ok(())
        })
    }
    fn get_links(&self, id: TaskId) -> Result<Vec<Link>> {
        self.get_links_impl(id)
    }
    fn add_comment(&self, task_id: TaskId, author: &str, body: &str) -> Result<CommentId> {
        let comment_id = self.with_transaction(|conn| {
            let comment_id = Self::add_comment_on_conn(conn, task_id, author, body)?;
            let payload = serde_json::to_string(&CommentAddedPayload {
                comment_id,
                task_id,
                author: author.to_string(),
                body: body.to_string(),
            })
            ?;
            Self::append_event_on_conn(conn, EVT_COMMENT_ADDED, &payload)?;
            Ok(comment_id)
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::CommentAdded {
            task_id,
            comment_id,
        });
        Ok(comment_id)
    }
    fn list_comments(&self, task_id: TaskId) -> Result<Vec<Comment>> {
        self.list_comments_impl(task_id)
    }
    fn create_run(&self, task_id: TaskId, profile: &str) -> Result<RunId> {
        let run_id = self.with_transaction(|conn| {
            let run_id = Self::create_run_on_conn(conn, task_id, profile)?;
            let payload = serde_json::to_string(&RunStartedPayload {
                run_id: run_id.clone(),
                task_id,
                profile: profile.to_string(),
            })
            ?;
            Self::append_event_on_conn(conn, EVT_RUN_STARTED, &payload)?;
            Ok(run_id)
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::RunStarted {
            task_id,
            run_id: run_id.clone(),
        });
        Ok(run_id)
    }
    fn complete_run(&self, run_id: &str, status: RunStatus, exit_code: Option<i32>) -> Result<()> {
        let task_id = self.with_transaction(|conn| {
            let task_id: i64 = conn.query_row(
                "SELECT task_id FROM runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )?;
            Self::complete_run_on_conn(conn, run_id, status, exit_code)?;
            let payload = serde_json::to_string(&RunCompletedPayload {
                run_id: run_id.to_string(),
                task_id: task_id as u64,
                status: status.as_str().to_string(),
                exit_code,
            })
            ?;
            Self::append_event_on_conn(conn, EVT_RUN_COMPLETED, &payload)?;
            Ok(task_id as u64)
        })?;
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
