---
name: iota-src-daemon
description: Use when working on the internal TCP daemon, EnginePool reuse by cwd, daemon prompt protocol, warm ACP clients, or files under src/daemon.
triggers:
  - src/daemon
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

## Sub-modules

| Module | Purpose |
|--------|---------|
| `pool` | `EnginePool` — per-cwd engine instance management |
| `proto` | `DaemonPromptRequest` / `DaemonPromptResponse` wire types |

## Key Types

- `EnginePool` — maps cwd → `IotaEngine` with warm ACP clients
- `DaemonPromptRequest` — inbound prompt (backend, cwd, prompt, timeout)
- `DaemonPromptResponse` — result (ok, output, error, timing)
