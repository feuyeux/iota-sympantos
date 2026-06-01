# Design Document

## Overview

本次重构以"最小必要复杂度"为原则，对 iota-sympantos workspace 执行五项外科手术式变更：

1. **删除 `storage/` 模块**（SupabaseStore + 相关 Tauri command）
2. **内联 `engine/session_ledger.rs`** 薄包装层到 `engine/prompt.rs`
3. **合并 `SessionLedger` 和 `ApprovalStore`** 到同一 SQLite 文件（`store.sqlite`）
4. **保留 `iota-kanban/event_sync.rs`**（有条件保留，无代码变更）
5. **为 `AdvancedBridge`** 添加 hermes 不可用时的降级处理

每项变更均独立可验证，互不依赖，可按任意顺序执行。

---

## Architecture

### 变更前后对比

**SQLite 文件布局（`~/.i6/context/`）**

| 文件 | 变更前 | 变更后 |
|------|--------|--------|
| `events.sqlite` | CacheStore + ObservabilityStore | 不变 |
| `memory.sqlite` | MemoryStore | 不变 |
| `sessions.sqlite` | SessionLedger | **删除** |
| `approvals.sqlite` | ApprovalStore | **删除** |
| `store.sqlite` | — | **新增**（SessionLedger + ApprovalStore 合并） |

**`iota-core/src/` 模块布局**

| 模块 | 变更前 | 变更后 |
|------|--------|--------|
| `storage/` | SupabaseStore、PipelineArtifact 等 | **整目录删除** |
| `engine/session_ledger.rs` | 4 个 `impl IotaEngine` 薄包装方法 | **删除，方法内联到 `engine/prompt.rs`** |
| `engine/prompt.rs` | 调用 `session_ledger.rs` 中的方法 | 直接包含这 4 个方法 |
| `config/paths.rs` | `sessions_db()` + `approvals_db()` | 替换为 `store_db()` |

**`iota-desktop/src-tauri/src/lib.rs`**

| 变更 | 说明 |
|------|------|
| 删除 `use iota_core::storage::SupabaseStore` | 移除 import |
| 删除 `sync_pipeline_artifacts` Tauri command | 移除函数体 + `invoke_handler` 注册 |
| 删除 `SyncPipelineResult` 结构体 | 仅被 `sync_pipeline_artifacts` 使用 |

**`iota-kanban/src/bridge.rs`**

| 变更 | 说明 |
|------|------|
| 新增 `is_available()` 检查 | 已存在，需确认行为符合规范 |
| `specify` / `decompose` 前置检查 | 若 `hermes_bin` 不存在，提前返回 `Err` |
| 新增 `ensure_bridge_available()` 函数 | 供 TUI kanban command handler 调用 |

---

## Components and Interfaces

### 1. 删除 `storage/` 模块

**涉及文件：**
- 删除：`crates/iota-core/src/storage/` 整目录（`mod.rs`、`models.rs`、`retry.rs`、`supabase.rs`、`SKILL.md`、`storage_tests.rs`）
- 修改：`crates/iota-core/src/lib.rs` — 删除 `pub mod storage;`
- 修改：`crates/iota-core/Cargo.toml` — 检查 `reqwest` 是否仍被其他模块使用

**`reqwest` 依赖分析：**

`reqwest` 在 `iota-core/Cargo.toml` 中声明为 workspace 依赖。删除 `storage/` 后，需检查以下文件是否仍使用 `reqwest`：
- `skill/cache.rs` — HTTP 拉取 skill 文件
- `memory/embedding.rs` — 向量化嵌入 HTTP 调用

若上述文件仍使用 `reqwest`，则 **保留** `iota-core/Cargo.toml` 中的 `reqwest` 条目。

**`iota-desktop` 变更：**

```rust
// 删除以下 import
use iota_core::storage::SupabaseStore;

// 删除以下结构体
pub struct SyncPipelineResult { ... }

// 删除以下 Tauri command 函数
#[tauri::command]
fn sync_pipeline_artifacts(base_path: Option<String>) -> Result<SyncPipelineResult, String> { ... }

// 从 invoke_handler 中移除
tauri::generate_handler![
    // ...
    sync_pipeline_artifacts  // ← 删除此行
]
```

---

### 2. 内联 `engine/session_ledger.rs`

