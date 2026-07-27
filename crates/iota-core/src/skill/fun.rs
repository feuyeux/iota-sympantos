use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
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
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;

pub fn run_stdio() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut stdin = stdin.lock();
    while let Some(line) = crate::mcp::read_limited_line(&mut stdin)? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line).with_context(|| {
            let preview = line.chars().take(256).collect::<String>();
            format!("Invalid JSON-RPC: {preview}")
        })?;
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
    let cwd = script.parent().context("Python fun source has no parent")?;
    run_command("python3", &[script.as_os_str().to_os_string()], Some(cwd), timeout_ms)
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
    let effective_timeout = timeout_ms.max(30_000);
    if !bin.exists() {
        let compile_args = vec![
            OsString::from("runner.rs"),
            OsString::from("-o"),
            bin.as_os_str().to_os_string(),
        ];
        #[cfg(windows)]
        {
            let mut compile_args = compile_args;
            // rust-lld avoids depending on external MSVC linker installations.
            compile_args.push(OsString::from("-C"));
            compile_args.push(OsString::from("linker=rust-lld"));
            let compiled = run_command("rustc", &compile_args, Some(&cwd), effective_timeout);
            if compiled.is_err() {
                return Ok(fallback_material());
            }
        }
        #[cfg(not(windows))]
        {
            let compiled = run_command("rustc", &compile_args, Some(&cwd), effective_timeout);
            if compiled.is_err() {
                return Ok(fallback_material());
            }
        }
    }
    match run_command(bin.as_os_str(), &[], Some(&cwd), effective_timeout) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Ok(fallback_material()),
    }
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
        cwd.join("random_action.h"),
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
    let effective_timeout = timeout_ms.max(30_000);
    if !bin.exists() {
        let compiled = run_command(
            compiler,
            &[
                OsString::from("random_action.cpp"),
                OsString::from("random_action_runner.cpp"),
                OsString::from("-std=c++17"),
                OsString::from("-O2"),
                OsString::from("-o"),
                bin.as_os_str().to_os_string(),
            ],
            Some(&cwd),
            effective_timeout,
        );
        if compiled.is_err() {
            return Ok(fallback_action());
        }
    }
    match run_command(bin.as_os_str(), &[], Some(&cwd), effective_timeout) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Ok(fallback_action()),
    }
}

fn run_zig(timeout_ms: u64) -> Result<String> {
    let cwd = fun_root()?.join("zig");
    let sources = [cwd.join("runner.zig"), cwd.join("random_size.zig")];
    ensure_files(&sources)?;
    let bin = cached_binary_path("zig", &sources)?;
    let effective_timeout = timeout_ms.max(30_000);
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
            effective_timeout,
        )?;
    }
    run_command(bin.as_os_str(), &[], Some(&cwd), effective_timeout)
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
        {
            cmd.env("USERPROFILE", &home);
            cmd.env("HOME", &home);

            if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
                cmd.env("LOCALAPPDATA", local_app_data);
            } else {
                cmd.env("LOCALAPPDATA", home.join("AppData").join("Local"));
            }

            if let Some(app_data) = std::env::var_os("APPDATA") {
                cmd.env("APPDATA", app_data);
            } else {
                cmd.env("APPDATA", home.join("AppData").join("Roaming"));
            }
        }
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
    let output_limit_exceeded = Arc::new(AtomicBool::new(false));
    let stdout_limit = Arc::clone(&output_limit_exceeded);
    let stdout_handle = std::thread::spawn(move || read_limited_output(&mut stdout, stdout_limit));
    let stderr_limit = Arc::clone(&output_limit_exceeded);
    let stderr_handle = std::thread::spawn(move || read_limited_output(&mut stderr, stderr_limit));

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("Failed to wait for {}", command_label))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            kill_child_tree(&mut child);
            let _ = child.wait();
            drop(stdout_handle);
            drop(stderr_handle);
            return Err(anyhow!("tool timed out after {}ms", timeout_ms));
        }
        if output_limit_exceeded.load(Ordering::Relaxed) {
            kill_child_tree(&mut child);
            let _ = child.wait();
            let _ = join_output(stdout_handle, "stdout");
            let _ = join_output(stderr_handle, "stderr");
            return Err(anyhow!(
                "tool output exceeded {} bytes per stream",
                MAX_COMMAND_OUTPUT_BYTES
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_output(stdout_handle, "stdout")?;
    let stderr = join_output(stderr_handle, "stderr")?;
    if output_limit_exceeded.load(Ordering::Relaxed) {
        return Err(anyhow!(
            "tool output exceeded {} bytes per stream",
            MAX_COMMAND_OUTPUT_BYTES
        ));
    }
    let mut text = String::from_utf8_lossy(&stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&stderr));
    if status.success() {
        Ok(trim_output(&text))
    } else {
        Err(anyhow!(trim_output(&text)))
    }
}

fn read_limited_output(reader: &mut impl Read, limit_exceeded: Arc<AtomicBool>) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(output);
        }
        let remaining = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(output.len());
        let retained = remaining.min(read);
        output.extend_from_slice(&chunk[..retained]);
        if retained < read {
            limit_exceeded.store(true, Ordering::Relaxed);
        }
    }
}

