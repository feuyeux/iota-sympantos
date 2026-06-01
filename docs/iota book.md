# iota-sympantos book

iota-sympantos 是一个用 Rust 写的轻量级 agent harness。一句话概括：它用 ACP（Agent Control Protocol）把好几个 AI 编程助手统一驱动起来，然后在这些后端之上补齐它们各自缺的那层本地能力——上下文注入、持久记忆、技能加载、MCP 工具、运行事件、可观测性，以及 Kanban 长任务调度。

这本书想讲清楚的不只是「iota 怎么用」，更是「iota 为什么这样设计」：每个模块背后都有一组明确的取舍，知道它**不做**什么，往往比知道它做什么更能说明它的边界。

![runtime architecture](../img_result/runtime_architecture.png)

站在用户这一侧，iota 就是一个 CLI/TUI/Desktop 工具：

- `iota run codex "..."` 一条命令打一次后端。
- `iota` 直接进 ratatui 交互式终端。
- `iota run --daemon ...` 复用常驻 daemon 里已经热好的 ACP 进程。
- `crates/iota-desktop` 是 Tauri + React 的本地工作台。
- `iota kanban ...` 把干不完的长任务写进事件溯源任务板，再丢给 Hermes worker 去跑。

但往里看一层，它其实是一个本地 agent runtime，各部分各司其职：

- ACP 后端只管模型推理和写代码这件事。
- iota-core 管进程、协议、上下文、记忆、技能，以及把五花八门的事件归一成一种。
- SQLite 是所有本地状态唯一的落地点。
- MCP 是模型读写 iota 能力时走的标准工具接口。
- RuntimeEvent 是 UI、日志、token usage、inspector 共用的那门「事件语言」。

那为什么要单独造这么一层 runtime？因为今天每个 AI 编程助手都倾向于把配置、上下文、记忆、工具权限、可观测性一股脑塞进自己的产品里，彼此并不相通。换个后端，记忆就断了，工具授权要重来一遍。iota 想做的，就是把这些能力从单个产品里拆出来、沉到本地，让 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 这五个后端共享同一套工作记忆和任务系统。后端可以随时换，runtime 这层不变。

## 产品设计哲学

iota 的每一处实现，背后都站着一组明确的「做什么 / 不做什么」立场。这些立场不是事后总结出来的漂亮话，而是真正决定了 iota 的边界——也决定了它不去和谁抢饭碗。理解整套系统，得先从最底下这块地基讲起。

这块地基，就是把 AI 编程助手当成可替换的「执行器」，而不是产品。iota 通过 ACP 协议，把 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 一视同仁地看成「推理 + 写代码」的执行器：同一次会话里，你可以从 codex 切到 claude 再切回来，上下文不丢。它刻意不去碰的，是每个后端原生的工具调用协议、prompt 模板和权限体系——iota 不重写这些，不自带 LLM 客户端，也不试图把各家的 reasoning 风格捏成一种。道理很朴素：这些工具本身就是成熟的 coding agent，重造一遍只会很快过时。iota 只在它们暴露出来的 ACP 边界上做拦截、归一和编排，把推理这件事整个交还给后端，自己专心做 runtime 该做的事。这也是贯穿全书的第一性原则——**iota 的价值在编排，不在推理**——后面六条立场，都是从这一条延伸出来的。

顺着这条原则往下走，第二块地基是**本地优先（local-first），干脆就按单用户单机来设计**。所有持久化都落 SQLite，所有配置只读 `~/.i6/nimia.yaml`，daemon 默认监听 `127.0.0.1:47661`，可观测数据双写本地 SQLite 和（可选的）OpenTelemetry 栈。它不要求外部数据库、消息队列或中央配置服务，不假设多租户，也不内建网络鉴权。这么定是因为 agent runtime 干的全是「贴着机器」的活——直接读写本地源码、调本地 shell、起外部进程。本地优先换来三件实在的事：部署就一行命令、备份就拷一个目录、调试就直接打开 sqlite。至于多用户、多租户，那是调用方在 iota 之上自己该搭的，不该让 runtime 背这个包袱。

