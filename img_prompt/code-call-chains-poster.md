# docs/code-call-chains.md poster prompt

Selected GPT-Image2 template: `infographic-engine`

Use style tags: `pen-and-ink technical story poster`, `sequential flow diagram`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos code call chains`.

Scene: a prompt capsule travels through a simple five-station dispatch board. Station 1 is `main.rs → cli::run()` with command switches shown as labeled toggle levers: `run`, `check`, `bench <cold|warm>`, `observability`, `logs`, `trace`, `mcp <context|fun>`, `context-mcp`, `fun-mcp`, `kanban`, `skill`, `__daemon`. Station 2 splits into two paths: TUI path via `tui/loop.rs` spawning a background engine task and rendering through `tui/render.rs`; daemon path via TCP at `127.0.0.1:47661` and `daemon/pool.rs`. Station 3 is `engine/mod.rs` (`IotaEngine`): request hash, replay / join running, skill match, memory recall, context capsule, session ledger, telemetry event recording. Station 4 is ACP: `initialize → session/new → session/prompt → session/update → session/request_permission → session/complete` shown as a ribbon. Station 5 shows backend process output returning upstream to RuntimeEvent and stores.

Four external gates:

- SQLite stores: `events.sqlite` · `memory.sqlite` · `approvals.sqlite` · `sessions.sqlite`
- MCP sidecars: `iota context-mcp` · `iota fun-mcp` · `iota mcp context` · `iota mcp fun`
- ACP child stdio pipe: `stdin / stdout JSON-RPC 2.0`
- TCP socket: `DaemonPromptRequest / DaemonPromptResponse`

Composition: portrait poster, 2:3 aspect ratio. Five stations as numbered boxes connected by bold arrows, ACP ribbon across the middle, external gates on the margin. Title `Code Call Chains` at top, subtitle `entry → runtime boundary → backend stream → writeback` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on the capsule and ACP ribbon. Simple and uncluttered.

Mood: clear debugging map, easy to follow at a glance.

Text requirements: all visible text must be Chinese or English only. Preserve exact file paths and command names.

Negative prompt: spaghetti arrows, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, fake stack traces, obsolete module names, `tui.rs` as a single-file TUI, `skill/sandbox_executor.rs`, `telemetry/console.rs`, wrong command labels, Korean text, non-Chinese non-English text.
