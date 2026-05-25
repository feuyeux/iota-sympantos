# iota-sympantos Runtime Architecture / 运行时架构

Selected GPT-Image2 template: `infographic-engine`

Generate a clean, professional, high-readability software architecture diagram for the current Rust implementation.

## Overall Layout / 总体布局

Use a wide horizontal canvas, preferably 21:9 or ultra-wide landscape.

Use a structured 4-layer architecture map coordinating the four Cargo workspace crates (`iota-cli`, `iota-core`, `iota-kanban`, and `iota-desktop`):

1. Entry and Interaction Layer (`iota-cli` TUI and `iota-desktop` Tauri GUI)
2. Runtime Orchestration Layer (`iota-core` TCP daemon and core engines)
3. Integration, Execution, and Kanban Planning Layer (`iota-core` integration, `iota-kanban` engines, and desktop local database CRUD)
4. Persistence and Telemetry Foundation (`iota-core` store and telemetry systems)

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

- Pink = CLI / TUI (inside `iota-cli`)
- Violet = Desktop GUI (inside `iota-desktop` React & Tauri UI)
- Orange = Daemon (inside `iota-core`)
- Blue = Engine Core (inside `iota-core`)
- Green = Context / Memory (inside `iota-core`)
- Cyan = ACP Adapter (inside `iota-core`)
- Teal = Backend Processes
- Purple = Skill / MCP / Fn / Kanban (inside `iota-core` and `iota-kanban`)
- Gray = Store / Telemetry (inside `iota-core` foundation)

## Main Modules / 主模块内容

### 1. Presentation & Interaction (iota-cli & iota-desktop crates)

Files:

- CLI/TUI: `crates/iota-cli/src/main.rs`, `crates/iota-cli/src/cli/mod.rs`, `crates/iota-cli/src/cli/observability_cmd.rs`, `crates/iota-cli/src/tui/mod.rs`, `crates/iota-cli/src/tui/loop.rs`, `crates/iota-cli/src/tui/render.rs`, `crates/iota-cli/src/tui/input.rs`, `crates/iota-cli/src/tui/scrollback.rs`, `crates/iota-cli/src/tui/status_bar.rs`, `crates/iota-cli/src/tui/events.rs`, `crates/iota-cli/src/tui/terminal_lifecycle.rs`
- Desktop: `crates/iota-desktop/src/App.tsx`, `crates/iota-desktop/src/components/ChatWorkbench.tsx`, `crates/iota-desktop/src/components/RightInspector.tsx`, `crates/iota-desktop/src-tauri/src/main.rs`, `crates/iota-desktop/src-tauri/src/lib.rs`, `crates/iota-desktop/src-tauri/src/daemon_client.rs`

Show:

- CLI/TUI: User input, `cli::run()` command routing, TUI prompt queue, background engine task, streaming output, approval overlay, and native scrollback.
- Desktop GUI: Tauri desktop app running React frontend. Chat workbench with resizable split layout (dragging splitter) containing a main chat area (Hermes default backend) and a right inspector panel (RightInspector) housing tabs for observability summary/percentiles, memory context snapshot, and active tool approvals.
- Desktop IPC: `daemon_client.rs` connecting to daemon at `127.0.0.1:47661` for config snapshot, prompt submission, approval responses, and turn cancellation.

### 2. Daemon TCP Plane (iota-core crate)

Files:

`crates/iota-core/src/daemon/mod.rs`, `crates/iota-core/src/daemon/pool.rs`, `crates/iota-core/src/daemon/proto.rs`

Show:

- `iota run --daemon`
- Local TCP daemon: `127.0.0.1:47661`
- Override: `IOTA_DAEMON_ADDR`
- Auto-start through `current_exe __daemon`
- JSON-line request / response
- `EnginePool` reuses `IotaEngine` per cwd
- Connection concurrency limit and graceful Ctrl+C shutdown

### 3. Engine Core (iota-core crate)

Files:

`crates/iota-core/src/engine/mod.rs`, `crates/iota-core/src/engine/prompt.rs`, `crates/iota-core/src/engine/memory_ops.rs`, `crates/iota-core/src/engine/session_ledger.rs`, `crates/iota-core/src/engine/telemetry.rs`, `crates/iota-core/src/runtime_event.rs`

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

### 4. Context Fabric + Memory (iota-core crate)

Files:

`crates/iota-core/src/context/mod.rs`, `crates/iota-core/src/memory/store.rs`, `crates/iota-core/src/memory/embedding.rs`, `crates/iota-core/src/mcp/server.rs`

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

### 5. ACP Adapter (iota-core crate)

Files:

`crates/iota-core/src/acp/mod.rs`, `crates/iota-core/src/acp/backend.rs`, `crates/iota-core/src/acp/client.rs`, `crates/iota-core/src/acp/stream_reader.rs`, `crates/iota-core/src/acp/wire.rs`, `crates/iota-core/src/acp/session.rs`, `crates/iota-core/src/acp/permission.rs`, `crates/iota-core/src/acp/message.rs`, `crates/iota-core/src/acp/types.rs`, `crates/iota-core/src/acp/parser.rs`, `crates/iota-core/src/acp/util.rs`

Show:

