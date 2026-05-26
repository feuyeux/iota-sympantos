# iota-sympantos 系统架构与设计图表集 (AI 生图提示词)

本文档包含 iota-sympantos 项目的完整架构图表生成提示词，涵盖系统架构、运行时流程、观测性和调试四个维度。

---

## 目录

1. [系统架构总览](#1-系统架构总览)
   - 1.1 [分层架构与组件依赖](#11-分层架构与组件依赖)
   - 1.2 [完整运行时架构图](#12-完整运行时架构图)
   - 1.3 [架构总览海报](#13-架构总览海报)
2. [运行时执行流程](#2-运行时执行流程)
   - 2.1 [Prompt 执行时序](#21-prompt-执行时序)
   - 2.2 [代码调用链路](#22-代码调用链路)
   - 2.3 [Backend 调用与 IPC](#23-backend-调用与-ipc)
3. [Context Fabric 与记忆系统](#3-context-fabric-与记忆系统)
   - 3.1 [记忆分类与生命周期](#31-记忆分类与生命周期)
   - 3.2 [六桶记忆召回机制](#32-六桶记忆召回机制)
4. [观测性与调试](#4-观测性与调试)
   - 4.1 [OpenTelemetry 观测性架构](#41-opentelemetry-观测性架构)
   - 4.2 [调试工作流](#42-调试工作流)

---

## 1. 系统架构总览

### 1.1 分层架构与组件依赖

**用途**: 展示 iota-sympantos 的四层架构和组件间的依赖关系

**Prompt**:
```
A technical layered architecture diagram in a clean colored-pen schematic style, titled "iota-sympantos Component Layers and Dependencies". 
The background is off-white. The diagram is divided vertically into four distinct boxes representing layers with strict dependency flow:

1. Layer 1: "Presentation (cli & tui)" - outlined in deep navy blue
   - Blocks: "main.rs", "cli/mod.rs", "tui.rs", "tui/composer.rs", "tui/markdown.rs", "tui/status_bar.rs"
   
2. Layer 2: "Orchestration" - outlined in muted forest green
   - Large block: "IotaEngine (engine.rs)"
   - Block: "daemon/mod.rs & pool.rs (EnginePool)"
   - Block: "runtime_event.rs (RuntimeEvent normalization)"
   
3. Layer 3: "Protocol & Tools" - outlined in terracotta orange
   - Blocks: "AcpClient (acp/mod.rs)", "acp::permission", "acp::session", "acp::wire"
   - Blocks: "mcp::router", "mcp::client"
   - Blocks: "context::ContextEngine", "skill::SkillRegistry", "skill::runner"
   
4. Layer 4: "External Boundaries" - outlined in dark charcoal gray
   - Block: "Backend Subprocesses" (Claude Code, Codex, Gemini CLI, Hermes, OpenCode)
   - Block: "MCP Sidecars" (iota-context, iota-fun)
   - Block: "SQLite Stores" (events.sqlite, memory.sqlite, sessions.sqlite, approvals.sqlite)

Connections and Flows:
- Purple solid arrows from "IotaEngine" to "SQLite Stores" labeled "SQL I/O"
- Terracotta arrow from "IotaEngine" to "AcpClient" labeled "injects EffectiveConfig"
- Terracotta arrow from "AcpClient" to "mcp::router" labeled "delegates tool_call filter"
- Dual connection between "tui.rs" and "IotaEngine":
  - Blue arrow labeled "mpsc (streams output chunks)"
  - Red arrow labeled "oneshot (TUI approval decision)"
- Dark gray socket line between "cli/mod.rs" and "daemon" labeled "TCP 127.0.0.1:47661"
- Blue pipe lines between "AcpClient" and "Backend Subprocesses" labeled "Stdio (stdin/stdout/stderr)"

Style instructions:
- Colored pen-on-paper schematic style, off-white background
- Clean sans-serif typeface, consistent font size within each layer
- Harmonious color coding for layers and connections
- Continuous, solid lines with no overlapping, generous white borders
```

---

### 1.2 完整运行时架构图

**用途**: 展示完整的运行时架构，包含所有模块、数据流和序列标记（双语版本）

**Prompt**:
```
A wide landscape technical architecture infographic titled "iota-sympantos Runtime Architecture / 运行时架构".
Use clean bilingual system diagram style with thin line vector layout, pen-and-ink engineering poster aesthetic, precise module map, color-coded flow arrows.

Canvas: 16:9 or 2:1 ratio, white background, rounded module columns, precise grid alignment.

Top legend:
- Pink: TUI / Presentation
- Orange: Daemon
- Blue: Engine
- Green: Context / Memory
- Cyan: ACP
- Teal: Backend
- Purple: Skill / MCP / Fn
- Gray: Store / Telemetry

Sequence suffixes: T=TUI, C=CLI, D=Daemon, M=Memory, A=ACP, B=Backend, K=Skill, F=Fn Runner, S=Store, O=Observability

Main layout: 8 vertical columns + wide bottom store/telemetry band

Column 1: Entry / CLI / TUI
Files: src/main.rs, src/cli/mod.rs, src/tui.rs, src/tui/composer.rs, src/tui/markdown.rs, src/tui/state.rs, src/tui/status_bar.rs, src/tui/theme.rs, src/config.rs, src/native/mod.rs, src/utils.rs
Show:
- User input enters CLI prompt or TUI composer
- main.rs -> cli::run()
- Default no-args path enters TUI
- Commands: iota run/check/bench/logs/trace/native/skill
- TUI background engine task, streaming output, approval overlay, pager/help/quit overlays, prompt queue while engine running

Column 2: Daemon TCP Plane
Files: src/daemon/mod.rs, src/daemon/pool.rs, src/daemon/proto.rs
Show:
- iota run --daemon
- Local TCP daemon at 127.0.0.1:47661 (overridable by IOTA_DAEMON_ADDR)
- Daemon auto-start through current_exe __daemon
- JSON line request/response
- EnginePool reuses IotaEngine per cwd
- 8 connection concurrency limit, 10 MiB request cap
- Graceful Ctrl+C shutdown

Column 3: Engine Core
Files: src/engine.rs, src/runtime_event.rs
Show:
- IotaEngine
- ACP client pool keyed by (backend, cwd)
- Request hash replay, join running execution
- Session ledger and handoff
- Memory extraction / deterministic memory answer
- Skill match and optional engine-run MCP skill
- Memory recall, context capsule composition
- ACP invocation, CacheStore writeback
- OTel metrics/logs/spans
- Normalized RuntimeEvent

Column 4: Context Fabric + Memory
Files: src/context/mod.rs, src/context/server.rs, src/store/memory.rs, src/store/embedding.rs
Show:
- ContextEngine
- <iota-context> capsule, DialogueBuffer
- Workspace summary from git status --short
- Memory tools prompt, skill index, handoff
- Recall buckets
- Six memory taxonomy buckets: identity, preference, strategic, domain, procedural, episodic
- iota-context MCP stdio sidecar
- Memory search/write, skill search/load, session summary, handoff publish/read
- Resources, vector/hybrid search
- Ollama embeddings if configured, fallback 128-dimension local trigram embedding

Column 5: ACP Adapter
Files: src/acp/mod.rs, src/acp/wire.rs, src/acp/session.rs, src/acp/permission.rs
Show:
- AcpClient owns backend child process stdin/stdout
- JSON-RPC 2.0 newline-delimited protocol
- initialize, session/new, session/prompt, streaming session/update, session/request_permission, session/complete
- Session id reuse, mcpServers rendering
- Supports empty mcpServers, string_array and object env shapes
- Permission handling: auto-approve iota_*, mcp__iota-*, backend whitelist hits; otherwise route to TUI or stdin
- ACP-side MCP tool call intercept through router

Column 6: Backend Processes
Show five backend rows:
- Claude Code: command npx, aliases claude/claudecode
  Env: ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL, ANTHROPIC_MODEL
- Codex: command npx, alias codex
  Env: OPENAI_API_KEY, ROUTER_API_KEY, OPENAI_BASE_URL, OPENAI_MODEL
- Gemini CLI: command npx, aliases gemini/gemini-cli
  Env: GEMINI_API_KEY, GEMINI_MODEL
- Hermes Agent: command hermes acp, alias hermes
  Env: HERMES_INFERENCE_PROVIDER, HERMES_MODEL, provider-native key and base URL variables
  Note: Do not show HERMES_HOME override. Hermes keeps its own default home.
- OpenCode: command npx, aliases opencode/open-code
  Env: OPENCODE_MODEL

Column 7: Skill / MCP / Fn Runners
Files: src/skill/mod.rs, src/skill/runner.rs, src/skill/cache.rs, src/skill/fun_server.rs, src/mcp/mod.rs, src/mcp/client.rs, src/mcp/router.rs
Show:
- SkillRegistry load roots: workspace skills/, workspace .iota/skills, configured skill roots, ~/.i6/skills
- Frontmatter parsing, trigger matching, backend compatibility
- SkillRunner, execution.mode = mcp, sequential or parallel MCP tool calls, template rendering
- MCP client, ACP-side MCP router
- Intercept methods: tools/call, mcp/tools/call, mcp/tool_call
- Route iota memory/skill/session/handoff/fun tools, reject external tools
- iota-fun MCP stdio server
- Seven Fn runners: Python, TypeScript, Rust, Go, Java, C++, Zig

Column 8: Native Projection
Files: src/native/mod.rs
Show:
- iota native-materialize
- Memory/skill native file projection, backend-native files
- Block replacement markers: <!-- IOTA_START -->, <!-- IOTA_END -->
- Useful for backends without MCP support

Bottom wide band: Store / Telemetry / Observability
Files: src/store/mod.rs, src/store/cache.rs, src/store/memory.rs, src/store/embedding.rs, src/store/approval.rs, src/store/ledger.rs, src/telemetry/mod.rs, src/telemetry/console.rs, src/telemetry/logs.rs, src/telemetry/metrics.rs, src/telemetry/spans.rs

Show store blocks:
- CacheStore: path ~/.i6/context/events.sqlite, replay/dedupe only, request hash, running join, fencing token, output replay, 30-day completed/failed retention
- MemoryStore: path ~/.i6/context/memory.sqlite (may be overridden by context_engine.memory_db), taxonomy, dedup, TTL, merge mode, FTS/LIKE, vector/hybrid search, memory_embedding table
- ApprovalStore: path ~/.i6/context/approvals.sqlite, request/decision recording, default risk classification
- SessionLedger: path ~/.i6/context/sessions.sqlite, iota session, backend session, turn, handoff
- Local logs: stderr tracing layer, daily files under ~/.i6/logs/, controlled by IOTA_LOG_FILE
- OpenTelemetry: default endpoint http://localhost:4317, OTEL_ENABLED=false disables export, traces/logs/metrics
- Docker observability stack: OTel Collector 4317/4318, Loki 3100, Jaeger 16686, Prometheus 9090, Grafana 3000

Important corrections from old diagram:
- Do not show src/store/events.rs. Current implementation uses src/store/cache.rs; events.sqlite is CacheStore replay/dedupe storage, not a durable RuntimeEvent audit stream.
- Do not show a single ~/.i6/context.db. Current stores are split across events.sqlite, memory.sqlite, approvals.sqlite, and sessions.sqlite.
- Do not show Promtail or old SQLite/EventStore metrics pipeline.
- Do not show Docker mounting ~/.i6.
- Do not show project-level config discovery. Configuration comes only from ~/.i6/nimia.yaml.

Flow arrows:
- Pink arrows from Entry/TUI to Engine
- Orange arrows from CLI daemon path to Daemon and then Engine
- Blue arrows through Engine core lifecycle
- Green arrows between Engine, ContextEngine, MemoryStore, and context MCP
- Cyan arrows between Engine and ACP Adapter
- Teal arrows between ACP Adapter and backend processes
- Purple arrows between Engine/ACP router and Skill/MCP/Fn runners
- Gray arrows from Engine and stores to Store/Telemetry bottom band

Sequence markers (use circled markers):
1C CLI entry, 1T TUI entry, 2C command dispatch, 3D daemon route, 4D daemon EnginePool, 5E engine request lifecycle, 6K skill registry load, 7M memory recall, 8C context capsule, 9A ensure ACP client, 10A initialize/session/new/session/prompt, 11B backend streaming update, 12A permission handling, 13K MCP/skill/fn tool route, 14S cache/memory/ledger writeback, 15O telemetry export, 16T TUI streaming render/approval overlay

Visual style:
- Wide landscape infographic, 16:9 or 2:1 ratio
- White background, thin rounded rectangles, precise grid alignment
- Bilingual labels: Chinese first, English second, separated by /
- Keep labels readable and concise
- Use small icons only when they clarify meaning: terminal, database, gear, shield, book, network socket, telescope, chart
- Clean technical pen-line style with light color accents matching the legend
- The image must look like an updated version of a reference architecture diagram, not a new unrelated poster

Negative prompt:
Unreadable tiny text, random fake file paths, obsolete modules, src/store/events.rs, single context.db, Promtail, project-level config discovery, Hermes home override, excessive decorative art, messy arrows, 3D render, dark background, neon cyberpunk, stock cloud icons, blurry labels, incorrect backend names, Korean text, non-Chinese non-English labels
```

---

### 1.3 架构总览海报

**用途**: 以故事化的方式展示整体架构，适合用于文档封面或概览

**Prompt**:
```
Use gpt-image-2-style-library: pen-and-ink technical story poster, hand-drawn architectural cutaway, fine cross-hatching, precise black ink, warm paper texture, playful engineering narrative.

Create a vertical poster for the document "iota-sympantos architecture overview".

Scene: a compact Rust CLI/TUI control tower named "iota-sympantos" sits in the center like a tiny railway signal station. From the tower, five labeled rail lines run outward to five AI backend stations: Claude Code, Codex, Gemini CLI, Hermes, and OpenCode. Below the tower is a transparent underground cutaway showing Context Fabric, SQLite stores, ACP JSON-RPC pipes, MCP sidecars, telemetry instruments, and native projection workshops as connected rooms. Small human engineers in simple work clothes carry context capsules, memory ledgers, skill scrolls, and approval stamps between rooms. The story should feel like a busy but orderly miniature city where every subsystem has a job.

Composition: portrait poster, 2:3 aspect ratio. Strong central vertical hierarchy: Entry at the top, Presentation below it, Service Orchestration in the middle, Context/Protocol/Store/Runtime Events as four connected chambers, External Boundaries at the bottom. Use arrows, pipes, labels, and little signs, but keep all text short and readable. Add the title "iota-sympantos Architecture" as hand-lettered text at the top.

Style: elegant black-and-white steel-nib pen drawing, precise linework, dense but legible cross-hatching, technical diagram mixed with storybook world-building, subtle magenta accent only on the main tower beacon and status rail. No photorealism, no 3D render, no glossy UI mockup, no gradient background.

Mood: curious, clever, organized, a little whimsical, showing complex orchestration as a navigable machine-city.

Negative prompt: blurry text, unreadable labels, crowded random symbols, corporate stock art, neon cyberpunk, colored comic style, watercolor, oil paint, low-detail sketch, distorted terminals, broken arrows, fake code blocks.
```

---

## 2. 运行时执行流程

### 2.1 Prompt 执行时序

**用途**: 展示从用户输入到输出返回的完整执行时序，包括快速路径和完整执行路径

**Prompt**:
```
A technical execution flowchart representing the core prompt sequence of the "IotaEngine", styled as a colored-pen schematic on off-white paper with a Left-to-Right two-column layout.

Left Column: "Initialization & Fast Paths" (outlined in navy blue)
- Start Node: "TuiApp / CLI" (navy blue block) triggers submit(prompt) and calls IotaEngine::prompt_in_cwd_timed()
- "request_hash" block (forest green): calculates SHA256 of backend + null-byte + cwd + null-byte + prompt as idempotency key
- Decision Diamond (terracotta orange): "Is matched skill execution mode == MCP?"
  - If YES: route (orange arrow) to skill::runner::run_engine_skill() which calls mcp::client to execute local script tools, bypass ACP and exit
  - If NO: proceed
- Decision Diamond (terracotta orange): "CacheStore::find_completed_by_request_hash() matches?"
  - If YES: route (blue arrow) to read completed execution, return cached text (Replay hit), and exit
  - If NO: proceed
- Decision Diamond (terracotta orange): "CacheStore::find_running_by_request_hash() matches?"
  - If YES: route (red arrow) to wait for running execution ID to complete, join, and exit
  - If NO: proceed to CacheStore::begin_execution_with_id() to allocate a fencing token and start execution

Right Column: "Execution & Completion" (outlined in forest green)
- "Session Ledger Handoff" block (purple): calls SessionLedger::ensure_session() and SessionLedger::publish_handoff()
- "Recall" block (purple): calls MemoryStore::recall_buckets_with_thresholds() querying memory.sqlite
- "Compose Prompt" block (forest green): calls ContextEngine::compose_effective_prompt() to inject rendered memory buckets, git status workspace output, and skill index into an XML <iota-context> capsule, limited by character budgets
- "AcpClient" block (terracotta): calls ensure_session_timed(). If CWD changes, sends session/new (injecting context and fun mcpServers list). Then sends session/prompt request to backend stdin
- "Read Loop" block (terracotta): polls stdout line-by-line using BufReader::lines():
  - If session/update -> stream text chunks to TUI mpsc channel (blue line)
  - If session/request_permission -> check tool_whitelist or request confirmation in TUI overlay (red line)
  - If tools/call -> call mcp::router::try_intercept_tool_call() to execute memory search/write locally (purple line), then write JSON-RPC result back to stdin
  - If session/complete or prompt response matches -> exit loop
- End Node: calls MemoryStore::insert(episodic), writes CacheStore::finish_execution(), and returns text output

Connections:
- Draw horizontal colored arrows from the Left Column decision paths to the Right Column execution blocks

Style instructions:
- Colored pen-on-paper schematic style on off-white background
- Clean sans-serif typeface, consistent font size within each hierarchical level
- Harmonious color coding for components
- Continuous, solid lines with no overlapping or broken strokes, generous white borders
```

---

### 2.2 代码调用链路

**用途**: 展示从入口点到运行时边界的完整代码调用链，包括直接路径和 Daemon 路径

**Prompt**:
```
Use gpt-image-2-style-library: pen-and-ink technical story poster, sequential flow diagram, hand-lettered annotations, fine cross-hatching, mechanical process narrative, warm paper texture.

Create a vertical poster for the document "iota-sympantos code call chains".

Scene: depict a journey of one prompt as a small sealed message capsule traveling through a mechanical dispatch system. It begins at src/main.rs, enters cli::run(), passes command switches for run, tui, check, bench, logs, trace, context-mcp, fun-mcp, native-materialize, and __daemon, then splits into two illustrated paths: the direct ACP route and the daemon TCP route.

Direct route moves through:
- Parsing, configuration
- Engine orchestration
- Cache replay (request_hash lookup)
- Memory recall (MemoryStore::recall_buckets_with_thresholds)
- Skill matching (SkillRegistry)
- Context capsule assembly (ContextEngine::compose_effective_prompt)
- ACP session creation (AcpClient::ensure_session_timed)
- Streaming updates (session/update)
- Final output

Daemon route shows:
- Local TCP gate at 127.0.0.1:47661
- Warm engine pooling (EnginePool keyed by cwd)
- Response returning to the user

Composition: portrait poster, 2:3 aspect ratio. 
Arrange the call chain as a large board-game-like path with numbered stations and arrows. 
Put "initialize -> session/new -> session/prompt -> session/update -> session/complete" as a clear ribbon across the middle. 
Show external boundaries as illustrated gates: git subprocess, ACP child process, MCP stdio sidecar, SQLite files, and TCP socket. 
Add the title "Code Call Chains" at the top and a small subtitle "from entry point to runtime boundary".

Style: black ink steel-nib illustration, crisp contour lines, engineering notebook feel, fine stippling and cross-hatching, readable miniature labels, light magenta accent on the active message capsule and protocol ribbon. Keep it playful through the journey metaphor, but technically accurate and structured.

Mood: adventurous debugging map, a prompt crossing checkpoints and machines, clear enough to teach the runtime path at a glance.

Negative prompt: unreadable spaghetti arrows, random pseudo-code, fantasy map parchment cliches, photorealistic devices, glossy dashboard, cyberpunk glow, excessive colors, cluttered icons, distorted terminal text.
```

---

### 2.3 Backend 调用与 IPC

**用途**: 展示 Backend 调用的三个阶段：预处理、IPC 调用、后处理

**Prompt**:
```
A technical column-based layout diagram titled "Backend Call Stages and IPC Interface", rendered in a colored-pen schematic on off-white paper. 
The diagram is split into three vertical sections showing the exact execution and tool invocation pipelines:

Column 1: "Preprocessing Stage & Context Loading" (outlined in forest green)
- Labeled "Memory Recall Pipeline": Shows MemoryStore::recall_buckets_with_thresholds() reading memory.sqlite to load RecallBuckets
- Labeled "Skill Registry Pipeline": Shows SkillRegistry loading and matching skill triggers
- Labeled "Prompt Assembly": Shows ContextEngine::compose_effective_prompt() combining matched skills, recalled memory buckets, and workspace git status into the XML <iota-context> capsule, with character-level budget trimming
- Labeled "Telemetry Span": Shows global tracer initializing and binding execution context

Column 2: "Backend Call (IO/IPC) Stage" (outlined in terracotta orange)
- AcpClient spawning subprocess via tokio::process::Command with stdin/stdout/stderr piped and kill_on_drop(true)
- Labeled pipelines showing structured JSON-RPC messages written to child stdin and read from child stdout
- Labeled "Daemon IPC boundary" (purple block) showing TCP Socket at 127.0.0.1:47661, regulated by Semaphore(8) concurrency control and matched via EnginePool CWD lookup

Column 3: "Postprocessing Stage & Execution Routes" (outlined in charcoal gray)
- Labeled "MCP Interception Route": Shows mcp::router intercepting tools/call (memory search/write tools) to bypass backend and call local handlers
- Labeled "Memory Write Pipeline": Shows MemoryStore::insert(episodic) persisting session memories back to memory.sqlite
- Labeled "Skill Executor Route": Shows skill::runner::run_engine_skill() launching local script tools via mcp::client
- Labeled exception handler (red block) showing terminate_child_tree cleaning up residual process trees

Connections and Flow:
- Distinct colored arrows show trace pipelines (purple for memory data flow, orange for skill loading/execution, blue for MCP tool routing, red for process execution)

Style instructions:
- Colored pen-on-paper schematic style on off-white background
- Clean sans-serif typeface, consistent font size within each hierarchical level
- Continuous, solid lines with no overlapping or broken strokes, generous white borders
```

---

## 3. Context Fabric 与记忆系统

### 3.1 记忆分类与生命周期

**用途**: 展示记忆系统的数据库模式、分类体系和生命周期管理

**Prompt**:
```
A technical data-mapping and database schema diagram titled "Memory Taxonomy, Scopes and Lifecycles", rendered in a colored-pen schematic style on off-white paper. 
The diagram contains three interconnected schema blocks:

Left Block: "SQLite Table Schema & Visibility Candidates" (outlined in purple)
- Table "memory": columns (id [TEXT PRIMARY KEY], type [TEXT CHECK(type IN ('semantic','episodic','procedural'))], facet [TEXT CHECK(facet IN ('identity','preference','strategic','domain'))], scope [TEXT CHECK(scope IN ('session','project','user','global'))], scope_id [TEXT], content [TEXT], content_hash [TEXT], confidence [REAL], expires_at [INTEGER], created_at, updated_at, supersedes, source_backend, source_session_id, source_execution_id, metadata_json, ttl_days, owner, visibility)
- Table "memory_embedding": columns (memory_id [TEXT PRIMARY KEY, FK], vector_blob [BLOB], updated_at)
- Table "sessions": columns (iota_session_id [TEXT PRIMARY KEY], cwd [TEXT], active_backend [TEXT], model [TEXT], turn_count [INTEGER], created_at, last_used_at)
- Table "backend_sessions", table "turns", and table "handoffs" under sessions.sqlite
- Shows user_scope_candidates(user_id) and project_scope_candidates(project_id) filtering scope_id

Center Block: "Recall Buckets Mapping" (outlined in forest green)
Map lines connect filtered memory records into 6 output buckets of RecallBuckets based on RecallThresholds:
- "identity" bucket (scope=User, type=Semantic, facet=Identity, threshold >= 0.85) -> injected as <memory type="identity"> (blue outline)
- "preference" bucket (scope=User, type=Semantic, facet=Preference, threshold >= 0.80) -> injected as <memory type="preference"> (blue outline)
- "strategic" bucket (scope=Project, type=Semantic, facet=Strategic, threshold >= 0.80) -> injected as <memory type="strategic"> (blue outline)
- "domain" bucket (scope=Project, type=Semantic, facet=Domain, threshold >= 0.80) -> injected as <memory type="domain"> (blue outline)
- "procedural" bucket (scope=Project, type=Procedural, threshold >= 0.75) -> injected as <memory type="procedural"> (blue outline)
- "episodic" bucket (scope=Session/Project, type=Episodic, threshold >= 0.70) -> injected as <memory type="episodic"> (blue outline)

Right Block: "Lifecycle Controls" (outlined in terracotta orange)
- "Deduplication" (orange): ON CONFLICT(scope, scope_id, type, facet, content_hash) DO UPDATE SET updated_at, expires_at, confidence=MAX(memory.confidence, excluded.confidence)
- "TTL Expiry" (orange): filters expires_at > now
- "Compaction" (orange): compact_episodic_scope() deletes episodic records offset by episodic_compaction_keep

Solid lines with arrows trace the lifecycles from DB tables, through scope-filtering, into the XML prompt injection.

Style instructions:
- Colored pen-on-paper schematic style on off-white background
- Clean sans-serif typeface, consistent font size within each hierarchical level
- Harmonious color coding for components
- Continuous, solid lines with no overlapping or broken strokes, generous white borders
```

---

### 3.2 六桶记忆召回机制

**用途**: 展示六桶记忆系统的召回流程和向量/混合搜索机制

**Prompt**:
```
A technical flow diagram titled "Six-Bucket Memory Recall and Search Mechanisms", rendered in a colored-pen schematic style on off-white paper.

The diagram shows three interconnected sections:

Left Section: "Recall Query Input" (outlined in navy blue)
- Input parameters: user_id, project_id, session_id, RecallThresholds
- user_scope_candidates(user_id) -> ["user-sympantos", "local-user", user_id]
- project_scope_candidates(project_id) -> [project_id, "iota-sympantos", basename(project_id)]

Center Section: "Six-Bucket Filtering" (outlined in forest green)
Shows six parallel query paths, each with:
1. Identity: query(scope=User, scope_id IN user_ids, type=Semantic, facet=Identity, confidence >= 0.85, limit=20)
2. Preference: query(scope=User, scope_id IN user_ids, type=Semantic, facet=Preference, confidence >= 0.80, limit=30)
3. Strategic: query(scope=Project, scope_id IN project_ids, type=Semantic, facet=Strategic, confidence >= 0.80, limit=30)
4. Domain: query(scope=Project, scope_id IN project_ids, type=Semantic, facet=Domain, confidence >= 0.80, limit=50)
5. Procedural: query(scope=Project, scope_id IN project_ids, type=Procedural, confidence >= 0.75, limit=10)
6. Episodic: query(scope=Session, scope_id=session_id, type=Episodic, confidence >= 0.70, limit=20) + query(scope=Project, scope_id IN project_ids, type=Episodic, confidence >= 0.70, limit=20)

Each query path shows:
- SQL WHERE clause filters
- ORDER BY confidence DESC, updated_at DESC, created_at DESC
- LIMIT enforcement
- TTL expiry filter (expires_at > now)

Right Section: "Search Modes" (outlined in terracotta orange)
Shows three search strategies:
1. Keyword Search:
   - FTS5 (if available): memory_fts MATCH query, ORDER BY rank
   - Fallback LIKE: content LIKE '%query%' ESCAPE '\'
2. Vector Search:
   - Embed query using EmbeddingEngine (Ollama or local trigram)
   - Load memory_embedding.vector_blob for all candidates
   - Calculate cosine similarity
   - Score = 0.65 * similarity + 0.20 * token_overlap + 0.15 * confidence
   - Filter score > 0.05
3. Hybrid Search:
   - Run both keyword and vector searches with 3x limit
   - Merge results using reciprocal rank fusion (RRF)
   - Weight vector results 1.2x higher
   - Return top limit results

Bottom: "Output Assembly" (outlined in purple)
- RecallBuckets struct with six Vec<MemoryRecord> fields
- Injected into <iota-context> capsule as XML <memory type="..."> blocks
- Character budget enforcement by ContextEngine

Style instructions:
- Colored pen-on-paper schematic style on off-white background
- Clean sans-serif typeface, consistent font size within each hierarchical level
- Harmonious color coding for components
- Show SQL snippets in monospace font
- Continuous, solid lines with no overlapping or broken strokes, generous white borders
```

---

## 4. 观测性与调试

### 4.1 OpenTelemetry 观测性架构

**用途**: 展示完整的观测性架构，包括本地日志和 Docker 观测性栈

**Prompt**:
```
Use gpt-image-2-style-library: pen-and-ink technical story poster, observatory control room, signal routing diagram, fine cross-hatching, hand-drawn infrastructure map, warm paper texture.

Create a vertical poster for the document "iota observability".

Scene: show iota as a small command desk sending three kinds of signals into an old-fashioned observatory: logs, traces, and metrics. The signals travel through brass-labeled tubes into an OpenTelemetry Collector at the center. From there, three paths branch to Loki as a log archive library, Jaeger as a trace telescope charting spans across the sky, and Prometheus as a metric gauge wall with moving needles. Grafana appears as a large wall screen that combines the three views. A separate lower-left corner shows local operation without Docker: stderr, daily files under ~/.i6/logs/, and SQLite stores under ~/.i6/context/.

Composition: portrait poster, 2:3 aspect ratio. Central hub-and-spoke layout with the OTel Collector as the main switching lens. Put Docker observability stack on the right side and local fallback behavior on the left side. Include readable short labels: OTLP :4317, Loki :3100, Jaeger :16686, Prometheus :9090, Grafana :3000, OTEL_ENABLED=false, and iota logs / iota trace / iota metrics. Add the title "iota Observability" at the top.

Style: meticulous black-and-white pen drawing, Victorian scientific instrument meets modern infrastructure diagram, cross-hatching, stippled shadows, precise arrows, minimal magenta accent on live telemetry pulses. Avoid making it look like a generic cloud architecture slide.

Mood: investigative, transparent, a control room where every runtime signal can be followed from source to storage.

Negative prompt: blurry dashboards, unreadable labels, stock cloud icons, overbright neon, colorful SaaS illustration, 3D render, abstract blobs, random graphs without meaning, fake brand logos.
```

---

### 4.2 调试工作流

**用途**: 展示使用 VS Code 和 CodeLLDB 调试 iota-sympantos 的完整工作流

**Prompt**:
```
Use gpt-image-2-style-library: pen-and-ink technical story poster, debugging workshop, hand-drawn developer desk, fine cross-hatching, annotated troubleshooting map, warm paper texture.

Create a vertical poster for the document "iota-sympantos debugging guide".

Scene: an engineer sits at a VS Code workbench with CodeLLDB tools arranged like precision instruments. A large Rust CLI machine is open for inspection: main.rs, cli/mod.rs, engine.rs, acp/mod.rs, and tui.rs appear as labeled access panels. Breakpoints glow as tiny red pinheads on the machine. A side panel shows debug configurations as selectable brass tags: Debug TUI, Debug Run, Debug Run with Daemon, Debug Check, Debug Context MCP Sidecar, Debug Fun MCP Server, Debug Bench Cold, and Debug Daemon. At the bottom, a terminal recovery station shows RUST_LOG=debug, RUST_BACKTRACE=1, local log files, and a reset lever for raw terminal recovery.

Composition: portrait poster, 2:3 aspect ratio. Make the story read from top to bottom: prerequisites, configurations, breakpoints, stepping controls, variable inspection, TUI debugging, ACP subprocess boundary. Include a small keyboard strip with F5, F10, F11, Shift+F11, and Shift+F5. Add the title "Debugging iota-sympantos" at the top.

Style: black ink pen illustration, crisp technical linework, cross-hatched shadows, annotated workshop poster, readable labels, subtle magenta accent on breakpoint dots and the active debug path. It should feel practical, hands-on, and slightly playful without becoming cartoonish.

Mood: focused troubleshooting, a repair manual for a complex but understandable runtime.

Negative prompt: chaotic code wall, unreadable text, photorealistic office, glossy app screenshot, neon cyberpunk, excessive red, fantasy laboratory, distorted keyboards, fake stack traces, low-detail doodle.
```

---

## 附录：图表使用指南

### 生成建议

1. **技术架构图** (1.1, 1.2, 3.1, 3.2): 适合使用 GPT-4 with DALL-E 或 Midjourney，强调清晰的技术细节和准确的标签
2. **故事化海报** (1.3, 2.2, 4.1, 4.2): 适合使用 Midjourney 或 Stable Diffusion，强调艺术风格和叙事性

### 风格一致性

所有图表遵循统一的视觉语言：
- **技术图**: 彩色笔绘示意图风格，米白色背景，清晰的无衬线字体
- **海报图**: 钢笔墨水插画风格，精细交叉阴影，温暖纸质纹理，洋红色点缀

### 更新维护

当代码结构发生变化时，需要更新的图表：
- 新增模块 → 更新 1.1, 1.2
- 执行流程变化 → 更新 2.1, 2.3
- 记忆系统变化 → 更新 3.1, 3.2
- 观测性变化 → 更新 4.1

---

**文档版本**: 2024-01 (基于 AGENTS.md 最新实现)
**维护者**: iota-sympantos 开发团队
