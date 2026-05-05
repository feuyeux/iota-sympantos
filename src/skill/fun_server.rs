use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const TOOLS: [(&str, &str); 7] = [
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
        "description": format!("Execute the configured pet-generator {} function with iota guardrails", language),
        "inputSchema": {"type":"object","properties":{"timeout_ms":{"type":"integer"}},"required":[]}
    })).collect()
}

pub fn run_tool(name: &str, args: &Value) -> Result<String> {
    let timeout_ms = args
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(10_000)
        .min(60_000);
    match name {
        "fun.python" => run_python(timeout_ms),
        "fun.typescript" => run_typescript(timeout_ms),
        "fun.rust" => run_rust(timeout_ms),
        "fun.go" => run_go(timeout_ms),
        "fun.java" => run_java(timeout_ms),
        "fun.cpp" => run_cpp(timeout_ms),
        "fun.zig" => run_zig(timeout_ms),
        _ => Err(anyhow!("unknown tool {}", name)),
    }
}

fn run_python(timeout_ms: u64) -> Result<String> {
    let script = fun_root()?.join("python").join("random_number.py");
    ensure_file(&script)?;
    let source = format!(
        "import importlib.util; spec = importlib.util.spec_from_file_location('random_number', r'{}'); module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module); print(module.random_number())",
        script.display()
    );
    run_interpreter(
        "python3",
        &[OsString::from("-c"), OsString::from(source)],
        timeout_ms,
    )
}

fn run_typescript(timeout_ms: u64) -> Result<String> {
    let cwd = fun_root()?.join("typescript");
    let runner = cwd.join("runner.js");
    ensure_file(&runner)?;
    ensure_file(&cwd.join("randomColor.ts"))?;
    run_command("node", &[runner.into_os_string()], Some(&cwd), timeout_ms)
}

fn run_rust(timeout_ms: u64) -> Result<String> {
    let cwd = fun_root()?.join("rust");
    let sources = [cwd.join("runner.rs"), cwd.join("random_material.rs")];
    ensure_files(&sources)?;
    let bin = cached_binary_path("rust", &sources)?;
    if !bin.exists() {
        run_command(
            "rustc",
            &[
                OsString::from("runner.rs"),
                OsString::from("-o"),
                bin.as_os_str().to_os_string(),
            ],
            Some(&cwd),
            timeout_ms,
        )?;
    }
    run_command(bin.as_os_str(), &[], Some(&cwd), timeout_ms)
}

fn run_go(timeout_ms: u64) -> Result<String> {
    let cwd = fun_root()?.join("go");
    let sources = [cwd.join("random_shape.go"), cwd.join("runner.go")];
    ensure_files(&sources)?;
    let bin = cached_binary_path("go", &sources)?;
    if !bin.exists() {
        run_command(
            "go",
            &[
                OsString::from("build"),
                OsString::from("-o"),
                bin.as_os_str().to_os_string(),
                OsString::from("random_shape.go"),
                OsString::from("runner.go"),
            ],
            Some(&cwd),
            timeout_ms,
        )?;
    }
    run_command(bin.as_os_str(), &[], Some(&cwd), timeout_ms)
}

fn run_java(timeout_ms: u64) -> Result<String> {
    let cwd = fun_root()?.join("java");
    let sources = [
        cwd.join("RandomAnimal.java"),
        cwd.join("RandomAnimalRunner.java"),
    ];
    ensure_files(&sources)?;
    let class_dir = cached_class_dir_path("java", &sources)?;
    let class = class_dir.join("RandomAnimalRunner.class");
    if !class.exists() {
        fs::create_dir_all(&class_dir)
            .with_context(|| format!("Failed to create {}", class_dir.display()))?;
        run_command(
            "javac",
            &[
                OsString::from("-encoding"),
                OsString::from("UTF-8"),
                OsString::from("-d"),
                class_dir.as_os_str().to_os_string(),
                OsString::from("RandomAnimal.java"),
                OsString::from("RandomAnimalRunner.java"),
            ],
            Some(&cwd),
            timeout_ms,
        )?;
    }
    run_command(
        "java",
        &[
            OsString::from("-cp"),
            class_dir.as_os_str().to_os_string(),
            OsString::from("RandomAnimalRunner"),
        ],
        Some(&cwd),
        timeout_ms,
    )
}

fn run_cpp(timeout_ms: u64) -> Result<String> {
    let cwd = fun_root()?.join("cpp");
    let sources = [
        cwd.join("random_action.cpp"),
        cwd.join("random_action_runner.cpp"),
    ];
    ensure_files(&sources)?;
    let compiler = if command_available("clang++") {
        "clang++"
    } else {
        "g++"
    };
    let bin = cached_binary_path("cpp", &sources)?;
    if !bin.exists() {
        run_command(
            compiler,
            &[
                OsString::from("random_action_runner.cpp"),
                OsString::from("-std=c++17"),
                OsString::from("-O2"),
                OsString::from("-o"),
                bin.as_os_str().to_os_string(),
            ],
            Some(&cwd),
            timeout_ms,
        )?;
    }
    run_command(bin.as_os_str(), &[], Some(&cwd), timeout_ms)
}

