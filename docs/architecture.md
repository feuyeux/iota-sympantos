# iota-sympantos 架构总览

iota-sympantos 是一个 Rust workspace，用 ACP 编排多个 AI 编程助手后端，并在同一套配置、Context Fabric、memory、skill、observability 和 Kanban 体系下提供 CLI、TUI、daemon 和 Tauri desktop 入口。

相关文档：

- [code-call-chains.md](code-call-chains.md)
- [command.md](command.md)
- [observability.md](observability.md)
- [debugging.md](debugging.md)
- [desktop-mvp-acceptance.md](desktop-mvp-acceptance.md)

## Workspace 结构

```text
crates/
├── iota-cli/
│   └── src/
│       ├── main.rs
│       ├── cli/                  # run/check/bench/kanban/skill/observability/mcp/__daemon
│       └── tui/                  # interactive terminal UI
├── iota-core/
│   └── src/
│       ├── acp/                  # ACP backend process + JSON-RPC wire
│       ├── config/               # ~/.i6/nimia.yaml
│       ├── context/              # ContextEngine + capsule
│       ├── daemon/               # TCP daemon + desktop protocol
│       ├── engine/               # IotaEngine orchestration
│       ├── mcp/                  # iota-context MCP + router + tool dispatch
│       ├── memory/               # memory taxonomy + embedding + recall
│       ├── runtime_event/        # normalized event stream
│       ├── skill/                # skill registry + engine-run MCP skill + iota-fun
│       ├── store/                # cache/approvals/ledger/observability SQLite stores
│       └── telemetry/            # tracing + OpenTelemetry
├── iota-kanban/
│   └── src/                      # board/task state machine, SQLite event sourcing, worker, sync
└── iota-desktop/
    ├── src/                      # React UI
    └── src-tauri/                # Tauri commands + daemon client + Kanban commands
```

## 分层架构

```text
Entry
  iota-cli main.rs
  iota-desktop Tauri main.rs

Presentation
  CLI commands
  TUI
  React desktop workbench

Service orchestration
  IotaEngine
  daemon EnginePool
  desktop daemon protocol
  Kanban dispatcher/bridge

Context and tools
  ContextEngine
  MemoryStore
  SkillRegistry
  MCP server/router/tool_dispatch
  iota-fun

Protocol and external boundaries
  ACP child processes
  MCP stdio sidecars
  TCP daemon JSON-line protocol
  git/compiler/interpreter child processes

Stores and observability
  SQLite stores under ~/.i6
  RuntimeEvent
  OpenTelemetry, Loki, Jaeger, Prometheus
```

## Crate 职责

| Crate | 职责 |
| :--- | :--- |
| `iota-cli` | 用户命令入口、TUI、daemon autostart、observability 查询、Kanban CLI |
| `iota-core` | ACP/MCP/daemon/engine/config/context/memory/skill/store/telemetry 核心运行时 |
| `iota-kanban` | Kanban 领域模型、状态机、SQLite event sourcing、Hermes worker、shadow workspace、event sync |
| `iota-desktop` | Tauri + React desktop，复用 daemon streaming protocol；提供 chat/config/inspector/memory/context UI，并在 Rust commands 中接入 Kanban store |

## 核心模块

| 模块 | 职责 |
| :--- | :--- |
| `engine/` | 按 `(backend, cwd)` 复用 ACP client；处理 session ledger、handoff、memory recall/write、skill short-circuit、context capsule、ACP 调用、events 和 store 写回 |
| `acp/` | 后端枚举、命令解析、子进程生命周期、`initialize/session/new/session/prompt`、stream reader、permission、wire parse |
| `daemon/` | 默认 `127.0.0.1:47661` TCP daemon；legacy CLI request/response；desktop protocol v2；config、backend check、observability、memory/context snapshot |
| `config/` | 唯一读取 `~/.i6/nimia.yaml`；生成 effective config、backend command/env、context options、MCP server 注入 |
| `context/` | 组装 `<iota-context>` capsule：session、memory tools、memory buckets、working memory、workspace、skills、handoff |
| `memory/` | 六桶 memory taxonomy、FTS/LIKE、vector/hybrid search、embedding API 或 local trigram fallback |
| `skill/` | workspace/config/home skill 加载；trigger 匹配；engine-run MCP skill；skill pull/cache；iota-fun 7 语言 MCP server |
| `mcp/` | iota-context MCP stdio server、MCP client、ACP tool-call router、共享 tool dispatch |
| `store/` | execution lifecycle、approval、session ledger、observability SQLite stores |
| `runtime_event/` | 把 ACP update、complete、permission、usage、tool、error 归一为 `RuntimeEvent` |
| `telemetry/` | tracing、OpenTelemetry meter/exporter |

