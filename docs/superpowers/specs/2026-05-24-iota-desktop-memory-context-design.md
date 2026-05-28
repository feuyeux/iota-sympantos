# iota-desktop Memory / Context Viewer Design

> Archive note: this is a historical design spec. For current behavior and commands, see [../../iota book.md](../../iota%20book.md), [../../architecture.md](../../architecture.md), and [../../command.md](../../command.md).

## Goal

Add a daemon-first, read-only desktop view for persistent memory and non-persistent runtime context.

The feature should let a user inspect:

- the six persistent memory buckets: identity, preference, strategic, domain, procedural, episodic
- the most recent actual context capsule sent to a backend during the current desktop/daemon session

The first version is diagnostic and explanatory. It does not add memory editing, memory deletion, memory creation, or persistent storage for runtime context.

## Scope

In scope:

- Dedicated `Memory / Context` workspace in `iota-desktop`
- Split-lens layout: persistent memory on the left, runtime context on the right
- Workspace scope by default, with an `All` scope switch
- Read-only memory browsing, filtering, selection, and detail display
- Recent actual context capsule display with structured section summaries and a full text viewer
- Right inspector link from a selected turn to the dedicated workspace
- Daemon protocol, Tauri command, Rust types, TypeScript types, and tests

Out of scope:

- Creating, editing, deleting, or merging memory records from desktop
- Persisting full runtime context capsule text to SQLite, observability logs, or files
- Previewing a future prompt's context capsule
- Cross-session runtime context history
- Replacing `~/.i6/nimia.yaml` or bypassing the daemon-first desktop architecture

## Architecture

Use the existing daemon-first desktop boundary.

Data flow:

```text
iota-desktop React
  -> Tauri command get_memory_context_snapshot(scopeMode)
  -> daemon_client
  -> daemon JSON-line message GetMemoryContextSnapshot { cwd, scope_mode }
  -> iota-core daemon handler
      -> MemoryStore read-only bucket snapshot
      -> EnginePool / IotaEngine recent runtime context snapshot
      -> config/context metadata
  -> MemoryContextSnapshot response
```

The desktop app must not directly create `IotaEngine` and must not directly open `MemoryStore`. The Tauri layer remains a bridge between React and the daemon.

## Protocol

Add client message:

```rust
GetMemoryContextSnapshot {
    cwd: PathBuf,
    scope_mode: DesktopMemoryScopeMode,
}
```

Add server message:

```rust
MemoryContextSnapshot {
    snapshot: DesktopMemoryContextSnapshot,
}
```

Prefer typed structs over `serde_json::Value` for the new API.

Core response shape:

