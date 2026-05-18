use anyhow::Result;

use crate::acp;
use crate::config;
use crate::daemon;
use crate::mcp::server as mcp_server;
use crate::skill::fun;
use crate::telemetry::{self, TelemetryConfig};
use crate::tui;

mod daemon_cmd;
mod info_cmd;
mod observability_cmd;
mod run_cmd;
mod skill_cmd;

#[derive(Clone, Copy)]
enum BenchMode {
    Cold,
    Warm,
}

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
                    daemon_cmd::run_prompt_via_daemon(&options).await
                } else {
                    run_cmd::run_direct(&options).await
                };
            }
            "mcp" => match args.get(1).map(String::as_str) {
                Some("context") => return mcp_server::run_stdio(),
                Some("fun") => return fun::run_stdio(),
                _ => {
                    eprintln!("Usage: iota mcp <context|fun>");
                    std::process::exit(2);
                }
            },
            "context-mcp" => {
                return mcp_server::run_stdio();
            }
            "fun-mcp" => {
                return fun::run_stdio();
            }
            "observability" => {
                return observability_cmd::run_observability_command(&args[1..]).await;
            }
            "skill" => {
                return skill_cmd::run_skill_command(&args[1..]).await;
            }
            "__daemon" => {
                let config = config::read_config()?;
                let daemon_addr = daemon::daemon_addr();
                return daemon::run_daemon(config, &daemon_addr, acp::DEFAULT_TIMEOUT_MS, false)
                    .await;
            }
            "check" => {
                let use_daemon = daemon_cmd::has_daemon_flag(&args[1..]);
                if use_daemon {
                    daemon_cmd::warm_daemon_for_current_dir(Vec::new()).await?;
                }
                let config = config::read_config()?;
                info_cmd::print_combined_info(&config)?;
                return Ok(());
            }
            "bench" => {
                return run_benchmark(&args[1..]).await;
            }
            "bench-cold" => {
                return run_benchmark_mode(BenchMode::Cold, &args[1..]).await;
            }
            "bench-warm" => {
                return run_benchmark_mode(BenchMode::Warm, &args[1..]).await;
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

pub(super) fn print_route_timing(
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
        "Usage:\n  iota\n  iota check [--daemon|-d]\n  iota bench <cold|warm> [rounds] [--daemon|-d]\n  iota run [backend] [options] <prompt>\n  iota observability <logging|tokens|metrics|logs|trace> ...\n  iota mcp <context|fun>\n  iota context-mcp\n  iota fun-mcp\n  iota skill pull <source> [name]\n\nNotes:\n  No arguments enters the TUI.\n  check prints one combined JSON structure.\n  Add --daemon or -d to route supported commands through the local daemon; it starts silently if needed.\n\nObservability subcommands:\n  iota observability logging <recent|events> ...\n  iota observability tokens <recent|summary|export> ...\n  iota observability metrics [--prometheus]\n  iota observability logs <execution_id>   (Loki)\n  iota observability trace <trace_id>      (Jaeger)\n\nConfiguration:\n  All backend config is read from ~/.i6/nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota run --help` for run options."
    );
}

async fn run_benchmark(args: &[String]) -> Result<()> {
    let mode = match args.first().map(String::as_str) {
        Some("cold") => BenchMode::Cold,
        Some("warm") => BenchMode::Warm,
        _ => {
            eprintln!("Usage: iota bench <cold|warm> [rounds] [--daemon]");
            std::process::exit(2);
        }
    };
    run_benchmark_mode(mode, &args[1..]).await
}

async fn run_benchmark_mode(mode: BenchMode, args: &[String]) -> Result<()> {
    let config = config::read_config()?;
    let use_daemon = daemon_cmd::has_daemon_flag(args);
    let rounds = parse_rounds(args).unwrap_or(3);

    if use_daemon {
        return daemon_cmd::run_daemon_benchmark(&config, rounds).await;
    }

    match mode {
        BenchMode::Cold => daemon_cmd::run_cold_benchmark(config, rounds).await,
        BenchMode::Warm => daemon_cmd::run_warm_benchmark(config, rounds).await,
    }
}

fn parse_rounds(args: &[String]) -> Option<usize> {
    args.iter()
        .filter(|arg| !matches!(arg.as_str(), "--daemon" | "-d" | "--require-daemon"))
        .find_map(|value| value.parse::<usize>().ok())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_rounds_skips_daemon_flags() {
        let args = vec!["--daemon".to_string(), "5".to_string()];
        assert_eq!(super::parse_rounds(&args), Some(5));
    }
}
