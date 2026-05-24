---
name: iota-desktop
description: Use when working on the Tauri desktop workbench, React components, Tauri command bindings, daemon IPC messages, or files under crates/iota-desktop.
triggers:
  - crates/iota-desktop
  - desktop
  - tauri
  - react
---

# iota-desktop — Tauri Desktop GUI Workbench

Desktop graphical user interface for iota-sympantos combining React frontend and Tauri backend. It connects to the local TCP daemon for model execution and utilizes the Kanban library for task management.

## Responsibilities

- **Chat & Control Interface**: A React frontend chat client supporting multi-model execution, streaming responses, and runtime approval overlays.
- **Daemon Client**: A Tauri backend component connecting to the `iota` daemon TCP server to send prompts, check configurations, and manage active turns.
- **Kanban Board Integrations**: Connects to the event-sourced `SqliteKanbanStore` directly inside Rust commands to view, update, and comment on task boards.
- **Observability Viewer**: Displays token usage analytics and percentiles retrieved via the daemon client.

## Structure

```
crates/iota-desktop/
├── src/                     # React Frontend
│   ├── components/          # React layout and widget components
│   ├── App.tsx              # Root application view
│   ├── api.ts               # JavaScript bindings calling Tauri commands
│   ├── types.ts             # Shared frontend domain types
│   └── turnReducer.ts       # State machine for turns and stream chunks
└── src-tauri/               # Tauri Rust Backend
    ├── src/
    │   ├── lib.rs           # Tauri command definitions and state setup
    │   ├── daemon_client.rs # TCP stream client to connect to local daemon
    │   └── main.rs          # Application entry point
    └── tauri.conf.json      # Tauri package metadata and devUrl configs
```

## Key Tauri Commands

- `get_config` / `save_backend_model` — Read/Write nimia.yaml settings via daemon.
- `submit_prompt` / `cancel_turn` — Execute prompts or cancel turns asynchronously.
- `handle_approval` — Respond (allow/deny) to pending tool calls.
- `get_observability_summary` — Retrieve aggregated token usage statistics.
- `list_boards` / `list_tasks` / `create_task` / `transition_task` — direct Kanban SQLite operations.
