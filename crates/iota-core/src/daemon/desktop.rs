use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::acp::{AcpBackend, permission::ApprovalRequest};
use crate::config::{read_config, save_config};
use crate::daemon::pool::EnginePool;
use crate::daemon::proto::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonServerMessage, DesktopConfigSnapshot,
    apply_desktop_model_update,
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
        handle_message(
            message,
            Arc::clone(&writer),
            Arc::clone(&engine_pool),
            approvals.clone(),
        )
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
            start_turn(
                turn_id,
                cwd,
                backend,
                prompt,
                timeout_ms,
                writer,
                engine_pool,
                approvals,
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
                            event: Box::new(event),
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
    let mut line =
        serde_json::to_vec(message).context("Failed to encode desktop daemon message")?;
    line.push(b'\n');
    let mut writer = writer.lock().await;
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod desktop_tests;
