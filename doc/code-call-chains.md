# iota-sympantos 代码调用链

本文按入口和运行时边界梳理当前代码调用链，重点标注 IPC、子进程、网络和持久化边界。架构分层见 [architecture.md](architecture.md)。

## 入口总览

```text
src/main.rs
  -> cli::run()
```

`main.rs` 只注册模块并启动 Tokio runtime。所有用户可见入口由 `src/cli/mod.rs` 分发。

## CLI 命令分发

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

全局约束：

- 配置只由 `config::read_config()` 读取 `~/.i6/nimia.yaml`。
- `iota run --daemon` 不能与 `--show-native` 同用。
- `--log-events` 输出 normalized runtime events；`--timing` 输出 route/ACP timing JSON。

## 链路 1：CLI 直接运行 ACP 后端

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

Engine 内部调用链：

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

IPC / 外部边界：

- `git status --short` 是同步子进程，engine 用 `spawn_blocking` 包裹 context 组装。
- ACP backend 是 child process，stdin/stdout 上跑换行分隔 JSON-RPC 2.0。
- `CacheStore`、`MemoryStore`、`SessionLedger` 是 SQLite 文件边界。

## 链路 2：ACP client 协议驱动

启动链：

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

Prompt 链：

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

辅助模块：

| 模块 | 调用点 | 职责 |
|---|---|---|
| `acp/wire.rs` | `read_prompt_events_for_id()`, `wait_for_response()` | 带 timeout 的 line read、JSON parse、response id 判断、error 格式化 |
| `runtime_event.rs` | ACP event loop | update/complete/permission/usage/tool/error 到 `RuntimeEvent` |
| `acp/permission.rs` | permission request | 自动批准 iota tool/whitelist，或走 TUI/stdin |
| `mcp/router.rs` | ACP tool-call event | 路由 iota tools，拒绝外部 tools |
| `acp/session.rs` | `ensure_session_timed()` | 渲染 `cwd` 和 `mcpServers` |

## 链路 3：CLI 经 daemon 运行

客户端链：

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

Daemon 进程链：

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

Connection 处理：

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

Prompt 请求：

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

Warm 请求：

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

Daemon shutdown：

```text
Ctrl+C in daemon process
  -> CancellationToken::cancel()
  -> engine_pool.all_engines()
  -> each IotaEngine::shutdown_all_clients()
  -> each AcpClient::shutdown()
```

IPC 边界：

- CLI 和 daemon 使用本机 TCP JSON line，默认 `127.0.0.1:47661`，可由 `IOTA_DAEMON_ADDR` 覆盖。
- daemon 自身由 CLI 以 `current_exe __daemon` 静默启动。
- daemon 内部的 engine pool 按 cwd 复用 engine，不按 backend 分桶；backend 级复用在 `IotaEngine` 的 `(backend, cwd)` client pool 内完成。

## 链路 4：TUI 交互运行

初始化链：

```text
iota / iota tui
  -> cli::run()
  -> config::read_config()
  -> tui::run(config)
       -> stdout is_terminal 检查
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

事件循环：

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

Approval 浮层：

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

TUI 边界：

- TUI 不走 daemon；它持有进程内 `IotaEngine`。
- ACP stream 到 TUI 是进程内 Tokio mpsc，不是 IPC。
- 真正外部边界仍是 ACP backend、MCP sidecar、git、SQLite 和 function tools。

## 链路 5：Context Fabric 注入

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

Workspace summary：

```text
ContextEngine::compose_effective_prompt()
  -> render_workspace(cwd)
       -> [child process] git status --short
       -> take first 20 changed lines
```

Context disabled path：

```text
context_engine.enabled = false
or context_engine.injection = off
  -> ContextEngine.enabled = false
  -> compose_effective_prompt() returns original prompt
```

## 链路 6：Memory 写入、搜索和 embedding

Engine 自动写入：

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

LLM 主动写入：

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

Memory search：

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

`IotaEngine` 打开的 memory store 使用 `context_engine.embedding`。`context-mcp` 和 `mcp::router` 当前通过 `MemoryStore::open()` 打开默认 store，因此 MCP 查询侧使用本地 trigram fallback。

Embedding schema：

```text
memory_embedding
  memory_id TEXT PRIMARY KEY
  vector_blob BLOB NOT NULL
  updated_at INTEGER NOT NULL
```

## 链路 7：Engine-run skill 与 MCP

触发链：

```text
SkillRegistry::load_cached()
  -> parse skill frontmatter
  -> compatible_skills(backend)
  -> match_skill(backend, prompt)
       -> prompt lowercased contains any trigger
```

执行链：

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

边界：

- 每个 `call_stdio()` 启动一个 MCP server 子进程。
- parallel skill 会并发启动多个 tool 调用。
- 命中 engine-run skill 时可以完全绕过 ACP 后端。

## 链路 8：MCP sidecar - iota-context

启动方式：

```text
iota context-mcp
  -> cli::run()
  -> context::server::run_stdio()
```

初始化：

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

JSON-RPC methods：

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

调用方：

- ACP backend 根据 `session/new.mcpServers` 启动。
- `skill::runner` 可作为 engine-run MCP skill server 启动。

## 链路 9：MCP sidecar - iota-fun

启动方式：

```text
iota fun-mcp
  -> cli::run()
  -> skill::fun_server::run_stdio()
```

JSON-RPC methods：

```text
initialize
tools/list
tools/call
  -> run_tool()
