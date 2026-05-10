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

When export is disabled, iota still installs a `tracing_subscriber` fmt layer and writes logs to stderr. Local rolling file logging is enabled by default under `~/.i6/logs/` unless `IOTA_LOG_FILE=off` is set. If OTel exporter setup fails, iota falls back to stderr plus the same local file logging path and continues running.

## Running The CLI With Or Without Docker

The same `iota` binary and the same application config are used in both modes. Docker is only the observability backend.

App/backend configuration always comes from:

```bash
~/.i6/nimia.yaml
```

Docker does not replace `nimia.yaml`, does not mount `~/.i6`, does not run ACP backends for iota, and does not change prompt routing. It only provides a place for OTel logs, traces, and metrics to be stored and queried.

### Without Docker

Run iota normally:

```bash
iota check
iota run codex "ping"
iota run --daemon codex "ping"
iota
```

Behavior:

- iota reads `~/.i6/nimia.yaml`.
- Logs are printed to stderr.
- Logs are also written to daily local files like `~/.i6/logs/iota.log.YYYY-MM-DD` by default.
- If telemetry is enabled, iota attempts to export OTel logs/traces/metrics to `OTEL_EXPORTER_OTLP_ENDPOINT`, default `http://localhost:4317`.
- If no collector is listening there, iota still runs; logs remain available locally, while traces and OTel metrics have no local durable store.
- `iota metrics` can expose local CacheStore counters in Prometheus text format even without Docker.
- Local SQLite stores under `~/.i6/context/` still work for memory, sessions, approvals, and CacheStore replay/dedupe.

To run without OTLP export attempts:

```bash
OTEL_ENABLED=false iota run codex "ping"
```

To disable local file logging for one command:

```bash
IOTA_LOG_FILE=off iota run codex "ping"
```

To change file log retention:

```bash
IOTA_LOG_RETENTION_DAYS=14 iota run codex "ping"
IOTA_LOG_RETENTION_DAYS=off iota run codex "ping"
```

### With Docker

Start the observability services:

```bash
cd docker/observability
docker compose up -d
```

Then run the same iota commands from any working directory:

```bash
iota run codex "ping"
iota run --daemon codex "ping"
iota
```

Behavior:

- iota still reads `~/.i6/nimia.yaml`.
- iota exports OTel logs/traces/metrics to `http://localhost:4317` by default.
- The Docker stack receives those signals through the OTel Collector.
- Logs are stored in Loki, traces in Jaeger, and metrics in Prometheus.

Useful URLs:

```text
Grafana:    http://localhost:3000
Jaeger:     http://localhost:16686
Prometheus: http://localhost:9090
Loki API:   http://localhost:3100
```

### Different Ports Or Remote Observability

You usually do not need a different iota config. Use environment variables only when the OTel/Loki/Jaeger endpoints differ from the defaults:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:14317 iota run codex "ping"
IOTA_LOKI_URL=http://localhost:3100 iota logs <execution_id>
IOTA_JAEGER_URL=http://localhost:16686 iota trace <trace_id>
IOTA_JAEGER_URL=http://localhost:16686 iota trace --execution <execution_id>
```

### End-To-End Verification

The repository includes a lightweight verification script that starts the Docker stack, runs one prompt, extracts `execution_id` from `--timing`, then checks Loki, Jaeger, and Prometheus:

```bash
scripts/verify-observability.sh
```

Optional flags:

```bash
scripts/verify-observability.sh --backend codex --prompt "ping"
scripts/verify-observability.sh --no-restart
scripts/verify-observability.sh --down
```

Environment overrides are still supported: `IOTA_VERIFY_BACKEND`, `IOTA_VERIFY_PROMPT`, port variables, `IOTA_LOKI_URL`, `IOTA_JAEGER_URL`, and `PROMETHEUS_METRIC_QUERIES`. By default the Prometheus check tries `iota_execution_count_total` and `iota_execution_count`, because the collector/Prometheus path may normalize OTel metric names differently.

Requirements: Docker, `curl`, `jq`, Cargo, a working `~/.i6/nimia.yaml`, and a reachable backend/model for the selected backend.

## Where Data Is Stored

### Running Without Docker

If no OTLP collector is listening on `localhost:4317`, the local `iota` process still runs normally, but exported telemetry has nowhere durable to land.

| Signal | Local behavior without Docker |
|--------|-------------------------------|
| Logs | Written to stderr by the console tracing layer and to daily files like `~/.i6/logs/iota.log.YYYY-MM-DD` by default. Also attempted as OTLP logs if telemetry is enabled. |
| Traces | Attempted as OTLP spans to `OTEL_EXPORTER_OTLP_ENDPOINT`. No local trace database is written by iota. |
| Metrics | Recorded with the OTel meter and attempted as OTLP metrics. `iota metrics` exposes local CacheStore counters from `~/.i6/context/events.sqlite` in Prometheus text format. |
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
| Logs | iota -> OTLP gRPC -> Collector -> Loki. Query in Grafana Explore with the Loki datasource, or use `iota logs <execution_id>` against `IOTA_LOKI_URL` / `http://localhost:3100`. `iota logs` tries an `execution_id` label query, then a `service_name="iota"` text filter, then a service-level scan with client-side filtering. |
| Traces | iota -> OTLP gRPC -> Collector -> Jaeger. Query in Jaeger UI, Grafana Jaeger datasource, `iota trace <trace_id>`, or `iota trace --execution <execution_id>` against `IOTA_JAEGER_URL` / `http://localhost:16686`. `trace --execution` first tries to resolve a trace id through Loki, then falls back to Jaeger tag search on `iota.execution.id`; output includes trace id, span count, and spans sorted by start time. |
| Metrics | iota -> OTLP gRPC -> Collector -> Prometheus remote write. Query in Prometheus or Grafana. |

