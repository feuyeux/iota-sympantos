# docs/debugging.md poster prompt

Selected GPT-Image2 template: `poster-layout-system`

Use style tags: `pen-and-ink technical story poster`, `developer workbench diagram`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos debugging guide` inside the split Cargo workspace (`iota-cli`, `iota-core`, `iota-kanban`).

Scene: an engineer at a developer workbench using CodeLLDB tools, inspecting a Rust CLI machine named `iota-sympantos`. The machine is open with labeled access panels. Breakpoints appear as small magenta-red dots on the panels. A side panel contains current debug configuration tags, and a recovery station at the bottom shows environment variables, logs, and TUI recovery controls.

Key components of the workbench:

- Labeled Access Panels (accurate workspace file paths):
  - `crates/iota-cli/src/cli/mod.rs` · `crates/iota-cli/src/cli/run_cmd.rs`
  - `crates/iota-cli/src/tui/mod.rs` · `crates/iota-cli/src/tui/loop.rs` · `crates/iota-cli/src/tui/render.rs`
  - `crates/iota-core/src/engine/mod.rs` · `crates/iota-core/src/runtime_event.rs`
  - `crates/iota-core/src/acp/client.rs` · `crates/iota-core/src/acp/stream_reader.rs` · `crates/iota-core/src/acp/wire.rs`
  - `crates/iota-kanban/src/dispatcher.rs` · `crates/iota-kanban/src/worker.rs`
  - `crates/iota-desktop/src-tauri/src/lib.rs` · `crates/iota-desktop/src-tauri/src/daemon_client.rs`
- Debug configuration tags: Debug TUI · Debug Run Direct · Debug Run via Daemon · Debug Check · Debug Context MCP · Debug Fun MCP · Debug Kanban Work · Debug Desktop (Tauri Dev)
- Command strip: `iota` · `iota run --no-daemon` · `iota run --daemon` · `iota check` · `iota context-mcp` · `iota fun-mcp` · `iota bench-cold 3` · `iota kanban sync`
- Runtime boundaries: TUI native terminal scrollback · ACP child process stdio (JSON-RPC 2.0) · daemon TCP `127.0.0.1:47661` · desktop IPC channels
- Bottom recovery station: `RUST_LOG=debug` · `RUST_BACKTRACE=1` · daily log files under `~/.i6/logs/` · `IOTA_LOG_DIR` · terminal-reset lever (TUI Panic Hook cleanup guard)
- Observability helpers: `iota observability tokens summary` · `iota observability logging recent` · `iota logs <execution_id>` · `iota trace <trace_id>`
- Keyboard shortcuts strip: `F5` (Start / Continue) · `F10` (Step Over) · `F11` (Step Into) · `Shift+F5` (Stop)

Composition: portrait poster, 2:3 aspect ratio. Labeled access panels in the center, configuration tags on the side, recovery station at the bottom, small keyboard strip. Title `Debugging iota-sympantos` at top, subtitle `CodeLLDB · Workspace · ACP · Daemon · Observability` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on breakpoint dots and active debug path. Simple and uncluttered.

Mood: focused repair manual, clear and easy to follow at a glance.

Text requirements: all visible text must be Chinese or English only. Preserve exact file paths, command names, and environment variable names.

Negative prompt: chaotic code wall, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, fake stack traces, obsolete module names, `src/tui.rs` as a single-file TUI, `src/store/events.rs`, `telemetry/console.rs`, `skill/sandbox_executor.rs`, wrong debug names, Korean text, non-Chinese non-English text, and legacy single-crate `src/` prefix paths.
