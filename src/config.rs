use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::acp::AcpBackend;
use crate::acp::session::{AcpMcpEnvShape, AcpMcpServer, AcpSessionOptions};

pub mod paths {
    use anyhow::{Context, Result};
    use std::path::PathBuf;

    #[derive(Debug, Clone)]
    pub struct StorePaths {
        root: PathBuf,
    }

    impl StorePaths {
        pub fn new(root: PathBuf) -> Self {
            Self { root }
        }

        pub fn resolve() -> Result<Self> {
            let home = dirs::home_dir().context("Failed to get home directory")?;
            Ok(Self::new(home.join(".i6").join("context")))
        }

        pub fn events_db(&self) -> PathBuf {
            self.root.join("events.sqlite")
        }

        pub fn memory_db(&self) -> PathBuf {
            self.root.join("memory.sqlite")
        }

        pub fn sessions_db(&self) -> PathBuf {
            self.root.join("sessions.sqlite")
        }

        pub fn approvals_db(&self) -> PathBuf {
            self.root.join("approvals.sqlite")
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ModelConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CommandConfig {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackendConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_whitelist: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct EmbeddingConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextEngineConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_context_injection")]
    pub injection: ContextInjection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_db: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_roots: Vec<String>,
    #[serde(default)]
    pub native_overlays: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<ContextBudgetsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_thresholds: Option<RecallThresholdsConfig>,
    #[serde(default = "default_episodic_compaction_keep")]
    pub episodic_compaction_keep: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fun: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingConfig>,
}

impl Default for ContextEngineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            injection: default_context_injection(),
            memory_db: None,
            skill_roots: Vec::new(),
            native_overlays: false,
            budgets: None,
            recall_thresholds: None,
            episodic_compaction_keep: default_episodic_compaction_keep(),
            mcp: None,
            fun: None,
            embedding: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextInjection {
    Auto,
    Off,
    Prompt,
    Mcp,
}

impl ContextInjection {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::Prompt => "prompt",
            Self::Mcp => "mcp",
        }
    }

    pub fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }
}

impl Default for ContextInjection {
    fn default() -> Self {
        Self::Auto
    }
}

impl Serialize for ContextInjection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ContextInjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "off" => Self::Off,
            "prompt" => Self::Prompt,
            "mcp" => Self::Mcp,
            _ => {
                return Err(serde::de::Error::custom(format!(
                    "invalid context_engine.injection '{}'; expected auto, off, prompt, or mcp",
                    value
                )));
            }
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextBudgetsConfig {
    #[serde(default = "default_memory_chars")]
    pub memory_chars: usize,
    #[serde(default = "default_skills_chars")]
    pub skills_chars: usize,
    #[serde(default = "default_dialogue_chars")]
    pub dialogue_chars: usize,
    #[serde(default = "default_workspace_chars")]
    pub workspace_chars: usize,
}

