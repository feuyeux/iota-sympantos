use super::*;
use serde_json::json;

#[test]
fn maps_agent_message_to_output() {
    let event = map_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"agent_message_chunk","content":[{"text":"hi"}]}})),
    )
    .unwrap();
    assert!(matches!(event, RuntimeEvent::Output(OutputEvent { text, .. }) if text == "hi"));
}

#[test]
fn extracts_token_usage_from_session_complete_payload() {
    let usage = token_usage_from_value(&json!({
        "model": "test-model",
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 8
        }
    }))
    .unwrap();
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(8));
    assert_eq!(usage.total_tokens, Some(20));
    assert_eq!(usage.model.as_deref(), Some("test-model"));
}

#[test]
fn maps_session_complete_to_state() {
    let event = map_acp_event(
        "session/complete",
        Some(&json!({
            "model": "test-model",
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 8
            }
        })),
    )
    .unwrap();
    assert!(matches!(event, RuntimeEvent::State(StateEvent { state, .. }) if state == "complete"));
}

#[test]
fn tool_call_event_is_mapped() {
    let event = map_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"tool_call","id":"t1","name":"iota_memory_search","arguments":{"query":"rust"}}})),
    )
    .unwrap();
    assert!(
        matches!(event, RuntimeEvent::ToolCall(ToolCallEvent { name, .. }) if name == "iota_memory_search")
    );
}

#[test]
fn unknown_session_update_maps_to_state() {
    let event = map_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"thinking"}})),
    )
    .unwrap();
    assert!(matches!(event, RuntimeEvent::State(StateEvent { state, .. }) if state == "thinking"));
}

#[test]
fn error_update_maps_to_error_event() {
    let event = map_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"error","message":"timeout","code":504}})),
    )
    .unwrap();
    assert!(
        matches!(event, RuntimeEvent::Error(ErrorEvent { message, code, .. }) if message == "timeout" && code == Some(504))
    );
}

#[test]
fn session_complete_emits_token_usage_too() {
    let events = crate::runtime_event::map_acp_events(
        "session/complete",
        Some(&json!({"model":"gpt-4o","usage":{"prompt_tokens":10,"completion_tokens":5}})),
    );
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, RuntimeEvent::TokenUsage(_)))
    );
    assert!(events.iter().any(|e| matches!(e, RuntimeEvent::State(_))));
}

#[test]
fn request_permission_maps_to_approval_request() {
    let event = map_acp_event(
        "session/request_permission",
        Some(&json!({"requestId":"req-1","toolName":"shell","command":"rm -rf /tmp/x"})),
    )
    .unwrap();
    assert!(
        matches!(event, RuntimeEvent::ApprovalRequest(ApprovalRequestEvent { id, tool_name, .. }) if id == "req-1" && tool_name == "shell")
    );
}
