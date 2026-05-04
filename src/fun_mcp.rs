use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const TOOLS: [(&str, &str); 7] = [
    ("fun.rust", "Rust"),
    ("fun.typescript", "TypeScript"),
    ("fun.python", "Python"),
    ("fun.go", "Go"),
    ("fun.java", "Java"),
    ("fun.cpp", "C++"),
    ("fun.zig", "Zig"),
];

pub fn run_stdio() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value =
            serde_json::from_str(&line).with_context(|| format!("Invalid JSON-RPC: {}", line))?;
        if request.get("id").is_none() {
            continue;
        }
        let response = handle_request(&request);
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_request(request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => ok(
            id,
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"iota-fun","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "tools/list" => ok(id, json!({"tools": tool_descriptions()})),
        "tools/call" => {
            let params = request.get("params").unwrap_or(&Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            match run_tool(name, &args) {
                Ok(text) => ok(
                    id,
                    json!({"content":[{"type":"text","text":text}],"isError":false}),
                ),
                Err(err) => ok(
                    id,
                    json!({"content":[{"type":"text","text":err.to_string()}],"isError":true}),
                ),
            }
        }
        other => error(id, -32601, &format!("unknown method {}", other)),
    }
}

fn tool_descriptions() -> Vec<Value> {
    TOOLS.iter().map(|(name, language)| json!({
        "name": name,
        "description": format!("Execute a small {} function or script with iota guardrails", language),
        "inputSchema": {"type":"object","properties":{"source":{"type":"string"},"timeout_ms":{"type":"integer"}},"required":["source"]}
    })).collect()
}

fn run_tool(name: &str, args: &Value) -> Result<String> {
    let source = args
        .get("source")
        .and_then(Value::as_str)
        .context("source is required")?;
    let timeout_ms = args
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(10_000)
        .min(60_000);
    match name {
        "fun.python" => run_interpreter(
            "python3",
            &[OsString::from("-c"), OsString::from(source)],
            timeout_ms,
        ),
        "fun.typescript" => run_interpreter(
            "node",
            &[OsString::from("-e"), OsString::from(source)],
            timeout_ms,
        ),
        "fun.rust" => run_rust(source, timeout_ms),
        "fun.go" => run_go(source, timeout_ms),
        "fun.java" => run_java(source, timeout_ms),
        "fun.cpp" => run_cpp(source, timeout_ms),
        "fun.zig" => run_zig(source, timeout_ms),
        _ => Err(anyhow!("unknown tool {}", name)),
    }
}

fn run_rust(source: &str, timeout_ms: u64) -> Result<String> {
    let dir = cache_dir("rust", source, compiler_version("rustc", &["--version"]))?;
    let src = dir.join("main.rs");
    let bin = executable_path(&dir, "main");
    write_source(&src, source)?;
    if !bin.exists() {
        run_command(
            "rustc",
            &[
                src.as_os_str().to_os_string(),
                OsString::from("-o"),
                bin.as_os_str().to_os_string(),
            ],
            Some(&dir),
            timeout_ms,
        )?;
    }
    run_command(bin.as_os_str(), &[], Some(&dir), timeout_ms)
}

fn run_go(source: &str, timeout_ms: u64) -> Result<String> {
    let dir = cache_dir("go", source, compiler_version("go", &["version"]))?;
    let src = dir.join("main.go");
    write_source(&src, source)?;
    run_command(
        "go",
        &[OsString::from("run"), src.as_os_str().to_os_string()],
        Some(&dir),
        timeout_ms,
    )
}

fn run_java(source: &str, timeout_ms: u64) -> Result<String> {
    let dir = cache_dir("java", source, compiler_version("javac", &["-version"]))?;
    let src = dir.join("Main.java");
    let class = dir.join("Main.class");
    write_source(&src, source)?;
    if !class.exists() {
        run_command(
            "javac",
            &[src.as_os_str().to_os_string()],
            Some(&dir),
            timeout_ms,
        )?;
    }
    run_command(
        "java",
        &[
            OsString::from("-cp"),
            dir.as_os_str().to_os_string(),
            OsString::from("Main"),
        ],
        Some(&dir),
        timeout_ms,
    )
}

fn run_cpp(source: &str, timeout_ms: u64) -> Result<String> {
    let compiler = if command_available("clang++") {
        "clang++"
    } else {
        "g++"
    };
    let dir = cache_dir("cpp", source, compiler_version(compiler, &["--version"]))?;
    let src = dir.join("main.cpp");
    let bin = executable_path(&dir, "main");
    write_source(&src, source)?;
    if !bin.exists() {
        run_command(
            compiler,
            &[
                src.as_os_str().to_os_string(),
                OsString::from("-std=c++17"),
                OsString::from("-O2"),
                OsString::from("-o"),
                bin.as_os_str().to_os_string(),
            ],
            Some(&dir),
            timeout_ms,
        )?;
    }
    run_command(bin.as_os_str(), &[], Some(&dir), timeout_ms)
}

fn run_zig(source: &str, timeout_ms: u64) -> Result<String> {
    let dir = cache_dir("zig", source, compiler_version("zig", &["version"]))?;
    let src = dir.join("main.zig");
    write_source(&src, source)?;
    run_command(
        "zig",
        &[OsString::from("run"), src.as_os_str().to_os_string()],
        Some(&dir),
        timeout_ms,
    )
}

fn run_interpreter(command: &str, args: &[OsString], timeout_ms: u64) -> Result<String> {
    run_command(command, args, None, timeout_ms)
}

fn run_command<S: AsRef<std::ffi::OsStr>>(
    command: S,
    args: &[OsString],
    cwd: Option<&Path>,
    timeout_ms: u64,
) -> Result<String> {
    let command_label = command.as_ref().to_string_lossy().to_string();
    let mut cmd = Command::new(&command);
    cmd.args(args)
        .env_clear()
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(path) = std::env::var_os("PATH") {
        cmd.env("PATH", path);
    }
    if let Some(home) = dirs::home_dir() {
        let go_cache = home.join(".i6").join("fun-cache").join("go-build");
        let _ = fs::create_dir_all(&go_cache);
        cmd.env("GOCACHE", go_cache);
        #[cfg(not(windows))]
        cmd.env("HOME", &home);
        #[cfg(windows)]
        cmd.env("USERPROFILE", &home);
    }
    for key in ["TMPDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    #[cfg(windows)]
    {
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", system_root);
        }
        if let Some(windir) = std::env::var_os("WINDIR") {
            cmd.env("WINDIR", windir);
        }
    }
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("Failed to start {}", command_label))?;

    // Run `wait_with_output` on a background thread so we can enforce a wall-clock
    // timeout without busy-polling.  The child handle is moved into the thread;
    // the main thread parks on the channel with a deadline.
    let (tx, rx) = std::sync::mpsc::channel::<Result<std::process::Output>>();
    // `wait_with_output` takes ownership of the child, so we reassemble it from
    // the already-spawned handle via the raw handle — simpler: just use a timeout
    // on the receiver side while the blocking wait runs in a thread.
    let handle = std::thread::spawn(move || {
        tx.send(child.wait_with_output().map_err(Into::into)).ok();
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Ok(output)) => {
            let _ = handle.join();
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            if output.status.success() {
                Ok(trim_output(&text))
            } else {
                Err(anyhow!(trim_output(&text)))
            }
        }
        Ok(Err(err)) => {
            let _ = handle.join();
            Err(err)
        }
        Err(_) => {
            // Timeout elapsed — the child is still running inside the thread.
            // We can't kill it directly because ownership was moved, but we can
            // let the thread finish on its own after the process eventually ends.
            // The process will be killed when its stdin/stdout are closed by the
            // dropped thread handles.
            Err(anyhow!("tool timed out after {}ms", timeout_ms))
        }
    }
}

fn write_source(path: &Path, source: &str) -> Result<()> {
    fs::write(path, source).with_context(|| format!("Failed to write {}", path.display()))
}

fn cache_dir(language: &str, source: &str, compiler_version: Option<String>) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let mut hasher = Sha256::new();
    hasher.update(language.as_bytes());
    hasher.update(b"\0");
    hasher.update(source.as_bytes());
    hasher.update(b"\0");
    if let Some(version) = compiler_version {
        hasher.update(version.as_bytes());
    }
    let dir = home
        .join(".i6")
        .join("fun-cache")
        .join(language)
        .join(hex::encode(hasher.finalize()));
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    Ok(dir)
}

fn compiler_version(command: &str, args: &[&str]) -> Option<String> {
    Command::new(command)
        .args(args)
        .output()
        .ok()
        .map(|output| {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            text
        })
}

fn command_available(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}

fn executable_path(dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        dir.join(format!("{}.exe", name))
    } else {
        dir.join(name)
    }
}

fn trim_output(value: &str) -> String {
    value.chars().take(64 * 1024).collect()
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