**当前状态：**

`engine/session_ledger.rs` 包含 4 个 `pub(super) impl IotaEngine` 方法，全部被 `engine/prompt.rs` 调用：

| 方法 | 调用位置（`prompt.rs`） | 委托目标（`store/ledger.rs`） |
|------|------------------------|-------------------------------|
| `ensure_ledger_session` | `run()` 开头 | `ledger.ensure_session()` + `ledger.record_backend_session()` |
| `prepare_backend_handoff` | `run()` 开头 | `ledger.publish_handoff()` |
| `persist_backend_session_id` | `run()` ACP 成功路径 | `ledger.record_backend_session()` |
| `record_ledger_turn` | `run()` 成功/失败路径 + `finalize_local_turn()` | `ledger.record_turn()` |

**操作：**

1. 将 `engine/session_ledger.rs` 的全部内容（4 个方法）**剪切**到 `engine/prompt.rs` 末尾，作为同一 `impl IotaEngine` 块的一部分
2. 删除 `engine/session_ledger.rs` 文件
3. 从 `engine/mod.rs` 中删除 `mod session_ledger;` 声明（如存在）

**方法签名不变，行为不变。** 唯一变化是物理文件位置。

---

### 3. 合并 `SessionLedger` 和 `ApprovalStore` 到 `store.sqlite`

#### 3a. `StorePaths` 变更

```rust
// 变更前
impl StorePaths {
    pub fn events_db(&self) -> PathBuf { self.root.join("events.sqlite") }
    pub fn memory_db(&self) -> PathBuf { self.root.join("memory.sqlite") }
    pub fn sessions_db(&self) -> PathBuf { self.root.join("sessions.sqlite") }
    pub fn approvals_db(&self) -> PathBuf { self.root.join("approvals.sqlite") }
}

// 变更后
impl StorePaths {
    pub fn events_db(&self) -> PathBuf { self.root.join("events.sqlite") }
    pub fn memory_db(&self) -> PathBuf { self.root.join("memory.sqlite") }
    pub fn store_db(&self) -> PathBuf { self.root.join("store.sqlite") }
    // sessions_db() 和 approvals_db() 删除 — 编译器会找出所有残留调用
}
```

#### 3b. `SessionLedger::open` 变更

```rust
// 变更前
impl SessionLedger {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = super::db::open_db(path)?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS sessions ...")?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(StorePaths::resolve()?.sessions_db())
    }
}

// 变更后
impl SessionLedger {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = super::db::open_db(path)?;
        // 两个独立的 execute_batch，互不阻塞
        conn.execute_batch("CREATE TABLE IF NOT EXISTS sessions ...")?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(StorePaths::resolve()?.store_db())  // ← 改为 store_db()
    }
}
```

#### 3c. `ApprovalStore::open` 变更

```rust
// 变更前
impl ApprovalStore {
    pub fn default_path() -> Result<PathBuf> {
        Ok(StorePaths::resolve()?.approvals_db())
    }
}

// 变更后
impl ApprovalStore {
    pub fn default_path() -> Result<PathBuf> {
        Ok(StorePaths::resolve()?.store_db())  // ← 改为 store_db()
    }
}
```

#### 3d. 合并后的初始化顺序

`store.sqlite` 被 `SessionLedger` 和 `ApprovalStore` 共享。两者各自调用 `open_db(path)` 时，`db::open_db` 会应用 WAL pragma，然后各自执行自己的 `execute_batch` 建表语句。由于 SQLite 的 `CREATE TABLE IF NOT EXISTS` 是幂等的，两者并发初始化是安全的。

**初始化顺序（运行时）：**

```
SessionLedger::open(store_db_path)
  └─ db::open_db(path)          → WAL pragma applied
  └─ execute_batch(sessions DDL) → sessions, backend_sessions, turns, handoffs

ApprovalStore::open(store_db_path)
  └─ db::open_db(path)          → WAL pragma applied (idempotent)
  └─ execute_batch(approvals DDL) → approval_requests, approval_decisions
```

两个 `execute_batch` 调用独立，任一失败不影响另一个。

---

### 4. `AdvancedBridge` 降级处理

#### 4a. `is_available()` 行为规范

当前实现：

