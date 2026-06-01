# Implementation Plan: docs-and-skill-update

## Overview

Pure documentation update: create one new SKILL.md, fix one existing SKILL.md, and update seven documentation files. All changes are text edits to Markdown files — no Rust source code is modified. Each task maps directly to one or more requirements and uses the source code as the sole source of truth.

## Tasks

- [x] 1. Create `crates/iota-core/src/storage/SKILL.md`
  - [x] 1.1 Write the new SKILL.md file for the storage module
    - Create the file at `crates/iota-core/src/storage/SKILL.md` with valid YAML frontmatter (`name`, `description`, `triggers`).
    - Set `name: iota-src-storage`, `description` referencing Supabase pipeline artifact persistence, and `triggers` including `crates/iota-core/src/storage`, `SupabaseStore`, `PipelineArtifact`, `pipeline artifact`, `SUPABASE_URL`.
    - Document Responsibilities: Supabase REST API client, pipeline artifact persistence, exponential-backoff retry (3 retries, 2 s base delay), environment variable configuration (`SUPABASE_URL`/`NIMIA_SUPABASE_URL`, `SUPABASE_ANON_KEY`/`NIMIA_SUPABASE_ANON_KEY`).
    - List Sub-modules table: `supabase`, `models`, `retry`.
    - List Key Types: `SupabaseStore`, `PipelineArtifact`, `PipelineRecord`, `PipelineStatus`, `ResearchData`, `ScriptData`, `XOptimizerData`.
    - Add a note that this module is independent from the SQLite store layer under `crates/iota-core/src/store/`.
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

- [x] 2. Fix `crates/iota-core/src/engine/SKILL.md`
  - [x] 2.1 Replace `ClientKey` with `AcpClientKey` in the Key Types section
    - Change the line `- \`ClientKey\` — \`(AcpBackend, PathBuf)\` key for client reuse` to `- \`AcpClientKey\` — \`(AcpBackend, PathBuf)\` key for ACP client pool reuse`.
    - _Requirements: 2.1, 2.2_
  - [x] 2.2 Update `triggers` frontmatter field
    - Replace `- ClientKey` with `- AcpClientKey` in the `triggers` list.
    - _Requirements: 2.1_

- [x] 3. Fix `AGENTS.md` — source structure, test file list, TUI table, CLI commands
  - [x] 3.1 Add `storage/` module entry under `iota-core/src/`
    - In the source tree code block, after the `store/` block, add a `storage/` entry with description `Supabase pipeline artifact persistence (SupabaseStore, PipelineArtifact)`.
    - _Requirements: 3.1_
  - [x] 3.2 Add `util.rs` under `iota-core/src/acp/`
    - In the `acp/` block, add `│   ├── util.rs        # Helpers: elapsed_ms, should_forward_backend_stderr`.
    - _Requirements: 3.2_
  - [x] 3.3 Add `effective.rs`, `helpers.rs`, `paths.rs` under `iota-core/src/config/`
    - In the `config/` block, add the three files with descriptions: `effective.rs` — `EffectiveConfig — resolved config with defaults`; `helpers.rs` — `expand_home_path, normalize_command`; `paths.rs` — `StorePaths — ~/.i6/context store path resolution`.
    - _Requirements: 3.3_
  - [x] 3.4 Correct engine test file reference in the unit test list
    - In the iota-core test file list, replace `engine_tests.rs` (or any reference to it) with `engine/tests.rs`.
    - _Requirements: 3.4_
  - [x] 3.5 Add `mcp/client_tests.rs` to the iota-core test file list
    - Add `mcp/client_tests.rs` to the iota-core section of the unit test file list.
    - _Requirements: 3.5_
  - [x] 3.6 Add `/memory` slash command to the TUI feature table
    - In the "TUI 功能（已完成）" table, add a row: `/memory`（`/mem`）本地 memory recall / hybrid search | `tui/slash_command.rs` | ✅.
    - _Requirements: 12.1_
  - [x] 3.7 Add `iota __bench_cache` to the CLI command list
    - In the CLI 命令 code block, add `iota __bench_cache          # 内部缓存 benchmark（3 轮 Claude Code 对话，输出 token 统计）`.
    - _Requirements: 14.2_

- [x] 4. Fix `docs/iota book.md` — capsule section order, trivial prompt, Kanban tools, vector search formula
  - [x] 4.1 Correct the `<iota-context>` capsule section order description
    - In the Context Fabric chapter, update the capsule XML example and any surrounding prose to reflect the actual implementation order: `memory-tools` → `model` → `skills` → `memory` → `session` → `handoff` → `working-memory` → `workspace`.
    - _Requirements: 4.1_
  - [x] 4.2 Document trivial prompt conditions
    - In the Context Fabric chapter, update the trivial prompt description to state the exact conditions: ≤80 characters and does not contain `iota_memory`, `remember`, `recall`, or `skill` keywords; these prompts use a minimal capsule that skips memory, skills, and workspace sections.
    - _Requirements: 4.2_
  - [x] 4.3 Document Kanban tool guidance in `memory-tools` section
    - In the Context Fabric chapter, note that the `memory-tools` section includes Kanban tool guidance (`iota_kanban_create_task`, `iota_kanban_ready_task`, `iota_kanban_list_tasks`) when MCP tools are available.
    - _Requirements: 4.3_
  - [x] 4.4 Add vector search hybrid scoring formula
    - In the Memory 系统 chapter, add the `search_vector()` hybrid scoring formula: `0.65 × cosine_similarity + 0.20 × token_overlap + 0.15 × confidence`.
    - _Requirements: 13.1_
  - [x] 4.5 Add hybrid search weighting note
    - In the Memory 系统 chapter, note that `search_hybrid()` combines keyword and vector results with vector results weighted 1.2× vs keyword results at 1.0×.
    - _Requirements: 13.2_