本地优先还顺手定下了配置的归宿：**配置只有一个源——`~/.i6/nimia.yaml`，不搞项目级自动发现**。backend、model、API key、MCP server、retention 这些 runtime 配置全部集中读这一份文件，缺了哪项，由 `effective.rs` 给一个能讲清楚来历的默认值。它不读 `./iota.yaml`、`.iota/config.yaml`、`pyproject.toml` 之类的同名配置，也不做「一路往上找最近一份配置」的自动发现。原因在于 agent 配置里塞着 API key、provider base URL、后端 home 这些东西，集中到 user home 才躲得开三个坑：误提交进版本库、跨项目互相污染，以及最阴的那种——「在 A 项目下不小心用了 B 项目的 key」。项目级的偏好应该靠 workspace skill（`./.iota/skills`）和 cwd-scoped memory 去表达，而不是再开一份配置文件。

配置之外，上下文怎么递给后端也是一处刻意的取舍：**上下文是一个 capsule，不是协议字段**。iota 把 memory、skill、working memory、workspace 状态、handoff、session 元数据统统拼成一段 XML 风格的文本（`<iota-context>`），跟用户的 prompt 一起发过去。它不指望各后端的 system prompt slot、developer message 或专有 context API，也不奢望后端能「读懂 iota 的私有协议」。capsule 这种形式对所有后端都是可读的纯文本，兼容性最好；XML-like 的 tag 让模型一眼能分清哪些是「背景数据」、哪些才是「用户真正的请求」；拼好的 capsule 还能离线 diff、回放、审计——这些是埋进协议字段里做不到的。

上下文进得去，工具调用也得管得住：**工具调用必须可拦截、可解释、可审计**。Approval、tool call、tool result、token usage、memory 写入全都归一成 `RuntimeEvent` 落进 SQLite；MCP 工具同时挂在 stdio server 和 ACP stream router 两条路上，保证派发逻辑是同一套；外部 MCP 工具默认一律拒绝，并用 `isError:true` 的 envelope 把拒绝原因原样回传给模型。它不让后端绕过 iota 直接动 memory DB 或 Kanban DB，既不静默批准，也不静默拒绝。道理很直接：只要后端有本事悄悄写本地状态，runtime 的「事实来源」当场就塌了。把事件归一、把调用拦下来，UI、可观测性、ledger 看到的才是同一个世界——否则你永远在追查「数据库里这条记录到底谁写的」。

把单次调用管住之后，跨越多次 prompt 的长任务又是另一回事：**长任务交给 Kanban，别交给对话历史**。凡是「一次 prompt 干不完」的活，都装进 `iota-kanban` 的事件溯源任务板，由状态机推着走 triage→todo→ready→running→done，再由 dispatcher 调度 Hermes worker 去执行，而 Hermes 只能透过 shadow DB 间接碰主库。iota 不指望 agent 在对话上下文里「记住还要做什么」，也不靠外部任务系统（Jira/Linear）来协调 dispatcher。这是因为对话历史会被截断、被压缩、跨 session 直接丢掉，拿它当「待办清单」迟早出事；任务板才是结构化、能恢复、能同步的事实层，再加一层 shadow DB，外部 worker 崩了也脏不到 iota 主库。

最后一块地基贯穿前面所有实现：**跨平台是硬约束，不是「锦上添花」**。home 路径一律走 `dirs::home_dir()`，命令过一遍 `normalize_command()`（Windows 上把 `npx` 改成 `npx.cmd`），子进程统一 `Stdio::piped()` + `kill_on_drop(true)`。runtime 代码里绝不硬编码 `~`、`/` 或 `\`，也不擅自接管外部 home 目录（比如 Hermes 的 `HERMES_HOME`，配置里写了也不覆盖）。这层 runtime 要起外部进程、写本地库、注入 MCP server，还得常驻成 daemon，任何平台差异只要往后拖到「调用现场再处理」，最后都会变成最难查的那类 bug——进程泄漏、路径错乱。所以宁可在代码里把它一次性吃掉。

## iota-sympantos 总览

![layered architecture](../img_result/layered_architecture.png)

iota-sympantos 包含四个 crate：

| Crate | 类型 | 职责 |
| :--- | :--- | :--- |
| `iota-cli` | Binary crate | CLI 命令、TUI、daemon client、Kanban CLI、observability CLI |
| `iota-core` | Library crate | ACP/MCP/daemon/engine/config/context/memory/skill/store/telemetry 核心运行时 |
| `iota-kanban` | Library crate | Event-sourced Kanban、状态机、Hermes worker、shadow DB、跨节点同步 |
| `iota-desktop/src-tauri` | Tauri crate | 桌面端 Rust 命令、daemon client、Kanban 命令绑定 |

核心目录结构：

```sh
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

