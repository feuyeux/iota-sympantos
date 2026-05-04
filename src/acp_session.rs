use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

use crate::acp::AcpBackend;

#[derive(Debug, Clone)]
pub struct AcpMcpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

pub fn session_new_params(backend: AcpBackend, cwd: &Path, servers: &[AcpMcpServer]) -> Value {
    let cwd = cwd.display().to_string();
    let mcp_servers = servers
        .iter()
        .map(|server| render_mcp_server(backend, server))
        .collect::<Vec<_>>();
    json!({ "cwd": cwd, "mcpServers": mcp_servers })
}

fn render_mcp_server(backend: AcpBackend, server: &AcpMcpServer) -> Value {
    if backend == AcpBackend::Hermes {
        let env = server
            .env
            .iter()
            .map(|(key, value)| format!("{}={}", key, value))
            .collect::<Vec<_>>();
        json!({
            "name": server.name,
            "type": "stdio",
            "command": server.command,
            "args": server.args,
            "env": env,
        })
    } else {
        json!({
            "name": server.name,
            "command": server.command,
            "args": server.args,
            "env": server.env,
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
    fn renders_gemini_mcp_servers_with_object_env() {
        let params = session_new_params(AcpBackend::Gemini, &PathBuf::from("."), &[server()]);
        let first = &params["mcpServers"][0];
        assert_eq!(first["name"], "iota-context");
        assert_eq!(first["command"], "iota");
        assert_eq!(first["env"]["TOKEN"], "redacted");
        assert!(first.get("type").is_none());
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
