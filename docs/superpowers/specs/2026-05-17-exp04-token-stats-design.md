---
name: exp04-token-stats-design
description: 实验4设计文档：5个backend的token统计实验，验证多次执行相同prompt时token消耗的稳定性和backend间的差异
metadata:
  type: spec
  experiment: exp04-token-stats
  date: 2026-05-17
---

# iota-sympantos 实验4：Backend Token 统计实验设计

| 字段 | 值 |
|------|-----|
| 实验代号 | exp04-token-stats |
| 设计日期 | 2026-05-17 |
| 实验对象 | 5个backend的token消耗统计与稳定性分析 |
| 参考实验 | exp01-memory, exp03-acp-runtime |

---

## 一、实验目标

验证 5 个 backend 在多次执行相同 prompt 时的 token 消耗稳定性，并对比不同 backend 的 token 消耗差异。

**核心问题：**
1. 同一 backend 多次执行时，token 消耗是否稳定？
2. 哪个 backend 消耗的 token 最多/最少？
3. 缓存机制（cache_read_input_tokens, cache_creation_input_tokens）在各 backend 的表现如何？

---

## 二、实验环境

| 项目 | 值 |
|------|-----|
| OS | macOS (Darwin 25.4.0) |
| 工作目录 | `/Users/han/coding/creative/iota-sympantos` |
| Binary | `target/release/iota` |
| 配置来源 | `~/.i6/nimia.yaml` |
| 数据采集 | observability metrics + EventStore |
| 备用数据源 | backend 原生输出（`--show-native`） |

**测试 backend：**
- claude-code
- codex
- gemini
- hermes
- opencode

---

## 三、测量指标

| 指标 | 字段名 | 说明 |
|------|--------|------|
| 输入 tokens | `input_tokens` | 每次请求发送给模型的 token 数量 |
| 缓存读取 tokens | `cache_read_input_tokens` | 从缓存中读取的 token 数量 |
| 缓存写入 tokens | `cache_creation_input_tokens` | 写入缓存的 token 数量 |
| 输出 tokens | `output_tokens` | 模型生成的 token 数量 |

**派生指标：**
- 总输入 tokens = `input_tokens + cache_creation_input_tokens`
- 缓存命中率 = `cache_read_input_tokens / (input_tokens + cache_read_input_tokens)` （如适用）
- 总 tokens = `input_tokens + cache_creation_input_tokens + cache_read_input_tokens + output_tokens`

---

## 四、测试 prompt

使用一个简单但足够触发完整 ACP 流程的 prompt：

```
请用一句话介绍 Rust 语言的主要特点。
```

**选择理由：**
- 足够简单，输出稳定
- 不涉及工具调用，避免额外的 token 消耗变量
- 中文 prompt，与 exp01/exp03 保持一致

---

## 五、测量方法

### 5.1 环境准备

```bash
# 构建 release 版本
cargo build --release

# 验证 backend 配置
./target/release/iota check

# 清理旧的测试数据（可选）
# 如果需要干净的 EventStore 环境，可以备份并清空
```

### 5.2 数据采集流程

**每个 backend 执行 3 轮测试：**

```bash
# 定义固定 prompt
PROMPT="请用一句话介绍 Rust 语言的主要特点。"

# 对每个 backend 执行 3 轮
for backend in claude-code codex gemini hermes opencode; do
  echo "=== Testing backend: $backend ==="
  
  for run in 1 2 3; do
    echo "--- Run $run ---"
    ./target/release/iota run --backend $backend "$PROMPT"
    
    # 记录 execution_id 用于后续数据提取
    # execution_id 会在 observability 命令中使用
    
    sleep 2  # 避免请求过快
  done
  
  echo ""
done
```

### 5.3 Token 数据提取

**方法 1：通过 observability metrics（优先）**

```bash
# 查看最近的执行记录
./target/release/iota observability logging recent --limit 20

# 查看 metrics 汇总
./target/release/iota observability metrics

# 如果 metrics 包含 token usage 统计，直接使用
./target/release/iota observability metrics --prometheus | grep token
```

**方法 2：通过 EventStore 查询特定 execution**

```bash
# 获取特定 execution 的详细事件
./target/release/iota observability logging events <execution-id>

# 从事件流中提取 token_usage 事件
# 查找 event_type 为 token_usage 或包含 token 相关字段的事件
```

**方法 3：直接查询 EventStore SQLite（备用）**

```bash
# 如果 observability 命令不够用，直接查询数据库
sqlite3 ~/.i6/context/events.sqlite "
  SELECT execution_id, backend, 
         json_extract(payload, '$.input_tokens') as input_tokens,
         json_extract(payload, '$.cache_read_input_tokens') as cache_read,
         json_extract(payload, '$.cache_creation_input_tokens') as cache_write,
         json_extract(payload, '$.output_tokens') as output_tokens
  FROM events
  WHERE event_type = 'token_usage'
    AND created_at > datetime('now', '-1 hour')
  ORDER BY created_at DESC
  LIMIT 15;
"
```

**方法 4：从 backend 原生输出解析（fallback）**

如果 observability 系统没有采集到 token 数据：

```bash
# 重新运行并捕获完整输出
./target/release/iota run --backend claude-code --show-native "$PROMPT" 2>&1 | tee output.log

# 从 output.log 中手动提取 token 统计
# 不同 backend 的格式可能不同，需要针对性解析
```

---

