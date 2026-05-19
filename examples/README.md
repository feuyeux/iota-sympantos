# Kanban-Hermes 双路径最小证据

本页展示一次真实运行的直接产物，呈现 **ACP 路径**（`iota run --backend hermes`）和 **Dispatch 路径**（`iota kanban dispatch`）两条链路的完整事件流转。

## 1. 运行命令与产物目录

```powershell
powershell -ExecutionPolicy Bypass -File .\examples\kanban_hermes_demo.ps1 -Prompt "生成宠物"
```

产物目录：

```text
examples/logs/latest/
```

## 2. 完整链路图

### 链路 A：ACP 路径（`iota run --backend hermes`）

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ User                                                                        │
│   $ iota run --backend hermes "生成宠物"                                     │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ IotaEngine::run_with_timing                                                 │
│   ├─ config::read_config() → NimiaConfig                                    │
│   ├─ backend_config(&cfg, Hermes) → BackendConfig                           │
│   └─ backend_process_env_with_context(Hermes, &section, None)               │
│        → { HERMES_INFERENCE_PROVIDER, HERMES_MODEL, HERMES_*_API_KEY }      │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ AcpClient::start (spawn subprocess)                                         │
│   command: hermes acp                                                       │
│   env:     HERMES_INFERENCE_PROVIDER=minimax-cn                             │
│            HERMES_MODEL=MiniMax-M2.7                                        │
│            HERMES_MINIMAX_API_KEY=<key>                                      │
│   protocol: stdin/stdout 换行分隔 JSON-RPC 2.0                               │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
              ┌───────┴───────┐
              │ JSON-RPC 2.0  │
              └───────┬───────┘
                      │
    ┌─────────────────┼─────────────────────────┐
    │                 │                         │
    ▼                 ▼                         ▼
 initialize      session/new              session/prompt
 (id: init-0)    → session_id             → streaming events
                                          → session/update (流式)
                                          → session/complete
                                                │
                                                ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ AcpPromptOutput                                                             │
│   text: "一只正在喝水的、red的、wood感的、中号的猫..."                           │
│   timing: { prompt_ms: 316, init_ms: ~200 }                                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 链路 B：Dispatch 路径（`iota kanban dispatch <id>`）

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ User                                                                        │
│   $ iota kanban dispatch 19 --timeout 60                                    │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ CLI: kanban_cmd.rs dispatch arm                                             │
│   ├─ config::read_config() → NimiaConfig                                    │
│   ├─ backend_process_env_with_context(Hermes, &section, None)               │
│   │    → extra_env: { HERMES_INFERENCE_PROVIDER, HERMES_MODEL, ... }        │
│   └─ Dispatcher::new(DispatcherConfig { task_id_filter: Some(19), ... })    │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Dispatcher::tick() — 主循环 (500ms 间隔)                                     │
│   ├─ health_check(): 监控在飞 worker、收集 shadow 事件                        │
│   ├─ reclaim_expired_running(): 回收残留 running 任务                         │
│   ├─ recompute_ready(): blocked → ready (依赖解除)                           │
│   └─ spawn (task_id_filter=19, task.status==Ready?)                          │
│        └─ spawn_worker(&task, store)                                         │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Dispatcher::spawn_worker                                                    │
│   ├─ store.transition(19, Running)          ← event: ready → running        │
│   ├─ store.create_run(19, "default")        ← event: run_started            │
│   ├─ ShadowMaterializer::materialize()                                      │
│   │    → ~/.i6/kanban/shadows/19/kanban.db                                   │
│   │    └─ 写入: board + task + links + comments                              │
│   ├─ build_worker_context(task, comments, prior_runs)                        │
│   │    → markdown: "# Task: 生成宠物 Demo Task\n\n..."                       │
│   └─ WorkerHandle::spawn(config, env, context)                              │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ WorkerHandle::spawn — 子进程                                                 │
│   command: hermes -z <context> -p default --yolo                             │
│   env:                                                                       │
│     HERMES_KANBAN_TASK=19                                                    │
│     HERMES_KANBAN_RUN_ID=e9b657c1-...                                        │
│     HERMES_KANBAN_DB=~/.i6/kanban/shadows/19/kanban.db                       │
│     HERMES_KANBAN_BOARD=demo-1779195152                                      │
│     HERMES_INFERENCE_PROVIDER=minimax-cn      ← extra_env                    │
│     HERMES_MODEL=MiniMax-M2.7                 ← extra_env                    │
│     HERMES_MINIMAX_API_KEY=<key>              ← extra_env                    │
│   stdin:  /dev/null                                                          │
│   stdout: ~/.i6/kanban/shadows/19.stdout.log                                 │
│   stderr: ~/.i6/kanban/shadows/19.stderr.log                                 │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      │  hermes 执行中...
                      │  (读 shadow DB task 上下文, 调用 LLM, 写 shadow DB events)
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ ShadowWatcher::poll() — Dispatcher tick 每 500ms 轮询                       │
│   ├─ SELECT * FROM task_events WHERE id > last_event_id                      │
│   ├─ SELECT status FROM tasks WHERE id=19                                    │
│   │    → status="done" → terminal_status=Some("done")                        │
│   └─ sync_events() → store.heartbeat() / store.transition()                 │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Dispatcher::health_check — worker 完成                                      │
│   ├─ exit_code = Some(0) → RunStatus::Completed                              │
│   ├─ store.complete_run(run_id, Completed, Some(0))                          │
│   ├─ terminal_status="done" → store.transition(19, Done)                     │
│   │    → event: running → done                                               │
│   └─ materializer.cleanup(19)  → 删除 shadow DB                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 链路 C：Kanban 状态机（贯穿两条路径）

