# iota observability - Architecture Diagrams

## Data Flow: Execution to Storage

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

## Query Path: observability command

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

## Database Schema Relationships

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

## Event Types to Observability

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

## Prometheus Metrics Export

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

## Execution State Machine

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

## TUI Status Display Integration

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

## Module Dependencies

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


event_store.rs
│
├─► Uses: RuntimeEvent, OutputEvent, TokenUsageEvent
│   Location: runtime_event.rs
│
├─► Uses: AcpPromptTiming
│   Location: acp.rs
│
├─► Provides: ExecutionRecord, ObservabilitySummary, PrometheusMetrics
│
└─► Storage: SQLite (rusqlite crate)
    Location: ~/.i6/context/events.sqlite


runtime_event.rs
│
├─► Uses: serde_json::Value
│
├─► Provides: RuntimeEvent enum + variants
│   └─ Used by: event_store.rs, acp.rs
│
└─► Mapping: map_acp_events(method, params) → RuntimeEvent[]


tui/status_bar.rs
│
├─► Uses: ObservabilityMeta
│   Source: tui/state.rs
│
└─► Produces: Status bar text
    Output: "Xms · Y tok · exec Z"
```

