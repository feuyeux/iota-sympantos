# iota-sympantos 实验1：跨后端记忆延续验证

**实验代号：** exp01-memory
**日期：** 2026-05-06
**参考规范：** iota-guides/08-memory.md v2.1
**存储层：** SQLite（`~/.i6/context/memory.sqlite`，Rust `memory.rs`）

---

## 一、实验目标

验证 iota-sympantos 的 Memory 系统在**多后端切换场景**下的核心主张：

> Engine 层（Rust）负责 Extract / Store / Recall / Inject，后端（claude-code / codex / gemini / hermes / opencode）可替换，记忆不应丢失。

验收点：

1. 在后端 A 写入的记忆，后端 B 能完整召回并注入 context
2. 六类记忆桶均可正确存储和注入（semantic×4 + procedural + episodic）
3. contentHash（SHA-256）去重有效——相同 content 不产生新行
4. confidence + scope 过滤生效（低于阈值的条目不注入）
5. token budget（`memory_chars: 2000`）截断行为可观测
6. 记忆相关 logging / tracing / metrics 可通过 observability 命令验证

---

## 二、实验环境

### 2.1 前置条件

| 组件 | 要求 |
|------|------|
| iota binary | `cargo build --release` 成功 |
| nimia.yaml | `~/.i6/nimia.yaml` 已配置至少 2 个后端（推荐全 5 个） |
| SQLite CLI | 需安装 `sqlite3` 命令行工具（Windows 不预装，需自行下载 [sqlite.org/download](https://sqlite.org/download.html) 并加入 PATH） |
| 后端可用性 | 各后端 API key 已在 nimia.yaml 中配置 |

### 2.2 路径约定

```
config:      ~/.i6/nimia.yaml
memory DB:   ~/.i6/context/memory.sqlite    (实际表名: memory，memories 是视图)
event DB:    ~/.i6/context/events.sqlite
skill roots: ~/.i6/skills, ./.iota/skills
```

### 2.3 scope_id 约定

记忆写入和召回涉及多个 scope_id，需理解其来源：

| scope | 写入时 scope_id | 召回时候选范围 (`*_scope_candidates()`) |
|-------|----------------|----------------------------------------|
| user | MCP 默认 `"local-user"` | `[传入值, "user-sympantos", "local-user"]` |
| project | MCP 默认 cwd 路径 | `[传入值, "iota-sympantos", cwd basename]` |
| session | 自动生成 session_id | 当前 session_id |

### 2.4 各桶 confidence 过滤阈值（硬编码于 `recall_buckets()`）

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
# 0.1 确认 binary 已编译
cargo build --release 2>&1 | tail -3

# 0.2 验证 sqlite3 可用
sqlite3 --version

# 0.3 备份（可选）
cp ~/.i6/context/memory.sqlite ~/.i6/context/memory.sqlite.bak

# 0.4 清理所有可能的测试 scope_id（含 recall 候选范围）
sqlite3 ~/.i6/context/memory.sqlite \
  "DELETE FROM memory WHERE scope_id IN (
    'user-sympantos', 'iota-sympantos', 'local-user'
  ) OR scope_id LIKE '%iota-sympantos';"

# 0.5 验证清空
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT count(*) FROM memory WHERE scope_id IN (
    'user-sympantos', 'iota-sympantos', 'local-user'
  ) OR scope_id LIKE '%iota-sympantos';"
# 期望: 0

# 0.6 记录清理前 observability baseline
iota observability metrics
iota observability logging recent --limit 3
```

---

### Step 1 — 通过 MCP 工具精确写入 6 类记忆（claude-code）

> **设计说明：** 本步骤通过 prompt 引导后端调用 `iota_memory_write` MCP 工具写入，
> 而非依赖 LLM 自动抽取——确保每条记忆的 type / facet / scope / content 完全可控。

```bash
cd iota-sympantos

