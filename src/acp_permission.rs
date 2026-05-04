use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::OnceLock;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, oneshot};

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
/// instead of blocking stdin.
static TUI_APPROVAL_TX: OnceLock<mpsc::Sender<ApprovalRequest>> = OnceLock::new();

/// Call once before starting the TUI event loop.
pub fn install_tui_approval_channel(tx: mpsc::Sender<ApprovalRequest>) {
    let _ = TUI_APPROVAL_TX.set(tx);
}

pub async fn answer_permission_request(
    stdin: &mut ChildStdin,
    id: Value,
    params: Value,
    execution_id: Option<&str>,
) -> Result<ApprovalDecisionEvent> {
    let tool_name = params
        .get("toolName")
        .or_else(|| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();

    let approved = if let Some(tx) = TUI_APPROVAL_TX.get() {
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

    if TUI_APPROVAL_TX.get().is_some() {
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
        reason: Some(if TUI_APPROVAL_TX.get().is_some() {
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
