use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command as TokioCommand};
use tokio::time::{Duration, timeout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
pub struct AcpRunOptions {
    pub backend: AcpBackend,
    pub cwd: PathBuf,
    pub prompt: String,
    pub show_native: bool,
    pub env: BTreeMap<String, String>,
    pub cleanup_dirs: Vec<PathBuf>,
    pub timeout_ms: u64,
    pub command_override: Option<(String, Vec<String>)>,
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

#[derive(Debug, Deserialize)]
struct AcpWireMessage {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<AcpWireError>,
}

#[derive(Debug, Deserialize)]
struct AcpWireError {
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

pub fn parse_acp_args(args: &[String]) -> Result<AcpRunOptions> {
    let mut backend = AcpBackend::Codex;
    let mut cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut show_native = false;
    let mut timeout_ms = 5_000_u64;
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
            "--timeout-ms" => {
                index += 1;
                let value = args.get(index).context("--timeout-ms requires a value")?;
                timeout_ms = value.parse().context("--timeout-ms must be an integer")?;
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
        env: BTreeMap::new(),
        cleanup_dirs: Vec::new(),
        timeout_ms,
        command_override: None,
    })
}

pub fn print_acp_help() {
    println!(
        "Usage:\n  iota-sympantos acp [backend] [options] <prompt>\n\nOptions:\n  -b, --backend <name>   claude-code | codex | gemini | hermes | opencode\n      --cwd <path>       Working directory for session/new\n      --show-native      Print raw ACP messages to stderr\n      --timeout-ms <ms>  ACP response timeout (default: 5000)\n  -h, --help             Show this help\n\nExamples:\n  iota-sympantos acp codex \"What is 2+2?\"\n  iota-sympantos acp --backend gemini --cwd D:\\\\coding\\\\creative \"Summarize this repo\""
    );
}

pub async fn run_acp_prompt(options: AcpRunOptions) -> Result<()> {
    let result = run_acp_prompt_inner(&options).await;
    for dir in &options.cleanup_dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
    result
}

async fn run_acp_prompt_inner(options: &AcpRunOptions) -> Result<()> {
    let (executable, args) = if let Some((executable, args)) = &options.command_override {
        (executable.clone(), args.clone())
    } else {
        let (executable, args) = options.backend.command();
        (
            executable.to_string(),
            args.into_iter().map(str::to_string).collect(),
        )
    };
    eprintln!(
        "Starting ACP backend '{}' with command: {} {}",
        options.backend,
        executable,
        args.join(" ")
    );

    let mut child = TokioCommand::new(executable)
        .args(&args)
        .envs(&options.env)
        .current_dir(&options.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to start ACP backend '{}'", options.backend))?;

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
            if !line.trim().is_empty() {
                eprintln!("[acp stderr] {}", line);
            }
        }
    });

    let mut lines = BufReader::new(stdout).lines();
    let run_result = async {
        send_request(
            &mut stdin,
            "init-0",
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "iota-sympantos", "version": env!("CARGO_PKG_VERSION") }
            }),
        )
        .await?;

        wait_for_response(
            &mut lines,
            "init-0",
            options.show_native,
            options.timeout_ms,
        )
        .await
        .context("ACP initialize failed")?;

        send_request(
            &mut stdin,
            "session:new",
            "session/new",
            session_new_params(options.backend, &options.cwd),
        )
        .await?;

        let session_result = wait_for_response(
            &mut lines,
            "session:new",
            options.show_native,
            options.timeout_ms,
        )
        .await
        .context("ACP session/new failed")?;
        let session_id = session_result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("ACP session/new result did not include sessionId")?;

        send_request(
            &mut stdin,
            "prompt:0",
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": options.prompt }]
            }),
        )
        .await?;

        read_prompt_events(&mut lines, &mut stdin, options).await
    }
    .await;

    let _ = stdin.shutdown().await;
    if timeout(Duration::from_millis(100), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
    run_result
}

async fn read_prompt_events<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    stdin: &mut ChildStdin,
    options: &AcpRunOptions,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    read_prompt_events_for_id(
        lines,
        stdin,
        options.show_native,
        options.timeout_ms,
        "prompt:0",
    )
    .await
}

async fn read_prompt_events_for_id<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    stdin: &mut ChildStdin,
    show_native: bool,
    timeout_ms: u64,
    expected_prompt_id: &str,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut printed_stream = false;
    loop {
        let Some(line) = read_next_line(lines, timeout_ms, "ACP prompt response timed out").await?
        else {
            break;
        };
        let message = parse_message_line(&line, show_native)?;

        if let Some(error) = &message.error {
            bail!(format_acp_error(error));
        }

        if is_response_id(&message, expected_prompt_id) {
            if let Some(result) = &message.result {
                if let Some(text) = extract_text(result) {
                    print!("{}", text);
                    io::stdout().flush()?;
                }
                if is_terminal_result(result) {
                    break;
                }
            }
        }

        let Some(method) = message.method.as_deref() else {
            continue;
        };

        match method {
            "session/update" | "session_update" => {
                if let Some(text) = text_from_session_update(message.params.as_ref()) {
                    printed_stream = true;
                    print!("{}", text);
                    io::stdout().flush()?;
                }
            }
            "session/complete" | "session_complete" => {
                if !printed_stream {
                    if let Some(text) = message.params.as_ref().and_then(extract_final_text) {
                        print!("{}", text);
                    }
                }
                println!();
                break;
            }
            "session/request_permission" | "request_permission" | "permission/request" => {
                answer_permission_request(stdin, &message).await?;
            }
            _ => {
                if show_native {
                    eprintln!("[acp native] {}", line);
                }
            }
        }
    }
    Ok(())
}

