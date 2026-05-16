# OpenTelemetry Alignment Design

Status note: this is the historical design document for the OTel migration. For the current runtime behavior and storage locations, see `docs/observability.md`.

**Date**: 2026-05-09
**Approach**: Plan B - Full replacement of all three signal types

## 1. Architecture Overview

```
iota process
  ├── OTel TracerProvider
  ├── OTel MeterProvider
  └── OTel LoggerProvider
        │
        │ OTLP gRPC
        ▼
  OTel Collector (Docker, :4317/:4318)
  ├── traces  → Jaeger (:16686)
  ├── metrics → Prometheus (:9090)
  └── logs    → Loki (:3100)
                    │
                    ▼
              Grafana (:3000)
```

### Removals

- `prometheus` crate
- `tracing-appender` (file logging)
- `tracing-subscriber` (replaced by OTel pipeline)
- SQLite `EventStore` (`events`, `executions`, `observability_counters`, `observability_gauges` tables)
- `~/.i6/context/events.sqlite`
- `~/.i6/logs/` file logs
- `iota observability logging/timing/metrics` CLI subcommands

### New Rust Dependencies

- `opentelemetry = "0.29"`
- `opentelemetry_sdk = "0.29"`
- `opentelemetry-otlp = "0.29"`
- `tracing-opentelemetry = "0.30"` (bridges existing `tracing` macros to OTel span/log)

### Retained

- `tracing` crate itself (macro calls retained, converted via `tracing-opentelemetry` layer)
- TUI status bar display (data source changed to OTel span/metric callbacks)

### Configuration

- Endpoint via `OTEL_EXPORTER_OTLP_ENDPOINT` or `~/.i6/config.toml`, default `http://localhost:4317`
- Silent discard when unreachable (OTel SDK default behavior)

## 2. Span Model

### Span Hierarchy

```
execution (root span)
  SpanKind: INTERNAL
  Attributes:
    iota.execution.id, iota.session.id, iota.backend, iota.request.hash

  ├── process_spawn (child span, duration = process_spawn_ms)
  ├── init (child span, duration = init_ms)
  ├── session_new (child span, duration = session_new_ms)
  └── prompt (child span, duration = prompt_ms)
      ├── memory.recall (child span, attr: iota.memory.operation=recall)
      ├── tool_call (child span per call, attr: iota.tool.name, iota.tool.call_id)
      ├── memory.search (child span)
      ├── memory.write (child span)
      └── memory.compaction (child span)
```

### Execution Status to Span Status

| execution status | Span Status | Notes |
|---|---|---|
| completed | Ok | |
| failed | Error | error message from RuntimeEvent::Error |
| running timeout | Error | message: "execution timed out" |

### RuntimeEvent to OTel Mapping

| RuntimeEvent | OTel Representation |
|---|---|
| Output | Span Event on prompt span |
| Log | OTel Log Record (see section 4) |
| ToolCall + ToolResult | child span (call start to result end) |
| TokenUsage | Span Event on prompt span + feed to Metric |
| Error | Span Event + set span Status ERROR |
| State | Span Event |
| Memory | child span |
| ApprovalRequest + ApprovalDecision | child span (request start to decision end) |

### Resource Definition

```
service.name: "iota"
service.version: env!("CARGO_PKG_VERSION")
host.name: hostname()
```

## 3. Metrics Model

### Metric Definitions (OTel Semantic Convention Naming)

| Metric Name | Type | Unit | Attributes | Notes |
|---|---|---|---|---|
| iota.execution.count | Counter | {execution} | status: completed/failed | Merged original attempts/completed/failed |
| iota.cache.hit.count | Counter | {hit} | - | |
| iota.cache.miss.count | Counter | {miss} | - | |
| iota.execution.active | UpDownCounter | {execution} | - | Currently running count |
| iota.session.active | UpDownCounter | {session} | - | |
| iota.prompt.queued | UpDownCounter | {prompt} | - | |
| iota.token.usage.count | Counter | {event} | - | Token usage event count |
| iota.token.input | Counter | {token} | - | |
| iota.token.output | Counter | {token} | - | |
| iota.token.total | Counter | {token} | - | |
| iota.prompt.duration | Histogram | s | backend | Original prompt_latency_ms, unit converted to seconds |
| iota.init.duration | Histogram | s | backend | Original init_latency_ms, unit converted to seconds |

### Histogram Bucket Boundaries (seconds)

```
[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]
```

### Instrumentation Points

| Code Location | Metrics Recorded |
|---|---|
| engine.rs finish_execution | iota.execution.count (+1, attr status) |
| engine.rs begin_execution | iota.execution.active (+1) |
| engine.rs finish_execution | iota.execution.active (-1) |
| engine.rs cache hit path | iota.cache.hit.count |
| engine.rs cache miss path | iota.cache.miss.count |
| engine.rs record_timing | iota.prompt.duration, iota.init.duration |
| acp.rs token usage event | iota.token.input/output/total, iota.token.usage.count |
| TUI gauge update points | iota.session.active, iota.prompt.queued |

### Removals

