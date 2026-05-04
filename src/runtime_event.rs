use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::extract_text;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum RuntimeEvent {
    Output(OutputEvent),
    State(StateEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Error(ErrorEvent),
    Extension(ExtensionEvent),
    TokenUsage(TokenUsageEvent),
    Memory(MemoryEvent),
    ApprovalRequest(ApprovalRequestEvent),
    ApprovalDecision(ApprovalDecisionEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputEvent {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEvent {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEvent {
    pub id: String,
    pub name: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionEvent {
    pub name: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestEvent {
    pub id: String,
    pub tool_name: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecisionEvent {
    pub request_id: String,
    pub approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl RuntimeEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Output(_) => "output",
            Self::State(_) => "state",
            Self::ToolCall(_) => "tool_call",
            Self::ToolResult(_) => "tool_result",
            Self::Error(_) => "error",
            Self::Extension(_) => "extension",
            Self::TokenUsage(_) => "token_usage",
            Self::Memory(_) => "memory",
            Self::ApprovalRequest(_) => "approval_request",
            Self::ApprovalDecision(_) => "approval_decision",
        }
    }
}

#[allow(dead_code)]
pub fn map_acp_event(method: &str, params: Option<&Value>) -> Option<RuntimeEvent> {
    map_acp_events(method, params).into_iter().next()
}

pub fn map_acp_events(method: &str, params: Option<&Value>) -> Vec<RuntimeEvent> {
    let Some(params) = params else {
        return Vec::new();
    };
    match method {
        "session/update" | "session_update" => map_session_update(params).into_iter().collect(),
        "session/complete" | "session_complete" => {
            let mut events = vec![RuntimeEvent::State(StateEvent {
                state: "complete".to_string(),
                detail: Some(params.clone()),
            })];
            if let Some(usage) = token_usage_from_value(params) {
                events.push(RuntimeEvent::TokenUsage(usage));
            }
            events
        }
        "session/request_permission" | "request_permission" | "permission/request" => {
            let id = params
                .get("requestId")
                .or_else(|| params.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("permission")
                .to_string();
            let tool_name = params
                .get("toolName")
                .or_else(|| params.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            vec![RuntimeEvent::ApprovalRequest(ApprovalRequestEvent {
                id,
                tool_name,
                payload: params.clone(),
            })]
        }
        other => vec![RuntimeEvent::Extension(ExtensionEvent {
            name: other.to_string(),
            payload: params.clone(),
        })],
    }
}

pub fn token_usage_from_value(value: &Value) -> Option<TokenUsageEvent> {
    let usage = find_usage_object(value)?;
    let input_tokens = first_u64(
        usage,
        &[
            "input_tokens",
            "inputTokens",
            "prompt_tokens",
            "promptTokens",
            "cache_read_input_tokens",
        ],
    );
    let output_tokens = first_u64(
        usage,
        &[
            "output_tokens",
            "outputTokens",
            "completion_tokens",
            "completionTokens",
            "generated_tokens",
        ],
    );
    let total_tokens = first_u64(usage, &["total_tokens", "totalTokens"]).or_else(|| {
        input_tokens
            .zip(output_tokens)
            .map(|(input, output)| input + output)
    });
    if input_tokens.is_none() && output_tokens.is_none() && total_tokens.is_none() {
        return None;
    }
    Some(TokenUsageEvent {
        input_tokens,
        output_tokens,
        total_tokens,
        model: first_string(value, &["model", "modelName"])
            .or_else(|| first_string(usage, &["model", "modelName"])),
        payload: usage.clone(),
    })
}

pub fn map_acp_error(message: String, code: Option<i64>, data: Option<Value>) -> RuntimeEvent {
    RuntimeEvent::Error(ErrorEvent {
        message,
        code,
        data,
    })
}

fn find_usage_object(value: &Value) -> Option<&Value> {
    for key in ["usage", "tokenUsage", "token_usage", "tokens"] {
        if let Some(candidate) = value.get(key).filter(|candidate| candidate.is_object()) {
            return Some(candidate);
        }
    }
    if has_any_token_key(value) {
        return Some(value);
    }
    None
}

fn has_any_token_key(value: &Value) -> bool {
    [
        "input_tokens",
        "inputTokens",
        "prompt_tokens",
        "promptTokens",
        "output_tokens",
        "outputTokens",
        "completion_tokens",
        "completionTokens",
        "total_tokens",
        "totalTokens",
    ]
    .iter()
    .any(|key| value.get(*key).is_some())
}

fn first_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(Value::as_u64).or_else(|| {
            value
                .get(*key)
                .and_then(Value::as_i64)
                .and_then(|v| v.try_into().ok())
        })
    })
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn map_session_update(params: &Value) -> Option<RuntimeEvent> {
    let update = params.get("update").unwrap_or(params);
    let update_type = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    match update_type {
        "agent_message" | "agent_message_chunk" => extract_text(update).map(|text| {
            RuntimeEvent::Output(OutputEvent {
                text,
                role: Some("assistant".to_string()),
            })
        }),
        "tool_call" | "tool_use" => Some(RuntimeEvent::ToolCall(ToolCallEvent {
            id: update
                .get("id")
                .or_else(|| update.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_string(),
            name: update
                .get("name")
                .or_else(|| update.get("toolName"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            arguments: update
                .get("arguments")
                .or_else(|| update.get("input"))
                .cloned()
                .unwrap_or(Value::Null),
        })),
        "tool_result" | "tool_output" => Some(RuntimeEvent::ToolResult(ToolResultEvent {
            id: update
                .get("id")
                .or_else(|| update.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_string(),
            name: update
                .get("name")
                .or_else(|| update.get("toolName"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            ok: !update
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            result: update
                .get("result")
                .or_else(|| update.get("content"))
                .cloned()
                .unwrap_or(Value::Null),
        })),
        "error" => Some(RuntimeEvent::Error(ErrorEvent {
            message: update
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("ACP session update error")
                .to_string(),
            code: update.get("code").and_then(Value::as_i64),
            data: Some(update.clone()),
        })),
        other => Some(RuntimeEvent::State(StateEvent {
            state: other.to_string(),
            detail: Some(update.clone()),
        })),
    }
}

#[cfg(test)]
mod tests {
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
        assert!(
            matches!(event, RuntimeEvent::State(StateEvent { state, .. }) if state == "complete")
        );
    }
}
