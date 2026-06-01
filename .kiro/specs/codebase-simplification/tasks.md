# Implementation Plan: Codebase Simplification

## Overview

对 iota-sympantos workspace 执行五项外科手术式重构，按独立变更逐步推进：删除 `storage/` 模块、内联 `engine/session_ledger.rs`、合并 SQLite 文件、保留 `event_sync.rs`（无代码变更）、为 `AdvancedBridge` 添加降级处理，最后清理残留测试文件并更新文档。每项变更均可独立验证，互不依赖。

---

## Tasks

- [x] 1. 删除 `storage/` 模块及其 Tauri command
  - [x] 1.1 删除 `crates/iota-core/src/storage/` 整目录
    - 删除 `storage/mod.rs`、`storage/models.rs`、`storage/retry.rs`、`storage/supabase.rs`、`storage/SKILL.md`
    - 从 `crates/iota-core/src/lib.rs` 中删除 `pub mod storage;` 声明
    - 检查 `crates/iota-core/Cargo.toml`：若 `reqwest` 仅被 `storage/` 使用则删除该依赖；若 `skill/cache.rs` 或 `memory/embedding.rs` 仍使用则保留
    - _Requirements: 1.1, 1.3, 1.4, 1.5_

  - [x] 1.2 从 `iota-desktop` 中移除 `sync_pipeline_artifacts` Tauri command
    - 从 `crates/iota-desktop/src-tauri/src/lib.rs` 删除 `use iota_core::storage::SupabaseStore` import
    - 删除 `SyncPipelineResult` 结构体定义
    - 删除 `sync_pipeline_artifacts` 函数体
    - 从 `tauri::generate_handler![]` 宏调用中移除 `sync_pipeline_artifacts` 条目
    - _Requirements: 1.2, 1.6_

  - [x] 1.3 验证 `storage/` 删除后编译通过
    - 运行 `cargo build --workspace`，确认 exit code 0
    - 运行 `cargo build -p iota-desktop`，确认 exit code 0
    - 运行 `grep -r "iota_core::storage" crates/`，确认无输出
    - _Requirements: 1.1, 1.2, 1.3_

- [x] 2. 清理 `storage/` 删除后的残留测试文件
  - [x] 2.1 删除 `storage_tests.rs` 并清理 `lib_tests.rs`
    - 删除 `crates/iota-core/src/storage/storage_tests.rs`（随目录一起删除，确认已包含在任务 1.1 中）
    - 检查 `crates/iota-desktop/src-tauri/src/lib_tests.rs`，删除所有引用 `sync_pipeline_artifacts` 的 `#[test]` 函数
    - _Requirements: 6.1, 6.2, 6.4, 6.5_

  - [x] 2.2 验证测试套件编译通过
    - 运行 `cargo test --workspace`，确认 exit code 0，无 `storage`/`sync_pipeline_artifacts`/`SupabaseStore`/`PipelineArtifact` 相关编译错误
    - _Requirements: 6.3_

- [x] 3. Checkpoint — 确认 storage 删除阶段完成
  - 确保 `cargo build --workspace` 和 `cargo test --workspace` 均 exit code 0，如有问题请向用户反馈。

- [x] 4. 内联 `engine/session_ledger.rs` 薄包装层
  - [x] 4.1 将 4 个方法从 `session_ledger.rs` 移入 `engine/prompt.rs`
    - 将 `persist_backend_session_id`、`ensure_ledger_session`、`prepare_backend_handoff`、`record_ledger_turn` 四个 `pub(super) impl IotaEngine` 方法剪切到 `engine/prompt.rs` 末尾，作为同一 `impl IotaEngine` 块的一部分
    - 方法签名、参数类型、委托调用目标（`SessionLedger` 方法）保持不变
    - 删除 `engine/session_ledger.rs` 文件
    - 从 `engine/mod.rs` 中删除 `mod session_ledger;` 声明（如存在）
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [x] 4.2 为内联后的方法编写单元测试
    - 在 `crates/iota-core/src/engine/tests.rs` 中新增测试：验证 `prepare_backend_handoff` 在 backend 切换时调用 `publish_handoff`，在 backend 不变时不调用
    - _Requirements: 2.3, 2.4_

  - [x] 4.3 验证内联后编译通过
    - 运行 `cargo build --workspace`，确认 exit code 0
    - 运行 `grep -r "session_ledger" crates/iota-core/src/engine/`，确认仅 `prompt.rs` 中有方法定义，无 `mod session_ledger`
    - _Requirements: 2.2_

