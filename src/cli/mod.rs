use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::process::Stdio;

use crate::acp;
use crate::config::{self, NimiaConfig};
use crate::context::server as context_server;
use crate::daemon::{self, DaemonPromptRequest};
use crate::engine::IotaEngine;
use crate::skill::SkillRegistry;
use crate::skill::fun_server;
use crate::store::memory::MemoryStore;
use crate::telemetry::{self, TelemetryConfig};
use crate::{native, skill, tui};

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
                return if options.use_daemon {
                    run_prompt_via_daemon(&options).await
                } else {
                    let config = config::read_config()?;
                    let mut engine = IotaEngine::create_session(
                        config,
                        options.show_native,
                        options.timeout_ms,
                        None,
                    );
                    if options.multi_backend {
                        // Run all backends and collect results
                        use tokio::spawn;
                        let mut handles = Vec::new();
                        for backend in acp::ALL_BACKENDS {
                            let backend_name = backend.to_string();
                            let config = config::read_config()?;
                            let mut engine = IotaEngine::create_session(
                                config,
                                options.show_native,
                                options.timeout_ms,
                                None,
                            );
                            let cwd = options.cwd.clone();
                            let prompt = options.prompt.clone();
                            let timing = options.timing;
                            handles.push(spawn(async move {
                                let result = engine.run_with_timing(backend, cwd, &prompt).await;
                                engine.shutdown().await;
                                (backend_name, result, timing)
                            }));
                        }
                        for handle in handles {
                            let (backend_name, result, timing) = handle.await?;
                            match result {
                                Ok(output) => {
                                    if timing {
                                        print_route_timing(
                                            "direct",
                                            acp::AcpBackend::parse(&backend_name)
                                                .unwrap_or(acp::AcpBackend::Codex),
                                            Some(&output.timing),
                                        );
                                    }
                                    let text = output.text;
                                    if !text.is_empty() {
                                        println!("[{}] {}", backend_name, text);
                                    }
                                }
                                Err(e) => eprintln!("[{}] Error: {}", backend_name, e),
                            }
                        }
                    } else {
                        // existing single backend logic
                        let result = engine
                            .run_with_timing(options.backend, options.cwd, &options.prompt)
                            .await;
                        engine.shutdown().await;
                        let output = result?;
                        if options.log_events {
                            for event in &output.events {
                                eprintln!("{}", serde_json::to_string(event).unwrap_or_default());
                            }
                        }
                        if options.timing {
                            print_route_timing("direct", options.backend, Some(&output.timing));
                        }
                        let text = output.text;
                        if !text.is_empty() {
                            println!("{}", text);
                        }
                    }
                    Ok(())
                };
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

fn print_help() {
    println!(
        "Usage:\n  iota\n  iota check [--daemon|-d]\n  iota bench-cold [rounds] [--daemon|-d]\n  iota bench-warm [rounds] [--daemon|-d]\n  iota run [backend] [options] <prompt>\n  iota logs <execution_id>\n  iota trace <trace_id>\n  iota context-mcp\n  iota fun-mcp\n  iota native-materialize --dry-run <path> <content>\n  iota skill pull <source> [name]\n\nNotes:\n  No arguments enters the TUI.\n  check prints one combined JSON structure.\n  Add --daemon or -d to route supported commands through the local daemon; it starts silently if needed.\n\nConfiguration:\n  All backend config is read from ~/.i6/nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota run --help` for run options."
    );
}

async fn run_logs_command(args: &[String]) -> Result<()> {
    let execution_id = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("Usage: iota logs <execution_id>"))?;
    let loki_url =
        std::env::var("IOTA_LOKI_URL").unwrap_or_else(|_| "http://localhost:3100".to_string());
    let query = format!(r#"{{iota_execution_id="{}"}}"#, execution_id);
    let url = format!(
        "{}/loki/api/v1/query_range?query={}&limit=1000",
        loki_url,
        urlencoding::encode(&query)
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Loki at {}", loki_url))?;
    if !resp.status().is_success() {
        bail!("Loki query failed with status {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await?;
    if let Some(results) = body["data"]["result"].as_array() {
        for stream in results {
            if let Some(values) = stream["values"].as_array() {
                for entry in values {
                    if let Some(arr) = entry.as_array() {
                        if arr.len() >= 2 {
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

async fn run_trace_command(args: &[String]) -> Result<()> {
    let trace_id = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("Usage: iota trace <trace_id>"))?;
    let jaeger_url =
        std::env::var("IOTA_JAEGER_URL").unwrap_or_else(|_| "http://localhost:16686".to_string());
    let url = format!("{}/api/traces/{}", jaeger_url, trace_id);
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Jaeger at {}", jaeger_url))?;
    if !resp.status().is_success() {
        bail!("Jaeger query failed with status {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await?;
    if let Some(traces) = body["data"].as_array() {
        for trace in traces {
            if let Some(spans) = trace["spans"].as_array() {
                for span in spans {
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
                    println!("{}├── {} ({}ms)", indent, name, duration_ms);
                }
            }
        }
    } else {
        println!("No trace found for {}", trace_id);
    }
    Ok(())
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
    let child = std::process::Command::new(exe)
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
        let mut child = child;
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
            let result = engine.run_prompt_text(backend, cwd.clone(), "ping").await;
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
    engine.warm_all_enabled_backends(cwd.clone()).await?;

    println!("backend,round,latency_ms,status");
    for round in 1..=rounds {
        for backend in acp::ALL_BACKENDS {
            if !engine.has_warm_client(backend, &cwd) {
                continue;
            }
            let started = std::time::Instant::now();
            let result = engine.run_prompt_text(backend, cwd.clone(), "ping").await;
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
}
