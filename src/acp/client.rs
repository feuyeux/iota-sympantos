use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command as TokioCommand};
use tokio::time::{Duration, timeout};

use super::message::{JsonRpcRequest, JsonRpcResponse};
use super::stream_reader::read_prompt_events_for_id;
use super::session::session_new_params_with_options;
use super::types::{AcpClientStartOptions, AcpSessionResolution};
use super::util::{elapsed_ms, should_forward_backend_stderr};
use super::wire::{format_acp_error, is_response_id, parse_message_line, read_next_line};
use super::{AcpClient, AcpPromptOutput};

impl AcpClient {
    pub async fn start(options: AcpClientStartOptions) -> Result<Self> {
        let AcpClientStartOptions {
            backend,
            cwd,
            env,
            command_override,
            mcp_servers,
            session_options,
            tool_whitelist,
            show_native,
            timeout_ms,
        } = options;

        let (executable, args) = if let Some((executable, args)) = command_override {
            (executable, args)
        } else {
            let (executable, args) = backend.command();
            (
                executable.to_string(),
                args.into_iter().map(str::to_string).collect(),
            )
        };
        if show_native {
            eprintln!(
                "Starting ACP backend '{}' with command: {} {}",
                backend,
                executable,
                args.join(" ")
            );
        }

        let spawn_started = Instant::now();
        tracing::info!(backend = %backend, command = %executable, "acp.process.start");
        let mut child = TokioCommand::new(executable)
            .args(&args)
            .envs(&env)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to start ACP backend '{}'", backend))?;
        let process_spawn_ms = elapsed_ms(spawn_started);
        tracing::info!(backend = %backend, process_spawn_ms, "acp.process.spawned");

        let mut stdin = child
            .stdin
            .take()
            .context("ACP backend stdin was not piped")?;
        let stdout = child
            .stdout
            .take()
            .context("ACP backend stdout was not piped")?;
        let stderr = child
            .stderr
            .take()
            .context("ACP backend stderr was not piped")?;

        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                if show_native {
                    eprintln!("[acp stderr] {}", line);
                } else if should_forward_backend_stderr(&line) {
                    eprintln!("[acp stderr:{}] {}", backend, line);
                }
            }
        });

        let mut lines = BufReader::new(stdout).lines();
        let init_started = Instant::now();
        let init_result = timeout(Duration::from_millis(timeout_ms), async {
            send_request(
                &mut stdin,
                "init-0",
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {},
                    "clientInfo": { "name": "iota", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
            wait_for_response(&mut lines, "init-0", show_native, timeout_ms)
                .await
                .context("ACP initialize failed")?;
            Ok::<(), anyhow::Error>(())
        })
        .await
        .unwrap_or_else(|_| Err(anyhow!("ACP initialize timed out after {}ms", timeout_ms)));
        let init_ms = elapsed_ms(init_started);
        tracing::info!(backend = %backend, init_ms, "acp.init.done");

        if let Err(err) = init_result {
            let _ = stdin.shutdown().await;
            terminate_child_tree(&mut child).await;
            return Err(err);
        }

        Ok(Self {
            backend,
            cwd,
            session_id: None,
            stdin,
            lines,
            child,
            show_native,
            timeout_ms,
            prompt_counter: 0,
            startup_timing: super::AcpStartupTiming {
                process_spawn_ms,
                init_ms,
            },
            mcp_servers,
            session_options,
            tool_whitelist,
            stream_tx: None,
        })
    }

    pub fn set_stream_sender(&mut self, tx: Option<tokio::sync::mpsc::Sender<String>>) {
        self.stream_tx = tx;
    }

    pub async fn execute(
        &mut self,
        cwd: &std::path::PathBuf,
        prompt: &str,
        execution_id: Option<&str>,
    ) -> Result<AcpPromptOutput> {
        let total_started = Instant::now();
        self.prompt_counter += 1;
        let result = timeout(Duration::from_millis(self.timeout_ms), async {
            let session = self.ensure_session_timed(cwd).await?;
            tracing::info!(backend = %self.backend, session_reused = session.reused, session_new_ms = session.session_new_ms, "acp.session.resolved");
            let id = format!("prompt:{}", self.prompt_counter);
            let prompt_started = Instant::now();
            send_request(
                &mut self.stdin,
                id.clone(),
                "session/prompt",
                json!({
                    "sessionId": session.session_id,
                    "prompt": [{ "type": "text", "text": prompt }]
                }),
            )
            .await?;
            let stream_tx = self.stream_tx.clone();
            let (text, events) = read_prompt_events_for_id(
                &mut self.lines,
                &mut self.stdin,
                super::stream_reader::PromptReadOptions {
                    backend: self.backend,
                    tool_whitelist: &self.tool_whitelist,
                    show_native: self.show_native,
                    timeout_ms: self.timeout_ms,
                    expected_prompt_id: &id,
                    stream_tx: stream_tx.as_ref(),
                    execution_id,
                },
            )
            .await?;
            tracing::info!(backend = %self.backend, prompt_ms = elapsed_ms(prompt_started), events = events.len(), "acp.prompt.done");
            Ok::<_, anyhow::Error>((
                text,
                events,
                session.reused,
                session.session_new_ms,
                elapsed_ms(prompt_started),
                session.session_id,
            ))
        })
        .await
        .unwrap_or_else(|_| Err(anyhow!("ACP prompt timed out after {}ms", self.timeout_ms)))?;
        let (text, events, session_reused, session_new_ms, prompt_ms, backend_session_id) = result;
        Ok(AcpPromptOutput {
            text,
            backend_session_id: Some(backend_session_id),
            execution_id: None,
            events,
            timing: super::AcpPromptTiming {
                client_started: false,
                process_spawned: false,
                process_spawn_ms: None,
                init_ms: None,
                session_reused,
                session_new_ms,
                prompt_ms,
                total_ms: elapsed_ms(total_started),
            },
        })
    }

    async fn ensure_session_timed(
        &mut self,
        cwd: &std::path::PathBuf,
    ) -> Result<AcpSessionResolution> {
        if self.cwd == *cwd {
            if let Some(session_id) = self.session_id.clone() {
                return Ok(AcpSessionResolution {
                    session_id,
                    reused: true,
                    session_new_ms: None,
                });
            }
        } else {
            self.cwd = cwd.clone();
            self.session_id = None;
        }

        let session_request_id = format!("session:new:{}", self.prompt_counter);
        let session_started = Instant::now();
        send_request(
            &mut self.stdin,
            session_request_id.clone(),
            "session/new",
            session_new_params_with_options(
                self.backend,
                &self.cwd,
                &self.mcp_servers,
                self.session_options,
            ),
        )
        .await?;
        let session_result = wait_for_response(
            &mut self.lines,
            &session_request_id,
            self.show_native,
            self.timeout_ms,
        )
        .await
        .context("ACP session/new failed")?;
        let session_id = session_result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("ACP session/new result did not include sessionId")?;
        let elapsed_ms_val = elapsed_ms(session_started);
        self.session_id = Some(session_id.clone());
        tracing::info!(session_id = %session_id, session_new_ms = elapsed_ms_val, "acp.session.created");
        Ok(AcpSessionResolution {
            session_id,
            reused: false,
            session_new_ms: Some(elapsed_ms_val),
        })
    }

    pub fn startup_timing(&self) -> super::AcpStartupTiming {
        self.startup_timing.clone()
    }

    pub fn set_timeout_ms(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    pub async fn shutdown(mut self) {
        tracing::info!(backend = %self.backend, "acp.process.exit");
        let _ = self.stdin.shutdown().await;
        terminate_child_tree(&mut self.child).await;
    }
}

async fn terminate_child_tree(child: &mut Child) {
    let Some(_pid) = child.id() else {
        return;
    };

    if timeout(Duration::from_millis(100), child.wait())
        .await
        .is_ok()
    {
        return;
    }

    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &_pid.to_string(), "/T", "/F"])
            .output();
    }

    #[cfg(not(windows))]
    {
        let _ = child.kill().await;
    }

    if timeout(Duration::from_millis(1_500), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

pub(super) async fn send_request(
    stdin: &mut ChildStdin,
    id: impl Into<String>,
    method: &str,
    params: Value,
) -> Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0",
        id: id.into(),
        method,
        params,
    };
    let mut line = serde_json::to_vec(&request).context("Failed to serialize ACP request")?;
    line.push(b'\n');
    stdin
        .write_all(line.as_slice())
        .await
        .context("Failed to write ACP request")?;
    stdin.flush().await.context("Failed to flush ACP stdin")?;
    Ok(())
}

pub(super) async fn send_response(stdin: &mut ChildStdin, id: Value, result: Value) -> Result<()> {
    let response = JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result,
    };
    let mut line = serde_json::to_vec(&response).context("Failed to serialize ACP response")?;
    line.push(b'\n');
    stdin
        .write_all(line.as_slice())
        .await
        .context("Failed to write ACP response")?;
    stdin.flush().await.context("Failed to flush ACP stdin")?;
    Ok(())
}

pub(super) async fn wait_for_response<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    expected_id: &str,
    show_native: bool,
    timeout_ms: u64,
) -> Result<Value>
where
    R: tokio::io::AsyncRead + Unpin,
{
    while let Some(line) = read_next_line(
        lines,
        timeout_ms,
        &format!(
            "ACP backend timed out after {}ms waiting for response '{}'",
            timeout_ms, expected_id
        ),
    )
    .await?
    {
        let message = parse_message_line(&line, show_native)?;
        if !is_response_id(&message, expected_id) {
            continue;
        }
        if let Some(error) = message.error {
            bail!(format_acp_error(&error));
        }
        return Ok(message.result.unwrap_or(Value::Null));
    }
    Err(anyhow!(
        "ACP backend exited before response '{}' was received",
        expected_id
    ))
}