- [x] 5. 合并 `SessionLedger` 和 `ApprovalStore` 到 `store.sqlite`
  - [x] 5.1 更新 `StorePaths`：删除 `sessions_db()` 和 `approvals_db()`，新增 `store_db()`
    - 修改 `crates/iota-core/src/config/paths.rs`：删除 `sessions_db()` 和 `approvals_db()` 方法，新增 `pub fn store_db(&self) -> PathBuf { self.root.join("store.sqlite") }`
    - 编译器将自动定位所有残留调用点
    - _Requirements: 3.1_

  - [x] 5.2 更新 `SessionLedger::default_path()` 使用 `store_db()`
    - 修改 `crates/iota-core/src/store/ledger.rs`：将 `default_path()` 中的 `StorePaths::resolve()?.sessions_db()` 替换为 `StorePaths::resolve()?.store_db()`
    - 确认 `SessionLedger::open` 中的 sessions/turns/handoffs DDL 仍通过独立 `execute_batch` 调用执行
    - _Requirements: 3.1, 3.2, 3.6_

  - [x] 5.3 更新 `ApprovalStore::default_path()` 使用 `store_db()`
    - 修改 `crates/iota-core/src/store/approvals.rs`：将 `default_path()` 中的 `StorePaths::resolve()?.approvals_db()` 替换为 `StorePaths::resolve()?.store_db()`
    - 确认 `ApprovalStore::open` 中的 approval_requests/approval_decisions DDL 仍通过独立 `execute_batch` 调用执行
    - _Requirements: 3.1, 3.2, 3.6_

  - [x] 5.4 更新 `store/ledger_tests.rs` 和 `store/approvals_tests.rs` 中的路径断言
    - 修改 `crates/iota-core/src/store/ledger_tests.rs`：将 `default_path()` 断言从期望 `sessions.sqlite` 改为期望 `store.sqlite`
    - 修改 `crates/iota-core/src/store/approvals_tests.rs`：将 `default_path()` 断言从期望 `approvals.sqlite` 改为期望 `store.sqlite`
    - _Requirements: 3.1_

  - [x] 5.5 验证合并后编译和测试通过
    - 运行 `cargo build --workspace`，确认 exit code 0
    - 运行 `grep -r "sessions_db\|approvals_db" crates/`，确认无输出
    - 运行 `cargo test -p iota-core -- store`，确认路径断言通过
    - _Requirements: 3.1, 3.3, 3.4_

- [x] 6. Checkpoint — 确认 SQLite 合并阶段完成
  - 确保 `cargo build --workspace` 和 `cargo test --workspace` 均 exit code 0，如有问题请向用户反馈。