impl Default for ContextBudgetsConfig {
    fn default() -> Self {
        Self {
            memory_chars: default_memory_chars(),
            skills_chars: default_skills_chars(),
            dialogue_chars: default_dialogue_chars(),
            workspace_chars: default_workspace_chars(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecallThresholdsConfig {
    #[serde(default = "default_identity_threshold")]
    pub identity: f64,
    #[serde(default = "default_preference_threshold")]
    pub preference: f64,
    #[serde(default = "default_strategic_threshold")]
    pub strategic: f64,
    #[serde(default = "default_domain_threshold")]
    pub domain: f64,
    #[serde(default = "default_procedural_threshold")]
    pub procedural: f64,
    #[serde(default = "default_episodic_threshold")]
    pub episodic: f64,
}

impl Default for RecallThresholdsConfig {
    fn default() -> Self {
        Self {
            identity: default_identity_threshold(),
            preference: default_preference_threshold(),
            strategic: default_strategic_threshold(),
            domain: default_domain_threshold(),
            procedural: default_procedural_threshold(),
            episodic: default_episodic_threshold(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ContextEngineBackendConfig {
    #[serde(default)]
    pub gemini: Option<BackendContextConfig>,
    #[serde(default)]
    pub opencode: Option<BackendContextConfig>,
    #[serde(default)]
    pub hermes: Option<BackendContextConfig>,
    #[serde(default, rename = "claude-code")]
    pub claude_code: Option<BackendContextConfig>,
    #[serde(default)]
    pub codex: Option<BackendContextConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackendContextConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_session_new: Option<serde_yaml::Value>,
    #[serde(default)]
    pub always_send_empty_mcp_servers: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_env_shape: Option<String>,
    #[serde(default)]
    pub override_home: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct NimiaConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_code: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hermes: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_engine: Option<ContextEngineConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_engine_backend: Option<ContextEngineBackendConfig>,
}

#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    backends: BTreeMap<AcpBackend, BackendConfig>,
    backend_context: BTreeMap<AcpBackend, BackendContextConfig>,
    context_engine: ContextEngineConfig,
    memory_db_path: Option<PathBuf>,
    skill_roots: Vec<PathBuf>,
    mcp_servers: BTreeMap<AcpBackend, Vec<AcpMcpServer>>,
    session_options: BTreeMap<AcpBackend, AcpSessionOptions>,
    tool_whitelist: BTreeMap<AcpBackend, Vec<String>>,
    recall_thresholds: RecallThresholdsConfig,
    episodic_compaction_keep: usize,
    embedding: Option<EmbeddingConfig>,
}

impl EffectiveConfig {
    pub fn from_config(config: &NimiaConfig) -> Self {
        let backends = crate::acp::ALL_BACKENDS
            .iter()
            .filter_map(|backend| {
                backend_config(config, *backend)
                    .cloned()
                    .map(|cfg| (*backend, cfg))
            })
            .collect();
        let backend_context = crate::acp::ALL_BACKENDS
            .iter()
            .filter_map(|backend| {
                backend_context_config(config, *backend)
                    .cloned()
                    .map(|cfg| (*backend, cfg))
            })
            .collect();
        let mcp_servers = crate::acp::ALL_BACKENDS
            .iter()
            .map(|backend| (*backend, context_mcp_servers(config, *backend)))
            .collect();
        let session_options = crate::acp::ALL_BACKENDS
            .iter()
            .map(|backend| (*backend, context_session_options(config, *backend)))
            .collect();
        let tool_whitelist = crate::acp::ALL_BACKENDS
            .iter()
            .map(|backend| (*backend, context_tool_whitelist(config, *backend)))
            .collect();
        Self {
            backends,
            backend_context,
            context_engine: config.context_engine.clone().unwrap_or_default(),
            memory_db_path: context_memory_db_path(config).ok(),
            skill_roots: context_skill_roots(config),
            mcp_servers,
            session_options,
            tool_whitelist,
            recall_thresholds: context_recall_thresholds(config),
            episodic_compaction_keep: context_episodic_compaction_keep(config),
            embedding: context_embedding_config(config),
        }
    }

    pub fn backend_config(&self, backend: AcpBackend) -> Option<&BackendConfig> {
        self.backends.get(&backend)
    }

    pub fn backend_context_config(&self, backend: AcpBackend) -> Option<&BackendContextConfig> {
        self.backend_context.get(&backend)
    }

    pub fn context_engine(&self) -> &ContextEngineConfig {
        &self.context_engine
    }

    pub fn memory_db_path(&self) -> Option<&PathBuf> {
        self.memory_db_path.as_ref()
    }

    pub fn skill_roots(&self) -> &[PathBuf] {
        &self.skill_roots
    }

    pub fn context_mcp_servers(&self, backend: AcpBackend) -> Vec<AcpMcpServer> {
        self.mcp_servers.get(&backend).cloned().unwrap_or_default()
    }

    pub fn context_session_options(&self, backend: AcpBackend) -> AcpSessionOptions {
        self.session_options
            .get(&backend)
            .copied()
            .unwrap_or_default()
    }

    pub fn context_tool_whitelist(&self, backend: AcpBackend) -> Vec<String> {
        self.tool_whitelist
            .get(&backend)
            .cloned()
            .unwrap_or_default()
    }

    pub fn recall_thresholds(&self) -> &RecallThresholdsConfig {
        &self.recall_thresholds
    }

    pub fn episodic_compaction_keep(&self) -> usize {
        self.episodic_compaction_keep
    }

    pub fn embedding_config(&self) -> Option<EmbeddingConfig> {
        self.embedding.clone()
    }
}

pub fn config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".i6").join("nimia.yaml"))
}

pub fn read_config() -> Result<NimiaConfig> {
    let path = config_path()?;
    if !path.exists() {
        bail!("Backend config not found: {}", path.display());
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_yaml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
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
    if backend_context.map(|cfg| cfg.override_home).unwrap_or(true) {
        if let Some(home) = section.home.as_deref().filter(|value| !value.is_empty()) {
            if let Some(env_key) = backend_home_env_key(backend) {
                process_env
                    .entry(env_key.to_string())
                    .or_insert(expand_home_path(home).unwrap_or_else(|_| home.to_string()));
            }
        }
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

fn default_enabled() -> bool {
    true
}

fn default_identity_threshold() -> f64 {
    0.85
}

fn default_preference_threshold() -> f64 {
    0.80
}

fn default_strategic_threshold() -> f64 {
    0.80
}

fn default_domain_threshold() -> f64 {
    0.80
}

fn default_procedural_threshold() -> f64 {
    0.75
}

fn default_episodic_threshold() -> f64 {
    0.70
}

fn default_episodic_compaction_keep() -> usize {
    40
}

pub fn expand_home_path(value: &str) -> Result<String> {
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        if value == "~" {
            return Ok(home.display().to_string());
        }
        return Ok(home.join(&value[2..]).display().to_string());
    }
    Ok(value.to_string())
}

pub fn normalize_command(command: &str) -> String {
    if cfg!(windows) && command.eq_ignore_ascii_case("npx") {
        "npx.cmd".to_string()
    } else {
        command.to_string()
    }
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

pub fn context_memory_db_path(config: &NimiaConfig) -> Result<PathBuf> {
    if let Some(path) = config
        .context_engine
        .as_ref()
        .and_then(|cfg| cfg.memory_db.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(expand_home_path(path)?));
    }
    Ok(paths::StorePaths::resolve()?.memory_db())
}

pub fn context_skill_roots(config: &NimiaConfig) -> Vec<PathBuf> {
    config
        .context_engine
        .as_ref()
        .map(|cfg| {
            cfg.skill_roots
                .iter()
                .filter_map(|root| expand_home_path(root).ok().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn context_mcp_servers(config: &NimiaConfig, backend: AcpBackend) -> Vec<AcpMcpServer> {
    if !context_mcp_session_enabled(config, backend) {
        return Vec::new();
    }
    let Some(engine) = config.context_engine.as_ref() else {
        return default_context_mcp_servers();
    };
    if !engine.enabled || engine.injection.is_off() {
        return Vec::new();
    }

    let mut servers = Vec::new();
    if let Some(server) =
        command_to_mcp_server("iota-context", engine.mcp.as_ref(), &["context-mcp"])
    {
        servers.push(server);
    }
    if let Some(server) = command_to_mcp_server("iota-fun", engine.fun.as_ref(), &["fun-mcp"]) {
        servers.push(server);
    }
    if servers.is_empty() {
        default_context_mcp_servers()
    } else {
        servers
    }
}

pub fn context_mcp_session_enabled(config: &NimiaConfig, backend: AcpBackend) -> bool {
    let backend_config = backend_context_config(config, backend);
    if let Some(value) = backend_config.and_then(|cfg| cfg.mcp_session_new.as_ref()) {
        return yaml_flag(
            value,
            matches!(backend, AcpBackend::ClaudeCode | AcpBackend::Codex),
        );
    }
    matches!(
        backend,
        AcpBackend::Gemini | AcpBackend::Hermes | AcpBackend::OpenCode
    )
}

pub fn backend_context_config(
    config: &NimiaConfig,
    backend: AcpBackend,
) -> Option<&BackendContextConfig> {
    config
        .context_engine_backend
        .as_ref()
        .and_then(|cfg| match backend {
            AcpBackend::ClaudeCode => cfg.claude_code.as_ref(),
            AcpBackend::Codex => cfg.codex.as_ref(),
            AcpBackend::Gemini => cfg.gemini.as_ref(),
            AcpBackend::Hermes => cfg.hermes.as_ref(),
            AcpBackend::OpenCode => cfg.opencode.as_ref(),
        })
}

pub fn context_session_options(config: &NimiaConfig, backend: AcpBackend) -> AcpSessionOptions {
    let Some(backend_context) = backend_context_config(config, backend) else {
        return AcpSessionOptions::default();
    };
    AcpSessionOptions {
        always_send_empty_mcp_servers: backend_context.always_send_empty_mcp_servers,
        mcp_env_shape: backend_context
            .mcp_env_shape
            .as_deref()
            .and_then(AcpMcpEnvShape::parse)
            .unwrap_or_default(),
    }
}

pub fn context_tool_whitelist(config: &NimiaConfig, backend: AcpBackend) -> Vec<String> {
    backend_config(config, backend)
        .map(|cfg| cfg.tool_whitelist.clone())
        .unwrap_or_default()
}

pub fn context_recall_thresholds(config: &NimiaConfig) -> RecallThresholdsConfig {
    config
        .context_engine
        .as_ref()
        .and_then(|cfg| cfg.recall_thresholds.clone())
        .unwrap_or_default()
}

pub fn context_episodic_compaction_keep(config: &NimiaConfig) -> usize {
    config
        .context_engine
        .as_ref()
        .map(|cfg| cfg.episodic_compaction_keep.max(1))
        .unwrap_or_else(default_episodic_compaction_keep)
}

pub fn context_embedding_config(config: &NimiaConfig) -> Option<EmbeddingConfig> {
    config
        .context_engine
        .as_ref()
        .and_then(|cfg| cfg.embedding.clone())
}

fn yaml_flag(value: &serde_yaml::Value, try_is_enabled: bool) -> bool {
    match value {
        serde_yaml::Value::Bool(value) => *value,
        serde_yaml::Value::String(value) => match value.as_str() {
            "true" | "yes" | "on" => true,
            "false" | "no" | "off" => false,
            "try" => try_is_enabled,
            _ => false,
        },
        _ => false,
    }
}

fn command_to_mcp_server(
    default_name: &str,
    command: Option<&CommandConfig>,
    default_args: &[&str],
) -> Option<AcpMcpServer> {
    let (command, args) = match command {
        Some(command) if !command.command.trim().is_empty() => {
            let args = command
                .args
                .iter()
                .filter_map(|arg| expand_home_path(arg).ok())
                .collect::<Vec<_>>();
            (
                normalize_command(&expand_home_path(&command.command).ok()?),
                args,
            )
        }
        Some(_) => return None,
        None => {
            let command = std::env::current_exe()
                .ok()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "iota".to_string());
            (
                command,
                default_args.iter().map(|arg| arg.to_string()).collect(),
            )
        }
    };
    Some(AcpMcpServer {
        name: default_name.to_string(),
        command,
        args,
        env: BTreeMap::new(),
    })
}

fn default_context_mcp_servers() -> Vec<AcpMcpServer> {
    [
        command_to_mcp_server("iota-context", None, &["context-mcp"]),
        command_to_mcp_server("iota-fun", None, &["fun-mcp"]),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn default_context_injection() -> ContextInjection {
    ContextInjection::Auto
}

fn default_memory_chars() -> usize {
    2000
}

fn default_skills_chars() -> usize {
    1200
}

fn default_dialogue_chars() -> usize {
    1500
}

fn default_workspace_chars() -> usize {
    800
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
