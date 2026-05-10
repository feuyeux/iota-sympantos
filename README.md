# iota sympantos

Cross-platform Rust CLI/TUI that routes prompts to five ACP backends (claude-code / codex / gemini / hermes / opencode), sharing a unified memory, skill, and context layer.

## Core features

- **Cross-backend memory** — Rust engine layer SQLite storage (SHA-256 deduplication, FTS5, 6 recall buckets). Memory written by any backend can be recalled and injected by any other backend.
- **Deterministic skills** — YAML-declared skills are dispatched by the Rust engine; trigger matching and output templates are backend-agnostic, so all backends produce consistent structured results.
- **iota-fun multilanguage execution** — 7-language snippet runner (C++ / TypeScript / Rust / Zig / Java / Python / Go), with compilation cache and `parallel: true` support.
- **Daemon hot path** — Optional TCP daemon keeps ACP clients pre-warmed; any command can be routed through it with `--daemon`/`-d`.
- **Interactive TUI** — ratatui loop with multiline editor, Markdown rendering, streaming output, and a permission approval overlay.

## Architecture

![Architecture Overview](images/iota-sympantos-architecture.png)

| Layer | Modules |
|-------|---------|
| **UI** | `src/cli/mod.rs`, `src/tui.rs` + `src/tui/` |
| **Orchestration** | `engine.rs`, `acp/`, `mcp/`, `context/`, `skill/`, `daemon/` |
| **Storage** | `store/memory.rs`, `store/cache.rs`, `store/ledger.rs`, `store/approval.rs` |
| **Observability** | `telemetry/` + Docker OTel Collector / Jaeger / Prometheus / Loki / Grafana |

See [`doc/architecture.md`](doc/architecture.md) and [`doc/code-call-chains.md`](doc/code-call-chains.md) for details.

## Documentation

| Document | Description |
|----------|-------------|
| [`doc/architecture.md`](doc/architecture.md) | System architecture design |
| [`doc/code-call-chains.md`](doc/code-call-chains.md) | Code call chains |
| [`doc/observability.md`](doc/observability.md) | Observability system in depth |
| [`doc/debugging.md`](doc/debugging.md) | Debugging guide |

## Feature lab

| # | Topic | Report |
|---|-------|--------|
| 01 | Cross-backend memory continuity — 6 recall buckets, SHA-256 deduplication, confidence filtering, token budget | [`gefsi/exp01-memory.md`](gefsi/exp01-memory.md) |
| 02 | Skill + iota-fun multilanguage execution — trigger matching, parallel tools, compilation cache, 5-backend consistency | [`gefsi/exp02-skill-fun.md`](gefsi/exp02-skill-fun.md) |

## Quick start

### Build

```bash
cargo build --offline
cargo install --path .
```

### Configuration

Config file: `~/.i6/nimia.yaml`. Key fields for each backend:

```yaml
codex:
  enabled: true
  acp:
    command: npx
    args: ["-y", "@zed-industries/codex-acp@0.12.0"]
  version_mapping:
    acp: "0.12.0"
    bin: "0.128.0"
  model:
    provider: ninerouter
    name: gh/gpt-5.5
    base_url: http://localhost:20128/v1
    api_key: "<router-api-key>"
```

Run `iota check` to inspect the resolved configuration for all backends.

### Running

```bash
iota                                              # interactive TUI
iota run codex "ping"                             # single prompt, direct connection
iota run --daemon codex --timeout-ms 20000 "ping" # routed through daemon (hot path)
iota check                                        # check configuration and backend status
iota logs <execution_id>                          # query execution logs from Loki
iota trace <trace_id>                             # query trace waterfall from Jaeger
iota trace --execution <execution_id>             # resolve trace id from Loki, then query Jaeger
iota metrics --once                               # print local CacheStore Prometheus metrics
iota metrics --listen 127.0.0.1:47662             # expose /metrics for Prometheus scrape
```

`--timing` prints route and ACP phase timing in JSON format to stderr.

### Observability

iota uses OpenTelemetry. By default `iota` sends logs/traces/metrics to `OTEL_EXPORTER_OTLP_ENDPOINT`, defaulting to `http://localhost:4317`. If no Docker observability stack is running, the program still executes; logs are written to stderr and by default to daily rolling files under `~/.i6/logs/`, but OTLP data has no durable backend.

Local file logging is controlled by environment variables:

```bash
IOTA_LOG_FILE=off iota run codex "ping"           # disable local file logging
IOTA_LOG_DIR=/tmp/iota-logs iota run codex "ping" # change local log directory
IOTA_LOG_RETENTION_DAYS=14 iota run codex "ping"  # delete iota.log.YYYY-MM-DD files older than 14 days
```

Start the local observability backend:

```bash
cd docker/observability
docker compose up -d
```

If the default ports are already in use by another stack, override the host ports:

```bash
OTEL_GRPC_PORT=14317 OTEL_HTTP_PORT=14318 JAEGER_PORT=16687 \
PROMETHEUS_PORT=19090 LOKI_PORT=13100 GRAFANA_PORT=13000 \
docker compose up -d
```

Where data goes:

| Signal | Without Docker | With Docker |
|--------|----------------|-------------|
| Logs | stderr + `~/.i6/logs/iota.log.YYYY-MM-DD`; attempted as OTLP when telemetry is enabled | OTel Collector -> Loki; query via Grafana Loki datasource or `iota logs <execution_id>` |
| Traces | Attempted to OTLP endpoint; iota does not write a local trace database | OTel Collector -> Jaeger; query via Jaeger UI / Grafana / `iota trace <trace_id>` |
| Metrics | Recorded by OTel meter and attempted to OTLP endpoint; `iota metrics` can expose local CacheStore metrics | OTel Collector -> Prometheus remote write; query via Grafana / Prometheus |

`~/.i6/context/events.sqlite` is currently the `CacheStore`, used for execution replay/dedupe — it is not an observability event store. See [`doc/observability.md`](doc/observability.md) for details.
