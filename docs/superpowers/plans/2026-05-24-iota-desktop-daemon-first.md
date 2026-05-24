# iota-desktop Daemon-first Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `iota-desktop` a Chat-first Tauri workbench that talks to the reusable iota daemon instead of creating `IotaEngine` directly.

**Architecture:** Keep the existing CLI daemon request/response API intact, and add a desktop JSON-line streaming protocol over the same local TCP daemon. The Tauri backend becomes a thin daemon client and event bridge; React owns chat state through a turn reducer and renders a right-side execution inspector.

**Tech Stack:** Rust 2024, Tokio TCP JSON-line daemon, serde tagged enums, Tauri 2 commands/events, React 19, TypeScript, Tailwind CSS v4, existing `#[path = "..._tests.rs"]` Rust test convention.

---

## File Structure

- Modify: `crates/iota-core/src/daemon/proto.rs`
  Add desktop protocol message enums and typed config/status DTOs while preserving `DaemonPromptRequest`, `DaemonPromptResponse`, and `DaemonWarmRequest`.
- Create: `crates/iota-core/src/daemon/proto_tests.rs`
  Serde roundtrip tests for legacy and desktop protocol messages.
- Modify: `crates/iota-core/src/daemon/mod.rs`
  Route incoming connections by protocol shape, preserve single-response legacy handling, and add streaming desktop handling.
- Create: `crates/iota-core/src/daemon/desktop.rs`
  Desktop stream handler: hello/version, start turn, text chunks, runtime events, approval forwarding, config snapshot, backend checks.
- Create: `crates/iota-core/src/daemon/desktop_tests.rs`
  Unit tests for message classification and approval registry behavior.
- Modify: `crates/iota-core/src/daemon/pool.rs`
  Expose safe config refresh/update helpers needed by daemon config writes.
- Modify: `crates/iota-core/src/config/loader.rs`
  Add a `save_config()` helper that writes `~/.i6/nimia.yaml`.
- Modify: `crates/iota-core/src/config/mod.rs`
  Export `save_config()` for daemon config updates.
- Modify: `crates/iota-core/src/daemon/SKILL.md`
  Document that daemon has legacy CLI API and desktop streaming API.
- Create: `crates/iota-desktop/src-tauri/src/daemon_client.rs`
  Tauri-side daemon client: autostart/connect, send JSON lines, read streaming messages.
- Modify: `crates/iota-desktop/src-tauri/src/lib.rs`
  Remove direct `IotaEngine` cache and bridge Tauri commands/events through `daemon_client`.
- Modify: `crates/iota-desktop/src-tauri/src/lib_tests.rs`
  Replace direct API key tests with desktop command/client tests that do not construct `IotaEngine`.
- Create: `crates/iota-desktop/src/types.ts`
  Frontend turn, event, approval, usage, and config types.
- Create: `crates/iota-desktop/src/turnReducer.ts`
  Pure reducer for daemon stream events.
- Create: `crates/iota-desktop/src/turnReducer.test.ts`
  Reducer tests for streaming text, runtime events, approvals, completion, failure, and cancellation.
- Create: `crates/iota-desktop/src/api.ts`
  Typed Tauri invoke/listen wrappers.
- Create: `crates/iota-desktop/src/components/ChatWorkbench.tsx`
  Main Chat-first workspace shell.
- Create: `crates/iota-desktop/src/components/RightInspector.tsx`
  Turn status, timing, usage, tool call, approval, and runtime event inspector.
- Create: `crates/iota-desktop/src/components/ConfigPanel.tsx`
  Config snapshot and masked API key editing through daemon APIs.
- Modify: `crates/iota-desktop/src/App.tsx`
  Reduce to app shell and route state to new components.
- Modify: `crates/iota-desktop/src/App.css`
  Keep global theme only; move layout classes into components via Tailwind class names.
- Modify: `crates/iota-desktop/package.json`
  Add a frontend test script if no test runner exists.
- Modify: `docs/architecture.md`
  Update daemon and desktop architecture notes after implementation.

## Task 1: Add Desktop Daemon Protocol Types

**Files:**
- Modify: `crates/iota-core/src/daemon/proto.rs`
- Create: `crates/iota-core/src/daemon/proto_tests.rs`
- Modify: `crates/iota-core/src/daemon/mod.rs`

- [ ] **Step 1: Add failing serde tests**

Create `crates/iota-core/src/daemon/proto_tests.rs` with:

```rust
use super::*;
use crate::runtime_event::{OutputEvent, RuntimeEvent};

#[test]
fn legacy_prompt_request_still_roundtrips() {
    let request = DaemonPromptRequest {
        backend: "gemini".to_string(),
        cwd: "/tmp/project".to_string(),
        prompt: "hello".to_string(),
        execution_id: Some("exec-1".to_string()),
        timeout_ms: Some(1000),
        timing: true,
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(!json.contains("StartTurn"));

    let decoded: DaemonPromptRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.backend, "gemini");
    assert_eq!(decoded.execution_id.as_deref(), Some("exec-1"));
}

#[test]
fn desktop_start_turn_roundtrips() {
    let message = DaemonClientMessage::StartTurn {
        turn_id: "turn-1".to_string(),
        cwd: "/tmp/project".into(),
        backend: "codex".to_string(),
        prompt: "implement feature".to_string(),
        timeout_ms: Some(600_000),
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"start_turn\""));

    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonClientMessage::StartTurn { turn_id, backend, .. }
            if turn_id == "turn-1" && backend == "codex"
    ));
}

#[test]
fn desktop_server_event_roundtrips_runtime_event() {
    let message = DaemonServerMessage::TurnEvent {
        turn_id: "turn-1".to_string(),
        event: RuntimeEvent::Output(OutputEvent {
            text: "chunk".to_string(),
            role: Some("assistant".to_string()),
        }),
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"turn_event\""));

    let decoded: DaemonServerMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonServerMessage::TurnEvent {
            event: RuntimeEvent::Output(OutputEvent { text, .. }),
            ..
        } if text == "chunk"
    ));
}

#[test]
fn desktop_config_snapshot_masks_api_keys() {
    let mut config = crate::config::NimiaConfig::default();
    let mut backend = crate::config::BackendConfig::default();
    let mut model = crate::config::ModelConfig::default();
    model.api_key = Some("secret-value".to_string());
    backend.model = Some(model);
    config.gemini = Some(backend);

    let snapshot = DesktopConfigSnapshot::from_config(&config);
    let json = serde_json::to_string(&snapshot).unwrap();

    assert!(!json.contains("secret-value"));
    assert!(json.contains("\"api_key_configured\":true"));
}
```

- [ ] **Step 2: Run the new tests and verify they fail**

Run:

```bash
cargo test -p iota-core daemon::proto_tests -- --nocapture
```

Expected: FAIL because `DaemonClientMessage`, `DaemonServerMessage`, and `DesktopConfigSnapshot` are not defined and `proto_tests` is not wired.

- [ ] **Step 3: Add protocol types**

Append to `crates/iota-core/src/daemon/proto.rs`:

```rust
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
        event: RuntimeEvent,
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
            config_path: crate::config::config_path().unwrap_or_else(|_| PathBuf::from("~/.i6/nimia.yaml")),
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
        model.provider = update.provider;
    }
    if update.name.is_some() {
        model.name = update.name;
    }
    if update.base_url.is_some() {
        model.base_url = update.base_url;
    }
    if update.api_key_update.is_some() {
        model.api_key = update.api_key_update;
    }
    backend_config.model = Some(model);
    *section = Some(backend_config);
}
```

