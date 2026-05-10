use super::*;

#[test]
fn renders_claude_skill_with_allowed_tools() {
    let skill = crate::skill::Skill {
        metadata: crate::skill::SkillMetadata {
            name: "review".to_string(),
            version: None,
            summary: Some("Review code".to_string()),
            description: None,
            triggers: Vec::new(),
            backends: Vec::new(),
            execution: crate::skill::SkillExecution::default(),
            output: crate::skill::SkillOutput::default(),
            failure_policy: None,
        },
        body: "Body".to_string(),
        path: PathBuf::from("SKILL.md"),
        priority: 0,
    };
    let rendered = render_backend_skill(AcpBackend::ClaudeCode, &skill);
    assert!(rendered.contains("allowed-tools"));
    assert!(rendered.contains("Body"));
}

#[test]
fn replaces_only_iota_block() {
    let existing = "user\n<!-- IOTA_START -->\nold\n<!-- IOTA_END -->\nmore\n";
    let updated = replace_iota_block(existing, "<!-- IOTA_START -->\nnew\n<!-- IOTA_END -->\n");
    assert!(updated.contains("user"));
    assert!(updated.contains("new"));
    assert!(updated.contains("more"));
    assert!(!updated.contains("old"));
}
