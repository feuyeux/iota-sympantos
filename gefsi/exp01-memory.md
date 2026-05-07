# iota-sympantos 实验 1：跨后端记忆延续验证

| 字段 | 值 |
|------|-----|
| 实验代号 | exp01-memory |
| 执行日期 | 2026-05-06 |
| 参考规范 | iota-guides/08-memory.md v2.1 |
| 存储层 | SQLite `~/.i6/context/memory.sqlite`（Rust `memory.rs`） |

---

## 一、实验目标

验证 iota-sympantos Memory 系统在**多后端切换场景**下的核心主张：

> Engine 层（Rust）负责 Extract / Store / Recall / Inject，后端可替换，记忆不应丢失。

**验收点：**

| # | 验收项 |
|---|--------|
| 1 | 后端 A 写入的记忆，后端 B 能完整召回并注入 context |
| 2 | 六类记忆桶均可正确存储和注入（semantic×4 + procedural + episodic） |
| 3 | contentHash（SHA-256）去重有效——相同 content 不产生新行 |
| 4 | confidence + scope 过滤生效（低于阈值的条目不注入） |
| 5 | token budget（`memory_chars: 2000`）截断行为可观测 |
| 6 | logging / tracing / metrics 可通过 observability 命令验证 |

---

## 二、实验环境

### 2.1 前置条件

| 组件 | 要求 |
|------|------|
| iota binary | `cargo build --release` 成功 |
| nimia.yaml | `~/.i6/nimia.yaml` 已配置至少 2 个后端（推荐全 5 个） |
| SQLite CLI | `sqlite3` ≥ 3.53，需包含 `ENABLE_FTS5` 编译选项 |
| 后端可用性 | 各后端 API key 已在 nimia.yaml 中配置 |

> 官方 sqlite-tools-win-x64-3530100 → `~/tools/sqlite/sqlite3.exe`（3.53.1 64-bit，含 FTS3/4/5）。
> PowerShell 中通过 alias 指向新二进制。

### 2.2 路径约定

| 路径 | 用途 |
|------|------|
| `~/.i6/nimia.yaml` | 唯一配置来源 |
| `~/.i6/context/memory.sqlite` | 记忆存储（表名 `memory`） |
| `~/.i6/context/events.sqlite` | 事件持久化 |
| `~/.i6/skills` / `./.iota/skills` | Skill 根目录 |

### 2.3 scope_id 约定

| scope | 写入时 scope_id | 召回候选范围 |
|-------|----------------|--------------|
| user | `"local-user"` | `[传入值, "user-sympantos", "local-user"]` |
| project | cwd 路径 | `[传入值, "iota-sympantos", cwd basename]` |
| session | 自动生成 | 当前 session_id |

### 2.4 各桶 confidence 过滤阈值

硬编码于 `recall_buckets()`：

| 桶 | min_confidence |
|----|----------------|
| identity | 0.85 |
| preference | 0.80 |
| strategic | 0.80 |
| domain | 0.80 |
| procedural | 0.75 |
| episodic | 0.70 |

---

## 三、实验步骤

### Step 0 — 环境准备

```bash
cargo build --release       # → Finished `release` profile in 0.40s
sqlite3 --version           # → 3.53.1 2026-05-05 (64-bit)
```

清理测试 scope 数据：

```bash
sqlite3 ~/.i6/context/memory.sqlite \
  "DELETE FROM memory WHERE scope_id IN (
    'user-sympantos', 'iota-sympantos', 'local-user'
  ) OR scope_id LIKE '%iota-sympantos';"
# 验证: count=0
```

---

### Step 1 — 通过 MCP 工具写入 6 类记忆（claude-code）

> **设计：** 通过 prompt 引导后端调用 `iota_memory_write` MCP 工具，确保每条记忆的 type/facet/scope/content 完全可控。

