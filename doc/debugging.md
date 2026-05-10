# iota-sympantos debugging guide

## Prerequisites

1. **VS Code extension**: install [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb) (extension ID: `vadimcn.vscode-lldb`)
2. **Rust toolchain**: ensure `rustc` and `cargo` are installed at version ≥ 1.95.0
3. **Config file**: ensure `~/.i6/nimia.yaml` is correctly configured (including backend credentials)

## Debug configurations

| Configuration name | Description | Launch args |
|--------------------|-------------|-------------|
| Debug TUI (default mode) | Interactive TUI mode | No args |
| Debug Run (single execution) | Single prompt execution | `run <backend> <prompt>` |
| Debug Run with Daemon | Route via daemon | `run --daemon <backend> <prompt>` |
| Debug Check | Print backend JSON info | `check` |
| Debug Context MCP Sidecar | Start iota-context MCP | `context-mcp` |
| Debug Fun MCP Server | Start iota-fun MCP | `fun-mcp` |
| Debug Bench Cold | Cold-start benchmark | `bench-cold 3` |
| Debug Daemon (internal) | Start internal daemon process | `__daemon` |

## Usage

### 1. Set breakpoints

Click to the left of a line number in VS Code to set a breakpoint (red dot). Common debug entry points:

- `src/main.rs:16` — program entry
- `src/cli/mod.rs` — command dispatch
- `src/engine.rs` — ACP runtime orchestration
- `src/acp/mod.rs` — ACP protocol interaction
- `src/tui.rs` — TUI main loop

### 2. Start debugging

- Press `F5` or click the green triangle in the Run and Debug panel
- Select the relevant configuration from the dropdown
- The "Debug Run" configuration shows an input box to select the backend and enter a prompt

### 3. Debug controls

| Shortcut | Action |
|----------|--------|
| `F5` | Continue / start debugging |
| `F10` | Step over |
| `F11` | Step into |
| `Shift+F11` | Step out |
| `Shift+F5` | Stop debugging |
| `Cmd+Shift+F5` | Restart debugging |

### 4. Inspect variables

When paused, use these panels to inspect state:
- **Variables** — local variables in the current scope
- **Watch** — custom watch expressions
- **Call Stack** — the call stack
- **Debug Console** — execute LLDB expressions (e.g. `p variable_name`)

## Environment variables

The debug configurations set these defaults:

```
RUST_LOG=debug        # enable debug-level logging, output to stderr
RUST_BACKTRACE=1      # enable full stack traces
```

Logs are written to stderr by default and to daily rolling files under `~/.i6/logs/`. When `OTEL_ENABLED` is not disabled, logs are also sent via OTLP to `OTEL_EXPORTER_OTLP_ENDPOINT`, defaulting to `http://localhost:4317`. Application logs are not written to SQLite.

Use `IOTA_LOG` to override the tracing filter; if `IOTA_LOG` is not set, `RUST_LOG` is used. The default filter is `warn,iota_sympantos=info`.

Use `IOTA_LOG_FILE=off` to disable local file logging; `IOTA_LOG_DIR=/path/to/logs` to change the log directory; `IOTA_LOG_RETENTION_DAYS=14` to change daily file retention, or `IOTA_LOG_RETENTION_DAYS=off` to disable auto-cleanup.

Local CacheStore metrics can be viewed in Prometheus text format:

```bash
iota metrics --once
iota metrics --listen 127.0.0.1:47662
```

To filter logs by module, set `RUST_LOG`:

```
RUST_LOG=iota_sympantos::acp=debug,iota_sympantos::engine=debug
```

To adjust only iota module logs, use:

```
IOTA_LOG=iota_sympantos::acp=debug,iota_sympantos::engine=debug
```

## TUI debugging notes

TUI mode uses `crossterm` to take over the terminal; the terminal may be in raw mode when a breakpoint pauses execution. Recommendations:

1. Prefer setting breakpoints before TUI initialization (at the `cli/mod.rs` command dispatch stage)
2. When debugging TUI internals, use conditional breakpoints inside event handler functions
3. If the terminal state is corrupted after stopping the debugger, run `reset` in the terminal to recover

## Conditional breakpoints

Right-click a breakpoint → Edit Breakpoint, and add a condition expression:

```rust
// break only for a specific backend
backend == AcpBackend::Claude

// break only when the prompt contains specific text
prompt.contains("test")
```

## Logpoints

Right-click a line number → Add Logpoint, and enter a log template (execution does not pause):

```
Received event: {event:?}
```

## Common issues

### CodeLLDB fails to start

Confirm that the CodeLLDB extension is installed and that debugging permissions have been granted on macOS (System Preferences → Privacy & Security → Developer Tools).

### Breakpoints not hit

1. Confirm the build uses the debug profile (no `--release` in the `cargo build` inside `launch.json`)
2. Check whether the code has been optimized or inlined (debug mode defaults to `opt-level = 0`)
3. Breakpoints inside async functions may need to be placed on the line after the `.await`

### Terminal taken over by TUI

Use the "integrated" terminal when debugging TUI. If you need to view stdout output at the same time, consider using the "Debug Check" or "Debug Run" configuration instead.

### Debugging ACP subprocesses

ACP backends are external processes (launched via `npx`) and cannot be breakpointed directly. To debug ACP interactions, set breakpoints at:

- `src/acp/wire.rs` — reading and parsing JSON-RPC messages
- `src/acp/mod.rs` — sending requests and handling responses
- `src/acp/session.rs` — session parameter construction
