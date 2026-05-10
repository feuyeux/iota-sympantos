use super::*;
use rusqlite::OptionalExtension;
use serde_json::json;

#[test]
fn records_request_with_execution_id_before_decision() {
    let store = ApprovalStore::open(Path::new(":memory:")).unwrap();
    let request_id = store
        .record_request(
            Some("exec-1"),
            "codex",
            "shell",
            &json!({"command":"echo hi"}),
        )
        .unwrap();
    store
        .record_decision(&request_id, true, "test decision")
        .unwrap();

    let conn = crate::utils::lock_sqlite_conn(&store.conn);
    let execution_id = conn
        .query_row(
            "SELECT execution_id FROM approval_requests WHERE request_id = ?1",
            params![request_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .unwrap()
        .flatten();
    assert_eq!(execution_id.as_deref(), Some("exec-1"));

    let approved = conn
        .query_row(
            "SELECT approved FROM approval_decisions WHERE request_id = ?1",
            params![request_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(approved, 1);
}

#[test]
fn classify_shell_operation() {
    let dims = classify_operation("bash_exec", &json!({}));
    assert!(dims.contains(&ApprovalDimension::Shell));
}

#[test]
fn classify_network_operation_from_url_in_payload() {
    let dims = classify_operation("file_write", &json!({"url": "https://example.com/data"}));
    assert!(dims.contains(&ApprovalDimension::Network));
}

#[test]
fn classify_privilege_escalation_from_sudo() {
    let dims = classify_operation("run_command", &json!({"command": "sudo apt-get update"}));
    assert!(dims.contains(&ApprovalDimension::PrivilegeEscalation));
}

#[test]
fn default_decision_requires_manual_approval() {
    let dims = vec![ApprovalDimension::Shell];
    let decision = default_decision(&dims);
    assert!(!decision.approved);

    let empty_dims: Vec<ApprovalDimension> = vec![];
    let empty_decision = default_decision(&empty_dims);
    assert!(!empty_decision.approved);
    assert!(empty_decision.reason.contains("manual"));
}
