# context — Context Fabric

Composes the `<iota-context>` XML capsule injected into every prompt, assembling memory recall, skill triggers, working memory, and workspace state within budget limits.

## Responsibilities

- Compose effective prompts by assembling context sections
- Manage working memory buffer (circular buffer of recent turns)
- Enforce character budgets per section (memory, skills, working memory)

> **Note:** The MCP sidecar (`iota-context`) that was formerly in `context/server.rs` now lives in [`mcp/server.rs`](../mcp/SKILL.md).

## Key Types

- `ContextEngine` — capsule composer with budget enforcement
- `WorkingMemoryBuffer` — circular buffer of last N prompt/output summaries
- `WorkingMemoryTurn` — single turn record (backend + prompt/output summaries)
- `ComposeInput` — input bundle for capsule composition
