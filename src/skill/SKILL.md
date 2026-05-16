# skill — Skill Layer

Loads `.md`/`.yaml` skill manifests, matches triggers against prompts, and executes skills via engine-run or MCP fun-server.

## Responsibilities

- Load skill manifests from configured roots (local or HTTP)
- Match prompt text against skill trigger patterns
- Execute skills via engine-run (delegated prompt) or MCP (7-language fun-server)
- Cache remote skills locally

## Sub-modules

| Module | Purpose |
|--------|---------|
| `cache` | HTTP/local skill fetching and caching |
| `fun_server` | `iota-fun` MCP server — 7-language execution (C++, Go, Java, Python, Rust, TypeScript, Zig) |
| `runner` | Engine-run skill execution — delegated prompt turns |

## Key Types

- `SkillRegistry` — loaded skill collection with trigger matching
- `Skill` — single skill definition with metadata and execution config
- `SkillMetadata` — name, description, triggers, tags
- `SkillExecution` — execution mode and parameters
- `SkillExecutionMode` — EngineRun or Mcp
- `SkillCache` — local cache for pulled skills