| 子步骤 | type | facet | scope | scope_id | content 摘要 | confidence | 返回 ID |
|--------|------|-------|-------|----------|-------------|-----------|---------|
| 1-A | semantic | identity | user | local-user | 用户名 Sympantos，实验员角色 | 0.95 | `b8517b2e` |
| 1-B | semantic | preference | user | local-user | 中文回答，英文日志，Markdown，2 空格 | 0.90 | `595bbb72` |
| 1-C | semantic | strategic | project | iota-sympantos | 2026 Q2 完成跨后端验证 | 0.90 | `1ba45094` |
| 1-D | semantic | domain | project | iota-sympantos | SQLite + Rust Engine + SHA-256 去重 + 6 桶 | 0.90 | `eb2a22d0` |
| 1-E | procedural | — | project | iota-sympantos | 6 步实验流程 | 0.85 | `605134c9` |
| 1-F | episodic | — | project | iota-sympantos | Step1 完成 6 类写入，准备切换验证 | 0.80 | `c85def24` |

**执行命令示例（1-A）：**

```bash
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=identity, scope=user, scope_id=local-user,
   content=\"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证\",
   confidence=0.95"
```

**DB 最终验证：** scope_id IN (`local-user`, `iota-sympantos`) 下共 6 条记忆，类型分布正确。

---

### Step 2 — identity 召回验证（codex）

```bash
.\target\release\iota.exe run --backend codex --trace "我是谁？请介绍你对我的了解"
```

**结果：** 回复包含 "Sympantos" 和跨后端记忆延续验证职责。

**trace 验证：** `[memory:inject]` → `identity` 数组包含 `b8517b2e`（scope_id=`local-user`，confidence=0.95）。

**判定：** ☑ 通过

---

### Step 3 — preference 召回验证（gemini）

```bash
.\target\release\iota.exe run --backend gemini --trace "你知道我的回答语言偏好和报告格式吗？"
```

**结果：** 中文回复，提及英文日志、Markdown 格式和 2 空格缩进。

**trace 验证：** `preference` 数组包含 `595bbb72`，content 含 "中文" 和 "Markdown"。

**判定：** ☑ 通过

---

### Step 4 — strategic + domain 召回验证（hermes）

```bash
.\target\release\iota.exe run --backend hermes --trace "告诉我当前项目的目标和技术实现"
```

**结果：** 提及 Q2 目标、SQLite 存储层、Rust Engine、SHA-256 content_hash 去重。

**trace 验证：**

- `strategic` 含 `1ba45094`（scope_id=`iota-sympantos`）
- `domain` 含 `eb2a22d0`（scope_id=`iota-sympantos`）

**判定：** ☑ 通过

---

### Step 5 — procedural + episodic 召回验证（opencode）

```bash
.\target\release\iota.exe run --backend opencode --trace "回顾实验步骤，以及本次实验发生了什么"
```

**结果：** 覆盖 6 步实验流程和 Step1 完成 6 类记忆写入的经历。

**trace 验证：**

- `procedural` 含 `605134c9`
- `episodic` 含 `c85def24`，content 含 "6 类记忆写入"

**判定：** ☑ 通过

---

### Step 6 — contentHash 去重验证

> **设计：** 重复写入与 Step 1-A 完全相同的 content，验证不产生新行。

**写入前状态：**

```
id=b8517b2e | hash=5ee43f7a... | created_at=1778057289 | updated_at=1778057289
```

**重复写入（claude-code）：** 返回同一 ID `b8517b2e`，merge_mode=auto。

**写入后状态：**

```
id=b8517b2e | hash=5ee43f7a... | created_at=1778057289 | updated_at=1778057876
```

**判定：** ☑ 通过 — 仍只有 1 行，content_hash 相同，仅 `updated_at` 更新。

---

### Step 7 — confidence 过滤验证

写入两条低 confidence 记录：

| type | facet | confidence | 阈值 | content |
|------|-------|-----------|------|---------|
| semantic | identity | 0.50 | 0.85 | "低置信度测试：这条记忆不应被注入" |
| procedural | — | 0.60 | 0.75 | "低置信度测试：这条 procedural 不应被注入" |

**DB 验证：** 两条记录已存在于 `memory` 表。

**召回验证（codex）：**

```bash
.\target\release\iota.exe run --backend codex --trace "你知道关于我的所有信息吗？"
```

**trace 验证：**

- `identity` 数组不包含 "低置信度测试"（0.50 < 0.85）
- `procedural` 数组不包含 "低置信度测试"（0.60 < 0.75）
- 高 confidence 原始记录正常出现

