# telemetry — Observability

OpenTelemetry integration providing OTLP export, structured logging, Prometheus metrics, and distributed tracing.

## Responsibilities

- Initialize OpenTelemetry SDK with OTLP exporter
- Structured logging via `tracing` subscriber
- Prometheus-compatible metrics (counters, histograms)
- Span creation and propagation for distributed tracing
- Console output formatting

## Sub-modules

| Module | Purpose |
|--------|---------|
| `console` | Console log formatting and filtering |
| `logs` | Structured log configuration and export |
| `metrics` | Prometheus metrics: counters, histograms, gauges |
| `spans` | Span creation helpers and context propagation |

## Key Types

- `TelemetryConfig` — OTLP endpoint, service name, log level
- `OtelGuard` — RAII guard for graceful shutdown