- [ ] **Step 4: Wire protocol tests**

At the bottom of `crates/iota-core/src/daemon/proto.rs`, add:

```rust
#[cfg(test)]
#[path = "proto_tests.rs"]
mod proto_tests;
```

- [ ] **Step 5: Export protocol types**

Change the `pub use proto::{...};` line in `crates/iota-core/src/daemon/mod.rs` to:

```rust
pub use proto::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonPromptRequest, DaemonPromptResponse,
    DaemonServerMessage, DaemonWarmRequest, DesktopBackendSnapshot, DesktopConfigSnapshot,
    DesktopModelConfig, apply_desktop_model_update,
};
```

- [ ] **Step 6: Run protocol tests**

Run:

```bash
cargo test -p iota-core daemon::proto_tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit protocol types**

Run:

```bash
git add crates/iota-core/src/daemon/proto.rs crates/iota-core/src/daemon/proto_tests.rs crates/iota-core/src/daemon/mod.rs
git commit -m "feat: add desktop daemon protocol types"
```

Expected: One commit with protocol types and tests.

## Task 2: Add Desktop Daemon Stream Handler

**Files:**
- Create: `crates/iota-core/src/daemon/desktop.rs`
- Create: `crates/iota-core/src/daemon/desktop_tests.rs`
- Modify: `crates/iota-core/src/daemon/mod.rs`
- Modify: `crates/iota-core/src/daemon/pool.rs`
- Modify: `crates/iota-core/src/config/loader.rs`
- Modify: `crates/iota-core/src/config/mod.rs`

- [ ] **Step 1: Add failing approval registry tests**

Create `crates/iota-core/src/daemon/desktop_tests.rs` with:

```rust
use super::*;
use tokio::sync::oneshot;

#[tokio::test]
async fn approval_registry_delivers_decision_once() {
    let registry = ApprovalRegistry::default();
    let (tx, rx) = oneshot::channel();
    registry.insert("approval-1".to_string(), tx).await;

    assert!(registry.respond("approval-1", true).await);
    assert_eq!(rx.await.unwrap(), true);
    assert!(!registry.respond("approval-1", false).await);
}

