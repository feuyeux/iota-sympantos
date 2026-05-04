# iota-sympantos 代码调用链总览

本文按实际入口和运行时边界总结工程调用链。重点标注进程间调用（IPC），并覆盖 `src/` 下所有模块。

## 总入口

```text
src/main.rs
  -> cli::run()
```

`main.rs` 只注册模块并启动 Tokio runtime，所有用户可见入口都由 `cli::run()` 分发。

## 链路 1：CLI 直接运行 ACP 后端

```text
iota run [backend] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
  -> config::read_config()
  -> IotaEngine::new()
       -> ContextEngine::from_config()
       -> MemoryStore::open(context_memory_db_path)
       -> EventStore::open(default_path)
       -> SessionLedger::open(default_path)
  -> IotaEngine::prompt_in_cwd_timed()
       -> request_hash()
       -> EventStore::find_completed_by_request_hash() / output_text()  (cache replay)
       -> EventStore::find_running_by_request_hash() / get_execution()   (join running)
       -> SessionLedger::ensure_session() / record_backend_session()
       -> IotaEngine::prepare_handoff()
       -> EventStore::begin_execution_with_id()
       -> SkillRegistry::load()
       -> SkillRegistry::match_skill()
       -> MemoryStore::recall_buckets()
       -> ContextEngine::compose_effective_prompt()
            -> render_workspace()
               -> [IPC: child process] git status --short
       -> IotaEngine::ensure_client()
            -> config::backend_config()
            -> config::backend_process_env()
            -> config::normalized_acp_command()
            -> config::context_mcp_servers()
            -> AcpClient::start()
                 -> [IPC: child process + stdio JSON-RPC] npx/hermes/opencode ACP backend
                 -> acp::send_request("initialize")
                 -> acp::wait_for_response()
       -> AcpClient::prompt_with_cwd_timed_for_execution()
            -> AcpClient::ensure_session_timed()
                 -> acp_session::session_new_params()
                 -> acp::send_request("session/new")
                 -> acp::wait_for_response()
            -> acp::send_request("session/prompt")
            -> acp::read_prompt_events_for_id()
                 -> acp_wire::read_next_line()
                 -> acp_wire::parse_message_line()
                 -> runtime_event::map_acp_events()
                 -> acp_permission::answer_permission_request()     (if permission request)
                 -> mcp_router::try_intercept_tool_call()           (if tool call event)
       -> EventStore::append_event() / record_timing() / finish_execution()
       -> SessionLedger::record_turn()
       -> MemoryStore::insert() episodic memory
       -> MemoryStore::insert() explicit keyword memory
  -> cli prints output
  -> IotaEngine::shutdown()
       -> AcpClient::shutdown()
       -> [IPC cleanup] terminate ACP child process/tree
```

进程间调用标注：

- `AcpClient::start()` 使用 `tokio::process::Command` 启动 Claude Code、Codex、Gemini、Hermes、OpenCode 的 ACP 进程。通信是 stdin/stdout 上的换行分隔 JSON-RPC 2.0。
- ACP 初始化和对话协议为 `initialize -> session/new -> session/prompt -> session/update... -> session/complete`。
- `ContextEngine::render_workspace()` 同步执行 `git status --short`，由 engine 通过 `spawn_blocking` 包裹。
- `acp_permission::answer_permission_request()` 会把 `session/request_permission` 回写到 ACP 子进程 stdin。
- `mcp_router` 处理 ACP 进程发回来的 MCP tool-call 形态请求；可路由 iota 内部工具，默认拒绝外部 MCP 工具。

## 链路 2：CLI 经 daemon 运行

```text
iota run --daemon [backend] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
  -> cli::run_prompt_via_daemon()
  -> cli::send_prompt_autostart_daemon()
       -> agent::send_prompt()
            -> [IPC: TCP] connect 127.0.0.1:47661
            -> send one JSON line DaemonPromptRequest
       -> on connect error:
            -> cli::start_daemon_silently()
                 -> [IPC: child process] current_exe __daemon
            -> cli::wait_for_daemon()
            -> agent::send_prompt() retry
```

Daemon 进程端：

```text
iota __daemon
  -> cli::run()
  -> config::read_config()
  -> agent::run_daemon()
       -> TcpListener::bind(127.0.0.1:47661)
       -> accept loop
       -> tokio::spawn(handle_connection)
            -> read one JSON line
            -> handle_prompt() or handle_warm()
```

