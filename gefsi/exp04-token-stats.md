# iota-sympantos 实验4：Backend Token 统计

| 字段 | 值 |
|------|-----|
| 实验代号 | exp04-token-stats |
| 执行日期 | 2026-05-17 |
| 实验对象 | 5个backend的token消耗统计与稳定性分析 |
| 测试 prompt | 请用一句话介绍 Rust 语言的主要特点。 |
| 数据采集方式 | iota observability system + EventStore SQLite |

---

## 一、原始数据表

| Backend | Run | execution_id | input_tokens | cache_read_input_tokens | cache_creation_input_tokens | output_tokens | total_tokens | 备注 |
|---------|-----|--------------|--------------|------------------------|----------------------------|--------------|--------------|------|
| claude-code | 1 | | | | | | | |
| claude-code | 2 | | | | | | | |
| claude-code | 3 | | | | | | | |
| codex | 1 | | | | | | | |
| codex | 2 | | | | | | | |
| codex | 3 | | | | | | | |
| gemini | 1 | | | | | | | |
| gemini | 2 | | | | | | | |
| gemini | 3 | | | | | | | |
| hermes | 1 | | | | | | | |
| hermes | 2 | | | | | | | |
| hermes | 3 | | | | | | | |
| opencode | 1 | | | | | | | |
| opencode | 2 | | | | | | | |
| opencode | 3 | | | | | | | |

---

## 二、统计汇总表

| Backend | input_tokens (avg±std) | cache_read (avg±std) | cache_write (avg±std) | output_tokens (avg±std) | total_tokens (avg±std) | CV total | 稳定性 |
|---------|------------------------|----------------------|----------------------|-------------------------|------------------------|---------|--------|
| claude-code | | | | | | | |
| codex | | | | | | | |
| gemini | | | | | | | |
| hermes | | | | | | | |
| opencode | | | | | | | |

---

## 三、分析与结论

（待填写）

### 3.1 稳定性分析

### 3.2 Backend 对比分析

### 3.3 缓存行为分析

### 3.4 结论

---

## 四、验收矩阵

| # | 验收项 | 状态 |
|---|--------|------|
| 1 | 数据完整性（15条记录） | 待验证 |
| 2 | Token字段齐全 | 待验证 |
| 3 | 稳定性可量化 | 待验证 |
| 4 | Backend排序 | 待验证 |
| 5 | 异常情况记录 | 待验证 |
| 6 | 数据可复验 | 待验证 |