#[tokio::test]
async fn approval_registry_returns_false_for_missing_id() {
    let registry = ApprovalRegistry::default();
    assert!(!registry.respond("missing", true).await);
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test -p iota-core daemon::desktop_tests -- --nocapture
```

Expected: FAIL because `desktop` module and `ApprovalRegistry` do not exist.

- [ ] **Step 3: Create desktop handler skeleton**

Create `crates/iota-core/src/daemon/desktop.rs` with:

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::acp::{permission::ApprovalRequest, AcpBackend};
use crate::config::{read_config, save_config};
use crate::daemon::pool::EnginePool;
use crate::daemon::proto::{
    apply_desktop_model_update, DaemonClientMessage, DaemonServerMessage, DesktopConfigSnapshot,
    DESKTOP_PROTOCOL_VERSION,
};

#[derive(Default, Clone)]
pub(crate) struct ApprovalRegistry {
    pending: Arc<Mutex<BTreeMap<String, oneshot::Sender<bool>>>>,
}

impl ApprovalRegistry {
    pub async fn insert(&self, approval_id: String, tx: oneshot::Sender<bool>) {
        self.pending.lock().await.insert(approval_id, tx);
    }

    pub async fn respond(&self, approval_id: &str, approved: bool) -> bool {
        let tx = self.pending.lock().await.remove(approval_id);
        if let Some(tx) = tx {
            let _ = tx.send(approved);
            true
        } else {
            false
        }
    }
}

pub(crate) async fn handle_desktop_connection<R>(
    first_message: DaemonClientMessage,
    reader: BufReader<R>,
    write_half: OwnedWriteHalf,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let writer = Arc::new(Mutex::new(write_half));
    handle_message(
        first_message,
        Arc::clone(&writer),
        Arc::clone(&engine_pool),
        approvals.clone(),
    )
    .await?;

    let mut reader = reader;
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let message: DaemonClientMessage =
            serde_json::from_str(line.trim()).context("Failed to decode desktop daemon message")?;
        handle_message(message, Arc::clone(&writer), Arc::clone(&engine_pool), approvals.clone())
            .await?;
        line.clear();
    }
    Ok(())
}

async fn handle_message(
    message: DaemonClientMessage,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
) -> Result<()> {
    match message {
        DaemonClientMessage::Hello {
            protocol_version, ..
        } => {
            if protocol_version != DESKTOP_PROTOCOL_VERSION {
                send_message(
                    &writer,
                    &DaemonServerMessage::ProtocolError {
                        message: format!(
                            "unsupported desktop daemon protocol version {}; expected {}",
                            protocol_version, DESKTOP_PROTOCOL_VERSION
                        ),
                    },
                )
                .await?;
            } else {
                send_message(
                    &writer,
                    &DaemonServerMessage::HelloAccepted {
                        protocol_version: DESKTOP_PROTOCOL_VERSION,
                    },
                )
                .await?;
            }
        }
        DaemonClientMessage::StartTurn {
            turn_id,
            cwd,
            backend,
            prompt,
            timeout_ms,
        } => {
            start_turn(turn_id, cwd, backend, prompt, timeout_ms, writer, engine_pool, approvals)
                .await?;
        }
        DaemonClientMessage::RespondApproval {
            approval_id,
            approved,
        } => {
            let accepted = approvals.respond(&approval_id, approved).await;
            send_message(
                &writer,
                &DaemonServerMessage::ApprovalResponded {
                    approval_id: approval_id.clone(),
                    accepted,
                },
            )
            .await?;
            if !accepted {
                send_message(
                    &writer,
                    &DaemonServerMessage::ProtocolError {
                        message: format!("approval id {} was not pending", approval_id),
                    },
                )
                .await?;
            }
        }
        DaemonClientMessage::CancelTurn { turn_id } => {
            send_message(&writer, &DaemonServerMessage::TurnCancelled { turn_id }).await?;
        }
        DaemonClientMessage::GetConfig => {
            let config = read_config().context("Failed to read config")?;
            send_message(
                &writer,
                &DaemonServerMessage::ConfigSnapshot {
                    config: DesktopConfigSnapshot::from_config(&config),
                },
            )
            .await?;
        }
        DaemonClientMessage::SaveBackendModel { backend, model } => {
            let backend = AcpBackend::parse(&backend)?;
            let mut config = read_config().context("Failed to read config")?;
            apply_desktop_model_update(&mut config, backend, model);
            save_config(&config).context("Failed to save config")?;
            engine_pool.lock().await.replace_config(config.clone());
            send_message(
                &writer,
                &DaemonServerMessage::ConfigSnapshot {
                    config: DesktopConfigSnapshot::from_config(&config),
                },
            )
            .await?;
        }
        DaemonClientMessage::CheckBackend { backend } => {
            let parsed = AcpBackend::parse(&backend);
            let (ok, details) = match parsed {
                Ok(_) => (true, "backend name is recognized".to_string()),
                Err(err) => (false, err.to_string()),
            };
            send_message(
                &writer,
                &DaemonServerMessage::BackendCheckResult {
                    backend,
                    ok,
                    details,
                },
            )
            .await?;
        }
        DaemonClientMessage::GetObservabilitySummary { cwd } => {
            send_message(
                &writer,
                &DaemonServerMessage::ObservabilitySummary {
                    summary: serde_json::json!({ "cwd": cwd }),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn start_turn(
    turn_id: String,
    cwd: PathBuf,
    backend: String,
    prompt: String,
    timeout_ms: Option<u64>,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
) -> Result<()> {
    let backend = AcpBackend::parse(&backend)?;
    send_message(&writer, &DaemonServerMessage::TurnStarted { turn_id: turn_id.clone() }).await?;

    let engine = engine_pool.lock().await.engine_for(cwd.clone());
    let (stream_tx, mut stream_rx) = mpsc::channel::<String>(100);
    let (approval_tx, mut approval_rx) = mpsc::channel::<ApprovalRequest>(10);
    crate::acp::permission::install_tui_approval_channel(approval_tx).await;

    let stream_writer = Arc::clone(&writer);
    let stream_turn_id = turn_id.clone();
    tokio::spawn(async move {
        while let Some(chunk) = stream_rx.recv().await {
            let _ = send_message(
                &stream_writer,
                &DaemonServerMessage::TextChunk {
                    turn_id: stream_turn_id.clone(),
                    chunk,
                },
            )
            .await;
        }
    });

    let approval_writer = Arc::clone(&writer);
    let approval_turn_id = turn_id.clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            let approval_id = uuid::Uuid::new_v4().to_string();
            let (reply_tx, reply_rx) = oneshot::channel();
            approvals.insert(approval_id.clone(), reply_tx).await;
            let _ = send_message(
                &approval_writer,
                &DaemonServerMessage::ApprovalRequested {
                    turn_id: approval_turn_id.clone(),
                    approval_id,
                    tool_name: req.tool_name,
                    params: req.params,
                },
            )
            .await;
            let decision = reply_rx.await.unwrap_or(false);
            let _ = req.reply.send(decision);
        }
    });

    tokio::spawn(async move {
        let result = {
            let mut engine = engine.lock().await;
            if let Some(timeout_ms) = timeout_ms {
                engine.set_acp_timeout_ms(timeout_ms);
            }
            engine.set_stream_output_sender(Some(stream_tx));
            let result = engine.run_with_timing(backend, cwd, &prompt).await;
            engine.set_stream_output_sender(None);
            result
        };

        match result {
            Ok(output) => {
                for event in output.events {
                    let _ = send_message(
                        &writer,
                        &DaemonServerMessage::TurnEvent {
                            turn_id: turn_id.clone(),
                            event,
                        },
                    )
                    .await;
                }
                let _ = send_message(
                    &writer,
                    &DaemonServerMessage::TurnCompleted {
                        turn_id,
                        text: output.text,
                        timing: output.timing,
                    },
                )
                .await;
            }
            Err(err) => {
                let _ = send_message(
                    &writer,
                    &DaemonServerMessage::TurnFailed {
                        turn_id,
                        error: err.to_string(),
                    },
                )
                .await;
            }
        }
    });

    Ok(())
}

async fn send_message(
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    message: &DaemonServerMessage,
) -> Result<()> {
    let mut line = serde_json::to_vec(message).context("Failed to encode desktop daemon message")?;
    line.push(b'\n');
    let mut writer = writer.lock().await;
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod desktop_tests;
```

- [ ] **Step 4: Add config save and replacement helpers**

Add this function to `crates/iota-core/src/config/loader.rs` after `read_config()`:

```rust
pub fn save_config(config: &NimiaConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_yaml::to_string(config).context("Failed to encode config")?;
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
```

Change the loader export in `crates/iota-core/src/config/mod.rs` to:

```rust
pub use loader::{config_path, read_config, save_config};
```

Add to `impl EnginePool` in `crates/iota-core/src/daemon/pool.rs`:

```rust
    pub fn replace_config(&mut self, config: NimiaConfig) {
        self.config = config;
        self.engines.clear();
    }
```

- [ ] **Step 5: Wire desktop module**

At the top of `crates/iota-core/src/daemon/mod.rs`, add:

```rust
mod desktop;
```

In `run_daemon`, create a daemon-wide approval registry before the accept loop:

```rust
let desktop_approvals = desktop::ApprovalRegistry::default();
```

Inside the accept branch, clone it into each spawned connection task:

```rust
let desktop_approvals = desktop_approvals.clone();
tokio::spawn(async move {
    let _permit = permit.acquire_owned().await;
    if let Err(err) = handle_connection(stream, engine_pool, desktop_approvals).await {
        eprintln!("daemon request failed: {}", err);
    }
});
```

Change `handle_connection` signature:

```rust
async fn handle_connection(
    stream: TcpStream,
    engine_pool: Arc<Mutex<EnginePool>>,
    desktop_approvals: desktop::ApprovalRegistry,
) -> Result<()> {
```

In `handle_connection`, replace the current body with:

```rust
// Limit inbound request size to 10 MiB to prevent memory exhaustion from
// a malicious or misbehaving client sending an unbounded line.
const MAX_REQUEST_BYTES: u64 = 10 * 1024 * 1024;
let (read_half, mut write_half) = stream.into_split();
let limited = tokio::io::AsyncReadExt::take(read_half, MAX_REQUEST_BYTES + 1);
let mut reader = BufReader::new(limited);
let mut request_line = String::new();
let bytes_read = reader.read_line(&mut request_line).await?;
if bytes_read as u64 > MAX_REQUEST_BYTES {
    anyhow::bail!("daemon request exceeded {} byte limit", MAX_REQUEST_BYTES);
}
let request: serde_json::Value =
    serde_json::from_str(request_line.trim()).context("Failed to decode daemon request")?;

if matches!(
    request.get("type").and_then(serde_json::Value::as_str),
    Some("hello" | "start_turn" | "respond_approval" | "cancel_turn" | "get_config" | "save_backend_model" | "check_backend" | "get_observability_summary")
) {
    let first_message: DaemonClientMessage =
        serde_json::from_value(request).context("Failed to decode desktop daemon message")?;
    desktop::handle_desktop_connection(
        first_message,
        reader,
        write_half,
        engine_pool,
        desktop_approvals,
    )
    .await?;
    return Ok(());
}

let response = if request.get("type").and_then(serde_json::Value::as_str) == Some("warm") {
    let request: DaemonWarmRequest =
        serde_json::from_value(request).context("Failed to decode daemon warm request")?;
    handle_warm(request, engine_pool).await
} else {
    let request: DaemonPromptRequest =
        serde_json::from_value(request).context("Failed to decode daemon prompt request")?;
    handle_prompt(request, engine_pool).await
};
let mut line = serde_json::to_vec(&response).context("Failed to encode daemon response")?;
line.push(b'\n');
write_half.write_all(&line).await?;
write_half.flush().await?;
Ok(())
```

Add `DaemonClientMessage` to the existing daemon imports in `mod.rs` if it is not already in scope.

- [ ] **Step 6: Run Rust formatting**

Run:

```bash
cargo fmt --all --check
```

Expected: PASS.

- [ ] **Step 7: Run desktop daemon tests**

Run:

```bash
cargo test -p iota-core daemon::desktop_tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Run daemon protocol tests**

Run:

```bash
cargo test -p iota-core daemon -- --nocapture
```

Expected: PASS for daemon-related tests.

- [ ] **Step 9: Commit desktop stream handler**

Run:

```bash
git add crates/iota-core/src/daemon/mod.rs crates/iota-core/src/daemon/desktop.rs crates/iota-core/src/daemon/desktop_tests.rs crates/iota-core/src/daemon/pool.rs crates/iota-core/src/config/loader.rs crates/iota-core/src/config/mod.rs
git commit -m "feat: add desktop daemon stream handler"
```

Expected: One commit.

## Task 3: Add Tauri Daemon Client

**Files:**
- Create: `crates/iota-desktop/src-tauri/src/daemon_client.rs`
- Modify: `crates/iota-desktop/src-tauri/src/lib.rs`
- Modify: `crates/iota-desktop/src-tauri/src/lib_tests.rs`

- [ ] **Step 1: Add failing daemon client serialization test**

Replace `crates/iota-desktop/src-tauri/src/lib_tests.rs` with:

```rust
use super::*;
use iota_core::daemon::{DaemonClientMessage, DESKTOP_PROTOCOL_VERSION};

#[test]
fn desktop_hello_uses_current_protocol_version() {
    let message = daemon_client::hello_message();
    assert!(matches!(
        message,
        DaemonClientMessage::Hello {
            protocol_version: DESKTOP_PROTOCOL_VERSION,
            ..
        }
    ));
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
cargo test -p iota-desktop desktop_hello_uses_current_protocol_version -- --nocapture
```

Expected: FAIL because `daemon_client` does not exist.

- [ ] **Step 3: Create daemon client module**

Create `crates/iota-desktop/src-tauri/src/daemon_client.rs` with:

```rust
use anyhow::{Context, Result};
use iota_core::daemon::{
    daemon_addr, DaemonClientMessage, DaemonServerMessage, DESKTOP_PROTOCOL_VERSION,
};
use std::path::PathBuf;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub fn hello_message() -> DaemonClientMessage {
    DaemonClientMessage::Hello {
        client_name: "iota-desktop".to_string(),
        protocol_version: DESKTOP_PROTOCOL_VERSION,
    }
}

pub async fn start_turn(
    window: tauri::Window,
    turn_id: String,
    cwd: PathBuf,
    backend: String,
    prompt: String,
) -> Result<()> {
    let mut stream = connect_or_start().await?;
    write_message(&mut stream, &hello_message()).await?;
    write_message(
        &mut stream,
        &DaemonClientMessage::StartTurn {
            turn_id,
            cwd,
            backend,
            prompt,
            timeout_ms: Some(600_000),
        },
    )
    .await?;

    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            match serde_json::from_str::<DaemonServerMessage>(line.trim()) {
                Ok(message) => {
                    let _ = window.emit("daemon-message", message);
                }
                Err(err) => {
                    let _ = window.emit("daemon-client-error", err.to_string());
                }
            }
            line.clear();
        }
    });

    Ok(())
}

