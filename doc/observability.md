# iota observability

## Overview

`iota observability` 提供对执行指标、性能数据和系统状态的实时观测，分三个主命令：

| 命令 | 职责 |
|------|------|
| `logging` | 浏览执行日志与事件流 |
| `tracing` | 检查延迟与 timing 数据 |
| `metrics` | 查看聚合计数与指标 |

数据持久化在 `~/.i6/context/events.sqlite`，自动保留 30 天。实现为纯本地查询：命令打开 `EventStore::default_path()` 通过类型化 Rust API 查询，不暴露任意 SQL。

---

## Command Reference

### CLI 入口

```
iota observability <group> [subcommand] [options]
iota obs ...          # alias
```

简写与兼容：
- `iota obs` = `iota observability`
- `log` / `trace` / `metric` 均为对应主命令的别名
- `summary` / `recent` 旧子命令软废弃，保留但不在 help 中展示

### logging — 浏览执行日志与事件流

```bash
iota observability logging recent [--limit N]        # 最近 N 条执行记录
iota observability logging errors [--limit N]        # 仅 failed 执行
iota observability logging events <execution-id>     # 某次执行的完整事件流（seq + event_type + payload）
iota observability logging tools [--limit N]         # 近期 tool_call 事件
iota observability logging approvals [--limit N]     # 近期 approval_request/decision 事件
```

### tracing — 查看延迟与 timing 数据

```bash
iota observability tracing recent [--limit N]        # 近期执行（含 timing 字段）
iota observability tracing slow [--limit N]          # 最慢的 N 条执行（按 total_ms DESC）
iota observability tracing breakdown <execution-id>  # 单次执行 5 段耗时分解
iota observability tracing summary                   # avg / p95 延迟统计
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

### metrics — 聚合计数与指标

```bash
iota observability metrics                           # 人类可读 JSON 聚合
iota observability metrics --prometheus              # Prometheus exposition 格式（17 个指标）
iota observability metrics tokens                    # token 用量详细拆解
iota observability metrics cache                     # cache hit/miss 比率
iota observability metrics sessions                  # active sessions / queued prompts
iota observability metrics latency                   # 延迟均值 + p95
```

---

## Architecture Diagrams

### Data Flow: Execution to Storage

```
┌─────────────────────────────────────────────────────────────────┐
│                    IotaEngine::prompt_in_cwd_timed()            │
└────────────────────┬────────────────────────────────────────────┘
                     │
                     ├─► request_hash = SHA256(backend || cwd || prompt)
                     │
                     ├─► Cache lookup: find_completed_by_request_hash()
                     │   │
                     │   └─► Found: record_cache_hit(), return output
                     │
                     ├─► Begin new execution
                     │   │
                     │   └─► EventStore::begin_execution_with_id()
                     │       │
                     │       └─► INSERT INTO executions
                     │           (execution_id='...', status='running')
                     │
                     ├─► Stream ACP events
                     │   │
                     │   ├─► acp::read_prompt_events_for_id()
                     │   │
                     │   ├─► runtime_event::map_acp_events()
                     │   │   (Output, ToolCall, TokenUsage, Error, etc.)
                     │   │
                     │   └─► EventStore::append_event(RuntimeEvent)
                     │       │
                     │       └─► INSERT INTO events
                     │           (execution_id, seq, event_type, event_json)
                     │
                     ├─► Record timing
                     │   │
                     │   └─► EventStore::record_timing(AcpPromptTiming)
                     │       │
                     │       └─► UPDATE executions
                     │           SET prompt_ms, init_ms, total_ms
                     │
                     └─► Finish execution
                         │
                         └─► EventStore::finish_execution("completed"/"failed")
                             │
                             └─► UPDATE executions
                                 SET status, finished_at
