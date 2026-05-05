# Architecture: Layered Module Map

iota-sympantos 是一个 Rust CLI/TUI 编排器。核心运行路径是：用户入口进入表现层，表现层调用服务编排层，服务编排层组合 Context Fabric 与持久化存储，再通过协议适配层驱动 ACP/MCP 子进程。配置层为所有运行路径提供只读配置和后端环境变量渲染。

本文描述当前代码的实际分层和模块关系。调用链细节见 [code-call-chains.md](code-call-chains.md)，分层架构和链路图见 [`images/architecture-flow.svg`](images/architecture-flow.svg)，历史调用链图见 `images/code-call-chains.svg`。

## Layer Diagram

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ Entry Layer                                                                   │
│   main.rs                                                                     │
└──────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ Presentation Layer                                                            │
│   cli/mod.rs             tui.rs + tui/*                                       │
│   command routing        interactive terminal UI                              │
└──────────────────────────────────────────────────────────────────────────────┘
                    │                                  │
                    ▼                                  ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ Service Orchestration Layer                                                   │
│   engine.rs                                daemon/mod.rs + pool.rs + proto.rs  │
│   IotaEngine, client pool, turn lifecycle  daemon TCP warm/prompt plane        │
└──────────────────────────────────────────────────────────────────────────────┘
           │                         │                         │
           ▼                         ▼                         ▼
┌────────────────────────────┐ ┌────────────────────────────┐ ┌────────────────┐
│ Context Fabric Layer        │ │ Protocol Adapter Layer      │ │ Store Layer    │
│ context/mod.rs              │ │ acp/mod.rs + wire.rs        │ │ store/events   │
│ skill/mod.rs + runner.rs    │ │ acp/session.rs              │ │ store/memory   │
│ native/mod.rs               │ │ acp/permission.rs           │ │ store/approval │
│ context/server.rs           │ │ mcp/client.rs + router.rs   │ │ store/ledger   │
│ skill/fun_server.rs         │ │                             │ │                │
└────────────────────────────┘ └────────────────────────────┘ └────────────────┘
           │                         │                         ▲
           │                         ▼                         │
           │              ┌──────────────────────┐             │
           └─────────────▶│ External Process /   │◀────────────┘
                          │ Network Boundaries    │
                          │ ACP backend, daemon   │
                          │ MCP stdio, git, tools │
                          └──────────────────────┘

┌──────────────────────────────────────────────────────────────────────────────┐
│ Configuration and Shared Utility Layer                                        │
│   config.rs                      utils.rs                                     │
│   nimia.yaml schema/env/MCP      now_ts/summarize/lock recovery               │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Layers and Responsibilities

### Entry Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `main.rs` | Declares modules, starts Tokio runtime, calls `cli::run()` | `cli` |

`main.rs` contains no runtime policy. It is only the binary entrypoint.

### Presentation Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `cli/mod.rs` | Parses commands, dispatches `run/check/tui/bench/observability/context-mcp/fun-mcp/native-materialize/skill`, prints CLI output, autostarts daemon | `acp`, `daemon`, `config`, `engine`, `store::events`, `store::memory`, `native`, `skill`, `tui`, `context::server`, `skill::fun_server` |
| `tui.rs` | Owns ratatui terminal lifecycle, event loop, prompt queue, stream rendering, approval overlay, engine task spawning | `engine`, `acp`, `acp::permission`, `config`, `store::events`, `tui/*` |
| `tui/composer.rs` | Multi-line input editor: Unicode grapheme cursor, history search, kill/yank, word movement | none project-level |
| `tui/markdown.rs` | Converts Markdown into ratatui lines | none project-level |
| `tui/status_bar.rs` | Bottom status bar with backend/model/observability hints | `acp`, `tui/state` |
| `tui/theme.rs` | ratatui styles | none project-level |
| `tui/state.rs` | Conversation history and observability view models | none project-level |

Presentation code should not own ACP sessions directly. It reaches backend execution through `IotaEngine` or daemon client APIs.

### Service Orchestration Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `engine.rs` | Central orchestration facade. Owns ACP client pool keyed by `(backend, cwd)`, composes context, handles execution idempotency, runs skills, records events/memory/session data | `acp`, `config`, `context`, `store::events`, `store::memory`, `store::ledger`, `runtime_event`, `skill::runner`, `skill`, `utils` |
| `daemon/mod.rs` | Local daemon TCP server. Keeps engines alive across CLI invocations, accepts `prompt` and `warm` requests, handles graceful shutdown via `CancellationToken` | `acp`, `config`, `engine` |
| `daemon/pool.rs` | `EnginePool`: maintains one `IotaEngine` per cwd so ACP subprocess connections are reused across CLI invocations | `config`, `engine` |
| `daemon/proto.rs` | Wire types: `DaemonPromptRequest`, `DaemonPromptResponse`, `DaemonWarmRequest` | `acp::AcpPromptTiming`, `runtime_event` |

`engine.rs` is the boundary where product behavior is decided: replay/join execution, prepare handoff, decide whether a skill short-circuits ACP, and persist completed turns. It should coordinate stores and protocols, not embed their implementation details.

### Protocol Adapter Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `acp/mod.rs` | ACP backend enum, command parsing for `iota run`, ACP child-process lifecycle, JSON-RPC request/response, session lifecycle, prompt event loop, timings | `acp::permission`, `acp::session`, `acp::wire`, `mcp::router`, `runtime_event` |
| `acp/wire.rs` | Timeout line reads, ACP JSON message parsing, response id matching, error formatting | none project-level |
| `acp/session.rs` | Renders `session/new` params and per-backend `mcpServers` shape; `AcpSessionOptions` controls env shape and empty-server behavior. Gemini: no `name` field. Codex: env as object. Others: `name+type+env["K=V"]`. | `acp::AcpBackend` |
| `acp/permission.rs` | Handles ACP permission requests. Auto-approves `iota_*`/`mcp__iota-*` tools by returning `{"optionId":"allow"}` without any prompt. Non-iota tools route to TUI approval channel or terminal yes/no. Writes approval records. | `store::approval`, `runtime_event` |
| `mcp/client.rs` | Engine-side stdio MCP client. Starts MCP server process, initializes, calls tools | none project-level |
| `mcp/router.rs` | Intercepts ACP-side MCP tool-call messages and routes safe iota tools or denies external tools | `store::memory`, `skill` |
| `runtime_event.rs` | Normalizes ACP updates/errors/tool/usage/approval events into internal `RuntimeEvent` | none project-level |

Protocol adapters should not know CLI/TUI/daemon details. Their job is to translate process protocols into typed results and events.

### Context Fabric Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `context/mod.rs` | ContextEngine, budgeted context capsule composition, dialogue buffer, workspace `git status` summary, `<memory-tools>` injection for LLM-driven memory writes | `acp`, `config`, `store::memory`, `skill`, `utils` |
| `skill/mod.rs` | Distributed skill registry, YAML frontmatter parsing, trigger matching, backend compatibility, skill index rendering | `acp` |
| `skill/runner.rs` | Executes `execution.mode = mcp` skills through MCP tools and renders skill output | `mcp::client`, `runtime_event`, `skill` |
| `context/server.rs` | `iota-context` MCP stdio server exposing memory, skill, session, handoff tools/resources | `store::memory`, `store::ledger`, `skill`, `acp` |
| `skill/fun_server.rs` | `iota-fun` MCP stdio server. Runs small snippets in Rust/TypeScript/Python/Go/Java/C++/Zig | standard library process/filesystem |
| `native/mod.rs` | Projects memory and skills into backend-native files such as `AGENTS.md`, `MEMORY.md`, `GEMINI.md` | `acp`, `store::memory`, `skill` |
| `skill/cache.rs` | Pulls skills from local files or HTTP(S), sanitizes names, stores into `~/.i6/skills` | external network/filesystem |

Context Fabric supplies the background data and deterministic tools used by engine and ACP/MCP backends.

### Store Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `store/events.rs` | SQLite execution/event/observability store. Provides idempotency lock, fencing token, cache replay, metrics | `runtime_event`, `acp::AcpPromptTiming`, `utils` |
| `store/memory.rs` | SQLite memory store with taxonomy, recall buckets, FTS/LIKE search, deduplication, TTL | `utils` |
| `store/approval.rs` | Approval request/decision persistence and default operation classification | `utils` |
| `store/ledger.rs` | SQLite sessions, backend sessions, turns, handoffs, summaries | `utils` |

Store modules should remain below orchestration. They expose typed operations and do not call CLI/TUI/daemon/ACP clients.

### Configuration and Shared Utility Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `config.rs` | Reads only `~/.i6/nimia.yaml`, maps backend config to commands/env, expands home paths, renders MCP server config, per-backend context options (`context_engine_backend`) | `acp::AcpBackend`, `acp::session::AcpMcpServer`, `acp::session::AcpSessionOptions` |
| `utils.rs` | Shared low-level helpers: timestamp, summary truncation, poisoned mutex recovery | none project-level |

`config.rs` references `AcpBackend`, `AcpMcpServer`, and `AcpSessionOptions` to keep backend-specific config rendering type-safe. The `context_engine_backend` section provides per-backend control over MCP injection (`mcp_session_new`, `always_send_empty_mcp_servers`, `mcp_env_shape`, `override_home`).

## Primary Runtime Flows

### 全链路架构图

```text
┌─────────────────────────────────────────────────────────────────────────┐
│  用户                                                                    │
│  iota <prompt>  /  iota tui  /  iota run [backend] <prompt>            │
└───────────────────────────┬─────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  cli.rs  →  IotaEngine                                                  │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │ Context Fabric                                                    │   │
│  │  context.rs: memory buckets + skill index + <memory-tools>       │   │
│  │  skills.rs: trigger match → skill_runner.rs → mcp_client.rs      │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                    │                          │                          │
│                    │ engine-run skill          │ ACP backend prompt      │
│                    ▼                          ▼                          │
│  ┌──────────────────────┐     ┌────────────────────────────────────┐    │
│  │ MCP sidecar (skill)  │     │ ACP backend process                │    │
│  │ iota fun-mcp /       │     │ claude-code / codex / gemini /     │    │
│  │ iota context-mcp     │     │ hermes / opencode                  │    │
│  │ [stdio JSON-RPC]     │     │ [stdio JSON-RPC]                   │    │
│  └──────────────────────┘     └─────────────┬──────────────────────┘    │
│                                             │                           │
│                               ┌─────────────▼──────────────────────┐   │
│                               │ MCP sidecar (backend-started)       │   │
│                               │ iota context-mcp / iota fun-mcp     │   │
│                               │ [stdio JSON-RPC per mcpServers]     │   │
│                               └─────────────┬──────────────────────┘   │
│                                             │                           │
│                               ┌─────────────▼──────────────────────┐   │
│                               │ acp_permission.rs                   │   │
│                               │ session/request_permission          │   │
│                               │  iota_* → auto-approve (optionId)  │   │
│                               │  other  → TUI overlay / stdin y/n  │   │
│                               └─────────────┬──────────────────────┘   │
│                                             │                           │
│  ┌──────────────────────────────────────────▼─────────────────────┐    │
│  │ Store Layer                                                      │    │
│  │  memory.rs (SQLite FTS5)  event_store.rs  session_ledger.rs     │    │
│  └──────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

### 内存写回路径（LLM → MCP → SQLite）

```text
context.rs
  └─ compose_effective_prompt()
       └─ injects <memory-tools> block
            ┌────────────────────────────────────────────┐
            │  ACP backend sends prompt to LLM            │
            │  LLM decides to call iota_memory_write      │
            └───────────────────┬────────────────────────┘
                                │ session/request_permission
                                ▼
                     acp_permission.rs
                       tool_name from params.toolCall.title
                       is_iota_tool = true
                       send {optionId: "allow"}  ──────────────────────┐
                                                                        │ approved
                                ┌───────────────────────────────────────┘
                                ▼
                     context-mcp sidecar
                       tools/call iota_memory_write
                                │
                                ▼
                     memory.rs → SQLite
```

### Skill 并行执行路径（engine-run MCP skill）

```text
IotaEngine::prompt_in_cwd_timed()
  └─ SkillRegistry::match_skill()   (trigger 匹配)
       └─ skill_runner::run_engine_skill()
            ├─ parallel: true
            │    └─ mcp_client::call_stdio_batch()
            │         ├─ [child process] iota fun-mcp
            │         │    ├─ fun.cpp   → clang++/g++ → binary
            │         │    ├─ fun.rust  → rustc → binary
            │         │    ├─ fun.zig   → zig run
            │         │    ├─ fun.java  → javac → java
            │         │    ├─ fun.go    → go run
            │         │    ├─ fun.ts    → node -e
            │         │    └─ fun.python→ python3 -c
            │         └─ (~100ms wall time, 编译缓存 ~/.i6/fun-cache/)
            └─ render_template()
                 └─ SkillRunOutput → engine records Output event
```

### CLI Direct Prompt

```text
main -> cli -> config -> engine
engine -> event_store/session_ledger/memory/skills/context
engine -> acp -> ACP backend process
acp -> runtime_event/acp_permission/mcp_router as events arrive
engine -> stores for output, timing, memory, handoff
```

### CLI Daemon Prompt

```text
main -> cli
cli -> spawn current_exe __daemon if needed     [process boundary]
cli -> daemon::send_prompt over TCP             [IPC boundary]
daemon -> EnginePool::engine_for(cwd) -> same direct prompt chain
```

### TUI Prompt

```text
main -> cli -> tui
TUI event loop -> composer/state/render modules
TUI -> spawned engine task -> same direct prompt chain
ACP stream chunks -> in-process mpsc -> TUI rendering
ACP permission request -> acp::permission -> TUI approval channel
```

### Engine-Run MCP Skill

```text
engine -> skill match
engine -> skill::runner -> mcp::client
mcp::client -> iota fun-mcp/context-mcp child process   [stdio MCP boundary]
context/server -> store::memory/store::ledger/skill
skill/fun_server -> compiler/interpreter child processes [process boundary]
engine records ToolCall/ToolResult/Output events
```

### Backend-Started MCP Sidecar

```text
engine -> config::context_mcp_servers()
engine -> acp::session::session_new_params_with_options()
acp -> ACP backend process
ACP backend -> starts iota context-mcp / fun-mcp          [backend-controlled process boundary]
MCP tool/resource calls -> sidecar stdio JSON-RPC
```

### LLM-Driven Memory Write via MCP

```text
context.rs injects <memory-tools> block into prompt
  -> ACP backend sends prompt to LLM
  -> LLM decides to call mcp__iota-context__iota_memory_write
  -> ACP backend sends session/request_permission
       params.toolCall.title = "mcp__iota-context__iota_memory_write"
       params.options = [{optionId: "allow"}, {optionId: "allow_always"}, ...]
  -> acp_permission::answer_permission_request()
       tool name extracted from params.toolCall.title
       is_iota_tool = true (starts with "mcp__iota-")
       send_response({optionId: "allow"})  [no user prompt]
  -> ACP backend calls context-mcp sidecar tool
  -> context_mcp::call_tool("iota_memory_write")
  -> MemoryStore::insert()
  -> SQLite memory persisted
```

Alternative pre-authorization path (settings.json):

```text
~/.claude/settings.json
  permissions.allow: ["mcp__iota-context__*"]
  -> claude-code never sends session/request_permission for matching tools
  -> MCP tool call proceeds immediately
```

### Per-Backend MCP Server Rendering

```text
acp::session::render_mcp_server(backend, server, env_shape)
  ├── Gemini        -> {type:"stdio", command, args, env:["K=V",...]}    (no "name" field)
  ├── ClaudeCode/Hermes -> {name, type:"stdio", command, args, env:["K=V",...]}   (default)
  └── env_shape=Object  -> env rendered as {K:V,...} instead of ["K=V",...]   (configurable)
```

## Module Relationship Rules

Allowed dependency direction:

```text
entry -> presentation -> service -> protocol
presentation -> service -> context
service -> context, protocol, store
context -> protocol client where needed, store data types
protocol -> runtime_event, approval policy, safe internal tool routing
store -> runtime_event or plain data models only
config/utils -> lower-level shared support
```

Important constraints:

- `engine.rs` may coordinate all domains, but detailed SQL, ACP wire parsing, MCP JSON-RPC, and TUI rendering stay in their modules.
- `acp/mod.rs` must not depend on `engine.rs`, `daemon/`, `cli/`, or `tui.rs`.
- Store modules must not call UI, daemon, ACP client, or MCP client code.
- TUI submodules should stay under `src/tui/`; top-level `tui.rs` remains runtime composition and terminal lifecycle.
- External process and network boundaries must be explicit in docs and diagrams.

## External Boundaries

| Boundary | Module | Mechanism | Purpose |
|---|---|---|---|
| Daemon autostart | `cli/mod.rs` | `std::process::Command current_exe __daemon` | Hidden long-lived local service |
| Daemon request | `daemon/mod.rs` | TCP JSON line on `127.0.0.1:47661` | Prompt/warm routing |
| ACP backend | `acp/mod.rs` | child process stdio JSON-RPC | Claude Code, Codex, Gemini, Hermes, OpenCode |
| MCP sidecar via ACP | `acp/session.rs` + backend | `mcpServers` launched by backend | `iota-context`, `iota-fun` tools/resources |
| MCP sidecar via skill | `mcp/client.rs` | child process stdio MCP | engine-run deterministic skills |
| Workspace state | `context/mod.rs` | `git status --short` child process | context capsule workspace summary |
| Function tools | `skill/fun_server.rs` | compiler/interpreter child processes | Run small code snippets |
| Skill pull | `skill/cache.rs` | local filesystem or HTTP(S) | Load external skills |

## Extension Points

| Extension | Target Modules | Pattern |
|---|---|---|
| New ACP backend | `acp.rs`, `config.rs`, config template | Add enum variant, aliases, command/env mapping, `ALL_BACKENDS` entry |
| New CLI command | `cli.rs` | Add match arm and handler; reuse service/context/store modules |
| New TUI widget | `src/tui/*`, `tui.rs` | Keep display state in submodule, compose in `tui.rs` |
| New persistent fact | Store module + `engine.rs` write site | Store owns schema/query; engine decides when to persist |
| New MCP tool | `context_mcp.rs` or `fun_mcp.rs` | Add tool descriptor and `tools/call` handler |
| New engine-run skill behavior | `skills.rs`, `skill_runner.rs` | Extend skill metadata and runner while keeping ACP path unchanged |
| Native projection target | `native_materializer.rs` | Add backend path/render branch |

## Future Directory Split

When top-level modules become too large, split by architectural domain rather than by helper type:

```text
src/
  ui/
    cli.rs
    tui.rs
    tui/...
  service/
    engine.rs
    agent.rs
  protocol/
    acp.rs
    acp_wire.rs
    acp_session.rs
    acp_permission.rs
    mcp_client.rs
    mcp_router.rs
  context/
    capsule.rs
    skills.rs
    skill_runner.rs
    context_mcp.rs
    fun_mcp.rs
    materialize.rs
  store/
    events.rs
    memory.rs
    approvals.rs
    sessions.rs
  config.rs
  utils.rs
```

The split should be mechanical first: move code, add `pub use` compatibility re-exports, and avoid changing runtime semantics in the same patch.
