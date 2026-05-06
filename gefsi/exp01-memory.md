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

- 背景：原 `sqlite3` 可执行文件位于 `d:/zoo/Android/Sdk/platform-tools/sqlite3.exe`，版本 `3.44.3 (32-bit)`，`pragma compile_options` 未出现 `ENABLE_FTS5`，导致对当前库执行变更语句时报 `no such module: fts5`。
- 处理过程：
  - 通过官方直链下载 `https://www.sqlite.org/2026/sqlite-tools-win-x64-3530100.zip`
  - 解压到 `C:/Users/feuye/tools/sqlite/`
  - 使用新二进制 `C:/Users/feuye/tools/sqlite/sqlite3.exe`
- 安装后验证：
  - 版本：`3.53.1 (64-bit)`
  - `pragma compile_options` 包含：`ENABLE_FTS3` / `ENABLE_FTS4` / `ENABLE_FTS5`
  - 功能烟测通过：

```sql
CREATE VIRTUAL TABLE t USING fts5(c);
INSERT INTO t(c) VALUES('hello fts5');
SELECT rowid, c FROM t WHERE t MATCH 'hello';

-- 返回 1 行: hello fts5
```

- 当前会话已切换：PowerShell 中 `sqlite3` 已通过 alias 指向 `C:/Users/feuye/tools/sqlite/sqlite3.exe`。

> 注：按“每步结束后停下来确认”的方式执行。

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
```

执行结果

```
    Finished `release` profile [optimized] target(s) in 0.40s
```

```bash
# 0.2 验证 sqlite3 可用
Set-Alias sqlite3 "C:\Users\feuye\tools\sqlite\sqlite3.exe"
sqlite3 --version
```

执行结果

```
3.53.1 2026-05-05 10:34:17 (64-bit)
```

```bash
# 0.3 备份（可选）
#cp ~/.i6/context/memory.sqlite ~/.i6/context/memory.sqlite.bak

# 0.4 清理所有可能的测试 scope_id（含 recall 候选范围）

sqlite3 ~/.i6/context/memory.sqlite \
  "DELETE FROM memory WHERE scope_id IN (
    'user-sympantos', 'iota-sympantos', 'local-user'
  ) OR scope_id LIKE '%iota-sympantos';"
```

执行结果

```
（无输出，执行成功）
```

```bash
# 0.5 验证清空
sqlite3 ~/.i6/context/memory.sqlite \
  "SELECT count(*) FROM memory WHERE scope_id IN (
    'user-sympantos', 'iota-sympantos', 'local-user'
  ) OR scope_id LIKE '%iota-sympantos';"
```

执行结果

```
0
```

---

### Step 1 — 通过 MCP 工具精确写入 6 类记忆（claude-code）

> **设计说明：** 本步骤通过 prompt 引导后端调用 `iota_memory_write` MCP 工具写入，
> 而非依赖 LLM 自动抽取——确保每条记忆的 type / facet / scope / content 完全可控。

> **执行前处理：** 发现直接 `iota run` 会复用 cwd 最近 session，导致旧 session episodic 记忆被注入；已改为非 daemon `run` 每进程使用新 iota session。补充清理旧实验 session episodic 记录后，验证相关测试记录 count=0。

> **1-E 修复：** 首次执行时 backend 将 procedural 写入默认 cwd scope。原因是 context capsule 将 project scope_id 默认值表达为固定值；已改为“用户未指定 scope_id 时的默认值”，重建后重跑 1-E，DB 验证 `scope_id=iota-sympantos`。

```bash
cd iota-sympantos

# 1-A semantic/identity (scope=user)
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=identity, scope=user, scope_id=local-user,
   content=\"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证\",
   confidence=0.95"
```

执行结果

```
已完成记忆写入，ID: `b8517b2e-be70-425e-b57b-aecf67ec2630`

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":0,"truncated":false},"domain":[],"episodic":[],"identity":[],"preference":[],"procedural":[],"strategic":[]}
```

```bash
# 1-B semantic/preference (scope=user)
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=preference, scope=user, scope_id=local-user,
   content=\"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格\",
   confidence=0.90"
```

执行结果

```
已成功写入用户偏好记忆：

