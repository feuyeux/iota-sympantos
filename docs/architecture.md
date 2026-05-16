# iota-sympantos 架构总览

iota-sympantos 是一个轻量级 Rust CLI/TUI 编排器。它把用户入口、Context Fabric、持久化存储、ACP 后端和 MCP sidecar 串成统一运行时，使 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 可以在同一套配置、记忆和技能体系下工作。

本文按当前源码组织架构。具体调用链见 [code-call-chains.md](code-call-chains.md)，观测命令见 [observability.md](observability.md)，调试手册见 [debugging.md](debugging.md)。

## 当前源码结构

```text
src/
├── main.rs                  # binary 入口，注册模块并进入 cli::run()
├── cli/
│   └── mod.rs               # 命令分发、daemon autostart、bench、logs/trace、native、skill
├── tui.rs                   # ratatui 主循环、终端生命周期、engine task、stream/approval channel
├── tui/
│   ├── composer.rs          # 多行输入、Unicode 光标、历史搜索、kill/yank、word motion
│   ├── markdown.rs          # Markdown 到 ratatui Line 渲染
│   ├── status_bar.rs        # 底部状态栏
│   ├── theme.rs             # TUI 主题
│   └── state.rs             # 对话、历史和观测展示状态
├── engine.rs                # IotaEngine 编排、ACP client pool、上下文、skill、store 写回
├── acp/
│   ├── mod.rs               # ACP backend enum、子进程生命周期、JSON-RPC 请求/响应、prompt loop
│   ├── permission.rs        # ACP 权限请求、TUI approval channel、iota tool 自动批准
│   ├── session.rs           # session/new 参数和 mcpServers 渲染
│   └── wire.rs              # line read/parse、response id 匹配、ACP error 格式化
├── daemon/
│   ├── mod.rs               # 本机 TCP daemon，单请求单响应 JSON line，warm/prompt
│   ├── pool.rs              # EnginePool，按 cwd 复用 IotaEngine
│   └── proto.rs             # daemon wire types
├── config.rs                # ~/.i6/nimia.yaml、有效配置、后端 env/command、context options
├── context/
│   └── mod.rs               # ContextEngine、context capsule、WorkingMemoryBuffer、workspace summary
├── skill/
│   ├── mod.rs               # SkillRegistry、frontmatter、trigger、backend compatibility
│   ├── runner.rs            # execution.mode=mcp 的 engine-run skill
│   ├── cache.rs             # skill pull/cache
│   └── fun_server.rs        # iota-fun MCP stdio server，7 语言代码片段执行
├── mcp/
│   ├── mod.rs               # MCP 模块入口
│   ├── client.rs            # engine 侧 stdio MCP client
│   ├── server.rs            # iota-context MCP stdio server（从 context/server.rs 迁移）
│   ├── router.rs            # ACP 侧 tool-call 拦截，委托 tool_dispatch
│   └── tool_dispatch.rs     # 共享工具派发逻辑（解析器、验证器、handlers）
├── native/
│   └── mod.rs               # memory/skill 原生文件投影
├── store/
│   ├── mod.rs               # store layer 入口
│   ├── approval.rs          # approval 事件记录和默认风险分类
│   ├── embedding.rs         # Ollama API / 本地 trigram embedding
│   ├── cache.rs             # execution lifecycle
│   ├── ledger.rs            # session、backend session、turn、handoff
│   └── memory.rs            # memory taxonomy、FTS、vector/hybrid search、recall buckets
├── runtime_event.rs         # 统一 RuntimeEvent
└── utils.rs                 # 时间戳、摘要、poison lock recovery
```

