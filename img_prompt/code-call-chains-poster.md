# docs/code-call-chains.md poster prompt

Use `gpt-image-2-style-library`: `pen-and-ink technical story poster`, `sequential flow diagram`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos code call chains`.

Scene: a prompt capsule travels through a simple five-station dispatch board. Station 1 is `main.rs → cli::run()` with command switches shown as labeled toggle levers: `run`, `check`, `bench <cold|warm>`, `mcp <context|fun>`, `observability`, `skill`, `__daemon`. Station 2 splits into two paths: TUI path via `tui/loop.rs` spawning an engine task, and daemon path via TCP at `127.0.0.1:47661`. Station 3 is `engine/mod.rs` (IotaEngine): skill match, memory recall, context capsule. Station 4 is ACP: `initialize → session/new → session/prompt → session/update → session/complete` shown as a ribbon. Station 5 shows backend process output returning upstream.

Four external gates (simple hatched borders):
- SQLite stores: events · memory · approvals · sessions
- MCP sidecars: iota-context · iota-fun
- ACP child stdio pipe
- TCP socket (daemon)

Composition: portrait poster, 2:3 aspect ratio. Five stations as numbered boxes connected by bold arrows, ACP ribbon across the middle, external gates on the margin. Title `Code Call Chains` at top, subtitle `entry → runtime boundary` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on the capsule and ACP ribbon. Simple and uncluttered.

Mood: clear debugging map, easy to follow at a glance.

Negative prompt: spaghetti arrows, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, old command names (bench-cold, bench-warm, context-mcp, fun-mcp, logs, trace as top-level), obsolete module names (tui.rs single-file, skill/sandbox_executor.rs, telemetry/console.rs).
