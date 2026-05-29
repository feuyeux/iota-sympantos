//! Shared tool dispatch logic for iota MCP tools.
//!
//! Both the stdio MCP server (`mcp::server`) and the ACP stream interceptor
//! (`mcp::router`) delegate tool execution to this module so that parsing,
//! validation, and business logic live in exactly one place.

use std::path::Path;

use serde_json::{Value, json};

use iota_kanban::{Board, CreateTaskRequest, KanbanStore, Status, Task, TaskFilter, TaskId};

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
    pub kanban: Option<&'a dyn KanbanStore>,
    pub skills: &'a SkillRegistry,
    pub workspace: &'a Path,
}

// ---------------------------------------------------------------------------
// McpTool Trait & Registry
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::LazyLock;

/// Trait defining a dynamic MCP tool.
pub trait McpTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String>;
}

pub struct McpToolRegistry {
    tools: HashMap<String, Box<dyn McpTool>>,
}

impl McpToolRegistry {
    pub fn new() -> Self {
        let mut tools: HashMap<String, Box<dyn McpTool>> = HashMap::new();

        let t = MemorySearchTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = MemoryWriteTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = SkillSearchTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = SkillLoadTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = SessionSummaryTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = HandoffPublishTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = HandoffReadTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = KanbanCreateTaskTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = KanbanListTasksTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = KanbanReadyTaskTool;
        tools.insert(t.name().to_string(), Box::new(t));

        Self { tools }
    }

    pub fn is_known_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn dispatch(&self, name: &str, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        if let Some(tool) = self.tools.get(name) {
            tool.execute(ctx, args)
        } else {
            Err(format!("unknown tool {}", name))
        }
    }

    pub fn list_tools(&self) -> Vec<Value> {
        // Sort tools by name to ensure stable list order
        let mut sorted_tools: Vec<&Box<dyn McpTool>> = self.tools.values().collect();
        sorted_tools.sort_by_key(|t| t.name());
        sorted_tools
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "inputSchema": tool.input_schema(),
                })
            })
            .collect()
    }
}

pub static REGISTRY: LazyLock<McpToolRegistry> = LazyLock::new(McpToolRegistry::new);

/// Execute a named iota tool and return the raw business-logic result.
///
/// Compatibility wrapper forwarding to the global `REGISTRY`.
#[allow(dead_code)]
pub fn dispatch_tool(ctx: &ToolContext, name: &str, args: &Value) -> Result<Value, String> {
    REGISTRY.dispatch(name, ctx, args)
}

/// Return whether `name` is a tool this module can dispatch.
///
/// Compatibility wrapper forwarding to the global `REGISTRY`.
#[allow(dead_code)]
pub fn is_known_tool(name: &str) -> bool {
    REGISTRY.is_known_tool(name)
}

// ---------------------------------------------------------------------------
// Default McpTool Implementations
// ---------------------------------------------------------------------------

struct MemorySearchTool;
impl McpTool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "iota_memory_search"
    }

    fn description(&self) -> &'static str {
        "Search unified iota memory by keyword. Returns matching records across all types and scopes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search keyword"},
                "limit": {"type": "integer", "description": "Max results (default 20)"},
                "mode": {"type": "string", "enum": ["hybrid", "vector", "keyword"], "description": "Search strategy (default hybrid)"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_memory_search(ctx, args)
    }
}

struct MemoryWriteTool;
impl McpTool for MemoryWriteTool {
    fn name(&self) -> &'static str {
        "iota_memory_write"
    }

    fn description(&self) -> &'static str {
        "Persist one memory record to iota's unified memory store. Classification, split, scope, and confidence policy is defined by the core skill `iota-memory-taxonomy`; load that skill before choosing memory fields. This tool enforces only the storage protocol shape."
    }

    fn input_schema(&self) -> Value {
        json!({
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
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_memory_write(ctx, args)
    }
}

struct SkillSearchTool;
impl McpTool for SkillSearchTool {
    fn name(&self) -> &'static str {
        "iota_skill_search"
    }

    fn description(&self) -> &'static str {
        "Search available iota skill index for the current backend."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "backend": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_skill_search(ctx, args)
    }
}

struct SkillLoadTool;
impl McpTool for SkillLoadTool {
    fn name(&self) -> &'static str {
        "iota_skill_load"
    }

    fn description(&self) -> &'static str {
        "Load the full body of a named iota skill."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_skill_load(ctx, args)
    }
}

struct SessionSummaryTool;
impl McpTool for SessionSummaryTool {
    fn name(&self) -> &'static str {
        "iota_session_summary"
    }

    fn description(&self) -> &'static str {
        "Read summary of the current iota session."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_session_summary(ctx, args)
    }
}

struct HandoffPublishTool;
impl McpTool for HandoffPublishTool {
    fn name(&self) -> &'static str {
        "iota_handoff_publish"
    }

    fn description(&self) -> &'static str {
        "Publish a handoff summary when switching backends."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": true
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_handoff_publish(ctx, args)
    }
}

struct HandoffReadTool;
impl McpTool for HandoffReadTool {
    fn name(&self) -> &'static str {
        "iota_handoff_read"
    }

    fn description(&self) -> &'static str {
        "Read the latest handoff for this session."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": true
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_handoff_read(ctx, args)
    }
}

