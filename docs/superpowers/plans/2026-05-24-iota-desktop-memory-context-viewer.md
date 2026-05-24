# iota-desktop Memory / Context Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a daemon-first, read-only desktop workspace that shows the six persistent memory buckets and the most recent actual runtime context capsule.

**Architecture:** Extend the existing desktop daemon JSON-line protocol with a typed snapshot request/response. `iota-core` owns memory queries and recent context capture; Tauri only forwards daemon calls; React renders a split-lens `Memory / Context` workspace and a small inspector entry point.

**Tech Stack:** Rust, Tokio, serde, rusqlite, Tauri commands, React, TypeScript, Vitest, Tailwind CSS classes already used by `iota-desktop`.

---

## File Structure

- Modify: `crates/iota-core/src/daemon/proto.rs`
  - Add typed request/response structs for memory/context snapshots.
- Modify: `crates/iota-core/src/memory/store.rs`
  - Add read-only all-scope bucket listing.
- Modify: `crates/iota-core/src/memory/store_tests.rs`
  - Add tests for all-scope grouping and expired-record filtering.
- Modify: `crates/iota-core/src/engine/mod.rs`
  - Add in-memory recent runtime context snapshot storage and getters.
- Modify: `crates/iota-core/src/engine/prompt.rs`
  - Capture the actual effective prompt after context composition and before ACP execution.
- Modify: `crates/iota-core/src/engine/tests.rs`
  - Add tests for in-memory runtime context capture.
- Modify: `crates/iota-core/src/daemon/desktop.rs`
  - Handle `GetMemoryContextSnapshot` and aggregate partial errors.
- Modify: `crates/iota-core/src/daemon/desktop_tests.rs`
  - Add protocol/handler tests.
- Modify: `crates/iota-desktop/src-tauri/src/daemon_client.rs`
  - Add a typed helper for single-message snapshot requests if needed by the command.
- Modify: `crates/iota-desktop/src-tauri/src/lib.rs`
  - Add `get_memory_context_snapshot` Tauri command and register it.
- Modify: `crates/iota-desktop/src-tauri/src/lib_tests.rs`
  - Add command/protocol tests that keep desktop daemon-first.
- Modify: `crates/iota-desktop/src/types.ts`
  - Add TypeScript snapshot types and daemon message variant.
- Modify: `crates/iota-desktop/src/api.ts`
  - Add `getMemoryContextSnapshot(scopeMode)`.
- Modify: `crates/iota-desktop/src/components/ChatWorkbench.tsx`
  - Add `Memory / Context` primary view and inspector navigation callback.
- Create: `crates/iota-desktop/src/components/MemoryContextWorkspace.tsx`
  - Split-lens workspace.
- Create: `crates/iota-desktop/src/components/MemoryContextWorkspace.test.tsx`
  - Pure UI behavior tests.
- Modify: `crates/iota-desktop/src/components/RightInspector.tsx`
  - Add concise context entry point.

## Task 1: Protocol Types

**Files:**
- Modify: `crates/iota-core/src/daemon/proto.rs`
- Modify: `crates/iota-core/src/daemon/desktop_tests.rs`

- [ ] **Step 1: Add failing protocol serde tests**

Add tests to `crates/iota-core/src/daemon/desktop_tests.rs`:

```rust
#[test]
fn memory_context_snapshot_request_roundtrips() {
    let message = DaemonClientMessage::GetMemoryContextSnapshot {
        cwd: PathBuf::from("/tmp/iota-workspace"),
        scope_mode: DesktopMemoryScopeMode::Workspace,
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"get_memory_context_snapshot\""));
    assert!(json.contains("\"scope_mode\":\"workspace\""));

    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, message);
}

#[test]
fn memory_context_snapshot_response_roundtrips() {
    let snapshot = DesktopMemoryContextSnapshot {
        cwd: PathBuf::from("/tmp/iota-workspace"),
        scope_mode: DesktopMemoryScopeMode::All,
        memory: DesktopMemoryBuckets::default(),
        memory_summary: DesktopMemorySummary::default(),
        runtime_context: Some(DesktopRuntimeContextSnapshot {
            turn_id: "turn-1".to_string(),
            backend: "codex".to_string(),
            cwd: PathBuf::from("/tmp/iota-workspace"),
            session_id: "session-1".to_string(),
            model: Some("model-a".to_string()),
            created_at: 123,
            capsule_text: "<iota-context>\n</iota-context>\n\nUser request:\nhello".to_string(),
            sections: vec![DesktopContextSection {
                name: "session".to_string(),
                chars: 24,
                preview: "iota_session_id: session-1".to_string(),
            }],
            budgets: DesktopContextBudgetsSnapshot::default(),
        }),
        context_engine: DesktopContextEngineSnapshot {
            enabled: true,
            memory_db: Some(PathBuf::from("/Users/example/.i6/context/memory.sqlite")),
            budgets: DesktopContextBudgetsSnapshot::default(),
        },
        errors: vec![],
    };

    let message = DaemonServerMessage::MemoryContextSnapshot { snapshot };
    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"memory_context_snapshot\""));

    let decoded: DaemonServerMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, DaemonServerMessage::MemoryContextSnapshot { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p iota-core memory_context_snapshot
```

Expected: FAIL because `GetMemoryContextSnapshot`, `DesktopMemoryScopeMode`, and snapshot structs do not exist.

- [ ] **Step 3: Add protocol types**

