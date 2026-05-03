# iota-sympantos TUI / ACP Process Model

本文档记录当前 Rust 版 `iota` 的进程模型、ACP 复用策略和 2026-05-04 的本机延迟基准。

## Runtime Summary

当前实现有三条运行路径：

| Path | Command | Process / reuse model |
|---|---|---|
| Single CLI | `iota acp <backend> <prompt>` | 优先连接 `iota daemon`；daemon 不可用时回退到本次 CLI 进程内 `IotaEngine`。|
| TUI | `iota tui` | TUI 启动后不再预热所有后端；用户第一次选择某 backend 时 lazy-start 对应 ACP client，并在 TUI 退出前复用。|
| Daemon | `iota daemon` | 在 `127.0.0.1:47661` 保持一个常驻 `IotaEngine`，跨 CLI 调用复用 backend+cwd 对应的 ACP subprocess 和 session。|

`IotaEngine` 的缓存 key 是 `backend + cwd`。同一个 key 会复用同一个 `AcpClient`；`AcpClient` 首次 prompt 创建 ACP `sessionId`，后续 prompt 复用该 session。cwd 变化时会使用新的 cache key，避免不同工作目录共享同一个 ACP session。

## Benchmark Commands

| Command | Meaning |
|---|---|
| `iota bench-cold <rounds>` | 每个样本独立启动 backend、发送 prompt、回收，用于测冷启动。|
| `iota bench-warm <rounds>` | 先预热 enabled backends，再在同一批 warmed clients 上重复发送 prompt，用于测进程内热路径。|
| `iota daemon` + `iota acp ...` | 常驻 engine 场景，用于测跨 CLI 调用复用 ACP client/session 的路径。|

## Logical Backend Processes

| Backend | ACP startup command |
|---|---|
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@latest` |
| Codex | `npx -y @zed-industries/codex-acp@latest` |
| Gemini | `npx -y @google/gemini-cli@latest --acp` |
| Hermes | `hermes acp` |
| OpenCode | `npx -y opencode-ai@latest acp` |

On Windows, each logical backend channel may expand into multiple OS processes because `npx`, `cmd.exe`, `node.exe`, adapter binaries, and Python wrappers can spawn children. Treat process count as platform-dependent; the stable design unit is the ACP client/channel.

## 2026-05-04 ACP Daemon vs Direct CLI Benchmark

Run time: 2026-05-04 01:53:02 +08:00

Prompt: `say hello. Reply with exactly: hello`

Protocol:

- Each path ran 5 warmup samples followed by 5 measured samples.
- `iota-acp` was measured through `iota daemon` on `127.0.0.1:47661`; `iota acp` connected to the daemon and reused backend+cwd clients/sessions.
- Direct commands used each backend's non-interactive CLI path:
  - Claude: `claude --settings C:\Users\feuye\.claude\settings-minimax.json --print --bare --permission-mode auto`
  - Codex: `codex exec`
  - Gemini: `gemini --prompt`
  - Hermes: `hermes --oneshot`
  - OpenCode: `opencode run`
- Raw samples: `benchmark-latest.json`; summary: `benchmark-latest-summary.json`.
- Secrets and stdout bodies are not included in this document.

### Summary

| Backend | iota-acp ok | iota-acp p50 ms | iota-acp p90 ms | direct ok | direct p50 ms | direct p90 ms | p50 delta ms | p50 ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| claude-code | 5/5 | 9082 | 9743 | 5/5 | 2754 | 4994 | 6328 | 3.30 |
| codex | 5/5 | 12701 | 13629 | 5/5 | 3368 | 3649 | 9333 | 3.77 |
| gemini | 5/5 | 16943 | 17236 | 5/5 | 11069 | 61483 | 5874 | 1.53 |
| hermes | 5/5 | 9837 | 10355 | 5/5 | 7747 | 7785 | 2090 | 1.27 |
| opencode | 5/5 | 10176 | 10212 | 5/5 | 6099 | 6207 | 4077 | 1.67 |

### Measured Samples

| Backend | Path | Round latencies ms |
|---|---|---|
| claude-code | iota-acp | 12479, 8482, 7475, 9082, 9743 |
| claude-code | direct | 2570, 2554, 2754, 4994, 10903 |
| codex | iota-acp | 16832, 11585, 11585, 12701, 13629 |
| codex | direct | 7206, 3117, 3368, 3146, 3649 |
| gemini | iota-acp | 16413, 17236, 16943, 24384, 12329 |
| gemini | direct | 10762, 11069, 10651, 71942, 61483 |
| hermes | iota-acp | 9837, 10355, 10444, 9540, 9332 |
| hermes | direct | 7747, 7447, 7746, 7785, 7820 |
| opencode | iota-acp | 10212, 10020, 10176, 10604, 9945 |
| opencode | direct | 5705, 6207, 5943, 6784, 6099 |

## Interpretation

The current daemon path successfully reuses backend ACP subprocesses and ACP sessions across `iota acp` invocations, but the measured p50 latency is still higher than each backend's direct non-interactive CLI path on this machine.

Likely causes to investigate next:

1. ACP adapter overhead: `iota-acp` still goes through the ACP adapter layer and JSON-RPC event mapping, while direct commands use each backend's optimized one-shot path.
2. Backend behavior differences: direct CLIs may use simpler prompt-only modes, while ACP adapters may initialize richer session/tool capabilities.
3. Daemon serialization: the current daemon protects one `IotaEngine` with a mutex, so requests are intentionally serialized. This is safe for shared ACP stdout/stdin, but not optimized for parallel throughput.
4. Session creation details: `AcpClient` now reuses `sessionId`, but backend-specific adapters may still perform per-prompt setup internally.
5. Gemini direct outliers: Gemini direct p90 is much higher than p50 in this run, so larger samples are needed before drawing strong tail-latency conclusions.

Optimization target remains: bring `iota-acp` p50 close to the direct CLI p50 for simple prompts, while preserving the daemon/TUI benefits of persistent ACP clients, unified backend selection, and future app/agent integration.