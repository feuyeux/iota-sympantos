pub mod console;
pub mod logs;
pub mod metrics;
pub mod spans;

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Local, NaiveDate};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider,
};
use std::fs;
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter, Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt,
};

type BoxedTelemetryLayer = Box<dyn Layer<Registry> + Send + Sync + 'static>;

pub struct TelemetryConfig {
    pub endpoint: String,
    pub enabled: bool,
    pub file_log: FileLogConfig,
}

pub struct FileLogConfig {
    pub mode: FileLogMode,
    pub dir: PathBuf,
    pub retention: LogRetention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLogMode {
    Off,
    Auto,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogRetention {
    Disabled,
    Days(u64),
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_string()),
            enabled: std::env::var("OTEL_ENABLED")
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
            file_log: FileLogConfig::default(),
        }
    }
}

impl Default for FileLogConfig {
    fn default() -> Self {
        Self {
            mode: FileLogMode::from_env_value(std::env::var("IOTA_LOG_FILE").ok().as_deref()),
            dir: std::env::var("IOTA_LOG_DIR")
                .map(|value| expand_log_dir(&value))
                .unwrap_or_else(|_| default_log_dir()),
            retention: LogRetention::from_env_value(
                std::env::var("IOTA_LOG_RETENTION_DAYS").ok().as_deref(),
            ),
        }
    }
}

impl FileLogMode {
    fn from_env_value(value: Option<&str>) -> Self {
        match value.map(|value| value.trim().to_ascii_lowercase()) {
            None => Self::Auto,
            Some(value) if value.is_empty() || matches!(value.as_str(), "auto") => Self::Auto,
            Some(value) if matches!(value.as_str(), "off" | "false" | "0" | "none") => Self::Off,
            Some(value) if matches!(value.as_str(), "always" | "true" | "1" | "on") => Self::Always,
            Some(_) => Self::Auto,
        }
    }
}

impl LogRetention {
    fn from_env_value(value: Option<&str>) -> Self {
        match value.map(|value| value.trim().to_ascii_lowercase()) {
            None => Self::Days(30),
            Some(value) if value.is_empty() => Self::Days(30),
            Some(value) if matches!(value.as_str(), "off" | "false" | "0" | "none") => {
                Self::Disabled
            }
            Some(value) => value
                .parse::<u64>()
                .map(Self::Days)
                .unwrap_or(Self::Days(30)),
        }
    }

    fn days(self) -> Option<u64> {
        match self {
            Self::Disabled => None,
            Self::Days(days) => Some(days),
        }
    }
}

fn default_log_dir() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".i6").join("logs"))
        .unwrap_or_else(|| PathBuf::from(".i6").join("logs"))
}

fn expand_log_dir(value: &str) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(value)
}

pub struct OtelGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
    _file_log_guard: Option<WorkerGuard>,
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

pub fn init(config: &TelemetryConfig) -> Result<OtelGuard> {
    match init_inner(config) {
        Ok(guard) => Ok(guard),
        Err(err) => {
            eprintln!(
                "[iota telemetry] OpenTelemetry disabled for this process: {err}. Falling back to stderr tracing."
            );
            let file_log_guard = install_stderr_tracing(&config.file_log);
            Ok(OtelGuard {
                tracer_provider: None,
                meter_provider: None,
                logger_provider: None,
                _file_log_guard: file_log_guard,
            })
        }
    }
}

fn init_inner(config: &TelemetryConfig) -> Result<OtelGuard> {
    let resource = build_resource();

    if !config.enabled {
        let file_log_guard = install_stderr_tracing(&config.file_log);
        return Ok(OtelGuard {
            tracer_provider: None,
            meter_provider: None,
            logger_provider: None,
            _file_log_guard: file_log_guard,
        });
    }

    // Traces
    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;

    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;

    let log_exporter = LogExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();
    global::set_tracer_provider(tracer_provider.clone());

    // Metrics
    let metric_reader =
        opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter).build();
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_reader(metric_reader)
        .build();
    global::set_meter_provider(meter_provider.clone());

    // Logs
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();

    // tracing-opentelemetry bridge
    let otel_trace_layer =
        tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer("iota"));
    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider);
    let filter = logging_filter();
    let stderr_layer = console::stderr_layer();
    let file_log = prepare_file_log_layer(&config.file_log);
    let file_layer = file_log.layer;

    tracing_subscriber::registry()
        .with(file_layer)
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
        _file_log_guard: file_log.guard,
    })
}

fn install_stderr_tracing(file_config: &FileLogConfig) -> Option<WorkerGuard> {
    let filter = logging_filter();
    let file_log = prepare_file_log_layer(file_config);
    tracing_subscriber::registry()
        .with(file_log.layer)
        .with(filter)
        .with(console::stderr_layer())
        .try_init()
        .ok();
    file_log.guard
}

struct PreparedFileLogLayer {
    layer: Option<BoxedTelemetryLayer>,
    guard: Option<WorkerGuard>,
}

struct FileLogLayer {
    layer: BoxedTelemetryLayer,
    guard: WorkerGuard,
}

fn prepare_file_log_layer(config: &FileLogConfig) -> PreparedFileLogLayer {
    match build_file_log_layer(config) {
        Ok(Some(file_log)) => PreparedFileLogLayer {
            layer: Some(file_log.layer),
            guard: Some(file_log.guard),
        },
        Ok(None) => PreparedFileLogLayer {
            layer: None,
            guard: None,
        },
        Err(err) => {
            eprintln!("[iota telemetry] Failed to initialize file logging: {err}");
            PreparedFileLogLayer {
                layer: None,
                guard: None,
            }
        }
    }
}