fn run_zig(timeout_ms: u64) -> Result<String> {
    let cwd = fun_root()?.join("zig");
    let sources = [cwd.join("runner.zig"), cwd.join("random_size.zig")];
    ensure_files(&sources)?;
    let bin = cached_binary_path("zig", &sources)?;
    if !bin.exists() {
        run_command(
            "zig",
            &[
                OsString::from("build-exe"),
                OsString::from("runner.zig"),
                OsString::from("-O"),
                OsString::from("ReleaseFast"),
                OsString::from("-lc"),
                OsString::from(format!("-femit-bin={}", bin.display())),
            ],
            Some(&cwd),
            timeout_ms,
        )?;
    }
    run_command(bin.as_os_str(), &[], Some(&cwd), timeout_ms)
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
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to start {}", command_label))?;

    let mut stdout = child.stdout.take().context("tool stdout was not piped")?;
    let mut stderr = child.stderr.take().context("tool stderr was not piped")?;
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout
            .read_to_end(&mut buf)
            .map(|_| buf)
            .map_err(Into::into)
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stderr
            .read_to_end(&mut buf)
            .map(|_| buf)
            .map_err(Into::into)
    });

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("Failed to wait for {}", command_label))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_output(stdout_handle, "stdout");
            let _ = join_output(stderr_handle, "stderr");
            return Err(anyhow!("tool timed out after {}ms", timeout_ms));
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_output(stdout_handle, "stdout")?;
    let stderr = join_output(stderr_handle, "stderr")?;
    let mut text = String::from_utf8_lossy(&stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&stderr));
    if status.success() {
        Ok(trim_output(&text))
    } else {
        Err(anyhow!(trim_output(&text)))
    }
}

fn join_output(
    handle: std::thread::JoinHandle<Result<Vec<u8>>>,
    stream_name: &str,
) -> Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| anyhow!("tool {} reader thread panicked", stream_name))?
}

fn fun_root() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("skills").join("pet-generator").join("iota-fun"));
        candidates.push(
            cwd.join("iota-skill")
                .join("pet-generator")
                .join("iota-fun"),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().take(8) {
            candidates.push(
                ancestor
                    .join("skills")
                    .join("pet-generator")
                    .join("iota-fun"),
            );
            candidates.push(
                ancestor
                    .join("iota-skill")
                    .join("pet-generator")
                    .join("iota-fun"),
            );
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_dir())
        .context("Failed to locate pet-generator iota-fun directory")
}

fn ensure_files(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        ensure_file(path)?;
    }
    Ok(())
}

fn ensure_file(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(anyhow!("Fun source file not found: {}", path.display()))
    }
}

fn cached_binary_path(language: &str, sources: &[PathBuf]) -> Result<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    cached_path(language, sources, suffix)
}

fn cached_class_dir_path(language: &str, sources: &[PathBuf]) -> Result<PathBuf> {
    cached_path(language, sources, "-classes")
}

fn cached_path(language: &str, sources: &[PathBuf], suffix: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let mut hasher = Sha256::new();
    hasher.update(b"v3");
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(std::env::consts::ARCH.as_bytes());
    hasher.update(language.as_bytes());
    for source in sources {
        let metadata = source
            .metadata()
            .with_context(|| format!("Failed to stat {}", source.display()))?;
        hasher.update(source.to_string_lossy().as_bytes());
        hasher.update(metadata.len().to_string().as_bytes());
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(duration.as_millis().to_string().as_bytes());
            }
        }
    }
    let dir = home.join(".i6").join("iota-fun");
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let hash = hex::encode(hasher.finalize());
    Ok(dir.join(format!("iota-fun-{}-{}{}", language, &hash[..16], suffix)))
}

fn command_available(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}

fn trim_output(value: &str) -> String {
    value.trim().chars().take(64 * 1024).collect()
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn run_command_times_out_without_waiting_for_child_completion() {
        let started = Instant::now();
        let err = run_command(
            "sh",
            &[OsString::from("-c"), OsString::from("sleep 5")],
            None,
            100,
        )
        .unwrap_err();

        assert!(err.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(windows)]
    #[test]
    fn run_command_times_out_without_waiting_for_child_completion() {
        let started = Instant::now();
        let err = run_command(
            "cmd",
            &[
                OsString::from("/C"),
                OsString::from("ping -n 6 127.0.0.1 >NUL"),
            ],
            None,
            100,
        )
        .unwrap_err();

        assert!(err.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
