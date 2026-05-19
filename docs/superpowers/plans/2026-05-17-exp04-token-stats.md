# exp04 Token Observability 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 建立从 ACP usage 采集、token 字段归一化、本地 observability 持久化、`iota observability` 查询到 TUI 展示的完整链路，并用 5 个 backend × 3 轮 exp04 实验验证。

**Architecture:** `ACP message -> RuntimeEvent::TokenUsage -> ObservabilityStore -> iota observability -> TUI ObservabilityMeta -> exp04 report`

**Tech Stack:** Rust, SQLite, ratatui, ACP JSON-RPC, OpenTelemetry metrics

---

## 1. 实施原则

1. `--show-native` 只作为 parser 对照和临时 fallback，不作为最终数据源。
2. 所有实验数据最终必须从 `iota observability` 获取。
3. 归一化字段必须保留 raw payload，避免丢失 provider 特有字段。
4. provider reported total 和 normalized total 必须分开记录。
5. 字段缺失要显式表示为 `None` / `N/A`，不能用 `0` 伪装。

---

## 2. Task 1: 扩展 TokenUsage 数据模型

**Files:**

- Modify: `src/runtime_event/mod.rs`
- Modify: `src/runtime_event/tests.rs`
- Modify: `src/tui/state.rs`

- [x] **Step 1: 扩展 `TokenUsageEvent`**

新增字段：

```rust
pub provider: Option<String>,
pub backend: Option<String>,
pub execution_id: Option<String>,
pub session_id: Option<String>,
pub source: Option<String>,
pub input_tokens: Option<u64>,
pub cache_read_input_tokens: Option<u64>,
pub cache_creation_input_tokens: Option<u64>,
pub output_tokens: Option<u64>,
pub thinking_tokens: Option<u64>,
pub tool_use_prompt_tokens: Option<u64>,
pub provider_reported_total_tokens: Option<u64>,
pub normalized_total_tokens: Option<u64>,
pub raw_payload: Value,
```

- [x] **Step 2: 保持兼容字段**

当前 TUI 使用的 `cache_tokens` / `total_tokens` 要么保留兼容访问，要么一次性迁移所有调用点。

- [x] **Step 3: 添加单元测试**

覆盖：

- OpenAI Responses
- OpenAI Chat / Completions
- Anthropic Messages
- Gemini `usageMetadata`
- Gemini ACP `_meta.quota.token_count`
- opencode `thoughtTokens`
- codex `usage_update.used`

---

## 3. Task 2: 实现 TokenUsageNormalizer

**Files:**

- Modify/Add: `src/runtime_event/mod.rs` 或 `src/runtime_event/token_usage.rs`
- Modify: `src/acp/stream_reader.rs`

- [x] **Step 1: 提取 usage source**

识别以下来源：

| Source | 路径 |
| :--------| :------|
| `prompt_result.usage` | `result.usage` |
| `prompt_result.gemini_quota` | `result._meta.quota.token_count` |
| `session_update.usage_update` | `params.update.sessionUpdate == "usage_update"` |
| `session_complete.usage` | `session/complete` params |

- [x] **Step 2: 实现 provider 字段映射**

按 spec 中的 OpenAI / Anthropic / Gemini / adapter-only 口径映射。

- [x] **Step 3: 实现 normalized total 计算**

规则：

- Anthropic: `input + cache_read + cache_creation + output + thinking`
- Gemini: 优先 provider total；否则不重复加 cached content
- OpenAI: 优先 provider total；details 只作为拆分
- Adapter-only: 只有 `provider_reported_total_tokens`，`normalized_total_tokens` 可为空

- [x] **Step 4: 不丢 raw payload**

所有 token event 必须包含原始 usage JSON。

---

## 4. Task 3: 增加 ObservabilityStore

**Files:**

- Add/Modify: `src/store/observability.rs`
- Modify: `src/store/mod.rs`
- Modify: `src/engine/telemetry.rs`
- Modify: `src/engine/prompt.rs`

- [x] **Step 1: 新增 SQLite 表**

创建 `token_usage_events` 表：

```sql
CREATE TABLE IF NOT EXISTS token_usage_events (
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
);
```

- [x] **Step 2: 写入 token usage event**

在 engine 收到 `RuntimeEvent::TokenUsage` 后持久化，必须附带 execution/backend/session 信息。

- [x] **Step 3: 提供查询 API**

实现：

- recent token events
- events by execution id
- backend summary
- JSON export

