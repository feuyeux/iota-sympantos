# Requirements Document

## Introduction

iota-sympantos 是一个轻量级 Rust workspace，核心职责是通过 ACP 协议编排多个 AI 编程助手后端。本次重构以"最小必要复杂度"为原则，识别并消除工程中的冗余模块、重复抽象和过度设计，在不损失核心功能的前提下降低维护成本、减少外部依赖、提升代码可读性。

经过深度代码审查，识别出以下主要问题：

1. **`storage/` 模块（SupabaseStore）**：仅在 `iota-desktop` 的一个 Tauri command（`sync_pipeline_artifacts`）中被调用，用于将 `script_agent_output/` 目录下的 JSON 文件上传到 Supabase。该功能与工程核心职责（ACP 协议编排）完全无关，属于独立的内容发布 pipeline 副业务，引入了 `reqwest` blocking 外部依赖。
2. **`engine/session_ledger.rs`**：是 `IotaEngine` 的 impl 扩展文件，通过委托调用 `store/ledger.rs` 的 `SessionLedger`，本身不含独立逻辑，仅是一层薄包装，可直接内联到 `engine/prompt.rs` 或 `engine/mod.rs`。
3. **`store/` 层使用 4 个独立 SQLite 文件**：`events.sqlite`（CacheStore + ObservabilityStore 共用）、`sessions.sqlite`（SessionLedger）、`approvals.sqlite`（ApprovalStore）、`memory.sqlite`（MemoryStore）。其中 `CacheStore` 和 `ObservabilityStore` 已共用 `events.sqlite`，但 `SessionLedger` 和 `ApprovalStore` 各自独立，可合并以减少文件句柄和路径管理复杂度。
4. **`iota-kanban/event_sync.rs`**：实现了基于 TCP 的跨节点 Kanban 事件同步（serve/pull/push），是一个分布式特性，对于单机开发工具场景属于过度设计，但已有 CLI 命令（`iota kanban sync serve/pull/push`）调用，需评估是否保留。
5. **`iota-kanban/bridge.rs`（AdvancedBridge）**：通过调用 `hermes kanban specify/decompose` 命令实现 LLM 辅助任务分解，已集成到 TUI `/kanban specify` 和 `/kanban decompose` 命令，属于有效功能，但依赖 hermes 二进制可用性。

---

## Glossary

- **Codebase**：iota-sympantos Rust workspace，包含 `iota-core`、`iota-cli`、`iota-kanban`、`iota-desktop` 四个 crate。
- **ACP**：Agent Control Protocol，基于 stdin/stdout 的换行分隔 JSON-RPC 2.0 协议，用于编排 AI 后端。
- **SupabaseStore**：`crates/iota-core/src/storage/` 模块中的 Supabase REST API 客户端，用于持久化 pipeline artifacts。
- **SQLite_Store_Layer**：`crates/iota-core/src/store/` 模块，包含 `CacheStore`、`ApprovalStore`、`SessionLedger`、`ObservabilityStore` 四个基于 SQLite 的存储组件。
- **PipelineArtifact**：`storage/models.rs` 中定义的联合类型，覆盖 Research、Script、XOptimizer 三个 pipeline 阶段的产出物。
- **sync_pipeline_artifacts**：`iota-desktop` 中唯一调用 `SupabaseStore` 的 Tauri command，用于将本地 JSON 文件上传到 Supabase。
- **AdvancedBridge**：`iota-kanban/bridge.rs` 中的结构体，通过调用 `hermes kanban specify/decompose` 实现 LLM 辅助任务分解。
- **EventSync**：`iota-kanban/event_sync.rs` 中实现的基于 TCP 的跨节点 Kanban 事件同步机制。
- **ShadowMaterializer**：`iota-kanban/shadow.rs` 中的结构体，为 hermes worker 创建兼容其 schema 的临时 SQLite 数据库副本。
- **IotaEngine**：`crates/iota-core/src/engine/mod.rs` 中的核心编排引擎，管理 ACP 客户端池、上下文、记忆和技能。
- **SessionLedger**：`store/ledger.rs` 中的 SQLite 存储组件，持久化 session、turn 和 backend handoff 记录。
- **WorkingMemoryBuffer**：`context/mod.rs` 中的短期工作记忆缓冲区，存储最近 N 轮对话摘要。
- **Refactoring_Tool**：执行本次重构的工具或开发者。

---

## Requirements

### Requirement 1: 删除 `storage/` 模块（SupabaseStore）

**User Story:** As a developer maintaining iota-sympantos, I want to remove the `storage/` module and its Supabase dependency, so that the codebase only contains components directly related to ACP backend orchestration.