这套拆分有它的道理：`iota-core` 刻意不碰任何 UI，CLI、TUI、daemon、desktop 才能共用同一份 runtime，不会出现「同一件事四个地方各写一遍」；`iota-kanban` 单独成一个领域库，是为了不让任务板那套状态机逻辑和 ACP 编排搅在一起；至于 `iota-cli` 和 `iota-desktop`，纯粹是两层不同的 presentation，谁也不该知道对方的存在。

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

这条链路看着长，真正的设计意图就两点。

一是「先把家里收拾好，再请客」。在碰后端之前，iota 先把 memory recall、skill index、working memory、workspace 的 git 状态、backend handoff 全部拼进 `<iota-context>`，连同用户原始 prompt 一起递过去。这样无论后端是谁，看到的背景信息都是同一套——后端不需要、也不应该知道这套背景是怎么攒出来的。

二是「进来的千奇百怪，出去的只有一种」。ACP 那边的 streaming update、permission request、tool call、usage、complete、error，形状各不相同，但到了 iota 这层全部映射成 `RuntimeEvent`。于是 CLI/TUI/Desktop/observability 谁都不用去理解五个后端的协议差异，它们只认 `RuntimeEvent` 这一种语言。

### 运行时主线的关键取舍

- **并发预计算**：memory recall 和 workspace 状态采集（`render_workspace`）通过 `tokio::join!` 并发执行（见 `engine/prompt.rs` 的 `memory_task` / `workspace_task`）。两者都是阻塞 I/O、独立无依赖，并发可以把『prompt 前置成本』压缩到单个最慢操作的耗时。
- **Trivial 快速通道**：当 prompt ≤ 80 字符且不含 `iota_memory`/`remember`/`recall`/`skill` 关键词时，`compose_minimal_prompt()` 跳过 memory、skill、workspace 段，仅保留 memory-tools、session、model、handoff。原因是简单问题不值得为它准备完整背景，避免无谓 token 消耗以及 LLM cache 的非必要失效。
- **Deterministic memory answer 短路**：当 prompt 是『我叫什么名字 / 偏好什么』等可被 recall buckets 直接回答的查询时，`deterministic_memory_answer()` 不会调用任何 ACP 后端，直接以本地 engine 角色返回。这条路径让『问记忆』零成本、零网络、零外部依赖。
- **Engine-run skill 优先于后端**：`SkillRegistry::match_skill` 命中且 `runner::run_engine_skill` 返回 `Some` 时，整个 turn 在 iota 内部完成；只有 advisory skill 才让 prompt 继续走向 ACP 后端。原因是 deterministic 工具能提供可测试、可缓存、权限边界更清晰的执行路径。
- **Memory 持久化意图校验（fail-fast）**：当 prompt 里带着『记住 / 持久化』这类意图、但 ACP 输出里**没有**一条成功的 `iota_memory_write` tool result 时，`run()` 会直接把这次 execution 标成 `Failed` 并抛错。这一条看着严苛，其实是刻意的：用户说『记住』，是带着契约预期的；要是让模型回一句『好的，我记住了』就糊弄过去、底下却什么都没写，这种『静默失败』比直接报错危险得多——错觉会一直延续到用户某天发现记忆是空的。宁可当场失败。
- **后端切换走 handoff，不转发 history**：切后端时，只把 working memory 的摘要塞进 `<handoff>` 给新后端，而不是把整段对话历史原样重放一遍。因为不同后端的 session 本来就是各管各的，一份摘要既够新后端接上文，又不至于让 token 账单失控。

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

为什么是起外部进程、而不是接 SDK？还是那句话：这些工具本身就是成熟的 coding agent。iota 只需要在它们现成的 ACP 边界上把上下文、权限、事件统一掉，犯不着去重新实现每家 provider 的工具调用协议——那是吃力不讨好、而且追不上人家迭代的活。

ACP 进程用 `tokio::process::Command` 启动，stdin/stdout/stderr 全部 piped，并设 `kill_on_drop(true)`。这让 Rust runtime 能在跨平台环境下把子进程收干净，不留孤儿进程。Windows 上 `normalize_command()` 会把 `npx` 改写成 `npx.cmd`。

### 执行的幂等性与 fencing

`CacheStore::begin_execution_with_id()`（`store/cache.rs`）在每次 prompt turn 开始时做三件事：

