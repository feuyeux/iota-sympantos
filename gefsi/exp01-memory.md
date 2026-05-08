# iota-sympantos 实验 1：跨后端记忆延续验证

| 字段 | 值 |
|------|-----|
| 实验代号 | exp01-memory |
| 本次执行日期 | 2026-05-07 |
| 执行目录 | `D:\coding\creative\iota-sympantos` |
| 参考规范 | iota-guides/08-memory.md v2.1 |
| 存储层 | SQLite `~/.i6/context/memory.sqlite`（Rust `memory.rs`） |
| 结果日志 | `gefsi/logs/exp01-final-*`、`gefsi/logs/exp01-full-log-*-fixed.*` |

---

## 一、实验目标

验证 iota-sympantos Memory 系统在多后端切换场景下的核心主张：

> Engine 层负责 Extract / Store / Recall / Inject，后端可替换，记忆不应丢失。

验收点：

| # | 验收项 |
|---|--------|
| 1 | 后端 A 写入的记忆，后端 B 能召回并注入 context |
| 2 | 六类记忆桶均可正确存储和注入 |
| 3 | contentHash（SHA-256）去重有效 |
| 4 | confidence + scope 过滤生效 |
| 5 | `memory_chars: 2000` 截断行为可观测 |
| 6 | logging / tracing / metrics 可通过 observability 命令验证 |
| 7 | 完整记忆日志链路可观测，包括 backend tool call、Memory API route、Engine recall/inject/episodic |
| 8 | 控制台 trace 直接输出 memory read/write 的结构化日志 |

---

## 二、实验环境

### 2.1 前置条件

| 组件 | 本次结果 |
|------|----------|
| iota binary | `cargo build --release` 成功 |
| nimia.yaml | `C:\Users\feuye\.i6\nimia.yaml` |
| SQLite CLI | `C:\Users\feuye\Tools\sqlite\sqlite3.exe` |
| SQLite version | `3.53.1 2026-05-05 ... (64-bit)` |
| SQLite compile options | `ENABLE_FTS3`, `ENABLE_FTS4`, `ENABLE_FTS5` |
| 默认 PATH sqlite3 | `3.44.3 ... (32-bit)`，不支持 FTS5，不用于本实验 DB 操作 |

后端配置检查：

```powershell
.\target\release\iota.exe check
```

| backend | 状态 | model |
|---------|------|-------|
| claude-code | configured | `MiniMax-M2.7` |
| codex | configured | `gh/gpt-5.4` |
| gemini | configured | `gemini-2.5-flash` |
| hermes | configured | `MiniMax-M2.7` |
| opencode | configured | `minimax-cn-coding-plan/MiniMax-M2.7` |

### 2.2 路径约定

| 路径 | 用途 |
|------|------|
| `~/.i6/nimia.yaml` | 唯一配置来源 |
| `~/.i6/context/memory.sqlite` | 记忆存储（表名 `memory`） |
| `~/.i6/context/events.sqlite` | 事件持久化 |
| `gefsi/logs/` | 本次实验命令输出 |

### 2.3 scope_id 约定

| scope | 写入时 scope_id | 召回候选范围 |
|-------|----------------|--------------|
| user | `local-user` | `[传入值, "user-sympantos", "local-user"]` |
| project | `iota-sympantos` | `[传入值, "iota-sympantos", cwd basename]` |
| session | 自动生成 | 当前 `session_id` |

### 2.4 confidence 过滤阈值

| 桶 | min_confidence |
|----|----------------|
| identity | 0.85 |
| preference | 0.80 |
| strategic | 0.80 |
| domain | 0.80 |
| procedural | 0.75 |
| episodic | 0.70 |

---

## 三、本次修复

本轮先修复再重跑。修复内容：