- `AcpClient` owns backend child process stdin/stdout
- JSON-RPC 2.0 newline-delimited protocol
- `initialize → session/new → session/prompt → session/update → session/request_permission → session/complete`
- Session id reuse and `mcpServers` rendering
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

### 7. Skill / MCP / Fn / Kanban (iota-core & iota-kanban crates)

Files:

`crates/iota-core/src/skill/mod.rs`, `crates/iota-core/src/skill/runner.rs`, `crates/iota-core/src/skill/cache.rs`, `crates/iota-core/src/skill/fun.rs`, `crates/iota-core/src/mcp/client.rs`, `crates/iota-core/src/mcp/router.rs`, `crates/iota-core/src/mcp/tool_dispatch.rs`

`crates/iota-kanban/src/lib.rs`, `crates/iota-kanban/src/types.rs`, `crates/iota-kanban/src/store.rs`, `crates/iota-kanban/src/sqlite_store.rs`, `crates/iota-kanban/src/state_machine.rs`, `crates/iota-kanban/src/event_sourcing.rs`, `crates/iota-kanban/src/dispatcher.rs`, `crates/iota-kanban/src/worker.rs`, `crates/iota-kanban/src/shadow.rs`, `crates/iota-kanban/src/bridge.rs`, `crates/iota-kanban/src/event_sync.rs`

Show:

- `SkillRegistry` loading from roots: `skills/`, `.iota/skills`, configured skill roots, `~/.i6/skills`
- `SkillRunner` execution (mcp mode, sequential/parallel)
- `iota-fun` 7 language stdio MCP server (Python, TypeScript, Rust, Go, Java, C++, Zig)
- Kanban Engine: event-sourced SqliteKanbanStore, triage→todo→ready→running→done→archived state machine, Dispatcher, WorkerHandle (spawns hermes -z), ShadowMaterializer/Watcher, AdvancedBridge (decompose/specify), cross-node EventSync (export/import/serve/pull/push)
- Desktop integration: Tauri command handlers (`list_boards`, `list_tasks`, `create_task`, `transition_task`) calling `SqliteKanbanStore` methods directly in Rust backend to synchronize React board views.

### 8. Store / Telemetry / Observability (iota-core crate foundation)

Bottom wide foundation band.

Files:

`crates/iota-core/src/store/mod.rs`, `crates/iota-core/src/store/cache.rs`, `crates/iota-core/src/store/observability.rs`, `crates/iota-core/src/store/approvals.rs`, `crates/iota-core/src/store/ledger.rs`, `crates/iota-core/src/store/db.rs`, `crates/iota-core/src/telemetry/mod.rs`, `crates/iota-core/src/telemetry/metrics.rs`, `crates/iota-core/src/telemetry/stderr.rs`

Show store blocks:

- CacheStore: `~/.i6/context/events.sqlite`, replay, dedupe, request hash, running join, fencing token, retention
- ObservabilityStore: token usage events, execution-level best-record dedupe, P50/P95/P99, time-window query, backend summary
- MemoryStore: `~/.i6/context/memory.sqlite` (six taxonomy buckets, FTS/LIKE, vector/hybrid, dedup, merge)
- ApprovalStore: approvals logging & policy
- SessionLedger: sessions/turns/handoff tracking
- Local daily logs under `~/.i6/logs/` (override `IOTA_LOG_DIR`)
- OpenTelemetry: endpoints, metrics/traces/logs OTLP export, Jaeger/Grafana Docker stack

## Flow Markers / 流程编号

Show numbered circular markers:

1. CLI / TUI / Desktop Entry (`iota-cli` or `iota-desktop`)
2. Command Dispatch (`cli/mod.rs` or Tauri handlers)
3. Optional TCP Daemon Route (`daemon/pool.rs` or `daemon_client.rs` from desktop)
4. Engine lifecycle initialization (`iota-core` engine)
5. Skill Registry loading & trigger matching
6. Memory recall (MemoryStore 6 buckets)
7. Context Capsule composition (git status & short/full prompts)
8. ACP Client instantiation (Stdio subprocess spawn)
9. ACP Session Protocol (initialize → session/new → session/prompt)
10. Stream updating & approval routing (TUI overlay, Desktop approval component, or CLI approval)
11. Tool intercept through MCP router
12. Kanban task ready transition & dispatcher pick-up
13. Kanban Dispatcher spawns Hermes Worker
14. Event syncing and shadow materializer project
15. Store writebacks (Cache, memory, approvals, ledger)
16. Telemetry aggregation & OpenTelemetry export

## Negative Prompt / 避免内容

Avoid unreadable tiny text, random fake file paths, obsolete module mappings, `src/store/events.rs`, single `context.db`, Promtail, project-level config discovery, Hermes home override, excessive decorative art, messy arrows, 3D render, dark background, neon cyberpunk, stock cloud icons, blurry labels, incorrect backend names, Korean text, non-Chinese non-English labels, `telemetry/console.rs`, `context/server.rs`, `skill/sandbox_executor.rs`, `src/tui.rs` as a single-file TUI, `src/memory/store.rs` shown as `src/store/memory.rs`, and legacy single-crate root path `src/` prefix instead of `crates/iota-cli/src/`, `crates/iota-core/src/`, `crates/iota-desktop/src-tauri/src/`, or `crates/iota-kanban/src/`.
