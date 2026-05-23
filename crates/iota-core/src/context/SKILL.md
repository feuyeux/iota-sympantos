---
name: iota-src-context
description: Use when working on Context Fabric prompt capsules, memory/skill/workspace injection, working memory, context budgets, or files under crates/iota-core/src/context.
triggers:
  - crates/iota-core/src/context
  - ContextEngine
  - ComposeInput
  - WorkingMemoryBuffer
  - iota-context
  - context capsule
---

# context — Context Fabric

Composes the `<iota-context>` XML capsule injected into every prompt, assembling memory recall, skill triggers, working memory, and workspace state within budget limits.

## Responsibilities

- Compose effective prompts by assembling context sections
- Manage working memory buffer (circular buffer of recent turns)
- Enforce character budgets per section (memory, skills, working memory)

> **Note:** The MCP sidecar (`iota-context`) that was formerly in `crates/iota-core/src/context/server.rs` now lives in [`crates/iota-core/src/mcp/server.rs`](../mcp/SKILL.md).

## Key Types

- `ContextEngine` — capsule composer with budget enforcement
- `WorkingMemoryBuffer` — circular buffer of last N prompt/output summaries
- `WorkingMemoryTurn` — single turn record (backend + prompt/output summaries)
- `ComposeInput` — input bundle for capsule composition
