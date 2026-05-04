# iota observability — Quick Reference

## File Locations

| File | Purpose |
|------|---------|
| `src/cli.rs` | CLI command handler & Prometheus export |
| `src/event_store.rs` | SQLite storage, queries, data structures |
| `src/runtime_event.rs` | Event type definitions |
| `src/acp.rs` | `AcpPromptTiming` struct |
| `src/tui/state.rs` | TUI observability state |
| `src/tui/status_bar.rs` | Status bar display |

## CLI Commands

```
iota observability <command> [subcommand] [options]
iota obs ...          # alias
```

### logging — 浏览执行日志与事件流

```bash
iota observability logging recent [--limit N]        # 最近 N 条执行记录
iota observability logging errors [--limit N]        # 仅 failed 执行
iota observability logging events <execution-id>     # 某次执行的完整事件流
iota observability logging tools [--limit N]         # 近期 tool_call 事件
iota observability logging approvals [--limit N]     # 近期 approval_request/decision 事件
```

### tracing — 查看延迟与 timing 数据

```bash
iota observability tracing recent [--limit N]        # 近期执行（含 timing 字段）
iota observability tracing slow [--limit N]          # 最慢的 N 条执行
iota observability tracing breakdown <execution-id>  # 单次执行 5 段耗时分解
iota observability tracing summary                   # avg / p95 延迟统计
```

### metrics — 聚合计数与指标

```bash
iota observability metrics                           # 人类可读 JSON 聚合
iota observability metrics --prometheus              # Prometheus exposition 格式
iota observability metrics tokens                    # token 用量详细拆解
iota observability metrics cache                     # cache hit/miss 比率
iota observability metrics sessions                  # active sessions / queued prompts
iota observability metrics latency                   # 延迟均值 + p95
```

### 软废弃（兼容保留，不在 help 中显示）

```bash
iota observability summary [--limit N]
iota observability recent [--limit N]
```

## Data Structures

### Core Types
- **ExecutionRecord** — 单次执行元数据（14 字段）
- **ObservabilitySummary** — 聚合统计（11 字段 + latest[]）
- **TokenUsageSummary** — token 计数（4 字段）
- **PrometheusMetrics** — Prometheus 导出格式（13 字段）
- **AcpPromptTiming** — 执行时序细分（8 字段）

### RuntimeEvent Variants
- Output, State, ToolCall, ToolResult, Error
- Extension, TokenUsage, Memory
- ApprovalRequest, ApprovalDecision

## SQLite Schema

```
~/.i6/context/events.sqlite

├── events       (execution_id, seq, event_type, event_json, created_at)
├── executions   (execution_id, session_id, backend, request_hash, status,
│                 started_at, finished_at, fencing_token,
│                 process_spawn_ms, init_ms, session_new_ms, prompt_ms, total_ms)
├── observability_counters  (name, value)
└── observability_gauges    (name, value, updated_at)
```

Index: `idx_executions_running_lock` — UNIQUE (backend, request_hash) WHERE status='running'

## EventStore API

### Write Operations
```rust
begin_execution_with_id(backend, session_id, request_hash, execution_id?) -> String
append_event(execution_id, event: &RuntimeEvent) -> i64
finish_execution(execution_id, status: &str)
record_timing(execution_id, timing: &AcpPromptTiming)
record_cache_hit()
record_cache_miss()
set_active_sessions(value: u64)
set_queued_prompts(value: u64)
```

### Query Operations
```rust
recent_executions(limit) -> Vec<ExecutionRecord>
executions_by_status(status, limit) -> Vec<ExecutionRecord>   // 新增
execution_events(execution_id) -> Vec<(i64, String, RuntimeEvent)>  // 新增
events_since(execution_id, after_seq) -> Vec<(i64, RuntimeEvent)>   // 新增
slowest_executions(limit) -> Vec<ExecutionRecord>              // 新增
get_execution(execution_id) -> Option<ExecutionRecord>
output_text(execution_id) -> Option<String>
observability_summary(limit) -> ObservabilitySummary
prometheus_metrics() -> PrometheusMetrics
find_completed_by_request_hash(backend, request_hash) -> Option<ExecutionRecord>
find_running_by_request_hash(backend, request_hash) -> Option<ExecutionRecord>
```

## Prometheus Metrics (17 total)

| Type | Metric |
|------|--------|
| Counter | `iota_execution_attempts_total` |
| Counter | `iota_execution_completed_total` |
| Counter | `iota_execution_failed_total` |
| Counter | `iota_cache_hits_total` |
| Counter | `iota_cache_misses_total` |
| Gauge | `iota_execution_running` |
| Gauge | `iota_active_sessions` |
| Gauge | `iota_queued_prompts` |
| Gauge | `iota_token_usage_events_total` |
| Gauge | `iota_input_tokens_total` |
| Gauge | `iota_output_tokens_total` |
| Gauge | `iota_tokens_total` |
| Gauge | `iota_prompt_latency_ms_avg` |
| Gauge | `iota_total_latency_ms_avg` |
| Gauge | `iota_total_latency_ms_p95` |
| Histogram | `iota_prompt_latency_ms` (buckets: 50ms–60s) |
| Histogram | `iota_init_latency_ms` (same buckets) |

## Key Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `RUNNING_EXECUTION_TTL_SECS` | 3600 | 超时 running 执行自动标记 failed |
| `METRICS_SAMPLE_LIMIT` | 10000 | Prometheus 查询只取最近 1 万条 |
| `RETENTION_DAYS` | 30 | 自动清理 30 天前的完成/失败记录 |

## Cache Key

```rust
SHA256(backend || \0 || cwd || \0 || prompt)
```

## Execution Lifecycle

```
begin_execution_with_id()  → status='running'
append_event()             → 逐条写入事件
record_timing()            → 写入 5 段耗时
finish_execution()         → status='completed'/'failed'
```

## Testing

```bash
cargo test event_store::tests --lib
```

覆盖：执行 ID 幂等性、事件序列化顺序、timing 持久化、summary 聚合计算。
