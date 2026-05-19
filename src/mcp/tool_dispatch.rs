//! Shared tool dispatch logic for iota MCP tools.
//!
//! Both the stdio MCP server (`mcp::server`) and the ACP stream interceptor
//! (`mcp::router`) delegate tool execution to this module so that parsing,
//! validation, and business logic live in exactly one place.

use std::path::Path;

use serde_json::{Value, json};

use crate::memory::{
    MemoryFacet, MemoryInsert, MemoryMergeMode, MemoryScope, MemorySearchMode, MemoryStore,
    MemoryType,
};
use crate::skill::SkillRegistry;
use crate::store::ledger::SessionLedger;

// ---------------------------------------------------------------------------
// ToolContext — injected dependencies for tool handlers
// ---------------------------------------------------------------------------

/// All external dependencies a tool handler may need, passed by the caller so
/// this module never opens databases or reads the filesystem on its own.
pub struct ToolContext<'a> {
    pub memory: Option<&'a MemoryStore>,
    pub ledger: Option<&'a SessionLedger>,
    pub skills: &'a SkillRegistry,
    pub workspace: &'a Path,
}

// ---------------------------------------------------------------------------
// dispatch_tool — single entry point
// ---------------------------------------------------------------------------

/// Execute a named iota tool and return the raw business-logic result.
///
/// Returns `Ok(Value)` on success or `Err(String)` with a human-readable
/// message on failure.  The caller is responsible for wrapping the result in
/// whatever protocol envelope it needs (MCP content array, JSON-RPC, etc.).
pub fn dispatch_tool(ctx: &ToolContext, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "iota_memory_search" => dispatch_memory_search(ctx, args),
        "iota_memory_write" => dispatch_memory_write(ctx, args),
        "iota_skill_search" => dispatch_skill_search(ctx, args),
        "iota_skill_load" => dispatch_skill_load(ctx, args),
        "iota_session_summary" => dispatch_session_summary(ctx, args),
        "iota_handoff_publish" => dispatch_handoff_publish(ctx, args),
        "iota_handoff_read" => dispatch_handoff_read(ctx, args),
        _ => Err(format!("unknown tool {}", name)),
    }
}

/// Return whether `name` is a tool this module can dispatch.
pub fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        "iota_memory_search"
            | "iota_memory_write"
            | "iota_skill_search"
            | "iota_skill_load"
            | "iota_session_summary"
            | "iota_handoff_publish"
            | "iota_handoff_read"
    )
}

// ---------------------------------------------------------------------------
// Tool handlers
// ---------------------------------------------------------------------------

fn dispatch_memory_search(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .map(parse_memory_search_mode)
        .transpose()?
        .unwrap_or(MemorySearchMode::Hybrid);
    let memory = ctx
        .memory
        .ok_or_else(|| "memory store is unavailable".to_string())?;
    let records = memory
        .search_with_mode(query, limit, mode)
        .map_err(|err| err.to_string())?;
    Ok(json!({"records": records, "mode": format!("{:?}", mode).to_lowercase()}))
}

fn dispatch_memory_write(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let memory = ctx
        .memory
        .ok_or_else(|| "memory store is unavailable".to_string())?;
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
        .unwrap_or_else(|| default_memory_scope_id(&scope, args, ctx.workspace));
    let confidence = required_confidence(args)?;
    let ttl_days = args.get("ttl_days").and_then(Value::as_i64).unwrap_or(7);
    let merge_mode = args
        .get("merge_mode")
        .and_then(Value::as_str)
        .map(parse_memory_merge_mode)
        .transpose()?
        .unwrap_or(MemoryMergeMode::Auto);
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
    Ok(json!({"id": id, "merge_mode": format!("{:?}", merge_mode).to_lowercase()}))
}

fn dispatch_skill_search(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let backend = args
        .get("backend")
        .and_then(Value::as_str)
        .unwrap_or("codex");
    let backend = crate::acp::AcpBackend::parse(backend).map_err(|err| err.to_string())?;
    Ok(
        json!({"index": ctx.skills.skill_index(backend, 4000), "diagnostics": ctx.skills.diagnostics()}),
    )
}

