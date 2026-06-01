# Design Document: docs-and-skill-update

## Overview

本 feature 通过深入阅读 iota-sympantos Rust workspace 的全部源代码，对以下文档进行精确修正和补全：

- `crates/iota-core/src/storage/SKILL.md`（新建）
- `crates/iota-core/src/engine/SKILL.md`（修正类型名称）
- `AGENTS.md`（修正源码结构、TUI 表、CLI 命令列表）
- `docs/iota book.md`（修正 capsule section 顺序、补全 vector search 公式）
- `docs/architecture.md`（补全 storage/ 模块）
- `docs/code-call-chains.md`（修正 ACP 协议顺序描述）
- `docs/command.md`（补全 --trace-timing 选项）
- `docs/observability.md`（补全 token usage 字段）
- `gefsi/README.md`（验证实验文件列表）
- `README.md`（补全文档链接）

所有修改均以源码为唯一事实来源，不引入推测性内容。

## Architecture

本 feature 是纯文档修改，不涉及任何 Rust 源码变更。修改范围：

```
iota-sympantos/
├── crates/iota-core/src/
│   ├── storage/SKILL.md          ← 新建
│   └── engine/SKILL.md           ← 修正 ClientKey → AcpClientKey
├── docs/
│   ├── iota book.md              ← 修正 capsule 顺序、补全 vector search 公式
│   ├── architecture.md           ← 补全 storage/ 模块
│   ├── code-call-chains.md       ← 修正 ACP 协议顺序
│   ├── command.md                ← 补全 --trace-timing
│   └── observability.md          ← 补全 cache_tokens、total_tokens 字段
├── gefsi/README.md               ← 验证（已正确，无需修改）
├── AGENTS.md                     ← 修正源码结构、TUI 表、CLI 命令
└── README.md                     ← 补全 docker.md、desktop-mvp-acceptance.md 链接
```

## Components and Interfaces

### 1. storage/SKILL.md（新建）

**来源验证**：`crates/iota-core/src/storage/mod.rs`、`models.rs`、`supabase.rs`、`retry.rs`

关键事实（来自源码）：
- 公开类型：`SupabaseStore`、`PipelineArtifact`、`PipelineRecord`、`PipelineStatus`、`ResearchData`、`ScriptData`、`XOptimizerData`
- 子模块：`supabase`、`models`、`retry`
- 环境变量：`SUPABASE_URL` / `NIMIA_SUPABASE_URL`、`SUPABASE_ANON_KEY` / `NIMIA_SUPABASE_ANON_KEY`
- 重试策略：3 次重试，2 秒基础延迟，指数退避
- 与 `crates/iota-core/src/store/`（SQLite 层）完全独立

**SKILL.md 结构**（遵循 AGENTS.md 中的模块上下文规范）：
```yaml
---
name: iota-src-storage
description: Use when working on Supabase pipeline artifact persistence, SupabaseStore, PipelineArtifact, or files under crates/iota-core/src/storage.
triggers:
  - crates/iota-core/src/storage
  - SupabaseStore
  - PipelineArtifact
  - pipeline artifact
  - SUPABASE_URL
---
```

### 2. engine/SKILL.md（修正）

**来源验证**：`crates/iota-core/src/engine/mod.rs`

当前错误：`ClientKey` — `(AcpBackend, PathBuf)` key for client reuse

正确内容：`AcpClientKey` — `(AcpBackend, PathBuf)` key for ACP client pool reuse

同时需要更新 `triggers` 字段：将 `ClientKey` 改为 `AcpClientKey`。

### 3. AGENTS.md（多处修正）

**3a. 源码结构 — 补全 storage/ 模块**

在 `iota-core/src/` 下，`store/` 之后添加：
```
├── storage/           # Supabase pipeline artifact persistence (SupabaseStore, PipelineArtifact)
```

**3b. 源码结构 — 补全 acp/util.rs**

