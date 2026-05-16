//! Native materializer — projects memory and skills into backend-native files
//! (e.g. `MEMORY.md`, `AGENTS.md`) so backends that cannot use MCP still see
//! iota context.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

use crate::acp::AcpBackend;
use crate::memory::{MemoryRecord, MemoryStore};
use crate::skill::{Skill, SkillRegistry};

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
    let path = match backend {
        AcpBackend::ClaudeCode => workspace.join("MEMORY.md"),
        AcpBackend::Gemini => home_dir()?.join(".gemini").join("GEMINI.md"),
        AcpBackend::OpenCode | AcpBackend::Codex => workspace.join("AGENTS.md"),
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
    let safe_name = sanitize_skill_name(skill_name);
    let path = match backend {
        AcpBackend::ClaudeCode => workspace
            .join(".claude")
            .join("skills")
            .join(format!("iota-{}", safe_name))
            .join("SKILL.md"),
        AcpBackend::Gemini | AcpBackend::OpenCode | AcpBackend::Codex => {
            let Some(dir) = backend_skill_home_dir(backend, &home_dir()?) else {
                return Err(anyhow!(
                    "backend {:?} does not map to a home skill directory",
                    backend
                ));
            };
            dir.join(format!("iota-{}.md", safe_name))
        }
        AcpBackend::Hermes => return Ok(None),
    };
    Ok(Some(path))
}

fn sanitize_skill_name(skill_name: &str) -> String {
    // Keep only alphanumeric, hyphen, underscore to prevent traversal and
    // unsupported path characters across platforms.
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
    // Truncate to avoid overly long path components.
    safe_name[..safe_name.len().min(64)].to_string()
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("Failed to get home directory")
}

fn backend_skill_home_dir(backend: AcpBackend, home: &Path) -> Option<PathBuf> {
    match backend {
        AcpBackend::Gemini => Some(home.join(".gemini").join("skills")),
        AcpBackend::OpenCode => Some(home.join(".opencode").join("skills")),
        AcpBackend::Codex => Some(home.join(".codex").join("skills")),
        _ => None,
    }
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
    let description = skill
        .metadata
        .description
        .as_deref()
        .or(skill.metadata.summary.as_deref())
        .unwrap_or("");

    match backend {
        AcpBackend::ClaudeCode => format!(
            "---\nname: {}\ndescription: {}\nallowed-tools: []\n---\n\n{}\n",
            skill.metadata.name, description, skill.body
        ),
        AcpBackend::Hermes => format!(
            "---\nname: {}\nconditions: []\n---\n\n{}\n",
            skill.metadata.name, skill.body
        ),
        _ => format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}\n",
            skill.metadata.name, description, skill.body
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
    collect_memory_preview(&mut previews, backend, workspace, memory)?;
    collect_skill_previews(&mut previews, backend, workspace, skills)?;
    Ok(previews)
}

fn collect_memory_preview(
    previews: &mut Vec<MaterializePreview>,
    backend: AcpBackend,
    workspace: &Path,
    memory: Option<&MemoryStore>,
) -> Result<()> {
    if let (Some(memory), Some(path)) = (memory, backend_memory_path(backend, workspace)?) {
        let records = memory.search("", 100).unwrap_or_default();
        previews.push(dry_run(&path, &render_memory_records(&records))?);
    }
    Ok(())
}

fn collect_skill_previews(
    previews: &mut Vec<MaterializePreview>,
    backend: AcpBackend,
    workspace: &Path,
    skills: Option<&SkillRegistry>,
) -> Result<()> {
    if let Some(skills) = skills {
        for skill in skills.compatible_skills(backend) {
            if let Some(path) = backend_skill_path(backend, workspace, &skill.metadata.name)? {
                previews.push(dry_run(&path, &render_backend_skill(backend, skill))?);
            }
        }
    }
    Ok(())
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
    if let Some(start) = existing.find(START)
        && let Some(end_rel) = existing[start..].find(END)
    {
        let end_index = start + end_rel + END.len();
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
#[path = "native_tests.rs"]
mod tests;