```

### Query Path: observability command

```
CLI: iota observability logging/tracing/metrics
     |
     |-> EventStore::open(~/.i6/context/events.sqlite)
     |
     `-> run_observability_command(&args)
         |
         |-> logging: recent_executions / executions_by_status / execution_events
         |   |-> recent and failed execution rows
         |   |-> full event stream for one execution
         |   `-> filtered ToolCall and Approval events from recent executions
         |
         |-> tracing: recent_executions / slowest_executions / get_execution / observability_summary
         |   |-> timing rows ordered by recency or total_ms
         |   |-> process_spawn/init/session_new/prompt/total breakdown
         |   `-> avg and p95 latency summary
         |
         `-> metrics: observability_summary / prometheus_metrics
             |-> JSON aggregate output
             `-> Prometheus Registry + TextEncoder exposition
```

### Execution State Machine

```
                START
                  │
                  ▼
         ┌─────────────────┐
         │  begin_execution │
         │  status='running'│
         └────────┬────────┘
                  │
        ┌─────────▼─────────┐
        │   append_event()   │  (0+ times)
        │   RuntimeEvent  ◄──┼─ Output, TokenUsage, ToolCall, Error, etc.
        └─────────┬─────────┘
                  │
        ┌─────────▼──────────┐
        │  record_timing()    │
        │ (latency breakdown) │
        └─────────┬──────────┘
                  │
        ┌─────────▼───────────────┐
        │  finish_execution()     │
        │  status='completed'/'failed'
        └─────────┬───────────────┘
                  │
        ┌─────────▼─────────────┐
        │  Cache registration   │
        │  (if successful)      │
        │                       │
        │ find_completed_by()   │
        │ ──► record_cache_hit()│
        └─────────┬─────────────┘
                  │
                  ▼
               FINALIZED

STALE CLEANUP (auto-executed on init):
  Running > 1 hour ──► status='failed', finished_at=now
                      (frees cache key lock)
```

### Event Types to Observability

```
RuntimeEvent Enum
├─ Output(OutputEvent)
│  └─► Recorded in events table
│
├─ TokenUsage(TokenUsageEvent)  ◄───── PRIMARY OBSERVABILITY EVENT
│  │
│  └─► Persisted in events table
│      │
│      ├─► Extracted by token_usage_summary()
│      │
│      └─► Aggregated into:
│          ├─ input_tokens total
│          ├─ output_tokens total
│          ├─ total_tokens total
│          └─ token_usage.events count
│
├─ ToolCall(ToolCallEvent)
│  └─► Recorded in events table
│
├─ Error(ErrorEvent)
│  └─► Recorded in events table
│
└─ State(StateEvent)
   └─► Recorded in events table

TIMING FLOW:
AcpPromptTiming (from acp.rs)
├─ process_spawn_ms (subprocess startup)
├─ init_ms (ACP initialization)
├─ session_new_ms (session creation)
├─ prompt_ms (prompt processing)
└─ total_ms (end-to-end)
   │
   └─► record_timing() ──► executions table
       │
       └─► Extracted by observability_summary() for:
           ├─ avg_total_ms
           ├─ avg_prompt_ms
           ├─ p95_total_ms (percentile)
           └─ Prometheus histogram buckets
```

### Database Schema Relationships