在 `acp/` 下添加：
```
│   ├── util.rs        # Helpers: elapsed_ms, should_forward_backend_stderr
```

**3c. 源码结构 — 补全 config/ 子文件**

在 `config/` 下添加（来自源码验证）：
```
│   ├── effective.rs   # EffectiveConfig — resolved config with defaults
│   ├── helpers.rs     # expand_home_path, normalize_command
│   └── paths.rs       # StorePaths — ~/.i6/context store path resolution
```

**3d. 测试文件列表 — 修正 engine 测试文件名**

当前：`engine_tests.rs`（不存在）
正确：`engine/tests.rs`（已验证存在于 `crates/iota-core/src/engine/tests.rs`）

**3e. 测试文件列表 — 补全 mcp/client_tests.rs**

在 iota-core 测试文件列表中添加 `mcp/client_tests.rs`（已验证存在）。

**3f. TUI 功能表 — 补全 /memory 命令**

在 TUI 功能表中添加：
```
| /memory（/mem）本地 memory recall / hybrid search | `tui/slash_command.rs` | ✅ |
```

**3g. TUI 功能表 — 补全 tui/events.rs**

需要验证 `tui/events.rs` 是否存在（`tui/events_tests.rs` 已在测试列表中）。

**3h. CLI 命令列表 — 补全 __bench_cache**

在 CLI 命令列表中添加：
```bash
iota __bench_cache          # 内部缓存 benchmark（3 轮 Claude Code 对话，输出 token 统计）
```

**3i. CLI 命令列表 — 补全 logs/trace 别名说明**

`iota logs <execution_id>` 和 `iota trace <trace_id>` 已在列表中，但需确认注释说明它们是 `iota observability` 的顶层别名。

### 4. docs/iota book.md（两处修正）

**4a. Context Fabric capsule section 顺序**

来源：`crates/iota-core/src/context/mod.rs` 中 `compose_effective_prompt()` 函数的实际执行顺序。

当前文档顺序（错误）：
```
memory-tools → model → skills → memory → session → handoff → working-memory → workspace
```

实际代码顺序（正确，与文档一致）：
经过仔细阅读 `context/mod.rs`，实际顺序为：
1. `memory-tools`（`push_memory_tools()`）
2. `model`（if model is set）
3. `skills`（if skills available）
4. `memory`（if memory available）
5. `session`
6. `handoff`（if handoff available）
7. `working-memory`（if non-empty）
8. `workspace`（if non-empty）

当前 iota book.md 中的 capsule 示例顺序为：
```
<memory-tools> → <model> → <skills> → <memory> → <session> → <handoff> → <working-memory> → <workspace>
```

这与代码实际顺序一致。但 requirements 指出文档描述与实际不一致，需要核实并修正文档中的描述文字（非代码块）。

同时需要补充：
- trivial prompt 条件：≤80 字符且不含 `iota_memory`/`remember`/`recall`/`skill` 关键词
- `memory-tools` section 包含 Kanban 工具提示（`iota_kanban_create_task`、`iota_kanban_ready_task`、`iota_kanban_list_tasks`）

**4b. Memory vector search 评分公式**

来源：`crates/iota-core/src/memory/store.rs` 中 `search_vector()` 函数：
```rust
let score = 0.65 * similarity + 0.20 * overlap + 0.15 * record.confidence;
```

来源：`search_hybrid()` 函数：
```rust
add_ranked_record(&mut ranking, index, record, 1.0);  // keyword
add_ranked_record(&mut ranking, index, record, 1.2);  // vector
```

需要在 Memory 系统章节补充：
- `search_vector()` 混合评分公式：`0.65 × cosine_similarity + 0.20 × token_overlap + 0.15 × confidence`
- `search_hybrid()` 权重：vector 结果权重 1.2×，keyword 结果权重 1.0×

### 5. docs/architecture.md（补全 storage/ 模块）