```rust
pub enum DesktopMemoryScopeMode {
    Workspace,
    All,
}

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

Memory records should include the current persisted fields needed for read-only diagnosis:

- `id`
- `type`
- `facet`
- `scope`
- `scope_id`
- `content`
- `confidence`
- `created_at`
- `updated_at`
- `expires_at`

Runtime context should include:

- `turn_id`
- `backend`
- `cwd`
- `session_id`
- `model`
- `created_at`
- `capsule_text`
- parsed or derived `sections`
- budget metadata when available

Errors are non-fatal. If memory is unavailable but runtime context exists, return the context and include a memory error. If runtime context is unavailable, return memory plus a clear empty state reason.

## Persistent Memory Behavior

`Workspace` mode is the default. It should use the same project identity semantics as current recall: user/global candidates plus the current project candidates derived from `cwd`, and session records where a current session id is available.

`All` mode reads all non-expired memory records and groups them into the six display buckets:

- `identity`: semantic records with facet `identity`
- `preference`: semantic records with facet `preference`
- `strategic`: semantic records with facet `strategic`
- `domain`: semantic records with facet `domain`
- `procedural`: procedural records
- `episodic`: episodic records

Ordering should be stable and useful for browsing:

- confidence descending
- updated time descending
- created time descending

The first version may do text filtering client-side over the loaded snapshot. A later version can add daemon-backed search.

## Runtime Context Behavior

Show the most recent actual context capsule sent to a backend, not a preview.

Capture point:

- after `ContextEngine::compose_effective_prompt()` returns the final effective prompt
- before sending the prompt to the ACP backend

The snapshot is in-memory only. It must not be persisted to:

- `memory.sqlite`
- observability store
- daemon logs
- desktop frontend local storage
- files

If the desktop app, daemon, or engine process restarts, the runtime context panel should show an empty state until another turn sends a context capsule.

If the context engine is disabled or injection is off, the runtime context panel should say so and show the latest turn metadata when available.

## UI

Add a primary workspace switch between `Chat` and `Memory / Context`.

The `Memory / Context` workspace uses a split-lens layout.

Left side: `Persistent Memory`

- segmented control: `Workspace` / `All`
- search/filter input for current loaded records
- six bucket summary with counts and empty states
- selected bucket record list
- selected record detail view with full content and metadata
- no create/edit/delete controls

Right side: `Runtime Context`

- latest turn metadata: backend, cwd, session, model, timestamp
- structured section summaries for context capsule sections such as `memory-tools`, `model`, `skills`, `memory`, `session`, `handoff`, `working-memory`, and `workspace`
- collapsed `Full Capsule` viewer with read-only text and copy affordance
- clear empty state when no context has been captured

Right inspector enhancement:

- selected turn shows a concise context summary and an `Open in Memory / Context` action
- the inspector does not render the full capsule
- when no turn is selected, the inspector continues to show observability and should not load full memory by default

## Component Boundaries

Suggested frontend components:

- `MemoryContextWorkspace`
- `MemoryBucketSummary`
- `MemoryRecordList`
- `MemoryRecordDetail`
- `RuntimeContextPanel`
- `ContextSectionList`

Suggested Rust additions:

- protocol structs in `crates/iota-core/src/daemon/proto.rs`
- daemon handler in `crates/iota-core/src/daemon/desktop.rs`
- read-only memory bucket helper in `crates/iota-core/src/memory/store.rs`
- recent context snapshot state in the engine layer
- Tauri command in `crates/iota-desktop/src-tauri/src/lib.rs`
- daemon client method in `crates/iota-desktop/src-tauri/src/daemon_client.rs`

## Testing

Rust tests:

- protocol serde roundtrip for snapshot request/response
- daemon handler returns snapshot with partial errors instead of failing the full request
- memory all-scope bucket grouping excludes expired records
- workspace mode follows existing recall scope semantics
- runtime context snapshot is updated after context composition and kept in memory only
- desktop Tauri command uses daemon client and does not instantiate `IotaEngine`

TypeScript tests:

- scope switch triggers snapshot reload
- bucket selection and local filtering are stable
- empty memory buckets render empty states
- missing runtime context renders an empty state
- full capsule is hidden by default and can be expanded

Verification commands:

```bash
cargo test -p iota-core daemon::desktop memory::store
cargo test -p iota-desktop
cd crates/iota-desktop && npm test && npm run build
```

Manual verification:

- Launch `iota-desktop`
- Run a prompt from `Chat`
- Open `Memory / Context`
- Confirm workspace memory buckets load read-only
- Confirm the runtime context panel shows the last actual turn capsule
- Switch to `All` and confirm the memory scope changes
- Restart desktop/daemon and confirm runtime context is empty until a new turn runs

## Risks

- Full capsule text can contain sensitive project or prompt information. Keeping it in memory only limits persistence risk, but screenshots and copy actions still require user care.
- All-scope memory views may become large. The first implementation should cap results per bucket or use conservative limits if the store is large.
- Parsing capsule sections from XML-like text should be best-effort. The full capsule text remains the source of truth.
- Workspace mode must follow existing recall semantics so desktop does not show a different memory universe than the engine injects.
