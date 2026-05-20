# docs/debugging.md poster prompt

Selected GPT-Image2 template: `poster-layout-system`

Use style tags: `pen-and-ink technical story poster`, `developer workbench diagram`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos debugging guide`.

Scene: an engineer at a workbench with CodeLLDB tools, inspecting a Rust CLI machine named `iota-sympantos`. The machine is open with labeled access panels. Breakpoints appear as small magenta-red dots on the panels. A side panel contains current debug configuration tags, and a recovery station at the bottom shows environment variables, logs, and reset tools.

Key components of the workbench:

- Access panels: `cli/mod.rs` · `engine/mod.rs` · `acp/client.rs` · `acp/stream_reader.rs` · `tui/loop.rs` · `tui/render.rs` · `runtime_event/mod.rs`
- Debug configuration tags: Debug TUI · Debug Run · Debug Run with Daemon · Debug Check · Debug Context MCP Sidecar · Debug Fun MCP Server · Debug Bench Cold
- Command strip: `iota` · `iota run --no-daemon` · `iota run --daemon` · `iota check` · `iota context-mcp` · `iota fun-mcp` · `iota bench-cold 3`
- Runtime boundaries: TUI native terminal scrollback · ACP child process stdin/stdout · daemon TCP `127.0.0.1:47661`
- Bottom recovery station: `RUST_LOG=debug` · `RUST_BACKTRACE=1` · log files under `~/.i6/logs/` · `IOTA_LOG_DIR` · terminal-reset lever
- Observability helpers: `iota observability tokens summary` · `iota observability logging recent` · `iota logs <execution_id>` · `iota trace <trace_id>`
- Keyboard shortcuts strip: `F5` · `F10` · `F11` · `Shift+F5`

Composition: portrait poster, 2:3 aspect ratio. Labeled access panels in the center, configuration tags on the side, recovery station at the bottom, small keyboard strip. Title `Debugging iota-sympantos` at top, subtitle `CodeLLDB · TUI · ACP · Daemon · Observability` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on breakpoint dots and active debug path. Simple and uncluttered.

Mood: focused repair manual, clear and easy to follow at a glance.

Text requirements: all visible text must be Chinese or English only. Preserve exact file paths, command names, and environment variable names.

Negative prompt: chaotic code wall, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, fake stack traces, obsolete module names, `tui.rs` as a single-file TUI, `telemetry/console.rs`, `skill/sandbox_executor.rs`, wrong debug names, Korean text, non-Chinese non-English text.