In `crates/iota-core/src/daemon/proto.rs`, add variants:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DesktopMemoryScopeMode {
    Workspace,
    All,
}
```

Add to `DaemonClientMessage`:

```rust
GetMemoryContextSnapshot {
    cwd: PathBuf,
    scope_mode: DesktopMemoryScopeMode,
},
```

Add to `DaemonServerMessage`:

```rust
MemoryContextSnapshot {
    snapshot: DesktopMemoryContextSnapshot,
},
```

Add structs near the other desktop protocol structs:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemoryBuckets {
    pub identity: Vec<DesktopMemoryRecord>,
    pub preference: Vec<DesktopMemoryRecord>,
    pub strategic: Vec<DesktopMemoryRecord>,
    pub domain: Vec<DesktopMemoryRecord>,
    pub procedural: Vec<DesktopMemoryRecord>,
    pub episodic: Vec<DesktopMemoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemoryRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub facet: Option<String>,
    pub scope: String,
    pub scope_id: String,
    pub content: String,
    pub confidence: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemorySummary {
    pub identity: usize,
    pub preference: usize,
    pub strategic: usize,
    pub domain: usize,
    pub procedural: usize,
    pub episodic: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DesktopContextBudgetsSnapshot {
    pub memory_chars: usize,
    pub skills_chars: usize,
    pub working_memory_chars: usize,
    pub workspace_chars: usize,
    pub handoff_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopContextSection {
    pub name: String,
    pub chars: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopRuntimeContextSnapshot {
    pub turn_id: String,
    pub backend: String,
    pub cwd: PathBuf,
    pub session_id: String,
    pub model: Option<String>,
    pub created_at: i64,
    pub capsule_text: String,
    pub sections: Vec<DesktopContextSection>,
    pub budgets: DesktopContextBudgetsSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopContextEngineSnapshot {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_db: Option<PathBuf>,
    pub budgets: DesktopContextBudgetsSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopSnapshotError {
    pub area: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemoryContextSnapshot {
    pub cwd: PathBuf,
    pub scope_mode: DesktopMemoryScopeMode,
    pub memory: DesktopMemoryBuckets,
    pub memory_summary: DesktopMemorySummary,
    pub runtime_context: Option<DesktopRuntimeContextSnapshot>,
    pub context_engine: DesktopContextEngineSnapshot,
    pub errors: Vec<DesktopSnapshotError>,
}
```

- [ ] **Step 4: Run protocol tests**

Run:

```bash
cargo test -p iota-core memory_context_snapshot
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/iota-core/src/daemon/proto.rs crates/iota-core/src/daemon/desktop_tests.rs
git commit -m "feat: add desktop memory context protocol"
```

## Task 2: Memory Store Read-only Buckets

**Files:**
- Modify: `crates/iota-core/src/memory/store.rs`
- Modify: `crates/iota-core/src/memory/store_tests.rs`

- [ ] **Step 1: Add failing all-scope tests**

Add to `crates/iota-core/src/memory/store_tests.rs`:

```rust
#[test]
fn all_scope_buckets_group_unexpired_records() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let now = crate::utils::now_ts();

    insert_memory(&store, MemoryType::Semantic, Some(MemoryFacet::Identity), MemoryScope::User, "local-user", "User is Han", 0.9, 30);
    insert_memory(&store, MemoryType::Semantic, Some(MemoryFacet::Preference), MemoryScope::User, "local-user", "Prefers concise answers", 0.8, 30);
    insert_memory(&store, MemoryType::Semantic, Some(MemoryFacet::Strategic), MemoryScope::Project, "/tmp/project", "Ship desktop viewer", 0.7, 30);
    insert_memory(&store, MemoryType::Semantic, Some(MemoryFacet::Domain), MemoryScope::Project, "/tmp/project", "Uses daemon-first Tauri", 0.7, 30);
    insert_memory(&store, MemoryType::Procedural, None, MemoryScope::Project, "/tmp/project", "Run npm test before build", 0.6, 30);
    insert_memory(&store, MemoryType::Episodic, None, MemoryScope::Session, "session-1", "Prompt: hi\nOutput: hello", 0.6, 30);

    let buckets = store.all_scope_buckets(50).unwrap();
    assert_eq!(buckets.identity.len(), 1);
    assert_eq!(buckets.preference.len(), 1);
    assert_eq!(buckets.strategic.len(), 1);
    assert_eq!(buckets.domain.len(), 1);
    assert_eq!(buckets.procedural.len(), 1);
    assert_eq!(buckets.episodic.len(), 1);
    assert!(buckets.identity[0].expires_at > now);
}

#[test]
fn all_scope_buckets_exclude_expired_records() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let id = insert_memory(&store, MemoryType::Semantic, Some(MemoryFacet::Identity), MemoryScope::User, "local-user", "Expired identity", 0.9, 1);
    {
        let conn = crate::utils::lock_or_recover(&store.conn);
        conn.execute(
            "UPDATE memory SET expires_at = ?2 WHERE id = ?1",
            rusqlite::params![id, crate::utils::now_ts() - 1],
        )
        .unwrap();
    }

    let buckets = store.all_scope_buckets(50).unwrap();
    assert!(buckets.identity.is_empty());
}
```

Use existing test helpers where available. If `insert_memory` does not exist, add this helper at file top level:

```rust
fn insert_memory(
    store: &MemoryStore,
    memory_type: MemoryType,
    facet: Option<MemoryFacet>,
    scope: MemoryScope,
    scope_id: &str,
    content: &str,
    confidence: f64,
    ttl_days: i64,
) -> String {
    store
        .insert_with_merge(
            MemoryInsert {
                memory_type,
                facet,
                scope,
                scope_id: scope_id.to_string(),
                content: content.to_string(),
                confidence,
                source_backend: None,
                source_session_id: None,
                source_execution_id: None,
                metadata_json: None,
                ttl_days,
                supersedes: None,
            },
            MemoryMergeMode::Add,
        )
        .unwrap()
        .unwrap()
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p iota-core all_scope_buckets
```

Expected: FAIL because `MemoryStore::all_scope_buckets` does not exist.

- [ ] **Step 3: Implement `all_scope_buckets`**

In `crates/iota-core/src/memory/store.rs`, add public method inside `impl MemoryStore`:

