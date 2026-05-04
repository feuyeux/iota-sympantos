# iota

Cross-platform Rust CLI/TUI for sending prompts to five ACP backends using configuration from `~/.i6/nimia.yaml`.

Targets Windows, macOS, and Linux. All path handling, command spawning, and env var mapping is platform-aware.

## Architecture

The Rust code is split into extension-oriented modules, mirroring the larger `iota` separation while keeping this crate lightweight:

```text
src/
├── main.rs      # thin binary entrypoint
├── cli.rs       # command dispatch for default TUI, check/run/bench and daemon routing
├── tui.rs       # interactive prompt loop and warmed backend selection
├── engine.rs    # ACP runtime orchestration, warm backend pool, benchmarks
├── agent.rs     # local daemon for cross-CLI ACP client reuse and internal warm control plane
├── config.rs    # nimia.yaml schema, config loading, backend env rendering
└── acp.rs       # ACP JSON-RPC protocol driver + timing instrumentation
```

`cli` and `tui` are intentionally separate from `engine`: CLI/TUI own user interaction, while engine owns backend process orchestration. `agent` is the local daemon boundary for cross-CLI client reuse.

## Configuration

Backend configuration is read only from `~/.i6/nimia.yaml`. The runtime does not read external project config, network overlays, Redis, npm cache discovery, or generated backend data.

Each backend section commonly uses these fields:

- `enabled`: whether CLI/TUI may use this backend. TUI only warms enabled backends.
- `acp`: command and args used to start the backend ACP adapter.
- `update`: optional command and args used by check output for update/version probing.
- `home`: optional backend config directory, expanded with `~/` when supported.
- `model`: provider, model name, endpoint, and API key; iota renders this into backend process environment variables.

Example:

```yaml
codex:
  enabled: true
  acp:
    command: npx
    args: ["-y", "@zed-industries/codex-acp@latest"]
  model:
    provider: ninerouter
    name: gh/gpt-5.5
    base_url: http://localhost:20128/v1
    api_key: "<router-api-key>"
```

Use `iota check` to inspect the effective configuration for all five backend sections.

## Usage

```bash
cargo build --offline
```

Install the short command locally:

```bash
cargo install --path .
```

After install, use `iota` from your shell:

```powershell
# Windows
target\debug\iota.exe
target\debug\iota.exe check
target\debug\iota.exe check --daemon
target\debug\iota.exe run codex --timeout-ms 20000 "ping"
target\debug\iota.exe run --daemon --trace-timing codex --timeout-ms 20000 "ping"
```

```bash
# macOS / Linux
target/debug/iota
target/debug/iota check
target/debug/iota check --daemon
target/debug/iota run codex --timeout-ms 20000 "ping"
target/debug/iota run --daemon --trace-timing codex --timeout-ms 20000 "ping"
```

`iota` with no arguments enters the interactive TUI. The explicit `tui` command is no longer needed.

`check` prints one combined JSON structure: config path, daemon address, per-backend check status, command labels, update/version probe command, and configured model.

`daemon` and `warm` are no longer user-facing commands. Add `--daemon` or `-d` to supported commands when you want daemon routing. If the daemon is not running, iota starts it silently and continues. The first daemon-routed request starts/reuses the needed ACP client, so that request is also the warm path. Override the daemon address with `IOTA_DAEMON_ADDR=127.0.0.1:47662` if the default port is unavailable. `iota run` runs directly in-process unless `--daemon/-d` is present. `--trace-timing` prints route plus ACP phase timings to stderr as JSON. `bench-cold` measures one backend process per sample; `bench-warm` prewarms once in-process; adding `--daemon/-d` measures the daemon hot path.
