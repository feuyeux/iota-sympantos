# native — Native File Materializer

Projects memory and skill content into backend-native markdown files (`MEMORY.md`, `AGENTS.md`) for backends that don't support MCP.

## Responsibilities

- Generate `MEMORY.md` from memory recall buckets
- Generate `AGENTS.md` from skill registry
- Dry-run preview mode (`MaterializePreview`)
- Resolve per-backend file paths

## Key Types

- `MaterializePreview` — dry-run result with projected file contents
