# iota-sympantos 架构总览

iota-sympantos 是一个轻量级 Rust CLI/TUI 编排器。它把用户入口、Context Fabric、持久化存储、ACP 后端和 MCP sidecar 串成统一运行时，使 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 可以在同一套配置、记忆和技能体系下工作。

本文按当前源码组织架构。具体调用链见 [code-call-chains.md](code-call-chains.md)，观测命令见 [observability.md](observability.md)，调试手册见 [debugging.md](debugging.md)。

## 当前源码结构

```text
crates/
├── iota-cli/
│   └── src/
│       ├── main.rs          # binary 入口，进入 cli::run()
│       ├── cli/
│       │   ├── mod.rs       # 命令分发、daemon autostart、bench、observability、logs/trace、skill
│       │   └── observability_cmd.rs # token usage 查询、汇总、导出和 Prometheus 文本输出
│       ├── tui/
│       │   ├── input.rs     # 多行输入、Unicode 光标、历史搜索、kill/yank、word motion
│       │   ├── markdown.rs  # Markdown 到 ratatui Line 渲染
│       │   ├── scrollback.rs # 终端内联滚动区（无 alt-screen，原生滚动支持）
│       │   ├── status_bar.rs # 底部状态栏（backend · model / 快捷键 / 观测状态）
│       │   ├── render.rs    # 主渲染器（history/composer/overlay/state）
│       │   ├── state.rs     # 对话、历史和观测展示状态
│       │   ├── loop.rs      # Tokio event loop（turn dispatch、stream、approval）
│       │   ├── events.rs    # TUI 事件定义（ApprovalRequest、observability）
│       │   ├── terminal_lifecycle.rs # raw mode、panic hook、alternate screen guard
│       │   └── theme.rs     # TUI 主题
│       └── tui.rs           # TUI 模块入口，导出 `run()` bootstrap
└── iota-core/
    └── src/
        ├── engine/          # IotaEngine 编排、ACP client pool、上下文、skill、store 写回
        ├── acp/             # ACP backend enum、子进程生命周期、JSON-RPC 请求/响应、prompt loop
        ├── daemon/          # 本机 TCP daemon，EnginePool，daemon wire types
        ├── config/          # ~/.i6/nimia.yaml、有效配置、后端 env/command、context options
        ├── context/         # ContextEngine、context capsule、WorkingMemoryBuffer、workspace summary
        ├── skill/           # SkillRegistry、engine-run skill、skill cache、iota-fun MCP server
        ├── mcp/             # MCP client/server、ACP tool-call router、tool dispatch
        ├── store/           # cache、approvals、observability、ledger
        ├── memory/          # memory taxonomy、FTS、vector/hybrid search、recall buckets
        ├── runtime_event/   # 统一 RuntimeEvent
        ├── telemetry/       # tracing / OpenTelemetry setup
        └── utils/           # 时间戳、摘要、poison lock recovery
```