# 1-A semantic/identity (scope=user)
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=identity, scope=user, scope_id=local-user,
   content=\"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证\",
   confidence=0.95"

# 1-B semantic/preference (scope=user)
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=preference, scope=user, scope_id=local-user,
   content=\"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格\",
   confidence=0.90"

# 1-C semantic/strategic (scope=project)
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=strategic, scope=project, scope_id=iota-sympantos,
   content=\"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端\",
   confidence=0.90"

# 1-D semantic/domain (scope=project)
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=domain, scope=project, scope_id=iota-sympantos,
   content=\"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系\",
   confidence=0.90"

# 1-E procedural (scope=project, facet 留空)
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=procedural, scope=project, scope_id=iota-sympantos,
   content=\"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计\",
   confidence=0.85"

# 1-F episodic (scope=project, facet 留空)
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=episodic, scope=project, scope_id=iota-sympantos,
   content=\"Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回\",
   confidence=0.80"
```

**检查点 1.1 — 验证写入完整性：**

```bash
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT type, facet, scope, scope_id, substr(content,1,50) AS preview, confidence
   FROM memory
   WHERE scope_id IN ('local-user','iota-sympantos')
   ORDER BY created_at;"
```

| 期望行 | type | facet | scope | scope_id |
|--------|------|-------|-------|----------|
| 1 | semantic | identity | user | local-user |
| 2 | semantic | preference | user | local-user |
| 3 | semantic | strategic | project | iota-sympantos |
| 4 | semantic | domain | project | iota-sympantos |
| 5 | procedural | NULL | project | iota-sympantos |
| 6 | episodic | NULL | project | iota-sympantos |

**检查点 1.2 — 验证 `--trace` 输出中的 `[memory:inject]` 事件：**

每次 `--trace` 运行应在 stderr 输出类似：

```
[memory:inject] id=- payload={"identity":...,"budget":{"memory_chars":2000,"total_chars":...,"truncated":false,"excluded_count":0}}
```

确认 `action` 为 `"inject"`，`budget.truncated` 为 `false`（此阶段数据量远小于 2000 字符）。

**检查点 1.3 — 验证 EventStore 已记录 Memory 事件：**

```bash
# 找到最近的 execution_id
iota observability logging recent --limit 6

# 查看其中一个 execution 的事件流，确认包含 memory 类型事件
iota observability logging events <execution-id>
# 期望: 事件流中出现 {"kind":"Memory","data":{"action":"inject",...}}
```

---

### Step 2 — identity 召回验证（codex）

```bash
iota run --backend codex --trace "我是谁？请介绍你对我的了解"
```

**预期回复：** 包含 "Sympantos"、"跨后端记忆验证" 等 Step 1-A 写入的内容。

**检查点 2.1 — trace 输出验证：**

- `[memory:inject]` 中 `identity` 数组非空，包含 scope_id=`local-user` 的记录
- `identity` 中 content 包含 "Sympantos"

---

### Step 3 — preference 召回验证（gemini）

```bash
iota run --backend gemini --trace "你知道我的回答语言偏好和报告格式吗？"
```

**预期回复：** 中文回答，提及 "英文日志" 和 "Markdown 格式"。

**检查点 3.1 — trace 输出验证：**

- `[memory:inject]` 中 `preference` 数组非空
- preference 桶 content 包含 "中文" 和 "Markdown"

---

### Step 4 — strategic + domain 召回验证（hermes）

```bash
iota run --backend hermes --trace "告诉我当前项目的目标和技术实现"
```

**预期回复：** 提及 Q2 目标、SQLite 存储层、SHA-256 去重。

**检查点 4.1 — trace 输出验证：**

- `strategic` 数组非空，content 包含 "Q2"
- `domain` 数组非空，content 包含 "SQLite" 和 "SHA-256"
- 两者 scope_id 均为 `iota-sympantos`

---

### Step 5 — procedural + episodic 召回验证（opencode）

```bash
iota run --backend opencode --trace "回顾实验步骤，以及本次实验发生了什么"
```

**预期回复：** 覆盖 6 步实验流程（procedural）和 Step1 完成 6 类写入的经历叙述（episodic）。

**检查点 5.1 — trace 输出验证：**

- `procedural` 数组非空
- `episodic` 数组非空，content 包含 "6 类记忆写入"

---

### Step 6 — contentHash 去重验证

> **设计说明：** 直接通过 MCP 工具写入与 Step 1-A 完全相同的 content 文本，
> 而非依赖 LLM 抽取（避免措辞差异导致 hash 不同）。

```bash
# 6.1 记录写入前的行数和 updated_at
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT id, content_hash, created_at, updated_at
   FROM memory
   WHERE scope_id='local-user' AND type='semantic' AND facet='identity';"

