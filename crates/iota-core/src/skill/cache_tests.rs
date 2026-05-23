use super::*;

#[test]
fn accepts_normal_names() {
    assert!(sanitize_file_name("my-skill.md").is_ok());
    assert!(sanitize_file_name("skill_v2.yaml").is_ok());
    assert!(sanitize_file_name("Skill123").is_ok());
}

#[test]
fn rejects_path_traversal() {
    assert!(sanitize_file_name("../../.bashrc").is_err());
    assert!(sanitize_file_name("..").is_err());
    assert!(sanitize_file_name(".").is_err());
}

#[test]
fn strips_directory_prefix() {
    // Path::file_name extracts only the final component.
    let name = sanitize_file_name("subdir/skill.md").unwrap();
    assert_eq!(name, "skill.md");
}

#[test]
fn replaces_unsafe_chars() {
    let name = sanitize_file_name("my skill (v2)!.md").unwrap();
    assert!(!name.contains(' '));
    assert!(!name.contains('('));
    assert!(!name.contains(')'));
    assert!(!name.contains('!'));
}

#[test]
fn rejects_empty_and_too_long() {
    assert!(sanitize_file_name("").is_err());
    let long = "a".repeat(129);
    assert!(sanitize_file_name(&long).is_err());
}