fn kill_child_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
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
    let language = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    let file = path.file_name().and_then(|value| value.to_str());
    let expected = match (language, file) {
        (Some("python"), Some("random_number.py")) => "1bd1030a00f5682712982851a91f72b7c639cc79ab53d3c705dfde140b033881",
        (Some("typescript"), Some("randomColor.ts")) => "f6ff1218de3bc67f89e1c7c6ff79b66ddc26060f4364ba8c6ab4c1708cad7f75",
        (Some("typescript"), Some("runner.js")) => "822d063c706c3bcd8fdc141f1111416b64a57f53beeb03a4cc104681300d4f85",
        (Some("rust"), Some("random_material.rs")) => "13e3e2696a1f1ea844aa9ce9c6ba40c18a4301fc28a8f70c1d391c598d0b12fb",
        (Some("rust"), Some("runner.rs")) => "2191e06624cc8081f642c330be89c1e8599c396e114c552174fe4ec54112b2ed",
        (Some("go"), Some("random_shape.go")) => "eb3f59324aa189b5453257a2187efde30acdc93e033c67750571a77572c8e857",
        (Some("go"), Some("runner.go")) => "b90e4ce2be84bfd33d408fc86bf9a7a1a9456033358132478188c73ec957fd39",
        (Some("java"), Some("RandomAnimal.java")) => "ecedecaae43a659cb427f51e818f3f071ce5d90693cd50d1d8fe628f18e6dfb4",
        (Some("java"), Some("RandomAnimalRunner.java")) => "0e39a3f42a047caec15a8081f25acc741c5f47550d83c3780af01a900f20765e",
        (Some("cpp"), Some("random_action.h")) => "4c881ea8f935374b51d684dffcac2faaff77ed43760e3d8eee2f909681e02edd",
        (Some("cpp"), Some("random_action.cpp")) => "e7ab0c1e95fbe46195f31725caff10cdb7e85aaa5a28affd337061219ab80be2",
        (Some("cpp"), Some("random_action_runner.cpp")) => "43eeef77330e6d375a0456ef6f94a9a20c220122e11a9c5809115a859402b654",
        (Some("zig"), Some("random_size.zig")) => "ac7af0b5523b21f47311855cd621eb4921e495768f9af792ca48ce3a6143d713",
        (Some("zig"), Some("runner.zig")) => "dd7ec946191b9e010116811b2012fbe30bdb2217542c435d6a15ea0556f9a988",
        _ => return Err(anyhow!("Untrusted iota-fun source path: {}", path.display())),
    };
    let bytes = fs::read(path)
        .with_context(|| format!("Failed to read fun source {}", path.display()))?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected {
        return Err(anyhow!(
            "Refusing modified iota-fun source {} (sha256 mismatch)",
            path.display()
        ));
    }
    Ok(())
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
        let bytes = fs::read(source)
            .with_context(|| format!("Failed to read {}", source.display()))?;
        hasher.update(source.to_string_lossy().as_bytes());
        hasher.update(Sha256::digest(&bytes));
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

fn fallback_action() -> String {
    let actions = ["睡觉", "奔跑", "喝水", "吃饭", "捕捉", "发呆"];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    actions[nanos % actions.len()].to_string()
}

fn fallback_material() -> String {
    let materials = ["wood", "metal", "glass", "plastic", "stone"];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    materials[nanos % materials.len()].to_string()
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
#[path = "fun_tests.rs"]
mod fun_tests;