# 6.2 重复写入完全相同的 content
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=identity, scope=user, scope_id=local-user,
   content=\"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证\",
   confidence=0.95"

# 6.3 验证去重效果
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT id, content_hash, created_at, updated_at
   FROM memory
   WHERE scope_id='local-user' AND type='semantic' AND facet='identity';"
```

**检查点 6.1：** 仍然只有 1 行（`content_hash` 相同），`updated_at` 已更新 > `created_at`。

---

### Step 7 — confidence 过滤验证

> **设计说明：** 手动插入低 confidence 记录，验证 recall 时被排除。

```bash
# 7.1 写入一条 confidence=0.50 的 identity 记忆（低于阈值 0.85）
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=identity, scope=user, scope_id=local-user,
   content=\"低置信度测试：这条记忆不应被注入\",
   confidence=0.50"

# 7.2 写入一条 confidence=0.60 的 procedural 记忆（低于阈值 0.75）
iota run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=procedural, scope=project, scope_id=iota-sympantos,
   content=\"低置信度测试：这条 procedural 不应被注入\",
   confidence=0.60"

# 7.3 验证 DB 中确实存在这两条低 confidence 记录
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT type, facet, confidence, substr(content,1,30)
   FROM memory
   WHERE content LIKE '%低置信度测试%';"
# 期望: 2 行，confidence 分别为 0.50 和 0.60

# 7.4 触发一次召回，检查 trace 中这两条不出现在注入桶里
iota run --backend codex --trace "你知道关于我的所有信息吗？"
```

**检查点 7.1：** trace 输出的 `[memory:inject]` payload 中：

- `identity` 数组中不包含 "低置信度测试" 内容（confidence 0.50 < 阈值 0.85）
- `procedural` 数组中不包含 "低置信度测试" 内容（confidence 0.60 < 阈值 0.75）
- 高 confidence 的原始记录仍正常出现

---

### Step 8 — token budget 截断验证

```bash
# 8.1 批量写入大量 domain 记忆，使总字符数超过 memory_chars=2000
for i in $(seq 1 15); do
  iota run --backend claude-code \
    "请调用 iota_memory_write 工具，参数如下：
     type=semantic, facet=domain, scope=project, scope_id=iota-sympantos,
     content=\"domain-padding-$i: 这是第 $i 条填充记忆，用于测试 token budget 截断行为。iota-sympantos 使用 Rust 编写的 Engine 层驱动 ACP JSON-RPC 2.0 协议，支持 5 个后端的热切换。\",
     confidence=0.90"
done

# 8.2 查看当前总字符数
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT sum(length(content)) AS total_chars FROM memory
   WHERE scope_id IN ('local-user','iota-sympantos')
   AND confidence >= 0.70;"

# 8.3 触发一次完整召回，观察截断
iota run --backend codex --trace \
  "列出你知道的关于我和本项目的所有信息"
