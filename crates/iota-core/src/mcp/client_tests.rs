use crate::mcp::client::{McpToolCall, McpToolResult};
use serde_json::json;

#[test]
fn mcp_tool_call_serializes_correctly() {
    let call = McpToolCall {
        name: "read_file".to_string(),
        arguments: json!({"path": "/tmp/test.txt"}),
    };
    let json = serde_json::to_string(&call).unwrap();
    assert!(json.contains("\"name\":\"read_file\""));
    assert!(json.contains("\"path\":\"/tmp/test.txt\""));
}

#[test]
fn mcp_tool_call_default_arguments_is_null() {
    let call: McpToolCall = serde_json::from_str(r#"{"name":"list_tools"}"#).unwrap();
    assert_eq!(call.name, "list_tools");
    assert!(call.arguments.is_null());
}

#[test]
fn mcp_tool_result_ok_roundtrips() {
    let result = McpToolResult {
        ok: true,
        content: json!({"text": "file contents here"}),
        error: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("error"));

    let decoded: McpToolResult = serde_json::from_str(&json).unwrap();
    assert!(decoded.ok);
    assert_eq!(decoded.content["text"], "file contents here");
    assert!(decoded.error.is_none());
}

#[test]
fn mcp_tool_result_error_roundtrips() {
    let result = McpToolResult {
        ok: false,
        content: json!(null),
        error: Some("tool not found".to_string()),
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"error\":\"tool not found\""));

    let decoded: McpToolResult = serde_json::from_str(&json).unwrap();
    assert!(!decoded.ok);
    assert_eq!(decoded.error.as_deref(), Some("tool not found"));
}

#[tokio::test]
#[ignore]
async fn mcp_session_start_fails_with_nonexistent_command() {
    use crate::mcp::client::McpSession;
    use std::collections::BTreeMap;

    let result = McpSession::start("/nonexistent/mcp-server", &[], &BTreeMap::new(), 1000).await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore]
async fn call_stdio_fails_with_nonexistent_command() {
    use crate::mcp::client::call_stdio;
    use std::collections::BTreeMap;

    let result = call_stdio(
        "/nonexistent/mcp-server",
        &[],
        &BTreeMap::new(),
        McpToolCall {
            name: "test".to_string(),
            arguments: json!({}),
        },
        1000,
    )
    .await;
    assert!(result.is_err());
}