pub async fn send_one(message: DaemonClientMessage) -> Result<Vec<DaemonServerMessage>> {
    let mut stream = connect_or_start().await?;
    write_message(&mut stream, &hello_message()).await?;
    write_message(&mut stream, &message).await?;

    let mut reader = BufReader::new(stream);
    let mut messages = Vec::new();
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        messages.push(serde_json::from_str(line.trim())?);
        if matches!(
            messages.last(),
            Some(DaemonServerMessage::ConfigSnapshot { .. })
                | Some(DaemonServerMessage::BackendCheckResult { .. })
                | Some(DaemonServerMessage::ObservabilitySummary { .. })
                | Some(DaemonServerMessage::ApprovalResponded { .. })
                | Some(DaemonServerMessage::ProtocolError { .. })
        ) {
            break;
        }
        line.clear();
    }
    Ok(messages)
}

async fn connect_or_start() -> Result<TcpStream> {
    let addr = daemon_addr();
    match TcpStream::connect(&addr).await {
        Ok(stream) => Ok(stream),
        Err(first_err) => {
            autostart_daemon().context("Failed to autostart daemon")?;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            TcpStream::connect(&addr)
                .await
                .with_context(|| format!("Failed to connect to daemon at {} after autostart: {}", addr, first_err))
        }
    }
}

fn autostart_daemon() -> Result<()> {
    let daemon_exe = locate_iota_cli().context("Failed to locate iota CLI for daemon autostart")?;
    std::process::Command::new(daemon_exe)
        .arg("__daemon")
        .spawn()
        .context("Failed to spawn iota daemon")?;
    Ok(())
}