#### Acceptance Criteria

1. WHEN the `storage/` module is deleted, THE Codebase SHALL produce exit code 0 from `cargo build --workspace` with no reference to `SupabaseStore`, `PipelineArtifact`, `PipelineRecord`, `ResearchData`, `ScriptData`, or `XOptimizerData` in any source file.
2. WHEN the `sync_pipeline_artifacts` Tauri command is removed from `iota-desktop/src-tauri/src/lib.rs`, THE Codebase SHALL produce exit code 0 from `cargo build -p iota-desktop` and all remaining `#[tauri::command]`-annotated functions SHALL remain present and callable.
3. THE Codebase SHALL NOT contain any `use iota_core::storage` import in any `.rs` file after the deletion.
4. IF `reqwest` appears in `iota-core/Cargo.toml` and is no longer referenced by any module other than the deleted `storage/` module, THEN THE `iota-core/Cargo.toml` SHALL remove the `reqwest` dependency entry; IF `reqwest` is still used by other modules (e.g., `skill/cache.rs`, `memory/embedding.rs`), THEN the dependency SHALL be retained unchanged.
5. IF `iota-core/src/lib.rs` contains a `pub mod storage` declaration, THEN THE `lib.rs` SHALL remove that declaration after the deletion.
6. WHEN any `.rs` file outside `storage/` imports a symbol from `iota_core::storage` (e.g., `use iota_core::storage::SupabaseStore`), THE Codebase SHALL remove that import so that `cargo build --workspace` produces exit code 0.

---

### Requirement 2: 内联 `engine/session_ledger.rs` 薄包装层

**User Story:** As a developer reading the engine module, I want the session ledger delegation logic to be co-located with the prompt execution flow, so that I can understand the full turn lifecycle without navigating to a separate file.

#### Acceptance Criteria

1. WHEN `engine/session_ledger.rs` is deleted, THE four methods `persist_backend_session_id`, `ensure_ledger_session`, `prepare_backend_handoff`, and `record_ledger_turn` SHALL be moved into `engine/prompt.rs` as `impl IotaEngine` methods, since `engine/prompt.rs` is the sole caller of all four methods.
2. WHEN the inline refactoring is complete, THE Codebase SHALL produce exit code 0 from `cargo build --workspace`, and each of the four moved methods SHALL invoke the same `SessionLedger` method with the same argument types as before the refactoring.
3. WHEN `prepare_backend_handoff` is called with a backend value different from `last_used_backend`, THE `SessionLedger` SHALL receive a `publish_handoff` call where `from_backend` equals `last_used_backend` and `to_backend` equals the backend argument passed to `prepare_backend_handoff`.
4. WHEN `prepare_backend_handoff` is called with a backend value equal to `last_used_backend`, THE `SessionLedger` SHALL NOT receive a `publish_handoff` call.

---

### Requirement 3: 合并 `SessionLedger` 和 `ApprovalStore` 到同一 SQLite 文件

**User Story:** As a developer operating iota-sympantos, I want the session ledger and approval store to share a single SQLite database file, so that the number of open file handles is reduced and path management is simplified.

#### Acceptance Criteria

1. WHEN `SessionLedger` and `ApprovalStore` are updated to use the same SQLite file path, THE `StorePaths` SHALL expose a `store_db()` method returning `<root>/store.sqlite` and SHALL remove the `sessions_db()` and `approvals_db()` methods so that any remaining callers fail at compile time.
2. WHEN the merged database is opened, THE SQLite_Store_Layer SHALL execute the sessions/turns/handoffs schema setup and the approval_requests/approval_decisions schema setup as two separate `execute_batch` calls, so that an error in one schema does not prevent the other from being returned independently.
3. THE `CacheStore` and `ObservabilityStore` SHALL continue to use `events_db()` as their shared path, unchanged.
4. THE `MemoryStore` SHALL continue to use `memory_db()` as its path, unchanged.
5. WHEN the merged store is first released, THE commit message SHALL contain a migration note stating that users must delete `sessions.sqlite` and `approvals.sqlite` from their `~/.i6/` directory before upgrading.
6. WHEN the merged store is opened with WAL mode, THE SQLite_Store_Layer SHALL apply `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;` to the merged database via a single `execute_batch` call, and IF that call returns an error, THEN THE SQLite_Store_Layer SHALL propagate the error and SHALL NOT return a usable database connection.

---

### Requirement 4: 评估并保留 `iota-kanban/event_sync.rs`（有条件保留）

**User Story:** As a developer reviewing the kanban module, I want the event sync feature to be clearly documented as an optional distributed feature, so that its scope and activation conditions are explicit.

