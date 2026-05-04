use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use tokio::io::BufReader;
use tokio::time::{Duration, timeout};

#[derive(Debug, Deserialize)]
pub struct AcpWireMessage {
    #[serde(default)]
    pub id: Option<Value>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<AcpWireError>,
}

#[derive(Debug, Deserialize)]
pub struct AcpWireError {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

pub async fn read_next_line<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    timeout_ms: u64,
    message: &str,
) -> Result<Option<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    read_next_line_with_duration(lines, Duration::from_millis(timeout_ms), message).await
}

async fn read_next_line_with_duration<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    duration: Duration,
    message: &str,
) -> Result<Option<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    timeout(duration, lines.next_line())
        .await
        .map_err(|_| anyhow!(message.to_string()))?
        .context("Failed to read ACP stdout")
}

pub fn parse_message_line(line: &str, show_native: bool) -> Result<AcpWireMessage> {
    if show_native {
        eprintln!("[acp <=] {}", line);
    }
    serde_json::from_str::<AcpWireMessage>(line)
        .with_context(|| format!("ACP backend emitted non-JSON line: {}", line))
}

pub fn is_response_id(message: &AcpWireMessage, expected: &str) -> bool {
    match message.id.as_ref() {
        Some(Value::String(id)) => id == expected,
        Some(Value::Number(id)) => id.to_string() == expected,
        _ => false,
    }
}

pub fn format_acp_error(error: &AcpWireError) -> String {
    let mut text = error.message.clone();
    if let Some(code) = error.code {
        text = format!("ACP error {}: {}", code, text);
    }
    if let Some(data) = &error.data {
        text = format!("{} ({})", text, data);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_response_id_and_error() {
        let message = parse_message_line(r#"{"jsonrpc":"2.0","id":"x","result":{"ok":true}}"#, false).unwrap();
        assert!(is_response_id(&message, "x"));
        assert!(!is_response_id(&message, "y"));

        let error = parse_message_line(r#"{"jsonrpc":"2.0","id":"x","error":{"code":1,"message":"bad"}}"#, false).unwrap();
        let error = error.error.unwrap();
        assert_eq!(format_acp_error(&error), "ACP error 1: bad");
    }
}