1. **回收僵尸 running**：把同一 `(backend, request_hash)` 下、`started_at < now - running_ttl_secs` 仍挂在 running 的旧记录强制标为 `failed`。这是一种「自愈」设计：daemon 崩了、进程被 kill，都不会留下永久挂起的状态，下一次同请求进来时自动就把场子清了，不需要额外起个监控进程去扫尸。
2. **同 id 幂等返回**：如果调用方传入了 `requested_execution_id` 且其 `request_hash` 完全匹配，直接复用旧 id。这让 daemon 重试或前端重连时不会产生重复 execution。
3. **同 hash 拒绝并发**：同一 `(backend, request_hash)` 已有 running 记录时直接 `bail!("execution already running")`，避免同一请求被并发提交两次。

每条 execution 还会被分配单调递增的 `fencing_token = MAX(fencing_token) + 1`。fencing token 不参与『内容是否相同』的判断，它的唯一职责是给运维和回放工具一个『严格全序』的标识：在事件回放或长任务恢复时，可以判断哪条记录是更晚一次写入，从而拒绝陈旧状态覆盖。

为什么不直接拿 execution_id 来干这些事？因为 execution_id 是 UUID，表达不了全序；而 request_hash 又可能撞重（同一个 prompt 发了多次）。所以干脆三者各管一摊：UUID 给身份，hash 答『是不是同一个请求』，fencing token 给时序。三个问题用三个字段回答，总比硬塞一个字段去兼顾三件事要干净。

## 配置系统

![configuration env mapping](../img_result/configuration_env_mapping.png)

配置只从 `~/.i6/nimia.yaml` 读，不读项目级配置，也不做自动发现。这是个刻意的选择：agent runtime 的配置里塞着 API key、base URL、模型名、后端 home、MCP 注入、store retention 这些东西，把它们集中到用户 home 下，能最大限度躲开误提交和跨项目污染这两个坑。

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

Context Fabric 的核心类型是 `ContextEngine`。它把多个本地来源组装成一个 XML 风格的 `<iota-context>` capsule，各 section 按以下固定顺序排列：

```text
<iota-context>
  <memory-tools>...</memory-tools>   <!-- 1. 静态工具提示，最大化 LLM cache prefix 命中 -->
  <model>...</model>                 <!-- 2. 当前模型名称（如已配置） -->
  <skills>...</skills>               <!-- 3. skill index（如有匹配 skill） -->
  <memory>...</memory>               <!-- 4. 召回的持久记忆 -->
  <session>...</session>             <!-- 5. session_id、backend、cwd -->
  <handoff>...</handoff>             <!-- 6. 后端切换摘要（如有） -->
  <working-memory>...</working-memory> <!-- 7. 最近多轮摘要（如非空） -->
  <workspace>...</workspace>         <!-- 8. git status 变更文件（如非空） -->
</iota-context>

User request:
...
```

这个顺序由 `compose_effective_prompt()` 的实现决定：静态/低频变化的 section 排在前面（最大化 LLM prompt cache prefix 命中），动态/每轮变化的 section 排在最后。

为什么把 cache prefix 命中当成一等公民来伺候？Anthropic、OpenAI 这些 provider 做 prompt cache 都是以『prefix 不变』为复用条件的。memory-tools 文案、model 名称、skill index 这些东西多轮之间几乎不变，session id 在同一会话里也不变；而 handoff、working-memory、workspace 是每轮都可能变的。把易变的 section 挤到最后，前缀就有最大概率命中 cache，单 turn 成本能直接掉一个数量级。这也是为什么 capsule **永远不落磁盘缓存**——每次都重新拼，靠的是 provider 那边的 cache，而不是 iota 自己再维护一套缓存去和它抢生意。

`memory-tools` section 包含持久记忆工具的使用指引。当 MCP 工具可用时，还会注入 Kanban 工具提示，指导模型使用 `iota_kanban_create_task`、`iota_kanban_ready_task`、`iota_kanban_list_tasks` 管理任务，并说明 iota 是 Kanban DB 的唯一事实来源。

为什么用 capsule？因为五个后端的 prompt API 未必有同一套 system/developer/context slot。把上下文渲染成一段普通文本，是兼容性最高的最大公约数；同时 XML-like 的 tag 又让模型能轻松分清「背景数据」和「用户真正的请求」，不至于把上下文当指令误读。

Context Fabric 有预算控制：

