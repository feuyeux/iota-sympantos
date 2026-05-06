use super::*;
use crate::store::memory::{MemoryFacet, MemoryRecord, MemoryScope, MemoryType};

#[test]
fn memory_inject_payload_uses_configured_budget() {
    let buckets = RecallBuckets {
        identity: vec![memory_record("one")],
        preference: vec![memory_record("two")],
        ..Default::default()
    };
    let payload = memory_inject_payload(&buckets, 5);
    let budget = payload.get("budget").unwrap();

    assert_eq!(budget.get("memory_chars").and_then(|v| v.as_u64()), Some(5));
    assert_eq!(budget.get("total_chars").and_then(|v| v.as_u64()), Some(6));
    assert_eq!(
        budget.get("truncated").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        budget.get("excluded_count").and_then(|v| v.as_u64()),
        Some(1)
    );
}

fn memory_record(content: &str) -> MemoryRecord {
    MemoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        memory_type: MemoryType::Semantic,
        facet: Some(MemoryFacet::Identity),
        scope: MemoryScope::User,
        scope_id: "local-user".to_string(),
        content: content.to_string(),
        confidence: 1.0,
        created_at: 1,
        updated_at: 1,
        expires_at: 999,
    }
}

#[test]
fn memory_inject_payload_within_budget_no_truncation() {
    let record = memory_record("short");
    let buckets = RecallBuckets {
        identity: vec![record],
        ..Default::default()
    };
    let payload = memory_inject_payload(&buckets, 10_000);
    let budget = payload.get("budget").unwrap();
    assert_eq!(
        budget.get("truncated").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        budget.get("excluded_count").and_then(|v| v.as_u64()),
        Some(0)
    );
}

#[test]
fn memory_inject_payload_empty_buckets_returns_zero_total() {
    let buckets = RecallBuckets::default();
    let payload = memory_inject_payload(&buckets, 1000);
    let budget = payload.get("budget").unwrap();
    assert_eq!(budget.get("total_chars").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(
        budget.get("truncated").and_then(|v| v.as_bool()),
        Some(false)
    );
}
