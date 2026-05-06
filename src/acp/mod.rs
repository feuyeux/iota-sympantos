use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command as TokioCommand};
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

pub mod permission;
pub mod session;
pub mod wire;

use crate::mcp::router;
use crate::runtime_event::{self, RuntimeEvent};
use permission as acp_permission;
use session::{AcpMcpServer, AcpSessionOptions, session_new_params_with_options};
use wire::{AcpWireMessage, format_acp_error, is_response_id, parse_message_line, read_next_line};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AcpBackend {
    ClaudeCode,
    Codex,
    Gemini,
    Hermes,
    OpenCode,
}

pub const ALL_BACKENDS: [AcpBackend; 5] = [
    AcpBackend::ClaudeCode,
    AcpBackend::Codex,
    AcpBackend::Gemini,
    AcpBackend::Hermes,
    AcpBackend::OpenCode,
];

pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

impl AcpBackend {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "claude" | "claude-code" | "claudecode" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "gemini" | "gemini-cli" => Ok(Self::Gemini),
            "hermes" | "hermes-agent" => Ok(Self::Hermes),
            "opencode" | "open-code" => Ok(Self::OpenCode),
            other => bail!(
                "Unknown ACP backend '{}'. Expected one of: claude-code, codex, gemini, hermes, opencode",
                other
            ),
        }
    }

    pub fn command(self) -> (&'static str, Vec<&'static str>) {
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        match self {
            Self::ClaudeCode => (
                npx,
                vec!["-y", "@agentclientprotocol/claude-agent-acp@latest"],
            ),
            Self::Codex => (npx, vec!["-y", "@zed-industries/codex-acp@latest"]),
            Self::Gemini => (npx, vec!["-y", "@google/gemini-cli@latest", "--acp"]),
            Self::Hermes => ("hermes", vec!["acp"]),
            Self::OpenCode => (npx, vec!["-y", "opencode-ai@latest", "acp"]),
        }
    }
}

impl fmt::Display for AcpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Hermes => "hermes",
            Self::OpenCode => "opencode",
        };
        f.write_str(value)
    }
}

pub struct AcpRunOptions {
    pub backend: AcpBackend,
    pub cwd: PathBuf,
    pub prompt: String,
    pub show_native: bool,
    pub use_daemon: bool,
    pub trace: bool,
    pub trace_timing: bool,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpStartupTiming {
    pub process_spawn_ms: u64,
    pub init_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpPromptTiming {
    pub client_started: bool,
    pub process_spawned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_spawn_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_ms: Option<u64>,
    pub session_reused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_new_ms: Option<u64>,
    pub prompt_ms: u64,
    pub total_ms: u64,
}

pub struct AcpPromptOutput {
    pub text: String,
    pub timing: AcpPromptTiming,
    pub backend_session_id: Option<String>,
    pub execution_id: Option<String>,
    pub events: Vec<RuntimeEvent>,
}

impl AcpPromptOutput {
    pub fn synthetic(text: String) -> Self {
        Self {
            text,
            backend_session_id: None,
            execution_id: None,
            events: Vec::new(),
            timing: AcpPromptTiming {
                client_started: false,
                process_spawned: false,
                process_spawn_ms: None,
                init_ms: None,
                session_reused: false,
                session_new_ms: None,
                prompt_ms: 0,
                total_ms: 0,
            },
        }
    }
}

struct AcpSessionResolution {
    session_id: String,
    reused: bool,
    session_new_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: String,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    result: Value,
}

pub fn parse_acp_args(args: &[String]) -> Result<AcpRunOptions> {
    let mut backend = AcpBackend::Codex;
    let mut cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut show_native = false;
    let mut use_daemon = false;
    let mut trace = false;
    let mut trace_timing = false;
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    let mut prompt_parts = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "-b" | "--backend" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("--backend requires a backend name")?;
                backend = AcpBackend::parse(value)?;
            }
            "--cwd" => {
                index += 1;
                let value = args.get(index).context("--cwd requires a path")?;
                cwd = PathBuf::from(value);
            }
            "--show-native" => {
                show_native = true;
            }
            "-d" | "--daemon" | "--require-daemon" => {
                use_daemon = true;
            }
            "--trace" => {
                trace = true;
            }
            "--trace-timing" => {
                trace_timing = true;
            }
            "--timeout-ms" => {
                index += 1;
                let value = args.get(index).context("--timeout-ms requires a value")?;
                timeout_ms = value.parse().context("--timeout-ms must be an integer")?;
                if timeout_ms == 0 {
                    bail!("--timeout-ms must be greater than 0");
                }
            }
            "-h" | "--help" => {
                print_acp_help();
                std::process::exit(0);
            }
            "--" => {
                prompt_parts.extend(args[index + 1..].iter().cloned());
                break;
            }
            value if is_backend_alias(value) && prompt_parts.is_empty() => {
                backend = AcpBackend::parse(value)?;
            }
            value => {
                prompt_parts.push(value.to_string());
            }
        }
        index += 1;
    }

    let prompt = if prompt_parts.is_empty() {
        read_prompt_from_stdin()?
    } else {
        prompt_parts.join(" ")
    };

    if prompt.trim().is_empty() {
        bail!("ACP prompt is empty");
    }

    Ok(AcpRunOptions {
        backend,
        cwd,
        prompt,
        show_native,
        use_daemon,
        trace,
        trace_timing,
        timeout_ms,
    })
}