## 分层架构

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ Entry                                                                      │
│   main.rs                                                                  │
└────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌────────────────────────────────────────────────────────────────────────────┐
│ Presentation                                                               │
│   cli/mod.rs                         tui.rs + tui/*                        │
│   CLI command routing                 interactive terminal UI              │
└────────────────────────────────────────────────────────────────────────────┘
                    │                                     │
                    ▼                                     ▼
┌────────────────────────────────────────────────────────────────────────────┐
│ Service Orchestration                                                       │
│   engine.rs                         daemon/mod.rs + pool.rs + proto.rs      │
│   IotaEngine, turn lifecycle         warm local service over TCP            │
└────────────────────────────────────────────────────────────────────────────┘
      │                    │                      │                     │
      ▼                    ▼                      ▼                     ▼
┌───────────────┐   ┌────────────────┐   ┌────────────────┐   ┌────────────────┐
│ Context       │   │ Protocol       │   │ Store          │   │ Runtime Events │
│ context/*     │   │ acp/*, mcp/*   │   │ store/*        │   │ runtime_event  │
│ skill/*       │   │ JSON-RPC       │   │ SQLite         │   │ normalized     │
│ native/*      │   │ stdio/TCP      │   │ + embedding    │   │ event stream   │
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
│   config.rs: ~/.i6/nimia.yaml, commands, env, MCP/session options           │
│   utils.rs: timestamps, summarization, lock recovery                        │
└────────────────────────────────────────────────────────────────────────────┘
```

## 模块职责

### Entry

| 模块 | 职责 | 下游 |
|---|---|---|
| `main.rs` | 注册顶层模块，启动 Tokio runtime，调用 `cli::run()` | `cli` |

`main.rs` 不持有业务策略。

### Presentation

| 模块 | 职责 | 主要下游 |
|---|---|---|
| `cli/mod.rs` | 解析命令并分发 `run/check/tui/bench/logs/trace/context-mcp/fun-mcp/native-materialize/skill/__daemon`；负责 daemon autostart 和 CLI 输出 | `config`, `engine`, `daemon`, `acp`, `store`, `native`, `skill`, `tui` |
| `tui.rs` | 终端生命周期、事件循环、prompt 队列、后台 engine task、流式输出、approval 浮层、pager/help/quit overlay | `engine`, `acp::permission`, `tui/*` |
| `tui/composer.rs` | 多行编辑、历史、搜索、词移动和 kill buffer | 无项目级依赖 |
| `tui/markdown.rs` | Markdown 渲染为 ratatui 文本行 | 无项目级依赖 |
| `tui/status_bar.rs` | backend/model/快捷键/观测状态栏 | `acp`, `tui::state` |
| `tui/theme.rs` | ratatui 颜色和样式 | 无项目级依赖 |
| `tui/state.rs` | 对话历史和观测展示模型 | 无项目级依赖 |

Presentation 层不直接拥有 ACP session；后端执行统一经过 `IotaEngine` 或 daemon client API。

### Service Orchestration

| 模块 | 职责 | 主要下游 |
|---|---|---|
| `engine.rs` | 核心编排门面。按 `(backend, cwd)` 维护 ACP client pool；处理 session ledger、handoff、memory recall/write、skill 短路、context capsule、ACP 调用和事件落库 | `acp`, `config`, `context`, `skill`, `store`, `runtime_event` |
| `daemon/mod.rs` | `127.0.0.1:47661` 默认 TCP daemon；支持 `IOTA_DAEMON_ADDR`；每连接一条 JSON request/response；8 并发限流；10 MiB 请求上限；Ctrl+C 优雅关闭 | `engine`, `daemon::pool`, `daemon::proto` |
| `daemon/pool.rs` | `EnginePool` 按 cwd 复用 `IotaEngine`，从而复用 ACP 子进程和 session/handoff 状态 | `engine`, `config` |
| `daemon/proto.rs` | `DaemonPromptRequest`、`DaemonPromptResponse`、`DaemonWarmRequest` | `runtime_event`, `acp::AcpPromptTiming` |

`engine.rs` 是行为决策边界；SQL、ACP wire、MCP JSON-RPC 和 TUI 渲染仍保留在各自模块。

### Protocol

| 模块 | 职责 | 主要下游 |
|---|---|---|
| `acp/mod.rs` | `AcpBackend`、默认 adapter 命令、`parse_acp_args()`、ACP 子进程启动、`initialize/session/new/session/prompt`、流式事件读取和 timing | `acp::permission`, `acp::session`, `acp::wire`, `mcp::router`, `runtime_event` |
| `acp/session.rs` | 生成 `session/new` params；渲染 `mcpServers`；支持 `always_send_empty_mcp_servers` 和 env `string_array/object` 两种形态 | `acp::AcpBackend` |
| `acp/wire.rs` | ACP stdout line timeout、JSON parse、response id 判断、error 格式化 | 无项目级依赖 |
| `acp/permission.rs` | 处理 `session/request_permission`；`iota_*`、`mcp__iota-*` 或 backend `tool_whitelist` 命中时自动批准；否则走 TUI 或 stdin；记录 approval 事件 | `store::approval`, `runtime_event` |
| `mcp/client.rs` | engine-run skill 使用的 stdio MCP client；启动 server、initialize、tools/call | 无项目级依赖 |
| `mcp/server.rs` | `iota-context` MCP stdio server；JSON-RPC 协议适配，工具执行委托 `tool_dispatch` | `mcp::tool_dispatch`, `runtime_event`, `memory`, `store::ledger`, `skill` |
| `mcp/router.rs` | 拦截 ACP 侧 `tools/call` / `mcp/tools/call` / `mcp/tool_call`；委托 `tool_dispatch` 执行 iota 工具，拒绝外部工具 | `mcp::tool_dispatch`, `memory`, `store::ledger`, `skill`, `skill::fun_server` |
| `mcp/tool_dispatch.rs` | 共享工具派发逻辑：`ToolContext` 依赖注入、`dispatch_tool()` 统一入口、所有解析器和验证器 | `memory`, `store::ledger`, `skill` |
| `runtime_event.rs` | 把 ACP update、complete、permission、usage、tool、error 统一为 `RuntimeEvent` | `acp::extract_text` |

协议层只做协议翻译和安全路由，不依赖 CLI/TUI/daemon/engine。

### Context Fabric

| 模块 | 职责 | 主要下游 |
|---|---|---|
| `context/mod.rs` | 组装 `<iota-context>` capsule：session/model、memory tools 提示、memory buckets、working memory、workspace `git status --short`、skill index、handoff | `config`, `memory`, `skill` |
| `skill/mod.rs` | 加载 workspace `skills/`、workspace `.iota/skills`、配置 roots、`~/.i6/skills`；解析 YAML frontmatter；按 backend 和 trigger 匹配 | `acp::AcpBackend` |
| `skill/runner.rs` | 执行 `execution.mode = mcp` skill；可顺序或并行调用 MCP tools；渲染 template | `mcp::client`, `runtime_event`, `skill` |
| `skill/cache.rs` | 从本地路径或 HTTP(S) 拉取 skill，并写入 `~/.i6/skills` | filesystem/network |
| `skill/fun_server.rs` | `iota-fun` MCP stdio server；运行 `fun.python/typescript/rust/go/java/cpp/zig` | 外部解释器/编译器 |
| `native/mod.rs` | 将 memory/skill 投影到 backend 原生文件，使用 `<!-- IOTA_START -->` / `<!-- IOTA_END -->` 块替换 | `memory`, `skill`, `acp::AcpBackend` |

Context Fabric 提供 prompt 背景、可确定工具和对不支持 MCP 后端的原生投影。

### Store

| 模块 | 职责 | 默认路径 |
|---|---|---|
| `store/cache.rs` | execution lifecycle、status、fencing token | `~/.i6/context/events.sqlite` |
| `memory/store.rs` | memory taxonomy、dedup、TTL、merge mode、recall buckets、FTS/LIKE、vector/hybrid search | `~/.i6/context/memory.sqlite` 或 `context_engine.memory_db` |
| `memory/embedding.rs` | embedding 计算；engine 按 `context_engine.embedding` 配置 Ollama `/api/embeddings`，失败或未配置时走 128 维本地 trigram fallback | 存入 `memory_embedding` 表 |
| `store/approval.rs` | approval request/decision 记录、风险维度分类、默认人工审批策略 | `~/.i6/context/approvals.sqlite` |
| `store/ledger.rs` | iota session、backend session、turn、handoff | `~/.i6/context/sessions.sqlite` |

Store 模块只暴露 typed operations，不调用 UI、daemon、ACP client 或 MCP client。

### Configuration

| 模块 | 职责 |
|---|---|
| `config.rs` | 唯一读取 `~/.i6/nimia.yaml`；构建 `EffectiveConfig`；展开 `~/`；规范化 Windows `npx`；渲染 backend command/env；注入 context MCP server；读取 recall thresholds、embedding、skill roots、backend whitelist 和 session options |
| `utils.rs` | `now_ts()`、`summarize()`、`lock_or_recover()` |

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
  -> terminal guard + panic hook + raw mode
  -> event loop
       -> Composer handles keys
       -> submit creates background engine task
       -> ACP chunks stream over Tokio mpsc
       -> permission request appears as approval overlay
       -> render history/composer/status/pager/help/approval
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
|---|---|---|---|
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
|---|---|
| `enabled` | 是否参与 `check`、warm、bench |
| `acp.command` / `acp.args` | ACP adapter 启动命令 |
| `version_mapping` | 记录 adapter/bin 版本，供 `check` 输出 |
| `home` | backend 自定义 home；Codex/Hermes 当前不映射 home env |
| `model` | provider/name/base_url/api_key |
| `tool_whitelist` | 权限自动批准规则，支持简单 wildcard |

### Model env 映射

| Backend | 映射 |
|---|---|
| Claude Code | `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, `ANTHROPIC_MODEL`, `ANTHROPIC_SMALL_FAST_MODEL`, `ANTHROPIC_DEFAULT_*_MODEL`, `API_TIMEOUT_MS`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` |
| Codex | `ROUTER_API_KEY`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`；同时追加 `-c model=...`、`model_provider`、provider base_url/env_key/wire_api |
| Gemini | `GEMINI_API_KEY`, `GEMINI_MODEL` |
| Hermes | `HERMES_INFERENCE_PROVIDER`, `HERMES_MODEL`，以及 provider 原生 key/base_url，例如 `MINIMAX_CN_API_KEY` |
| OpenCode | `OPENCODE_MODEL` |

### Context engine section

| 字段 | 含义 |
|---|---|
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
|---|---|
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
ToolCall
ToolResult
Error
Extension
TokenUsage
Memory
ApprovalRequest
ApprovalDecision
```

`RuntimeEvent` 随 `AcpPromptOutput.events` 返回；engine 只把 execution lifecycle 和 timing 写入本地 store。

### Memory taxonomy

| Type | Facet | 典型 scope | Recall bucket |
|---|---|---|---|
| `semantic` | `identity` | `user` | identity |
| `semantic` | `preference` | `user` | preference |
| `semantic` | `strategic` | `project` | strategic |
| `semantic` | `domain` | `project` | domain |
| `procedural` | none | `project` | procedural |
| `episodic` | none | `session` / `project` | episodic |

Memory search 支持 `keyword`、`vector`、`hybrid`。Vector 数据写入 `memory_embedding` 表；本地 fallback 使用 128 维 trigram hash projection。注意：engine 打开的 `MemoryStore` 会使用 `context_engine.embedding`，但 `mcp::server` 和 `mcp::router` 当前通过 `MemoryStore::open()` 打开默认 store，查询侧使用本地 fallback。

## 外部边界

| 边界 | 发起方 | 目标 | 协议/机制 |
|---|---|---|---|
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

- `acp/*` 不依赖 `engine.rs`、`daemon/*`、`cli/*` 或 `tui.rs`。
- Store 模块不调用 UI、daemon、ACP client 或 MCP client。
- TUI 子组件保持在 `src/tui/`，顶层 `tui.rs` 只负责组合、事件循环和终端生命周期。
- 外部进程、TCP、网络和 SQLite 边界在文档和实现中保持显式。
- 所有路径处理使用 `Path`/`PathBuf`；home 目录通过 `dirs::home_dir()` 解析。

## 扩展点

| 目标 | 修改位置 | 模式 |
|---|---|---|
| 新 ACP 后端 | `src/acp/mod.rs`, `src/config.rs`, `nimia.yaml.template` | 增加 enum、alias、默认命令、`ALL_BACKENDS`、backend config/env/home 映射 |
| 新 CLI 命令 | `src/cli/mod.rs` | 添加 match arm 和 handler，复用 service/context/store |
| 新 TUI 组件 | `src/tui/*`, `src/tui.rs` | 状态和渲染下沉到子模块，顶层只组合 |
| 新 RuntimeEvent | `runtime_event.rs`, 相关生产方 | 增加事件类型；按需接入 CLI/TUI 输出 |
| 新 memory 能力 | `memory/`, `mcp/tool_dispatch.rs` | Store 拥有 schema/query，tool_dispatch 暴露工具 |
| 新 MCP 工具 | `mcp/tool_dispatch.rs`，`mcp/server.rs` tools()，必要时 `mcp/router.rs` | 添加 descriptor、dispatch handler、路由策略 |
| 新 engine-run skill 行为 | `skill/mod.rs`, `skill/runner.rs` | 扩展 metadata/runner，不改变 ACP prompt path |
| 新 native projection | `native/mod.rs`, `cli/mod.rs` | 添加目标路径和 render/apply 分支 |
