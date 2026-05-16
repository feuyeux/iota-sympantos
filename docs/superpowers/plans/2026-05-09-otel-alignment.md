# OpenTelemetry Alignment Implementation Plan

Status note: this is the historical implementation plan for the OTel migration. Some steps describe pre-migration code such as `EventStore` and old observability commands. For current runtime behavior and storage locations, see `docs/observability.md`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the custom SQLite-based observability stack with full OpenTelemetry SDK integration, exporting traces/metrics/logs via OTLP to a Docker-based Jaeger + Prometheus + Loki + Grafana backend.

**Architecture:** iota process uses OTel Rust SDK (TracerProvider, MeterProvider, LoggerProvider) to emit all three signal types over OTLP gRPC to an OTel Collector, which routes traces to Jaeger, metrics to Prometheus, and logs to Loki. Grafana provides unified visualization. The existing SQLite EventStore is deleted — execution cache/replay/dedupe logic is extracted into a minimal CacheStore. Existing `tracing` macros are bridged to OTel via `tracing-opentelemetry`.

**Tech Stack:** Rust, opentelemetry 0.29, opentelemetry_sdk 0.29, opentelemetry-otlp 0.29, tracing-opentelemetry 0.30, Docker Compose, OTel Collector, Jaeger, Prometheus, Loki, Grafana

---

## Critical Pre-Requisite: EventStore Decomposition

The current `EventStore` (`src/store/events.rs`) handles both observability AND execution cache/replay/dedupe. The spec calls for deleting EventStore entirely, but these non-observability features must be preserved:

- `request_hash()` — idempotency key generation
- `begin_execution_with_id()` — execution identity, fencing tokens, stale cleanup
- `find_completed_by_request_hash()` — replay cache lookup
- `find_running_by_request_hash()` — join in-flight dedup
- `output_text()` — replay output reconstruction
- `finish_execution()` — status tracking for replay eligibility

These will be extracted into a new minimal `CacheStore` before deleting EventStore.

## File Structure

### New Files
| File | Responsibility |
|---|---|
| `src/telemetry.rs` | OTel SDK initialization (TracerProvider, MeterProvider, LoggerProvider, OTLP exporter, console processor, OtelGuard) |
| `src/telemetry/metrics.rs` | OTel metric instrument definitions and recording helpers |
| `src/telemetry/spans.rs` | Span creation helpers for execution, ACP phases, tool calls |
| `src/telemetry/logs.rs` | OTel log record emission, LogEvent-to-LogRecord conversion |
| `src/telemetry/console.rs` | Console stdout/stderr processor for realtime output |
| `src/store/cache.rs` | Minimal CacheStore — execution identity, replay, dedupe (extracted from EventStore) |
| `docker/observability/docker-compose.yml` | Full observability stack |
| `docker/observability/otel-collector-config.yaml` | Collector pipeline config |
| `docker/observability/prometheus.yml` | Prometheus config |
| `docker/observability/grafana/provisioning/datasources/datasources.yaml` | Grafana data source provisioning |

### Modified Files
| File | Changes |
|---|---|
| `Cargo.toml` | Remove prometheus/tracing-appender/tracing-subscriber; add OTel crates |
| `src/main.rs` | Add `mod telemetry;` declaration |
| `src/store/mod.rs` | Remove `pub mod events;`, add `pub mod cache;` |
| `src/engine.rs` | Replace EventStore with CacheStore + OTel spans/metrics/logs |
| `src/cli/mod.rs` | Replace init_logging with telemetry::init; remove observability commands; add `iota logs`/`iota trace` |
| `src/runtime_event.rs` | Add `to_otel_log()` method on LogEvent |
| `src/acp/mod.rs` | Add OTel spans for ACP phases |
| `src/acp/permission.rs` | Add OTel spans for approval flow |
| `src/context/server.rs` | Replace `emit_route_log` with OTel log emission |
| `src/tui.rs` | Remove EventStore field, keep ObservabilityMeta from AcpPromptOutput |
| `src/tui/status_bar.rs` | No changes needed (data source is ObservabilityMeta, unchanged) |

### Deleted Files
| File | Reason |
|---|---|
| `src/store/events.rs` | Replaced by CacheStore + OTel |
| `src/store/events_tests.rs` | Tests for deleted module |

---

### Task 1: Docker Compose Observability Stack

**Files:**
- Create: `docker/observability/docker-compose.yml`
- Create: `docker/observability/otel-collector-config.yaml`
- Create: `docker/observability/prometheus.yml`
- Create: `docker/observability/grafana/provisioning/datasources/datasources.yaml`

- [ ] **Step 1: Create directory structure**

Run: `mkdir -p docker/observability/grafana/provisioning/datasources`

- [ ] **Step 2: Create OTel Collector config**

Create `docker/observability/otel-collector-config.yaml`:

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

- [ ] **Step 3: Create Prometheus config**

Create `docker/observability/prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s
```

- [ ] **Step 4: Create Grafana datasource provisioning**

Create `docker/observability/grafana/provisioning/datasources/datasources.yaml`:

```yaml
apiVersion: 1

datasources:
  - name: Jaeger
    type: jaeger
    access: proxy
    url: http://jaeger:16686
    isDefault: false
    editable: true

  - name: Prometheus
    type: prometheus
    access: proxy
    url: http://prometheus:9090
    isDefault: true
    editable: true

  - name: Loki
    type: loki
    access: proxy
    url: http://loki:3100
    isDefault: false
    editable: true
```

- [ ] **Step 5: Create Docker Compose file**

Create `docker/observability/docker-compose.yml`:

```yaml
services:
  otel-collector:
    image: otel/opentelemetry-collector-contrib:latest
    ports:
      - "4317:4317"
      - "4318:4318"
    volumes:
      - ./otel-collector-config.yaml:/etc/otelcol-contrib/config.yaml
    depends_on:
      - jaeger
      - loki

  jaeger:
    image: jaegertracing/jaeger:latest
    ports:
      - "16686:16686"
    environment:
      - COLLECTOR_OTLP_ENABLED=true

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    command:
      - --config.file=/etc/prometheus/prometheus.yml
      - --web.enable-remote-write-receiver
      - --storage.tsdb.retention.time=30d

  loki:
    image: grafana/loki:latest
    ports:
      - "3100:3100"

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
      - GF_AUTH_ANONYMOUS_ENABLED=true
      - GF_AUTH_ANONYMOUS_ORG_ROLE=Viewer
    volumes:
      - ./grafana/provisioning:/etc/grafana/provisioning
    depends_on:
      - jaeger
      - prometheus
      - loki
```

- [ ] **Step 6: Verify stack starts**

Run:
```bash
cd docker/observability && docker compose up -d
```
Expected: All 5 services start. Verify with:
```bash
docker compose ps
```
Expected: all services show "running" status.

Then verify endpoints:
```bash
curl -s http://localhost:4317 || echo "gRPC port open (expected no HTTP response)"
curl -s http://localhost:16686/ | head -c 100
curl -s http://localhost:9090/-/ready
curl -s http://localhost:3100/ready
curl -s http://localhost:3000/api/health
```

