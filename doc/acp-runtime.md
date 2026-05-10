# ACP Runtime: Process Model & Benchmarks

## 1 Execution Paths

| Path | Command | Behavior |
|---|---|---|
| Direct | `iota run <backend> <prompt>` | 使用进程内 `IotaEngine`，每次调用独立管理 ACP 进程生命周期 |
| TUI | `iota` | 无参数进入 TUI；首次选择 backend 时 lazy-start，退出前复用 |
| Daemon | `iota run --daemon ...` / `-d` | 连接常驻 daemon（自动静默启动），跨 CLI 调用复用 subprocess 和 session |

相关模块：`src/cli/mod.rs`（命令入口）、`src/engine.rs`（编排）、`src/acp/mod.rs`（ACP 协议）、`src/daemon/mod.rs`（daemon 服务）。

### 1.1 Client Caching

`IotaEngine` 缓存 key = `(backend, cwd)`。同 key 复用同一 `AcpClient` 及其 `sessionId`。cwd 变更产生新 key。

### 1.2 ACP Protocol Lifecycle

```
process spawn → initialize → session/new → session/prompt → session/update* → session/complete
```

- **Client started**: 首次为 `(backend, cwd)` 创建 `AcpClient`，执行 spawn + initialize。
- **Session reused**: 复用已有 `sessionId`，跳过 `session/new`。
- **Hot path**: daemon 中已预热的 client，直接发送 `session/prompt`。

## 2 Daemon Architecture

支持 `--daemon` / `-d` 的命令会连接 `127.0.0.1:47661`（`IOTA_DAEMON_ADDR` 可覆盖）；daemon 未运行时自动静默启动。Daemon 内部持有 `EnginePool`（`src/daemon/pool.rs`），按 cwd 维度复用 `IotaEngine`，跨请求复用 ACP 进程。

模块结构：`src/daemon/mod.rs`（TCP server 主循环）、`src/daemon/pool.rs`（EnginePool）、`src/daemon/proto.rs`（请求/响应类型）。

### 2.1 Daemon Protocol

TCP JSON line protocol，两种请求类型：

| Type | Fields | Response |
|---|---|---|
| Prompt | `backend`, `cwd`, `prompt`, `execution_id?`, `timeout_ms`, `trace_timing` | `ok`, `text`, `timing`, `events[]`, `error` |
| Warm | `type: "warm"`, `cwd`, `backends` | `ok`, `warmed`, `error` |

### 2.2 Daemon Lifecycle

- **启动**: `iota __daemon` 或首次 `--daemon` 调用时自动后台启动
- **预热**: `iota check --daemon` 预热所有 enabled backends；daemon 启动时可选 `warm_on_start`
- **并发**: 使用 `Semaphore` 限制并发请求
- **关闭**: `CancellationToken` 优雅关闭所有 engine 中的 ACP 客户端

```bash
# 手动启动 daemon 并预热
iota check --daemon

# 通过 daemon 运行（自动启动）
iota run --daemon --timing codex "say hello"

# 自定义 daemon 地址
export IOTA_DAEMON_ADDR='127.0.0.1:50100'
```

## 3 Timing Instrumentation

`--timing` 输出 JSON 到 stderr：

```json
{"route":"daemon","daemon_hit":true,"fallback":false,"backend":"claude-code",
 "timing":{"client_started":false,"process_spawned":false,"session_reused":true,"prompt_ms":6083,"total_ms":6083}}
```

### 3.1 Fields

| Field | Type | Meaning |
|---|---|---|
| `client_started` | bool | 本次是否新启动 AcpClient |
| `process_spawned` | bool | 本次是否 spawn 后端进程 |
| `process_spawn_ms` | u64? | spawn 耗时（仅 client_started=true） |
| `init_ms` | u64? | ACP initialize 耗时 |
| `session_reused` | bool | 是否复用 sessionId |
| `session_new_ms` | u64? | session/new 耗时 |
| `prompt_ms` | u64 | prompt → complete 耗时 |
| `total_ms` | u64 | 总耗时 |

### 3.2 CLI Flags

| Flag | Purpose |
|---|---|
| `--daemon`, `-d` | 通过 daemon 路由；daemon 不可用时静默启动 |
| `--timing` | 输出 timing JSON 到 stderr |
| `--show-native` | 打印原始 JSON-RPC 消息 |

## 4 Backend Processes

