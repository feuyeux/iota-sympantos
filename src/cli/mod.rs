use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::acp;
use crate::config::{self, NimiaConfig};
use crate::context::server as context_server;
use crate::daemon::{self, DaemonPromptRequest};
use crate::engine::IotaEngine;
use crate::skill::SkillRegistry;
use crate::skill::fun_server;
use crate::store::cache::{CacheMetricsSnapshot, CacheStore};
use crate::store::memory::MemoryStore;
use crate::telemetry::{self, TelemetryConfig};
use crate::{native, skill, tui};

const DEFAULT_METRICS_ADDR: &str = "127.0.0.1:47662";

type LocalMetricsSnapshot = CacheMetricsSnapshot;

pub async fn run() -> Result<()> {
    let _otel_guard = telemetry::init(&TelemetryConfig::default())?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(command) = args.first().map(String::as_str) {
        match command {
            "run" => {
                let options = acp::parse_acp_args(&args[1..])?;
                if options.use_daemon && options.show_native {
                    anyhow::bail!("--daemon cannot be combined with --show-native");
                }
                if options.use_daemon {
                    return run_prompt_via_daemon(&options).await;
                } else {
                    let config = config::read_config()?;
                    let mut engine = IotaEngine::new_for_session_cwd(
                        config,
                        options.show_native,
                        options.timeout_ms,
                        None,
                    );
                    let result = engine
                        .prompt_in_cwd_timed(options.backend, options.cwd, &options.prompt)
                        .await;
                    engine.shutdown().await;
                    let output = result?;
                    if options.log_events {
                        for event in &output.events {
                            eprintln!("{}", serde_json::to_string(event).unwrap_or_default());
                        }
                    }
                    if options.timing {
                        print_route_timing(
                            "direct",
                            options.backend,
                            output.execution_id.as_deref(),
                            Some(&output.timing),
                        );
                    }
                    let text = output.text;
                    if !text.is_empty() {
                        println!("{}", text);
                    }
                    return Ok(());
                }
            }
            "context-mcp" => {
                return context_server::run_stdio();
            }
            "fun-mcp" => {
                return fun_server::run_stdio();
            }
            "native-materialize" => {
                return run_native_materialize(&args[1..]);
            }
            "logs" => {
                return run_logs_command(&args[1..]).await;
            }
            "trace" => {
                return run_trace_command(&args[1..]).await;
            }
            "metrics" => {
                return run_metrics_command(&args[1..]).await;
            }
            "skill" => {
                return run_skill_command(&args[1..]).await;
            }
            "__daemon" => {
                let config = config::read_config()?;
                let daemon_addr = daemon::daemon_addr();
                return daemon::run_daemon(config, &daemon_addr, acp::DEFAULT_TIMEOUT_MS, false)
                    .await;
            }
            "check" => {
                let use_daemon = has_daemon_flag(&args[1..]);
                if use_daemon {
                    warm_daemon_for_current_dir(Vec::new()).await?;
                }
                let config = config::read_config()?;
                print_combined_info(&config)?;
                return Ok(());
            }
            "tui" => {
                let config = config::read_config()?;
                return tui::run(config).await;
            }
            "bench-cold" => {
                let config = config::read_config()?;
                let use_daemon = has_daemon_flag(&args[1..]);
                let rounds = parse_rounds(&args[1..]).unwrap_or(3);
                if use_daemon {
                    return run_daemon_benchmark(&config, rounds).await;
                }
                return run_cold_benchmark(config, rounds).await;
            }
            "bench-warm" => {
                let config = config::read_config()?;
                let use_daemon = has_daemon_flag(&args[1..]);
                let rounds = parse_rounds(&args[1..]).unwrap_or(3);
                if use_daemon {
                    return run_daemon_benchmark(&config, rounds).await;
                }
                return run_warm_benchmark(config, rounds).await;
            }
            "-h" | "--help" | "help" => {
                print_help();
                return Ok(());
            }
            other => {
                eprintln!("Unknown command: {}", other);
                print_help();
                std::process::exit(2);
            }
        }
    }

    let config = config::read_config()?;
    tui::run(config).await
}

fn print_route_timing(
    route: &str,
    backend: acp::AcpBackend,
    execution_id: Option<&str>,
    timing: Option<&acp::AcpPromptTiming>,
) {
    eprintln!(
        "[iota run timing] {}",
        serde_json::json!({
            "route": route,
            "daemon_hit": route == "daemon",
            "fallback": false,
            "backend": backend.to_string(),
            "execution_id": execution_id,
            "timing": timing,
        })
    );
}

