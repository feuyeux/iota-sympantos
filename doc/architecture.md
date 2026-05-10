# iota-sympantos architecture overview

iota-sympantos is a lightweight Rust CLI/TUI orchestrator. It connects the user entry point, Context Fabric, persistent storage, ACP backends, and MCP sidecars into a unified runtime, allowing Claude Code, Codex, Gemini CLI, Hermes, and OpenCode to share the same configuration, memory, and skill layer.

This document follows the current source organization. For call chains see [code-call-chains.md](code-call-chains.md), for observability paths see [observability.md](observability.md), and for debugging see [debugging.md](debugging.md).

## Current source structure

```text
src/
├── main.rs                  # binary entry point; registers modules and calls cli::run()
├── cli/
│   └── mod.rs               # command dispatch, daemon autostart, bench, logs/trace, native, skill
├── tui.rs                   # ratatui main loop, terminal lifecycle, engine task, stream/approval channel
├── tui/
│   ├── composer.rs          # multi-line input, Unicode cursor, history search, kill/yank, word motion
│   ├── markdown.rs          # Markdown to ratatui Line rendering
│   ├── status_bar.rs        # bottom status bar
│   ├── theme.rs             # TUI theme
│   └── state.rs             # conversation, history, and observability display state
├── engine.rs                # IotaEngine orchestration, ACP client pool, context, skill, store writeback
├── acp/
│   ├── mod.rs               # ACP backend enum, subprocess lifecycle, JSON-RPC request/response, prompt loop
│   ├── permission.rs        # ACP permission requests, TUI approval channel, iota tool auto-approve
│   ├── session.rs           # session/new parameters and mcpServers rendering
│   └── wire.rs              # line read/parse, response id matching, ACP error formatting
├── daemon/
│   ├── mod.rs               # local TCP daemon, single-request single-response JSON line, warm/prompt
│   ├── pool.rs              # EnginePool, reuse IotaEngine per cwd
│   └── proto.rs             # daemon wire types
├── config.rs                # ~/.i6/nimia.yaml, effective config, backend env/command, context options
├── context/
│   ├── mod.rs               # ContextEngine, context capsule, DialogueBuffer, workspace summary
│   └── server.rs            # iota-context MCP stdio server
├── skill/
│   ├── mod.rs               # SkillRegistry, frontmatter, trigger, backend compatibility
│   ├── runner.rs            # engine-run skill for execution.mode=mcp
│   ├── cache.rs             # skill pull/cache
│   └── fun_server.rs        # iota-fun MCP stdio server, 7-language code snippet execution
├── mcp/
│   ├── mod.rs               # MCP module entry point
│   ├── client.rs            # engine-side stdio MCP client
│   └── router.rs            # ACP-side tool-call intercept and iota tool routing
├── native/
│   └── mod.rs               # memory/skill native file projection
├── store/
│   ├── mod.rs               # store layer entry point
│   ├── approval.rs          # approval event recording and default risk classification
│   ├── embedding.rs         # Ollama API / local trigram embedding
│   ├── cache.rs             # execution replay/dedupe cache
│   ├── ledger.rs            # session, backend session, turn, handoff
│   └── memory.rs            # memory taxonomy, FTS, vector/hybrid search, recall buckets
├── telemetry/               # OpenTelemetry providers, instruments, stderr/file layers
├── runtime_event.rs         # unified RuntimeEvent
└── utils.rs                 # timestamps, summarization, poison lock recovery
```

## Layered architecture

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

## Module responsibilities

### Entry

| Module | Responsibility | Downstream |
|---|---|---|
| `main.rs` | Registers top-level modules, starts the Tokio runtime, calls `cli::run()` | `cli` |

`main.rs` holds no business logic.

### Presentation

