# engine — IotaEngine Orchestration

Core orchestration layer that manages ACP client pools, executes prompt turns, and coordinates memory, skills, ledger, and handoffs.

## Responsibilities

- Create and reuse ACP client sessions per (backend, cwd)
- Execute prompt turns with context injection and event collection
- Persist turn results to session ledger and episodic memory
- Handle backend handoff (switching between backends mid-session)
- Manage event loop for streaming prompt execution

## Sub-modules

| Module | Purpose |
|--------|---------|
| `event_loop` | Streaming event processing during prompt execution |
| `handoff` | Backend switching with session context transfer |
| `memory_ops` | Memory extraction, classification, recall, and injection |
| `prompt` | Core prompt execution flow with early-return paths |

## Key Types

- `IotaEngine` — main orchestrator holding ACP clients, stores, config
- `ClientKey` — `(AcpBackend, PathBuf)` key for client reuse
