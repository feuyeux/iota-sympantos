# iota-sympantos code call chains

This document traces the current code call chains by entry point and runtime boundary, with emphasis on IPC, subprocess, network, and persistence boundaries. For the layered architecture see [architecture.md](architecture.md).

## Entry overview

```text
src/main.rs
  -> cli::run()
```

`main.rs` only registers modules and starts the Tokio runtime. All user-visible entry points are dispatched by `src/cli/mod.rs`.

## CLI command dispatch

```text
cli::run()
  -> telemetry::init(TelemetryConfig::default())
  -> std::env::args().skip(1)
  -> match first arg:
       "run"                -> ACP prompt path
       "context-mcp"        -> context::server::run_stdio()
       "fun-mcp"            -> skill::fun_server::run_stdio()
       "native-materialize" -> run_native_materialize()
       "logs"               -> query Loki
       "trace"              -> query Jaeger
       "skill"              -> run_skill_command()
       "__daemon"           -> daemon::run_daemon()
       "check"              -> optional warm daemon + print_combined_info()
       "tui"                -> tui::run()
       "bench-cold"         -> run_cold_benchmark() or daemon benchmark
       "bench-warm"         -> run_warm_benchmark() or daemon benchmark
       no args              -> tui::run()
```

Global constraints:

- Config is read only from `~/.i6/nimia.yaml` via `config::read_config()`.
- `iota run --daemon` cannot be combined with `--show-native`.
- `--log-events` outputs normalized runtime events; `--timing` outputs route/ACP timing JSON.

## Path 1: CLI direct ACP backend execution

```text
iota run [backend] [options] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
       -> backend defaults to Codex
       -> --backend / backend alias
       -> --cwd
       -> --show-native
       -> --log-events / --timing
       -> --timeout-ms
       -> prompt from args or stdin
  -> config::read_config()
  -> IotaEngine::new_for_session_cwd(config, show_native, timeout_ms, None)
       -> EffectiveConfig::from_config()
       -> ContextEngine::from_config()
  -> MemoryStore::open_with_embedding(memory_db, embedding_config)
       -> CacheStore::open(events.sqlite)
       -> SessionLedger::open(sessions.sqlite)
       -> latest_session_for_cwd() or new UUID session
  -> IotaEngine::prompt_in_cwd_timed(backend, cwd, prompt)
  -> print output text
  -> optional log events / timing to stderr
  -> IotaEngine::shutdown()
       -> AcpClient::shutdown()
```

Engine internal call chain:

```text
IotaEngine::prompt_in_cwd_timed_with_execution_id()
  -> request_hash(backend, cwd, prompt)
  -> SkillRegistry::load_cached()
       -> workspace/skills
       -> workspace/.iota/skills
       -> configured skill_roots
       -> ~/.i6/skills
  -> SkillRegistry::match_skill()
  -> compute skip_replay:
       matched skill
       memory query
       memory-classifiable prompt
       explicit iota_memory_write
  -> if !skip_replay:
       -> CacheStore::find_completed_by_request_hash()
       -> CacheStore::output_text()
       -> return synthetic output on cache hit
  -> if !skip_replay:
       -> CacheStore::find_running_by_request_hash()
       -> poll CacheStore::get_execution() until completed/failed/timeout
       -> return synthetic output on joined running execution
  -> ensure_session_ledger()
       -> SessionLedger::ensure_session()
       -> SessionLedger::record_backend_session()
  -> prepare_handoff()
       -> SessionLedger::publish_handoff()
       -> MemoryStore::insert(handoff episodic memory)
  -> CacheStore::begin_execution_with_id()
       -> idempotency lock
       -> stale running cleanup
       -> fencing token allocation
  -> record RuntimeEvent::State(started)
  -> extract_structured_memories()
  -> optional memory-write-only short circuit
  -> optional engine-run skill short circuit
  -> memory recall
       -> MemoryStore::recall_buckets_with_thresholds()
       -> record RuntimeEvent::Memory(inject)
  -> optional deterministic memory answer short circuit
  -> ContextEngine::compose_effective_prompt() via spawn_blocking
       -> render_workspace()
            -> [child process] git status --short
  -> ensure_client()
  -> AcpClient::prompt_with_cwd_timed_for_execution()
  -> record RuntimeEvent list
  -> OTel tracing/metrics record timing and status
  -> CacheStore::finish_execution()
  -> SessionLedger::record_turn()
  -> DialogueBuffer::push_turn()
  -> MemoryStore::insert(episodic prompt/output memory)
```