| Module | Responsibility | Main downstream |
|---|---|---|
| `cli/mod.rs` | Parses commands and dispatches `run/check/tui/bench/logs/trace/context-mcp/fun-mcp/native-materialize/skill/__daemon`; handles daemon autostart, OTel initialization, and CLI output | `config`, `engine`, `daemon`, `acp`, `native`, `skill`, `tui`, `telemetry` |
| `tui.rs` | Terminal lifecycle, event loop, prompt queue, background engine task, streaming output, approval overlay, pager/help/quit overlay | `engine`, `acp::permission`, `tui/*` |
| `tui/composer.rs` | Multi-line editing, history, search, word motion, and kill buffer | No project-level dependencies |
| `tui/markdown.rs` | Markdown rendering to ratatui text lines | No project-level dependencies |
| `tui/status_bar.rs` | backend/model/key-hints/observability status bar | `acp`, `tui::state` |
| `tui/theme.rs` | ratatui colors and styles | No project-level dependencies |
| `tui/state.rs` | Conversation history and observability display model | No project-level dependencies |

The presentation layer does not own ACP sessions directly; all backend execution goes through `IotaEngine` or the daemon client API.

### Service orchestration

| Module | Responsibility | Main downstream |
|---|---|---|
| `engine.rs` | Core orchestration facade. Maintains an ACP client pool keyed by `(backend, cwd)`; handles replay/join-running, session ledger, handoff, memory recall/write, skill short-circuit, context capsule, ACP invocation, CacheStore writeback, and OTel metrics/logs | `acp`, `config`, `context`, `skill`, `store`, `runtime_event`, `telemetry` |
| `daemon/mod.rs` | TCP daemon on `127.0.0.1:47661` (overridable via `IOTA_DAEMON_ADDR`); one JSON request/response per connection; 8-connection concurrency limit; 10 MiB request cap; graceful Ctrl+C shutdown | `engine`, `daemon::pool`, `daemon::proto` |
| `daemon/pool.rs` | `EnginePool` reuses `IotaEngine` per cwd, reusing ACP subprocesses and session/handoff state | `engine`, `config` |
| `daemon/proto.rs` | `DaemonPromptRequest`, `DaemonPromptResponse`, `DaemonWarmRequest` | `runtime_event`, `acp::AcpPromptTiming` |

`engine.rs` is the behavioral decision boundary; SQL, ACP wire, MCP JSON-RPC, and TUI rendering remain in their own modules.

### Protocol

| Module | Responsibility | Main downstream |
|---|---|---|
| `acp/mod.rs` | `AcpBackend`, default adapter commands, `parse_acp_args()`, ACP subprocess launch, `initialize/session/new/session/prompt`, streaming event reading and timing | `acp::permission`, `acp::session`, `acp::wire`, `mcp::router`, `runtime_event` |
| `acp/session.rs` | Generates `session/new` params; renders `mcpServers`; supports `always_send_empty_mcp_servers` and both `string_array/object` env shapes | `acp::AcpBackend` |
| `acp/wire.rs` | ACP stdout line timeout, JSON parsing, response id matching, error formatting | No project-level dependencies |
| `acp/permission.rs` | Handles `session/request_permission`; auto-approves `iota_*`, `mcp__iota-*`, or backend `tool_whitelist` matches; otherwise routes to TUI or stdin; records approval events | `store::approval`, `runtime_event` |
| `mcp/client.rs` | stdio MCP client used by engine-run skills; launches the server, initializes, and calls tools | No project-level dependencies |
| `mcp/router.rs` | Intercepts ACP-side `tools/call` / `mcp/tools/call` / `mcp/tool_call`; routes iota memory/skill/session/handoff/fun tools; rejects external tools | `store::memory`, `store::ledger`, `skill`, `skill::fun_server` |
| `runtime_event.rs` | Normalizes ACP update, complete, permission, usage, tool, and error events into `RuntimeEvent` | `acp::extract_text` |

The protocol layer only performs protocol translation and security routing; it does not depend on CLI/TUI/daemon/engine.

### Context Fabric

