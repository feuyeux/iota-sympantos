# Architecture: Module Layering

iota-sympantos 采用分层模块架构，每层只依赖下层，不允许反向引用。

## Layer Diagram

```
┌─────────────────────────────────────────────────────┐
│                    main.rs                           │  entrypoint
│                  (dispatches to cli)                 │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│                  Presentation Layer                  │
│                                                     │
│   cli.rs              tui.rs             app.rs     │
│   command parsing     interactive REPL   (reserved) │
│   routing & output    lazy backend use              │
└────────┬──────────────────┬─────────────────────────┘
         │                  │
┌────────▼──────────────────▼─────────────────────────┐
│                  Service Layer                       │
│                                                     │
│   agent.rs                    engine.rs             │
│   daemon TCP server           IotaEngine            │
│   internal warm plane      client pool (BTreeMap)│
│   prompt/warm dispatch     lifecycle management  │
└────────┬──────────────────────┬─────────────────────┘
         │                      │
┌────────▼──────────────────────▼─────────────────────┐
│                  Protocol Layer                      │
│                                                     │
│   acp.rs                                            │
│   AcpClient: process spawn, JSON-RPC 2.0 driver    │
│   AcpBackend enum, session lifecycle                │
│   timing instrumentation (AcpPromptTiming)          │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│                  Configuration Layer                 │
│                                                     │
│   config.rs                                         │
│   NimiaConfig / BackendConfig / ModelConfig schema   │
│   nimia.yaml loading, backend_process_env()         │
│   command normalization (npx → npx.cmd on Windows)  │
└─────────────────────────────────────────────────────┘
```

## Module Responsibilities

### main.rs — Entrypoint

唯一职责：声明模块、启动 tokio runtime、调用 `cli::run()`。零业务逻辑。

### Presentation Layer

用户交互层，负责解析输入和格式化输出。

| Module | Role | Depends On |
|---|---|---|
| `cli.rs` | 命令分发（默认 TUI、check/run/bench、--daemon 路由）、参数解析、输出格式化 | agent, engine, config, acp |
| `tui.rs` | 交互式 REPL：读取 `<backend> <prompt>`，lazy-start backend，显示结果 | engine, acp, config |
| `app.rs` | 保留模块：未来 HTTP/WebSocket read-model 接口 | — |

**约束**：Presentation 不直接操作 AcpClient 或 subprocess，通过 engine 抽象访问。

### Service Layer

业务编排层，管理 ACP 客户端生命周期和跨进程复用。

| Module | Role | Depends On |
|---|---|---|
| `engine.rs` | `IotaEngine`：按 `(backend, cwd)` key 缓存 AcpClient；提供 warm/prompt/shutdown API | acp, config |
| `agent.rs` | Daemon TCP 服务器：保持常驻 IotaEngine，接受内部 prompt/warm JSON 请求 | engine, acp, config |

**约束**：engine 不感知 CLI 参数或 TUI 交互；agent 不直接解析用户命令。

### Protocol Layer

ACP 协议实现层，负责与后端子进程的 JSON-RPC 2.0 通信。

| Module | Role | Depends On |
|---|---|---|
| `acp.rs` | `AcpClient`：进程 spawn、initialize、session 管理、prompt 收发、timing 采集 | config (间接，通过参数) |

**约束**：acp 不知道 daemon、TUI、CLI 的存在；只接收参数并驱动协议。

### Configuration Layer

配置读取与环境变量渲染，所有其他层的共享基础设施。

| Module | Role | Depends On |
|---|---|---|
| `config.rs` | YAML schema、config 读取、backend env 渲染、command normalization | — (仅标准库 + serde + dirs) |

**约束**：config 不引用任何上层模块。

## Dependency Rules

```
main → cli
cli  → agent, engine, config, acp, tui
tui  → engine, acp, config
agent → engine, acp, config
engine → acp, config
acp  → config (通过函数参数，非直接 use)
config → (无内部依赖)
```

禁止的依赖方向：

- config/acp 不得 `use crate::engine` / `use crate::cli` / `use crate::agent`
- engine 不得 `use crate::cli` / `use crate::tui` / `use crate::agent`
- agent 不得 `use crate::cli` / `use crate::tui`

## Data Flow

### CLI Single-Shot Prompt

```
cli::run()
  → IotaEngine::new() → engine.prompt_in_cwd_timed()
                            → acp::AcpClient::start() + prompt
```

With `--daemon` / `-d`, the same prompt is routed through `agent::send_prompt()`. If the daemon is unavailable, CLI silently starts the hidden `__daemon` process and retries.

### Daemon Warm

```
cli::ensure_daemon_running() / check --daemon / check --daemon
  → hidden __daemon process if needed
  → agent::send_warm(daemon_addr, request)
      → TCP → agent daemon → engine.warm_enabled_backends_in_cwd()
                                → acp::AcpClient::start() × N (parallel)
  ← DaemonPromptResponse { warmed: N }
```

### TUI Interactive

```
iota (no args) → tui::run()
  → loop {
      read "<backend> <prompt>"
      → engine.prompt_in_cwd(backend, cwd, prompt)
          → ensure_client() [lazy AcpClient::start if first use]
          → client.prompt_with_cwd_timed()
      ← print response
    }
  → engine.shutdown()
```

## Key Design Decisions

1. **IotaEngine 持有所有 AcpClient** — 单一所有者，简化生命周期。daemon 用 `Arc<Mutex<IotaEngine>>` 共享。

2. **Cache key = (backend, cwd)** — 不同工作目录隔离 ACP session 状态，避免文件系统上下文串扰。

3. **Lazy client start** — TUI 和 direct ACP 路径只在首次使用 backend 时 spawn 进程，减少启动时间。

4. **Daemon 自动启动和预热** — `--daemon` / `-d` 静默启动 hidden daemon；`check --daemon` 预热 enabled backends，`run --daemon` 的首次请求预热目标 backend。

5. **Timing 下沉到 protocol layer** — `AcpPromptTiming` 在 acp.rs 中用 `Instant` 采集，engine 补充 `client_started` 标记，cli 负责输出。各层职责清晰。

6. **Config 纯数据** — config.rs 只做 schema 解析和 env 渲染，不持有运行时状态。

## Extension Points

| Extension | Target Module | Pattern |
|---|---|---|
| 新 backend | acp.rs + config.rs | 加 enum variant + parse/command/env arms |
| 新 CLI 命令 | cli.rs | 加 match arm + handler function |
| HTTP API | app.rs | 接入 IotaEngine，类似 agent.rs 但用 HTTP |
| 并行 prompt | engine.rs | 替换 Mutex 为 per-backend lock 或 actor |
| 持久化 session | acp.rs | session resume protocol (ACP spec 扩展) |