## 运行路径

### CLI 直接执行

```text
iota run [backend] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
  -> config::read_config()
  -> IotaEngine::create_session()
  -> IotaEngine::run_with_timing()
       -> execution lifecycle
       -> session ledger + handoff
       -> memory extraction / recall
       -> skill match and optional engine-run MCP skill
       -> context capsule
       -> ACP session/prompt
       -> RuntimeEvent + token usage + memory/session writeback
  -> stdout
```

### CLI 经 daemon 执行

```text
iota run --daemon ...
  -> daemon client connects IOTA_DAEMON_ADDR or 127.0.0.1:47661
  -> if failed, spawn current_exe __daemon
  -> daemon EnginePool::engine_for(cwd)
  -> same IotaEngine prompt path
  -> DaemonPromptResponse JSON line
```

### TUI 执行

```text
iota
  -> tui::run(config)
  -> TuiApp + IotaEngine
  -> install TUI approval channel
  -> crossterm raw mode + mouse capture + TerminalGuard
  -> event loop
       -> input/history/slash commands
       -> tokio engine task
       -> streaming chunks over mpsc
       -> approval overlay
       -> markdown/status/render
```

### Desktop 执行

```text
React ChatWorkbench
  -> Tauri command submit_prompt
  -> src-tauri daemon_client::start_turn()
  -> TCP Hello + StartTurn
  -> daemon desktop handler
  -> IotaEngine::run_with_timing()
  -> TextChunk / TurnEvent / ApprovalRequested / TurnCompleted
  -> Tauri emits daemon-message
  -> turnsReducer updates transcript and inspector
```

## ACP 后端