| Module | Responsibility | Main downstream |
|---|---|---|
| `context/mod.rs` | Assembles the `<iota-context>` capsule: session/model, memory tools prompt, memory buckets, dialogue, workspace `git status --short`, skill index, handoff | `config`, `store::memory`, `skill` |
| `context/server.rs` | `iota-context` MCP stdio server; exposes memory/search/write, skill/search/load, session_summary, handoff_publish/read, and resources | `store::memory`, `store::ledger`, `skill` |
| `skill/mod.rs` | Loads skills from workspace `skills/`, workspace `.iota/skills`, configured roots, and `~/.i6/skills`; parses YAML frontmatter; matches by backend and trigger | `acp::AcpBackend` |
| `skill/runner.rs` | Executes `execution.mode = mcp` skills; can call MCP tools sequentially or in parallel; renders templates | `mcp::client`, `runtime_event`, `skill` |
| `skill/cache.rs` | Pulls skills from a local path or HTTP(S) URL and writes them to `~/.i6/skills` | filesystem/network |
| `skill/fun_server.rs` | `iota-fun` MCP stdio server; runs `fun.python/typescript/rust/go/java/cpp/zig` | external interpreters/compilers |
| `native/mod.rs` | Projects memory/skills to backend-native files using `<!-- IOTA_START -->` / `<!-- IOTA_END -->` block replacement | `store::memory`, `skill`, `acp::AcpBackend` |

Context Fabric provides prompt background, deterministic tools, and native projection for backends that do not support MCP.

### Store

| Module | Responsibility | Default path |
|---|---|---|
| `store/cache.rs` | Execution replay/dedupe cache; request hash, running join, fencing token, output replay, 30-day cache retention | `~/.i6/context/events.sqlite` |
| `store/memory.rs` | Memory taxonomy, dedup, TTL, merge mode, recall buckets, FTS/LIKE, vector/hybrid search | `~/.i6/context/memory.sqlite` or `context_engine.memory_db` |
| `store/embedding.rs` | Embedding computation; uses Ollama `/api/embeddings` when configured via `context_engine.embedding`; falls back to a 128-dimension local trigram embedding on failure or if unconfigured | stored in `memory_embedding` table |
| `store/approval.rs` | Approval request/decision recording, risk dimension classification, default manual-review policy | `~/.i6/context/approvals.sqlite` |
| `store/ledger.rs` | iota session, backend session, turn, handoff | `~/.i6/context/sessions.sqlite` |

Store modules expose only typed operations; they do not call UI, daemon, ACP client, or MCP client.

### Telemetry

| Module | Responsibility |
|---|---|
| `telemetry/mod.rs` | Initializes OTel tracer/meter/logger providers; default OTLP endpoint is `http://localhost:4317`; installs stderr tracing layer and configurable local rolling file layer |
| `telemetry/metrics.rs` | Defines OTel instruments for execution/cache/token/latency |
| `telemetry/logs.rs` | Helper mappings from `LogEvent` to OTel attributes |
| `telemetry/spans.rs` | Execution/phase/tool/memory/approval span helpers |
| `telemetry/console.rs` | stderr fmt layer |

The Docker observability stack is in `docker/observability/` and includes OTel Collector, Jaeger, Prometheus, Loki, and Grafana.

### Configuration

| Module | Responsibility |
|---|---|
| `config.rs` | Sole reader of `~/.i6/nimia.yaml`; builds `EffectiveConfig`; expands `~/`; normalizes Windows `npx`; renders backend command/env; injects context MCP server; reads recall thresholds, embedding, skill roots, backend whitelist, and session options |
| `utils.rs` | `now_ts()`, `summarize()`, `lock_or_recover()` |

Configuration does not auto-discover project-level config files. `nimia.yaml.template` is the configuration template.

## Key execution paths

### CLI direct execution

```text
iota run [backend] [options] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
  -> config::read_config()
  -> IotaEngine::new_for_session_cwd()
  -> IotaEngine::prompt_in_cwd_timed()
       -> request hash replay / running join
       -> session ledger + handoff
       -> memory extraction / deterministic memory answer
       -> skill match and optional engine-run MCP skill
       -> memory recall + context capsule
       -> ensure ACP client
       -> ACP session/prompt
       -> event/timing/session/memory writeback
  -> stdout
```

### CLI via daemon

