# docs/architecture.md poster prompt

Selected GPT-Image2 template: `poster-layout-system`

Use style tags: `pen-and-ink technical story poster`, `hand-drawn architectural cutaway`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos architecture overview`.

Scene: a compact Rust CLI/TUI control tower named `iota-sympantos` in the center, with five labeled rail lines running outward to five AI backend stations: Claude Code, Codex, Gemini CLI, Hermes, OpenCode. Below the tower is a transparent underground cutaway with connected rooms representing Context Fabric, MemoryStore, CacheStore, ObservabilityStore, ApprovalStore, SessionLedger, ACP JSON-RPC pipes, MCP sidecars, Skill/Fn runners, Kanban dispatcher, Hermes worker, and telemetry instruments.

Nine labeled levels from top to bottom:

- Floor 1 Entry: `main.rs` · `cli/mod.rs` · `tui/loop.rs` · `tui/input.rs`
- Floor 2 TUI: `tui/render.rs` · `scrollback.rs` · `events.rs` · `terminal_lifecycle.rs`
- Floor 3 Daemon: `daemon/mod.rs` · `daemon/pool.rs` · TCP `127.0.0.1:47661`
- Floor 4 Engine: `engine/mod.rs` · `engine/prompt.rs` · `engine/memory_ops.rs` · `runtime_event/mod.rs`
- Floor 5 Context: `context/mod.rs` · `mcp/server.rs` · `memory/store.rs` · `memory/embedding.rs`
- Floor 6 ACP: `acp/client.rs` · `acp/stream_reader.rs` · `acp/permission.rs` · `acp/session.rs`
- Floor 7 Backends: Claude Code · Codex · Gemini CLI · Hermes · OpenCode
- Floor 8 Skill / MCP / Fn: `skill/mod.rs` · `skill/fun.rs` · `mcp/router.rs` · `mcp/tool_dispatch.rs`
- Basement Store / Telemetry: `store/cache.rs` · `store/observability.rs` · `store/approvals.rs` · `store/ledger.rs` · `telemetry/stderr.rs` · `telemetry/metrics.rs`

Composition: portrait poster, 2:3 aspect ratio. Strong vertical hierarchy with Entry at top and Store / Telemetry at bottom, floors connected by elevator shafts and pipelines, rails leading outward. Title `iota-sympantos Architecture` at top, subtitle `modules & integration layout` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on the main tower beacon, prompt capsule, and status rail. Simple and uncluttered.

Mood: curious and organized, showing complex orchestration as a navigable machine-city.

Text requirements: all visible text must be Chinese or English only. Preserve exact file names and command labels.

Negative prompt: spaghetti arrows, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, obsolete module names, `tui.rs` as a single-file TUI, `telemetry/console.rs`, `skill/sandbox_executor.rs`, `context/server.rs`, fake cloud logos, raw API keys, Korean text, non-Chinese non-English text.
