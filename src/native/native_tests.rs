use super::*;
use crate::memory::{MemoryFacet, MemoryInsert, MemoryScope, MemoryType};
use crate::skill::SkillRegistry;

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

#[test]
fn ignores_end_marker_before_start() {
    let existing =
        "header\n<!-- IOTA_END -->\nmid\n<!-- IOTA_START -->\nold\n<!-- IOTA_END -->\nfooter\n";
    let updated = replace_iota_block(existing, "<!-- IOTA_START -->\nnew\n<!-- IOTA_END -->\n");
    assert!(updated.contains("header"));
    assert!(updated.contains("footer"));
    assert!(updated.contains("new"));
    assert!(!updated.contains("old"));
}

#[test]
fn backend_skill_path_sanitizes_special_characters() {
    let workspace = Path::new("/tmp");
    let path = backend_skill_path(AcpBackend::ClaudeCode, workspace, "../../a:b?c*name")
        .unwrap()
        .unwrap();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("iota-"));
    assert!(path_str.contains("a-b-c-name"));
    assert!(!path_str.contains(":"));
    assert!(!path_str.contains("*"));
    assert!(!path_str.contains("?"));
    assert!(path_str.ends_with("SKILL.md"));
}

#[test]
fn backend_skill_path_truncates_long_skill_name() {
    let workspace = Path::new("/tmp");
    let long_name = "a".repeat(80);
    let path = backend_skill_path(AcpBackend::ClaudeCode, workspace, &long_name)
        .unwrap()
        .unwrap();
    let path_str = path.to_string_lossy();
    let marker = "iota-";
    let pos = path_str
        .find(marker)
        .expect("path should contain iota marker");
    let after = &path_str[pos + marker.len()..];
    let segment = after
        .split('/')
        .next()
        .expect("skill directory segment should exist");
    assert_eq!(segment.len(), 64);
}

#[test]
fn dry_run_backend_projection_includes_memory_and_skill_previews() {
    let workspace = std::env::temp_dir().join(format!("iota-native-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(workspace.join("skills")).unwrap();

    let skill_file = workspace.join("skills").join("review.md");
    std::fs::write(
        &skill_file,
        "---\nname: review\nsummary: Review code\n---\nUse this skill to review code.",
    )
    .unwrap();

    let memory = MemoryStore::open(Path::new(":memory:")).unwrap();
    memory
        .insert(MemoryInsert {
            memory_type: MemoryType::Semantic,
            facet: Some(MemoryFacet::Domain),
            scope: MemoryScope::Project,
            scope_id: "proj".to_string(),
            content: "ACP uses JSON-RPC over stdio".to_string(),
            confidence: 0.9,
            source_backend: None,
            source_session_id: None,
            source_execution_id: None,
            metadata_json: None,
            ttl_days: 365,
            supersedes: None,
        })
        .unwrap();

    let registry = SkillRegistry::load(&workspace, &[]);
    let previews = dry_run_backend_projection(
        AcpBackend::ClaudeCode,
        &workspace,
        Some(&memory),
        Some(&registry),
    )
    .unwrap();

    let has_memory_preview = previews
        .iter()
        .any(|preview| preview.path.ends_with("MEMORY.md"));
    let has_skill_preview = previews.iter().any(|preview| {
        preview
            .path
            .to_string_lossy()
            .contains("iota-review/SKILL.md")
    });

    assert!(has_memory_preview);
    assert!(has_skill_preview);
    assert!(previews.iter().any(|preview| {
        preview.path.ends_with("MEMORY.md")
            && preview.content.contains("ACP uses JSON-RPC over stdio")
            && preview.content.contains("## domain")
    }));

    let _ = std::fs::remove_dir_all(&workspace);
}
