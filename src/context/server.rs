use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

use crate::runtime_event::LogEvent;
use crate::skill::SkillRegistry;
use crate::store::ledger::SessionLedger;
use crate::store::memory::{
    MemoryFacet, MemoryInsert, MemoryMergeMode, MemoryScope, MemorySearchMode, MemoryStore,
    MemoryType,
};

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
                Err(message) => {
                    if matches!(name, "iota_memory_search" | "iota_memory_write") {
                        emit_route_log(
                            "warn",
                            if name == "iota_memory_search" {
                                "memory.search.result"
                            } else {
                                "memory.write.result"
                            },
                            json!({
                                "tool_name": name,
                                "ok": false,
                                "error": message.clone(),
                            }),
                        );
                    }
                    ok(
                        id,
                        json!({"content":[{"type":"text","text":message}],"isError":true}),
                    )
                }
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
            let mode = args
                .get("mode")
                .and_then(Value::as_str)
                .map(parse_memory_search_mode)
                .transpose()?
                .unwrap_or(MemorySearchMode::Hybrid);
            emit_route_log(
                "info",
                "memory.search.call",
                json!({
                    "tool_name": "iota_memory_search",
                    "query": query,
                    "limit": limit,
                    "mode": format!("{:?}", mode).to_lowercase(),
                }),
            );
            tracing::info!(
                tool_name = "iota_memory_search",
                query = %query,
                limit,
                mode = ?mode,
                "context MCP memory search tool call received"
            );
            let memory = memory.ok_or_else(|| "memory store is unavailable".to_string())?;
            memory
                .search_with_mode(query, limit, mode)
                .map(|records| {
                    emit_route_log(
                        "info",
                        "memory.search.result",
                        json!({
                            "tool_name": "iota_memory_search",
                            "query": query,
                            "limit": limit,
                            "mode": format!("{:?}", mode).to_lowercase(),
                            "record_count": records.len(),
                            "record_ids": records.iter().map(|record| record.id.as_str()).collect::<Vec<_>>(),
                            "ok": true,
                        }),
                    );
                    tracing::info!(
                        tool_name = "iota_memory_search",
                        query = %query,
                        limit,
                        mode = ?mode,
                        record_count = records.len(),
                        record_ids = ?records.iter().map(|record| record.id.as_str()).collect::<Vec<_>>(),
                        "context MCP memory search tool call completed"
                    );
                    json!({"records":records, "mode": format!("{:?}", mode).to_lowercase()})
                })
                .map_err(|err| err.to_string())
        }
        "iota_memory_write" => {
            let memory = memory.ok_or_else(|| "memory store is unavailable".to_string())?;
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| "content is required".to_string())?;
            let memory_type = parse_memory_type(required_string(args, "type")?)?;
            let facet = args
                .get("facet")
                .and_then(Value::as_str)
                .map(parse_memory_facet)
                .transpose()?;
            validate_memory_shape(memory_type.clone(), facet.clone())?;
            let scope = parse_memory_scope(required_string(args, "scope")?)?;
            let scope_id = args
                .get("scope_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| default_memory_scope_id(&scope, args, workspace));
            let confidence = required_confidence(args)?;
            let ttl_days = args.get("ttl_days").and_then(Value::as_i64).unwrap_or(7);
            let merge_mode = args
                .get("merge_mode")
                .and_then(Value::as_str)
                .map(parse_memory_merge_mode)
                .transpose()?
                .unwrap_or(MemoryMergeMode::Auto);
            emit_route_log(
                "info",
                "memory.write.call",
                json!({
                    "tool_name": "iota_memory_write",
                    "type": memory_type.as_str(),
                    "facet": facet.as_ref().map(MemoryFacet::as_str),
                    "scope": scope.as_str(),
                    "scope_id": scope_id.clone(),
                    "confidence": confidence,
                    "content_chars": content.chars().count(),
                    "merge_mode": format!("{:?}", merge_mode).to_lowercase(),
                    "source_backend": args.get("source_backend").and_then(Value::as_str),
                    "source_session_id": args.get("source_session_id").and_then(Value::as_str),
                    "source_execution_id": args.get("source_execution_id").and_then(Value::as_str),
                }),
            );
            tracing::info!(
                tool_name = "iota_memory_write",
                memory_type = %memory_type.as_str(),
                facet = facet.as_ref().map(MemoryFacet::as_str).unwrap_or("-"),
                scope = %scope.as_str(),
                scope_id = %scope_id,
                merge_mode = ?merge_mode,
                content_chars = content.chars().count(),
                source_backend = args.get("source_backend").and_then(|value| value.as_str()).unwrap_or("-"),
                source_session_id = args.get("source_session_id").and_then(|value| value.as_str()).unwrap_or("-"),
                source_execution_id = args.get("source_execution_id").and_then(|value| value.as_str()).unwrap_or("-"),
                "context MCP memory write tool call received"
            );
            let id = memory
                .insert_with_merge(
                    MemoryInsert {
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
                    },
                    merge_mode,
                )
                .map_err(|err| err.to_string())?;
            tracing::info!(
                tool_name = "iota_memory_write",
                memory_id = id.as_deref().unwrap_or("-"),
                merge_mode = ?merge_mode,
                skipped = id.is_none(),
                "context MCP memory write tool call completed"
            );
            emit_route_log(
                "info",
                "memory.write.result",
                json!({
                    "tool_name": "iota_memory_write",
                    "memory_id": id.clone(),
                    "merge_mode": format!("{:?}", merge_mode).to_lowercase(),
                    "skipped": id.is_none(),
                    "ok": true,
                }),
            );
            Ok(json!({"id": id, "merge_mode": format!("{:?}", merge_mode).to_lowercase()}))
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

fn emit_route_log(level: &str, event: &str, fields: Value) {
    let mut log = LogEvent::new(level, "iota::context::server", event);
    log.route = Some("mcp-sidecar".to_string());
    log.fields = fields;
    if let Ok(line) = serde_json::to_string(&log) {
        eprintln!("[iota log] {}", line);
    }
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "iota_memory_search",
            "description": "Search unified iota memory by keyword. Returns matching records across all types and scopes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search keyword"},
                    "limit": {"type": "integer", "description": "Max results (default 20)"},
                    "mode": {"type": "string", "enum": ["hybrid", "vector", "keyword"], "description": "Search strategy (default hybrid)"}
                }
            }
        }),
        json!({
            "name": "iota_memory_write",
            "description": "Persist a memory item to iota's unified memory store. Call proactively when you learn something worth remembering: user identity, preferences, project goals, domain facts, or step-by-step procedures. Persisted memories are injected into future sessions across all backends.\n\ntype+facet combinations:\n- semantic/identity  → who the user is (name, role)\n- semantic/preference → how the user likes things done\n- semantic/strategic → project goals, decisions\n- semantic/domain    → technical facts about the project\n- procedural        → step-by-step how-to (no facet)\n- episodic          → what happened in this session (no facet)\n\nscope_id is optional. Defaults match Engine recall: user → \"local-user\", project → current cwd path, session → source_session_id/session_id if provided.",
            "inputSchema": {
                "type": "object",
                "required": ["content", "type", "scope", "confidence"],
                "properties": {
                    "content":    {"type": "string"},
                    "type":       {"type": "string", "enum": ["semantic", "episodic", "procedural"]},
                    "facet":      {"type": "string", "enum": ["identity", "preference", "strategic", "domain"]},
                    "scope":      {"type": "string", "enum": ["user", "project", "session", "global"]},
                    "scope_id":   {"type": "string"},
                    "merge_mode": {"type": "string", "enum": ["auto", "add", "update", "none"]},
                    "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                    "ttl_days":   {"type": "integer"},
                    "metadata":   {"type": "object"},
                    "source_backend": {"type": "string"},
                    "source_session_id": {"type": "string"},
                    "source_execution_id": {"type": "string"},
                    "supersedes": {"type": "string"}
                },
                "allOf": [
                    {
                        "if": {"properties": {"type": {"const": "semantic"}}, "required": ["type"]},
                        "then": {"required": ["facet"]}
                    },
                    {
                        "if": {"properties": {"type": {"enum": ["episodic", "procedural"]}}, "required": ["type"]},
                        "then": {"not": {"required": ["facet"]}}
                    }
                ]
            }
        }),
        json!({
            "name": "iota_skill_search",
            "description": "Search available iota skill index for the current backend.",
            "inputSchema": {"type": "object", "properties": {"backend": {"type": "string"}}}
        }),
        json!({
            "name": "iota_skill_load",
            "description": "Load the full body of a named iota skill.",
            "inputSchema": {"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}}
        }),
        json!({
            "name": "iota_session_summary",
            "description": "Read summary of the current iota session.",
            "inputSchema": {"type": "object", "properties": {"session_id": {"type": "string"}}}
        }),
        json!({
            "name": "iota_handoff_publish",
            "description": "Publish a handoff summary when switching backends.",
            "inputSchema": {"type": "object", "additionalProperties": true}
        }),
        json!({
            "name": "iota_handoff_read",
            "description": "Read the latest handoff for this session.",
            "inputSchema": {"type": "object", "additionalProperties": true}
        }),
    ]
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