```rust
pub fn is_available(&self) -> bool {
    Command::new(&self.hermes_bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
```

此实现在 `hermes_bin` 不存在时会返回 `false`（spawn 失败 → `unwrap_or(false)`），行为已符合规范。

**需要补充：** 在 `specify` 和 `decompose` 方法开头添加前置检查：

```rust
pub fn specify(&self, task_id: TaskId, store: &dyn KanbanStore) -> Result<SpecifyResult> {
    // 新增：前置检查
    if !self.hermes_bin.exists() {
        anyhow::bail!(
            "hermes binary not found: {}",
            self.hermes_bin.display()
        );
    }
    // ... 原有逻辑
}

pub fn decompose(&self, task_id: TaskId, store: &dyn KanbanStore) -> Result<DecomposeResult> {
    // 新增：前置检查
    if !self.hermes_bin.exists() {
        anyhow::bail!(
            "hermes binary not found: {}",
            self.hermes_bin.display()
        );
    }
    // ... 原有逻辑
}
```

#### 4b. 新增 `ensure_bridge_available` 函数

```rust
/// Returns Ok(()) if the bridge is available, Err with a human-readable message otherwise.
pub fn ensure_bridge_available(bridge: &AdvancedBridge) -> Result<()> {
    if bridge.is_available() {
        Ok(())
    } else {
        anyhow::bail!(
            "hermes binary not found or not executable: {}",
            bridge.hermes_bin.display()
        )
    }
}
```

此函数放在 `bridge.rs` 中，作为模块级公开函数（非 `impl AdvancedBridge` 方法），供 TUI kanban command handler 调用。

#### 4c. TUI kanban command handler 变更

在 `iota-cli/src/tui/kanban_command.rs` 中，`/kanban specify` 和 `/kanban decompose` 的处理路径：

```rust
// 变更前（伪代码）
match bridge.specify(task_id, &store) {
    Ok(result) => { /* 显示结果 */ }
    Err(e) => { /* 可能 panic 或显示不友好错误 */ }
}

// 变更后
match ensure_bridge_available(&bridge) {
    Err(e) => {
        output_lines.push(e.to_string());
        return;  // 不 panic，直接返回
    }
    Ok(()) => {}
}
match bridge.specify(task_id, &store) {
    Ok(result) => { /* 显示结果 */ }
    Err(e) => {
        output_lines.push(e.to_string());
    }
}
```

---

### 5. 文档更新

#### 5a. `AGENTS.md` 源码结构部分

- 删除 `storage/` 条目及其描述行
- 将 `engine/` 子模块列表中的 `session_ledger.rs` 行删除

#### 5b. `docs/architecture.md`

- 删除所有提及 `SupabaseStore` 或 `storage/` 模块的段落/表格行

#### 5c. `store/SKILL.md`（如存在）

- 将 `sessions_db()` 和 `approvals_db()` 的路径描述替换为单一 `store_db()` 条目

---

## Data Models

### SQLite 文件映射（变更后）

```
~/.i6/context/
├── events.sqlite    — CacheStore (execution lifecycle) + ObservabilityStore
├── memory.sqlite    — MemoryStore (6-bucket memory system)
└── store.sqlite     — SessionLedger (sessions/turns/handoffs) + ApprovalStore (approval_requests/decisions)

~/.i6/kanban/
└── iota.db          — SqliteKanbanStore (tasks/boards/events/runs/comments/links)
```

### `store.sqlite` 表结构（合并后）

来自 `SessionLedger`：
```sql
CREATE TABLE IF NOT EXISTS sessions (
  iota_session_id TEXT PRIMARY KEY,
  cwd TEXT NOT NULL,
  active_backend TEXT,
  model TEXT,
  turn_count INTEGER DEFAULT 0,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS backend_sessions (
  iota_session_id TEXT NOT NULL,
  backend TEXT NOT NULL,
  backend_session_id TEXT,
  cwd TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL,
  PRIMARY KEY (iota_session_id, backend, cwd)
);
CREATE TABLE IF NOT EXISTS turns (
  turn_id TEXT PRIMARY KEY,
  iota_session_id TEXT NOT NULL,
  backend TEXT NOT NULL,
  execution_id TEXT,
  prompt_hash TEXT,
  output_summary TEXT,
  status TEXT,
  started_at INTEGER,
  finished_at INTEGER
);
CREATE TABLE IF NOT EXISTS handoffs (
  iota_session_id TEXT NOT NULL,
  from_backend TEXT,
  to_backend TEXT,
  cwd TEXT NOT NULL,
  summary TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
```

