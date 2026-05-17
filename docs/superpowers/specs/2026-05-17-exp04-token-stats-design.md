---
name: exp04-token-stats-design
description: 实验4设计文档：统一 token observability 链路，并用 5 个 backend 的 token 统计实验验证采集、查询和 TUI 展示
metadata:
  type: spec
  experiment: exp04-token-stats
  date: 2026-05-17
---

# iota-sympantos 实验4：Token Observability 链路设计

| 字段 | 值 |
|------|-----|
| 实验代号 | exp04-token-stats |
| 设计日期 | 2026-05-17 |
| 终极目标 | 统一从 `iota observability` 获取 token 消耗数据 |
| 验证对象 | claude-code、codex、gemini、hermes、opencode |
| 参考实验 | exp01-memory, exp03-acp-runtime |

---

## 1. 设计目标

exp04 不应停留在 `--show-native` 日志解析。最终链路必须满足：

1. 所有 ACP backend 上报的 usage 信息都进入统一的 `RuntimeEvent::TokenUsage`。
2. token usage 被持久化到本地 observability store，可按 `execution_id` 回溯。
3. CLI 通过 `iota observability ...` 查询 token 明细、汇总和 metrics。
4. TUI 使用同一份归一化结构展示本轮耗时、token、缓存和 execution id。
5. `--show-native` 只作为 parser 调试和 fallback 验证，不作为实验的长期数据源。

---

## 2. 当前问题

当前实现和旧实验设计之间存在偏差：

| 问题 | 影响 |
|------|------|
| 当前 CLI 没有 `iota observability` 子命令 | 旧计划中的优先数据源不可执行 |
| `~/.i6/context/events.sqlite` 当前只保存 execution lifecycle | 无法从本地 store 查询 token usage 事件 |
| `TokenUsageEvent` 只有 `input/cache/output/total` | 无法区分 cache read、cache write、thinking、provider total |
| 不同平台 usage 字段语义不同 | 直接排序 `total_tokens` 会混淆 OpenAI、Anthropic、Gemini 口径 |
| TUI 只显示 `input|cache|output` | 无法展示 cache write、thinking token 和 total 口径 |
| exp04 结果使用 `--show-native` 手工抽取 | 可复验性弱，容易产生抄录错误 |

---

## 3. Token 字段语义

### 3.1 平台字段对比

| 语义 | OpenAI Responses API | OpenAI Chat / Completions | Anthropic Messages API | Gemini / Google GenAI |
|------|----------------------|---------------------------|------------------------|-----------------------|
| 输入 token | `usage.input_tokens` | `usage.prompt_tokens` | `usage.input_tokens` | `usageMetadata.promptTokenCount` |
| 输出 token | `usage.output_tokens` | `usage.completion_tokens` | `usage.output_tokens` | `usageMetadata.candidatesTokenCount` |
| 总 token | `usage.total_tokens` | `usage.total_tokens` | 无直接字段，需计算 | `usageMetadata.totalTokenCount` |
| 缓存命中输入 token | `usage.input_tokens_details.cached_tokens` | `usage.prompt_tokens_details.cached_tokens` | `usage.cache_read_input_tokens` | `usageMetadata.cachedContentTokenCount` |
| 缓存写入输入 token | 无常规同名字段 | 无常规同名字段 | `usage.cache_creation_input_tokens` | 无常规同名字段 |
| 推理 / thinking token | `usage.output_tokens_details.reasoning_tokens` | `usage.completion_tokens_details.reasoning_tokens` | 通常无单独 usage 字段 | `usageMetadata.thoughtsTokenCount` |
| 工具结果回灌 token | 工具/事件口径不统一 | 工具/事件口径不统一 | `usage.server_tool_use.web_search_requests` 等 | `usageMetadata.toolUsePromptTokenCount` |
| 流式 usage | final usage / usage event，视接口 | 需 `stream_options.include_usage` | stream 事件中有 usage | 最后一个 chunk 才有 `usageMetadata` |

### 3.2 归一化原则

