use anyhow::{Context, Result};
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, IntCounter, Registry, TextEncoder, opts,
};
use serde::Serialize;
use std::process::Stdio;
use tracing_subscriber::EnvFilter;

use crate::acp;
use crate::config::{self, NimiaConfig};
use crate::context::server as context_server;
use crate::daemon::{self, DaemonPromptRequest};
use crate::engine::IotaEngine;
use crate::runtime_event::RuntimeEvent;
use crate::skill::SkillRegistry;
use crate::skill::fun_server;
use crate::store::events::EventStore;
use crate::store::memory::MemoryStore;
use crate::{native, skill, tui};

pub async fn run() -> Result<()> {
    init_tracing();
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
                    if options.trace {
                        print_trace_events(&output.events);
                    }
                    if options.trace_timing {
                        print_route_timing("direct", options.backend, Some(&output.timing));
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
            "observability" | "obs" => {
                return run_observability_command(&args[1..]);
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
    timing: Option<&acp::AcpPromptTiming>,
) {
    eprintln!(
        "[iota run timing] {}",
        serde_json::json!({
            "route": route,
            "daemon_hit": route == "daemon",
            "fallback": false,
            "backend": backend.to_string(),
            "timing": timing,
        })
    );
}

fn print_trace_events(events: &[RuntimeEvent]) {
    for event in events {
        match event {
            RuntimeEvent::ToolCall(call) => {
                if !print_memory_tool_call(call) {
                    eprintln!("[{}] call {} args={}", call.id, call.name, call.arguments);
                }
            }
            RuntimeEvent::ToolResult(result) => {
                if !print_memory_tool_result(result) {
                    eprintln!(
                        "[{}] result {} ok={} value={}",
                        result.id,
                        result.name,
                        result.ok,
                        trace_result_value(&result.result)
                    );
                }
            }
            RuntimeEvent::Output(output) if output.role.as_deref() == Some("engine") => {
                eprintln!("[skill:output] {} bytes", output.text.len());
            }
            RuntimeEvent::Memory(memory) => {
                eprintln!(
                    "[memory:{}] id={} payload={}",
                    memory.action,
                    memory.memory_id.as_deref().unwrap_or("-"),
                    memory.payload
                );
            }
            _ => {}
        }
    }
}

fn print_memory_tool_call(call: &crate::runtime_event::ToolCallEvent) -> bool {
    match call.name.as_str() {
        "iota_memory_search" => {
            eprintln!(
                "[memory:read] id={} query={} limit={} mode={} args={}",
                call.id,
                json_field(&call.arguments, "query"),
                json_field(&call.arguments, "limit"),
                json_field(&call.arguments, "mode"),
                call.arguments
            );
            true
        }
        "iota_memory_write" => {
            let content_chars = call
                .arguments
                .get("content")
                .and_then(serde_json::Value::as_str)
                .map(|content| content.chars().count().to_string())
                .unwrap_or_else(|| "-".to_string());
            eprintln!(
                "[memory:write] id={} type={} facet={} scope={} scope_id={} confidence={} content_chars={} args={}",
                call.id,
                json_field(&call.arguments, "type"),
                json_field(&call.arguments, "facet"),
                json_field(&call.arguments, "scope"),
                json_field(&call.arguments, "scope_id"),
                json_field(&call.arguments, "confidence"),
                content_chars,
                call.arguments
            );
            true
        }
        _ => false,
    }
}

fn print_memory_tool_result(result: &crate::runtime_event::ToolResultEvent) -> bool {
    match result.name.as_str() {
        "iota_memory_search" => {
            eprintln!(
                "[memory:read:result] id={} ok={} record_count={} value={}",
                result.id,
                result.ok,
                memory_record_count(&result.result)
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                trace_result_value(&result.result)
            );
            true
        }
        "iota_memory_write" => {
            eprintln!(
                "[memory:write:result] id={} ok={} memory_id={} value={}",
                result.id,
                result.ok,
                memory_result_id(&result.result).unwrap_or("-"),
                trace_result_value(&result.result)
            );
            true
        }
        _ => false,
    }
}

fn json_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .map(|value| match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "-".to_string())
}

fn memory_result_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("structuredContent")
                .and_then(|structured| structured.get("id"))
                .and_then(serde_json::Value::as_str)
        })
}

fn memory_record_count(value: &serde_json::Value) -> Option<usize> {
    value
        .get("records")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .or_else(|| {
            value
                .get("structuredContent")
                .and_then(|structured| structured.get("records"))
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
        })
}

