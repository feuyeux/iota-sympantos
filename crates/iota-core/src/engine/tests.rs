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

#[test]
fn test_resume_session_restores_backend_and_working_memory() {
    let ledger_path = unique_test_path("engine-ledger");
    let memory_path = unique_test_path("engine-memory");
    let cwd = std::env::current_dir().unwrap();
    let session_id = "test-session-123";

    // Initialize stores
    let ledger = SessionLedger::open(&ledger_path).unwrap();
    let memory = MemoryStore::open(&memory_path).unwrap();

    // 1. Record session and active backend in ledger
    ledger
        .ensure_session(session_id, &cwd, Some("claude-code"), None)
        .unwrap();

    // 2. Insert a turn into memory store as episodic memory
    let turn_content = "Prompt: write a rust script\nOutput: fn main() {}";
    memory
        .insert(crate::memory::MemoryInsert {
            memory_type: MemoryType::Episodic,
            facet: None,
            scope: MemoryScope::Session,
            scope_id: session_id.to_string(),
            content: turn_content.to_string(),
            confidence: 0.8,
            source_backend: Some("claude-code".to_string()),
            source_session_id: Some(session_id.to_string()),
            source_execution_id: None,
            metadata_json: None,
            ttl_days: 7,
            supersedes: None,
        })
        .unwrap();

    // Verify latest session query works
    let latest = ledger.latest_session_for_cwd(&cwd).unwrap().unwrap();
    assert_eq!(latest, session_id);

    // Create a config
    let config = NimiaConfig::default();

    // Instantiate engine without session_cwd
    let mut engine = IotaEngine::create_session(config, false, 1_000, None);

    // Inject our in-memory databases
    engine.session_ledger_store = Some(ledger);
    engine.memory_store = Some(memory);

    // Assert that initially engine has no session state
    assert!(engine.last_used_backend.is_none());
    assert_eq!(engine.working_memory.render(800), "");

    // Now resume session state with cwd
    engine.resume_session_state(Some(&cwd));

    // Verify state was correctly restored
    assert_eq!(engine.engine_session_id, session_id);
    assert_eq!(engine.last_used_backend, Some(AcpBackend::ClaudeCode));

    // Reconstruct the expected rendered working memory summary
    let wm_summary = engine.working_memory.render(800);
    assert!(
        wm_summary.contains("[claude-code] user: write a rust script; assistant: fn main() {}")
    );
}
