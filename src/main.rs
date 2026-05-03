use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod acp;

type BackendEnv = BTreeMap<String, Option<String>>;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct AcpCommandConfig {
    command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct BackendConfig {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acp: Option<AcpCommandConfig>,
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
            "tui" => {
                let config = read_config()?;
                return run_tui(&config).await;
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
        "Usage:\n  iota-sympantos check\n  iota-sympantos tui\n  iota-sympantos acp [backend] [options] <prompt>\n\nConfiguration:\n  All backend config is read from %USERPROFILE%\\.i6\\nimia.yaml.\n  No external project config, network overlay, or auto-discovery is used.\n\nRun `iota-sympantos acp --help` for ACP options."
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
        println!("{}: {}", name, status);
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

fn normalized_acp_command(acp: &AcpCommandConfig) -> (String, Vec<String>) {
    (normalize_command(&acp.command), acp.args.clone())
}
async fn run_tui(config: &NimiaConfig) -> Result<()> {
    use std::io::{self, Write as _};

    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut clients = Vec::new();

    for backend in acp::ALL_BACKENDS {
        let Some(section) = backend_config(config, backend) else {
            continue;
        };
        if !section.enabled {
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

        let mut env = backend_process_env(backend, section);
        let mut cleanup_dirs = Vec::new();
        if backend == acp::AcpBackend::Hermes {
            if let Some(cleanup_dir) = prepare_hermes_home(&mut env)? {
                cleanup_dirs.push(cleanup_dir);
            }
        }

        match acp::AcpClient::start(
            backend,
            cwd.clone(),
            env,
            Some(normalized_acp_command(acp_config)),
            false,
            20_000,
        )
        .await
        {
            Ok(client) => clients.push((client, cleanup_dirs)),
            Err(err) => eprintln!("Failed to warm {}: {}", backend, err),
        }
    }

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

    for (client, cleanup_dirs) in clients {
        client.shutdown().await;
        for dir in cleanup_dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }

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

    if options.backend == acp::AcpBackend::Hermes {
        if let Some(cleanup_dir) = prepare_hermes_home(&mut options.env)? {
            options.cleanup_dirs.push(cleanup_dir);
        }
    }

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
        process_env
            .entry(backend_home_env_key(backend).to_string())
            .or_insert(expand_home_path(home).unwrap_or_else(|_| home.to_string()));
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
            if let Some(value) = env_value(env, "api_key") {
                process_env
                    .entry("HERMES_API_KEY".to_string())
                    .or_insert_with(|| value.clone());
                process_env
                    .entry("HERMES_AUTH_TOKEN".to_string())
                    .or_insert(value);
            }
            insert_env(env, &mut process_env, "base_url", "HERMES_BASE_URL");
            insert_env(env, &mut process_env, "model", "HERMES_MODEL");
            insert_env(env, &mut process_env, "provider", "HERMES_PROVIDER");
        }
        acp::AcpBackend::OpenCode => {
            insert_env(env, &mut process_env, "model", "OPENCODE_MODEL");
        }
    }

    process_env
}

fn backend_home_env_key(backend: acp::AcpBackend) -> &'static str {
    match backend {
        acp::AcpBackend::ClaudeCode => "CLAUDE_CONFIG_DIR",
        acp::AcpBackend::Codex => "CODEX_HOME",
        acp::AcpBackend::Gemini => "GEMINI_CONFIG_DIR",
        acp::AcpBackend::Hermes => "HERMES_HOME",
        acp::AcpBackend::OpenCode => "OPENCODE_CONFIG_DIR",
    }
}

fn prepare_hermes_home(env: &mut BTreeMap<String, String>) -> Result<Option<PathBuf>> {
    let api_key = first_non_empty(env, &["HERMES_API_KEY", "HERMES_AUTH_TOKEN"]);
    let base_url = first_non_empty(env, &["HERMES_BASE_URL", "HERMES_ENDPOINT"]);
    let model = first_non_empty(env, &["HERMES_MODEL", "HERMES_DEFAULT_MODEL"]);
    let explicit_provider = first_non_empty(env, &["HERMES_PROVIDER", "HERMES_INFERENCE_PROVIDER"]);

    if api_key.is_none() && base_url.is_none() && model.is_none() && explicit_provider.is_none() {
        return Ok(None);
    }

    let base_url = base_url.unwrap_or_default();
    let provider =
        explicit_provider.unwrap_or_else(|| infer_hermes_provider(&base_url).to_string());
    let model = model.unwrap_or_else(|| "MiniMax-M2.7".to_string());
    let base_url = if base_url.is_empty() {
        default_hermes_base_url(&provider).to_string()
    } else {
        base_url
    };
    let api_key = api_key.unwrap_or_default();

    let hermes_home = if let Some(home) = env.get("HERMES_HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(home)
    } else {
        unique_temp_dir("iota-sympantos-hermes")?
    };
    let cleanup = !env.contains_key("HERMES_HOME");

    write_hermes_config(&hermes_home, &provider, &model, &base_url)?;
    env.insert("HERMES_HOME".to_string(), hermes_home.display().to_string());
    env.insert("HERMES_INFERENCE_PROVIDER".to_string(), provider.clone());
    env.insert("HERMES_MODEL".to_string(), model);
    render_hermes_provider_env(env, &provider, &api_key, &base_url);

    Ok(if cleanup { Some(hermes_home) } else { None })
}

fn unique_temp_dir(prefix: &str) -> Result<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before UNIX_EPOCH")?
        .as_millis();
    let path = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), now));
    fs::create_dir_all(&path).context("Failed to create temporary directory")?;
    Ok(path)
}

fn write_hermes_config(path: &Path, provider: &str, model: &str, base_url: &str) -> Result<()> {
    fs::create_dir_all(path).context("Failed to create Hermes home")?;
    let config = serde_json::json!({
        "model": {
            "default": model,
            "provider": provider,
            "base_url": base_url,
        },
        "toolsets": ["hermes-acp"],
        "terminal": {
            "backend": "local",
            "cwd": ".",
        },
    });
    let content = serde_yaml::to_string(&config).context("Failed to serialize Hermes config")?;
    let mut file =
        fs::File::create(path.join("config.yaml")).context("Failed to write Hermes config")?;
    file.write_all(content.as_bytes())?;
    Ok(())
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

fn first_non_empty(env: &BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| env.get(*key))
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(str::to_string)
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