The Docker Compose file does not mount `~/.i6`. Docker is not reading local SQLite files. It only receives telemetry over OTLP.

The compose file pins images by digest instead of using `latest`, so verification runs are not silently changed by upstream image retags. Refresh the digests intentionally when upgrading the observability stack.

### Migrating From The Old Stack

The old SQLite/EventStore metrics stack and Promtail path are retired. If older containers still exist from a previous checkout, remove them before validating current telemetry:

```bash
docker compose -f docker/observability/docker-compose.yml down
docker stop iota-sympantos-promtail-1 iota-sympantos-iota-metrics-exporter-1
docker rm iota-sympantos-promtail-1 iota-sympantos-iota-metrics-exporter-1
cd docker/observability && docker compose up -d
```

## CLI Commands

The old `iota observability ...` / `iota obs ...` command group has been removed.

Current observability-related commands:

```bash
iota run codex --log-events "ping"      # print normalized runtime events to stderr for this turn
iota run codex --timing "ping"          # print route, execution_id, and ACP timing JSON to stderr
iota logs <execution_id>                # query Loki at IOTA_LOKI_URL or http://localhost:3100
iota trace <trace_id>                   # query Jaeger at IOTA_JAEGER_URL or http://localhost:16686
iota trace --execution <execution_id>   # resolve through Loki or Jaeger tag search, then print traces
iota metrics --once                     # print local CacheStore metrics in Prometheus text format
iota metrics --listen 127.0.0.1:47662   # serve local CacheStore metrics at /metrics
```

`--log-events` is a per-run stderr diagnostic stream for normalized runtime events. It is separate from the persistent local tracing file logs under `~/.i6/logs/`.

`--timing` prints route, `execution_id`, and ACP phase timing to stderr for the current run. It is not stored in SQLite by the current implementation; timing-related tracing logs and metrics are exported through OTel when telemetry is enabled.

`iota metrics` is intentionally narrower than the OTel metrics pipeline: it exposes local persistent CacheStore counters such as `iota_cache_executions_total{status="completed"}` and `iota_cache_outputs_total`. Runtime histograms and token counters still belong to the OTel Collector/Prometheus path.

## Environment Variables

| Variable | Default | Meaning |
|----------|---------|---------|
| `OTEL_ENABLED` | enabled | Set to `false` or `0` to disable OTLP exporters. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP gRPC endpoint for logs, traces, and metrics. |
| `IOTA_LOG_FILE` | `auto` | Local file logging mode: `auto`/unset writes daily files, `always` is accepted as an explicit enable, `off`/`false`/`0` disables it. |
| `IOTA_LOG_DIR` | `~/.i6/logs` | Directory for local daily log files. `~/` is expanded at runtime. |
| `IOTA_LOG_RETENTION_DAYS` | `30` | Deletes `iota.log.YYYY-MM-DD` files older than this many days when file logging initializes. Use `off`/`false`/`0`/`none` to disable cleanup. |
| `IOTA_METRICS_ADDR` | `127.0.0.1:47662` | Default listen address for `iota metrics` when `--listen` is not provided. |
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
       -> optional daily file fmt layer under ~/.i6/logs
       -> tracing-opentelemetry span bridge
       -> opentelemetry-appender-tracing log bridge
       -> OTel MeterProvider
  -> OTLP gRPC endpoint, default localhost:4317
  -> OTel Collector
       -> traces  -> Jaeger
       -> metrics -> Prometheus remote write
       -> logs    -> Loki OTLP HTTP
  -> Grafana datasources: Jaeger, Prometheus, Loki

iota metrics
  -> CacheStore snapshot from ~/.i6/context/events.sqlite
  -> Prometheus text format on stdout with --once, or HTTP /metrics when listening
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

## Implementation Notes

- Execution, memory, tool-call, approval, and ACP phase spans are emitted under the active execution context where an `execution_id` exists. Low-level ACP spans include `acp.process.spawn`, `acp.initialize`, `acp.session.new`, and `acp.prompt`.
- `iota metrics` provides local Prometheus exposition for CacheStore counters; full runtime metrics continue through OTel Collector and Prometheus remote write.
- `~/.i6/logs/` cleanup is controlled by `IOTA_LOG_RETENTION_DAYS` and only deletes files matching `iota.log.YYYY-MM-DD`.