- `prometheus` crate and all Registry/TextEncoder code
- EventStore::prometheus_metrics()
- PrometheusMetrics struct
- observability_counters / observability_gauges tables
- CLI `iota observability metrics --prometheus`

Prometheus exposed via OTel Collector prometheusremotewrite exporter. Metric names auto-convert `.` to `_`, Counters auto-append `_total`.

## 4. Logs Model

### OTel LogRecord Mapping

| LogEvent Field | OTel LogRecord Field | Notes |
|---|---|---|
| ts | Timestamp + ObservedTimestamp | ms to ns |
| level | SeverityNumber + SeverityText | info=9, warn=13, error=17 |
| event | Body | e.g. memory.recall.completed |
| target | InstrumentationScope.name | e.g. iota::engine |
| - | Resource | Shared with Traces |
| execution_id | Attributes["iota.execution.id"] | |
| session_id | Attributes["iota.session.id"] | |
| backend | Attributes["iota.backend"] | |
| route | Attributes["iota.route"] | |
| tool_name | Attributes["iota.tool.name"] | |
| tool_call_id | Attributes["iota.tool.call_id"] | |
| ok | Attributes["iota.ok"] | |
| latency_ms | Attributes["iota.latency_ms"] | |
| fields | Attributes (flattened) | JSON object keys flattened to attributes |

### Trace-Log Correlation

LogRecord automatically carries active span trace_id and span_id, enabling trace-to-log jumps in Grafana.

### Existing tracing Macro Handling

| Current | Approach |
|---|---|
| tracing::info!() / warn!() / error!() | Bridged to OTel LogRecord via tracing-opentelemetry layer |
| tracing::span!() | Bridged to OTel Span (section 2) |

RuntimeEvent::Log structured logs explicitly sent via OTel Logs API.

### Removals

- tracing-appender (file logs ~/.i6/logs/)
- tracing-subscriber fmt layer
- EventStore event writing and queries
- iota observability logging CLI subcommand
- print_log_events() console rendering

### Console Output

| Output | Content |
|---|---|
| stdout | iota logs query results, iota trace query results, iota run execution results |
| stderr | iota run --log-events realtime diagnostic stream, trace/logs URL hints, error messages |

## 5. Docker Compose Stack

### Components and Ports

| Service | Image | Ports | Purpose |
|---|---|---|---|
| otel-collector | otel/opentelemetry-collector-contrib:latest | 4317 (gRPC), 4318 (HTTP) | OTLP receive, distribute to backends |
| jaeger | jaegertracing/jaeger:latest | 16686 (UI) | Traces storage and query |
| prometheus | prom/prometheus:latest | 9090 | Metrics storage and query |
| loki | grafana/loki:latest | 3100 | Logs storage and query |
| grafana | grafana/grafana:latest | 3000 | Unified visualization |

### Collector Configuration

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
      http:
        endpoint: 0.0.0.0:4318

processors:
  batch:
    timeout: 5s
    send_batch_size: 1024

exporters:
  otlphttp/jaeger:
    endpoint: http://jaeger:4317

  prometheusremotewrite:
    endpoint: http://prometheus:9090/api/v1/write

  loki:
    endpoint: http://loki:3100/loki/api/v1/push

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlphttp/jaeger]
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [prometheusremotewrite]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: [loki]
```

### Prometheus Configuration

Enable remote write receiver with startup flag `--web.enable-remote-write-receiver`.

```yaml
global:
  scrape_interval: 15s
```

### Grafana Provisioning

Auto-register three data sources via provisioning: Jaeger, Prometheus, Loki.

### File Layout

```
docker/observability/
├── docker-compose.yml
├── otel-collector-config.yaml
├── prometheus.yml
└── grafana/
    └── provisioning/
        └── datasources/
            └── datasources.yaml
```

### iota Configuration

```toml
# ~/.i6/config.toml or environment variables
[observability]
endpoint = "http://localhost:4317"   # OTEL_EXPORTER_OTLP_ENDPOINT
protocol = "grpc"                     # grpc | http
enabled = true                        # can disable OTel export
```

## 6. Code Change Scope

### File-Level Change List

| File | Change Type | Content |
|---|---|---|
| Cargo.toml | Modify | Remove prometheus, tracing-appender, tracing-subscriber; add opentelemetry, opentelemetry_sdk, opentelemetry-otlp, tracing-opentelemetry |
| src/telemetry.rs | **New** | OTel init: TracerProvider, MeterProvider, LoggerProvider, Resource, OTLP exporter, console processor |
| src/engine.rs | Modify | Remove EventStore calls; use OTel tracer for execution root span + child spans; use OTel meter for metrics |
| src/runtime_event.rs | Modify | Retain enum definition; add to_otel_log() to convert LogEvent to OTel LogRecord |
| src/acp/mod.rs | Modify | Timing data written to OTel span instead of EventStore |
| src/context/server.rs | Modify | Route log changed to OTel log sending |
| src/cli/mod.rs | Modify | Remove init_logging() replaced by telemetry::init(); remove iota observability subcommands; remove Prometheus output; add iota logs / iota trace commands |
| src/store/events.rs | **Delete** | Entire EventStore module |
| src/store/mod.rs | Modify | Remove events module declaration |
| src/tui.rs | Modify | ObservabilityMeta changed to OTel span callback data source |
| src/tui/status_bar.rs | Modify | Render logic unchanged, data source changed |
| src/mcp/router.rs | Modify | Replace EventStore references |
| src/acp/permission.rs | Modify | Approval events changed to OTel span |

### New Module src/telemetry.rs

```rust
pub fn init(config: &ObservabilityConfig) -> OtelGuard {
    // 1. Build Resource (service.name, version, host)
    // 2. Build OTLP gRPC exporter (configurable endpoint)
    // 3. Init TracerProvider + BatchSpanProcessor
    // 4. Init MeterProvider + PeriodicReader
    // 5. Init LoggerProvider + BatchLogProcessor
    // 6. Register tracing-opentelemetry layer (bridge tracing macros)
    // 7. Register ConsoleProcessor (stdout/stderr output)
    // 8. Return OtelGuard (Drop flushes + shuts down)
}