fn locate_iota_cli() -> Result<std::path::PathBuf> {
    if let Ok(path) = std::env::var("IOTA_CLI_PATH") {
        let path = std::path::PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }

    let exe_name = if cfg!(windows) { "iota.exe" } else { "iota" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(exe_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    let current = std::env::current_exe().context("Failed to locate current executable")?;
    if let Some(dir) = current.parent() {
        let sibling = dir.join(exe_name);
        if sibling.exists() {
            return Ok(sibling);
        }
    }

    anyhow::bail!("set IOTA_CLI_PATH or install the iota CLI in PATH")
}

async fn write_message(stream: &mut TcpStream, message: &DaemonClientMessage) -> Result<()> {
    let mut line = serde_json::to_vec(message).context("Failed to encode daemon message")?;
    line.push(b'\n');
    stream.write_all(&line).await?;
    stream.flush().await?;
    Ok(())
}
```

- [ ] **Step 4: Wire module in Tauri lib**

Add near the top of `crates/iota-desktop/src-tauri/src/lib.rs`:

```rust
mod daemon_client;
```

- [ ] **Step 5: Replace `AppState` engine fields**

Change `AppState` in `lib.rs` to remove `engines` and `pending_approvals`:

```rust
pub struct AppState {
    pub kanban_store: Arc<Mutex<SqliteKanbanStore>>,
    pub shadows_dir: PathBuf,
}
```

Remove `get_or_create_engine`, `is_api_key_configured`, direct `ApprovalRequest` import, and direct `IotaEngine` import from `lib.rs`.

- [ ] **Step 6: Replace chat/config Tauri commands**

Replace `get_config`, `save_api_key`, `submit_prompt`, and `handle_approval` in `lib.rs` with:

```rust
#[tauri::command]
async fn get_config() -> Result<iota_core::daemon::DesktopConfigSnapshot, String> {
    let messages = daemon_client::send_one(iota_core::daemon::DaemonClientMessage::GetConfig)
        .await
        .map_err(|e| e.to_string())?;
    messages
        .into_iter()
        .find_map(|message| match message {
            iota_core::daemon::DaemonServerMessage::ConfigSnapshot { config } => Some(config),
            _ => None,
        })
        .ok_or_else(|| "daemon did not return config snapshot".to_string())
}

#[tauri::command]
async fn save_backend_model(
    backend_str: String,
    model: iota_core::daemon::DesktopModelConfig,
) -> Result<iota_core::daemon::DesktopConfigSnapshot, String> {
    let messages = daemon_client::send_one(iota_core::daemon::DaemonClientMessage::SaveBackendModel {
        backend: backend_str,
        model,
    })
    .await
    .map_err(|e| e.to_string())?;
    messages
        .into_iter()
        .find_map(|message| match message {
            iota_core::daemon::DaemonServerMessage::ConfigSnapshot { config } => Some(config),
            _ => None,
        })
        .ok_or_else(|| "daemon did not return config snapshot".to_string())
}

#[tauri::command]
async fn submit_prompt(
    prompt: String,
    backend_str: String,
    window: tauri::Window,
) -> Result<String, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let cwd = std::env::current_dir().unwrap_or(home);
    let turn_id = uuid::Uuid::new_v4().to_string();
    daemon_client::start_turn(window, turn_id.clone(), cwd, backend_str, prompt)
        .await
        .map_err(|e| e.to_string())?;
    Ok(turn_id)
}

#[tauri::command]
async fn handle_approval(req_id: String, approved: bool) -> Result<(), String> {
    daemon_client::send_one(iota_core::daemon::DaemonClientMessage::RespondApproval {
        approval_id: req_id,
        approved,
    })
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}
```

- [ ] **Step 7: Update invoke handler and managed state**

In the `invoke_handler!` list, replace `save_api_key` with `save_backend_model`.

In `.manage(AppState { ... })`, remove `engines` and `pending_approvals`.

- [ ] **Step 8: Run desktop Rust test**

Run:

```bash
cargo test -p iota-desktop desktop_hello_uses_current_protocol_version -- --nocapture
```

Expected: PASS.

- [ ] **Step 9: Run all desktop Rust tests**

Run:

```bash
cargo test -p iota-desktop
```

Expected: PASS.

- [ ] **Step 10: Commit desktop daemon client**

Run:

```bash
git add crates/iota-desktop/src-tauri/src/daemon_client.rs crates/iota-desktop/src-tauri/src/lib.rs crates/iota-desktop/src-tauri/src/lib_tests.rs
git commit -m "feat: route desktop through daemon client"
```

Expected: One commit.

## Task 4: Add Frontend Turn Types And Reducer

**Files:**
- Create: `crates/iota-desktop/src/types.ts`
- Create: `crates/iota-desktop/src/turnReducer.ts`
- Create: `crates/iota-desktop/src/turnReducer.test.ts`
- Modify: `crates/iota-desktop/package.json`

- [ ] **Step 1: Add frontend test script**

Modify `crates/iota-desktop/package.json` scripts to include:

```json
"test": "node --test --import tsx src/*.test.ts"
```

Add `tsx` to dev dependencies:

```bash
cd crates/iota-desktop
npm install -D tsx
```

Expected: `package.json` and lockfile update.

- [ ] **Step 2: Add failing reducer tests**

Create `crates/iota-desktop/src/turnReducer.test.ts`:

```ts
import assert from "node:assert/strict";
import test from "node:test";
import { initialTurnsState, turnsReducer } from "./turnReducer";

test("text_chunk appends assistant text for the matching turn", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "text_chunk", turn_id: "turn-1", chunk: "hi" },
  });

  assert.equal(updated.turns["turn-1"].assistantText, "hi");
  assert.equal(updated.turns["turn-1"].status, "running");
});

test("approval_requested marks the turn as waiting for approval", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "approval_requested",
      turn_id: "turn-1",
      approval_id: "approval-1",
      tool_name: "shell",
      params: { command: "ls" },
    },
  });

  assert.equal(updated.turns["turn-1"].status, "waiting_approval");
  assert.equal(updated.turns["turn-1"].approvals[0].id, "approval-1");
});

test("turn_completed stores timing and completes the turn", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "codex",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "turn_completed",
      turn_id: "turn-1",
      text: "final",
      timing: { total_ms: 12 },
    },
  });

  assert.equal(updated.turns["turn-1"].status, "completed");
  assert.equal(updated.turns["turn-1"].assistantText, "final");
});

test("turn_failed preserves partial text and stores error", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "codex",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const chunked = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "text_chunk", turn_id: "turn-1", chunk: "partial" },
  });

  const failed = turnsReducer(chunked, {
    type: "daemon_message",
    message: { type: "turn_failed", turn_id: "turn-1", error: "boom" },
  });

  assert.equal(failed.turns["turn-1"].status, "failed");
  assert.equal(failed.turns["turn-1"].assistantText, "partial");
  assert.equal(failed.turns["turn-1"].error, "boom");
});
```

- [ ] **Step 3: Run reducer tests and verify they fail**

Run:

```bash
cd crates/iota-desktop
npm test
```

Expected: FAIL because `turnReducer.ts` does not exist.

- [ ] **Step 4: Add frontend types**

Create `crates/iota-desktop/src/types.ts`:

```ts
export type TurnStatus = "queued" | "running" | "waiting_approval" | "completed" | "failed" | "cancelled";

export type RuntimeEventView = {
  kind: string;
  data: unknown;
};

export type ApprovalView = {
  id: string;
  toolName: string;
  params: unknown;
  status: "pending" | "approved" | "denied";
};

export type ToolCallView = {
  id: string;
  name: string;
  arguments: unknown;
  ok?: boolean;
  result?: unknown;
};

export type DesktopTurn = {
  id: string;
  backend: string;
  cwd: string;
  status: TurnStatus;
  userPrompt: string;
  assistantText: string;
  events: RuntimeEventView[];
  toolCalls: ToolCallView[];
  approvals: ApprovalView[];
  timing?: unknown;
  usage?: unknown;
  error?: string;
};

export type DaemonServerMessage =
  | { type: "hello_accepted"; protocol_version: number }
  | { type: "protocol_error"; message: string }
  | { type: "turn_started"; turn_id: string }
  | { type: "text_chunk"; turn_id: string; chunk: string }
  | { type: "turn_event"; turn_id: string; event: RuntimeEventView }
  | { type: "approval_requested"; turn_id: string; approval_id: string; tool_name: string; params: unknown }
  | { type: "approval_responded"; approval_id: string; accepted: boolean }
  | { type: "turn_completed"; turn_id: string; text: string; timing: unknown }
  | { type: "turn_failed"; turn_id: string; error: string }
  | { type: "turn_cancelled"; turn_id: string }
  | { type: "config_snapshot"; config: DesktopConfigSnapshot }
  | { type: "backend_check_result"; backend: string; ok: boolean; details: string }
  | { type: "observability_summary"; summary: unknown };

export type DesktopModelConfig = {
  provider?: string;
  name?: string;
  base_url?: string;
  api_key_configured: boolean;
  api_key_update?: string;
};

export type DesktopBackendSnapshot = {
  backend: string;
  enabled: boolean;
  model?: DesktopModelConfig;
};

export type DesktopConfigSnapshot = {
  config_path: string;
  backends: Record<string, DesktopBackendSnapshot>;
};
```

- [ ] **Step 5: Add reducer implementation**

Create `crates/iota-desktop/src/turnReducer.ts`:

```ts
import type { DaemonServerMessage, DesktopTurn, RuntimeEventView, ToolCallView } from "./types";

export type TurnsState = {
  activeTurnId?: string;
  turns: Record<string, DesktopTurn>;
  order: string[];
  pendingError?: string;
};

export const initialTurnsState: TurnsState = {
  turns: {},
  order: [],
};

export type TurnsAction =
  | { type: "turn_started"; turnId: string; backend: string; cwd: string; prompt: string }
  | { type: "daemon_message"; message: DaemonServerMessage }
  | { type: "approval_decision"; approvalId: string; approved: boolean };

