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
    if mcp_servers.is_empty() && !options.always_send_empty_mcp_servers {
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn server() -> AcpMcpServer {
        let mut env = BTreeMap::new();
        env.insert("TOKEN".to_string(), "redacted".to_string());
        AcpMcpServer {
            name: "iota-context".to_string(),
            command: "iota".to_string(),
            args: vec!["context-mcp".to_string()],
            env,
        }
    }

    #[test]
    fn renders_gemini_mcp_servers_with_string_env() {
        let params = session_new_params(AcpBackend::Gemini, &PathBuf::from("."), &[server()]);
        let first = &params["mcpServers"][0];
        // Gemini does not include "name" in the server object
        assert_eq!(first["type"], "stdio");
        assert_eq!(first["env"][0], "TOKEN=redacted");
        assert!(first.get("name").is_none());
    }

    #[test]
    fn renders_hermes_mcp_servers_with_string_env() {
        let params = session_new_params(AcpBackend::Hermes, &PathBuf::from("."), &[server()]);
        let first = &params["mcpServers"][0];
        assert_eq!(first["name"], "iota-context");
        assert_eq!(first["type"], "stdio");
        assert_eq!(first["env"][0], "TOKEN=redacted");
    }
}
