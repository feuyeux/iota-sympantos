# iota observability Documentation Index

## 文档列表

| 文档 | 用途 |
|------|------|
| **OBSERVABILITY_SUMMARY.md** | 命令参考 + 架构概览，从这里开始 |
| **observability-quick-reference.md** | 开发时快速查阅（API / Schema / 常量） |
| **observability-implementation.md** | 深度技术参考（含完整源码） |
| **observability-diagrams.md** | ASCII 架构图与数据流图 |

---

## 命令速查

```bash
# 日志 / 事件流
iota observability logging recent [--limit N]
iota observability logging errors [--limit N]
iota observability logging events <execution-id>
iota observability logging tools [--limit N]
iota observability logging approvals [--limit N]

# 延迟 / timing
iota observability tracing recent [--limit N]
iota observability tracing slow [--limit N]
iota observability tracing breakdown <execution-id>
iota observability tracing summary

# 聚合指标
iota observability metrics [--prometheus]
iota observability metrics tokens
iota observability metrics cache
iota observability metrics sessions
iota observability metrics latency

# 子命令 help
iota observability --help
iota observability logging --help
iota observability tracing --help
iota observability metrics --help
```

---

## 关键文件

```
src/cli.rs           CLI 路由与命令实现
src/event_store.rs   SQLite 存储层 + 所有查询方法
src/runtime_event.rs RuntimeEvent 枚举（10 种类型）
src/acp.rs           AcpPromptTiming 结构体
src/tui/status_bar.rs TUI 状态栏 observability 显示

~/.i6/context/events.sqlite   数据库（4 张表，30 天保留）
```

---

## 关键设计

- **无直接 SQL 暴露** — CLI 全部通过 `EventStore` 的 Rust API 访问数据
- **30 天自动清理** — `EventStore::init()` 时触发
- **Running TTL 1 小时** — 超时执行自动标记 failed
- **Prometheus 采样上限 10,000** — 保持查询 O(1)

---

**Last Updated:** 2026-05-04