#### Acceptance Criteria

1. THE `iota-kanban/event_sync.rs` module SHALL be retained as-is, given that `iota kanban sync serve/pull/push` CLI commands actively use it.
2. WHEN `iota kanban sync serve <addr>` is executed and the TCP bind succeeds, THE EventSync SHALL bind a TCP listener on the specified address and serve incoming event bundle requests.
3. IF `iota kanban sync serve <addr>` is executed and TCP binding fails due to port conflict or permission issues, THEN THE EventSync SHALL return an `Err` containing the OS error and SHALL NOT retry the bind.
4. WHEN `iota kanban sync pull <addr>` is executed and the peer is reachable within the 30-second I/O timeout, THE EventSync SHALL connect to the peer, request events since the per-source sync cursor persisted in the local store, and import the received bundle into the local store.
5. IF `iota kanban sync pull <addr>` is executed and the TCP connection to the peer fails or times out, THEN THE EventSync SHALL return an `Err` describing the connection failure.
6. THE `iota-kanban/src/lib.rs` SHALL continue to re-export `EventSyncServer`, `EventSyncClient`, `serve_sync`, `pull_sync`, and `push_sync` without change.

---

### Requirement 5: 为 `AdvancedBridge` 添加 hermes 不可用时的降级处理

**User Story:** As a TUI user running `/kanban specify` or `/kanban decompose`, I want a clear error message when hermes is not installed, so that I understand why the command failed instead of seeing a cryptic process spawn error.

#### Acceptance Criteria

1. WHEN `AdvancedBridge::specify` or `AdvancedBridge::decompose` is called and the `hermes_bin` path stored in the `AdvancedBridge` struct does not exist on the filesystem, THE AdvancedBridge SHALL return an `Err` whose `to_string()` contains the literal text `"hermes binary not found"` followed by the value of `hermes_bin`.
2. WHEN `ensure_bridge_available` returns an `Err`, THE TUI kanban command handler SHALL append the error's `to_string()` to its output lines and return without panicking.
3. THE `AdvancedBridge::is_available()` method SHALL return `false` when `hermes_bin` does not exist on the filesystem or is not executable by the current process.
4. WHEN `AdvancedBridge::is_available()` returns `false`, THE `ensure_bridge_available` function SHALL return an `Err` whose `to_string()` contains the value of `hermes_bin` as a string.

---

### Requirement 6: 清理 `storage/` 删除后的残留测试文件

**User Story:** As a developer running the test suite, I want all test files related to deleted modules to be removed, so that `cargo test` does not fail on missing module references.

#### Acceptance Criteria

1. WHEN the developer deletes the `storage/` directory, THE `storage/storage_tests.rs` file SHALL also be deleted as part of the same change.
2. IF `iota-desktop/src-tauri/src/lib_tests.rs` contains any `#[test]` function whose name or body references the identifier `sync_pipeline_artifacts`, THEN those test functions SHALL be removed from the file.
3. WHEN all deletions are complete, THE Codebase SHALL produce exit code 0 from `cargo test --workspace`, with no compilation errors referencing the identifiers `storage`, `sync_pipeline_artifacts`, `SupabaseStore`, or `PipelineArtifact`.
4. IF `iota-core/src/lib.rs` contains a `pub mod storage` declaration after the deletion, THEN the build SHALL fail; THE `lib.rs` SHALL NOT contain that declaration.
5. WHEN any `.rs` file outside the deleted `storage/` directory contains an import of any symbol from the `iota_core::storage` path, THE Codebase SHALL remove that import so that `cargo build --workspace` produces exit code 0.

---

### Requirement 7: 文档更新——同步 AGENTS.md 和 docs/ 中的模块描述

**User Story:** As a developer or AI tool reading AGENTS.md, I want the source structure documentation to reflect the actual codebase after simplification, so that the documentation does not mislead about the existence of removed modules.

#### Acceptance Criteria

1. WHEN `storage/` is deleted, THE `AGENTS.md` source structure section SHALL remove the `storage/` entry and its description, and this documentation update MAY be done separately from the code deletion as long as both are completed before the next release.
2. WHEN `engine/session_ledger.rs` is inlined, THE `AGENTS.md` source structure section SHALL update the `engine/` sub-module list to reflect the change, and this documentation update MAY be done separately from the code change as long as both are completed before the next release.
3. THE `docs/architecture.md` SHALL be updated to remove any reference to `SupabaseStore` or the `storage/` module.
4. WHEN `StorePaths` is updated to merge `sessions_db()` and `approvals_db()`, THE `store/SKILL.md` Configuration section SHALL reflect the new single `store_db()` path.
