# AGENTS.md

## Language constraint

All code comments, commit messages, and artifacts in this repository **must use Chinese or English only**. Korean and other languages are prohibited.

---

## Project overview

iota-sympantos is a lightweight Rust CLI that orchestrates multiple AI coding assistant backends via the ACP (Agent Control Protocol) protocol. It supports both single-shot execution and interactive TUI modes, with five backends: Claude Code, Codex, Gemini CLI, Hermes, and OpenCode.

---

## Source structure

```
iota-sympantos/
├── src/
│   ├── main.rs              # binary entry point
│   ├── cli/
│   │   └── mod.rs           # command dispatch (run/check/tui/bench, etc.)
│   ├── tui.rs               # interactive TUI main loop
│   ├── tui/
│   │   ├── composer.rs      # multi-line input component (kill buffer/Ctrl+R/word motion)
│   │   ├── markdown.rs      # markdown rendering (pulldown-cmark)
│   │   ├── status_bar.rs    # bottom status bar (backend·model / key hints)
│   │   ├── theme.rs         # ratatui color theme (magenta primary)
│   │   └── state.rs         # TUI state
│   ├── engine.rs            # ACP runtime orchestration, client pool
│   ├── acp/
│   │   ├── mod.rs           # ACP JSON-RPC 2.0 protocol driver, AcpClient
│   │   ├── permission.rs    # permission request handling (iota tool auto-approve)
│   │   ├── session.rs       # session/new parameter rendering, mcpServers shape
│   │   └── wire.rs          # line read/parse, response id matching
│   ├── daemon/
│   │   ├── mod.rs           # internal daemon TCP server (127.0.0.1:47661)
│   │   ├── pool.rs          # EnginePool (reuse IotaEngine per cwd)
│   │   └── proto.rs         # DaemonPromptRequest/Response wire types
│   ├── config.rs            # nimia.yaml config parsing + per-backend context options
│   ├── runtime_event.rs     # unified event types (Output/ToolCall/Approval, etc.)
│   ├── store/
│   │   ├── mod.rs           # store layer entry point
│   │   ├── cache.rs         # CacheStore execution replay/dedupe
│   │   ├── embedding.rs     # Ollama API / local trigram embedding
│   │   ├── memory.rs        # MemoryStore (6-bucket taxonomy)
│   │   ├── approval.rs      # ApprovalStore + policy
│   │   └── ledger.rs        # SessionLedger + backend-switch handoff
│   ├── telemetry/
│   │   ├── mod.rs           # OpenTelemetry provider/exporter initialization
│   │   ├── console.rs       # stderr tracing layer
│   │   ├── logs.rs          # LogEvent attribute helpers
│   │   ├── metrics.rs       # OTel metrics instruments
│   │   └── spans.rs         # OTel span helpers
│   ├── context/
│   │   ├── mod.rs           # ContextEngine + capsule assembly + budget
│   │   └── server.rs        # iota-context MCP sidecar (stdio)
│   ├── skill/
│   │   ├── mod.rs           # SkillRegistry (distributed loading + trigger matching)
│   │   ├── runner.rs        # engine-run skill execution
│   │   ├── cache.rs         # skill pull/cache (HTTP or local)
│   │   └── fun_server.rs    # iota-fun 7-language MCP server (stdio)
│   ├── mcp/
│   │   ├── mod.rs           # MCP layer entry point
│   │   ├── client.rs        # engine-side MCP client
│   │   └── router.rs        # MCP tool call intercept router
│   ├── native/
│   │   └── mod.rs           # native file projection (optional)
│   └── utils.rs             # shared utilities
├── doc/
│   ├── architecture.md      # layered architecture and module responsibilities
│   ├── code-call-chains.md  # entry points, IPC, and call chains
│   └── observability.md     # OTel, Docker observability, and local storage boundaries
├── gefsi/
│   └── exp03-acp-runtime.md # ACP process model and benchmark validation report
├── Cargo.toml
└── ~/.i6/nimia.yaml         # sole configuration source
```

---

## ACP protocol flow

Each backend is an external process launched via `npx` (or `hermes acp`), using newline-delimited JSON-RPC 2.0 over stdin/stdout:

```
initialize → session/new → session/prompt → streaming session/update → session/complete
```

Execution paths:

