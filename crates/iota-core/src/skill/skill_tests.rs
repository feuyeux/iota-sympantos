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

#[test]
fn core_memory_taxonomy_skill_is_available_without_workspace_files() {
    let workspace = std::env::temp_dir().join(format!(
        "iota-empty-skill-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    let registry = SkillRegistry::load(&workspace, &[]);

    let skill = registry
        .get("iota-memory-taxonomy")
        .expect("core memory taxonomy skill should be built in");

    assert!(
        skill
            .path
            .ends_with("src/skill/core/iota-memory-taxonomy/SKILL.md")
    );
    assert!(skill.body.contains("identity"));
    assert!(skill.body.contains("preference"));
    assert!(skill.body.contains("strategic"));
    assert!(skill.body.contains("domain"));
    assert!(skill.body.contains("procedural"));
    assert!(skill.body.contains("episodic"));
    assert!(skill.body.contains("one iota_memory_write call"));
    assert!(!skill.body.contains("主驾"));
    assert!(!skill.body.contains("智舱"));
}

#[test]
fn core_memory_taxonomy_skill_appears_in_skill_index() {
    let workspace = std::env::temp_dir().join(format!(
        "iota-empty-skill-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    let registry = SkillRegistry::load(&workspace, &[]);
    let index = registry.skill_index(AcpBackend::Codex, 4000);

    assert!(index.contains("iota-memory-taxonomy"));
    assert!(index.contains("classify and write persistent memories"));
}
