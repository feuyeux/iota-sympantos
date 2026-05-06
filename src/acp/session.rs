use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

use crate::acp::AcpBackend;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AcpMcpEnvShape {
    #[default]
    StringArray,
    Object,
}

impl AcpMcpEnvShape {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "string_array" | "string-array" | "array" => Some(Self::StringArray),
            "object" | "map" => Some(Self::Object),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpSessionOptions {
    pub always_send_empty_mcp_servers: bool,
    pub mcp_env_shape: AcpMcpEnvShape,
}

#[derive(Debug, Clone)]
pub struct AcpMcpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[cfg(test)]
pub fn session_new_params(backend: AcpBackend, cwd: &Path, servers: &[AcpMcpServer]) -> Value {
    session_new_params_with_options(backend, cwd, servers, AcpSessionOptions::default())
}

pub fn session_new_params_with_options(
    backend: AcpBackend,
    cwd: &Path,
    servers: &[AcpMcpServer],
    options: AcpSessionOptions,
) -> Value {
    let cwd = cwd.display().to_string();
    let mcp_servers = servers
        .iter()
        .map(|server| render_mcp_server(backend, server, options.mcp_env_shape))
        .collect::<Vec<_>>();
    let requires_mcp_servers_field =
        options.always_send_empty_mcp_servers || backend == AcpBackend::Codex;
    if mcp_servers.is_empty() && !requires_mcp_servers_field {
        json!({ "cwd": cwd })
    } else {
        json!({ "cwd": cwd, "mcpServers": mcp_servers })
    }
}

fn render_mcp_server(
    backend: AcpBackend,
    server: &AcpMcpServer,
    env_shape: AcpMcpEnvShape,
) -> Value {
    // Codex does not support mcp_session_new MCP servers in the same way.
    // ClaudeCode, Hermes, Gemini all require env as an array of "KEY=VALUE" strings
    // and an explicit type="stdio".
    let env: Value = match env_shape {
        AcpMcpEnvShape::StringArray => server
            .env
            .iter()
            .map(|(key, value)| Value::String(format!("{}={}", key, value)))
            .collect::<Vec<_>>()
            .into(),
        AcpMcpEnvShape::Object => json!(server.env),
    };
    if backend == AcpBackend::Gemini {
        // Gemini does not want the "name" field in the top-level mcp server object.
        json!({
            "type": "stdio",
            "command": server.command,
            "args": server.args,
            "env": env,
        })
    } else {
        json!({
            "name": server.name,
            "type": "stdio",
            "command": server.command,
            "args": server.args,
            "env": env,
        })
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
