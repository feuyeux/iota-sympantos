use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

mod acp;

type BackendEnv = BTreeMap<String, Option<String>>;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct CommandConfig {
    command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct BackendConfig {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acp: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    update: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    env: Option<BackendEnv>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct NimiaConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claude_code: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    codex: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gemini: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opencode: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hermes: Option<BackendConfig>,
}

fn default_enabled() -> bool {
    true
}

fn get_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".i6").join("nimia.yaml"))
}

fn read_config() -> Result<NimiaConfig> {
    let config_path = get_config_path()?;
    if !config_path.exists() {
        bail!("Backend config not found: {}", config_path.display());
    }
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", config_path.display()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(command) = args.first().map(String::as_str) {
        match command {
            "acp" => {
                let mut options = acp::parse_acp_args(&args[1..])?;
                let config = read_config()?;
                options = prepare_acp_options(options, &config)?;
                return acp::run_acp_prompt(options).await;
            }
            "check" => {
                let config = read_config()?;
                print_config_summary(&config);
                return Ok(());
            }
            "info" => {
                let config = read_config()?;
                return print_backend_info(&config).await;
            }
            "tui" => {
                let config = read_config()?;
                return run_tui(&config).await;
            }
            "bench-warm" => {
                let config = read_config()?;
                let rounds = args
                    .get(1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(3);
                return run_warm_benchmark(&config, rounds).await;
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

    let config = read_config()?;
    println!("iota-sympantos config: {}", get_config_path()?.display());
    print_config_summary(&config);
    Ok(())
}

fn print_help() {
    println!(
        "Usage:\n  iota-sympantos check\n  iota-sympantos info\n  iota-sympantos tui\n  iota-sympantos bench-warm [rounds]\n  iota-sympantos acp [backend] [options] <prompt>\n\nConfiguration:\n  All backend config is read from %USERPROFILE%\\.i6\\nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota-sympantos acp --help` for ACP options."
    );
}

fn print_config_summary(config: &NimiaConfig) {
    for backend in [
        acp::AcpBackend::ClaudeCode,
        acp::AcpBackend::Codex,
        acp::AcpBackend::Gemini,
        acp::AcpBackend::Hermes,
        acp::AcpBackend::OpenCode,
    ] {
        let name = backend.to_string();
        let section = backend_config(config, backend);
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
        println!("{}: {} (update: {})", name, status, update);
    }
}

fn expand_home_path(value: &str) -> Result<String> {
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        if value == "~" {
            return Ok(home.display().to_string());
        }
        return Ok(home.join(&value[2..]).display().to_string());
    }
    Ok(value.to_string())
}

fn normalize_command(command: &str) -> String {
    if cfg!(windows) && command.eq_ignore_ascii_case("npx") {
        "npx.cmd".to_string()
    } else {
        command.to_string()
    }
}

fn normalized_acp_command(acp: &CommandConfig) -> (String, Vec<String>) {
    (normalize_command(&acp.command), acp.args.clone())
}
async fn print_backend_info(config: &NimiaConfig) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    println!("| Backend | Enabled | Tool | Version | Model |");
    println!("|---|---:|---|---|---|");

    for backend in acp::ALL_BACKENDS {
        let Some(section) = backend_config(config, backend) else {
            println!("| `{}` | no | missing section | - | - |", backend);
            continue;
        };
        if !section.enabled {
            println!("| `{}` | no | disabled | - | - |", backend);
            continue;
        }
        let Some(acp_config) = section.acp.as_ref() else {
            println!("| `{}` | yes | missing acp | - | - |", backend);
            continue;
        };
        if acp_config.command.trim().is_empty() {
            println!("| `{}` | yes | missing acp.command | - | - |", backend);
            continue;
        }

        let env = backend_process_env(backend, section);
        let cleanup_dirs: Vec<PathBuf> = Vec::new();

        let result = acp::fetch_acp_info(acp::AcpInfoOptions {
            backend,
            cwd: cwd.clone(),
            env,
            command_override: Some(normalized_acp_command(acp_config)),
            timeout_ms: 20_000,
        })
        .await;

        for dir in cleanup_dirs {
            let _ = fs::remove_dir_all(dir);
        }

        match result {
            Ok(info) => println!(
                "| `{}` | yes | {} | {} | {} |",
                backend,
                table_cell(info.agent_name.as_deref().unwrap_or("unknown")),
                table_cell(info.agent_version.as_deref().unwrap_or("unknown")),
                table_cell(info.current_model.as_deref().unwrap_or("unknown"))
            ),
            Err(err) => println!(
                "| `{}` | yes | error | error | {} |",
                backend,
                table_cell(&err.to_string())
            ),
        }
    }

    Ok(())
}

fn table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}
async fn warm_enabled_backends(
    config: &NimiaConfig,
    cwd: PathBuf,
    show_native: bool,
    timeout_ms: u64,
) -> Result<Vec<(acp::AcpClient, Vec<PathBuf>)>> {
    let mut clients = Vec::new();

    for backend in acp::ALL_BACKENDS {
        let Some(section) = backend_config(config, backend) else {
            continue;
        };
        if !section.enabled {
            continue;
        }
        if backend == acp::AcpBackend::ClaudeCode {
            eprintln!(
                "Skipping {} in TUI warm mode: ACP adapter does not support reliable warm prompt reuse",
                backend
            );
            continue;
        }
        let Some(acp_config) = section.acp.as_ref() else {
            eprintln!("Skipping {}: missing acp config", backend);
            continue;
        };
        if acp_config.command.trim().is_empty() {
            eprintln!("Skipping {}: missing acp.command", backend);
            continue;
        }

        let env = backend_process_env(backend, section);
        let cleanup_dirs = Vec::new();

        match acp::AcpClient::start(
            backend,
            cwd.clone(),
            env,
            Some(normalized_acp_command(acp_config)),
            show_native,
            timeout_ms,
        )
        .await
        {
            Ok(client) => clients.push((client, cleanup_dirs)),
            Err(err) => eprintln!("Failed to warm {}: {}", backend, err),
        }
    }

    Ok(clients)
}

