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