fn trace_result_value(value: &serde_json::Value) -> String {
    if let Some(content) = value.get("content").and_then(serde_json::Value::as_array) {
        let text = content
            .iter()
            .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("\\n");
        if !text.is_empty() {
            return text;
        }
    }
    value.to_string()
}

fn print_help() {
    println!(
        "Usage:\n  iota\n  iota check [--daemon|-d]\n  iota bench-cold [rounds] [--daemon|-d]\n  iota bench-warm [rounds] [--daemon|-d]\n  iota run [backend] [options] <prompt>\n  iota observability <logging|tracing|metrics> [subcommand] [options]\n  iota context-mcp\n  iota fun-mcp\n  iota native-materialize --dry-run <path> <content>\n  iota skill pull <source> [name]\n\nNotes:\n  No arguments enters the TUI.\n  check prints one combined JSON structure.\n  Add --daemon or -d to route supported commands through the local daemon; it starts silently if needed.\n\nConfiguration:\n  All backend config is read from ~/.i6/nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota run --help` for run options."
    );
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("iota_sympantos=warn"))
        .unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

fn run_observability_command(args: &[String]) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("help");
    if matches!(command, "-h" | "--help" | "help") {
        print_observability_help();
        return Ok(());
    }
    let store = EventStore::open(&EventStore::default_path()?)?;
    let sub_args = if args.len() > 1 {
        &args[1..]
    } else {
        &[] as &[String]
    };
    match command {
        "logging" | "log" => run_obs_logging(sub_args, &store),
        "tracing" | "trace" => run_obs_tracing(sub_args, &store),
        "metrics" | "metric" => run_obs_metrics(sub_args, &store),
        // soft-deprecated aliases kept for backwards compat
        "summary" => {
            let limit = parse_limit(args).unwrap_or(10);
            println!(
                "{}",
                serde_json::to_string_pretty(&store.observability_summary(limit)?)?
            );
            Ok(())
        }
        "recent" => {
            let limit = parse_limit(args).unwrap_or(10);
            println!(
                "{}",
                serde_json::to_string_pretty(&store.recent_executions(limit)?)?
            );
            Ok(())
        }
        other => anyhow::bail!(
            "Unknown observability command '{}'. Run `iota observability --help` for usage.",
            other
        ),
    }
}

fn print_observability_help() {
    println!(
        "Usage:\n  iota observability <command> [subcommand] [options]\n\nCommands:\n  logging   Browse execution logs and event streams\n  tracing   Inspect timing and latency data\n  metrics   View aggregated counters and gauges\n\nRun `iota observability <command> --help` for subcommand details."
    );
}

// ── logging ──────────────────────────────────────────────────────────────────

