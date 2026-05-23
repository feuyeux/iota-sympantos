---
name: kanban
description: Use when working on kanban task board, dispatch, shadow DB materialization, hermes worker lifecycle, event sourcing, or cross-node sync.
triggers:
  - crates/iota-core/src/kanban
  - kanban
  - dispatch
  - shadow
  - ShadowWatcher
  - ShadowMaterializer
  - Dispatcher
  - KanbanStore
  - worker
---

# kanban — Event-sourced task board with shadow DB hijack for hermes execution

## Responsibilities

- Event-sourced task CRUD (boards, tasks, runs, comments, links)
- State machine: triage → todo → ready → running → done → archived (+ blocked)
- Shadow DB materialization: project single task into hermes-compatible SQLite
- Worker lifecycle: spawn hermes `-z`, monitor exit, recover results
- ShadowWatcher: poll shadow DB for hermes writes, sync back to main store
- Cross-node event sync (export/import/serve/pull/push)
- AdvancedBridge: decompose/specify orchestration via LLM

## Sub-modules

| Module            | Purpose                                                                   |
| :---------------- | :------------------------------------------------------------------------ |
| types.rs          | Task, Board, Run, Comment, Link domain types + CreateTaskRequest          |
| store.rs          | KanbanStore trait (CRUD + event sourcing interface)                       |
| sqlite_store.rs   | SqliteKanbanStore — full event-sourced implementation                     |
| state_machine.rs  | Status enum, valid transitions, transition validation                     |
| event_sourcing.rs | Event replay, apply_event, EventPayload variants                          |
| dispatcher.rs     | Dispatcher — polls ready tasks, spawns workers, health checks             |
| worker.rs         | WorkerHandle — spawn hermes -z, kill process tree, log routing            |
| shadow.rs         | ShadowMaterializer (project task→shadow DB) + ShadowWatcher (poll events) |
| bridge.rs         | AdvancedBridge — decompose/specify via LLM backend                        |
| event_sync.rs     | Export/import event bundles, HTTP serve/pull/push                         |

## Key Types

- `Task` — core entity with id, board_id, title, body, status, assignee, priority, tags
- `Board` — named container (slug + name)
- `Run` — execution record (run_id, task_id, profile, status, timestamps)
- `Status` — triage|todo|ready|running|done|archived|blocked
- `Dispatcher` — owns workers map + materializer, drives tick() loop
- `DispatcherConfig` — tick_interval, max_concurrent, claim_ttl, heartbeat_timeout, hermes_bin, shadows_dir
- `ShadowMaterializer` — creates shadow DB with hermes-compatible schema
- `ShadowWatcher` — polls shadow task_events for terminal status
- `WorkerHandle` — child process + run_id + started_at
- `KanbanStore` — trait for all CRUD + event operations

## Design: Shadow DB Hijack

iota's DB is the single source of truth. hermes never touches it directly.

1. Materializer creates `shadows/{task_id}/kanban.db` with hermes-compatible schema
2. Worker spawns `hermes -z` with `HERMES_KANBAN_DB` pointing to shadow
3. hermes reads task via `kanban_show`, executes work, writes `kanban_complete`
4. ShadowWatcher detects terminal status in shadow, syncs to main store
5. Materializer cleans up shadow directory after success

## State Transition Drivers

| Transition           | Driver            | Mechanism                                 |
| -------------------- | ----------------- | ----------------------------------------- |
| triage→todo→ready    | User/orchestrator | `iota kanban move` CLI                    |
| ready→running        | Dispatcher        | claim on spawn (prevents double-dispatch) |
| running→done/blocked | hermes            | kanban_complete in shadow → watcher sync  |
