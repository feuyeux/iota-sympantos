use crate::runtime_event::LogEvent;
use opentelemetry::KeyValue;

pub fn log_event_attributes(log: &LogEvent) -> Vec<KeyValue> {
    let mut attrs = Vec::new();
    if let Some(ref eid) = log.execution_id {
        attrs.push(KeyValue::new("iota.execution.id", eid.clone()));
    }
    if let Some(ref sid) = log.session_id {
        attrs.push(KeyValue::new("iota.session.id", sid.clone()));
    }
    if let Some(ref b) = log.backend {
        attrs.push(KeyValue::new("iota.backend", b.clone()));
    }
    if let Some(ref r) = log.route {
        attrs.push(KeyValue::new("iota.route", r.clone()));
    }
    if let Some(ref tn) = log.tool_name {
        attrs.push(KeyValue::new("iota.tool.name", tn.clone()));
    }
    if let Some(ref tcid) = log.tool_call_id {
        attrs.push(KeyValue::new("iota.tool.call_id", tcid.clone()));
    }
    if let Some(ok) = log.ok {
        attrs.push(KeyValue::new("iota.ok", ok));
    }
    if let Some(ms) = log.latency_ms {
        attrs.push(KeyValue::new("iota.latency_ms", ms as i64));
    }
    if let serde_json::Value::Object(map) = &log.fields {
        for (k, v) in map {
            let key = format!("iota.field.{}", k);
            match v {
                serde_json::Value::String(s) => attrs.push(KeyValue::new(key, s.clone())),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        attrs.push(KeyValue::new(key, i));
                    } else if let Some(f) = n.as_f64() {
                        attrs.push(KeyValue::new(key, f));
                    }
                }
                serde_json::Value::Bool(b) => attrs.push(KeyValue::new(key, *b)),
                other => attrs.push(KeyValue::new(key, other.to_string())),
            }
        }
    }
    attrs
}