| Budget | 默认用途 |
| :--- | :--- |
| `memory_chars` | 记忆召回内容 |
| `skills_chars` | skill index |
| `working_memory_chars` | 最近多轮摘要 |
| `workspace_chars` | git/workspace 状态 |
| `handoff_chars` | 后端切换摘要 |

实现上还有一个低延迟优化：trivial prompt 会走 minimal capsule，跳过 memory、skill 和 workspace 的大段注入。判定条件为：prompt 长度 ≤ 80 字符，且不包含 `iota_memory`、`remember`、`recall`、`skill` 任意关键词。普通路径中 memory recall 和 `git status --short` 会并发执行，减少 prompt 前置等待。

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

为什么要分桶？因为 agent 得分得清「用户是谁」「用户偏好什么」「项目长期目标是什么」「领域事实是什么」「哪些是可复用的流程」和「最近刚发生了什么」。这几件事的「保质期」完全不同：身份和偏好该长期稳定，某次会话的痕迹过几天就该过期。要是把所有记忆揉成一串无结构文本，模型根本分不清哪些该信、哪些该忘。

存储实现有几个关键点：

- 使用 SQLite，优先 FTS5 做全文检索。
- 用 SHA-256 content hash 做去重。
- 支持 `auto`、`add`、`update`、`none` merge mode。
- 支持 TF-IDF embedding 的 vector/hybrid search；如果配置了 embedding API，则 `EmbeddingEngine` 可走外部 embedding 服务。
- `search_vector()` 使用混合评分公式：`score = 0.65 × cosine_similarity + 0.20 × token_overlap + 0.15 × confidence`，过滤掉 score ≤ 0.05 的结果。
- `search_hybrid()` 合并 keyword 和 vector 两路结果，vector 结果权重为 1.2×，keyword 结果权重为 1.0×，按 reciprocal rank 加权后排序。
- 插入前先计算 embedding，再拿数据库 mutex，避免外部 embedding 调用阻塞 SQLite 连接。

为什么是这组权重？纯向量相似度容易『漂』——长得像、意思却不一样的记忆会被拉到最顶上；引入 token overlap 是为了把关键词信号重新拉回来，引入 confidence 则是让用户或工具明确标过的高置信记忆优先浮上来。0.05 的阈值是个『噪声地板』：低于它的结果几乎不可能被模型用上，索性全丢，顺便给 capsule 减负。

为什么 vector 权重是 1.2×、keyword 是 1.0×？hybrid 路径假设语义相似比关键词命中更值得保留，但又不希望 keyword 命中被语义噪声彻底淹没。这个比值是经验值，可通过 `RecallThresholdsConfig` 调整。

为什么『先 embedding 再拿 mutex』这个顺序很重要？embedding 可能要走外网调用（OpenAI/Anthropic 的 embedding API），耗时不可预测；要是先拿住 SQLite 连接的 mutex 再去调 embedding，那一整段网络等待期间所有其它读写都得陥在外面。把网络 I/O 挪到临界区外面，是 SQLite 单连接架构下的常规自卫动作。

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

`iota-fun` 是一个 MCP stdio server，能跑七种语言的代码片段：C++、Go、Java、Python、Rust、TypeScript、Zig。它适合装那些确定性的小工具，比如示例 skill `skills/pet-generator`。为什么不干脆让所有 skill 都自由跑 shell？因为 deterministic 的工具能给出可测试、可缓存、权限边界更清楚的本地能力——这些是『让模型随便执行 shell』换不来的。

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

为什么把工具派发集中到 `tool_dispatch.rs`？因为同一套业务逻辑要被两个入口复用：

- stdio MCP server：后端通过 `mcpServers` 正常调用工具。
- ACP router：后端输出中出现 iota tool-call 风格事件时，runtime 可以拦截并执行。

集中派发避免出现“stdio server 和 router 行为不一致”的隐性 bug。

### Router 的拒绝策略

`mcp/router.rs` 的 `route_tool_call()` 对工具调用分四类处理：

| 工具来源 | 处理 | 返回 |
| :--- | :--- | :--- |
| `tool_dispatch::REGISTRY` 已注册的 iota 工具 | 本地执行，包成 MCP `content` envelope | `isError:false` + structured content |
| `iota-fun` 七语言 sandbox | 走 `skill::fun::run_tool` 本地执行 | `isError:false` |
| 以 `iota_` 前缀但未注册 | 拒绝 | `isError:true`，文案 `not routable in this context` |
| 其它任意外部 MCP 工具 | 拒绝 | `isError:true`，文案 `denied by iota policy` |