- **Direct path**: `IotaEngine::prompt_in_cwd`, starts and reuses ACP clients on demand
- **Daemon path**: routed through the internal daemon via `IotaEngine` (`--daemon` / `-d`)

---

## Backend adapters

| Backend | Command | Aliases |
|---------|---------|---------|
| Claude Code | `npx` | `claude`, `claudecode` |
| Codex | `npx` | `codex` |
| Gemini CLI | `npx` | `gemini`, `gemini-cli` |
| Hermes Agent | `hermes acp` | `hermes` |
| OpenCode | `npx` | `opencode`, `open-code` |

---

## Configuration (nimia.yaml)

Configuration is read **only** from `~/.i6/nimia.yaml`. There is no project-level config or auto-discovery.

### `model` field mapping

```yaml
model:
  provider: minimax-cn
  name: MiniMax-M2.7
  base_url: https://api.minimaxi.com/anthropic
  api_key: <api-key>
```

At runtime, `backend_process_env_with_context()` maps the model config to the environment variables required by each backend:

- `claude-code`: api_key → `ANTHROPIC_API_KEY` + `ANTHROPIC_AUTH_TOKEN`; base_url → `ANTHROPIC_BASE_URL`; name → `ANTHROPIC_MODEL`
- `codex`: api_key → `OPENAI_API_KEY` + `ROUTER_API_KEY`; base_url → `OPENAI_BASE_URL`; name → `OPENAI_MODEL`
- `gemini`: api_key → `GEMINI_API_KEY`; name → `GEMINI_MODEL`
- `hermes`: api_key/base_url/name/provider → provider-native environment variables
- `opencode`: name → `OPENCODE_MODEL`

### Hermes special handling

Hermes uses its own default `HERMES_HOME` (`~/AppData/Local/hermes` on Windows, `~/.hermes` on Unix). **Do not override `HERMES_HOME`.**

The hermes config in nimia.yaml maps to provider-native environment variables that Hermes reads via `os.getenv()`:

- `provider` → `HERMES_INFERENCE_PROVIDER`
- `name` → `HERMES_MODEL`
- api_key + base_url → provider-specific variables resolved by `render_hermes_provider_env()`

---

## CLI commands

```bash
iota                     # enter TUI (default)
iota check [--daemon|-d] # print merged JSON backend info
iota run <backend> ...   # single-shot execution
iota run --daemon ...    # route via daemon, auto-started silently
iota bench-cold [N] [--daemon]
iota bench-warm [N] [--daemon]
iota logs <execution-id> # query Loki
iota trace <trace-id>    # query Jaeger
iota context-mcp         # start iota-context MCP sidecar (stdio)
iota fun-mcp             # start iota-fun 7-language MCP server (stdio)
iota native-materialize  # project memory/skills to native files
iota skill pull <source> [name]
iota __daemon            # internal daemon entry point
```

---

## TUI features (completed)

| Feature | File | Status |
|---------|------|--------|
| Multi-line input (Shift+Enter for newline) | `tui/composer.rs` | ✅ |
| Unicode grapheme cursor | `tui/composer.rs` | ✅ |
| Kill buffer (Ctrl+K/Ctrl+Y) | `tui/composer.rs` | ✅ |
| Ctrl+U/Ctrl+W word deletion | `tui/composer.rs` | ✅ |
| Alt+B/Alt+F word motion | `tui/composer.rs` | ✅ |
| Ctrl+R incremental history search | `tui/composer.rs` | ✅ |
| Markdown rendering | `tui/markdown.rs` | ✅ |
| Status bar (magenta primary, backend·model) | `tui/status_bar.rs` | ✅ |
| Run indicator (spinner + elapsed time) | `tui.rs` | ✅ |
| Ctrl+T fullscreen pager | `tui.rs` | ✅ |
| ? help overlay | `tui.rs` | ✅ |
| Double Ctrl+C quit confirmation | `tui.rs` | ✅ |
| Esc to interrupt running task | `tui.rs` | ✅ |
| Tab queue (buffer input while running) | `tui.rs` | ✅ |
| Overlay enum (None/Help/Pager/QuitConfirm) | `tui.rs` | ✅ |

### TUI current state

