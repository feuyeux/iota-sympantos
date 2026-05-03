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

use crate::acp::{AcpBackend, AcpPromptTiming};
use crate::config::NimiaConfig;
use crate::engine::IotaEngine;

pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:47661";

pub fn daemon_addr() -> String {
    std::env::var("IOTA_DAEMON_ADDR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DAEMON_ADDR.to_string())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonPromptRequest {
    pub backend: String,
    pub cwd: String,
    pub prompt: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub trace_timing: bool,
}

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonWarmRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backends: Vec<String>,
}

pub async fn run_daemon(
    config: NimiaConfig,
    addr: &str,
    timeout_ms: u64,
    warm_on_start: bool,
) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let engine = Arc::new(Mutex::new(IotaEngine::new(
        config,
        cwd.clone(),
        false,
        timeout_ms,
    )));
    if warm_on_start {
        eprintln!("warming enabled ACP backends before accepting daemon requests");
        let mut engine_guard = engine.lock().await;
        let warmed = engine_guard.warm_enabled_backends_in_cwd(cwd).await?;
        eprintln!("warmed {} ACP backend(s)", warmed);
    }
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
    send_request(addr, request).await
}

pub async fn send_warm(addr: &str, request: &DaemonWarmRequest) -> Result<DaemonPromptResponse> {
    send_request(addr, request).await
}

async fn send_request<T: Serialize + ?Sized>(
    addr: &str,
    request: &T,
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
    let request: serde_json::Value =
        serde_json::from_str(request_line.trim()).context("Failed to decode daemon request")?;
    let response = if request.get("type").and_then(serde_json::Value::as_str) == Some("warm") {
        let request: DaemonWarmRequest =
            serde_json::from_value(request).context("Failed to decode daemon warm request")?;
        handle_warm(request, engine).await
    } else {
        let request: DaemonPromptRequest =
            serde_json::from_value(request).context("Failed to decode daemon prompt request")?;
        handle_prompt(request, engine).await
    };
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
                timing: None,
                warmed: None,
            };
        }
    };
    let cwd = PathBuf::from(request.cwd);
    let mut engine = engine.lock().await;
    match engine
        .prompt_in_cwd_timed(backend, cwd, &request.prompt)
        .await
    {
        Ok(output) => DaemonPromptResponse {
            ok: true,
            text: Some(output.text),
            error: None,
            timing: Some(output.timing),
            warmed: None,
        },
        Err(err) => DaemonPromptResponse {
            ok: false,
            text: None,
            error: Some(err.to_string()),
            timing: None,
            warmed: None,
        },
    }
}

async fn handle_warm(
    request: DaemonWarmRequest,
    engine: Arc<Mutex<IotaEngine>>,
) -> DaemonPromptResponse {
    let cwd = PathBuf::from(request.cwd);
    let mut engine = engine.lock().await;
    let result = if request.backends.is_empty() {
        engine.warm_enabled_backends_in_cwd(cwd).await
    } else {
        warm_selected_backends(&mut engine, cwd, &request.backends).await
    };

    match result {
        Ok(warmed) => DaemonPromptResponse {
            ok: true,
            text: None,
            error: None,
            timing: None,
            warmed: Some(warmed),
        },
        Err(err) => DaemonPromptResponse {
            ok: false,
            text: None,
            error: Some(err.to_string()),
            timing: None,
            warmed: None,
        },
    }
}

async fn warm_selected_backends(
    engine: &mut IotaEngine,
    cwd: PathBuf,
    backends: &[String],
) -> Result<usize> {
    let mut warmed = 0;
    for backend in backends {
        let backend = AcpBackend::parse(backend)?;
        engine.warm_backend_in_cwd(backend, cwd.clone()).await?;
        warmed += 1;
    }
    Ok(warmed)
}
