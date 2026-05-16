# docs/architecture.md poster prompt

Use `gpt-image-2-style-library`: `pen-and-ink technical story poster`, `hand-drawn architectural cutaway`, `fine cross-hatching`, `precise black ink`, `warm paper texture`, `playful engineering narrative`.

Create a vertical poster for the document `iota-sympantos architecture overview`.

Scene: a compact Rust CLI/TUI control tower named `iota-sympantos` sits in the center like a tiny railway signal station. From the tower, five labeled rail lines run outward to five AI backend stations: Claude Code, Codex, Gemini CLI, Hermes, and OpenCode. Below the tower is a transparent underground cutaway showing Context Fabric, SQLite stores, ACP JSON-RPC pipes, MCP sidecars, telemetry instruments, and native projection workshops as connected rooms. Small human engineers in simple work clothes carry context capsules, memory ledgers, skill scrolls, and approval stamps between rooms. The story should feel like a busy but orderly miniature city where every subsystem has a job.

The tower is organized as 8 floors plus a basement:
- Floor 1 (Entry): main.rs, cli/mod.rs, tui/mod.rs, tui/input.rs, tui/scrollback.rs, tui/loop.rs
- Floor 2 (Daemon): daemon/mod.rs, daemon/pool.rs, daemon/proto.rs — TCP at 127.0.0.1:47661
- Floor 3 (Engine): engine/mod.rs, engine/prompt.rs, engine/memory_ops.rs, runtime_event/mod.rs
- Floor 4 (Context): context/mod.rs, mcp/server.rs, store/memory.rs
- Floor 5 (ACP): acp/mod.rs, acp/client.rs, acp/stream_reader.rs, acp/permission.rs
- Floor 6 (Backends): five platform stations
- Floor 7 (Skill): skill/mod.rs, skill/fun.rs, mcp/router.rs, mcp/tool_dispatch.rs
- Floor 8 (Native): native/mod.rs
- Basement (Store): store/cache.rs, store/approvals.rs, store/ledger.rs, telemetry/stderr.rs

Composition: portrait poster, 2:3 aspect ratio. Strong central vertical hierarchy: Entry at the top, Presentation below it, Service Orchestration in the middle, Context/Protocol/Store/Runtime Events as four connected chambers, External Boundaries at the bottom. Use arrows, pipes, labels, and little signs, but keep all text short and readable. Add the title `iota-sympantos Architecture` as hand-lettered text at the top.

Style: elegant black-and-white steel-nib pen drawing, precise linework, dense but legible cross-hatching, technical diagram mixed with storybook world-building, subtle magenta accent only on the main tower beacon and status rail. No photorealism, no 3D render, no glossy UI mockup, no gradient background.

Mood: curious, clever, organized, a little whimsical, showing complex orchestration as a navigable machine-city.

Negative prompt: blurry text, unreadable labels, crowded random symbols, corporate stock art, neon cyberpunk, colored comic style, watercolor, oil paint, low-detail sketch, distorted terminals, broken arrows, fake code blocks, obsolete module names, tui.rs (single file), acp/mod.rs (without client.rs), telemetry/console.rs.