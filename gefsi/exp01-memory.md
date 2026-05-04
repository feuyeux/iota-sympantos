# iota-sympantos 实验1：跨后端记忆延续验证

**实验代号：** exp01-memory  
**日期：** 2026-05-05  
**参考规范：** iota-guides/08-memory.md v2.1  
**存储层：** SQLite（`~/.i6/context/memory.sqlite`，Rust `memory.rs`）

---

## 一、实验目标

验证 iota-sympantos 的 Memory 系统在**多后端切换场景**下的核心主张：

> Engine 层（Rust）负责 Extract / Store / Recall / Inject，后端（claude-code / codex / gemini / hermes / opencode）可替换，记忆不应丢失。

验收点：
1. 在后端 A 写入的记忆，后端 B 能完整召回并注入 context
2. 六类记忆桶均可正确存储和注入（semantic×4 + procedural + episodic）
3. contentHash（SHA-256）去重有效——重复写入不产生新行
4. confidence + scope 过滤生效（低于 minConfidence 的条目不注入）
5. token budget（`memory_chars: 2000`）截断行为可观测

---

## 二、实验环境

```
项目路径：   /Users/han/coding/iota-sympantos
config:      ~/.i6/nimia.yaml
memory DB:   ~/.i6/context/memory.sqlite
skill roots: ~/.i6/skills, ./.iota/skills
```

**后端顺序：** claude-code → codex → gemini → hermes → opencode

**测试约定：**
```
scope_id (user):    "user-sympantos"
scope_id (project): "iota-sympantos"
scope_id (session): 每次运行自动生成
```

---

## 三、实验步骤

### Step 0 — 环境准备

```bash
# 确认 iota-sympantos binary 已编译
cargo build --release 2>&1 | tail -3

# 备份并清理测试相关 memory 行（保留其他用户数据）
sqlite3 ~/.i6/context/memory.sqlite \
  "DELETE FROM memories WHERE scope_id IN ('user-sympantos','iota-sympantos');"

# 验证清空
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT count(*) FROM memories WHERE scope_id IN ('user-sympantos','iota-sympantos');"
# 期望: 0
```

---

### Step 1 — 写入阶段（claude-code）

依次触发 6 类记忆写入：

```bash
cd /Users/han/coding/iota-sympantos

# 1-A semantic/identity
iota run --backend claude-code --trace \
  "我叫 Sympantos，是 iota-sympantos 的实验用户，负责跨后端记忆验证"

# 1-B semantic/preference
iota run --backend claude-code --trace \
  "我偏好中文回答，实验日志用英文，报告格式为 Markdown"

# 1-C semantic/strategic
iota run --backend claude-code --trace \
  "本项目目标：在 2026 Q2 完成 iota-sympantos 跨后端记忆延续的完整验证，覆盖 6 类记忆桶"

# 1-D semantic/domain
iota run --backend claude-code --trace \
  "iota-sympantos 使用 SQLite 作为 memory 存储层，Rust 实现 Engine，SHA-256 做内容去重"

# 1-E procedural
iota run --backend claude-code --trace \
  "实验步骤：1) 清理 SQLite 测试行  2) claude-code 写入  3) 切换后端召回  4) 检查 visibility"

# 1-F episodic
iota run --backend claude-code --trace \
  "本轮 Step1 已通过 claude-code 完成 5 类记忆写入，准备切换后端验证召回"
```

**检查点 1.1** — 验证 SQLite 写入：

```bash
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT type, facet, scope, scope_id, substr(content,1,40) as content_preview, confidence
   FROM memories
   WHERE scope_id IN ('user-sympantos','iota-sympantos')
   ORDER BY created_at;"
# 期望: 5~6 行，涵盖 semantic/procedural/episodic
```

---

### Step 2 — identity 召回（codex）

```bash
iota run --backend codex --trace "我是谁？请介绍你对我的了解"
```

**预期：** 回复中出现 "Sympantos"、"跨后端记忆验证" 等 Step 1-A 写入的内容。

