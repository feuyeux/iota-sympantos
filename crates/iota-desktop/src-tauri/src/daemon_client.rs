use anyhow::{Context, Result, anyhow};
use iota_core::daemon::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonServerMessage, daemon_addr,
};
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

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
    let primary_addr = desktop_daemon_addr();
    if let Ok(stream) = connect_and_handshake(&primary_addr).await {
        return Ok(stream);
    }

    let _guard = autostart_lock().lock().await;
    let primary_addr = desktop_daemon_addr();
    if let Ok(stream) = connect_and_handshake(&primary_addr).await {
        return Ok(stream);
    }

    let fallback_addr = fallback_daemon_addr();
    set_desktop_daemon_addr(fallback_addr.clone());
    if let Ok(stream) = connect_and_handshake(&fallback_addr).await {
        return Ok(stream);
    }

    autostart_daemon(&fallback_addr).context("Failed to autostart daemon")?;
    wait_for_daemon(&fallback_addr).await
}

fn desktop_daemon_addr() -> String {
    DESKTOP_DAEMON_ADDR
        .get_or_init(|| RwLock::new(daemon_addr()))
        .read()
        .map(|addr| addr.clone())
        .unwrap_or_else(|_| daemon_addr())
}

fn set_desktop_daemon_addr(addr: String) {
    if let Ok(mut current) = DESKTOP_DAEMON_ADDR
        .get_or_init(|| RwLock::new(daemon_addr()))
        .write()
    {
        *current = addr;
    }
}

fn fallback_daemon_addr() -> String {
    std::env::var("IOTA_DESKTOP_DAEMON_ADDR")
        .ok()
        .map(|addr| addr.trim().to_string())
        .filter(|addr| !addr.is_empty())
        .unwrap_or_else(|| "127.0.0.1:47662".to_string())
}

static DESKTOP_DAEMON_ADDR: OnceLock<RwLock<String>> = OnceLock::new();

fn autostart_lock() -> &'static Mutex<()> {
    static AUTOSTART_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    AUTOSTART_LOCK.get_or_init(|| Mutex::new(()))
}

async fn wait_for_daemon(addr: &str) -> Result<TcpStream> {
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);
    let mut last_error = None;
    while started.elapsed() < timeout {
        match connect_and_handshake(addr).await {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                last_error = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    match last_error {
        Some(err) => anyhow::bail!(
            "Failed to connect to daemon at {} after autostart: {}",
            addr,
            err
        ),
        None => anyhow::bail!("Failed to connect to daemon at {} after autostart", addr),
    }
}

async fn connect_and_handshake(addr: &str) -> Result<TcpStream> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("Failed to connect to daemon at {}", addr))?;
    write_message(&mut stream, &hello_message()).await?;
    wait_for_hello(&mut stream).await?;
    Ok(stream)
}

fn autostart_daemon(addr: &str) -> Result<()> {
    let daemon_exe = locate_iota_cli().context("Failed to locate iota CLI for daemon autostart")?;
    std::process::Command::new(daemon_exe)
        .arg("__daemon")
        .env("IOTA_DAEMON_ADDR", addr)
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
    let current = std::env::current_exe().context("Failed to locate current executable")?;
    if let Some(dir) = current.parent() {
        let sibling = dir.join(exe_name);
        if sibling.exists() {
            return Ok(sibling);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(exe_name);
            if candidate.exists() {
                return Ok(candidate);
            }
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
    let bytes_read = reader.read_line(&mut line).await?;
    if bytes_read == 0 {
        return Err(anyhow!(
            "Connected daemon does not support the desktop protocol. Stop the existing iota daemon and restart iota-desktop."
        ));
    }
    let message: DaemonServerMessage = serde_json::from_str(line.trim()).with_context(|| {
        "Connected daemon returned an invalid desktop handshake response; restart the iota daemon"
    })?;
    match message {
        DaemonServerMessage::HelloAccepted { .. } => Ok(()),
        DaemonServerMessage::ProtocolError { message } => anyhow::bail!(message),
        other => anyhow::bail!("daemon returned unexpected handshake message: {:?}", other),
    }
}
