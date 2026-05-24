use anyhow::{Context, Result};
use iota_core::daemon::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonServerMessage, daemon_addr,
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
    wait_for_hello(&mut stream).await?;
    write_message(
        &mut stream,
        &DaemonClientMessage::StartTurn {
            turn_id: turn_id.clone(),
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
        let mut saw_terminal = false;
        loop {
            let bytes_read = match reader.read_line(&mut line).await {
                Ok(bytes_read) => bytes_read,
                Err(err) => {
                    emit_client_error(&window, Some(&turn_id), err.to_string());
                    return;
                }
            };
            if bytes_read == 0 {
                break;
            }
            match serde_json::from_str::<DaemonServerMessage>(line.trim()) {
                Ok(message) => {
                    saw_terminal = is_terminal_turn_message(&message) || saw_terminal;
                    let _ = window.emit("daemon-message", message);
                }
                Err(err) => {
                    emit_client_error(&window, Some(&turn_id), err.to_string());
                }
            }
            line.clear();
        }
        if !saw_terminal {
            emit_client_error(
                &window,
                Some(&turn_id),
                "daemon stream ended before the turn reached a terminal state".to_string(),
            );
        }
    });

    Ok(())
}

#[derive(Clone, serde::Serialize)]
struct DaemonClientErrorPayload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    message: String,
}

fn emit_client_error(window: &tauri::Window, turn_id: Option<&str>, message: String) {
    let _ = window.emit(
        "daemon-client-error",
        DaemonClientErrorPayload { turn_id, message },
    );
}

fn is_terminal_turn_message(message: &DaemonServerMessage) -> bool {
    matches!(
        message,
        DaemonServerMessage::TurnCompleted { .. }
            | DaemonServerMessage::TurnFailed { .. }
            | DaemonServerMessage::TurnCancelled { accepted: true, .. }
    )
}

pub async fn send_one(message: DaemonClientMessage) -> Result<Vec<DaemonServerMessage>> {
    let mut stream = connect_or_start().await?;
    write_message(&mut stream, &hello_message()).await?;
    wait_for_hello(&mut stream).await?;
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
                | Some(DaemonServerMessage::TurnCancelled { .. })
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
            TcpStream::connect(&addr).await.with_context(|| {
                format!(
                    "Failed to connect to daemon at {} after autostart: {}",
                    addr, first_err
                )
            })
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

async fn wait_for_hello(stream: &mut TcpStream) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let message: DaemonServerMessage = serde_json::from_str(line.trim())?;
    match message {
        DaemonServerMessage::HelloAccepted { .. } => Ok(()),
        DaemonServerMessage::ProtocolError { message } => anyhow::bail!(message),
        other => anyhow::bail!("daemon returned unexpected handshake message: {:?}", other),
    }
}
