use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

use super::wire::AcpWireMessage;

#[derive(Debug, Serialize)]
pub(super) struct JsonRpcRequest<'a> {
    pub(super) jsonrpc: &'static str,
    pub(super) id: String,
    pub(super) method: &'a str,
    pub(super) params: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct JsonRpcResponse {
    pub(super) jsonrpc: &'static str,
    pub(super) id: Value,
    pub(super) result: Value,
}

pub(super) fn text_from_session_update(params: Option<&Value>) -> Option<String> {
    let params = params?;
    let update = params.get("update").unwrap_or(params);
    let session_update = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .and_then(Value::as_str);

    match session_update {
        Some("agent_message") | Some("agent_message_chunk") => extract_text(update),
        _ => None,
    }
}

pub(super) fn extract_final_text(value: &Value) -> Option<String> {
    value
        .get("finalMessage")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| extract_text(value))
}

pub fn extract_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    for key in ["text", "content", "message", "result", "output"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }

    if let Some(content) = value.get("content").and_then(Value::as_object)
        && let Some(text) = content.get("text").and_then(Value::as_str)
    {
        return Some(text.to_string());
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

pub(super) fn is_terminal_result(result: &Value) -> bool {
    result.get("stopReason").and_then(Value::as_str).is_some() || extract_text(result).is_some()
}

pub(super) fn permission_request_id(message: &AcpWireMessage) -> Result<Value> {
    message
        .id
        .clone()
        .or_else(|| {
            message
                .params
                .as_ref()
                .and_then(|params| params.get("requestId").cloned())
        })
        .context("ACP permission request did not include an id or requestId")
}

pub(super) fn acp_tool_call_parts(params: Option<&Value>) -> (String, Value) {
    let params = params.unwrap_or(&Value::Null);
    let name = params
        .get("name")
        .or_else(|| params.get("toolName"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let arguments = params
        .get("arguments")
        .or_else(|| params.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    (name, arguments)
}