- [ ] **Step 7: Tear down and commit**

Run:
```bash
cd docker/observability && docker compose down
```

```bash
git add docker/observability/
git commit -m "feat: add Docker Compose observability stack (OTel Collector + Jaeger + Prometheus + Loki + Grafana)"
```

---

### Task 2: Update Cargo Dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Remove old telemetry dependencies**

In `Cargo.toml`, remove these three lines from `[dependencies]`:
```toml
tracing-appender = "0.2"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
prometheus = "0.13"
```

- [ ] **Step 2: Add OTel dependencies**

In `Cargo.toml`, add to `[dependencies]`:
```toml
opentelemetry = "0.29"
opentelemetry_sdk = { version = "0.29", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.29", features = ["grpc-tonic", "trace", "metrics", "logs"] }
tracing-opentelemetry = "0.30"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "registry"] }
tonic = "0.12"
opentelemetry-appender-tracing = "0.29"
hostname = "0.4"
```

Note: `tracing-subscriber` is kept but reconfigured — we still need `Registry` and `EnvFilter` for the `tracing-opentelemetry` layer. `tracing-appender` and `prometheus` are fully removed.

- [ ] **Step 3: Verify it compiles (expect errors — that's OK at this stage)**

Run:
```bash
cargo check 2>&1 | head -50
```
Expected: Compilation errors in files that import removed crates. This is expected and will be fixed in subsequent tasks.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: update deps - remove prometheus/tracing-appender, add opentelemetry stack"
```

---

### Task 3: Extract CacheStore from EventStore

**Files:**
- Create: `src/store/cache.rs`
- Modify: `src/store/mod.rs`

This extracts the non-observability parts of EventStore (execution identity, replay cache, dedupe) into a minimal CacheStore. The observability parts (event stream, counters, gauges, prometheus metrics, observability summary) are NOT copied — they will be replaced by OTel.

- [ ] **Step 1: Create CacheStore**

Create `src/store/cache.rs`:

```rust
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::runtime_event::{OutputEvent, RuntimeEvent};
use crate::utils::now_ts;

const RUNNING_EXECUTION_TTL_SECS: i64 = 60 * 60;

#[derive(Clone)]
pub struct CacheStore {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStatus {
    Running,
    Completed,
    Failed,
    Unknown(String),
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Unknown(v) => v.as_str(),
        }
    }
}

impl From<&str> for ExecutionStatus {
    fn from(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            other => Self::Unknown(other.to_string()),
        }
    }
}

impl std::fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl serde::Serialize for ExecutionStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedExecution {
    pub execution_id: String,
    pub session_id: String,
    pub backend: String,
    pub request_hash: String,
    pub status: ExecutionStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub fencing_token: i64,
}

impl CacheStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating cache store directory: {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening cache store: {}", path.display()))?;
        let store = Self { conn: Arc::new(Mutex::new(conn)) };
        store.init()?;
        Ok(store)
    }

    pub fn default_path() -> Result<PathBuf> {
        crate::config::paths::StorePaths::resolve()
            .map(|p| p.events_db())
    }

    pub fn begin_execution_with_id(
        &self,
        backend: &str,
        session_id: &str,
        request_hash: &str,
        execution_id: Option<&str>,
    ) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_ts();

        // Expire stale running executions
        let cutoff = now - RUNNING_EXECUTION_TTL_SECS;
        tx.execute(
            "UPDATE cache_executions SET status = 'failed', finished_at = ?1
             WHERE backend = ?2 AND request_hash = ?3 AND status = 'running' AND started_at < ?4",
            params![now, backend, request_hash, cutoff],
        )?;

        let eid = execution_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        // Deduplicate
        if let Some(existing_hash) = tx.query_row(
            "SELECT request_hash FROM cache_executions WHERE execution_id = ?1",
            params![eid],
            |row| row.get::<_, String>(0),
        ).optional()? {
            if existing_hash == request_hash {
                tx.commit()?;
                return Ok(eid);
            }
        }

        let fencing_token: i64 = tx.query_row(
            "SELECT COALESCE(MAX(fencing_token), 0) + 1 FROM cache_executions
             WHERE backend = ?1 AND request_hash = ?2",
            params![backend, request_hash],
            |row| row.get(0),
        )?;

        tx.execute(
            "INSERT INTO cache_executions
             (execution_id, session_id, backend, request_hash, status, started_at, fencing_token)
             VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6)",
            params![eid, session_id, backend, request_hash, now, fencing_token],
        )?;

        tx.commit()?;
        Ok(eid)
    }

    pub fn append_output(&self, execution_id: &str, event: &RuntimeEvent) -> Result<()> {
        // Only persist Output events for replay
        if let RuntimeEvent::Output(_) = event {
            let conn = self.conn.lock().unwrap();
            let json = serde_json::to_string(event)?;
            let seq: i64 = conn.query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM cache_outputs WHERE execution_id = ?1",
                params![execution_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO cache_outputs (execution_id, seq, event_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![execution_id, seq, json, now_ts()],
            )?;
        }
        Ok(())
    }

    pub fn finish_execution(&self, execution_id: &str, status: ExecutionStatus) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE cache_executions SET status = ?1, finished_at = ?2 WHERE execution_id = ?3",
            params![status.as_str(), now_ts(), execution_id],
        )?;
        Ok(())
    }

    pub fn find_completed_by_request_hash(
        &self,
        backend: &str,
        request_hash: &str,
    ) -> Result<Option<CachedExecution>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status,
                    started_at, finished_at, fencing_token
             FROM cache_executions
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'completed'
             ORDER BY fencing_token DESC LIMIT 1",
            params![backend, request_hash],
            |row| Ok(CachedExecution {
                execution_id: row.get(0)?,
                session_id: row.get(1)?,
                backend: row.get(2)?,
                request_hash: row.get(3)?,
                status: ExecutionStatus::from(row.get::<_, String>(4)?.as_str()),
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
                fencing_token: row.get(7)?,
            }),
        ).optional().map_err(Into::into)
    }

    pub fn find_running_by_request_hash(
        &self,
        backend: &str,
        request_hash: &str,
    ) -> Result<Option<CachedExecution>> {
        let conn = self.conn.lock().unwrap();
        let now = now_ts();
        let cutoff = now - RUNNING_EXECUTION_TTL_SECS;

        // Expire stale first
        conn.execute(
            "UPDATE cache_executions SET status = 'failed', finished_at = ?1
             WHERE backend = ?2 AND request_hash = ?3 AND status = 'running' AND started_at < ?4",
            params![now, backend, request_hash, cutoff],
        )?;

        conn.query_row(
            "SELECT execution_id, session_id, backend, request_hash, status,
                    started_at, finished_at, fencing_token
             FROM cache_executions
             WHERE backend = ?1 AND request_hash = ?2 AND status = 'running'
             ORDER BY fencing_token DESC LIMIT 1",
            params![backend, request_hash],
            |row| Ok(CachedExecution {
                execution_id: row.get(0)?,
                session_id: row.get(1)?,
                backend: row.get(2)?,
                request_hash: row.get(3)?,
                status: ExecutionStatus::from(row.get::<_, String>(4)?.as_str()),
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
                fencing_token: row.get(7)?,
            }),
        ).optional().map_err(Into::into)
    }

    pub fn output_text(&self, execution_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT event_json FROM cache_outputs
             WHERE execution_id = ?1 ORDER BY seq ASC"
        )?;
        let rows = stmt.query_map(params![execution_id], |row| {
            row.get::<_, String>(0)
        })?;

        let mut parts = Vec::new();
        for row in rows {
            let json_str = row?;
            if let Ok(RuntimeEvent::Output(OutputEvent { text, .. })) =
                serde_json::from_str::<RuntimeEvent>(&json_str)
            {
                parts.push(text);
            }
        }

        if parts.is_empty() {
            Ok(None)
        } else {
            Ok(Some(parts.join("")))
        }
    }

    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cache_executions (
                execution_id  TEXT PRIMARY KEY,
                session_id    TEXT NOT NULL,
                backend       TEXT NOT NULL,
                request_hash  TEXT NOT NULL,
                status        TEXT NOT NULL,
                started_at    INTEGER NOT NULL,
                finished_at   INTEGER,
                fencing_token INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS cache_outputs (
                execution_id TEXT NOT NULL,
                seq          INTEGER NOT NULL,
                event_json   TEXT NOT NULL,
                created_at   INTEGER NOT NULL,
                PRIMARY KEY (execution_id, seq)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_cache_running_lock
                ON cache_executions(backend, request_hash) WHERE status = 'running';"
        )?;

        // Purge records older than 30 days
        let cutoff = now_ts() - 30 * 24 * 60 * 60;
        conn.execute(
            "DELETE FROM cache_outputs WHERE execution_id IN (
                SELECT execution_id FROM cache_executions
                WHERE status != 'running' AND started_at < ?1
            )",
            params![cutoff],
        )?;
        conn.execute(
            "DELETE FROM cache_executions WHERE status != 'running' AND started_at < ?1",
            params![cutoff],
        )?;

        Ok(())
    }
}

