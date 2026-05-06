use super::*;
use serde_json::json;

#[test]
fn denies_external_mcp_tools() {
    let result = route_tool_call("external_shell", &json!({})).unwrap();
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
}

#[test]
fn memory_scope_id_defaults_match_engine_recall_keys() {
    assert_eq!(
        default_memory_scope_id(&MemoryScope::User, &json!({})),
        "local-user"
    );
    assert_eq!(
        default_memory_scope_id(&MemoryScope::Global, &json!({})),
        "global"
    );
    assert_eq!(
        default_memory_scope_id(
            &MemoryScope::Session,
            &json!({"source_session_id":"session-1"})
        ),
        "session-1"
    );
    assert_eq!(
        default_memory_scope_id(&MemoryScope::Project, &json!({})),
        std::env::current_dir().unwrap().display().to_string()
    );
}
