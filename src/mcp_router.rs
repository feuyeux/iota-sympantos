use anyhow::Result;
use serde_json::{Value, json};

use crate::memory::MemoryStore;
use crate::skills::SkillRegistry;

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
            let store = MemoryStore::open(&MemoryStore::default_path()?)?;
            let records = store.search(query, limit)?;
            Ok(
                json!({"content":[{"type":"text","text":serde_json::to_string(&records)?}],"structuredContent":{"records":records},"isError":false}),
            )
        }
        "iota_skill_search" => {
            let workspace = std::env::current_dir()?;
            let backend = arguments
                .get("backend")
                .and_then(Value::as_str)
                .unwrap_or("codex");
            let backend = crate::acp::AcpBackend::parse(backend)?;
            let registry = SkillRegistry::load(&workspace, &[]);
            Ok(
                json!({"content":[{"type":"text","text":registry.skill_index(backend, 4000)}],"isError":false}),
            )
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn denies_external_mcp_tools() {
        let result = route_tool_call("external_shell", &json!({})).unwrap();
        assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    }
}
