## Kanban 任务看板

### 任务生命周期

```
Triage → Todo → Ready → Running → Done → Archived
                          ↓
                        Blocked → Ready（blockers 全完成后自动解除）
```

### TUI 命令

```bash
# 看板与任务
/kanban boards
/kanban board create <slug> <name>
/kanban view [board]          # ASCII 列视图
/kanban list [status]
/kanban create <title>
/kanban show <id>
/kanban move <id> <status>
/kanban assign <id> <@user>
/kanban comment <id> <text>

# 调度控制
/kanban dispatch [id]         # 手动触发一次 tick
/kanban daemon                # 切换自动调度（30s 间隔）

# 多节点同步
/kanban sync <peer_url> [cursor]
```

别名：`/kb`、`/k`

### CLI 命令

```bash
iota kanban list
iota kanban create <board> <title>
iota kanban move <id> <status>
iota kanban specify <id>      # 用 Hermes 自动生成任务规格
iota kanban decompose <id>    # 自动分解为子任务

# 事件导出 / 导入
iota kanban export events.json
iota kanban import events.json

# 多节点同步服务
iota kanban serve-sync [addr]
iota kanban pull <addr> [cursor]
iota kanban push <addr> [cursor]
```

### Dispatcher

每 30 秒自动执行一次调度循环（`/kanban daemon` 启动）：

1. **health_check** — 检查活跃 worker，处理超时（claim_ttl=15min，heartbeat_timeout=90s）
2. **reclaim_expired** — 回收无 worker 的孤儿 Running 任务
3. **recompute_ready** — blockers 全完成时自动解除 Blocked 任务
4. **spawn_workers** — 为 Ready 任务启动 hermes worker（最多 4 个并发）

每个 worker 获得独立的 Shadow SQLite DB（`~/.i6/kanban/shadows/<task_id>/kanban.db`），包含完整任务上下文。

### Event Sourcing

所有写操作追加到 `events` 表，支持：

```bash
# 从事件日志完整重建状态（多节点同步基础）
store.replay_events(&events)

# 增量导出（cursor = 上次同步的最大 event id）
store.events_since(cursor)
```

### 多节点同步

```bash
# 节点 A 启动 sync server（默认端口 47662）
iota kanban serve-sync 0.0.0.0:7890

# 节点 B 双向同步
/kanban sync http://192.168.1.10:7890
/kanban sync http://192.168.1.10:7890 42   # 增量，cursor=42
```

HTTP API：`GET /events?since=N` · `POST /events` · `GET /health`

## Live Demo

```bash
cargo run --example kanban_live_demo -- "Build a login module"
```

实时输出看板状态，直到任务完成：

```
[init] board created: demo
[init] task  created: #1 -- Build a login module

+ Demo Board +---...---+
| TRIAGE(1) | TODO(0) | ... | DONE(0) |
|   #1 Build |         |     |         |
+---...---+

-- #1 : triage -> todo
-- #1 : todo -> ready
   ~ [scheduler] Task queued for execution.
-- #1 : ready -> running
   > run started  task #1 / 3039423a
   ~ [worker] Step 1/3 complete.
   ...
-- #1 : running -> done

Completed -- final state
Comments (5): scheduler / worker x3 / system
Runs (1):  3039423a -> completed (exit=0)
```

## 