# iota-sympantos Observability CLI Implementation Analysis

## Overview
The `iota observability` CLI subcommand provides comprehensive insights into execution metrics, tracing, and performance data. The system uses SQLite for persistent storage and exposes three main subcommands: `summary`, `recent`, and `metrics`.

---

## 1. CLI Command Handler

### Location: `src/cli.rs` (lines 59-187)

#### Main Entry Point
```rust
"observability" | "obs" => {
    return run_observability_command(&args[1..]);
}
```

#### Handler Function
```rust
fn run_observability_command(args: &[String]) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("summary");
    if matches!(command, "-h" | "--help" | "help") {
        print_observability_help();
        return Ok(());
    }
    let limit = parse_limit(args).unwrap_or(10);
    let store = EventStore::open(&EventStore::default_path()?)?;
    match command {
        "summary" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.observability_summary(limit)?)?
            );
        }
        "recent" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.recent_executions(limit)?)?
            );
        }
        "metrics" => {
            print_prometheus_metrics(&store)?;
        }
        other => anyhow::bail!(
            "Unknown observability command '{}'. Expected summary, recent, or metrics",
            other
        ),
    }
    Ok(())
}
```

#### Subcommands
1. **summary** — Aggregate metrics: execution counts, latency averages, p95, tokens, recent executions
2. **recent** — Recent execution records with persisted timing fields
3. **metrics** — Prometheus text exposition metrics

#### Help Text
```
Usage:
  iota observability summary [--limit N]
  iota observability recent [--limit N]
  iota observability metrics

Commands:
  summary   Print aggregate execution counts, latency averages, p95, tokens, and recent executions
  recent    Print recent execution records with persisted timing fields
  metrics   Print Prometheus text exposition metrics
```

---

## 2. Data Structures

### ExecutionRecord
**Location:** `src/event_store.rs` (lines 20-35)

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub session_id: String,
    pub backend: String,
    pub request_hash: String,
    pub status: String,                    // "running", "completed", "failed"
    pub started_at: i64,                   // Unix timestamp
    pub finished_at: Option<i64>,          // Unix timestamp
    pub fencing_token: i64,                // Used for ordering
    pub process_spawn_ms: Option<u64>,     // Time to spawn process
    pub init_ms: Option<u64>,              // ACP initialization time
    pub session_new_ms: Option<u64>,       // Session creation time
    pub prompt_ms: Option<u64>,            // Prompt execution time
    pub total_ms: Option<u64>,             // Total end-to-end time
}
```

### ObservabilitySummary
**Location:** `src/event_store.rs` (lines 37-52)

```rust
#[derive(Debug, Clone, serde::Serialize)]
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
    pub latest: Vec<ExecutionRecord>,      // Recent N records
}
```

### TokenUsageSummary
**Location:** `src/event_store.rs` (lines 54-60)

```rust
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TokenUsageSummary {
    pub events: u64,              // Number of token usage events
    pub input_tokens: u64,        // Total input tokens
    pub output_tokens: u64,       // Total output tokens
    pub total_tokens: u64,        // Total tokens
}
```

### PrometheusMetrics
**Location:** `src/event_store.rs` (lines 62-78)

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct PrometheusMetrics {
    pub execution_attempts: u64,
    pub execution_completed: u64,
    pub execution_failed: u64,
    pub execution_running: u64,
    pub avg_total_ms: Option<f64>,
    pub avg_prompt_ms: Option<f64>,
    pub p95_total_ms: Option<u64>,
    pub prompt_latency_ms: Vec<u64>,       // Histogram bucket values
    pub init_latency_ms: Vec<u64>,         // Histogram bucket values
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub active_sessions: u64,
    pub queued_prompts: u64,
    pub token_usage: TokenUsageSummary,
}
```

### ObservabilityMeta (TUI)
**Location:** `src/tui/state.rs`

