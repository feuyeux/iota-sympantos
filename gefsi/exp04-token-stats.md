# iota-sympantos 实验4：Backend Token 统计

| 字段 | 值 |
|------|-----|
| 实验代号 | exp04-token-stats |
| 执行日期 | 2026-05-17 |
| 实验对象 | 5个backend的token消耗统计与稳定性分析 |
| 测试 prompt | 请用一句话介绍 Rust 语言的主要特点。 |
| 数据采集方式 | iota run 输出 + EventStore SQLite 查询 |
| Binary | target/release/iota |
| 配置来源 | ~/.i6/nimia.yaml |

---

## 二、数据采集详情

### 2.1 数据获取渠道

| 数据项 | 获取渠道 | 命令/方法 | 写入位置 |
|--------|----------|-----------|----------|
| execution_id | iota run 输出 | `./target/release/iota run --backend <backend> "<prompt>"` | 本文档"原始数据表" |
| session_id | iota run 输出 | 同上 | 待补充 |
| input_tokens | EventStore SQLite | `sqlite3 ~/.i6/context/events.sqlite "SELECT ... WHERE event_type='token_usage'..."` | 本文档"原始数据表" |
| cache_read_input_tokens | EventStore SQLite | 同上 | 本文档"原始数据表" |
| cache_creation_input_tokens | EventStore SQLite | 同上 | 本文档"原始数据表" |
| output_tokens | EventStore SQLite | 同上 | 本文档"原始数据表" |
| timing metrics (total_ms, etc.) | iota run 输出 (--trace-timing) | `./target/release/iota run --backend <backend> --trace-timing "<prompt>"` | 待补充 |

### 2.2 EventStore 查询方法

**直接查询 EventStore SQLite：**
```bash
sqlite3 ~/.i6/context/events.sqlite "
  SELECT execution_id, backend,
         json_extract(event_json, '\$.payload.input_tokens') as input_tokens,
         json_extract(event_json, '\$.payload.cache_read_input_tokens') as cache_read,
         json_extract(event_json, '\$.payload.cache_creation_input_tokens') as cache_write,
         json_extract(event_json, '\$.payload.output_tokens') as output_tokens
  FROM events
  WHERE event_type = 'token_usage'
    AND execution_id IN ('<exec_id_1>', '<exec_id_2>', ...)
  ORDER BY execution_id, created_at;
"
```

**按 execution_id 查询完整事件流：**
```bash
./target/release/iota logs <execution_id>
# 或
sqlite3 ~/.i6/context/events.sqlite "
  SELECT event_type, event_json
  FROM events
  WHERE execution_id = '<execution_id>'
  ORDER BY created_at;
"
```

### 2.3 已执行的测试运行

| Backend | Run | execution_id | 获取渠道 | 执行时间 | 备注 |
|---------|-----|--------------|----------|----------|------|
| claude-code | 1 | ccd5ec56-3f1c-4044-a707-e96be9d1c895 | iota run 输出 | 2026-05-17 | 3条claude-code连续相同时间戳 |
| claude-code | 2 | 3bffe027-49df-4243-809d-bd4b5fbfd87d | iota run 输出 | 2026-05-17 | |
| claude-code | 3 | 00805a18-7c47-446d-8f25-c5b1d79388c1 | iota run 输出 | 2026-05-17 | |
| codex | 1 | 019fc1b8-0380-4e8e-9b41-fcb50ea87c51 | iota run 输出 | 2026-05-17 | |
| codex | 2 | 23b348b1-24ba-47ac-a84d-8accf31293a5 | iota run 输出 | 2026-05-17 | |
| codex | 3 | 8be826c8-34b7-4888-901d-da2c986b8bd3 | iota run 输出 | 2026-05-17 | |
| gemini | 1 | 103322f8-ab52-4488-9fb6-ff3264faa152 | iota run 输出 | 2026-05-17 | 输出最稳定，3次完全相同 |
| gemini | 2 | e60a515b-ed74-48a3-914e-a5a8d0ba8cc5 | iota run 输出 | 2026-05-17 | |
| gemini | 3 | 0a60a1cf-b01e-4b53-9c30-131032567472 | iota run 输出 | 2026-05-17 | |
| hermes | 1 | 71734a9a-43fd-4571-a696-bc06c2d4d3c6 | iota run 输出 | 2026-05-17 | |
| hermes | 2 | f5b15398-da6f-4376-9cc9-06ca8fbac56e | iota run 输出 | 2026-05-17 | |
| hermes | 3 | 87eb4475-741a-45ed-9621-e271e059fff2 | iota run 输出 | 2026-05-17 | |
| opencode | 1 | f6044810-3ac0-45d6-b156-6dca23166922 | iota run 输出 | 2026-05-17 | |
| opencode | 2 | d119104e-b4af-454f-8ea1-34a4a85db6dd | iota run 输出 | 2026-05-17 | |
| opencode | 3 | 8aa0fb3f-7343-463e-9940-ef58d920e836 | iota run 输出 | 2026-05-17 | |