```
┌──────────────────────────────────────────────────────────────────┐
│                    executions TABLE                              │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ execution_id TEXT PRIMARY KEY                             │  │
│  │ session_id TEXT                                           │  │
│  │ backend TEXT                                             │  │
│  │ request_hash TEXT  ◄──── SHA256(backend||cwd||prompt)    │  │
│  │ status TEXT  ◄──────────── "running"/"completed"/"failed"│  │
│  │ started_at INTEGER         (Unix timestamp)              │  │
│  │ finished_at INTEGER        (Unix timestamp)              │  │
│  │ fencing_token INTEGER      (monotonic counter)           │  │
│  │ process_spawn_ms INTEGER   (timing breakdown)            │  │
│  │ init_ms INTEGER            (timing breakdown)            │  │
│  │ session_new_ms INTEGER     (timing breakdown)            │  │
│  │ prompt_ms INTEGER          (timing breakdown)            │  │
│  │ total_ms INTEGER           (end-to-end timing)           │  │
│  └────┬─────────────────────────────────────────────────────┘  │
│       │                                                          │
│       │  1:M relationship                                       │
│       │                                                          │
│       └──────────────────┬──────────────────────────────────┐   │
│                          │                                  │   │
│  ┌──────────────────────▼────────────────────────────────┐ │   │
│  │            events TABLE                              │ │   │
│  │ ┌──────────────────────────────────────────────────┐ │ │   │
│  │ │ execution_id TEXT (FK to executions)            │ │ │   │
│  │ │ seq INTEGER  (per-execution sequence)           │ │ │   │
│  │ │ event_type TEXT  (e.g. "output", "token_usage")│ │ │   │
│  │ │ event_json TEXT  (serialized RuntimeEvent)      │ │ │   │
│  │ │ created_at INTEGER (timestamp)                  │ │ │   │
│  │ │ PRIMARY KEY (execution_id, seq)                │ │ │   │
│  │ └──────────────────────────────────────────────────┘ │ │   │
│  └─────────────────────────────────────────────────────┘ │   │
│                                                          │   │
│  ┌─────────────────────────────────────────────────────┐ │   │
│  │   observability_counters TABLE                      │ │   │
│  │ ┌───────────────────────────────────────────────┐ │ │   │
│  │ │ name TEXT PRIMARY KEY                         │ │ │   │
│  │ │ value INTEGER                                 │ │ │   │
│  │ │ ("cache_hit", "cache_miss")                  │ │ │   │
│  │ └───────────────────────────────────────────────┘ │ │   │
│  └─────────────────────────────────────────────────────┘ │   │
│                                                          │   │
│  ┌─────────────────────────────────────────────────────┐ │   │
│  │   observability_gauges TABLE                        │ │   │
│  │ ┌───────────────────────────────────────────────┐ │ │   │
│  │ │ name TEXT PRIMARY KEY                         │ │ │   │
│  │ │ value INTEGER                                 │ │ │   │
│  │ │ updated_at INTEGER                            │ │ │   │
│  │ │ ("active_sessions", "queued_prompts")        │ │ │   │
│  │ └───────────────────────────────────────────────┘ │ │   │
│  └─────────────────────────────────────────────────────┘ │   │
└──────────────────────────────────────────────────────────────┘

Storage Location: ~/.i6/context/events.sqlite
Mode: WAL (write-ahead logging)
Synchronous: NORMAL
Retention: 30 days (auto-purge)
```

### Prometheus Metrics Export

```
EventStore::prometheus_metrics()
│
├─► COUNTERS (increment-only)
│   ├─ iota_execution_attempts_total
│   ├─ iota_execution_completed_total
│   ├─ iota_execution_failed_total
│   ├─ iota_cache_hits_total
│   └─ iota_cache_misses_total
│
├─► GAUGES (current state)
│   ├─ iota_execution_running
│   ├─ iota_active_sessions
│   ├─ iota_queued_prompts
│   ├─ iota_token_usage_events_total
│   ├─ iota_input_tokens_total
│   ├─ iota_output_tokens_total
│   ├─ iota_tokens_total
│   ├─ iota_prompt_latency_ms_avg
│   ├─ iota_total_latency_ms_avg
│   └─ iota_total_latency_ms_p95
│
└─► HISTOGRAMS (distributions)
    ├─ iota_prompt_latency_ms
    │  └─ Buckets: [50, 100, 250, 500, 1k, 2.5k, 5k, 10k, 30k, 60k] ms
    │
    └─ iota_init_latency_ms
       └─ Buckets: [50, 100, 250, 500, 1k, 2.5k, 5k, 10k, 30k, 60k] ms

Output Format: Prometheus Text Exposition (OpenMetrics)
```

### TUI Status Display Integration