IPC / external boundaries:

- `git status --short` is a synchronous subprocess; the engine wraps context assembly in `spawn_blocking`.
- The ACP backend is a child process running newline-delimited JSON-RPC 2.0 over stdin/stdout.
- `CacheStore`, `MemoryStore`, and `SessionLedger` are SQLite file boundaries.

## Path 2: ACP client protocol driver

Startup chain:

```text
IotaEngine::ensure_client()
  -> effective_config.backend_config(backend)
  -> backend_process_env_with_context()
  -> normalized_acp_command()
  -> context_mcp_servers()
  -> context_session_options()
  -> context_tool_whitelist()
  -> AcpClient::start()
       -> resolve command from config or AcpBackend::command()
       -> TokioCommand::new(command)
            .args(args)
            .envs(env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
       -> spawn stderr reader task
       -> send_request("initialize")
       -> wait_for_response()
```

Prompt chain:

```text
AcpClient::prompt_with_cwd_timed_for_execution()
  -> ensure_session_timed()
       -> session_new_params_with_options()
       -> send_request("session/new")
       -> wait_for_response()
       -> store backend session_id if available
  -> send_request("session/prompt")
  -> read_prompt_events_for_id()
       -> wire::read_next_line()
       -> wire::parse_message_line()
       -> if response id matches prompt id:
            collect final result
       -> if method event:
            runtime_event::map_acp_events()
            stream output chunks to TUI sender when installed
       -> if session/request_permission:
            acp::permission::answer_permission_request()
       -> if tools/call style method:
            mcp::router::try_intercept_tool_call()
       -> stop on session/complete or prompt response
```

ACP protocol order:

```text
initialize
  -> session/new
  -> session/prompt
  -> session/update ...
  -> session/request_permission? ...
  -> session/complete
```

Helper modules:

| Module | Call site | Responsibility |
|---|---|---|
| `acp/wire.rs` | `read_prompt_events_for_id()`, `wait_for_response()` | Line read with timeout, JSON parse, response id matching, error formatting |
| `runtime_event.rs` | ACP event loop | Normalize update/complete/permission/usage/tool/error to `RuntimeEvent` |
| `acp/permission.rs` | permission request | Auto-approve iota tool/whitelist; otherwise route to TUI/stdin |
| `mcp/router.rs` | ACP tool-call event | Route iota tools; reject external tools |
| `acp/session.rs` | `ensure_session_timed()` | Render `cwd` and `mcpServers` |

## Path 3: CLI via daemon

Client chain:

```text
iota run --daemon [backend] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
  -> run_prompt_via_daemon()
  -> send_prompt_autostart_daemon()
       -> daemon::send_prompt(addr, DaemonPromptRequest)
            -> [TCP] connect daemon_addr()
            -> write one JSON line
            -> read one JSON line DaemonPromptResponse
       -> if connect failed:
            -> start_daemon_silently()
                 -> [child process] current_exe __daemon
            -> wait_for_daemon()
            -> retry daemon::send_prompt()
  -> print response.text
  -> optional log events / timing
```

Daemon process chain:

```text
iota __daemon
  -> cli::run()
  -> config::read_config()
  -> daemon::run_daemon(config, addr, DEFAULT_TIMEOUT_MS, warm_on_start=false)
       -> cwd = current_dir()
       -> EnginePool::new()
       -> TcpListener::bind(addr)
       -> Semaphore::new(8)
       -> install Ctrl+C CancellationToken
       -> accept loop
            -> tokio::spawn(handle_connection)
```

