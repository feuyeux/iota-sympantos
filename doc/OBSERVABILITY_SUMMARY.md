# iota observability — Implementation Summary

## Overview

`iota observability` 提供对执行指标、性能数据和系统状态的实时观测，分三个主命令：

| 命令 | 职责 |
|------|------|
| `logging` | 浏览执行日志与事件流 |
| `tracing` | 检查延迟与 timing 数据 |
| `metrics` | 查看聚合计数与指标 |

数据持久化在 `~/.i6/context/events.sqlite`，自动保留 30 天。

---

## Command Reference

### `iota observability logging`

```bash
logging recent [--limit N]        # 最近执行记录（id / backend / status / 时间）
logging errors [--limit N]        # 仅 failed 执行
logging events <execution-id>     # 某次执行的完整事件流（seq + event_type + payload）
logging tools [--limit N]         # 近期 tool_call 事件汇总
logging approvals [--limit N]     # 近期 approval_request / decision 事件
```

### `iota observability tracing`

```bash
tracing recent [--limit N]        # 近期执行 + 完整 timing 字段
tracing slow [--limit N]          # 最慢执行（按 total_ms DESC）
tracing breakdown <execution-id>  # 单次执行 5 段耗时分解（JSON）
tracing summary                   # avg / p95 延迟统计
```

`tracing breakdown` 输出格式：
```json
{
  "execution_id": "...",
  "backend": "codex",
  "status": "completed",
  "phases": [
    {"phase": "process_spawn", "ms": 120},
    {"phase": "init",          "ms": 340},
    {"phase": "session_new",   "ms": null},
    {"phase": "prompt",        "ms": 1200},
    {"phase": "total",         "ms": 1680}
  ]
}
```

### `iota observability metrics`

```bash
metrics                  # 人类可读 JSON（executions / latency / tokens / cache / runtime）
metrics --prometheus     # Prometheus text exposition 格式（17 个指标）
metrics tokens           # token 用量拆解 + 每次执行平均值
metrics cache            # hits / misses / hit_rate
metrics sessions         # active_sessions / queued_prompts
metrics latency          # avg_prompt_ms / avg_total_ms / p95_total_ms
```

### 简写与兼容

- `iota obs` = `iota observability`
- `log|trace|metric` 均为对应主命令的别名
- `summary` / `recent` 旧子命令软废弃，保留但不在 help 中展示

---

## Core Components

### CLI Handler — `src/cli.rs`

- `run_observability_command()` — 顶层路由
- `run_obs_logging()` — logging 子命令
- `run_obs_tracing()` — tracing 子命令
- `run_obs_metrics()` — metrics 子命令
- `print_prometheus_metrics()` — Prometheus 文本输出

### Data Storage — `src/event_store.rs`

SQLite，4 张表，WAL 模式，NORMAL sync，30 天自动清理。

**新增查询方法（本次迭代）：**

```rust
pub fn executions_by_status(status: &str, limit: usize) -> Result<Vec<ExecutionRecord>>
pub fn execution_events(execution_id: &str) -> Result<Vec<(i64, String, RuntimeEvent)>>
pub fn events_since(execution_id: &str, after_seq: i64) -> Result<Vec<(i64, RuntimeEvent)>>
pub fn slowest_executions(limit: usize) -> Result<Vec<ExecutionRecord>>
```

### Event Types — `src/runtime_event.rs`

```rust
pub enum RuntimeEvent {
    Output(OutputEvent),
    State(StateEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Error(ErrorEvent),
    Extension(ExtensionEvent),
    TokenUsage(TokenUsageEvent),
    Memory(MemoryEvent),
    ApprovalRequest(ApprovalRequestEvent),
    ApprovalDecision(ApprovalDecisionEvent),
}
```

---

## Data Structures