**执行命令记录：**
```bash
PROMPT="请用一句话介绍 Rust 语言的主要特点。"
BACKENDS="claude-code codex gemini hermes opencode"

for backend in $BACKENDS; do
  for run in 1 2 3; do
    ./target/release/iota run --backend $backend "$PROMPT"
    sleep 3
  done
done
```

---

## 三、Token 数据提取（待执行）

### 3.1 提取命令

**方法1: 直接查询 EventStore SQLite**
```bash
sqlite3 ~/.i6/context/events.sqlite "
  SELECT execution_id,
         json_extract(event_json, '\$.payload.input_tokens') as input_tokens,
         json_extract(event_json, '\$.payload.cache_read_input_tokens') as cache_read,
         json_extract(event_json, '\$.payload.cache_creation_input_tokens') as cache_write,
         json_extract(event_json, '\$.payload.output_tokens') as output_tokens
  FROM events
  WHERE event_type = 'token_usage'
    AND execution_id IN (
      'ccd5ec56-3f1c-4044-a707-e96be9d1c895',
      '3bffe027-49df-4243-809d-bd4b5fbfd87d',
      '00805a18-7c47-446d-8f25-c5b1d79388c1',
      '019fc1b8-0380-4e8e-9b41-fcb50ea87c51',
      '23b348b1-24ba-47ac-a84d-8accf31293a5',
      '8be826c8-34b7-4888-901d-da2c986b8bd3',
      '103322f8-ab52-4488-9fb6-ff3264faa152',
      'e60a515b-ed74-48a3-914e-a5a8d0ba8cc5',
      '0a60a1cf-b01e-4b53-9c30-131032567472',
      '71734a9a-43fd-4571-a696-bc06c2d4d3c6',
      'f5b15398-da6f-4376-9cc9-06ca8fbac56e',
      '87eb4475-741a-45ed-9621-e271e059fff2',
      'f6044810-3ac0-45d6-b156-6dca23166922',
      'd119104e-b4af-454f-8ea1-34a4a85db6dd',
      '8aa0fb3f-7343-463e-9940-ef58d920e836'
    )
  ORDER BY execution_id;
"
```

**方法2: 使用 iota logs 查询特定 execution**
```bash
# 对每个 execution_id 执行
./target/release/iota logs <execution_id>
```

**方法3: 使用 --show-native 查看完整输出（fallback）**
```bash
./target/release/iota run --backend <backend> --show-native "$PROMPT"
```

### 3.2 预期字段位置

EventStore 中 token_usage 事件可能在以下位置：
- `event_json.payload.input_tokens`
- `event_json.payload.cache_read_input_tokens`
- `event_json.payload.cache_creation_input_tokens`
- `event_json.payload.output_tokens`

---

## 四、原始数据表（待填充）

| Backend | Run | execution_id | input_tokens | cache_read_input_tokens | cache_creation_input_tokens | output_tokens | total_tokens | 备注 |
|---------|-----|--------------|--------------|------------------------|----------------------------|--------------|--------------|------|
| claude-code | 1 | ccd5ec56-3f1c-4044-a707-e96be9d1c895 | | | | | | |
| claude-code | 2 | 3bffe027-49df-4243-809d-bd4b5fbfd87d | | | | | | |
| claude-code | 3 | 00805a18-7c47-446d-8f25-c5b1d79388c1 | | | | | | |
| codex | 1 | 019fc1b8-0380-4e8e-9b41-fcb50ea87c51 | | | | | | |
| codex | 2 | 23b348b1-24ba-47ac-a84d-8accf31293a5 | | | | | | |
| codex | 3 | 8be826c8-34b7-4888-901d-da2c986b8bd3 | | | | | | |
| gemini | 1 | 103322f8-ab52-4488-9fb6-ff3264faa152 | | | | | | 输出最稳定 |
| gemini | 2 | e60a515b-ed74-48a3-914e-a5a8d0ba8cc5 | | | | | | |
| gemini | 3 | 0a60a1cf-b01e-4b53-9c30-131032567472 | | | | | | |
| hermes | 1 | 71734a9a-43fd-4571-a696-bc06c2d4d3c6 | | | | | | |
| hermes | 2 | f5b15398-da6f-4376-9cc9-06ca8fbac56e | | | | | | |
| hermes | 3 | 87eb4475-741a-45ed-9621-e271e059fff2 | | | | | | |
| opencode | 1 | f6044810-3ac0-45d6-b156-6dca23166922 | | | | | | |
| opencode | 2 | d119104e-b4af-454f-8ea1-34a4a85db6dd | | | | | | |
| opencode | 3 | 8aa0fb3f-7343-463e-9940-ef58d920e836 | | | | | | |