fn run_obs_logging(args: &[String], store: &EventStore) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("help");
    if matches!(sub, "-h" | "--help" | "help") {
        println!(
            "Usage:\n  iota observability logging recent [--limit N]        Recent executions (id, backend, status, time)\n  iota observability logging errors [--limit N]        Failed executions only\n  iota observability logging events <execution-id>     Full event stream for one execution\n  iota observability logging tools [--limit N] [--tool NAME]\n                                                              tool_call events across recent executions\n  iota observability logging approvals [--limit N]     approval_request/decision events"
        );
        return Ok(());
    }
    let limit = parse_limit(args).unwrap_or(20);
    match sub {
        "recent" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.recent_executions(limit)?)?
            );
        }
        "errors" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.executions_by_status("failed", limit)?)?
            );
        }
        "events" => {
            let execution_id = args.get(1).ok_or_else(|| {
                anyhow::anyhow!("Usage: iota observability logging events <execution-id>")
            })?;
            #[derive(serde::Serialize)]
            struct EventEntry {
                seq: i64,
                event_type: String,
                event: serde_json::Value,
            }
            let events = store.execution_events(execution_id)?;
            let out: Vec<EventEntry> = events
                .into_iter()
                .map(|(seq, event_type, event)| EventEntry {
                    seq,
                    event_type,
                    event: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        "tools" => {
            use crate::runtime_event::RuntimeEvent;
            #[derive(serde::Serialize)]
            struct ToolEntry {
                execution_id: String,
                backend: String,
                seq: i64,
                tool_name: String,
                arguments: serde_json::Value,
            }
            let tool_filter = parse_tool_filter(args);
            let executions = store.recent_executions(limit.saturating_mul(5))?;
            let mut entries: Vec<ToolEntry> = Vec::new();
            'outer: for exec in &executions {
                for (seq, _, event) in store.execution_events(&exec.execution_id)? {
                    if let RuntimeEvent::ToolCall(tc) = event {
                        if tool_filter.is_some_and(|name| name != tc.name.as_str()) {
                            continue;
                        }
                        entries.push(ToolEntry {
                            execution_id: exec.execution_id.clone(),
                            backend: exec.backend.clone(),
                            seq,
                            tool_name: tc.name,
                            arguments: tc.arguments,
                        });
                        if entries.len() >= limit {
                            break 'outer;
                        }
                    }
                }
            }
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
        "approvals" => {
            use crate::runtime_event::RuntimeEvent;
            #[derive(serde::Serialize)]
            struct ApprovalEntry {
                execution_id: String,
                backend: String,
                seq: i64,
                event_type: String,
                detail: serde_json::Value,
            }
            let executions = store.recent_executions(limit.saturating_mul(5))?;
            let mut entries: Vec<ApprovalEntry> = Vec::new();
            'outer: for exec in &executions {
                for (seq, event_type, event) in store.execution_events(&exec.execution_id)? {
                    let detail = match &event {
                        RuntimeEvent::ApprovalRequest(_) | RuntimeEvent::ApprovalDecision(_) => {
                            Some(serde_json::to_value(&event).unwrap_or(serde_json::Value::Null))
                        }
                        _ => None,
                    };
                    if let Some(detail) = detail {
                        entries.push(ApprovalEntry {
                            execution_id: exec.execution_id.clone(),
                            backend: exec.backend.clone(),
                            seq,
                            event_type,
                            detail,
                        });
                        if entries.len() >= limit {
                            break 'outer;
                        }
                    }
                }
            }
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
        other => anyhow::bail!(
            "Unknown logging subcommand '{}'. Run `iota observability logging --help`.",
            other
        ),
    }
    Ok(())
}

// ── tracing ───────────────────────────────────────────────────────────────────

fn run_obs_tracing(args: &[String], store: &EventStore) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("help");
    if matches!(sub, "-h" | "--help" | "help") {
        println!(
            "Usage:\n  iota observability tracing recent [--limit N]        Recent executions with timing fields\n  iota observability tracing slow [--limit N]          Slowest executions by total_ms\n  iota observability tracing breakdown <execution-id>  5-phase latency breakdown for one execution\n  iota observability tracing summary                   avg/p95 latency statistics"
        );
        return Ok(());
    }
    let limit = parse_limit(args).unwrap_or(20);
    match sub {
        "recent" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.recent_executions(limit)?)?
            );
        }
        "slow" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.slowest_executions(limit)?)?
            );
        }
        "breakdown" => {
            let execution_id = args.get(1).ok_or_else(|| {
                anyhow::anyhow!("Usage: iota observability tracing breakdown <execution-id>")
            })?;
            let record = store
                .get_execution(execution_id)?
                .ok_or_else(|| anyhow::anyhow!("Execution '{}' not found", execution_id))?;
            #[derive(serde::Serialize)]
            struct Phase {
                phase: &'static str,
                ms: Option<u64>,
            }
            #[derive(serde::Serialize)]
            struct Breakdown {
                execution_id: String,
                backend: String,
                status: String,
                process_spawn_ms: Option<u64>,
                init_ms: Option<u64>,
                session_new_ms: Option<u64>,
                prompt_ms: Option<u64>,
                total_ms: Option<u64>,
                phases: Vec<Phase>,
            }
            let breakdown = Breakdown {
                phases: vec![
                    Phase {
                        phase: "process_spawn",
                        ms: record.process_spawn_ms,
                    },
                    Phase {
                        phase: "init",
                        ms: record.init_ms,
                    },
                    Phase {
                        phase: "session_new",
                        ms: record.session_new_ms,
                    },
                    Phase {
                        phase: "prompt",
                        ms: record.prompt_ms,
                    },
                    Phase {
                        phase: "total",
                        ms: record.total_ms,
                    },
                ],
                execution_id: record.execution_id,
                backend: record.backend,
                status: record.status.to_string(),
                process_spawn_ms: record.process_spawn_ms,
                init_ms: record.init_ms,
                session_new_ms: record.session_new_ms,
                prompt_ms: record.prompt_ms,
                total_ms: record.total_ms,
            };
            println!("{}", serde_json::to_string_pretty(&breakdown)?);
        }
        "summary" => {
            let s = store.observability_summary(0)?;
            #[derive(serde::Serialize)]
            struct TracingSummary {
                total_executions: u64,
                completed_executions: u64,
                failed_executions: u64,
                running_executions: u64,
                avg_prompt_ms: Option<f64>,
                avg_total_ms: Option<f64>,
                p95_total_ms: Option<u64>,
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&TracingSummary {
                    total_executions: s.total_executions,
                    completed_executions: s.completed_executions,
                    failed_executions: s.failed_executions,
                    running_executions: s.running_executions,
                    avg_prompt_ms: s.avg_prompt_ms,
                    avg_total_ms: s.avg_total_ms,
                    p95_total_ms: s.p95_total_ms,
                })?
            );
        }
        other => anyhow::bail!(
            "Unknown tracing subcommand '{}'. Run `iota observability tracing --help`.",
            other
        ),
    }
    Ok(())
}

