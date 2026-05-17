Create a high-resolution technical architecture diagram titled:

# iota-sympantos Runtime Architecture / 运行时架构

Generate a clean, professional, high-readability software architecture diagram.

## 1. Overall Layout / 总体布局

Use a wide horizontal canvas, preferably 21:9 or ultra-wide landscape.

Use a structured architecture-map layout with:

- White or very light warm-gray background
- Precise grid alignment
- Rounded rectangle modules
- Thin vector-style arrows
- Large readable typography
- Generous padding
- Clear hierarchy
- No tiny unreadable labels
- No overlapping arrows or text

The diagram should use a **3-zone layout**:

### Zone A — Top Control Layer / 顶部控制层

Place these modules in the top row:

1. Entry / CLI / TUI
2. Daemon TCP Plane
3. Engine Core
4. Context Fabric + Memory

This top layer represents user entry, command routing, daemon reuse, engine lifecycle, context building, and memory recall.

### Zone B — Runtime Integration Layer / 中部运行集成层

Place these modules in the middle row:

5. ACP Adapter
6. Backend Processes
7. Skill / MCP / Fn Runners
8. Native Projection

This layer represents backend communication, ACP protocol handling, skill execution, MCP routing, function runners, and native file projection.

### Zone C — Persistent Store + Observability Layer / 底部存储与可观测性层

Use one wide bottom band spanning the whole diagram:

9. Store / Telemetry / Observability

This bottom band should visually look like the system foundation.

Divide it into smaller sub-blocks:
- CacheStore
- ObservabilityStore
- MemoryStore
- ApprovalStore
- SessionLedger
- Local Logs
- OpenTelemetry
- Docker Observability Stack

## 2. Visual Style / 视觉风格

Use a modern technical architecture style:

- Clean SaaS / developer documentation aesthetic
- Flat vector design
- Subtle shadows only
- Rounded cards
- Soft pastel section colors
- Thin arrows with arrowheads
- Numbered circular sequence markers
- Bilingual English / Chinese labels
- File paths in monospace style
- Module titles in bold
- Avoid decorative illustration
- Avoid 3D rendering
- Avoid cyberpunk or dark neon style

Use a calm professional color palette:

- Pink = TUI / Presentation
- Orange = Daemon
- Blue = Engine
- Green = Context / Memory
- Cyan = ACP
- Teal = Backend
- Purple = Skill / MCP / Fn
- Gray = Store / Telemetry

Use light tinted backgrounds for modules, not saturated colors.

## 3. Typography / 字体要求

Use large readable fonts.

Hierarchy:

- Main title: very large, bold
- Module titles: large, bold
- Key concepts: medium
- File paths: small but still readable, monospace
- Detailed bullet text: compact but legible

Do not shrink text excessively.  
If content is too dense, summarize within each module instead of making text tiny.

## 4. Top Legend / 顶部图例

Add a compact legend below the title:

Pink = TUI / Presentation  
Orange = Daemon  
Blue = Engine  
Green = Context / Memory  
Cyan = ACP  
Teal = Backend  
Purple = Skill / MCP / Fn  
Gray = Store / Telemetry

Add sequence suffix legend:

T = TUI  
C = CLI  
D = Daemon  
E = Engine  
M = Memory  
A = ACP  
B = Backend  
K = Skill  
F = Fn Runner  
S = Store  
O = Observability

## 5. Sequence Flow Markers / 流程编号

Show numbered circular markers along the main arrows:

1C CLI entry  
1T TUI entry  
2C command dispatch  
3D daemon route  
4D daemon EnginePool  
5E engine request lifecycle  
6K skill registry load  
7M memory recall  
8C context capsule  
9A ensure ACP client  
10A initialize / session/new / session/prompt  
11B backend streaming update  
12A permission handling  
13K MCP / skill / fn tool route  
14S cache / memory / ledger writeback  
15O telemetry export  
16T TUI streaming render / approval overlay

Markers should be visually prominent but not oversized.

## 6. Main Modules / 主模块内容

### 1. Entry / CLI / TUI

Files:
`src/main.rs`  
`src/cli/mod.rs`  
`src/tui/mod.rs`  
`src/tui/input.rs`  
`src/tui/markdown.rs`  
`src/tui/scrollback.rs`  
`src/tui/status_bar.rs`  
`src/tui/render.rs`  
`src/tui/state.rs`  
`src/tui/loop.rs`  
`src/tui/events.rs`  
`src/tui/terminal_lifecycle.rs`  
`src/tui/theme.rs`  
`src/utils/mod.rs`

