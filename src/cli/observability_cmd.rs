use anyhow::{Context, Result, bail};

pub(super) async fn run_logs_command(args: &[String]) -> Result<()> {
    let execution_id = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("Usage: iota logs <execution_id>"))?;
    let loki_url =
        std::env::var("IOTA_LOKI_URL").unwrap_or_else(|_| "http://localhost:3100".to_string());
    let query = format!(r#"{{iota_execution_id=\"{}\"}}"#, execution_id);
    let url = format!(
        "{}/loki/api/v1/query_range?query={}&limit=1000",
        loki_url,
        urlencoding::encode(&query)
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Loki at {}", loki_url))?;
    if !resp.status().is_success() {
        bail!("Loki query failed with status {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await?;
    if let Some(results) = body["data"]["result"].as_array() {
        for stream in results {
            if let Some(values) = stream["values"].as_array() {
                for entry in values {
                    if let Some(arr) = entry.as_array()
                        && arr.len() >= 2
                        && let Some(line) = arr[1].as_str()
                    {
                        println!("{}", line);
                    }
                }
            }
        }
    } else {
        println!("No logs found for execution {}", execution_id);
    }
    Ok(())
}

pub(super) async fn run_trace_command(args: &[String]) -> Result<()> {
    let trace_id = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("Usage: iota trace <trace_id>"))?;
    let jaeger_url =
        std::env::var("IOTA_JAEGER_URL").unwrap_or_else(|_| "http://localhost:16686".to_string());
    let url = format!("{}/api/traces/{}", jaeger_url, trace_id);
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Jaeger at {}", jaeger_url))?;
    if !resp.status().is_success() {
        bail!("Jaeger query failed with status {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await?;
    if let Some(traces) = body["data"].as_array() {
        for trace in traces {
            if let Some(spans) = trace["spans"].as_array() {
                for span in spans {
                    let name = span["operationName"].as_str().unwrap_or("?");
                    let duration_us = span["duration"].as_u64().unwrap_or(0);
                    let duration_ms = duration_us / 1000;
                    let depth = if span["references"]
                        .as_array()
                        .map(|r| r.is_empty())
                        .unwrap_or(true)
                    {
                        0
                    } else {
                        1
                    };
                    let indent = "  ".repeat(depth);
                    println!("{}├── {} ({}ms)", indent, name, duration_ms);
                }
            }
        }
    } else {
        println!("No trace found for {}", trace_id);
    }
    Ok(())
}