为什么默认拒绝外部 MCP 工具？因为在 ACP stream 里拦到的工具调用，本质上是后端在“借 iota 的身份”去干某件事。但 iota 在这个位置并不是 MCP 客户端，既没有连到外部 server，也没有用户授权语义——这时候默认放行才是真正危险的。明确拒绝、并用 `isError:true` 把原因回传，让模型『看得到』为什么被拦、自己调整策略，远比静默吞掉调用要可解释。

## RuntimeEvent 与可观测性

![observability architecture](../img_result/observability_architecture.png)

`RuntimeEvent` 是 iota 的统一事件语言。它位于 `crates/iota-core/src/runtime_event`，覆盖输出、状态、日志、工具调用、工具结果、错误、扩展事件、token usage、memory、approval request 和 approval decision。

为什么不让 UI 直接消费 ACP 原始事件？因为五个后端 adapter 的 usage、tool update、permission 形状都可能不一样。要是让 UI 去适配每一家，那每加一个后端都得改一轮 UI。归一之后，上层只面对一种事件：

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

为什么 TUI 直接持有 `IotaEngine`，而 desktop 走 daemon？TUI 是单进程的终端工具，直接抱住 engine 能省掉一层 IPC；desktop 则要管 GUI 生命周期、要能自动拉起 daemon、还要跨窗口共享事件流，daemon-first 才接得上。

## Daemon 热路径

![code call chains](../img_result/code_call_chains.png)

Daemon 位于 `crates/iota-core/src/daemon`，默认监听 `127.0.0.1:47661`。它提供两套 JSON-line 协议：

| 协议 | 使用方 | 形状 |
| :--- | :--- | :--- |
| Legacy prompt protocol | CLI `--daemon`、bench、check warm | 一次请求，一次响应 |
| Desktop protocol v2 | Tauri desktop | Hello handshake，多消息 streaming turn |

核心类型是 `EnginePool`。它按 cwd 复用 `IotaEngine`，而 `IotaEngine` 内部按 `(backend, cwd)` 复用 ACP client。这样 warm path 能跳过 expensive process spawn、initialize 和 session/new。

为什么按 cwd 分池？很多 coding agent 的上下文、权限、MCP server、workspace state 都是和目录绑在一起的。要是跨 cwd 复用同一个 backend session，很可能把项目 A 的上下文泄漏进项目 B。

`EnginePool::engine_for(cwd)` 用 `BTreeMap<EngineKey, Arc<Mutex<IotaEngine>>>` 做按 cwd 复用：

- **做什么**：相同 cwd 的多次请求复用同一个 `IotaEngine`，从而复用 ACP client、session_id、working memory 和 backend handoff 状态。
- **不做什么**：不按 backend 分池，不按用户分池，不做 session-level GC（pool 在 daemon 生命周期内只增不减；进程退出时由 OS 一次性回收）。
- **为什么**：不按 backend 分是因为 IotaEngine 内部已经按 `(backend, cwd)` 二级 key 复用 ACP client；再在外层按 backend 分会破坏『同一个 cwd 内 backend 切换不丢上下文』。不做 GC 是因为单用户 daemon 通常并发的 cwd 数量很少（数个项目目录），定期 GC 反而会清掉马上要复用的热连接。

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

为什么要内置 Kanban？AI 编程任务经常不是一次 prompt 能干完的：它们要拆分、排队、执行、观察、失败恢复、同步。Kanban 给 agent runtime 一个结构化的任务层，而不是把所有长期目标都填进随时会被截断的对话历史里。

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

这样既能复用 Hermes 现有的 Kanban 能力，又把 iota 主库和外部 worker 隔开，互不打扰。

### Shadow DB 的设计取舍

- **做什么**：为每个被调度的 task 在 `shadows/{task_id}/kanban.db` 创建一份独立的 SQLite，schema 与 hermes_cli 兼容（boards / tasks / task_events / task_runs / task_links / task_comments / kanban_notify_subs）。worker 通过 `HERMES_KANBAN_DB` 指向这份 shadow DB 读写。
- **不做什么**：shadow DB **不是**主库的镜像，只物化『当前正在跑的这一个 task 及其依赖』。也不让 Hermes 直连 iota 主库，即使主库 schema 高度兼容。
- **为什么**：Hermes 的 Kanban 实现假设自己拥有那个 DB（claim、心跳、worker_pid 写回都直接更新行），让它直连主库等于把『事实来源』分一半给外部进程。Shadow + watcher 的回写路径让 iota 始终是 Kanban 主库的唯一 writer，从根本上避免『两边事实漂移』和『主库被 worker crash 损坏』的风险。

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

