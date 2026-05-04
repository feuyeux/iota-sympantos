use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

use crate::memory::{MemoryFacet, MemoryInsert, MemoryScope, MemoryStore, MemoryType};
use crate::session_ledger::SessionLedger;
use crate::skills::SkillRegistry;

pub fn run_stdio() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let memory = MemoryStore::default_path()
        .ok()
        .and_then(|path| MemoryStore::open(&path).ok());
    let workspace = std::env::current_dir().context("Failed to get current directory")?;
    let skills = SkillRegistry::load(&workspace, &[]);
    let ledger = SessionLedger::default_path()
        .ok()
        .and_then(|path| SessionLedger::open(&path).ok());

    for line in stdin.lock().lines() {
        // A broken pipe on stdin means the parent closed the connection — exit cleanly.
        let line = match line {
            Ok(l) => l,
            Err(err) if err.kind() == io::ErrorKind::BrokenPipe => break,
            Err(err) => return Err(err.into()),
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Value =
            serde_json::from_str(&line).with_context(|| format!("Invalid JSON-RPC: {}", line))?;
        if request.get("id").is_none() {
            continue;
        }
        let response = handle_request(
            &request,
            memory.as_ref(),
            ledger.as_ref(),
            &skills,
            &workspace,
        );
        // A broken pipe on stdout means the parent stopped reading — exit cleanly.
        match writeln!(stdout, "{}", serde_json::to_string(&response)?) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::BrokenPipe => break,
            Err(err) => return Err(err.into()),
        }
        if let Err(err) = stdout.flush() {
            if err.kind() == io::ErrorKind::BrokenPipe {
                break;
            }
            return Err(err.into());
        }
    }
    Ok(())
}

fn handle_request(
    request: &Value,
    memory: Option<&MemoryStore>,
    ledger: Option<&SessionLedger>,
    skills: &SkillRegistry,
    workspace: &std::path::Path,
) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => ok(
            id,
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"iota-context","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "tools/list" => ok(id, json!({"tools": tools()})),
        "tools/call" => {
            let params = request.get("params").unwrap_or(&Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            match call_tool(name, &args, memory, ledger, skills, workspace) {
                Ok(value) => ok(
                    id,
                    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
                ),
                Err(message) => ok(
                    id,
                    json!({"content":[{"type":"text","text":message}],"isError":true}),
                ),
            }
        }
        "resources/list" => ok(
            id,
            json!({"resources":[
                {"uri":"iota://memory/project/local","name":"project memory"},
                {"uri":"iota://skill/index","name":"skill index"},
                {"uri":"iota://session/local/summary","name":"session summary"},
                {"uri":"iota://workspace/local/rules","name":"workspace rules"}
            ]}),
        ),
        "resources/read" => {
            let params = request.get("params").unwrap_or(&Value::Null);
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            match read_resource(uri, memory, ledger, skills, workspace) {
                Ok(value) => ok(
                    id,
                    json!({"contents":[{"uri":uri,"mimeType":"application/json","text":value.to_string()}]}),
                ),
                Err(message) => ok(
                    id,
                    json!({"contents":[{"uri":uri,"mimeType":"text/plain","text":message}],"isError":true}),
                ),
            }
        }
        other => error(id, -32601, &format!("unknown method {}", other)),
    }
}

fn call_tool(
    name: &str,
    args: &Value,
    memory: Option<&MemoryStore>,
    ledger: Option<&SessionLedger>,
    skills: &SkillRegistry,
    workspace: &std::path::Path,
) -> std::result::Result<Value, String> {
    match name {
        "iota_memory_search" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            let memory = memory.ok_or_else(|| "memory store is unavailable".to_string())?;
            memory
                .search(query, limit)
                .map(|records| json!({"records":records}))
                .map_err(|err| err.to_string())
        }
        "iota_memory_write" => {
            let memory = memory.ok_or_else(|| "memory store is unavailable".to_string())?;
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| "content is required".to_string())?;
            let memory_type = parse_memory_type(
                args.get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("episodic"),
            )?;
            let facet = args
                .get("facet")
                .and_then(Value::as_str)
                .map(parse_memory_facet)
                .transpose()?;
            let scope = parse_memory_scope(
                args.get("scope")
                    .and_then(Value::as_str)
                    .unwrap_or("session"),
            )?;
            let scope_id = args
                .get("scope_id")
                .and_then(Value::as_str)
                .unwrap_or("local")
                .to_string();
            let confidence = args
                .get("confidence")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let ttl_days = args.get("ttl_days").and_then(Value::as_i64).unwrap_or(7);
            let id = memory
                .insert(MemoryInsert {
                    memory_type,
                    facet,
                    scope,
                    scope_id,
                    content: content.to_string(),
                    confidence,
                    source_backend: args
                        .get("source_backend")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    source_session_id: args
                        .get("source_session_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    source_execution_id: args
                        .get("source_execution_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    metadata_json: args.get("metadata").map(Value::to_string),
                    ttl_days,
                    supersedes: args
                        .get("supersedes")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
                .map_err(|err| err.to_string())?;
            Ok(json!({"id": id}))
        }
        "iota_skill_search" => {
            let backend = args
                .get("backend")
                .and_then(Value::as_str)
                .unwrap_or("codex");
            let backend = crate::acp::AcpBackend::parse(backend).map_err(|err| err.to_string())?;
            Ok(
                json!({"index": skills.skill_index(backend, 4000), "diagnostics": skills.diagnostics()}),
            )
        }
        "iota_skill_load" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "name is required".to_string())?;
            let skill = skills
                .get(name)
                .ok_or_else(|| format!("skill '{}' not found", name))?;
            Ok(json!({"metadata": skill.metadata, "body": skill.body}))
        }
        "iota_session_summary" => {
            let ledger = ledger.ok_or_else(|| "session ledger is unavailable".to_string())?;
            let session_id = args
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("local");
            ledger
                .summary(session_id)
                .map(|summary| {
                    json!({"summary": summary.map(|summary| json!({
                        "iota_session_id": summary.iota_session_id,
                        "cwd": summary.cwd,
                        "active_backend": summary.active_backend,
                        "turn_count": summary.turn_count,
                        "last_output_summary": summary.last_output_summary,
                    }))})
                })
                .map_err(|err| err.to_string())
        }
        "iota_handoff_publish" => {
            let ledger = ledger.ok_or_else(|| "session ledger is unavailable".to_string())?;
            let session_id = args
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("local");
            let summary = args
                .get("summary")
                .and_then(Value::as_str)
                .ok_or_else(|| "summary is required".to_string())?;
            let from_backend = args.get("from_backend").and_then(Value::as_str);
            let to_backend = args.get("to_backend").and_then(Value::as_str);
            ledger
                .publish_handoff(session_id, from_backend, to_backend, workspace, summary)
                .map(|_| json!({"ok": true}))
                .map_err(|err| err.to_string())
        }
        "iota_handoff_read" => {
            let ledger = ledger.ok_or_else(|| "session ledger is unavailable".to_string())?;
            let session_id = args
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("local");
            let to_backend = args.get("to_backend").and_then(Value::as_str);
            ledger
                .read_handoff(session_id, to_backend, workspace)
                .map(|handoff| json!({"handoff": handoff}))
                .map_err(|err| err.to_string())
        }
        _ => Err(format!("unknown tool {}", name)),
    }
}