pub struct AcpClient {
    backend: AcpBackend,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    child: Child,
    session_id: String,
    show_native: bool,
    timeout_ms: u64,
    prompt_counter: u64,
}

impl AcpClient {
    pub async fn start(
        backend: AcpBackend,
        cwd: PathBuf,
        env: BTreeMap<String, String>,
        command_override: Option<(String, Vec<String>)>,
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
        eprintln!(
            "Starting warm ACP backend '{}' with command: {} {}",
            backend,
            executable,
            args.join(" ")
        );

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
                if !line.trim().is_empty() {
                    eprintln!("[acp stderr] {}", line);
                }
            }
        });

        let mut lines = BufReader::new(stdout).lines();
        send_request(
            &mut stdin,
            "init-0",
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "iota-sympantos", "version": env!("CARGO_PKG_VERSION") }
            }),
        )
        .await?;
        wait_for_response(&mut lines, "init-0", show_native, timeout_ms)
            .await
            .context("ACP initialize failed")?;

        send_request(
            &mut stdin,
            "session:new",
            "session/new",
            session_new_params(backend, &cwd),
        )
        .await?;
        let session_result = wait_for_response(&mut lines, "session:new", show_native, timeout_ms)
            .await
            .context("ACP session/new failed")?;
        let session_id = session_result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("ACP session/new result did not include sessionId")?;

        Ok(Self {
            backend,
            stdin,
            lines,
            child,
            session_id,
            show_native,
            timeout_ms,
            prompt_counter: 0,
        })
    }

    pub async fn prompt(&mut self, prompt: &str) -> Result<()> {
        self.prompt_counter += 1;
        let id = format!("prompt:{}", self.prompt_counter);
        send_request(
            &mut self.stdin,
            id.clone(),
            "session/prompt",
            json!({
                "sessionId": self.session_id,
                "prompt": [{ "type": "text", "text": prompt }]
            }),
        )
        .await?;
        read_prompt_events_for_id(
            &mut self.lines,
            &mut self.stdin,
            self.show_native,
            self.timeout_ms,
            &id,
        )
        .await
    }

    pub async fn shutdown(mut self) {
        let _ = self.stdin.shutdown().await;
        if timeout(Duration::from_millis(100), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }

    pub fn backend(&self) -> AcpBackend {
        self.backend
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
    line.push(b'\n');
    stdin
        .write_all(&line)
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
        .write_all(&line)
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
            "ACP backend timed out waiting for response '{}'",
            expected_id
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

async fn read_next_line<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    timeout_ms: u64,
    message: &str,
) -> Result<Option<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    read_next_line_with_duration(lines, Duration::from_millis(timeout_ms), message).await
}

async fn read_next_line_with_duration<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    duration: Duration,
    message: &str,
) -> Result<Option<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    timeout(duration, lines.next_line())
        .await
        .map_err(|_| anyhow!(message.to_string()))?
        .context("Failed to read ACP stdout")
}

fn parse_message_line(line: &str, show_native: bool) -> Result<AcpWireMessage> {
    if show_native {
        eprintln!("[acp <=] {}", line);
    }
    serde_json::from_str::<AcpWireMessage>(line)
        .with_context(|| format!("ACP backend emitted non-JSON line: {}", line))
}

fn is_response_id(message: &AcpWireMessage, expected: &str) -> bool {
    match message.id.as_ref() {
        Some(Value::String(id)) => id == expected,
        Some(Value::Number(id)) => id.to_string() == expected,
        _ => false,
    }
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

fn extract_text(value: &Value) -> Option<String> {
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

fn session_new_params(_backend: AcpBackend, cwd: &PathBuf) -> Value {
    let cwd = cwd.display().to_string();
    json!({ "cwd": cwd, "mcpServers": [] })
}

async fn answer_permission_request(stdin: &mut ChildStdin, message: &AcpWireMessage) -> Result<()> {
    let id = message
        .id
        .clone()
        .or_else(|| {
            message
                .params
                .as_ref()
                .and_then(|params| params.get("requestId").cloned())
        })
        .context("ACP permission request did not include an id or requestId")?;

    let tool_name = message
        .params
        .as_ref()
        .and_then(|params| {
            params
                .get("toolName")
                .or_else(|| params.get("name"))
                .and_then(Value::as_str)
        })
        .unwrap_or("tool");
    let approved = prompt_yes_no(&format!("Approve ACP tool request '{}'? ", tool_name)).await?;
    send_response(stdin, id, json!({ "approved": approved })).await
}

async fn prompt_yes_no(message: &str) -> Result<bool> {
    let message = message.to_string();
    tokio::task::spawn_blocking(move || -> Result<bool> {
        print!("{}(y/n): ", message);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().eq_ignore_ascii_case("y"))
    })
    .await
    .context("Permission prompt task failed")?
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
    matches!(
        value,
        "claude"
            | "claude-code"
            | "claudecode"
            | "codex"
            | "gemini"
            | "gemini-cli"
            | "hermes"
            | "hermes-agent"
            | "opencode"
            | "open-code"
    )
}

fn format_acp_error(error: &AcpWireError) -> String {
    let mut text = error.message.clone();
    if let Some(code) = error.code {
        text = format!("ACP error {}: {}", code, text);
    }
    if let Some(data) = &error.data {
        text = format!("{} ({})", text, data);
    }
    text
}