| 文件 | 修复点 |
|------|--------|
| `src/runtime_event.rs` | 将 ACP `tool_call_update` 中的 `mcp__iota-context__iota_memory_*` 归一化为真实 `ToolCall` / `ToolResult`；避免把中间态空 `content: []` 误判为结果 |
| `src/cli/mod.rs` | `--trace` 新增 `[memory:read]`、`[memory:read:result]`、`[memory:write]`、`[memory:write:result]` 控制台日志 |
| `src/mcp/client.rs` | iota 内部 MCP client 启动的 sidecar stderr 改为转发到控制台，便于本地 route 诊断 |
| `src/runtime_event_tests.rs` | 增加 `tool_call_update` 写入、读取、失败结果归一化测试 |

修复后关键现象：

```text
[memory:write] id=call_function_xigjqebwv2ab_1 type=semantic facet=identity scope=user scope_id=local-user confidence=0.5 content_chars=16 args=...
[memory:write:result] id=call_function_xigjqebwv2ab_1 ok=true memory_id=a2528017-a4f9-4e04-8a07-a26a64b23c11 value=...
[memory:read] id=call_function_8hglqvifb4la_1 query=exp01-full-log-probe-20260507-fixed limit=5 mode=hybrid args=...
[memory:read:result] id=call_function_8hglqvifb4la_1 ok=true record_count=5 value=...
```

追加修复完成情况（同日追加）：

| 建议 | 实现 | 验证 |
|------|------|------|
| backend tool write schema / 客户端校验 | `iota_memory_write` 的 MCP schema 和运行时入口都要求 `content`、`type`、`scope`、`confidence`；`confidence` 必须在 `[0,1]` 内；schema 和运行时都约束 `semantic` 必须带 `facet`，`episodic/procedural` 不得带 `facet` | `cargo test memory_write` 通过；缺少 confidence 的直连 `context-mcp` 调用返回 `isError=true`、`confidence is required` |
| backend 管理的 sidecar route 日志进入主日志 | 默认 `iota-context` MCP server 注入 `RUST_LOG=iota::context::server=info`；ACP backend stderr 会在非 `--show-native` 模式下转发 memory route 相关行 | `cargo test context_mcp_server_enables_memory_route_logging` 通过 |
| `observability logging tools` 按工具名过滤和 call/result 审计 | 新增 `--tool NAME` / `--tool-name NAME`；新增 `--mode calls|results|pairs`，其中 `pairs` 会按 execution 和 tool call id 输出 call/result 成对审计视图 | `cargo run -- observability logging tools --limit 3 --tool iota_memory_write --mode pairs` 只返回 `iota_memory_write` 闭环记录 |

2026-05-08 追加实现完整日志输出方案：

| 项 | 实现 | 预期验证 |
|----|------|----------|
| 统一结构化日志事件 | 新增 `RuntimeEvent::Log(LogEvent)`，字段包含 `ts`、`level`、`target`、`execution_id`、`session_id`、`backend`、`route`、`event`、`tool_name`、`tool_call_id`、`ok`、`latency_ms`、`fields` | `observability logging events <execution-id>` 可看到 `event_type=log` |
| 控制台 trace 共用结构 | `iota run --trace` 的 `[memory:read/write]` 输出由 `LogEvent` 渲染，避免和 EventStore 字段漂移 | memory tool call/result 控制台格式保持兼容 |
| Engine memory 结构化审计 | recall started/completed/failed、inject、engine-keyword write、episodic write、compaction 均写入 `RuntimeEvent::Log` | `observability logging logs --event memory.write.result` 可查 engine 写入结果 |
| MCP sidecar route JSONL | `context-mcp` 在 memory search/write call/result 上输出 `[iota log] {...LogEvent...}`，失败也输出 `ok=false` | 直连 `context-mcp` 或 backend stderr 转发可见结构化 route 行 |
| tools audit 扫描完整性 | `observability logging tools` 新增 `--scan N`，默认至少扫描 500 条 execution | 稀疏工具调用不再受 `limit * 5` 取样影响 |
| pairs 异常状态 | `--mode pairs` 新增 `status=completed/missing_call/missing_result`，`result_seq` 和 `ok` 支持为空 | 可以诊断只有 call 或只有 result 的异常链路 |
| log 查询入口 | 新增 `observability logging logs [--event NAME] [--scan N]` | 可直接按结构化 event 名过滤日志 |

---

## 四、实验步骤与本次结果

### Step 0 - 环境准备