- [x] **Step 4: 测试持久化**

使用临时 SQLite 文件验证 insert/query/summary。

---

## 5. Task 4: 实现 `iota observability` CLI

**Files:**

- Modify: `src/cli/mod.rs`
- Modify/Add: `src/cli/observability_cmd.rs`

- [x] **Step 1: 增加命令入口**

```bash
iota observability logging recent --limit 20
iota observability logging events <execution_id>
iota observability tokens recent --limit 20
iota observability tokens summary --since 1h
iota observability tokens export --format json
iota observability metrics
iota observability metrics --prometheus
```

- [x] **Step 2: 保持旧入口兼容**

`iota logs <execution_id>` 和 `iota trace <trace_id>` 保持现有语义，不混入本地 token store。

- [x] **Step 3: 输出格式**

`tokens recent` 默认表格输出，`--json` 输出结构化 JSON。

- [x] **Step 4: metrics 输出**

`metrics --prometheus` 至少包含：

```text
iota_token_usage_count
iota_token_input_total
iota_token_cache_read_total
iota_token_cache_creation_total
iota_token_output_total
iota_token_thinking_total
iota_token_provider_reported_total
iota_token_normalized_total
```

---

## 6. Task 5: 接入 TUI 展示

**Files:**

- Modify: `src/tui/state.rs`
- Modify: `src/tui/loop.rs`
- Modify: `src/tui/status_bar.rs`
- Modify: `src/tui/render.rs`
- Modify: `src/tui/scrollback.rs`

- [x] **Step 1: 扩展 `ObservabilityMeta`**

新增：

```rust
pub cache_read_input_tokens: Option<u64>,
pub cache_creation_input_tokens: Option<u64>,
pub thinking_tokens: Option<u64>,
pub provider_reported_total_tokens: Option<u64>,
pub normalized_total_tokens: Option<u64>,
```

- [x] **Step 2: 从 TokenUsageEvent 填充 TUI 状态**

TUI 不解析 raw payload，只使用 normalizer 输出。

- [x] **Step 3: 更新状态栏**

完整格式：

```text
1234ms · in 277 · cache r24154/w3215 · out 85 · think 32 · total 27731 · exec abc12345
```

降级格式：

```text
1234ms · total 23045 · exec abc12345
```

- [x] **Step 4: 更新测试**

覆盖完整 token、只有 total、缺失 execution id 三种场景。

---

## 7. Task 6: 用 exp04 复验链路

**Files:**

- Modify: `gefsi/exp04-token-stats.md`
- Create/Update: `gefsi/logs/exp04-*.log` only when parser fallback is needed

- [x] **Step 1: 构建**

```bash
cargo build --release
```

- [x] **Step 2: 执行 15 轮**

```bash
PROMPT="请用一句话介绍 Rust 语言的主要特点。"
for backend in claude-code codex gemini hermes opencode; do
  for run in 1 2 3; do
    ./target/release/iota run --no-daemon --backend "$backend" "$PROMPT"
    sleep 2
  done
done
```

- [x] **Step 3: 从 observability 查询**

```bash
./target/release/iota observability tokens recent --limit 20 --json
./target/release/iota observability tokens summary --since 1h --json
```

- [x] **Step 4: 更新实验报告**

报告必须包含：

- 原始 observability 数据表
- provider reported total 排序
- normalized total 排序
- 字段缺失说明
- TUI 展示截图或文本记录

---

## 8. 验证命令

```bash
cargo test runtime_event
cargo test observability
cargo test tui
cargo build --release
./target/release/iota observability tokens recent --limit 5
./target/release/iota observability metrics --prometheus
```

---

## 9. 验收矩阵

| # | 验收项 | 判定标准 |
| :---| :--------| :----------|
| 1 | Token parser | OpenAI / Anthropic / Gemini / adapter-only fixtures 全部通过 |
| 2 | 持久化 | token usage events 可按 execution id 查询 |
| 3 | CLI 查询 | `iota observability tokens recent/summary/export` 可用 |
| 4 | Metrics | `metrics --prometheus` 输出 token 聚合指标 |
| 5 | TUI 展示 | 状态栏显示 token/cache/thinking/total/execution id |
| 6 | exp04 数据 | 15 条记录全部来自 `iota observability` |
| 7 | 口径清晰 | 报告区分 provider reported total 和 normalized total |
| 8 | fallback 边界 | `--show-native` 只用于 parser 对照 |

---

*计划更新时间：2026-05-17*