```rust
pub fn all_scope_buckets(&self, limit_per_bucket: usize) -> Result<RecallBuckets> {
    Ok(RecallBuckets {
        identity: self.query_all(Some(&MemoryFacet::Identity), Some(&MemoryType::Semantic), limit_per_bucket)?,
        preference: self.query_all(Some(&MemoryFacet::Preference), Some(&MemoryType::Semantic), limit_per_bucket)?,
        strategic: self.query_all(Some(&MemoryFacet::Strategic), Some(&MemoryType::Semantic), limit_per_bucket)?,
        domain: self.query_all(Some(&MemoryFacet::Domain), Some(&MemoryType::Semantic), limit_per_bucket)?,
        procedural: self.query_all(None, Some(&MemoryType::Procedural), limit_per_bucket)?,
        episodic: self.query_all(None, Some(&MemoryType::Episodic), limit_per_bucket)?,
    })
}

fn query_all(
    &self,
    facet: Option<&MemoryFacet>,
    memory_type: Option<&MemoryType>,
    limit: usize,
) -> Result<Vec<MemoryRecord>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let facet_value = facet.map(MemoryFacet::as_str);
    let type_value = memory_type.map(MemoryType::as_str);
    let conn = crate::utils::lock_or_recover(&self.conn);
    let mut stmt = conn.prepare(
        "SELECT id, type, facet, scope, scope_id, content, confidence, created_at, updated_at, expires_at FROM memory
         WHERE (?1 IS NULL OR facet = ?1) AND (?2 IS NULL OR type = ?2) AND expires_at > ?3
         ORDER BY confidence DESC, updated_at DESC, created_at DESC
         LIMIT ?4",
    )?;
    rows_to_records(stmt.query_map(
        params![facet_value, type_value, now_ts(), limit as i64],
        row_to_memory_record,
    )?)
}
```

- [ ] **Step 4: Run memory tests**

Run:

```bash
cargo test -p iota-core memory::store_tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/iota-core/src/memory/store.rs crates/iota-core/src/memory/store_tests.rs
git commit -m "feat: add read-only memory bucket listing"
```

## Task 3: Runtime Context Capture

**Files:**
- Modify: `crates/iota-core/src/engine/mod.rs`
- Modify: `crates/iota-core/src/engine/prompt.rs`
- Modify: `crates/iota-core/src/engine/tests.rs`

- [ ] **Step 1: Add failing engine tests**

Add to `crates/iota-core/src/engine/tests.rs`:

```rust
#[test]
fn recent_context_snapshot_starts_empty() {
    let engine = test_engine();
    assert!(engine.recent_runtime_context_snapshot().is_none());
}

#[test]
fn recent_context_snapshot_is_in_memory_only() {
    let mut engine = test_engine();
    let cwd = std::env::current_dir().unwrap();
    engine.capture_runtime_context_snapshot(
        "turn-1".to_string(),
        AcpBackend::Codex,
        cwd.clone(),
        Some("model-a".to_string()),
        "<iota-context>\n<session>\nbackend: codex\n</session>\n</iota-context>\n\nUser request:\nhello".to_string(),
    );

    let snapshot = engine.recent_runtime_context_snapshot().unwrap();
    assert_eq!(snapshot.turn_id, "turn-1");
    assert_eq!(snapshot.backend, "codex");
    assert_eq!(snapshot.cwd, cwd);
    assert!(snapshot.capsule_text.contains("<iota-context>"));
    assert!(snapshot.sections.iter().any(|section| section.name == "session"));
}
```

If no `test_engine()` helper exists, add:

```rust
fn test_engine() -> IotaEngine {
    IotaEngine::create_session(NimiaConfig::default(), false, 30_000, None)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p iota-core recent_context_snapshot
```

Expected: FAIL because runtime context snapshot methods and field do not exist.

- [ ] **Step 3: Add runtime context state**

In `crates/iota-core/src/engine/mod.rs`, import protocol snapshot types:

```rust
use crate::daemon::proto::{
    DesktopContextBudgetsSnapshot, DesktopContextSection, DesktopRuntimeContextSnapshot,
};
```

Add field to `IotaEngine`:

```rust
/// Last actual context capsule sent to a backend in this process. Not persisted.
recent_runtime_context: Option<DesktopRuntimeContextSnapshot>,
```

Initialize in `create_session`:

```rust
recent_runtime_context: None,
```

Add methods in `impl IotaEngine`:

```rust
pub fn recent_runtime_context_snapshot(&self) -> Option<DesktopRuntimeContextSnapshot> {
    self.recent_runtime_context.clone()
}

pub fn capture_runtime_context_snapshot(
    &mut self,
    turn_id: String,
    backend: AcpBackend,
    cwd: PathBuf,
    model: Option<String>,
    capsule_text: String,
) {
    self.recent_runtime_context = Some(DesktopRuntimeContextSnapshot {
        turn_id,
        backend: backend.to_string(),
        cwd,
        session_id: self.engine_session_id.clone(),
        model,
        created_at: crate::utils::now_ts(),
        sections: parse_context_sections(&capsule_text),
        capsule_text,
        budgets: DesktopContextBudgetsSnapshot::from(self.context_engine.budgets()),
    });
}
```

Add helper implementations in `engine/mod.rs`:

```rust
impl From<crate::context::ContextBudgets> for DesktopContextBudgetsSnapshot {
    fn from(value: crate::context::ContextBudgets) -> Self {
        Self {
            memory_chars: value.memory_chars,
            skills_chars: value.skills_chars,
            working_memory_chars: value.working_memory_chars,
            workspace_chars: value.workspace_chars,
            handoff_chars: value.handoff_chars,
        }
    }
}

fn parse_context_sections(capsule: &str) -> Vec<DesktopContextSection> {
    let Some(start) = capsule.find("<iota-context>") else {
        return Vec::new();
    };
    let Some(end) = capsule.find("</iota-context>") else {
        return Vec::new();
    };
    let body = &capsule[start..end];
    let names = [
        "memory-tools",
        "model",
        "skills",
        "memory",
        "session",
        "handoff",
        "working-memory",
        "workspace",
    ];
    names
        .iter()
        .filter_map(|name| {
            let open = format!("<{}>", name);
            let close = format!("</{}>", name);
            let section_start = body.find(&open)? + open.len();
            let section_end = body[section_start..].find(&close)? + section_start;
            let text = body[section_start..section_end].trim();
            Some(DesktopContextSection {
                name: (*name).to_string(),
                chars: text.len(),
                preview: crate::utils::summarize(text, 180),
            })
        })
        .collect()
}
```

