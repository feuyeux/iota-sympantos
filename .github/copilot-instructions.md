# iota-sympantos — Copilot Instructions

## Build & Run

```powershell
cargo build                          # debug build
cargo build --release                # release build
cargo build --offline                # no network (all deps in Cargo.lock)

# Run commands
target\debug\iota-sympantos.exe check
target\debug\iota-sympantos.exe tui
target\debug\iota-sympantos.exe acp codex --timeout-ms 20000 --cwd D:\path\to\project "your prompt"
target\debug\iota-sympantos.exe acp --help
```

No test suite exists in this repository.

## Architecture

Two source files only:

- **`src/main.rs`** — CLI entrypoint, config loading (`~/.i6/nimia.yaml`), env translation per backend, TUI loop, and Hermes-specific home preparation.
- **`src/acp.rs`** — ACP protocol driver. Spawns a backend subprocess, speaks JSON-RPC 2.0 over newline-delimited stdin/stdout, streams `session/update` events, and handles `session/request_permission` interactively.

### ACP protocol flow

Every backend (claude-code, codex, gemini, hermes, opencode) is an external process launched via `npx` (or `hermes acp`). The protocol is:

```
initialize  →  session/new  →  session/prompt  →  stream session/update events  →  session/complete
```

There are two execution paths:
- **`run_acp_prompt`** — single-shot: start process, send one prompt, print output, kill.
- **`AcpClient`** — persistent: used by TUI to warm backends once and send multiple prompts reusing the same `sessionId`.

### Configuration

Config is read **only** from `~/.i6/nimia.yaml`. No project-level config, env-var discovery, or auto-detection is performed. The path is hardcoded via `dirs::home_dir()`.

## Key Conventions

### `env` key handling in `nimia.yaml`

The `env` map in each backend section has two semantics:

- **SCREAMING_SNAKE_CASE keys** (e.g., `ROUTER_API_KEY`) are passed literally as process env vars.
- **lowercase generic keys** (`api_key`, `base_url`, `model`, `provider`) are translated to backend-specific env vars in `backend_process_env()` in `main.rs`:
  - `claude-code`: `api_key` → `ANTHROPIC_API_KEY` + `ANTHROPIC_AUTH_TOKEN`; `base_url` → `ANTHROPIC_BASE_URL`; `model` → `ANTHROPIC_MODEL`
  - `codex`: `api_key` → `OPENAI_API_KEY` + `ROUTER_API_KEY`; `base_url` → `OPENAI_BASE_URL`
  - `gemini`: `api_key` → `GEMINI_API_KEY`; `model` → `GEMINI_MODEL`
  - `hermes`: `api_key`, `base_url`, `model`, `provider` — see below
  - `opencode`: `model` → `OPENCODE_MODEL`

### Hermes special handling

Hermes requires a `config.yaml` written to `HERMES_HOME` before startup. `prepare_hermes_home()` creates this file, infers the `provider` from `base_url` if not explicit, and sets provider-specific env vars (`MINIMAX_CN_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc.). If `HERMES_HOME` is not set, a temp dir is created and cleaned up after the run.

### Windows `npx` normalization

`normalize_command()` rewrites `"npx"` to `"npx.cmd"` on Windows. Always use `"npx"` in config/code; the normalization is applied at the call site in `main.rs`.

### Backend name aliases

`AcpBackend::parse()` accepts multiple aliases per backend. Authoritative names for `--backend`:
`claude-code`, `codex`, `gemini`, `hermes`, `opencode`. Aliases like `claude`, `claudecode`, `gemini-cli`, `open-code` also work.

### Adding a new backend

1. Add a variant to `AcpBackend` enum in `acp.rs`.
2. Implement `parse()`, `command()`, and `Display` arms.
3. Add to `ALL_BACKENDS`.
4. Add a field to `NimiaConfig` and `BackendConfig` in `main.rs`.
5. Add a case in `backend_config()`, `backend_home_env_key()`, and `backend_process_env()`.
6. Add a backend section to `nimia.yaml.template`.

### Debugging ACP wire messages

Pass `--show-native` to print every raw JSON-RPC line to stderr:
```powershell
target\debug\iota-sympantos.exe acp codex --show-native "ping"
```