在核心模块表中，`store/` 行之后添加：
```
| `storage/` | Supabase pipeline artifact persistence（optional，独立于 SQLite store 层） |
```

在 Workspace 结构代码块中，`store/` 之后添加：
```
├── storage/              # Supabase REST API client for pipeline artifact persistence
```

同时在 config/ 模块描述中补充 `effective.rs`、`helpers.rs`、`paths.rs` 的职责说明。

### 6. docs/code-call-chains.md（修正 ACP 协议顺序）

来源：`crates/iota-core/src/acp/client.rs` 中 `execute()` 函数的实际流程。

当前文档中的 Protocol order 部分：
```
initialize
  -> session/new
  -> session/prompt
  -> session/update ...
  -> session/request_permission? ...
  -> session/complete
```

这与实际代码一致，但需要明确标注 `session/request_permission` 是可选的（`?` 已有，但需要补充文字说明）。

### 7. docs/command.md（补全 --trace-timing）

来源：requirements 中指出 `--trace-timing` 是 `--timing` 的别名，在实验报告中使用。

在 `iota run` 常用选项表中添加：
```
| `--trace-timing` | `--timing` 的别名，输出 route、spawn、init、prompt、total timing JSON |
```

### 8. docs/observability.md（补全 token usage 字段）

来源：`crates/iota-core/src/runtime_event/mod.rs` 中 `TokenUsageEvent` 结构体。

当前文档缺少的字段：
- `cache_tokens`：`cache_read_input_tokens` 的别名字段（在 `token_usage_from_value()` 中赋值）
- `total_tokens`：中间计算字段（`provider_reported_total_tokens` 或 `input + output + thinking` 的和）

来源：`normalized_total_tokens()` 函数的实际逻辑：
- Anthropic：`input + cache_read + cache_creation + output + thinking`
- OpenAI/Gemini/adapter：`provider_reported_total_tokens` 或 `input + output + thinking + tool_use_prompt`

### 9. gefsi/README.md（验证）

来源：requirements 事实 15 — "gefsi/ 目录中实际有 5 个实验文件（exp01-exp05），README.md 中的文档表格已正确列出。"

经验证，`gefsi/README.md` 已正确列出所有 5 个实验文件，无需修改。

### 10. README.md（补全文档链接）

当前文档表缺少：
- `docs/docker.md`（文件已存在）
- `docs/desktop-mvp-acceptance.md`（文件已存在）

需要在文档表中添加这两个条目。

## Data Models

本 feature 不涉及数据模型变更。所有修改均为文档内容更新。

关键数据结构（用于文档准确性验证）：

**TokenUsageEvent 字段（来自 runtime_event/mod.rs）**：
```rust
pub struct TokenUsageEvent {
    pub input_tokens: Option<u64>,
    pub cache_tokens: Option<u64>,              // alias for cache_read_input_tokens
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub thinking_tokens: Option<u64>,
    pub tool_use_prompt_tokens: Option<u64>,
    pub total_tokens: Option<u64>,              // intermediate computed field
    pub provider_reported_total_tokens: Option<u64>,
    pub normalized_total_tokens: Option<u64>,
    // ...
}
```

**DaemonClientMessage / DaemonServerMessage 新增类型（来自 daemon/proto.rs）**：
```rust
// DaemonClientMessage
Ping { seq: u64 }

// DaemonServerMessage
Pong { seq: u64 }

// 协议版本常量
DESKTOP_PROTOCOL_VERSION: u32 = 2
PROTOCOL_VERSION_MIN: u32 = 2
PROTOCOL_VERSION_MAX: u32 = 3
```

**AcpClientKey（来自 engine/mod.rs）**：
```rust
type AcpClientKey = (AcpBackend, PathBuf);
```

**storage 模块公开类型（来自 storage/mod.rs）**：
```rust
pub use models::{
    PipelineArtifact, PipelineRecord, PipelineStatus,
    ResearchData, ScriptData, XOptimizerData,
};
pub use supabase::SupabaseStore;
```

