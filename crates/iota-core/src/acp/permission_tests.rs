use super::*;
use tokio::sync::mpsc;

#[test]
fn wildcard_star_matches_everything() {
    assert!(tool_is_whitelisted("any_tool", &["*".to_string()]));
}

#[test]
fn exact_match() {
    assert!(tool_is_whitelisted(
        "iota_memory_write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn prefix_wildcard() {
    assert!(tool_is_whitelisted(
        "iota_skill_run",
        &["iota_skill_*".to_string()]
    ));
}

#[test]
fn suffix_wildcard() {
    assert!(tool_is_whitelisted(
        "mcp__iota_read",
        &["*_read".to_string()]
    ));
}

#[test]
fn no_match_returns_false() {
    assert!(!tool_is_whitelisted(
        "dangerous_tool",
        &["safe_tool".to_string()]
    ));
}

#[test]
fn empty_rule_never_matches() {
    assert!(!tool_is_whitelisted("any", &["".to_string()]));
}

#[test]
fn empty_whitelist_never_matches() {
    assert!(!tool_is_whitelisted("any", &[]));
}

#[test]
fn mcp_prefixed_tool_matches_tail() {
    assert!(tool_is_whitelisted(
        "mcp__iota-context__iota_memory_write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn dash_underscore_canonicalization() {
    assert!(tool_is_whitelisted(
        "iota-memory-write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn case_insensitive_matching() {
    assert!(tool_is_whitelisted(
        "Iota_Memory_Write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn canonical_tool_name_normalizes() {
    assert_eq!(canonical_tool_name("Foo-Bar Baz"), "foo_barbaz");
}

#[test]
fn wildcard_match_exact() {
    assert!(wildcard_match("abc", "abc"));
    assert!(!wildcard_match("abc", "xyz"));
}

#[test]
fn wildcard_match_star_alone() {
    assert!(wildcard_match("anything", "*"));
}

#[test]
fn wildcard_match_prefix() {
    assert!(wildcard_match("iota_skill_run", "iota_skill_*"));
    assert!(!wildcard_match("other_run", "iota_skill_*"));
}

#[test]
fn wildcard_match_suffix() {
    assert!(wildcard_match("mcp__tool_read", "*_read"));
    assert!(!wildcard_match("mcp__tool_write", "*_read"));
}

#[test]
fn tool_rule_match_with_double_underscore_prefix() {
    assert!(tool_rule_match(
        "mcp__context__iota_memory_write",
        "iota_memory_write"
    ));
}

#[test]
fn multiple_rules_any_match_wins() {
    let rules = vec!["safe_tool".to_string(), "iota_*".to_string()];
    assert!(tool_is_whitelisted("iota_read", &rules));
    assert!(tool_is_whitelisted("safe_tool", &rules));
    assert!(!tool_is_whitelisted("dangerous", &rules));
}

#[tokio::test]
async fn scoped_approval_channel_is_registered_and_removed() {
    let (tx, _rx) = mpsc::channel(1);
    install_scoped_approval_channel("turn-test".to_string(), tx).await;
    assert!(
        scoped_approval_lock()
            .read()
            .await
            .contains_key("turn-test")
    );

    remove_scoped_approval_channel("turn-test").await;
    assert!(
        !scoped_approval_lock()
            .read()
            .await
            .contains_key("turn-test")
    );
}