**判定：** ☑ 通过

---

### Step 8 — token budget 截断验证

**准备：** 批量写入 15 条 `domain-padding-N` 记忆（confidence=0.90），使总字符数超过 budget。

```bash
foreach ($i in 1..15) {
  .\target\release\iota.exe run --backend claude-code \
    "请调用 iota_memory_write: type=semantic, facet=domain, scope=project,
     scope_id=iota-sympantos, content=\"domain-padding-$i: ...\", confidence=0.90"
}
```

**写入后统计：**

```sql
SELECT sum(length(content)) FROM memory
WHERE scope_id IN ('local-user','iota-sympantos') AND confidence >= 0.70;
-- → 2278 chars（超过 memory_chars=2000）
```

**触发召回（codex）：**

```bash
.\target\release\iota.exe run --backend codex --trace "列出你知道的关于我和本项目的所有信息"
```

**trace 中 budget 字段：**

```json
{"memory_chars": 2000, "total_chars": 2278, "truncated": true, "excluded_count": 3}
```

**判定：** ☑ 通过

---

### Step 9 — Observability 审计

#### 9.1 Logging

```bash
.\target\release\iota.exe observability logging recent --limit 30
```

覆盖 Step 1~8 所有后端（claude-code / codex / gemini / hermes / opencode），共 39 条 completed 记录。

```bash
.\target\release\iota.exe observability logging events 7c1986b2-3a0d-4cc8-b413-c4fa6a36e205
```

Step 1-A 完整事件流：`started → memory inject → tool_call → tool_call_update → toolResponse → assistant output`。

#### 9.2 Tracing

```bash
.\target\release\iota.exe observability tracing summary
```

```json
{"total_executions": 39, "completed": 39, "failed": 0,
 "avg_prompt_ms": 11951, "avg_total_ms": 12774, "p95_total_ms": 26740}
```

```bash
.\target\release\iota.exe observability tracing breakdown 7c1986b2-3a0d-4cc8-b413-c4fa6a36e205
```

| 阶段 | 耗时 (ms) |
|------|-----------|
| process_spawn | 13 |
| init | 1389 |
| session_new | 892 |
| prompt | 7006 |
| **total** | **7899** |

#### 9.3 Metrics

```bash
.\target\release\iota.exe observability metrics
```

| 维度 | 值 |
|------|-----|
| executions.total | 39 |
| executions.completed | 39 |
| executions.failed | 0 |
| latency.avg_total_ms | 12774 |
| latency.p95_total_ms | 26740 |
| cache.hit_rate | 0.0 |

```bash
.\target\release\iota.exe observability metrics --prometheus
```

```
iota_execution_attempts_total 39
iota_execution_completed_total 39
iota_execution_failed_total 0
iota_prompt_latency_ms_sum 394386
iota_total_latency_ms_p95 26740
```

> **备注：** token 用量为 0——当前 ACP 后端尚未上报 token 事件。

---

### Step 10 — 完整记忆日志链路用例

> **设计目标：** 该步骤专门验证“记忆日志是否完整”，不是重复验证记忆正确性。
> 需要同时看到三类证据：
>
> 1. backend 通过 ACP / MCP `tools/call` 发起 `iota_memory_write` / `iota_memory_search`
> 2. iota 路由 memory API 时打印 search/write 的参数和结果
> 3. Engine 自身的 recall / inject / episodic write 进入 tracing 和 EventStore

#### 10.1 日志捕获准备

```powershell
cd D:\coding\creative\iota-sympantos
New-Item -ItemType Directory -Force gefsi\logs | Out-Null

$env:RUST_LOG = "iota_sympantos=info"
$marker = "exp01-full-log-probe-20260507"
```

清理上一次同名探针数据，确保日志和 DB 结果易读：

```powershell
sqlite3 "$env:USERPROFILE\.i6\context\memory.sqlite" `
  "DELETE FROM memory WHERE content LIKE '%exp01-full-log-probe-20260507%';"
