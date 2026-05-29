use crate::store::cache::{CacheStore, ExecutionStatus};
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
