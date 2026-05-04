use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::acp::AcpBackend;
use crate::memory::{MemoryRecord, MemoryStore};
use crate::skills::{Skill, SkillRegistry};

const START: &str = "<!-- IOTA_START -->";
const END: &str = "<!-- IOTA_END -->";

#[derive(Debug, Clone)]
pub struct MaterializePreview {
    pub path: PathBuf,
    pub changed: bool,
    pub content: String,
}

pub fn dry_run(path: &Path, body: &str) -> Result<MaterializePreview> {
    let existing = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?
    } else {
        String::new()
    };
    let block = format!("{}\n{}\n{}\n", START, body.trim(), END);
    let content = replace_iota_block(&existing, &block);
    Ok(MaterializePreview {
        path: path.to_path_buf(),
        changed: content != existing,
        content,
    })
}

pub fn backend_memory_path(backend: AcpBackend, workspace: &Path) -> Result<Option<PathBuf>> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let path = match backend {
        AcpBackend::ClaudeCode => workspace.join("MEMORY.md"),
        AcpBackend::Gemini => home.join(".gemini").join("GEMINI.md"),
        AcpBackend::OpenCode => workspace.join("AGENTS.md"),
        AcpBackend::Codex => workspace.join("AGENTS.md"),
        AcpBackend::Hermes => return Ok(None),
    };
    Ok(Some(path))
}

#[allow(dead_code)]
pub fn backend_skill_path(
    backend: AcpBackend,
    workspace: &Path,
    skill_name: &str,
) -> Result<Option<PathBuf>> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    // Sanitize the skill name so it is safe to use as a filesystem path segment:
    // keep only alphanumeric characters, hyphens, and underscores; replace
    // everything else (including path separators, `.`, spaces, Windows reserved
    // characters such as `:` `*` `?` `"` `<` `>` `|`) with a hyphen.
    // This also prevents path traversal via `..` components.
    let safe_name: String = skill_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Truncate to 64 characters to avoid overly long path components.
    let safe_name = &safe_name[..safe_name.len().min(64)];
    let path = match backend {
        AcpBackend::ClaudeCode => workspace
            .join(".claude")
            .join("skills")
            .join(format!("iota-{}", safe_name))
            .join("SKILL.md"),
        AcpBackend::Gemini => home
            .join(".gemini")
            .join("skills")
            .join(format!("iota-{}.md", safe_name)),
        AcpBackend::OpenCode => home
            .join(".opencode")
            .join("skills")
            .join(format!("iota-{}.md", safe_name)),
        AcpBackend::Codex => home
            .join(".codex")
            .join("skills")
            .join(format!("iota-{}.md", safe_name)),
        AcpBackend::Hermes => return Ok(None),
    };
    Ok(Some(path))
}

#[allow(dead_code)]
pub fn dry_run_backend_memory(
    backend: AcpBackend,
    workspace: &Path,
    body: &str,
) -> Result<Option<MaterializePreview>> {
    let Some(path) = backend_memory_path(backend, workspace)? else {
        return Ok(None);
    };
    dry_run(&path, body).map(Some)
}

#[allow(dead_code)]
pub fn render_backend_skill(backend: AcpBackend, skill: &Skill) -> String {
    match backend {
        AcpBackend::ClaudeCode => format!(
            "---\nname: {}\ndescription: {}\nallowed-tools: []\n---\n\n{}\n",
            skill.metadata.name,
            skill
                .metadata
                .description
                .as_deref()
                .or(skill.metadata.summary.as_deref())
                .unwrap_or(""),
            skill.body
        ),
        AcpBackend::Hermes => format!(
            "---\nname: {}\nconditions: []\n---\n\n{}\n",
            skill.metadata.name, skill.body
        ),
        _ => format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}\n",
            skill.metadata.name,
            skill
                .metadata
                .description
                .as_deref()
                .or(skill.metadata.summary.as_deref())
                .unwrap_or(""),
            skill.body
        ),
    }
}

#[allow(dead_code)]
pub fn dry_run_backend_skill(
    backend: AcpBackend,
    workspace: &Path,
    skill: &Skill,
) -> Result<Option<MaterializePreview>> {
    let Some(path) = backend_skill_path(backend, workspace, &skill.metadata.name)? else {
        return Ok(None);
    };
    dry_run(&path, &render_backend_skill(backend, skill)).map(Some)
}

pub fn render_memory_records(records: &[MemoryRecord]) -> String {
    let mut body = String::from("# iota memory projection\n\n");
    for record in records {
        body.push_str("- ");
        body.push_str(record.content.trim());
        body.push('\n');
    }
    body
}

pub fn dry_run_backend_projection(
    backend: AcpBackend,
    workspace: &Path,
    memory: Option<&MemoryStore>,
    skills: Option<&SkillRegistry>,
) -> Result<Vec<MaterializePreview>> {
    let mut previews = Vec::new();
    if let (Some(memory), Some(path)) = (memory, backend_memory_path(backend, workspace)?) {
        let records = memory.search("", 100).unwrap_or_default();
        previews.push(dry_run(&path, &render_memory_records(&records))?);
    }
    if let Some(skills) = skills {
        for skill in skills.compatible_skills(backend) {
            if let Some(path) = backend_skill_path(backend, workspace, &skill.metadata.name)? {
                previews.push(dry_run(&path, &render_backend_skill(backend, skill))?);
            }
        }
    }
    Ok(previews)
}

pub fn apply(path: &Path, body: &str) -> Result<bool> {
    let preview = dry_run(path, body)?;
    if preview.changed {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(path, preview.content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(preview.changed)
}

fn replace_iota_block(existing: &str, block: &str) -> String {
    if let (Some(start), Some(end)) = (existing.find(START), existing.find(END)) {
        let end_index = end + END.len();
        let mut output = String::new();
        output.push_str(&existing[..start]);
        output.push_str(block);
        output.push_str(existing[end_index..].trim_start_matches('\n'));
        return output;
    }
    if existing.trim().is_empty() {
        block.to_string()
    } else {
        format!("{}\n{}", existing.trim_end(), block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_claude_skill_with_allowed_tools() {
        let skill = crate::skills::Skill {
            metadata: crate::skills::SkillMetadata {
                name: "review".to_string(),
                version: None,
                summary: Some("Review code".to_string()),
                description: None,
                triggers: Vec::new(),
                backends: Vec::new(),
                execution: crate::skills::SkillExecution::default(),
                output: crate::skills::SkillOutput::default(),
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
}
