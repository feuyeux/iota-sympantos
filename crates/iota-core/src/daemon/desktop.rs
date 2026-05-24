use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::acp::{AcpBackend, permission::ApprovalRequest};
use crate::config::{BackendConfig, NimiaConfig, backend_config, read_config, save_config};
use crate::daemon::pool::EnginePool;
use crate::daemon::proto::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonServerMessage, DesktopConfigSnapshot,
    apply_desktop_model_update,
};
use crate::store::observability::ObservabilityStore;

#[derive(Default, Clone)]
pub(crate) struct ApprovalRegistry {
    pending: Arc<Mutex<BTreeMap<String, PendingApproval>>>,
}

struct PendingApproval {
    turn_id: String,
    tx: oneshot::Sender<bool>,
}

impl ApprovalRegistry {
    pub async fn insert(&self, turn_id: String, approval_id: String, tx: oneshot::Sender<bool>) {
        self.pending
            .lock()
            .await
            .insert(approval_id, PendingApproval { turn_id, tx });
    }

    pub async fn respond(&self, approval_id: &str, approved: bool) -> bool {
        let pending = self.pending.lock().await.remove(approval_id);
        if let Some(pending) = pending {
            let _ = pending.tx.send(approved);
            true
        } else {
            false
        }
    }

    pub async fn deny_for_turn(&self, turn_id: &str) -> usize {
        let mut pending = self.pending.lock().await;
        let approval_ids = pending
            .iter()
            .filter(|(_, approval)| approval.turn_id == turn_id)
            .map(|(approval_id, _)| approval_id.clone())
            .collect::<Vec<_>>();
        let denied_count = approval_ids.len();
        for approval_id in approval_ids {
            if let Some(approval) = pending.remove(&approval_id) {
                let _ = approval.tx.send(false);
            }
        }
        denied_count
    }
}

#[derive(Default, Clone)]
pub(crate) struct TurnRegistry {
    active: Arc<Mutex<BTreeMap<String, ActiveTurn>>>,
}

#[derive(Clone)]
struct ActiveTurn {
    handle: Arc<tokio::task::JoinHandle<()>>,
    writer: Arc<Mutex<OwnedWriteHalf>>,
}

impl TurnRegistry {
    pub async fn insert(
        &self,
        turn_id: String,
        handle: tokio::task::JoinHandle<()>,
        writer: Arc<Mutex<OwnedWriteHalf>>,
    ) {
        self.active.lock().await.insert(
            turn_id,
            ActiveTurn {
                handle: Arc::new(handle),
                writer,
            },
        );
    }

    async fn remove(&self, turn_id: &str) -> Option<ActiveTurn> {
        self.active.lock().await.remove(turn_id)
    }

    pub async fn abort(&self, turn_id: &str) -> Option<Arc<Mutex<OwnedWriteHalf>>> {
        let turn = self.remove(turn_id).await?;
        turn.handle.abort();
        Some(turn.writer)
    }
}

pub(crate) async fn handle_desktop_connection<R>(
    first_message: DaemonClientMessage,
    reader: BufReader<R>,
    write_half: OwnedWriteHalf,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
    turns: TurnRegistry,
) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let writer = Arc::new(Mutex::new(write_half));
    let connection_turns = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut handshake_ok = false;
    if !matches!(first_message, DaemonClientMessage::Hello { .. }) {
        send_message(
            &writer,
            &DaemonServerMessage::ProtocolError {
                message: "desktop daemon hello is required before other messages".to_string(),
            },
        )
        .await?;
        return Ok(());
    }
    if matches!(
        first_message,
        DaemonClientMessage::Hello {
            protocol_version: DESKTOP_PROTOCOL_VERSION,
            ..
        }
    ) {
        handshake_ok = true;
    }
    handle_message(
        first_message,
        Arc::clone(&writer),
        Arc::clone(&engine_pool),
        approvals.clone(),
        turns.clone(),
        Arc::clone(&connection_turns),
    )
    .await?;
    if !handshake_ok {
        return Ok(());
    }

    let mut reader = reader;
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let message: DaemonClientMessage =
            serde_json::from_str(line.trim()).context("Failed to decode desktop daemon message")?;
        if !handshake_ok {
            send_message(
                &writer,
                &DaemonServerMessage::ProtocolError {
                    message: "desktop daemon hello with matching protocol version is required"
                        .to_string(),
                },
            )
            .await?;
            break;
        }
        if matches!(message, DaemonClientMessage::Hello { .. }) {
            handshake_ok = matches!(
                message,
                DaemonClientMessage::Hello {
                    protocol_version: DESKTOP_PROTOCOL_VERSION,
                    ..
                }
            );
        }
        handle_message(
            message,
            Arc::clone(&writer),
            Arc::clone(&engine_pool),
            approvals.clone(),
            turns.clone(),
            Arc::clone(&connection_turns),
        )
        .await?;
        line.clear();
    }
    cleanup_connection_turns(connection_turns, turns, approvals).await;
    Ok(())
}