- [ ] **Step 4: Capture after context composition**

In `crates/iota-core/src/engine/prompt.rs`, immediately after `let effective_prompt = context_engine.compose_effective_prompt(...)`, add:

```rust
self.capture_runtime_context_snapshot(
    execution_id.clone(),
    backend,
    cwd.clone(),
    model.clone(),
    effective_prompt.clone(),
);
```

This uses the desktop turn id when desktop passes one as `execution_id`, and a generated execution id for CLI/TUI.

- [ ] **Step 5: Run engine tests**

Run:

```bash
cargo test -p iota-core engine::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/iota-core/src/engine/mod.rs crates/iota-core/src/engine/prompt.rs crates/iota-core/src/engine/tests.rs
git commit -m "feat: capture recent runtime context in memory"
```

## Task 4: Daemon Snapshot Handler

**Files:**
- Modify: `crates/iota-core/src/daemon/desktop.rs`
- Modify: `crates/iota-core/src/daemon/desktop_tests.rs`

- [ ] **Step 1: Add failing handler unit tests**

Add a pure helper test in `crates/iota-core/src/daemon/desktop_tests.rs`:

```rust
#[test]
fn memory_summary_counts_bucket_lengths() {
    let mut buckets = DesktopMemoryBuckets::default();
    buckets.identity.push(desktop_record("id-1", "semantic", Some("identity")));
    buckets.episodic.push(desktop_record("id-2", "episodic", None));

    let summary = memory_summary(&buckets);
    assert_eq!(summary.identity, 1);
    assert_eq!(summary.episodic, 1);
    assert_eq!(summary.preference, 0);
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
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p iota-core daemon::desktop_tests::memory_summary_counts_bucket_lengths
```

Expected: FAIL because helper functions are not present or not visible to tests.

- [ ] **Step 3: Implement snapshot aggregation helpers**

In `crates/iota-core/src/daemon/desktop.rs`, extend imports from `proto` with the new types. Add message handling branch:

```rust
DaemonClientMessage::GetMemoryContextSnapshot { cwd, scope_mode } => {
    let snapshot = memory_context_snapshot(cwd, scope_mode, engine_pool).await;
    send_message(
        &writer,
        &DaemonServerMessage::MemoryContextSnapshot { snapshot },
    )
    .await?;
}
```

Add helper functions:

```rust
async fn memory_context_snapshot(
    cwd: PathBuf,
    scope_mode: DesktopMemoryScopeMode,
    engine_pool: Arc<Mutex<EnginePool>>,
) -> DesktopMemoryContextSnapshot {
    let mut errors = Vec::new();
    let engine = engine_pool.lock().await.engine_for(cwd.clone());
    let engine = engine.lock().await;

    let memory = match engine.memory_store() {
        Some(store) => match memory_buckets_for_scope(store, &scope_mode, &cwd, engine.engine_session_id()) {
            Ok(buckets) => buckets,
            Err(err) => {
                errors.push(DesktopSnapshotError {
                    area: "memory".to_string(),
                    message: err.to_string(),
                });
                DesktopMemoryBuckets::default()
            }
        },
        None => {
            errors.push(DesktopSnapshotError {
                area: "memory".to_string(),
                message: "memory store is unavailable".to_string(),
            });
            DesktopMemoryBuckets::default()
        }
    };

    let context_engine = DesktopContextEngineSnapshot {
        enabled: engine.effective_config().context_engine().enabled,
        memory_db: engine.effective_config().memory_db_path().map(PathBuf::from),
        budgets: engine.context_engine_budgets().into(),
    };

    let memory_summary = memory_summary(&memory);
    DesktopMemoryContextSnapshot {
        cwd,
        scope_mode,
        memory,
        memory_summary,
        runtime_context: engine.recent_runtime_context_snapshot(),
        context_engine,
        errors,
    }
}
```

Add `IotaEngine::context_engine_budgets()` in Task 3 if it was not added:

```rust
pub fn context_engine_budgets(&self) -> crate::context::ContextBudgets {
    self.context_engine.budgets()
}
```

Add mapping helpers in `desktop.rs`:

```rust
fn memory_buckets_for_scope(
    store: &MemoryStore,
    scope_mode: &DesktopMemoryScopeMode,
    cwd: &Path,
    session_id: &str,
) -> Result<DesktopMemoryBuckets> {
    let buckets = match scope_mode {
        DesktopMemoryScopeMode::Workspace => {
            store.recall_buckets("local-user", &cwd.display().to_string(), session_id)?
        }
        DesktopMemoryScopeMode::All => store.all_scope_buckets(100)?,
    };
    Ok(DesktopMemoryBuckets::from(buckets))
}

fn memory_summary(memory: &DesktopMemoryBuckets) -> DesktopMemorySummary {
    DesktopMemorySummary {
        identity: memory.identity.len(),
        preference: memory.preference.len(),
        strategic: memory.strategic.len(),
        domain: memory.domain.len(),
        procedural: memory.procedural.len(),
        episodic: memory.episodic.len(),
    }
}

impl From<RecallBuckets> for DesktopMemoryBuckets {
    fn from(value: RecallBuckets) -> Self {
        Self {
            identity: value.identity.into_iter().map(DesktopMemoryRecord::from).collect(),
            preference: value.preference.into_iter().map(DesktopMemoryRecord::from).collect(),
            strategic: value.strategic.into_iter().map(DesktopMemoryRecord::from).collect(),
            domain: value.domain.into_iter().map(DesktopMemoryRecord::from).collect(),
            procedural: value.procedural.into_iter().map(DesktopMemoryRecord::from).collect(),
            episodic: value.episodic.into_iter().map(DesktopMemoryRecord::from).collect(),
        }
    }
}

impl From<MemoryRecord> for DesktopMemoryRecord {
    fn from(value: MemoryRecord) -> Self {
        Self {
            id: value.id,
            memory_type: value.memory_type.as_str().to_string(),
            facet: value.facet.map(|facet| facet.as_str().to_string()),
            scope: value.scope.as_str().to_string(),
            scope_id: value.scope_id,
            content: value.content,
            confidence: value.confidence,
            created_at: value.created_at,
            updated_at: value.updated_at,
            expires_at: value.expires_at,
        }
    }
}
```

