use anyhow::{Result, bail};
use serde_json::{Value, json};
use tokio::io::BufReader;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;

use crate::mcp::router;
use crate::runtime_event::{self, RuntimeEvent, ToolCallEvent, ToolResultEvent};

use super::AcpBackend;
use super::message::{
    acp_tool_call_parts, extract_final_text, extract_text, is_terminal_result,
    permission_request_id, text_from_session_update,
};
use super::permission as acp_permission;
use super::wire::{format_acp_error, is_response_id, parse_message_line, read_next_line};

pub(super) struct PromptReadOptions<'a> {
    pub(super) backend: AcpBackend,
    pub(super) tool_whitelist: &'a [String],
    pub(super) show_native: bool,
    pub(super) timeout_ms: u64,
    pub(super) expected_prompt_id: &'a str,
    pub(super) stream_tx: Option<&'a mpsc::Sender<String>>,
    pub(super) event_tx: Option<&'a mpsc::Sender<RuntimeEvent>>,
    pub(super) execution_id: Option<&'a str>,
    pub(super) cwd: &'a std::path::Path,
}

pub(super) async fn read_prompt_events_for_id<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    stdin: &mut ChildStdin,
    options: PromptReadOptions<'_>,
) -> Result<(String, Vec<RuntimeEvent>)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let PromptReadOptions {
        backend,
        tool_whitelist,
        show_native,
        timeout_ms,
        expected_prompt_id,
        stream_tx,
        event_tx,
        execution_id,
        cwd,
    } = options;
    let mut output = String::new();
    let mut events = Vec::new();
    let mut streamed = false;
    let timeout_message = format!("ACP prompt timed out after {}ms", timeout_ms);
    loop {
        let Some(line) = read_next_line(lines, timeout_ms, &timeout_message).await? else {
            break;
        };
        let message = parse_message_line(&line, show_native)?;

        if let Some(error) = &message.error {
            events.push(runtime_event::map_acp_error(
                error.message.clone(),
                error.code,
                error.data.clone(),
            ));
            bail!(format_acp_error(error));
        }

        if is_response_id(&message, expected_prompt_id)
            && let Some(result) = &message.result
        {
            if let Some(text) = extract_text(result) {
                output.push_str(&text);
            }
            if let Some(usage) = runtime_event::token_usage_from_value(result) {
                push_event(&mut events, event_tx, RuntimeEvent::TokenUsage(usage));
            }
            if is_terminal_result(result) {
                break;
            }
        }

        let Some(method) = message.method.as_deref() else {
            continue;
        };

        for event in runtime_event::map_acp_events(method, message.params.as_ref()) {
            push_event(&mut events, event_tx, event);
        }

        match method {
            "session/update" | "session_update" => {
                if let Some(text) = text_from_session_update(message.params.as_ref()) {
                    streamed = true;
                    output.push_str(&text);
                    if let Some(tx) = stream_tx {
                        let _ = tx.try_send(text);
                    }
                }
            }
            "session/complete" | "session_complete" => {
                if !streamed
                    && let Some(text) = message.params.as_ref().and_then(extract_final_text)
                {
                    output.push_str(&text);
                }
                break;
            }
            "session/request_permission" | "request_permission" | "permission/request" => {
                let id = permission_request_id(&message)?;
                let params = message.params.clone().unwrap_or(Value::Null);
                let decision = acp_permission::answer_permission_request(
                    stdin,
                    id,
                    params,
                    execution_id,
                    backend,
                    tool_whitelist,
                    Some(cwd),
                )
                .await?;
                push_event(
                    &mut events,
                    event_tx,
                    RuntimeEvent::ApprovalDecision(decision),
                );
            }
            _ => {
                if let (Some(id), Some(intercepted)) = (
                    message.id.clone(),
                    router::try_intercept_tool_call(method, message.params.as_ref()),
                ) {
                    let (tool_name, tool_arguments) = acp_tool_call_parts(message.params.as_ref());
                    let call_id = id.as_str().unwrap_or("tool-call").to_string();
                    tracing::info!(
                        backend = %backend,
                        execution_id = execution_id.unwrap_or("-"),
                        tool_call_id = %call_id,
                        tool_name = %tool_name,
                        arguments = %tool_arguments,
                        "ACP backend tool call intercepted"
                    );
                    push_event(
                        &mut events,
                        event_tx,
                        RuntimeEvent::ToolCall(ToolCallEvent {
                            id: call_id.clone(),
                            name: tool_name.clone(),
                            arguments: tool_arguments.clone(),
                        }),
                    );
                    let result = intercepted.unwrap_or_else(|err| json!({"content":[{"type":"text","text":err.to_string()}],"isError":true}));
                    let ok = !result
                        .get("isError")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    tracing::info!(
                        backend = %backend,
                        execution_id = execution_id.unwrap_or("-"),
                        tool_call_id = %call_id,
                        tool_name = %tool_name,
                        ok,
                        result = %result,
                        "ACP backend tool result returned"
                    );
                    push_event(
                        &mut events,
                        event_tx,
                        RuntimeEvent::ToolResult(ToolResultEvent {
                            id: call_id,
                            name: tool_name,
                            ok,
                            result: result.clone(),
                        }),
                    );
                    super::client::send_response(stdin, id, result).await?;
                    continue;
                }

                if show_native {
                    eprintln!("[acp native] {}", line);
                }
            }
        }
    }
    Ok((output, events))
}

fn push_event(
    events: &mut Vec<RuntimeEvent>,
    event_tx: Option<&mpsc::Sender<RuntimeEvent>>,
    event: RuntimeEvent,
) {
    if let Some(tx) = event_tx {
        if let Err(e) = tx.try_send(event.clone()) {
            tracing::error!(
                error = %e,
                event = ?event,
                "Failed to send RuntimeEvent to TUI; event may have been dropped"
            );
        }
    }
    events.push(event);
}