Connection handling:

```text
daemon::handle_connection()
  -> read one line, max 10 MiB
  -> parse serde_json::Value
  -> if type == "warm":
       -> decode DaemonWarmRequest
       -> handle_warm()
     else:
       -> decode DaemonPromptRequest
       -> handle_prompt()
  -> write one JSON line DaemonPromptResponse
```

Prompt request:

```text
daemon::handle_prompt()
  -> AcpBackend::parse(request.backend)
  -> EnginePool::engine_for(cwd)
       -> create IotaEngine::new_for_session_cwd(..., Some(cwd)) if absent
  -> optional engine.set_timeout_ms(request.timeout_ms)
  -> IotaEngine::prompt_in_cwd_timed_with_execution_id()
       -> same engine + ACP chain as direct run
  -> DaemonPromptResponse { ok, text/error, timing, events }
```

Warm request:

```text
iota check --daemon / bench-* --daemon / internal warm path
  -> cli::warm_daemon_for_current_dir()
  -> daemon::send_warm(DaemonWarmRequest { type:"warm", cwd, backends })
  -> daemon::handle_warm()
       -> warm_all_backends() if backends empty
       -> warm_selected_backends() otherwise
       -> IotaEngine::warm_backend_in_cwd()
       -> ensure_client()
       -> AcpClient::start()
  -> DaemonPromptResponse { warmed }
```

Daemon shutdown:

```text
Ctrl+C in daemon process
  -> CancellationToken::cancel()
  -> engine_pool.all_engines()
  -> each IotaEngine::shutdown_all_clients()
  -> each AcpClient::shutdown()
```

IPC boundaries:

- CLI and daemon communicate over local TCP JSON lines, defaulting to `127.0.0.1:47661`, overridable via `IOTA_DAEMON_ADDR`.
- The daemon is started silently by the CLI as `current_exe __daemon`.
- The daemon's internal engine pool reuses engines by cwd, not by backend; backend-level reuse is handled inside `IotaEngine`'s `(backend, cwd)` client pool.

## Path 4: TUI interactive execution

Initialization chain:

```text
iota / iota tui
  -> cli::run()
  -> config::read_config()
  -> tui::run(config)
       -> stdout is_terminal check
       -> TuiApp::new()
            -> IotaEngine::new_for_session_cwd(config, false, DEFAULT_TIMEOUT_MS, current_dir)
       -> acp::permission::install_tui_approval_channel()
       -> set panic hook
       -> enter alternate screen
       -> enable raw mode
       -> enable mouse capture
       -> TerminalGuard owns cleanup
       -> run_loop()
```

Event loop:

```text
tui::run_loop()
  -> crossterm EventStream
  -> frame tick limiter around 120 FPS
  -> keyboard/mouse/resize events
  -> Composer::handle_key()
       -> submit/newline/history/search/word motion/kill/yank
  -> TuiApp::submit()
       -> enqueue or start prompt
  -> when prompt starts:
       -> tokio::spawn(engine task)
            -> IotaEngine::set_stream_sender(Some(tx))
            -> IotaEngine::prompt_in_cwd_timed()
            -> IotaEngine::set_stream_sender(None)
            -> send result to UI channel
  -> stream_rx receives output chunks
  -> approval_rx receives ApprovalRequest
  -> render()
       -> header/history/composer/status
       -> markdown::render()
       -> status_bar::render()
       -> overlays: help / pager / quit confirm / approval
```

Approval overlay:

```text
ACP backend sends session/request_permission
  -> acp::permission::answer_permission_request()
  -> TUI approval channel is installed
  -> send ApprovalRequest { tool_name, params, reply }
  -> TUI renders approval overlay
  -> user decision returns over oneshot
  -> send ACP JSON-RPC permission response
  -> ApprovalStore records request/decision
```

TUI boundaries:

- The TUI does not use the daemon; it holds an in-process `IotaEngine`.
- ACP stream to TUI is an in-process Tokio mpsc channel, not IPC.
- True external boundaries remain the ACP backend, MCP sidecar, git, SQLite, and function tools.