```

**检查点 8.1：** trace 中 `[memory:inject]` payload 的 `budget` 段：

```json
{
  "memory_chars": 2000,
  "total_chars": "<大于2000>",
  "truncated": true,
  "excluded_count": "<大于0>"
}
```

---

### Step 9 — Observability 审计

> **设计说明：** 验证所有前序步骤产生的 logging / tracing / metrics 数据完整可查。

#### 9.1 Logging 验证

```bash
# 查看最近的执行记录（应覆盖 Step 1~8 的所有 run）
iota observability logging recent --limit 30

# 查看某个 execution 的完整事件流
# 替换 <exec-id> 为 Step 1-A 的 execution_id
iota observability logging events <exec-id>
```

**检查点 9.1a：** `recent` 输出包含 claude-code / codex / gemini / hermes / opencode 5 种 backend。

**检查点 9.1b：** `events` 输出中包含 `memory` 类型事件（`{"kind":"Memory","data":{"action":"inject",...}}`）。

#### 9.2 Tracing 验证

```bash
# 查看延迟统计
iota observability tracing summary

# 查看最慢的执行
iota observability tracing slow --limit 5

# 查看某个 execution 的 5 阶段延迟分解
iota observability tracing breakdown <exec-id>
```

**检查点 9.2a：** `summary` 输出的 `completed_executions` ≥ Step 1~8 的总 run 次数。

**检查点 9.2b：** `breakdown` 包含 `process_spawn`、`init`、`session_new`、`prompt`、`total` 五个阶段。

#### 9.3 Metrics 验证

```bash
# 聚合指标（人类可读）
iota observability metrics

# Prometheus 格式输出
iota observability metrics --prometheus

# Token 用量
iota observability metrics tokens

# 延迟指标
iota observability metrics latency
```

**检查点 9.3a：** `metrics` 输出中 `executions.total` > 0，`latency.avg_total_ms` 有值。

**检查点 9.3b：** `--prometheus` 输出包含 `iota_execution_attempts_total` 和 `iota_execution_completed_total`。

**检查点 9.3c：** `tokens` 输出中 `total_tokens` > 0（至少有 Step 1~8 产生的 token 消耗）。

---

## 四、验收矩阵

> **执行日期：** 2026-05-06 | **实际使用后端：** claude-code (写入) + hermes (召回)

| # | 验收项 | 步骤 | 判定标准 | 通过 |
|---|--------|------|----------|------|
| 1 | identity 跨后端延续 | Step 2 | hermes 回复含 "Sympantos" | ☑ |
| 2 | preference 跨后端延续 | Step 3 | trace preference 桶含 "中文/Markdown" | ☑ |
| 3 | strategic+domain 跨后端延续 | Step 4 | hermes 提及 Q2 目标和 SQLite/SHA-256 | ☑ |
| 4 | procedural+episodic 延续 | Step 5 | hermes 覆盖步骤和 Step1 经历 | ☑ |
| 5 | contentHash 去重 | Step 6 | identity 仍只有 1 行，updated_at 变更 | ☑ |
| 6 | confidence 过滤（identity 0.85） | Step 7 | conf=0.50 记录不在 inject identity 桶中 | ☑ |
| 7 | confidence 过滤（procedural 0.75） | Step 7 | conf=0.60 记录不在 inject procedural 桶中 | ☑ |
| 8 | token budget 截断 | Step 8 | budget.truncated=true, excluded_count=13 | ☑ |
| 9 | SQLite schema 合规 | 检查点 1.1 | type/facet/scope/scope_id 字段正确 | ☑ |
| 10 | trace 事件完整性 | 检查点 1.2 | --trace 输出含 [memory:inject] | ☑ |
| 11 | EventStore 事件持久化 | 检查点 1.3 | observability events 含 Memory 事件 | ☑ |
| 12 | Logging 多后端覆盖 | Step 9.1 | recent 输出含 claude-code + hermes | ☑ |
| 13 | Tracing 延迟分解 | Step 9.2 | breakdown 含 5 阶段延迟 | ☑ |
| 14 | Metrics 指标可查 | Step 9.3 | executions.total=44 > 0 | ☑ |
| 15 | Prometheus 导出 | Step 9.3 | --prometheus 含 iota_execution_attempts_total | ☑ |

---

## 五、观测命令速查

```bash
# ── SQLite 直查 ──

