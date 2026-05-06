use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::skill::SkillRegistry;
use crate::store::ledger::SessionLedger;
use crate::store::memory::{
    MemoryFacet, MemoryInsert, MemoryMergeMode, MemoryScope, MemorySearchMode, MemoryStore,
    MemoryType,
};

pub fn try_intercept_tool_call(method: &str, params: Option<&Value>) -> Option<Result<Value>> {
    if !matches!(method, "tools/call" | "mcp/tools/call" | "mcp/tool_call") {
        return None;
    }
    let params = params.cloned().unwrap_or(Value::Null);
    let name = params
        .get("name")
        .or_else(|| params.get("toolName"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments = params
        .get("arguments")
        .or_else(|| params.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    Some(route_tool_call(name, &arguments))
}

pub fn route_tool_call(name: &str, arguments: &Value) -> Result<Value> {
    match name {
        "iota_memory_search" => {
            let query = arguments.get("query").and_then(Value::as_str).unwrap_or("");
            let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            let mode = arguments
                .get("mode")
                .and_then(Value::as_str)
                .map(parse_memory_search_mode)
                .transpose()?
                .unwrap_or(MemorySearchMode::Hybrid);
            let store = MemoryStore::open(&MemoryStore::default_path()?)?;
            let records = store.search_with_mode(query, limit, mode)?;
            Ok(
                json!({"content":[{"type":"text","text":serde_json::to_string(&records)?}],"structuredContent":{"records":records,"mode":format!("{:?}", mode).to_lowercase()},"isError":false}),
            )
        }
        "iota_memory_write" => route_memory_write(arguments),
        "iota_skill_search" => route_skill_search(arguments),
        "iota_skill_load" => route_skill_load(arguments),
        "iota_session_summary" => route_session_summary(arguments),
        "iota_handoff_publish" => route_handoff_publish(arguments),
        "iota_handoff_read" => route_handoff_read(arguments),
        _ if is_fun_tool(name) => route_fun_tool(name, arguments),
        _ if name.starts_with("iota_") => Ok(json!({
            "content":[{"type":"text","text":format!("iota tool '{}' is not routable in this context", name)}],
            "isError":true
        })),
        _ => Ok(json!({
            "content":[{"type":"text","text":format!("external MCP tool '{}' denied by iota policy", name)}],
            "isError":true
        })),
    }
}

fn route_memory_write(arguments: &Value) -> Result<Value> {
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("content is required"))?;
    let memory_type = parse_memory_type(
        arguments
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("episodic"),
    )?;
    let facet = arguments
        .get("facet")
        .and_then(Value::as_str)
        .map(parse_memory_facet)
        .transpose()?;
    let scope = parse_memory_scope(
        arguments
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("session"),
    )?;
    let scope_id = arguments
        .get("scope_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| default_memory_scope_id(&scope, arguments));
    let merge_mode = arguments
        .get("merge_mode")
        .and_then(Value::as_str)
        .map(parse_memory_merge_mode)
        .transpose()?
        .unwrap_or(MemoryMergeMode::Auto);
    let store = MemoryStore::open(&MemoryStore::default_path()?)?;
    let id = store.insert_with_merge(
        MemoryInsert {
            memory_type,
            facet,
            scope,
            scope_id,
            content: content.to_string(),
            confidence: arguments
                .get("confidence")
                .and_then(Value::as_f64)
                .unwrap_or(1.0),
            source_backend: arguments
                .get("source_backend")
                .and_then(Value::as_str)
                .map(str::to_string),
            source_session_id: arguments
                .get("source_session_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            source_execution_id: arguments
                .get("source_execution_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            metadata_json: arguments.get("metadata").map(Value::to_string),
            ttl_days: arguments
                .get("ttl_days")
                .and_then(Value::as_i64)
                .unwrap_or(7),
            supersedes: arguments
                .get("supersedes")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        merge_mode,
    )?;
    Ok(
        json!({"content":[{"type":"text","text":id.clone().unwrap_or_default()}],"structuredContent":{"id":id,"merge_mode":format!("{:?}", merge_mode).to_lowercase()},"isError":false}),
    )
}

fn route_skill_search(arguments: &Value) -> Result<Value> {
    let workspace = std::env::current_dir()?;
    let backend = arguments
        .get("backend")
        .and_then(Value::as_str)
        .unwrap_or("codex");
    let backend = crate::acp::AcpBackend::parse(backend)?;
    let registry = SkillRegistry::load(&workspace, &[]);
    let index = registry.skill_index(backend, 4000);
    Ok(
        json!({"content":[{"type":"text","text":index}],"structuredContent":{"index":index,"diagnostics":registry.diagnostics()},"isError":false}),
    )
}

fn route_skill_load(arguments: &Value) -> Result<Value> {
    let name = arguments
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("name is required"))?;
    let workspace = std::env::current_dir()?;
    let registry = SkillRegistry::load(&workspace, &[]);
    let skill = registry
        .get(name)
        .ok_or_else(|| anyhow!("skill {} not found", name))?;
    Ok(
        json!({"content":[{"type":"text","text":skill.body}],"structuredContent":{"metadata":skill.metadata,"body":skill.body},"isError":false}),
    )
}

fn route_session_summary(arguments: &Value) -> Result<Value> {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let ledger = SessionLedger::open(&SessionLedger::default_path()?)?;
    let summary = ledger.summary(session_id)?;
    Ok(
        json!({"content":[{"type":"text","text":serde_json::to_string(&summary)?}],"structuredContent":{"summary":summary},"isError":false}),
    )
}

fn route_handoff_publish(arguments: &Value) -> Result<Value> {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let summary = arguments
        .get("summary")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("summary is required"))?;
    let workspace = std::env::current_dir()?;
    let ledger = SessionLedger::open(&SessionLedger::default_path()?)?;
    ledger.publish_handoff(
        session_id,
        arguments.get("from_backend").and_then(Value::as_str),
        arguments.get("to_backend").and_then(Value::as_str),
        &workspace,
        summary,
    )?;
    Ok(
        json!({"content":[{"type":"text","text":"ok"}],"structuredContent":{"ok":true},"isError":false}),
    )
}

fn route_handoff_read(arguments: &Value) -> Result<Value> {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let workspace = std::env::current_dir()?;
    let ledger = SessionLedger::open(&SessionLedger::default_path()?)?;
    let handoff = ledger.read_handoff(
        session_id,
        arguments.get("to_backend").and_then(Value::as_str),
        &workspace,
    )?;
    Ok(
        json!({"content":[{"type":"text","text":handoff.clone().unwrap_or_default()}],"structuredContent":{"handoff":handoff},"isError":false}),
    )
}

fn route_fun_tool(name: &str, arguments: &Value) -> Result<Value> {
    let text = crate::skill::fun_server::run_tool(name, arguments)?;
    Ok(json!({"content":[{"type":"text","text":text}],"isError":false}))
}

fn is_fun_tool(name: &str) -> bool {
    crate::skill::fun_server::TOOLS
        .iter()
        .any(|(tool, _)| *tool == name)
}

fn parse_memory_type(value: &str) -> Result<MemoryType> {
    match value {
        "semantic" => Ok(MemoryType::Semantic),
        "episodic" => Ok(MemoryType::Episodic),
        "procedural" => Ok(MemoryType::Procedural),
        other => Err(anyhow!("invalid memory type {}", other)),
    }
}

fn parse_memory_facet(value: &str) -> Result<MemoryFacet> {
    match value {
        "identity" => Ok(MemoryFacet::Identity),
        "preference" => Ok(MemoryFacet::Preference),
        "strategic" => Ok(MemoryFacet::Strategic),
        "domain" => Ok(MemoryFacet::Domain),
        other => Err(anyhow!("invalid memory facet {}", other)),
    }
}

fn parse_memory_scope(value: &str) -> Result<MemoryScope> {
    match value {
        "session" => Ok(MemoryScope::Session),
        "project" => Ok(MemoryScope::Project),
        "user" => Ok(MemoryScope::User),
        "global" => Ok(MemoryScope::Global),
        other => Err(anyhow!("invalid memory scope {}", other)),
    }
}

fn parse_memory_merge_mode(value: &str) -> Result<MemoryMergeMode> {
    match value {
        "auto" => Ok(MemoryMergeMode::Auto),
        "add" => Ok(MemoryMergeMode::Add),
        "update" => Ok(MemoryMergeMode::Update),
        "none" => Ok(MemoryMergeMode::None),
        other => Err(anyhow!("invalid memory merge_mode {}", other)),
    }
}

fn parse_memory_search_mode(value: &str) -> Result<MemorySearchMode> {
    match value {
        "keyword" => Ok(MemorySearchMode::Keyword),
        "vector" => Ok(MemorySearchMode::Vector),
        "hybrid" => Ok(MemorySearchMode::Hybrid),
        other => Err(anyhow!("invalid memory search mode {}", other)),
    }
}

fn default_memory_scope_id(scope: &MemoryScope, arguments: &Value) -> String {
    match scope {
        MemoryScope::User => "local-user".to_string(),
        MemoryScope::Project => std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "local-project".to_string()),
        MemoryScope::Session => arguments
            .get("source_session_id")
            .or_else(|| arguments.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or("local")
            .to_string(),
        MemoryScope::Global => "global".to_string(),
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