Prompt 请求：

```text
agent::handle_prompt()
  -> AcpBackend::parse()
  -> EnginePool::engine_for(backend, cwd)
       -> IotaEngine::new() if no cached engine
  -> IotaEngine::prompt_in_cwd_timed_with_execution_id()
       -> same engine/ACP chain as 链路 1
  -> write one JSON line DaemonPromptResponse
```

Warm 请求：

```text
iota check --daemon / bench-* --daemon / warm path
  -> cli::warm_daemon_for_current_dir()
  -> agent::send_warm()
       -> [IPC: TCP] DaemonWarmRequest
  -> agent::handle_warm()
       -> warm_all_backends() or warm_selected_backends()
       -> IotaEngine::warm_backend_in_cwd()
       -> AcpClient::start()
            -> [IPC: child process + stdio JSON-RPC] ACP backend initialize
```

进程间调用标注：

- CLI 客户端和 daemon 使用本机 TCP，协议是单请求单响应 JSON line。
- daemon 自身由 CLI 静默启动为 `current_exe __daemon`。
- daemon 内部复用 `IotaEngine` 和 ACP 子进程，因此后续请求可复用 ACP session。
- daemon shutdown 捕获 Ctrl+C，逐个关闭 engine 中的 ACP child。

## 链路 3：TUI 交互运行

```text
iota / iota tui
  -> cli::run()
  -> config::read_config()
  -> tui::run()
       -> stdout is_terminal 检查
       -> TuiApp::new()
            -> IotaEngine::new()
            -> EventStore::open()
       -> acp_permission::install_tui_approval_channel()
       -> set panic hook + enter alternate screen + raw mode
       -> tui::run_loop()
```

TUI 事件循环：

```text
tui::run_loop()
  -> EventStream keyboard/resize
  -> Composer::handle_key()
  -> TuiApp::submit()
       -> mpsc TurnMessage::Prompt
  -> pending prompt
       -> tokio::spawn(engine task)
            -> IotaEngine::set_stream_sender(Some(tx))
            -> IotaEngine::prompt_in_cwd_timed()
                 -> same engine/ACP chain as 链路 1
            -> IotaEngine::set_stream_sender(None)
            -> engine result channel
  -> stream_rx receives session/update chunks
  -> approval_rx receives ApprovalRequest
  -> render()
       -> render_header/history/composer/status/pager/help/approval
       -> tui::markdown::render()
       -> tui::status_bar::render()
       -> tui::theme::* styles
```

输入组件链：

```text
tui/composer.rs
  -> Unicode grapheme cursor helpers
  -> Composer::handle_key()
       -> submit/newline/history/search/word movement/kill-yank
  -> ComposerAction consumed by tui::run_loop()
```

TUI 状态链：

```text
tui/state.rs
  -> ConversationEntry / HistoryState / ObservabilityMeta
  -> render_entries() / observability_from_output()
```

进程间调用标注：

- TUI 不通过 daemon；它持有一个进程内 `IotaEngine`。
- 真正跨进程点仍在 engine 的 ACP 子进程、MCP sidecar、permission response。
- `session/update` 流式文本经 `AcpClient.stream_tx` 回到 TUI channel，不是进程间调用，是进程内 Tokio mpsc。

## 链路 4：ACP 子进程协议驱动

```text
AcpClient::start()
  -> resolve command from config or AcpBackend::command()
  -> TokioCommand::new(command).args(args).envs(env).current_dir(cwd)
  -> stdin/stdout/stderr piped, kill_on_drop(true)
  -> spawn stderr reader task
  -> send_request("initialize")
  -> wait_for_response("init-0")
```

```text
AcpClient::prompt_with_cwd_timed_for_execution()
  -> ensure_session_timed()
       -> session_new_params()
       -> send_request("session/new")
       -> wait_for_response()
  -> send_request("session/prompt")
  -> read_prompt_events_for_id()
       -> parse every stdout JSON line
       -> collect response text
       -> map runtime events
       -> handle session/request_permission
       -> intercept tools/call if needed
```

辅助模块：

