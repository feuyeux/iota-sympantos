# exp04 Token 统计实验实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对5个backend执行3轮测试，采集input_tokens、cache_read_input_tokens、cache_creation_input_tokens、output_tokens，分析token消耗稳定性和backend间差异。

**Architecture:** 使用 iota observability 系统采集 token 数据，通过 EventStore 和 metrics 命令提取统计数据，结果记录到 gefsi/exp04-token-stats.md。

**Tech Stack:** iota CLI, observability system, EventStore SQLite, bash scripts

---

## 概述

本实验执行 5 个 backend × 3 轮 = 15 次测试，每次使用固定 prompt：
```
请用一句话介绍 Rust 语言的主要特点。
```

数据采集优先级：
1. `iota observability metrics` 中的 token usage 统计
2. `iota observability logging events <execution_id>` 中的 token_usage 事件
3. 直接查询 EventStore SQLite
4. `--show-native` 输出解析（fallback）

---

## 环境准备

### Task 1: 环境检查与构建

**Files:**
- Modify: `gefsi/exp04-token-stats.md` (实验结果文档)

- [ ] **Step 1: 构建 release 版本**

```bash
cargo build --release
```

预期：构建成功，无错误

- [ ] **Step 2: 验证 backend 配置**

```bash
./target/release/iota check
```

预期：5 个 backend (claude-code, codex, gemini, hermes, opencode) 均 configured

- [ ] **Step 3: 查看当前 metrics 状态（基准）**

```bash
./target/release/iota observability metrics
```

预期：显示当前 metrics 统计（用于后续对比）

- [ ] **Step 4: 创建实验结果文档框架**

创建 `gefsi/exp04-token-stats.md`，包含：
- 实验信息表（实验代号、日期、环境）
- 原始数据表（15行 × backend/run/execution_id/token指标）
- 统计汇总表

- [ ] **Step 5: Commit 环境准备**

```bash
git add gefsi/exp04-token-stats.md
git commit -m "feat(exp04): initial token stats experiment document"
```

---

## 执行测试

### Task 2: 执行 15 轮测试

**Files:**
- Modify: `gefsi/exp04-token-stats.md` (更新 execution_id 和原始数据)

- [ ] **Step 1: 定义测试脚本**

```bash
PROMPT="请用一句话介绍 Rust 语言的主要特点。"
BACKENDS="claude-code codex gemini hermes opencode"

for backend in $BACKENDS; do
  echo "=== Testing backend: $backend ==="
  for run in 1 2 3; do
    echo "--- Run $run ---"
    ./target/release/iota run --backend $backend "$PROMPT"
    sleep 3
  done
  echo ""
done
```

预期：每个 backend 执行 3 次，共 15 次执行

- [ ] **Step 2: 记录所有 execution_id**

每次执行后从输出中提取 execution_id，记录到实验文档的原始数据表中

- [ ] **Step 3: Commit 中间结果**

```bash
git add gefsi/exp04-token-stats.md
git commit -m "feat(exp04): execute 15 test runs and record execution IDs"
```

---

## 数据提取

### Task 3: 从 observability 系统提取 token 数据

**Files:**
- Modify: `gefsi/exp04-token-stats.md`

- [ ] **Step 1: 提取 metrics 中的 token 统计**

```bash
./target/release/iota observability metrics
```

从输出中查找 token_usage 相关指标

- [ ] **Step 2: 尝试 prometheus 格式输出**

```bash
./target/release/iota observability metrics --prometheus | grep -i token
```

- [ ] **Step 3: 查询特定 execution 的详细事件**

对每个 execution_id 执行：
```bash
./target/release/iota observability logging events <execution_id>
```

从事件流中查找 `event_type: token_usage` 或包含 token 字段的事件

- [ ] **Step 4: 填充原始数据表**

将每个 backend/run 的 token 数据填入表格：
- input_tokens
- cache_read_input_tokens
- cache_creation_input_tokens
- output_tokens

- [ ] **Step 5: Commit 数据提取结果**

```bash
git add gefsi/exp04-token-stats.md
git commit -m "feat(exp04): extract token data from observability system"
```

---

## 数据补充（备用）

### Task 4: 使用 EventStore SQLite 直接查询（备用方案）

**Files:**
- Modify: `gefsi/exp04-token-stats.md`