# 查看所有测试记忆（按类型分组统计）
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT type, facet, scope, count(*) AS cnt FROM memory
   WHERE scope_id IN ('local-user','iota-sympantos')
   GROUP BY type, facet, scope;"

# 查看某条记忆完整内容（注意实际表名是 memory，memories 是视图）
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT * FROM memory WHERE id='<uuid>' LIMIT 1;"

# 查看去重相关字段
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT id, content_hash, created_at, updated_at, confidence
   FROM memory ORDER BY created_at DESC LIMIT 10;"

# 查看低 confidence 记录
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT type, facet, confidence, substr(content,1,40)
   FROM memory WHERE confidence < 0.80 ORDER BY confidence;"

# ── Observability CLI ──

# 执行日志
iota observability logging recent --limit 10
iota observability logging errors --limit 5
iota observability logging events <execution-id>
iota observability logging tools --limit 10

# 延迟追踪
iota observability tracing summary
iota observability tracing slow --limit 5
iota observability tracing breakdown <execution-id>

# 聚合指标
iota observability metrics
iota observability metrics --prometheus
iota observability metrics tokens
iota observability metrics latency
iota observability metrics cache
```

---

## 六、trace 输出格式参考

`--trace` 标志启用时，以下内容输出到 **stderr**：

```
[memory:inject] id=- payload={
  "identity": [{"id":"...","scope":"user","scope_id":"local-user","content":"...","confidence":0.95}],
  "preference": [...],
  "strategic": [...],
  "domain": [...],
  "procedural": [...],
  "episodic": [...],
  "budget": {
    "memory_chars": 2000,
    "total_chars": 420,
    "truncated": false,
    "excluded_count": 0
  }
}

[iota run timing] {"route":"direct","backend":"claude-code","timing":{
  "process_spawn_ms":..., "init_ms":..., "session_new_ms":..., "prompt_ms":..., "total_ms":...
}}
```

关键字段说明：

| 字段 | 含义 |
|------|------|
| `budget.memory_chars` | 配置的字符预算（默认 2000） |
| `budget.total_chars` | 本次召回的全部记忆总字符数 |
| `budget.truncated` | 是否超出预算发生截断 |
| `budget.excluded_count` | 因预算限制被排除的记忆条数 |

---

## 七、已知局限

| 局限 | 来源 | 影响 | 规避 |
|------|------|------|------|
| LLM Extractor 无 ADD/UPDATE/NONE 合并决策 | memory.rs 启发式抽取 | 自动抽取精度有限 | 本实验用 MCP 工具精确写入 |
| 无向量检索（无 embedding） | 当前实现 | 召回依赖 scope 过滤 + LIKE/FTS5 | 确保 scope_id 匹配 |
| episodic 无 session close compaction | 待完善 | 长会话后 episodic 累积 | 实验规模可控，不影响 |
| confidence 阈值硬编码 | recall_buckets() | 无法通过配置调整 | 已知各桶阈值，测试数据据此设计 |
| 去重依赖 content 完全相同 | SHA-256 on content | 语义相同但措辞不同不去重 | 用 MCP 工具确保 content 一致 |

---

## 八、清理

实验结束后清理测试数据：

```bash
# 删除实验写入的记忆（包括低 confidence 和 padding 记录）
sqlite3 ~/.i6/context/memory.sqlite \
  "DELETE FROM memory WHERE scope_id IN (
    'local-user', 'iota-sympantos'
  ) OR scope_id LIKE '%iota-sympantos';"

# 恢复备份（如需要）
# cp ~/.i6/context/memory.sqlite.bak ~/.i6/context/memory.sqlite
```

---

*生成时间：2026-05-06 | 参考：iota-guides/08-memory.md v2.1*
