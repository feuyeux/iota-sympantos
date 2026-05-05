# iota-sympantos 断点调试指南

## 前置要求

1. **VS Code 扩展**：安装 [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb)（扩展 ID：`vadimcn.vscode-lldb`）
2. **Rust 工具链**：确保 `rustc`、`cargo` 已安装且版本 ≥ 1.95.0
3. **配置文件**：确保 `~/.i6/nimia.yaml` 已正确配置（含后端凭据）

## 调试配置一览

| 配置名称 | 说明 | 启动参数 |
|----------|------|----------|
| Debug TUI (默认模式) | 交互式 TUI 模式 | 无参数 |
| Debug Run (单次执行) | 单次 prompt 执行 | `run <backend> <prompt>` |
| Debug Run with Daemon | 经 daemon 路由执行 | `run --daemon <backend> <prompt>` |
| Debug Check | 输出后端 JSON 信息 | `check` |
| Debug Context MCP Sidecar | 启动 iota-context MCP | `context-mcp` |
| Debug Fun MCP Server | 启动 iota-fun MCP | `fun-mcp` |
| Debug Bench Cold | 冷启动基准测试 | `bench-cold 3` |
| Debug Daemon (内部) | 启动内部 daemon 进程 | `__daemon` |

## 使用方法

### 1. 设置断点

在 VS Code 编辑器中点击行号左侧设置断点（红色圆点），常见调试入口：

- `src/main.rs:16` — 程序入口
- `src/cli/mod.rs` — 命令分发
- `src/engine.rs` — ACP 运行时编排
- `src/acp/mod.rs` — ACP 协议交互
- `src/tui.rs` — TUI 主循环

### 2. 启动调试

- 按 `F5` 或点击 Run and Debug 面板中的绿色三角
- 从下拉列表选择对应配置
- "Debug Run" 配置会弹出输入框让你选择后端和输入 prompt

### 3. 调试控制

| 快捷键 | 操作 |
|--------|------|
| `F5` | 继续 / 启动调试 |
| `F10` | 单步跳过 (Step Over) |
| `F11` | 单步进入 (Step Into) |
| `Shift+F11` | 单步跳出 (Step Out) |
| `Shift+F5` | 停止调试 |
| `Cmd+Shift+F5` | 重启调试 |

### 4. 查看变量

调试暂停时可在以下面板查看状态：
- **Variables** — 当前作用域的局部变量
- **Watch** — 自定义监视表达式
- **Call Stack** — 调用栈
- **Debug Console** — 执行 LLDB 表达式（如 `p variable_name`）

## 环境变量

调试配置默认设置：

```
RUST_LOG=debug        # 启用 tracing debug 级别日志
RUST_BACKTRACE=1      # 启用完整 backtrace
```

如需过滤特定模块日志，修改 `RUST_LOG`：

```
RUST_LOG=iota_sympantos::acp=trace,iota_sympantos::engine=debug
```

## TUI 调试注意事项

TUI 模式使用 `crossterm` 占据终端，断点暂停时终端可能处于 raw mode。建议：

1. 优先在 TUI 初始化前（`cli/mod.rs` 命令分发阶段）设置断点
2. 调试 TUI 内部逻辑时，在事件处理函数中设置条件断点
3. 如果终端状态异常，调试停止后在终端执行 `reset` 恢复

## 条件断点

右键断点 → Edit Breakpoint，添加条件表达式：

```rust
// 仅在特定后端时断住
backend == AcpBackend::Claude

// 仅在包含特定文本时断住
prompt.contains("test")
```

## 日志断点 (Logpoint)

右键行号 → Add Logpoint，输入日志模板（不暂停执行）：

```
Received event: {event:?}
```

## 常见问题

### CodeLLDB 无法启动

确认已安装 CodeLLDB 扩展，且 macOS 上已授予调试权限（System Preferences → Privacy & Security → Developer Tools）。

### 断点不命中

1. 确认编译使用 debug profile（launch.json 中的 `cargo build` 无 `--release`）
2. 检查代码是否被优化内联（debug 模式默认 `opt-level = 0`）
3. async 函数内部断点可能需要在 `.await` 后的行设置

### 终端被 TUI 占用

调试 TUI 时使用 "integrated" terminal。如果需要同时查看 stdout 输出，考虑使用 "Debug Check" 或 "Debug Run" 配置。

### ACP 子进程调试

ACP 后端是外部进程（npx 启动），无法直接断点。调试 ACP 交互请在以下位置设置断点：

- `src/acp/wire.rs` — 读取/解析 JSON-RPC 消息
- `src/acp/mod.rs` — 发送请求和处理响应
- `src/acp/session.rs` — session 参数构建