pub fn print_acp_help() {
    println!(
        "Usage:\n  iota run [backend] [options] <prompt>\n\nOptions:\n  -b, --backend <name>   claude-code | codex | gemini | hermes | opencode\n      --cwd <path>       Working directory for session/new\n      --show-native      Print raw ACP messages to stderr\n  -d, --daemon           Route through daemon; starts it silently if needed\n      --trace            Print normalized skill/tool trace events to stderr\n      --trace-timing     Print route and ACP phase timings to stderr as JSON\n      --timeout-ms <ms>  ACP response timeout (default: 30000)\n  -h, --help             Show this help\n\nExamples:\n  iota run codex \"What is 2+2?\"\n  iota run --daemon --trace-timing codex \"What is 2+2?\"\n  iota run --backend gemini --cwd D:\\\\coding\\\\creative \"Summarize this repo\""
    );
}

async fn read_prompt_events_for_id<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    stdin: &mut ChildStdin,
    backend: AcpBackend,
    tool_whitelist: &[String],
    show_native: bool,
    timeout_ms: u64,
    expected_prompt_id: &str,
    stream_tx: Option<&mpsc::Sender<String>>,
    execution_id: Option<&str>,
) -> Result<(String, Vec<RuntimeEvent>)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut output = String::new();
    let mut events = Vec::new();
    let mut streamed = false;
    let timeout_message = format!("ACP prompt timed out after {}ms", timeout_ms);
    loop {
        let Some(line) = read_next_line(lines, timeout_ms, &timeout_message).await? else {
            break;
        };
        let message = parse_message_line(&line, show_native)?;

        if let Some(error) = &message.error {
            events.push(runtime_event::map_acp_error(
                error.message.clone(),
                error.code,
                error.data.clone(),
            ));
            bail!(format_acp_error(error));
        }

        if is_response_id(&message, expected_prompt_id) {
            if let Some(result) = &message.result {
                if let Some(text) = extract_text(result) {
                    output.push_str(&text);
                }
                if is_terminal_result(result) {
                    break;
                }
            }
        }

        let Some(method) = message.method.as_deref() else {
            continue;
        };

        for event in runtime_event::map_acp_events(method, message.params.as_ref()) {
            events.push(event);
        }

        match method {
            "session/update" | "session_update" => {
                if let Some(text) = text_from_session_update(message.params.as_ref()) {
                    streamed = true;
                    output.push_str(&text);
                    if let Some(tx) = stream_tx {
                        let _ = tx.try_send(text);
                    }
                }
            }
            "session/complete" | "session_complete" => {
                if !streamed {
                    if let Some(text) = message.params.as_ref().and_then(extract_final_text) {
                        output.push_str(&text);
                    }
                }
                break;
            }
            "session/request_permission" | "request_permission" | "permission/request" => {
                let id = permission_request_id(&message)?;
                let params = message.params.clone().unwrap_or(Value::Null);
                let decision = acp_permission::answer_permission_request(
                    stdin,
                    id,
                    params,
                    execution_id,
                    backend,
                    tool_whitelist,
                )
                .await?;
                events.push(RuntimeEvent::ApprovalDecision(decision));
            }
            _ => {
                if let (Some(id), Some(result)) = (
                    message.id.clone(),
                    router::try_intercept_tool_call(method, message.params.as_ref()),
                ) {
                    let result = result.unwrap_or_else(|err| json!({"content":[{"type":"text","text":err.to_string()}],"isError":true}));
                    send_response(stdin, id, result).await?;
                    continue;
                }

                if show_native {
                    eprintln!("[acp native] {}", line);
                }
            }
        }
    }
    Ok((output, events))
}

pub struct AcpClient {
    backend: AcpBackend,
    cwd: PathBuf,
    session_id: Option<String>,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    child: Child,
    show_native: bool,
    timeout_ms: u64,
    prompt_counter: u64,
    startup_timing: AcpStartupTiming,
    mcp_servers: Vec<AcpMcpServer>,
    session_options: AcpSessionOptions,
    tool_whitelist: Vec<String>,
    /// When set, each streamed output chunk is forwarded to the TUI.
    stream_tx: Option<mpsc::Sender<String>>,
}

