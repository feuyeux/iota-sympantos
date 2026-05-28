use super::*;
use iota_kanban::{KanbanStore, SqliteKanbanStore, Status};
use serde_json::json;

#[test]
fn memory_scope_id_defaults_with_workspace() {
    let workspace = std::path::Path::new("/tmp/iota-project");
    assert_eq!(
        default_memory_scope_id(&MemoryScope::User, &json!({}), workspace),
        "local-user"
    );
    assert_eq!(
        default_memory_scope_id(&MemoryScope::Project, &json!({}), workspace),
        workspace.display().to_string()
    );
    assert_eq!(
        default_memory_scope_id(
            &MemoryScope::Session,
            &json!({"source_session_id": "session-1"}),
            workspace
        ),
        "session-1"
    );
    assert_eq!(
        default_memory_scope_id(
            &MemoryScope::Session,
            &json!({"session_id": "s1"}),
            workspace
        ),
        "s1"
    );
    assert_eq!(
        default_memory_scope_id(&MemoryScope::Global, &json!({}), workspace),
        "global"
    );
}

#[test]
fn confidence_validation() {
    assert_eq!(
        required_confidence(&json!({})).unwrap_err(),
        "confidence is required"
    );
    assert_eq!(
        required_confidence(&json!({"confidence": 1.5})).unwrap_err(),
        "confidence must be between 0 and 1"
    );
    assert_eq!(
        required_confidence(&json!({"confidence": "0.75"})).unwrap(),
        0.75
    );
    assert_eq!(
        required_confidence(&json!({"confidence": 0.9})).unwrap(),
        0.9
    );
}

#[test]
fn memory_shape_validation() {
    assert_eq!(
        validate_memory_shape(MemoryType::Semantic, None).unwrap_err(),
        "semantic memory requires a facet"
    );
    assert_eq!(
        validate_memory_shape(MemoryType::Procedural, Some(MemoryFacet::Domain)).unwrap_err(),
        "only semantic memory may set facet"
    );
    validate_memory_shape(MemoryType::Semantic, Some(MemoryFacet::Domain)).unwrap();
    validate_memory_shape(MemoryType::Episodic, None).unwrap();
}

#[test]
fn is_known_tool_recognizes_iota_tools() {
    assert!(is_known_tool("iota_memory_search"));
    assert!(is_known_tool("iota_memory_write"));
    assert!(is_known_tool("iota_skill_search"));
    assert!(is_known_tool("iota_skill_load"));
    assert!(is_known_tool("iota_session_summary"));
    assert!(is_known_tool("iota_handoff_publish"));
    assert!(is_known_tool("iota_handoff_read"));
    assert!(is_known_tool("iota_kanban_create_task"));
    assert!(is_known_tool("iota_kanban_list_tasks"));
    assert!(is_known_tool("iota_kanban_ready_task"));
    assert!(!is_known_tool("external_tool"));
    assert!(!is_known_tool("iota_unknown"));
}

#[test]
fn kanban_create_task_defaults_to_ready() {
    let store = SqliteKanbanStore::open(std::path::Path::new(":memory:")).unwrap();
    let workspace = std::path::Path::new("/tmp/iota-project");
    let skills = crate::skill::SkillRegistry::load(workspace, &[]);
    let ctx = ToolContext {
        memory: None,
        ledger: None,
        kanban: Some(&store),
        skills: &skills,
        workspace,
    };

    let result = dispatch_tool(
        &ctx,
        "iota_kanban_create_task",
        &json!({
            "title": "Research Agent - TinyFish trending to Supabase",
            "assignee": "research-agent",
            "tags": ["research", "supabase"]
        }),
    )
    .unwrap();

    let task_id = result["task_id"].as_u64().unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Ready);
    assert_eq!(task.assignee.as_deref(), Some("research-agent"));
    assert_eq!(result["auto_dispatch"], true);
}
