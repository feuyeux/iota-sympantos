use super::*;
use serde_json::json;

#[test]
fn memory_write_schema_requires_confidence() {
    let write_tool = tools()
        .into_iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("iota_memory_write"))
        .expect("memory write tool should be listed");

    assert_eq!(
        write_tool["inputSchema"]["required"],
        json!(["content", "type", "scope", "confidence"])
    );
}

#[test]
fn memory_write_schema_declares_type_facet_conditions() {
    let write_tool = tools()
        .into_iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("iota_memory_write"))
        .expect("memory write tool should be listed");

    let all_of = write_tool["inputSchema"]["allOf"]
        .as_array()
        .expect("schema should contain conditional constraints");
    assert_eq!(all_of.len(), 2);
    assert_eq!(all_of[0]["then"]["required"], json!(["facet"]));
    assert_eq!(
        all_of[1]["if"]["properties"]["type"]["enum"],
        json!(["episodic", "procedural"])
    );
    assert_eq!(all_of[1]["then"]["not"]["required"], json!(["facet"]));
}