// ── metrics ───────────────────────────────────────────────────────────────────

fn run_obs_metrics(args: &[String], store: &EventStore) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("");
    if matches!(sub, "-h" | "--help" | "help") {
        println!(
            "Usage:\n  iota observability metrics [--prometheus]   Human-readable aggregate (or Prometheus exposition)\n  iota observability metrics tokens            Token usage breakdown\n  iota observability metrics cache             Cache hit/miss ratio\n  iota observability metrics sessions          Active sessions and queued prompts\n  iota observability metrics latency           Latency avg and p95"
        );
        return Ok(());
    }
    // bare `iota observability metrics` or `iota observability metrics --prometheus`
    if sub.is_empty() || sub.starts_with('-') {
        let use_prometheus = args.iter().any(|a| a == "--prometheus" || a == "-p");
        if use_prometheus {
            return print_prometheus_metrics(store);
        }
        let s = store.observability_summary(5)?;
        #[derive(serde::Serialize)]
        struct MetricsSummary {
            executions: serde_json::Value,
            latency: serde_json::Value,
            tokens: serde_json::Value,
            cache: serde_json::Value,
            runtime: serde_json::Value,
        }
        let out = MetricsSummary {
            executions: serde_json::json!({
                "total": s.total_executions,
                "completed": s.completed_executions,
                "failed": s.failed_executions,
                "running": s.running_executions,
            }),
            latency: serde_json::json!({
                "avg_prompt_ms": s.avg_prompt_ms,
                "avg_total_ms": s.avg_total_ms,
                "p95_total_ms": s.p95_total_ms,
            }),
            tokens: serde_json::json!({
                "events": s.token_usage.events,
                "input_tokens": s.token_usage.input_tokens,
                "output_tokens": s.token_usage.output_tokens,
                "total_tokens": s.token_usage.total_tokens,
            }),
            cache: serde_json::json!({
                "hits": s.cache_hits,
                "misses": s.cache_misses,
                "hit_rate": if s.cache_hits + s.cache_misses > 0 {
                    Some(s.cache_hits as f64 / (s.cache_hits + s.cache_misses) as f64)
                } else { None },
            }),
            runtime: serde_json::json!({
                "active_sessions": s.active_sessions,
                "queued_prompts": s.queued_prompts,
            }),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    match sub {
        "tokens" => {
            let s = store.observability_summary(0)?;
            let u = &s.token_usage;
            let avg_input = if u.events > 0 {
                Some(u.input_tokens as f64 / u.events as f64)
            } else {
                None
            };
            let avg_output = if u.events > 0 {
                Some(u.output_tokens as f64 / u.events as f64)
            } else {
                None
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "token_usage_events": u.events,
                    "input_tokens": u.input_tokens,
                    "output_tokens": u.output_tokens,
                    "total_tokens": u.total_tokens,
                    "avg_input_per_execution": avg_input,
                    "avg_output_per_execution": avg_output,
                }))?
            );
        }
        "cache" => {
            let s = store.observability_summary(0)?;
            let total = s.cache_hits + s.cache_misses;
            let hit_rate = if total > 0 {
                Some(s.cache_hits as f64 / total as f64)
            } else {
                None
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "hits": s.cache_hits,
                    "misses": s.cache_misses,
                    "total": total,
                    "hit_rate": hit_rate,
                }))?
            );
        }
        "sessions" => {
            let s = store.observability_summary(0)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "active_sessions": s.active_sessions,
                    "queued_prompts": s.queued_prompts,
                }))?
            );
        }
        "latency" => {
            let s = store.observability_summary(0)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "avg_prompt_ms": s.avg_prompt_ms,
                    "avg_total_ms": s.avg_total_ms,
                    "p95_total_ms": s.p95_total_ms,
                }))?
            );
        }
        other => anyhow::bail!(
            "Unknown metrics subcommand '{}'. Run `iota observability metrics --help`.",
            other
        ),
    }
    Ok(())
}

