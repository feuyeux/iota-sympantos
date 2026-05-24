//! TCP JSON-line protocol types for the iota daemon.
//!
//! These structs define the wire format exchanged between the CLI client and
//! the background daemon process over `127.0.0.1:47661` (default).

use serde::{Deserialize, Serialize};

use crate::acp::AcpPromptTiming;
use crate::runtime_event::RuntimeEvent;

/// A prompt request sent by the CLI to the daemon.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonPromptRequest {
    pub backend: String,
    pub cwd: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub timing: bool,
}

/// Response to both prompt and warm requests.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonPromptResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<AcpPromptTiming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warmed: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RuntimeEvent>,
}

/// A warm-up request sent by the CLI to pre-start ACP backends.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonWarmRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backends: Vec<String>,
}

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::acp::AcpBackend;
use crate::config::{BackendConfig, ModelConfig, NimiaConfig};

pub const DESKTOP_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonClientMessage {
    Hello {
        client_name: String,
        protocol_version: u32,
    },
    StartTurn {
        turn_id: String,
        cwd: PathBuf,
        backend: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    RespondApproval {
        approval_id: String,
        approved: bool,
    },
    CancelTurn {
        turn_id: String,
    },
    GetConfig,
    SaveBackendModel {
        backend: String,
        model: DesktopModelConfig,
    },
    CheckBackend {
        backend: String,
    },
    GetObservabilitySummary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonServerMessage {
    HelloAccepted {
        protocol_version: u32,
    },
    ProtocolError {
        message: String,
    },
    TurnStarted {
        turn_id: String,
    },
    TextChunk {
        turn_id: String,
        chunk: String,
    },
    TurnEvent {
        turn_id: String,
        event: Box<RuntimeEvent>,
    },
    ApprovalRequested {
        turn_id: String,
        approval_id: String,
        tool_name: String,
        params: serde_json::Value,
    },
    ApprovalResponded {
        approval_id: String,
        accepted: bool,
    },
    TurnCompleted {
        turn_id: String,
        text: String,
        timing: crate::acp::AcpPromptTiming,
    },
    TurnFailed {
        turn_id: String,
        error: String,
    },
    TurnCancelled {
        turn_id: String,
        accepted: bool,
    },
    ConfigSnapshot {
        config: DesktopConfigSnapshot,
    },
    BackendCheckResult {
        backend: String,
        ok: bool,
        details: String,
    },
    ObservabilitySummary {
        summary: serde_json::Value,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DesktopModelConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_update: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopBackendSnapshot {
    pub backend: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<DesktopModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopConfigSnapshot {
    pub config_path: PathBuf,
    pub backends: BTreeMap<String, DesktopBackendSnapshot>,
}

impl DesktopConfigSnapshot {
    pub fn from_config(config: &NimiaConfig) -> Self {
        let mut backends = BTreeMap::new();
        for backend in crate::acp::ALL_BACKENDS {
            let key = backend.to_string();
            let snapshot = backend_snapshot(config, backend);
            backends.insert(key, snapshot);
        }

        Self {
            config_path: crate::config::config_path()
                .unwrap_or_else(|_| PathBuf::from("~/.i6/nimia.yaml")),
            backends,
        }
    }
}

fn backend_snapshot(config: &NimiaConfig, backend: AcpBackend) -> DesktopBackendSnapshot {
    let section = match backend {
        AcpBackend::ClaudeCode => config.claude_code.as_ref(),
        AcpBackend::Codex => config.codex.as_ref(),
        AcpBackend::Gemini => config.gemini.as_ref(),
        AcpBackend::Hermes => config.hermes.as_ref(),
        AcpBackend::OpenCode => config.opencode.as_ref(),
    };

    DesktopBackendSnapshot {
        backend: backend.to_string(),
        enabled: section.map(|cfg| cfg.enabled).unwrap_or(true),
        model: section.and_then(|cfg| cfg.model.as_ref()).map(mask_model),
    }
}

fn mask_model(model: &ModelConfig) -> DesktopModelConfig {
    DesktopModelConfig {
        provider: model.provider.clone(),
        name: model.name.clone(),
        base_url: model.base_url.clone(),
        api_key_configured: model
            .api_key
            .as_deref()
            .map(|key| {
                let key = key.trim();
                !key.is_empty() && key != "<api-key>" && key != "YOUR_API_KEY"
            })
            .unwrap_or(false),
        api_key_update: None,
    }
}

pub fn apply_desktop_model_update(
    config: &mut NimiaConfig,
    backend: AcpBackend,
    update: DesktopModelConfig,
) {
    let section: &mut Option<BackendConfig> = match backend {
        AcpBackend::ClaudeCode => &mut config.claude_code,
        AcpBackend::Codex => &mut config.codex,
        AcpBackend::Gemini => &mut config.gemini,
        AcpBackend::Hermes => &mut config.hermes,
        AcpBackend::OpenCode => &mut config.opencode,
    };

    let mut backend_config = section.clone().unwrap_or_default();
    let mut model = backend_config.model.clone().unwrap_or_default();
    if update.provider.is_some() {
        model.provider = normalize_optional_text(update.provider);
    }
    if update.name.is_some() {
        model.name = normalize_optional_text(update.name);
    }
    if update.base_url.is_some() {
        model.base_url = normalize_optional_text(update.base_url);
    }
    if update.api_key_update.is_some() {
        model.api_key = normalize_optional_text(update.api_key_update);
    }
    backend_config.model = Some(model);
    *section = Some(backend_config);
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "proto_tests.rs"]
mod proto_tests;
