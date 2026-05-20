# iota-sympantos Runtime Architecture / 运行时架构

Selected GPT-Image2 template: `infographic-engine`

Generate a clean, professional, high-readability software architecture diagram for the current Rust implementation.

## Overall Layout / 总体布局

Use a wide horizontal canvas, preferably 21:9 or ultra-wide landscape.

Use a structured 4-layer architecture map:

1. Entry and interaction layer
2. Runtime orchestration layer
3. Integration and execution layer
4. Persistence and observability foundation

Use a white or very light warm-gray background, precise grid alignment, rounded rectangle modules, thin vector-style arrows, large readable typography, generous padding, and clear hierarchy. Use compact labels instead of paragraphs.

## Visual Style / 视觉风格

Use a modern technical infographic style:

- Clean SaaS / developer documentation aesthetic
- Flat vector design
- Subtle shadows only
- Soft pastel section colors
- Thin arrows with arrowheads
- Numbered circular sequence markers
- Bilingual English / Chinese labels
- Module titles in bold
- File paths in monospace
- Avoid decorative illustration
- Avoid 3D rendering
- Avoid cyberpunk or dark neon style

Color palette:

- Pink = CLI / TUI
- Orange = Daemon
- Blue = Engine
- Green = Context / Memory
- Cyan = ACP
- Teal = Backend
- Purple = Skill / MCP / Fn / Kanban
- Gray = Store / Telemetry

## Main Modules / 主模块内容

### 1. Entry / CLI / TUI

Files:

`src/main.rs`, `src/cli/mod.rs`, `src/cli/run_cmd.rs`, `src/cli/daemon_cmd.rs`, `src/cli/observability_cmd.rs`, `src/cli/kanban_cmd.rs`, `src/tui/mod.rs`, `src/tui/loop.rs`, `src/tui/render.rs`, `src/tui/input.rs`, `src/tui/scrollback.rs`, `src/tui/status_bar.rs`, `src/tui/events.rs`, `src/tui/terminal_lifecycle.rs`

Show:

- User input enters CLI prompt or TUI composer
- `main.rs → cli::run()`
- Default no-args path enters TUI
- Commands: `run`, `check`, `bench <cold|warm>`, `observability`, `logs`, `trace`, `mcp <context|fun>`, `context-mcp`, `fun-mcp`, `kanban`, `skill`, `__daemon`
- TUI native terminal scrollback, background engine task, streaming output
- Approval overlay, pager, help, quit confirmation
- Prompt queue while engine is running
- Slash commands and Kanban view

### 2. Daemon TCP Plane

Files:

`src/daemon/mod.rs`, `src/daemon/pool.rs`, `src/daemon/proto.rs`

Show:

- `iota run --daemon`
- Local TCP daemon: `127.0.0.1:47661`
- Override: `IOTA_DAEMON_ADDR`
- Auto-start through `current_exe __daemon`
- JSON-line request / response
- `EnginePool` reuses `IotaEngine` per cwd
- Connection concurrency limit
- Request size cap
- Graceful Ctrl+C shutdown

### 3. Engine Core

Files:

`src/engine/mod.rs`, `src/engine/prompt.rs`, `src/engine/memory_ops.rs`, `src/engine/session_ledger.rs`, `src/engine/telemetry.rs`, `src/runtime_event/mod.rs`

Show:

- Parse unique configuration from `~/.i6/nimia.yaml`
- `IotaEngine`
- ACP client pool keyed by backend and cwd
- Request hash replay and running execution join
- Session ledger and handoff
- Memory extraction and recall
- Skill match and optional engine-run MCP skill
- Context capsule composition
- ACP invocation
- CacheStore writeback
- Runtime telemetry recording

Normalized `RuntimeEvent`:

`Output`, `State`, `Log`, `ToolCall`, `ToolResult`, `Error`, `Extension`, `TokenUsage`, `Memory`, `ApprovalRequest`, `ApprovalDecision`

### 4. Context Fabric + Memory

Files:

`src/context/mod.rs`, `src/memory/store.rs`, `src/memory/embedding.rs`, `src/mcp/server.rs`

Show:

- `ContextEngine`
- `<iota-context>` capsule
- `WorkingMemoryBuffer`
- Workspace summary from `git status --short`
- Skill index and handoff summary
- Trivial prompt fast path
- Minimal capsule for short prompts
- Memory MCP tools
- Vector / hybrid search
- Ollama embeddings if configured
- Local fallback embedding

Six memory taxonomy buckets:

`identity`, `preference`, `strategic`, `domain`, `procedural`, `episodic`

### 5. ACP Adapter

Files:

`src/acp/mod.rs`, `src/acp/backend.rs`, `src/acp/client.rs`, `src/acp/stream_reader.rs`, `src/acp/wire.rs`, `src/acp/session.rs`, `src/acp/permission.rs`, `src/acp/message.rs`, `src/acp/types.rs`, `src/acp/parser.rs`

