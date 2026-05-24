use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::acp::AcpBackend;

use super::NimiaConfig;
use super::context::BackendContextConfig;
use crate::config::model::ModelConfig;
use crate::config::{expand_home_path, normalize_command};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CommandConfig {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct BackendVersionMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp: Option<String>,
    #[serde(default, alias = "backend", skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackendConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp: Option<CommandConfig>,
    #[serde(default, alias = "versions", skip_serializing_if = "Option::is_none")]
    pub version_mapping: Option<BackendVersionMapping>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_whitelist: Vec<String>,
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;

fn default_enabled() -> bool {
    true
}

pub fn backend_config(config: &NimiaConfig, backend: AcpBackend) -> Option<&BackendConfig> {
    match backend {
        AcpBackend::ClaudeCode => config.claude_code.as_ref(),
        AcpBackend::Codex => config.codex.as_ref(),
        AcpBackend::Gemini => config.gemini.as_ref(),
        AcpBackend::Hermes => config.hermes.as_ref(),
        AcpBackend::OpenCode => config.opencode.as_ref(),
    }
}

pub fn command_label(command: &CommandConfig) -> String {
    if command.command.trim().is_empty() {
        return "missing command".to_string();
    }
    let mut parts = Vec::with_capacity(command.args.len() + 1);
    parts.push(command.command.clone());
    parts.extend(command.args.iter().cloned());
    parts.join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendReadiness {
    pub ok: bool,
    pub details: String,
}

pub fn backend_readiness(config: &NimiaConfig, backend: AcpBackend) -> BackendReadiness {
    let Some(section) = backend_config(config, backend) else {
        return BackendReadiness {
            ok: false,
            details: "missing section".to_string(),
        };
    };
    if !section.enabled {
        return BackendReadiness {
            ok: false,
            details: "disabled".to_string(),
        };
    }
    if section
        .acp
        .as_ref()
        .is_some_and(|acp| !acp.command.trim().is_empty())
    {
        return BackendReadiness {
            ok: true,
            details: "configured".to_string(),
        };
    }
    BackendReadiness {
        ok: false,
        details: "missing acp.command".to_string(),
    }
}

pub fn configured_model(section: &BackendConfig) -> Option<String> {
    section
        .model
        .as_ref()
        .and_then(|model| model.name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn normalized_acp_command(
    backend: AcpBackend,
    section: &BackendConfig,
    acp: &CommandConfig,
) -> (String, Vec<String>) {
    let mut args = acp.args.clone();
    if backend == AcpBackend::Codex {
        args.extend(codex_config_args(section));
    }
    (normalize_command(&acp.command), args)
}

pub fn backend_process_env_with_context(
    backend: AcpBackend,
    section: &BackendConfig,
    backend_context: Option<&BackendContextConfig>,
) -> BTreeMap<String, String> {
    let model = section.model.as_ref();
    let mut process_env = BTreeMap::new();
    if backend_context.map(|cfg| cfg.override_home).unwrap_or(true)
        && let Some(home) = section.home.as_deref().filter(|value| !value.is_empty())
        && let Some(env_key) = backend_home_env_key(backend)
    {
        process_env
            .entry(env_key.to_string())
            .or_insert(expand_home_path(home).unwrap_or_else(|_| home.to_string()));
    }

    match backend {
        AcpBackend::ClaudeCode => {
            if let Some(api_key) = model_value(model, |model| model.api_key.as_deref()) {
                process_env.insert("ANTHROPIC_AUTH_TOKEN".to_string(), api_key.clone());
                process_env.insert("ANTHROPIC_API_KEY".to_string(), api_key);
            }
            if let Some(base_url) = model_value(model, |model| model.base_url.as_deref()) {
                process_env.insert("ANTHROPIC_BASE_URL".to_string(), base_url);
            }
            if let Some(name) = model_value(model, |model| model.name.as_deref()) {
                process_env.insert("ANTHROPIC_MODEL".to_string(), name.clone());
                process_env.insert("ANTHROPIC_SMALL_FAST_MODEL".to_string(), name.clone());
                process_env.insert("ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(), name.clone());
                process_env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), name.clone());
                process_env.insert("ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(), name);
            }
            process_env.insert("API_TIMEOUT_MS".to_string(), "3000000".to_string());
            process_env.insert(
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
                "1".to_string(),
            );
        }
        AcpBackend::Codex => {
            if let Some(api_key) = model_value(model, |model| model.api_key.as_deref()) {
                process_env.insert("ROUTER_API_KEY".to_string(), api_key.clone());
                process_env.insert("OPENAI_API_KEY".to_string(), api_key);
            }
            if let Some(base_url) = model_value(model, |model| model.base_url.as_deref()) {
                process_env.insert("OPENAI_BASE_URL".to_string(), base_url);
            }
            if let Some(name) = model_value(model, |model| model.name.as_deref()) {
                process_env.insert("OPENAI_MODEL".to_string(), name);
            }
        }
        AcpBackend::Gemini => {
            if let Some(api_key) = model_value(model, |model| model.api_key.as_deref()) {
                process_env.insert("GEMINI_API_KEY".to_string(), api_key);
            }
            if let Some(name) = model_value(model, |model| model.name.as_deref()) {
                process_env.insert("GEMINI_MODEL".to_string(), name);
            }
        }
        AcpBackend::Hermes => {
            let provider = model_value(model, |model| model.provider.as_deref())
                .or_else(|| {
                    model_value(model, |model| model.base_url.as_deref())
                        .map(|url| infer_hermes_provider(&url).to_string())
                })
                .unwrap_or_else(|| "minimax-cn".to_string());
            let api_key = model_value(model, |model| model.api_key.as_deref()).unwrap_or_default();
            let base_url = model_value(model, |model| model.base_url.as_deref())
                .unwrap_or_else(|| default_hermes_base_url(&provider).to_string());
            render_hermes_provider_env(&mut process_env, &provider, &api_key, &base_url);
            process_env.insert("HERMES_INFERENCE_PROVIDER".to_string(), provider);
            if let Some(name) = model_value(model, |model| model.name.as_deref()) {
                process_env.insert("HERMES_MODEL".to_string(), name);
            }
        }
        AcpBackend::OpenCode => {
            if let Some(name) = model_value(model, |model| model.name.as_deref()) {
                process_env.insert("OPENCODE_MODEL".to_string(), name);
            }
        }
    }
    process_env
}

fn codex_config_args(section: &BackendConfig) -> Vec<String> {
    let Some(model) = section.model.as_ref() else {
        return Vec::new();
    };
    let provider = model_value(Some(model), |model| model.provider.as_deref());
    let mut args = Vec::new();
    if let Some(name) = model_value(Some(model), |model| model.name.as_deref()) {
        push_codex_config_arg(&mut args, "model", &name);
    }
    if let Some(provider) = provider.as_deref() {
        push_codex_config_arg(&mut args, "model_provider", provider);
    }
    if let (Some(provider), Some(base_url)) = (
        provider.as_deref(),
        model_value(Some(model), |model| model.base_url.as_deref()),
    ) {
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.name", provider),
            provider,
        );
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.base_url", provider),
            &base_url,
        );
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.env_key", provider),
            "ROUTER_API_KEY",
        );
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.wire_api", provider),
            "responses",
        );
    }
    args
}

fn push_codex_config_arg(args: &mut Vec<String>, key: &str, value: &str) {
    args.push("-c".to_string());
    args.push(format!("{}={}", key, toml_string(value)));
}

fn toml_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

fn backend_home_env_key(backend: AcpBackend) -> Option<&'static str> {
    match backend {
        AcpBackend::ClaudeCode => Some("CLAUDE_CONFIG_DIR"),
        AcpBackend::Codex => None,
        AcpBackend::Gemini => Some("GEMINI_CONFIG_DIR"),
        AcpBackend::Hermes => None,
        AcpBackend::OpenCode => Some("OPENCODE_CONFIG_DIR"),
    }
}

fn model_value<F>(model: Option<&ModelConfig>, getter: F) -> Option<String>
where
    F: FnOnce(&ModelConfig) -> Option<&str>,
{
    getter(model?)
        .map(str::trim)
        .filter(|value| !value.is_empty())
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