fn print_prometheus_metrics(store: &EventStore) -> Result<()> {
    let metrics = store.prometheus_metrics()?;
    let registry = Registry::new();
    let execution_attempts = IntCounter::with_opts(opts!(
        "iota_execution_attempts_total",
        "Total recorded executions"
    ))?;
    execution_attempts.inc_by(metrics.execution_attempts);
    registry.register(Box::new(execution_attempts))?;

    for (name, help, value) in [
        (
            "iota_execution_completed_total",
            "Completed executions",
            metrics.execution_completed,
        ),
        (
            "iota_execution_failed_total",
            "Failed executions",
            metrics.execution_failed,
        ),
        (
            "iota_execution_running",
            "Currently running executions",
            metrics.execution_running,
        ),
        (
            "iota_active_sessions",
            "Active ACP sessions tracked by the engine",
            metrics.active_sessions,
        ),
        (
            "iota_queued_prompts",
            "Queued TUI prompts waiting for the current turn",
            metrics.queued_prompts,
        ),
        (
            "iota_token_usage_events_total",
            "Token usage events captured",
            metrics.token_usage.events,
        ),
        (
            "iota_input_tokens_total",
            "Captured input tokens",
            metrics.token_usage.input_tokens,
        ),
        (
            "iota_output_tokens_total",
            "Captured output tokens",
            metrics.token_usage.output_tokens,
        ),
        (
            "iota_tokens_total",
            "Captured total tokens",
            metrics.token_usage.total_tokens,
        ),
    ] {
        let gauge = Gauge::with_opts(opts!(name, help))?;
        gauge.set(value as f64);
        registry.register(Box::new(gauge))?;
    }

    for (name, help, value) in [
        (
            "iota_cache_hits_total",
            "Completed execution cache hits",
            metrics.cache_hits,
        ),
        (
            "iota_cache_misses_total",
            "Completed execution cache misses",
            metrics.cache_misses,
        ),
    ] {
        let counter = IntCounter::with_opts(opts!(name, help))?;
        counter.inc_by(value);
        registry.register(Box::new(counter))?;
    }

    for (name, help, value) in [
        (
            "iota_prompt_latency_ms_avg",
            "Average prompt latency in milliseconds",
            metrics.avg_prompt_ms,
        ),
        (
            "iota_total_latency_ms_avg",
            "Average total latency in milliseconds",
            metrics.avg_total_ms,
        ),
        (
            "iota_total_latency_ms_p95",
            "P95 total latency in milliseconds",
            metrics.p95_total_ms.map(|value| value as f64),
        ),
    ] {
        if let Some(value) = value {
            let gauge = Gauge::with_opts(opts!(name, help))?;
            gauge.set(value);
            registry.register(Box::new(gauge))?;
        }
    }

    register_histogram(
        &registry,
        "iota_prompt_latency_ms",
        "Prompt latency in milliseconds",
        &metrics.prompt_latency_ms,
    )?;
    register_histogram(
        &registry,
        "iota_init_latency_ms",
        "ACP initialization latency in milliseconds",
        &metrics.init_latency_ms,
    )?;

    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&registry.gather(), &mut buffer)?;
    println!("{}", String::from_utf8(buffer)?);
    Ok(())
}

fn register_histogram(registry: &Registry, name: &str, help: &str, values: &[u64]) -> Result<()> {
    let histogram = Histogram::with_opts(HistogramOpts::new(name, help).buckets(vec![
        50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0, 30_000.0, 60_000.0,
    ]))?;
    for value in values {
        histogram.observe(*value as f64);
    }
    registry.register(Box::new(histogram))?;
    Ok(())
}

fn parse_limit(args: &[String]) -> Option<usize> {
    args.windows(2)
        .find_map(|pair| (pair[0] == "--limit").then(|| pair[1].parse::<usize>().ok()))
        .flatten()
}

fn parse_tool_filter(args: &[String]) -> Option<&str> {
    args.windows(2).find_map(|pair| {
        matches!(pair[0].as_str(), "--tool" | "--tool-name").then_some(pair[1].as_str())
    })
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
        trace_timing: options.trace_timing,
    };
    let daemon_addr = daemon::daemon_addr();
    let response = send_prompt_autostart_daemon(&daemon_addr, &request).await?;
    if options.trace {
        print_trace_events(&response.events);
    }
    if options.trace_timing {
        print_route_timing("daemon", options.backend, response.timing.as_ref());
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
                trace_timing: false,
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
    fn parses_observability_tool_filter() {
        let args = vec![
            "tools".to_string(),
            "--limit".to_string(),
            "5".to_string(),
            "--tool".to_string(),
            "iota_memory_write".to_string(),
        ];

        assert_eq!(parse_tool_filter(&args), Some("iota_memory_write"));
    }
}
