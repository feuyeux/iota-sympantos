# Requirements Document

## Introduction

本 feature 的目标是：深入阅读 iota-sympantos 所有 crate 的源代码，掌握当前实现的真实状态，然后对以下文档进行完善、补全和修正：

- `docs/iota book.md`
- `gefsi/*.md`（所有实验报告）
- `docs/*.md`（所有文档）
- `AGENTS.md`
- `README.md`
- 源代码目录中的 `SKILL.md`（补全缺失的 SKILL.md，修正已有 SKILL.md 中的错误）

通过阅读源码，已确认以下关键事实：

1. `crates/iota-core/src/storage/` 目录存在（Supabase pipeline artifact 存储），但缺少 `SKILL.md`。
2. `crates/iota-core/src/engine/` 中的 `AcpClientKey` 实际命名为 `AcpClientKey`（不是 `ClientKey`），engine SKILL.md 中有误。
3. `context/mod.rs` 中 `<iota-context>` capsule 的 section 顺序与 iota book.md 中描述的顺序不完全一致（实际顺序：memory-tools → model → skills → memory → session → handoff → working-memory → workspace）。
4. `storage/` 模块是 Supabase REST API 客户端，用于 pipeline artifact 持久化，与 SQLite store 层完全独立，AGENTS.md 的源码结构中未列出该模块。
5. `config/` 模块中实际存在 `effective.rs`、`helpers.rs`、`paths.rs` 等文件，AGENTS.md 中未完整列出。
6. `acp/` 模块中实际存在 `util.rs` 文件，AGENTS.md 中未列出。
7. `engine/` 模块中实际存在 `tests.rs` 文件（不是 `engine_tests.rs`），AGENTS.md 中未列出。
8. `context/mod.rs` 中 `is_trivial_prompt()` 函数实现了 trivial prompt 快速路径，iota book.md 中有提及但未精确描述触发条件（≤80 字符且不含 memory/skill 关键词）。
9. `memory/store.rs` 中 `recall_buckets_with_thresholds()` 的 identity/preference 桶使用 `MemoryScope::User`，strategic/domain 桶使用 `MemoryScope::Project`，这与文档描述一致。
10. `daemon/proto.rs` 中 `DESKTOP_PROTOCOL_VERSION = 2`，`PROTOCOL_VERSION_MIN = 2`，`PROTOCOL_VERSION_MAX = 3`，文档中只提到 v2。
11. `daemon/desktop.rs` 中新增了 `Ping`/`Pong` 消息类型，文档中未列出。
12. `context/mod.rs` 中 `push_memory_tools()` 函数包含 Kanban 工具提示（`iota_kanban_create_task`、`iota_kanban_ready_task`、`iota_kanban_list_tasks`），文档中未提及这些 MCP 工具。
13. `skill/mod.rs` 中 `SkillCache` 使用 mtime 签名做缓存失效，而不是简单的内存缓存。
14. `memory/store.rs` 中 `search_vector()` 使用混合评分：`0.65 * similarity + 0.20 * overlap + 0.15 * confidence`，文档中未提及。
15. `gefsi/` 目录中实际有 5 个实验文件（exp01-exp05），README.md 中的文档表格已正确列出。

---

## Glossary

- **SKILL.md**：放在源码模块目录中的 AI coding 工具上下文文件，包含 YAML frontmatter（name/description/triggers）和模块职责描述。
- **iota book**：`docs/iota book.md`，面向程序员和 AI 从业者的系统化技术指南。
- **gefsi**：实验报告目录，保存各功能的验证记录。
- **AcpClientKey**：`crates/iota-core/src/engine/mod.rs` 中的 `(AcpBackend, PathBuf)` 复合键，用于 ACP client pool 的复用。
- **storage 模块**：`crates/iota-core/src/storage/`，Supabase REST API 客户端，用于 pipeline artifact 持久化，与 SQLite store 层独立。
- **trivial prompt**：长度 ≤80 字符且不含 `iota_memory`/`remember`/`recall`/`skill` 关键词的 prompt，走 minimal capsule 快速路径。
- **Desktop Protocol**：daemon 与 Tauri desktop 之间的 JSON-line 流式协议，当前版本范围 `[2, 3]`。

---

## Requirements

### Requirement 1:补全 storage 模块的 SKILL.md

**User Story:** As an AI coding tool, I want a SKILL.md in `crates/iota-core/src/storage/`, so that I can quickly understand the Supabase pipeline artifact storage module without reading all implementation files.

#### Acceptance Criteria