fn parse_memory_merge_mode(value: &str) -> std::result::Result<MemoryMergeMode, String> {
    match value {
        "auto" => Ok(MemoryMergeMode::Auto),
        "add" => Ok(MemoryMergeMode::Add),
        "update" => Ok(MemoryMergeMode::Update),
        "none" => Ok(MemoryMergeMode::None),
        other => Err(format!("invalid memory merge_mode {}", other)),
    }
}

fn parse_memory_search_mode(value: &str) -> std::result::Result<MemorySearchMode, String> {
    match value {
        "keyword" => Ok(MemorySearchMode::Keyword),
        "vector" => Ok(MemorySearchMode::Vector),
        "hybrid" => Ok(MemorySearchMode::Hybrid),
        other => Err(format!("invalid memory search mode {}", other)),
    }
}

fn required_string<'a>(args: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{} is required", key))
}

fn required_confidence(args: &Value) -> std::result::Result<f64, String> {
    let confidence = args
        .get("confidence")
        .and_then(value_as_f64)
        .ok_or_else(|| "confidence is required".to_string())?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err("confidence must be between 0 and 1".to_string());
    }
    Ok(confidence)
}

fn validate_memory_shape(
    memory_type: MemoryType,
    facet: Option<MemoryFacet>,
) -> std::result::Result<(), String> {
    if memory_type == MemoryType::Semantic && facet.is_none() {
        return Err("semantic memory requires a facet".to_string());
    }
    if memory_type != MemoryType::Semantic && facet.is_some() {
        return Err("only semantic memory may set facet".to_string());
    }
    Ok(())
}

fn default_memory_scope_id(
    scope: &MemoryScope,
    args: &Value,
    workspace: &std::path::Path,
) -> String {
    match scope {
        MemoryScope::User => "local-user".to_string(),
        MemoryScope::Project => workspace.display().to_string(),
        MemoryScope::Session => args
            .get("source_session_id")
            .or_else(|| args.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or("local")
            .to_string(),
        MemoryScope::Global => "global".to_string(),
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
    })
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

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