- 类型: semantic / preference
- 内容: 偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格
- 置信度: 0.90
- 记录ID: 595bbb72-08c9-4433-97e2-bde10db826d1

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":48,"truncated":false},"domain":[],"episodic":[],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[],"procedural":[],"strategic":[]}
```

```bash
# 1-C semantic/strategic (scope=project)
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=strategic, scope=project, scope_id=iota-sympantos,
   content=\"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端\",
   confidence=0.90"
```

执行结果

```
已写入项目记忆：`iota-sympantos` 作用域，ID: `1ba45094`。

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":86,"truncated":false},"domain":[],"episodic":[],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[],"strategic":[]}
```

```bash
# 1-D semantic/domain (scope=project)
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=domain, scope=project, scope_id=iota-sympantos,
   content=\"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系\",
   confidence=0.90"
```

执行结果

```
已将 domain 类别语义记忆写入项目范围，ID: `eb2a22d0-21e9-45b1-83a2-db0c384b718f`。

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":129,"truncated":false},"domain":[],"episodic":[],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[],"strategic":[{"confidence":0.9,"content":"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端","id":"1ba45094-cd1d-4b3a-aa45-7a456af50ecb","scope":"project","scope_id":"iota-sympantos"}]}
```

```bash
# 1-E procedural (scope=project, facet 留空)
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=procedural, scope=project, scope_id=iota-sympantos,
   content=\"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计\",
   confidence=0.85"
```

执行结果

```
已写入 project scope，memory_id: `605134c9-9173-485e-bbaf-0029300b7f34`，merge_mode=auto。

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":204,"truncated":false},"domain":[{"confidence":0.9,"content":"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系","id":"eb2a22d0-21e9-45b1-83a2-db0c384b718f","scope":"project","scope_id":"iota-sympantos"}],"episodic":[],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[],"strategic":[{"confidence":0.9,"content":"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端","id":"1ba45094-cd1d-4b3a-aa45-7a456af50ecb","scope":"project","scope_id":"iota-sympantos"}]}

DB 验证：605134c9-9173-485e-bbaf-0029300b7f34|procedural||project|iota-sympantos|0.85
```

```bash
# 1-F episodic (scope=project, facet 留空)
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=episodic, scope=project, scope_id=iota-sympantos,
   content=\"Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回\",
   confidence=0.80"
```

执行结果

```
已写入，ID: `c85def24-f984-4181-a328-9139de3a5307`，merge_mode=auto。

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":288,"truncated":false},"domain":[{"confidence":0.9,"content":"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系","id":"eb2a22d0-21e9-45b1-83a2-db0c384b718f","scope":"project","scope_id":"iota-sympantos"}],"episodic":[],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[{"confidence":0.85,"content":"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计","id":"605134c9-9173-485e-bbaf-0029300b7f34","scope":"project","scope_id":"iota-sympantos"}],"strategic":[{"confidence":0.9,"content":"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端","id":"1ba45094-cd1d-4b3a-aa45-7a456af50ecb","scope":"project","scope_id":"iota-sympantos"}]}

DB 验证：scope_id IN ('local-user','iota-sympantos') 下共有 6 条目标记忆：
- semantic/identity: user/local-user confidence=0.95
- semantic/preference: user/local-user confidence=0.90
- semantic/strategic: project/iota-sympantos confidence=0.90
- semantic/domain: project/iota-sympantos confidence=0.90
- procedural: project/iota-sympantos confidence=0.85
- episodic: project/iota-sympantos confidence=0.80
```

---

### Step 2 — identity 召回验证（codex）

```bash
.\target\release\iota.exe run --backend codex --trace "我是谁？请介绍你对我的了解"
```

执行结果

```
身份：
- 用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":406,"truncated":false},"domain":[{"confidence":0.9,"content":"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系","id":"eb2a22d0-21e9-45b1-83a2-db0c384b718f","scope":"project","scope_id":"iota-sympantos"}],"episodic":[{"confidence":0.8,"content":"Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回","id":"c85def24-f984-4181-a328-9139de3a5307","scope":"project","scope_id":"iota-sympantos"}],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[{"confidence":0.85,"content":"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计","id":"605134c9-9173-485e-bbaf-0029300b7f34","scope":"project","scope_id":"iota-sympantos"}],"strategic":[{"confidence":0.9,"content":"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端","id":"1ba45094-cd1d-4b3a-aa45-7a456af50ecb","scope":"project","scope_id":"iota-sympantos"}]}

