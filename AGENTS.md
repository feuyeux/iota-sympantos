# AGENTS.md

## Project Overview

iota-sympantos is a lightweight Rust CLI that orchestrates multiple AI coding assistant backends through the ACP (Agent Control Protocol). It provides both single-shot and interactive TUI modes for sending prompts to backends like Claude Code, Codex, Gemini CLI, Hermes, and OpenCode.


## Codex Operating Identity

This repository is maintained by Codex as the coding agent in this workspace. When acting on tasks here, operate as Codex: make concrete code/doc changes, run local verification, preserve unrelated user changes, and keep ACP/backend details grounded in the current Rust code.

Do not describe yourself as one of the five runtime backends. Claude Code, Codex, Gemini CLI, Hermes, and OpenCode are target backends orchestrated by iota-sympantos; the working agent editing this repository is Codex.

## Cross-Platform Requirement

**All code, configuration, and path handling must work on Windows, macOS, and Linux.** This is a hard constraint for every change:

- Use `dirs::home_dir()` for home directory resolution; never hardcode `~`, `%USERPROFILE%`, or `$HOME` in runtime code.
- `normalize_command()` rewrites `"npx"` to `"npx.cmd"` on Windows. Always use `"npx"` in config/code.
- Path separators: use `Path`/`PathBuf` for filesystem operations, never string-concatenate with `\` or `/`.
- Backend home directories differ per OS (e.g., Hermes uses `~/AppData/Local/hermes` on Windows, `~/.hermes` on Unix). Never assume a fixed path — let each backend resolve its own default home.
- Process spawning: `Stdio::piped()` and `kill_on_drop(true)` work cross-platform via tokio. No platform-specific process management.
- Config paths in `nimia.yaml` use `~/` prefix which is expanded by `expand_home_path()` at runtime. Do not use Windows-only or Unix-only path formats in the template.
- Test manually on Windows (primary dev platform) before submitting; CI covers Linux.

## Workspace Structure

```text
iota-sympantos/
├── src/
│   ├── main.rs          # thin binary entrypoint
│   ├── cli.rs           # command dispatch for default TUI, check/run/bench, daemon routing
│   ├── tui.rs           # interactive loop over lazily warmed ACP clients
│   ├── engine.rs        # ACP runtime orchestration, warm pool, benchmarks
│   ├── agent.rs         # local daemon for cross-CLI ACP client reuse + internal warm control plane
│   ├── app.rs           # future app-facing read model/projection entrypoint
│   ├── config.rs        # nimia.yaml schema, config loading, env translation
│   └── acp.rs           # ACP JSON-RPC 2.0 protocol driver + timing instrumentation
├── doc/
│   └── acp-runtime.md   # runtime process model, daemon control plane, benchmarks
├── Cargo.toml           # Rust 2024 edition, tokio async runtime
└── ~/.i6/nimia.yaml     # User config resolved through dirs::home_dir()
```

## Source Of Truth

Use current code first — runtime responsibilities are split across `cli`, `tui`, `engine`, `agent`, `config`, and `acp` modules. Then refer to `~/.i6/nimia.yaml` for runtime configuration semantics.

If code and this document diverge, prefer the current code and update this file to match.

## Architecture

### ACP Protocol Flow

Every backend is an external process launched via `npx` (or `hermes acp`). The JSON-RPC 2.0 protocol over newline-delimited stdin/stdout follows:

```
initialize → session/new → session/prompt → stream session/update events → session/complete
```

Two execution paths exist:

- **`IotaEngine::prompt_in_cwd`** — runtime path: lazily starts one ACP client per backend+cwd and reuses it until engine shutdown. `iota run` uses an in-process engine by default and does not probe the daemon. Adding `--daemon` or `-d` routes through the daemon, silently starts it if needed, and the first request warms the required backend client.
- **`AcpClient`** — persistent: used by `IotaEngine` to keep backend subprocesses alive and reuse ACP `sessionId` for repeated prompts in the same cwd.

### Daemon & Warm Control Plane

The daemon is an internal helper started automatically by `--daemon` / `-d`. It listens on `127.0.0.1:47661` (override with `IOTA_DAEMON_ADDR`) and accepts two JSON request types over TCP:

- **Prompt request**: `{"backend", "cwd", "prompt", "timeout_ms", "trace_timing"}` — dispatches through `IotaEngine::prompt_in_cwd_timed`, returns `{"ok", "text", "timing"}`.
- **Warm request**: `{"request_type": "warm", "cwd", "backends"}` — starts ACP clients without sending a model prompt, returns `{"ok", "warmed"}`.

There are no user-facing `daemon` or `warm` commands. Warm requests remain an internal control-plane message; `check --daemon` can prewarm enabled backends, and `run --daemon` warms the selected backend as part of the prompt request.

### ACP Timing Instrumentation

`AcpPromptTiming` tracks per-prompt phase latencies: `process_spawn_ms`, `init_ms`, `session_new_ms`, `prompt_ms`, `total_ms`, plus boolean flags `client_started`, `process_spawned`, `session_reused`. Use `--trace-timing` on `iota run` to emit this as JSON to stderr. Use `--daemon` / `-d` to route through the daemon during benchmarks.

### Backend Adapters

| Backend | Command | Aliases |
|---|---|---|
| Claude Code | `npx` | `claude`, `claudecode` |
| Codex | `npx` | `codex` |
| Gemini CLI | `npx` | `gemini`, `gemini-cli` |
| Hermes Agent | `hermes acp` | `hermes` |
| OpenCode | `npx` | `opencode`, `open-code` |

All backends are ACP-only. Backend name resolution is handled by `AcpBackend::parse()` in `acp.rs`.

### Configuration

Config is read **only** from `~/.i6/nimia.yaml`. No project-level config, env-var discovery, or auto-detection is performed.

#### `model` key handling in `nimia.yaml`

`nimia.yaml` does not expose a generic `env` section anymore. Each backend declares model and credential data under `model`:

```yaml
model:
  provider: minimax-cn
  name: MiniMax-M2.7
  base_url: https://api.minimaxi.com/anthropic
  api_key: <api-key>
