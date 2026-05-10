Use `gpt-image-2-style-library`: `technical architecture infographic`, `clean bilingual system diagram`, `thin line vector layout`, `pen-and-ink engineering poster`, `precise module map`, `color-coded flow arrows`.

  Create an updated version of the provided Image #1 according to the latest `iota-sympantos` code implementation.

  The image should remain a wide landscape architecture diagram, similar to the reference image: large canvas, clean white background, rounded module columns, bilingual Chinese / English labels, numbered sections, colored
  arrows, sequence markers, and small file-path tags. The result should be readable as a technical architecture map, not a decorative poster.

  Title:
  `iota-sympantos Runtime Architecture / 运行时架构`

  Top legend:
  - Pink: TUI / Presentation
  - Orange: Daemon
  - Blue: Engine
  - Green: Context / Memory
  - Cyan: ACP
  - Teal: Backend
  - Purple: Skill / MCP / Fn
  - Gray: Store / Telemetry
  - Sequence suffixes: `T = TUI`, `C = CLI`, `D = Daemon`, `M = Memory`, `A = ACP`, `B = Backend`, `K = Skill`, `F = Fn Runner`, `S = Store`, `O = Observability`

  Main layout:
  Use 8 vertical columns plus a wide bottom store / telemetry band.

  Column 1: Entry / CLI / TUI
  Files:
  - `src/main.rs`
  - `src/cli/mod.rs`
  - `src/tui.rs`
  - `src/tui/composer.rs`
  - `src/tui/markdown.rs`
  - `src/tui/state.rs`
  - `src/tui/status_bar.rs`
  - `src/tui/theme.rs`
  - `src/config.rs`
  - `src/native/mod.rs`
  - `src/utils.rs`

  Show:
  - User input enters CLI prompt or TUI composer
  - `main.rs -> cli::run()`
  - Default no-args path enters TUI
  - `iota run/check/bench/logs/trace/native/skill`
  - TUI background engine task
  - streaming output
  - approval overlay
  - pager/help/quit overlays
  - prompt queue while engine is running

  Column 2: Daemon TCP Plane
  Files:
  - `src/daemon/mod.rs`
  - `src/daemon/pool.rs`
  - `src/daemon/proto.rs`

  Show:
  - `iota run --daemon`
  - local TCP daemon at `127.0.0.1:47661`
  - overridable by `IOTA_DAEMON_ADDR`
  - daemon auto-start through `current_exe __daemon`
  - JSON line request / response
  - `EnginePool` reuses `IotaEngine` per cwd
  - 8 connection concurrency limit
  - 10 MiB request cap
  - graceful Ctrl+C shutdown

  Column 3: Engine Core
  Files:
  - `src/engine.rs`
  - `src/runtime_event.rs`

  Show:
  - `IotaEngine`
  - ACP client pool keyed by `(backend, cwd)`
  - request hash replay
  - join running execution
  - session ledger and handoff
  - memory extraction / deterministic memory answer
  - skill match and optional engine-run MCP skill
  - memory recall
  - context capsule composition
  - ACP invocation
  - CacheStore writeback
  - OTel metrics / logs / spans
  - normalized `RuntimeEvent`

  Column 4: Context Fabric + Memory
  Files:
  - `src/context/mod.rs`
  - `src/context/server.rs`
  - `src/store/memory.rs`
  - `src/store/embedding.rs`

  Show:
  - `ContextEngine`
  - `<iota-context>` capsule
  - `DialogueBuffer`
  - workspace summary from `git status --short`
  - memory tools prompt
  - skill index
  - handoff
  - recall buckets
  - six memory taxonomy buckets:
    1. identity
    2. preference
    3. strategic
    4. domain
    5. procedural
    6. episodic
  - `iota-context` MCP stdio sidecar
  - memory search / write
  - skill search / load
  - session summary
  - handoff publish / read
  - resources
  - vector / hybrid search
  - Ollama embeddings if configured
  - fallback 128-dimension local trigram embedding

  Column 5: ACP Adapter
  Files:
  - `src/acp/mod.rs`
  - `src/acp/wire.rs`
  - `src/acp/session.rs`
  - `src/acp/permission.rs`
  - `src/acp/session_tests.rs`
  - `src/acp/wire_tests.rs`

  Show:
  - `AcpClient`
  - owns backend child process stdin/stdout
  - JSON-RPC 2.0 newline-delimited protocol
  - `initialize`
  - `session/new`
  - `session/prompt`
  - streaming `session/update`
  - `session/request_permission`
  - `session/complete`
  - session id reuse
  - `mcpServers` rendering
  - supports empty `mcpServers`
  - supports `string_array` and `object` env shapes
  - permission handling:
    - auto-approve `iota_*`
    - auto-approve `mcp__iota-*`
    - auto-approve backend whitelist hits
    - otherwise route to TUI or stdin
  - ACP-side MCP tool call intercept through router

  Column 6: Backend Processes
  Show five backend rows:
  - Claude Code, command `npx`, aliases `claude`, `claudecode`
  - Codex, command `npx`, alias `codex`
  - Gemini CLI, command `npx`, aliases `gemini`, `gemini-cli`
  - Hermes Agent, command `hermes acp`, alias `hermes`
  - OpenCode, command `npx`, aliases `opencode`, `open-code`

  Show environment mapping from `~/.i6/nimia.yaml`:
  - Claude Code: `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, `ANTHROPIC_MODEL`
  - Codex: `OPENAI_API_KEY`, `ROUTER_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`
  - Gemini: `GEMINI_API_KEY`, `GEMINI_MODEL`
  - Hermes: `HERMES_INFERENCE_PROVIDER`, `HERMES_MODEL`, provider-native key and base URL variables
  - OpenCode: `OPENCODE_MODEL`

  Important note:
  - Do not show `HERMES_HOME` override. Hermes keeps its own default home.

  Column 7: Skill / MCP / Fn Runners
  Files:
  - `src/skill/mod.rs`
  - `src/skill/runner.rs`
  - `src/skill/cache.rs`
  - `src/skill/fun_server.rs`
  - `src/mcp/mod.rs`
  - `src/mcp/client.rs`
  - `src/mcp/router.rs`

  Show:
  - `SkillRegistry`
  - load roots:
    - workspace `skills/`
    - workspace `.iota/skills`
    - configured skill roots
    - `~/.i6/skills`
  - frontmatter parsing
  - trigger matching
  - backend compatibility
  - `SkillRunner`
  - `execution.mode = mcp`
  - sequential or parallel MCP tool calls
  - template rendering
  - `MCP client`
  - ACP-side `MCP router`
  - intercept methods:
    - `tools/call`
    - `mcp/tools/call`
    - `mcp/tool_call`
  - route iota memory / skill / session / handoff / fun tools
  - reject external tools
  - `iota-fun` MCP stdio server
  - seven Fn runners:
    - Python
    - TypeScript
    - Rust
    - Go
    - Java
    - C++
    - Zig

  Column 8: Native Projection
  Files:
  - `src/native/mod.rs`

  Show:
  - `iota native-materialize`
  - memory / skill native file projection
  - backend-native files
  - block replacement markers:
    - `<!-- IOTA_START -->`
    - `<!-- IOTA_END -->`
  - useful for backends without MCP support

  Bottom wide band: Store / Telemetry / Observability
  Files:
  - `src/store/mod.rs`
  - `src/store/cache.rs`
  - `src/store/memory.rs`
  - `src/store/embedding.rs`
  - `src/store/approval.rs`
  - `src/store/ledger.rs`
  - `src/telemetry/mod.rs`
  - `src/telemetry/console.rs`
  - `src/telemetry/logs.rs`
  - `src/telemetry/metrics.rs`
  - `src/telemetry/spans.rs`

  Show store blocks:
  - `CacheStore`
    - path `~/.i6/context/events.sqlite`
    - replay / dedupe only
    - request hash
    - running join
    - fencing token
    - output replay
    - 30-day completed / failed retention
  - `MemoryStore`
    - path `~/.i6/context/memory.sqlite`
    - may be overridden by `context_engine.memory_db`
    - taxonomy
    - dedup
    - TTL
    - merge mode
    - FTS / LIKE
    - vector / hybrid search
    - `memory_embedding` table
  - `ApprovalStore`
    - path `~/.i6/context/approvals.sqlite`
    - request / decision recording
    - default risk classification
  - `SessionLedger`
    - path `~/.i6/context/sessions.sqlite`
    - iota session
    - backend session
    - turn
    - handoff
  - `Local logs`
    - stderr tracing layer
    - daily files under `~/.i6/logs/`
    - controlled by `IOTA_LOG_FILE`
  - `OpenTelemetry`
    - default endpoint `http://localhost:4317`
    - `OTEL_ENABLED=false` disables export
    - traces, logs, metrics
  - Docker observability stack:
    - OTel Collector `4317 / 4318`
    - Loki `3100`
    - Jaeger `16686`
    - Prometheus `9090`
    - Grafana `3000`

  Important correction from old diagram:
  Do not show `src/store/events.rs`. The current implementation uses `src/store/cache.rs`; `events.sqlite` is CacheStore replay / dedupe storage, not a durable RuntimeEvent audit stream.
  Do not show a single `~/.i6/context.db`. Current stores are split across `events.sqlite`, `memory.sqlite`, `approvals.sqlite`, and `sessions.sqlite`.
  Do not show Promtail or old SQLite/EventStore metrics pipeline.
  Do not show Docker mounting `~/.i6`.
  Do not show project-level config discovery. Configuration comes only from `~/.i6/nimia.yaml`.

  Flow arrows:
  - Pink arrows from Entry / TUI to Engine
  - Orange arrows from CLI daemon path to Daemon and then Engine
  - Blue arrows through Engine core lifecycle
  - Green arrows between Engine, ContextEngine, MemoryStore, and context MCP
  - Cyan arrows between Engine and ACP Adapter
  - Teal arrows between ACP Adapter and backend processes
  - Purple arrows between Engine / ACP router and Skill / MCP / Fn runners
  - Gray arrows from Engine and stores to Store / Telemetry bottom band

  Sequence markers:
  Use circled markers like the reference image:
  - `1C` CLI entry
  - `1T` TUI entry
  - `2C` command dispatch
  - `3D` daemon route
  - `4D` daemon EnginePool
  - `5E` engine request lifecycle
  - `6K` skill registry load
  - `7M` memory recall
  - `8C` context capsule
  - `9A` ensure ACP client
  - `10A` initialize / session/new / session/prompt
  - `11B` backend streaming update
  - `12A` permission handling
  - `13K` MCP / skill / fn tool route
  - `14S` cache / memory / ledger writeback
  - `15O` telemetry export
  - `16T` TUI streaming render / approval overlay

  Visual style:
  - Wide landscape infographic, 16:9 or 2:1 ratio.
  - White background, thin rounded rectangles, precise grid alignment.
  - Use bilingual labels: Chinese first, English second, separated by `/`.
  - Keep labels readable and concise.
  - Use small icons only when they clarify meaning: terminal, database, gear, shield, book, network socket, telescope, chart.
  - Use a clean technical pen-line style with light color accents matching the legend.
  - The image must look like an updated version of the reference architecture diagram, not a new unrelated poster.

  Negative prompt:
  Unreadable tiny text, random fake file paths, obsolete modules, `src/store/events.rs`, single `context.db`, Promtail, project-level config discovery, Hermes home override, excessive decorative art, messy arrows, 3D render,
  dark background, neon cyberpunk, stock cloud icons, blurry labels, incorrect backend names, Korean text, non-Chinese non-English labels.