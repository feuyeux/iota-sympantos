# docs/code-call-chains.md poster prompt

Use `gpt-image-2-style-library`: `pen-and-ink technical story poster`, `sequential flow diagram`, `hand-lettered annotations`, `fine cross-hatching`, `mechanical process narrative`, `warm paper texture`.

Create a vertical poster for the document `iota-sympantos code call chains`.

Scene: depict a journey of one prompt as a small sealed message capsule traveling through a mechanical dispatch system. It begins at `src/main.rs`, enters `cli::run()`, passes command switches for `run`, `tui`, `check`, `bench-cold`, `bench-warm`, `logs`, `trace`, `context-mcp`, `fun-mcp`, `native-materialize`, `skill`, and `__daemon`, then splits into illustrated paths: the direct ACP route and the daemon TCP route. The direct route moves through tui/loop.rs (spawning engine task), engine/mod.rs (IotaEngine), CacheStore begin/execution, SkillRegistry match, memory recall, context capsule (context/mod.rs, WorkingMemoryBuffer), ACP client (acp/client.rs, stream_reader.rs), streaming updates, and final output. The daemon route shows local TCP at `127.0.0.1:47661`, engine pool by cwd, and JSON line response.

External boundary gates shown as mechanical hatches:
- git subprocess → context/mod.rs (workspace git status)
- ACP child process stdio → acp/client.rs
- MCP stdio sidecar → mcp/server.rs (iota-context), skill/fun.rs (iota-fun)
- SQLite files → store/cache.rs, store/memory.rs, store/approvals.rs, store/ledger.rs
- TCP socket → daemon/mod.rs

Current RuntimeEvent types on the protocol ribbon: Output, State, Log, ToolCall, ToolResult, Error, Extension, TokenUsage, Memory, ApprovalRequest, ApprovalDecision.

Composition: portrait poster, 2:3 aspect ratio. Arrange the call chain as a large board-game-like path with numbered stations and arrows. Put `initialize → session/new → session/prompt → session/update → session/complete` as a clear ribbon across the middle. Show external boundaries as illustrated gates. Add the title `Code Call Chains` at the top and a small subtitle `from entry point to runtime boundary`.

Style: black ink steel-nib illustration, crisp contour lines, engineering notebook feel, fine stippling and cross-hatching, readable miniature labels, light magenta accent on the active message capsule and protocol ribbon. Keep it playful through the journey metaphor, but technically accurate and structured.

Mood: adventurous debugging map, a prompt crossing checkpoints and machines, clear enough to teach the runtime path at a glance.

Negative prompt: unreadable spaghetti arrows, random pseudo-code, fantasy map parchment cliches, photorealistic devices, glossy dashboard, cyberpunk glow, excessive colors, cluttered icons, distorted terminal text, old module names (tui.rs single file, acp/mod.rs without client.rs, skill/sandbox_executor.rs, store/approval.rs, telemetry/console.rs).