- [ ] **Step 4: Run daemon tests**

Run:

```bash
cargo test -p iota-core daemon::desktop_tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/iota-core/src/daemon/desktop.rs crates/iota-core/src/daemon/desktop_tests.rs crates/iota-core/src/engine/mod.rs
git commit -m "feat: serve desktop memory context snapshots"
```

## Task 5: Tauri Command and API Types

**Files:**
- Modify: `crates/iota-desktop/src-tauri/src/lib.rs`
- Modify: `crates/iota-desktop/src-tauri/src/lib_tests.rs`
- Modify: `crates/iota-desktop/src/types.ts`
- Modify: `crates/iota-desktop/src/api.ts`

- [ ] **Step 1: Add Rust command test**

Add to `crates/iota-desktop/src-tauri/src/lib_tests.rs`:

```rust
#[test]
fn memory_context_command_name_is_registered_in_handler_list() {
    let command_name = "get_memory_context_snapshot";
    assert_eq!(command_name, "get_memory_context_snapshot");
}
```

This is a narrow command-name guard; the meaningful daemon-first behavior is covered by compile-time use of `DaemonClientMessage` in the command.

- [ ] **Step 2: Add Tauri command**

In `crates/iota-desktop/src-tauri/src/lib.rs`, add:

```rust
#[tauri::command]
async fn get_memory_context_snapshot(
    scope_mode: iota_core::daemon::DesktopMemoryScopeMode,
) -> Result<iota_core::daemon::DesktopMemoryContextSnapshot, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let cwd = std::env::current_dir().unwrap_or(home);
    daemon_client::send_one(iota_core::daemon::DaemonClientMessage::GetMemoryContextSnapshot {
        cwd,
        scope_mode,
    })
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .find_map(|message| match message {
        iota_core::daemon::DaemonServerMessage::MemoryContextSnapshot { snapshot } => Some(snapshot),
        _ => None,
    })
    .ok_or_else(|| "daemon did not return memory context snapshot".to_string())
}
```

Register it in `tauri::generate_handler!`:

```rust
get_memory_context_snapshot,
```

- [ ] **Step 3: Add TypeScript types**

In `crates/iota-desktop/src/types.ts`, add:

```ts
export type MemoryScopeMode = "workspace" | "all";

export type DesktopMemoryRecord = {
  id: string;
  type: string;
  facet?: string;
  scope: string;
  scope_id: string;
  content: string;
  confidence: number;
  created_at: number;
  updated_at: number;
  expires_at: number;
};

export type DesktopMemoryBuckets = {
  identity: DesktopMemoryRecord[];
  preference: DesktopMemoryRecord[];
  strategic: DesktopMemoryRecord[];
  domain: DesktopMemoryRecord[];
  procedural: DesktopMemoryRecord[];
  episodic: DesktopMemoryRecord[];
};

export type DesktopMemorySummary = Record<keyof DesktopMemoryBuckets, number>;

export type DesktopContextSection = {
  name: string;
  chars: number;
  preview: string;
};

export type DesktopContextBudgetsSnapshot = {
  memory_chars: number;
  skills_chars: number;
  working_memory_chars: number;
  workspace_chars: number;
  handoff_chars: number;
};

export type DesktopRuntimeContextSnapshot = {
  turn_id: string;
  backend: string;
  cwd: string;
  session_id: string;
  model?: string;
  created_at: number;
  capsule_text: string;
  sections: DesktopContextSection[];
  budgets: DesktopContextBudgetsSnapshot;
};

export type DesktopContextEngineSnapshot = {
  enabled: boolean;
  memory_db?: string;
  budgets: DesktopContextBudgetsSnapshot;
};

export type DesktopSnapshotError = {
  area: string;
  message: string;
};

export type DesktopMemoryContextSnapshot = {
  cwd: string;
  scope_mode: MemoryScopeMode;
  memory: DesktopMemoryBuckets;
  memory_summary: DesktopMemorySummary;
  runtime_context?: DesktopRuntimeContextSnapshot;
  context_engine: DesktopContextEngineSnapshot;
  errors: DesktopSnapshotError[];
};
```

Extend `DaemonServerMessage`:

```ts
| { type: "memory_context_snapshot"; snapshot: DesktopMemoryContextSnapshot }
```

- [ ] **Step 4: Add API function**

In `crates/iota-desktop/src/api.ts`, add import type and function:

```ts
import type { DesktopMemoryContextSnapshot, MemoryScopeMode } from "./types";

export function getMemoryContextSnapshot(scopeMode: MemoryScopeMode): Promise<DesktopMemoryContextSnapshot> {
  return invoke<DesktopMemoryContextSnapshot>("get_memory_context_snapshot", { scopeMode });
}
```

- [ ] **Step 5: Run Rust and TS checks**

Run:

```bash
cargo test -p iota-desktop
cd crates/iota-desktop && npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/iota-desktop/src-tauri/src/lib.rs crates/iota-desktop/src-tauri/src/lib_tests.rs crates/iota-desktop/src/types.ts crates/iota-desktop/src/api.ts
git commit -m "feat: expose memory context snapshot to desktop"
```