执行：

```powershell
cargo build --release
& "$env:USERPROFILE\Tools\sqlite\sqlite3.exe" --version
```

结果：

| 检查项 | 结果 |
|--------|------|
| release build | 通过 |
| sqlite version | `3.53.1 2026-05-05 ... (64-bit)` |
| FTS5 | 通过 |

清理测试 scope 数据：

```powershell
$sqlite = "$env:USERPROFILE\Tools\sqlite\sqlite3.exe"
$memoryDb = "$env:USERPROFILE\.i6\context\memory.sqlite"
& $sqlite $memoryDb "DELETE FROM memory
  WHERE scope_id IN ('user-sympantos','iota-sympantos','local-user')
     OR scope_id LIKE '%iota-sympantos'
     OR content LIKE '%exp01-full-log-probe-%'
     OR content LIKE '%domain-padding-%'
     OR content LIKE '%低置信度测试%';"
```

结果：清理后匹配记录数为 `0`。

备注：PATH 上的 Android `sqlite3.exe` 不支持 FTS5，会在删除触发 `memory_fts` trigger 时失败；本实验改用 `C:\Users\feuye\Tools\sqlite\sqlite3.exe`。

---

### Step 1 - 通过 MCP 工具写入 6 类记忆（claude-code）

执行方式：`claude-code` 后端通过 `iota-context` MCP 工具调用 `iota_memory_write`。每条命令均使用 `--trace --timeout-ms 180000` 并写入 `gefsi/logs/exp01-final-step1-*.txt`。

本次 DB 结果：

| 子步骤 | type | facet | scope | scope_id | confidence | short_id | 结果 |
|--------|------|-------|-------|----------|------------|----------|------|
| 1-A | semantic | identity | user | local-user | 0.95 | `a68ec01a` | 通过 |
| 1-B | semantic | preference | user | local-user | 0.90 | `84ec24a4` | 通过 |
| 1-C | semantic | strategic | project | iota-sympantos | 0.90 | `3b0e6dad` | 通过 |
| 1-D | semantic | domain | project | iota-sympantos | 0.90 | `680aeb70` | 通过 |
| 1-E | procedural | - | project | iota-sympantos | 1.00 | `ac413811` | 通过，后端省略 confidence，存储层使用默认 1.00 |
| 1-F | episodic | - | project | iota-sympantos | 0.80 | `d75d5464` | 通过 |

统计：6 条记录，6 个桶各 1 条。

---

### Step 2 - identity 召回验证（codex）

执行：

```powershell
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "我是谁？请介绍你对我的了解"
```

结果：

| 检查项 | 本次结果 |
|--------|----------|
| 后端回复 | 包含 `用户名 Sympantos`、`iota-sympantos 实验员`、`跨后端记忆延续验证` |
| trace | `[memory:inject]` 中 `identity` 包含 `a68ec01a` |
| 判定 | 通过 |

---

### Step 3 - preference 召回验证（gemini）

执行：

```powershell
.\target\release\iota.exe run --backend gemini --trace --timeout-ms 180000 "你知道我的回答语言偏好和报告格式吗？"
```

结果：

| 检查项 | 本次结果 |
|--------|----------|
| 后端回复 | 中文回复，提及中文回答、英文日志/命令/代码标识、Markdown、2 空格 |
| trace | `[memory:inject]` 中 `preference` 包含 `84ec24a4` |
| 判定 | 通过 |

---

### Step 4 - strategic + domain 召回验证（hermes）

执行：

```powershell
.\target\release\iota.exe run --backend hermes --trace --timeout-ms 180000 "告诉我当前项目的目标和技术实现"
```

结果：

| 检查项 | 本次结果 |
|--------|----------|
| 后端回复 | 提及 2026 Q2 目标、Rust、SQLite、recall/inject、SHA-256 content_hash、6 桶 |
| trace | `strategic` 包含 `3b0e6dad`，`domain` 包含 `680aeb70` |
| 判定 | 通过 |

---

### Step 5 - procedural + episodic 召回验证（opencode）

执行：

