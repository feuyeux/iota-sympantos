```
Create a high-resolution technical architecture diagram titled:

iota-sympantos Runtime Architecture / 运行时架构

Generate a clean, professional software architecture map with a white background, rounded rectangle modules, thin-line vector-style arrows, color-coded sections, numbered sequence markers, bilingual Chinese / English labels, readable typography, and precise grid alignment.

Use a two-row layout to avoid compression:
Top row:
1. Entry / CLI / TUI
2. Daemon TCP Plane
3. Engine Core
4. Context Fabric + Memory

Second row:
5. ACP Adapter
6. Backend Processes
7. Skill / MCP / Fn Runners
8. Native Projection

Bottom wide band:
9. Store / Telemetry / Observability

Use large readable fonts. File paths should use a monospaced font style. Module titles should be bold and clear. Do not make tiny unreadable labels. Keep generous padding and line spacing. Do not let arrows overlap text.

Top legend:
Pink = TUI / Presentation
Orange = Daemon
Blue = Engine
Green = Context / Memory
Cyan = ACP
Teal = Backend
Purple = Skill / MCP / Fn
Gray = Store / Telemetry

Sequence suffixes:
T = TUI, C = CLI, D = Daemon, M = Memory, A = ACP, B = Backend, K = Skill, F = Fn Runner, S = Store, O = Observability

Show sequence markers:
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

Column 1: Entry / CLI / TUI
Files:
src/main.rs
src/cli/mod.rs
src/tui/mod.rs
src/tui/input.rs
src/tui/markdown.rs
src/tui/scrollback.rs
src/tui/status_bar.rs
src/tui/render.rs
src/tui/state.rs
src/tui/loop.rs
src/tui/events.rs
src/tui/terminal_lifecycle.rs
src/tui/theme.rs
src/config/mod.rs
src/native/mod.rs
src/utils.rs

Show:
User input enters CLI prompt or TUI composer
main.rs -> cli::run()
Default no-args path enters TUI
iota run / check / bench / logs / trace / native / skill
TUI inline 5-row viewport, no alt-screen
30 FPS tick-throttled loop
Banner printed to terminal scrollback on startup
Background engine task with streaming output
Approval overlay via TUI channel
Pager / Help / Quit overlays
Prompt queue while engine is running, Tab to queue

Column 2: Daemon TCP Plane
Files:
src/daemon/mod.rs
src/daemon/pool.rs
src/daemon/proto.rs

Show:
iota run --daemon
Local TCP daemon at 127.0.0.1:47661
Overridable by IOTA_DAEMON_ADDR
Daemon auto-start through current_exe __daemon
JSON line request / response
EnginePool reuses IotaEngine per cwd
8 connection concurrency limit
10 MiB request cap
Graceful Ctrl+C shutdown

Column 3: Engine Core
Files:
src/engine/mod.rs
src/engine/prompt.rs
src/engine/memory_ops.rs
src/engine/session_ledger.rs
src/engine/telemetry.rs
src/engine/tests.rs
src/runtime_event/mod.rs
src/runtime_event/tests.rs

Show:
IotaEngine
ACP client pool keyed by (backend, cwd)
Request hash replay
Join running execution
Session ledger and handoff
Memory extraction / deterministic memory answer
Skill match and optional engine-run MCP skill
Memory recall
Context capsule composition
ACP invocation
CacheStore writeback
OTel metrics / logs / spans
Normalized RuntimeEvent:
Output / State / Log / ToolCall / ToolResult / Error / Extension / TokenUsage / Memory / ApprovalRequest / ApprovalDecision

Column 4: Context Fabric + Memory
Files:
src/context/mod.rs
src/mcp/server.rs
src/store/memory.rs
src/store/embedding.rs

Show:
ContextEngine
<iota-context> capsule
WorkingMemoryBuffer
Workspace summary from git status --short
Memory tools prompt
Skill index
Handoff
Recall buckets
Trivial prompt fast path
Minimal capsule for short prompts under 80 chars
Six memory taxonomy buckets:
identity / preference / strategic / domain / procedural / episodic
iota-context MCP stdio sidecar
Memory search / write
Skill search / load
Session summary
Handoff publish / read
Resources
Vector / hybrid search
Ollama embeddings if configured
Fallback 128-dimension local trigram embedding

Column 5: ACP Adapter
Files:
src/acp/mod.rs
src/acp/client.rs
src/acp/stream_reader.rs
src/acp/wire.rs
src/acp/session.rs
src/acp/permission.rs
src/acp/message.rs
src/acp/types.rs
src/acp/parser.rs
src/acp/backend.rs

Show:
AcpClient
Owns backend child process stdin/stdout
JSON-RPC 2.0 newline-delimited protocol
initialize
session/new
session/prompt
streaming session/update
session/request_permission
session/complete
Session id reuse
mcpServers rendering
Supports empty mcpServers
Supports string_array and object env shapes
Permission handling:
Auto-approve iota_*
Auto-approve mcp__iota-*
Auto-approve backend whitelist hits
Otherwise route to TUI or stdin
ACP-side MCP tool call intercept through router

Column 6: Backend Processes
Show five backend rows:
Claude Code, command npx, aliases claude, claude-code, claudecode
Codex, command npx, alias codex
Gemini CLI, command npx, aliases gemini, gemini-cli
Hermes Agent, command hermes acp, alias hermes
OpenCode, command npx, aliases opencode, open-code

Environment mapping from ~/.i6/nimia.yaml:
Claude Code:
ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL, ANTHROPIC_MODEL

Codex:
OPENAI_API_KEY, ROUTER_API_KEY, OPENAI_BASE_URL, OPENAI_MODEL

Gemini:
GEMINI_API_KEY, GEMINI_MODEL

Hermes:
HERMES_INFERENCE_PROVIDER, HERMES_MODEL, provider-native key and base URL variables

OpenCode:
OPENCODE_MODEL

Important note:
Do not show HERMES_HOME override. Hermes keeps its own default home.

Column 7: Skill / MCP / Fn Runners
Files:
src/skill/mod.rs
src/skill/runner.rs
src/skill/cache.rs
src/skill/fun.rs
src/skill/fun_tests.rs
src/mcp/mod.rs
src/mcp/client.rs
src/mcp/router.rs
src/mcp/tool_dispatch.rs

Show:
SkillRegistry
Load roots:
workspace skills/
workspace .iota/skills
configured skill roots
~/.i6/skills
Frontmatter parsing
Trigger matching
Backend compatibility
SkillRunner
execution.mode = mcp
Sequential or parallel MCP tool calls
Template rendering
MCP client
ACP-side MCP router
Intercept methods:
tools/call
mcp/tools/call
mcp/tool_call
Route iota memory / skill / session / handoff / fun tools
Reject external tools
iota-fun MCP stdio server
Seven Fn runners:
Python / TypeScript / Rust / Go / Java / C++ / Zig

Column 8: Native Projection
Files:
src/native/mod.rs

Show:
iota native-materialize
Memory / skill native file projection
Backend-native files
Block replacement markers:
<!-- IOTA_START -->
<!-- IOTA_END -->
Useful for backends without MCP support

Bottom band: Store / Telemetry / Observability
Files:
src/store/mod.rs
src/store/cache.rs
src/store/memory.rs
src/store/embedding.rs
src/store/approvals.rs
src/store/approvals_tests.rs
src/store/ledger.rs
src/telemetry/mod.rs
src/telemetry/stderr.rs
src/telemetry/metrics.rs
src/telemetry/tests.rs

Show store blocks:
CacheStore:
Path ~/.i6/context/events.sqlite
Replay / dedupe only
Request hash
Running join
Fencing token
Output replay
30-day completed / failed retention

MemoryStore:
Path ~/.i6/context/memory.sqlite
May be overridden by context_engine.memory_db
Taxonomy
Dedup
TTL
Merge mode
FTS / LIKE
Vector / hybrid search
memory_embedding table

ApprovalStore:
Path ~/.i6/context/approvals.sqlite
Request / decision recording
Default risk classification

SessionLedger:
Path ~/.i6/context/sessions.sqlite
iota session
backend session
turn
handoff

Local logs:
stderr tracing layer
Daily files under ~/.i6/logs/
Controlled by IOTA_LOG_DIR

OpenTelemetry:
Default endpoint http://localhost:4317
OTEL_ENABLED=false disables export
Traces, logs, metrics

Docker observability stack:
OTel Collector 4317 / 4318
Loki 3100
Jaeger 16686
Prometheus 9090
Grafana 3000

Flow arrows:
Pink arrows from Entry / TUI to Engine
Orange arrows from CLI daemon path to Daemon and then Engine
Blue arrows through Engine core lifecycle
Green arrows between Engine, ContextEngine, MemoryStore, and context MCP
Cyan arrows between Engine and ACP Adapter
Teal arrows between ACP Adapter and backend processes
Purple arrows between Engine / ACP router and Skill / MCP / Fn runners
Gray arrows from Engine and stores to Store / Telemetry bottom band

Negative prompt:
Unreadable tiny text, random fake file paths, obsolete modules, src/store/events.rs, single context.db, Promtail, project-level config discovery, Hermes home override, excessive decorative art, messy arrows, 3D render, dark background, neon cyberpunk, stock cloud icons, blurry labels, incorrect backend names, Korean text, non-Chinese non-English labels, telemetry/console.rs, context/server.rs, skill/sandbox_executor.rs.
```