Show:

- `AcpClient` owns backend child process stdin/stdout
- JSON-RPC 2.0 newline-delimited protocol
- `initialize → session/new → session/prompt → session/update → session/request_permission → session/complete`
- Session id reuse
- `mcpServers` rendering
- Supports empty `mcpServers`
- Supports `string_array` and `object` env shapes
- ACP-side MCP tool call intercept through router
- Auto-approve `iota_*` and `mcp__iota-*`
- Otherwise route permission request to TUI overlay or stdin

### 6. Backend Processes

Show five backend rows:

1. Claude Code — `npx`, aliases `claude`, `claude-code`, `claudecode`
2. Codex — `npx`, alias `codex`
3. Gemini CLI — `npx`, aliases `gemini`, `gemini-cli`
4. Hermes Agent — `hermes acp`, alias `hermes`
5. OpenCode — `npx`, aliases `opencode`, `open-code`

Show:

- Credentials sourced from `~/.i6/nimia.yaml`
- Environment built via `backend_process_env_with_context()`
- Windows `npx` normalized to `npx.cmd`
- Do not show raw API keys
- Do not show `HERMES_HOME` override

### 7. Skill / MCP / Fn / Kanban

Files:

`src/skill/mod.rs`, `src/skill/runner.rs`, `src/skill/cache.rs`, `src/skill/fun.rs`, `src/mcp/client.rs`, `src/mcp/router.rs`, `src/mcp/tool_dispatch.rs`, `src/kanban/mod.rs`, `src/kanban/sqlite_store.rs`, `src/kanban/dispatcher.rs`, `src/kanban/worker.rs`, `src/kanban/bridge.rs`

Show:

- `SkillRegistry`
- Load roots: workspace `skills/`, workspace `.iota/skills`, configured skill roots, `~/.i6/skills`
- Frontmatter parsing and trigger matching
- Backend compatibility
- `SkillRunner`
- `execution.mode = mcp`
- Sequential or parallel MCP tool calls
- `iota-fun` MCP stdio server
- Seven Fn runners: `Python`, `TypeScript`, `Rust`, `Go`, `Java`, `C++`, `Zig`
- Kanban task board, event sourcing, dispatcher, Hermes worker, shadow materializer, event sync

### 8. Store / Telemetry / Observability

Bottom wide foundation band.

Files:

`src/store/cache.rs`, `src/store/observability.rs`, `src/store/approvals.rs`, `src/store/ledger.rs`, `src/telemetry/mod.rs`, `src/telemetry/metrics.rs`, `src/telemetry/stderr.rs`

Show store blocks:

- CacheStore: `~/.i6/context/events.sqlite`, replay, dedupe, request hash, running join, fencing token, retention
- ObservabilityStore: `~/.i6/context/events.sqlite`, token usage events, execution-level best-record dedupe, P50/P95/P99, time-window query, backend summary
- MemoryStore: `~/.i6/context/memory.sqlite` or `context_engine.memory_db`, six buckets, FTS/LIKE, vector/hybrid search, dedup, TTL, merge
- ApprovalStore: `~/.i6/context/approvals.sqlite`, request / decision recording, risk classification
- SessionLedger: `~/.i6/context/sessions.sqlite`, sessions, turns, handoff tracking
- Local logs: daily files under `~/.i6/logs/`, override `IOTA_LOG_DIR`
- OpenTelemetry: default endpoint `http://localhost:4317`, `OTEL_ENABLED=false`, traces, logs, metrics
- Docker Observability Stack: OTel Collector `4317 / 4318`, Loki `3100`, Jaeger `16686`, Prometheus `9090`, Grafana `3000`

## Flow Markers / 流程编号

Show numbered circular markers:

1. CLI / TUI entry
2. Command dispatch
3. Optional daemon route
4. EnginePool reuse
5. Engine request lifecycle
6. Skill registry load
7. Memory recall
8. Context capsule
9. ACP client ensure
10. ACP session protocol
11. Backend streaming update
12. Permission handling
13. MCP / skill / fn / kanban route
14. Cache / memory / ledger writeback
15. Observability and OTel export
16. TUI streaming render / approval overlay

## Negative Prompt / 避免内容

Avoid unreadable tiny text, random fake file paths, obsolete modules, `src/store/events.rs`, single `context.db`, Promtail, project-level config discovery, Hermes home override, excessive decorative art, messy arrows, 3D render, dark background, neon cyberpunk, stock cloud icons, blurry labels, incorrect backend names, Korean text, non-Chinese non-English labels, `telemetry/console.rs`, `context/server.rs`, `skill/sandbox_executor.rs`, `src/tui.rs` as a single-file TUI, and `src/memory/store.rs` shown as `src/store/memory.rs`.
