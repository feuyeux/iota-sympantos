use super::*;
use crate::runtime_event::{
    ApprovalDecisionEvent, ApprovalRequestEvent, ErrorEvent, ToolCallEvent,
};

#[test]
fn execution_id_conflict_is_rejected() {
    let store = EventStore::open(Path::new(":memory:")).unwrap();
    let id = store
        .begin_execution_with_id("codex", "session", "hash-a", Some("exec-1"))
        .unwrap();
    assert_eq!(id, "exec-1");
    let same = store
        .begin_execution_with_id("codex", "session", "hash-a", Some("exec-1"))
        .unwrap();
    assert_eq!(same, "exec-1");
    let conflict = store.begin_execution_with_id("codex", "session", "hash-b", Some("exec-1"));
    assert!(conflict.is_err());
}

#[test]
fn persists_runtime_events_in_sequence() {
    let store = EventStore::open(Path::new(":memory:")).unwrap();
    let execution_id = store
        .begin_execution_with_id("codex", "session", "hash-a", Some("exec-events"))
        .unwrap();
    let events = [
        RuntimeEvent::Output(OutputEvent {
            text: "hello".to_string(),
            role: Some("assistant".to_string()),
        }),
        RuntimeEvent::ToolCall(ToolCallEvent {
            id: "tool-1".to_string(),
            name: "iota_memory_search".to_string(),
            arguments: serde_json::json!({"query":"hello"}),
        }),
        RuntimeEvent::ApprovalRequest(ApprovalRequestEvent {
            id: "approval-1".to_string(),
            tool_name: "shell".to_string(),
            payload: serde_json::json!({"command":"echo hello"}),
        }),
        RuntimeEvent::ApprovalDecision(ApprovalDecisionEvent {
            request_id: "approval-1".to_string(),
            approved: true,
            reason: Some("test".to_string()),
        }),
        RuntimeEvent::Error(ErrorEvent {
            message: "boom".to_string(),
            code: Some(1),
            data: None,
        }),
    ];

    for (index, event) in events.iter().enumerate() {
        let seq = store.append_event(&execution_id, event).unwrap();
        assert_eq!(seq, index as i64 + 1);
    }

    let stored = store.events_since(&execution_id, 0).unwrap();
    assert_eq!(stored.len(), events.len());
    assert_eq!(
        stored.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert!(matches!(stored[0].1, RuntimeEvent::Output(_)));
    assert!(matches!(stored[1].1, RuntimeEvent::ToolCall(_)));
    assert!(matches!(stored[2].1, RuntimeEvent::ApprovalRequest(_)));
    assert!(matches!(stored[3].1, RuntimeEvent::ApprovalDecision(_)));
    assert!(matches!(stored[4].1, RuntimeEvent::Error(_)));
    assert_eq!(
        store.output_text(&execution_id).unwrap().as_deref(),
        Some("hello")
    );
}

#[test]
fn persists_execution_timing_and_summarizes() {
    let store = EventStore::open(Path::new(":memory:")).unwrap();
    let execution_id = store
        .begin_execution_with_id("codex", "session", "hash-a", Some("exec-timing"))
        .unwrap();
    store
        .record_timing(
            &execution_id,
            &AcpPromptTiming {
                client_started: true,
                process_spawned: true,
                process_spawn_ms: Some(10),
                init_ms: Some(20),
                session_reused: false,
                session_new_ms: Some(30),
                prompt_ms: 40,
                total_ms: 100,
            },
        )
        .unwrap();
    store
        .finish_execution(&execution_id, ExecutionStatus::Completed)
        .unwrap();

    let record = store.get_execution(&execution_id).unwrap().unwrap();
    assert_eq!(record.process_spawn_ms, Some(10));
    assert_eq!(record.init_ms, Some(20));
    assert_eq!(record.session_new_ms, Some(30));
    assert_eq!(record.prompt_ms, Some(40));
    assert_eq!(record.total_ms, Some(100));

    let summary = store.observability_summary(5).unwrap();
    assert_eq!(summary.total_executions, 1);
    assert_eq!(summary.completed_executions, 1);
    assert_eq!(summary.avg_total_ms, Some(100.0));
    assert_eq!(summary.p95_total_ms, Some(100));
    assert_eq!(summary.latest.len(), 1);
}

#[test]
fn execution_status_preserves_unknown_store_values() {
    let status = ExecutionStatus::from("legacy-state");
    assert_eq!(status, ExecutionStatus::Unknown("legacy-state".to_string()));
    assert_eq!(status.as_str(), "legacy-state");
}