- [x] 7. 为 `AdvancedBridge` 添加 hermes 不可用时的降级处理
  - [x] 7.1 在 `specify` 和 `decompose` 方法开头添加 `hermes_bin` 存在性前置检查
    - 修改 `crates/iota-kanban/src/bridge.rs`：在 `specify` 方法开头添加 `if !self.hermes_bin.exists() { anyhow::bail!("hermes binary not found: {}", self.hermes_bin.display()); }`
    - 同样在 `decompose` 方法开头添加相同的前置检查
    - 确认错误消息包含字面文本 `"hermes binary not found"` 后跟 `hermes_bin` 路径值
    - _Requirements: 5.1_

  - [x] 7.2 新增 `ensure_bridge_available` 模块级公开函数
    - 在 `crates/iota-kanban/src/bridge.rs` 中新增 `pub fn ensure_bridge_available(bridge: &AdvancedBridge) -> Result<()>`
    - 实现：若 `bridge.is_available()` 返回 `false`，则 `anyhow::bail!("hermes binary not found or not executable: {}", bridge.hermes_bin.display())`
    - 确认 `is_available()` 在 `hermes_bin` 不存在或不可执行时返回 `false`
    - _Requirements: 5.3, 5.4_

  - [x] 7.3 更新 TUI kanban command handler 使用 `ensure_bridge_available`
    - 修改 `crates/iota-cli/src/tui/kanban_command.rs`：在 `/kanban specify` 和 `/kanban decompose` 处理路径中，在调用 `bridge.specify`/`bridge.decompose` 之前先调用 `ensure_bridge_available(&bridge)`
    - 若 `ensure_bridge_available` 返回 `Err`，将 `e.to_string()` 追加到 `output_lines` 并 `return`，不 panic
    - _Requirements: 5.2_

  - [x] 7.4 为 `AdvancedBridge` 降级处理编写单元测试
    - 在 `crates/iota-kanban/src/bridge_tests.rs` 中新增测试：当 `hermes_bin` 指向不存在路径时，`specify` 和 `decompose` 返回包含 `"hermes binary not found"` 的 `Err`
    - 新增测试：`is_available()` 在 `hermes_bin` 不存在时返回 `false`
    - 新增测试：`ensure_bridge_available` 在 `is_available()` 为 `false` 时返回包含 `hermes_bin` 路径的 `Err`
    - _Requirements: 5.1, 5.3, 5.4_

  - [x] 7.5 验证 AdvancedBridge 变更编译和测试通过
    - 运行 `cargo build -p iota-kanban`，确认 exit code 0
    - 运行 `cargo test -p iota-kanban -- bridge`，确认新增测试通过
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

- [x] 8. 更新文档
  - [x] 8.1 更新 `AGENTS.md` 源码结构部分
    - 从 `AGENTS.md` 的源码结构树中删除 `storage/` 条目及其描述行
    - 从 `engine/` 子模块列表中删除 `session_ledger.rs` 行
    - _Requirements: 7.1, 7.2_

  - [x] 8.2 更新 `docs/architecture.md`
    - 删除 `docs/architecture.md` 中所有提及 `SupabaseStore` 或 `storage/` 模块的段落、表格行或列表项
    - _Requirements: 7.3_

  - [x] 8.3 更新 `store/SKILL.md` 路径描述
    - 修改 `crates/iota-core/src/store/SKILL.md`（如存在）：将 `sessions_db()` 和 `approvals_db()` 的路径描述替换为单一 `store_db()` 条目，指向 `~/.i6/context/store.sqlite`
    - _Requirements: 7.4_

- [x] 9. Final Checkpoint — 确认所有变更完成
  - 确保 `cargo build --workspace` exit code 0
  - 确保 `cargo test --workspace` exit code 0
  - 确保 `grep -r "iota_core::storage" crates/` 无输出
  - 确保 `grep -r "sessions_db\|approvals_db" crates/` 无输出
  - 确保 `grep -r "session_ledger" crates/iota-core/src/engine/` 仅 `prompt.rs` 中有方法定义
  - 如有问题请向用户反馈。

---

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- 任务 4（`event_sync.rs` 保留）无需代码变更，已在需求中确认保留现状
- 每项变更均独立可验证，互不依赖，可按任意顺序执行
- SQLite 合并后用户需手动删除旧文件：`rm ~/.i6/context/sessions.sqlite && rm ~/.i6/context/approvals.sqlite`
- 测试文件遵循项目规范：独立 `*_tests.rs` 文件，使用 `#[path]` 属性引用，禁止内联 `#[cfg(test)]` 块

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["2.1", "4.1", "5.1"] },
    { "id": 2, "tasks": ["1.3", "2.2", "5.2", "5.3", "7.1"] },
    { "id": 3, "tasks": ["4.2", "4.3", "5.4", "7.2"] },
    { "id": 4, "tasks": ["5.5", "7.3", "8.1", "8.2", "8.3"] },
    { "id": 5, "tasks": ["7.4"] },
    { "id": 6, "tasks": ["7.5"] }
  ]
}
```