```

#### 10.2 backend tool write 日志链路

```powershell
.\target\release\iota.exe run --backend claude-code --trace `
  "请必须调用 iota_memory_write 工具一次，不要只口头回答。参数如下：
   type=semantic, facet=domain, scope=project, scope_id=iota-sympantos,
   content=\"exp01-full-log-probe-20260507: backend tool write probe, 用于验证完整记忆日志链路\",
   confidence=0.91,
   metadata={\"case\":\"exp01-full-log\",\"phase\":\"tool-write\"}" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-write.txt
```

**必须出现的日志片段：**

```text
engine memory recall started
engine memory recall completed
engine memory inject event recorded
ACP backend tool call intercepted
routing memory write tool call
memory write tool call completed
ACP backend tool result returned
[<tool-call-id>] call iota_memory_write args=...
[<tool-call-id>] result iota_memory_write ok=true value=...
[memory:inject] id=- payload=...
```

说明：如果某个后端通过 session/new 注入的 `context-mcp` stdio server 调用工具，而不是走 ACP client-side intercept，则日志片段应包含：

```text
context MCP memory write tool call received
context MCP memory write tool call completed
```

#### 10.3 backend tool search 日志链路

```powershell
.\target\release\iota.exe run --backend claude-code --trace `
  "请必须调用 iota_memory_search 工具一次，不要只口头回答。参数如下：
   query=\"exp01-full-log-probe-20260507\", limit=5, mode=hybrid。
   然后用一句话总结搜索结果。" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-search.txt
```

**必须出现的日志片段：**

```text
engine memory recall started
engine memory recall completed
engine memory inject event recorded
ACP backend tool call intercepted
routing memory search tool call
memory search tool call completed
ACP backend tool result returned
[<tool-call-id>] call iota_memory_search args=...
[<tool-call-id>] result iota_memory_search ok=true value=...
record_count=1
exp01-full-log-probe-20260507
```

如果走 `context-mcp` stdio server，则 search API 侧日志应为：

```text
context MCP memory search tool call received
context MCP memory search tool call completed
```

#### 10.4 Engine 自动 episodic 写入日志链路

该命令不要包含 `iota_memory_write` 字样，避免显式 memory tool prompt 跳过自动 episodic 写入。

```powershell
.\target\release\iota.exe run --backend codex --trace `
  "请用一句话回答：exp01-full-log-probe-20260507 普通 turn 日志探针已收到。" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-episodic.txt
```

**必须出现的日志片段：**

```text
engine memory recall started
engine memory recall completed
engine memory inject event recorded
engine episodic memory write started
engine episodic memory write completed
engine episodic memory compaction completed
[memory:inject] id=- payload=...
```

#### 10.5 自动检查日志文件

```powershell
Select-String -Path gefsi\logs\exp01-full-log-*.txt -Pattern `
  "engine memory recall started", `
  "engine memory recall completed", `
  "engine memory inject event recorded", `
  "ACP backend tool call intercepted", `
  "routing memory write tool call", `
  "memory write tool call completed", `
  "routing memory search tool call", `
  "memory search tool call completed", `
  "engine episodic memory write started", `
  "engine episodic memory write completed", `
  "\[memory:inject\]", `
  "call iota_memory_", `
  "result iota_memory_"
```

判定标准：

- write 日志文件至少包含 `iota_memory_write` 的 call/result、memory write route、tool result returned
- search 日志文件至少包含 `iota_memory_search` 的 call/result、memory search route、record_count
- episodic 日志文件至少包含 engine episodic write started/completed
- 三个日志文件都包含 engine recall 和 memory inject

#### 10.6 EventStore 持久化验证

查看最近执行，找到上述三个 probe 的 execution_id：

```powershell
.\target\release\iota.exe observability logging recent --limit 10 `
  | Tee-Object gefsi\logs\exp01-full-log-recent.json
```

任选 Step 10.2 或 10.3 的 execution_id，查看完整事件流：

```powershell
$recent = Get-Content gefsi\logs\exp01-full-log-recent.json -Raw | ConvertFrom-Json
$execId = ($recent | Where-Object { $_.backend -eq "claude-code" } | Select-Object -First 1).execution_id
.\target\release\iota.exe observability logging events $execId `
  | Tee-Object gefsi\logs\exp01-full-log-events.json
