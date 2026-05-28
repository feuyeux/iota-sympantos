# iota-sympantos 文档索引

iota-sympantos 是一个 Rust workspace，包含 CLI/TUI、核心运行时、Kanban 模块和 Tauri 桌面端。配置统一来自 `~/.i6/nimia.yaml`。

| 文档 | 说明 |
| :--- | :--- |
| [iota_book.md](iota_book.md) | **《iota 技术指南》** —— 面向程序员与 AI 从业者的系统化核心设计与实现指南 |
| [architecture.md](architecture.md) | 分层架构、crate 职责、后端、配置、扩展点 |
| [code-call-chains.md](code-call-chains.md) | CLI、TUI、daemon、desktop、ACP、MCP、memory、kanban 调用链 |
| [command.md](command.md) | CLI 命令和 TUI slash command |
| [observability.md](observability.md) | RuntimeEvent、token usage、metrics、logs/trace 边界 |
| [debugging.md](debugging.md) | 本地调试、日志、常见问题 |
| [desktop-mvp-acceptance.md](desktop-mvp-acceptance.md) | 桌面端 MVP 验收清单 |
| [docker.md](docker.md) | Docker daemon 和可观测性栈 |

## 关键路径

```bash
~/.i6/nimia.yaml             # 唯一配置来源，包含后端模型和凭据
~/.i6/context/memory.sqlite  # memory store
~/.i6/context/events.sqlite  # execution lifecycle + token observability
~/.i6/context/sessions.sqlite
~/.i6/context/approvals.sqlite
~/.i6/kanban/iota.db         # kanban event-sourced store
~/.i6/logs/                  # 本地工程日志
```

## 常用命令

```bash
iota
iota check [--daemon|-d]
iota run [backend] [options] <prompt>
iota bench <cold|warm> [rounds] [--daemon|-d]
iota mcp <context|fun>
iota kanban create-board <slug> <name>
iota observability tokens recent --limit 20
cd crates/iota-desktop && npm run dev
```