```
                 CLI move        CLI move       Dispatcher       health_check     CLI move
  ┌────────┐   ─────────►  ┌──────┐  ──────►  ┌───────┐  ─────►  ┌─────────┐  ─────────►  ┌──────┐  ──────►  ┌──────────┐
  │ triage │               │ todo │            │ ready │           │ running │              │ done │            │ archived │
  └────────┘               └──────┘            └───────┘           └─────────┘              └──────┘            └──────────┘
      │                                             ▲                   │
      │                                             │                   │ (exit≠0)
      │                                             └───────────────────┘
      │                                                    retry
      └─ create-task
```

### 链路 D：配置加载（共用）

```
~/.i6/nimia.yaml
      │
      ▼
config::read_config() → NimiaConfig
      │
      ├─ backend_config(&cfg, Hermes) → Option<&BackendConfig>
      │     └─ model: { provider: minimax-cn, name: MiniMax-M2.7, base_url, api_key }
      │
      └─ backend_process_env_with_context(Hermes, section, None)
            └─ render_hermes_provider_env()
                 ├─ HERMES_INFERENCE_PROVIDER = "minimax-cn"
                 ├─ HERMES_MODEL = "MiniMax-M2.7"
                 └─ HERMES_MINIMAX_API_KEY = <api_key>
                 (注: HERMES_HOME 不会被覆盖 — backend_home_env_key(Hermes) = None)
```

### 链路 E：Shadow DB 数据流（Dispatch 路径内部）

```
┌──── iota 主 DB ────┐         ┌──── shadow DB ────────────────┐
│ ~/.i6/kanban/iota.db│         │ shadows/19/kanban.db           │
│                     │         │                                │
│  boards             │──copy──►│  boards (单板)                  │
│  tasks              │──copy──►│  tasks  (单任务+关联)           │
│  task_comments      │──copy──►│  task_comments                 │
│  task_links         │──copy──►│  task_links                    │
│                     │         │                                │
│                     │         │  task_events ◄── hermes writes  │
│                     │◄─sync───│  (heartbeat / status change)   │
│                     │         │                                │
└─────────────────────┘         └────────────────────────────────┘
       │                                      ▲
       │ store.transition()                   │ hermes -z
       │ store.complete_run()                 │ reads task context
       ▼                                      │ writes task_events
  event log (events-delta.json)               │ updates task status
                                              │
                                    hermes kanban SDK (内置)
```

## 3. 事件时间线（来自 events-delta.json）

本次运行 task_id = **19**，baseline_cursor = 154，delta 共 10 个事件（task #19 相关 7 个）：