## Path 5: Context Fabric injection

```text
IotaEngine::prompt_in_cwd_timed_with_execution_id()
  -> MemoryStore::recall_buckets_with_thresholds()
       -> identity: semantic/identity/user
       -> preference: semantic/preference/user
       -> strategic: semantic/strategic/project
       -> domain: semantic/domain/project
       -> procedural: procedural/project
       -> episodic: episodic/session + episodic/project
  -> DialogueBuffer::render()
  -> prepare_handoff()
  -> ContextEngine::compose_effective_prompt()
       -> <iota-context>
            <session>
            <memory-tools>
            <model> optional
            <memory> buckets
            <dialogue>
            <workspace>
            <skills>
            <handoff>
          </iota-context>
       -> "User request:"
       -> original prompt
```

Workspace summary:

```text
ContextEngine::compose_effective_prompt()
  -> render_workspace(cwd)
       -> [child process] git status --short
       -> take first 20 changed lines
```

Context disabled path:

```text
context_engine.enabled = false
or context_engine.injection = off
  -> ContextEngine.enabled = false
  -> compose_effective_prompt() returns original prompt
```

## Path 6: Memory write, search, and embedding

Engine automatic write:

```text
completed ACP/skill output
  -> IotaEngine::write_episodic_memory()
  -> MemoryStore::insert()
       -> insert_with_merge(..., MemoryMergeMode::Auto)
       -> validate taxonomy
       -> dedup by scope/scope_id/type/facet/content_hash
       -> upsert_embedding()
       -> SQLite memory + memory_embedding
```

LLM-initiated write:

```text
ContextEngine injects <memory-tools>
  -> backend LLM calls iota_memory_write
  -> ACP backend sends session/request_permission
  -> acp::permission::answer_permission_request()
       -> tool_name starts with iota_ or mcp__iota-
       -> auto approve
       -> send option outcome if options exist, otherwise {approved:true}
  -> backend calls MCP sidecar tool
  -> context::server::call_tool("iota_memory_write")
       or mcp::router::route_memory_write()
  -> MemoryStore::insert_with_merge()
```

Memory search:

```text
iota_memory_search { query, limit, mode }
  -> mode defaults to hybrid
  -> MemoryStore::search_with_mode()
       keyword:
         -> FTS5 phrase search if available
         -> fallback LIKE
       vector:
         -> EmbeddingEngine::embed(query)
              -> Ollama /api/embeddings if this store was opened with embedding config
              -> local trigram fallback if API absent/fails
         -> cosine(vector, memory_embedding.vector_blob)
         -> score = similarity + token overlap + confidence
       hybrid:
         -> merge keyword and vector rankings
```

The memory store opened by `IotaEngine` uses `context_engine.embedding`. `context-mcp` and `mcp::router` currently open the default store via `MemoryStore::open()`, so MCP query-side uses the local trigram fallback.

Embedding schema:

```text
memory_embedding
  memory_id TEXT PRIMARY KEY
  vector_blob BLOB NOT NULL
  updated_at INTEGER NOT NULL
```

## Path 7: Engine-run skill and MCP

Trigger chain:

```text
SkillRegistry::load_cached()
  -> parse skill frontmatter
  -> compatible_skills(backend)
  -> match_skill(backend, prompt)
       -> prompt lowercased contains any trigger
```

Execution chain:

```text
matched skill with execution.mode = "mcp"
  -> skill::runner::run_engine_skill(skill, prompt)
       -> server = execution.server or "iota-fun"
       -> server_command()
            "iota-fun"     -> current_exe fun-mcp
            "iota-context" -> current_exe context-mcp
            custom         -> custom command, no args
       -> build McpToolCall list
       -> if execution.parallel:
            -> run_batch()
            -> futures_util::future::join_all()
          else:
            -> run_sequential()
       -> mcp::client::call_stdio()
            -> [child process + stdio JSON-RPC] MCP server
            -> initialize
            -> notifications/initialized
            -> tools/call
       -> render_template()
       -> replace {{alias}} and {{tool_results}}
       -> SkillRunOutput { text, events }
  -> engine records ToolCall/ToolResult/Output
  -> finish execution without ACP backend prompt
```