async fn handle_message(
    message: DaemonClientMessage,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
    turns: TurnRegistry,
    connection_turns: Arc<Mutex<Vec<String>>>,
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
            start_turn(
                turn_id,
                cwd,
                backend,
                prompt,
                timeout_ms,
                writer,
                engine_pool,
                approvals,
                turns,
                connection_turns,
            )
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
            let accepted = cancel_turn(&turns, &approvals, &turn_id).await;
            send_message(
                &writer,
                &DaemonServerMessage::TurnCancelled { turn_id, accepted },
            )
            .await?;
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
            let (ok, details) = match AcpBackend::parse(&backend) {
                Ok(backend) => {
                    let config = read_config().context("Failed to read config")?;
                    let result = backend_check_result(&config, backend);
                    (result.ok, result.details)
                }
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
            let summary = observability_summary(cwd)?;
            send_message(
                &writer,
                &DaemonServerMessage::ObservabilitySummary { summary },
            )
            .await?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn start_turn(
    turn_id: String,
    cwd: PathBuf,
    backend: String,
    prompt: String,
    timeout_ms: Option<u64>,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
    turns: TurnRegistry,
    connection_turns: Arc<Mutex<Vec<String>>>,
) -> Result<()> {
    let backend = AcpBackend::parse(&backend)?;
    send_message(
        &writer,
        &DaemonServerMessage::TurnStarted {
            turn_id: turn_id.clone(),
        },
    )
    .await?;

    let engine = engine_pool.lock().await.engine_for(cwd.clone());
    let (stream_tx, mut stream_rx) = mpsc::channel::<String>(100);
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let streamed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let (approval_tx, mut approval_rx) = mpsc::channel::<ApprovalRequest>(10);
    crate::acp::permission::install_scoped_approval_channel(turn_id.clone(), approval_tx).await;

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

    let event_writer = Arc::clone(&writer);
    let event_turn_id = turn_id.clone();
    let event_seen = Arc::clone(&streamed_events);
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let Ok(key) = serde_json::to_string(&event) {
                event_seen.lock().await.push(key);
            }
            let _ = send_message(
                &event_writer,
                &DaemonServerMessage::TurnEvent {
                    turn_id: event_turn_id.clone(),
                    event: Box::new(event),
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
            approvals
                .insert(approval_turn_id.clone(), approval_id.clone(), reply_tx)
                .await;
            let sent = send_message(
                &approval_writer,
                &DaemonServerMessage::ApprovalRequested {
                    turn_id: approval_turn_id.clone(),
                    approval_id: approval_id.clone(),
                    tool_name: req.tool_name,
                    params: req.params,
                },
            )
            .await;
            if sent.is_err() {
                approvals.respond(&approval_id, false).await;
                let _ = req.reply.send(false);
                continue;
            }
            let decision = tokio::select! {
                decision = reply_rx => decision.unwrap_or(false),
                _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {
                    approvals.respond(&approval_id, false).await;
                    false
                }
            };
            let _ = req.reply.send(decision);
        }
    });

    let task_writer = Arc::clone(&writer);
    let task_turn_id = turn_id.clone();
    let task_turns = turns.clone();
    let task_streamed_events = Arc::clone(&streamed_events);
    let handle = tokio::spawn(async move {
        let result = {
            let mut engine = engine.lock().await;
            if let Some(timeout_ms) = timeout_ms {
                engine.set_acp_timeout_ms(timeout_ms);
            }
            engine.set_stream_output_sender(Some(stream_tx));
            engine.set_stream_event_sender(Some(event_tx));
            let result = engine.run(backend, cwd, &prompt, Some(&task_turn_id)).await;
            engine.set_stream_output_sender(None);
            engine.set_stream_event_sender(None);
            result
        };

        task_turns.remove(&task_turn_id).await;
        crate::acp::permission::remove_scoped_approval_channel(&task_turn_id).await;

        match result {
            Ok(output) => {
                let mut seen = task_streamed_events.lock().await;
                for event in output.events {
                    let key = serde_json::to_string(&event).unwrap_or_default();
                    if seen.iter().any(|sent| sent == &key) {
                        continue;
                    }
                    seen.push(key);
                    let _ = send_message(
                        &task_writer,
                        &DaemonServerMessage::TurnEvent {
                            turn_id: task_turn_id.clone(),
                            event: Box::new(event),
                        },
                    )
                    .await;
                }
                let _ = send_message(
                    &task_writer,
                    &DaemonServerMessage::TurnCompleted {
                        turn_id: task_turn_id,
                        text: output.text,
                        timing: output.timing,
                    },
                )
                .await;
            }
            Err(err) => {
                let _ = send_message(
                    &task_writer,
                    &DaemonServerMessage::TurnFailed {
                        turn_id: task_turn_id,
                        error: err.to_string(),
                    },
                )
                .await;
            }
        }
    });

    connection_turns.lock().await.push(turn_id.clone());
    turns.insert(turn_id, handle, Arc::clone(&writer)).await;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackendCheck {
    pub ok: bool,
    pub details: String,
}

pub(crate) fn backend_check_result(config: &NimiaConfig, backend: AcpBackend) -> BackendCheck {
    let Some(section) = backend_config(config, backend) else {
        return BackendCheck {
            ok: false,
            details: "backend section is missing".to_string(),
        };
    };
    if !section.enabled {
        return BackendCheck {
            ok: false,
            details: "backend is disabled".to_string(),
        };
    }
    let Some(acp) = section.acp.as_ref() else {
        return BackendCheck {
            ok: false,
            details: "missing acp config".to_string(),
        };
    };
    if acp.command.trim().is_empty() {
        return BackendCheck {
            ok: false,
            details: "missing acp.command".to_string(),
        };
    }
    if !api_key_configured(section) {
        return BackendCheck {
            ok: false,
            details: "missing API key".to_string(),
        };
    }
    BackendCheck {
        ok: true,
        details: "backend is configured".to_string(),
    }
}

fn api_key_configured(section: &BackendConfig) -> bool {
    section
        .model
        .as_ref()
        .and_then(|model| model.api_key.as_deref())
        .map(valid_api_key)
        .unwrap_or(false)
}

fn valid_api_key(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value != "<api-key>" && value != "YOUR_API_KEY"
}

async fn abort_turn(turns: &TurnRegistry, approvals: &ApprovalRegistry, turn_id: &str) -> bool {
    let Some(writer) = turns.abort(turn_id).await else {
        return false;
    };
    approvals.deny_for_turn(turn_id).await;
    crate::acp::permission::remove_scoped_approval_channel(turn_id).await;
    close_writer(&writer).await;
    true
}

async fn cancel_turn(turns: &TurnRegistry, approvals: &ApprovalRegistry, turn_id: &str) -> bool {
    let Some(writer) = turns.abort(turn_id).await else {
        return false;
    };
    approvals.deny_for_turn(turn_id).await;
    crate::acp::permission::remove_scoped_approval_channel(turn_id).await;
    let _ = send_message(
        &writer,
        &DaemonServerMessage::TurnCancelled {
            turn_id: turn_id.to_string(),
            accepted: true,
        },
    )
    .await;
    close_writer(&writer).await;
    true
}

async fn close_writer(writer: &Arc<Mutex<OwnedWriteHalf>>) {
    let mut writer = writer.lock().await;
    let _ = writer.shutdown().await;
}

async fn cleanup_connection_turns(
    connection_turns: Arc<Mutex<Vec<String>>>,
    turns: TurnRegistry,
    approvals: ApprovalRegistry,
) {
    let turn_ids = std::mem::take(&mut *connection_turns.lock().await);
    for turn_id in turn_ids {
        abort_turn(&turns, &approvals, &turn_id).await;
    }
}

async fn send_message(
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    message: &DaemonServerMessage,
) -> Result<()> {
    let mut line =
        serde_json::to_vec(message).context("Failed to encode desktop daemon message")?;
    line.push(b'\n');
    let mut writer = writer.lock().await;
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

fn observability_summary(cwd: Option<PathBuf>) -> Result<serde_json::Value> {
    let store = ObservabilityStore::default_path().and_then(|path| ObservabilityStore::open(&path));
    match store {
        Ok(store) => {
            let since_ts = crate::utils::now_ts() - 7 * 24 * 60 * 60;
            let token_summary = store.token_summary_since(since_ts)?;
            let recent_token_executions = store.recent_token_executions(10)?;
            Ok(serde_json::json!({
                "cwd": cwd,
                "window_secs": 7 * 24 * 60 * 60,
                "token_summary": token_summary,
                "recent_token_executions": recent_token_executions,
            }))
        }
        Err(err) => Ok(serde_json::json!({
            "cwd": cwd,
            "window_secs": 7 * 24 * 60 * 60,
            "token_summary": [],
            "recent_token_executions": [],
            "error": err.to_string(),
        })),
    }
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod desktop_tests;