struct KanbanCreateTaskTool;
impl McpTool for KanbanCreateTaskTool {
    fn name(&self) -> &'static str {
        "iota_kanban_create_task"
    }

    fn description(&self) -> &'static str {
        "Create a task in iota's Kanban DB. Defaults status to triage for raw ideas. Use status=ready or iota_kanban_ready_task only when the task should be dispatcher-claimable."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string"},
                "status": {"type": "string", "enum": ["triage", "todo", "ready", "running", "blocked", "done", "archived"], "description": "Default triage"},
                "assignee": {"type": "string", "description": "Hermes profile name, e.g. research-agent"},
                "priority": {"type": "integer"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "board_slug": {"type": "string"},
                "board_name": {"type": "string"},
                "workspace_kind": {"type": "string"},
                "workspace_path": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_kanban_create_task(ctx, args)
    }
}

struct KanbanListTasksTool;
impl McpTool for KanbanListTasksTool {
    fn name(&self) -> &'static str {
        "iota_kanban_list_tasks"
    }

    fn description(&self) -> &'static str {
        "List tasks from iota's Kanban DB for verification after creating or dispatching tasks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["triage", "todo", "ready", "running", "blocked", "done", "archived"]},
                "assignee": {"type": "string"},
                "limit": {"type": "integer"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_kanban_list_tasks(ctx, args)
    }
}

struct KanbanReadyTaskTool;
impl McpTool for KanbanReadyTaskTool {
    fn name(&self) -> &'static str {
        "iota_kanban_ready_task"
    }

    fn description(&self) -> &'static str {
        "Move an existing iota Kanban task to ready so the desktop dispatcher can execute it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": {"type": "integer"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_kanban_ready_task(ctx, args)
    }
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

fn dispatch_kanban_create_task(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    let title = required_string(args, "title")?;
    let board = resolve_kanban_board(ctx, args)?;
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .map(parse_kanban_status)
        .transpose()?
        .unwrap_or(Status::Triage);
    let tags = args
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "tags must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let task_id = kanban
        .create_task(CreateTaskRequest {
            board_id: board.id,
            title: title.to_string(),
            body: args.get("body").and_then(Value::as_str).map(str::to_string),
            status: Some(status),
            assignee: args
                .get("assignee")
                .and_then(Value::as_str)
                .map(str::to_string),
            priority: args
                .get("priority")
                .and_then(Value::as_i64)
                .map(|value| value as i32),
            tags,
            workspace_kind: args
                .get("workspace_kind")
                .and_then(Value::as_str)
                .map(str::to_string),
            workspace_path: args
                .get("workspace_path")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from),
        })
        .map_err(|err| err.to_string())?;
    Ok(json!({
        "ok": true,
        "task_id": task_id,
        "status": status.as_str(),
        "board": board,
        "auto_dispatch": status == Status::Ready
    }))
}

fn dispatch_kanban_list_tasks(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .map(parse_kanban_status)
        .transpose()?;
    let tasks = kanban
        .list_tasks(TaskFilter {
            status,
            assignee: args
                .get("assignee")
                .and_then(Value::as_str)
                .map(str::to_string),
            limit: args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize),
            ..Default::default()
        })
        .map_err(|err| err.to_string())?;
    Ok(json!({"tasks": summarize_kanban_tasks(tasks)}))
}

fn dispatch_kanban_ready_task(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    let task_id = args
        .get("task_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "task_id is required".to_string())? as TaskId;
    kanban
        .transition(task_id, Status::Ready)
        .map_err(|err| err.to_string())?;
    Ok(json!({"ok": true, "task_id": task_id, "status": "ready", "auto_dispatch": true}))
}

fn resolve_kanban_board(ctx: &ToolContext, args: &Value) -> Result<Board, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    if let Some(slug) = args.get("board_slug").and_then(Value::as_str) {
        return match kanban.get_board(slug) {
            Ok(board) => Ok(board),
            Err(_) => {
                let name = args
                    .get("board_name")
                    .and_then(Value::as_str)
                    .unwrap_or(slug);
                let board_id = kanban
                    .create_board(slug, name)
                    .map_err(|err| err.to_string())?;
                kanban.get_board(slug).map_err(|err| {
                    format!("created board {board_id}, but failed to read it back: {err}")
                })
            }
        };
    }
    let boards = kanban.list_boards().map_err(|err| err.to_string())?;
    if let Some(board) = boards.into_iter().next() {
        return Ok(board);
    }
    let board_id = kanban
        .create_board("default", "Default")
        .map_err(|err| err.to_string())?;
    kanban
        .get_board("default")
        .map_err(|err| format!("created board {board_id}, but failed to read it back: {err}"))
}

fn summarize_kanban_tasks(tasks: Vec<Task>) -> Vec<Value> {
    tasks
        .into_iter()
        .map(|task| {
            json!({
                "id": task.id,
                "title": task.title,
                "status": task.status.as_str(),
                "assignee": task.assignee,
                "priority": task.priority,
                "tags": task.tags,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Parsers & validators (single canonical copy)
// ---------------------------------------------------------------------------

fn parse_kanban_status(value: &str) -> Result<Status, String> {
    value
        .parse::<Status>()
        .map_err(|_| format!("invalid kanban status {}", value))
}

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