Boundaries:

- Each `call_stdio()` starts a new MCP server subprocess.
- Parallel skills launch multiple tool calls concurrently.
- When an engine-run skill is matched, the ACP backend can be bypassed entirely.

## Path 8: MCP sidecar — iota-context

Launch:

```text
iota context-mcp
  -> cli::run()
  -> context::server::run_stdio()
```

Initialization:

```text
context::server::run_stdio()
  -> MemoryStore::default_path() + MemoryStore::open()
       -> no context_engine.embedding config is loaded here
  -> workspace = current_dir()
  -> SkillRegistry::load(workspace, [])
  -> SessionLedger::default_path() + SessionLedger::open()
  -> stdin line loop
  -> handle_request()
```

JSON-RPC methods:

```text
initialize
  -> protocolVersion 2024-11-05
  -> capabilities tools/resources
  -> serverInfo iota-context

tools/list
  -> tools()

tools/call
  -> call_tool()
       iota_memory_search
       iota_memory_write
       iota_skill_search
       iota_skill_load
       iota_session_summary
       iota_handoff_publish
       iota_handoff_read

resources/list
  -> iota://memory/project/local
  -> iota://skill/index
  -> iota://session/local/summary
  -> iota://workspace/local/rules

resources/read
  -> read_resource()
```

Callers:

- The ACP backend starts this server based on `session/new.mcpServers`.
- `skill::runner` can start it as an engine-run MCP skill server.

## Path 9: MCP sidecar — iota-fun

Launch:

```text
iota fun-mcp
  -> cli::run()
  -> skill::fun_server::run_stdio()
```

JSON-RPC methods:

```text
initialize
tools/list
tools/call
  -> run_tool()
```

Tool execution chain:

```text
fun.python
  -> run_interpreter("python3", ["-c", source])

fun.typescript
  -> run_interpreter("node", ["-e", source])

fun.rust
  -> write_source(main.rs)
  -> rustc
  -> compiled binary

fun.go
  -> write_source(main.go)
  -> go run

fun.java
  -> write_source(Main.java)
  -> javac
  -> java -cp

fun.cpp
  -> write_source(main.cpp)
  -> clang++ or g++
  -> compiled binary

fun.zig
  -> write_source(main.zig)
  -> zig run
```

Boundaries:

- `fun-mcp` is itself a stdio JSON-RPC MCP server.
- Language runners call interpreters, compilers, or compiled binaries via `std::process::Command`.
- Compilation cache is at `~/.i6/fun-cache/<language>/<hash>`.

## Path 10: Backend-started MCP server rendering

Configuration chain:

```text
EffectiveConfig::from_config()
  -> context_mcp_servers(config, backend)
       -> context_mcp_session_enabled()
       -> context_engine.enabled and injection != off
       -> command_to_mcp_server("iota-context", context_engine.mcp, ["context-mcp"])
       -> command_to_mcp_server("iota-fun", context_engine.fun, ["fun-mcp"])
  -> context_session_options(config, backend)
       -> always_send_empty_mcp_servers
       -> mcp_env_shape
```

session/new parameters:

```text
acp::session::session_new_params_with_options()
  -> cwd = cwd.display().to_string()
  -> render_mcp_server(server, env_shape)
       -> {
            "name": server.name,
            "type": "stdio",
            "command": server.command,
            "args": server.args,
            "env": ["K=V"] or {"K":"V"}
          }
  -> if servers empty and not required:
       { "cwd": cwd }
     else:
       { "cwd": cwd, "mcpServers": [...] }
```

Default injection rules:

| Backend | Injects `mcpServers` by default |
|---|---|
| Claude Code | Only when `context_engine_backend.claude-code.mcp_session_new` is `true/try/on` |
| Codex | Only when `context_engine_backend.codex.mcp_session_new` is `true/try/on`; sends `mcpServers` even when empty |
| Gemini | Enabled by default |
| Hermes | Enabled by default |
| OpenCode | Enabled by default |

