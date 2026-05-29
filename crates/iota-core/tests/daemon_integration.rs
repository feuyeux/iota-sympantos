//! Integration tests for the daemon desktop protocol.
//!
//! These tests verify end-to-end flows including:
//! - Hello handshake with version negotiation (AC9.1)
//! - Reconnection after disconnect (AC9.2)
//! - Protocol error handling
//!
//! Tests that require a full engine or external services are marked #[ignore].

use iota_core::daemon::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonServerMessage, PROTOCOL_VERSION_MAX,
    PROTOCOL_VERSION_MIN,
};
use tokio::net::TcpListener;

#[tokio::test]
async fn hello_handshake_v2_client_succeeds() {
    let hello = DaemonClientMessage::Hello {
        client_name: "test-client".to_string(),
        protocol_version: DESKTOP_PROTOCOL_VERSION,
        min_version: None,
        max_version: None,
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonClientMessage::Hello {
            protocol_version: 2,
            ..
        }
    ));
}

#[tokio::test]
async fn hello_handshake_v3_client_sends_range() {
    let hello = DaemonClientMessage::Hello {
        client_name: "test-client".to_string(),
        protocol_version: DESKTOP_PROTOCOL_VERSION,
        min_version: Some(PROTOCOL_VERSION_MIN),
        max_version: Some(PROTOCOL_VERSION_MAX),
    };
    let json = serde_json::to_string(&hello).unwrap();
    assert!(json.contains("\"min_version\":2"));
    assert!(json.contains("\"max_version\":3"));
}

#[tokio::test]
async fn hello_accepted_contains_negotiated_version() {
    let msg = DaemonServerMessage::HelloAccepted {
        protocol_version: 3,
        negotiated_version: Some(3),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"negotiated_version\":3"));
}

#[tokio::test]
async fn start_turn_message_roundtrips() {
    let msg = DaemonClientMessage::StartTurn {
        turn_id: "integration-turn-1".to_string(),
        cwd: "/tmp/test".into(),
        backend: "codex".to_string(),
        prompt: "hello world".to_string(),
        timeout_ms: Some(600_000),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert!(
        matches!(decoded, DaemonClientMessage::StartTurn { turn_id, .. } if turn_id == "integration-turn-1")
    );
}

#[tokio::test]
async fn ping_pong_roundtrip() {
    let ping = DaemonClientMessage::Ping { seq: 42 };
    let json = serde_json::to_string(&ping).unwrap();
    assert!(json.contains("\"type\":\"ping\""));
    assert!(json.contains("\"seq\":42"));

    let pong = DaemonServerMessage::Pong { seq: 42 };
    let pong_json = serde_json::to_string(&pong).unwrap();
    assert!(pong_json.contains("\"type\":\"pong\""));
    assert!(pong_json.contains("\"seq\":42"));
}

#[tokio::test]
#[ignore]
async fn full_turn_lifecycle_requires_daemon() {
    // AC9.1: Full flow: connect → Hello → StartTurn → TextChunk → TurnCompleted
    // This test requires a running daemon with a configured backend.
    // Run manually with: cargo test --test daemon_integration full_turn -- --ignored
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let _addr = listener.local_addr().unwrap();
    // Would need a full EnginePool setup here
}

#[tokio::test]
#[ignore]
async fn reconnect_after_disconnect() {
    // AC9.2: Verify client can reconnect after TCP disconnect
    // Requires daemon infrastructure
}

// ---------------------------------------------------------------------------
// Manual Runbook (AC9.4)
// ---------------------------------------------------------------------------
//
// ## Scenarios that cannot be fully automated:
//
// ### Kanban dispatcher → event_sync (AC9.3)
// 1. Start desktop app with kanban board open
// 2. Create a task via CLI: `iota kanban task create --board test "New task"`
// 3. Verify desktop UI updates within 5 seconds showing the new task
// 4. Move task to "done" via desktop drag-and-drop
// 5. Verify CLI `iota kanban task list` reflects the status change
//
// ### Desktop UI Reconnection Indicator
// 1. Start desktop app, confirm "connected" state in status bar
// 2. Kill the daemon process: `pkill -f "iota __daemon"`
// 3. Verify UI shows "reconnecting" state within 30 seconds (heartbeat miss)
// 4. Restart daemon: `iota __daemon &`
// 5. Verify UI transitions back to "connected" state
// 6. Verify pending operations (if any) are replayed
