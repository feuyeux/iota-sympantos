use anyhow::{Context, Result};

use crate::acp;
use crate::agent::{self, DaemonPromptRequest};
use crate::config::{self, NimiaConfig};
use crate::engine::IotaEngine;
use crate::tui;

pub async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(command) = args.first().map(String::as_str) {
        match command {
            "acp" => {
                let options = acp::parse_acp_args(&args[1..])?;
                if !options.show_native {
                    let request = DaemonPromptRequest {
                        backend: options.backend.to_string(),
                        cwd: options.cwd.display().to_string(),
                        prompt: options.prompt.clone(),
                        timeout_ms: Some(options.timeout_ms),
                    };
                    if let Ok(response) =
                        agent::send_prompt(agent::DEFAULT_DAEMON_ADDR, &request).await
                    {
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
                }

                let config = config::read_config()?;
                let mut engine = IotaEngine::new(
                    config,
                    options.cwd.clone(),
                    options.show_native,
                    options.timeout_ms,
                );
                let result = engine
                    .prompt_in_cwd(options.backend, options.cwd, &options.prompt)
                    .await;
                engine.shutdown().await;
                let text = result?;
                if !text.is_empty() {
                    println!("{}", text);
                }
                return Ok(());
            }
            "daemon" => {
                let config = config::read_config()?;
                return agent::run_daemon(config, agent::DEFAULT_DAEMON_ADDR, 30_000).await;
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

fn print_help() {
    println!(
        "Usage:\n  iota check\n  iota info\n  iota tui\n  iota daemon\n  iota bench-cold [rounds]\n  iota bench-warm [rounds]\n  iota acp [backend] [options] <prompt>\n\nConfiguration:\n  All backend config is read from %USERPROFILE%\\.i6\\nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota acp --help` for ACP options."
    );
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