**检查点 2.1** — 观察 `--trace` 输出中的 `[memory:inject]` 段，确认 identity bucket 被选中。

---

### Step 3 — preference 召回（gemini）

```bash
iota run --backend gemini --trace "你知道我的回答语言偏好和报告格式吗？"
```

**预期：** 回复为中文，且提及"英文日志"和"Markdown 格式"。

---

### Step 4 — strategic + domain 召回（hermes）

```bash
iota run --backend hermes --trace "告诉我当前项目的目标和技术实现"
```

**预期：** 提及 Q2 目标、SQLite 存储层、SHA-256 去重。

---

### Step 5 — procedural + episodic 召回（opencode）

```bash
iota run --backend opencode --trace "回顾实验步骤，以及本次实验发生了什么"
```

**预期：** 覆盖 4 步实验流程（procedural）和 Step1 的经历叙述（episodic）。

---

### Step 6 — contentHash 去重验证

回到 claude-code，重复写入 Step 1-A 完全相同的内容：

```bash
iota run --backend claude-code --trace \
  "我叫 Sympantos，是 iota-sympantos 的实验用户，负责跨后端记忆验证"
```

**检查点 6.1** — 确认 SQLite 未新增行，仅 updated_at 更新：

```bash
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT id, content_hash, created_at, updated_at
   FROM memories
   WHERE scope_id='user-sympantos' AND type='semantic' AND facet='identity';"
# 期望: 仍只有 1 行，updated_at > created_at
```

---

### Step 7 — token budget 截断验证

```bash
iota run --backend claude-code --trace \
  "请列出你知道的关于我和本项目的所有信息，包括身份、偏好、目标、技术栈、操作步骤、历史经历"
```

**检查点 7.1** — 若 memory_chars=2000 被触及，`--trace` 输出中应出现截断标记或 excluded 记录。

---

## 四、验收矩阵

| 验收项 | 步骤 | 判定标准 |
|---|---|---|
| identity 跨后端延续 | Step 2 | codex 回复含 "Sympantos" |
| preference 跨后端延续 | Step 3 | gemini 中文回复，提及日志/格式偏好 |
| strategic+domain 跨后端延续 | Step 4 | hermes 提及 Q2 目标和 SQLite/SHA-256 |
| procedural+episodic 延续 | Step 5 | opencode 覆盖 4 步骤和 Step1 经历 |
| contentHash 去重 | Step 6 | identity 表只有 1 行，updated_at 变更 |
| confidence 过滤生效 | Step 1~5 | trace 中低 confidence 条目不出现在 inject 段 |
| token budget 截断 | Step 7 | trace 中出现截断 / excluded 信息 |
| SQLite schema 合规 | 检查点 1.1 | type/facet/scope/scope_id 字段与 memory.rs 一致 |

---

## 五、观测命令速查

```bash
# 查看所有测试记忆（按类型分组统计）
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT type, facet, scope, count(*) as cnt FROM memories
   WHERE scope_id IN ('user-sympantos','iota-sympantos')
   GROUP BY type, facet, scope;"

# 查看某条记忆完整内容
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT * FROM memories WHERE id='<uuid>' LIMIT 1;"

# 查看 updated_at 变更情况（去重验证）
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT id, content_hash, created_at, updated_at, confidence
   FROM memories ORDER BY created_at DESC LIMIT 10;"
```

---

## 六、已知局限

| 局限 | 来源 | 影响 |
|---|---|---|
| LLM Extractor 无 ADD/UPDATE/NONE 合并决策 | memory.rs 启发式抽取 | 抽取精度有限，可能漏写 |
| 无向量检索（无 embedding） | iota-sympantos 当前实现 | 召回依赖精确匹配/scope 过滤 |
| episodic 无 session close compaction | 待完善 | 长会话后 episodic 累积 |
| budget 截断后无详细 excluded 列表 | trace 输出待完善 | 截断行为可观测性有限 |

---

*生成时间：2026-05-05 | 参考：iota-guides/08-memory.md v2.1*
