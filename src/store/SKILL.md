---
name: iota-src-store
description: Use when working on SQLite-backed cache, approval policy persistence, session ledger storage, execution idempotency, or files under src/store.
triggers:
  - src/store
  - CacheStore
  - ApprovalStore
  - SessionLedger
  - approval policy
  - execution cache
---

# store — SQLite Store Layer

SQLite-backed persistence for execution cache, tool approvals, and session ledger.

## Responsibilities

- Execution lifecycle caching and deduplication (`CacheStore`)
- Tool approval event recording and policy lookup (`ApprovalStore`)
- Session/turn/handoff tracking (`SessionLedger`)

## Sub-modules

| Module | Purpose |
|--------|---------|
| `approvals` | `ApprovalStore` — tool approval events and policy |
| `cache` | `CacheStore` — execution replay and deduplication |
| `ledger` | `SessionLedger` — sessions, backend sessions, turns, handoffs |

## Key Types

- `CacheStore` — execution caching with idempotency and fencing
- `ApprovalStore` — tool approval persistence and policy lookup
- `SessionLedger` — session/turn/handoff tracking