| Backend | 默认命令 | 别名 | 备注 |
| :--- | :--- | :--- | :--- |
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@latest` | `claude`, `claude-code`, `claudecode` | Claude Code ACP adapter |
| Codex | `npx -y @zed-industries/codex-acp@0.12.0` | `codex` | Codex ACP adapter |
| Gemini CLI | `npx -y @google/gemini-cli@latest --acp` | `gemini`, `gemini-cli` | Gemini ACP mode |
| Hermes | `hermes acp` | `hermes`, `hermes-agent` | 不覆盖 `HERMES_HOME` |
| OpenCode | `npx -y opencode-ai@latest acp` | `opencode`, `open-code` | OpenCode ACP mode |

Windows 上 `normalize_command()` 会把 `npx` 改为 `npx.cmd`。

## 配置模型

配置只从 `~/.i6/nimia.yaml` 读取。顶层包含五个 backend section、`model`、`context_engine`、`context_engine_backend` 和 store/observability 相关配置。

Model env 映射：

| Backend | 映射 |
| :--- | :--- |
| Claude Code | `ANTHROPIC_API_KEY`、`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_BASE_URL`、`ANTHROPIC_MODEL` |
| Codex | `OPENAI_API_KEY`、`ROUTER_API_KEY`、`OPENAI_BASE_URL`、`OPENAI_MODEL`，并按需追加 Codex `-c` 配置 |
| Gemini | `GEMINI_API_KEY`、`GEMINI_MODEL` |
| Hermes | `HERMES_INFERENCE_PROVIDER`、`HERMES_MODEL` 和 provider 原生 key/base URL |
| OpenCode | `OPENCODE_MODEL` |

Hermes 使用自己的默认 home，配置和 desktop 都不应覆盖 `HERMES_HOME`。

## 数据和存储

| Store | 默认路径 | 作用 |
| :--- | :--- | :--- |
| `MemoryStore` | `~/.i6/context/memory.sqlite` | memory taxonomy、recall、search、embedding |
| `CacheStore` | `~/.i6/context/events.sqlite` | execution lifecycle、status、fencing |
| `ObservabilityStore` | `~/.i6/context/events.sqlite` | token usage events、summary、metrics |
| `SessionLedger` | `~/.i6/context/sessions.sqlite` | sessions、backend sessions、turns、handoff |
| `ApprovalStore` | `~/.i6/context/approvals.sqlite` | approval request/decision |
| `SqliteKanbanStore` | `~/.i6/kanban/iota.db` | board/task/comment/link/run/event sourcing |

## Kanban

`iota-kanban` 提供：

- `Task`、`Board`、`Run`、`Comment`、`Link` 领域类型。
- 状态机：`triage -> todo -> ready -> running -> done -> archived`，支持 `blocked`。
- SQLite event-sourced store。
- `Dispatcher` 调度 ready task 给 Hermes worker。
- `ShadowMaterializer` 和 `ShadowWatcher` 管理 shadow workspace。
- `AdvancedBridge` 支持 `specify` 和 `decompose`。
- event sync：export/import/serve/pull/push。

## Desktop

Desktop 由 React + Tauri 组成，当前界面是一个 daemon-first 的本地工作台：

- Frontend：`ChatWorkbench` 是主 shell，包含 Chat/Config 视图、后端选择器、daemon 状态、prompt form 和可调宽右侧 inspector。
- Inspector：`RightInspector` 承载 `Observability`、`Memory`、`Context` 三个 tab；`MemoryContextWorkspace` 是只读 memory bucket 和 runtime context capsule 浏览器。
- State：`turnReducer` 只处理 turn 状态，折叠 `TextChunk`、`TurnEvent`、approval、cancel、failure 和 late daemon error。
- Tauri commands：config、prompt、approval、cancel、backend check、observability summary、memory/context snapshot、current workspace、Kanban CRUD。
- Daemon protocol：`DESKTOP_PROTOCOL_VERSION = 2`，使用 `DaemonClientMessage` 和 `DaemonServerMessage` tagged enum。
- Daemon autostart：优先连接默认 daemon；失败时尝试 desktop fallback address；再通过 `IOTA_CLI_PATH`、sibling binary 或 `PATH` 启动 `iota __daemon`。
- Kanban：Rust commands 直接打开 `~/.i6/kanban/iota.db`；当前 React workbench 尚未暴露 Kanban board UI。

## 依赖规则

- `iota-core/src/acp/` 不依赖 CLI、TUI、desktop 或 daemon UI 层。
- Store 模块只暴露 typed operations，不调用 UI、daemon、ACP client 或 MCP client。
- Presentation 层不直接拥有 ACP session；后端执行统一经过 `IotaEngine` 或 daemon API。
- 外部进程、TCP、network、SQLite 边界必须显式。
- 路径处理使用 `Path`/`PathBuf`，home 目录通过 `dirs::home_dir()`。
- 测试必须放在独立 `*_tests.rs` 文件中，用 `#[path = "..."]` 引用，禁止内联 `mod tests`。

## 扩展点

| 目标 | 修改位置 |
| :--- | :--- |
| 新 ACP 后端 | `acp/`、`config/`、`nimia.yaml.template`、backend env/home 映射 |
| 新 CLI 命令 | `crates/iota-cli/src/cli/mod.rs` 和独立 handler |
| 新 TUI 组件 | `crates/iota-cli/src/tui/*`，状态和渲染下沉到子模块 |
| 新 RuntimeEvent | `runtime_event/` 和相关 producer/consumer |
| 新 MCP 工具 | `mcp/tool_dispatch.rs`、`mcp/server.rs`、必要时 `mcp/router.rs` |
| 新 memory 能力 | `memory/` 和 `mcp/tool_dispatch.rs` |
| 新 desktop daemon 消息 | `daemon/proto.rs`、`daemon/desktop.rs`、`src-tauri/src/daemon_client.rs`、frontend reducer/types |
| 新 Kanban 行为 | `iota-kanban` domain/store/state machine，CLI 和 desktop commands 按需接入 |