| Feature | File | Status |
|---------|------|--------|
| Panic hook terminal restore | `tui.rs` | ✅ |
| Error path terminal restore (RAII guard) | `tui.rs` | ✅ |
| stdout is-terminal check | `tui.rs` | ✅ |
| Engine turn background task execution | `tui.rs` | ✅ |
| Approval overlay | `tui.rs` / `acp/permission.rs` | ✅ |
| Frame rate limiter (~120 FPS) | `tui.rs` | ✅ |
| Streaming output incremental rendering | `tui.rs` / `engine.rs` / `acp/mod.rs` | ✅ |
| Mouse capture enabled | `tui.rs` | ✅ |

### TUI improvements pending

| Feature | Priority | Notes |
|---------|----------|-------|
| Mouse wheel scrolling | P2 | Mouse capture is enabled but scroll events do not form a complete scroll interaction |
| Keyboard enhancement flags | P2 | Shift+Enter still depends on terminal support in some terminals |
| Window title (OSC) | P3 | Terminal window title not yet set |
| External editor (Ctrl+X) | P3 | `$EDITOR` / `$VISUAL` integration not yet implemented |

---

## Context Fabric implementation status (vs. plan-0504 / plan-0504-plus)

| Phase | Description | File | Status |
|-------|-------------|------|--------|
| 1 | RuntimeEvent normalization | `runtime_event.rs` | ✅ |
| 1 | CacheStore SQLite replay/dedupe | `store/cache.rs` | ✅ |
| 1 | Execution idempotency + lock + fencing | `store/cache.rs` | ✅ |
| 2 | Context Capsule + budget | `context/mod.rs` | ✅ |
| 3 | MemoryStore (6-bucket taxonomy) | `store/memory.rs` | ✅ |
| 3 | 6-bucket recall queries | `store/memory.rs` | ✅ |
| 3 | DialogueBuffer | `context/mod.rs` | ✅ |
| 4 | SkillRegistry distributed loading | `skill/mod.rs` | ✅ |
| 4 | Skill trigger matching | `skill/mod.rs` | ✅ |
| 4b | Engine-run skill execution | `skill/runner.rs` | ✅ |
| 4b | 7-language fn engine (iota-fun MCP) | `skill/fun_server.rs` | ✅ |
| 4b | MCP client | `mcp/client.rs` | ✅ |
| 5a | MCP sidecar (iota-context) | `context/server.rs` | ✅ |
| 5a | ACP mcpServers injection | `acp/session.rs` | ✅ |
| 5b | MCP response channel / intercept | `mcp/router.rs` | ✅ |
| 6 | Approval normalization + persistence | `store/approval.rs` | ✅ |
| 7 | SessionLedger + handoff | `store/ledger.rs` | ✅ |
| 8 | Native materializer | `native/mod.rs` | ✅ |
| 9 | Config extension (context_engine) | `config.rs` | ✅ |
| 10 | OTel telemetry stack | `telemetry/*`, `docker/observability/*` | ✅ |

**All phases implemented.**

---

## Cross-platform requirements

**All code, configuration, and path handling must support Windows/macOS/Linux:**

- Use `dirs::home_dir()` to resolve the home directory; never hardcode `~`, `%USERPROFILE%`, or `$HOME`
- `normalize_command()` rewrites `"npx"` to `"npx.cmd"` on Windows
- Use `Path`/`PathBuf` for filesystem operations; never concatenate `\` or `/` as strings
- Backend home directories vary by OS (e.g. Hermes uses `~/AppData/Local/hermes` on Windows)
- Use `Stdio::piped()` and `kill_on_drop(true)` for process spawning (tokio cross-platform)
- Use `~/` prefix for paths in config templates; expanded at runtime by `expand_home_path()`
- Test manually on Windows (primary development platform) before committing; CI covers Linux

---

## Security requirements

- Never commit API keys, tokens, passwords, or any sensitive information
- `nimia.yaml` contains backend credentials; it must not be committed to version control
- Redact sensitive information in documentation and debug output
- `--show-native` may expose sensitive protocol content; use only for local debugging

---

## Adding a new backend

1. Add a variant to the `AcpBackend` enum in `acp/mod.rs`
2. Implement `parse()`, `command()`, and `Display` branches
3. Add to `ALL_BACKENDS`
4. Add fields to `NimiaConfig` and `BackendConfig` in `config.rs`
5. Add branches in `backend_config()`, `backend_home_env_key()`, and `backend_process_env_with_context()`
6. Add a backend config section to `nimia.yaml.template`