```

At runtime, `backend_process_env()` renders this model config into the process environment expected by each backend:

- `claude-code`: `api_key` -> `ANTHROPIC_API_KEY` + `ANTHROPIC_AUTH_TOKEN`; `base_url` -> `ANTHROPIC_BASE_URL`; `name` -> `ANTHROPIC_MODEL`
- `codex`: `api_key` -> `OPENAI_API_KEY` + `ROUTER_API_KEY`; `base_url` -> `OPENAI_BASE_URL`; `name` -> `OPENAI_MODEL`
- `gemini`: `api_key` -> `GEMINI_API_KEY`; `name` -> `GEMINI_MODEL`
- `hermes`: `api_key`, `base_url`, `name`, `provider` -> provider-native env vars; see Hermes special handling below
- `opencode`: `name` -> `OPENCODE_MODEL`

### Hermes Special Handling

Hermes uses its own default `HERMES_HOME` (typically `~/AppData/Local/hermes` on Windows, `~/.hermes` on Unix) which contains a full `config.yaml`, `.env`, state database, skills, and logs. **Do not override `HERMES_HOME`** — Hermes requires the complete configuration and state tree in its home directory; pointing it to a bare directory breaks initialization.

Instead, nimia.yaml's `api_key`, `base_url`, `model`, and `provider` for Hermes are translated directly into provider-native environment variables that Hermes reads via `os.getenv()`:

- `provider` -> `HERMES_INFERENCE_PROVIDER`
- `name` -> `HERMES_MODEL`
- `api_key` + `base_url` -> provider-specific env vars resolved by `render_hermes_provider_env()`:
  - `minimax-cn`: `MINIMAX_CN_API_KEY`, `MINIMAX_CN_BASE_URL`
  - `minimax`: `MINIMAX_API_KEY`, `MINIMAX_BASE_URL`
  - `anthropic`: `ANTHROPIC_API_KEY`, `ANTHROPIC_TOKEN`, `ANTHROPIC_BASE_URL`
  - fallback: `OPENAI_API_KEY`, `OPENAI_BASE_URL`

The `home` field in nimia.yaml's hermes section is intentionally **ignored** (not mapped to `HERMES_HOME`). Hermes reads credentials from process environment variables and its own `.env` file; no `.env` or `config.yaml` is written by iota-sympantos.

Note: Hermes's `load_hermes_dotenv(override=True)` means its `.env` values take precedence over process env vars. If the API key in `~/.hermes/.env` differs from nimia.yaml, the `.env` value wins.

### Windows `npx` Normalization

`normalize_command()` rewrites `"npx"` to `"npx.cmd"` on Windows. Always use `"npx"` in config/code; normalization is applied in `config.rs` before backend process launch.

## Build & Run

```bash
cargo build                          # debug build (all platforms)
cargo build --release                # release build
cargo build --offline                # no network (all deps in Cargo.lock)
```

```powershell
# Windows
target\debug\iota.exe
target\debug\iota.exe check
target\debug\iota.exe check --daemon
target\debug\iota.exe run codex --timeout-ms 20000 "your prompt"
target\debug\iota.exe run --daemon --trace-timing claude-code --timeout-ms 30000 "say hello"
```

```bash
# macOS / Linux
target/debug/iota
target/debug/iota check
target/debug/iota check --daemon
target/debug/iota run codex --timeout-ms 20000 "your prompt"
target/debug/iota run --daemon --trace-timing claude-code --timeout-ms 30000 "say hello"
```

If the default daemon port `47661` is unavailable, set `IOTA_DAEMON_ADDR` before daemon-routed commands:

```powershell
$env:IOTA_DAEMON_ADDR = '127.0.0.1:50100'
```

No formal test suite exists in this repository. Use `cargo build`, `iota check`, `iota run --help`, and focused direct/daemon manual runs.

## Development Workflow

1. Make changes in the module that owns the behavior: `cli.rs` for command dispatch, `tui.rs` for interactive UI, `engine.rs` for process orchestration, `agent.rs` for daemon/internal warm control plane, `config.rs` for config/env translation, and `acp.rs` for wire protocol/timing.
2. `cargo build` to verify compilation.
3. Test manually via `target\debug\iota.exe run <backend> "ping"`.
4. Use `--show-native` to debug ACP wire messages.
5. Use `--trace-timing` to verify daemon hot path and phase latencies.

## Codex Tooling Notes

- `apply_patch` is a FREEFORM tool. Do not call it with JSON such as `{ "input": "..." }`.
- When using `apply_patch`, the tool message body must be the raw unified diff text, beginning with `*** Begin Patch` and ending with `*** End Patch`.
- If the environment or tool bridge keeps wrapping `apply_patch` as JSON and patching fails repeatedly, stop after one failed retry. Use a scoped fallback edit method only for the requested file, then verify with a read command and `git diff`.
- Do not leave the turn stuck in repeated tool-call attempts; preserve progress and report the fallback clearly.


## Adding a New Backend

1. Add a variant to `AcpBackend` enum in `acp.rs`.
2. Implement `parse()`, `command()`, and `Display` arms.
3. Add to `ALL_BACKENDS`.
4. Add a field to `NimiaConfig` and `BackendConfig` in `config.rs`.
5. Add a case in `backend_config()`, `backend_home_env_key()`, and `backend_process_env()`.
6. Add a backend section to `nimia.yaml.template`.

## Current Architecture Constraints

- **Cross-platform first**: every feature, path, command, and env var mapping must work on Windows, macOS, and Linux. See "Cross-Platform Requirement" above.
- All backend protocol events use ACP JSON-RPC 2.0 over stdin/stdout; do not add vendor SDK dependencies.
- Backend credentials are resolved through `nimia.yaml` env translation; do not add alternative credential discovery mechanisms.
- `session/request_permission` events are handled interactively in TUI mode.
- PowerShell scripts that invoke this tool must use single quotes or `${var}` braces around variables followed by colons to avoid scope resolution bugs.

## Review Focus

When reviewing code, pay extra attention to:

- ACP protocol message ordering and `sessionId` lifecycle
- Backend env var translation correctness in `backend_process_env()`
- Hermes provider-native env var mapping (not intermediate HERMES_API_KEY variables)
- Never override HERMES_HOME; Hermes requires its full default home directory
- Windows command normalization edge cases
- Error handling in JSON-RPC stream parsing
- Permission request handling in both single-shot and TUI modes
- Daemon prompt/internal warm request dispatch in `agent.rs`
- `AcpPromptTiming` accuracy — `Instant`-based measurements must not double-count phases
- `IOTA_DAEMON_ADDR` consistency: auto-started daemon and daemon-routed commands must read the same address

## Security

- Never commit API keys, tokens, passwords, or secrets.
- `nimia.yaml` contains backend credentials; it must not be committed to version control.
- Keep examples redacted in docs and debug output.
- `--show-native` may expose sensitive wire content; use only for local debugging.