pub struct OtelGuard { /* providers */ }
impl Drop for OtelGuard {
    fn drop(&mut self) {
        // flush all providers, graceful shutdown
    }
}
```

### Global Access Pattern

```rust
use opentelemetry::global;
let tracer = global::tracer("iota");
let meter = global::meter("iota");
let logger = global::logger("iota");
```

## 7. CLI Command Changes

### Removed Commands

Entire `iota observability` / `iota obs` command group deleted (all logging/timing/metrics subcommands).

### New Commands

| Command | Data Source | Output | Description |
|---|---|---|---|
| iota logs (execution_id) | Loki API | stdout | Query all logs for a single execution |
| iota trace (execution_id) | Jaeger API | stdout | Query span waterfall for a single execution |

### Retained CLI

| Command | Description |
|---|---|
| iota run --log-events | stderr realtime log stream (console processor driven) |

### Auto Output After Execution (stderr)

```
trace:  http://localhost:16686/trace/<trace_id>
logs:   http://localhost:3000/explore?left={"queries":[{"expr":"{iota_execution_id=\"<execution_id>\"}"}]}
```

## 8. Log Gap Fill

### Currently Existing Log Nodes

| Node | Event Name |
|---|---|
| memory recall start/complete/fail | memory.recall.started/completed/failed |
| memory inject | memory.inject |
| memory search call/result | memory.search.call/result |
| memory write call/result | memory.write.call/result |
| memory compaction | memory.compaction |

### Logs To Add

| Phase | Event Name | Level | Key Attributes |
|---|---|---|---|
| **Execution Lifecycle** | | | |
| Execution start | execution.started | INFO | execution_id, backend, session_id, request_hash |
| Execution complete | execution.completed | INFO | execution_id, total_ms, status |
| Execution failure | execution.failed | ERROR | execution_id, error, total_ms |
| **Cache** | | | |
| Cache hit | cache.hit | INFO | execution_id, request_hash |
| Cache miss | cache.miss | DEBUG | request_hash |
| **ACP Process** | | | |
| Process spawn | acp.process.spawn | INFO | backend, process_spawn_ms |
| Init complete | acp.init.completed | INFO | backend, init_ms |
| Session created | acp.session.created | INFO | session_id, session_new_ms |
| Process exit | acp.process.exit | INFO | backend, exit_code |
| Process crash | acp.process.crash | ERROR | backend, exit_code, stderr |
| **Prompt** | | | |
| Prompt sent | prompt.sent | INFO | execution_id, prompt_len, backend |
| First token | prompt.first_token | DEBUG | execution_id, ttft_ms |
| Prompt complete | prompt.completed | INFO | execution_id, prompt_ms |
| **Tool Calls** | | | |
| Tool call start | tool.call.started | INFO | tool_name, tool_call_id, execution_id |
| Tool call complete | tool.call.completed | INFO | tool_name, tool_call_id, ok, latency_ms |
| Tool call failure | tool.call.failed | ERROR | tool_name, tool_call_id, error |
| **Approval** | | | |
| Approval request | approval.requested | INFO | tool_name, execution_id |
| Approval decision | approval.decided | INFO | tool_name, decision, latency_ms |
| **Token** | | | |
| Token usage | token.usage | INFO | input_tokens, output_tokens, total_tokens |
| **Output** | | | |
| Stream chunk | output.chunk | DEBUG | execution_id, chunk_len |
| Final output | output.final | INFO | execution_id, output_len |
| **Error** | | | |
| Runtime error | runtime.error | ERROR | execution_id, error, source |
| **MCP Route** | | | |
| Route request | mcp.route.request | INFO | route, method |
| Route response | mcp.route.response | INFO | route, status, latency_ms |
| Route error | mcp.route.error | ERROR | route, error |

### Log Level Strategy

| Level | Usage |
|---|---|
| ERROR | Failures, crashes, abnormal exits |
| WARN | Timeout retries, degradation |
| INFO | Key lifecycle nodes (default visible) |
| DEBUG | High-frequency/detailed data (chunk, cache miss, first token) |

Default log level INFO, adjustable via OTEL_LOG_LEVEL or iota config.