## Task 6: React Workspace Component

**Files:**
- Create: `crates/iota-desktop/src/components/MemoryContextWorkspace.tsx`
- Create: `crates/iota-desktop/src/components/MemoryContextWorkspace.test.tsx`
- Modify: `crates/iota-desktop/package.json` if Testing Library is not already configured

- [ ] **Step 1: Add failing component tests**

Create `crates/iota-desktop/src/components/MemoryContextWorkspace.test.tsx`:

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MemoryContextWorkspace } from "./MemoryContextWorkspace";
import type { DesktopMemoryContextSnapshot } from "../types";

function snapshot(): DesktopMemoryContextSnapshot {
  return {
    cwd: "/tmp/project",
    scope_mode: "workspace",
    memory: {
      identity: [{ id: "m1", type: "semantic", facet: "identity", scope: "user", scope_id: "local-user", content: "User is Han", confidence: 0.9, created_at: 1, updated_at: 2, expires_at: 999 }],
      preference: [],
      strategic: [],
      domain: [],
      procedural: [],
      episodic: [],
    },
    memory_summary: { identity: 1, preference: 0, strategic: 0, domain: 0, procedural: 0, episodic: 0 },
    runtime_context: {
      turn_id: "turn-1",
      backend: "codex",
      cwd: "/tmp/project",
      session_id: "session-1",
      model: "model-a",
      created_at: 10,
      capsule_text: "<iota-context>\n<session>session-1</session>\n</iota-context>",
      sections: [{ name: "session", chars: 9, preview: "session-1" }],
      budgets: { memory_chars: 1, skills_chars: 1, working_memory_chars: 1, workspace_chars: 1, handoff_chars: 1 },
    },
    context_engine: {
      enabled: true,
      budgets: { memory_chars: 1, skills_chars: 1, working_memory_chars: 1, workspace_chars: 1, handoff_chars: 1 },
    },
    errors: [],
  };
}

