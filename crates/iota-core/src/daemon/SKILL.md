---
name: iota-src-daemon
description: Use when working on the internal TCP daemon, EnginePool reuse by cwd, daemon prompt protocol, warm ACP clients, or files under crates/iota-core/src/daemon.
triggers:
  - crates/iota-core/src/daemon
  - EnginePool
  - DaemonPromptRequest
  - DaemonPromptResponse
  - __daemon
  - 127.0.0.1:47661
---

# daemon — Background Daemon

TCP server on `127.0.0.1:47661` that keeps `IotaEngine` instances alive across CLI invocations, eliminating cold-start overhead.

## Responsibilities

- Accept JSON prompt requests over TCP
- Maintain an `EnginePool` keyed by working directory
- Reuse warm ACP backend connections
- Auto-start on first `--daemon` CLI call
- Provide two local JSON-line APIs: legacy CLI request/response and desktop streaming turns

## Security / trust boundary

The TCP listener performs **no authentication or authorization**. It relies
on binding to loopback (`127.0.0.1`) and on treating the whole host as one
trust domain. Any local process able to open a connection to the daemon can
submit prompts and read observability/memory/context data — including
another local user's process on a shared, multi-user host. This is an
accepted design assumption, not an oversight; do not run this daemon on an
untrusted-multi-user host without adding connection-level authentication.

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `pool` | `EnginePool` — per-cwd engine instance management |
| `proto` | `DaemonPromptRequest` / `DaemonPromptResponse` and desktop wire types |
| `desktop` | `handle_desktop_connection` — streams text chunks, events, and routes approvals |

## Key Types

- `EnginePool` — maps cwd → `IotaEngine` with warm ACP clients
- `DaemonPromptRequest` — inbound prompt (backend, cwd, prompt, timeout)
- `DaemonPromptResponse` — result (ok, output, error, timing)
- `DaemonClientMessage` — desktop client command (start turn, cancel turn, getConfig, saveBackendModel, respondApproval)
- `DaemonServerMessage` — desktop streaming server event (helloAccepted, textChunk, turnEvent, approvalRequested)
