# iota-sympantos

Lightweight Rust CLI that orchestrates multiple AI coding assistants via ACP (Agent Control Protocol).

## Capabilities

- **ACP protocol**: JSON-RPC 2.0 over stdin/stdout, supporting Claude Code, Codex, Gemini CLI, Hermes, OpenCode
- **TUI mode**: Interactive ratatui chat with markdown rendering, multi-line composer, kill-buffer, Ctrl+R history search
- **CLI mode**: Single-shot prompt execution, backend health check, cold/warm/daemon benchmarks
- **Memory**: SQLite-backed 6-bucket memory (identity/preference/strategic/domain/procedural/episodic) with FTS5 and TF-IDF embedding search
- **Context Fabric**: Composes XML capsules with memory recall, skill triggers, working memory, and budget enforcement
- **Skill system**: Loads `.md`/`.yaml` skill manifests with trigger matching; 7-language MCP fun runner (C++, Go, Java, Python, Rust, TypeScript, Zig)
- **Daemon**: Background TCP server reusing warm ACP connections across CLI invocations
- **Observability**: OpenTelemetry traces, structured logs, Prometheus metrics

## Structure

```
src/
├── main.rs              # Entry point
├── acp/                 # ACP JSON-RPC 2.0 protocol driver & client pool
├── cli/                 # Command dispatch (run/check/tui/bench/logs/trace/skill)
├── config/              # nimia.yaml parsing, backend/model mapping, effective config
├── context/             # Context Fabric: capsule composition, budget enforcement
├── daemon/              # Background daemon TCP server & engine pool
├── engine/              # IotaEngine orchestration, prompt execution, handoff
├── mcp/                 # MCP protocol layer: server, router, tool dispatch, client
├── memory/              # Persistent memory store (FTS5 + embedding) & types
├── native/              # Native file materializer (MEMORY.md, AGENTS.md)
├── runtime_event/       # Unified event types for telemetry & routing
├── skill/               # Skill registry, trigger matching, cache, fun-server
├── store/               # SQLite stores (cache, approval, ledger)
├── telemetry/           # OpenTelemetry: OTLP exporter, logs, metrics, spans
├── tui/                 # ratatui interactive chat UI
└── utils/               # Shared utilities (timing, summarize, mutex recovery)
```

## Configuration

Single config source: `~/.i6/nimia.yaml`

## Cross-platform

Supports Windows, macOS, Linux. Uses `dirs::home_dir()`, `Path`/`PathBuf`, and platform-aware command normalization.