## 分层架构

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ Entry                                                                      │
│   crates/iota-cli/src/main.rs                                              │
└────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌────────────────────────────────────────────────────────────────────────────┐
│ Presentation                                                               │
│   crates/iota-cli/src/cli/            crates/iota-cli/src/tui.rs + tui/*    │
│   CLI command routing                 interactive terminal UI              │
└────────────────────────────────────────────────────────────────────────────┘
                    │                                     │
                    ▼                                     ▼
┌────────────────────────────────────────────────────────────────────────────┐
│ Service Orchestration                                                       │
│   crates/iota-core/src/engine/       crates/iota-core/src/daemon/           │
│   IotaEngine, turn lifecycle         warm local service over TCP            │
└────────────────────────────────────────────────────────────────────────────┘
      │                    │                      │                     │
      ▼                    ▼                      ▼                     ▼
┌───────────────┐   ┌────────────────┐   ┌────────────────┐   ┌────────────────┐
│ Context       │   │ Protocol       │   │ Store          │   │ Runtime Events │
│ core context  │   │ core acp/mcp   │   │ core store     │   │ runtime_event  │
│ core skill    │   │ JSON-RPC       │   │ SQLite         │   │ normalized     │
│                │   │ stdio/TCP      │   │ + embedding    │   │ event stream   │
└───────────────┘   └────────────────┘   └────────────────┘   └────────────────┘
          │                  │                    ▲
          │                  ▼                    │
          │        ┌──────────────────────┐       │
          └───────▶│ External Boundaries  │◀──────┘
                   │ ACP backend process  │
                   │ MCP sidecar process  │
                   │ git / compilers / IO │
                   └──────────────────────┘

┌────────────────────────────────────────────────────────────────────────────┐
│ Shared Configuration and Utilities                                          │
│   crates/iota-core/src/config/: ~/.i6/nimia.yaml, env, MCP/session options  │
│   crates/iota-core/src/utils/: timestamps, summarization, lock recovery     │
└────────────────────────────────────────────────────────────────────────────┘
```

## 模块职责

### Entry

| 模块 | 职责 | 下游 |
| :---| :---| :---|
| `crates/iota-cli/src/main.rs` | 启动 Tokio runtime，调用 `cli::run()` | `cli` |

`crates/iota-cli/src/main.rs` 不持有业务策略。

### Presentation

| 模块 | 职责 | 主要下游 |
| :---| :---| :---|
| `crates/iota-cli/src/cli/mod.rs` | 解析命令并分发 `run/check/tui/bench/observability/logs/trace/context-mcp/fun-mcp/skill/__daemon`；负责 daemon autostart 和 CLI 输出 | `config`, `engine`, `daemon`, `acp`, `store`, `skill`, `tui` |
| `crates/iota-cli/src/cli/observability_cmd.rs` | `iota observability logging/tokens/metrics`；查询本地 token usage、backend summary、JSON export 和 Prometheus 文本指标 | `store::observability` |
| `crates/iota-cli/src/tui.rs` | 终端生命周期、事件循环、prompt 队列、后台 engine task、流式输出、approval 浮层、pager/help/quit overlay | `engine`, `acp::permission`, `tui/*` |
| `crates/iota-cli/src/tui/input.rs` | 多行编辑、历史、搜索、词移动和 kill buffer | 无项目级依赖 |
| `crates/iota-cli/src/tui/markdown.rs` | Markdown 渲染为 ratatui 文本行 | 无项目级依赖 |
| `crates/iota-cli/src/tui/scrollback.rs` | 终端内联滚动区管理（无 alt-screen，原生终端滚动/copy/selection） | 无项目级依赖 |
| `crates/iota-cli/src/tui/status_bar.rs` | backend/model/快捷键/观测状态栏 | `acp`, `tui::state` |
| `crates/iota-cli/src/tui/render.rs` | 主渲染器（history/composer/overlay/state） | `tui::state`, `tui::markdown` |
| `crates/iota-cli/src/tui/state.rs` | 对话历史和观测展示模型 | 无项目级依赖 |
| `crates/iota-cli/src/tui/loop.rs` | Tokio event loop（turn dispatch、stream、approval） | `tui`, `engine` |
| `crates/iota-cli/src/tui/events.rs` | TUI 事件定义（ApprovalRequest、observability） | `acp::permission` |
| `crates/iota-cli/src/tui/terminal_lifecycle.rs` | raw mode、panic hook、alternate screen guard | 无项目级依赖 |
| `crates/iota-cli/src/tui/theme.rs` | ratatui 颜色和样式 | 无项目级依赖 |

Presentation 层不直接拥有 ACP session；后端执行统一经过 `IotaEngine` 或 daemon client API。

### Service Orchestration

| 模块 | 职责 | 主要下游 |
| :---| :---| :---|
| `crates/iota-core/src/engine/` | 核心编排门面。按 `(backend, cwd)` 维护 ACP client pool；处理 session ledger、handoff、memory recall/write、skill 短路、context capsule、ACP 调用和事件落库 | `acp`, `config`, `context`, `skill`, `store`, `runtime_event` |
| `crates/iota-core/src/daemon/mod.rs` | `127.0.0.1:47661` 默认 TCP daemon；支持 `IOTA_DAEMON_ADDR`；每连接一条 JSON request/response；8 并发限流；10 MiB 请求上限；Ctrl+C 优雅关闭 | `engine`, `daemon::pool`, `daemon::proto` |
| `crates/iota-core/src/daemon/pool.rs` | `EnginePool` 按 cwd 复用 `IotaEngine`，从而复用 ACP 子进程和 session/handoff 状态 | `engine`, `config` |
| `crates/iota-core/src/daemon/proto.rs` | `DaemonPromptRequest`、`DaemonPromptResponse`、`DaemonWarmRequest` | `runtime_event`, `acp::AcpPromptTiming` |

`crates/iota-core/src/engine/` 是行为决策边界；SQL、ACP wire、MCP JSON-RPC 和 TUI 渲染仍保留在各自模块。

### Protocol

| 模块 | 职责 | 主要下游 |
| :---| :---| :---|
| `crates/iota-core/src/acp/mod.rs` | `AcpBackend`、默认 adapter 命令、`parse_acp_args()`、ACP 子进程启动、`initialize/session/new/session/prompt`、流式事件读取和 timing | `acp::permission`, `acp::session`, `acp::wire`, `mcp::router`, `runtime_event` |
| `crates/iota-core/src/acp/session.rs` | 生成 `session/new` params；渲染 `mcpServers`；支持 `always_send_empty_mcp_servers` 和 env `string_array/object` 两种形态 | `acp::AcpBackend` |
| `crates/iota-core/src/acp/wire.rs` | ACP stdout line timeout、JSON parse、response id 判断、error 格式化 | 无项目级依赖 |
| `crates/iota-core/src/acp/permission.rs` | 处理 `session/request_permission`；`iota_*`、`mcp__iota-*` 或 backend `tool_whitelist` 命中时自动批准；否则走 TUI 或 stdin；记录 approval 事件 | `store::approval`, `runtime_event` |
| `crates/iota-core/src/mcp/client.rs` | engine-run skill 使用的 stdio MCP client；启动 server、initialize、tools/call | 无项目级依赖 |
| `crates/iota-core/src/mcp/server.rs` | `iota-context` MCP stdio server；JSON-RPC 协议适配，工具执行委托 `tool_dispatch` | `mcp::tool_dispatch`, `runtime_event`, `memory`, `store::ledger`, `skill` |
| `crates/iota-core/src/mcp/router.rs` | 拦截 ACP 侧 `tools/call` / `mcp/tools/call` / `mcp/tool_call`；委托 `tool_dispatch` 执行 iota 工具，拒绝外部工具 | `mcp::tool_dispatch`, `memory`, `store::ledger`, `skill`, `skill::fun_server` |
| `crates/iota-core/src/mcp/tool_dispatch.rs` | 共享工具派发逻辑：`ToolContext` 依赖注入、`dispatch_tool()` 统一入口、所有解析器和验证器 | `memory`, `store::ledger`, `skill` |
| `crates/iota-core/src/runtime_event/mod.rs` | 把 ACP update、complete、permission、usage、tool、error 统一为 `RuntimeEvent` | `acp::extract_text` |

协议层只做协议翻译和安全路由，不依赖 CLI/TUI/daemon/engine。

### Context Fabric

| 模块 | 职责 | 主要下游 |
| :---| :---| :---|
| `crates/iota-core/src/context/mod.rs` | 组装 `<iota-context>` capsule：session/model、memory tools 提示、memory buckets、working memory、workspace `git status --short`、skill index、handoff | `config`, `memory`, `skill` |
| `crates/iota-core/src/skill/mod.rs` | 加载 workspace `skills/`、workspace `.iota/skills`、配置 roots、`~/.i6/skills`；解析 YAML frontmatter；按 backend 和 trigger 匹配 | `acp::AcpBackend` |
| `crates/iota-core/src/skill/runner.rs` | 执行 `execution.mode = mcp` skill；可顺序或并行调用 MCP tools；渲染 template | `mcp::client`, `runtime_event`, `skill` |
| `crates/iota-core/src/skill/cache.rs` | 从本地路径或 HTTP(S) 拉取 skill，并写入 `~/.i6/skills` | filesystem/network |
| `crates/iota-core/src/skill/fun_server.rs` | `iota-fun` MCP stdio server；运行 `fun.python/typescript/rust/go/java/cpp/zig` | 外部解释器/编译器 |
Context Fabric 提供 prompt 背景和可确定工具。

### Store

| 模块 | 职责 | 默认路径 |
| :---| :---| :---|
| `crates/iota-core/src/store/cache.rs` | execution lifecycle、status、fencing token；`CacheStore` 在 `open()` 时读取一次 `StoreConfig` 并缓存到 struct 字段（消除热路径磁盘 IO）；批量状态查询 `get_execution_statuses()` 使用单条 `WHERE IN (...)` | `~/.i6/context/events.sqlite` |
| `crates/iota-core/src/store/observability.rs` | token usage events、raw payload、execution-level 最优记录去重（按 `token_event_score` 选 best）、backend 聚合 summary；新增 `token_percentiles(backend)` → P50/P95/P99、`token_usage_between(from, to)` 时间窗口查询；`record_token_usage()` 内联校验 `computed ≤ provider_total` | `~/.i6/context/events.sqlite` |
| `crates/iota-core/src/memory/store.rs` | memory taxonomy、dedup、TTL、merge mode、recall buckets、FTS/LIKE、vector/hybrid search | `~/.i6/context/memory.sqlite` 或 `context_engine.memory_db` |
| `crates/iota-core/src/memory/embedding.rs` | embedding 计算；engine 按 `context_engine.embedding` 配置 Ollama `/api/embeddings`，失败或未配置时走 128 维本地 trigram fallback | 存入 `memory_embedding` 表 |
| `crates/iota-core/src/store/approvals.rs` | approval request/decision 记录、风险维度分类、默认人工审批策略；新增 `get_pending_requests()` 和 `get_decision_history(execution_id, limit)` | `~/.i6/context/approvals.sqlite` |
| `crates/iota-core/src/store/ledger.rs` | iota session、backend session、turn、handoff；新增 `session_stats(session_id)` 和 `get_handoff_history(session_id)` 用于会话分析 | `~/.i6/context/sessions.sqlite` |

Store 模块只暴露 typed operations，不调用 UI、daemon、ACP client 或 MCP client。

### Configuration

| 模块 | 职责 |
| :---| :---|
| `crates/iota-core/src/config/` | 唯一读取 `~/.i6/nimia.yaml`；构建 `EffectiveConfig`；展开 `~/`；规范化 Windows `npx`；渲染 backend command/env；注入 context MCP server；读取 recall thresholds、embedding、skill roots、backend whitelist 和 session options；`StoreConfig` 提供四个可配置数据保留参数（`cache_retention_days`、`cache_running_ttl_secs`、`observability_retention_days`、`approvals_max_pending_age_secs`） |
| `crates/iota-core/src/utils/` | `now_ts()`、`summarize()`、`lock_or_recover()` |

配置不会自动读取项目级配置。`nimia.yaml.template` 是配置模板。

## 关键运行路径

### CLI 直接执行

```text
iota run [backend] [options] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
  -> config::read_config()
  -> IotaEngine::create_session()
  -> IotaEngine::run_with_timing()
       -> execution lifecycle start
       -> session ledger + handoff
       -> memory extraction / deterministic memory answer
       -> skill match and optional engine-run MCP skill
       -> memory recall + context capsule
       -> ensure ACP client
       -> ACP session/prompt
       -> event/timing/session/memory writeback
  -> stdout
```

### CLI 经 daemon 执行

```text
iota run --daemon ...
  -> CLI connects 127.0.0.1:47661
  -> if failed: spawn current_exe __daemon, wait, retry
  -> daemon EnginePool::engine_for(cwd)
  -> same IotaEngine prompt path
  -> JSON-line DaemonPromptResponse
```

### TUI 执行

```text
iota / iota tui
  -> tui::run()
  -> TuiApp::new(IotaEngine)
  -> install_tui_approval_channel()
  -> set panic hook
  -> enable raw mode
  -> inline viewport (5 rows: spinner + composer + status bar)
  -> TerminalGuard owns cleanup
  -> loop::run_loop()
       -> crossterm EventStream
       -> frame tick limiter ~30 FPS（throttled）
       -> keyboard/mouse/resize events
       -> Composer::handle_key()
       -> TuiApp::submit()
       -> tokio::spawn(engine task)
            -> IotaEngine::set_stream_output_sender(Some(tx))
            -> IotaEngine::run_with_timing()
            -> stream chunks to stream_rx
       -> approval_rx receives ApprovalRequest
       -> render()
            -> header/history/composer/status
            -> markdown::render()
            -> status_bar::render()
            -> overlays: help / quit confirm / approval
```

### Context 和 memory 写回

```text
ContextEngine::compose_effective_prompt()
  -> injects <memory-tools>
  -> backend LLM may call iota_memory_write
  -> ACP permission auto-approves iota tool or whitelist hit
  -> context-mcp / mcp_router handles tools/call
  -> MemoryStore::insert_with_merge()
  -> memory + memory_embedding persisted
```

### Engine-run MCP skill

```text
SkillRegistry::match_skill()
  -> skill.metadata.execution.mode == "mcp"
  -> skill::runner::run_engine_skill()
  -> mcp::client::call_stdio(_batch)
  -> iota fun-mcp / iota context-mcp / custom server
  -> ToolCall/ToolResult/Output events
  -> ACP backend prompt is skipped
```

## ACP 后端

| Backend | 默认命令 | 别名 | 备注 |
| :---| :---| :---| :---|
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@latest` | `claude`, `claude-code`, `claudecode` | 配置模板 pin 到 `0.32.0` |
| Codex | `npx -y @zed-industries/codex-acp@0.12.0` | `codex` | `normalized_acp_command()` 会追加 Codex `-c` 参数 |
| Gemini CLI | `npx -y @google/gemini-cli@latest --acp` | `gemini`, `gemini-cli` | 配置模板 pin 到 `0.41.2` |
| Hermes | `hermes acp` | `hermes`, `hermes-agent` | 不覆盖 `HERMES_HOME`，provider env 由 `render_hermes_provider_env()` 生成 |
| OpenCode | `npx -y opencode-ai@latest acp` | `opencode`, `open-code` | 配置模板 pin 到 `1.14.40` |

Windows 上 `normalize_command()` 会把 `npx` 改为 `npx.cmd`。

## 配置模型

配置只从 `~/.i6/nimia.yaml` 读取。顶层包含五个 backend section、`context_engine` 和 `context_engine_backend`。

### Backend section

| 字段 | 含义 |
| :---| :---|
| `enabled` | 是否参与 `check`、warm、bench |
| `acp.command` / `acp.args` | ACP adapter 启动命令 |
| `version_mapping` | 记录 adapter/bin 版本，供 `check` 输出 |
| `home` | backend 自定义 home；Codex/Hermes 当前不映射 home env |
| `model` | provider/name/base_url/api_key |
| `tool_whitelist` | 权限自动批准规则，支持简单 wildcard |

### Model env 映射

| Backend | 映射 |
| :---| :---|
| Claude Code | `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, `ANTHROPIC_MODEL`, `ANTHROPIC_SMALL_FAST_MODEL`, `ANTHROPIC_DEFAULT_*_MODEL`, `API_TIMEOUT_MS`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` |
| Codex | `ROUTER_API_KEY`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`；同时追加 `-c model=...`、`model_provider`、provider base_url/env_key/wire_api |
| Gemini | `GEMINI_API_KEY`, `GEMINI_MODEL` |
| Hermes | `HERMES_INFERENCE_PROVIDER`, `HERMES_MODEL`，以及 provider 原生 key/base_url，例如 `MINIMAX_CN_API_KEY` |
| OpenCode | `OPENCODE_MODEL` |

### Context engine section

| 字段 | 含义 |
| :---| :---|
| `enabled` / `injection` | 控制 context；当前实现中只有 `injection=off` 会禁用 prompt capsule，其它值都会启用注入 |
| `memory_db` | memory SQLite 路径 |
| `skill_roots` | 额外 skill root；实际加载还包括 workspace `skills/`、workspace `.iota/skills`、`~/.i6/skills` |
| `budgets` | memory/skills/working memory/workspace 字符预算 |
| `recall_thresholds` | 六类 recall bucket 置信度阈值 |
| `episodic_compaction_keep` | episodic 压缩保留数量 |
| `mcp` / `fun` | context/fun MCP server 启动命令 |
| `embedding` | Ollama `/api/embeddings` 配置；未配置或失败时使用本地 trigram fallback |

### Per-backend context options

| 字段 | 含义 |
| :---| :---|
| `mcp_session_new` | 控制是否在 `session/new` 注入 `mcpServers`；`try` 只对 Claude Code/Codex 默认启用 |
| `always_send_empty_mcp_servers` | 没有 MCP server 时也发送空数组 |
| `mcp_env_shape` | `string_array` 或 `object` |
| `override_home` | 是否把 backend `home` 映射到对应 env；Hermes 模板默认 `false` |

当前 `acp/session.rs` 对所有 backend 都渲染 `{name,type,command,args,env}`；env 形态可由配置切换为字符串数组或对象。Codex 即使 server 为空也会发送 `mcpServers` 字段。

## 数据模型

### RuntimeEvent

```text
Output
State
Log
ToolCall
ToolResult
Error
Extension
TokenUsage
Memory
ApprovalRequest
ApprovalDecision
```

`Log` 事件携带结构化字段（ts、level、target、event、tool_name、latency_ms 等），用于 CLI `--log-events` 输出。`RuntimeEvent` 随 `AcpPromptOutput.events` 返回；engine 会把 execution lifecycle/timing 写入 `CacheStore`，并把 `TokenUsage` enrich 后写入 `ObservabilityStore`。

### Memory taxonomy

| Type | Facet | 典型 scope | Recall bucket |
| :---| :---| :---| :---|
| `semantic` | `identity` | `user` | identity |
| `semantic` | `preference` | `user` | preference |
| `semantic` | `strategic` | `project` | strategic |
| `semantic` | `domain` | `project` | domain |
| `procedural` | none | `project` | procedural |
| `episodic` | none | `session` / `project` | episodic |

Memory search 支持 `keyword`、`vector`、`hybrid`。Vector 数据写入 `memory_embedding` 表；本地 fallback 使用 128 维 trigram hash projection。注意：engine 打开的 `MemoryStore` 会使用 `context_engine.embedding`，但 `mcp::server` 和 `mcp::router` 当前通过 `MemoryStore::open()` 打开默认 store，查询侧使用本地 fallback。

## 外部边界

| 边界 | 发起方 | 目标 | 协议/机制 |
| :---| :---| :---| :---|
| Daemon autostart | CLI | `current_exe __daemon` | child process |
| Daemon request | CLI | `127.0.0.1:47661` | TCP JSON line |
| ACP backend | engine | Claude/Codex/Gemini/Hermes/OpenCode adapter | child process stdio JSON-RPC 2.0 |
| ACP permission response | `acp::permission` | ACP backend stdin | JSON-RPC response |
| MCP sidecar via backend | ACP backend | `iota context-mcp` / `iota fun-mcp` | backend-controlled stdio MCP |
| MCP sidecar via skill | `skill::runner` | MCP server | child process stdio JSON-RPC |
| Workspace summary | `context::render_workspace` | `git status --short` | child process |
| Function tools | `skill::fun_server` | python/node/rustc/go/javac/java/clang++/g++/zig/binary | child process |
| Skill pull | CLI | local file or HTTP(S) URL | filesystem/network |
| SQLite stores | engine/MCP/CLI | `~/.i6/context/*.sqlite` | filesystem |

## 依赖规则

允许的方向：

```text
entry -> presentation -> service -> protocol
presentation -> service -> context/store
service -> context + protocol + store + runtime_event
context -> store + skill + selected protocol client
protocol -> runtime_event + store approval/router only
store -> runtime_event or plain data models
config/utils -> shared support
```

约束：

- `crates/iota-core/src/acp/` 不依赖 `crates/iota-core/src/engine/`、`crates/iota-core/src/daemon/`、`crates/iota-cli/src/cli/` 或 `crates/iota-cli/src/tui.rs`。
- Store 模块不调用 UI、daemon、ACP client 或 MCP client。
- TUI 子组件保持在 `crates/iota-cli/src/tui/`，顶层 `crates/iota-cli/src/tui.rs` 只负责组合、事件循环和终端生命周期。
- 外部进程、TCP、网络和 SQLite 边界在文档和实现中保持显式。
- 所有路径处理使用 `Path`/`PathBuf`；home 目录通过 `dirs::home_dir()` 解析。

## 扩展点

| 目标 | 修改位置 | 模式 |
| :---| :---| :---|
| 新 ACP 后端 | `crates/iota-core/src/acp/mod.rs`, `crates/iota-core/src/config/`, `nimia.yaml.template` | 增加 enum、alias、默认命令、`ALL_BACKENDS`、backend config/env/home 映射 |
| 新 CLI 命令 | `crates/iota-cli/src/cli/mod.rs` | 添加 match arm 和 handler，复用 service/context/store |
| 新 TUI 组件 | `crates/iota-cli/src/tui/*`, `crates/iota-cli/src/tui.rs` | 状态和渲染下沉到子模块，顶层只组合 |
| 新 RuntimeEvent | `crates/iota-core/src/runtime_event/`, 相关生产方 | 增加事件类型；按需接入 CLI/TUI 输出 |
| 新 memory 能力 | `crates/iota-core/src/memory/`, `crates/iota-core/src/mcp/tool_dispatch.rs` | Store 拥有 schema/query，tool_dispatch 暴露工具 |
| 新 MCP 工具 | `crates/iota-core/src/mcp/tool_dispatch.rs`，`crates/iota-core/src/mcp/server.rs` tools()，必要时 `crates/iota-core/src/mcp/router.rs` | 添加 descriptor、dispatch handler、路由策略 |
| 新 engine-run skill 行为 | `crates/iota-core/src/skill/mod.rs`, `crates/iota-core/src/skill/runner.rs` | 扩展 metadata/runner，不改变 ACP prompt path |
