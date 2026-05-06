use super::*;

#[test]
fn mcp_servers_default_to_backend_capability() {
    let config = NimiaConfig {
        context_engine: Some(ContextEngineConfig::default()),
        ..NimiaConfig::default()
    };
    assert_eq!(context_mcp_servers(&config, AcpBackend::Codex).len(), 0);
    assert_eq!(context_mcp_servers(&config, AcpBackend::Gemini).len(), 2);
}

#[test]
fn mcp_try_enables_claude_and_codex() {
    let config = NimiaConfig {
        context_engine: Some(ContextEngineConfig::default()),
        context_engine_backend: Some(ContextEngineBackendConfig {
            codex: Some(BackendContextConfig {
                mcp_session_new: Some(serde_yaml::Value::String("try".to_string())),
                ..BackendContextConfig::default()
            }),
            ..ContextEngineBackendConfig::default()
        }),
        ..NimiaConfig::default()
    };
    assert_eq!(context_mcp_servers(&config, AcpBackend::Codex).len(), 2);
}

#[test]
fn recall_thresholds_default_values() {
    let config = NimiaConfig::default();
    let thresholds = context_recall_thresholds(&config);
    assert_eq!(thresholds.identity, 0.85);
    assert_eq!(thresholds.preference, 0.80);
    assert_eq!(thresholds.strategic, 0.80);
    assert_eq!(thresholds.domain, 0.80);
    assert_eq!(thresholds.procedural, 0.75);
    assert_eq!(thresholds.episodic, 0.70);
}

#[test]
fn episodic_compaction_keep_default_is_40() {
    let config = NimiaConfig::default();
    assert_eq!(context_episodic_compaction_keep(&config), 40);
}

#[test]
fn normalize_command_adds_cmd_suffix_on_windows() {
    let result = normalize_command("npx");
    if cfg!(windows) {
        assert_eq!(result, "npx.cmd");
    } else {
        assert_eq!(result, "npx");
    }
}

#[test]
fn expand_home_path_leaves_non_tilde_unchanged() {
    assert_eq!(
        expand_home_path("/absolute/path").unwrap(),
        "/absolute/path"
    );
    assert_eq!(expand_home_path("relative").unwrap(), "relative");
}