## Correctness Properties

本 feature 是纯文档修改，不涉及业务逻辑代码。文档修改的正确性通过以下方式验证：

1. **源码事实核对**：每处修改均有对应的源码位置作为事实来源（已在 Components and Interfaces 中标注）。
2. **内容存在性检查**：可以通过文本搜索验证关键内容是否存在于目标文件中。

由于本 feature 不包含可以进行属性测试的纯函数或业务逻辑，PBT 不适用。文档修改的验证策略见 Testing Strategy 章节。

## Error Handling

文档修改不涉及运行时错误处理。潜在风险：

1. **内容遗漏**：某个需要修改的位置被遗漏。缓解：逐条对照 requirements 中的 14 个需求进行检查。
2. **内容不准确**：文档描述与源码不符。缓解：所有修改均以源码为唯一事实来源，已在设计阶段完成源码阅读和验证。
3. **格式破坏**：修改破坏了 Markdown 表格或代码块格式。缓解：修改后进行格式检查。
4. **YAML frontmatter 语法错误**：新建的 SKILL.md 中 YAML 格式不合法。缓解：遵循现有 SKILL.md 的格式模板。

## Testing Strategy

本 feature 的测试策略以内容验证为主，不使用属性测试。

### 验证方法

**文件存在性检查**：
- `crates/iota-core/src/storage/SKILL.md` 必须存在

**内容存在性检查（grep 验证）**：

| 目标文件 | 必须包含的内容 |
| :--- | :--- |
| `storage/SKILL.md` | `SupabaseStore`、`PipelineArtifact`、`PipelineRecord`、`PipelineStatus`、`ResearchData`、`ScriptData`、`XOptimizerData`、`supabase`、`models`、`retry`、`SUPABASE_URL` |
| `engine/SKILL.md` | `AcpClientKey`（不含独立的 `ClientKey`） |
| `AGENTS.md` | `storage/`（在 iota-core/src/ 下）、`util.rs`（在 acp/ 下）、`effective.rs`、`helpers.rs`、`paths.rs`（在 config/ 下）、`mcp/client_tests.rs`、`engine/tests.rs`（不含 `engine_tests.rs`）、`/memory`（在 TUI 表中）、`__bench_cache` |
| `docs/iota book.md` | `0.65`、`cosine`（或 `similarity`）、`0.20`、`0.15`、`1.2`、`trivial`、`80`、`iota_kanban_create_task` |
| `docs/architecture.md` | `storage/`（在 iota-core 模块表中）、`effective.rs`、`helpers.rs`、`paths.rs` |
| `docs/code-call-chains.md` | `optional`（在 session/request_permission 附近） |
| `docs/command.md` | `--trace-timing` |
| `docs/observability.md` | `cache_tokens`、`total_tokens`、`Anthropic`（在 normalized_total_tokens 说明中） |
| `README.md` | `docker.md`、`desktop-mvp-acceptance.md` |

**YAML frontmatter 合法性**：
- `storage/SKILL.md` 的 frontmatter 必须包含 `name`、`description`、`triggers` 字段

**源码一致性**（人工核对）：
- `engine/SKILL.md` 中的 `AcpClientKey` 描述与 `engine/mod.rs` 中的实际类型定义一致
- `storage/SKILL.md` 中的职责描述与 `storage/mod.rs` 中的文档注释一致
- `docs/observability.md` 中的 `normalized_total_tokens` 计算公式与 `runtime_event/mod.rs` 中的 `normalized_total_tokens()` 函数一致

### 不适用 PBT 的原因

本 feature 的所有修改均为文档内容更新：
- 没有纯函数可以进行输入变化测试
- 没有业务逻辑需要验证普遍性质
- 正确性完全由"内容是否与源码事实一致"决定，这是一个人工核对 + 内容存在性检查的问题，而非属性测试问题
