use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio::task::JoinSet;

use crate::mcp_client::{self, McpToolCall};
use crate::runtime_event::{RuntimeEvent, ToolCallEvent, ToolResultEvent};
use crate::skills::{Skill, render_template};

pub struct SkillRunOutput {
    pub text: String,
    pub events: Vec<RuntimeEvent>,
}

pub async fn run_engine_skill(skill: &Skill, prompt: &str) -> Result<Option<SkillRunOutput>> {
    if skill.metadata.execution.mode != "mcp" {
        return Ok(None);
    }

    let mut events = Vec::new();
    let mut tool_outputs = Vec::new();
    if !skill.metadata.execution.tools.is_empty() {
        let server = skill
            .metadata
            .execution
            .server
            .as_deref()
            .unwrap_or("iota-fun");
        let (command, args) = server_command(server);
        for tool in &skill.metadata.execution.tools {
            let arguments = json!({"source": prompt});
            let call_id = format!("skill:{}:{}", skill.metadata.name, tool.label());
            events.push(RuntimeEvent::ToolCall(ToolCallEvent {
                id: call_id,
                name: tool.name.clone(),
                arguments,
            }));
        }
        let calls = skill
            .metadata
            .execution
            .tools
            .iter()
            .map(|tool| {
                (
                    format!("skill:{}:{}", skill.metadata.name, tool.label()),
                    tool.name.clone(),
                    tool.label().to_string(),
                    json!({"source": prompt}),
                )
            })
            .collect::<Vec<_>>();
        let results = if skill.metadata.execution.parallel {
            run_parallel(command, args, calls).await
        } else {
            run_sequential(command, args, calls).await
        };
        for (call_id, tool, alias, ok, result) in results {
            tool_outputs.push(json!({"name": tool, "as": alias, "result": result.clone()}));
            events.push(RuntimeEvent::ToolResult(ToolResultEvent {
                id: call_id,
                name: tool,
                ok,
                result,
            }));
        }
    }

    let mut text = render_template(skill, prompt);
    for item in &tool_outputs {
        if let (Some(alias), Some(result)) =
            (item.get("as").and_then(Value::as_str), item.get("result"))
        {
            text = text.replace(&format!("{{{{{}}}}}", alias), &render_tool_result(result));
        }
    }
    if text.contains("{{tool_results}}") {
        text = text.replace("{{tool_results}}", &render_tool_results(&tool_outputs));
    } else if !tool_outputs.is_empty() {
        text.push_str("\n");
        text.push_str(&render_tool_results(&tool_outputs));
    }

    events.push(RuntimeEvent::ToolResult(ToolResultEvent {
        id: format!("skill:{}", skill.metadata.name),
        name: skill.metadata.name.clone(),
        ok: true,
        result: json!({"mode":"mcp","text":text}),
    }));
    Ok(Some(SkillRunOutput { text, events }))
}

fn server_command(server: &str) -> (String, Vec<String>) {
    if server == "iota-fun" {
        let command = std::env::current_exe()
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "iota".to_string());
        (command, vec!["fun-mcp".to_string()])
    } else if server == "iota-context" {
        let command = std::env::current_exe()
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "iota".to_string());
        (command, vec!["context-mcp".to_string()])
    } else {
        (server.to_string(), Vec::new())
    }
}

fn render_tool_results(results: &[Value]) -> String {
    serde_json::to_string_pretty(results).unwrap_or_else(|_| "[]".to_string())
}

fn render_tool_result(result: &Value) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string())
}

async fn run_sequential(
    command: String,
    args: Vec<String>,
    calls: Vec<(String, String, String, Value)>,
) -> Vec<(String, String, String, bool, Value)> {
    let mut results = Vec::new();
    for (call_id, tool, alias, arguments) in calls {
        results.push(run_one_tool(&command, &args, call_id, tool, alias, arguments).await);
    }
    results
}

async fn run_parallel(
    command: String,
    args: Vec<String>,
    calls: Vec<(String, String, String, Value)>,
) -> Vec<(String, String, String, bool, Value)> {
    let mut set = JoinSet::new();
    for (call_id, tool, alias, arguments) in calls {
        let command = command.clone();
        let args = args.clone();
        set.spawn(
            async move { run_one_tool(&command, &args, call_id, tool, alias, arguments).await },
        );
    }
    let mut results = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok(result) => results.push(result),
            Err(err) => results.push((
                "skill:join".to_string(),
                "join".to_string(),
                "join".to_string(),
                false,
                json!({"error": err.to_string()}),
            )),
        }
    }
    results
}

async fn run_one_tool(
    command: &str,
    args: &[String],
    call_id: String,
    tool: String,
    alias: String,
    arguments: Value,
) -> (String, String, String, bool, Value) {
    let result = mcp_client::call_stdio(
        command,
        args,
        &BTreeMap::new(),
        McpToolCall {
            name: tool.clone(),
            arguments,
        },
        10_000,
    )
    .await;
    match result {
        Ok(result) => (call_id, tool, alias, result.ok, result.content),
        Err(err) => (
            call_id,
            tool,
            alias,
            false,
            json!({"error": err.to_string()}),
        ),
    }
}
