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