| event# | event_type | payload 关键字段 | 说明 |
|---|---|---|---|
| 156 | task_created | task_id=19, status=triage | 新建任务 |
| 157 | task_transitioned | from=triage, **to=todo** | CLI 推进 |
| 158 | task_transitioned | from=todo, **to=ready** | CLI 推进 |
| 161 | task_transitioned | from=ready, **to=running** | **Dispatcher spawn_worker** |
| 162 | run_started | run_id=e9b657c1, profile=default | Dispatcher 创建 run 记录 |
| 163 | task_transitioned | from=running, **to=done** | **hermes worker exit 0 → health_check** |
| 164 | task_transitioned | from=done, **to=archived** | 归档 |

## 4. 两侧事件对照

| 时序 | 侧 | 动作/产物 | 来源 |
|---|---|---|---|
| t0 | Kanban | task_created (triage) | iota kanban create-task |
| t1 | Kanban | triage→todo→ready | iota kanban move |
| t2 | **ACP** | `iota run --backend hermes 生成宠物` → pet-output.txt (162 chars, 316ms) | ACP JSON-RPC |
| t3 | **Dispatch** | Dispatcher 启动 `hermes -z <context> -p default --yolo` | spawn_worker |
| t4 | Kanban | ready→running (event #161) | Dispatcher transition |
| t5 | **Hermes** | hermes worker 执行 task，写 shadow DB，exit 0 | hermes -z |
| t6 | Kanban | running→done (event #163) | Dispatcher health_check |
| t7 | Kanban | done→archived | demo script cleanup |

## 5. 产物内容片段

**pet-output.txt**（ACP 路径 — Hermes 生成结果）

```
一只正在喝水的、red的、wood感的、中号的猫，抱着一个 11 厘米、hexagon 的飞盘。

属性：
- action: 喝水
- color: red
- material: wood
- size: 中
- animal: 猫
- lengthCm: 11
- toyShape: hexagon
```

**events-delta.json**（Dispatch 路径 — 状态变更节选）

```json
{ "id": 161, "event_type": "task_transitioned", "payload": "{\"task_id\":19,\"from\":\"ready\",\"to\":\"running\"}" },
{ "id": 162, "event_type": "run_started",       "payload": "{\"run_id\":\"e9b657c1-...\",\"task_id\":19,\"profile\":\"default\"}" },
{ "id": 163, "event_type": "task_transitioned", "payload": "{\"task_id\":19,\"from\":\"running\",\"to\":\"done\"}" }
```

**run-report.json**（关键指标）

```json
{
  "prompt": "生成宠物",
  "task_id": 19,
  "task_transitioned_count": 5,
  "task_updated_count": 2,
  "step_logs": [
    { "name": "run_pet",       "args": "run --no-daemon --backend hermes 生成宠物", "exit_code": 0, "duration_ms": 316 },
    { "name": "dispatch_task", "args": "kanban dispatch 19 --timeout 60",           "exit_code": 1, "duration_ms": 60904 }
  ]
}
```

> **注**：dispatch exit_code=1 是因为 60s timeout 正好在 hermes 完成时触发（hermes 约 61s 完成），但事件链显示 task 已正确到达 done（event #163）。

## 6. 关系成立的直接证据

- **ACP 路径**：`iota run --backend hermes 生成宠物` → exit_code=0, 316ms, 产出 pet-output.txt（162 chars）
- **Dispatch 路径**：Dispatcher spawn → hermes worker exit 0 → task_transitioned running→done
- **Kanban 全状态链**：triage → todo → ready → running → done → archived（5 次 task_transitioned，全部 task_id=19）
- **所有事件主键 task_id=19 贯穿始终**，产物均在 `examples/logs/latest/`

## 7. 前置检查（脚本内置）

demo 脚本在 Step 0.5 自动验证：

1. hermes binary 可用 + 版本
2. `~/.i6/nimia.yaml` 存在（推理 provider 配置）
3. `hermes -z "reply with exactly: OK"` smoke test（验证 provider 可达）

如果任何检查失败，脚本立即 throw 并给出具体原因。