export function turnsReducer(state: TurnsState, action: TurnsAction): TurnsState {
  if (action.type === "turn_started") {
    const turn: DesktopTurn = {
      id: action.turnId,
      backend: action.backend,
      cwd: action.cwd,
      status: "queued",
      userPrompt: action.prompt,
      assistantText: "",
      events: [],
      toolCalls: [],
      approvals: [],
    };
    return {
      ...state,
      activeTurnId: action.turnId,
      order: [...state.order, action.turnId],
      turns: { ...state.turns, [action.turnId]: turn },
    };
  }

  if (action.type === "approval_decision") {
    return mapTurns(state, (turn) => ({
      ...turn,
      approvals: turn.approvals.map((approval) =>
        approval.id === action.approvalId
          ? { ...approval, status: action.approved ? "approved" : "denied" }
          : approval,
      ),
    }));
  }

  const message = action.message;
  if (message.type === "protocol_error") {
    return { ...state, pendingError: message.message };
  }
  if (!("turn_id" in message)) {
    return state;
  }

  const existing = state.turns[message.turn_id];
  if (!existing) return state;

  const updated = reduceTurn(existing, message);
  return {
    ...state,
    activeTurnId: message.turn_id,
    turns: { ...state.turns, [message.turn_id]: updated },
  };
}

function reduceTurn(turn: DesktopTurn, message: Extract<DaemonServerMessage, { turn_id: string }>): DesktopTurn {
  switch (message.type) {
    case "turn_started":
      return { ...turn, status: "running" };
    case "text_chunk":
      return { ...turn, status: "running", assistantText: turn.assistantText + message.chunk };
    case "turn_event":
      return applyRuntimeEvent({ ...turn, events: [...turn.events, message.event] }, message.event);
    case "approval_requested":
      return {
        ...turn,
        status: "waiting_approval",
        approvals: [
          ...turn.approvals,
          { id: message.approval_id, toolName: message.tool_name, params: message.params, status: "pending" },
        ],
      };
    case "turn_completed":
      return { ...turn, status: "completed", assistantText: message.text, timing: message.timing };
    case "turn_failed":
      return { ...turn, status: "failed", error: message.error };
    case "turn_cancelled":
      return { ...turn, status: "cancelled" };
  }
}

function applyRuntimeEvent(turn: DesktopTurn, event: RuntimeEventView): DesktopTurn {
  if (event.kind === "TokenUsage") {
    return { ...turn, usage: event.data };
  }
  if (event.kind === "ToolCall" && isObject(event.data)) {
    const toolCall: ToolCallView = {
      id: String(event.data.id ?? ""),
      name: String(event.data.name ?? ""),
      arguments: event.data.arguments,
    };
    return { ...turn, toolCalls: [...turn.toolCalls, toolCall] };
  }
  if (event.kind === "ToolResult" && isObject(event.data)) {
    return {
      ...turn,
      toolCalls: turn.toolCalls.map((tool) =>
        tool.id === event.data.id
          ? { ...tool, ok: Boolean(event.data.ok), result: event.data.result }
          : tool,
      ),
    };
  }
  return turn;
}

function mapTurns(state: TurnsState, f: (turn: DesktopTurn) => DesktopTurn): TurnsState {
  const turns: Record<string, DesktopTurn> = {};
  for (const id of Object.keys(state.turns)) {
    turns[id] = f(state.turns[id]);
  }
  return { ...state, turns };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
```

- [ ] **Step 6: Run reducer tests**

Run:

```bash
cd crates/iota-desktop
npm test
```

Expected: PASS.

- [ ] **Step 7: Commit frontend reducer**

Run:

```bash
git add crates/iota-desktop/package.json crates/iota-desktop/package-lock.json crates/iota-desktop/src/types.ts crates/iota-desktop/src/turnReducer.ts crates/iota-desktop/src/turnReducer.test.ts
git commit -m "feat: add desktop turn reducer"
```

Expected: One commit.

## Task 5: Build Chat Workbench And Right Inspector

**Files:**
- Create: `crates/iota-desktop/src/api.ts`
- Create: `crates/iota-desktop/src/components/ChatWorkbench.tsx`
- Create: `crates/iota-desktop/src/components/RightInspector.tsx`
- Modify: `crates/iota-desktop/src/App.tsx`
- Modify: `crates/iota-desktop/src/App.css`

- [ ] **Step 1: Create typed frontend API wrapper**

Create `crates/iota-desktop/src/api.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DaemonServerMessage, DesktopConfigSnapshot, DesktopModelConfig } from "./types";

export function submitPrompt(prompt: string, backend: string): Promise<string> {
  return invoke<string>("submit_prompt", { prompt, backendStr: backend });
}

export function getConfig(): Promise<DesktopConfigSnapshot> {
  return invoke<DesktopConfigSnapshot>("get_config");
}

export function saveBackendModel(backend: string, model: DesktopModelConfig): Promise<DesktopConfigSnapshot> {
  return invoke<DesktopConfigSnapshot>("save_backend_model", { backendStr: backend, model });
}

export function handleApproval(reqId: string, approved: boolean): Promise<void> {
  return invoke<void>("handle_approval", { reqId, approved });
}

export function listenDaemonMessages(callback: (message: DaemonServerMessage) => void): Promise<() => void> {
  return listen<DaemonServerMessage>("daemon-message", (event) => callback(event.payload));
}
```

- [ ] **Step 2: Add Right Inspector component**

Create `crates/iota-desktop/src/components/RightInspector.tsx`:

```tsx
import { AlertCircle, CheckCircle2, Clock, Terminal, Zap } from "lucide-react";
import type { DesktopTurn } from "../types";
import { handleApproval } from "../api";

type Props = {
  turn?: DesktopTurn;
  onApprovalDecision: (approvalId: string, approved: boolean) => void;
};

