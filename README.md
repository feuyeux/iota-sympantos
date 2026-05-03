# iota

Cross-platform Rust CLI/TUI for sending prompts to five ACP backends using configuration from `~/.i6/nimia.yaml`.

Targets Windows, macOS, and Linux. All path handling, command spawning, and env var mapping is platform-aware.

## Architecture

The Rust code is split into extension-oriented modules, mirroring the larger `iota` separation while keeping this crate lightweight:

```text
src/
├── main.rs      # thin binary entrypoint
├── cli.rs       # command dispatch for check/info/acp/tui/daemon/warm/bench
├── tui.rs       # interactive prompt loop and warmed backend selection
├── engine.rs    # ACP runtime orchestration, warm backend pool, benchmarks
├── agent.rs     # local daemon for cross-CLI/TUI ACP client reuse + warm control plane
├── app.rs       # future app-facing read model/projection entrypoint
├── config.rs    # nimia.yaml schema, config loading, backend env rendering
└── acp.rs       # ACP JSON-RPC protocol driver + timing instrumentation
```

`cli` and `tui` are intentionally separate from `engine`: CLI/TUI own user interaction, while engine owns backend process orchestration. `app` and `agent` are explicit extension modules for future HTTP/WebSocket and application read-model work.

## Configuration

Backend configuration is read only from `~/.i6/nimia.yaml`. The runtime does not read external project config, network overlays, Redis, npm cache discovery, or generated backend data.

Each backend section uses only these fields:

- `enabled`: whether CLI/TUI may use this backend. TUI only warms enabled backends.
- `acp`: command and args used to install/update/start the backend ACP adapter.
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

See `nimia.yaml.template` for all five backend sections.

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
target\debug\iota.exe check
target\debug\iota.exe tui
target\debug\iota.exe daemon --warm
target\debug\iota.exe warm codex
target\debug\iota.exe acp codex --timeout-ms 20000 "ping"
target\debug\iota.exe acp --require-daemon --trace-timing codex --timeout-ms 20000 "ping"
```

```bash
# macOS / Linux
target/debug/iota check
target/debug/iota tui
target/debug/iota daemon --warm
target/debug/iota warm codex
target/debug/iota acp codex --timeout-ms 20000 "ping"
target/debug/iota acp --require-daemon --trace-timing codex --timeout-ms 20000 "ping"
```

`check` validates backend sections, enabled state, and `acp.command`. It does not update versions or rewrite backend paths.

`iota daemon` keeps one local `IotaEngine` alive on `127.0.0.1:47661`. Override that address with `IOTA_DAEMON_ADDR=127.0.0.1:47662` if the default port is unavailable. Start the daemon with `--warm` when optimizing repeated CLI calls, or run `iota warm [backend ...]` against an existing daemon; both prestart ACP clients for the chosen working directory without sending a model prompt. `iota acp` first tries that daemon and falls back to an in-process engine if it is not running. Use `--require-daemon` for benchmarks that must fail rather than silently fall back, and `--trace-timing` to print route plus ACP phase timings to stderr as JSON. TUI starts backend clients lazily on first use and reuses them, including their ACP session, until TUI exits. `bench-cold` measures one backend process per sample; `bench-warm` prewarms once and measures repeated prompts on the same warmed clients.
