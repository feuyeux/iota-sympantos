# iota-sympantos 文档中心

## 📚 文档导航

### 核心文档

| 文档 | 说明 | 大小 |
|------|------|------|
| [architecture.md](architecture.md) | 系统架构设计 | - |
| [code-call-chains.md](code-call-chains.md) | 代码调用链路 | - |
| [observability.md](observability.md) | Observability 系统详解 | - |
| [debugging.md](debugging.md) | 调试指南 | - |

## 🎯 记忆系统快速入门

### 三个核心路径

```bash
~/.i6/context/memory.sqlite    # 记忆数据库（6 类桶 + FTS5）
~/.i6/context/events.sqlite    # CacheStore：execution replay/dedupe，不是观测事件库
~/.i6/context/sessions.sqlite  # SessionLedger：session/backend session/turn/handoff
~/.i6/context/approvals.sqlite # ApprovalStore：权限请求和决策
```

### 三种排障查看方式

#### 1. stderr tracing 日志

```bash
IOTA_LOG=iota_sympantos=debug iota run codex "你的 prompt"
```

当前实现没有 `~/.i6/logs` file appender。`IOTA_LOG` / `RUST_LOG` 控制 stderr tracing 过滤规则。

#### 2. 控制台 log-events（单次运行）

```bash
iota run --backend codex --log-events "你的 prompt"
```

输出：

```
[memory:write] id=... type=semantic facet=identity confidence=0.95
[memory:write:result] id=... ok=true memory_id=a2528017-...
[memory:inject] {"identity":1,"preference":1,...}
```

#### 3. observability 命令（事件）

旧的 `iota observability ...` / `iota obs ...` 命令组已移除。当前持久化观测依赖 OpenTelemetry 和 Docker observability stack：

```bash
# 启动 Jaeger / Prometheus / Loki / Grafana
cd docker/observability
docker compose up -d

# 查询 Loki 日志
iota logs <execution-id>

# 查询 Jaeger trace
iota trace <trace-id>
```

无 Docker 时，logs/traces/metrics 只会尝试发送到 `OTEL_EXPORTER_OTLP_ENDPOINT`，默认 `http://localhost:4317`；没有 collector 时不会写入本地观测数据库。

#### 需要 grep 时：保存本次 log-events 输出

```bash
# 从本次 log-events 输出查找
iota run --backend codex --log-events "你的 prompt" 2> memory-events.log
grep "memory:write" memory-events.log
grep "memory:inject" memory-events.log

# 从本地 CacheStore 只能查看 replay/dedupe cache，不包含完整 memory log event
sqlite3 ~/.i6/context/events.sqlite \
  "SELECT execution_id, backend, status, datetime(started_at, 'unixepoch')
   FROM cache_executions ORDER BY started_at DESC LIMIT 10;"
```

### 三个常用查询

```bash
# 1. 查看最近的记忆
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT substr(id,1,8), type, facet, confidence, substr(content,1,60)
   FROM memory ORDER BY updated_at DESC LIMIT 10;"

# 2. 全文搜索记忆
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT substr(id,1,8), type, snippet(memory_fts, 0, '[', ']', '...', 20)
   FROM memory_fts WHERE memory_fts MATCH 'keyword' LIMIT 5;"

# 3. 查看记忆分布
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT type, facet, COUNT(*) FROM memory GROUP BY type, facet;"
```

---

## 📖 详细内容

当前仓库没有单独的 `memory-guide.md` / `memory-quick-reference.md`。记忆系统说明分布在：

- [architecture.md](architecture.md)：模块职责、memory taxonomy、Store 路径。
- [code-call-chains.md](code-call-chains.md)：memory recall/write、MCP sidecar、engine-run skill 调用链。
- [observability.md](observability.md)：当前 OTel 观测路径，以及 `events.sqlite` 不再是观测事件库的说明。
- [gefsi/exp01-memory.md](../gefsi/exp01-memory.md)：历史实验记录，部分命令反映旧 EventStore/observability CLI 实现。

---

## 🧪 实验报告

| 报告 | 说明 |
|------|------|
| [gefsi/exp01-memory.md](../gefsi/exp01-memory.md) | 跨后端记忆延续验证实验 |
| [gefsi/exp02-skill-fun.md](../gefsi/exp02-skill-fun.md) | Skill + iota-fun 多语言执行实验 |

---

## 🔗 相关链接

- [项目 README](../README.md)
- [AGENTS.md](../AGENTS.md)
- [实验报告](../gefsi/)
