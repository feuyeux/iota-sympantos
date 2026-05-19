//! Event sourcing layer for the kanban store.
//!
//! Every mutation is recorded as a structured event in the append-only events table.
//! Events can be replayed to rebuild state, enabling multi-node sync.

use serde::{Deserialize, Serialize};

use super::types::*;

// ---------------------------------------------------------------------------
// Event type constants
// ---------------------------------------------------------------------------

pub const EVT_BOARD_CREATED: &str = "board_created";
pub const EVT_TASK_CREATED: &str = "task_created";
pub const EVT_TASK_UPDATED: &str = "task_updated";
pub const EVT_TASK_DELETED: &str = "task_deleted";
pub const EVT_TASK_TRANSITIONED: &str = "task_transitioned";
pub const EVT_LINK_CREATED: &str = "link_created";
pub const EVT_LINK_REMOVED: &str = "link_removed";
pub const EVT_COMMENT_ADDED: &str = "comment_added";
pub const EVT_RUN_STARTED: &str = "run_started";
pub const EVT_RUN_COMPLETED: &str = "run_completed";

// ---------------------------------------------------------------------------
// Payload structs (serialized as JSON into events.payload)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardCreatedPayload {
    pub board_id: BoardId,
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreatedPayload {
    pub task_id: TaskId,
    pub board_id: BoardId,
    pub title: String,
    pub body: Option<String>,
    pub status: String,
    pub assignee: Option<String>,
    pub priority: i32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdatedPayload {
    pub task_id: TaskId,
    pub patch: TaskPatchPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPatchPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDeletedPayload {
    pub task_id: TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTransitionedPayload {
    pub task_id: TaskId,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCreatedPayload {
    pub from_id: TaskId,
    pub to_id: TaskId,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkRemovedPayload {
    pub from_id: TaskId,
    pub to_id: TaskId,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentAddedPayload {
    pub comment_id: CommentId,
    pub task_id: TaskId,
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStartedPayload {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCompletedPayload {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub status: String,
    pub exit_code: Option<i32>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kanban::{KanbanStore, SqliteKanbanStore};
    use std::path::Path;

    fn test_store() -> SqliteKanbanStore {
        SqliteKanbanStore::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn events_recorded_on_mutations() {
        let store = test_store();
        store.create_board("test", "Test Board").unwrap();
        store
            .create_task(CreateTaskRequest {
                board_id: 1,
                title: "My task".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store.transition(1, Status::Todo).unwrap();

        let events = store.events_since(0).unwrap();
        assert!(
            events.len() >= 3,
            "expected at least 3 events, got {}",
            events.len()
        );
        assert_eq!(events[0].event_type, "board_created");
        assert_eq!(events[1].event_type, "task_created");
        assert_eq!(events[2].event_type, "task_transitioned");
    }

    #[test]
    fn replay_rebuilds_state() {
        let store1 = test_store();
        store1.create_board("dev", "Development").unwrap();
        store1
            .create_task(CreateTaskRequest {
                board_id: 1,
                title: "Build feature".to_string(),
                body: Some("Details here".to_string()),
                status: None,
                assignee: Some("alice".to_string()),
                priority: Some(5),
                tags: vec!["urgent".to_string()],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store1.transition(1, Status::Todo).unwrap();
        store1.add_comment(1, "bob", "Looks good").unwrap();

        // Collect events from store1
        let events = store1.events_since(0).unwrap();

        // Replay into a fresh store
        let store2 = test_store();
        let applied = store2.replay_events(&events).unwrap();
        assert_eq!(applied, events.len());

        // Verify state matches
        let boards = store2.list_boards().unwrap();
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].slug, "dev");

        let task = store2.get_task(1).unwrap();
        assert_eq!(task.title, "Build feature");
        assert_eq!(task.status, Status::Todo);
        assert_eq!(task.assignee.as_deref(), Some("alice"));

        let comments = store2.list_comments(1).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "Looks good");
    }

    #[test]
    fn replay_skips_invalid_events() {
        let store = test_store();
        let events = vec![
            KanbanEvent {
                id: 1,
                event_type: "unknown_type".to_string(),
                payload: "{}".to_string(),
                created_at: 0,
            },
            KanbanEvent {
                id: 2,
                event_type: "board_created".to_string(),
                payload: r#"{"board_id":1,"slug":"x","name":"X"}"#.to_string(),
                created_at: 0,
            },
        ];
        let applied = store.replay_events(&events).unwrap();
        assert_eq!(applied, 2); // unknown events succeed (silently skip)
    }

    #[test]
    fn event_payload_round_trips() {
        let payload = TaskCreatedPayload {
            task_id: 42,
            board_id: 1,
            title: "Test task".to_string(),
            body: Some("Body content".to_string()),
            status: "triage".to_string(),
            assignee: Some("alice".to_string()),
            priority: 5,
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        };
        let json = serde_json::to_string(&payload).unwrap();
        let deserialized: TaskCreatedPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, 42);
        assert_eq!(deserialized.title, "Test task");
        assert_eq!(deserialized.tags, vec!["tag1", "tag2"]);
    }

    #[test]
    fn replay_link_events() {
        let store1 = test_store();
        store1.create_board("b", "Board").unwrap();
        store1
            .create_task(CreateTaskRequest {
                board_id: 1,
                title: "T1".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store1
            .create_task(CreateTaskRequest {
                board_id: 1,
                title: "T2".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store1.create_link(1, 2, LinkKind::Blocks).unwrap();

        let events = store1.events_since(0).unwrap();

        let store2 = test_store();
        store2.replay_events(&events).unwrap();

        let links = store2.get_links(1).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Blocks);
    }
}