fn print_help() {
    println!(
        "Usage:\n  iota\n  iota check [--daemon|-d]\n  iota bench-cold [rounds] [--daemon|-d]\n  iota bench-warm [rounds] [--daemon|-d]\n  iota run [backend] [options] <prompt>\n  iota logs <execution_id>\n  iota trace <trace_id>\n  iota trace --execution <execution_id>\n  iota metrics [--listen <addr>] [--once]\n  iota context-mcp\n  iota fun-mcp\n  iota native-materialize --dry-run <path> <content>\n  iota skill pull <source> [name]\n\nNotes:\n  No arguments enters the TUI.\n  check prints one combined JSON structure.\n  Add --daemon or -d to route supported commands through the local daemon; it starts silently if needed.\n\nConfiguration:\n  All backend config is read from ~/.i6/nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota run --help` for run options."
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetricsOptions {
    listen_addr: String,
    once: bool,
}

fn parse_metrics_options(args: &[String]) -> Result<MetricsOptions> {
    let mut listen_addr = std::env::var("IOTA_METRICS_ADDR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_METRICS_ADDR.to_string());
    let mut once = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--listen" | "--addr" => {
                let value = args
                    .get(index + 1)
                    .context("iota metrics --listen requires an address")?;
                listen_addr = value.clone();
                index += 2;
            }
            "--once" => {
                once = true;
                index += 1;
            }
            "-h" | "--help" => {
                anyhow::bail!("Usage: iota metrics [--listen <addr>] [--once]");
            }
            other => anyhow::bail!("Unknown iota metrics option: {}", other),
        }
    }
    Ok(MetricsOptions { listen_addr, once })
}

async fn run_metrics_command(args: &[String]) -> Result<()> {
    let options = parse_metrics_options(args)?;
    if options.once {
        let snapshot = read_local_metrics_snapshot()?;
        print!("{}", format_local_prometheus_metrics(&snapshot));
        return Ok(());
    }
    run_metrics_server(&options.listen_addr).await
}

fn read_local_metrics_snapshot() -> Result<LocalMetricsSnapshot> {
    let path = CacheStore::default_path()?;
    CacheStore::open(&path)?.metrics_snapshot()
}

fn format_local_prometheus_metrics(snapshot: &LocalMetricsSnapshot) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let mut output = String::new();
    output.push_str("# HELP iota_build_info Build metadata for this iota binary.\n");
    output.push_str("# TYPE iota_build_info gauge\n");
    output.push_str(&format!(
        "iota_build_info{{version=\"{}\"}} 1\n",
        escape_prometheus_label_value(version)
    ));
    output.push_str("# HELP iota_cache_executions_total Cached executions by status.\n");
    output.push_str("# TYPE iota_cache_executions_total gauge\n");
    for (status, count) in &snapshot.execution_status_counts {
        output.push_str(&format!(
            "iota_cache_executions_total{{status=\"{}\"}} {}\n",
            escape_prometheus_label_value(status),
            count
        ));
    }
    output.push_str("# HELP iota_cache_outputs_total Cached output events retained locally.\n");
    output.push_str("# TYPE iota_cache_outputs_total gauge\n");
    output.push_str(&format!(
        "iota_cache_outputs_total {}\n",
        snapshot.outputs_total
    ));
    output
}

fn escape_prometheus_label_value(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('\n', r"\n")
        .replace('"', r#"\""#)
}

async fn run_metrics_server(listen_addr: &str) -> Result<()> {
    let listener = TcpListener::bind(listen_addr)
        .await
        .with_context(|| format!("Failed to bind metrics server at {}", listen_addr))?;
    eprintln!("iota metrics listening on http://{}/metrics", listen_addr);
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(err) = handle_metrics_connection(stream).await {
                eprintln!("metrics request failed: {}", err);
            }
        });
    }
}

async fn handle_metrics_connection(mut stream: TcpStream) -> Result<()> {
    let mut buf = vec![0_u8; 8192];
    let read = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (status, content_type, body) = if path == "/metrics" || path.starts_with("/metrics?") {
        let snapshot = read_local_metrics_snapshot()?;
        (
            "200 OK",
            "text/plain; version=0.0.4; charset=utf-8",
            format_local_prometheus_metrics(&snapshot),
        )
    } else {
        (
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found\n".to_string(),
        )
    };
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        content_type,
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn run_logs_command(args: &[String]) -> Result<()> {
    let execution_id = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("Usage: iota logs <execution_id>"))?;
    let loki_url =
        std::env::var("IOTA_LOKI_URL").unwrap_or_else(|_| "http://localhost:3100".to_string());
    let client = reqwest::Client::new();
    for query in loki_log_queries(execution_id) {
        let body = query_loki_logs(&client, &loki_url, &query).await?;
        if print_loki_lines(&body, execution_id) {
            return Ok(());
        }
    }
    println!("No logs found for execution {}", execution_id);
    Ok(())
}

