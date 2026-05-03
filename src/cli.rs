use anyhow::{Context, Result};

use crate::acp;
use crate::agent::{self, DaemonPromptRequest, DaemonWarmRequest};
use crate::config::{self, NimiaConfig};
use crate::engine::IotaEngine;
use crate::tui;

pub async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(command) = args.first().map(String::as_str) {
        match command {
            "acp" => {
                let options = acp::parse_acp_args(&args[1..])?;
                let daemon_addr = agent::daemon_addr();
                if !options.show_native {
                    let request = DaemonPromptRequest {
                        backend: options.backend.to_string(),
                        cwd: options.cwd.display().to_string(),
                        prompt: options.prompt.clone(),
                        timeout_ms: Some(options.timeout_ms),
                        trace_timing: options.trace_timing,
                    };
                    match agent::send_prompt(&daemon_addr, &request).await {
                        Ok(response) => {
                            if options.trace_timing {
                                print_route_timing(
                                    "daemon",
                                    options.backend,
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
                        }
                        Err(err) => {
                            if options.require_daemon {
                                anyhow::bail!(
                                    "Daemon is required but unavailable at {}: {}",
                                    daemon_addr,
                                    err
                                );
                            }
                            eprintln!(
                                "[iota acp] daemon unavailable at {}; falling back to in-process engine: {}",
                                daemon_addr, err
                            );
                            if options.trace_timing {
                                print_fallback_route(options.backend, &err.to_string());
                            }
                        }
                    }
                }

                let config = config::read_config()?;
                let mut engine = IotaEngine::new(
                    config,
                    options.cwd.clone(),
                    options.show_native,
                    options.timeout_ms,
                );
                let result = engine
                    .prompt_in_cwd_timed(options.backend, options.cwd, &options.prompt)
                    .await;
                engine.shutdown().await;
                let output = result?;
                if options.trace_timing {
                    print_route_timing("fallback", options.backend, Some(&output.timing));
                }
                let text = output.text;
                if !text.is_empty() {
                    println!("{}", text);
                }
                return Ok(());
            }
            "daemon" => {
                let config = config::read_config()?;
                let warm_on_start = args[1..].iter().any(|arg| arg == "--warm");
                let daemon_addr = agent::daemon_addr();
                return agent::run_daemon(config, &daemon_addr, 30_000, warm_on_start).await;
            }
            "warm" => {
                return run_daemon_warm(&args[1..]).await;
            }
            "check" => {
                let config = config::read_config()?;
                print_config_summary(&config);
                return Ok(());
            }
            "info" => {
                let config = config::read_config()?;
                print_backend_info(&config);
                return Ok(());
            }
            "tui" => {
                let config = config::read_config()?;
                return tui::run(config).await;
            }
            "bench-cold" => {
                let config = config::read_config()?;
                let rounds = args
                    .get(1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(3);
                return run_cold_benchmark(config, rounds).await;
            }
            "bench-warm" => {
                let config = config::read_config()?;
                let rounds = args
                    .get(1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(3);
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
    println!("iota config: {}", config::config_path()?.display());
    print_config_summary(&config);
    Ok(())
}

fn print_route_timing(
    route: &str,
    backend: acp::AcpBackend,
    timing: Option<&acp::AcpPromptTiming>,
) {
    eprintln!(
        "[iota acp timing] {}",
        serde_json::json!({
            "route": route,
            "daemon_hit": route == "daemon",
            "fallback": route == "fallback",
            "backend": backend.to_string(),
            "timing": timing,
        })
    );
}

fn print_fallback_route(backend: acp::AcpBackend, error: &str) {
    eprintln!(
        "[iota acp timing] {}",
        serde_json::json!({
            "route": "fallback",
            "daemon_hit": false,
            "fallback": true,
            "backend": backend.to_string(),
            "daemon_error": error,
        })
    );
}

fn print_help() {
    println!(
        "Usage:\n  iota check\n  iota info\n  iota tui\n  iota daemon [--warm]\n  iota warm [--cwd <path>] [backend ...]\n  iota bench-cold [rounds]\n  iota bench-warm [rounds]\n  iota acp [backend] [options] <prompt>\n\nConfiguration:\n  All backend config is read from %USERPROFILE%\\.i6\\nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota acp --help` for ACP options."
    );
}

async fn run_daemon_warm(args: &[String]) -> Result<()> {
    let mut cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut backends = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--cwd" => {
                index += 1;
                let value = args.get(index).context("--cwd requires a path")?;
                cwd = value.into();
            }
            "-h" | "--help" => {
                println!(
                    "Usage:\n  iota warm [--cwd <path>] [backend ...]\n\nExamples:\n  iota warm\n  iota warm codex claude-code\n  iota warm --cwd D:\\\\coding\\\\creative\\\\iota-sympantos codex"
                );
                return Ok(());
            }
            value => backends.push(value.to_string()),
        }
        index += 1;
    }

    let request = DaemonWarmRequest {
        request_type: "warm".to_string(),
        cwd: cwd.display().to_string(),
        backends,
    };
    let daemon_addr = agent::daemon_addr();
    let response = agent::send_warm(&daemon_addr, &request).await?;
    if response.ok {
        println!("warmed {} backend(s)", response.warmed.unwrap_or(0));
        return Ok(());
    }
    if let Some(error) = response.error {
        anyhow::bail!(error);
    }
    anyhow::bail!("Daemon warm request failed without an error message")
}

fn print_config_summary(config: &NimiaConfig) {
    for backend in acp::ALL_BACKENDS {
        let section = config::backend_config(config, backend);
        let status = match section {
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
        let update = section
            .and_then(|section| section.update.as_ref())
            .map(|update| update.command.as_str())
            .unwrap_or("-");
        println!("{}: {} (update: {})", backend, status, update);
    }
}

fn print_backend_info(config: &NimiaConfig) {
    println!("| Backend | Enabled | Tool | Version | Model |");
    println!("|---|---:|---|---|---|");
    for backend in acp::ALL_BACKENDS {
        let Some(section) = config::backend_config(config, backend) else {
            println!("| `{}` | no | missing section | - | - |", backend);
            continue;
        };
        if !section.enabled {
            println!(
                "| `{}` | no | disabled | - | {} |",
                backend,
                table_cell(&config::configured_model(section).unwrap_or_else(|| "-".to_string()))
            );
            continue;
        }
        let tool = section
            .acp
            .as_ref()
            .map(config::command_label)
            .unwrap_or_else(|| "missing acp".to_string());
        let version_probe = section
            .update
            .as_ref()
            .map(config::command_label)
            .unwrap_or_else(|| "-".to_string());
        let model = config::configured_model(section).unwrap_or_else(|| "-".to_string());
        println!(
            "| `{}` | yes | {} | {} | {} |",
            backend,
            table_cell(&tool),
            table_cell(&version_probe),
            table_cell(&model)
        );
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
            let mut engine = IotaEngine::new(config.clone(), cwd.clone(), false, 30_000);
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

async fn run_warm_benchmark(config: NimiaConfig, rounds: usize) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut engine = IotaEngine::new(config, cwd.clone(), false, 30_000);
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

fn table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}
