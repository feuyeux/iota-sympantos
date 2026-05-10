# iota observability

## Current Model

iota now uses OpenTelemetry as the primary observability path. The CLI initializes `telemetry::init()` at startup and emits `tracing` logs, OTel metrics, and OTel traces to an OTLP endpoint.

Default endpoint:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
```

Telemetry is enabled by default. Disable export with:

```bash
OTEL_ENABLED=false iota run codex "ping"
```

When export is disabled, iota still installs a `tracing_subscriber` fmt layer and writes logs to stderr. It does not write file logs.

## Where Data Is Stored

### Running Without Docker

If no OTLP collector is listening on `localhost:4317`, the local `iota` process still runs normally, but exported telemetry has nowhere durable to land.

| Signal | Local behavior without Docker |
|--------|-------------------------------|
| Logs | Written to stderr by the console tracing layer. Also attempted as OTLP logs if telemetry is enabled. No `~/.i6/logs` file appender exists in the current implementation. |
| Traces | Attempted as OTLP spans to `OTEL_EXPORTER_OTLP_ENDPOINT`. No local trace database is written by iota. |
| Metrics | Recorded with the OTel meter and attempted as OTLP metrics. No local Prometheus text endpoint or metrics SQLite store is written by iota. |
| Execution cache | `~/.i6/context/events.sqlite`, but this is now `CacheStore` for replay/dedupe only, not an observability event stream. |
| Memory | `~/.i6/context/memory.sqlite` unless `context_engine.memory_db` overrides it. |
| Sessions | `~/.i6/context/sessions.sqlite`. |
| Approvals | `~/.i6/context/approvals.sqlite`. |

`~/.i6/context/events.sqlite` keeps two cache tables: `cache_executions` and `cache_outputs`. Completed and failed cache rows older than 30 days are purged. It does not store full RuntimeEvent audit history, counters, gauges, or Prometheus samples.

### Running With Docker

Start the observability backend from the repository root:

```bash
cd docker/observability
docker compose up -d
```

The stack runs:

| Component | Port | Role |
|-----------|------|------|
| OTel Collector | `4317` gRPC, `4318` HTTP | Receives OTLP from iota and routes signals. |
| Jaeger | `16686` | Stores and queries traces. |
| Prometheus | `9090` | Stores metrics through remote write from the collector. |
| Loki | `3100` | Stores logs through OTLP HTTP from the collector. |
| Grafana | `3000` | Visualizes Jaeger, Prometheus, and Loki datasources. |

With Docker running and the default endpoint unchanged:

| Signal | Docker storage/query path |
|--------|---------------------------|
| Logs | iota -> OTLP gRPC -> Collector -> Loki. Query in Grafana Explore with the Loki datasource, or use `iota logs <execution_id>` against `IOTA_LOKI_URL` / `http://localhost:3100`. Current Loki labels include `service_name="iota"` and `execution_id` when a log event carries an execution id. |
| Traces | iota -> OTLP gRPC -> Collector -> Jaeger. Query in Jaeger UI, Grafana Jaeger datasource, or `iota trace <trace_id>` against `IOTA_JAEGER_URL` / `http://localhost:16686`. |
| Metrics | iota -> OTLP gRPC -> Collector -> Prometheus remote write. Query in Prometheus or Grafana. |

The Docker Compose file does not mount `~/.i6`. Docker is not reading local SQLite files. It only receives telemetry over OTLP.

## CLI Commands

The old `iota observability ...` / `iota obs ...` command group has been removed.

Current observability-related commands:

```bash
iota run codex --log-events "ping"      # print normalized runtime events to stderr for this turn
iota run codex --timing "ping"          # print route and ACP timing JSON to stderr
iota logs <execution_id>                # query Loki at IOTA_LOKI_URL or http://localhost:3100
iota trace <trace_id>                   # query Jaeger at IOTA_JAEGER_URL or http://localhost:16686
```

`--log-events` is a per-run stderr diagnostic stream. It is not a persistent local log store.

`--timing` prints route and ACP phase timing to stderr for the current run. It is not stored in SQLite by the current implementation; timing-related tracing logs and metrics are exported through OTel when telemetry is enabled.

## Environment Variables

| Variable | Default | Meaning |
|----------|---------|---------|
| `OTEL_ENABLED` | enabled | Set to `false` or `0` to disable OTLP exporters. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP gRPC endpoint for logs, traces, and metrics. |
| `IOTA_LOG` | `warn,iota_sympantos=info` | Preferred tracing filter. |
| `RUST_LOG` | fallback only | Used if `IOTA_LOG` is unset. |
| `IOTA_LOKI_URL` | `http://localhost:3100` | Base URL used by `iota logs`. |
| `IOTA_JAEGER_URL` | `http://localhost:16686` | Base URL used by `iota trace`. |

## Data Flow

```text
iota process
  -> tracing macros + OTel metrics API
  -> telemetry::init()
       -> tracing fmt layer to stderr
       -> tracing-opentelemetry span bridge
       -> opentelemetry-appender-tracing log bridge
       -> OTel MeterProvider
  -> OTLP gRPC endpoint, default localhost:4317
  -> OTel Collector
       -> traces  -> Jaeger
       -> metrics -> Prometheus remote write
       -> logs    -> Loki OTLP HTTP
  -> Grafana datasources: Jaeger, Prometheus, Loki
```

## CacheStore Versus Observability

`src/store/cache.rs` preserves execution replay and join-running behavior after the old EventStore was removed.

CacheStore responsibilities:

- request hash dedupe for `(backend, cwd, prompt)`
- completed-output replay
- running-execution lock and join-running lookup
- stale running cleanup after one hour
- 30 day cleanup for completed/failed cache rows

Observability responsibilities now belong to OpenTelemetry, not SQLite.

## Known Gaps

- Execution, memory, tool-call, and approval span helpers are wired into the main paths. Some low-level ACP protocol phases are still represented as tracing logs/metrics rather than nested child spans.
- `iota trace` expects a trace id, not an execution id. Logs are queried by Loki's `execution_id` label with a `service_name="iota"` text-search fallback.
- There is no local Prometheus exposition command in the current CLI.
