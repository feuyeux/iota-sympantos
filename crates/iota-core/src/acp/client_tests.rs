use crate::acp::client::agent_capabilities_from_initialize;
use crate::acp::types::AcpAgentCapabilities;
use serde_json::json;

#[test]
fn parses_load_and_resume_capabilities_independently() {
    let load = agent_capabilities_from_initialize(&json!({
        "agentCapabilities": { "loadSession": true }
    }));
    assert!(load.load_session);
    assert!(!load.resume_session);

    let resume = agent_capabilities_from_initialize(&json!({
        "agentCapabilities": { "sessionCapabilities": { "resume": {} } }
    }));
    assert!(!resume.load_session);
    assert!(resume.resume_session);
}

#[test]
fn missing_restore_capabilities_fail_closed() {
    assert_eq!(
        agent_capabilities_from_initialize(&json!({"agentCapabilities": {}})),
        AcpAgentCapabilities::default()
    );
}