- [ ] **Step 1: 直接查询 EventStore**

```bash
sqlite3 ~/.i6/context/events.sqlite "
  SELECT execution_id, backend, 
         json_extract(payload, '\$.input_tokens') as input_tokens,
         json_extract(payload, '\$.cache_read_input_tokens') as cache_read,
         json_extract(payload, '\$.cache_creation_input_tokens') as cache_write,
         json_extract(payload, '\$.output_tokens') as output_tokens
  FROM events
  WHERE event_type = 'token_usage'
    AND created_at > datetime('now', '-24 hours')
  ORDER BY created_at DESC
  LIMIT 20;
"
```

- [ ] **Step 2: 如果 observability 数据不完整，使用 fallback**

对缺失数据的 backend 执行：
```bash
./target/release/iota run --backend <backend> --show-native "$PROMPT" 2>&1 | tee gefsi/logs/exp04-<backend>-<run>.log
```

- [ ] **Step 3: 从原生输出解析 token 数据**

根据不同 backend 的输出格式，提取 token 统计信息

- [ ] **Step 4: 更新原始数据表**

- [ ] **Step 5: Commit 补充数据**

```bash
git add gefsi/exp04-token-stats.md gefsi/logs/
git commit -m "feat(exp04): supplement token data from EventStore fallback"
```

---

## 数据分析

### Task 5: 统计分析与结论

**Files:**
- Modify: `gefsi/exp04-token-stats.md`

- [ ] **Step 1: 计算统计汇总表**

对每个 backend 计算：
- mean, std, CV (变异系数)
- 稳定性判定 (CV < 5%: 非常稳定, 5-10%: 较稳定, >10%: 波动大)

```python
# 示例计算
import statistics

data = {
    'claude-code': {'input_tokens': [x, y, z], 'output_tokens': [a, b, c], ...},
    ...
}

for backend, metrics in data.items():
    for metric_name, values in metrics.items():
        mean = statistics.mean(values)
        std = statistics.stdev(values) if len(values) > 1 else 0
        cv = (std / mean * 100) if mean > 0 else 0
```

- [ ] **Step 2: Backend 对比分析**

- 按总 token 消耗排序
- 分析 input_tokens 差异（tokenization 差异）
- 分析 output_tokens 差异（生成内容长度）
- 分析缓存效率

- [ ] **Step 3: 缓存行为分析**

- Run 1 vs Run 2/3 的 cache_read_input_tokens 对比
- 计算缓存命中率

- [ ] **Step 4: 填写结论**

根据分析结果填写：
- 哪个 backend 最稳定
- 哪个 backend 总消耗最高/最低
- 缓存机制表现

- [ ] **Step 5: Commit 分析结果**

```bash
git add gefsi/exp04-token-stats.md
git commit -m "feat(exp04): complete statistical analysis and conclusions"
```

---

## 最终验收

### Task 6: 验收标准检查

- [ ] **检查 1: 数据完整性**

5 个 backend × 3 轮 = 15 条记录全部采集成功

- [ ] **检查 2: Token 字段齐全**

每条记录至少包含 input_tokens 和 output_tokens

- [ ] **检查 3: 稳定性可量化**

每个 backend 有 mean、std、CV

- [ ] **检查 4: Backend 排序清晰**

按总 token 消耗从高到低排序

- [ ] **检查 5: 异常情况记录**

失败的 backend 或缺失的数据已在备注中说明

- [ ] **最终 Commit**

```bash
git add gefsi/exp04-token-stats.md
git commit -m "feat(exp04): complete token statistics experiment"
```

---

## 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `gefsi/exp04-token-stats.md` | 创建/修改 | 实验结果文档 |
| `gefsi/logs/exp04-*.log` | 创建 | Fallback 模式下的原始输出日志 |

---

## 验收矩阵

| # | 验收项 | 判定标准 |
|---|--------|----------|
| 1 | 数据完整性 | 15 条记录全部采集 |
| 2 | Token 字段齐全 | 至少 input_tokens + output_tokens |
| 3 | 稳定性可量化 | mean, std, CV |
| 4 | Backend 排序 | 总消耗排序 |
| 5 | 异常记录 | 失败/缺失有备注 |
| 6 | 数据可复验 | execution_id 记录 |

---

*计划创建时间：2026-05-17*
