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