Show:
- User input enters CLI prompt or TUI composer
- `main.rs → cli::run()`
- Default no-args path enters TUI
- Commands: `run / check / bench / logs / trace / native / skill`
- TUI inline 5-row viewport, no alt-screen
- 30 FPS tick-throttled loop
- Banner printed to terminal scrollback on startup
- Background engine task with streaming output
- Approval overlay via TUI channel
- Pager / Help / Quit overlays
- Prompt queue while engine is running
- Tab to queue

### 2. Daemon TCP Plane

Files:
`src/daemon/mod.rs`  
`src/daemon/pool.rs`  
`src/daemon/proto.rs`

Show:
- `iota run --daemon`
- Local TCP daemon: `127.0.0.1:47661`
- Override: `IOTA_DAEMON_ADDR`
- Auto-start through `current_exe __daemon`
- JSON-line request / response
- `EnginePool` reuses `IotaEngine` per cwd
- 8 connection concurrency limit
- 10 MiB request cap
- Graceful Ctrl+C shutdown

### 3. Engine Core

Files:
`src/engine/mod.rs`  
`src/engine/prompt.rs`  
`src/engine/memory_ops.rs`  
`src/engine/session_ledger.rs`  
`src/engine/telemetry.rs`  
`src/engine/tests.rs`  
`src/runtime_event/mod.rs`  
`src/runtime_event/tests.rs`

Show:
- `IotaEngine`
- ACP client pool keyed by `(backend, cwd)`
- Request hash replay
- Join running execution
- Session ledger and handoff
- Memory extraction
- Deterministic memory answer
- Skill match
- Optional engine-run MCP skill
- Memory recall
- Context capsule composition
- ACP invocation
- CacheStore writeback
- OTel metrics / logs / spans

Normalized `RuntimeEvent`:
`Output / State / Log / ToolCall / ToolResult / Error / Extension / TokenUsage / Memory / ApprovalRequest / ApprovalDecision`

### 4. Context Fabric + Memory

Files:
`src/context/mod.rs`  
`src/mcp/server.rs`  
`src/memory/store.rs`  
`src/memory/embedding.rs`

Show:
- `ContextEngine`
- `<iota-context>` capsule
- `WorkingMemoryBuffer`
- Workspace summary from `git status --short`
- Memory tools prompt
- Skill index
- Handoff
- Recall buckets
- Trivial prompt fast path
- Minimal capsule for short prompts under 80 chars

Six memory taxonomy buckets:
`identity / preference / strategic / domain / procedural / episodic`

Context MCP sidecar:
- `iota-context` MCP stdio sidecar
- Memory search / write
- Skill search / load
- Session summary
- Handoff publish / read
- Resources
- Vector / hybrid search
- Ollama embeddings if configured
- Fallback 128-dimension local trigram embedding

### 5. ACP Adapter

Files:
`src/acp/mod.rs`  
`src/acp/client.rs`  
`src/acp/stream_reader.rs`  
`src/acp/wire.rs`  
`src/acp/session.rs`  
`src/acp/permission.rs`  
`src/acp/message.rs`  
`src/acp/types.rs`  
`src/acp/parser.rs`  
`src/acp/backend.rs`

Show:
- `AcpClient`
- Owns backend child process stdin/stdout
- JSON-RPC 2.0 newline-delimited protocol
- `initialize`
- `session/new`
- `session/prompt`
- Streaming `session/update`
- `session/request_permission`
- `session/complete`
- Session id reuse
- `mcpServers` rendering
- Supports empty `mcpServers`
- Supports `string_array` and `object` env shapes

Permission handling:
- Auto-approve `iota_*`
- Auto-approve `mcp__iota-*`
- Auto-approve backend whitelist hits
- Otherwise route to TUI or stdin
- ACP-side MCP tool call intercept through router

### 6. Backend Processes

Show five backend rows:

1. Claude Code  
   command: `npx`  
   aliases: `claude`, `claude-code`, `claudecode`

2. Codex  
   command: `npx`  
   alias: `codex`

3. Gemini CLI  
   command: `npx`  
   aliases: `gemini`, `gemini-cli`

4. Hermes Agent  
   command: `hermes acp`  
   alias: `hermes`

5. OpenCode  
   command: `npx`  
   aliases: `opencode`, `open-code`

Show:
- Credentials sourced from `~/.i6/nimia.yaml`
- Environment built via `backend_process_env_with_context()`

Do not show per-backend env var details.  
Do not show `HERMES_HOME` override.

### 7. Skill / MCP / Fn Runners

