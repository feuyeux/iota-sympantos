use crate::store::cache::{CacheStore, ExecutionStatus, get_execution_status};
use crate::utils::now_ts;
use rusqlite::{Connection, params};
use std::path::Path;

#[test]
fn begin_execution_rejects_existing_running_request_before_sqlite_constraint() {
    let store = CacheStore::open(Path::new(":memory:")).unwrap();

    store
        .begin_execution_with_id("codex", "session", "same-request", None)
        .unwrap();
    let err = store
        .begin_execution_with_id("codex", "session", "same-request", None)
        .unwrap_err();

    assert!(err.to_string().contains("execution already running"));
    assert!(!err.to_string().contains("UNIQUE constraint failed"));
}

#[test]
fn begin_execution_allows_same_request_after_completion() {
    let store = CacheStore::open(Path::new(":memory:")).unwrap();

    let first = store
        .begin_execution_with_id("codex", "session", "same-request", None)
        .unwrap();
    store
        .finish_execution(&first, ExecutionStatus::Completed)
        .unwrap();
    let second = store
        .begin_execution_with_id("codex", "session", "same-request", None)
        .unwrap();

    assert_ne!(second, first);
}

#[test]
fn migrated_legacy_database_accepts_new_execution_for_completed_request() {
    let path =
        std::env::temp_dir().join(format!("iota-cache-legacy-{}.sqlite", uuid::Uuid::new_v4()));
    let now = now_ts();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE cache_executions (
    execution_id  TEXT PRIMARY KEY,
    session_id    TEXT NOT NULL,
    backend       TEXT NOT NULL,
    request_hash  TEXT NOT NULL,
    status        TEXT NOT NULL,
    started_at    INTEGER NOT NULL,
    finished_at   INTEGER,
    fencing_token INTEGER NOT NULL DEFAULT 0,
    UNIQUE(backend, request_hash)
);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cache_executions
         (execution_id, session_id, backend, request_hash, status, started_at, finished_at, fencing_token)
         VALUES (?1, ?2, ?3, ?4, 'completed', ?5, ?6, 1)",
        params![
            "old-exec",
            "session",
            "codex",
            "same-request",
            now - 1,
            now
        ],
    )
    .unwrap();
    drop(conn);

    let store = CacheStore::open(&path).unwrap();
    let new_execution = store
        .begin_execution_with_id("codex", "session", "same-request", None)
        .unwrap();

    assert_ne!(new_execution, "old-exec");

    let status = {
        let conn = Connection::open(&path).unwrap();
        get_execution_status(&conn, "old-exec").unwrap()
    };
    assert_eq!(status, Some(ExecutionStatus::Completed));

    let _ = std::fs::remove_file(path);
}
