# docs/observability.md poster prompt

Use `gpt-image-2-style-library`: `pen-and-ink technical story poster`, `observatory control room`, `signal routing diagram`, `fine cross-hatching`, `warm paper texture`.

Create a vertical poster for the document `iota observability`.

Scene: iota sits at a simple signal desk with two operating modes side by side. Left side (local): stderr tracing, daily files under `~/.i6/logs/`, and four SQLite stores under `~/.i6/context/` (events · memory · approvals · sessions). Right side (Docker stack): signals flow through an OTel Collector into three destinations — Loki (logs), Jaeger (traces), Prometheus (metrics) — combined on a Grafana wall screen. A small label panel shows key addresses: `OTLP :4317`, `Loki :3100`, `Jaeger :16686`, `Prometheus :9090`, `Grafana :3000`. Below the desk, six RuntimeEvent packet types float as labeled envelopes: Output · ToolCall · TokenUsage · Memory · ApprovalRequest · Error.

Three CLI commands shown as buttons: `iota observability logs <id>`, `iota observability trace <id>`, `iota observability tokens summary`.

Composition: portrait poster, 2:3 aspect ratio. Left half = local fallback, right half = Docker stack, OTel Collector as the bridge. Title `iota Observability` at top.

Style: meticulous black-and-white pen drawing, precise arrows, minimal magenta accent on live telemetry pulses. Clean and readable, not cluttered.

Mood: transparent, investigative — every runtime signal traceable from source to storage.

Negative prompt: blurry dashboards, stock cloud icons, overbright neon, fake brand logos, random graphs, 3D render, old command names (iota logs, iota trace as top-level), obsolete modules (telemetry/console.rs, store/approval.rs, Promtail).