```powershell
.\target\release\iota.exe run --backend opencode --trace --timeout-ms 180000 "回顾实验步骤，以及本次实验发生了什么"
```

结果：

| 检查项 | 本次结果 |
|--------|----------|
| 后端回复 | 覆盖 6 步实验流程和 Step 1 完成 6 类记忆写入 |
| trace | `procedural` 包含 `ac413811`，`episodic` 包含 `d75d5464` |
| 判定 | 通过 |

---

### Step 6 - contentHash 去重验证

重复写入 Step 1-A 的 identity content。

写入前：

| id | hash12 | created_at | updated_at | confidence |
|----|--------|------------|------------|------------|
| `a68ec01a-0d3a-44f9-a859-ad4aeab93722` | `5ee43f7ae37d` | `1778155663` | `1778155663` | 0.95 |

重复写入后：

| id | hash12 | created_at | updated_at | confidence |
|----|--------|------------|------------|------------|
| `a68ec01a-0d3a-44f9-a859-ad4aeab93722` | `5ee43f7ae37d` | `1778155663` | `1778155776` | 0.95 |

结果：

| 检查项 | 本次结果 |
|--------|----------|
| 同 content 行数 | `1` |
| ID/hash | 不变 |
| `updated_at` | 更新 |
| 控制台日志 | `[memory:write]` 和 `[memory:write:result]` 显示真实 memory_id `a68ec01a...` |
| 判定 | 通过 |

---

### Step 7 - confidence 过滤验证

目标：写入低于阈值的 identity 和 procedural，验证存在于 DB 但不进入注入桶。

本次过程：

| 记录 | 写入方式 | short_id | confidence | 结果 |
|------|----------|----------|------------|------|
| low identity | claude-code tool call | `a2528017` | 0.50 | 低于 0.85，未注入 |
| low procedural | 直连 `iota context-mcp` JSON-RPC | `c74b1f49` | 0.60 | 低于 0.75，未注入 |

验证命令：

```powershell
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "你知道关于我的所有信息吗？"
```

结果：

| 检查项 | 本次结果 |
|--------|----------|
| DB | `a2528017` 和 `c74b1f49` 均存在 |
| trace identity | 只包含高置信度 `a68ec01a`，不包含低置信度测试文本 |
| trace procedural | 只包含高置信度 `ac413811`，不包含低置信度测试文本 |
| 控制台日志 | low identity 写入显示 `[memory:write] confidence=0.5` 和 `[memory:write:result] memory_id=a2528017...` |
| 判定 | 通过 |

---

### Step 8 - token budget 截断验证

准备：通过 `iota context-mcp` 批量写入 15 条 `domain-padding-N` 记录，confidence=0.90。

统计：

| 指标 | 值 |
|------|----|
| padding_count | 15 |
| padding_chars | 2481 |
| eligible_chars | 2916 |

触发召回：

```powershell
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "列出你知道的关于我和本项目的所有信息"
```

trace 中 budget：

```json
{"memory_chars":2000,"total_chars":2916,"truncated":true,"excluded_count":7}
```

判定：通过。

---

### Step 9 - Observability 审计

执行：

```powershell
.\target\release\iota.exe observability logging recent --limit 80
.\target\release\iota.exe observability tracing summary
.\target\release\iota.exe observability metrics
.\target\release\iota.exe observability metrics --prometheus
.\target\release\iota.exe observability logging events 6b8a00be-6ff4-4653-92a6-5e0f1a51ce3e
.\target\release\iota.exe observability tracing breakdown 6b8a00be-6ff4-4653-92a6-5e0f1a51ce3e
.\target\release\iota.exe observability logging tools --limit 20
.\target\release\iota.exe observability logging tools --limit 3 --tool iota_memory_write --mode pairs
```

本地 EventStore 全量统计（包含历史运行，不只本实验）：

| 指标 | 值 |
|------|----|
| total_executions | 102 |
| completed_executions | 93 |
| failed_executions | 4 |
| running_executions | 5 |
| avg_prompt_ms | 11293.59 |
| avg_total_ms | 12398.13 |
| p95_total_ms | 24780 |
| cache.hit_rate | 0.07258064516129033 |
| token usage events | 0 |

