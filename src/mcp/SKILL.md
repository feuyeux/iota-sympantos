---
name: iota-src-mcp
description: Use when working on MCP stdio server/client code, iota-context tools, ACP tool-call interception, shared tool dispatch, or files under src/mcp.
triggers:
  - src/mcp
  - MCP
  - iota-context
  - iota_memory_write
  - tool_dispatch
  - McpClient
---

# mcp — MCP Protocol Layer

Centralised MCP (Model Context Protocol) implementation: stdio server, ACP stream interceptor, shared tool dispatch, and subprocess client.

## Responsibilities

- Serve `iota-context` as a stdio JSON-RPC MCP server (`iota context-mcp`)
- Intercept `iota_*` tool calls from ACP prompt responses
- Provide a single canonical tool dispatch layer (parsers, validators, handlers)
- Spawn and manage stdio MCP sidecar processes

## Sub-modules

| Module | Purpose |
|--------|---------|
| `server` | Stdio MCP server — JSON-RPC protocol adapter for `iota context-mcp` |
| `router` | ACP stream interceptor — detects and routes `iota_*` tool calls |
| `tool_dispatch` | Shared tool execution logic — parsers, validators, handlers (used by both `server` and `router`) |
| `client` | MCP stdio client — process management and JSON-RPC communication |
