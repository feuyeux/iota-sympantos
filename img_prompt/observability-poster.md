# docs/observability.md poster prompt

Use `gpt-image-2-style-library`: `pen-and-ink technical story poster`, `observatory control room`, `signal routing diagram`, `fine cross-hatching`, `hand-drawn infrastructure map`, `warm paper texture`.

Create a vertical poster for the document `iota observability`.

Scene: show iota as a small command desk sending three kinds of signals into an old-fashioned observatory: logs, traces, and metrics. The signals travel through brass-labeled tubes into an OpenTelemetry Collector at the center. From there, three paths branch to Loki as a log archive library, Jaeger as a trace telescope charting spans across the sky, and Prometheus as a metric gauge wall with moving needles. Grafana appears as a large wall screen that combines the three views. A separate lower-left corner shows local operation without Docker: stderr, daily files under `~/.i6/logs/`, and SQLite stores under `~/.i6/context/`.

Composition: portrait poster, 2:3 aspect ratio. Central hub-and-spoke layout with the OTel Collector as the main switching lens. Put Docker observability stack on the right side and local fallback behavior on the left side. Include readable short labels: `OTLP :4317`, `Loki :3100`, `Jaeger :16686`, `Prometheus :9090`, `Grafana :3000`, `OTEL_ENABLED=false`, and `iota logs / iota trace / iota metrics`. Add the title `iota Observability` at the top.

Style: meticulous black-and-white pen drawing, Victorian scientific instrument meets modern infrastructure diagram, cross-hatching, stippled shadows, precise arrows, minimal magenta accent on live telemetry pulses. Avoid making it look like a generic cloud architecture slide.

Mood: investigative, transparent, a control room where every runtime signal can be followed from source to storage.

Negative prompt: blurry dashboards, unreadable labels, stock cloud icons, overbright neon, colorful SaaS illustration, 3D render, abstract blobs, random graphs without meaning, fake brand logos.