async fn shutdown_clients(clients: Vec<(acp::AcpClient, Vec<PathBuf>)>) {
    for (client, cleanup_dirs) in clients {
        client.shutdown().await;
        for dir in cleanup_dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

async fn run_warm_benchmark(config: &NimiaConfig, rounds: usize) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut clients = warm_enabled_backends(config, cwd, false, 30_000).await?;
    println!("backend,round,latency_ms,status");

    for round in 1..=rounds {
        for (client, _) in clients.iter_mut() {
            let backend = client.backend();
            let started = std::time::Instant::now();
            let result = client.prompt("ping").await;
            let elapsed = started.elapsed().as_millis();
            let status = if result.is_ok() { "ok" } else { "error" };
            println!("{},{},{},{}", backend, round, elapsed, status);
            if let Err(err) = result {
                eprintln!("{} round {} failed: {}", backend, round, err);
            }
        }
    }

    shutdown_clients(clients).await;
    Ok(())
}
async fn run_tui(config: &NimiaConfig) -> Result<()> {
    use std::io::{self, Write as _};

    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut clients = warm_enabled_backends(config, cwd, false, 20_000).await?;

    println!("Warm ACP backends: {}", clients.len());
    println!("Enter '<backend> <prompt>' or 'exit'. Example: codex ping");

    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
            break;
        }
        let Some((backend_name, prompt)) = line.split_once(char::is_whitespace) else {
            eprintln!("Expected '<backend> <prompt>'");
            continue;
        };
        let backend = match acp::AcpBackend::parse(backend_name) {
            Ok(backend) => backend,
            Err(err) => {
                eprintln!("{}", err);
                continue;
            }
        };
        let Some((client, _)) = clients
            .iter_mut()
            .find(|(client, _)| client.backend() == backend)
        else {
            eprintln!("Backend {} is not warmed/enabled", backend);
            continue;
        };
        if let Err(err) = client.prompt(prompt.trim()).await {
            eprintln!("{}", err);
        }
    }

    shutdown_clients(clients).await;
    Ok(())
}
fn prepare_acp_options(
    mut options: acp::AcpRunOptions,
    config: &NimiaConfig,
) -> Result<acp::AcpRunOptions> {
    let config_path = get_config_path()?;
    let section = backend_config(config, options.backend).with_context(|| {
        format!(
            "Missing backend section for {} in {}",
            options.backend,
            config_path.display()
        )
    })?;
    if !section.enabled {
        bail!(
            "Backend {} is disabled in {}",
            options.backend,
            config_path.display()
        );
    }
    let acp = section.acp.as_ref().with_context(|| {
        format!(
            "Missing acp config for backend {} in {}",
            options.backend,
            config_path.display()
        )
    })?;
    if acp.command.trim().is_empty() {
        bail!(
            "Missing acp.command for backend {} in {}",
            options.backend,
            config_path.display()
        );
    }

    options.env = backend_process_env(options.backend, section);
    options.command_override = Some(normalized_acp_command(acp));

    Ok(options)
}
fn backend_config(config: &NimiaConfig, backend: acp::AcpBackend) -> Option<&BackendConfig> {
    match backend {
        acp::AcpBackend::ClaudeCode => config.claude_code.as_ref(),
        acp::AcpBackend::Codex => config.codex.as_ref(),
        acp::AcpBackend::Gemini => config.gemini.as_ref(),
        acp::AcpBackend::Hermes => config.hermes.as_ref(),
        acp::AcpBackend::OpenCode => config.opencode.as_ref(),
    }
}

