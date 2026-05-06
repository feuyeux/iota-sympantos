use super::*;

#[test]
fn parses_response_id_and_error() {
    let message =
        parse_message_line(r#"{"jsonrpc":"2.0","id":"x","result":{"ok":true}}"#, false).unwrap();
    assert!(is_response_id(&message, "x"));
    assert!(!is_response_id(&message, "y"));

    let error = parse_message_line(
        r#"{"jsonrpc":"2.0","id":"x","error":{"code":1,"message":"bad"}}"#,
        false,
    )
    .unwrap();
    let error = error.error.unwrap();
    assert_eq!(format_acp_error(&error), "ACP error 1: bad");
}

#[test]
fn numeric_id_is_matched() {
    let message = parse_message_line(r#"{"jsonrpc":"2.0","id":42,"result":{}}"#, false).unwrap();
    assert!(is_response_id(&message, "42"));
    assert!(!is_response_id(&message, "43"));
}

#[test]
fn error_without_code_omits_prefix() {
    let error = AcpWireError {
        code: None,
        message: "connection reset".to_string(),
        data: None,
    };
    assert_eq!(format_acp_error(&error), "connection reset");
}

#[test]
fn error_with_data_appends_data() {
    let error = AcpWireError {
        code: Some(500),
        message: "internal".to_string(),
        data: Some(serde_json::json!({"detail": "oops"})),
    };
    let text = format_acp_error(&error);
    assert!(text.contains("ACP error 500: internal"));
    assert!(text.contains("oops"));
}

#[test]
fn parse_rejects_non_json() {
    assert!(parse_message_line("not json", false).is_err());
}

#[test]
fn method_message_has_no_id() {
    let message =
        parse_message_line(r#"{"jsonrpc":"2.0","method":"ping","params":{}}"#, false).unwrap();
    assert!(!is_response_id(&message, "0"));
    assert_eq!(message.method.as_deref(), Some("ping"));
}