fn build_file_log_layer(config: &FileLogConfig) -> Result<Option<FileLogLayer>> {
    if config.mode == FileLogMode::Off {
        return Ok(None);
    }

    fs::create_dir_all(&config.dir)?;
    cleanup_old_log_files(&config.dir, config.retention)?;
    let file_appender = tracing_appender::rolling::daily(&config.dir, "iota.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    let layer: BoxedTelemetryLayer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .boxed();

    Ok(Some(FileLogLayer { layer, guard }))
}

fn cleanup_old_log_files(dir: &std::path::Path, retention: LogRetention) -> Result<usize> {
    let Some(days) = retention.days() else {
        return Ok(0);
    };
    let today = Local::now().date_naive();
    let mut deleted = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if should_delete_log_file(&file_name, today, days) {
            fs::remove_file(entry.path())?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

fn should_delete_log_file(file_name: &str, today: NaiveDate, retention_days: u64) -> bool {
    let Some(date_part) = file_name.strip_prefix("iota.log.") else {
        return false;
    };
    let Ok(file_date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") else {
        return false;
    };
    let cutoff = today - ChronoDuration::days(retention_days as i64);
    file_date < cutoff
}

fn logging_filter() -> EnvFilter {
    let env_val = std::env::var("IOTA_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "warn,iota=info,iota_sympantos=info".to_string());
    EnvFilter::try_new(&env_val)
        .unwrap_or_else(|_| EnvFilter::new("warn,iota=info,iota_sympantos=info"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_log_mode_defaults_to_auto() {
        assert_eq!(FileLogMode::from_env_value(None), FileLogMode::Auto);
        assert_eq!(FileLogMode::from_env_value(Some("")), FileLogMode::Auto);
        assert_eq!(FileLogMode::from_env_value(Some("auto")), FileLogMode::Auto);
        assert_eq!(
            FileLogMode::from_env_value(Some("bogus")),
            FileLogMode::Auto
        );
    }

    #[test]
    fn file_log_mode_parses_disable_values() {
        assert_eq!(FileLogMode::from_env_value(Some("off")), FileLogMode::Off);
        assert_eq!(FileLogMode::from_env_value(Some("false")), FileLogMode::Off);
        assert_eq!(FileLogMode::from_env_value(Some("0")), FileLogMode::Off);
        assert_eq!(FileLogMode::from_env_value(Some("none")), FileLogMode::Off);
    }

    #[test]
    fn file_log_mode_parses_always_values() {
        assert_eq!(
            FileLogMode::from_env_value(Some("always")),
            FileLogMode::Always
        );
        assert_eq!(
            FileLogMode::from_env_value(Some("true")),
            FileLogMode::Always
        );
        assert_eq!(FileLogMode::from_env_value(Some("1")), FileLogMode::Always);
        assert_eq!(FileLogMode::from_env_value(Some("on")), FileLogMode::Always);
    }

    #[test]
    fn expand_log_dir_leaves_non_tilde_paths_unchanged() {
        assert_eq!(
            expand_log_dir("/tmp/iota-logs"),
            PathBuf::from("/tmp/iota-logs")
        );
        assert_eq!(
            expand_log_dir("relative-logs"),
            PathBuf::from("relative-logs")
        );
    }

    #[test]
    fn file_log_layer_is_skipped_when_disabled_and_created_for_auto() {
        let dir = std::env::temp_dir().join(format!("iota-log-test-{}", uuid::Uuid::new_v4()));
        assert!(!dir.exists());

        let off = FileLogConfig {
            mode: FileLogMode::Off,
            dir: dir.clone(),
            retention: LogRetention::Days(30),
        };
        assert!(build_file_log_layer(&off).unwrap().is_none());
        assert!(!dir.exists());

        let auto = FileLogConfig {
            mode: FileLogMode::Auto,
            dir: dir.clone(),
            retention: LogRetention::Days(30),
        };
        let layer = build_file_log_layer(&auto).unwrap();
        assert!(layer.is_some());
        assert!(dir.is_dir());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_log_layer_writes_tracing_events() {
        let dir =
            std::env::temp_dir().join(format!("iota-log-write-test-{}", uuid::Uuid::new_v4()));
        let config = FileLogConfig {
            mode: FileLogMode::Always,
            dir: dir.clone(),
            retention: LogRetention::Days(30),
        };
        let file_log = build_file_log_layer(&config).unwrap().unwrap();
        let guard = file_log.guard;
        let subscriber = tracing_subscriber::registry()
            .with(file_log.layer)
            .with(EnvFilter::new("info"));

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "iota_sympantos::telemetry", "file-log-test-message");
        });
        drop(guard);

        let mut contents = String::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                contents.push_str(&std::fs::read_to_string(path).unwrap());
            }
        }
        assert!(contents.contains("file-log-test-message"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn log_retention_defaults_to_thirty_days_and_can_be_disabled() {
        assert_eq!(LogRetention::from_env_value(None).days(), Some(30));
        assert_eq!(LogRetention::from_env_value(Some("14")).days(), Some(14));
        assert_eq!(LogRetention::from_env_value(Some("off")).days(), None);
        assert_eq!(LogRetention::from_env_value(Some("0")).days(), None);
    }

    #[test]
    fn log_retention_deletes_only_expired_iota_logs() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();

        assert!(should_delete_log_file("iota.log.2026-04-09", today, 30));
        assert!(!should_delete_log_file("iota.log.2026-04-10", today, 30));
        assert!(!should_delete_log_file("other.log.2026-04-09", today, 30));
        assert!(!should_delete_log_file("iota.log.not-a-date", today, 30));
    }
}
