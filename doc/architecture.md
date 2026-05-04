# Architecture: Layered Module Map

iota-sympantos 是一个 Rust CLI/TUI 编排器。核心运行路径是：用户入口进入表现层，表现层调用服务编排层，服务编排层组合 Context Fabric 与持久化存储，再通过协议适配层驱动 ACP/MCP 子进程。配置层为所有运行路径提供只读配置和后端环境变量渲染。

本文描述当前代码的实际分层和模块关系。调用链细节见 [code-call-chains.md](code-call-chains.md)，关系图见 `images/code-call-chains.svg`。

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
│   cli.rs                 tui.rs + tui/*                                       │
│   command routing        interactive terminal UI                              │
└──────────────────────────────────────────────────────────────────────────────┘
                    │                                  │
                    ▼                                  ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ Service Orchestration Layer                                                   │
│   engine.rs                                      agent.rs                      │
│   IotaEngine, client pool, turn lifecycle        daemon TCP warm/prompt plane │
└──────────────────────────────────────────────────────────────────────────────┘
           │                         │                         │
           ▼                         ▼                         ▼
┌────────────────────────────┐ ┌────────────────────────────┐ ┌────────────────┐
│ Context Fabric Layer        │ │ Protocol Adapter Layer      │ │ Store Layer    │
│ context.rs                  │ │ acp.rs/acp_wire.rs          │ │ event_store.rs │
│ skills.rs/skill_runner.rs   │ │ acp_session.rs              │ │ memory.rs      │
│ native_materializer.rs      │ │ acp_permission.rs           │ │ approval.rs    │
│ context_mcp.rs/fun_mcp.rs   │ │ mcp_client.rs/mcp_router.rs │ │ session_ledger │
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
| `cli.rs` | Parses commands, dispatches `run/check/tui/bench/observability/context-mcp/fun-mcp/native-materialize/skill`, prints CLI output, autostarts daemon | `acp`, `agent`, `config`, `engine`, `event_store`, `memory`, `native_materializer`, `skills`, `skill_registry_cache`, `tui`, `context_mcp`, `fun_mcp` |
| `tui.rs` | Owns ratatui terminal lifecycle, event loop, prompt queue, stream rendering, approval overlay, engine task spawning | `engine`, `acp`, `acp_permission`, `config`, `event_store`, `tui/*` |
| `tui/composer.rs` | Multi-line input editor: Unicode grapheme cursor, history search, kill/yank, word movement | none project-level |
| `tui/markdown.rs` | Converts Markdown into ratatui lines | none project-level |
| `tui/status_bar.rs` | Bottom status bar with backend/model/observability hints | `acp`, `tui/state` |
| `tui/theme.rs` | ratatui styles | none project-level |
| `tui/state.rs` | Conversation history and observability view models | none project-level |

Presentation code should not own ACP sessions directly. It reaches backend execution through `IotaEngine` or daemon client APIs.

### Service Orchestration Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `engine.rs` | Central orchestration facade. Owns ACP client pool keyed by `(backend, cwd)`, composes context, handles execution idempotency, runs skills, records events/memory/session data | `acp`, `config`, `context`, `event_store`, `memory`, `runtime_event`, `session_ledger`, `skill_runner`, `skills`, `utils` |
| `agent.rs` | Local daemon. Keeps engines alive across CLI invocations, accepts TCP JSON-line `prompt` and `warm` requests, handles graceful shutdown | `acp`, `config`, `engine` |

`engine.rs` is the boundary where product behavior is decided: replay/join execution, prepare handoff, decide whether a skill short-circuits ACP, and persist completed turns. It should coordinate stores and protocols, not embed their implementation details.

### Protocol Adapter Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `acp.rs` | ACP backend enum, command parsing for `iota run`, ACP child-process lifecycle, JSON-RPC request/response, session lifecycle, prompt event loop, timings | `acp_permission`, `acp_session`, `acp_wire`, `mcp_router`, `runtime_event` |
| `acp_wire.rs` | Timeout line reads, ACP JSON message parsing, response id matching, error formatting | none project-level |
| `acp_session.rs` | Renders `session/new` params and backend-specific `mcpServers` shape | `acp::AcpBackend` |
| `acp_permission.rs` | Handles ACP permission requests, bridges to TUI channel or terminal yes/no, writes approval records, sends ACP response | `approval`, `runtime_event` |
| `mcp_client.rs` | Engine-side stdio MCP client. Starts MCP server process, initializes, calls tools | none project-level |
| `mcp_router.rs` | Intercepts ACP-side MCP tool-call messages and routes safe iota tools or denies external tools | `memory`, `skills` |
| `runtime_event.rs` | Normalizes ACP updates/errors/tool/usage/approval events into internal `RuntimeEvent` | none project-level |

Protocol adapters should not know CLI/TUI/daemon details. Their job is to translate process protocols into typed results and events.

### Context Fabric Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `context.rs` | ContextEngine, budgeted context capsule composition, dialogue buffer, workspace `git status` summary | `acp`, `config`, `memory`, `skills`, `utils` |
| `skills.rs` | Distributed skill registry, YAML frontmatter parsing, trigger matching, backend compatibility, skill index rendering | `acp` |
| `skill_runner.rs` | Executes `execution.mode = mcp` skills through MCP tools and renders skill output | `mcp_client`, `runtime_event`, `skills` |
| `context_mcp.rs` | `iota-context` MCP stdio server exposing memory, skill, session, handoff tools/resources | `memory`, `session_ledger`, `skills`, `acp` |
| `fun_mcp.rs` | `iota-fun` MCP stdio server. Runs small snippets in Rust/TypeScript/Python/Go/Java/C++/Zig | standard library process/filesystem |
| `native_materializer.rs` | Projects memory and skills into backend-native files such as `AGENTS.md`, `MEMORY.md`, `GEMINI.md` | `acp`, `memory`, `skills` |
| `skill_registry_cache.rs` | Pulls skills from local files or HTTP(S), sanitizes names, stores into `~/.i6/skills` | external network/filesystem |

Context Fabric supplies the background data and deterministic tools used by engine and ACP/MCP backends.

### Store Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `event_store.rs` | SQLite execution/event/observability store. Provides idempotency lock, fencing token, cache replay, metrics | `runtime_event`, `acp::AcpPromptTiming`, `utils` |
| `memory.rs` | SQLite memory store with taxonomy, recall buckets, FTS/LIKE search, deduplication, TTL | `utils` |
| `approval.rs` | Approval request/decision persistence and default operation classification | `utils` |
| `session_ledger.rs` | SQLite sessions, backend sessions, turns, handoffs, summaries | `utils` |

Store modules should remain below orchestration. They expose typed operations and do not call CLI/TUI/daemon/ACP clients.

### Configuration and Shared Utility Layer

| Module | Responsibility | Depends On |
|---|---|---|
| `config.rs` | Reads only `~/.i6/nimia.yaml`, maps backend config to commands/env, expands home paths, renders MCP server config | `acp::AcpBackend`, `acp_session::AcpMcpServer` |
| `utils.rs` | Shared low-level helpers: timestamp, summary truncation, poisoned mutex recovery | none project-level |

`config.rs` is intentionally close to the bottom of the stack, but in the current flat layout it references `AcpBackend` and `AcpMcpServer` to keep backend-specific config rendering type-safe.

## Primary Runtime Flows

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
cli -> agent::send_prompt over TCP              [IPC boundary]
daemon agent -> engine pool -> same direct prompt chain
```

### TUI Prompt

```text
main -> cli -> tui
TUI event loop -> composer/state/render modules
TUI -> spawned engine task -> same direct prompt chain
ACP stream chunks -> in-process mpsc -> TUI rendering
ACP permission request -> acp_permission -> TUI approval channel
```

### Engine-Run MCP Skill

```text
engine -> skills match
engine -> skill_runner -> mcp_client
mcp_client -> iota fun-mcp/context-mcp child process    [stdio MCP boundary]
context_mcp -> memory/session_ledger/skills
fun_mcp -> compiler/interpreter child processes          [process boundary]
engine records ToolCall/ToolResult/Output events
```

### Backend-Started MCP Sidecar

```text
engine -> config::context_mcp_servers()
engine -> acp_session::session_new_params()
acp -> ACP backend process
ACP backend -> starts iota context-mcp / fun-mcp          [backend-controlled process boundary]
MCP tool/resource calls -> sidecar stdio JSON-RPC
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
- `acp.rs` must not depend on `engine.rs`, `agent.rs`, `cli.rs`, or `tui.rs`.
- Store modules must not call UI, daemon, ACP client, or MCP client code.
- TUI submodules should stay under `src/tui/`; top-level `tui.rs` remains runtime composition and terminal lifecycle.
- External process and network boundaries must be explicit in docs and diagrams.

## External Boundaries

| Boundary | Module | Mechanism | Purpose |
|---|---|---|---|
| Daemon autostart | `cli.rs` | `std::process::Command current_exe __daemon` | Hidden long-lived local service |
| Daemon request | `agent.rs` | TCP JSON line on `127.0.0.1:47661` | Prompt/warm routing |
| ACP backend | `acp.rs` | child process stdio JSON-RPC | Claude Code, Codex, Gemini, Hermes, OpenCode |
| MCP sidecar via ACP | `acp_session.rs` + backend | `mcpServers` launched by backend | `iota-context`, `iota-fun` tools/resources |
| MCP sidecar via skill | `mcp_client.rs` | child process stdio MCP | engine-run deterministic skills |
| Workspace state | `context.rs` | `git status --short` child process | context capsule workspace summary |
| Function tools | `fun_mcp.rs` | compiler/interpreter child processes | Run small code snippets |
| Skill pull | `skill_registry_cache.rs` | local filesystem or HTTP(S) | Load external skills |

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