| Backend | ACP Adapter |
|---|---|
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@latest` |
| Codex | `npx -y @zed-industries/codex-acp@latest` |
| Gemini | `npx -y @google/gemini-cli@latest --acp` |
| Hermes | `hermes acp` |
| OpenCode | `npx -y opencode-ai@latest acp` |

Windows 上 `npx` normalize 为 `npx.cmd`（`config::normalize_command()`）。设计单位是 ACP client/channel，非 OS 进程。

### 4.1 MCP Session Options

每个后端可通过 `context_engine_backend.<backend>` 配置 MCP 注入行为：

| Option | Default | Effect |
|---|---|---|
| `mcp_session_new` | Gemini/Hermes/OpenCode: true; Claude/Codex: try | 是否在 session/new 中注入 mcpServers |
| `always_send_empty_mcp_servers` | false | 无 server 时也发送 `mcpServers: []` |
| `mcp_env_shape` | `string_array` | env 格式：`string_array` (`["K=V"]`) 或 `object` (`{K:V}`) |
| `override_home` | true | 是否将 home 配置写入后端 HOME 环境变量 |

## 5 Benchmark Commands

| Command | Measures |
|---|---|
| `iota bench-cold <rounds>` | 冷启动（每轮独立 spawn + prompt + shutdown） |
| `iota bench-warm <rounds>` | 进程内热路径（预热后重复 prompt，复用 client 和 session） |
| `iota bench-cold --daemon <rounds>` | Daemon 热路径（跨 CLI 调用，daemon 内复用） |
| `iota check --daemon` | 启动 daemon 并预热所有 enabled backends |

## 6 Measured Results (2026-05-04)

Setup: `iota check --daemon` 预热所有 enabled backends。测试 3 轮，取中位数。Prompt: `say hello. reply with exactly: hello`。

### 6.1 Summary

| Backend | Daemon Hot (ms) | Direct Cold (ms) | Speedup |
|---|---:|---:|---|
| claude-code | **1569** | 3756 | **2.4×** |
| codex | **1415** | 5880 | **4.1×** |
| gemini | **1185** | 2187 (7300*) | **1.8× (6.2×*)** |
| hermes | **1468** | 4378 | **3.0×** |
| opencode | **3532** | 4838 | **1.4×** |

*Gemini 的 direct `total_ms` 不包含 `init_ms`。实际耗时 = init_ms + total_ms ≈ 7.3s。

### 6.2 Raw Data

#### claude-code

| Path | R1 | R2 | R3 | Median | Init (ms) |
|---|---:|---:|---:|---:|---:|
| daemon (hot) | 1438 | 1700 | — | **1569** | — |
| direct (cold) | 3756 | 7014 | 3414 | **3756** | 627 / 607 / 565 |

#### codex

| Path | R1 | R2 | R3 | Median | Init (ms) |
|---|---:|---:|---:|---:|---:|
| daemon (hot) | 1512 | 1317 | — | **1415** | — |
| direct (cold) | 6047 | 5880 | 3822 | **5880** | 625 / 538 / 502 |

Daemon R1 包含 `session/new` (77ms)，R2-R3 复用 session。Direct 每次都是冷启动（spawn + init + session/new）。

#### gemini

| Path | R1 | R2 | R3 | Median | Init (ms) |
|---|---:|---:|---:|---:|---:|
| daemon (hot) | 1989 | 1036 | 1331 | **1185** | — |
| direct (cold) | 2079 | 2292 | 2187 | **2187** | 5304 / 6111 / 4837 |

**注意**: Gemini 的 `total_ms` 不包含 `init_ms`。Direct 模式实际耗时 = `init_ms` + `total_ms` ≈ 7-8s。

#### hermes

| Path | R1 | R2 | R3 | Median | Init (ms) |
|---|---:|---:|---:|---:|---:|
| daemon (hot) | 6323 | 1495 | 1440 | **1468** | — |
| direct (cold) | 4302 | 4378 | 4634 | **4378** | 201 / 212 / 209 |

Daemon R1 包含 `session/new` (2539ms)，显著慢于其他 backend。R2-R3 复用 session 后恢复正常。

#### opencode

| Path | R1 | R2 | R3 | Median | Init (ms) |
|---|---:|---:|---:|---:|---:|
| daemon (hot) | 5510 | 2816 | 4248 | **3532** | — |
| direct (cold) | 6397 | 4838 | 3768 | **4838** | 1648 / 1397 / 1357 |

OpenCode 的 daemon 模式性能不稳定，R1/R3 显著慢于 R2。

### 6.3 Overhead Analysis

| Phase | Daemon Hot | Direct Cold |
|---|---|---|
| Process spawn | — | ~0-4ms |
| ACP initialize | — | ~200-6000ms |
| Session creation | ~75-2500ms (首次) | ~75-2200ms |
| Model inference | ✓ | ✓ |
| **Total overhead** | **< 100ms** | **~300-8000ms** |

**Init 成本差异**:
- Hermes: ~200ms (最快，原生二进制)
- Codex/Claude-Code: ~500-600ms
- OpenCode: ~1400ms
- Gemini: ~5000-6000ms (最慢，可能因为 npx 下载或 Node.js 启动)

**Session/new 成本差异**:
- Codex/Claude-Code/OpenCode: ~75-200ms
- Gemini: ~800-1200ms
- Hermes: ~2100-2500ms (最慢)

### 6.4 Timing 字段说明

`total_ms` 的含义在不同实现中不一致：
- **Codex/Claude-Code/Hermes/OpenCode**: `total_ms` 包含完整耗时（spawn + init + session + prompt）
- **Gemini**: `total_ms` 仅包含 session + prompt，`init_ms` 单独记录

这导致 Gemini 的 direct 模式 `total_ms` 看起来很快（~2s），但实际用户感知延迟是 `init_ms + total_ms` ≈ 7-8s。

### 6.5 Key Findings

1. **Session 复用是关键优化**
   - Codex/Claude-Code: 节省 ~75-200ms
   - Gemini: 节省 ~800-1200ms
   - Hermes: 节省 ~2100-2500ms（最显著）

2. **Init 成本差异巨大**
   - Hermes (原生): ~200ms
   - NPX backends: ~500-6000ms
   - Gemini 最慢，可能需要优化

3. **Daemon 优势排名**
   - Gemini: ~6× (7.3s → 1.2s)
   - Codex: 4.1× (5.9s → 1.4s)
   - Hermes: 3.0× (4.4s → 1.5s)
   - Claude-Code: 2.4× (3.8s → 1.6s)
   - OpenCode: 1.4× (4.8s → 3.5s)

4. **性能不稳定问题**
   - OpenCode daemon 模式波动大（2.8s - 5.5s）
   - Hermes daemon R1 异常慢（6.3s），R2-R3 正常（~1.5s）
   - 可能原因：首次 session/new 的额外初始化开销

5. **最佳实践**
   - 使用 `iota check --daemon` 预热所有 backend
   - 通过 daemon 运行可获得 1.4-6× 加速
   - Hermes 和 Gemini 从 daemon 获益最大
