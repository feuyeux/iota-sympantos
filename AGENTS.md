# AGENTS.md

## Project Overview

iota-sympantos is a lightweight Rust CLI that orchestrates multiple AI coding assistant backends through the ACP (Agent Control Protocol). It provides both single-shot and interactive TUI modes for sending prompts to backends like Claude Code, Codex, Gemini CLI, Hermes, and OpenCode.

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
│   ├── main.rs          # CLI entrypoint, config loading, TUI loop, env translation
│   └── acp.rs           # ACP JSON-RPC 2.0 protocol driver
├── Cargo.toml           # Rust 2024 edition, tokio async runtime
└── ~/.i6/nimia.yaml     # User config (hardcoded path via dirs::home_dir())
```

## Source Of Truth

Use current code first — there are only two source files. Then refer to `~/.i6/nimia.yaml` for runtime configuration semantics.

If code and this document diverge, prefer the current code and update this file to match.

## Architecture

### ACP Protocol Flow

Every backend is an external process launched via `npx` (or `hermes acp`). The JSON-RPC 2.0 protocol over newline-delimited stdin/stdout follows:

```
initialize → session/new → session/prompt → stream session/update events → session/complete
```

Two execution paths exist:

- **`run_acp_prompt`** — single-shot: start process, send one prompt, print output, kill.
- **`AcpClient`** — persistent: used by TUI to warm backends once and send multiple prompts reusing the same `sessionId`.

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

#### `env` key handling in `nimia.yaml`

- **SCREAMING_SNAKE_CASE keys** (e.g., `ROUTER_API_KEY`) are passed literally as process env vars.
- **lowercase generic keys** (`api_key`, `base_url`, `model`, `provider`) are translated to backend-specific env vars in `backend_process_env()`:
  - `claude-code`: `api_key` → `ANTHROPIC_API_KEY` + `ANTHROPIC_AUTH_TOKEN`; `base_url` → `ANTHROPIC_BASE_URL`; `model` → `ANTHROPIC_MODEL`
  - `codex`: `api_key` → `OPENAI_API_KEY` + `ROUTER_API_KEY`; `base_url` → `OPENAI_BASE_URL`
  - `gemini`: `api_key` → `GEMINI_API_KEY`; `model` → `GEMINI_MODEL`
  - `hermes`: `api_key`, `base_url`, `model`, `provider` — see Hermes special handling below
  - `opencode`: `model` → `OPENCODE_MODEL`

### Hermes Special Handling

Hermes uses its own default `HERMES_HOME` (typically `~/AppData/Local/hermes` on Windows, `~/.hermes` on Unix) which contains a full `config.yaml`, `.env`, state database, skills, and logs. **Do not override `HERMES_HOME`** — Hermes requires the complete configuration and state tree in its home directory; pointing it to a bare directory breaks initialization.

Instead, nimia.yaml's `api_key`, `base_url`, `model`, and `provider` for Hermes are translated directly into provider-native environment variables that Hermes reads via `os.getenv()`:

- `provider` → `HERMES_INFERENCE_PROVIDER`
- `model` → `HERMES_MODEL`
- `api_key` + `base_url` → provider-specific env vars resolved by `render_hermes_provider_env()`:
  - `minimax-cn`: `MINIMAX_CN_API_KEY`, `MINIMAX_CN_BASE_URL`
  - `minimax`: `MINIMAX_API_KEY`, `MINIMAX_BASE_URL`
  - `anthropic`: `ANTHROPIC_API_KEY`, `ANTHROPIC_TOKEN`, `ANTHROPIC_BASE_URL`
  - fallback: `OPENAI_API_KEY`, `OPENAI_BASE_URL`

The `home` field in nimia.yaml's hermes section is intentionally **ignored** (not mapped to `HERMES_HOME`). Hermes reads credentials from process environment variables and its own `.env` file; no `.env` or `config.yaml` is written by iota-sympantos.

Note: Hermes's `load_hermes_dotenv(override=True)` means its `.env` values take precedence over process env vars. If the API key in `~/.hermes/.env` differs from nimia.yaml, the `.env` value wins.

### Windows `npx` Normalization

`normalize_command()` rewrites `"npx"` to `"npx.cmd"` on Windows. Always use `"npx"` in config/code; normalization is applied at the call site in `main.rs`.

## Build & Run

```bash
cargo build                          # debug build (all platforms)
cargo build --release                # release build
cargo build --offline                # no network (all deps in Cargo.lock)
```

```powershell
# Windows
target\debug\iota-sympantos.exe check
target\debug\iota-sympantos.exe acp codex --timeout-ms 20000 "your prompt"
```

```bash
# macOS / Linux
target/debug/iota-sympantos check
target/debug/iota-sympantos acp codex --timeout-ms 20000 "your prompt"
```

No test suite exists in this repository.

## Development Workflow

1. Make changes in `src/main.rs` or `src/acp.rs`.
2. `cargo build` to verify compilation.
3. Test manually via `target\debug\iota-sympantos.exe acp <backend> "ping"`.
4. Use `--show-native` to debug ACP wire messages.

## Adding a New Backend

1. Add a variant to `AcpBackend` enum in `acp.rs`.
2. Implement `parse()`, `command()`, and `Display` arms.
3. Add to `ALL_BACKENDS`.
4. Add a field to `NimiaConfig` and `BackendConfig` in `main.rs`.
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

## Security

- Never commit API keys, tokens, passwords, or secrets.
- `nimia.yaml` contains backend credentials; it must not be committed to version control.
- Keep examples redacted in docs and debug output.
- `--show-native` may expose sensitive wire content; use only for local debugging.