```text
iota run --daemon ...
  -> CLI connects 127.0.0.1:47661
  -> if failed: spawn current_exe __daemon, wait, retry
  -> daemon EnginePool::engine_for(cwd)
  -> same IotaEngine prompt path
  -> JSON-line DaemonPromptResponse
```

### TUI execution

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

### Context and memory writeback

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

## ACP backends

| Backend | Default command | Aliases | Notes |
|---|---|---|---|
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@latest` | `claude`, `claude-code`, `claudecode` | Config template pins to `0.32.0` |
| Codex | `npx -y @zed-industries/codex-acp@0.12.0` | `codex` | `normalized_acp_command()` appends Codex `-c` args |
| Gemini CLI | `npx -y @google/gemini-cli@latest --acp` | `gemini`, `gemini-cli` | Config template pins to `0.41.2` |
| Hermes | `hermes acp` | `hermes`, `hermes-agent` | Does not override `HERMES_HOME`; provider env generated by `render_hermes_provider_env()` |
| OpenCode | `npx -y opencode-ai@latest acp` | `opencode`, `open-code` | Config template pins to `1.14.40` |

On Windows, `normalize_command()` rewrites `npx` to `npx.cmd`.

## Configuration model

Configuration is read only from `~/.i6/nimia.yaml`. The top level contains five backend sections, `context_engine`, and `context_engine_backend`.

### Backend section

| Field | Meaning |
|---|---|
| `enabled` | Whether the backend participates in `check`, warm, and bench |
| `acp.command` / `acp.args` | ACP adapter launch command |
| `version_mapping` | Records adapter/bin versions for `check` output |
| `home` | Backend custom home; Codex/Hermes do not currently map a home env |
| `model` | provider/name/base_url/api_key |
| `tool_whitelist` | Auto-approve permission rules; supports simple wildcards |

### Model env mapping

| Backend | Mapping |
|---|---|
| Claude Code | `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, `ANTHROPIC_MODEL`, `ANTHROPIC_SMALL_FAST_MODEL`, `ANTHROPIC_DEFAULT_*_MODEL`, `API_TIMEOUT_MS`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` |
| Codex | `ROUTER_API_KEY`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`; also appends `-c model=...`, `model_provider`, provider base_url/env_key/wire_api |
| Gemini | `GEMINI_API_KEY`, `GEMINI_MODEL` |
| Hermes | `HERMES_INFERENCE_PROVIDER`, `HERMES_MODEL`, and provider-native key/base_url such as `MINIMAX_CN_API_KEY` |
| OpenCode | `OPENCODE_MODEL` |

### Context engine section

| Field | Meaning |
|---|---|
| `enabled` / `injection` | Controls context; only `injection=off` disables the prompt capsule; all other values enable injection |
| `memory_db` | Memory SQLite path |
| `skill_roots` | Additional skill roots; actual loading also includes workspace `skills/`, workspace `.iota/skills`, and `~/.i6/skills` |
| `budgets` | Character budgets for memory/skills/dialogue/workspace |
| `recall_thresholds` | Confidence thresholds for each of the six recall buckets |
| `episodic_compaction_keep` | Number of episodic entries to keep after compaction |
| `mcp` / `fun` | Launch commands for context/fun MCP servers |
| `embedding` | Ollama `/api/embeddings` config; falls back to local trigram embedding if unconfigured or on failure |

### Per-backend context options

| Field | Meaning |
|---|---|
| `mcp_session_new` | Whether to inject `mcpServers` in `session/new`; `try` is enabled by default for Claude Code and Codex only |
| `always_send_empty_mcp_servers` | Send an empty array even when no MCP servers are configured |
| `mcp_env_shape` | `string_array` or `object` |
| `override_home` | Whether to map the backend `home` to the corresponding env; Hermes template defaults to `false` |

`acp/session.rs` currently renders `{name,type,command,args,env}` for all backends; the env shape can be switched between string array and object via config. Codex sends the `mcpServers` field even when the server list is empty.

## Data model

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