- `acp_wire.rs`：读取带 timeout 的 stdout 行、解析 ACP JSON、匹配 response id、格式化 ACP error。
- `acp_session.rs`：构造 `session/new` params，包含 cwd 和 `mcpServers`；Hermes 的 env 渲染为字符串数组，其它后端为对象。
- `acp_permission.rs`：处理权限请求。TUI 运行时走 TUI approval channel；非 TUI 时走 stdin yes/no，并写 `ApprovalStore`。
- `runtime_event.rs`：把 ACP `session/update`、usage、tool、approval、error 等协议形态归一为 `RuntimeEvent`。
- `mcp_router.rs`：拦截 ACP 侧 `tools/call`/`mcp/tools/call`/`mcp/tool_call`，只允许部分 `iota_*` 工具，默认拒绝外部工具。

## 链路 5：Context Fabric 注入链

```text
IotaEngine::prompt_in_cwd_timed_with_execution_id()
  -> SkillRegistry::load(cwd, configured_roots)
       -> workspace/.iota/skills
       -> configured skill_roots
       -> ~/.i6/skills
       -> parse YAML frontmatter + body
  -> MemoryStore::recall_buckets(local-user, cwd, session_id)
  -> DialogueBuffer::render()
  -> prepare_handoff()
       -> SessionLedger::publish_handoff()
       -> MemoryStore::insert(handoff episodic memory)
  -> ContextEngine::compose_effective_prompt()
       -> session/model metadata
       -> memory buckets
       -> dialogue summary
       -> workspace git status
       -> skill index
       -> handoff summary
       -> user prompt
```

写回链：

```text
ACP/skill output completed
  -> EventStore::append_event(Output/Tool/Token/Error/Approval...)
  -> EventStore::record_timing() + finish_execution()
  -> SessionLedger::record_turn()
  -> DialogueBuffer::push_turn()
  -> MemoryStore::insert(episodic prompt/output memory)
  -> extract_explicit_memory()
       -> MemoryStore::insert(semantic memory if prompt contains remember/save/记住/保存)
```

## 链路 6：engine-run skill 与 MCP

当 prompt 命中一个 `execution.mode = "mcp"` 的 skill：

```text
IotaEngine::prompt_in_cwd_timed_with_execution_id()
  -> SkillRegistry::match_skill()
  -> skill_runner::run_engine_skill()
       -> server_command("iota-fun" | "iota-context" | custom)
       -> build McpToolCall list
       -> sequential:
            -> mcp_client::call_stdio()
       -> parallel/batch:
            -> mcp_client::call_stdio_batch()
       -> render_template()
       -> return SkillRunOutput
  -> EventStore::append_event(ToolCall/ToolResult/Output)
  -> finish execution without ACP backend prompt
```

进程间调用标注：

- `mcp_client::call_stdio()` 和 `call_stdio_batch()` 每次启动一个 MCP server 子进程，通过 stdio JSON-RPC 调用 `initialize`、`notifications/initialized`、`tools/call`。
- 默认 server 是当前 `iota` 可执行文件加 `fun-mcp` 或 `context-mcp` 子命令。
- 命中 engine-run skill 时可以完全绕过 ACP 后端 prompt，只由 MCP 工具结果生成输出。

## 链路 7：MCP sidecar - iota-context

```text
iota context-mcp
  -> cli::run()
  -> context_mcp::run_stdio()
       -> MemoryStore::open(default_path)
       -> SkillRegistry::load(current_dir, [])
       -> SessionLedger::open(default_path)
       -> stdin loop JSON-RPC
       -> handle_request()
```

支持的 MCP 方法：

```text
initialize
  -> return capabilities tools/resources

tools/list
  -> tools()

tools/call
  -> call_tool()
       -> iota_memory_search -> MemoryStore::search()
       -> iota_memory_write  -> MemoryStore::insert()
       -> iota_skill_search  -> SkillRegistry::skill_index()
       -> iota_skill_load    -> SkillRegistry::get()
       -> iota_session_summary -> SessionLedger::summary()
       -> iota_handoff_publish -> SessionLedger::publish_handoff()
       -> iota_handoff_read    -> SessionLedger::read_handoff()

resources/list
  -> static iota:// resources

resources/read
  -> read_resource()
       -> memory/search, skill/get, session/summary, workspace/rules
```

进程间调用标注：

