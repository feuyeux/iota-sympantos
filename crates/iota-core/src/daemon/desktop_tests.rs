use super::*;
use crate::config::{ContextEngineConfig, ContextInjection, NimiaConfig, RecallThresholdsConfig};
use crate::memory::{MemoryFacet, MemoryInsert, MemoryMergeMode, MemoryScope, MemoryStore, MemoryType};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
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

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accept = tokio::spawn(async move { listener.accept().await.unwrap().0.into_split().1 });
    let _client = TcpStream::connect(addr).await.unwrap();
    let write_half = accept.await.unwrap();

    registry
        .insert(
            "turn-1".to_string(),
            handle,
            Arc::new(Mutex::new(write_half)),
        )
        .await;

    assert!(registry.abort("turn-1").await.is_some());
    assert!(registry.abort("turn-1").await.is_none());
}

#[tokio::test]
async fn desktop_connection_rejects_message_before_hello() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.unwrap().0;
        let pool = Arc::new(Mutex::new(EnginePool::new(
            NimiaConfig::default(),
            false,
            1000,
        )));
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let message: DaemonClientMessage = serde_json::from_str(line.trim()).unwrap();
        handle_desktop_connection(
            message,
            reader,
            write_half,
            pool,
            ApprovalRegistry::default(),
            TurnRegistry::default(),
        )
        .await
        .unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    let message = DaemonClientMessage::GetConfig;
    let mut line = serde_json::to_vec(&message).unwrap();
    line.push(b'\n');
    client.write_all(&line).await.unwrap();
    let mut reader = BufReader::new(client);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    let response: DaemonServerMessage = serde_json::from_str(response.trim()).unwrap();

    assert!(matches!(
        response,
        DaemonServerMessage::ProtocolError { .. }
    ));
    server.await.unwrap();
}

#[test]
fn memory_summary_counts_bucket_lengths() {
    let mut buckets = DesktopMemoryBuckets::default();
    buckets
        .identity
        .push(desktop_record("id-1", "semantic", Some("identity")));
    buckets
        .episodic
        .push(desktop_record("id-2", "episodic", None));

    let summary = memory_summary(&buckets);
    assert_eq!(summary.identity, 1);
    assert_eq!(summary.episodic, 1);
    assert_eq!(summary.preference, 0);
}

#[tokio::test]
async fn memory_context_snapshot_workspace_uses_configured_recall_thresholds() {
    let memory_path = std::env::temp_dir().join(format!(
        "iota-desktop-memory-thresholds-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let cwd = std::env::current_dir().unwrap();
    let store = MemoryStore::open(&memory_path).unwrap();
    store
        .insert_with_merge(
            MemoryInsert {
                memory_type: MemoryType::Semantic,
                facet: Some(MemoryFacet::Identity),
                scope: MemoryScope::User,
                scope_id: "local-user".to_string(),
                content: "Low confidence identity should stay hidden".to_string(),
                confidence: 0.4,
                source_backend: None,
                source_session_id: None,
                source_execution_id: None,
                metadata_json: None,
                ttl_days: 30,
                supersedes: None,
            },
            MemoryMergeMode::Add,
        )
        .unwrap();

    let pool = Arc::new(Mutex::new(EnginePool::new(
        NimiaConfig {
            context_engine: Some(ContextEngineConfig {
                memory_db: Some(memory_path.display().to_string()),
                recall_thresholds: Some(RecallThresholdsConfig {
                    identity: 0.9,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        false,
        1000,
    )));

    let snapshot =
        memory_context_snapshot(cwd, DesktopMemoryScopeMode::Workspace, pool).await;
    assert!(snapshot.memory.identity.is_empty());
}

#[tokio::test]
async fn memory_context_snapshot_reports_injection_off_as_disabled() {
    let cwd = std::env::current_dir().unwrap();
    let pool = Arc::new(Mutex::new(EnginePool::new(
        NimiaConfig {
            context_engine: Some(ContextEngineConfig {
                injection: ContextInjection::Off,
                ..Default::default()
            }),
            ..Default::default()
        },
        false,
        1000,
    )));

    let snapshot =
        memory_context_snapshot(cwd, DesktopMemoryScopeMode::Workspace, pool).await;
    assert!(!snapshot.context_engine.enabled);
}

fn desktop_record(id: &str, memory_type: &str, facet: Option<&str>) -> DesktopMemoryRecord {
    DesktopMemoryRecord {
        id: id.to_string(),
        memory_type: memory_type.to_string(),
        facet: facet.map(str::to_string),
        scope: "user".to_string(),
        scope_id: "local-user".to_string(),
        content: "content".to_string(),
        confidence: 1.0,
        created_at: 1,
        updated_at: 2,
        expires_at: 3,
    }
}