The current implementation no longer writes all `RuntimeEvent` entries to a SQLite EventStore. `RuntimeEvent` is still used as the in-memory return value for engine/daemon/TUI and for `iota run --log-events` stderr output; persistent observability is handled by the OpenTelemetry backend.

### Observability signals

| Signal | Producer | Destination |
|---|---|---|
| Logs | `tracing::*` macros and OTel log bridge | stderr, daily files like `~/.i6/logs/iota.log.YYYY-MM-DD`, plus OTLP logs to `OTEL_EXPORTER_OTLP_ENDPOINT` |
| Traces | `tracing-opentelemetry` layer / OTel tracer provider | OTLP traces to Collector, then Jaeger in Docker stack |
| Metrics | `telemetry::metrics` OTel instruments | OTLP metrics to Collector, then Prometheus remote write in Docker stack |
| Local metrics exposition | `iota metrics` | Prometheus text format for CacheStore counters from `~/.i6/context/events.sqlite` |

There is no current `iota observability` command group. Local file logs are written under `~/.i6/logs/` by default and can be controlled with `IOTA_LOG_FILE` / `IOTA_LOG_DIR` / `IOTA_LOG_RETENTION_DAYS`; see [observability.md](observability.md).

### Memory taxonomy

| Type | Facet | Typical scope | Recall bucket |
|---|---|---|---|
| `semantic` | `identity` | `user` | identity |
| `semantic` | `preference` | `user` | preference |
| `semantic` | `strategic` | `project` | strategic |
| `semantic` | `domain` | `project` | domain |
| `procedural` | none | `project` | procedural |
| `episodic` | none | `session` / `project` | episodic |

Memory search supports `keyword`, `vector`, and `hybrid` modes. Vector data is written to the `memory_embedding` table; the local fallback uses a 128-dimension trigram hash projection. Note: the `MemoryStore` opened by the engine uses `context_engine.embedding`, but `context-mcp` and `mcp::router` currently open the default store via `MemoryStore::open()` and use the local fallback on the query side.

## External boundaries

| Boundary | Initiator | Target | Protocol/mechanism |
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

## Dependency rules

Allowed directions:

```text
entry -> presentation -> service -> protocol
presentation -> service -> context/store
service -> context + protocol + store + runtime_event
context -> store + skill + selected protocol client
protocol -> runtime_event + store approval/router only
store -> runtime_event or plain data models
config/utils -> shared support
```

Constraints:

- `acp/*` does not depend on `engine.rs`, `daemon/*`, `cli/*`, or `tui.rs`
- Store modules do not call UI, daemon, ACP client, or MCP client
- TUI subcomponents stay in `src/tui/`; top-level `tui.rs` is responsible only for composition, event loop, and terminal lifecycle
- External process, TCP, network, and SQLite boundaries are kept explicit in both documentation and implementation
- All path handling uses `Path`/`PathBuf`; home directory is resolved via `dirs::home_dir()`

## Extension points

| Goal | Where to modify | Pattern |
|---|---|---|
| New ACP backend | `src/acp/mod.rs`, `src/config.rs`, `nimia.yaml.template` | Add enum variant, aliases, default command, `ALL_BACKENDS`, backend config/env/home mapping |
| New CLI command | `src/cli/mod.rs` | Add match arm and handler, reuse service/context/store |
| New TUI component | `src/tui/*`, `src/tui.rs` | Push state and rendering into submodules; top level composes only |
| New RuntimeEvent | `runtime_event.rs`, relevant producers | Add event type; wire into `--log-events`, TUI/daemon return values, or OTel logs/metrics as needed |
| New memory capability | `store/memory.rs`, `store/embedding.rs`, `context/server.rs`, `mcp/router.rs` | Store owns schema/query; MCP/router exposes tools |
| New MCP tool | `context/server.rs` or `skill/fun_server.rs`, and `mcp/router.rs` if needed | Add descriptor, `tools/call` handler, and routing policy |
| New engine-run skill behavior | `skill/mod.rs`, `skill/runner.rs` | Extend metadata/runner; do not change the ACP prompt path |
| New native projection | `native/mod.rs`, `cli/mod.rs` | Add target path and render/apply branch |