1. THE SKILL.md SHALL be created at `crates/iota-core/src/storage/SKILL.md`.
2. THE SKILL.md SHALL include a valid YAML frontmatter with `name`, `description`, and `triggers` fields.
3. THE SKILL.md SHALL accurately describe the Supabase REST API client responsibilities: pipeline artifact persistence, exponential-backoff retry, and environment variable configuration (`SUPABASE_URL`/`SUPABASE_ANON_KEY` or `NIMIA_SUPABASE_*` aliases).
4. THE SKILL.md SHALL list the key types: `SupabaseStore`, `PipelineArtifact`, `PipelineRecord`, `PipelineStatus`, `ResearchData`, `ScriptData`, `XOptimizerData`.
5. THE SKILL.md SHALL list the sub-modules: `supabase`, `models`, `retry`.
6. THE SKILL.md SHALL note that this module is independent from the SQLite store layer under `crates/iota-core/src/store/`.

---

### Requirement 2:修正 engine SKILL.md 中的类型名称错误

**User Story:** As an AI coding tool, I want the engine SKILL.md to accurately reflect the actual type names in the source code, so that I can reference the correct identifiers when working on engine-related code.

#### Acceptance Criteria

1. WHEN the engine SKILL.md lists key types, THE SKILL.md SHALL use `AcpClientKey` (not `ClientKey`) as the name for the `(AcpBackend, PathBuf)` composite key.
2. THE SKILL.md SHALL accurately describe `AcpClientKey` as the key for ACP client pool reuse, keyed by `(AcpBackend, PathBuf)`.

---

### Requirement 3:修正 AGENTS.md 中的源码结构描述

**User Story:** As a developer, I want AGENTS.md to accurately list all source files and modules, so that I can navigate the codebase without confusion.

#### Acceptance Criteria

1. THE AGENTS.md SHALL add `storage/` module entry under `iota-core/src/` with description: `Supabase pipeline artifact persistence (SupabaseStore, PipelineArtifact)`.
2. THE AGENTS.md SHALL add `util.rs` under `iota-core/src/acp/` with description: `Helpers: elapsed_ms, should_forward_backend_stderr`.
3. THE AGENTS.md SHALL add `effective.rs`, `helpers.rs`, `paths.rs` under `iota-core/src/config/` with accurate descriptions.
4. THE AGENTS.md SHALL correct the engine test file reference from `engine_tests.rs` to `tests.rs` in the unit test file list.
5. THE AGENTS.md SHALL add `mcp/client_tests.rs` to the iota-core test file list (it exists in the actual codebase).

---

### Requirement 4:修正 iota book.md 中的 context capsule section 顺序

**User Story:** As a developer, I want the iota book to accurately describe the `<iota-context>` capsule section order, so that I understand the actual prompt structure sent to backends.

#### Acceptance Criteria

1. WHEN the iota book describes the `<iota-context>` capsule structure, THE iota book SHALL list sections in the actual implementation order: `memory-tools` → `model` → `skills` → `memory` → `session` → `handoff` → `working-memory` → `workspace`.
2. THE iota book SHALL note that trivial prompts (≤80 characters, no memory/skill keywords) use a minimal capsule that skips memory, skills, and workspace sections.
3. THE iota book SHALL mention that `memory-tools` section includes Kanban tool guidance (`iota_kanban_create_task`, `iota_kanban_ready_task`, `iota_kanban_list_tasks`) when MCP tools are available.

---

### Requirement 5:补全 daemon 协议文档中的 Ping/Pong 消息

**User Story:** As a developer, I want the documentation to accurately list all daemon protocol message types, so that I can implement or debug desktop-daemon communication correctly.

#### Acceptance Criteria

1. WHEN the iota book or architecture.md lists Desktop Protocol messages, THE documentation SHALL include `Ping { seq }` in `DaemonClientMessage` and `Pong { seq }` in `DaemonServerMessage`.
2. THE documentation SHALL note that the Desktop Protocol version range is `[2, 3]` (PROTOCOL_VERSION_MIN=2, PROTOCOL_VERSION_MAX=3), not just version 2.

---

### Requirement 6:修正 code-call-chains.md 中的 ACP 协议顺序描述

**User Story:** As a developer, I want the code call chains document to accurately reflect the ACP protocol flow, so that I can trace the actual execution path.

#### Acceptance Criteria

1. WHEN code-call-chains.md describes the ACP protocol order, THE document SHALL accurately reflect the actual implementation: `initialize → session/new → session/prompt → session/update ... → session/request_permission? ... → session/complete`.
2. THE document SHALL note that `session/request_permission` is optional and only occurs when the backend requests tool permission.

---

### Requirement 7:补全 gefsi/README.md 中的实验文件描述

**User Story:** As a developer, I want the gefsi README to accurately describe all experiment files, so that I can quickly find the relevant experiment report.