- `context-mcp` 作为 MCP sidecar 时通常由 ACP 后端根据 `session/new.mcpServers` 启动，也可能由 `skill_runner` 启动。
- 通信协议为 stdio JSON-RPC 2.0。

## 链路 8：MCP sidecar - iota-fun

```text
iota fun-mcp
  -> cli::run()
  -> fun_mcp::run_stdio()
       -> stdin loop JSON-RPC
       -> handle_request()
            -> initialize / tools/list / tools/call
            -> run_tool()
```

工具执行链：

```text
fun.python      -> run_interpreter("python3", ["-c", source])
fun.typescript  -> run_interpreter("node", ["-e", source])
fun.rust        -> write_source(main.rs) -> rustc -> compiled binary
fun.go          -> write_source(main.go) -> go run
fun.java        -> write_source(Main.java) -> javac -> java -cp
fun.cpp         -> write_source(main.cpp) -> clang++/g++ -> compiled binary
fun.zig         -> write_source(main.zig) -> zig run
```

进程间调用标注：

- `fun-mcp` 本身是 stdio JSON-RPC MCP server。
- `run_command()` 使用 `std::process::Command` 启动解释器、编译器、运行产物等外部进程。
- 编译缓存位于 `~/.i6/fun-cache/<language>/<hash>`。

## 链路 9：观测、检查、benchmark、物化、skill pull

观测命令：

```text
iota observability summary/recent/metrics
  -> cli::run_observability_command()
  -> EventStore::open(default_path)
  -> observability_summary() / recent_executions() / prometheus_metrics()
  -> print JSON or Prometheus text
```

检查命令：

```text
iota check [--daemon]
  -> optional warm_daemon_for_current_dir()
  -> config::read_config()
  -> print_combined_info()
       -> backend_info() for ALL_BACKENDS
       -> config::backend_config() / command_label() / configured_model()
```

Benchmark：

```text
iota bench-cold
  -> for each enabled backend:
       -> new IotaEngine
       -> prompt_in_cwd("ping")
       -> shutdown

iota bench-warm
  -> one IotaEngine
  -> warm_enabled_backends_in_cwd()
  -> repeated prompt_in_cwd("ping")

iota bench-* --daemon
  -> run_daemon_benchmark()
       -> send_prompt_autostart_daemon()
```

Native materialize：

```text
iota native-materialize ...
  -> cli::run_native_materialize()
       -> native_materializer::backend_memory_path()
       -> native_materializer::dry_run() / dry_run_backend_projection()
       -> native_materializer::apply()
```

Skill pull：

```text
iota skill pull <source> [name]
  -> cli::run_skill_command()
  -> skill_registry_cache::pull_skill()
       -> local path copy or HTTP(S) GET via reqwest
       -> sanitize file name
       -> write into ~/.i6/skills
```

进程间调用标注：

- `bench-* --daemon` 走 TCP daemon。
- `bench-cold`/`bench-warm` 走 ACP 子进程。
- `skill pull` 的 HTTP(S) URL 是网络进程外调用，不是本机 IPC；本地路径是文件系统复制。

## 存储子系统调用链

```text
MemoryStore
  -> open(default or configured SQLite path)
  -> init tables + FTS5 triggers
  -> insert(): taxonomy validation, dedup, supersedes, TTL
  -> recall_buckets(): identity/preference/strategic/domain/procedural/episodic
  -> search(): FTS phrase search, fallback LIKE
```

```text
EventStore
  -> open(default SQLite path)
  -> executions/events/observability tables
  -> begin_execution_with_id(): idempotency + running lock + fencing token
  -> append_event(): sequence per execution
  -> finish_execution()/record_timing()
  -> find_completed/find_running/output_text(): cache replay and join running
  -> observability_summary()/prometheus_metrics()
```

```text
SessionLedger
  -> open(default SQLite path)
  -> sessions/backend_sessions/turns/handoffs tables
  -> latest_session_for_cwd()
  -> ensure_session()
  -> record_backend_session()
  -> record_turn()
  -> publish_handoff()/read_handoff()
  -> summary()
```

```text
ApprovalStore
  -> acp_permission::answer_permission_request()
  -> approval::classify_operation()
  -> approval::default_decision()
  -> record_request()/record_decision()
```

## 模块覆盖表