本轮 Step 1 到 Step 8 覆盖 fencing token `89..102`，包含 `claude-code`、`codex`、`gemini`、`hermes`、`opencode`。

Step 7 low identity breakdown：

| phase | ms |
|-------|----|
| process_spawn | 13 |
| init | 1182 |
| session_new | 785 |
| prompt | 15300 |
| total | 16085 |

EventStore 事件流检查：

| execution_id | 结果 |
|--------------|------|
| `6b8a00be-6ff4-4653-92a6-5e0f1a51ce3e` | 含 `state started`、`memory inject`、泛化 `tool_call name=tool`、归一化 `tool_call name=iota_memory_write`、归一化 `tool_result name=iota_memory_write`、`output` |

`observability logging tools --limit 20` 已能列出真实工具名：

| seq | tool_name | 说明 |
|-----|-----------|------|
| 8 | `tool` | ACP 后端原始泛化事件 |
| 10 | `iota_memory_write` | 从 `tool_call_update.rawInput` 归一化出的真实工具事件 |

后续补充：`observability logging tools --limit 3 --tool iota_memory_write --mode pairs` 已支持按真实工具名过滤，并输出 `tool_call` / `tool_result` 成对审计视图。

判定：通过。修复后工具调用和工具结果都可在 EventStore 中以真实工具名审计。

---

### Step 10 - 完整记忆日志链路用例

设计目标：验证记忆日志链路是否可审计，而不是重复验证记忆内容正确性。

本次 marker：

```text
exp01-full-log-probe-20260507-fixed
```

#### 10.1 日志捕获准备

执行：

```powershell
New-Item -ItemType Directory -Force gefsi\logs | Out-Null
$env:RUST_LOG = "info"
```

结果：

| 检查项 | 本次结果 |
|--------|----------|
| `RUST_LOG=info` | 可打印 `iota::engine` 和 `iota::context::server` info 日志 |
| marker 清理 | 已执行 `DELETE FROM memory WHERE content LIKE '%exp01-full-log-probe-20260507-fixed%'` |

#### 10.2 backend tool write 日志链路

执行：