判定：符合预期。回复包含 "Sympantos" 和跨后端记忆延续验证职责；[memory:inject] 的 identity 数组非空，包含 scope_id=`local-user` 的 Step 1-A 记录。
```

**预期回复：** 包含 "Sympantos"、"跨后端记忆验证" 等 Step 1-A 写入的内容。

- `[memory:inject]` 中 `identity` 数组非空，包含 scope_id=`local-user` 的记录
- `identity` 中 content 包含 "Sympantos"

---

### Step 3 — preference 召回验证（gemini）

```bash
.\target\release\iota.exe run --backend gemini --trace "你知道我的回答语言偏好和报告格式吗？"
```

执行结果

```
偏好：
- 偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":406,"truncated":false},"domain":[{"confidence":0.9,"content":"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系","id":"eb2a22d0-21e9-45b1-83a2-db0c384b718f","scope":"project","scope_id":"iota-sympantos"}],"episodic":[{"confidence":0.8,"content":"Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回","id":"c85def24-f984-4181-a328-9139de3a5307","scope":"project","scope_id":"iota-sympantos"}],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[{"confidence":0.85,"content":"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计","id":"605134c9-9173-485e-bbaf-0029300b7f34","scope":"project","scope_id":"iota-sympantos"}],"strategic":[{"confidence":0.9,"content":"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端","id":"1ba45094-cd1d-4b3a-aa45-7a456af50ecb","scope":"project","scope_id":"iota-sympantos"}]}

判定：符合预期。回复为中文，提及英文日志、Markdown 格式和 2 空格缩进；[memory:inject] 的 preference 数组非空，content 包含 "中文" 和 "Markdown"。
```

**预期回复：** 中文回答，提及 "英文日志" 和 "Markdown 格式"。

- `[memory:inject]` 中 `preference` 数组非空
- preference 桶 content 包含 "中文" 和 "Markdown"

---

### Step 4 — strategic + domain 召回验证（hermes）

```bash
.\target\release\iota.exe run --backend hermes --trace "告诉我当前项目的目标和技术实现"
```

执行结果

```
项目目标：
- 项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端
技术事实：
- iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":406,"truncated":false},"domain":[{"confidence":0.9,"content":"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系","id":"eb2a22d0-21e9-45b1-83a2-db0c384b718f","scope":"project","scope_id":"iota-sympantos"}],"episodic":[{"confidence":0.8,"content":"Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回","id":"c85def24-f984-4181-a328-9139de3a5307","scope":"project","scope_id":"iota-sympantos"}],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[{"confidence":0.85,"content":"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计","id":"605134c9-9173-485e-bbaf-0029300b7f34","scope":"project","scope_id":"iota-sympantos"}],"strategic":[{"confidence":0.9,"content":"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端","id":"1ba45094-cd1d-4b3a-aa45-7a456af50ecb","scope":"project","scope_id":"iota-sympantos"}]}

判定：符合预期。回复提及 Q2 目标、SQLite 存储层、Rust Engine、SHA-256 content_hash 去重；strategic/domain 均为 scope_id=`iota-sympantos`。
```

**预期回复：** 提及 Q2 目标、SQLite 存储层、SHA-256 去重。

- `strategic` 数组非空，content 包含 "Q2"
- `domain` 数组非空，content 包含 "SQLite" 和 "SHA-256"
- 两者 scope_id 均为 `iota-sympantos`

---

### Step 5 — procedural + episodic 召回验证（opencode）

```bash
.\target\release\iota.exe run --backend opencode --trace "回顾实验步骤，以及本次实验发生了什么"
```

执行结果

```
实验步骤：
- 实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计
历史经历：
- Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":406,"truncated":false},"domain":[{"confidence":0.9,"content":"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系","id":"eb2a22d0-21e9-45b1-83a2-db0c384b718f","scope":"project","scope_id":"iota-sympantos"}],"episodic":[{"confidence":0.8,"content":"Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回","id":"c85def24-f984-4181-a328-9139de3a5307","scope":"project","scope_id":"iota-sympantos"}],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"preference":[{"confidence":0.9,"content":"偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格","id":"595bbb72-08c9-4433-97e2-bde10db826d1","scope":"user","scope_id":"local-user"}],"procedural":[{"confidence":0.85,"content":"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计","id":"605134c9-9173-485e-bbaf-0029300b7f34","scope":"project","scope_id":"iota-sympantos"}],"strategic":[{"confidence":0.9,"content":"项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端","id":"1ba45094-cd1d-4b3a-aa45-7a456af50ecb","scope":"project","scope_id":"iota-sympantos"}]}

