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

#[test]
fn memory_write_requires_type_scope_and_confidence() {
    let missing_type = route_tool_call(
        "iota_memory_write",
        &json!({
            "content": "remember this",
            "scope": "project",
            "confidence": 0.9
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(missing_type.contains("type is required"));

    let missing_confidence = route_tool_call(
        "iota_memory_write",
        &json!({
            "content": "remember this",
            "type": "semantic",
            "facet": "domain",
            "scope": "project"
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(missing_confidence.contains("confidence is required"));
}

#[test]
fn memory_write_rejects_out_of_range_confidence() {
    let err = route_tool_call(
        "iota_memory_write",
        &json!({
            "content": "remember this",
            "type": "semantic",
            "facet": "domain",
            "scope": "project",
            "confidence": 1.2
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("confidence must be between 0 and 1"));
}

#[test]
fn memory_write_rejects_invalid_type_facet_shape() {
    let missing_facet = route_tool_call(
        "iota_memory_write",
        &json!({
            "content": "remember this",
            "type": "semantic",
            "scope": "project",
            "confidence": 0.9
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(missing_facet.contains("semantic memory requires a facet"));

    let illegal_facet = route_tool_call(
        "iota_memory_write",
        &json!({
            "content": "remember this",
            "type": "procedural",
            "facet": "domain",
            "scope": "project",
            "confidence": 0.9
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(illegal_facet.contains("only semantic memory may set facet"));
}
