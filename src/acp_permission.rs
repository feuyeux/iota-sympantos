use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::OnceLock;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::approval::{self, ApprovalStore};
use crate::runtime_event::ApprovalDecisionEvent;

/// A pending approval request forwarded to the TUI.
pub struct ApprovalRequest {
    /// Human-readable tool name shown in the overlay.
    pub tool_name: String,
    /// Full params for storage.
    #[allow(dead_code)]
    pub params: Value,
    /// Reply with `true` = approved, `false` = denied.
    pub reply: oneshot::Sender<bool>,
}

/// When the TUI is active it installs a sender here; permission handling uses it
/// instead of blocking stdin.  Uses tokio::sync::RwLock so the channel can be
/// replaced when the TUI restarts within the same process, and reads never block
/// the tokio worker thread.
static TUI_APPROVAL_TX: OnceLock<RwLock<Option<mpsc::Sender<ApprovalRequest>>>> = OnceLock::new();

fn approval_lock() -> &'static RwLock<Option<mpsc::Sender<ApprovalRequest>>> {
    TUI_APPROVAL_TX.get_or_init(|| RwLock::new(None))
}

/// Install (or replace) the approval channel.  Call before starting the TUI event loop.
pub async fn install_tui_approval_channel(tx: mpsc::Sender<ApprovalRequest>) {
    *approval_lock().write().await = Some(tx);
}

/// Remove the approval channel (e.g. when the TUI exits).
#[allow(dead_code)]
pub async fn uninstall_tui_approval_channel() {
    *approval_lock().write().await = None;
}

pub async fn answer_permission_request(
    stdin: &mut ChildStdin,
    id: Value,
    params: Value,
    execution_id: Option<&str>,
) -> Result<ApprovalDecisionEvent> {
    tracing::debug!("[acp-permission] params={}", params);
    let tool_name = params
        .get("toolName")
        .or_else(|| params.get("name"))
        .or_else(|| params.get("tool"))
        .or_else(|| params.get("toolCall").and_then(|tc| tc.get("title")))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();

    // Read the channel once to avoid holding the lock across .await points and
    // to prevent double-locking (tokio::sync::RwLock is not reentrant).
    let tui_tx: Option<mpsc::Sender<ApprovalRequest>> = approval_lock().read().await.clone();

    // iota's own MCP tools are internal infrastructure — auto-approve without prompting.
    // Tool names may arrive as "iota_memory_write" or "mcp__iota-context__iota_memory_write".
    let is_iota_tool = tool_name.starts_with("iota_")
        || tool_name.contains("__iota_")
        || tool_name.starts_with("mcp__iota-");
    if is_iota_tool {
        tracing::debug!(tool = %tool_name, "[acp-permission] auto-approved iota tool");
        // Claude-code expects a response with optionId (not just {approved: true}).
        // Pick "allow_always" from the offered options if available, else "allow_once".
        let option_id = params
            .get("options")
            .and_then(Value::as_array)
            .and_then(|opts| {
                // Prefer "allow" (allow_once) over "allow_always" to avoid
                // claude-code prompting for a second confirmation when persisting
                // the "always allow" setting.
                opts.iter()
                    .find(|o| o.get("optionId").and_then(Value::as_str) == Some("allow"))
                    .or_else(|| {
                        opts.iter().find(|o| {
                            o.get("optionId")
                                .and_then(Value::as_str)
                                .map(|s| s.starts_with("allow"))
                                == Some(true)
                        })
                    })
                    .and_then(|o| o.get("optionId").and_then(Value::as_str))
            })
            .unwrap_or("allow");
        tracing::debug!(option_id, "[acp-permission] sending optionId");
        send_response(stdin, id.clone(), json!({ "optionId": option_id })).await?;
        return Ok(ApprovalDecisionEvent {
            request_id: id
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| id.to_string()),
            approved: true,
            reason: Some("auto-approved iota tool".to_string()),
        });
    }

    let approved = if let Some(tx) = tui_tx.clone() {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = ApprovalRequest {
            tool_name: tool_name.clone(),
            params: params.clone(),
            reply: reply_tx,
        };
        if tx.send(req).await.is_ok() {
            reply_rx.await.unwrap_or(false)
        } else {
            false
        }
    } else {
        let store = ApprovalStore::open_default().ok();
        let persisted_id = if let Some(store) = &store {
            store
                .record_request(execution_id, "acp", &tool_name, &params)
                .ok()
        } else {
            None
        };
        let dimensions = approval::classify_operation(&tool_name, &params);
        let policy = approval::default_decision(&dimensions);
        let result = prompt_yes_no(&format!(
            "Approve ACP tool request '{}' [{}]? ",
            tool_name, policy.reason
        ))
        .await?;
        if let (Some(store), Some(request_id)) = (&store, persisted_id.as_deref()) {
            let _ = store.record_decision(request_id, result, "interactive user decision");
        }
        result
    };

    let via_tui = tui_tx.is_some();
    if via_tui {
        if let Ok(store) = ApprovalStore::open_default() {
            if let Ok(request_id) = store.record_request(execution_id, "acp", &tool_name, &params) {
                let _ = store.record_decision(&request_id, approved, "tui user decision");
            }
        }
    }

    send_response(stdin, id.clone(), json!({ "approved": approved })).await?;
    Ok(ApprovalDecisionEvent {
        request_id: id
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string()),
        approved,
        reason: Some(if via_tui {
            "tui user decision".to_string()
        } else {
            "interactive user decision".to_string()
        }),
    })
}

async fn send_response(stdin: &mut ChildStdin, id: Value, result: Value) -> Result<()> {
    let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let mut line = serde_json::to_vec(&response).context("Failed to serialize ACP response")?;
    line.push(b'\n');
    stdin
        .write_all(line.as_slice())
        .await
        .context("Failed to write ACP response")?;
    stdin.flush().await.context("Failed to flush ACP stdin")?;
    Ok(())
}

async fn prompt_yes_no(message: &str) -> Result<bool> {
    let message = message.to_string();
    tokio::task::spawn_blocking(move || -> Result<bool> {
        print!("{}(y/n): ", message);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().eq_ignore_ascii_case("y"))
    })
    .await
    .context("Permission prompt task failed")?
}
