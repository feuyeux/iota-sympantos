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