| 字段 | 含义 | 说明 |
|------|------|------|
| `input_tokens` | provider 报告的输入 token 主字段 | Gemini 中已包含 cached content；Anthropic 中不含 cache read/write |
| `cache_read_input_tokens` | 缓存命中的输入 token | OpenAI 从 details 提取，Anthropic 从 `cache_read_input_tokens` 提取，Gemini 从 `cachedContentTokenCount` 提取 |
| `cache_creation_input_tokens` | 缓存写入 token | Anthropic 支持；其他平台通常为空 |
| `output_tokens` | 模型输出 token | 不包含 thinking，除非 provider 自身把它合入输出 |
| `thinking_tokens` | reasoning/thinking token | OpenAI/Gemini/opencode adapter 可提供，缺失时为空 |
| `tool_use_prompt_tokens` | 工具结果回灌 token | 只在 provider 明确上报时填写 |
| `provider_reported_total_tokens` | provider 或 adapter 报告的总数 | 例如 OpenAI `total_tokens`、Gemini `totalTokenCount`、ACP `usage_update.used` |
| `normalized_total_tokens` | iota 统一计算口径 | 用于跨 backend 排序，必须记录计算策略 |
| `raw_payload` | 原始 usage JSON | 用于排查字段映射和后续扩展 |

`normalized_total_tokens` 的初始计算规则：

1. Anthropic 口径：`input_tokens + cache_read_input_tokens + cache_creation_input_tokens + output_tokens + thinking_tokens`。
2. Gemini 口径：优先使用 `provider_reported_total_tokens`；否则用 `input_tokens + output_tokens + thinking_tokens + tool_use_prompt_tokens`，不重复加 cached content。
3. OpenAI 口径：优先使用 `provider_reported_total_tokens`；cache read 仅作为输入拆分，不重复加总。
4. Adapter-only 口径：如果只有 `usage_update.used`，只填 `provider_reported_total_tokens`，`normalized_total_tokens` 为空或标记为 adapter total。

---

## 4. 目标架构

```text
ACP stdout/stderr JSON-RPC
  -> AcpWireMessage
  -> runtime_event::map_acp_events()
  -> TokenUsageNormalizer
  -> RuntimeEvent::TokenUsage
  -> ObservabilityStore.persist_runtime_event()
  -> AcpPromptOutput.events
  -> CLI observability queries
  -> TUI ObservabilityMeta
```

### 4.1 采集层

采集层负责从 ACP 消息中提取 usage：

| ACP 来源 | 示例 | 处理方式 |
|----------|------|----------|
| `prompt:<id>.result.usage` | claude-code、hermes、opencode | 解析 `usage` 对象 |
| `prompt:<id>.result._meta.quota.token_count` | gemini | 解析 Gemini quota，并保留 `_meta` |
| `session/update usage_update` | codex、opencode、hermes | 作为 adapter-level total 或 context window usage |
| `session/complete` | 部分 adapter | 同样走 normalizer |

采集必须同时记录 `backend`、`execution_id`、`session_id`、`model` 和 source path，例如 `prompt_result.usage` 或 `session_update.usage_update`。

### 4.2 归一化层

新增 `TokenUsageNormalizer`，职责：

1. 识别 provider / adapter 字段形态。
2. 映射标准字段。
3. 计算 `normalized_total_tokens`。
4. 保留 `provider_reported_total_tokens` 和 `raw_payload`。
5. 不因字段缺失丢弃事件，缺失字段保持 `None`。

### 4.3 持久化层

新增或扩展本地 observability store，建议表：

```text
token_usage_events(
  id TEXT PRIMARY KEY,
  ts INTEGER NOT NULL,
  execution_id TEXT,
  session_id TEXT,
  backend TEXT NOT NULL,
  model TEXT,
  provider TEXT,
  source TEXT NOT NULL,
  input_tokens INTEGER,
  cache_read_input_tokens INTEGER,
  cache_creation_input_tokens INTEGER,
  output_tokens INTEGER,
  thinking_tokens INTEGER,
  tool_use_prompt_tokens INTEGER,
  provider_reported_total_tokens INTEGER,
  normalized_total_tokens INTEGER,
  raw_payload_json TEXT NOT NULL
)
```

可选汇总表：