export function RightInspector({ turn, onApprovalDecision }: Props) {
  if (!turn) {
    return (
      <aside className="w-[360px] border-l border-white/10 bg-[#070a13] p-4 text-sm text-gray-500">
        No active turn
      </aside>
    );
  }

  const pendingApproval = turn.approvals.find((approval) => approval.status === "pending");

  return (
    <aside className="w-[360px] border-l border-white/10 bg-[#070a13] p-4 overflow-y-auto">
      <section className="mb-5">
        <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
          <Zap className="h-4 w-4 text-primary" />
          Turn Status
        </div>
        <div className="mt-2 rounded-md border border-white/10 bg-white/[0.03] p-3 text-xs text-gray-300">
          <div className="flex justify-between"><span>Status</span><span>{turn.status}</span></div>
          <div className="flex justify-between"><span>Backend</span><span>{turn.backend}</span></div>
          <div className="truncate text-gray-500" title={turn.cwd}>{turn.cwd}</div>
        </div>
      </section>

      {pendingApproval && (
        <section className="mb-5 rounded-md border border-rose-500/30 bg-rose-500/10 p-3">
          <div className="flex items-center gap-2 text-sm font-semibold text-rose-200">
            <AlertCircle className="h-4 w-4" />
            Approval Required
          </div>
          <div className="mt-2 text-xs text-gray-300">{pendingApproval.toolName}</div>
          <pre className="mt-2 max-h-36 overflow-auto rounded bg-black/40 p-2 text-[11px] text-gray-400">
            {JSON.stringify(pendingApproval.params, null, 2)}
          </pre>
          <div className="mt-3 flex justify-end gap-2">
            <button
              className="rounded border border-white/10 px-3 py-1.5 text-xs text-gray-300 hover:bg-white/10"
              onClick={async () => {
                await handleApproval(pendingApproval.id, false);
                onApprovalDecision(pendingApproval.id, false);
              }}
            >
              Deny
            </button>
            <button
              className="rounded bg-primary px-3 py-1.5 text-xs text-white"
              onClick={async () => {
                await handleApproval(pendingApproval.id, true);
                onApprovalDecision(pendingApproval.id, true);
              }}
            >
              Approve
            </button>
          </div>
        </section>
      )}

      <section className="mb-5">
        <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
          <Clock className="h-4 w-4 text-primary" />
          Timing & Usage
        </div>
        <pre className="mt-2 max-h-36 overflow-auto rounded-md border border-white/10 bg-white/[0.03] p-3 text-[11px] text-gray-400">
          {JSON.stringify({ timing: turn.timing, usage: turn.usage }, null, 2)}
        </pre>
      </section>

      <section className="mb-5">
        <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
          <Terminal className="h-4 w-4 text-primary" />
          Tool Calls
        </div>
        <div className="mt-2 space-y-2">
          {turn.toolCalls.length === 0 ? <div className="text-xs text-gray-600">No tool calls</div> : null}
          {turn.toolCalls.map((tool) => (
            <div key={tool.id} className="rounded-md border border-white/10 bg-white/[0.03] p-2 text-xs text-gray-300">
              <div className="flex items-center justify-between">
                <span>{tool.name}</span>
                {tool.ok === true ? <CheckCircle2 className="h-3.5 w-3.5 text-emerald-400" /> : null}
              </div>
              <pre className="mt-1 max-h-24 overflow-auto text-[10px] text-gray-500">
                {JSON.stringify(tool.arguments, null, 2)}
              </pre>
            </div>
          ))}
        </div>
      </section>

      <section>
        <div className="text-sm font-semibold text-gray-200">Runtime Events</div>
        <div className="mt-2 space-y-1">
          {turn.events.map((event, index) => (
            <details key={index} className="rounded border border-white/10 bg-white/[0.03] p-2 text-xs text-gray-400">
              <summary>{event.kind}</summary>
              <pre className="mt-2 max-h-32 overflow-auto text-[10px]">{JSON.stringify(event.data, null, 2)}</pre>
            </details>
          ))}
        </div>
      </section>
    </aside>
  );
}
```

- [ ] **Step 3: Add Chat Workbench component**

Create `crates/iota-desktop/src/components/ChatWorkbench.tsx`:

```tsx
import { useEffect, useMemo, useReducer, useState } from "react";
import { Cpu, Send } from "lucide-react";
import { listenDaemonMessages, submitPrompt } from "../api";
import { initialTurnsState, turnsReducer } from "../turnReducer";
import { RightInspector } from "./RightInspector";

const BACKENDS = ["gemini", "claude", "hermes", "codex", "opencode"];

export function ChatWorkbench() {
  const [state, dispatch] = useReducer(turnsReducer, initialTurnsState);
  const [backend, setBackend] = useState("gemini");
  const [input, setInput] = useState("");
  const activeTurn = state.activeTurnId ? state.turns[state.activeTurnId] : undefined;

  useEffect(() => {
    let disposed = false;
    listenDaemonMessages((message) => {
      if (!disposed) dispatch({ type: "daemon_message", message });
    });
    return () => {
      disposed = true;
    };
  }, []);

  const transcript = useMemo(() => state.order.map((id) => state.turns[id]), [state.order, state.turns]);

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault();
    const prompt = input.trim();
    if (!prompt) return;
    setInput("");
    const turnId = await submitPrompt(prompt, backend);
    dispatch({
      type: "turn_started",
      turnId,
      backend,
      cwd: "",
      prompt,
    });
  }

  return (
    <div className="flex h-screen bg-[#0b0f19] text-gray-100">
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-white/10 bg-[#070a13] px-5 py-3">
          <div className="flex items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-md bg-primary">
              <Cpu className="h-5 w-5 text-white" />
            </div>
            <div>
              <h1 className="text-sm font-semibold">Iota Desktop</h1>
              <p className="text-xs text-gray-500">Daemon-first local workbench</p>
            </div>
          </div>
          <select
            value={backend}
            onChange={(event) => setBackend(event.target.value)}
            className="rounded-md border border-white/10 bg-white/[0.04] px-3 py-2 text-xs text-gray-200"
          >
            {BACKENDS.map((item) => (
              <option key={item} value={item} className="bg-[#0b0f19]">
                {item}
              </option>
            ))}
          </select>
        </header>

        <div className="flex-1 overflow-y-auto p-5">
          {transcript.map((turn) => (
            <div key={turn.id} className="mb-6">
              <div className="mb-2 flex justify-end">
                <div className="max-w-[72ch] rounded-md bg-primary px-4 py-3 text-sm text-white">
                  {turn.userPrompt}
                </div>
              </div>
              <div className="flex justify-start">
                <div className="max-w-[88ch] rounded-md border border-white/10 bg-white/[0.04] px-4 py-3 text-sm leading-6 text-gray-200 whitespace-pre-wrap">
                  {turn.assistantText || (turn.status === "failed" ? turn.error : "Running...")}
                </div>
              </div>
            </div>
          ))}
        </div>

        <form onSubmit={onSubmit} className="border-t border-white/10 bg-[#070a13] p-4">
          <div className="flex gap-3">
            <textarea
              value={input}
              onChange={(event) => setInput(event.target.value)}
              rows={3}
              className="min-h-[76px] flex-1 resize-none rounded-md border border-white/10 bg-white/[0.04] px-3 py-2 text-sm text-gray-100 outline-none focus:border-primary"
              placeholder={`Send a prompt through ${backend}`}
            />
            <button
              type="submit"
              disabled={!input.trim()}
              className="flex h-[76px] w-12 items-center justify-center rounded-md bg-primary text-white disabled:opacity-50"
            >
              <Send className="h-5 w-5" />
            </button>
          </div>
        </form>
      </main>

      <RightInspector
        turn={activeTurn}
        onApprovalDecision={(approvalId, approved) =>
          dispatch({ type: "approval_decision", approvalId, approved })
        }
      />
    </div>
  );
}
```

- [ ] **Step 4: Replace App shell**

Replace `crates/iota-desktop/src/App.tsx` with:

```tsx
import { ChatWorkbench } from "./components/ChatWorkbench";
import "./App.css";

function App() {
  return <ChatWorkbench />;
}

export default App;
```

- [ ] **Step 5: Keep global CSS minimal**

Replace `crates/iota-desktop/src/App.css` with:

```css
@import "tailwindcss";

@theme {
  --color-primary: hsl(325, 90%, 55%);
}

body {
  margin: 0;
  background-color: #0b0f19;
  color: #f3f4f6;
  font-family: Inter, system-ui, Avenir, Helvetica, Arial, sans-serif;
  overflow: hidden;
}