```

**必须在事件流中看到：**

```text
state started
memory inject
tool_call iota_memory_write 或 tool_call iota_memory_search
tool_result iota_memory_write 或 tool_result iota_memory_search
output
```

推荐用 PowerShell 快速筛选：

```powershell
Get-Content gefsi\logs\exp01-full-log-events.json -Raw `
  | Select-String -Pattern '"event_type": "memory"', '"event_type": "tool_call"', '"event_type": "tool_result"', 'iota_memory_'
```

#### 10.7 DB 侧确认

```powershell
sqlite3 "$env:USERPROFILE\.i6\context\memory.sqlite" `
  "SELECT id, type, facet, scope, scope_id, confidence, substr(content,1,80)
   FROM memory
   WHERE content LIKE '%exp01-full-log-probe-20260507%'
   ORDER BY updated_at DESC;"
```

预期至少包含：

- Step 10.2 写入的 `semantic/domain/project/iota-sympantos` 记录
- Step 10.4 自动写入的 `episodic/session/<session_id>` 记录

#### 10.8 清理

```powershell
sqlite3 "$env:USERPROFILE\.i6\context\memory.sqlite" `
  "DELETE FROM memory WHERE content LIKE '%exp01-full-log-probe-20260507%';"

Remove-Item Env:RUST_LOG -ErrorAction SilentlyContinue
```

**本用例通过标准：**

| 链路 | 证据 | 通过标准 |
|------|------|----------|
| Engine Recall | tracing + trace | 出现 recall started/completed 和 `[memory:inject]` |
| Backend Tool Call | tracing + `--trace` | 出现 ACP intercepted、`call iota_memory_*` |
| Memory API Route | tracing | 出现 routing/context MCP memory search/write received/completed |
| Tool Result | tracing + `--trace` + EventStore | 出现 result returned、`ToolResult ok=true` |
| Engine Episodic | tracing + DB | 出现 episodic write completed，DB 有 marker episodic |
| Event Persistence | observability events | 事件流包含 memory/tool_call/tool_result/output |

---

## 四、验收矩阵

| # | 验收项 | 步骤 | 判定标准 | 结果 |
|---|--------|------|----------|------|
| 1 | identity 跨后端延续 | Step 2 | codex 回复含 "Sympantos" | ☑ |
| 2 | preference 跨后端延续 | Step 3 | preference 桶含 "中文/Markdown" | ☑ |
| 3 | strategic+domain 跨后端延续 | Step 4 | hermes 提及 Q2 + SQLite/SHA-256 | ☑ |
| 4 | procedural+episodic 延续 | Step 5 | opencode 覆盖步骤和经历 | ☑ |
| 5 | contentHash 去重 | Step 6 | 仍 1 行，updated_at 变更 | ☑ |
| 6 | confidence 过滤（identity） | Step 7 | 0.50 不在注入桶中 | ☑ |
| 7 | confidence 过滤（procedural） | Step 7 | 0.60 不在注入桶中 | ☑ |
| 8 | token budget 截断 | Step 8 | truncated=true, excluded=3 | ☑ |
| 9 | SQLite schema 合规 | Step 1 | type/facet/scope/scope_id 正确 | ☑ |
| 10 | trace 事件完整性 | Step 2~5 | `--trace` 输出含 `[memory:inject]` | ☑ |
| 11 | EventStore 持久化 | Step 9.1 | events 含 Memory 事件 | ☑ |
| 12 | Logging 多后端覆盖 | Step 9.1 | recent 含 5 个后端 | ☑ |
| 13 | Tracing 延迟分解 | Step 9.2 | breakdown 含 5 阶段 | ☑ |
| 14 | Metrics 指标可查 | Step 9.3 | total=39 > 0 | ☑ |
| 15 | Prometheus 导出 | Step 9.3 | 含 `iota_execution_attempts_total` | ☑ |

**结论：15/15 全部通过。**

---

## 五、清理

```bash
sqlite3 ~/.i6/context/memory.sqlite \
  "DELETE FROM memory WHERE scope_id IN (
    'local-user', 'iota-sympantos'
  ) OR scope_id LIKE '%iota-sympantos';"
```
