# iota-sympantos 文档索引

| 文档 | 说明 |
|---|---|
| [architecture.md](architecture.md) | 分层架构、模块职责、扩展点 |
| [code-call-chains.md](code-call-chains.md) | CLI/TUI/daemon/ACP/Context Fabric 调用链 |
| [observability.md](observability.md) | 当前 logs、trace、RuntimeEvent、metrics、CacheStore 边界 |
| [debugging.md](debugging.md) | 调试环境变量、日志和常见排查方式 |

## 关键本地路径

```bash
~/.i6/nimia.yaml             # 唯一配置来源
~/.i6/context/memory.sqlite  # memory store
~/.i6/context/events.sqlite  # execution lifecycle tables
~/.i6/context/sessions.sqlite
~/.i6/context/approvals.sqlite
~/.i6/logs/                  # 工程日志
```

## 常用观测方式

```bash
iota run --timing <backend> "prompt"
iota run --log-events <backend> "prompt"
iota logs <execution_id>
iota trace <trace_id>
```

当前没有 `iota observability` 子命令；本地 SQLite 只保留 execution lifecycle、memory、session 和 approval 等 store。