fn dispatch_skill_load(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "name is required".to_string())?;
    let skill = ctx
        .skills
        .get(name)
        .ok_or_else(|| format!("skill '{}' not found", name))?;
    Ok(json!({"metadata": skill.metadata, "body": skill.body}))
}

fn dispatch_session_summary(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let ledger = ctx
        .ledger
        .ok_or_else(|| "session ledger is unavailable".to_string())?;
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    ledger
        .summary(session_id)
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

fn dispatch_handoff_publish(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let ledger = ctx
        .ledger
        .ok_or_else(|| "session ledger is unavailable".to_string())?;
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
        .publish_handoff(session_id, from_backend, to_backend, ctx.workspace, summary)
        .map(|_| json!({"ok": true}))
        .map_err(|err| err.to_string())
}

fn dispatch_handoff_read(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let ledger = ctx
        .ledger
        .ok_or_else(|| "session ledger is unavailable".to_string())?;
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let to_backend = args.get("to_backend").and_then(Value::as_str);
    ledger
        .read_handoff(session_id, to_backend, ctx.workspace)
        .map(|handoff| json!({"handoff": handoff}))
        .map_err(|err| err.to_string())
}

// ---------------------------------------------------------------------------
// Parsers & validators (single canonical copy)
// ---------------------------------------------------------------------------

pub fn parse_memory_type(value: &str) -> Result<MemoryType, String> {
    match value {
        "semantic" => Ok(MemoryType::Semantic),
        "episodic" => Ok(MemoryType::Episodic),
        "procedural" => Ok(MemoryType::Procedural),
        other => Err(format!("invalid memory type {}", other)),
    }
}

pub fn parse_memory_facet(value: &str) -> Result<MemoryFacet, String> {
    match value {
        "identity" => Ok(MemoryFacet::Identity),
        "preference" => Ok(MemoryFacet::Preference),
        "strategic" => Ok(MemoryFacet::Strategic),
        "domain" => Ok(MemoryFacet::Domain),
        other => Err(format!("invalid memory facet {}", other)),
    }
}

pub fn parse_memory_scope(value: &str) -> Result<MemoryScope, String> {
    match value {
        "session" => Ok(MemoryScope::Session),
        "project" => Ok(MemoryScope::Project),
        "user" => Ok(MemoryScope::User),
        "global" => Ok(MemoryScope::Global),
        other => Err(format!("invalid memory scope {}", other)),
    }
}

pub fn parse_memory_merge_mode(value: &str) -> Result<MemoryMergeMode, String> {
    match value {
        "auto" => Ok(MemoryMergeMode::Auto),
        "add" => Ok(MemoryMergeMode::Add),
        "update" => Ok(MemoryMergeMode::Update),
        "none" => Ok(MemoryMergeMode::None),
        other => Err(format!("invalid memory merge_mode {}", other)),
    }
}

pub fn parse_memory_search_mode(value: &str) -> Result<MemorySearchMode, String> {
    match value {
        "keyword" => Ok(MemorySearchMode::Keyword),
        "vector" => Ok(MemorySearchMode::Vector),
        "hybrid" => Ok(MemorySearchMode::Hybrid),
        other => Err(format!("invalid memory search mode {}", other)),
    }
}

pub fn validate_memory_shape(
    memory_type: MemoryType,
    facet: Option<MemoryFacet>,
) -> Result<(), String> {
    if memory_type == MemoryType::Semantic && facet.is_none() {
        return Err("semantic memory requires a facet".to_string());
    }
    if memory_type != MemoryType::Semantic && facet.is_some() {
        return Err("only semantic memory may set facet".to_string());
    }
    Ok(())
}

pub fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{} is required", key))
}

pub fn required_confidence(args: &Value) -> Result<f64, String> {
    let confidence = args
        .get("confidence")
        .and_then(value_as_f64)
        .ok_or_else(|| "confidence is required".to_string())?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err("confidence must be between 0 and 1".to_string());
    }
    Ok(confidence)
}

pub fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
    })
}

pub fn default_memory_scope_id(scope: &MemoryScope, args: &Value, workspace: &Path) -> String {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "tool_dispatch_tests.rs"]
mod tool_dispatch_tests;