判定：符合预期。回复覆盖 6 步实验流程和 Step1 完成 6 类记忆写入的经历；[memory:inject] 的 procedural/episodic 数组均非空。
```

**预期回复：** 覆盖 6 步实验流程（procedural）和 Step1 完成 6 类写入的经历叙述（episodic）。

- `procedural` 数组非空
- `episodic` 数组非空，content 包含 "6 类记忆写入"

---

### Step 6 — contentHash 去重验证

> **设计说明：** 直接通过 MCP 工具写入与 Step 1-A 完全相同的 content 文本，
> 而非依赖 LLM 抽取（避免措辞差异导致 hash 不同）。

```bash
# 6.1 记录写入前的行数和 updated_at
sqlite3 "$env:USERPROFILE\.i6\context\memory.sqlite" \
  "SELECT id, content_hash, created_at, updated_at
   FROM memory
   WHERE scope_id='local-user' AND type='semantic' AND facet='identity';"
```

执行结果

```
b8517b2e-be70-425e-b57b-aecf67ec2630|5ee43f7ae37dd84d3e92df25e0ca3898588c0d6b1d89bf6620d9fc1fbe0097ca|1778057289|1778057289
```

```bash
# 6.2 重复写入完全相同的 content
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=identity, scope=user, scope_id=local-user,
   content=\"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证\",
   confidence=0.95"
```

执行结果

```
`iota_memory_write` 调用成功。

| 参数 | 值 |
|------|-----|
| id | `b8517b2e-be70-425e-b57b-aecf67ec2630` |
| merge_mode | auto |
| type | semantic |
| facet | identity |
| scope | user |
| scope_id | local-user |
| confidence | 0.95 |

记忆已写入用户桶 `local-user`。

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":406,"truncated":false},"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"...":"其余桶正常注入"}
```

```bash
# 6.3 验证去重效果
sqlite3 "$env:USERPROFILE\.i6\context\memory.sqlite" \
  "SELECT id, content_hash, created_at, updated_at
   FROM memory
   WHERE scope_id='local-user' AND type='semantic' AND facet='identity';"
```

执行结果

```
b8517b2e-be70-425e-b57b-aecf67ec2630|5ee43f7ae37dd84d3e92df25e0ca3898588c0d6b1d89bf6620d9fc1fbe0097ca|1778057289|1778057876

判定：符合预期。identity 仍只有 1 行，content_hash 相同，updated_at=1778057876 > created_at=1778057289。
```

**检查点 6.1：** 仍然只有 1 行（`content_hash` 相同），`updated_at` 已更新 > `created_at`。

---

### Step 7 — confidence 过滤验证

> **设计说明：** 手动插入低 confidence 记录，验证 recall 时被排除。

```bash
# 7.1 写入一条 confidence=0.50 的 identity 记忆（低于阈值 0.85）
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=semantic, facet=identity, scope=user, scope_id=local-user,
   content=\"低置信度测试：这条记忆不应被注入\",
   confidence=0.50"
```

执行结果

```
已写入低置信度测试记忆，ID: `bc50c522-7c11-41f4-8b32-5b59989a3551`，合并模式: auto。
```

```bash
# 7.2 写入一条 confidence=0.60 的 procedural 记忆（低于阈值 0.75）
.\target\release\iota.exe run --backend claude-code --trace \
  "请调用 iota_memory_write 工具，参数如下：
   type=procedural, scope=project, scope_id=iota-sympantos,
   content=\"低置信度测试：这条 procedural 不应被注入\",
   confidence=0.60"
```

执行结果

```
已写入，ID: `e8c08516-5d0c-441a-a364-1ef3b57fab03`，merge_mode=auto，confidence=0.60。
```

```bash
# 7.3 验证 DB 中确实存在这两条低 confidence 记录
sqlite3 "$env:USERPROFILE\.i6\context\memory.sqlite" \
  "SELECT type, facet, confidence, substr(content,1,30)
   FROM memory
   WHERE content LIKE '%低置信度测试%';"
```

执行结果

```
semantic|identity|0.5|低置信度测试：这条记忆不应被注入
procedural||0.6|低置信度测试：这条 procedural 不应被注入

