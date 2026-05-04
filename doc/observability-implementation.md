# iota-sympantos Observability CLI Implementation Reference

## Overview

`iota observability` reads the local SQLite event store and exposes three current command groups:

| Group | Purpose |
|---|---|
| `logging` | Browse executions, failures, event streams, tool calls, and approval events |
| `tracing` | Inspect timing records, slow executions, per-execution breakdowns, and latency summary |
| `metrics` | Print aggregate JSON or Prometheus exposition metrics |

The store is `~/.i6/context/events.sqlite`. The implementation is intentionally local-only: commands open `EventStore::default_path()` and query through typed Rust APIs rather than exposing arbitrary SQL.

Compatibility aliases still exist for older scripts: `iota obs` maps to `iota observability`, `log`/`trace`/`metric` map to the three groups, and old top-level `summary` / `recent` subcommands are still accepted but soft-deprecated.

## CLI Routing

Entry point: `src/cli.rs`

```text
iota observability <group> [subcommand] [options]
  -> run_observability_command()
  -> EventStore::open(EventStore::default_path())
  -> run_obs_logging() | run_obs_tracing() | run_obs_metrics()
```

### Logging

```bash
iota observability logging recent [--limit N]
iota observability logging errors [--limit N]
iota observability logging events <execution-id>
iota observability logging tools [--limit N]
iota observability logging approvals [--limit N]
```

Primary APIs:

```text
recent_executions(limit)
executions_by_status("failed", limit)
execution_events(execution_id)
```

`tools` and `approvals` scan recent execution event streams and filter normalized `RuntimeEvent::ToolCall`, `RuntimeEvent::ApprovalRequest`, and `RuntimeEvent::ApprovalDecision` values.

### Tracing

```bash
iota observability tracing recent [--limit N]
iota observability tracing slow [--limit N]
iota observability tracing breakdown <execution-id>
iota observability tracing summary
```

Primary APIs:

```text
recent_executions(limit)
slowest_executions(limit)
get_execution(execution_id)
observability_summary(0)
```

`breakdown` renders the five stored timing fields: `process_spawn_ms`, `init_ms`, `session_new_ms`, `prompt_ms`, and `total_ms`.

### Metrics

```bash
iota observability metrics
iota observability metrics --prometheus
iota observability metrics tokens
iota observability metrics cache
iota observability metrics sessions
iota observability metrics latency
```

The default metrics command prints JSON grouped into executions, latency, tokens, cache, and runtime. `--prometheus` builds a `prometheus::Registry` in-process from `EventStore::prometheus_metrics()` and prints text exposition.

## Stored Data Model

`EventStore` owns four SQLite tables:

```text
executions                one row per request/execution
events                    ordered RuntimeEvent stream per execution
observability_counters    cache hit/miss counters
observability_gauges      active sessions and queued prompts
```

`ExecutionRecord` stores execution identity, backend, request hash, status, fencing token, timestamps, and timing fields. `RuntimeEvent` stores normalized ACP/tool/token/memory/approval events as JSON.

The idempotency key is:

```text
SHA256(backend || "\0" || cwd || "\0" || prompt)
```

The partial unique index on `(backend, request_hash)` for running executions prevents duplicate active work. Stale running rows older than one hour are marked failed before new work begins or running rows are queried.

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

TUI also updates runtime gauges:

```text
set_active_sessions(value)
set_queued_prompts(value)
```

## Prometheus Metrics

Current exposition includes counters/gauges/histograms for execution counts, cache counts, active sessions, queued prompts, token totals, average/p95 latency, prompt latency distribution, and init latency distribution.

Metric names include:

```text
iota_execution_attempts_total
iota_execution_completed_total
iota_execution_failed_total
iota_cache_hits_total
iota_cache_misses_total
iota_execution_running
iota_active_sessions
iota_queued_prompts
iota_token_usage_events_total
iota_input_tokens_total
iota_output_tokens_total
iota_tokens_total
iota_prompt_latency_ms_avg
iota_total_latency_ms_avg
iota_total_latency_ms_p95
iota_prompt_latency_ms
iota_init_latency_ms
```

Prometheus latency sampling is capped by `METRICS_SAMPLE_LIMIT = 10000`.

## Related Files

| File | Responsibility |
|---|---|
| `src/cli.rs` | Observability CLI routing and output formatting |
| `src/event_store.rs` | SQLite schema, writes, queries, metrics aggregation |
| `src/runtime_event.rs` | Normalized event variants |
| `src/acp.rs` | `AcpPromptTiming` and ACP event collection |
| `src/engine.rs` | Execution lifecycle and store write sites |
| `src/tui/status_bar.rs` | TUI observability display |

## Testing

Focused tests:

```bash
cargo test event_store::tests --lib
```

Broader verification:

```bash
cargo test
cargo run -- observability --help
cargo run -- observability logging --help
cargo run -- observability tracing --help
cargo run -- observability metrics --help
```
