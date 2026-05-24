use super::*;

#[test]
fn skill_execution_mode_deserializes_normalized_values() {
    let mode: SkillExecutionMode = serde_yaml::from_str("\" McP \"").unwrap();
    assert_eq!(mode, SkillExecutionMode::Mcp);
    assert_eq!(serde_yaml::to_string(&mode).unwrap(), "mcp\n");
}

#[test]
fn skill_execution_mode_rejects_unknown_values() {
    let err = serde_yaml::from_str::<SkillExecutionMode>("\"automatic\"").unwrap_err();
    assert!(err.to_string().contains("invalid skill execution mode"));
}

#[test]
fn skill_cache_starts_empty() {
    let cache = SkillCache::default();
    assert!(cache.entry.is_none());
}

#[test]
fn skill_metadata_validation() {
    let mut metadata = SkillMetadata {
        name: "test".to_string(),
        version: None,
        summary: None,
        description: None,
        triggers: vec!["test".to_string()],
        backends: vec![],
        execution: SkillExecution::default(),
        output: SkillOutput::default(),
        failure_policy: None,
    };
    assert!(metadata.validate().is_ok());

    metadata.triggers = vec![];
    assert!(metadata.validate().is_ok());
    metadata.triggers = vec!["test".to_string()];

    metadata.name = " ".to_string();
    assert!(metadata.validate().is_err());
    metadata.name = "test".to_string();

    metadata.triggers = vec!["".to_string()];
    assert!(metadata.validate().is_err());
    metadata.triggers = vec!["test".to_string()];

    metadata.execution.tools = vec![
        SkillTool {
            name: "tool1".to_string(),
            alias: None,
        },
        SkillTool {
            name: "tool1".to_string(),
            alias: None,
        },
    ];
    assert!(metadata.validate().is_err());
}