- [x] 5. Fix `docs/architecture.md` — add `storage/` module and `config/` sub-files
  - [x] 5.1 Add `storage/` to the Workspace structure code block
    - In the `crates/iota-core/src/` tree, after `store/`, add `├── storage/              # Supabase REST API client for pipeline artifact persistence`.
    - _Requirements: 11.1_
  - [x] 5.2 Add `storage/` to the 核心模块 table
    - In the 核心模块 table, after the `store/` row, add `| \`storage/\` | Supabase pipeline artifact persistence（optional，独立于 SQLite store 层） |`.
    - _Requirements: 11.1_
  - [x] 5.3 Document `config/` sub-files `effective.rs`, `helpers.rs`, `paths.rs`
    - In the 核心模块 table or the config/ module description, note that `config/` includes `effective.rs` (resolved config with defaults), `helpers.rs` (path expansion, command normalization), and `paths.rs` (store path resolution).
    - _Requirements: 11.2_

- [x] 6. Fix `docs/code-call-chains.md` — clarify `session/request_permission` is optional
  - [x] 6.1 Add prose note about optional `session/request_permission`
    - In the "Protocol order" section of 链路 2, add a prose note below the protocol sequence stating that `session/request_permission` is optional and only occurs when the backend requests tool permission.
    - _Requirements: 6.1, 6.2_

- [x] 7. Fix `docs/command.md` — add `--trace-timing` option
  - [x] 7.1 Add `--trace-timing` row to the `iota run` options table
    - In the `iota run` 常用选项 table, add a row: `\`--trace-timing\`` | `--timing` 的别名，输出 route、spawn、init、prompt、total timing JSON.
    - _Requirements: 9.1_

- [x] 8. Fix `docs/observability.md` — add `cache_tokens`, `total_tokens`, and `normalized_total_tokens` formula
  - [x] 8.1 Add `cache_tokens` and `total_tokens` fields to the Token Usage table
    - In the Token Usage 字段 table, add rows for `cache_tokens` (`cache_read_input_tokens` 的别名字段) and `total_tokens` (中间计算字段：`provider_reported_total_tokens` 或 `input + output + thinking` 的和).
    - _Requirements: 10.1_
  - [x] 8.2 Document `normalized_total_tokens` provider-specific calculation
    - Below the Token Usage table, add a note explaining the provider-specific calculation: for Anthropic it sums `input + cache_read + cache_creation + output + thinking`; for OpenAI/Gemini/adapter it uses `provider_reported_total_tokens` or `input + output + thinking + tool_use_prompt`.
    - _Requirements: 10.2_

- [x] 9. Fix `README.md` — add `docker.md` and `desktop-mvp-acceptance.md` links
  - [x] 9.1 Add `docker.md` and `desktop-mvp-acceptance.md` to the documentation table
    - In the 文档 table, add rows for `docs/docker.md` (Docker 与外部观测栈) and `docs/desktop-mvp-acceptance.md` (Desktop MVP 验收标准).
    - _Requirements: 8.1, 8.2_

- [x] 10. Checkpoint — verify all documentation changes
  - Ensure all modified files are syntactically valid Markdown (no broken tables, unclosed code blocks, or malformed YAML frontmatter).
  - Verify that `crates/iota-core/src/storage/SKILL.md` exists and contains all required key types and sub-modules.
  - Verify that `engine/SKILL.md` no longer contains a standalone `ClientKey` entry and now uses `AcpClientKey`.
  - Ask the user if any questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP (none in this feature — all tasks are required documentation edits).
- No Rust source code is modified; all tasks are Markdown file edits.
- Each task references specific requirements for traceability.
- The design document has no Correctness Properties section (pure documentation feature), so no property-based test sub-tasks are included.
- `gefsi/README.md` is already correct per Requirement 7 and requires no changes.
- Requirement 12.2 (`tui/events.rs` source file) is already listed in the test file list as `tui/events_tests.rs`; the source file `tui/events.rs` is implied by the test file and does not require a separate AGENTS.md change beyond what task 3 covers.

## Task Dependency Graph

All documentation edits are independent of each other — no task writes to the same file as another task in the same wave, and none depend on the output of another. All leaf sub-tasks can run in a single wave.

```json
{
  "waves": [
    {
      "id": 0,
      "tasks": [
        "1.1",
        "2.1", "2.2",
        "3.1", "3.2", "3.3", "3.4", "3.5", "3.6", "3.7",
        "4.1", "4.2", "4.3", "4.4", "4.5",
        "5.1", "5.2", "5.3",
        "6.1",
        "7.1",
        "8.1", "8.2",
        "9.1"
      ]
    }
  ]
}
```
