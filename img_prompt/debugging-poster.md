# docs/debugging.md poster prompt

Use `gpt-image-2-style-library`: `pen-and-ink technical story poster`, `debugging workshop`, `hand-drawn developer desk`, `fine cross-hatching`, `annotated troubleshooting map`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos debugging guide`.

Scene: an engineer sits at a VS Code workbench with CodeLLDB tools arranged like precision instruments. A large Rust CLI machine is open for inspection: `main.rs`, `cli/mod.rs`, `engine/mod.rs`, `acp/client.rs`, `acp/stream_reader.rs`, `tui/mod.rs`, `tui/loop.rs`, `tui/input.rs`, `tui/render.rs` appear as labeled access panels. Breakpoints glow as tiny red pinheads on the machine. A side panel shows debug configurations as selectable brass tags: Debug TUI, Debug Run, Debug Run with Daemon, Debug Check, Debug Context MCP Sidecar, Debug Fun MCP Server, Debug Bench Cold, and Debug Daemon. At the bottom, a terminal recovery station shows `RUST_LOG=debug`, `RUST_BACKTRACE=1`, local log files under `~/.i6/logs/`, and a `reset` lever for raw terminal recovery.

The machine panels show these current source files:
- tui/mod.rs, tui/loop.rs — TUI entry and event loop
- tui/input.rs — multi-line editor with history, word motion, kill/yank
- tui/render.rs — main renderer
- acp/client.rs — ACP client protocol
- acp/stream_reader.rs — streaming event reader
- runtime_event/mod.rs — RuntimeEvent normalization

Composition: portrait poster, 2:3 aspect ratio. Make the story read from top to bottom: prerequisites, configurations, breakpoints, stepping controls, variable inspection, TUI debugging, ACP subprocess boundary. Include a small keyboard strip with `F5`, `F10`, `F11`, `Shift+F11`, and `Shift+F5`. Add the title `Debugging iota-sympantos` at the top.

Style: black ink pen illustration, crisp technical linework, cross-hatched shadows, annotated workshop poster, readable labels, subtle magenta accent on breakpoint dots and the active debug path. It should feel practical, hands-on, and slightly playful without becoming cartoonish.

Mood: focused troubleshooting, a repair manual for a complex but understandable runtime.

Negative prompt: chaotic code wall, unreadable text, photorealistic office, glossy app screenshot, neon cyberpunk, excessive red, fantasy laboratory, distorted keyboards, fake stack traces, low-detail doodle, obsolete file names (tui.rs, acp/mod.rs without client.rs), telemetry/console.rs.