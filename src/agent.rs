//! Agent service extension point.
//!
//! The agent surface keeps one [`crate::engine::IotaEngine`] alive across CLI
//! invocations so short `iota acp` commands can reuse ACP subprocesses.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::acp::AcpBackend;
use crate::config::NimiaConfig;
use crate::engine::IotaEngine;

pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:47661";

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonPromptRequest {
    pub backend: String,
    pub cwd: String,
    pub prompt: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonPromptResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn run_daemon(config: NimiaConfig, addr: &str, timeout_ms: u64) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let engine = Arc::new(Mutex::new(IotaEngine::new(config, cwd, false, timeout_ms)));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind daemon at {}", addr))?;
    eprintln!("iota agent daemon listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let engine = Arc::clone(&engine);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, engine).await {
                eprintln!("daemon request failed: {}", err);
            }
        });
    }
}

pub async fn send_prompt(
    addr: &str,
    request: &DaemonPromptRequest,
) -> Result<DaemonPromptResponse> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("Failed to connect to daemon at {}", addr))?;
    let mut line = serde_json::to_vec(request).context("Failed to encode daemon request")?;
    line.push(b'\n');
    stream.write_all(&line).await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).await?;
    if response.trim().is_empty() {
        anyhow::bail!("Daemon returned an empty response");
    }
    serde_json::from_str(response.trim()).context("Failed to decode daemon response")
}

async fn handle_connection(stream: TcpStream, engine: Arc<Mutex<IotaEngine>>) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let request: DaemonPromptRequest =
        serde_json::from_str(request_line.trim()).context("Failed to decode daemon request")?;
    let response = handle_prompt(request, engine).await;
    let mut stream = reader.into_inner();
    let mut line = serde_json::to_vec(&response).context("Failed to encode daemon response")?;
    line.push(b'\n');
    stream.write_all(&line).await?;
    stream.flush().await?;
    Ok(())
}

async fn handle_prompt(
    request: DaemonPromptRequest,
    engine: Arc<Mutex<IotaEngine>>,
) -> DaemonPromptResponse {
    let backend = match AcpBackend::parse(&request.backend) {
        Ok(backend) => backend,
        Err(err) => {
            return DaemonPromptResponse {
                ok: false,
                text: None,
                error: Some(err.to_string()),
            };
        }
    };
    let cwd = PathBuf::from(request.cwd);
    let mut engine = engine.lock().await;
    match engine.prompt_in_cwd(backend, cwd, &request.prompt).await {
        Ok(text) => DaemonPromptResponse {
            ok: true,
            text: Some(text),
            error: None,
        },
        Err(err) => DaemonPromptResponse {
            ok: false,
            text: None,
            error: Some(err.to_string()),
        },
    }
}