```powershell
.\target\release\iota.exe run --backend claude-code --trace --timeout-ms 180000 `
  "请必须调用 iota_memory_write 工具一次，不要只口头回答。参数如下：
   type=semantic, facet=domain, scope=project, scope_id=iota-sympantos,
   content=\"exp01-full-log-probe-20260507-fixed: backend tool write probe, 用于验证完整记忆日志链路\",
   confidence=0.91,
   metadata={\"case\":\"exp01-full-log\",\"phase\":\"tool-write-fixed\"}" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-write-fixed.txt
```

结果：

| 项 | 值 |
|----|----|
| backend | claude-code |
| execution_id | `5f0914d7-8a4b-43d2-86d7-07ad1efe668f` |
| session_id | `e4c00316-7399-4b86-a9e7-dabf1bdcc9e3` |
| 成功写入 ID | `4f325b36-f9d3-4808-b50c-afef2829a194` |
| DB confidence | `1.00`，后端本次省略了 prompt 中的 `confidence=0.91` |
| Engine recall/inject | 出现 `engine memory recall started/completed` 和 `engine memory inject event recorded` |

控制台 trace 证据：

```text
[memory:write] id=call_function_hd18e23j8uvt_1 type=semantic facet=domain scope=project scope_id=iota-sympantos confidence=- content_chars=75 args=...
[memory:write:result] id=call_function_hd18e23j8uvt_1 ok=true memory_id=4f325b36-f9d3-4808-b50c-afef2829a194 value={"id":"4f325b36-f9d3-4808-b50c-afef2829a194","merge_mode":"auto"}
```

EventStore 证据：

| seq | event_type | 关键内容 |
|-----|------------|----------|
| 2/3 | memory | inject payload，budget `truncated=true` |
| 8 | tool_call | 原始 ACP 泛化事件 `name=tool` |
| 9 | state | `tool_call_update`，`rawInput` 含 `type=semantic`、`facet=domain` |
| 10 | tool_call | 归一化事件 `name=iota_memory_write`，arguments 为 `rawInput` |
| 13 | state | `rawOutput={"id":"4f325b36-...","merge_mode":"auto"}` |
| 14 | tool_result | 归一化事件 `name=iota_memory_write`，`ok=true`，result 含 memory id |
| 18/19 | output | assistant 输出写入成功 ID |

判定：通过。

#### 10.3 backend tool search 日志链路

执行：

```powershell
.\target\release\iota.exe run --backend claude-code --trace --timeout-ms 180000 `
  "请必须调用 iota_memory_search 工具一次，不要只口头回答。参数如下：
   query=\"exp01-full-log-probe-20260507-fixed\", limit=5, mode=hybrid。
   然后用一句话总结搜索结果。" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-search-fixed.txt
```

结果：

| 项 | 值 |
|----|----|
| backend | claude-code |
| execution_id | `7445ea10-52cf-4fee-a82e-0ab8c5a5235a` |
| session_id | `4f3cda7f-5f5b-4320-9006-62a1739e5615` |
| 搜索结果 | 5 条记录 |
| 命中核心记录 | `4f325b36`，content 含 `exp01-full-log-probe-20260507-fixed` |
| Engine recall/inject | 出现 `engine memory recall started/completed` 和 `engine memory inject event recorded` |
| 自动 episodic | 搜索 turn 结束后写入 `db08c47e-feb0-4382-a2ad-a87f3ee74957` |

控制台 trace 证据：

```text
[memory:read] id=call_function_8hglqvifb4la_1 query=exp01-full-log-probe-20260507-fixed limit=5 mode=hybrid args=...
[memory:read:result] id=call_function_8hglqvifb4la_1 ok=true record_count=5 value=...
```

EventStore 证据：

| seq | event_type | 关键内容 |
|-----|------------|----------|
| 8 | tool_call | 原始 ACP 泛化事件 `name=tool` |
| 9 | state | `tool_call_update`，`rawInput.query=exp01-full-log-probe-20260507-fixed` |
| 10 | tool_call | 归一化事件 `name=iota_memory_search` |
| 13 | state | `rawOutput` 包含 `mode=hybrid`、`records`、`4f325b36` |
| 14 | tool_result | 归一化事件 `name=iota_memory_search`，`ok=true`，result 中 `records` 数为 5 |
| 19/20 | output | assistant 总结搜索结果 |

判定：通过。

#### 10.4 Engine 自动 episodic 写入日志链路

执行：

```powershell
.\target\release\iota.exe run --backend gemini --trace --timeout-ms 180000 `
  "请用一句话回答：exp01-full-log-probe-20260507-fixed 普通 turn 日志探针已收到。" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-episodic-fixed.txt
```

结果：

| 项 | 值 |
|----|----|
| backend | gemini |
| execution_id | `ab4afaad-946a-408a-b1e6-8b6cb8504306` |
| session_id | `cbe42e54-52a4-47e6-9e4b-660aa3b22101` |
| output | `好的，exp01-full-log-probe-20260507-fixed 普通 turn 日志探针已收到。` |
| episodic memory_id | `b14be7f7-b680-464c-8ffc-97a9a87c375c` |

日志证据：

```text
engine memory recall started
engine memory recall completed
engine memory inject event recorded
engine episodic memory write started
engine episodic memory write completed memory_id=b14be7f7-b680-464c-8ffc-97a9a87c375c
engine episodic memory compaction completed
[memory:inject]
```

判定：通过。

#### 10.5 Memory API route 日志链路

backend 通过 session/new 注入的 `context-mcp` stdio server 由后端进程管理。后续修复已为默认 `iota-context` sidecar 注入 `RUST_LOG=iota::context::server=info`，并让 ACP backend stderr 在非 `--show-native` 模式下转发 memory route 相关行；如果后端把 sidecar stderr 传回 ACP 进程 stderr，`iota run` 主日志即可捕获这些 route 行。为直接验证 Memory API route，本轮仍保留直连 sidecar probe：

```powershell
$env:RUST_LOG = "info"
@($init, $ready, $call) | .\target\release\iota.exe context-mcp *>&1 |
  Tee-Object gefsi\logs\exp01-full-log-route-direct-fixed.txt
```

结果：

| 日志片段 | 本次结果 |
|----------|----------|
| `context MCP memory search tool call received` | 出现，`query=exp01-full-log-probe-20260507-fixed`、`limit=5`、`mode=Hybrid` |
| `context MCP memory search tool call completed` | 出现，`record_count=5` |
| `record_ids` | 包含 `4f325b36`、`b14be7f7`、`db08c47e` |

判定：通过。Memory API route 自身可观测；主进程侧已具备 selective stderr 转发能力，剩余差异只取决于具体后端是否传回 sidecar stderr。

#### 10.6 自动检查日志文件

检查文件：

| 文件 | 关键结果 |
|------|----------|
| `exp01-full-log-write-fixed.txt` | 有 Engine recall/inject、`[memory:inject]`、`[memory:write]`、`[memory:write:result]` |
| `exp01-full-log-search-fixed.txt` | 有 Engine recall/inject、`[memory:inject]`、`[memory:read]`、`[memory:read:result]`、搜索总结 |
| `exp01-full-log-episodic-fixed.txt` | 有 Engine recall/inject、自动 episodic started/completed/compaction |
| `exp01-full-log-route-direct-fixed.txt` | 有 Memory API route received/completed、`record_count=5` |
| `exp01-full-log-events-write-fixed.json` | 有 `tool_call name=iota_memory_write` 和 `tool_result name=iota_memory_write` |
| `exp01-full-log-events-search-fixed.json` | 有 `tool_call name=iota_memory_search` 和 `tool_result name=iota_memory_search` |

判定：通过。

#### 10.7 EventStore 持久化验证

最近三条成功 probe：

| execution_id | backend | status | 关键证据 |
|--------------|---------|--------|----------|
| `5f0914d7-8a4b-43d2-86d7-07ad1efe668f` | claude-code | completed | `tool_call/tool_result iota_memory_write` |
| `7445ea10-52cf-4fee-a82e-0ab8c5a5235a` | claude-code | completed | `tool_call/tool_result iota_memory_search` |
| `ab4afaad-946a-408a-b1e6-8b6cb8504306` | gemini | completed | Engine 自动 episodic |

EventStore 结论：

| 证据 | 本次结果 |
|------|----------|
| `state started` | 有 |
| `memory inject` | 有 |
| `tool_call` | 有，包含原始 `tool` 和归一化真实工具名 |
| `tool_result` | 有，包含 `iota_memory_write` / `iota_memory_search` |
| `output` | 有 |

判定：通过。

#### 10.8 DB 侧确认

查询：

```powershell
& "$env:USERPROFILE\Tools\sqlite\sqlite3.exe" -header -column `
  "$env:USERPROFILE\.i6\context\memory.sqlite" `
  "SELECT substr(id,1,8) AS short_id, type, facet, scope, scope_id, confidence, substr(content,1,120)
   FROM memory
   WHERE content LIKE '%exp01-full-log-probe-20260507-fixed%'
   ORDER BY updated_at DESC;"
```

结果：

| short_id | type | facet | scope | scope_id | confidence | 说明 |
|----------|------|-------|-------|----------|------------|------|
| `b14be7f7` | episodic | - | session | `cbe42e54-52a4-47e6-9e4b-660aa3b22101` | 0.80 | gemini 普通 turn 自动 episodic |
| `db08c47e` | episodic | - | session | `4f3cda7f-5f5b-4320-9006-62a1739e5615` | 0.80 | claude-code search turn 自动 episodic |
| `4f325b36` | semantic | domain | project | `iota-sympantos` | 1.00 | backend tool write probe |

判定：通过。

#### 10.9 清理状态

本次没有在文档更新前删除 probe 记录，原因是 Step 10 的 DB 侧证据需要保留到文档完成。可在需要时执行：

```powershell
& "$env:USERPROFILE\Tools\sqlite\sqlite3.exe" "$env:USERPROFILE\.i6\context\memory.sqlite" `
  "DELETE FROM memory
   WHERE scope_id IN ('local-user','iota-sympantos')
      OR scope_id LIKE '%iota-sympantos'
      OR content LIKE '%exp01-full-log-probe-%'
      OR content LIKE '%domain-padding-%'
      OR content LIKE '%低置信度测试%';"
```

#### 10.10 结构化日志输出追加验证（2026-05-08）

本轮实现统一 `LogEvent` 后追加验证：

```powershell
cargo test
cargo build --release
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "我叫 exp-log-event-20260508"
.\target\release\iota.exe observability logging logs --limit 5 --event memory.write.result --scan 50
.\target\release\iota.exe observability logging tools --limit 5 --tool iota_memory_write --mode pairs --scan 500
@($init, $ready, $call) | .\target\release\iota.exe context-mcp *>&1
```

结果：

| 检查项 | 本次结果 |
|--------|----------|
| `cargo test` | 通过，107 passed |
| `cargo build --release` | 通过 |
| 控制台 trace | memory-write-only turn 输出 `[memory:write] {...LogEvent...}`，event 为 `memory.write` |
| `observability logging logs` | 查到 `event_type=log`，`event=memory.write.result`，`backend=codex`，`route=engine`，`ok=true` |
| `observability logging tools --mode pairs --scan 500` | 返回 `status=completed`、`call_seq`、`result_seq`、`ok=true`，并按 `iota_memory_write` 过滤 |
| `context-mcp` route JSONL | stderr 输出 `[iota log] {...}`，包含 `memory.write.call` 和 `memory.write.result`，`route=mcp-sidecar` |
| 验证数据清理 | 已删除 `exp-log-event-20260508` 与 `exp-route-log-20260508` 记忆记录 |

新增命令行为：

| 命令 | 说明 |
|------|------|
| `observability logging logs [--event NAME] [--scan N]` | 查询持久化结构化 `LogEvent` |
| `observability logging tools --mode pairs` | pair 输出新增 `status=completed/missing_call/missing_result` |
| `observability logging tools --scan N` | 控制扫描 execution 窗口，默认至少 500 条 |

---

## 五、验收矩阵

| # | 验收项 | 步骤 | 本次结果 |
|---|--------|------|----------|
| 1 | identity 跨后端延续 | Step 2 | 通过 |
| 2 | preference 跨后端延续 | Step 3 | 通过 |
| 3 | strategic + domain 跨后端延续 | Step 4 | 通过 |
| 4 | procedural + episodic 延续 | Step 5 | 通过 |
| 5 | contentHash 去重 | Step 6 | 通过 |
| 6 | confidence 过滤（identity） | Step 7 | 通过 |
| 7 | confidence 过滤（procedural） | Step 7 | 通过 |
| 8 | token budget 截断 | Step 8 | 通过，`truncated=true`, `excluded_count=7` |
| 9 | SQLite schema 合规 | Step 1 | 通过 |
| 10 | trace 事件完整性 | Step 2-5 | 通过 |
| 11 | EventStore 持久化 | Step 9 | 通过 |
| 12 | Logging 多后端覆盖 | Step 9 | 通过 |
| 13 | Tracing 延迟分解 | Step 9 | 通过 |
| 14 | Metrics 指标可查 | Step 9 | 通过 |
| 15 | Prometheus 导出 | Step 9 | 通过 |
| 16 | Step 10 Engine recall/inject 日志 | Step 10 | 通过 |
| 17 | Step 10 backend MCP tool call 审计 | Step 10 | 通过，真实 `iota_memory_*` 工具名已归一化 |
| 18 | Step 10 Memory API route 日志 | Step 10 | 通过，直连 `context-mcp` 可见 received/completed/record_count |
| 19 | Step 10 Engine 自动 episodic | Step 10 | 通过 |
| 20 | 控制台 memory read/write 日志 | Step 6/7/10 | 通过，新增 `[memory:read/write]` 和结果日志 |

结论：核心 memory 延续、去重、过滤、budget、observability 和完整日志链路均通过。本轮已修复上一轮暴露的 RuntimeEvent 工具名/结果归一化问题，并新增控制台 memory read/write 结构化日志。

---
