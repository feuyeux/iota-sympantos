```
Create a high-resolution technical architecture diagram titled:

iota-sympantos Runtime Architecture / 运行时架构

Generate a clean, professional software architecture map with a white background, rounded rectangles, thin vector arrows, color-coded zones, readable typography, and grid alignment.

Use a simple 3-tier layout (no excessive columns):

Top tier:
1. CLI Entry (main.rs · cli/mod.rs)
   Commands: run · check · bench · mcp · observability · skill · native-materialize · __daemon
2. TUI Loop (tui/loop.rs · tui/render.rs)
   Inline viewport · streaming render · approval overlay · pager/help/quit overlays

Middle tier (four main zones, left to right):
3. Daemon TCP (daemon/mod.rs)
   TCP 127.0.0.1:47661 · EnginePool per cwd · JSON-line request/response · auto-start via __daemon

4. IotaEngine (engine/mod.rs)
   Lifecycle: cache replay → skill match → memory recall → context capsule → ACP invocation → writeback
   RuntimeEvent types: Output · ToolCall · ToolResult · TokenUsage · Memory · ApprovalRequest · Error

5. ACP Adapter (acp/client.rs)
   JSON-RPC 2.0 protocol sequence:
   initialize → session/new → session/prompt → session/update (streaming) → session/complete
   Permission: auto-approve iota_* and mcp__iota-* · otherwise route to TUI/stdin

6. Five Backends (each shown as a labeled box with command):
   Claude Code — npx
   Codex — npx
   Gemini CLI — npx
   Hermes — hermes acp
   OpenCode — npx

Right-side extensions (attached to Engine zone):
7. Context Sidecar — iota-context (mcp/server.rs)
   Memory 6 buckets: identity · preference · strategic · domain · procedural · episodic
   Vector/hybrid search · Ollama embeddings · WorkingMemoryBuffer

8. Skill / Fun MCP — iota-fun (skill/fun.rs)
   SkillRegistry: trigger match · frontmatter · backend compat
   Fn runners: Python · TypeScript · Rust · Go · Java · C++ · Zig

Bottom tier (one unified band):
9. SQLite Stores — four labeled blocks:
   cache (~/.i6/context/events.sqlite) · observability (events.sqlite, shared) · memory (memory.sqlite) · approvals (approvals.sqlite) · ledger (sessions.sqlite)
   Config: StoreConfig in nimia.yaml — cache_retention_days · cache_running_ttl_secs · observability_retention_days · approvals_max_pending_age_secs
10. Local Telemetry — stderr tracing · daily files at ~/.i6/logs/
11. OpenTelemetry — OTel Collector :4317 → Loki :3100 · Jaeger :16686 · Prometheus :9090 · Grafana :3000

Color scheme:
Pink = Entry / TUI
Orange = Daemon
Blue = Engine core
Cyan = ACP protocol
Teal = Backends
Green = Context / Memory
Purple = Skill / MCP
Gray = Store / Telemetry

Key flows (arrows only, no prose inside boxes):
- CLI/TUI → Engine (direct path or via Daemon TCP)
- Engine → Context Sidecar (memory recall)
- Engine → Skill/Fun MCP (skill execution)
- Engine → ACP Adapter → Backend child process
- Engine + ACP → bottom Store band (writeback)
- Telemetry → OTel or local fallback

Keep all text large and readable. No tiny labels. Show arrows without overlapping text. Module labels use bold short names; key facts shown as compact bullet lines inside each zone box.

Negative prompt:
Tiny unreadable labels, random fake files, excessive columns (>8), detailed env var mappings inside diagram, 3D render, dark background, neon glow, stock icons, blurry text, old command names (bench-cold, bench-warm, context-mcp, fun-mcp, logs as top-level), obsolete modules (telemetry/console.rs, context/server.rs, skill/sandbox_executor.rs), Korean text, missing ObservabilityStore block.
```

