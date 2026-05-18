# Refactor Plan: Structure, Logging, Lightweight Code

## Goals
1. Directory-level abstraction and encapsulation (split oversized files)
2. Lighter-weight code — remove redundant logic and over-heavy implementations
3. Ensure info-level logs are emitted; clean up debug logs (upgrade to info or remove)

## Status: ✅ All tasks completed

---

## Tasks

### Task 1: Fix debug → info log levels in acp/mod.rs ✅
- Migrated to `src/acp/client.rs` (AcpClient extracted to its own module)
- All lifecycle events now use `tracing::info!` with structured keys (`acp.process.start`, `acp.process.spawned`, `acp.init.done`, `acp.session.resolved`, `acp.prompt.done`)
- Wire-level dump retained as a commented trace for protocol debugging

### Task 2: Fix debug → info in engine.rs + remove duplicate ✅
- Migrated to `src/engine/prompt.rs` and `src/engine/mod.rs`
- `prompt.requested` and `execution.started` use `tracing::info!`; duplicate removed

### Task 3: Clean up permission.rs debug logs ✅
- `src/acp/permission.rs`: diagnostic debug calls removed; auto-approve path has no log noise

### Task 4: Fix bare unwraps in store/embedding.rs ✅
- Migrated to `src/memory/embedding.rs`; all three call sites use `.context("...")?`

### Task 5: Fix redundant config reads in cli multi-backend loop ✅
- `src/cli/run_cmd.rs`: config read once before loop, cloned per spawn

### Task 6: Split engine.rs into engine/ module ✅
Completed split:
- `src/engine/mod.rs` — IotaEngine struct, session lifecycle, shutdown
- `src/engine/prompt.rs` — run_prompt, run_with_timing, early-return paths
- `src/engine/memory_ops.rs` — memory extract/classify/recall/inject helpers
- `src/engine/session_ledger.rs` — handoff summary, last_used_backend logic
- `src/engine/telemetry.rs` — token usage recording, observability bridge

### Task 7: Split acp/mod.rs into acp/client.rs ✅
Completed split:
- `src/acp/client.rs` — AcpClient struct, spawn/connect/shutdown/run_prompt
- `src/acp/backend.rs` — AcpBackend enum, parse/command, ALL_BACKENDS
- `src/acp/mod.rs` — pub re-exports, AcpRunOptions, run_prompt top-level entry
- Additional: `wire.rs`, `parser.rs`, `types.rs`, `message.rs`, `stream_reader.rs`, `util.rs`

### Task 8: Split cli/mod.rs into cli/ subcommands ✅
Completed split:
- `src/cli/mod.rs` — run() dispatch, shared helpers
- `src/cli/run_cmd.rs` — `iota run` command logic
- `src/cli/info_cmd.rs` — backend info and check commands
- `src/cli/daemon_cmd.rs` — daemon/bench commands
- `src/cli/observability_cmd.rs` — token stats and observability commands
- `src/cli/skill_cmd.rs` — skill pull command

### Task 9: Split config.rs into config/ module ✅
Completed split:
- `src/config/mod.rs` — NimiaConfig, load/read entry points, pub re-exports
- `src/config/schema.rs` — raw YAML schema structs
- `src/config/backend.rs` — BackendConfig, backend_process_env_with_context
- `src/config/model.rs` — ModelConfig, configured_model
- `src/config/effective.rs` — EffectiveConfig (merged runtime view)
- `src/config/paths.rs` — StorePaths, default path resolution
- `src/config/context.rs` — ContextEngineConfig
- `src/config/loader.rs` — YAML loading and path expansion
- `src/config/helpers.rs` — shared utilities

### Task 10: Fix remaining debug log in memory/store.rs ✅
- `search_keyword` FTS fallback upgraded from `tracing::debug!` to `tracing::warn!` with structured key `memory.fts.fallback`

---

## Verification

```
cargo check        # ✅ passes
cargo test         # ✅ all tests pass
```