fn backend_process_env(
    backend: acp::AcpBackend,
    section: &BackendConfig,
) -> BTreeMap<String, String> {
    let empty_env = BackendEnv::new();
    let env = section.env.as_ref().unwrap_or(&empty_env);
    let mut process_env = literal_env(env);

    if let Some(home) = section.home.as_deref().filter(|value| !value.is_empty()) {
        if let Some(env_key) = backend_home_env_key(backend) {
            process_env
                .entry(env_key.to_string())
                .or_insert(expand_home_path(home).unwrap_or_else(|_| home.to_string()));
        }
    }

    match backend {
        acp::AcpBackend::ClaudeCode => {
            if let Some(value) = env_value(env, "api_key") {
                process_env
                    .entry("ANTHROPIC_AUTH_TOKEN".to_string())
                    .or_insert_with(|| value.clone());
                process_env
                    .entry("ANTHROPIC_API_KEY".to_string())
                    .or_insert(value);
            }
            insert_env(env, &mut process_env, "base_url", "ANTHROPIC_BASE_URL");
            insert_env(env, &mut process_env, "model", "ANTHROPIC_MODEL");
        }
        acp::AcpBackend::Codex => {
            if let Some(value) = env_value(env, "api_key") {
                process_env
                    .entry("ROUTER_API_KEY".to_string())
                    .or_insert_with(|| value.clone());
                process_env
                    .entry("OPENAI_API_KEY".to_string())
                    .or_insert(value);
            }
            insert_env(env, &mut process_env, "base_url", "OPENAI_BASE_URL");
            insert_env(env, &mut process_env, "model", "OPENAI_MODEL");
        }
        acp::AcpBackend::Gemini => {
            insert_env(env, &mut process_env, "api_key", "GEMINI_API_KEY");
            insert_env(env, &mut process_env, "model", "GEMINI_MODEL");
        }
        acp::AcpBackend::Hermes => {
            // Resolve provider first — needed to map api_key to the correct env var.
            let provider = env_value(env, "provider")
                .or_else(|| env_value(env, "base_url").map(|u| infer_hermes_provider(&u).to_string()))
                .unwrap_or_else(|| "minimax-cn".to_string());
            let api_key = env_value(env, "api_key").unwrap_or_default();
            let base_url = env_value(env, "base_url")
                .unwrap_or_else(|| default_hermes_base_url(&provider).to_string());

            // Set provider-native env vars that Hermes actually reads.
            render_hermes_provider_env(&mut process_env, &provider, &api_key, &base_url);
            process_env
                .entry("HERMES_INFERENCE_PROVIDER".to_string())
                .or_insert(provider);
            if let Some(model) = env_value(env, "model") {
                process_env
                    .entry("HERMES_MODEL".to_string())
                    .or_insert(model);
            }
        }
        acp::AcpBackend::OpenCode => {
            insert_env(env, &mut process_env, "model", "OPENCODE_MODEL");
        }
    }

    process_env
}