Files:
`src/skill/mod.rs`  
`src/skill/runner.rs`  
`src/skill/cache.rs`  
`src/skill/fun.rs`  
`src/mcp/mod.rs`  
`src/mcp/client.rs`  
`src/mcp/router.rs`  
`src/mcp/tool_dispatch.rs`

Show:
- `SkillRegistry`
- Load roots:
  - workspace `skills/`
  - workspace `.iota/skills`
  - configured skill roots
  - `~/.i6/skills`
- Frontmatter parsing
- Trigger matching
- Backend compatibility
- `SkillRunner`
- `execution.mode = mcp`
- Sequential or parallel MCP tool calls
- Template rendering
- MCP client
- ACP-side MCP router

Intercept methods:
- `tools/call`
- `mcp/tools/call`
- `mcp/tool_call`

Route:
- iota memory tools
- iota skill tools
- iota session tools
- iota handoff tools
- iota fun tools

Show:
- Reject external tools
- `iota-fun` MCP stdio server

Seven Fn runners:
`Python / TypeScript / Rust / Go / Java / C++ / Zig`

### 8. Native Projection

Files:
`src/native/mod.rs`

Show:
- `iota native-materialize`
- Memory / skill native file projection
- Backend-native files
- Block replacement markers:
  - `<!-- IOTA_START -->`
  - `<!-- IOTA_END -->`
- Useful for backends without MCP support

### 9. Store / Telemetry / Observability

Bottom wide foundation band.

Show store blocks:

#### CacheStore
Path: `~/.i6/context/events.sqlite`

Show:
- Replay / dedupe
- Request hash
- Running join
- Fencing token
- 30-day retention

#### ObservabilityStore
Path: `~/.i6/context/events.sqlite` shared table

Show:
- RuntimeEvent recording
- Token usage
- Tool calls
- Configurable retention

#### MemoryStore
Path: `~/.i6/context/memory.sqlite`  
or `context_engine.memory_db`

Show:
- Six taxonomy buckets
- FTS / LIKE
- Vector / hybrid search
- Dedup
- TTL
- Merge

#### ApprovalStore
Path: `~/.i6/context/approvals.sqlite`

Show:
- Request / decision recording
- Risk classification

#### SessionLedger
Path: `~/.i6/context/sessions.sqlite`

Show:
- Session tracking
- Turn tracking
- Handoff tracking

#### Local Logs
Show:
- stderr tracing layer
- Daily files under `~/.i6/logs/`
- Controlled by `IOTA_LOG_DIR`

#### OpenTelemetry
Show:
- Default endpoint: `http://localhost:4317`
- `OTEL_ENABLED=false` disables export
- Traces, logs, metrics

#### Docker Observability Stack
Show:
- OTel Collector: `4317 / 4318`
- Loki: `3100`
- Jaeger: `16686`
- Prometheus: `9090`
- Grafana: `3000`

## 7. Flow Arrows / 箭头流向

Use color-coded arrows:

- Pink arrows: Entry / TUI → Engine
- Orange arrows: CLI daemon path → Daemon → Engine
- Blue arrows: Engine lifecycle flow
- Green arrows: Engine ↔ ContextEngine ↔ MemoryStore ↔ Context MCP
- Cyan arrows: Engine ↔ ACP Adapter
- Teal arrows: ACP Adapter ↔ Backend Processes
- Purple arrows: Engine / ACP Router ↔ Skill / MCP / Fn Runners
- Gray arrows: Engine / Stores → Store / Telemetry bottom band

Main flow should be visually readable from left to right and top to bottom.

Avoid crossing arrows whenever possible.  
Use elbow connectors or curved connectors only when needed.

## 8. Composition Rules / 构图规则

- Keep each module visually separated
- Use consistent card sizes where possible
- Put file paths in a compact file-list section inside each module
- Put behavior summaries in bullet groups
- Use icons only if simple and minimal:
  - terminal icon for CLI/TUI
  - server icon for daemon
  - gear icon for engine
  - memory chip icon for context/memory
  - plug icon for ACP
  - process icon for backends
  - tool icon for skills
  - database icon for store
- Icons must not dominate the diagram
- Text clarity is more important than decoration

## 9. Negative Prompt / 避免内容

Avoid:

- Unreadable tiny text
- Random fake file paths
- Obsolete modules
- `src/store/events.rs`
- Single `context.db`
- Promtail
- Project-level config discovery
- Hermes home override
- Excessive decorative art
- Messy arrows
- 3D render
- Dark background
- Neon cyberpunk
- Stock cloud icons
- Blurry labels
- Incorrect backend names
- Korean text
- Non-Chinese non-English labels
- `telemetry/console.rs`
- `context/server.rs`
- `skill/sandbox_executor.rs`