为什么 desktop 不直接创建 `IotaEngine`？因为 daemon 能把热路径、配置保存、approval registry、turn cancel、runtime event streaming 统统掽在一处。Tauri 只当个本地 UI bridge 就好，不该变成第二套 runtime——一个系统里两套 runtime，輟早要在状态上对不齐。

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

SQLite 连接通过 `Arc<Mutex<Connection>>` 共享，并设了 WAL 和 `synchronous=NORMAL`。这一套恰好契合本地单用户 agent runtime：部署简单、随手可备份、出事能直接打开库调，而且压根不需要外部数据库。

安全边界：

- 不提交 `~/.i6/nimia.yaml`。
- 文档和调试输出必须打码 API key/token。
- `--show-native` 可能暴露原生协议内容，只用于本地调试。
- Approval 请求会通过 TUI/Desktop 交互确认；iota 自有工具可按白名单自动批准。

### Approval 的维度分离

`store/approvals.rs` 的 `ApprovalDimension` 把工具调用按风险维度拆开：

| 维度 | 触发条件 |
| :--- | :--- |
| `Shell` | 任意命令执行 |
| `FileOutsideWorkspace` | 写到 cwd 之外的路径 |
| `Network` | 发起对外网络访问 |
| `McpExternal` | 调用非 iota 管理的外部 MCP 工具 |
| `PrivilegeEscalation` | 提权类操作 |

为什么不直接返回一个 `bool`？因为不同维度的风险等级、补救方式、用户教育成本都不一样。Shell 是高频动作，用户大多数时候会批准；写到工作区外的文件几乎总是误操作，应当醒目告警；网络访问要看目标是不是已知 provider。把维度暴露给 UI，UI 才能用差异化措辞告诉用户『这次为什么需要批准』，也才能基于维度建『自动批准白名单』。

`approval_requests` 和 `approval_decisions` 分两张表存：前者记录『被拦截过什么』，后者记录『用户/策略最终怎么决定』。即使没有 decision 记录，也能从 requests 表里看到模型曾经试图做什么——这是审计意义大于性能意义的设计。

## 跨平台设计

iota 要求 Windows/macOS/Linux 同时可用。代码中已经体现出几个原则：