| 模块 | 主要职责 | 被覆盖链路 |
|---|---|---|
| `main.rs` | Tokio 入口，转发 CLI | 总入口 |
| `cli.rs` | 命令分发、daemon autostart、bench、obs、native、skill | 1,2,9 |
| `config.rs` | `~/.i6/nimia.yaml` 解析、后端 env/command、MCP server 注入 | 1,2,4,5 |
| `engine.rs` | 核心编排、缓存、上下文、skill、ACP client 池、存储写回 | 1,2,3,5,6 |
| `acp.rs` | ACP client、子进程启动、JSON-RPC 请求/响应、prompt 事件读取 | 1,4 |
| `acp_wire.rs` | ACP line read/parse/response id/error | 4 |
| `acp_session.rs` | session/new 参数和 mcpServers 渲染 | 4,7,8 |
| `acp_permission.rs` | ACP 权限请求处理，TUI channel 或 stdin 决策 | 3,4 |
| `runtime_event.rs` | ACP 事件归一化 | 1,4,9 |
| `agent.rs` | daemon TCP server/client、engine pool、warm/prompt 请求 | 2 |
| `tui.rs` | ratatui 主循环、渲染、engine task、stream/approval channel | 3 |
| `tui/composer.rs` | 多行输入、历史、搜索、kill buffer、word motion | 3 |
| `tui/markdown.rs` | Markdown 到 ratatui line 渲染 | 3 |
| `tui/status_bar.rs` | 状态栏、模型和观测展示 | 3 |
| `tui/theme.rs` | TUI 样式 | 3 |
| `tui/state.rs` | 对话和历史状态 | 3 |
| `context.rs` | context capsule、dialogue buffer、workspace git status | 5 |
| `memory.rs` | SQLite memory store、FTS、recall/search | 5,7 |
| `event_store.rs` | SQLite event/execution/observability store | 1,2,3,9 |
| `session_ledger.rs` | SQLite session/turn/handoff ledger | 5,7 |
| `skills.rs` | 分布式 skill 加载、索引、trigger 匹配 | 5,6,7,9 |
| `skill_runner.rs` | engine-run MCP skill 执行 | 6 |
| `mcp_client.rs` | stdio MCP client，initialize/tools/call | 6 |
| `context_mcp.rs` | iota-context MCP server | 7 |
| `fun_mcp.rs` | iota-fun MCP server，7 语言执行 | 8 |
| `mcp_router.rs` | ACP tool-call 拦截和内部工具路由 | 4 |
| `approval.rs` | approval 分类、默认策略、持久化 | 4 |
| `native_materializer.rs` | memory/skill 原生文件投影 | 9 |
| `skill_registry_cache.rs` | skill pull/cache | 9 |
| `utils.rs` | 时间、文本摘要、poison lock 恢复 | 多条链路 |

## 进程间调用清单

| 位置 | 类型 | 发起方 | 目标 | 协议/用途 |
|---|---|---|---|---|
| `cli::start_daemon_silently()` | child process | CLI | `iota __daemon` | daemon autostart |
| `agent::send_request()` / `agent::run_daemon()` | TCP | CLI | daemon `127.0.0.1:47661` | JSON line request/response |
| `AcpClient::start()` | child process + stdio | engine | ACP backend | JSON-RPC 2.0 line protocol |
| `AcpClient::send_request()` | stdio | engine | ACP backend | `initialize/session/new/session/prompt` |
| `acp_permission::send_response()` | stdio | engine | ACP backend | permission decision response |
| `acp_session::session_new_params()` | child process delegated by backend | ACP backend | MCP servers | mcpServers tells backend how to spawn `iota context-mcp`/`fun-mcp` |
| `mcp_client::call_stdio(_batch)` | child process + stdio | skill runner | MCP server | MCP JSON-RPC initialize/tools/call |
| `context_mcp::run_stdio()` | stdio server | ACP backend or skill runner | iota-context | MCP tools/resources |
| `fun_mcp::run_stdio()` | stdio server | ACP backend or skill runner | iota-fun | MCP tools |
| `fun_mcp::run_command()` | child process | iota-fun | python/node/rustc/go/javac/java/clang++/g++/zig/binary | execute user-supplied small code |
| `context::render_workspace()` | child process | context engine | `git` | `git status --short` |
| `skill_registry_cache::pull_skill()` | network or filesystem | CLI | HTTP(S) URL or local path | fetch/copy skill |
