use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::PathBuf;

use super::store::KanbanStore;
use super::types::*;

// ---------------------------------------------------------------------------
// Shadow schema (hermes-compatible — matches hermes_cli/kanban_db.py)
// ---------------------------------------------------------------------------

const SHADOW_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS boards (
    id          INTEGER PRIMARY KEY,
    slug        TEXT    UNIQUE NOT NULL,
    name        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    id                   TEXT PRIMARY KEY,
    board_id             INTEGER NOT NULL,
    title                TEXT    NOT NULL,
    body                 TEXT,
    status               TEXT    NOT NULL DEFAULT 'triage',
    assignee             TEXT,
    priority             INTEGER NOT NULL DEFAULT 0,
    tags                 TEXT    NOT NULL DEFAULT '[]',
    workspace_kind       TEXT,
    workspace_path       TEXT,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    started_at           INTEGER,
    completed_at         INTEGER,
    claimed_at           INTEGER,
    claim_lock           TEXT,
    claim_expires        INTEGER,
    worker_pid           INTEGER,
    current_run_id       INTEGER,
    result               TEXT,
    tenant               TEXT,
    created_by           TEXT,
    branch_name          TEXT,
    max_runtime_seconds  INTEGER,
    model_override       TEXT,
    skills               TEXT,
    session_id           TEXT
);
CREATE TABLE IF NOT EXISTS task_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT    NOT NULL,
    run_id      INTEGER,
    kind        TEXT    NOT NULL,
    payload     TEXT,
    created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS task_comments (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT    NOT NULL,
    author      TEXT    NOT NULL,
    body        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS task_links (
    parent_id  TEXT NOT NULL,
    child_id   TEXT NOT NULL,
    PRIMARY KEY (parent_id, child_id)
);
CREATE TABLE IF NOT EXISTS task_runs (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id             TEXT NOT NULL,
    profile             TEXT,
    step_key            TEXT,
    status              TEXT NOT NULL,
    claim_lock          TEXT,
    claim_expires       INTEGER,
    worker_pid          INTEGER,
    max_runtime_seconds INTEGER,
    last_heartbeat_at   INTEGER,
    started_at          INTEGER NOT NULL,
    ended_at            INTEGER,
    outcome             TEXT,
    summary             TEXT,
    metadata            TEXT,
    error               TEXT
);
";

// ---------------------------------------------------------------------------
// ShadowDb
// ---------------------------------------------------------------------------

pub struct ShadowDb {
    pub path: PathBuf,
    #[allow(dead_code)]
    pub task_id: TaskId,
    /// The auto-increment run id in the shadow's `task_runs` table.
    /// Hermes expects this as an integer in `HERMES_KANBAN_RUN_ID`.
    pub run_id: i64,
}

// ---------------------------------------------------------------------------
// ShadowMaterializer
// ---------------------------------------------------------------------------

pub struct ShadowMaterializer {
    shadows_dir: PathBuf,
}

impl ShadowMaterializer {
    pub fn new(shadows_dir: PathBuf) -> Self {
        Self { shadows_dir }
    }

    /// Creates a shadow DB at `shadows_dir/<task_id>/kanban.db` populated
    /// with the board, the task, its linked tasks, its comments, and a
    /// running `task_runs` entry so hermes's kanban tools can find the
    /// active run via `HERMES_KANBAN_RUN_ID`.
    ///
    /// Returns `ShadowDb` whose `run_id` is the auto-increment PK of the
    /// run row (hermes expects an integer run id).
    pub fn materialize(
        &self,
        task: &Task,
        board: &Board,
        store: &dyn KanbanStore,
    ) -> Result<ShadowDb> {
        let task_dir = self.shadows_dir.join(task.id.to_string());
        std::fs::create_dir_all(&task_dir)
            .with_context(|| format!("creating shadow dir {}", task_dir.display()))?;

        let db_path = task_dir.join("kanban.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("opening shadow db {}", db_path.display()))?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(SHADOW_SCHEMA)?;

        // Insert board
        conn.execute(
            "INSERT OR REPLACE INTO boards (id, slug, name, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![board.id as i64, &board.slug, &board.name, board.created_at],
        )?;

        // Insert main task (with status=running, matching hermes's claim semantics)
        let task_id_str = task.id.to_string();
        insert_task_into(&conn, task, Some("running"))?;

        // Insert linked tasks and the links (hermes schema: parent_id/child_id)
        let links = store.get_links(task.id)?;
        for link in &links {
            let other_id = if link.from_id == task.id {
                link.to_id
            } else {
                link.from_id
            };
            if let Ok(other_task) = store.get_task(other_id) {
                insert_task_into(&conn, &other_task, None)?;
            }
            // In hermes schema: parent_id is the dependency (must finish first)
            // and child_id is the dependent task.
            let link_pair = match link.kind {
                LinkKind::Blocks => Some((link.from_id.to_string(), link.to_id.to_string())),
                LinkKind::Parent => Some((link.from_id.to_string(), link.to_id.to_string())),
                LinkKind::Related => None, // hermes has no "related" link type
            };
            if let Some((parent, child)) = link_pair {
                conn.execute(
                    "INSERT OR IGNORE INTO task_links (parent_id, child_id)
                     VALUES (?1, ?2)",
                    params![parent, child],
                )?;
            }
        }

        // Insert comments for the main task
        let comments = store.list_comments(task.id)?;
        for comment in &comments {
            conn.execute(
                "INSERT OR REPLACE INTO task_comments (id, task_id, author, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    comment.id as i64,
                    &task_id_str,
                    &comment.author,
                    &comment.body,
                    comment.created_at,
                ],
            )?;
        }

        // Create a task_runs entry (hermes expects this for kanban_complete)
        let now = crate::utils::now_ts();
        let profile = task.assignee.as_deref().unwrap_or("default");
        let claim_lock = format!("iota:{}", std::process::id());
        conn.execute(
            "INSERT INTO task_runs (task_id, profile, status, claim_lock, started_at)
             VALUES (?1, ?2, 'running', ?3, ?4)",
            params![&task_id_str, profile, &claim_lock, now],
        )?;
        let run_id = conn.last_insert_rowid();

        // Point the task at the active run + set claim
        conn.execute(
            "UPDATE tasks SET current_run_id = ?1, claim_lock = ?2, status = 'running'
             WHERE id = ?3",
            params![run_id, &claim_lock, &task_id_str],
        )?;

        Ok(ShadowDb {
            path: db_path,
            task_id: task.id,
            run_id,
        })
    }

    /// Removes the shadow directory for the given task.
    pub fn cleanup(&self, task_id: TaskId) -> Result<()> {
        let task_dir = self.shadows_dir.join(task_id.to_string());
        if task_dir.exists() {
            std::fs::remove_dir_all(&task_dir)
                .with_context(|| format!("removing shadow dir {}", task_dir.display()))?;
        }
        Ok(())
    }
}

fn insert_task_into(conn: &Connection, task: &Task, override_status: Option<&str>) -> Result<()> {
    let tags_json = serde_json::to_string(&task.tags)?;
    let workspace_path = task
        .workspace_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    let task_id_str = task.id.to_string();
    let status = override_status.unwrap_or(task.status.as_str());
    conn.execute(
        "INSERT OR REPLACE INTO tasks
         (id, board_id, title, body, status, assignee, priority, tags,
          workspace_kind, workspace_path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            &task_id_str,
            task.board_id as i64,
            &task.title,
            &task.body,
            status,
            &task.assignee,
            task.priority as i64,
            tags_json,
            &task.workspace_kind,
            workspace_path,
            task.created_at,
            task.updated_at,
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// ShadowWatcher
// ---------------------------------------------------------------------------

pub struct ShadowEvent {
    pub id: i64,
    pub task_id: TaskId,
    pub event_type: String,
    pub payload: String,
}

pub struct ShadowWatcher {
    db_path: PathBuf,
    task_id: TaskId,
    last_event_id: i64,
}

impl ShadowWatcher {
    pub fn new(db_path: PathBuf, task_id: TaskId) -> Self {
        Self {
            db_path,
            task_id,
            last_event_id: 0,
        }
    }

    /// Reads new task_events rows from the shadow DB.  Also checks whether
    /// the task status is terminal ("done" or "blocked").
    pub fn poll(&mut self) -> Result<(Vec<ShadowEvent>, Option<String>)> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("opening shadow db {}", self.db_path.display()))?;

        let task_id_str = self.task_id.to_string();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, kind, payload
             FROM task_events WHERE id > ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![self.last_event_id], |row| {
            Ok(ShadowEvent {
                id: row.get(0)?,
                task_id: self.task_id,
                event_type: row.get(2)?,
                payload: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            })
        })?;

        let mut events = Vec::new();
        for r in rows {
            events.push(r?);
        }

        // Check terminal task status
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![&task_id_str],
                |row| row.get(0),
            )
            .ok();

        let terminal = match status.as_deref() {
            Some("done") | Some("blocked") => status,
            _ => None,
        };

        Ok((events, terminal))
    }

    pub fn mark_events_synced(&mut self, events: &[ShadowEvent]) {
        if let Some(max_id) = events.iter().map(|event| event.id).max() {
            self.last_event_id = self.last_event_id.max(max_id);
        }
    }

    /// Applies a slice of shadow events to the iota KanbanStore.
    pub fn sync_events(
        &self,
        events: &[ShadowEvent],
        store: &dyn KanbanStore,
        run_id: &str,
    ) -> Result<()> {
        for event in events {
            match event.event_type.as_str() {
                "heartbeat" => {
                    store.heartbeat(run_id)?;
                }
                "comment" => {
                    let v: serde_json::Value =
                        serde_json::from_str(&event.payload).context("parsing comment payload")?;
                    let author = v["author"].as_str().unwrap_or("hermes");
                    let body = v["body"].as_str().unwrap_or("");
                    store.add_comment(event.task_id, author, body)?;
                }
                "status_change" => {
                    let v: serde_json::Value = serde_json::from_str(&event.payload)
                        .context("parsing status_change payload")?;
                    if event.task_id == self.task_id {
                        continue;
                    }
                    let to_str = v["to"]
                        .as_str()
                        .context("missing 'to' field in status_change payload")?;
                    let to: Status = to_str.parse()?;
                    store.transition(event.task_id, to)?;
                }
                "task_create" => {
                    let v: serde_json::Value = serde_json::from_str(&event.payload)
                        .context("parsing task_create payload")?;
                    let board_id = v["board_id"]
                        .as_u64()
                        .context("missing board_id in task_create payload")?;
                    let title = v["title"].as_str().unwrap_or("untitled").to_string();
                    let body = v["body"].as_str().map(|s| s.to_string());
                    let tags: Vec<String> = v["tags"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let req = CreateTaskRequest {
                        board_id,
                        title,
                        body,
                        status: None,
                        assignee: v["assignee"].as_str().map(|s| s.to_string()),
                        priority: v["priority"].as_i64().map(|n| n as i32),
                        tags,
                        workspace_kind: None,
                        workspace_path: None,
                    };
                    let new_task_id = store.create_task(req)?;
                    let link_kind_str = v["link_kind"].as_str().unwrap_or("related");
                    let link_kind: LinkKind = link_kind_str.parse().unwrap_or(LinkKind::Related);
                    store.create_link(event.task_id, new_task_id, link_kind)?;
                }
                _ => {} // Unknown event types are silently ignored
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::sqlite_store::SqliteKanbanStore;
    use super::super::store::KanbanStore;
    use super::*;
    use rusqlite::{Connection, params};
    use std::path::Path;

    fn test_tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iota-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open_store(path: &Path) -> SqliteKanbanStore {
        SqliteKanbanStore::open(path).unwrap()
    }

    fn make_board(store: &dyn KanbanStore) -> BoardId {
        store.create_board("test", "Test Board").unwrap()
    }

    fn make_task(store: &dyn KanbanStore, board_id: BoardId) -> TaskId {
        store
            .create_task(CreateTaskRequest {
                board_id,
                title: "test task".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap()
    }

    fn init_shadow_db(db_path: &Path) -> Connection {
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(super::SHADOW_SCHEMA).unwrap();
        conn
    }

    // -----------------------------------------------------------------------

    #[test]
    fn materialize_creates_shadow_db() {
        let tmp = test_tmp_dir();
        let store = open_store(&tmp.join("store.db"));

        let board_id = make_board(&store);
        let task_id = make_task(&store, board_id);
        store.add_comment(task_id, "alice", "hello world").unwrap();

        let task = store.get_task(task_id).unwrap();
        let board = store.get_board("test").unwrap();

        let materializer = ShadowMaterializer::new(tmp.join("shadows"));
        let shadow_db = materializer.materialize(&task, &board, &store).unwrap();

        assert!(shadow_db.path.exists());
        assert_eq!(shadow_db.task_id, task_id);

        let conn = Connection::open(&shadow_db.path).unwrap();
        let task_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id = ?1",
                params![task_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(task_count, 1);

        let comment_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_comments WHERE task_id = ?1",
                params![task_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(comment_count, 1);
    }

    #[test]
    fn materialize_includes_linked_tasks() {
        let tmp = test_tmp_dir();
        let store = open_store(&tmp.join("store.db"));

        let board_id = make_board(&store);
        let parent_id = make_task(&store, board_id);
        let child_id = make_task(&store, board_id);
        store
            .create_link(child_id, parent_id, LinkKind::Parent)
            .unwrap();

        let child_task = store.get_task(child_id).unwrap();
        let board = store.get_board("test").unwrap();

        let materializer = ShadowMaterializer::new(tmp.join("shadows"));
        let shadow_db = materializer
            .materialize(&child_task, &board, &store)
            .unwrap();

        let conn = Connection::open(&shadow_db.path).unwrap();
        let task_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(task_count, 2);
    }

    #[test]
    fn cleanup_removes_shadow_dir() {
        let tmp = test_tmp_dir();
        let store = open_store(&tmp.join("store.db"));

        let board_id = make_board(&store);
        let task_id = make_task(&store, board_id);
        let task = store.get_task(task_id).unwrap();
        let board = store.get_board("test").unwrap();

        let materializer = ShadowMaterializer::new(tmp.join("shadows"));
        let shadow_db = materializer.materialize(&task, &board, &store).unwrap();

        let shadow_dir = shadow_db.path.parent().unwrap().to_path_buf();
        assert!(shadow_dir.exists());

        materializer.cleanup(task_id).unwrap();
        assert!(!shadow_dir.exists());
    }

    #[test]
    fn watcher_polls_new_events() {
        let tmp = test_tmp_dir();
        let db_path = tmp.join("kanban.db");
        let task_id: TaskId = 42;
        let now = 1_000_000i64;

        let conn = init_shadow_db(&db_path);
        conn.execute(
            "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at)
             VALUES (?1, 1, 'test', 'triage', '[]', ?2, ?2)",
            params![task_id.to_string(), now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_events (task_id, kind, payload, created_at)
             VALUES (?1, 'heartbeat', '{}', ?2)",
            params![task_id.to_string(), now],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO task_events (task_id, kind, payload, created_at)
             VALUES (?1, 'comment', '{"author":"bot","body":"done"}', ?2)"#,
            params![task_id.to_string(), now],
        )
        .unwrap();
        drop(conn);

        let mut watcher = ShadowWatcher::new(db_path, task_id);

        let (events, terminal) = watcher.poll().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "heartbeat");
        assert_eq!(events[1].event_type, "comment");
        assert!(terminal.is_none());

        watcher.mark_events_synced(&events);
        let (events2, _) = watcher.poll().unwrap();
        assert_eq!(events2.len(), 0);
    }

    #[test]
    fn watcher_detects_terminal_status() {
        let tmp = test_tmp_dir();
        let db_path = tmp.join("kanban.db");
        let task_id: TaskId = 7;
        let now = 1_000_000i64;

        let conn = init_shadow_db(&db_path);
        conn.execute(
            "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at)
             VALUES (?1, 1, 'test', 'done', '[]', ?2, ?2)",
            params![task_id.to_string(), now],
        )
        .unwrap();
        drop(conn);

        let mut watcher = ShadowWatcher::new(db_path, task_id);
        let (_, terminal) = watcher.poll().unwrap();
        assert_eq!(terminal, Some("done".to_string()));
    }

    #[test]
    fn sync_events_applies_to_store() {
        let tmp = test_tmp_dir();
        let store = open_store(&tmp.join("store.db"));

        let board_id = make_board(&store);
        let task_id = make_task(&store, board_id);
        // Triage -> Todo -> Ready -> Running (valid transition chain)
        store.transition(task_id, Status::Todo).unwrap();
        store.transition(task_id, Status::Ready).unwrap();
        store.transition(task_id, Status::Running).unwrap();
        let run_id = store.create_run(task_id, "test-profile").unwrap();

        // db_path is not opened by sync_events; any path works here
        let watcher = ShadowWatcher::new(tmp.join("unused.db"), task_id);

        let events = vec![
            ShadowEvent {
                id: 1,
                task_id,
                event_type: "heartbeat".to_string(),
                payload: "{}".to_string(),
            },
            ShadowEvent {
                id: 2,
                task_id,
                event_type: "comment".to_string(),
                payload: r#"{"author":"bot","body":"task done"}"#.to_string(),
            },
        ];

        watcher.sync_events(&events, &store, &run_id).unwrap();

        let comments = store.list_comments(task_id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].author, "bot");
        assert_eq!(comments[0].body, "task done");
    }

    #[test]
    fn failed_sync_does_not_advance_event_cursor() {
        let tmp = test_tmp_dir();
        let db_path = tmp.join("kanban.db");
        let task_id: TaskId = 1;
        let now = 1_000_000i64;

        let conn = init_shadow_db(&db_path);
        conn.execute(
            "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at)
             VALUES (?1, 1, 'test', 'running', '[]', ?2, ?2)",
            params![task_id.to_string(), now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_events (task_id, kind, payload, created_at)
             VALUES (?1, 'comment', '{bad-json', ?2)",
            params![task_id.to_string(), now],
        )
        .unwrap();
        drop(conn);

        let store = open_store(&tmp.join("store.db"));
        let board_id = make_board(&store);
        let main_task_id = make_task(&store, board_id);
        let run_id = store.create_run(main_task_id, "test-profile").unwrap();
        let mut watcher = ShadowWatcher::new(db_path, main_task_id);

        let (events, _) = watcher.poll().unwrap();
        assert_eq!(events.len(), 1);
        assert!(watcher.sync_events(&events, &store, &run_id).is_err());

        let (events_again, _) = watcher.poll().unwrap();
        assert_eq!(events_again.len(), 1);
        assert_eq!(events_again[0].id, events[0].id);
    }

    #[test]
    fn sync_events_routes_comment_to_event_task_id() {
        let tmp = test_tmp_dir();
        let store = open_store(&tmp.join("store.db"));
        let board_id = make_board(&store);
        let main_task_id = make_task(&store, board_id);
        let linked_task_id = make_task(&store, board_id);
        let run_id = store.create_run(main_task_id, "test-profile").unwrap();
        let watcher = ShadowWatcher::new(tmp.join("unused.db"), main_task_id);

        let events = vec![ShadowEvent {
            id: 1,
            task_id: linked_task_id,
            event_type: "comment".to_string(),
            payload: r#"{"author":"bot","body":"linked note"}"#.to_string(),
        }];

        watcher.sync_events(&events, &store, &run_id).unwrap();

        assert!(store.list_comments(main_task_id).unwrap().is_empty());
        let comments = store.list_comments(linked_task_id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "linked note");
    }

    #[test]
    fn sync_events_defers_main_task_status_until_worker_exit() {
        let tmp = test_tmp_dir();
        let store = open_store(&tmp.join("store.db"));
        let board_id = make_board(&store);
        let task_id = make_task(&store, board_id);
        store.transition(task_id, Status::Todo).unwrap();
        store.transition(task_id, Status::Ready).unwrap();
        store.transition(task_id, Status::Running).unwrap();
        let run_id = store.create_run(task_id, "test-profile").unwrap();
        let watcher = ShadowWatcher::new(tmp.join("unused.db"), task_id);

        let events = vec![ShadowEvent {
            id: 1,
            task_id,
            event_type: "status_change".to_string(),
            payload: r#"{"to":"done"}"#.to_string(),
        }];

        watcher.sync_events(&events, &store, &run_id).unwrap();

        assert_eq!(store.get_task(task_id).unwrap().status, Status::Running);
    }
}