- Home 目录通过 `dirs::home_dir()` 获取。
- 路径使用 `Path` / `PathBuf`，不手写 `\` 或 `/` 拼接。
- Windows 上 `npx` 归一化为 `npx.cmd`。
- 子进程使用 `Stdio::piped()` 和 `kill_on_drop(true)`。
- Hermes home 不由 iota 覆盖，避免平台差异造成配置损坏。
- 配置模板里可以写 `~/...`，运行时由 `expand_home_path()` 展开。

为什么要这么严格？因为 agent runtime 会起外部进程、读写本地库、注入 MCP server，还得常驻成 daemon。平台差异这东西，只要往后拖到「调用现场再处理」，最后几乎都会变成最难查的那类 bug——进程泄漏、路径错乱、配置污染。与其如此，不如在代码里一次性把它吃掉。

## Docker 与外部观测栈

Docker 方案不是运行时必须条件，而是把 daemon 和观测依赖放进可重复环境中。它适合做集成测试、长时间运行 daemon、或在固定容器里连接宿主机 workspace。

仓库中有两组 compose：

| 文件 | 作用 |
| :--- | :--- |
| `docker/docker-compose.yml` | 启动 `iota-daemon` 和完整观测栈 |
| `docker/observability/docker-compose.yml` | 只启动 OpenTelemetry Collector、Jaeger、Prometheus、Loki、Grafana 等观测服务 |

`docker/Dockerfile` 用 `rust:1.95-slim-bookworm` 构建 release 版 `iota`，runtime 镜像装 `git`、`curl`、`sqlite3`、`nodejs`、`npm`。为什么 runtime 镜像还要 Node？因为 Claude Code、Codex、Gemini、OpenCode 的 ACP adapter 都可能是通过 `npx` 拉起来的，没有 Node 这几个后端压根启不了。

默认端口：

| 服务 | 端口 | 说明 |
| :--- | :--- | :--- |
| iota daemon | `47661` | CLI/desktop daemon protocol |
| OTLP collector | `4317`, `4318` | trace/metric/log ingestion |
| Jaeger | `16686` | trace 查询 |
| Prometheus | `9090` | metrics |
| Loki | `3100` | log 查询 |
| Grafana | `3000` | dashboard |

为什么外部观测和本地 SQLite observability 并存？两者接的是不同的活：SQLite 适合离线、低依赖、按 execution 查 token usage；OpenTelemetry/Loki/Jaeger/Prometheus 适合跨进程、跨时间窗口的运行时排障。问题不同，所以两者都留。

## 扩展开发指南

### 新增 ACP 后端

新增后端需要同时处理协议枚举、配置、命令、环境变量和文档。最小步骤：

1. 在 `crates/iota-core/src/acp/backend.rs` 添加 `AcpBackend` 变体、alias、`Display` 和 `ALL_BACKENDS`。
2. 在 `crates/iota-core/src/config/adapters.rs` 增加 `BackendAdapter`，定义默认 ACP command、home env、model env 映射和必要的追加参数。
3. 在 `crates/iota-core/src/config/schema.rs` / `backend.rs` 接入新的 backend config section。
4. 在 `nimia.yaml.template` 添加配置模板，注意不要写真实 API key。
5. 更新 `docs/command.md`、`docs/architecture.md`、本书和相关 `SKILL.md`。
6. 增加独立 `*_tests.rs`，覆盖 alias 解析、env 映射、command normalization 和 session/new 参数。

为什么扩展点在 config adapter、而不是散落在 engine 里？因为 engine 只该知道一件事：“我要一个能启动的 ACP client”。后端差异本质上是边界适配问题，放进 adapter，才不会每加一个后端就去污染 prompt 主线。

### 新增 MCP 工具

新增 iota 工具应优先走 `crates/iota-core/src/mcp/tool_dispatch.rs`：

1. 实现一个 `McpTool`。
2. 在 `McpToolRegistry::new()` 注册。
3. 把真实依赖放进 `ToolContext`，避免工具内部自己随意打开数据库或读配置。
4. 为 stdio server 和 router 共用路径添加测试。
5. 更新 Context Fabric 中对工具的提示，必要时新增 skill 指导模型何时调用。

为什么要先改 tool dispatch？因为 stdio MCP server 和 ACP router 都依赖它。只改 server，会造成「后端通过 MCP 能用、router 拦截却不能用」这种隐隱不一致——而这类 bug 恰恰是最难复现的。

### 新增 Skill

Skill 是最轻量的扩展方式。普通 advisory skill 只需要一个带 frontmatter 的 `SKILL.md`；engine-run skill 还需要声明 MCP server、工具和输出模板。建议规则：

- trigger 要具体，避免普通 prompt 误命中。
- deterministic 能力放进 MCP sidecar，模型判断放进 skill body。
- 输出模板保持稳定，便于测试。
- 需要持久化知识时，复用 `iota-memory-taxonomy`，不要在 skill 中发明另一套 memory 分类。

为什么 skill roots 同时支持 `~/.i6/skills` 和 workspace `.iota/skills`？用户级 skill 装个人习惯，workspace skill 装项目约定。两者都要有，但优先级和触发条件必须讲得清。

## 测试与工程约束

本仓库测试有一条明确约束：Rust 单元测试必须放在独立 `*_tests.rs` 文件中，源文件只用：

```rust
#[cfg(test)]
#[path = "module_tests.rs"]
mod tests;
```

测试文件使用 `use crate::...` 绝对路径，不使用 `use super::*`，测试函数直接放在文件顶层。

为什么这样做？因为这个项目模块多、边界多。拆出独立测试文件后，生产代码更短，测试命名更稳定，AI coding 工具改实现时也能第一时间定位到对应测试。

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

为什么保留历史记录？因为很多实现决策都是从实验和计划里长出来的，比如 daemon-first desktop、token usage 归一化、Kanban shadow DB。它们不是最新的命令手册，却能解释当前代码「为什么长成这个样子」。

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

本书的目标不是替代源码，而是给出一张可靠的地图。说到底，iota-sympantos 的核心思想就一句话：把 AI 编程助手后端当成可替换的执行器，把上下文、记忆、技能、工具、权限、事件和任务管理都沉到本地这层 Rust runtime 里统一治理。后端可以换，你资产化的这套本地能力不会跟着走。
