# MCP Protocol Baseline (Rust)

> Generated from `crates/iota-core/src/mcp/` — iota-sympantos Rust implementation.
> Sidecar comparison (iota-fun 7-language SDK): **待补充** — reference repository not accessible.

## Protocol Constants

| Constant | Value |
|----------|-------|
| Protocol Version | `2024-11-05` |
| Server Name | `iota-context` |
| Client Name | `iota` |

## Client Messages (client.rs)

### Initialization

| Method | Params |
|--------|--------|
| `initialize` | `protocolVersion: string`, `capabilities: {}`, `clientInfo: {name: string, version: string}` |
| `notifications/initialized` | `{}` |

### Tool Invocation

| Method | Params |
|--------|--------|
| `tools/call` | `name: string`, `arguments: object` |

### Data Structures

| Struct | Fields | Types |
|--------|--------|-------|
| `McpToolCall` | `name`, `arguments` | `String`, `serde_json::Value` |
| `McpToolResult` | `ok`, `content`, `error` | `bool`, `Value`, `Option<String>` |

## Server Messages (server.rs)

### Response Format

| Method | Result Fields |
|--------|--------------|
| `initialize` | `protocolVersion: string`, `capabilities: {tools: {}}`, `serverInfo: {name: string, version: string}` |
| `tools/list` | `tools: [{name, description, inputSchema}]` |
| `tools/call` | `content: [{type: "text", text: string}]`, `structuredContent: Value`, `isError: bool` |
| `resources/list` | `resources: [{uri, name, mimeType}]` |
| `resources/read` | `contents: [{uri, mimeType, text}]`, `isError: bool` |

### Error Response

| Field | Type |
|-------|------|
| `code` | `i64` |
| `message` | `String` |

## Router Rules (router.rs)

### Method Aliases

| Canonical | Aliases |
|-----------|---------|
| `tools/call` | `mcp/tools/call`, `mcp/tool_call` |

### Parameter Aliases

| Canonical | Alias |
|-----------|-------|
| `name` | `toolName` |
| `arguments` | `input` |

### Routing Priority

1. Known iota tools (via registry)
2. "fun" external tools
3. `iota_*` prefix → error if unrecognized
4. Other external tools → denied by policy

## Tool Dispatch (tool_dispatch.rs)

### Registered Tools

| Tool Name | Required Params | Optional Params | Return Fields |
|-----------|----------------|-----------------|---------------|
| `iota_memory_search` | `query: string` | `limit: integer(20)`, `mode: hybrid\|vector\|keyword` | `records`, `mode` |
| `iota_memory_write` | `content: string`, `type: semantic\|episodic\|procedural`, `scope: user\|project\|session\|global`, `confidence: number(0-1)` | `facet`, `scope_id`, `merge_mode`, `ttl_days`, `metadata`, `source_*`, `supersedes` | `{ok, id, merged}` |
| `iota_skill_search` | — | `backend: string(codex)` | `index`, `diagnostics` |
| `iota_skill_load` | `name: string` | — | `metadata`, `body` |
| `iota_session_summary` | — | `session_id: string` | `summary: {iota_session_id, cwd, active_backend, turn_count, last_output_summary}` |
| `iota_handoff_publish` | `session_id: string`, `summary: string` | `from_backend`, `to_backend` | `{ok: true}` |
| `iota_handoff_read` | `session_id: string` | `to_backend` | `{handoff: Value}` |
| `iota_kanban_create_task` | `title: string` | `body`, `status`, `assignee`, `priority`, `tags`, `auto_ready`, `board_slug`, `board_name`, `workspace_*` | `{ok, task_id, status, board, auto_ready, auto_dispatch}` |
| `iota_kanban_list_tasks` | — | `status`, `assignee`, `limit` | `{tasks: [{id, title, status, assignee, priority, tags}]}` |
| `iota_kanban_ready_task` | `task_id: integer` | — | `{ok, task_id, status, auto_dispatch}` |

### Domain Enums

| Enum | Values |
|------|--------|
| `MemoryType` | `semantic`, `episodic`, `procedural` |
| `MemoryFacet` | `identity`, `preference`, `strategic`, `domain` |
| `MemoryScope` | `session`, `project`, `user`, `global` |
| `MemoryMergeMode` | `auto`, `add`, `update`, `none` |
| `MemorySearchMode` | `keyword`, `vector`, `hybrid` |
| `KanbanStatus` | `triage`, `todo`, `ready`, `running`, `blocked`, `done`, `archived` |

## Drift Table

> iota-fun sidecar repository not accessible at `/platform/.ref-clones/`.
> Only Rust-side baseline established. Cross-language drift comparison pending.

| Field/Message | Rust | Sidecar | Status |
|---------------|------|---------|--------|
| All fields above | ✅ Present | — | Sidecar对比待补充 |
