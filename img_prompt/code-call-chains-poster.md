# docs/code-call-chains.md poster prompt

Selected GPT-Image2 template: `infographic-engine`

Use style tags: `pen-and-ink technical story poster`, `sequential flow diagram`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos code call chains` across Cargo workspace boundaries.

Scene: a prompt capsule travels through a simple technical 5-station dispatch board. 

- Station 1 is Entry: CLI Entry via `crates/iota-cli/src/main.rs → cli::run()` with toggle switches; and Desktop GUI Entry via `crates/iota-desktop/src/components/ChatWorkbench.tsx` calling Tauri commands in `crates/iota-desktop/src-tauri/src/lib.rs` (e.g., `submit_prompt`, `cancel_turn`, `get_config`).
- Station 2 splits into three parallel tracks: TUI path via `crates/iota-cli/src/tui/loop.rs` spawning a background engine task; desktop GUI path via `crates/iota-desktop/src-tauri/src/daemon_client.rs` sending TCP `StartTurn`/`CancelTurn` commands; and daemon CLI path directly querying local TCP daemon at `127.0.0.1:47661` routed through `crates/iota-core/src/daemon/mod.rs` & `pool.rs`.
- Station 3 is Core Orchestration: `crates/iota-core/src/engine/mod.rs` (`IotaEngine`) doing request hash, replay / join running execution, skill trigger matching, memory recall (6 buckets), context capsule composition, and session ledger tracking.
- Station 4 is ACP Ribbon: standard stdio JSON-RPC 2.0 pipe represented as a pipeline ribbon executing the sequence: `initialize → session/new → session/prompt → session/update → session/request_permission → session/complete`.
- Station 5 shows Backend Process returning output upstream to `crates/iota-core/src/runtime_event.rs` (which streams output back to TUI or emits "daemon-message" window events to React turnsReducer) and writing back to rusqlite stores in `crates/iota-core/src/store/`.
- Parallel loop shows Kanban Dispatcher (`crates/iota-kanban/src/dispatcher.rs`) picking up `ready` tasks, spawning `crates/iota-kanban/src/worker.rs` (spawning hermes -z agent process), sync events via `crates/iota-kanban/src/event_sync.rs` and projecting board cards to both TUI (`crates/iota-cli/src/tui/kanban_view.rs`) and Desktop inspector (`crates/iota-desktop/src/components/RightInspector.tsx`).

Four external gates:

- SQLite stores: `~/.i6/context/events.sqlite` · `memory.sqlite` · `approvals.sqlite` · `sessions.sqlite`
- MCP sidecars: `iota context-mcp` (iota-context server) · `iota fun-mcp` (iota-fun 7 language engine)
- ACP child stdio pipe: `stdin / stdout JSON-RPC 2.0`
- TCP socket: `DaemonPromptRequest / DaemonPromptResponse` (exchanged between core daemon and desktop client)

Composition: portrait poster, 2:3 aspect ratio. Five stations as numbered boxes connected by bold arrows, ACP ribbon across the middle, Kanban dispatcher loop on the right, external gates on the margins. Title `Code Call Chains` at top, subtitle `entry → workspace boundary → backend stream → writeback` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on the capsule, Kanban task cards, and ACP ribbon. Simple and uncluttered.

Mood: clear debugging map, easy to follow at a glance.

Text requirements: all visible text must be Chinese or English only. Preserve exact file paths, environment variables, and command names.

Negative prompt: spaghetti arrows, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, fake stack traces, obsolete module names, `src/tui.rs` as a single-file TUI, `src/store/events.rs`, `skill/sandbox_executor.rs`, `telemetry/console.rs`, wrong command labels, Korean text, non-Chinese non-English text, and legacy `src/` prefix paths.
