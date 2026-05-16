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
fn extracts_cache_token_usage_separately() {
    let usage = token_usage_from_value(&json!({
        "usage": {
            "prompt_tokens": 19,
            "prompt_tokens_details": {
                "cached_tokens": 7
            },
            "completion_tokens": 8
        }
    }))
    .unwrap();

    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.cache_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(8));
}

#[test]
fn uncached_prompt_tokens_take_precedence_for_input_tokens() {
    let usage = token_usage_from_value(&json!({
        "usage": {
            "prompt_tokens": 19,
            "uncached_prompt_tokens": 11,
            "prompt_tokens_details": {
                "cached_tokens": 7
            },
            "completion_tokens": 8
        }
    }))
    .unwrap();

    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.cache_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(8));
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
fn session_update_usage_emits_token_usage() {
    let events = map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "usage",
                "usage": {
                    "prompt_tokens": 19,
                    "prompt_tokens_details": {
                        "cached_tokens": 7
                    },
                    "completion_tokens": 8
                }
            }
        })),
    );

    let usage = events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::TokenUsage(usage) => Some(usage),
            _ => None,
        })
        .unwrap();

    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.cache_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(8));
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
fn claude_tool_call_update_emits_real_tool_call() {
    let events = crate::runtime_event::map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "title": "mcp__iota-context__iota_memory_write",
                "rawInput": {
                    "type": "semantic",
                    "facet": "domain",
                    "scope": "project",
                    "scope_id": "iota-sympantos",
                    "content": "remember this",
                    "confidence": 0.91
                }
            }
        })),
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::State(StateEvent { state, .. }) if state == "tool_call_update"))
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RuntimeEvent::ToolCall(ToolCallEvent { id, name, arguments })
                if id == "call-1"
                    && name == "iota_memory_write"
                    && arguments.get("scope_id").and_then(serde_json::Value::as_str) == Some("iota-sympantos")
        )
    }));
}

#[test]
fn claude_tool_call_update_emits_real_tool_result() {
    let events = crate::runtime_event::map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "_meta": {
                    "claudeCode": {
                        "toolName": "mcp__iota-context__iota_memory_search",
                        "toolResponse": "{\"records\":[{\"id\":\"m1\"}],\"mode\":\"hybrid\"}"
                    }
                },
                "status": "completed"
            }
        })),
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RuntimeEvent::ToolResult(ToolResultEvent { id, name, ok, result })
                if id == "call-1"
                    && name == "iota_memory_search"
                    && *ok
                    && result.get("records").and_then(serde_json::Value::as_array).map(Vec::len) == Some(1)
        )
    }));
}

#[test]
fn claude_failed_tool_call_update_emits_failed_tool_result() {
    let events = crate::runtime_event::map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "title": "mcp__iota-context__iota_memory_write",
                "rawOutput": "only semantic memory may set facet",
                "status": "failed"
            }
        })),
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RuntimeEvent::ToolResult(ToolResultEvent { id, name, ok, result })
                if id == "call-1"
                    && name == "iota_memory_write"
                    && !*ok
                    && result.as_str() == Some("only semantic memory may set facet")
        )
    }));
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