`mcp_session_new: try` is treated as enabled for Claude Code and Codex, and as disabled for other backends.

## Path 11: Permission and MCP router

Permission request:

```text
ACP backend -> session/request_permission
  -> runtime_event::map_acp_events()
       -> RuntimeEvent::ApprovalRequest
  -> acp::permission::answer_permission_request()
       -> extract tool_name from:
            toolName
            name
            tool
            toolCall.title
       -> is_iota_tool if:
            starts_with("iota_")
            contains("__iota_")
            starts_with("mcp__iota-")
       -> whitelist_hit via backend tool_whitelist
       -> if auto approved:
            send_approved_response()
       -> else if TUI channel installed:
            send ApprovalRequest to TUI
            wait oneshot
       -> else:
            ApprovalStore::record_request()
            classify_operation()
            prompt_yes_no()
            ApprovalStore::record_decision()
       -> send approved/denied response
       -> return ApprovalDecisionEvent
```

Response shape:

```text
if params.options contains allow_always / allow / allow*:
  -> { "outcome": { "outcome": "selected", "optionId": option_id } }
else approved:
  -> { "approved": true }
else denied and reject option exists:
  -> { "outcome": { "outcome": "selected", "optionId": "reject" } }
else:
  -> { "approved": false }
```

Router:

```text
mcp::router::try_intercept_tool_call(method, params)
  -> only handles:
       tools/call
       mcp/tools/call
       mcp/tool_call
  -> route_tool_call(name, arguments)
       iota_memory_search
       iota_memory_write
       iota_skill_search
       iota_skill_load
       iota_session_summary
       iota_handoff_publish
       iota_handoff_read
       fun.* tools
       iota_* unknown -> routable error
       external unknown -> denied by iota policy
```

## Path 12: Telemetry queries, check, benchmark, native, skill pull

Telemetry query commands:

```text
iota logs <execution_id>
  -> cli::run_logs_command()
  -> IOTA_LOKI_URL or http://localhost:3100
  -> Loki query_range API:
       1. {service_name="iota", execution_id="<execution_id>"}
       2. {service_name="iota"} |= "<execution_id>"
       3. {service_name="iota"}
  -> client-side filter by stream execution_id label or line text
  -> print matching log lines

iota trace <trace_id>
  -> cli::run_trace_command()
  -> IOTA_JAEGER_URL or http://localhost:16686
  -> Jaeger /api/traces/<trace_id>
  -> print span names and durations

iota trace --execution <execution_id>
  -> cli::run_trace_command()
  -> query Loki with same fallback sequence as iota logs
  -> extract trace_id / traceid / traceId from stream labels or JSON/text log lines
  -> query Jaeger /api/traces/<trace_id>
  -> print span names and durations

iota metrics [--once|--listen <addr>]
  -> cli::run_metrics_command()
  -> CacheStore::metrics_snapshot()
  -> format local CacheStore counters as Prometheus text
  -> stdout with --once, or HTTP /metrics when listening
```

The old `iota observability` / `iota obs` command group is not present in the current CLI.

Check:

```text
iota check [--daemon]
  -> if --daemon:
       -> warm_daemon_for_current_dir(Vec::new())
  -> config::read_config()
  -> print_combined_info()
       -> for ALL_BACKENDS:
            backend_config()
            command_label()
            configured_model()
            version_mapping
            enabled/check status
  -> JSON stdout
```

Benchmark:

```text
iota bench-cold [rounds]
  -> for each enabled backend and each round:
       -> new IotaEngine
       -> prompt_in_cwd("ping")
       -> shutdown

iota bench-warm [rounds]
  -> one IotaEngine
  -> warm_enabled_backends_in_cwd()
  -> for each warmed backend and each round:
       -> prompt_in_cwd("ping")

iota bench-* --daemon
  -> run_daemon_benchmark()
  -> repeated send_prompt_autostart_daemon("ping")
```