来自 `ApprovalStore`：
```sql
CREATE TABLE IF NOT EXISTS approval_requests (
  request_id TEXT PRIMARY KEY,
  execution_id TEXT,
  backend TEXT NOT NULL,
  tool_name TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS approval_decisions (
  request_id TEXT NOT NULL,
  approved INTEGER NOT NULL,
  reason TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(request_id) REFERENCES approval_requests(request_id)
);
CREATE INDEX IF NOT EXISTS idx_approval_decisions_order ON approval_decisions(request_id, created_at);
CREATE INDEX IF NOT EXISTS idx_approval_requests_execution ON approval_requests(execution_id, created_at);
```

---

## Error Handling

### `AdvancedBridge` 错误路径

| 场景 | 错误消息格式 | 来源 |
|------|-------------|------|
| `hermes_bin` 路径不存在 | `"hermes binary not found: <path>"` | `specify`/`decompose` 前置检查 |
| `is_available()` 返回 false | `"hermes binary not found or not executable: <path>"` | `ensure_bridge_available` |
| hermes 进程退出非零 | `"hermes kanban specify failed: <stderr>"` | 原有逻辑，不变 |
| hermes 超时 | `"hermes command timed out after 120000ms"` | 原有逻辑，不变 |

### SQLite 合并错误路径

| 场景 | 行为 |
|------|------|
| WAL pragma 失败 | `db::open_db` 返回 `Err`，调用方收到错误，不返回可用连接 |
| sessions DDL 失败 | `SessionLedger::open` 返回 `Err` |
| approvals DDL 失败 | `ApprovalStore::open` 返回 `Err` |
| 两者 DDL 互不影响 | 各自独立的 `execute_batch` 调用 |

---

## Migration Notes

### 用户升级指引（commit message 中包含）

```
MIGRATION: 升级前请删除旧的 SQLite 文件：
  rm ~/.i6/context/sessions.sqlite
  rm ~/.i6/context/approvals.sqlite

新版本将使用 ~/.i6/context/store.sqlite 统一存储 session 和 approval 数据。
旧文件不会被自动迁移，历史数据将丢失。
```

### `sync_pipeline_artifacts` 功能移除说明

该 Tauri command 用于将本地 JSON 文件上传到 Supabase，与工程核心职责（ACP 编排）无关。删除后，如需恢复该功能，可作为独立脚本实现，不应耦合到 `iota-core`。

---

## Testing Strategy

### 验证方法

每项变更完成后，执行以下验证：

| 验证命令 | 预期结果 |
|---------|---------|
| `cargo build --workspace` | exit code 0 |
| `cargo build -p iota-desktop` | exit code 0 |
| `cargo test --workspace` | exit code 0，无 `storage`/`sync_pipeline_artifacts`/`SupabaseStore`/`PipelineArtifact` 相关编译错误 |
| `grep -r "iota_core::storage" crates/` | 无输出 |
| `grep -r "sessions_db\|approvals_db" crates/` | 无输出 |
| `grep -r "session_ledger" crates/iota-core/src/engine/` | 仅 `prompt.rs` 中的方法定义，无 `mod session_ledger` |

### 单元测试变更

| 文件 | 操作 |
|------|------|
| `crates/iota-core/src/storage/storage_tests.rs` | 随目录一起删除 |
| `crates/iota-desktop/src-tauri/src/lib_tests.rs` | 删除引用 `sync_pipeline_artifacts` 的测试函数 |
| `crates/iota-core/src/store/ledger_tests.rs` | 更新 `default_path()` 断言：期望路径为 `store.sqlite` 而非 `sessions.sqlite` |
| `crates/iota-core/src/store/approvals_tests.rs` | 更新 `default_path()` 断言：期望路径为 `store.sqlite` 而非 `approvals.sqlite` |
| `crates/iota-kanban/src/bridge_tests.rs` | 新增测试：`hermes_bin` 不存在时 `specify`/`decompose` 返回含 `"hermes binary not found"` 的 `Err` |
