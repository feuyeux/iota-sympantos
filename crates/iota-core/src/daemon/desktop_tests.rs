use super::*;
use tokio::sync::oneshot;

#[tokio::test]
async fn approval_registry_delivers_decision_once() {
    let registry = ApprovalRegistry::default();
    let (tx, rx) = oneshot::channel();
    registry.insert("approval-1".to_string(), tx).await;

    assert!(registry.respond("approval-1", true).await);
    assert!(rx.await.unwrap());
    assert!(!registry.respond("approval-1", false).await);
}

#[tokio::test]
async fn approval_registry_returns_false_for_missing_id() {
    let registry = ApprovalRegistry::default();
    assert!(!registry.respond("missing", true).await);
}