Native materialize:

```text
iota native-materialize [--dry-run] <path> [content]
  -> native::dry_run() or native::apply()
  -> replace <!-- IOTA_START --> ... <!-- IOTA_END --> block

iota native-materialize [--dry-run] --backend <name> [workspace]
  -> native::backend_memory_path()
       ClaudeCode -> workspace/MEMORY.md
       Gemini     -> ~/.gemini/GEMINI.md
       OpenCode   -> workspace/AGENTS.md
       Codex      -> workspace/AGENTS.md
       Hermes     -> None

iota native-materialize [--dry-run] --all --backend <name> [workspace]
  -> config::read_config()
  -> SkillRegistry::load()
  -> MemoryStore::open()
  -> native::dry_run_backend_projection()
       -> memory projection
       -> compatible skill projection
  -> optional apply()
```

Skill pull:

```text
iota skill pull <source> [name]
  -> skill::cache::pull_skill()
       -> local path copy or HTTP(S) GET via reqwest
       -> sanitize destination name
       -> write into ~/.i6/skills
  -> print JSON { path }
```

## Store subsystem call chains

MemoryStore:

```text
MemoryStore::open_with_embedding(path, config)
  -> create parent dirs
  -> Connection::open()
  -> init_schema()
       -> memory
       -> memory_embedding
       -> indexes
       -> FTS5 table/triggers if available
  -> EmbeddingEngine::from_config(config)

insert_with_merge()
  -> validate_taxonomy()
  -> content_hash()
  -> existing exact duplicate?
       -> update timestamps/confidence/embedding
  -> related memory?
       -> auto/update/add/none merge handling
  -> insert memory
  -> upsert_embedding()

recall_buckets_with_thresholds()
  -> query / query_many
  -> sort by confidence and recency
  -> return six buckets

search_with_mode()
  -> keyword/vector/hybrid
```

CacheStore:

```text
CacheStore::open()
  -> cache_executions table
  -> cache_outputs table
  -> purge completed/failed cache records older than 30 days

begin_execution_with_id()
  -> transaction immediate
  -> stale running cleanup
  -> execution_id conflict detection
  -> fencing token allocation
  -> insert running execution

append_output()
  -> store only RuntimeEvent::Output for later replay

finish_execution()
  -> update status and finished_at

find_completed_by_request_hash()
find_running_by_request_hash()
output_text()
  -> replay and join-running support

request_hash()
  -> SHA-256 over backend, cwd, and prompt
```

SessionLedger:

```text
SessionLedger::open()
  -> sessions
  -> backend_sessions
  -> turns
  -> handoffs

latest_session_for_cwd()
ensure_session()
record_backend_session()
record_turn()
publish_handoff()
read_handoff()
summary()
```

ApprovalStore:

```text
ApprovalStore::open_default()
record_request()
record_decision()
classify_operation()
default_decision()
```

EmbeddingEngine:

```text
EmbeddingEngine::from_config()
  -> if base_url exists:
       reqwest blocking client with 15s timeout
     else:
       local only

embed(content)
  -> canonicalize()
  -> if API configured:
       POST {base_url}/api/embeddings
       body { model, prompt }
       optional bearer auth
       parse embedding
       on failure warn and fallback
  -> local_trigram()
       128-dim hash projection
       normalize()
```

## Module coverage table

