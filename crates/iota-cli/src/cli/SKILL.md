---
name: iota-src-cli
description: Use when working on iota CLI commands, argument dispatch, run/check/bench/memory/skill commands, daemon routing, or files under src/cli.
triggers:
  - src/cli
  - iota run
  - iota check
  - bench-cold
  - bench-warm
  - context-mcp
  - fun-mcp
---

# cli — Command Dispatch

Top-level CLI entry point. Parses arguments and routes to subcommand handlers.

## Commands

| Command | Handler | Description |
| :---------| :---------| :-------------|
| (default) | `tui::run()` | Interactive TUI mode |
| `run <backend> <prompt>` | `run_cmd` | Single-shot prompt execution |
| `check [--daemon]` | `info_cmd` | Backend health/info JSON output |
| `bench-cold [rounds]` | `daemon_cmd` | Cold-start latency benchmark |
| `bench-warm [rounds]` | `daemon_cmd` | Warm-connection benchmark |
| `context-mcp` | — | Spawn iota-context MCP sidecar (stdio) |
| `fun-mcp` | — | Spawn iota-fun 7-language MCP server (stdio) |
| `skill pull <src>` | `skill_cmd` | Pull remote skill definition |
| `logs/trace` | `observability_cmd` | Query telemetry data |
| `__daemon` | `daemon_cmd` | Internal daemon entry point |

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `daemon_cmd` | Daemon lifecycle, cold/warm/daemon benchmarks |
| `info_cmd` | `check` command — backend info aggregation |
| `observability_cmd` | Logs and trace query commands |
| `run_cmd` | Single-shot `run` command execution |
| `skill_cmd` | Skill pull/cache management |
