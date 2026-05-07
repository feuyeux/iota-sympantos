use super::*;
use serde_json::json;

#[test]
fn memory_scope_id_defaults_match_context_workspace() {
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
            &json!({"session_id":"s1"}),
            workspace
        ),
        "s1"
    );
}

#[test]
fn memory_write_schema_requires_confidence() {
    let write_tool = tools()
        .into_iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("iota_memory_write"))
        .expect("memory write tool should be listed");

    assert_eq!(
        write_tool["inputSchema"]["required"],
        json!(["content", "type", "scope", "confidence"])
    );
}

#[test]
fn memory_write_confidence_is_validated() {
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
}