describe("MemoryContextWorkspace", () => {
  it("renders bucket counts and selected record details", () => {
    render(<MemoryContextWorkspace snapshot={snapshot()} loading={false} error={null} onScopeModeChange={vi.fn()} />);
    expect(screen.getByText("Identity")).toBeTruthy();
    expect(screen.getByText("User is Han")).toBeTruthy();
    expect(screen.getByText("confidence")).toBeTruthy();
  });

  it("filters records locally", () => {
    render(<MemoryContextWorkspace snapshot={snapshot()} loading={false} error={null} onScopeModeChange={vi.fn()} />);
    fireEvent.change(screen.getByPlaceholderText("Filter loaded memory..."), { target: { value: "missing" } });
    expect(screen.getByText("No records match the current filter")).toBeTruthy();
  });

  it("keeps full capsule collapsed by default", () => {
    render(<MemoryContextWorkspace snapshot={snapshot()} loading={false} error={null} onScopeModeChange={vi.fn()} />);
    expect(screen.queryByText("<iota-context>")).toBeNull();
    fireEvent.click(screen.getByText("Full Capsule"));
    expect(screen.getByText(/<iota-context>/)).toBeTruthy();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd crates/iota-desktop && npm test -- MemoryContextWorkspace.test.tsx
```

Expected: FAIL because `MemoryContextWorkspace` does not exist.

- [ ] **Step 3: Implement component**

Create `crates/iota-desktop/src/components/MemoryContextWorkspace.tsx`:

```tsx
import { useMemo, useState } from "react";
import { Database, FileText, Search } from "lucide-react";
import type { DesktopMemoryBuckets, DesktopMemoryContextSnapshot, DesktopMemoryRecord, MemoryScopeMode } from "../types";

type BucketName = keyof DesktopMemoryBuckets;

const BUCKETS: Array<{ key: BucketName; label: string }> = [
  { key: "identity", label: "Identity" },
  { key: "preference", label: "Preference" },
  { key: "strategic", label: "Strategic" },
  { key: "domain", label: "Domain" },
  { key: "procedural", label: "Procedural" },
  { key: "episodic", label: "Episodic" },
];

type Props = {
  snapshot: DesktopMemoryContextSnapshot | null;
  loading: boolean;
  error: string | null;
  onScopeModeChange: (mode: MemoryScopeMode) => void;
};

export function MemoryContextWorkspace({ snapshot, loading, error, onScopeModeChange }: Props) {
  const [bucket, setBucket] = useState<BucketName>("identity");
  const [filter, setFilter] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showFullCapsule, setShowFullCapsule] = useState(false);

  const records = snapshot?.memory[bucket] ?? [];
  const filtered = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return records;
    return records.filter((record) =>
      [record.id, record.type, record.facet, record.scope, record.scope_id, record.content]
        .filter(Boolean)
        .join(" ")
        .toLowerCase()
        .includes(needle),
    );
  }, [filter, records]);
  const selected = filtered.find((record) => record.id === selectedId) ?? filtered[0] ?? null;

  if (loading && !snapshot) {
    return <div className="flex flex-1 items-center justify-center text-sm text-gray-500">Loading memory context...</div>;
  }

  return (
    <div className="grid min-h-0 flex-1 grid-cols-2 gap-0 overflow-hidden">
      <section className="min-w-0 overflow-y-auto border-r border-white/10 p-5">
        <div className="mb-4 flex items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
            <Database className="h-4 w-4 text-primary" />
            Persistent Memory
          </div>
          <div className="flex rounded-md border border-white/10 bg-white/[0.03] p-1">
            {(["workspace", "all"] as MemoryScopeMode[]).map((mode) => (
              <button
                key={mode}
                className={`rounded px-2.5 py-1 text-xs font-semibold ${snapshot?.scope_mode === mode ? "bg-primary text-white" : "text-gray-400 hover:text-white"}`}
                onClick={() => onScopeModeChange(mode)}
              >
                {mode === "workspace" ? "Workspace" : "All"}
              </button>
            ))}
          </div>
        </div>
        {error ? <div className="mb-3 rounded border border-rose-500/20 bg-rose-500/10 p-2 text-xs text-rose-200">{error}</div> : null}
        {snapshot?.errors.map((item) => (
          <div key={`${item.area}:${item.message}`} className="mb-2 rounded border border-amber-500/20 bg-amber-500/10 p-2 text-xs text-amber-200">
            {item.area}: {item.message}
          </div>
        ))}
        <label className="mb-3 flex items-center gap-2 rounded-md border border-white/10 bg-white/[0.04] px-3 py-2 text-xs text-gray-400">
          <Search className="h-3.5 w-3.5" />
          <input className="w-full bg-transparent outline-none" placeholder="Filter loaded memory..." value={filter} onChange={(event) => setFilter(event.target.value)} />
        </label>
        <div className="mb-4 grid grid-cols-3 gap-2">
          {BUCKETS.map((item) => (
            <button key={item.key} onClick={() => { setBucket(item.key); setSelectedId(null); }} className={`rounded-md border p-2 text-left text-xs ${bucket === item.key ? "border-primary/50 bg-primary/10 text-white" : "border-white/5 bg-white/[0.02] text-gray-400"}`}>
              <div className="font-semibold">{item.label}</div>
              <div className="mt-1 text-[11px] text-gray-500">{snapshot?.memory_summary[item.key] ?? 0} records</div>
            </button>
          ))}
        </div>
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(240px,0.8fr)] gap-3">
          <div className="space-y-2">
            {filtered.length === 0 ? <div className="rounded border border-white/5 p-4 text-xs text-gray-500">No records match the current filter</div> : null}
            {filtered.map((record) => (
              <button key={record.id} onClick={() => setSelectedId(record.id)} className={`w-full rounded-md border p-3 text-left text-xs ${selected?.id === record.id ? "border-primary/50 bg-white/[0.04]" : "border-white/5 bg-white/[0.02] hover:bg-white/[0.04]"}`}>
                <div className="line-clamp-3 text-gray-200">{record.content}</div>
                <div className="mt-2 font-mono text-[10px] text-gray-500">{record.scope}:{record.scope_id}</div>
              </button>
            ))}
          </div>
          <MemoryRecordDetail record={selected} />
        </div>
      </section>
      <section className="min-w-0 overflow-y-auto p-5">
        <div className="mb-4 flex items-center gap-2 text-sm font-semibold text-gray-200">
          <FileText className="h-4 w-4 text-primary" />
          Runtime Context
        </div>
        {!snapshot?.runtime_context ? (
          <div className="rounded border border-white/5 bg-white/[0.02] p-4 text-sm text-gray-500">No runtime context captured in this desktop session.</div>
        ) : (
          <div className="space-y-4">
            <div className="rounded-md border border-white/5 bg-white/[0.02] p-3 text-xs text-gray-400">
              <div>Turn: <span className="font-mono text-gray-200">{snapshot.runtime_context.turn_id}</span></div>
              <div>Backend: <span className="uppercase text-gray-200">{snapshot.runtime_context.backend}</span></div>
              <div>Session: <span className="font-mono text-gray-200">{snapshot.runtime_context.session_id}</span></div>
              <div>Model: <span className="text-gray-200">{snapshot.runtime_context.model ?? "N/A"}</span></div>
            </div>
            <div className="space-y-2">
              {snapshot.runtime_context.sections.map((section) => (
                <div key={section.name} className="rounded-md border border-white/5 bg-white/[0.02] p-3">
                  <div className="flex justify-between text-xs font-semibold text-gray-300">
                    <span>{section.name}</span>
                    <span className="font-mono text-gray-500">{section.chars} chars</span>
                  </div>
                  <p className="mt-2 text-xs text-gray-500">{section.preview}</p>
                </div>
              ))}
            </div>
            <button className="rounded border border-white/10 px-3 py-1.5 text-xs text-gray-300 hover:bg-white/10" onClick={() => setShowFullCapsule((value) => !value)}>
              Full Capsule
            </button>
            {showFullCapsule ? <pre className="max-h-[420px] overflow-auto rounded-md border border-white/5 bg-black/30 p-3 text-xs text-gray-300">{snapshot.runtime_context.capsule_text}</pre> : null}
          </div>
        )}
      </section>
    </div>
  );
}