```

工具执行链：

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

边界：

- `fun-mcp` 本身是 stdio JSON-RPC MCP server。
- 语言运行器通过 `std::process::Command` 调用解释器、编译器或编译产物。
- 编译缓存位于 `~/.i6/fun-cache/<language>/<hash>`。

## 链路 10：Backend-started MCP server 渲染

配置链：

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

session/new 参数：

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

默认启用规则：

| Backend | 默认是否注入 `mcpServers` |
|---|---|
| Claude Code | 仅当 `context_engine_backend.claude-code.mcp_session_new` 为 `true/try/on` |
| Codex | 仅当 `context_engine_backend.codex.mcp_session_new` 为 `true/try/on`；即使空 server 也发送 `mcpServers` |
| Gemini | 默认启用 |
| Hermes | 默认启用 |
| OpenCode | 默认启用 |

`mcp_session_new: try` 对 Claude Code 和 Codex 视为启用，对其他 backend 视为禁用。

## 链路 11：Permission 和 MCP router

权限请求：

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

Response shape：

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

Router：

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

## 链路 12：Telemetry queries、check、benchmark、native、skill pull

Telemetry query commands：

```text
iota logs <execution_id>
  -> cli::run_logs_command()
  -> IOTA_LOKI_URL or http://localhost:3100
  -> Loki query_range API with {service_name="iota"}
  -> client-side filter by stream execution_id or line text
  -> print matching log lines

iota trace <trace_id>
  -> cli::run_trace_command()
  -> IOTA_JAEGER_URL or http://localhost:16686
  -> Jaeger /api/traces/<trace_id>
  -> print span names and durations
```

The old `iota observability` / `iota obs` command group is not present in the current CLI.

Check：

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

Benchmark：

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

Native materialize：

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

Skill pull：

```text
iota skill pull <source> [name]
  -> skill::cache::pull_skill()
       -> local path copy or HTTP(S) GET via reqwest
       -> sanitize destination name
       -> write into ~/.i6/skills
  -> print JSON { path }
```

## 存储子系统调用链

MemoryStore：

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

CacheStore：

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

SessionLedger：

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

ApprovalStore：

```text
ApprovalStore::open_default()
record_request()
record_decision()
classify_operation()
default_decision()
```

EmbeddingEngine：

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

## 模块覆盖表

| 模块 | 主要职责 | 覆盖链路 |
|---|---|---|
| `main.rs` | Tokio 入口 | 入口总览 |
| `cli/mod.rs` | 命令分发、daemon autostart、bench、logs/trace、native、skill | 1,3,4,12 |
| `config.rs` | `~/.i6/nimia.yaml`、EffectiveConfig、backend command/env、MCP/session options、embedding config | 1,2,3,10 |
| `engine.rs` | 核心编排、replay/join、memory、skill、context、ACP pool、store 写回 | 1,3,4,5,6,7 |
| `acp/mod.rs` | ACP backend、子进程、JSON-RPC、prompt event loop | 1,2 |
| `acp/session.rs` | session/new 和 mcpServers | 2,10 |
| `acp/wire.rs` | ACP line read/parse/id/error | 2 |
| `acp/permission.rs` | ACP permission、auto approve、TUI/stdin approval | 4,11 |
| `runtime_event.rs` | 事件归一化 | 1,2,11,12 |
| `daemon/mod.rs` | daemon TCP server、warm/prompt、graceful shutdown | 3 |
| `daemon/pool.rs` | 按 cwd 复用 IotaEngine | 3 |
| `daemon/proto.rs` | daemon wire types | 3 |
| `tui.rs` | TUI 主循环、engine task、stream/approval channel | 4 |
| `tui/composer.rs` | 输入编辑器 | 4 |
| `tui/markdown.rs` | Markdown 渲染 | 4 |
| `tui/status_bar.rs` | 状态栏 | 4 |
| `tui/theme.rs` | TUI 样式 | 4 |
| `tui/state.rs` | 对话和观测状态 | 4 |
| `context/mod.rs` | context capsule、DialogueBuffer、workspace summary | 5 |
| `context/server.rs` | iota-context MCP server | 6,8 |
| `skill/mod.rs` | skill 加载、trigger、backend compatibility | 5,7,8,12 |
| `skill/runner.rs` | engine-run MCP skill | 7 |
| `skill/cache.rs` | skill pull/cache | 12 |
| `skill/fun_server.rs` | iota-fun MCP server 和语言执行 | 7,9 |
| `mcp/client.rs` | stdio MCP client | 7 |
| `mcp/router.rs` | ACP tool-call 拦截 | 6,11 |
| `native/mod.rs` | 原生文件投影 | 12 |
| `store/memory.rs` | memory taxonomy、recall、search、merge、TTL | 5,6,8,11 |
| `store/embedding.rs` | API/local embedding、cosine、blob encode/decode | 6 |
| `store/cache.rs` | execution replay/dedupe cache | 1,3,4,12 |
| `telemetry/mod.rs` | OTel provider/exporter initialization | 入口总览,12 |
| `telemetry/metrics.rs` | OTel instruments | 1,3,4,12 |
| `store/ledger.rs` | session/backend session/turn/handoff | 1,5,8,11 |
| `store/approval.rs` | approval 事件和风险分类 | 11 |
| `utils.rs` | 时间、摘要、lock recovery | 多条链路 |

## 进程间和外部调用清单

| 位置 | 类型 | 发起方 | 目标 | 协议/用途 |
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