## 六、数据记录格式

采集到的数据记录在 `gefsi/exp04-token-stats.md` 中，使用以下表格格式：

### 6.1 原始数据表

| Backend | Run | execution_id | input_tokens | cache_read | cache_write | output_tokens | total_tokens | 备注 |
|---------|-----|--------------|--------------|------------|-------------|---------------|--------------|------|
| claude-code | 1 | `abc123...` | 150 | 0 | 50 | 25 | 225 | |
| claude-code | 2 | `def456...` | 150 | 50 | 0 | 25 | 225 | 缓存命中 |
| claude-code | 3 | `ghi789...` | 150 | 50 | 0 | 26 | 226 | |
| ... | ... | ... | ... | ... | ... | ... | ... | |

### 6.2 统计汇总表

| Backend | input_tokens (avg±std) | cache_read (avg±std) | cache_write (avg±std) | output_tokens (avg±std) | total_tokens (avg±std) |
|---------|------------------------|----------------------|----------------------|-------------------------|------------------------|
| claude-code | 150.0±0.0 | 33.3±23.6 | 16.7±23.6 | 25.3±0.5 | 225.3±0.5 |
| codex | ... | ... | ... | ... | ... |
| gemini | ... | ... | ... | ... | ... |
| hermes | ... | ... | ... | ... | ... |
| opencode | ... | ... | ... | ... | ... |

---

## 七、分析方法

### 7.1 稳定性分析

**目标：** 评估同一 backend 多次运行时 token 消耗的波动程度。

**指标：**
- 标准差（std）：越小表示越稳定
- 变异系数（CV）：`std / mean`，归一化的波动指标
- 极差：`max - min`

**判定标准：**
- `CV < 5%`：非常稳定
- `5% ≤ CV < 10%`：较稳定
- `CV ≥ 10%`：波动较大

### 7.2 Backend 对比分析

**目标：** 识别哪个 backend 消耗 token 最多/最少。

**对比维度：**
1. **总 token 消耗排序**：从高到低排列
2. **输入 token 对比**：不同 backend 对相同 prompt 的 tokenization 差异
3. **输出 token 对比**：生成内容的长度差异
4. **缓存效率对比**：`cache_read_input_tokens` 占比

### 7.3 缓存行为分析

**目标：** 观察各 backend 的缓存机制表现。

**观察点：**
- Run 1（冷启动）：是否有 `cache_creation_input_tokens`
- Run 2/3（热路径）：是否有 `cache_read_input_tokens`
- 缓存命中率：`cache_read / (input + cache_read)`

**预期行为：**
- 支持 prompt caching 的 backend（如 Claude）应在 Run 2/3 显示缓存命中
- 不支持缓存的 backend 所有 run 的 `cache_*` 字段应为 0 或 null

---

## 八、验收标准

| # | 验收项 | 判定标准 |
|---|--------|----------|
| 1 | 数据完整性 | 5 个 backend × 3 轮 = 15 条记录全部采集成功 |
| 2 | Token 字段齐全 | 每条记录至少包含 `input_tokens` 和 `output_tokens` |
| 3 | 稳定性可量化 | 每个 backend 计算出 mean、std、CV |
| 4 | Backend 排序清晰 | 按总 token 消耗从高到低排序 |
| 5 | 缓存行为可观测 | 支持缓存的 backend 显示 cache_read/cache_write 数据 |
| 6 | 数据可复验 | 记录 execution_id，可通过 observability 命令回溯 |
| 7 | 异常情况记录 | 如果某个 backend 失败或数据缺失，在备注中说明 |

---

## 九、预期结果

基于 exp01/exp03 的经验，预期：

1. **Claude Code** 可能有较完整的 token 统计和缓存数据
2. **Gemini** 可能有完整的 token 统计
3. **其他 backend** 的 token 数据完整性取决于其 ACP adapter 实现

**如果遇到数据缺失：**
- 优先尝试从 `--show-native` 输出解析
- 如果仍然缺失，在备注中标注 "N/A - backend 未上报 token usage"
- 不强求所有 backend 都有完整的 4 个 token 指标，但至少要有 input + output

---

## 十、复验命令

```bash
# 环境检查
cargo build --release
./target/release/iota check

# 执行测试（完整脚本）
PROMPT="请用一句话介绍 Rust 语言的主要特点。"
for backend in claude-code codex gemini hermes opencode; do
  echo "=== $backend ==="
  for run in 1 2 3; do
    echo "Run $run"
    ./target/release/iota run --backend $backend "$PROMPT"
    sleep 2
  done
done

# 数据提取
./target/release/iota observability logging recent --limit 20
./target/release/iota observability metrics

# 如果需要查询特定 execution
./target/release/iota observability logging events <execution-id>

# 直接查询 EventStore（备用）
sqlite3 ~/.i6/context/events.sqlite "
  SELECT execution_id, backend, payload
  FROM events
  WHERE event_type = 'token_usage'
    AND created_at > datetime('now', '-1 hour')
  ORDER BY created_at DESC;
"
```

---

## 十一、实现计划

实现步骤将在独立的 implementation plan 中详细说明，主要包括：

1. 环境准备与验证
2. 执行 15 轮测试（5 backend × 3 runs）
3. 从 observability 系统提取 token 数据
4. 数据清洗与统计分析
5. 生成实验报告文档
6. 提交到 git

---

*设计完成时间：2026-05-17*