impl AcpClient {
    pub async fn start(
        backend: AcpBackend,
        cwd: PathBuf,
        env: BTreeMap<String, String>,
        command_override: Option<(String, Vec<String>)>,
        mcp_servers: Vec<AcpMcpServer>,
        session_options: AcpSessionOptions,
        tool_whitelist: Vec<String>,
        show_native: bool,
        timeout_ms: u64,
    ) -> Result<Self> {
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
        tracing::debug!(backend = %backend, command = %executable, "starting ACP backend process");
        let mut child = TokioCommand::new(executable)
            .args(&args)
            .envs(&env)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to start ACP backend '{}'", backend))?;
        let process_spawn_ms = elapsed_ms(spawn_started);
        tracing::debug!(backend = %backend, process_spawn_ms, "ACP backend process spawned");

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
                if show_native && !line.trim().is_empty() {
                    eprintln!("[acp stderr] {}", line);
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
        tracing::debug!(backend = %backend, init_ms, "ACP backend initialized");

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
            startup_timing: AcpStartupTiming {
                process_spawn_ms,
                init_ms,
            },
            mcp_servers,
            session_options,
            tool_whitelist,
            stream_tx: None,
        })
    }

    pub fn set_stream_sender(&mut self, tx: Option<mpsc::Sender<String>>) {
        self.stream_tx = tx;
    }

    pub async fn prompt_with_cwd_timed_for_execution(
        &mut self,
        cwd: &PathBuf,
        prompt: &str,
        execution_id: Option<&str>,
    ) -> Result<AcpPromptOutput> {
        let total_started = Instant::now();
        self.prompt_counter += 1;
        let result = timeout(Duration::from_millis(self.timeout_ms), async {
            let session = self.ensure_session_timed(cwd).await?;
            tracing::debug!(backend = %self.backend, session_reused = session.reused, session_new_ms = session.session_new_ms, "ACP session resolved");
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
                self.backend,
                &self.tool_whitelist,
                self.show_native,
                self.timeout_ms,
                &id,
                stream_tx.as_ref(),
                execution_id,
            )
            .await?;
            tracing::debug!(backend = %self.backend, prompt_ms = elapsed_ms(prompt_started), events = events.len(), "ACP prompt completed");
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
            timing: AcpPromptTiming {
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

    async fn ensure_session_timed(&mut self, cwd: &PathBuf) -> Result<AcpSessionResolution> {
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
        self.session_id = Some(session_id.clone());
        Ok(AcpSessionResolution {
            session_id,
            reused: false,
            session_new_ms: Some(elapsed_ms(session_started)),
        })
    }

    pub fn startup_timing(&self) -> AcpStartupTiming {
        self.startup_timing.clone()
    }

    pub fn set_timeout_ms(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    pub async fn shutdown(mut self) {
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

async fn send_request(
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
    tracing::debug!(
        method = method,
        "[acp =>] {}",
        String::from_utf8_lossy(&line)
    );
    line.push(b'\n');
    stdin
        .write_all(line.as_slice())
        .await
        .context("Failed to write ACP request")?;
    stdin.flush().await.context("Failed to flush ACP stdin")?;
    Ok(())
}

async fn send_response(stdin: &mut ChildStdin, id: Value, result: Value) -> Result<()> {
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

async fn wait_for_response<R>(
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

fn text_from_session_update(params: Option<&Value>) -> Option<String> {
    let params = params?;
    let update = params.get("update").unwrap_or(params);
    let session_update = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .and_then(Value::as_str);

    match session_update {
        Some("agent_message") | Some("agent_message_chunk") => extract_text(update),
        _ => None,
    }
}

fn extract_final_text(value: &Value) -> Option<String> {
    value
        .get("finalMessage")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| extract_text(value))
}

pub fn extract_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    for key in ["text", "content", "message", "result", "output"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }

    if let Some(content) = value.get("content").and_then(Value::as_object) {
        if let Some(text) = content.get("text").and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }

    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<String>();
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

fn is_terminal_result(result: &Value) -> bool {
    result.get("stopReason").and_then(Value::as_str).is_some() || extract_text(result).is_some()
}

fn permission_request_id(message: &AcpWireMessage) -> Result<Value> {
    message
        .id
        .clone()
        .or_else(|| {
            message
                .params
                .as_ref()
                .and_then(|params| params.get("requestId").cloned())
        })
        .context("ACP permission request did not include an id or requestId")
}

fn read_prompt_from_stdin() -> Result<String> {
    if io::stdin().is_terminal() {
        bail!("Missing ACP prompt. Pass a prompt argument or pipe text into stdin.");
    }

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("Failed to read prompt from stdin")?;
    Ok(input)
}

fn is_backend_alias(value: &str) -> bool {
    AcpBackend::parse(value).is_ok()
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
