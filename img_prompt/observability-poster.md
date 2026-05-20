# docs/observability.md poster prompt

Selected GPT-Image2 template: `infographic-engine`

Use style tags: `pen-and-ink technical story poster`, `signal routing diagram`, `hand-lettered annotations`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota observability`.

Scene: a simple signal desk with two operating modes side by side. Left side focuses on local files and SQLite databases. Right side shows signals flowing through an OTel Collector into Loki, Jaeger, and Prometheus, combined on a Grafana wall screen. In the center, RuntimeEvent packets flow from the engine to both local storage and OTLP export.

Key telemetry displays:

- Local logs: stderr tracing and daily files under `~/.i6/logs/`, override `IOTA_LOG_DIR`
- Local stores: `~/.i6/context/`, SQLite databases `events.sqlite`, `memory.sqlite`, `approvals.sqlite`, `sessions.sqlite`
- ObservabilityStore: `src/store/observability.rs`, token usage events, execution-level best-record dedupe, P50 / P95 / P99, `token_usage_between(from, to)`, `token_summary_since(ts)`, validation warning `computed > provider_total`
- Docker stack: OTel Collector (`4317 / 4318`) · Loki (`3100`) · Jaeger (`16686`) · Prometheus (`9090`) · Grafana (`3000`)
- OpenTelemetry files: `src/telemetry/mod.rs` · `src/telemetry/metrics.rs` · `src/telemetry/stderr.rs`
- RuntimeEvent packets: `Output` · `Log` · `ToolCall` · `ToolResult` · `TokenUsage` · `Memory` · `ApprovalRequest` · `ApprovalDecision` · `Error`
- CLI command buttons: `iota observability logging recent --limit N` · `iota observability logging events <execution_id>` · `iota observability tokens recent --limit N` · `iota observability tokens summary --since 1h` · `iota observability tokens export --format json` · `iota observability metrics --prometheus` · `iota observability logs <execution_id>` · `iota observability trace <trace_id>` · `iota logs <execution_id>` · `iota trace <trace_id>`

Composition: portrait poster, 2:3 aspect ratio. Left half for local observability, right half for Docker stack, OTel Collector as the central bridge, RuntimeEvent envelopes in the center. Title `iota Observability` at top, subtitle `RuntimeEvent → ObservabilityStore → CLI queries / OTLP export` below.

Style: black ink illustration, crisp lines, engineering notebook feel, readable labels, magenta accent on live telemetry pulses and warning badge. Simple and uncluttered.

Mood: transparent and investigative, showing how every runtime signal is traceable from source to storage.

Text requirements: all visible text must be Chinese or English only. Preserve exact command names, file paths, ports, and environment variable names.

Negative prompt: spaghetti arrows, unreadable text, cluttered icons, cyberpunk glow, photorealistic devices, fake brand logos, random graphs, obsolete module names, `telemetry/console.rs`, `store/approval.rs`, Promtail, wrong command labels, raw API keys, Korean text, non-Chinese non-English text.
