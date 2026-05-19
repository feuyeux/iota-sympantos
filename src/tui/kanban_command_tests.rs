use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::kanban::{
    CreateTaskRequest, Dispatcher, DispatcherConfig, KanbanStore, SqliteKanbanStore, Status,
};

fn test_store() -> Arc<dyn KanbanStore> {
    Arc::new(SqliteKanbanStore::open(Path::new(":memory:")).unwrap())
}

fn exec(args: &str, store: &Arc<dyn KanbanStore>) -> Vec<String> {
    super::execute(args, store, None)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_help() {
    let store = test_store();
    let out = exec("help", &store);
    assert!(
        out.iter().any(|l| l.contains("Kanban commands")),
        "help output should contain 'Kanban commands', got: {:?}",
        out
    );
    assert!(
        out.iter().any(|l| l.contains("dispatch") && l.contains("Tick dispatcher")),
        "help output should describe dispatch, got: {:?}",
        out
    );
}

#[test]
fn test_boards_empty() {
    let store = test_store();
    let out = exec("boards", &store);
    assert!(
        out.iter().any(|l| l.contains("No boards")),
        "expected 'No boards' message, got: {:?}",
        out
    );
}

#[test]
fn test_board_create_and_list() {
    let store = test_store();
    let create_out = exec("board create myboard My Board", &store);
    assert!(
        create_out[0].contains("Created board #1"),
        "expected board creation message, got: {:?}",
        create_out
    );

    let list_out = exec("boards", &store);
    assert!(
        list_out.iter().any(|l| l.contains("myboard")),
        "boards listing should contain 'myboard', got: {:?}",
        list_out
    );
}

#[test]
fn test_create_task_no_board() {
    let store = test_store();
    let out = exec("create Fix bug", &store);
    assert!(
        out.iter().any(|l| l.contains("No boards")),
        "expected error about no boards, got: {:?}",
        out
    );
}

#[test]
fn test_create_and_show_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    let create_out = exec("create Fix the login page", &store);
    assert!(
        create_out[0].contains("Created task #1: Fix the login page"),
        "expected task creation message, got: {:?}",
        create_out
    );

    let show_out = exec("show 1", &store);
    assert!(
        show_out.iter().any(|l| l.contains("Fix the login page")),
        "show should include the task title, got: {:?}",
        show_out
    );
    assert!(
        show_out.iter().any(|l| l.contains("triage")),
        "new task should be in triage status, got: {:?}",
        show_out
    );
}

#[test]
fn test_move_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Implement feature", &store);

    let move_out = exec("move 1 todo", &store);
    assert!(
        move_out[0].contains("Task #1 -> todo"),
        "expected successful move message, got: {:?}",
        move_out
    );
}

#[test]
fn test_move_invalid_transition() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Some task", &store);

    // Task starts at triage; triage->running is invalid
    let move_out = exec("move 1 running", &store);
    assert!(
        move_out[0].contains("Error:"),
        "expected error for invalid transition, got: {:?}",
        move_out
    );
}

#[test]
fn test_assign_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Build dashboard", &store);

    let assign_out = exec("assign 1 alice", &store);
    assert!(
        assign_out[0].contains("@alice"),
        "expected assignment confirmation with @alice, got: {:?}",
        assign_out
    );

    let show_out = exec("show 1", &store);
    assert!(
        show_out.iter().any(|l| l.contains("@alice")),
        "show should display @alice as assignee, got: {:?}",
        show_out
    );
}

#[test]
fn test_comment_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Review PR", &store);

    let comment_out = exec("comment 1 Looks good to me", &store);
    assert!(
        comment_out[0].contains("Comment added"),
        "expected comment confirmation, got: {:?}",
        comment_out
    );
}

#[test]
fn test_unknown_subcommand() {
    let store = test_store();
    let out = exec("foobar", &store);
    assert!(
        out[0].contains("Unknown kanban subcommand"),
        "expected unknown subcommand message, got: {:?}",
        out
    );
}

#[test]
fn test_list_empty() {
    let store = test_store();
    let out = exec("list", &store);
    assert!(
        out.iter().any(|l| l.contains("No tasks found")),
        "expected 'No tasks found' message, got: {:?}",
        out
    );
}