#### Acceptance Criteria

1. THE gefsi/README.md SHALL list all 5 experiment files (exp01 through exp05) with accurate topic descriptions.
2. WHEN the gefsi/README.md references current documentation, THE README SHALL include links to current docs files; links may reference the best-known current path even if the maintainer cannot guarantee all paths are up-to-date.

---

### Requirement 8:修正 README.md 中的文档链接和描述

**User Story:** As a developer, I want the README to accurately reflect the current documentation structure, so that I can navigate to the right docs.

#### Acceptance Criteria

1. THE README.md SHALL list `docs/docker.md` in the documentation table (it exists but is not listed).
2. THE README.md SHALL list `docs/desktop-mvp-acceptance.md` in the documentation table (it exists but is not listed).
3. THE README.md SHALL accurately describe the build command (the current README uses `cargo build -p iota-cli -p iota-core -p iota-kanban` which is correct).

---

### Requirement 9:修正 docs/command.md 中的 iota run 选项描述

**User Story:** As a developer, I want the command reference to accurately list all available options for `iota run`, so that I can use the CLI correctly.

#### Acceptance Criteria

1. WHEN docs/command.md lists `iota run` options, THE document SHALL include `--trace-timing` as an alias for `--timing` (used in experiment reports).
2. THE document SHALL accurately describe that `--backend <name>` and positional backend name are both supported.

---

### Requirement 10:修正 docs/observability.md 中的 token usage 字段描述

**User Story:** As a developer, I want the observability documentation to accurately describe all token usage fields, so that I can correctly interpret observability data.

#### Acceptance Criteria

1. WHEN docs/observability.md lists `RuntimeEvent::TokenUsage` fields, THE document SHALL include `cache_tokens` (alias for `cache_read_input_tokens`) and `total_tokens` (intermediate computed field) in addition to the currently listed fields.
2. THE document SHALL note that `normalized_total_tokens` uses provider-specific calculation: for Anthropic it sums input + cache_read + cache_creation + output + thinking; for OpenAI/Gemini/adapter it uses `provider_reported_total_tokens` or input + output + thinking.

---

### Requirement 11:修正 docs/architecture.md 中的模块描述

**User Story:** As a developer, I want the architecture document to accurately describe all modules and their responsibilities, so that I can understand the system design.

#### Acceptance Criteria

1. WHEN docs/architecture.md lists `iota-core` modules, THE document SHALL include `storage/` with description: `Supabase pipeline artifact persistence (optional, independent from SQLite store layer)`.
2. THE document SHALL note that `config/` module includes `effective.rs` (resolved config with defaults), `helpers.rs` (path expansion, command normalization), and `paths.rs` (store path resolution).

---

### Requirement 12:验证并修正 AGENTS.md 中的 TUI 功能表

**User Story:** As a developer, I want the AGENTS.md TUI feature table to accurately reflect the current implementation, so that I can understand what TUI features are available.

#### Acceptance Criteria

1. WHEN AGENTS.md lists TUI features, THE document SHALL include `/memory` (alias `/mem`) slash command for local memory recall and hybrid search (it is implemented in `tui/SKILL.md` but not in the AGENTS.md TUI table).
2. THE document SHALL accurately reflect that `tui/events.rs` exists (it is listed in the test files but not in the source structure).

---

### Requirement 13:补全 docs/iota book.md 中的 vector search 评分公式

**User Story:** As a developer, I want the iota book to accurately describe the memory vector search scoring formula, so that I can understand how semantic search results are ranked.

#### Acceptance Criteria

1. WHEN the iota book describes memory search, THE document SHALL mention the hybrid scoring formula used in `search_vector()`: `0.65 * cosine_similarity + 0.20 * token_overlap + 0.15 * confidence`.
2. THE document SHALL note that hybrid search (`search_hybrid()`) combines keyword and vector results with vector results weighted 1.2x vs keyword results at 1.0x; this hybrid search weighting documentation may stand alone without requiring the full memory search scoring formula context.

---

### Requirement 14:修正 AGENTS.md 中的 CLI 命令列表

**User Story:** As a developer, I want AGENTS.md to accurately list all CLI commands, so that I can use the CLI without referring to the source code.

#### Acceptance Criteria

1. WHEN AGENTS.md lists CLI commands, THE document SHALL include `iota bench <cold|warm> [轮次] [--daemon]` with the correct syntax (the current list has `iota bench <cold|warm> [轮次] [--daemon]` which is correct).
2. THE document SHALL include `iota __bench_cache` as an internal benchmark command (it exists in `cli/mod.rs`).
3. THE document SHALL note that `iota logs <execution_id>` and `iota trace <trace_id>` are top-level aliases for observability commands.
