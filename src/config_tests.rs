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
fn context_mcp_server_enables_memory_route_logging() {
    let config = NimiaConfig::default();
    let servers = context_mcp_servers(&config, AcpBackend::Gemini);
    let context = servers
        .iter()
        .find(|server| server.name == "iota-context")
        .expect("iota-context server should be present");

    assert_eq!(
        context.env.get("RUST_LOG").map(String::as_str),
        Some("iota::context::server=info")
    );
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

#[test]
fn context_injection_deserializes_normalized_values() {
    let injection: ContextInjection = serde_yaml::from_str("\" McP \"").unwrap();
    assert_eq!(injection, ContextInjection::Mcp);
    assert_eq!(serde_yaml::to_string(&injection).unwrap(), "mcp\n");
}

#[test]
fn context_injection_rejects_unknown_values() {
    let err = serde_yaml::from_str::<ContextInjection>("\"maybe\"").unwrap_err();
    assert!(err.to_string().contains("invalid context_engine.injection"));
}

#[test]
fn backend_version_mapping_deserializes_from_nimia() {
    let section: BackendConfig = serde_yaml::from_str(
        r#"
enabled: true
version_mapping:
    acp: "0.12.0"
    bin: "0.128.0"
"#,
    )
    .unwrap();

    assert_eq!(
        section.version_mapping,
        Some(BackendVersionMapping {
            acp: Some("0.12.0".to_string()),
            bin: Some("0.128.0".to_string()),
        })
    );
}

#[test]
fn store_paths_join_expected_database_names() {
    let root = std::path::PathBuf::from("root").join("context");
    let paths = paths::StorePaths::new(root.clone());
    assert_eq!(paths.events_db(), root.join("events.sqlite"));
    assert_eq!(paths.memory_db(), root.join("memory.sqlite"));
    assert_eq!(paths.sessions_db(), root.join("sessions.sqlite"));
    assert_eq!(paths.approvals_db(), root.join("approvals.sqlite"));
}
