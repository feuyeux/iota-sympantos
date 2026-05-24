use super::*;
use crate::runtime_event::{OutputEvent, RuntimeEvent};

#[test]
fn legacy_prompt_request_still_roundtrips() {
    let request = DaemonPromptRequest {
        backend: "gemini".to_string(),
        cwd: "/tmp/project".to_string(),
        prompt: "hello".to_string(),
        execution_id: Some("exec-1".to_string()),
        timeout_ms: Some(1000),
        timing: true,
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(!json.contains("StartTurn"));

    let decoded: DaemonPromptRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.backend, "gemini");
    assert_eq!(decoded.execution_id.as_deref(), Some("exec-1"));
}

#[test]
fn desktop_start_turn_roundtrips() {
    let message = DaemonClientMessage::StartTurn {
        turn_id: "turn-1".to_string(),
        cwd: "/tmp/project".into(),
        backend: "codex".to_string(),
        prompt: "implement feature".to_string(),
        timeout_ms: Some(600_000),
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"start_turn\""));

    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonClientMessage::StartTurn { turn_id, backend, .. }
            if turn_id == "turn-1" && backend == "codex"
    ));
}

#[test]
fn desktop_server_event_roundtrips_runtime_event() {
    let message = DaemonServerMessage::TurnEvent {
        turn_id: "turn-1".to_string(),
        event: Box::new(RuntimeEvent::Output(OutputEvent {
            text: "chunk".to_string(),
            role: Some("assistant".to_string()),
        })),
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"turn_event\""));

    let decoded: DaemonServerMessage = serde_json::from_str(&json).unwrap();
    if let DaemonServerMessage::TurnEvent { event, .. } = decoded {
        let RuntimeEvent::Output(OutputEvent { text, .. }) = *event else {
            panic!("expected Output event");
        };
        assert_eq!(text, "chunk");
        return;
    }
    panic!("decoded message did not match expected structure");
}

#[test]
fn desktop_config_snapshot_masks_api_keys() {
    let mut config = crate::config::NimiaConfig::default();
    let model = crate::config::ModelConfig {
        api_key: Some("secret-value".to_string()),
        ..Default::default()
    };
    let backend = crate::config::BackendConfig {
        enabled: true,
        model: Some(model),
        ..Default::default()
    };
    config.gemini = Some(backend);

    let snapshot = DesktopConfigSnapshot::from_config(&config);
    let json = serde_json::to_string(&snapshot).unwrap();

    assert!(!json.contains("secret-value"));
    assert!(json.contains("\"api_key_configured\":true"));
}

#[test]
fn desktop_model_update_preserves_untouched_fields() {
    let mut config = config_with_gemini_model();

    apply_desktop_model_update(
        &mut config,
        AcpBackend::Gemini,
        DesktopModelConfig {
            name: Some("gemini-2.5-flash".to_string()),
            ..Default::default()
        },
    );

    let model = config.gemini.unwrap().model.unwrap();
    assert_eq!(model.provider.as_deref(), Some("google"));
    assert_eq!(model.name.as_deref(), Some("gemini-2.5-flash"));
    assert_eq!(model.base_url.as_deref(), Some("https://example.test"));
    assert_eq!(model.api_key.as_deref(), Some("secret-value"));
}

#[test]
fn desktop_model_update_clears_blank_text_fields() {
    let mut config = config_with_gemini_model();

    apply_desktop_model_update(
        &mut config,
        AcpBackend::Gemini,
        DesktopModelConfig {
            provider: Some(" ".to_string()),
            base_url: Some(String::new()),
            ..Default::default()
        },
    );

    let model = config.gemini.unwrap().model.unwrap();
    assert_eq!(model.provider, None);
    assert_eq!(model.name.as_deref(), Some("gemini-1.5-pro"));
    assert_eq!(model.base_url, None);
    assert_eq!(model.api_key.as_deref(), Some("secret-value"));
}

fn config_with_gemini_model() -> crate::config::NimiaConfig {
    let model = crate::config::ModelConfig {
        provider: Some("google".to_string()),
        name: Some("gemini-1.5-pro".to_string()),
        base_url: Some("https://example.test".to_string()),
        api_key: Some("secret-value".to_string()),
    };
    let backend = crate::config::BackendConfig {
        enabled: true,
        model: Some(model),
        ..Default::default()
    };

    crate::config::NimiaConfig {
        gemini: Some(backend),
        ..Default::default()
    }
}

#[test]
fn memory_context_snapshot_request_roundtrips() {
    let message = DaemonClientMessage::GetMemoryContextSnapshot {
        cwd: PathBuf::from("/tmp/iota-workspace"),
        scope_mode: DesktopMemoryScopeMode::Workspace,
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"get_memory_context_snapshot\""));
    assert!(json.contains("\"scope_mode\":\"workspace\""));

    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, message);
}

#[test]
fn memory_context_snapshot_response_roundtrips() {
    let snapshot = DesktopMemoryContextSnapshot {
        cwd: PathBuf::from("/tmp/iota-workspace"),
        scope_mode: DesktopMemoryScopeMode::All,
        memory: DesktopMemoryBuckets::default(),
        memory_summary: DesktopMemorySummary::default(),
        runtime_context: Some(DesktopRuntimeContextSnapshot {
            turn_id: "turn-1".to_string(),
            backend: "codex".to_string(),
            cwd: PathBuf::from("/tmp/iota-workspace"),
            session_id: "session-1".to_string(),
            model: Some("model-a".to_string()),
            created_at: 123,
            capsule_text: "<iota-context>\n</iota-context>\n\nUser request:\nhello".to_string(),
            sections: vec![DesktopContextSection {
                name: "session".to_string(),
                chars: 24,
                preview: "iota_session_id: session-1".to_string(),
            }],
            budgets: DesktopContextBudgetsSnapshot::default(),
        }),
        context_engine: DesktopContextEngineSnapshot {
            enabled: true,
            memory_db: Some(PathBuf::from("/Users/example/.i6/context/memory.sqlite")),
            budgets: DesktopContextBudgetsSnapshot::default(),
        },
        errors: vec![],
    };

    let message = DaemonServerMessage::MemoryContextSnapshot { snapshot };
    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"memory_context_snapshot\""));

    let decoded: DaemonServerMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonServerMessage::MemoryContextSnapshot { .. }
    ));
}