| Module | Main responsibility | Paths covered |
|---|---|---|
| `main.rs` | Tokio entry point | Entry overview |
| `cli/mod.rs` | Command dispatch, daemon autostart, bench, logs/trace, native, skill | 1,3,4,12 |
| `config.rs` | `~/.i6/nimia.yaml`, EffectiveConfig, backend command/env, MCP/session options, embedding config | 1,2,3,10 |
| `engine.rs` | Core orchestration, replay/join, memory, skill, context, ACP pool, store writeback | 1,3,4,5,6,7 |
| `acp/mod.rs` | ACP backend, subprocess, JSON-RPC, prompt event loop | 1,2 |
| `acp/session.rs` | session/new and mcpServers | 2,10 |
| `acp/wire.rs` | ACP line read/parse/id/error | 2 |
| `acp/permission.rs` | ACP permission, auto approve, TUI/stdin approval | 4,11 |
| `runtime_event.rs` | Event normalization | 1,2,11,12 |
| `daemon/mod.rs` | Daemon TCP server, warm/prompt, graceful shutdown | 3 |
| `daemon/pool.rs` | Reuse IotaEngine per cwd | 3 |
| `daemon/proto.rs` | Daemon wire types | 3 |
| `tui.rs` | TUI main loop, engine task, stream/approval channel | 4 |
| `tui/composer.rs` | Input editor | 4 |
| `tui/markdown.rs` | Markdown rendering | 4 |
| `tui/status_bar.rs` | Status bar | 4 |
| `tui/theme.rs` | TUI styles | 4 |
| `tui/state.rs` | Conversation and observability state | 4 |
| `context/mod.rs` | Context capsule, DialogueBuffer, workspace summary | 5 |
| `context/server.rs` | iota-context MCP server | 6,8 |
| `skill/mod.rs` | Skill loading, trigger, backend compatibility | 5,7,8,12 |
| `skill/runner.rs` | Engine-run MCP skill | 7 |
| `skill/cache.rs` | Skill pull/cache | 12 |
| `skill/fun_server.rs` | iota-fun MCP server and language execution | 7,9 |
| `mcp/client.rs` | stdio MCP client | 7 |
| `mcp/router.rs` | ACP tool-call intercept | 6,11 |
| `native/mod.rs` | Native file projection | 12 |
| `store/memory.rs` | Memory taxonomy, recall, search, merge, TTL | 5,6,8,11 |
| `store/embedding.rs` | API/local embedding, cosine, blob encode/decode | 6 |
| `store/cache.rs` | Execution replay/dedupe cache | 1,3,4,12 |
| `telemetry/mod.rs` | OTel provider/exporter initialization | Entry overview, 12 |
| `telemetry/metrics.rs` | OTel instruments | 1,3,4,12 |
| `store/ledger.rs` | Session/backend session/turn/handoff | 1,5,8,11 |
| `store/approval.rs` | Approval events and risk classification | 11 |
| `utils.rs` | Timestamps, summarization, lock recovery | Multiple paths |

## Inter-process and external call inventory

| Location | Type | Initiator | Target | Protocol/purpose |
|---|---|---|---|---|
| `cli::start_daemon_silently()` | child process | CLI | `iota __daemon` | daemon autostart |
| `daemon::send_prompt()` / `send_warm()` | TCP | CLI | daemon | JSON line request/response |
| `AcpClient::start()` | child process + stdio | engine | ACP backend | JSON-RPC 2.0 line protocol |
| `AcpClient::send_request()` | stdio | engine | ACP backend | `initialize/session/new/session/prompt` |
| `acp::permission::send_response()` | stdio | engine | ACP backend | permission decision |
| `session_new_params_with_options()` | delegated child process | ACP backend | MCP servers | `mcpServers` tells backend how to spawn sidecars |
| `mcp::client::call_stdio()` | child process + stdio | skill runner | MCP server | initialize/tools/call |
| `context::server::run_stdio()` | stdio server | ACP backend or skill runner | iota-context | MCP tools/resources |
| `skill::fun_server::run_stdio()` | stdio server | ACP backend or skill runner | iota-fun | MCP tools |
| `skill::fun_server::run_command()` | child process | iota-fun | language runtime/compiler | execute code snippets |
| `context::render_workspace()` | child process | context engine | `git` | `git status --short` |
| `skill::cache::pull_skill()` | network/filesystem | CLI | HTTP(S) URL or local path | fetch/copy skill |
| `EmbeddingEngine::embed_api()` | network | memory store | Ollama-compatible API | `/api/embeddings` |
| SQLite stores | filesystem | engine/MCP/CLI | `~/.i6/context/*.sqlite` | persistence |
