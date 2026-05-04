use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum RuntimeEvent {
    Output(OutputEvent),
    State(StateEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Error(ErrorEvent),
    Extension(ExtensionEvent),
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
            Self::Memory(_) => "memory",
            Self::ApprovalRequest(_) => "approval_request",
            Self::ApprovalDecision(_) => "approval_decision",
        }
    }
}

pub fn map_acp_event(method: &str, params: Option<&Value>) -> Option<RuntimeEvent> {
    let params = params?;
    match method {
        "session/update" | "session_update" => map_session_update(params),
        "session/complete" | "session_complete" => Some(RuntimeEvent::State(StateEvent {
            state: "complete".to_string(),
            detail: Some(params.clone()),
        })),
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
            Some(RuntimeEvent::ApprovalRequest(ApprovalRequestEvent {
                id,
                tool_name,
                payload: params.clone(),
            }))
        }
        other => Some(RuntimeEvent::Extension(ExtensionEvent {
            name: other.to_string(),
            payload: params.clone(),
        })),
    }
}

pub fn map_acp_error(message: String, code: Option<i64>, data: Option<Value>) -> RuntimeEvent {
    RuntimeEvent::Error(ErrorEvent {
        message,
        code,
        data,
    })
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

fn extract_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    for key in ["text", "content", "message", "result", "output"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }
    if let Some(content) = value.get("content").and_then(Value::as_object) {
        if let Some(text) = content.get("text").and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<String>();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
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
}