```
TUI Render Loop
│
├─► ConversationEntry contains ObservabilityMeta
│   │
│   └─► ObservabilityMeta {
│       ├─ execution_id: Option<String>
│       ├─ total_ms: Option<u64>
│       ├─ prompt_ms: Option<u64>
│       ├─ input_tokens: Option<u64>
│       ├─ output_tokens: Option<u64>
│       └─ total_tokens: Option<u64>
│   }
│
├─► status_bar::render()
│   │
│   └─► observability_status(meta)
│       │
│       └─► Format parts:
│           ├─ If total_ms ──► "{total_ms}ms"
│           ├─ If total_tokens ──► "{total_tokens} tok"
│           ├─ If execution_id ──► "exec {id[0:8]}"
│           │
│           └─ Output: "145ms · 520 tok · exec abc12345"
│
└─► Status Bar Output
    "codex · claude-3-opus  ‖  145ms · 520 tok · exec abc12345  ‖  [↑↓]scroll [Ctrl+B]backend..."
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

### RuntimeEvent Variants

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

### Other Core Types

- **TokenUsageSummary** — token 计数（4 字段）
- **PrometheusMetrics** — Prometheus 导出格式（13 字段）
- **AcpPromptTiming** — 执行时序细分（8 字段）

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

Index: `idx_executions_running_lock` — UNIQUE (backend, request_hash) WHERE status='running'

Idempotency key: `SHA256(backend || "\0" || cwd || "\0" || prompt)`

---

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
executions_by_status(status, limit) -> Vec<ExecutionRecord>
execution_events(execution_id) -> Vec<(i64, String, RuntimeEvent)>
events_since(execution_id, after_seq) -> Vec<(i64, RuntimeEvent)>
slowest_executions(limit) -> Vec<ExecutionRecord>
get_execution(execution_id) -> Option<ExecutionRecord>
output_text(execution_id) -> Option<String>
observability_summary(limit) -> ObservabilitySummary
prometheus_metrics() -> PrometheusMetrics
find_completed_by_request_hash(backend, request_hash) -> Option<ExecutionRecord>
find_running_by_request_hash(backend, request_hash) -> Option<ExecutionRecord>
```

---

## Write Path

```text
IotaEngine::prompt_in_cwd_timed_with_execution_id()
  -> request_hash()
  -> find_completed_by_request_hash()       # cache replay
  -> find_running_by_request_hash()         # join in-flight equivalent work
  -> begin_execution_with_id()              # running lock + fencing token
  -> append_event(State started)
  -> append_event(Output/Tool/Token/Error/Approval...)
  -> record_timing(AcpPromptTiming)
  -> finish_execution("completed" | "failed")
```

TUI 同时更新运行时 gauge：

```text
set_active_sessions(value)
set_queued_prompts(value)
```

---

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

Prometheus latency sampling 限制为最近 10,000 条。

---

## Key Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `RUNNING_EXECUTION_TTL_SECS` | 3600 | 超时 running 执行自动标记 failed |
| `METRICS_SAMPLE_LIMIT` | 10000 | Prometheus 查询只取最近 1 万条 |
| `RETENTION_DAYS` | 30 | 自动清理 30 天前的完成/失败记录 |

---

## CLI Handler — Core Components

| File | Responsibility |
|------|----------------|
| `src/cli.rs` | Observability CLI routing and output formatting |
| `src/event_store.rs` | SQLite schema, writes, queries, metrics aggregation |
| `src/runtime_event.rs` | Normalized event variants |
| `src/acp.rs` | `AcpPromptTiming` and ACP event collection |
| `src/engine.rs` | Execution lifecycle and store write sites |
| `src/tui/state.rs` | TUI observability state |
| `src/tui/status_bar.rs` | TUI observability display |

### Module Dependencies

```
cli.rs
│
├─► Uses: EventStore (observability_summary, recent_executions, prometheus_metrics)
│   Location: event_store.rs
│
├─► Uses: Runtime structures (AcpPromptTiming)
│   Location: acp.rs
│
└─► Produces: JSON (observability_summary, recent) or Prometheus text (metrics)
```

---

## Testing

```bash
# 聚焦测试
cargo test event_store::tests --lib

# 完整验证
cargo test
cargo run -- observability --help
cargo run -- observability logging --help
cargo run -- observability tracing --help
cargo run -- observability metrics --help
```

覆盖：执行 ID 幂等性、事件序列化顺序、timing 持久化、summary 聚合计算。
