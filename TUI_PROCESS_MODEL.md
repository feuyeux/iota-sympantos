# iota-sympantos TUI Process Model

This note records the observed process layout for `iota-sympantos.exe tui` on Windows.

## Summary

TUI mode warms enabled ACP backends and reuses those ACP channels until the TUI exits.

In the observed run:

- Warm ACP backends: 5
- Main process: 1 `iota-sympantos.exe`
- Expected logical model: 1 main process + 5 warm ACP backend channels
- Actual Windows OS process count: 26

The OS process count is higher than 6 because `npx`, `cmd.exe`, `node.exe`, and backend-native binaries spawn wrapper and child processes.

## Main Process

| PID | PPID | Name | Command |
|---:|---:|---|---|
| 39248 | 6124 | `iota-sympantos.exe` | `D:\coding\creative\iota-sympantos\target\debug\iota-sympantos.exe tui` |

## Direct Children

| PID | PPID | Name | Backend | Command |
|---:|---:|---|---|---|
| 33032 | 39248 | `conhost.exe` | console | console host |
| 36156 | 39248 | `cmd.exe` | Gemini | `npx.cmd -y @google/gemini-cli@latest --acp` |
| 38348 | 39248 | `cmd.exe` | Codex | `npx.cmd -y @zed-industries/codex-acp@latest` |
| 38792 | 39248 | `cmd.exe` | Claude Code | `npx.cmd -y @agentclientprotocol/claude-agent-acp@latest` |
| 43280 | 39248 | `hermes.exe` | Hermes | `hermes acp` |
| 43952 | 39248 | `cmd.exe` | OpenCode | `npx.cmd -y opencode-ai@latest acp` |

## Backend Process Trees

| Backend | Observed process chain |
|---|---|
| Claude Code | `cmd.exe` 38792 -> `node.exe` 34144 -> `cmd.exe` 27584 -> `node.exe` 31728 -> `claude.exe` 40460 -> `conhost.exe` 15276 |
| Codex | `cmd.exe` 38348 -> `node.exe` 20224 -> `cmd.exe` 37196 -> `node.exe` 41284 -> `codex-acp.exe` 24024 |
| Gemini | `cmd.exe` 36156 -> `node.exe` 17300 -> `cmd.exe` 3964 -> `node.exe` 41372 -> `node.exe` 15648 |
| Hermes | `hermes.exe` 43280 -> `python.exe` 43308 -> `python.exe` 43324 |
| OpenCode | `cmd.exe` 43952 -> `node.exe` 43968 -> `cmd.exe` 25552 -> `node.exe` 43068 -> `opencode.exe` 2688 |

## Interpretation

The statement "1 main process + 5 backend processes = 6 processes" is accurate only at the logical ACP-channel level.

On Windows, each backend channel may expand into multiple OS processes:

- `npx` runs through `cmd.exe` and `node.exe`.
- JavaScript ACP adapters run under `node.exe`.
- Some adapters spawn backend-native binaries, such as `claude.exe`, `codex-acp.exe`, or `opencode.exe`.
- Hermes spawns Python processes.

Therefore, TUI mode should be described as:

> 1 `iota-sympantos` main process + up to 5 warm ACP backend channels. The actual OS process count depends on backend wrappers and platform behavior.

## Configuration Implication

The `enabled` property controls whether TUI creates a warm ACP channel for a backend.

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

If `enabled: false`, TUI skips that backend and does not create its ACP channel or related process tree.

CLI `acp <backend>` mode does not reuse backend processes. It starts one ACP channel for that request and exits after the request completes.
## Warm Backend Latency Benchmark

Observed on Windows after starting TUI-style warm ACP clients for all enabled backends.

Command used:

```powershell
target\debug\iota-sympantos.exe bench-warm 3
```

Benchmark behavior:

- Warm all enabled ACP backends first.
- Send `ping` to each backend for 3 rounds.
- Measure only warm prompt latency, not initial backend startup time.
- CLI `acp` mode is not involved and does not reuse processes.

### Raw Results

| Backend | Round | Latency | Status | Notes |
|---|---:|---:|---|---|
| `claude-code` | 1 | 30004 ms | error | ACP prompt response timed out |
| `claude-code` | 2 | 30008 ms | error | ACP prompt response timed out |
| `claude-code` | 3 | 30002 ms | error | ACP prompt response timed out; backend later emitted `EPIPE` |
| `codex` | 1 | 10004 ms | ok | Returned `pong`; emitted `gh/gpt-5.5` metadata warning |
| `codex` | 2 | 5034 ms | ok | Returned `pong`; emitted `gh/gpt-5.5` metadata warning |
| `codex` | 3 | 3781 ms | ok | Returned `pong`; emitted `gh/gpt-5.5` metadata warning |
| `gemini` | 1 | 2708 ms | ok | Returned a readiness response rather than literal `pong` |
| `gemini` | 2 | 3239 ms | ok | Returned a readiness response rather than literal `pong` |
| `gemini` | 3 | 5947 ms | ok | Returned a readiness response rather than literal `pong` |
| `hermes` | 1 | 472 ms | error | Backend emitted non-JSON stdout after API auth failure |
| `hermes` | 2 | 0 ms | error | Backend emitted non-JSON stdout after prior auth failure |
| `hermes` | 3 | 0 ms | error | Backend emitted non-JSON stdout after prior auth failure |
| `opencode` | 1 | 4651 ms | ok | Returned `pong` |
| `opencode` | 2 | 1802 ms | ok | Returned `pong` |
| `opencode` | 3 | 2220 ms | ok | Returned `pong` |

### Successful Prompt Latency Summary

| Backend | Successful rounds | Min | Max | Average |
|---|---:|---:|---:|---:|
| `claude-code` | 0/3 | n/a | n/a | n/a |
| `codex` | 3/3 | 3781 ms | 10004 ms | 6273 ms |
| `gemini` | 3/3 | 2708 ms | 5947 ms | 3965 ms |
| `hermes` | 0/3 | n/a | n/a | n/a |
| `opencode` | 3/3 | 1802 ms | 4651 ms | 2891 ms |

### Failure Notes

`claude-code` warmed successfully, but all three warm prompts timed out at the 30 second benchmark timeout. After the timeouts, the adapter emitted an `EPIPE`, indicating the process pipeline was no longer healthy.

`hermes` warmed successfully and created an ACP session under `~/.i6/hermes`, but prompt execution hit an upstream authentication failure:

```text
HTTP 401: login fail: Please carry the API secret key in the 'Authorization' field of the request header
```

Hermes then emitted human-readable diagnostic lines on stdout, which the ACP reader correctly treated as non-JSON protocol errors.

`codex`, `gemini`, and `opencode` all completed warm prompt requests. Codex still emitted this warning during successful requests:

```text
Model metadata for `gh/gpt-5.5` not found. Defaulting to fallback metadata; this can degrade performance and cause issues.
```

### Interpretation

Warm reuse currently improves successful backends, especially `gemini` and `opencode`, which returned in a few seconds after startup. However, warm process reuse also exposes backend health issues more clearly:

- A warm ACP channel can become unhealthy after repeated prompt timeouts.
- Backend stdout must remain strict JSON-RPC; non-JSON diagnostics break the ACP stream.
- Authentication/config errors should be resolved before counting a backend as healthy in TUI warm mode.

For TUI mode, the practical health rule should be:

> A backend is considered warm and healthy only after startup succeeds and at least one post-warm prompt succeeds within the configured prompt timeout.
