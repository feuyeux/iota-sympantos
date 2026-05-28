//! Unit tests for the storage layer.
//!
//! Tests retry logic, model conversions, and artifact construction.
//! Written to the unit-test规范: independent *_tests.rs files.

use crate::storage::{
    models::{
        PipelineArtifact, PipelineRecord, PipelineStatus, ResearchData, ScriptData, XOptimizerData,
    },
    retry::with_backoff,
};
use std::time::Duration;

#[test]
fn with_backoff_succeeds_on_first_attempt() {
    let result = with_backoff(|| Ok(42u32), 3, Duration::from_secs(1));
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn with_backoff_retries_then_succeeds() {
    let mut attempts = 0;
    let result = with_backoff(
        || {
            attempts += 1;
            if attempts < 3 {
                Err(anyhow::anyhow!(" transient"))
            } else {
                Ok(99)
            }
        },
        5,
        Duration::from_millis(10),
    );
    assert_eq!(result.unwrap(), 99);
    assert_eq!(attempts, 3);
}

#[test]
fn with_backoff_exhausts_and_returns_last_error() {
    let result: Result<u32, _> = with_backoff(
        || Err(anyhow::anyhow!("permanent")),
        3,
        Duration::from_millis(10),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "permanent");
}

#[test]
fn pipeline_artifact_research_into_record() {
    let data = ResearchData {
        topic_source: "test-source".into(),
        heat_score: 72,
        topic: "Rust async".into(),
        metadata: serde_json::json!({}),
    };
    let artifact = PipelineArtifact::research(data);
    let record = artifact.into_record();
    assert_eq!(record.stage, "research");
    assert_eq!(record.status, PipelineStatus::Pending);
    assert!(record.id != uuid::Uuid::nil());
}

#[test]
fn pipeline_artifact_script_into_record() {
    let data = ScriptData {
        title: "Test Script".into(),
        voiceover_script: serde_json::json!("vo text"),
        sections: serde_json::json!([]),
        format: "video".into(),
        output_path: "/tmp/test.md".into(),
        metadata: serde_json::json!({}),
    };
    let artifact = PipelineArtifact::script(data);
    let record = artifact.into_record();
    assert_eq!(record.stage, "script");
}

#[test]
fn pipeline_artifact_x_optimizer_into_record() {
    let data = XOptimizerData {
        post_text: "Hello world #rust".into(),
        hashtags: vec!["rust".into(), "ai".into()],
        media_urls: vec![],
        metadata: serde_json::json!({}),
    };
    let artifact = PipelineArtifact::x_optimizer(data);
    let record = artifact.into_record();
    assert_eq!(record.stage, "x_optimizer");
}

#[test]
fn pipeline_record_new_has_created_and_updated_at() {
    let record = PipelineRecord::new("test", serde_json::json!({}));
    assert_eq!(record.stage, "test");
    assert_eq!(record.status, PipelineStatus::Pending);
    // created_at and updated_at should be within a few seconds of each other
    let diff = (record.updated_at - record.created_at).num_seconds().abs();
    assert!(diff < 5);
}