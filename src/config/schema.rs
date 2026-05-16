use serde::{Deserialize, Serialize};

use super::{BackendConfig, ContextEngineBackendConfig, ContextEngineConfig};

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
