#![allow(dead_code)]

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
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

pub async fn call_stdio(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    call: McpToolCall,
    timeout_ms: u64,
) -> Result<McpToolResult> {
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
    write_json(&mut stdin, json!({"jsonrpc":"2.0","id":"call","method":"tools/call","params":{"name":call.name,"arguments":call.arguments}})).await?;
    let result = wait_id(&mut lines, "call", timeout_ms).await?;
    let _ = stdin.shutdown().await;
    let _ = child.kill().await;
    Ok(McpToolResult {
        ok: !result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        content: result,
        error: None,
    })
}

async fn write_json(stdin: &mut tokio::process::ChildStdin, value: Value) -> Result<()> {
    let mut line = serde_json::to_vec(&value)?;
    line.push(b'\n');
    stdin.write_all(&line).await?;
    stdin.flush().await?;
    Ok(())
}

async fn wait_id(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
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