fn loki_log_queries(execution_id: &str) -> Vec<String> {
    vec![
        format!(
            r#"{{service_name="iota", execution_id="{}"}}"#,
            execution_id
        ),
        format!(r#"{{service_name="iota"}} |= "{}""#, execution_id),
        r#"{service_name="iota"}"#.to_string(),
    ]
}

async fn query_loki_logs(
    client: &reqwest::Client,
    loki_url: &str,
    query: &str,
) -> Result<serde_json::Value> {
    let url = format!(
        "{}/loki/api/v1/query_range?query={}&limit=1000&since=1h",
        loki_url,
        urlencoding::encode(query)
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Loki at {}", loki_url))?;
    if !resp.status().is_success() {
        bail!("Loki query failed with status {}", resp.status());
    }
    Ok(resp.json().await?)
}

fn print_loki_lines(body: &serde_json::Value, execution_id: &str) -> bool {
    let mut printed = false;
    if let Some(results) = body["data"]["result"].as_array() {
        if results.is_empty() {
            return false;
        }
        for stream in results {
            let stream_matches = stream["stream"]
                .get("execution_id")
                .and_then(serde_json::Value::as_str)
                == Some(execution_id);
            if let Some(values) = stream["values"].as_array() {
                for entry in values {
                    if let Some(arr) = entry.as_array() {
                        if arr.len() >= 2 {
                            if let Some(line) = arr[1].as_str() {
                                if stream_matches || line.contains(execution_id) {
                                    println!("{}", line);
                                    printed = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        printed
    } else {
        false
    }
}

async fn run_trace_command(args: &[String]) -> Result<()> {
    let trace_target = parse_trace_target(args)?;
    let jaeger_url =
        std::env::var("IOTA_JAEGER_URL").unwrap_or_else(|_| "http://localhost:16686".to_string());
    let client = reqwest::Client::new();
    match trace_target {
        TraceTarget::TraceId(trace_id) => {
            let body = query_jaeger_trace(&client, &jaeger_url, &trace_id).await?;
            print_jaeger_trace(&body, &trace_id);
        }
        TraceTarget::ExecutionId(execution_id) => {
            let loki_url = std::env::var("IOTA_LOKI_URL")
                .unwrap_or_else(|_| "http://localhost:3100".to_string());
            if let Some(trace_id) =
                find_trace_id_for_execution(&client, &loki_url, &execution_id).await?
            {
                let body = query_jaeger_trace(&client, &jaeger_url, &trace_id).await?;
                print_jaeger_trace(&body, &trace_id);
            } else {
                let body =
                    query_jaeger_traces_for_execution(&client, &jaeger_url, &execution_id).await?;
                if jaeger_trace_count(&body) == 0 {
                    anyhow::bail!(
                        "No traces found for execution {}. Search Loki/Grafana for this execution id or pass `iota trace <trace_id>`.",
                        execution_id
                    );
                }
                print_jaeger_trace(&body, &execution_id);
            }
        }
    }
    Ok(())
}

enum TraceTarget {
    TraceId(String),
    ExecutionId(String),
}

fn parse_trace_target(args: &[String]) -> Result<TraceTarget> {
    match args {
        [trace_id] if trace_id != "--execution" => Ok(TraceTarget::TraceId(trace_id.clone())),
        [flag, execution_id] if matches!(flag.as_str(), "--execution" | "-e") => {
            Ok(TraceTarget::ExecutionId(execution_id.clone()))
        }
        _ => anyhow::bail!("Usage: iota trace <trace_id> | iota trace --execution <execution_id>"),
    }
}

async fn find_trace_id_for_execution(
    client: &reqwest::Client,
    loki_url: &str,
    execution_id: &str,
) -> Result<Option<String>> {
    for query in loki_log_queries(execution_id) {
        let body = query_loki_logs(client, loki_url, &query).await?;
        if let Some(trace_id) = extract_trace_id_from_loki(&body, execution_id) {
            return Ok(Some(trace_id));
        }
    }
    Ok(None)
}

fn extract_trace_id_from_loki(body: &serde_json::Value, execution_id: &str) -> Option<String> {
    let results = body["data"]["result"].as_array()?;
    for stream in results {
        let stream_obj = &stream["stream"];
        let stream_matches = stream_obj
            .get("execution_id")
            .and_then(serde_json::Value::as_str)
            == Some(execution_id);
        if let Some(trace_id) = trace_id_from_value(stream_obj) {
            if stream_matches || stream_contains_execution(stream, execution_id) {
                return Some(trace_id);
            }
        }
        if let Some(values) = stream["values"].as_array() {
            for entry in values {
                let Some(line) = entry
                    .as_array()
                    .and_then(|arr| arr.get(1))
                    .and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                if !(stream_matches || line.contains(execution_id)) {
                    continue;
                }
                if let Some(trace_id) = trace_id_from_line(line) {
                    return Some(trace_id);
                }
            }
        }
    }
    None
}

fn stream_contains_execution(stream: &serde_json::Value, execution_id: &str) -> bool {
    stream["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.as_array()?.get(1)?.as_str())
        .any(|line| line.contains(execution_id))
}

fn trace_id_from_line(line: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
        return trace_id_from_value(&value);
    }
    for key in ["trace_id", "traceid", "traceId"] {
        if let Some(trace_id) = trace_id_after_key(line, key) {
            return Some(trace_id);
        }
    }
    None
}

fn trace_id_from_value(value: &serde_json::Value) -> Option<String> {
    for key in ["trace_id", "traceid", "traceId"] {
        if let Some(trace_id) = value.get(key).and_then(serde_json::Value::as_str) {
            if is_trace_id_like(trace_id) {
                return Some(trace_id.to_string());
            }
        }
    }
    None
}

fn trace_id_after_key(line: &str, key: &str) -> Option<String> {
    let index = line.find(key)? + key.len();
    let suffix = &line[index..];
    let start = suffix.find(|ch: char| ch.is_ascii_hexdigit())?;
    let trace_id = suffix[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .collect::<String>();
    is_trace_id_like(&trace_id).then_some(trace_id)
}

fn is_trace_id_like(value: &str) -> bool {
    matches!(value.len(), 16 | 32) && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

async fn query_jaeger_trace(
    client: &reqwest::Client,
    jaeger_url: &str,
    trace_id: &str,
) -> Result<serde_json::Value> {
    let url = format!("{}/api/traces/{}", jaeger_url, trace_id);
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Jaeger at {}", jaeger_url))?;
    if !resp.status().is_success() {
        bail!("Jaeger query failed with status {}", resp.status());
    }
    Ok(resp.json().await?)
}

async fn query_jaeger_traces_for_execution(
    client: &reqwest::Client,
    jaeger_url: &str,
    execution_id: &str,
) -> Result<serde_json::Value> {
    let tags = format!(r#"{{"iota.execution.id":"{}"}}"#, execution_id);
    let url = format!(
        "{}/api/traces?service=iota&limit=100&tags={}",
        jaeger_url,
        urlencoding::encode(&tags)
    );
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Jaeger at {}", jaeger_url))?;
    if !resp.status().is_success() {
        bail!("Jaeger query failed with status {}", resp.status());
    }
    Ok(resp.json().await?)
}

fn jaeger_trace_count(body: &serde_json::Value) -> usize {
    body["data"].as_array().map_or(0, Vec::len)
}

fn print_jaeger_trace(body: &serde_json::Value, trace_id: &str) {
    print!("{}", format_jaeger_trace(body, trace_id));
}

fn format_jaeger_trace(body: &serde_json::Value, trace_id: &str) -> String {
    let mut output = String::new();
    if let Some(traces) = body["data"].as_array() {
        for trace in traces {
            if let Some(spans) = trace["spans"].as_array() {
                let current_trace_id = trace["traceID"].as_str().unwrap_or(trace_id);
                output.push_str(&format!(
                    "Trace {} ({} spans)\n",
                    current_trace_id,
                    spans.len()
                ));
                let mut ordered_spans = spans.iter().collect::<Vec<_>>();
                ordered_spans.sort_by_key(|span| span["startTime"].as_u64().unwrap_or(0));
                for span in ordered_spans {
                    let name = span["operationName"].as_str().unwrap_or("?");
                    let duration_us = span["duration"].as_u64().unwrap_or(0);
                    let duration_ms = duration_us / 1000;
                    let depth = if span["references"]
                        .as_array()
                        .map(|r| r.is_empty())
                        .unwrap_or(true)
                    {
                        0
                    } else {
                        1
                    };
                    let indent = "  ".repeat(depth);
                    output.push_str(&format!("{}- {} {}ms\n", indent, name, duration_ms));
                }
            }
        }
    } else {
        output.push_str(&format!("No trace found for {}\n", trace_id));
    }
    output
}

#[cfg(test)]
fn prometheus_query_has_results(body: &serde_json::Value) -> bool {
    body["data"]["result"]
        .as_array()
        .is_some_and(|results| !results.is_empty())
}

async fn run_skill_command(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("pull") => {
            let source = args
                .get(1)
                .context("skill pull requires a source path or URL")?;
            let name = args.get(2).map(String::as_str);
            let path = skill::cache::pull_skill(source, name).await?;
            println!(
                "{}",
                serde_json::json!({"path": path.display().to_string()})
            );
            Ok(())
        }
        _ => anyhow::bail!("Usage: iota skill pull <source> [name]"),
    }
}

fn run_native_materialize(args: &[String]) -> Result<()> {
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let backend = args
        .windows(2)
        .find_map(|pair| (pair[0] == "--backend").then_some(pair[1].as_str()))
        .map(acp::AcpBackend::parse)
        .transpose()?;
    let positional = args
        .iter()
        .enumerate()
        .filter(|(index, arg)| {
            arg.as_str() != "--dry-run"
                && arg.as_str() != "--all"
                && arg.as_str() != "--backend"
                && index
                    .checked_sub(1)
                    .is_none_or(|prev| args[prev] != "--backend")
        })
        .map(|(_, arg)| arg)
        .collect::<Vec<_>>();
    let body = positional
        .get(1)
        .map(|value| value.as_str())
        .unwrap_or("iota native overlay");
    if args.iter().any(|arg| arg == "--all") {
        let backend = backend.context("native-materialize --all requires --backend <name>")?;
        let workspace = positional
            .first()
            .map(std::path::PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        let config = config::read_config()?;
        let roots = config::context_skill_roots(&config);
        let skills = SkillRegistry::load(&workspace, &roots);
        let memory = config::context_memory_db_path(&config)
            .ok()
            .and_then(|path| MemoryStore::open(&path).ok());
        let previews = native::dry_run_backend_projection(
            backend,
            &workspace,
            memory.as_ref(),
            Some(&skills),
        )?;
        if dry_run {
            println!(
                "{}",
                serde_json::json!({
                    "previews": previews.iter().map(|preview| serde_json::json!({
                        "path": preview.path.display().to_string(),
                        "changed": preview.changed,
                        "content": preview.content,
                    })).collect::<Vec<_>>()
                })
            );
        } else {
            let changed = previews
                .iter()
                .map(|preview| native::apply(&preview.path, &preview.content))
                .collect::<Result<Vec<_>>>()?;
            println!("{}", serde_json::json!({"changed": changed}));
        }
        return Ok(());
    }
    let path = if let Some(backend) = backend {
        let workspace = positional
            .first()
            .map(std::path::PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        native::backend_memory_path(backend, &workspace)?
            .context("native materialization for this backend is deferred")?
    } else {
        positional
            .first()
            .map(std::path::PathBuf::from)
            .context("native-materialize requires a target path or --backend <name>")?
    };
    if dry_run {
        let preview = native::dry_run(&path, body)?;
        println!(
            "{}",
            serde_json::json!({
                "path": preview.path.display().to_string(),
                "changed": preview.changed,
                "content": preview.content,
            })
        );
    } else {
        let changed = native::apply(&path, body)?;
        println!(
            "{}",
            serde_json::json!({"path": path.display().to_string(), "changed": changed})
        );
    }
    Ok(())
}

async fn run_prompt_via_daemon(options: &acp::AcpRunOptions) -> Result<()> {
    let request = DaemonPromptRequest {
        backend: options.backend.to_string(),
        cwd: options.cwd.display().to_string(),
        prompt: options.prompt.clone(),
        execution_id: None,
        timeout_ms: Some(options.timeout_ms),
        timing: options.timing,
    };
    let daemon_addr = daemon::daemon_addr();
    let response = send_prompt_autostart_daemon(&daemon_addr, &request).await?;
    if options.log_events {
        for event in &response.events {
            eprintln!("{}", serde_json::to_string(event).unwrap_or_default());
        }
    }
    if options.timing {
        print_route_timing(
            "daemon",
            options.backend,
            response.execution_id.as_deref(),
            response.timing.as_ref(),
        );
    }
    if response.ok {
        if let Some(text) = response.text.filter(|text| !text.is_empty()) {
            println!("{}", text);
        }
        return Ok(());
    }
    if let Some(error) = response.error {
        anyhow::bail!(error);
    }
    anyhow::bail!("Daemon returned an unsuccessful response without an error")
}

async fn send_prompt_autostart_daemon(
    daemon_addr: &str,
    request: &DaemonPromptRequest,
) -> Result<daemon::DaemonPromptResponse> {
    match daemon::send_prompt(daemon_addr, request).await {
        Ok(response) => Ok(response),
        Err(first_error) => {
            start_daemon_silently()?;
            wait_for_daemon(daemon_addr).await.with_context(|| {
                format!(
                    "Failed to start daemon at {} after initial connection error: {}",
                    daemon_addr, first_error
                )
            })?;
            daemon::send_prompt(daemon_addr, request).await
        }
    }
}

async fn warm_daemon_for_current_dir(backends: Vec<String>) -> Result<()> {
    let request = daemon::DaemonWarmRequest {
        request_type: "warm".to_string(),
        cwd: std::env::current_dir()
            .context("Failed to get current directory")?
            .display()
            .to_string(),
        backends,
    };
    let daemon_addr = daemon::daemon_addr();
    let response = match daemon::send_warm(&daemon_addr, &request).await {
        Ok(response) => response,
        Err(first_error) => {
            start_daemon_silently()?;
            wait_for_daemon(&daemon_addr).await.with_context(|| {
                format!(
                    "Failed to start daemon at {} after initial warm error: {}",
                    daemon_addr, first_error
                )
            })?;
            daemon::send_warm(&daemon_addr, &request).await?
        }
    };
    if response.ok {
        return Ok(());
    }
    if let Some(error) = response.error {
        anyhow::bail!(error);
    }
    anyhow::bail!("Daemon warm request failed without an error message")
}

fn start_daemon_silently() -> Result<()> {
    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let mut child = std::process::Command::new(exe)
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start daemon process")?;
    // On Unix the daemon detaches naturally once its parent (this call) returns.
    // On Windows the Child handle must be explicitly dropped or waited to avoid
    // leaking a kernel handle.  We spawn a background thread to wait on it so
    // we don't block the caller.
    #[cfg(target_os = "windows")]
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    #[cfg(not(target_os = "windows"))]
    drop(child);
    Ok(())
}

async fn wait_for_daemon(addr: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("Timed out waiting for daemon at {}", addr);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

fn has_daemon_flag(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--daemon" || arg == "-d" || arg == "--require-daemon")
}

fn parse_rounds(args: &[String]) -> Option<usize> {
    args.iter()
        .filter(|arg| !matches!(arg.as_str(), "--daemon" | "-d" | "--require-daemon"))
        .find_map(|value| value.parse::<usize>().ok())
}

#[derive(Serialize)]
struct CombinedInfo {
    config_path: String,
    daemon_addr: String,
    backends: Vec<BackendInfo>,
}

#[derive(Serialize)]
struct BackendInfo {
    backend: String,
    enabled: bool,
    check_status: String,
    acp_command: String,
    version_mapping: BackendVersionInfo,
    model: String,
}

#[derive(Serialize)]
struct BackendVersionInfo {
    acp: Option<String>,
    bin: Option<String>,
}

fn print_combined_info(config: &NimiaConfig) -> Result<()> {
    let info = CombinedInfo {
        config_path: config::config_path()?.display().to_string(),
        daemon_addr: daemon::daemon_addr(),
        backends: acp::ALL_BACKENDS
            .iter()
            .copied()
            .map(|backend| backend_info(config, backend))
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

fn backend_info(config: &NimiaConfig, backend: acp::AcpBackend) -> BackendInfo {
    let section = config::backend_config(config, backend);
    let check_status = match section {
        Some(section) if !section.enabled => "disabled",
        Some(section)
            if section
                .acp
                .as_ref()
                .is_some_and(|acp| !acp.command.trim().is_empty()) =>
        {
            "configured"
        }
        Some(_) => "missing acp.command",
        None => "missing section",
    };
    let enabled = section.is_some_and(|section| section.enabled);
    let acp_command = section
        .and_then(|section| section.acp.as_ref())
        .map(config::command_label)
        .unwrap_or_else(|| "missing acp".to_string());
    let model = section
        .map(config::configured_model)
        .unwrap_or(None)
        .unwrap_or_else(|| "-".to_string());
    let version_mapping = backend_version_info(section, backend);

    BackendInfo {
        backend: backend.to_string(),
        enabled,
        check_status: check_status.to_string(),
        acp_command,
        version_mapping,
        model,
    }
}

fn backend_version_info(
    section: Option<&config::BackendConfig>,
    backend: acp::AcpBackend,
) -> BackendVersionInfo {
    let explicit = section.and_then(|section| section.version_mapping.as_ref());
    let acp = explicit
        .and_then(|mapping| non_empty_string(mapping.acp.as_ref()))
        .or_else(|| section.and_then(inferred_acp_version_spec));
    let bin = explicit
        .and_then(|mapping| non_empty_string(mapping.bin.as_ref()))
        .or_else(|| section.and_then(|section| inferred_bin_version_spec(backend, section)));

    BackendVersionInfo { acp, bin }
}

fn non_empty_string(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn inferred_acp_version_spec(section: &config::BackendConfig) -> Option<String> {
    section
        .acp
        .as_ref()
        .and_then(npm_package_spec)
        .and_then(|package| package_version(&package))
}

fn inferred_bin_version_spec(
    backend: acp::AcpBackend,
    section: &config::BackendConfig,
) -> Option<String> {
    if backend == acp::AcpBackend::Codex {
        return None;
    }
    let package = section.acp.as_ref().and_then(npm_package_spec);
    package.and_then(|package| package_version(&package))
}

fn npm_package_spec(command: &config::CommandConfig) -> Option<String> {
    command
        .args
        .iter()
        .find(|arg| !arg.starts_with('-') && arg.contains('@'))
        .cloned()
}

fn package_version(package: &str) -> Option<String> {
    let (_, version) = package.rsplit_once('@')?;
    let version = version.trim();
    if version.is_empty() || version == "latest" {
        return None;
    }
    Some(version.to_string())
}

async fn run_cold_benchmark(config: NimiaConfig, rounds: usize) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    println!("backend,round,latency_ms,status");
    for round in 1..=rounds {
        for backend in acp::ALL_BACKENDS {
            if config::backend_config(&config, backend).is_none_or(|section| !section.enabled) {
                continue;
            }
            let mut engine = IotaEngine::new(config.clone(), false, acp::DEFAULT_TIMEOUT_MS);
            let started = std::time::Instant::now();
            let result = engine.prompt_in_cwd(backend, cwd.clone(), "ping").await;
            let elapsed = started.elapsed().as_millis();
            engine.shutdown().await;
            let status = if result.is_ok() { "ok" } else { "error" };
            println!("{},{},{},{}", backend, round, elapsed, status);
            if let Err(err) = result {
                eprintln!("{} round {} failed: {}", backend, round, err);
            }
        }
    }
    Ok(())
}

async fn run_daemon_benchmark(config: &NimiaConfig, rounds: usize) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let daemon_addr = daemon::daemon_addr();
    println!("backend,round,latency_ms,status");
    for round in 1..=rounds {
        for backend in acp::ALL_BACKENDS {
            if config::backend_config(config, backend).is_none_or(|section| !section.enabled) {
                continue;
            }
            let request = DaemonPromptRequest {
                backend: backend.to_string(),
                cwd: cwd.display().to_string(),
                prompt: "ping".to_string(),
                execution_id: None,
                timeout_ms: Some(acp::DEFAULT_TIMEOUT_MS),
                timing: false,
            };
            let started = std::time::Instant::now();
            let result = send_prompt_autostart_daemon(&daemon_addr, &request).await;
            let elapsed = started.elapsed().as_millis();
            let status = match &result {
                Ok(response) if response.ok => "ok",
                _ => "error",
            };
            println!("{},{},{},{}", backend, round, elapsed, status);
            match result {
                Ok(response) if response.ok => {}
                Ok(response) => {
                    eprintln!(
                        "{} daemon round {} failed: {}",
                        backend,
                        round,
                        response
                            .error
                            .unwrap_or_else(|| "unknown daemon error".to_string())
                    );
                }
                Err(err) => eprintln!("{} daemon round {} failed: {}", backend, round, err),
            }
        }
    }
    Ok(())
}

async fn run_warm_benchmark(config: NimiaConfig, rounds: usize) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut engine = IotaEngine::new(config, false, acp::DEFAULT_TIMEOUT_MS);
    engine.warm_enabled_backends_in_cwd(cwd.clone()).await?;

    println!("backend,round,latency_ms,status");
    for round in 1..=rounds {
        for backend in acp::ALL_BACKENDS {
            if !engine.is_warmed_in_cwd(backend, &cwd) {
                continue;
            }
            let started = std::time::Instant::now();
            let result = engine.prompt_in_cwd(backend, cwd.clone(), "ping").await;
            let elapsed = started.elapsed().as_millis();
            let status = if result.is_ok() { "ok" } else { "error" };
            println!("{},{},{},{}", backend, round, elapsed, status);
            if let Err(err) = result {
                eprintln!("{} round {} failed: {}", backend, round, err);
            }
        }
    }

    engine.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_info_includes_version_mapping() {
        let config = NimiaConfig {
            codex: Some(config::BackendConfig {
                enabled: true,
                acp: Some(config::CommandConfig {
                    command: "npx".to_string(),
                    args: vec![
                        "-y".to_string(),
                        "@zed-industries/codex-acp@0.12.0".to_string(),
                    ],
                }),
                version_mapping: Some(config::BackendVersionMapping {
                    acp: Some("0.12.0".to_string()),
                    bin: Some("0.128.0".to_string()),
                }),
                ..config::BackendConfig::default()
            }),
            ..NimiaConfig::default()
        };

        let value = serde_json::to_value(backend_info(&config, acp::AcpBackend::Codex)).unwrap();

        assert_eq!(value["version_mapping"]["acp"], "0.12.0");
        assert_eq!(value["version_mapping"]["bin"], "0.128.0");
    }

    #[test]
    fn backend_info_does_not_infer_codex_bin_from_acp_adapter() {
        let config = NimiaConfig {
            codex: Some(config::BackendConfig {
                enabled: true,
                acp: Some(config::CommandConfig {
                    command: "npx".to_string(),
                    args: vec![
                        "-y".to_string(),
                        "@zed-industries/codex-acp@0.12.0".to_string(),
                    ],
                }),
                ..config::BackendConfig::default()
            }),
            ..NimiaConfig::default()
        };

        let value = serde_json::to_value(backend_info(&config, acp::AcpBackend::Codex)).unwrap();

        assert_eq!(value["version_mapping"]["acp"], "0.12.0");
        assert!(value["version_mapping"]["bin"].is_null());
    }

    #[test]
    fn loki_log_queries_try_label_then_text_then_service_scan() {
        let queries = loki_log_queries("exec-123");

        assert_eq!(
            queries[0],
            r#"{service_name="iota", execution_id="exec-123"}"#
        );
        assert_eq!(queries[1], r#"{service_name="iota"} |= "exec-123""#);
        assert_eq!(queries[2], r#"{service_name="iota"}"#);
    }

    #[test]
    fn print_loki_lines_matches_label_or_line_body() {
        let body = serde_json::json!({
            "data": {
                "result": [
                    {
                        "stream": {"service_name": "iota", "execution_id": "exec-label"},
                        "values": [["1", "label only line"]]
                    },
                    {
                        "stream": {"service_name": "iota"},
                        "values": [["2", "line mentions exec-body"]]
                    }
                ]
            }
        });

        assert!(print_loki_lines(&body, "exec-label"));
        assert!(print_loki_lines(&body, "exec-body"));
        assert!(!print_loki_lines(&body, "missing"));
    }

    #[test]
    fn parses_trace_target_by_trace_id_or_execution_id() {
        match parse_trace_target(&["abc123".to_string()]).unwrap() {
            TraceTarget::TraceId(value) => assert_eq!(value, "abc123"),
            TraceTarget::ExecutionId(_) => panic!("expected trace id"),
        }

        match parse_trace_target(&["--execution".to_string(), "exec-1".to_string()]).unwrap() {
            TraceTarget::ExecutionId(value) => assert_eq!(value, "exec-1"),
            TraceTarget::TraceId(_) => panic!("expected execution id"),
        }

        assert!(parse_trace_target(&[]).is_err());
        assert!(parse_trace_target(&["--execution".to_string()]).is_err());
    }

    #[test]
    fn extracts_trace_id_from_loki_stream_label_or_json_line() {
        let body = serde_json::json!({
            "data": {
                "result": [
                    {
                        "stream": {
                            "service_name": "iota",
                            "execution_id": "exec-label",
                            "trace_id": "0123456789abcdef0123456789abcdef"
                        },
                        "values": [["1", "label trace"]]
                    },
                    {
                        "stream": {"service_name": "iota"},
                        "values": [["2", "{\"execution_id\":\"exec-json\",\"traceId\":\"fedcba9876543210fedcba9876543210\"}"]]
                    }
                ]
            }
        });

        assert_eq!(
            extract_trace_id_from_loki(&body, "exec-label").as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            extract_trace_id_from_loki(&body, "exec-json").as_deref(),
            Some("fedcba9876543210fedcba9876543210")
        );
        assert!(extract_trace_id_from_loki(&body, "missing").is_none());
    }

    #[test]
    fn extracts_trace_id_from_text_line() {
        assert_eq!(
            trace_id_from_line("trace_id=0123456789abcdef execution_id=exec-1").as_deref(),
            Some("0123456789abcdef")
        );
        assert!(trace_id_from_line("trace_id=not-a-trace").is_none());
    }

    #[test]
    fn counts_jaeger_traces() {
        let body = serde_json::json!({"data": [{"traceID": "a"}, {"traceID": "b"}]});
        assert_eq!(jaeger_trace_count(&body), 2);
        assert_eq!(jaeger_trace_count(&serde_json::json!({"data": []})), 0);
        assert_eq!(jaeger_trace_count(&serde_json::json!({})), 0);
    }

    #[test]
    fn formats_jaeger_traces_with_summary_and_start_time_order() {
        let body = serde_json::json!({
            "data": [{
                "traceID": "trace-a",
                "spans": [
                    {"spanID": "2", "operationName": "later", "startTime": 20, "duration": 1000, "references": []},
                    {"spanID": "1", "operationName": "earlier", "startTime": 10, "duration": 2000, "references": [{"refType": "CHILD_OF"}]}
                ]
            }]
        });

        let formatted = format_jaeger_trace(&body, "exec-1");

        assert!(formatted.contains("Trace trace-a (2 spans)"));
        assert!(
            formatted.find("  - earlier 2ms").unwrap() < formatted.find("- later 1ms").unwrap()
        );
    }

    #[test]
    fn prometheus_success_detects_any_non_empty_metric_result() {
        let body = serde_json::json!({"data": {"result": [{"metric": {}, "value": [1, "1"]}]}});
        assert!(prometheus_query_has_results(&body));
        assert!(!prometheus_query_has_results(
            &serde_json::json!({"data": {"result": []}})
        ));
    }

    #[test]
    fn parses_metrics_options_defaults_and_overrides() {
        let defaults = parse_metrics_options(&[]).unwrap();
        assert_eq!(defaults.listen_addr, DEFAULT_METRICS_ADDR);
        assert!(!defaults.once);

        let options = parse_metrics_options(&[
            "--listen".to_string(),
            "127.0.0.1:19090".to_string(),
            "--once".to_string(),
        ])
        .unwrap();
        assert_eq!(options.listen_addr, "127.0.0.1:19090");
        assert!(options.once);
    }

    #[test]
    fn formats_local_prometheus_metrics() {
        let snapshot = LocalMetricsSnapshot {
            execution_status_counts: [("completed".to_string(), 2), ("failed".to_string(), 1)]
                .into_iter()
                .collect(),
            outputs_total: 3,
        };

        let text = format_local_prometheus_metrics(&snapshot);

        assert!(text.contains("# HELP iota_build_info"));
        assert!(text.contains("iota_build_info{version=\""));
        assert!(text.contains("iota_cache_executions_total{status=\"completed\"} 2"));
        assert!(text.contains("iota_cache_executions_total{status=\"failed\"} 1"));
        assert!(text.contains("iota_cache_outputs_total 3"));
    }
}
