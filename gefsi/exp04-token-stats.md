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
| claude-code | 1 | ccd5ec56-3f1c-4044-a707-e96be9d1c895 | | | | | | 3条claude-code连续相同时间戳 |
| claude-code | 2 | 3bffe027-49df-4243-809d-bd4b5fbfd87d | | | | | | |
| claude-code | 3 | 00805a18-7c47-446d-8f25-c5b1d79388c1 | | | | | | |
| codex | 1 | 019fc1b8-0380-4e8e-9b41-fcb50ea87c51 | | | | | | |
| codex | 2 | 23b348b1-24ba-47ac-a84d-8accf31293a5 | | | | | | |
| codex | 3 | 8be826c8-34b7-4888-901d-da2c986b8bd3 | | | | | | |
| gemini | 1 | 103322f8-ab52-4488-9fb6-ff3264faa152 | | | | | | |
| gemini | 2 | e60a515b-ed74-48a3-914e-a5a8d0ba8cc5 | | | | | | |
| gemini | 3 | 0a60a1cf-b01e-4b53-9c30-131032567472 | | | | | | |
| hermes | 1 | 71734a9a-43fd-4571-a696-bc06c2d4d3c6 | | | | | | |
| hermes | 2 | f5b15398-da6f-4376-9cc9-06ca8fbac56e | | | | | | |
| hermes | 3 | 87eb4475-741a-45ed-9621-e271e059fff2 | | | | | | |
| opencode | 1 | f6044810-3ac0-45d6-b156-6dca23166922 | | | | | | |
| opencode | 2 | d119104e-b4af-454f-8ea1-34a4a85db6dd | | | | | | |
| opencode | 3 | 8aa0fb3f-7343-463e-9940-ef58d920e836 | | | | | | |

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