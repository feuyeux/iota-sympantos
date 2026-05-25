# docs/architecture.md poster prompt

Selected GPT-Image2 template: `poster-layout-system`

Use style tags: `pen-and-ink technical story poster`, `hand-drawn architectural cutaway`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos architecture overview` representing the Cargo workspace structure (`iota-cli`, `iota-core`, `iota-kanban`, `iota-desktop`).

Scene: a compact Rust CLI/TUI control tower named `iota-sympantos` in the center, with five labeled rail lines running outward to five AI backend stations: Claude Code, Codex, Gemini CLI, Hermes, OpenCode. Below the tower is a transparent underground cutaway with connected rooms representing the Workspace Crates, Context Fabric, MemoryStore, CacheStore, ObservabilityStore, ApprovalStore, SessionLedger, ACP JSON-RPC pipes, MCP sidecars, Skill/Fn runners, Kanban dispatcher, Hermes worker, resizable Desktop GUI panel, and telemetry instruments.

Ten labeled levels from top to bottom:

- Floor 1 Entry: `crates/iota-cli/src/main.rs` · `crates/iota-cli/src/cli/mod.rs` · `cli/run_cmd.rs` · `cli/daemon_cmd.rs`
- Floor 2 Interaction: TUI Composer (`crates/iota-cli/src/tui/mod.rs` · `tui/loop.rs` · `tui/input.rs` · `tui/render.rs`) & Desktop GUI (`crates/iota-desktop/src/components/ChatWorkbench.tsx` · `RightInspector.tsx` · `src-tauri/src/lib.rs` Tauri commands)
- Floor 3 Daemon Plane: `crates/iota-core/src/daemon/mod.rs` · `daemon/pool.rs` · TCP `127.0.0.1:47661` · `crates/iota-desktop/src-tauri/src/daemon_client.rs`
- Floor 4 Engine Core: `crates/iota-core/src/engine/mod.rs` · `engine/prompt.rs` · `engine/memory_ops.rs` · `crates/iota-core/src/runtime_event.rs`
- Floor 5 Context Capsule & Memory: `crates/iota-core/src/context/mod.rs` · `crates/iota-core/src/memory/store.rs` · `memory/embedding.rs`
- Floor 6 ACP Wire: `crates/iota-core/src/acp/mod.rs` · `acp/client.rs` · `acp/wire.rs` · `acp/session.rs` · `acp/permission.rs`
- Floor 7 AI Backends: Claude Code · Codex · Gemini CLI · Hermes Agent · OpenCode
- Floor 8 Skill & MCP: `crates/iota-core/src/skill/mod.rs` · `skill/runner.rs` · `skill/fun.rs` · `crates/iota-core/src/mcp/client.rs` · `mcp/router.rs` · `mcp/tool_dispatch.rs` · `mcp/server.rs`
- Floor 9 Kanban Orchestration: `crates/iota-kanban/src/lib.rs` · `sqlite_store.rs` · `state_machine.rs` · `dispatcher.rs` · `worker.rs` (spawning hermes -z) · `shadow.rs` · `bridge.rs` · `event_sync.rs`
- Basement Store / Telemetry: `crates/iota-core/src/store/mod.rs` · `store/cache.rs` · `store/observability.rs` · `store/approvals.rs` · `store/ledger.rs` · `crates/iota-core/src/telemetry/mod.rs` · `telemetry/metrics.rs` · `telemetry/stderr.rs`

Composition: portrait poster, 2:3 aspect ratio. Strong vertical hierarchy with Entry at top and Store / Telemetry at bottom, floors connected by elevator shafts and pipelines, rails leading outward. Title `iota-sympantos Architecture` at top, subtitle `workspace modules & integration layout` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on the main tower beacon, prompt capsule, status rail, and Kanban board pins. Simple and uncluttered.

Mood: curious and organized, showing complex orchestration as a navigable machine-city.

Text requirements: all visible text must be Chinese or English only. Preserve exact file paths and command labels.

Negative prompt: spaghetti arrows, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, obsolete module names, `src/tui.rs` as a single-file TUI, `src/store/events.rs`, `telemetry/console.rs`, `skill/sandbox_executor.rs`, `context/server.rs`, fake cloud logos, raw API keys, Korean text, non-Chinese non-English text, and single-crate legacy `src/` prefix paths.
