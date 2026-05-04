//! Agent service extension point.
//!
//! The agent surface keeps one [`crate::engine::IotaEngine`] alive across CLI
//! invocations so short `iota run` commands can reuse ACP subprocesses.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::acp::{AcpBackend, AcpPromptTiming};
use crate::config::{NimiaConfig, backend_config};
use crate::engine::IotaEngine;

pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:47661";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EngineKey {
    backend: AcpBackend,
    cwd: PathBuf,
}

struct EnginePool {
    config: NimiaConfig,
    show_native: bool,
    timeout_ms: u64,
    engines: BTreeMap<EngineKey, Arc<Mutex<IotaEngine>>>,
}

impl EnginePool {
    fn new(config: NimiaConfig, show_native: bool, timeout_ms: u64) -> Self {
        Self {
            config,
            show_native,
            timeout_ms,
            engines: BTreeMap::new(),
        }
    }

    fn engine_for(&mut self, backend: AcpBackend, cwd: PathBuf) -> Arc<Mutex<IotaEngine>> {
        let key = EngineKey { backend, cwd };
        let timeout_ms = self.timeout_ms;
        self.engines
            .entry(key)
            .or_insert_with(|| {
                Arc::new(Mutex::new(IotaEngine::new(
                    self.config.clone(),
                    self.show_native,
                    timeout_ms,
                )))
            })
            .clone()
    }

    fn all_engines(&self) -> Vec<Arc<Mutex<IotaEngine>>> {
        self.engines.values().cloned().collect()
    }

    fn config(&self) -> NimiaConfig {
        self.config.clone()
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
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
    let engine_pool = Arc::new(Mutex::new(EnginePool::new(config, false, timeout_ms)));
    if warm_on_start {
        eprintln!("warming enabled ACP backends before accepting daemon requests");
        let warmed = warm_all_backends(Arc::clone(&engine_pool), cwd.clone()).await?;
        eprintln!("warmed {} ACP backend(s)", warmed);
    }
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind daemon at {}", addr))?;
    eprintln!("iota agent daemon listening on {}", addr);
    eprintln!("Press Ctrl+C to shut down gracefully");

    let concurrency = Arc::new(Semaphore::new(8));

    let shutdown_token = CancellationToken::new();
    let shutdown_signal = shutdown_token.clone();

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                eprintln!("\nReceived Ctrl+C, shutting down daemon...");
                shutdown_signal.cancel();
            }
            Err(err) => {
                eprintln!("Failed to listen for Ctrl+C: {}", err);
            }
        }
    });

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                eprintln!("Shutting down ACP clients...");
                let engines = engine_pool.lock().await.all_engines();
                let mut clients_count = 0;
                for engine in engines {
                    let mut engine_guard = engine.lock().await;
                    clients_count += engine_guard.clients_count();
                    engine_guard.shutdown_all_clients().await;
                }
                eprintln!("Shut down {} ACP client(s)", clients_count);
                eprintln!("Daemon shutdown complete");
                return Ok(());
            }
            accept_result = listener.accept() => {
                let (stream, _) = accept_result?;
                let engine_pool = Arc::clone(&engine_pool);
                let permit = Arc::clone(&concurrency);
                tokio::spawn(async move {
                    let _permit = permit.acquire_owned().await;
                    if let Err(err) = handle_connection(stream, engine_pool).await {
                        eprintln!("daemon request failed: {}", err);
                    }
                });
            }
        }
    }
}

pub async fn send_prompt(
    addr: &str,
    request: &DaemonPromptRequest,
) -> Result<DaemonPromptResponse> {
    send_request_with_retry(addr, request, 2, 100).await
}

async fn send_request_with_retry<T: Serialize + ?Sized>(
    addr: &str,
    request: &T,
    max_retries: usize,
    retry_delay_ms: u64,
) -> Result<DaemonPromptResponse> {
    let mut last_error = None;

    for attempt in 0..=max_retries {
        match send_request(addr, request).await {
            Ok(response) => return Ok(response),
            Err(err) => {
                last_error = Some(err);
                if attempt < max_retries {
                    tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
                }
            }
        }
    }

    Err(last_error.unwrap())
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

async fn handle_connection(stream: TcpStream, engine_pool: Arc<Mutex<EnginePool>>) -> Result<()> {
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
}

async fn handle_prompt(
    request: DaemonPromptRequest,
    engine_pool: Arc<Mutex<EnginePool>>,
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
    let engine = engine_pool.lock().await.engine_for(backend, cwd.clone());
    let mut engine = engine.lock().await;
    if let Some(timeout_ms) = request.timeout_ms {
        if timeout_ms == 0 {
            return DaemonPromptResponse {
                ok: false,
                text: None,
                error: Some("timeout_ms must be greater than 0".to_string()),
                timing: None,
                warmed: None,
            };
        }
        engine.set_timeout_ms(timeout_ms);
    }
    match engine
        .prompt_in_cwd_timed_with_execution_id(
            backend,
            cwd,
            &request.prompt,
            request.execution_id.as_deref(),
        )
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
    engine_pool: Arc<Mutex<EnginePool>>,
) -> DaemonPromptResponse {
    let cwd = PathBuf::from(request.cwd);
    let result = if request.backends.is_empty() {
        warm_all_backends(Arc::clone(&engine_pool), cwd).await
    } else {
        warm_selected_backends(engine_pool, cwd, &request.backends).await
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

async fn warm_all_backends(engine_pool: Arc<Mutex<EnginePool>>, cwd: PathBuf) -> Result<usize> {
    let config = engine_pool.lock().await.config();
    let enabled = crate::acp::ALL_BACKENDS
        .iter()
        .copied()
        .filter(|backend| {
            backend_config(&config, *backend)
                .map(|section| section.enabled)
                .unwrap_or(false)
        })
        .map(|backend| backend.to_string())
        .collect::<Vec<_>>();
    warm_selected_backends(engine_pool, cwd, &enabled).await
}

async fn warm_selected_backends(
    engine_pool: Arc<Mutex<EnginePool>>,
    cwd: PathBuf,
    backends: &[String],
) -> Result<usize> {
    let mut warmed = 0;
    for backend in backends {
        let backend = AcpBackend::parse(backend)?;
        let engine = engine_pool.lock().await.engine_for(backend, cwd.clone());
        engine
            .lock()
            .await
            .warm_backend_in_cwd(backend, cwd.clone())
            .await?;
        warmed += 1;
    }
    Ok(warmed)
}
