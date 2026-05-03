# iota-sympantos

Rust CLI/TUI for sending prompts to five ACP backends using configuration from `~/.i6/nimia.yaml`.

## Configuration

Backend configuration is read only from `~/.i6/nimia.yaml`. The runtime does not read external project config, network overlays, Redis, npm cache discovery, or generated backend data.

Each backend section uses only these fields:

- `enabled`: whether CLI/TUI may use this backend. TUI only warms enabled backends.
- `acp`: command and args used to install/update/start the backend ACP adapter.
- `home`: backend-specific home/config directory.
- `env`: environment variables passed to the backend process.

Example:

```yaml
codex:
  enabled: true
  acp:
    command: npx
    args: ["-y", "@zed-industries/codex-acp@latest"]
  home: ~/.codex-9router
  env:
    ROUTER_API_KEY: "sk_9router"
```

See `nimia.yaml.template` for all five backend sections.

## Usage

```powershell
cargo build --offline
target\debug\iota-sympantos.exe check
target\debug\iota-sympantos.exe tui
target\debug\iota-sympantos.exe acp codex --timeout-ms 20000 --cwd D:\coding\creative\iota-sympantos "ping"
```

`check` validates backend sections, enabled state, and `acp.command`. It does not update versions or rewrite backend paths.

CLI `acp` mode does not reuse backend processes. TUI mode warms enabled backends and reuses those ACP channels until TUI exits.