fn backend_home_env_key(backend: acp::AcpBackend) -> Option<&'static str> {
    match backend {
        acp::AcpBackend::ClaudeCode => Some("CLAUDE_CONFIG_DIR"),
        acp::AcpBackend::Codex => Some("CODEX_HOME"),
        acp::AcpBackend::Gemini => Some("GEMINI_CONFIG_DIR"),
        // Hermes manages its own home via get_hermes_home(); overriding
        // HERMES_HOME with a bare directory breaks its config/state expectations.
        acp::AcpBackend::Hermes => None,
        acp::AcpBackend::OpenCode => Some("OPENCODE_CONFIG_DIR"),
    }
}

fn literal_env(env: &BackendEnv) -> BTreeMap<String, String> {
    env.iter()
        .filter_map(|(key, value)| {
            let value = value.as_ref()?.trim();
            if value.is_empty() || !is_literal_env_key(key) {
                return None;
            }
            Some((key.clone(), value.to_string()))
        })
        .collect()
}

fn is_literal_env_key(key: &str) -> bool {
    key.chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn env_value(env: &BackendEnv, key: &str) -> Option<String> {
    env.get(key)
        .and_then(|value| value.as_ref())
        .filter(|value| !value.is_empty())
        .cloned()
}

fn insert_env(
    source: &BackendEnv,
    target: &mut BTreeMap<String, String>,
    generic_key: &str,
    process_key: &str,
) {
    if let Some(value) = env_value(source, generic_key) {
        target.entry(process_key.to_string()).or_insert(value);
    }
}

fn infer_hermes_provider(base_url: &str) -> &'static str {
    let normalized = base_url.to_lowercase();
    if normalized.contains("minimaxi.com") {
        "minimax-cn"
    } else if normalized.contains("minimax.io") {
        "minimax"
    } else if normalized.contains("anthropic.com") {
        "anthropic"
    } else {
        "custom"
    }
}

fn default_hermes_base_url(provider: &str) -> &'static str {
    match provider {
        "minimax-cn" => "https://api.minimaxi.com/anthropic",
        "minimax" => "https://api.minimax.io/anthropic",
        "anthropic" => "https://api.anthropic.com",
        _ => "",
    }
}

fn render_hermes_provider_env(
    env: &mut BTreeMap<String, String>,
    provider: &str,
    api_key: &str,
    base_url: &str,
) {
    match provider {
        "minimax-cn" => {
            insert_non_empty(env, "MINIMAX_CN_API_KEY", api_key);
            insert_non_empty(env, "MINIMAX_CN_BASE_URL", base_url);
        }
        "minimax" => {
            insert_non_empty(env, "MINIMAX_API_KEY", api_key);
            insert_non_empty(env, "MINIMAX_BASE_URL", base_url);
        }
        "anthropic" => {
            insert_non_empty(env, "ANTHROPIC_API_KEY", api_key);
            insert_non_empty(env, "ANTHROPIC_TOKEN", api_key);
            insert_non_empty(env, "ANTHROPIC_BASE_URL", base_url);
        }
        _ => {
            insert_non_empty(env, "OPENAI_API_KEY", api_key);
            insert_non_empty(env, "OPENAI_BASE_URL", base_url);
        }
    }
}

fn insert_non_empty(env: &mut BTreeMap<String, String>, key: &str, value: &str) {
    if !value.is_empty() {
        env.insert(key.to_string(), value.to_string());
    }
}