button,
select,
textarea {
  font: inherit;
}
```

- [ ] **Step 6: Run frontend build**

Run:

```bash
cd crates/iota-desktop
npm run build
```

Expected: PASS.

- [ ] **Step 7: Commit Chat workbench**

Run:

```bash
git add crates/iota-desktop/src/api.ts crates/iota-desktop/src/components/ChatWorkbench.tsx crates/iota-desktop/src/components/RightInspector.tsx crates/iota-desktop/src/App.tsx crates/iota-desktop/src/App.css
git commit -m "feat: build chat-first desktop workbench"
```

Expected: One commit.

## Task 6: Add Config Panel Through Daemon API

**Files:**
- Create: `crates/iota-desktop/src/components/ConfigPanel.tsx`
- Modify: `crates/iota-desktop/src/components/ChatWorkbench.tsx`

- [ ] **Step 1: Add ConfigPanel component**

Create `crates/iota-desktop/src/components/ConfigPanel.tsx`:

```tsx
import { useEffect, useState } from "react";
import { getConfig, saveBackendModel } from "../api";
import type { DesktopConfigSnapshot } from "../types";

export function ConfigPanel() {
  const [config, setConfig] = useState<DesktopConfigSnapshot | null>(null);
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});

  useEffect(() => {
    getConfig().then(setConfig).catch((err) => console.error(err));
  }, []);

  if (!config) {
    return <div className="p-4 text-sm text-gray-500">Loading config...</div>;
  }

  return (
    <div className="h-full overflow-y-auto p-5">
      <h2 className="text-sm font-semibold text-gray-100">Configuration</h2>
      <p className="mt-1 text-xs text-gray-500">{config.config_path}</p>
      <div className="mt-5 space-y-3">
        {Object.values(config.backends).map((backend) => (
          <div key={backend.backend} className="rounded-md border border-white/10 bg-white/[0.03] p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm font-medium text-gray-200">{backend.backend}</div>
                <div className="text-xs text-gray-500">
                  {backend.model?.name ?? "No model"} · API key {backend.model?.api_key_configured ? "configured" : "missing"}
                </div>
              </div>
              <span className={backend.enabled ? "text-xs text-emerald-400" : "text-xs text-gray-500"}>
                {backend.enabled ? "Enabled" : "Disabled"}
              </span>
            </div>
            <div className="mt-3 flex gap-2">
              <input
                type="password"
                value={apiKeys[backend.backend] ?? ""}
                onChange={(event) => setApiKeys({ ...apiKeys, [backend.backend]: event.target.value })}
                placeholder="Update API key"
                className="flex-1 rounded-md border border-white/10 bg-black/20 px-3 py-2 text-xs text-gray-200 outline-none focus:border-primary"
              />
              <button
                className="rounded-md bg-primary px-3 py-2 text-xs text-white"
                onClick={async () => {
                  const updated = await saveBackendModel(backend.backend, {
                    api_key_configured: Boolean(backend.model?.api_key_configured),
                    api_key_update: apiKeys[backend.backend] ?? "",
                  });
                  setConfig(updated);
                  setApiKeys({ ...apiKeys, [backend.backend]: "" });
                }}
              >
                Save
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add simple nav in ChatWorkbench**

In `ChatWorkbench.tsx`, import `ConfigPanel`:

```tsx
import { ConfigPanel } from "./ConfigPanel";
```

Add view state near other hooks:

```tsx
const [view, setView] = useState<"chat" | "config">("chat");
```

Add two buttons to the header before the backend select:

```tsx
<button className="rounded px-3 py-1.5 text-xs text-gray-300 hover:bg-white/10" onClick={() => setView("chat")}>Chat</button>
<button className="rounded px-3 py-1.5 text-xs text-gray-300 hover:bg-white/10" onClick={() => setView("config")}>Config</button>
```

Wrap the transcript/composer region so config replaces only the main content:

```tsx
{view === "chat" ? (
  <>
    {/* existing transcript and composer */}
  </>
) : (
  <ConfigPanel />
)}
```

- [ ] **Step 3: Run frontend build**

Run:

```bash
cd crates/iota-desktop
npm run build
```

Expected: PASS.

- [ ] **Step 4: Commit config panel**

Run:

```bash
git add crates/iota-desktop/src/components/ConfigPanel.tsx crates/iota-desktop/src/components/ChatWorkbench.tsx
git commit -m "feat: add daemon-backed desktop config panel"
```

Expected: One commit.

## Task 7: Verification And Documentation

**Files:**
- Modify: `docs/architecture.md`
- Modify: `crates/iota-core/src/daemon/SKILL.md`
- Modify: `crates/iota-desktop/README.md`

- [ ] **Step 1: Update daemon docs**

In `crates/iota-core/src/daemon/SKILL.md`, add a responsibility bullet:

```markdown
- Provide two local JSON-line APIs: legacy CLI request/response and desktop streaming turns.
```

In `docs/architecture.md`, update the daemon row to mention desktop streaming API:

```markdown
| `crates/iota-core/src/daemon/` | Local daemon on `127.0.0.1:47661`; supports CLI request/response and desktop streaming turns; reuses `IotaEngine` per cwd through `EnginePool` | `engine`, `daemon::pool`, `daemon::proto` |
```

Replace `crates/iota-desktop/README.md` with:

```markdown
# iota-desktop

Tauri desktop workbench for iota-sympantos.

The desktop app is daemon-first:

- React renders the Chat-first workbench.
- Tauri commands connect to the local iota daemon.
- The daemon owns `EnginePool`, `IotaEngine`, ACP processes, approvals, config reads, and runtime events.
- `~/.i6/nimia.yaml` remains the only configuration source.

## Development

```bash
cd crates/iota-desktop
npm install
npm run tauri dev
```

## Verification

```bash
cargo test -p iota-core daemon
cargo test -p iota-desktop
cd crates/iota-desktop && npm test && npm run build
```
```

- [ ] **Step 2: Run Rust formatting**

Run:

```bash
cargo fmt --all --check
```

Expected: PASS.

- [ ] **Step 3: Run Rust tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 4: Run Rust clippy**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Run CLI check**

Run:

```bash
cargo run -p iota-cli -- check
```

Expected: command exits successfully and prints merged backend/config information without secrets.

- [ ] **Step 6: Run frontend checks**

Run:

```bash
cd crates/iota-desktop
npm test
npm run build
```

Expected: both commands PASS.

- [ ] **Step 7: Run desktop manually**

Run:

```bash
cd crates/iota-desktop
npm run tauri dev
```

Expected: Tauri app opens to the Chat-first workbench. Send one prompt with a configured backend and verify text streams into chat and turn details appear in the right inspector.

- [ ] **Step 8: Commit docs and verification fixes**

Run:

```bash
git add docs/architecture.md crates/iota-core/src/daemon/SKILL.md crates/iota-desktop/README.md
git commit -m "docs: document daemon-first desktop workflow"
```

Expected: One commit.

## Final Verification

After all tasks are complete, run:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
cd crates/iota-desktop && npm test && npm run build
```

Expected: all commands pass.

Manual desktop verification:

```bash
cd crates/iota-desktop
npm run tauri dev
```

Expected:

- app opens to Chat-first workbench
- daemon connects or autostarts
- prompt submission returns a turn id
- streaming text appears in transcript
- right inspector shows turn status, events, timing, tool calls, and token usage when available
- approval modal/inspector action can approve or deny a pending tool call
- config panel reads masked config from daemon and can save a backend API key to `~/.i6/nimia.yaml`