---

## 五、统计汇总表（待计算）

| Backend | input_tokens (avg±std) | cache_read (avg±std) | cache_write (avg±std) | output_tokens (avg±std) | total_tokens (avg±std) | CV total | 稳定性 |
|---------|------------------------|----------------------|----------------------|-------------------------|------------------------|---------|--------|
| claude-code | 待提取 | 待提取 | 待提取 | 待提取 | 待提取 | - | - |
| codex | 待提取 | 待提取 | 待提取 | 待提取 | 待提取 | - | - |
| gemini | 待提取 | 待提取 | 待提取 | 待提取 | 待提取 | - | - |
| hermes | 待提取 | 待提取 | 待提取 | 待提取 | 待提取 | - | - |
| opencode | 待提取 | 待提取 | 待提取 | 待提取 | 待提取 | - | - |

**计算公式：**
- `avg = mean(values)`
- `std = stdev(values)` (样本标准差)
- `CV = (std / avg) × 100%` (变异系数)
- `稳定性判定`: CV < 5% → 非常稳定, 5% ≤ CV < 10% → 较稳定, CV ≥ 10% → 波动大

---

## 六、分析与结论（待填写）

### 6.1 稳定性分析

**待分析项：**
- 各 backend 3 次运行的 input_tokens / output_tokens 标准差
- 变异系数 (CV) 计算
- 稳定性等级判定

### 6.2 Backend 对比分析

**待分析项：**
- 总 token 消耗排序（从高到低）
- input_tokens 差异（不同 backend 的 tokenization 差异）
- output_tokens 差异（生成内容长度）
- 缓存效率对比（如适用）

### 6.3 缓存行为分析

**待分析项：**
- Run 1 vs Run 2/3 的 cache_read_input_tokens 对比
- cache_creation_input_tokens 是否只在首次出现
- 缓存命中率计算（如适用）

### 6.4 结论

**待填写：**
- 哪个 backend 最稳定
- 哪个 backend 总消耗最高/最低
- 各 backend 的缓存机制表现
- 发现的问题和限制

---

## 七、验收矩阵

| # | 验收项 | 状态 | 说明 |
|---|--------|------|------|
| 1 | 数据完整性（15条记录） | ✅ 已完成 | 5 backends × 3 runs = 15 条 execution_id 已记录 |
| 2 | Token字段齐全 | ⏳ 待提取 | input_tokens 等字段需从 EventStore 提取 |
| 3 | 稳定性可量化 | ⏳ 待计算 | 需先提取 token 数据 |
| 4 | Backend排序 | ⏳ 待分析 | 需先提取 token 数据 |
| 5 | 异常情况记录 | ⏳ 待确认 | 需检查是否有失败或缺失 |
| 6 | 数据可复验 | ✅ 已完成 | 15 个 execution_id 均已记录 |

---

## 八、复验命令

```bash
# 1. 环境检查
cargo build --release
./target/release/iota check

# 2. 提取 token 数据（执行以下 SQL）
sqlite3 ~/.i6/context/events.sqlite "
  SELECT execution_id, backend,
         json_extract(event_json, '\$.payload.input_tokens') as input_tokens,
         json_extract(event_json, '\$.payload.cache_read_input_tokens') as cache_read,
         json_extract(event_json, '\$.payload.cache_creation_input_tokens') as cache_write,
         json_extract(event_json, '\$.payload.output_tokens') as output_tokens
  FROM events
  WHERE event_type = 'token_usage'
  ORDER BY execution_id;
"

# 3. 查看特定 execution 的完整事件流
./target/release/iota logs <execution_id>

# 4. 使用 --show-native 获取原始输出（fallback）
./target/release/iota run --backend <backend> --show-native "请用一句话介绍 Rust 语言的主要特点。"
```

---

*文档更新时间：2026-05-17*
*当前状态：已完成 15 轮测试执行，execution_id 已记录，Token 数据待提取*