```rust
#[derive(Debug, Clone, Default)]
pub struct ObservabilityMeta {
    pub execution_id: Option<String>,
    pub total_ms: Option<u64>,
    pub prompt_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}
```

### AcpPromptTiming
**Location:** `src/acp.rs` (lines 102-114)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpPromptTiming {
    pub client_started: bool,
    pub process_spawned: bool,
    pub process_spawn_ms: Option<u64>,     // Time to spawn ACP process
    pub init_ms: Option<u64>,              // Time for ACP initialization
    pub session_reused: bool,
    pub session_new_ms: Option<u64>,       // Time to create new session
    pub prompt_ms: u64,                    // Time to process prompt
    pub total_ms: u64,                     // Total end-to-end time
}
```

### RuntimeEvent
**Location:** `src/runtime_event.rs` (lines 5-18)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum RuntimeEvent {
    Output(OutputEvent),
    State(StateEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Error(ErrorEvent),
    Extension(ExtensionEvent),
    TokenUsage(TokenUsageEvent),           // Captures token metrics
    Memory(MemoryEvent),
    ApprovalRequest(ApprovalRequestEvent),
    ApprovalDecision(ApprovalDecisionEvent),
}
```

#### TokenUsageEvent
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageEvent {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub model: Option<String>,
    pub payload: Value,
}
```

---

## 3. SQLite Schema

### Location: `src/event_store.rs` (lines 375-416)

#### Database Setup
- **Storage Location:** `~/.i6/context/events.sqlite`
- **WAL Mode:** Yes (for concurrency)
- **Synchronous:** NORMAL (for performance)
- **Retention:** 30 days for completed/failed executions

#### Table: events
```sql
CREATE TABLE IF NOT EXISTS events (
  execution_id TEXT NOT NULL,
  seq INTEGER NOT NULL,                   -- Sequence within execution
  event_type TEXT NOT NULL,               -- "output", "token_usage", "tool_call", etc.
  event_json TEXT NOT NULL,               -- Serialized RuntimeEvent
  created_at INTEGER NOT NULL,            -- Unix timestamp
  PRIMARY KEY (execution_id, seq)
);
```

#### Table: executions
```sql
CREATE TABLE IF NOT EXISTS executions (
  execution_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  backend TEXT NOT NULL,                  -- "codex", "claude", "gemini", etc.
  request_hash TEXT NOT NULL,             -- SHA256(backend || cwd || prompt)
  status TEXT NOT NULL,                   -- "running", "completed", "failed"
  started_at INTEGER NOT NULL,            -- Unix timestamp
  finished_at INTEGER,                    -- Unix timestamp
  fencing_token INTEGER NOT NULL DEFAULT 0,
  process_spawn_ms INTEGER,               -- Optional timing
  init_ms INTEGER,
  session_new_ms INTEGER,
  prompt_ms INTEGER,
  total_ms INTEGER
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_executions_running_lock 
  ON executions(backend, request_hash) WHERE status = 'running';
```

#### Table: observability_counters
```sql
CREATE TABLE IF NOT EXISTS observability_counters (
  name TEXT PRIMARY KEY,
  value INTEGER NOT NULL DEFAULT 0
);

-- Counters tracked:
--   "cache_hit"   - Execution cache hits
--   "cache_miss"  - Execution cache misses
```

#### Table: observability_gauges
```sql
CREATE TABLE IF NOT EXISTS observability_gauges (
  name TEXT PRIMARY KEY,
  value INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);

-- Gauges tracked:
--   "active_sessions"   - Active ACP sessions
--   "queued_prompts"    - Queued TUI prompts
```

---

## 4. EventStore API

### Location: `src/event_store.rs`

#### Execution Lifecycle

```rust
// Begin a new execution
pub fn begin_execution_with_id(
    &self,
    backend: &str,
    session_id: &str,
    request_hash: &str,
    execution_id: Option<&str>,
) -> Result<String>

// Record an individual event (output, token usage, tool call, etc.)
pub fn append_event(&self, execution_id: &str, event: &RuntimeEvent) -> Result<i64>

// Mark execution as finished
pub fn finish_execution(&self, execution_id: &str, status: &str) -> Result<()>

// Store timing metrics
pub fn record_timing(&self, execution_id: &str, timing: &AcpPromptTiming) -> Result<()>
```

#### Cache Tracking

```rust
pub fn record_cache_hit(&self) -> Result<()>
pub fn record_cache_miss(&self) -> Result<()>
```

#### Session/Prompt Metrics

```rust
pub fn set_active_sessions(&self, value: u64) -> Result<()>
pub fn set_queued_prompts(&self, value: u64) -> Result<()>
```

#### Query Methods

```rust
// Find a cached execution by content hash
pub fn find_completed_by_request_hash(
    &self,
    backend: &str,
    request_hash: &str,
) -> Result<Option<ExecutionRecord>>

// Find a running execution (used for join/dedup)
pub fn find_running_by_request_hash(
    &self,
    backend: &str,
    request_hash: &str,
) -> Result<Option<ExecutionRecord>>

// Get specific execution
pub fn get_execution(&self, execution_id: &str) -> Result<Option<ExecutionRecord>>

// Get execution output text
pub fn output_text(&self, execution_id: &str) -> Result<Option<String>>

// Get recent executions
pub fn recent_executions(&self, limit: usize) -> Result<Vec<ExecutionRecord>>
```

#### Observability Queries

```rust
// Get summary statistics
pub fn observability_summary(&self, limit: usize) -> Result<ObservabilitySummary>

// Get Prometheus metrics
pub fn prometheus_metrics(&self) -> Result<PrometheusMetrics>
```

---

## 5. Prometheus Metrics Export

### Location: `src/cli.rs` (lines 189-310)

#### Registered Metrics

**Execution Counters:**
- `iota_execution_attempts_total` - Total recorded executions
- `iota_execution_completed_total` - Completed executions
- `iota_execution_failed_total` - Failed executions
- `iota_execution_running` - Currently running executions

**Token Counters:**
- `iota_token_usage_events_total` - Token usage events captured
- `iota_input_tokens_total` - Captured input tokens
- `iota_output_tokens_total` - Captured output tokens
- `iota_tokens_total` - Captured total tokens

**Cache Counters:**
- `iota_cache_hits_total` - Completed execution cache hits
- `iota_cache_misses_total` - Completed execution cache misses

**Latency Gauges:**
- `iota_prompt_latency_ms_avg` - Average prompt latency
- `iota_total_latency_ms_avg` - Average total latency
- `iota_total_latency_ms_p95` - P95 total latency

**System Gauges:**
- `iota_active_sessions` - Active ACP sessions
- `iota_queued_prompts` - Queued TUI prompts

**Histograms:**
- `iota_prompt_latency_ms` - Prompt latency histogram
  - Buckets: 50, 100, 250, 500, 1000, 2500, 5000, 10000, 30000, 60000 ms
- `iota_init_latency_ms` - ACP initialization latency histogram
  - Same buckets as prompt latency

#### Sample Output
```
# HELP iota_execution_attempts_total Total recorded executions
# TYPE iota_execution_attempts_total gauge
iota_execution_attempts_total 42.0

# HELP iota_execution_completed_total Completed executions
# TYPE iota_execution_completed_total gauge
iota_execution_completed_total 40.0

# HELP iota_cache_hits_total Completed execution cache hits
# TYPE iota_cache_hits_total counter
iota_cache_hits_total 5.0

# HELP iota_prompt_latency_ms Prompt latency in milliseconds
# TYPE iota_prompt_latency_ms histogram
iota_prompt_latency_ms_bucket{le="50.0"} 2.0
iota_prompt_latency_ms_bucket{le="100.0"} 5.0
iota_prompt_latency_ms_bucket{le="250.0"} 15.0
...
```

---

## 6. Query Helpers

### Location: `src/event_store.rs` (lines 470-600)

#### Execution Counting

```rust
fn count_status(conn: &Connection, status: Option<&str>) -> Result<u64>
// Counts all or filtered by status: "running", "completed", "failed"
```

#### Timing Analysis

```rust
enum TimingColumn {
    TotalMs,
    PromptMs,
    InitMs,
}

fn avg_column(conn: &Connection, column: TimingColumn) -> Result<Option<f64>>
// Computes average latency

fn percentile_total_ms_limited(
    conn: &Connection,
    percentile: f64,
    limit: usize,
) -> Result<Option<u64>>
// Computes p95 from last N samples

fn latency_values_limited(
    conn: &Connection,
    column: TimingColumn,
    limit: usize,
) -> Result<Vec<u64>>
// Returns histogram bucket values
```

#### Token Analysis

```rust
fn token_usage_summary(conn: &Connection) -> Result<TokenUsageSummary>
// Aggregates token usage from TokenUsageEvent records
```

#### Observability State

```rust
fn counter_value(conn: &Connection, name: &str) -> Result<u64>
// Gets counter value (cache hits/misses)

fn gauge_value(conn: &Connection, name: &str) -> Result<u64>
// Gets gauge value (active sessions, queued prompts)
```

---

## 7. Data Flow: Execution → Storage

### Single Execution Lifecycle

```
1. IotaEngine::prompt_in_cwd_timed()
   ↓
2. EventStore::begin_execution_with_id()
   → INSERT INTO executions (execution_id, backend, request_hash, status='running')
   ↓
3. During execution:
   a. ACP session/update events arrive
   b. map_acp_events() converts to RuntimeEvent
   c. EventStore::append_event(RuntimeEvent)
      → INSERT INTO events (execution_id, seq, event_type, event_json)
   ↓
4. EventStore::record_timing(AcpPromptTiming)
   → UPDATE executions SET prompt_ms, init_ms, total_ms
   ↓
5. EventStore::finish_execution("completed" | "failed")
   → UPDATE executions SET status, finished_at
   ↓
6. Summary includes:
   - Execution status changes
   - All events per execution
   - Timing per execution
   - Aggregated metrics
```

### Cache Hit Detection

```
request_hash = SHA256(backend || cwd || prompt)
  ↓
EventStore::find_completed_by_request_hash(backend, request_hash)
  ↓
If found:
  - EventStore::record_cache_hit()
  - Return cached output_text()
  ↓
If not found:
  - Execute normally
  - EventStore::record_cache_miss()
```

---

## 8. TUI Integration

### Status Bar Display

**Location:** `src/tui/status_bar.rs` (lines 86-101)

```rust
fn observability_status(observability: &ObservabilityMeta) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(total_ms) = observability.total_ms {
        parts.push(format!("{}ms", total_ms));
    }
    if let Some(tokens) = observability.total_tokens {
        parts.push(format!("{} tok", tokens));
    }
    if let Some(execution_id) = observability.execution_id.as_deref() {
        let short = execution_id.chars().take(8).collect::<String>();
        if !short.is_empty() {
            parts.push(format!("exec {}", short));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

// Output example: "145ms · 520 tok · exec abc12345"
```

---

## 9. Key Design Decisions

### 1. Request Hashing for Cache Key
```rust
pub fn request_hash(backend: &str, cwd: &Path, prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(backend.as_bytes());
    hasher.update(b"\0");
    hasher.update(cwd.as_os_str().to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(prompt.as_bytes());
    hex::encode(hasher.finalize())
}
```
- Enables cache lookups on (backend, cwd, prompt) tuple
- Independent of execution ID or timing
- Used for deduplication and replay

### 2. Fencing Tokens
- Per-backend monotonic counter
- Ensures ordering in concurrent executions
- Prevents lost updates with multiple processes

### 3. Event Streaming
- Events stored sequentially per execution
- Enables replay and debugging of execution flow
- Compressed JSON storage (no binary overhead)

### 4. Sample Limiting
```rust
const METRICS_SAMPLE_LIMIT: usize = 10_000;
```
- Prometheus metrics computed on last 10k samples
- Bounds query latency on large datasets
- Percentile computation uses limited window

### 5. Auto-Purge
```rust
const RETENTION_DAYS: i64 = 30;
```
- Completed/failed executions purged after 30 days
- Events cascade-deleted with executions
- Bounds database growth

### 6. Running Execution TTL
```rust
const RUNNING_EXECUTION_TTL_SECS: i64 = 60 * 60;
```
- Marks executions as "failed" if running for > 1 hour
- Prevents stale locks on cache key
- Automatic cleanup of crashed sessions

---

## 10. Example Queries

### All-time Summary
```
iota observability summary --limit 10
```
Returns:
```json
{
  "total_executions": 152,
  "completed_executions": 148,
  "failed_executions": 2,
  "running_executions": 2,
  "avg_total_ms": 523.4,
  "avg_prompt_ms": 412.1,
  "p95_total_ms": 1280,
  "token_usage": {
    "events": 145,
    "input_tokens": 23450,
    "output_tokens": 12340,
    "total_tokens": 35790
  },
  "cache_hits": 34,
  "cache_misses": 114,
  "active_sessions": 2,
  "queued_prompts": 0,
  "latest": [ ... ExecutionRecord[] ... ]
}
```

### Recent Executions
```
iota observability recent --limit 20
```

### Prometheus Metrics
```
iota observability metrics | curl -X POST --data-binary @- http://localhost:9091/metrics/job/iota
```

---

## 11. Testing

### Location: `src/event_store.rs` (lines 613-727)

Test coverage includes:
1. **Execution ID conflict detection** - Ensures idempotency
2. **Event sequencing** - Verifies order and type preservation
3. **Timing persistence** - Validates latency storage and aggregation
4. **Summary computation** - Tests statistics accuracy

---

## 12. Architecture Layers

```
┌─────────────────────────────────────────┐
│  CLI: observability subcommand          │  cli.rs
│  (summary, recent, metrics)             │
└────────────────────┬────────────────────┘
                     │
┌────────────────────▼────────────────────┐
│  EventStore Query API                   │  event_store.rs
│  (observability_summary,                │
│   recent_executions,                    │
│   prometheus_metrics)                   │
└────────────────────┬────────────────────┘
                     │
┌────────────────────▼────────────────────┐
│  SQLite Database                        │  ~/.i6/context/events.sqlite
│  (executions, events, counters, gauges) │
└─────────────────────────────────────────┘
```

---

## 13. Integration Points

### From IotaEngine
```rust
// Before prompt execution:
EventStore::begin_execution_with_id()

// During ACP streaming:
EventStore::append_event(RuntimeEvent)

// After timing collected:
EventStore::record_timing(AcpPromptTiming)

// On completion:
EventStore::finish_execution(status)
```

### From TUI
```rust
// Extract for status bar:
ObservabilityMeta {
    execution_id: Some(id),
    total_ms: Some(timing.total_ms),
    total_tokens: Some(usage.total_tokens),
}
```

### From Cache System
```rust
// On hit/miss:
EventStore::record_cache_hit()
EventStore::record_cache_miss()
```

### From Agent/Daemon
```rust
// Session tracking:
EventStore::set_active_sessions(count)
EventStore::set_queued_prompts(count)
```

---

## 14. Future Extensions

Based on architecture and module growth plan:

1. **Event export** - Add streaming export to external observability systems
2. **Custom aggregations** - Time-series bucketing (hourly, daily)
3. **Percentile histograms** - P50, P90, P99 in addition to P95
4. **Tool call analysis** - Per-tool success rates and latencies
5. **Error categorization** - Group failures by error type
6. **Memory efficiency** - Event compression or archival
