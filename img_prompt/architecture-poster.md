# docs/architecture.md poster prompt

Use `gpt-image-2-style-library`: `pen-and-ink technical story poster`, `hand-drawn architectural cutaway`, `fine cross-hatching`, `precise black ink`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos architecture overview`.

Scene: a compact Rust CLI/TUI control tower named `iota-sympantos` in the center. Five labeled rail lines run outward to five AI backend stations: Claude Code, Codex, Gemini CLI, Hermes, OpenCode. Below is a transparent underground cutaway with connected rooms: Context Fabric, SQLite stores, ACP JSON-RPC pipes, MCP sidecars, and telemetry instruments. Small engineers carry context capsules and approval stamps between rooms.

The tower has 8 labeled floors:
- Floor 1 Entry: main.rs · cli/mod.rs · tui/loop.rs · tui/input.rs
- Floor 2 Daemon: daemon/mod.rs · pool.rs · TCP 127.0.0.1:47661
- Floor 3 Engine: engine/mod.rs · prompt.rs · memory_ops.rs · runtime_event/mod.rs
- Floor 4 Context: context/mod.rs · mcp/server.rs · store/memory.rs
- Floor 5 ACP: acp/client.rs · stream_reader.rs · permission.rs
- Floor 6 Backends: five platform stations
- Floor 7 Skill: skill/mod.rs · skill/fun.rs · mcp/router.rs
- Floor 8 Native: native/mod.rs
- Basement Store: store/cache.rs · store/observability.rs · approvals.rs · ledger.rs · telemetry/stderr.rs

Composition: portrait poster, 2:3 aspect ratio. Strong vertical hierarchy, Entry at top, Basement at bottom. Arrows, pipes, short labels. Title `iota-sympantos Architecture` hand-lettered at top. Magenta accent only on the main tower beacon and status rail.

Style: elegant black-and-white steel-nib pen drawing, dense but legible cross-hatching, technical diagram mixed with storybook world-building. No photorealism, no 3D render, no gradients.

Mood: curious, organized, showing complex orchestration as a navigable machine-city.

Negative prompt: blurry text, unreadable labels, crowded random symbols, neon cyberpunk, watercolor, oil paint, broken arrows, obsolete module names (tui.rs single-file, acp/mod.rs without client.rs, telemetry/console.rs).