pub fn request_hash(backend: &str, cwd: &Path, prompt: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(backend.as_bytes());
    hasher.update(b"\0");
    hasher.update(cwd.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(prompt.as_bytes());
    hex::encode(hasher.finalize())
}
```

- [ ] **Step 2: Update store/mod.rs**

In `src/store/mod.rs`, add the new module:

Add `pub mod cache;` to the module declarations.

Do NOT remove `pub mod events;` yet — that happens in Task 10.

- [ ] **Step 3: Verify CacheStore compiles**

Run:
```bash
cargo check 2>&1 | grep "error" | head -20
```
Expected: CacheStore-related code compiles. Other errors from removed crates are expected.

- [ ] **Step 4: Commit**

```bash
git add src/store/cache.rs src/store/mod.rs
git commit -m "feat: extract CacheStore from EventStore for execution replay/dedupe"
```

---

### Task 4: Create Telemetry Module — OTel Initialization

**Files:**
- Create: `src/telemetry.rs` (module root with submodules)
- Create: `src/telemetry/metrics.rs`
- Create: `src/telemetry/spans.rs`
- Create: `src/telemetry/logs.rs`
- Create: `src/telemetry/console.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create telemetry directory**

Run: `mkdir -p src/telemetry`

- [ ] **Step 2: Create src/telemetry/mod.rs (was src/telemetry.rs)**

Note: since we have submodules, this becomes `src/telemetry/mod.rs`.

Create `src/telemetry/mod.rs`:

```rust
pub mod console;
pub mod logs;
pub mod metrics;
pub mod spans;

use anyhow::Result;
use opentelemetry::global;
use opentelemetry_sdk::{
    logs::SdkLoggerProvider,
    metrics::SdkMeterProvider,
    trace::SdkTracerProvider,
    Resource,
};
use opentelemetry_sdk::trace::BatchSpanProcessor;
use opentelemetry_sdk::logs::BatchLogProcessor;
use opentelemetry_sdk::metrics::PeriodicReader;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter};
use opentelemetry::KeyValue;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Configuration for OTel export
pub struct TelemetryConfig {
    pub endpoint: String,
    pub enabled: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_string()),
            enabled: std::env::var("OTEL_ENABLED")
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
        }
    }
}

/// Guard that flushes and shuts down all OTel providers on drop
pub struct OtelGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Some(tp) = self.tracer_provider.take() {
            let _ = tp.shutdown();
        }
        if let Some(mp) = self.meter_provider.take() {
            let _ = mp.shutdown();
        }
        if let Some(lp) = self.logger_provider.take() {
            let _ = lp.shutdown();
        }
    }
}

fn build_resource() -> Resource {
    let version = env!("CARGO_PKG_VERSION");
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    Resource::builder()
        .with_attributes([
            KeyValue::new("service.name", "iota"),
            KeyValue::new("service.version", version),
            KeyValue::new("host.name", host),
        ])
        .build()
}

/// Initialize the full OTel telemetry stack.
///
/// If `config.enabled` is false or the OTLP endpoint is unreachable,
/// falls back to a no-op tracing subscriber with console output only.
pub fn init(config: &TelemetryConfig) -> Result<OtelGuard> {
    let resource = build_resource();

    if !config.enabled {
        // No-op: just set up a basic tracing subscriber for stderr
        let filter = logging_filter();
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_filter(filter))
            .try_init()
            .ok();

        return Ok(OtelGuard {
            tracer_provider: None,
            meter_provider: None,
            logger_provider: None,
        });
    }

    // --- Traces ---
    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();

    global::set_tracer_provider(tracer_provider.clone());

    // --- Metrics ---
    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;

    let metric_reader = PeriodicReader::builder(metric_exporter)
        .build();

    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_reader(metric_reader)
        .build();

    global::set_meter_provider(meter_provider.clone());

    // --- Logs ---
    let log_exporter = LogExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;

    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();

    // --- tracing-opentelemetry bridge ---
    let otel_trace_layer = tracing_opentelemetry::layer()
        .with_tracer(global::tracer("iota"));

    let otel_log_layer = opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(
        &logger_provider,
    );

    let filter = logging_filter();
    let stderr_layer = console::stderr_layer();

    tracing_subscriber::registry()
        .with(filter)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .with(stderr_layer)
        .try_init()
        .ok();

    Ok(OtelGuard {
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
        logger_provider: Some(logger_provider),
    })
}

fn logging_filter() -> EnvFilter {
    let env_val = std::env::var("IOTA_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "warn,iota_sympantos=info".to_string());
    EnvFilter::try_new(&env_val).unwrap_or_else(|_| EnvFilter::new("warn,iota_sympantos=info"))
}
```

- [ ] **Step 3: Create src/telemetry/console.rs**

```rust
use tracing_subscriber::fmt;

/// Create a stderr layer for realtime console output.
/// This replaces the old tracing-appender file logging.
/// When `--log-events` is active, events are rendered to stderr.
pub fn stderr_layer() -> fmt::Layer<tracing_subscriber::Registry> {
    fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(true)
        .with_level(true)
}
```

- [ ] **Step 4: Create src/telemetry/metrics.rs**

```rust
use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};
use std::sync::OnceLock;

pub struct IotaMetrics {
    pub execution_count: Counter<u64>,
    pub cache_hit_count: Counter<u64>,
    pub cache_miss_count: Counter<u64>,
    pub execution_active: UpDownCounter<i64>,
    pub session_active: UpDownCounter<i64>,
    pub prompt_queued: UpDownCounter<i64>,
    pub token_usage_count: Counter<u64>,
    pub token_input: Counter<u64>,
    pub token_output: Counter<u64>,
    pub token_total: Counter<u64>,
    pub prompt_duration: Histogram<f64>,
    pub init_duration: Histogram<f64>,
}

static METRICS: OnceLock<IotaMetrics> = OnceLock::new();

pub fn get() -> &'static IotaMetrics {
    METRICS.get_or_init(|| {
        let meter = global::meter("iota");

        let buckets = vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0];

        IotaMetrics {
            execution_count: meter
                .u64_counter("iota.execution.count")
                .with_unit("{execution}")
                .with_description("Total execution count by status")
                .build(),
            cache_hit_count: meter
                .u64_counter("iota.cache.hit.count")
                .with_unit("{hit}")
                .with_description("Cache hit count")
                .build(),
            cache_miss_count: meter
                .u64_counter("iota.cache.miss.count")
                .with_unit("{miss}")
                .with_description("Cache miss count")
                .build(),
            execution_active: meter
                .i64_up_down_counter("iota.execution.active")
                .with_unit("{execution}")
                .with_description("Currently running executions")
                .build(),
            session_active: meter
                .i64_up_down_counter("iota.session.active")
                .with_unit("{session}")
                .with_description("Active sessions")
                .build(),
            prompt_queued: meter
                .i64_up_down_counter("iota.prompt.queued")
                .with_unit("{prompt}")
                .with_description("Queued prompts")
                .build(),
            token_usage_count: meter
                .u64_counter("iota.token.usage.count")
                .with_unit("{event}")
                .with_description("Token usage event count")
                .build(),
            token_input: meter
                .u64_counter("iota.token.input")
                .with_unit("{token}")
                .with_description("Input tokens consumed")
                .build(),
            token_output: meter
                .u64_counter("iota.token.output")
                .with_unit("{token}")
                .with_description("Output tokens produced")
                .build(),
            token_total: meter
                .u64_counter("iota.token.total")
                .with_unit("{token}")
                .with_description("Total tokens")
                .build(),
            prompt_duration: meter
                .f64_histogram("iota.prompt.duration")
                .with_unit("s")
                .with_description("Prompt processing duration")
                .with_boundaries(buckets.clone())
                .build(),
            init_duration: meter
                .f64_histogram("iota.init.duration")
                .with_unit("s")
                .with_description("ACP initialization duration")
                .with_boundaries(buckets)
                .build(),
        }
    })
}
```

- [ ] **Step 5: Create src/telemetry/spans.rs**

```rust
use opentelemetry::{global, trace::{Span, SpanKind, Status, Tracer}, KeyValue};

/// Start the root execution span. Returns the span — caller must end it.
pub fn start_execution_span(
    execution_id: &str,
    session_id: &str,
    backend: &str,
    request_hash: &str,
) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    let mut span = tracer
        .span_builder("execution")
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("iota.execution.id", execution_id.to_string()),
            KeyValue::new("iota.session.id", session_id.to_string()),
            KeyValue::new("iota.backend", backend.to_string()),
            KeyValue::new("iota.request.hash", request_hash.to_string()),
        ])
        .start(&tracer);
    span
}

/// Start a child span for a named phase (process_spawn, init, session_new, prompt)
pub fn start_phase_span(name: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder(name.to_string())
        .with_kind(SpanKind::Internal)
        .start(&tracer)
}

/// Start a child span for a tool call
pub fn start_tool_span(tool_name: &str, tool_call_id: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder(format!("tool_call: {}", tool_name))
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("iota.tool.name", tool_name.to_string()),
            KeyValue::new("iota.tool.call_id", tool_call_id.to_string()),
        ])
        .start(&tracer)
}

/// Start a child span for a memory operation
pub fn start_memory_span(operation: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder(format!("memory.{}", operation))
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("iota.memory.operation", operation.to_string()),
        ])
        .start(&tracer)
}

/// Start a child span for an approval flow
pub fn start_approval_span(tool_name: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder("approval")
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("iota.tool.name", tool_name.to_string()),
        ])
        .start(&tracer)
}

/// Mark a span as completed successfully
pub fn end_span_ok(span: &mut impl Span) {
    span.set_status(Status::Ok);
    span.end();
}

/// Mark a span as failed with an error message
pub fn end_span_error(span: &mut impl Span, message: &str) {
    span.set_status(Status::error(message.to_string()));
    span.end();
}
```

- [ ] **Step 6: Create src/telemetry/logs.rs**

```rust
use crate::runtime_event::LogEvent;
use opentelemetry::KeyValue;

/// Convert a LogEvent's fields into OTel-compatible key-value attributes.
/// The tracing-opentelemetry bridge handles actual log emission;
/// this helper is for explicit structured log events that need
/// custom attributes beyond what tracing macros provide.
pub fn log_event_attributes(log: &LogEvent) -> Vec<KeyValue> {
    let mut attrs = Vec::new();

    if let Some(ref eid) = log.execution_id {
        attrs.push(KeyValue::new("iota.execution.id", eid.clone()));
    }
    if let Some(ref sid) = log.session_id {
        attrs.push(KeyValue::new("iota.session.id", sid.clone()));
    }
    if let Some(ref b) = log.backend {
        attrs.push(KeyValue::new("iota.backend", b.clone()));
    }
    if let Some(ref r) = log.route {
        attrs.push(KeyValue::new("iota.route", r.clone()));
    }
    if let Some(ref tn) = log.tool_name {
        attrs.push(KeyValue::new("iota.tool.name", tn.clone()));
    }
    if let Some(ref tcid) = log.tool_call_id {
        attrs.push(KeyValue::new("iota.tool.call_id", tcid.clone()));
    }
    if let Some(ok) = log.ok {
        attrs.push(KeyValue::new("iota.ok", ok));
    }
    if let Some(ms) = log.latency_ms {
        attrs.push(KeyValue::new("iota.latency_ms", ms as i64));
    }

    // Flatten JSON fields
    if let serde_json::Value::Object(map) = &log.fields {
        for (k, v) in map {
            let key = format!("iota.field.{}", k);
            match v {
                serde_json::Value::String(s) => attrs.push(KeyValue::new(key, s.clone())),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        attrs.push(KeyValue::new(key, i));
                    } else if let Some(f) = n.as_f64() {
                        attrs.push(KeyValue::new(key, f));
                    }
                }
                serde_json::Value::Bool(b) => attrs.push(KeyValue::new(key, *b)),
                other => attrs.push(KeyValue::new(key, other.to_string())),
            }
        }
    }

    attrs
}
```

- [ ] **Step 7: Update src/main.rs**

Add `mod telemetry;` to the module declarations in `src/main.rs` (after `mod store;`).

- [ ] **Step 8: Verify telemetry module compiles**

Run:
```bash
cargo check 2>&1 | grep "error" | head -20
```
Expected: telemetry module compiles. Other errors from old code are expected.

- [ ] **Step 9: Commit**

```bash
git add src/telemetry/ src/main.rs
git commit -m "feat: add OTel telemetry module with TracerProvider, MeterProvider, LoggerProvider init"
```

---

### Task 5: Replace CLI Init and Remove Observability Commands

**Files:**
- Modify: `src/cli/mod.rs`

This is the largest single-file change. We replace `init_logging()` with `telemetry::init()`, remove the entire `iota observability` command tree, remove Prometheus exposition code, and add `iota logs` / `iota trace` commands.

- [ ] **Step 1: Replace imports**

In `src/cli/mod.rs`, replace the old telemetry imports:

Remove:
```rust
use prometheus::{Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder};
use std::sync::OnceLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};
```

Add:
```rust
use crate::telemetry::{self, TelemetryConfig, OtelGuard};
```

Remove the `LOG_GUARD` static:
```rust
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
```

- [ ] **Step 2: Replace init_logging with telemetry::init in run()**

In the `run()` function, replace the call to `init_logging();` with:

```rust
let _otel_guard = telemetry::init(&TelemetryConfig::default())?;
```

The `_otel_guard` must live for the duration of `run()` to ensure proper shutdown.

- [ ] **Step 3: Remove init_logging function and helpers**

Delete these functions entirely:
- `fn init_logging()` (line 394 onwards)
- `fn logging_filter() -> EnvFilter` (line 434 onwards)
- `fn log_dir() -> Result<std::path::PathBuf>` (line 441 onwards)
- `fn env_flag(name: &str) -> Option<bool>` (line 449 onwards)

- [ ] **Step 4: Remove observability command routing**

In the `run()` command dispatch, remove the `"observability" | "obs"` match arm:
```rust
"observability" | "obs" => run_observability_command(&args[1..])?,
```

- [ ] **Step 5: Remove all observability functions**

Delete these functions entirely:
- `fn run_observability_command(args: &[String]) -> Result<()>` (line 458)
- `fn print_observability_help()` (line 498)
- `fn run_obs_logging(args: &[String], store: &EventStore) -> Result<()>` (line 506)
- `fn run_obs_timing(args: &[String], store: &EventStore) -> Result<()>` (line 811)
- `fn run_obs_metrics(args: &[String], store: &EventStore) -> Result<()>` (line 926)
- `fn print_prometheus_metrics(store: &EventStore) -> Result<()>` (line 1055)
- All parser helpers: `parse_limit`, `parse_scan`, `default_scan_limit`, `parse_log_event_filter`, `parse_tool_filter`, `parse_tool_event_mode` (lines 1189–1230)
- Types: `enum ToolEventMode`, `enum ToolEventEntry`, `struct LogEntry` (lines 608–657)
- Helpers: `collect_log_entries`, `collect_tool_event_entries` (lines 659–810)

- [ ] **Step 6: Remove print_log_events and related rendering functions**

Delete:
- `fn print_log_events(events: &[RuntimeEvent])` (line 150)
- `fn memory_tool_call_log(...) -> Option<LogEvent>` (line 187)
- `fn memory_tool_result_log(...) -> Option<LogEvent>` (line 228)
- `fn memory_event_log(...) -> LogEvent` (line 258)
- `fn render_log_event_text(...) -> String` (line 268)

- [ ] **Step 7: Add iota logs command**

Add to the `run()` command dispatch:
```rust
"logs" => run_logs_command(&args[1..]).await?,
```

Add the implementation:
```rust
async fn run_logs_command(args: &[String]) -> Result<()> {
    let execution_id = args.first()
        .ok_or_else(|| anyhow::anyhow!("Usage: iota logs <execution_id>"))?;

    let loki_url = std::env::var("IOTA_LOKI_URL")
        .unwrap_or_else(|_| "http://localhost:3100".to_string());

    let query = format!(
        r#"{{iota_execution_id="{}"}}"#,
        execution_id
    );
    let url = format!(
        "{}/loki/api/v1/query_range?query={}&limit=1000",
        loki_url,
        urlencoding::encode(&query)
    );

    let client = reqwest::Client::new();
    let resp = client.get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Loki at {}", loki_url))?;

    if !resp.status().is_success() {
        bail!("Loki query failed with status {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;

    // Parse Loki response and print log lines
    if let Some(results) = body["data"]["result"].as_array() {
        for stream in results {
            if let Some(values) = stream["values"].as_array() {
                for entry in values {
                    if let Some(arr) = entry.as_array() {
                        if arr.len() >= 2 {
                            // arr[0] = timestamp nanoseconds, arr[1] = log line
                            if let Some(line) = arr[1].as_str() {
                                println!("{}", line);
                            }
                        }
                    }
                }
            }
        }
    } else {
        println!("No logs found for execution {}", execution_id);
    }

    Ok(())
}
```

- [ ] **Step 8: Add iota trace command**

Add to the `run()` command dispatch:
```rust
"trace" => run_trace_command(&args[1..]).await?,
```

Add the implementation:
```rust
async fn run_trace_command(args: &[String]) -> Result<()> {
    let trace_id = args.first()
        .ok_or_else(|| anyhow::anyhow!("Usage: iota trace <trace_id>"))?;

    let jaeger_url = std::env::var("IOTA_JAEGER_URL")
        .unwrap_or_else(|_| "http://localhost:16686".to_string());

    let url = format!("{}/api/traces/{}", jaeger_url, trace_id);

    let client = reqwest::Client::new();
    let resp = client.get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Jaeger at {}", jaeger_url))?;

    if !resp.status().is_success() {
        bail!("Jaeger query failed with status {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;

    // Render a simplified span waterfall
    if let Some(traces) = body["data"].as_array() {
        for trace in traces {
            if let Some(spans) = trace["spans"].as_array() {
                for span in spans {
                    let name = span["operationName"].as_str().unwrap_or("?");
                    let duration_us = span["duration"].as_u64().unwrap_or(0);
                    let duration_ms = duration_us / 1000;

                    // Determine depth from references
                    let depth = if span["references"].as_array()
                        .map(|r| r.is_empty())
                        .unwrap_or(true)
                    {
                        0
                    } else {
                        1 // simplified; a full implementation would compute tree depth
                    };

                    let indent = "  ".repeat(depth);
                    println!("{}├── {} ({}ms)", indent, name, duration_ms);
                }
            }
        }
    } else {
        println!("No trace found for {}", trace_id);
    }

    Ok(())
}
```

- [ ] **Step 9: Add urlencoding dependency**

In `Cargo.toml`, add:
```toml
urlencoding = "2.1"
```

- [ ] **Step 10: Remove EventStore import from cli/mod.rs**

Remove:
```rust
use crate::store::events::EventStore;
```

Replace with (if not already present):
```rust
use crate::store::cache::CacheStore;
```

Note: The CLI no longer directly uses CacheStore — that's only used in engine. But we remove the EventStore import.

- [ ] **Step 11: Verify compilation**

Run:
```bash
cargo check 2>&1 | grep "error" | head -30
```
Expected: cli/mod.rs compiles. Errors in engine.rs are expected (still uses EventStore).

- [ ] **Step 12: Commit**

```bash
git add src/cli/mod.rs Cargo.toml
git commit -m "feat: replace init_logging with OTel init, remove observability CLI, add iota logs/trace commands"
```

---

### Task 6: Replace Engine Instrumentation

**Files:**
- Modify: `src/engine.rs`

This is the most complex change — replacing all EventStore usage in the engine with CacheStore + OTel spans/metrics/logs.

- [ ] **Step 1: Replace EventStore imports with CacheStore + OTel**

In `src/engine.rs`, replace:
```rust
use crate::store::events::{EventStore, ExecutionStatus};
```

With:
```rust
use crate::store::cache::{CacheStore, ExecutionStatus, request_hash};
use crate::telemetry::{metrics, spans};
use opentelemetry::trace::Span;
```

- [ ] **Step 2: Replace EventStore field in IotaEngine**

Replace:
```rust
event_store: Option<EventStore>,
```

With:
```rust
cache_store: Option<CacheStore>,
```

- [ ] **Step 3: Update constructor initialization**

In `create_session`, replace EventStore opening:
```rust
let event_store = EventStore::default_path()
    .ok()
    .and_then(|path| EventStore::open(&path).ok());
```

With:
```rust
let cache_store = CacheStore::default_path()
    .ok()
    .and_then(|path| CacheStore::open(&path).ok());
```

- [ ] **Step 4: Update prompt_in_cwd_timed_with_execution_id — cache/replay path**

Replace the `EventStore::request_hash` call:
```rust
let request_hash = EventStore::request_hash(backend.name(), &cwd, prompt);
```

With:
```rust
let request_hash = request_hash(backend.name(), &cwd, prompt);
```

Replace replay logic that calls `self.try_replay_completed(backend, &request_hash)` and `self.try_join_running(backend, &request_hash, ...)`:
- These methods internally use EventStore — update them to use `self.cache_store`.

- [ ] **Step 5: Update execution creation**

Replace EventStore begin_execution_with_id call with CacheStore equivalent. The call site around lines 253-276 should use `self.cache_store` instead of `self.event_store`.

- [ ] **Step 6: Add OTel span creation at execution start**

After creating the execution ID, add:
```rust
let mut root_span = spans::start_execution_span(
    &execution_id,
    &self.session_id,
    backend.name(),
    &request_hash,
);
metrics::get().execution_active.add(1, &[]);

tracing::info!(
    execution_id = %execution_id,
    backend = %backend,
    session_id = %self.session_id,
    request_hash = %request_hash,
    "execution.started"
);
```

- [ ] **Step 7: Replace record_event to use CacheStore + OTel**

Replace the `record_event` method:
```rust
fn record_event(&self, execution_id: &Option<String>, event: RuntimeEvent) {
    // Store output events for replay cache
    if let (Some(eid), RuntimeEvent::Output(_)) = (execution_id.as_ref(), &event) {
        if let Some(store) = &self.cache_store {
            let _ = store.append_output(eid, &event);
        }
    }
    // Token usage events feed OTel metrics
    if let RuntimeEvent::TokenUsage(ref tu) = event {
        let m = metrics::get();
        m.token_usage_count.add(1, &[]);
        if let Some(input) = tu.input_tokens {
            m.token_input.add(input, &[]);
        }
        if let Some(output) = tu.output_tokens {
            m.token_output.add(output, &[]);
        }
        if let Some(total) = tu.total_tokens {
            m.token_total.add(total, &[]);
        }
        tracing::info!(
            input_tokens = tu.input_tokens,
            output_tokens = tu.output_tokens,
            total_tokens = tu.total_tokens,
            "token.usage"
        );
    }
}
```

- [ ] **Step 8: Replace record_log_event to use tracing**

Replace:
```rust
fn record_log_event(
    &self,
    execution_id: Option<&str>,
    backend: AcpBackend,
    level: &str,
    event: &str,
    fields: serde_json::Value,
) {
    // Now just emit via tracing — the OTel bridge picks it up
    match level {
        "error" => tracing::error!(
            execution_id = execution_id,
            backend = %backend,
            event = event,
            fields = %fields,
            "{}", event
        ),
        "warn" => tracing::warn!(
            execution_id = execution_id,
            backend = %backend,
            event = event,
            fields = %fields,
            "{}", event
        ),
        _ => tracing::info!(
            execution_id = execution_id,
            backend = %backend,
            event = event,
            fields = %fields,
            "{}", event
        ),
    }
}
```

- [ ] **Step 9: Replace cache hit/miss recording with OTel metrics**

Replace:
```rust
fn record_cache_hit(&self) {
    if let Some(store) = &self.event_store {
        let _ = store.record_cache_hit();
    }
}

fn record_cache_miss(&self) {
    if let Some(store) = &self.event_store {
        let _ = store.record_cache_miss();
    }
}
```

With:
```rust
fn record_cache_hit(&self) {
    metrics::get().cache_hit_count.add(1, &[]);
    tracing::info!("cache.hit");
}

fn record_cache_miss(&self) {
    metrics::get().cache_miss_count.add(1, &[]);
    tracing::debug!("cache.miss");
}
```

- [ ] **Step 10: Replace record_active_sessions with OTel metric**

Replace:
```rust
fn record_active_sessions(&self) {
    if let Some(store) = &self.event_store {
        let _ = store.set_active_sessions(self.clients.len() as u64);
    }
}
```

With:
```rust
fn record_active_sessions(&self) {
    // UpDownCounter tracks the delta, so we set via add/subtract
    // For simplicity, we just log the current count
    let count = self.clients.len() as i64;
    metrics::get().session_active.add(count, &[]);
}
```

Note: UpDownCounter semantics require tracking the previous value to emit a delta. For simplicity in this first pass, consider tracking via a field. A more precise implementation can be done in a follow-up.

- [ ] **Step 11: Replace finish_execution and finish_execution_with_timing**

Replace:
```rust
fn finish_execution(&self, execution_id: &Option<String>, status: ExecutionStatus) {
    if let (Some(store), Some(eid)) = (&self.event_store, execution_id) {
        let _ = store.finish_execution(eid, status);
    }
}
```

With:
```rust
fn finish_execution(&self, execution_id: &Option<String>, status: ExecutionStatus) {
    if let (Some(store), Some(eid)) = (&self.cache_store, execution_id) {
        let _ = store.finish_execution(eid, status.clone());
    }
    let m = metrics::get();
    m.execution_active.add(-1, &[]);
    let status_attr = opentelemetry::KeyValue::new("status", status.as_str().to_string());
    m.execution_count.add(1, &[status_attr]);
}
```

Replace `finish_execution_with_timing`:
```rust
fn finish_execution_with_timing(
    &self,
    execution_id: &Option<String>,
    status: ExecutionStatus,
    timing: &acp::AcpPromptTiming,
) {
    self.finish_execution(execution_id, status.clone());

    let m = metrics::get();
    let backend_attr = self.active_backend
        .as_ref()
        .map(|b| opentelemetry::KeyValue::new("backend", b.name().to_string()))
        .unwrap_or_else(|| opentelemetry::KeyValue::new("backend", "unknown"));

    m.prompt_duration.record(timing.prompt_ms as f64 / 1000.0, &[backend_attr.clone()]);
    if let Some(init_ms) = timing.init_ms {
        m.init_duration.record(init_ms as f64 / 1000.0, &[backend_attr]);
    }

    // Log execution completion
    match status {
        ExecutionStatus::Completed => tracing::info!(
            execution_id = execution_id.as_deref(),
            total_ms = timing.total_ms,
            prompt_ms = timing.prompt_ms,
            status = "completed",
            "execution.completed"
        ),
        ExecutionStatus::Failed => tracing::error!(
            execution_id = execution_id.as_deref(),
            total_ms = timing.total_ms,
            status = "failed",
            "execution.failed"
        ),
        _ => {}
    }
}
```

- [ ] **Step 12: Update try_replay_completed and try_join_running**

These methods use EventStore — update to use CacheStore:

```rust
fn try_replay_completed(&self, backend: AcpBackend, request_hash: &str) -> Option<String> {
    let store = self.cache_store.as_ref()?;
    let record = store.find_completed_by_request_hash(backend.name(), request_hash).ok()??;
    store.output_text(&record.execution_id).ok()?
}
```

For `try_join_running`, update similarly to use `self.cache_store` and `CacheStore::find_running_by_request_hash`.

- [ ] **Step 13: Add stderr trace/logs URL output after execution**

At the end of successful execution (around line 576), add:
```rust
// Print trace/logs URLs to stderr
if let Some(ref eid) = execution_id {
    eprintln!("trace:  http://localhost:16686/trace/{}", eid);
    eprintln!(
        "logs:   http://localhost:3000/explore?left={{\"queries\":[{{\"expr\":\"{{iota_execution_id=\\\"{}\\\"}}\"}}]}}",
        eid
    );
}
```

- [ ] **Step 14: Verify engine compiles**

Run:
```bash
cargo check 2>&1 | grep "error" | head -30
```
Expected: engine.rs compiles with CacheStore + OTel.

- [ ] **Step 15: Commit**

```bash
git add src/engine.rs
git commit -m "feat: replace EventStore with CacheStore + OTel spans/metrics in engine"
```

---

### Task 7: Add OTel Spans to ACP Module

**Files:**
- Modify: `src/acp/mod.rs`
- Modify: `src/acp/permission.rs`

- [ ] **Step 1: Add log gap fill to AcpClient::start**

In `src/acp/mod.rs`, in the `start()` method, after the process is spawned and before init completes, add:

```rust
tracing::info!(
    backend = %backend,
    process_spawn_ms = startup_timing.process_spawn_ms,
    "acp.process.spawn"
);
tracing::info!(
    backend = %backend,
    init_ms = startup_timing.init_ms,
    "acp.init.completed"
);
```

- [ ] **Step 2: Add log gap fill to session creation**

In `ensure_session_timed`, after session is created:

```rust
tracing::info!(
    session_id = %session_id,
    session_new_ms = elapsed_ms,
    "acp.session.created"
);
```

- [ ] **Step 3: Add log gap fill to prompt lifecycle**

In `execute`:

At prompt send:
```rust
tracing::info!(
    execution_id = execution_id,
    backend = %self.backend,
    "prompt.sent"
);
```

At prompt complete:
```rust
tracing::info!(
    execution_id = execution_id,
    prompt_ms = timing.prompt_ms,
    "prompt.completed"
);
```

- [ ] **Step 4: Add log gap fill to tool calls in read_prompt_events_for_id**

Around the tool call interception (lines 351-376 where ToolCall and ToolResult events are emitted):

Before intercepted call:
```rust
tracing::info!(
    tool_name = %tool_name,
    tool_call_id = %call_id,
    execution_id = execution_id,
    "tool.call.started"
);
```

After result:
```rust
tracing::info!(
    tool_name = %tool_name,
    tool_call_id = %call_id,
    ok = ok,
    latency_ms = elapsed_ms,
    "tool.call.completed"
);
```

On error:
```rust
tracing::error!(
    tool_name = %tool_name,
    tool_call_id = %call_id,
    error = %err,
    "tool.call.failed"
);
```

- [ ] **Step 5: Add log gap fill to process exit**

In `shutdown()`:
```rust
tracing::info!(
    backend = %self.backend,
    "acp.process.exit"
);
```

- [ ] **Step 6: Add approval spans in permission.rs**

In `src/acp/permission.rs`, in `answer_permission_request`:

At request start:
```rust
tracing::info!(
    tool_name = %tool_name,
    execution_id = execution_id,
    "approval.requested"
);
```

At decision:
```rust
tracing::info!(
    tool_name = %tool_name,
    decision = %if approved { "approved" } else { "denied" },
    "approval.decided"
);
```

- [ ] **Step 7: Verify compilation**

Run:
```bash
cargo check 2>&1 | grep "error" | head -20
```

- [ ] **Step 8: Commit**

```bash
git add src/acp/mod.rs src/acp/permission.rs
git commit -m "feat: add OTel log gap fill for ACP process, prompt, tool calls, and approvals"
```

---

### Task 8: Update Context Server and MCP Router Logging

**Files:**
- Modify: `src/context/server.rs`

- [ ] **Step 1: Replace emit_route_log with tracing**

In `src/context/server.rs`, replace the `emit_route_log` function:

```rust
fn emit_route_log(level: &str, event: &str, fields: serde_json::Value) {
    match level {
        "error" => tracing::error!(
            route = "mcp-sidecar",
            event = event,
            fields = %fields,
            "{}", event
        ),
        "warn" => tracing::warn!(
            route = "mcp-sidecar",
            event = event,
            fields = %fields,
            "{}", event
        ),
        _ => tracing::info!(
            route = "mcp-sidecar",
            event = event,
            fields = %fields,
            "{}", event
        ),
    }
}
```

Note: `context/server.rs` runs as a subprocess (`run_stdio()`), so it has its own tracing subscriber. The OTel bridge in the main process won't affect this subprocess. For now, these logs go to stderr with `[iota log]` prefix as before. A future enhancement could add OTel SDK init to the subprocess.

- [ ] **Step 2: Add MCP route request/response logs**

In `handle_request`, add at entry:
```rust
tracing::info!(
    route = "mcp-sidecar",
    method = %method,
    "mcp.route.request"
);
```

And at successful response:
```rust
tracing::info!(
    route = "mcp-sidecar",
    status = "ok",
    "mcp.route.response"
);
```

On error:
```rust
tracing::error!(
    route = "mcp-sidecar",
    error = %err,
    "mcp.route.error"
);
```

- [ ] **Step 3: Remove LogEvent import if no longer needed**

Check if `crate::runtime_event::LogEvent` is still used in this file after the `emit_route_log` change. If not, remove the import.

- [ ] **Step 4: Verify compilation**

Run:
```bash
cargo check 2>&1 | grep "error" | head -20
```

- [ ] **Step 5: Commit**

```bash
git add src/context/server.rs
git commit -m "feat: replace context server route logging with tracing macros"
```

---

### Task 9: Update TUI — Remove EventStore Dependency

**Files:**
- Modify: `src/tui.rs`

- [ ] **Step 1: Remove EventStore import and field**

Remove:
```rust
use crate::store::events::EventStore;
```

Remove the `event_store` field from `TuiApp`:
```rust
event_store: Option<EventStore>,
```

Remove the initialization lines:
```rust
event_store: EventStore::default_path()
    .ok()
    .and_then(|path| EventStore::open(&path).ok()),
```

- [ ] **Step 2: Replace record_queued_prompts**

Replace:
```rust
fn record_queued_prompts(&self) {
    if let Some(store) = &self.event_store {
        let _ = store.set_queued_prompts(u64::from(self.queued_prompt.is_some()));
    }
}
```

With:
```rust
fn record_queued_prompts(&self) {
    use crate::telemetry::metrics;
    let value = if self.queued_prompt.is_some() { 1 } else { 0 };
    metrics::get().prompt_queued.add(value, &[]);
}
```

- [ ] **Step 3: Verify ObservabilityMeta is still populated**

Confirm that `observability_from_output` (line 645) derives data from `AcpPromptOutput` directly — it does NOT use EventStore. This function should continue working unchanged.

- [ ] **Step 4: Verify compilation**

Run:
```bash
cargo check 2>&1 | grep "error" | head -20
```

- [ ] **Step 5: Commit**

```bash
git add src/tui.rs
git commit -m "feat: remove EventStore dependency from TUI, use OTel metrics for queued prompts"
```

---

### Task 10: Delete EventStore and Clean Up

**Files:**
- Delete: `src/store/events.rs`
- Delete: `src/store/events_tests.rs`
- Modify: `src/store/mod.rs`

- [ ] **Step 1: Delete EventStore files**

```bash
rm src/store/events.rs src/store/events_tests.rs
```

- [ ] **Step 2: Remove events module from store/mod.rs**

In `src/store/mod.rs`, remove:
```rust
pub mod events;
```

Update the module doc comment to reflect that `events` is replaced by `cache`.

- [ ] **Step 3: Search for remaining EventStore references**

Run:
```bash
cargo check 2>&1 | grep "EventStore\|event_store" | head -20
```

Fix any remaining references. Common locations to check:
- `src/engine_tests.rs` — may need updating
- Any test files that import EventStore

- [ ] **Step 4: Full compilation check**

Run:
```bash
cargo check 2>&1
```
Expected: Clean compilation with no errors.

- [ ] **Step 5: Run existing tests**

Run:
```bash
cargo test 2>&1 | tail -30
```

Fix any test failures. Tests in `events_tests.rs` are deleted. Tests in `engine_tests.rs` may need updating to use CacheStore.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: delete EventStore, complete migration to CacheStore + OTel"
```

---

### Task 11: Add Log Gap Fill — Engine and Runtime Events

**Files:**
- Modify: `src/engine.rs`

This task adds the remaining log statements from the spec section 8 that weren't covered in Tasks 6 and 7.

- [ ] **Step 1: Add output logging in engine**

In the output recording path (where RuntimeEvent::Output events are processed), add:

For stream chunks:
```rust
tracing::debug!(
    execution_id = execution_id.as_deref(),
    chunk_len = text.len(),
    "output.chunk"
);
```

For final output:
```rust
tracing::info!(
    execution_id = execution_id.as_deref(),
    output_len = output.text.len(),
    "output.final"
);
```

- [ ] **Step 2: Add runtime error logging**

In the error handling path (around line 597-606), ensure:
```rust
tracing::error!(
    execution_id = execution_id.as_deref(),
    error = %err,
    source = "engine",
    "runtime.error"
);
```

- [ ] **Step 3: Verify compilation and run tests**

Run:
```bash
cargo check && cargo test 2>&1 | tail -20
```

- [ ] **Step 4: Commit**

```bash
git add src/engine.rs
git commit -m "feat: add remaining log gap fill for output and runtime errors"
```

---

### Task 12: Integration Verification

**Files:** No new files — this is a verification task.

- [ ] **Step 1: Start the Docker observability stack**

```bash
cd docker/observability && docker compose up -d
```

- [ ] **Step 2: Build and run iota with a test prompt**

```bash
cargo build && cargo run -- run --backend codex "echo hello"
```

Expected: Execution completes. stderr shows trace/logs URLs.

- [ ] **Step 3: Verify traces in Jaeger**

Open `http://localhost:16686` and search for service `iota`. Verify:
- Root `execution` span exists
- Child spans for phases (prompt, etc.) exist
- Span attributes include `iota.execution.id`, `iota.backend`

- [ ] **Step 4: Verify metrics in Prometheus**

Open `http://localhost:9090` and query:
```
iota_execution_count_total
```
Expected: Counter value > 0.

- [ ] **Step 5: Verify logs in Grafana/Loki**

Open `http://localhost:3000`, go to Explore, select Loki datasource, query:
```
{service_name="iota"}
```
Expected: Log entries from the execution appear.

- [ ] **Step 6: Test iota logs command**

```bash
cargo run -- logs <execution_id_from_step_2>
```
Expected: stdout prints log lines for that execution.

- [ ] **Step 7: Test iota trace command**

```bash
cargo run -- trace <trace_id_from_step_2>
```
Expected: stdout prints span waterfall.

- [ ] **Step 8: Tear down stack**

```bash
cd docker/observability && docker compose down
```

- [ ] **Step 9: Verify graceful degradation without stack**

```bash
cargo run -- run --backend codex "echo hello"
```
Expected: Execution completes normally. OTel export silently fails. No crashes.

- [ ] **Step 10: Final commit**

```bash
git add -A
git commit -m "chore: integration verification complete for OTel alignment"
```

---

## Summary of Commits

| Task | Commit Message |
|---|---|
| 1 | feat: add Docker Compose observability stack |
| 2 | chore: update deps - remove prometheus/tracing-appender, add opentelemetry stack |
| 3 | feat: extract CacheStore from EventStore for execution replay/dedupe |
| 4 | feat: add OTel telemetry module with TracerProvider, MeterProvider, LoggerProvider init |
| 5 | feat: replace init_logging with OTel init, remove observability CLI, add iota logs/trace |
| 6 | feat: replace EventStore with CacheStore + OTel spans/metrics in engine |
| 7 | feat: add OTel log gap fill for ACP process, prompt, tool calls, and approvals |
| 8 | feat: replace context server route logging with tracing macros |
| 9 | feat: remove EventStore dependency from TUI, use OTel metrics for queued prompts |
| 10 | feat: delete EventStore, complete migration to CacheStore + OTel |
| 11 | feat: add remaining log gap fill for output and runtime errors |
| 12 | chore: integration verification complete for OTel alignment |