### ExecutionRecord (14 fields)
```rust
pub struct ExecutionRecord {
    pub execution_id: String,
    pub session_id: String,
    pub backend: String,
    pub request_hash: String,
    pub status: String,           // "running" | "completed" | "failed"
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub fencing_token: i64,
    pub process_spawn_ms: Option<u64>,
    pub init_ms: Option<u64>,
    pub session_new_ms: Option<u64>,
    pub prompt_ms: Option<u64>,
    pub total_ms: Option<u64>,
}
```

### ObservabilitySummary (12 fields)
```rust
pub struct ObservabilitySummary {
    pub total_executions: u64,
    pub completed_executions: u64,
    pub failed_executions: u64,
    pub running_executions: u64,
    pub avg_total_ms: Option<f64>,
    pub avg_prompt_ms: Option<f64>,
    pub p95_total_ms: Option<u64>,
    pub token_usage: TokenUsageSummary,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub active_sessions: u64,
    pub queued_prompts: u64,
    pub latest: Vec<ExecutionRecord>,
}
```

---

## SQLite Schema

```sql
CREATE TABLE executions (
  execution_id    TEXT PRIMARY KEY,
  session_id      TEXT NOT NULL,
  backend         TEXT NOT NULL,
  request_hash    TEXT NOT NULL,
  status          TEXT NOT NULL,      -- running / completed / failed
  started_at      INTEGER NOT NULL,
  finished_at     INTEGER,
  fencing_token   INTEGER NOT NULL DEFAULT 0,
  process_spawn_ms INTEGER,
  init_ms          INTEGER,
  session_new_ms   INTEGER,
  prompt_ms        INTEGER,
  total_ms         INTEGER
);

CREATE TABLE events (
  execution_id TEXT NOT NULL,
  seq          INTEGER NOT NULL,
  event_type   TEXT NOT NULL,
  event_json   TEXT NOT NULL,
  created_at   INTEGER NOT NULL,
  PRIMARY KEY (execution_id, seq)
);

CREATE TABLE observability_counters (name TEXT PRIMARY KEY, value INTEGER NOT NULL DEFAULT 0);
CREATE TABLE observability_gauges   (name TEXT PRIMARY KEY, value INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL);
```

---

## Prometheus Metrics (17 total)

**Counters:** `iota_execution_attempts_total`, `iota_execution_completed_total`, `iota_execution_failed_total`, `iota_cache_hits_total`, `iota_cache_misses_total`

**Gauges:** `iota_execution_running`, `iota_active_sessions`, `iota_queued_prompts`, `iota_token_usage_events_total`, `iota_input_tokens_total`, `iota_output_tokens_total`, `iota_tokens_total`, `iota_prompt_latency_ms_avg`, `iota_total_latency_ms_avg`, `iota_total_latency_ms_p95`

**Histograms:** `iota_prompt_latency_ms`, `iota_init_latency_ms` — buckets: 50, 100, 250, 500, 1k, 2.5k, 5k, 10k, 30k, 60k ms

---

## Design Decisions

| Decision | Value | Reason |
|----------|-------|--------|
| Cache key | SHA256(backend ‖ cwd ‖ prompt) | 内容可寻址，去重 |
| Running TTL | 1 小时 | 自动清理超时执行 |
| Retention | 30 天 | 限制数据库增长 |
| Sample limit | 10,000 | Prometheus 查询 O(1) 延迟 |
| Database mode | WAL + NORMAL sync | 并发安全 + 性能平衡 |
| No direct SQL | CLI 全部通过 EventStore API | 保持封装，便于测试 |

---

## Integration Points

```
IotaEngine → begin_execution_with_id / append_event / record_timing / finish_execution
Cache      → record_cache_hit / record_cache_miss
Daemon     → set_active_sessions / set_queued_prompts
TUI        → ObservabilityMeta (status bar: "145ms · 520 tok · exec abc12345")
```

---

## Testing

```bash
cargo test event_store::tests --lib
```

覆盖：执行 ID 幂等性 · 事件序列顺序 · timing 持久化 · summary 聚合。

---

**Last Updated:** 2026-05-04
