use anyhow::{Context, Result};
use serde::Serialize;
use std::process::Stdio;

use crate::acp;
use crate::agent::{self, DaemonPromptRequest};
use crate::config::{self, NimiaConfig};
use crate::engine::IotaEngine;
use crate::memory::MemoryStore;
use crate::skills::SkillRegistry;
use crate::{context_mcp, fun_mcp, native_materializer, skill_registry_cache, tui};

pub async fn run() -> Result<()> {
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
                    let mut engine =
                        IotaEngine::new(config, options.show_native, options.timeout_ms);
                    let result = engine
                        .prompt_in_cwd_timed(options.backend, options.cwd, &options.prompt)
                        .await;
                    engine.shutdown().await;
                    let output = result?;
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
                return context_mcp::run_stdio();
            }
            "fun-mcp" => {
                return fun_mcp::run_stdio();
            }
            "native-materialize" => {
                return run_native_materialize(&args[1..]);
            }
            "skill" => {
                return run_skill_command(&args[1..]).await;
            }
            "__daemon" => {
                let config = config::read_config()?;
                let daemon_addr = agent::daemon_addr();
                return agent::run_daemon(config, &daemon_addr, acp::DEFAULT_TIMEOUT_MS, false)
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
        "Usage:\n  iota\n  iota check [--daemon|-d]\n  iota bench-cold [rounds] [--daemon|-d]\n  iota bench-warm [rounds] [--daemon|-d]\n  iota run [backend] [options] <prompt>\n  iota context-mcp\n  iota fun-mcp\n  iota native-materialize --dry-run <path> <content>\n  iota skill pull <source> [name]\n\nNotes:\n  No arguments enters the TUI.\n  check prints one combined JSON structure.\n  Add --daemon or -d to route supported commands through the local daemon; it starts silently if needed.\n\nConfiguration:\n  All backend config is read from ~/.i6/nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota run --help` for run options."
    );
}

async fn run_skill_command(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("pull") => {
            let source = args
                .get(1)
                .context("skill pull requires a source path or URL")?;
            let name = args.get(2).map(String::as_str);
            let path = skill_registry_cache::pull_skill(source, name).await?;
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
        let previews = native_materializer::dry_run_backend_projection(
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
                .map(|preview| native_materializer::apply(&preview.path, &preview.content))
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
        native_materializer::backend_memory_path(backend, &workspace)?
            .context("native materialization for this backend is deferred")?
    } else {
        positional
            .first()
            .map(std::path::PathBuf::from)
            .context("native-materialize requires a target path or --backend <name>")?
    };
    if dry_run {
        let preview = native_materializer::dry_run(&path, body)?;
        println!(
            "{}",
            serde_json::json!({
                "path": preview.path.display().to_string(),
                "changed": preview.changed,
                "content": preview.content,
            })
        );
    } else {
        let changed = native_materializer::apply(&path, body)?;
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
    let daemon_addr = agent::daemon_addr();
    let response = send_prompt_autostart_daemon(&daemon_addr, &request).await?;
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
) -> Result<agent::DaemonPromptResponse> {
    match agent::send_prompt(daemon_addr, request).await {
        Ok(response) => Ok(response),
        Err(first_error) => {
            start_daemon_silently()?;
            wait_for_daemon(daemon_addr).await.with_context(|| {
                format!(
                    "Failed to start daemon at {} after initial connection error: {}",
                    daemon_addr, first_error
                )
            })?;
            agent::send_prompt(daemon_addr, request).await
        }
    }
}

async fn warm_daemon_for_current_dir(backends: Vec<String>) -> Result<()> {
    let request = agent::DaemonWarmRequest {
        request_type: "warm".to_string(),
        cwd: std::env::current_dir()
            .context("Failed to get current directory")?
            .display()
            .to_string(),
        backends,
    };
    let daemon_addr = agent::daemon_addr();
    let response = match agent::send_warm(&daemon_addr, &request).await {
        Ok(response) => response,
        Err(first_error) => {
            start_daemon_silently()?;
            wait_for_daemon(&daemon_addr).await.with_context(|| {
                format!(
                    "Failed to start daemon at {} after initial warm error: {}",
                    daemon_addr, first_error
                )
            })?;
            agent::send_warm(&daemon_addr, &request).await?
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
    std::process::Command::new(exe)
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start daemon process")?;
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
    update_command: String,
    version_probe: String,
    model: String,
}

fn print_combined_info(config: &NimiaConfig) -> Result<()> {
    let info = CombinedInfo {
        config_path: config::config_path()?.display().to_string(),
        daemon_addr: agent::daemon_addr(),
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
    let update_command = section
        .and_then(|section| section.update.as_ref())
        .map(config::command_label)
        .unwrap_or_else(|| "-".to_string());
    let model = section
        .map(config::configured_model)
        .unwrap_or(None)
        .unwrap_or_else(|| "-".to_string());

    BackendInfo {
        backend: backend.to_string(),
        enabled,
        check_status: check_status.to_string(),
        acp_command,
        version_probe: update_command.clone(),
        update_command,
        model,
    }
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
    let daemon_addr = agent::daemon_addr();
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
