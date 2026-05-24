use super::*;
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
