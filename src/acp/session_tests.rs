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