补充修复：首次执行时 Engine 对显式 iota_memory_write prompt 额外自动写入 session episodic，导致该查询出现 session 噪声行。已修复为显式 memory tool prompt 不再额外生成自动 episodic，清理噪声后重跑验证。
```

```bash
# 7.4 触发一次召回，检查 trace 中这两条不出现在注入桶里
.\target\release\iota.exe run --backend codex --trace "你知道关于我的所有信息吗？"
```

执行结果

```
身份：
- 用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证
偏好：
- 偏好中文回答，实验日志用英文，报告格式为 Markdown，缩进用 2 空格
项目目标：
- 项目目标：2026 Q2 完成跨后端记忆延续完整验证，覆盖 6 类记忆桶和 5 个后端
技术事实：
- iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系
实验步骤：
- 实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计
历史经历：
- Step1 已通过 claude-code 完成全部 6 类记忆写入（semantic×4 identity/preference/strategic/domain + procedural + episodic），准备切换后端验证召回

[memory:inject] id=- payload={"budget":{"excluded_count":0,"memory_chars":2000,"total_chars":406,"truncated":false},"domain":[{"confidence":0.9,"content":"iota-sympantos 使用 SQLite 存储层，Rust Engine 实现，SHA-256 content_hash 去重，6 桶分类体系","id":"eb2a22d0-21e9-45b1-83a2-db0c384b718f","scope":"project","scope_id":"iota-sympantos"}],"identity":[{"confidence":0.95,"content":"用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证","id":"b8517b2e-be70-425e-b57b-aecf67ec2630","scope":"user","scope_id":"local-user"}],"procedural":[{"confidence":0.85,"content":"实验步骤：1)清理SQLite测试行 2)claude-code精确写入6类 3)逐后端切换召回 4)去重验证 5)budget截断 6)observability审计","id":"605134c9-9173-485e-bbaf-0029300b7f34","scope":"project","scope_id":"iota-sympantos"}],"...":"其余高置信桶正常注入"}