```text
execution_token_summaries(
  execution_id TEXT PRIMARY KEY,
  backend TEXT NOT NULL,
  model TEXT,
  input_tokens INTEGER,
  cache_read_input_tokens INTEGER,
  cache_creation_input_tokens INTEGER,
  output_tokens INTEGER,
  thinking_tokens INTEGER,
  provider_reported_total_tokens INTEGER,
  normalized_total_tokens INTEGER,
  updated_at INTEGER NOT NULL
)
```

### 4.4 CLI 查询层

新增统一命令：

```bash
iota observability logging recent --limit 20
iota observability logging events <execution_id>
iota observability tokens recent --limit 20
iota observability tokens summary --since 1h
iota observability tokens export --format json
iota observability metrics
iota observability metrics --prometheus
```

`tokens recent` 输出每轮 execution 的 token 明细；`tokens summary` 输出 backend 维度 mean/std/CV；`metrics` 输出进程或本地 store 聚合指标。

### 4.5 TUI 展示层

TUI 不直接解析 backend 原始字段，只消费 `RuntimeEvent::TokenUsage` 或 execution summary。

建议状态栏格式：

```text
1234ms · in 277 · cache r24154/w3215 · out 85 · think 32 · total 27731 · exec abc12345
```

字段缺失时降级：

```text
1234ms · total 23045 · exec abc12345
```

scrollback / pager 可展示更长的明细：

```text
tokens: input=277 cache_read=24154 cache_write=3215 output=85 normalized_total=27731 source=prompt_result.usage
```

---

## 5. exp04 验证方法

### 5.1 实验 prompt

```text
请用一句话介绍 Rust 语言的主要特点。
```

### 5.2 执行流程

```bash
cargo build --release
./target/release/iota check

PROMPT="请用一句话介绍 Rust 语言的主要特点。"
for backend in claude-code codex gemini hermes opencode; do
  for run in 1 2 3; do
    ./target/release/iota run --no-daemon --backend "$backend" "$PROMPT"
    sleep 2
  done
done
```

### 5.3 数据提取流程

终态只从 observability 获取：

```bash
./target/release/iota observability tokens recent --limit 20
./target/release/iota observability tokens summary --since 1h
./target/release/iota observability logging events <execution-id>
```

`--show-native` 仅用于 parser 对照：

```bash
./target/release/iota run --no-daemon --backend claude-code --show-native "$PROMPT"
```

---

## 6. 验收标准

| # | 验收项 | 判定标准 |
|---|--------|----------|
| 1 | 采集完整 | 5 个 backend 的 usage 或 adapter total 均能生成 `RuntimeEvent::TokenUsage` |
| 2 | 持久化可查 | 每条 token usage 可通过 `iota observability logging events <execution_id>` 回溯 |
| 3 | token 明细可查 | `iota observability tokens recent` 能输出 15 条 exp04 记录 |
| 4 | 汇总可量化 | `iota observability tokens summary` 能按 backend 输出 mean、std、CV |
| 5 | 口径明确 | 报告同时展示 provider reported total 和 normalized total |
| 6 | TUI 展示 | TUI 状态栏显示耗时、token、cache、execution id，字段缺失时可降级 |
| 7 | fallback 定位清晰 | `--show-native` 只用于 parser 验证，不作为最终实验数据源 |
| 8 | 异常可解释 | Codex 等只提供 total 的 backend 在 CLI/TUI/报告中明确标注字段缺失 |

---

## 7. 预期结果

1. claude-code 和 hermes 可上报 input/output/cache read，claude-code 还可能上报 cache write。
2. gemini 可上报 prompt/output，后续应扩展到 `usageMetadata` 全字段。
3. opencode 可上报 input/output/thinking/provider total。
4. codex ACP 当前可能只上报 `usage_update.used`，因此只能进入 adapter total。
5. exp04 的最终报告应以 observability 查询结果为准，现有 `gefsi/logs/exp04-*.log` 只作为基线样本。

---

## 8. 实现计划入口

详细任务见：

```text
docs/superpowers/plans/2026-05-17-exp04-token-stats.md
```

---

*设计更新时间：2026-05-17*
