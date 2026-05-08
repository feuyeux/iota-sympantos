# iota-sympantos 文档中心

## 📚 文档导航

### 核心文档

| 文档 | 说明 | 大小 |
|------|------|------|
| [architecture.md](architecture.md) | 系统架构设计 | - |
| [code-call-chains.md](code-call-chains.md) | 代码调用链路 | - |
| [observability.md](observability.md) | Observability 系统详解 | - |
| [debugging.md](debugging.md) | 调试指南 | - |

### 记忆系统文档 ⭐

| 文档 | 说明 | 大小 |
|------|------|------|
| [**memory-guide.md**](memory-guide.md) | **记忆系统完整指南** | 34 KB |
| [memory-quick-reference.md](memory-quick-reference.md) | 快速参考卡 | 2.2 KB |

---

## 🎯 记忆系统快速入门

### 三个核心路径

```bash
~/.i6/context/memory.sqlite    # 记忆数据库（6 类桶 + FTS5）
~/.i6/context/events.sqlite    # 事件（observability）
~/.i6/logs/                    # 工程日志（file appender）
```

### 三种排障查看方式

#### 1. 工程日志文件

```bash
ls ~/.i6/logs/
```

工程日志用于排查程序自身行为，不写入 SQLite。可用 `IOTA_LOG` 调整级别，用 `IOTA_LOG_DIR` 调整目录。

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

```bash
# 查看记忆写入日志
iota obs logging logs --event memory.write.result --limit 10

# 查看工具调用事件
iota obs logging tools --tool iota_memory_write --mode pairs

# 查看特定执行的事件流
iota obs logging events <execution-id>
```

#### 需要 grep 时：保存本次 log-events 输出

```bash
# 从本次 log-events 输出查找
iota run --backend codex --log-events "你的 prompt" 2> memory-events.log
grep "memory:write" memory-events.log
grep "memory:inject" memory-events.log

# 从 SQLite 查询
sqlite3 ~/.i6/context/events.sqlite \
  "SELECT execution_id, json_extract(event_json, '$.data.event')
   FROM events WHERE event_type='log'
   AND json_extract(event_json, '$.data.event') LIKE 'memory.%';"
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

### memory-guide.md 包含

1. **系统概述** - 跨后端记忆延续、SQLite + FTS5、6 类召回桶
2. **核心功能** - 记忆生命周期、写入流程、召回流程
3. **数据存储路径** - 核心数据库文件、配置文件、工程日志
4. **记忆类型与分桶** - semantic/episodic/procedural、6 类 facet、置信度阈值
5. **日志系统** - 工程日志、控制台 log-events、EventStore、MCP Sidecar 的职责边界
6. **使用指南** - 写入、搜索、直连测试、自动 episodic、数据库查询、清理
7. **observability 命令** - logging/timing/metrics 完整参考
8. **故障排查** - 8 个常见问题与解决方案
9. **快速参考** - 常用命令速查、关键路径、环境变量
10. **附录** - 数据库 Schema、触发器、索引

### memory-quick-reference.md 包含

- 一分钟速查卡
- 核心路径与常用命令
- grep 记忆日志技巧
- 6 类记忆桶阈值表
- 控制台日志格式
- 故障排查清单

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
