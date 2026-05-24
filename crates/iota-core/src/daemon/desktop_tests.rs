use super::*;
use crate::config::{BackendConfig, CommandConfig, ModelConfig, NimiaConfig};
use tokio::sync::oneshot;

#[tokio::test]
async fn approval_registry_delivers_decision_once() {
    let registry = ApprovalRegistry::default();
    let (tx, rx) = oneshot::channel();
    registry
        .insert("turn-1".to_string(), "approval-1".to_string(), tx)
        .await;

    assert!(registry.respond("approval-1", true).await);
    assert!(rx.await.unwrap());
    assert!(!registry.respond("approval-1", false).await);
}

#[tokio::test]
async fn approval_registry_returns_false_for_missing_id() {
    let registry = ApprovalRegistry::default();
    assert!(!registry.respond("missing", true).await);
}

#[tokio::test]
async fn approval_registry_denies_all_pending_for_turn() {
    let registry = ApprovalRegistry::default();
    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();
    let (other_tx, other_rx) = oneshot::channel();

    registry
        .insert("turn-1".to_string(), "approval-1".to_string(), tx1)
        .await;
    registry
        .insert("turn-1".to_string(), "approval-2".to_string(), tx2)
        .await;
    registry
        .insert("turn-2".to_string(), "approval-3".to_string(), other_tx)
        .await;

    assert_eq!(registry.deny_for_turn("turn-1").await, 2);
    assert!(!rx1.await.unwrap());
    assert!(!rx2.await.unwrap());
    assert!(registry.respond("approval-3", true).await);
    assert!(other_rx.await.unwrap());
}

#[tokio::test]
async fn turn_registry_cancel_reports_whether_turn_existed() {
    let registry = TurnRegistry::default();
    let handle = tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    });

    registry.insert("turn-1".to_string(), handle).await;

    assert!(registry.abort("turn-1").await);
    assert!(!registry.abort("turn-1").await);
}

#[test]
fn backend_check_fails_for_disabled_backend() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(false, "npx", "secret")),
        ..Default::default()
    };

    let result = backend_check_result(&config, AcpBackend::Gemini);

    assert!(!result.ok);
    assert!(result.details.contains("disabled"));
}

#[test]
fn backend_check_fails_for_missing_command() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(true, " ", "secret")),
        ..Default::default()
    };

    let result = backend_check_result(&config, AcpBackend::Gemini);

    assert!(!result.ok);
    assert!(result.details.contains("missing acp.command"));
}

#[test]
fn backend_check_fails_for_missing_api_key() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(true, "npx", "<api-key>")),
        ..Default::default()
    };

    let result = backend_check_result(&config, AcpBackend::Gemini);

    assert!(!result.ok);
    assert!(result.details.contains("missing API key"));
}

#[test]
fn backend_check_passes_for_configured_backend() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(true, "npx", "secret")),
        ..Default::default()
    };

    let result = backend_check_result(&config, AcpBackend::Gemini);

    assert!(result.ok);
    assert_eq!(result.details, "backend is configured");
}

fn gemini_config(enabled: bool, command: &str, api_key: &str) -> BackendConfig {
    BackendConfig {
        enabled,
        acp: Some(CommandConfig {
            command: command.to_string(),
            args: vec![],
        }),
        model: Some(ModelConfig {
            api_key: Some(api_key.to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}
