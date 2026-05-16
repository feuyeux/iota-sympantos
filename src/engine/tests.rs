use super::*;
use crate::config::{ContextEngineConfig, NimiaConfig};
use crate::memory::{MemoryFacet, MemoryRecord, MemoryScope, MemoryType};
use crate::store::cache::request_hash;

fn unique_test_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("iota-{}-{}.sqlite", name, uuid::Uuid::new_v4()))
}

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

#[tokio::test]
async fn run_returns_cache_begin_conflict_instead_of_continuing_without_execution_id() {
    let memory_path = unique_test_path("engine-memory");
    let cache_path = unique_test_path("engine-cache");
    let cwd = std::env::current_dir().unwrap();
    let prompt = "my name is cache conflict";
    let execution_id = "fixed-execution-id";
    let cache = CacheStore::open(&cache_path).unwrap();
    cache
        .begin_execution_with_id(
            "codex",
            "session",
            "different-request-hash",
            Some(execution_id),
        )
        .unwrap();

    let config = NimiaConfig {
        context_engine: Some(ContextEngineConfig {
            memory_db: Some(memory_path.display().to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut engine = IotaEngine::create_session(config, false, 1_000, Some(&cwd));
    engine.cache_store = Some(cache);
    engine.memory_store = Some(MemoryStore::open(&memory_path).unwrap());

    let err = match engine
        .run(AcpBackend::Codex, cwd.clone(), prompt, Some(execution_id))
        .await
    {
        Ok(_) => panic!("cache begin conflict must stop the turn"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("execution_id conflict"));
    assert_eq!(request_hash("codex", &cwd, prompt).len(), 64);
}
