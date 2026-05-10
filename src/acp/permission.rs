use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::OnceLock;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::runtime_event::ApprovalDecisionEvent;
use crate::store::approval::{self, ApprovalStore};
use crate::telemetry::spans;

use super::AcpBackend;

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
    backend: AcpBackend,
    tool_whitelist: &[String],
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
    let mut approval_span = spans::start_approval_span(&tool_name);

    tracing::info!(tool_name = %tool_name, execution_id = execution_id.unwrap_or("-"), "approval.requested");
    // Read the channel once to avoid holding the lock across .await points and
    // to prevent double-locking (tokio::sync::RwLock is not reentrant).
    let tui_tx: Option<mpsc::Sender<ApprovalRequest>> = approval_lock().read().await.clone();

    // iota's own MCP tools are internal infrastructure — auto-approve without prompting.
    // Tool names may arrive as "iota_memory_write" or "mcp__iota-context__iota_memory_write".
    let is_iota_tool = tool_name.starts_with("iota_")
        || tool_name.contains("__iota_")
        || tool_name.starts_with("mcp__iota-");
    let whitelist_hit = tool_is_whitelisted(&tool_name, tool_whitelist);
    if is_iota_tool || whitelist_hit {
        tracing::debug!(tool = %tool_name, "[acp-permission] auto-approved iota tool");
        send_approved_response(stdin, id.clone(), &params).await?;
        spans::end_span_ok(&mut approval_span);
        return Ok(ApprovalDecisionEvent {
            request_id: id
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| id.to_string()),
            approved: true,
            reason: Some(if is_iota_tool {
                "auto-approved iota tool".to_string()
            } else {
                format!("auto-approved by backend whitelist ({})", backend)
            }),
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

    tracing::info!(tool_name = %tool_name, approved, "approval.decided");
    send_approved_or_denied_response(stdin, id.clone(), approved, &params).await?;
    if approved {
        spans::end_span_ok(&mut approval_span);
    } else {
        spans::end_span_error(&mut approval_span, "approval denied");
    }
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

async fn send_approved_response(stdin: &mut ChildStdin, id: Value, params: &Value) -> Result<()> {
    // Claude-code ACP adapter expects: {outcome: {outcome: "selected", optionId: "..."}}
    // Prefer "allow_always" to persist the decision across the session.
    if let Some(option_id) = params
        .get("options")
        .and_then(Value::as_array)
        .and_then(|opts| {
            opts.iter()
                .find(|o| o.get("optionId").and_then(Value::as_str) == Some("allow_always"))
                .or_else(|| {
                    opts.iter()
                        .find(|o| o.get("optionId").and_then(Value::as_str) == Some("allow"))
                })
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
    {
        return send_response(
            stdin,
            id,
            json!({
                "outcome": { "outcome": "selected", "optionId": option_id }
            }),
        )
        .await;
    }
    send_response(stdin, id, json!({ "approved": true })).await
}

async fn send_approved_or_denied_response(
    stdin: &mut ChildStdin,
    id: Value,
    approved: bool,
    params: &Value,
) -> Result<()> {
    if approved {
        send_approved_response(stdin, id, params).await
    } else {
        // Use outcome format for denial as well.
        let reject_id = params
            .get("options")
            .and_then(Value::as_array)
            .and_then(|opts| {
                opts.iter()
                    .find(|o| o.get("optionId").and_then(Value::as_str) == Some("reject"))
                    .and_then(|o| o.get("optionId").and_then(Value::as_str))
            });
        if let Some(option_id) = reject_id {
            send_response(
                stdin,
                id,
                json!({ "outcome": { "outcome": "selected", "optionId": option_id } }),
            )
            .await
        } else {
            send_response(stdin, id, json!({ "approved": false })).await
        }
    }
}

fn tool_is_whitelisted(tool_name: &str, rules: &[String]) -> bool {
    rules.iter().any(|rule| tool_rule_match(tool_name, rule))
}

fn tool_rule_match(tool_name: &str, rule: &str) -> bool {
    let rule = canonical_tool_name(rule);
    if rule.is_empty() {
        return false;
    }
    if rule == "*" {
        return true;
    }

    let tool = canonical_tool_name(tool_name);
    let tool_tail = tool.split("__").last().unwrap_or(tool.as_str());

    wildcard_match(&tool, &rule)
        || wildcard_match(tool_tail, &rule)
        || wildcard_match(&tool, &format!("*__{}", rule))
}

fn wildcard_match(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(stripped) = pattern.strip_suffix('*') {
        return text.starts_with(stripped);
    }
    if let Some(stripped) = pattern.strip_prefix('*') {
        return text.ends_with(stripped);
    }
    text == pattern
}

fn canonical_tool_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_star_matches_everything() {
        assert!(tool_is_whitelisted("any_tool", &["*".to_string()]));
    }

    #[test]
    fn exact_match() {
        assert!(tool_is_whitelisted(
            "iota_memory_write",
            &["iota_memory_write".to_string()]
        ));
    }

    #[test]
    fn prefix_wildcard() {
        assert!(tool_is_whitelisted(
            "iota_skill_run",
            &["iota_skill_*".to_string()]
        ));
    }

    #[test]
    fn suffix_wildcard() {
        assert!(tool_is_whitelisted(
            "mcp__iota_read",
            &["*_read".to_string()]
        ));
    }

    #[test]
    fn no_match_returns_false() {
        assert!(!tool_is_whitelisted(
            "dangerous_tool",
            &["safe_tool".to_string()]
        ));
    }

    #[test]
    fn empty_rule_never_matches() {
        assert!(!tool_is_whitelisted("any", &["".to_string()]));
    }

    #[test]
    fn empty_whitelist_never_matches() {
        assert!(!tool_is_whitelisted("any", &[]));
    }

    #[test]
    fn mcp_prefixed_tool_matches_tail() {
        // "mcp__iota-context__iota_memory_write" should match rule "iota_memory_write"
        assert!(tool_is_whitelisted(
            "mcp__iota-context__iota_memory_write",
            &["iota_memory_write".to_string()]
        ));
    }

    #[test]
    fn dash_underscore_canonicalization() {
        assert!(tool_is_whitelisted(
            "iota-memory-write",
            &["iota_memory_write".to_string()]
        ));
    }

    #[test]
    fn case_insensitive_matching() {
        assert!(tool_is_whitelisted(
            "Iota_Memory_Write",
            &["iota_memory_write".to_string()]
        ));
    }

    #[test]
    fn canonical_tool_name_normalizes() {
        assert_eq!(canonical_tool_name("Foo-Bar Baz"), "foo_barbaz");
    }

    #[test]
    fn wildcard_match_exact() {
        assert!(wildcard_match("abc", "abc"));
        assert!(!wildcard_match("abc", "xyz"));
    }

    #[test]
    fn wildcard_match_star_alone() {
        assert!(wildcard_match("anything", "*"));
    }

    #[test]
    fn wildcard_match_prefix() {
        assert!(wildcard_match("iota_skill_run", "iota_skill_*"));
        assert!(!wildcard_match("other_run", "iota_skill_*"));
    }

    #[test]
    fn wildcard_match_suffix() {
        assert!(wildcard_match("mcp__tool_read", "*_read"));
        assert!(!wildcard_match("mcp__tool_write", "*_read"));
    }

    #[test]
    fn tool_rule_match_with_double_underscore_prefix() {
        // rule "iota_memory_write" should also match via "*__iota_memory_write" path
        assert!(tool_rule_match(
            "mcp__context__iota_memory_write",
            "iota_memory_write"
        ));
    }

    #[test]
    fn multiple_rules_any_match_wins() {
        let rules = vec!["safe_tool".to_string(), "iota_*".to_string()];
        assert!(tool_is_whitelisted("iota_read", &rules));
        assert!(tool_is_whitelisted("safe_tool", &rules));
        assert!(!tool_is_whitelisted("dangerous", &rules));
    }
}