#[test]
fn test_list_with_status_filter() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Task A", &store);
    exec("create Task B", &store);

    // Move task 1 to todo (valid: triage -> todo)
    exec("move 1 todo", &store);

    // List only todo tasks
    let out = exec("list todo", &store);
    assert!(
        out.iter().any(|l| l.contains("Task A")),
        "list todo should include Task A, got: {:?}",
        out
    );
    assert!(
        !out.iter().any(|l| l.contains("Task B")),
        "list todo should NOT include Task B (still in triage), got: {:?}",
        out
    );
}

#[test]
fn test_dispatch_no_ready() {
    let store = test_store();
    let out = exec("dispatch", &store);
    assert!(
        out.iter().any(|l| l.contains("No ready tasks")),
        "expected 'No ready tasks' message, got: {:?}",
        out
    );
}

#[test]
fn test_dispatch_ticks_dispatcher() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Ready task".to_string(),
            body: None,
            status: Some(Status::Ready),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);
    let tmp = std::env::temp_dir().join(format!("iota-kb-cmd-{}", uuid::Uuid::new_v4()));
    let dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        max_concurrent: 1,
        hermes_bin: Path::new("/missing/hermes-for-iota-test").to_path_buf(),
        shadows_dir: tmp.clone(),
        ..Default::default()
    })));

    let out = super::execute_with_dispatcher("dispatch", &store, None, Some(&dispatcher), None);

    assert!(
        out.iter().any(|l| l.contains("spawn failure")),
        "expected dispatcher report with spawn failure, got: {:?}",
        out
    );
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dispatch_reports_busy_instead_of_blocking_on_dispatcher_lock() {
    let store = test_store();
    let tmp = std::env::temp_dir().join(format!("iota-kb-cmd-{}", uuid::Uuid::new_v4()));
    let dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        shadows_dir: tmp.clone(),
        ..Default::default()
    })));
    let guard = dispatcher.lock().unwrap();

    let out = super::execute_with_dispatcher("dispatch", &store, None, Some(&dispatcher), None);

    drop(guard);
    assert!(
        out.iter()
            .any(|line| line.contains("already running in the background")),
        "expected non-blocking busy message, got: {:?}",
        out
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_daemon_toggle() {
    let store = test_store();
    let daemon_active = Arc::new(AtomicBool::new(true));

    let out = super::execute_with_dispatcher("daemon", &store, None, None, Some(&daemon_active));
    assert!(
        out.iter().any(|l| l.contains("daemon stopped")),
        "expected daemon stopped message, got: {:?}",
        out
    );
    assert!(!daemon_active.load(std::sync::atomic::Ordering::Relaxed));

    let out = super::execute_with_dispatcher("daemon", &store, None, None, Some(&daemon_active));
    assert!(
        out.iter().any(|l| l.contains("daemon started")),
        "expected daemon started message, got: {:?}",
        out
    );
    assert!(daemon_active.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_dispatch_with_task_id_readies_todo_task() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Todo task".to_string(),
            body: None,
            status: Some(Status::Todo),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);
    let tmp = std::env::temp_dir().join(format!("iota-kb-cmd-{}", uuid::Uuid::new_v4()));
    let dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        max_concurrent: 1,
        hermes_bin: Path::new("/missing/hermes-for-iota-test").to_path_buf(),
        shadows_dir: tmp.clone(),
        ..Default::default()
    })));

    let out = super::execute_with_dispatcher(
        &format!("dispatch #{}", task_id),
        &store,
        None,
        Some(&dispatcher),
        None,
    );

    // Task should have been transitioned to ready, then dispatch attempted (spawn failure)
    assert!(
        out.iter().any(|l| l.contains("spawn failure")),
        "expected spawn failure (hermes not found), got: {:?}",
        out
    );
    // Task stays ready after failed spawn
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dispatch_rejects_invalid_status_task() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Done task".to_string(),
            body: None,
            status: Some(Status::Done),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);

    let out = super::execute_with_dispatcher(
        &format!("dispatch {}", task_id),
        &store,
        None,
        None,
        None,
    );

    assert!(
        out.iter().any(|l| l.contains("must be")),
        "expected rejection message for done task, got: {:?}",
        out
    );
}
