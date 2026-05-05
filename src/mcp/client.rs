#![allow(dead_code)]

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{Duration, timeout};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub ok: bool,
    #[serde(default)]
    pub content: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Persistent session (reuse across multiple calls) ─────────────────────────

/// A long-lived MCP server connection.  Create once via [`McpSession::start`]
/// and reuse across multiple [`McpSession::call`] invocations to avoid the
/// per-call process spawn overhead.
pub struct McpSession {
    _child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    timeout_ms: u64,
}

impl McpSession {
    /// Spawn the MCP server and complete the initialization handshake.
    pub async fn start(
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        timeout_ms: u64,
    ) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to start MCP server {}", command))?;
        let mut stdin = child.stdin.take().context("MCP stdin not piped")?;
        let stdout = child.stdout.take().context("MCP stdout not piped")?;
        let mut lines = BufReader::new(stdout).lines();

        write_json(&mut stdin, json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"iota","version":env!("CARGO_PKG_VERSION")}}})).await?;
        wait_id(&mut lines, "init", timeout_ms).await?;
        write_json(
            &mut stdin,
            json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .await?;
        Ok(Self {
            _child: child,
            stdin,
            lines,
            timeout_ms,
        })
    }

    /// Send a tool call on the existing connection.
    pub async fn call(&mut self, call: McpToolCall) -> Result<McpToolResult> {
        write_json(
            &mut self.stdin,
            json!({"jsonrpc":"2.0","id":"call","method":"tools/call","params":{"name":call.name,"arguments":call.arguments}}),
        )
        .await?;
        let result = wait_id(&mut self.lines, "call", self.timeout_ms).await?;
        Ok(McpToolResult {
            ok: !result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            content: result,
            error: None,
        })
    }
}

// ── One-shot helper (kept for backwards compat / simple callers) ──────────────

/// Spawn a fresh MCP server process, make a single tool call, then exit.
///
/// For callers that make multiple sequential calls to the same server, prefer
/// [`McpSession::start`] + [`McpSession::call`] to amortize the spawn cost.
pub async fn call_stdio(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    call: McpToolCall,
    timeout_ms: u64,
) -> Result<McpToolResult> {
    let mut results = call_stdio_batch(command, args, env, vec![call], timeout_ms).await?;
    Ok(results
        .pop()
        .context("MCP batch returned no result for single call")?)
}

pub async fn call_stdio_batch(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    calls: Vec<McpToolCall>,
    timeout_ms: u64,
) -> Result<Vec<McpToolResult>> {
    let mut child = Command::new(command)
        .args(args)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to start MCP server {}", command))?;
    let mut stdin = child.stdin.take().context("MCP stdin not piped")?;
    let stdout = child.stdout.take().context("MCP stdout not piped")?;
    let mut lines = BufReader::new(stdout).lines();

    write_json(&mut stdin, json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"iota","version":env!("CARGO_PKG_VERSION")}}})).await?;
    wait_id(&mut lines, "init", timeout_ms).await?;
    write_json(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    )
    .await?;
    let mut results = Vec::with_capacity(calls.len());
    for (index, call) in calls.into_iter().enumerate() {
        let id = format!("call:{}", index);
        write_json(&mut stdin, json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":call.name,"arguments":call.arguments}})).await?;
        let result = wait_id(&mut lines, &format!("call:{}", index), timeout_ms).await?;
        results.push(McpToolResult {
            ok: !result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            content: result,
            error: None,
        });
    }
    let _ = stdin.shutdown().await;
    let _ = child.kill().await;
    Ok(results)
}

async fn write_json(stdin: &mut ChildStdin, value: Value) -> Result<()> {
    let mut line = serde_json::to_vec(&value)?;
    line.push(b'\n');
    stdin.write_all(&line).await?;
    stdin.flush().await?;
    Ok(())
}

async fn wait_id(
    lines: &mut Lines<BufReader<ChildStdout>>,
    id: &str,
    timeout_ms: u64,
) -> Result<Value> {
    let deadline = Duration::from_millis(timeout_ms);
    loop {
        let line = timeout(deadline, lines.next_line())
            .await
            .map_err(|_| anyhow!("MCP request timed out after {}ms", timeout_ms))??
            .context("MCP server exited before response")?;
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("MCP server emitted non-JSON line: {}", line))?;
        if value.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(anyhow!("MCP error: {}", error));
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
}
