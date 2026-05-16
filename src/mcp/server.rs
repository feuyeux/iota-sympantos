//! Stdio MCP server for iota-context.
//!
//! Implements the [MCP](https://modelcontextprotocol.io/) protocol over
//! stdin/stdout using JSON-RPC.  Exposes iota tools (`iota_memory_*`,
//! `iota_skill_*`, `iota_session_*`, `iota_handoff_*`) and resources
//! (`iota://memory/…`, `iota://skill/…`, `iota://session/…`, `iota://workspace/…`).
//!
//! All tool execution is delegated to [`super::tool_dispatch`] so that this
//! module is purely a protocol adapter.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

use crate::memory::MemoryStore;
use crate::runtime_event::LogEvent;
use crate::skill::SkillRegistry;
use crate::store::ledger::SessionLedger;

use super::tool_dispatch::{self, ToolContext};

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
            let ctx = ToolContext {
                memory,
                ledger,
                skills,
                workspace,
            };
            match tool_dispatch::dispatch_tool(&ctx, name, &args) {
                Ok(value) => {
                    emit_route_log_for_tool(name, true, &value);
                    ok(
                        id,
                        json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
                    )
                }
                Err(message) => {
                    emit_route_log_for_tool(name, false, &json!({"error": message}));
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
            let ctx = ToolContext {
                memory,
                ledger,
                skills,
                workspace,
            };
            match read_resource(uri, &ctx) {
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

fn emit_route_log_for_tool(name: &str, ok: bool, data: &Value) {
    if matches!(name, "iota_memory_search" | "iota_memory_write") {
        let event = if name == "iota_memory_search" {
            "memory.search.result"
        } else {
            "memory.write.result"
        };
        emit_route_log(
            if ok { "info" } else { "warn" },
            event,
            json!({
                "tool_name": name,
                "ok": ok,
                "data": data,
            }),
        );
    }
}

fn emit_route_log(level: &str, event: &str, fields: Value) {
    let mut log = LogEvent::new(level, "iota::mcp::server", event);
    log.route = Some("mcp-sidecar".to_string());
    log.fields = fields;
    if let Ok(line) = serde_json::to_string(&log) {
        eprintln!("[iota log] {}", line);
    }
}

fn read_resource(uri: &str, ctx: &ToolContext) -> Result<Value, String> {
    let parts = uri
        .strip_prefix("iota://")
        .ok_or_else(|| "unsupported resource URI".to_string())?;
    let pieces = parts.split('/').collect::<Vec<_>>();
    match pieces.as_slice() {
        ["memory", scope, scope_id] => {
            let memory = ctx
                .memory
                .ok_or_else(|| "memory store is unavailable".to_string())?;
            memory
                .search(scope_id, 100)
                .map(|records| json!({"scope": scope, "scope_id": scope_id, "records": records}))
                .map_err(|err| err.to_string())
        }
        ["skill", name] => {
            let skill = ctx
                .skills
                .get(name)
                .ok_or_else(|| format!("skill '{}' not found", name))?;
            Ok(json!({"metadata": skill.metadata, "body": skill.body}))
        }
        ["session", id, "summary"] => {
            let ledger = ctx
                .ledger
                .ok_or_else(|| "session ledger is unavailable".to_string())?;
            ledger
                .summary(id)
                .map(|summary| {
                    json!({"summary": summary.map(|s| json!({
                        "iota_session_id": s.iota_session_id,
                        "cwd": s.cwd,
                        "active_backend": s.active_backend,
                        "turn_count": s.turn_count,
                        "last_output_summary": s.last_output_summary,
                    }))})
                })
                .map_err(|err| err.to_string())
        }
        ["workspace", _, "rules"] => Ok(json!({"cwd": ctx.workspace.display().to_string()})),
        _ => Err(format!("unknown resource {}", uri)),
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

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
