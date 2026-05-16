#[cfg(test)]
mod backend_tests {
    use super::super::*;

    #[test]
    fn parse_all_aliases() {
        let cases = [
            ("claude", AcpBackend::ClaudeCode),
            ("claude-code", AcpBackend::ClaudeCode),
            ("claudecode", AcpBackend::ClaudeCode),
            ("codex", AcpBackend::Codex),
            ("gemini", AcpBackend::Gemini),
            ("gemini-cli", AcpBackend::Gemini),
            ("hermes", AcpBackend::Hermes),
            ("hermes-agent", AcpBackend::Hermes),
            ("opencode", AcpBackend::OpenCode),
            ("open-code", AcpBackend::OpenCode),
        ];
        for (input, expected) in cases {
            assert_eq!(
                AcpBackend::parse(input).unwrap(),
                expected,
                "input={}",
                input
            );
        }
    }

    #[test]
    fn parse_unknown_backend_errors() {
        assert!(AcpBackend::parse("unknown").is_err());
    }

    #[test]
    fn display_round_trips() {
        for backend in ALL_BACKENDS {
            let text = backend.to_string();
            assert_eq!(AcpBackend::parse(&text).unwrap(), backend);
        }
    }

    #[test]
    fn command_returns_valid_executable() {
        for backend in ALL_BACKENDS {
            let (exe, args) = backend.command();
            assert!(!exe.is_empty());
            assert!(!args.is_empty());
        }
    }

    #[test]
    fn is_backend_alias_matches_known() {
        assert!(parser::is_backend_alias("codex"));
        assert!(parser::is_backend_alias("gemini-cli"));
        assert!(!parser::is_backend_alias("unknown-tool"));
    }
}

#[cfg(test)]
mod extract_tests {
    use super::super::*;
    use serde_json::json;

    #[test]
    fn extract_text_from_string_value() {
        assert_eq!(extract_text(&json!("hello")), Some("hello".to_string()));
    }

    #[test]
    fn extract_text_from_text_key() {
        assert_eq!(extract_text(&json!({"text": "hi"})), Some("hi".to_string()));
    }

    #[test]
    fn extract_text_from_content_key() {
        assert_eq!(
            extract_text(&json!({"content": "data"})),
            Some("data".to_string())
        );
    }

    #[test]
    fn extract_text_from_message_key() {
        assert_eq!(
            extract_text(&json!({"message": "msg"})),
            Some("msg".to_string())
        );
    }

    #[test]
    fn extract_text_from_result_key() {
        assert_eq!(
            extract_text(&json!({"result": "res"})),
            Some("res".to_string())
        );
    }

    #[test]
    fn extract_text_from_output_key() {
        assert_eq!(
            extract_text(&json!({"output": "out"})),
            Some("out".to_string())
        );
    }

    #[test]
    fn extract_text_from_content_object_with_text() {
        assert_eq!(
            extract_text(&json!({"content": {"text": "nested"}})),
            Some("nested".to_string())
        );
    }

