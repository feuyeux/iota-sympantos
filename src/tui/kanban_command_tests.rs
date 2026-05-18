use std::path::Path;
use std::sync::Arc;

use crate::kanban::{KanbanStore, SqliteKanbanStore};

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