fn tools() -> Vec<Value> {
    [
        ("iota_memory_search", "Search unified iota memory"),
        ("iota_memory_write", "Write unified iota memory"),
        ("iota_skill_search", "Search skill index"),
        ("iota_skill_load", "Load a full skill body"),
        ("iota_session_summary", "Read session summary"),
        ("iota_handoff_publish", "Publish handoff"),
        ("iota_handoff_read", "Read handoff"),
    ]
    .into_iter()
    .map(|(name, description)| json!({"name":name,"description":description,"inputSchema":{"type":"object","additionalProperties":true}}))
    .collect()
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn parse_memory_type(value: &str) -> std::result::Result<MemoryType, String> {
    match value {
        "semantic" => Ok(MemoryType::Semantic),
        "episodic" => Ok(MemoryType::Episodic),
        "procedural" => Ok(MemoryType::Procedural),
        other => Err(format!("invalid memory type {}", other)),
    }
}

fn parse_memory_facet(value: &str) -> std::result::Result<MemoryFacet, String> {
    match value {
        "identity" => Ok(MemoryFacet::Identity),
        "preference" => Ok(MemoryFacet::Preference),
        "strategic" => Ok(MemoryFacet::Strategic),
        "domain" => Ok(MemoryFacet::Domain),
        other => Err(format!("invalid memory facet {}", other)),
    }
}

fn parse_memory_scope(value: &str) -> std::result::Result<MemoryScope, String> {
    match value {
        "session" => Ok(MemoryScope::Session),
        "project" => Ok(MemoryScope::Project),
        "user" => Ok(MemoryScope::User),
        "global" => Ok(MemoryScope::Global),
        other => Err(format!("invalid memory scope {}", other)),
    }
}

fn read_resource(
    uri: &str,
    memory: Option<&MemoryStore>,
    ledger: Option<&SessionLedger>,
    skills: &SkillRegistry,
    workspace: &std::path::Path,
) -> std::result::Result<Value, String> {
    let parts = uri
        .strip_prefix("iota://")
        .ok_or_else(|| "unsupported resource URI".to_string())?;
    let pieces = parts.split('/').collect::<Vec<_>>();
    match pieces.as_slice() {
        ["memory", scope, scope_id] => {
            let memory = memory.ok_or_else(|| "memory store is unavailable".to_string())?;
            memory
                .search(scope_id, 100)
                .map(|records| json!({"scope": scope, "scope_id": scope_id, "records": records}))
                .map_err(|err| err.to_string())
        }
        ["skill", name] => {
            let skill = skills
                .get(name)
                .ok_or_else(|| format!("skill '{}' not found", name))?;
            Ok(json!({"metadata": skill.metadata, "body": skill.body}))
        }
        ["session", id, "summary"] => {
            let ledger = ledger.ok_or_else(|| "session ledger is unavailable".to_string())?;
            ledger
                .summary(id)
                .map(|summary| {
                    json!({"summary": summary.map(|summary| json!({
                        "iota_session_id": summary.iota_session_id,
                        "cwd": summary.cwd,
                        "active_backend": summary.active_backend,
                        "turn_count": summary.turn_count,
                        "last_output_summary": summary.last_output_summary,
                    }))})
                })
                .map_err(|err| err.to_string())
        }
        ["workspace", _, "rules"] => Ok(json!({"cwd": workspace.display().to_string()})),
        _ => Err(format!("unknown resource {}", uri)),
    }
}