判定：符合预期。identity 数组不包含 "低置信度测试"；procedural 数组不包含 "低置信度测试"；高 confidence 的原始记录仍正常出现。
```

- `identity` 数组中不包含 "低置信度测试" 内容（confidence 0.50 < 阈值 0.85）
- `procedural` 数组中不包含 "低置信度测试" 内容（confidence 0.60 < 阈值 0.75）
- 高 confidence 的原始记录仍正常出现

---

### Step 8 — token budget 截断验证

```bash
# 8.1 批量写入大量 domain 记忆，使总字符数超过 memory_chars=2000
foreach ($i in 1..15) {
  .\target\release\iota.exe run --backend claude-code `
    "请调用 iota_memory_write 工具，参数如下：
     type=semantic, facet=domain, scope=project, scope_id=iota-sympantos,
     content=\"domain-padding-$i: 这是第 $i 条填充记忆，用于测试 token budget 截断行为。iota-sympantos 使用 Rust 编写的 Engine 层驱动 ACP JSON-RPC 2.0 协议，支持 5 个后端的热切换。\",
     confidence=0.90"
}
```

执行结果

```
15 条 domain-padding 记忆写入成功。返回 ID：
2c0c2da9-af1d-418b-926a-ac2e874d9f79
457c7b1c-249f-4cb2-b629-fa8460104788
80976e24-6fb1-4806-838f-98fb21b9add6
c5a7f597-9c88-4460-97e8-5b34830c0129
7c082876-0aa7-4aa7-b876-11c7a7024d5d
16c20527-4dfd-48bb-825a-42f4522515b9
68d3dff7-9adb-422d-bc64-3ba41e5eb52a
a9a1a0c0-ee2d-46c0-bdac-c6f59ef6cdfd
ab3b49f4-a348-4609-8c1b-26ed725ada2c
f34d9957-d583-4ab2-b1d8-234895bc443b
713921cc-c44a-46e9-9f86-cf1787ca7343
b1a9b31f-c722-457f-b09a-b16243d8fb71
320d88b6-efa9-4ced-a039-294a6fff8df1
d9ed228f-e98d-4dc2-8ca2-0f2d28f711ae
519e29b3-5226-4e20-a29a-c0635c07444a

DB 验证：domain-padding 记录 scope 分布为 iota-sympantos|15。
```

```bash
# 8.2 查看当前总字符数
sqlite3 "$env:USERPROFILE\.i6\context\memory.sqlite" \
  "SELECT sum(length(content)) AS total_chars FROM memory
   WHERE scope_id IN ('local-user','iota-sympantos')
   AND confidence >= 0.70;"
```

执行结果

```
2278
```

```bash
# 8.3 触发一次完整召回，观察截断
.\target\release\iota.exe run --backend codex --trace \
  "列出你知道的关于我和本项目的所有信息"
```

执行结果

```json
{
  "memory_chars": 2000,
  "total_chars": 2278,
  "truncated": true,
  "excluded_count": 3
}
```

判定：符合预期。trace 中 `[memory:inject]` 的 budget 显示 `total_chars=2278 > memory_chars=2000`，`truncated=true`，`excluded_count=3`。

---

### Step 9 — Observability 审计

> **设计说明：** 验证所有前序步骤产生的 logging / tracing / metrics 数据完整可查。

#### 9.1 Logging 验证

```bash
# 查看最近的执行记录（应覆盖 Step 1~8 的所有 run）
.\target\release\iota.exe observability logging recent --limit 30
```

执行结果

```
返回 30 条最近执行记录，覆盖 Step 2~8 和 Step 1 后半段；示例：
- 9b3c05fe-d637-4d90-b99f-eeeb5a18cae7 | codex | completed | Step 8.3 budget 召回
- 83846fb1-69a9-4c04-8f2f-99773b8cb102 | claude-code | completed | domain-padding-15
- f47ee614-3ed7-4281-afc2-28917e53dd5e | opencode | completed | Step 5
- 056f7116-3834-41ee-b941-ea79c36a2409 | hermes | completed | Step 4
- ad260d45-a426-4963-92bf-ff8c206f6ae7 | gemini | completed | Step 3
- 9dcd6c65-fbe0-480b-89d9-a3e9213c0414 | codex | completed | Step 2

补充：Step 1-A execution_id 通过 EventStore 查询定位为 7c1986b2-3a0d-4cc8-b413-c4fa6a36e205。
```

```bash
# 查看某个 execution 的完整事件流
# 替换 <exec-id> 为 Step 1-A 的 execution_id
.\target\release\iota.exe observability logging events 7c1986b2-3a0d-4cc8-b413-c4fa6a36e205
```

执行结果

```
关键事件：
- seq=1 state started
- seq=2 memory inject, budget.total_chars=0, truncated=false
- seq=10 tool_call
- seq=11 tool_call_update rawInput:
  type=semantic, facet=identity, scope=user, scope_id=local-user,
  content="用户名 Sympantos，角色：iota-sympantos 实验员，职责：跨后端记忆延续验证",
  confidence=0.95
- seq=13 toolResponse: {"id":"b8517b2e-be70-425e-b57b-aecf67ec2630","merge_mode":"auto"}
- seq=22~24 assistant output: 已完成记忆写入，ID: `b8517b2e-be70-425e-b57b-aecf67ec2630`

判定：符合预期。完整事件流包含 Memory inject、MCP tool call/update、tool response 和 assistant output。
```

#### 9.2 Tracing 验证

```bash
# 查看延迟统计
.\target\release\iota.exe observability tracing summary
```

执行结果

```json
{
  "total_executions": 39,
  "completed_executions": 39,
  "failed_executions": 0,
  "running_executions": 0,
  "avg_prompt_ms": 11951.09090909091,
  "avg_total_ms": 12774.30303030303,
  "p95_total_ms": 26740
}
```

```bash
# 查看最慢的执行
.\target\release\iota.exe observability tracing slow --limit 5
```

执行结果

```
最慢 5 条均成功返回。前 3 条：
- 18c53e9a-1091-458c-9540-a6e8839c8146 | claude-code | total_ms=27652
- b310a780-8ffd-4ec0-864c-73a2c9d52412 | claude-code | total_ms=26740
- 9f1d1ff9-989c-4301-9f53-b7da61908ec5 | claude-code | total_ms=24780
```

```bash
# 查看某个 execution 的 5 阶段延迟分解
.\target\release\iota.exe observability tracing breakdown 7c1986b2-3a0d-4cc8-b413-c4fa6a36e205
```

执行结果

```json
{
  "execution_id": "7c1986b2-3a0d-4cc8-b413-c4fa6a36e205",
  "backend": "claude-code",
  "status": "completed",
  "process_spawn_ms": 13,
  "init_ms": 1389,
  "session_new_ms": 892,
  "prompt_ms": 7006,
  "total_ms": 7899,
  "phases": [
    {"phase": "process_spawn", "ms": 13},
    {"phase": "init", "ms": 1389},
    {"phase": "session_new", "ms": 892},
    {"phase": "prompt", "ms": 7006},
    {"phase": "total", "ms": 7899}
  ]
}
```

#### 9.3 Metrics 验证

```bash
# 聚合指标（人类可读）
.\target\release\iota.exe observability metrics
```

执行结果

```json
{
  "executions": {"completed": 39, "failed": 0, "running": 0, "total": 39},
  "latency": {"avg_prompt_ms": 11951.09090909091, "avg_total_ms": 12774.30303030303, "p95_total_ms": 26740},
  "tokens": {"events": 0, "input_tokens": 0, "output_tokens": 0, "total_tokens": 0},
  "cache": {"hit_rate": 0.0, "hits": 0, "misses": 39},
  "runtime": {"active_sessions": 0, "queued_prompts": 0}
}
```

```bash
# Prometheus 格式输出
.\target\release\iota.exe observability metrics --prometheus
```

执行结果

```
iota_execution_attempts_total 39
iota_execution_completed_total 39
iota_execution_failed_total 0
iota_cache_misses_total 39
iota_prompt_latency_ms_count 33
iota_prompt_latency_ms_sum 394386
iota_total_latency_ms_avg 12774.30303030303
iota_total_latency_ms_p95 26740
```

```bash
# Token 用量
.\target\release\iota.exe observability metrics tokens
```

执行结果

```json
{
  "avg_input_per_execution": null,
  "avg_output_per_execution": null,
  "input_tokens": 0,
  "output_tokens": 0,
  "token_usage_events": 0,
  "total_tokens": 0
}
```

```bash
# 延迟指标
.\target\release\iota.exe observability metrics latency
```

执行结果

```json
{
  "avg_prompt_ms": 11951.09090909091,
  "avg_total_ms": 12774.30303030303,
  "p95_total_ms": 26740
}
```

---

## 四、验收矩阵

> **执行日期：** 2026-05-06 | **实际使用后端：** claude-code (写入) + codex/gemini/hermes/opencode (召回)

| # | 验收项 | 步骤 | 判定标准 | 通过 |
|---|--------|------|----------|------|
| 1 | identity 跨后端延续 | Step 2 | codex 回复含 "Sympantos" | ☑ |
| 2 | preference 跨后端延续 | Step 3 | trace preference 桶含 "中文/Markdown" | ☑ |
| 3 | strategic+domain 跨后端延续 | Step 4 | hermes 提及 Q2 目标和 SQLite/SHA-256 | ☑ |
| 4 | procedural+episodic 延续 | Step 5 | opencode 覆盖步骤和 Step1 经历 | ☑ |
| 5 | contentHash 去重 | Step 6 | identity 仍只有 1 行，updated_at 变更 | ☑ |
| 6 | confidence 过滤（identity 0.85） | Step 7 | conf=0.50 记录不在 inject identity 桶中 | ☑ |
| 7 | confidence 过滤（procedural 0.75） | Step 7 | conf=0.60 记录不在 inject procedural 桶中 | ☑ |
| 8 | token budget 截断 | Step 8 | budget.truncated=true, excluded_count=3 | ☑ |
| 9 | SQLite schema 合规 | 检查点 1.1 | type/facet/scope/scope_id 字段正确 | ☑ |
| 10 | trace 事件完整性 | 检查点 1.2 | --trace 输出含 [memory:inject] | ☑ |
| 11 | EventStore 事件持久化 | 检查点 1.3 | observability events 含 Memory 事件 | ☑ |
| 12 | Logging 多后端覆盖 | Step 9.1 | recent 输出含 claude-code + hermes | ☑ |
| 13 | Tracing 延迟分解 | Step 9.2 | breakdown 含 5 阶段延迟 | ☑ |
| 14 | Metrics 指标可查 | Step 9.3 | executions.total=39 > 0 | ☑ |
| 15 | Prometheus 导出 | Step 9.3 | --prometheus 含 iota_execution_attempts_total | ☑ |

---

## 五、清理

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
