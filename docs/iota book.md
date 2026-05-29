# iota book

面向程序员与 AI 从业者的 iota-sympantos 技术指南。

本书根据当前代码实现整理。它解释 iota-sympantos 是什么、为什么这样设计，以及关键模块如何协同工作。读者不需要先读完整源码，但建议把本书和 `docs/architecture.md`、`docs/code-call-chains.md`、`docs/command.md` 对照使用。

![runtime architecture](../img_result/runtime_architecture.png)

## 目录

1. [项目定位](#项目定位)
2. [Workspace 总览](#workspace-总览)
3. [运行时主线](#运行时主线)
4. [ACP 后端适配](#acp-后端适配)
5. [配置系统](#配置系统)
6. [Context Fabric](#context-fabric)
7. [Memory 系统](#memory-系统)
8. [Skill 与 iota-fun](#skill-与-iota-fun)
9. [MCP 工具层](#mcp-工具层)
10. [RuntimeEvent 与可观测性](#runtimeevent-与可观测性)
11. [CLI 与 TUI](#cli-与-tui)
12. [Daemon 热路径](#daemon-热路径)
13. [Kanban 长任务系统](#kanban-长任务系统)
14. [Desktop 工作台](#desktop-工作台)
15. [存储与数据边界](#存储与数据边界)
16. [跨平台设计](#跨平台设计)
17. [Docker 与外部观测栈](#docker-与外部观测栈)
18. [扩展开发指南](#扩展开发指南)
19. [测试与工程约束](#测试与工程约束)
20. [文档地图](#文档地图)
21. [从代码继续阅读](#从代码继续阅读)

## 项目定位

iota-sympantos 是一个轻量级 Rust workspace。它用 ACP（Agent Control Protocol）统一驱动多个 AI 编程助手后端，并在这些后端之上提供同一套本地能力：上下文注入、持久记忆、技能加载、MCP 工具、运行事件、可观测性和 Kanban 长任务调度。

从用户视角看，它是一个 CLI/TUI/Desktop 工具：

- `iota run codex "..."` 用单次命令调用一个后端。
- `iota` 进入 ratatui 交互式终端。
- `iota run --daemon ...` 复用常驻 daemon 中的热 ACP 进程。
- `crates/iota-desktop` 提供 Tauri + React 本地工作台。
- `iota kanban ...` 把长任务写入事件溯源任务板，并可以交给 Hermes worker 执行。

从系统视角看，它更像一个本地 agent runtime：

- ACP 后端负责模型推理和编码动作。
- iota-core 负责进程、协议、上下文、记忆、技能和事件归一化。
- SQLite 是本地状态的单一持久化层。
- MCP 是模型读写 iota 能力的标准工具接口。
- RuntimeEvent 是 UI、日志、token usage 和 inspector 共享的事件语言。

为什么需要这层 runtime？因为单个 AI 编程助手通常把配置、上下文、记忆、工具权限和可观测性绑在自己的产品内。iota 的目标是把这些能力抽出来，让 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 五个后端可以共享同一个本地工作记忆和任务系统。

## Workspace 总览

![layered architecture](../img_result/layered_architecture.png)

当前 workspace 包含四个 crate：

| Crate | 类型 | 职责 |
| :--- | :--- | :--- |
| `iota-cli` | Binary crate | CLI 命令、TUI、daemon client、Kanban CLI、observability CLI |
| `iota-core` | Library crate | ACP/MCP/daemon/engine/config/context/memory/skill/store/telemetry 核心运行时 |
| `iota-kanban` | Library crate | Event-sourced Kanban、状态机、Hermes worker、shadow DB、跨节点同步 |
| `iota-desktop/src-tauri` | Tauri crate | 桌面端 Rust 命令、daemon client、Kanban 命令绑定 |

核心目录结构：

```text
crates/
├── iota-cli/src/
│   ├── cli/                 # run/check/bench/mcp/skill/kanban/observability/__daemon
│   └── tui/                 # ratatui terminal UI
├── iota-core/src/
│   ├── acp/                 # ACP process and JSON-RPC wire protocol
│   ├── config/              # ~/.i6/nimia.yaml schema and effective config
│   ├── context/             # <iota-context> capsule composer
│   ├── daemon/              # TCP daemon, legacy CLI protocol, desktop protocol v2
│   ├── engine/              # IotaEngine orchestration
│   ├── mcp/                 # iota-context MCP server, router, tool dispatch
│   ├── memory/              # SQLite memory, taxonomy, FTS/vector/hybrid search
│   ├── runtime_event/       # normalized runtime events
│   ├── skill/               # skill registry, pull/cache, engine-run skill, iota-fun
│   ├── store/               # cache, approvals, session ledger, observability stores
│   └── telemetry/           # tracing and OpenTelemetry
├── iota-kanban/src/         # Kanban domain, event sourcing, dispatcher, worker, sync
└── iota-desktop/            # React frontend + Tauri backend
```

为什么拆成这些 crate？`iota-core` 保持 UI 无关，便于 CLI、TUI、daemon 和 desktop 共享同一套运行时；`iota-kanban` 独立成领域库，避免任务板逻辑和 ACP 编排互相污染；`iota-cli` 和 `iota-desktop` 是不同 presentation 层。

## 运行时主线

![execution flowchart](../img_result/execution_flowchart.png)

一次 prompt turn 的核心路径在 `IotaEngine::run()`：

```text
prompt
  -> request_hash
  -> load and match skills
  -> ensure session ledger
  -> prepare backend handoff
  -> begin execution cache record
  -> recall memory and render workspace concurrently
  -> compose <iota-context>
  -> ensure ACP client
  -> session/prompt
  -> collect RuntimeEvent
  -> persist token usage
  -> finish execution
  -> record session turn
  -> push working memory
  -> write episodic memory
```

这条路径有两个重要设计点。

第一，iota 在调用后端前先处理本地上下文。memory recall、skill index、working memory、workspace git 状态和 backend handoff 会被组装成 `<iota-context>`，再和用户原始 prompt 一起发送给后端。这样不同 ACP 后端都能看到同一套背景信息。

第二，iota 把后端输出转换为结构化事件。ACP 的 streaming update、permission request、tool call、usage、complete 和 error 都会映射成 `RuntimeEvent`，再被 CLI/TUI/Desktop/observability 使用。UI 不需要理解每个后端的原生协议差异。

## ACP 后端适配

![backend ipc stages](../img_result/backend_ipc_stages.png)

ACP 层位于 `crates/iota-core/src/acp`。它负责启动外部后端进程，通过 stdin/stdout 的换行分隔 JSON-RPC 2.0 驱动协议：

```text
initialize
  -> session/new
  -> session/prompt
  -> session/update ...
  -> session/request_permission? ...
  -> session/complete
```

当前支持五个后端：

| Backend | 默认命令 | 别名 | 特殊点 |
| :--- | :--- | :--- | :--- |
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@latest` | `claude`, `claude-code`, `claudecode` | 支持 Anthropic env 映射 |
| Codex | `npx -y @zed-industries/codex-acp@0.12.0` | `codex` | 通过 `-c` 追加 Codex provider/model 配置 |
| Gemini CLI | `npx -y @google/gemini-cli@latest --acp` | `gemini`, `gemini-cli` | 支持 `GEMINI_API_KEY` / `GEMINI_MODEL` |
| Hermes | `hermes acp` | `hermes`, `hermes-agent` | 不覆盖 `HERMES_HOME` |
| OpenCode | `npx -y opencode-ai@latest acp` | `opencode`, `open-code` | 使用 `OPENCODE_MODEL` |

为什么使用外部进程而不是 SDK？因为这些工具本身就是成熟的 coding agent。iota 只需要在它们已有的 ACP 边界上统一上下文、权限和事件，不必重新实现每个 provider 的工具调用协议。

ACP 进程启动使用 `tokio::process::Command`，stdin/stdout/stderr 全部 piped，并设置 `kill_on_drop(true)`。这让 Rust runtime 能在跨平台环境中正确回收子进程。Windows 上 `normalize_command()` 会把 `npx` 改写为 `npx.cmd`。

## 配置系统

![configuration env mapping](../img_result/configuration_env_mapping.png)

配置只从 `~/.i6/nimia.yaml` 读取，不读取项目级配置，也不做自动发现。这是一个刻意选择：agent runtime 的配置里包含 API key、base URL、模型名、后端 home、MCP 注入和 store retention；把它集中到用户 home 下可以降低误提交和跨项目污染风险。

主要 schema 在 `crates/iota-core/src/config`：

| 文件 | 职责 |
| :--- | :--- |
| `schema.rs` | `NimiaConfig` 和 `StoreConfig` |
| `backend.rs` | `BackendConfig`、readiness、command/env 映射入口 |
| `adapters.rs` | 五个后端的 `BackendAdapter` 实现 |
| `context.rs` | Context engine、MCP server、embedding、budget、threshold 配置 |
| `effective.rs` | 把 raw YAML 解析成带默认值的 effective config |
| `paths.rs` | `~/.i6/context` 下的 store path |
| `loader.rs` | `config_path()`、`read_config()`、`save_config()` |

模型配置通过 backend adapter 映射为外部后端期望的环境变量：

| Backend | 关键映射 |
| :--- | :--- |
| Claude Code | `ANTHROPIC_API_KEY`、`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_BASE_URL`、`ANTHROPIC_MODEL` |
| Codex | `OPENAI_API_KEY`、`ROUTER_API_KEY`、`OPENAI_BASE_URL`、`OPENAI_MODEL` |
| Gemini | `GEMINI_API_KEY`、`GEMINI_MODEL` |
| Hermes | `HERMES_INFERENCE_PROVIDER`、`HERMES_MODEL` 和 provider 原生 key/base URL |
| OpenCode | `OPENCODE_MODEL` |

Hermes adapter 的 `home_env_key()` 返回 `None`，所以即使配置里有 home，也不会覆盖 Hermes 自己的默认 home。这是为了避免破坏 Hermes 在 Windows/macOS/Linux 上已有的目录约定。

## Context Fabric

![architecture overview](../img_result/architecture_overview.png)

Context Fabric 的核心类型是 `ContextEngine`。它把多个本地来源组装成一个 XML 风格的 `<iota-context>` capsule：

```text
<iota-context>
  <memory-tools>...</memory-tools>
  <model>...</model>
  <skills>...</skills>
  <memory>...</memory>
  <session>...</session>
  <handoff>...</handoff>
  <working-memory>...</working-memory>
  <workspace>...</workspace>
</iota-context>

User request:
...
```

为什么用 capsule？因为不同后端的 prompt API 不一定有相同的 system/developer/context slot。把上下文渲染为一段普通文本，可以最大化兼容性。同时 XML-like tag 让模型容易区分“背景数据”和“用户真实请求”。

Context Fabric 有预算控制：

| Budget | 默认用途 |
| :--- | :--- |
| `memory_chars` | 记忆召回内容 |
| `skills_chars` | skill index |
| `working_memory_chars` | 最近多轮摘要 |
| `workspace_chars` | git/workspace 状态 |
| `handoff_chars` | 后端切换摘要 |

实现上还有一个低延迟优化：短小且不涉及上下文的 trivial prompt 会走 minimal capsule，跳过 memory、skill 和 workspace 的大段注入。普通路径中 memory recall 和 `git status --short` 会并发执行，减少 prompt 前置等待。

## Memory 系统

![memory taxonomy lifecycle](../img_result/memory_taxonomy_lifecycle.png)

Memory 系统位于 `crates/iota-core/src/memory`，默认数据库是 `~/.i6/context/memory.sqlite`。它提供三类 memory type、四类 semantic facet 和四种 scope：

| 维度 | 值 |
| :--- | :--- |
| Type | `semantic`、`episodic`、`procedural` |
| Semantic facet | `identity`、`preference`、`strategic`、`domain` |
| Scope | `user`、`project`、`session`、`global` |

召回时会整理成六个 bucket：

![memory recall buckets](../img_result/memory_recall_buckets.png)

| Bucket | 来源 |
| :--- | :--- |
| `identity` | semantic + identity |
| `preference` | semantic + preference |
| `strategic` | semantic + strategic |
| `domain` | semantic + domain |
| `procedural` | procedural |
| `episodic` | episodic |

为什么要分桶？因为 agent 需要区分“用户是谁”“用户偏好什么”“项目长期目标是什么”“领域事实是什么”“可复用流程是什么”和“最近发生了什么”。如果把所有记忆做成一串无结构文本，模型很难判断哪些事实应该长期稳定，哪些只是某次会话的痕迹。

存储实现有几个关键点：

- 使用 SQLite，优先 FTS5 做全文检索。
- 用 SHA-256 content hash 做去重。
- 支持 `auto`、`add`、`update`、`none` merge mode。
- 支持 TF-IDF embedding 的 vector/hybrid search；如果配置了 embedding API，则 `EmbeddingEngine` 可走外部 embedding 服务。
- 插入前先计算 embedding，再拿数据库 mutex，避免外部 embedding 调用阻塞 SQLite 连接。

模型写 memory 的主入口不是让模型直接改库，而是 MCP 工具 `iota_memory_write`。上下文里要求模型先加载 `iota-memory-taxonomy` skill，再按 taxonomy 写入原子 memory record。这样可以把“判断该记什么”的策略和“存储 schema 校验”分开。

## Skill 与 iota-fun

![skill system pipeline](../img_result/skill_system_pipeline.png)

Skill 层位于 `crates/iota-core/src/skill`。它加载 `.md` 或 `.yaml` skill manifest，根据 trigger 匹配 prompt，并支持两种使用方式：

| 模式 | 说明 |
| :--- | :--- |
| Advisory | 把 skill metadata/body 注入上下文，由后端模型阅读并执行 |
| MCP / engine-run | iota runtime 本地执行确定性工具，必要时短路外部 ACP 调用 |

Skill roots 来自配置，默认包含：

```text
~/.i6/skills
./.iota/skills
```

仓库内还内置了 core skill `iota-memory-taxonomy`，用于指导 memory 分类和写入粒度。

`iota-fun` 是一个 MCP stdio server，支持运行七种语言片段：C++、Go、Java、Python、Rust、TypeScript、Zig。它适合放确定性小工具，例如示例 skill `skills/pet-generator`。为什么不是所有 skill 都让模型自由执行 shell？因为 deterministic skill 能提供可测试、可缓存、权限边界更清晰的本地能力。

## MCP 工具层

MCP 层位于 `crates/iota-core/src/mcp`。它既提供 stdio server，也提供 ACP stream 中的 tool-call interceptor。

当前 `iota-context` MCP server 暴露的工具由 `tool_dispatch.rs` 的 registry 管理：

| Tool | 用途 |
| :--- | :--- |
| `iota_memory_search` | 搜索本地 memory |
| `iota_memory_write` | 写入一条 memory record |
| `iota_skill_search` | 搜索可用 skill index |
| `iota_skill_load` | 读取指定 skill 完整内容 |
| `iota_session_summary` | 读取 session 摘要 |
| `iota_handoff_publish` | 发布 backend handoff 摘要 |
| `iota_handoff_read` | 读取 handoff |

为什么把工具派发集中到 `tool_dispatch.rs`？因为同一套业务逻辑需要被两个入口复用：

- stdio MCP server：后端通过 `mcpServers` 正常调用工具。
- ACP router：后端输出中出现 iota tool-call 风格事件时，runtime 可以拦截并执行。

集中派发避免出现“stdio server 和 router 行为不一致”的隐性 bug。

## RuntimeEvent 与可观测性

![observability architecture](../img_result/observability_architecture.png)

`RuntimeEvent` 是 iota 的统一事件语言。它位于 `crates/iota-core/src/runtime_event`，覆盖输出、状态、日志、工具调用、工具结果、错误、扩展事件、token usage、memory、approval request 和 approval decision。

为什么不让 UI 直接消费 ACP 原始事件？因为五个后端 adapter 的 usage、tool update 和 permission 形状都可能不同。归一化之后：

- CLI 可以用 `--log-events` 打印事件。
- TUI 可以把 output、approval 和 token meta 渲染进 transcript。
- Desktop 可以把事件折叠到 inspector。
- ObservabilityStore 可以只关心 `TokenUsage`。
- OpenTelemetry 可以在同一套字段上打 span/metric/log。

Token usage 是当前实现里最重要的观测对象。`RuntimeEvent::TokenUsage` 会尽量归一化 OpenAI、Anthropic、Gemini 和 adapter-only usage 字段：

| 字段 | 含义 |
| :--- | :--- |
| `input_tokens` | 输入 token |
| `cache_read_input_tokens` | provider cache read |
| `cache_creation_input_tokens` | provider cache write |
| `output_tokens` | 输出 token |
| `thinking_tokens` | reasoning/thought token |
| `provider_reported_total_tokens` | provider 原始 total |
| `normalized_total_tokens` | iota 计算或归一化后的 total |

`ObservabilityStore` 写入 `~/.i6/context/events.sqlite`。同一 execution 里如果同时有 streaming usage 和 final usage，查询层会按完整度选择 canonical record，避免 summary 被重复计数。

## CLI 与 TUI

CLI 入口在 `crates/iota-cli/src/cli/mod.rs`。当前命令族：

| 命令 | 作用 |
| :--- | :--- |
| `iota` | 进入 TUI |
| `iota run [backend] [options] <prompt>` | 单次 prompt |
| `iota check [--daemon]` | 输出后端配置和 readiness |
| `iota bench <cold|warm>` | 冷/热启动 benchmark |
| `iota mcp <context|fun>` | 启动 MCP server |
| `iota skill pull <source> [name]` | 拉取 skill |
| `iota kanban ...` | Kanban board/task/dispatch/sync |
| `iota observability ...` | token/log/trace/metric 查询 |
| `iota __daemon` | 内部 daemon 入口 |

TUI 位于 `crates/iota-cli/src/tui`。它不是简单 stdin prompt，而是一个 ratatui 应用：

- 多行输入，支持 Unicode grapheme 光标。
- Kill buffer、Ctrl+U/Ctrl+W、Alt+B/Alt+F、Ctrl+R 历史搜索。
- Markdown 渲染和 scrollback。
- 流式输出增量渲染。
- Approval overlay。
- Ctrl+T pager、`?` help、二次 Ctrl+C 退出确认。
- Tab queue：后端运行时先缓存下一条输入。
- `/kanban` 和 `/memory` 本地 slash command。

为什么 TUI 直接持有 `IotaEngine`，而 desktop 走 daemon？TUI 是单进程终端工具，直接持有 engine 可以减少 IPC；desktop 需要 GUI 生命周期、自动启动和跨窗口事件流，daemon-first 更适合。

## Daemon 热路径

![code call chains](../img_result/code_call_chains.png)

Daemon 位于 `crates/iota-core/src/daemon`，默认监听 `127.0.0.1:47661`。它提供两套 JSON-line 协议：

| 协议 | 使用方 | 形状 |
| :--- | :--- | :--- |
| Legacy prompt protocol | CLI `--daemon`、bench、check warm | 一次请求，一次响应 |
| Desktop protocol v2 | Tauri desktop | Hello handshake，多消息 streaming turn |

核心类型是 `EnginePool`。它按 cwd 复用 `IotaEngine`，而 `IotaEngine` 内部按 `(backend, cwd)` 复用 ACP client。这样 warm path 能跳过 expensive process spawn、initialize 和 session/new。

为什么按 cwd 分池？很多 coding agent 的上下文、权限、MCP server 和 workspace state 都和目录绑定。跨 cwd 复用同一个 backend session 可能把项目 A 的上下文泄漏到项目 B。

Desktop protocol v2 支持：

- `Hello`
- `StartTurn`
- `RespondApproval`
- `CancelTurn`
- `GetConfig`
- `SaveBackendModel`
- `CheckBackend`
- `GetObservabilitySummary`
- `GetMemoryContextSnapshot`

Server 侧会回发 `TextChunk`、`TurnEvent`、`ApprovalRequested`、`TurnCompleted`、`TurnFailed`、`TurnCancelled` 等消息。

## Kanban 长任务系统

![kanban state machine event sourcing](../img_result/kanban_state_machine_event_sourcing.png)

`iota-kanban` 是一个 event-sourced task board。它的状态机是：

```text
triage -> todo -> ready -> running -> done -> archived
                         \-> blocked -> ready
                                      \-> done
running -> ready  # claim expired
```

为什么内置 Kanban？AI 编程任务经常不是一次 prompt 能完成的：它们需要拆分、排队、执行、观察、失败恢复和同步。Kanban 给 agent runtime 一个结构化任务层，而不是把所有长期目标都塞在对话历史里。

核心模块：

| 模块 | 职责 |
| :--- | :--- |
| `types.rs` | Task、Board、Run、Comment、Link、KanbanEvent |
| `store.rs` | `KanbanStore` trait |
| `sqlite_store.rs` | SQLite event-sourced implementation |
| `state_machine.rs` | 合法状态迁移 |
| `dispatcher.rs` | 扫描 ready task，调度 worker |
| `worker.rs` | 启动/杀死 Hermes `-z` worker |
| `shadow.rs` | 把单个 task materialize 到 Hermes 兼容 shadow DB |
| `bridge.rs` | `specify` / `decompose` 高级编排 |
| `event_sync.rs` | export/import/serve/pull/push event bundle |

Shadow DB 是这个设计的关键。

![kanban event sync bridge](../img_result/kanban_event_sync_bridge.png)

iota 的主库是事实来源，Hermes 不直接写主库。调度时：

1. `ShadowMaterializer` 为 task 创建 `shadows/{task_id}/kanban.db`。
2. `WorkerHandle` 启动 `hermes -z`，并设置 `HERMES_KANBAN_DB` 指向 shadow DB。
3. Hermes 在 shadow DB 中读取任务、写入完成事件。
4. `ShadowWatcher` 轮询 shadow DB，把终态同步回主库。
5. 成功后清理 shadow directory。

这样可以复用 Hermes 现有 Kanban 能力，同时把 iota 主库和外部 worker 隔离开。

## Desktop 工作台

![desktop tauri architecture](../img_result/desktop_tauri_architecture.png)

Desktop 位于 `crates/iota-desktop`。它由 React 前端和 Tauri Rust 后端组成，当前实现是 daemon-first：

```text
React ChatWorkbench
  -> src/api.ts invoke/listen wrappers
  -> Tauri commands in src-tauri/src/lib.rs
  -> daemon_client::connect_or_start()
  -> iota __daemon desktop protocol v2
  -> EnginePool / IotaEngine / ACP backend
  -> daemon-message / daemon-client-error window events
  -> turnReducer updates transcript and inspector state
```

前端主要组件：

| 组件 | 职责 |
| :--- | :--- |
| `ChatWorkbench.tsx` | 主 shell、后端选择、prompt form、Chat/Config view、daemon 状态、inspector 宽度 |
| `ConfigPanel.tsx` | 后端模型配置编辑，API key masked |
| `RightInspector.tsx` | turn 详情、approval、cancel、observability、memory/context tabs |
| `MemoryContextWorkspace.tsx` | 只读 memory bucket 和 runtime context capsule 浏览 |
| `turnReducer.ts` | 折叠 daemon stream message 与 RuntimeEvent |
| `api.ts` | Tauri invoke/listen 封装 |

Tauri command 主要分两类：

- Daemon-backed commands：`get_config`、`save_backend_model`、`submit_prompt`、`cancel_turn`、`handle_approval`、`check_backend`、`get_observability_summary`、`get_memory_context_snapshot`。
- Direct Kanban commands：`list_boards`、`list_tasks`、`create_task`、`transition_task`、`list_comments`、`add_comment`。

当前 React workbench 还没有挂载 Kanban board UI，但 Rust command surface 已经接入 `SqliteKanbanStore`，数据库位于 `~/.i6/kanban/iota.db`。

为什么 desktop 不直接创建 `IotaEngine`？因为 daemon 能统一热路径、配置保存、approval registry、turn cancel 和 runtime event streaming。Tauri 只做本地 UI bridge，不成为第二套 runtime。

## 存储与数据边界

所有本地持久化都在 `~/.i6` 下：

| Store | 默认路径 | 内容 |
| :--- | :--- | :--- |
| `MemoryStore` | `~/.i6/context/memory.sqlite` | memory taxonomy、FTS、embedding、recall |
| `CacheStore` | `~/.i6/context/events.sqlite` | execution lifecycle、running lock、fencing |
| `ObservabilityStore` | `~/.i6/context/events.sqlite` | token usage events、summary、percentiles |
| `SessionLedger` | `~/.i6/context/sessions.sqlite` | logical sessions、turns、backend handoff |
| `ApprovalStore` | `~/.i6/context/approvals.sqlite` | approval request/decision |
| `SqliteKanbanStore` | `~/.i6/kanban/iota.db` | board/task/comment/link/run/events |

SQLite 连接通过 `Arc<Mutex<Connection>>` 共享，并设置 WAL 和 `synchronous=NORMAL`。这适合本地单用户 agent runtime：部署简单、可备份、可直接调试，且不需要外部数据库。

安全边界：

- 不提交 `~/.i6/nimia.yaml`。
- 文档和调试输出必须打码 API key/token。
- `--show-native` 可能暴露原生协议内容，只用于本地调试。
- Approval 请求会通过 TUI/Desktop 交互确认；iota 自有工具可按白名单自动批准。

## 跨平台设计

iota 要求 Windows/macOS/Linux 同时可用。代码中已经体现出几个原则：

- Home 目录通过 `dirs::home_dir()` 获取。
- 路径使用 `Path` / `PathBuf`，不手写 `\` 或 `/` 拼接。
- Windows 上 `npx` 归一化为 `npx.cmd`。
- 子进程使用 `Stdio::piped()` 和 `kill_on_drop(true)`。
- Hermes home 不由 iota 覆盖，避免平台差异造成配置损坏。
- 配置模板里可以写 `~/...`，运行时由 `expand_home_path()` 展开。

为什么要这么严格？因为 agent runtime 会启动外部进程、读写本地库、注入 MCP server，并且常驻 daemon。平台差异如果留给调用现场处理，很容易变成难排查的进程泄漏、路径错误或配置污染。

## Docker 与外部观测栈

Docker 方案不是运行时必须条件，而是把 daemon 和观测依赖放进可重复环境中。它适合做集成测试、长时间运行 daemon、或在固定容器里连接宿主机 workspace。

仓库中有两组 compose：

| 文件 | 作用 |
| :--- | :--- |
| `docker/docker-compose.yml` | 启动 `iota-daemon` 和完整观测栈 |
| `docker/observability/docker-compose.yml` | 只启动 OpenTelemetry Collector、Jaeger、Prometheus、Loki、Grafana 等观测服务 |

`docker/Dockerfile` 使用 `rust:1.95-slim-bookworm` 构建 release 版 `iota`，runtime 镜像安装 `git`、`curl`、`sqlite3`、`nodejs`、`npm`。为什么 runtime 还需要 Node？因为 Claude Code、Codex、Gemini 和 OpenCode 的 ACP adapter 都可能通过 `npx` 启动。

默认端口：

| 服务 | 端口 | 说明 |
| :--- | :--- | :--- |
| iota daemon | `47661` | CLI/desktop daemon protocol |
| OTLP collector | `4317`, `4318` | trace/metric/log ingestion |
| Jaeger | `16686` | trace 查询 |
| Prometheus | `9090` | metrics |
| Loki | `3100` | log 查询 |
| Grafana | `3000` | dashboard |

为什么外部观测和本地 SQLite observability 并存？SQLite 适合离线、低依赖、按 execution 查询 token usage；OpenTelemetry/Loki/Jaeger/Prometheus 适合跨进程、跨时间窗口的运行时排障。两者服务的问题不同，因此都保留。

## 扩展开发指南

### 新增 ACP 后端

新增后端需要同时处理协议枚举、配置、命令、环境变量和文档。最小步骤：

1. 在 `crates/iota-core/src/acp/backend.rs` 添加 `AcpBackend` 变体、alias、`Display` 和 `ALL_BACKENDS`。
2. 在 `crates/iota-core/src/config/adapters.rs` 增加 `BackendAdapter`，定义默认 ACP command、home env、model env 映射和必要的追加参数。
3. 在 `crates/iota-core/src/config/schema.rs` / `backend.rs` 接入新的 backend config section。
4. 在 `nimia.yaml.template` 添加配置模板，注意不要写真实 API key。
5. 更新 `docs/command.md`、`docs/architecture.md`、本书和相关 `SKILL.md`。
6. 增加独立 `*_tests.rs`，覆盖 alias 解析、env 映射、command normalization 和 session/new 参数。

为什么扩展点在 config adapter 而不是散落在 engine 里？因为 engine 只应该知道“我要一个可启动的 ACP client”。后端差异属于边界适配问题，放进 adapter 能避免每加一个后端就污染 prompt 主线。

### 新增 MCP 工具

新增 iota 工具应优先走 `crates/iota-core/src/mcp/tool_dispatch.rs`：

1. 实现一个 `McpTool`。
2. 在 `McpToolRegistry::new()` 注册。
3. 把真实依赖放进 `ToolContext`，避免工具内部自己随意打开数据库或读配置。
4. 为 stdio server 和 router 共用路径添加测试。
5. 更新 Context Fabric 中对工具的提示，必要时新增 skill 指导模型何时调用。

为什么要先改 tool dispatch？因为 stdio MCP server 和 ACP router 都依赖它。只改 server 会造成“后端通过 MCP 能用，router 拦截不能用”的不一致。

### 新增 Skill

Skill 是最轻量的扩展方式。普通 advisory skill 只需要一个带 frontmatter 的 `SKILL.md`；engine-run skill 还需要声明 MCP server、工具和输出模板。建议规则：

- trigger 要具体，避免普通 prompt 误命中。
- deterministic 能力放进 MCP sidecar，模型判断放进 skill body。
- 输出模板保持稳定，便于测试。
- 需要持久化知识时，复用 `iota-memory-taxonomy`，不要在 skill 中发明另一套 memory 分类。

为什么 skill roots 支持 `~/.i6/skills` 和 workspace `.iota/skills`？用户级 skill 适合个人习惯，workspace skill 适合项目约定。两者都需要，但优先级和触发条件必须可解释。

## 测试与工程约束

本仓库测试有一条明确约束：Rust 单元测试必须放在独立 `*_tests.rs` 文件中，源文件只用：

```rust
#[cfg(test)]
#[path = "module_tests.rs"]
mod tests;
```

测试文件使用 `use crate::...` 绝对路径，不使用 `use super::*`，测试函数直接放在文件顶层。

为什么这样做？因为该项目模块多、边界多。独立测试文件能让生产代码更短，测试命名更稳定，也便于 AI coding 工具在修改实现时快速定位对应测试。

常用验证命令：

```bash
cargo test
cargo check --offline
cd crates/iota-desktop && npm test && npm run build
```

Desktop 开发使用：

```bash
cd crates/iota-desktop
npm run dev:clean
```

`dev:clean` 会停止旧 daemon，构建当前 workspace 的 `iota` CLI，并设置 `IOTA_CLI_PATH`，避免 Tauri 连到 PATH 中的旧 binary。

## 文档地图

当前文档分三层：

| 层级 | 位置 | 用法 |
| :--- | :--- | :--- |
| 当前手册 | `docs/iota book.md`、`architecture.md`、`code-call-chains.md`、`command.md`、`observability.md`、`debugging.md`、`docker.md` | 读当前系统行为和操作方式 |
| 模块上下文 | 各 crate/module 的 `SKILL.md` | 给 AI coding 工具快速理解局部模块 |
| 历史记录 | `gefsi/` | 保留实验结果 |

为什么保留历史记录？因为很多实现决策来自实验和计划，例如 daemon-first desktop、token usage 归一化、Kanban shadow DB。它们不是最新命令手册，但能解释当前代码为什么长成这样。

## 从代码继续阅读

如果你想从源码深入，建议按下面顺序：

1. `crates/iota-cli/src/cli/mod.rs`：看用户命令如何进入 runtime。
2. `crates/iota-core/src/engine/prompt.rs`：看一次 prompt turn 的真实主线。
3. `crates/iota-core/src/acp/client.rs` 和 `stream_reader.rs`：看 ACP 进程与流式事件。
4. `crates/iota-core/src/context/mod.rs`：看 `<iota-context>` 如何被渲染。
5. `crates/iota-core/src/memory/store.rs`：看 memory taxonomy、去重、search 和 recall。
6. `crates/iota-core/src/mcp/tool_dispatch.rs`：看 MCP 工具的统一业务实现。
7. `crates/iota-core/src/runtime_event/mod.rs`：看原生协议如何归一成事件。
8. `crates/iota-core/src/daemon/proto.rs` 和 `desktop.rs`：看 daemon IPC。
9. `crates/iota-kanban/src/sqlite_store.rs`、`dispatcher.rs`、`shadow.rs`：看长任务系统。
10. `crates/iota-desktop/src/components/ChatWorkbench.tsx` 和 `src-tauri/src/lib.rs`：看桌面端如何复用 daemon runtime。

本书的目标不是替代源码，而是给出一张可靠地图：iota-sympantos 的核心思想是把 AI 编程助手后端当作可替换执行器，把上下文、记忆、技能、工具、权限、事件和任务管理放在本地 Rust runtime 中统一治理。