    #[test]
    fn extract_text_from_content_array() {
        let value =
            json!({"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]});
        assert_eq!(extract_text(&value), Some("ab".to_string()));
    }

    #[test]
    fn extract_text_from_empty_content_array_returns_none() {
        assert_eq!(extract_text(&json!({"content": []})), None);
    }

    #[test]
    fn extract_text_from_number_returns_none() {
        assert_eq!(extract_text(&json!(42)), None);
    }

    #[test]
    fn extract_final_text_prefers_final_message() {
        let value = json!({"finalMessage": "final", "text": "other"});
        assert_eq!(extract_final_text(&value), Some("final".to_string()));
    }

    #[test]
    fn extract_final_text_falls_back_to_extract_text() {
        let value = json!({"text": "fallback"});
        assert_eq!(extract_final_text(&value), Some("fallback".to_string()));
    }

    #[test]
    fn is_terminal_result_with_stop_reason() {
        assert!(is_terminal_result(&json!({"stopReason": "end_turn"})));
    }

    #[test]
    fn is_terminal_result_with_text() {
        assert!(is_terminal_result(&json!({"text": "done"})));
    }

    #[test]
    fn is_terminal_result_empty_is_false() {
        assert!(!is_terminal_result(&json!({"foo": 1})));
    }
}

#[cfg(test)]
mod session_update_tests {
    use super::super::*;
    use serde_json::json;

    #[test]
    fn text_from_agent_message() {
        let params = json!({"update": {"sessionUpdate": "agent_message", "text": "chunk"}});
        assert_eq!(
            text_from_session_update(Some(&params)),
            Some("chunk".to_string())
        );
    }

    #[test]
    fn text_from_agent_message_chunk() {
        let params = json!({"update": {"sessionUpdate": "agent_message_chunk", "text": "c"}});
        assert_eq!(
            text_from_session_update(Some(&params)),
            Some("c".to_string())
        );
    }

    #[test]
    fn text_from_type_field() {
        let params = json!({"update": {"type": "agent_message", "text": "t"}});
        assert_eq!(
            text_from_session_update(Some(&params)),
            Some("t".to_string())
        );
    }

    #[test]
    fn text_from_unknown_update_type_returns_none() {
        let params = json!({"update": {"sessionUpdate": "tool_call", "text": "ignored"}});
        assert_eq!(text_from_session_update(Some(&params)), None);
    }

    #[test]
    fn text_from_none_params_returns_none() {
        assert_eq!(text_from_session_update(None), None);
    }
}

#[cfg(test)]
mod tool_call_parts_tests {
    use super::super::*;
    use serde_json::json;

    #[test]
    fn extracts_name_and_arguments() {
        let params = json!({"name": "read_file", "arguments": {"path": "/tmp"}});
        let (name, args) = acp_tool_call_parts(Some(&params));
        assert_eq!(name, "read_file");
        assert_eq!(args, json!({"path": "/tmp"}));
    }

    #[test]
    fn uses_tool_name_key() {
        let params = json!({"toolName": "write", "input": {"data": "x"}});
        let (name, args) = acp_tool_call_parts(Some(&params));
        assert_eq!(name, "write");
        assert_eq!(args, json!({"data": "x"}));
    }

    #[test]
    fn defaults_when_no_params() {
        let (name, args) = acp_tool_call_parts(None);
        assert_eq!(name, "tool");
        assert!(args.is_null());
    }
}

#[cfg(test)]
mod permission_request_id_tests {
    use super::super::*;
    use serde_json::json;

    #[test]
    fn extracts_id_from_message() {
        let msg = AcpWireMessage {
            id: Some(json!("req-1")),
            method: None,
            params: None,
            result: None,
            error: None,
        };
        assert_eq!(permission_request_id(&msg).unwrap(), json!("req-1"));
    }

    #[test]
    fn falls_back_to_request_id_in_params() {
        let msg = AcpWireMessage {
            id: None,
            method: None,
            params: Some(json!({"requestId": "fallback-id"})),
            result: None,
            error: None,
        };
        assert_eq!(permission_request_id(&msg).unwrap(), json!("fallback-id"));
    }

    #[test]
    fn errors_when_no_id() {
        let msg = AcpWireMessage {
            id: None,
            method: None,
            params: Some(json!({})),
            result: None,
            error: None,
        };
        assert!(permission_request_id(&msg).is_err());
    }
}

#[cfg(test)]
mod misc_tests {
    use super::super::*;

    #[test]
    fn synthetic_output_has_zero_timing() {
        let output = AcpPromptOutput::synthetic("test".to_string());
        assert_eq!(output.text, "test");
        assert_eq!(output.timing.total_ms, 0);
        assert!(!output.timing.client_started);
        assert!(output.backend_session_id.is_none());
    }

    #[test]
    fn should_forward_backend_stderr_patterns() {
        assert!(util::should_forward_backend_stderr(
            "context MCP memory loaded"
        ));
        assert!(util::should_forward_backend_stderr(
            "iota::context::server init"
        ));
        assert!(util::should_forward_backend_stderr("[iota log] something"));
        assert!(util::should_forward_backend_stderr("[mcp stderr: x]"));
        assert!(!util::should_forward_backend_stderr("some random output"));
    }

    #[test]
    fn elapsed_ms_is_non_negative() {
        let now = std::time::Instant::now();
        assert!(util::elapsed_ms(now) < 100);
    }
}

#[cfg(test)]
mod arg_tests {
    use super::super::*;

    #[test]
    fn parses_run_flags_and_prompt_parts() {
        let cwd = std::env::current_dir().unwrap();
        let args = vec![
            "--backend".to_string(),
            "gemini".to_string(),
            "--cwd".to_string(),
            cwd.display().to_string(),
            "--show-native".to_string(),
            "--daemon".to_string(),
            "--log-events".to_string(),
            "--timing".to_string(),
            "--timeout-ms".to_string(),
            "1234".to_string(),
            "hello".to_string(),
            "world".to_string(),
        ];

        let options = parse_acp_args(&args).unwrap();

        assert_eq!(options.backend, AcpBackend::Gemini);
        assert_eq!(options.cwd, cwd);
        assert_eq!(options.prompt, "hello world");
        assert!(options.show_native);
        assert!(options.use_daemon);
        assert!(options.log_events);
        assert!(options.timing);
        assert_eq!(options.timeout_ms, 1234);
    }

    #[test]
    fn parses_positional_backend_alias_before_prompt() {
        let options = parse_acp_args(&[
            "open-code".to_string(),
            "inspect".to_string(),
            "repo".to_string(),
        ])
        .unwrap();

        assert_eq!(options.backend, AcpBackend::OpenCode);
        assert_eq!(options.prompt, "inspect repo");
    }

    #[test]
    fn double_dash_treats_backend_like_prompt_text() {
        let options = parse_acp_args(&[
            "codex".to_string(),
            "--".to_string(),
            "gemini".to_string(),
            "literal".to_string(),
        ])
        .unwrap();

        assert_eq!(options.backend, AcpBackend::Codex);
        assert_eq!(options.prompt, "gemini literal");
    }

    #[test]
    fn rejects_zero_timeout() {
        let result = parse_acp_args(&[
            "--timeout-ms".to_string(),
            "0".to_string(),
            "prompt".to_string(),
        ]);

        let err = match result {
            Ok(_) => panic!("zero timeout should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("greater than 0"));
    }

    #[test]
    fn parses_5backend_flag() {
        let options = parse_acp_args(&["5backend".to_string(), "test prompt".to_string()]).unwrap();

        assert!(options.multi_backend);
        assert_eq!(options.prompt, "test prompt");
    }
}
