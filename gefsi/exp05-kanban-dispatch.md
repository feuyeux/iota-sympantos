# iota-sympantos 实验 5：Kanban DB 劫持 — iota 投影 + hermes 无感执行 + 结果回收

| 字段 | 值 |
| :------| :-----|
| 实验代号 | exp05-kanban-dispatch |
| 执行日期 | 2026-05-19 |
| 核心命题 | iota 的 DB 是唯一真相源；通过 shadow DB 劫持 hermes 的 kanban 读写 |
| 实验对象 | `iota kanban dispatch` → shadow 投影 → hermes `-z` 无感执行 → 结果回收 |
| 实现位置 | `src/kanban/worker.rs`, `src/kanban/shadow.rs`, `src/kanban/dispatcher.rs`, `src/cli/kanban_cmd.rs` |
| 关联改动 | shadow schema 对齐 hermes、worker spawn 切换为 `-z` 模式 |

---

## 一、实验目标

验证 **iota DB 劫持 hermes 读写** 的完整闭环：

- **唯一真相源**：iota 的 `~/.i6/kanban/iota.db` 是任务数据的唯一 owner
- **劫持机制**：shadow DB 是 iota 投影给 hermes 的"假" kanban DB（hermes 兼容 schema）
- **读劫持**：hermes `kanban_show` 读到的数据来自 iota 的 materialize 投影
- **写劫持**：hermes `kanban_complete` 写入 shadow DB → ShadowWatcher 回收到 iota 主 store

```text
iota kanban dispatch <task_id>
    → iota 从主 store 读取 task
    → Materializer 投影到 shadow DB（hermes schema，HERMES_KANBAN_DB 指向此处）
    → spawn hermes --yolo -z "work kanban task <id>"
    → hermes 收到 KANBAN_GUIDANCE 系统提示 + HERMES_KANBAN_TASK 环境变量
    → hermes 调用 kanban_show(task_id) → 读 shadow DB → 拿到 iota 投影的任务内容
    → hermes 执行任务（"生成宠物"）
    → hermes 调用 kanban_complete(summary=..., metadata=...) → 写 shadow DB
    → ShadowWatcher 检测到 task_events 中的 complete 事件
    → iota 回收结果，更新主 store 中 task 状态为 done
```

### 设计约束

| 约束 | 说明 |
|------|------|
| hermes 无感知 | hermes 不知道自己被劫持，它认为在操作正常的 kanban DB |
| iota 掌控生命周期 | shadow DB 由 iota 创建、监视、回收、清理 |
| 数据流单向 | iota→shadow（投影） + shadow→iota（回收），hermes 不直接碰 iota DB |

### 核心验证点

| # | 验证点 | 判定标准 |
|---|--------|----------|
| V1 | hermes 进程正常启动 | exit code = 0，stderr 无 traceback |
| V2 | 读劫持生效 | hermes stdout 包含与"生成宠物"相关的输出（证明 kanban_show 读到了投影数据） |
| V3 | 写劫持生效 | shadow DB 的 `task_events` 表有 `kind='completed'` 记录 |
| V4 | 结构化结果回收 | iota 从 shadow DB 的 payload 中提取 summary |
| V5 | 主 store 状态同步 | iota 主 store 中 task 状态变为 `done` |
| V6 | 端到端耗时合理 | < 120s（含 LLM 推理） |

---

## 二、前置条件

| 条件 | 说明 |
|------|------|
| hermes 可用 | `hermes --version` 返回 ≥ 0.14.0 |
| 推理 provider 配置 | `~/.i6/nimia.yaml` 中 hermes 段有有效的 provider + api_key |
| kanban store 存在 | `~/.i6/kanban/iota.db` |
| kanban 工具集已启用 | hermes config 中 `cli` 平台工具包含 kanban |

---

## 三、实验过程

### 3.1 准备阶段

```powershell
# 1. 创建 board（如已有可跳过）
iota kanban create-board pets "Pet Generator"

# 2. 创建 task
iota kanban create-task <board-id> "生成宠物"

# 3. 状态推进到 ready
iota kanban move <task-id> todo
iota kanban move <task-id> ready
```

### 3.2 执行阶段

```powershell
# 执行 dispatch（脚本封装见 gefsi/exp05-dispatch.ps1）
iota kanban dispatch <task-id> --timeout 120
```

### 3.3 校验阶段

```powershell
# V1: 检查 hermes 退出码 + stderr
Get-Content ~/.i6/kanban/shadows/<task-id>.stderr.log

# V2: 检查 hermes stdout（oneshot 模式的最终输出）
Get-Content ~/.i6/kanban/shadows/<task-id>.stdout.log

# V3 + V4: 检查 shadow DB 的 task_events
sqlite3 ~/.i6/kanban/shadows/<task-id>/kanban.db "SELECT kind, payload FROM task_events"

# V5: 检查主 store 状态
iota kanban show <task-id>
```

---

## 四、预期结果

### 4.1 成功路径

```
$ iota kanban dispatch 20 --timeout 120
Dispatching task #20: 生成宠物 ...
  [dispatch] worker spawned (ready -> running)
  [dispatch] worker finished: done
Task #20 dispatch complete: done
```

### 4.2 shadow DB task_events 预期内容

```sql
-- hermes kanban_complete 写入：
kind       | payload
-----------+----------------------------------------------------------
completed  | {"summary": "...", "metadata": {"pet_name": "...", ...}}
```

### 4.3 stdout.log 预期内容

hermes `-z` 模式只输出最终响应文本，预期为生成的宠物描述。

---

## 五、已知风险与降级方案

| 风险 | 影响 | 降级 |
|------|------|------|
| hermes kanban 工具集未加载 | kanban_show 不可用，agent 无法读取任务 | 检查 `hermes tools` 输出，确认 kanban 在 cli 平台启用 |
| KANBAN_GUIDANCE 未注入 | agent 不知道要调用 kanban_complete | 确认 HERMES_KANBAN_TASK 环境变量传递正确 |
| 推理 provider 超时 | hermes 卡住直到 dispatch timeout | 检查 nimia.yaml 中 hermes 的 provider 配置 |
| shadow DB schema 不兼容 | kanban_show 查询失败 | 对比 shadow schema 和 hermes kanban_db.py 中的 SCHEMA |

---

## 六、实验结果

> （待执行后回填）

### 6.1 执行记录

```
执行时间：
hermes 版本：
推理 provider：
模型：
```

### 6.2 各验证点结果

| # | 验证点 | 结果 | 备注 |
|---|--------|------|------|
| V1 | hermes 正常启动 | | |
| V2 | 读取到任务内容 | | |
| V3 | kanban_complete 调用 | | |
| V4 | 结构化结果写回 | | |
| V5 | iota 状态同步 | | |
| V6 | 耗时 | | |

### 6.3 产物

```
logs/exp05-stdout.txt    — hermes stdout
logs/exp05-stderr.txt    — hermes stderr
logs/exp05-shadow.sql    — shadow DB dump
```

---

## 七、结论

> （待回填）