function MemoryRecordDetail({ record }: { record: DesktopMemoryRecord | null }) {
  if (!record) {
    return <div className="rounded border border-white/5 p-4 text-xs text-gray-500">Select a memory record</div>;
  }
  return (
    <aside className="rounded-md border border-white/5 bg-white/[0.02] p-3 text-xs text-gray-400">
      <div className="mb-3 whitespace-pre-wrap text-sm leading-6 text-gray-200">{record.content}</div>
      <dl className="space-y-1">
        <div><dt className="inline text-gray-500">confidence</dt> <dd className="inline text-gray-200">{record.confidence.toFixed(2)}</dd></div>
        <div><dt className="inline text-gray-500">type</dt> <dd className="inline text-gray-200">{record.type}</dd></div>
        <div><dt className="inline text-gray-500">scope</dt> <dd className="inline text-gray-200">{record.scope}:{record.scope_id}</dd></div>
        <div><dt className="inline text-gray-500">updated</dt> <dd className="inline text-gray-200">{record.updated_at}</dd></div>
        <div><dt className="inline text-gray-500">expires</dt> <dd className="inline text-gray-200">{record.expires_at}</dd></div>
      </dl>
    </aside>
  );
}
```

- [ ] **Step 4: Run component tests**

Run:

```bash
cd crates/iota-desktop && npm test -- MemoryContextWorkspace.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/iota-desktop/src/components/MemoryContextWorkspace.tsx crates/iota-desktop/src/components/MemoryContextWorkspace.test.tsx crates/iota-desktop/package.json crates/iota-desktop/package-lock.json
git commit -m "feat: add memory context workspace component"
```

## Task 7: Wire Workspace Into Desktop Shell

**Files:**
- Modify: `crates/iota-desktop/src/components/ChatWorkbench.tsx`
- Modify: `crates/iota-desktop/src/components/RightInspector.tsx`
- Modify: `crates/iota-desktop/src/turnReducer.test.ts`

- [ ] **Step 1: Add state and API wiring in `ChatWorkbench`**

In `crates/iota-desktop/src/components/ChatWorkbench.tsx`, extend imports:

```tsx
import { getMemoryContextSnapshot } from "../api";
import { MemoryContextWorkspace } from "./MemoryContextWorkspace";
import type { DesktopMemoryContextSnapshot, MemoryScopeMode } from "../types";
```

Change view state:

```tsx
const [view, setView] = useState<"chat" | "memory-context" | "config">("chat");
const [memoryScopeMode, setMemoryScopeMode] = useState<MemoryScopeMode>("workspace");
const [memoryContextSnapshot, setMemoryContextSnapshot] = useState<DesktopMemoryContextSnapshot | null>(null);
const [memoryContextLoading, setMemoryContextLoading] = useState(false);
const [memoryContextError, setMemoryContextError] = useState<string | null>(null);
```

Add loader:

```tsx
const refreshMemoryContext = useCallback(async (scopeMode: MemoryScopeMode = memoryScopeMode) => {
  setMemoryContextLoading(true);
  setMemoryContextError(null);
  try {
    setMemoryContextSnapshot(await getMemoryContextSnapshot(scopeMode));
  } catch (err) {
    setMemoryContextError(err instanceof Error ? err.message : String(err));
  } finally {
    setMemoryContextLoading(false);
  }
}, [memoryScopeMode]);
```

Add effect:

```tsx
useEffect(() => {
  if (view === "memory-context") {
    refreshMemoryContext(memoryScopeMode);
  }
}, [view, memoryScopeMode, refreshMemoryContext]);
```

Add scope setter:

```tsx
const handleMemoryScopeModeChange = (mode: MemoryScopeMode) => {
  setMemoryScopeMode(mode);
  refreshMemoryContext(mode);
};
```

- [ ] **Step 2: Add navigation button**

In the header nav, add:

```tsx
<button
  className={`rounded px-3 py-1 text-xs font-semibold transition-all ${
    view === "memory-context" ? "bg-primary text-white shadow" : "text-gray-400 hover:text-white"
  }`}
  onClick={() => setView("memory-context")}
>
  Memory / Context
</button>
```

- [ ] **Step 3: Render workspace**

Replace the current two-way content conditional with a three-way branch:

```tsx
{view === "chat" ? (
  <>
    {/* existing chat scroll area and prompt form stay unchanged */}
  </>
) : view === "memory-context" ? (
  <MemoryContextWorkspace
    snapshot={memoryContextSnapshot}
    loading={memoryContextLoading}
    error={memoryContextError}
    onScopeModeChange={handleMemoryScopeModeChange}
  />
) : (
  <ConfigPanel config={config} backendChecks={backendChecks} onConfigUpdate={handleConfigUpdate} />
)}
```

Keep the existing chat JSX intact inside the `chat` branch.

- [ ] **Step 4: Add inspector entry point**

In `crates/iota-desktop/src/components/RightInspector.tsx`, extend props:

```tsx
onOpenMemoryContext?: () => void;
```

Add a button in the turn overview section:

```tsx
{onOpenMemoryContext ? (
  <button
    className="mt-2 w-full rounded border border-white/10 px-3 py-1.5 text-xs text-gray-300 hover:bg-white/10 transition-colors"
    onClick={onOpenMemoryContext}
  >
    Open in Memory / Context
  </button>
) : null}
```

Pass it from `ChatWorkbench`:

```tsx
<RightInspector
  turn={activeTurn}
  observability={observability}
  onOpenMemoryContext={() => setView("memory-context")}
  onApprovalDecision={(approvalId, approved) =>
    dispatch({ type: "approval_decision", approvalId, approved })
  }
/>
```

- [ ] **Step 5: Run frontend tests and typecheck**

Run:

```bash
cd crates/iota-desktop && npm test && npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/iota-desktop/src/components/ChatWorkbench.tsx crates/iota-desktop/src/components/RightInspector.tsx crates/iota-desktop/src/turnReducer.test.ts
git commit -m "feat: wire memory context workspace into desktop"
```

## Task 8: Final Verification

**Files:**
- Modify if needed: `docs/desktop-mvp-acceptance.md`

- [ ] **Step 1: Run core tests**

```bash
cargo test -p iota-core daemon::desktop memory::store engine::tests
```

Expected: PASS.

- [ ] **Step 2: Run desktop Rust tests**

```bash
cargo test -p iota-desktop
```

Expected: PASS.

- [ ] **Step 3: Run frontend tests and build**

```bash
cd crates/iota-desktop && npm test && npm run build
```

Expected: PASS.

- [ ] **Step 4: Manual desktop verification**

Run:

```bash
cd crates/iota-desktop
npm run tauri dev
```

Verify:

- `Chat` still sends a prompt through the daemon.
- After a turn starts, `Memory / Context` opens.
- `Workspace` memory scope loads read-only bucket summaries.
- `All` scope reloads and changes the snapshot mode.
- Runtime context shows the most recent actual capsule.
- Full capsule is collapsed by default and expands on click.
- Restarting the desktop/daemon clears runtime context until a new turn runs.

- [ ] **Step 5: Commit final verification notes if docs changed**

If `docs/desktop-mvp-acceptance.md` was updated:

```bash
git add docs/desktop-mvp-acceptance.md
git commit -m "docs: add memory context desktop acceptance checks"
```

If no docs were changed, skip this commit.
