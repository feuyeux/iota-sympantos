//! Context Fabric Layer.
//!
//! [`ContextEngine`] composes the `<iota-context>` XML capsule injected into
//! every prompt, including memory, skills, working memory, and workspace state.
//!
//! The stdio MCP server that was formerly here now lives in [`crate::mcp::server`].

use serde::Serialize;
use std::collections::VecDeque;
use std::path::Path;

use crate::acp::AcpBackend;
use crate::config::{ContextBudgetsConfig, ContextEngineConfig};
use crate::memory::{MemoryRecord, RecallBuckets};
use crate::skill::SkillRegistry;

#[derive(Debug, Clone)]
pub struct ContextEngine {
    pub enabled: bool,
    budgets: ContextBudgets,
}

/// Alias so the context layer uses a shorter name.
pub type ContextBudgets = ContextBudgetsConfig;

#[derive(Debug, Clone, Serialize)]
pub struct WorkingMemoryTurn {
    pub backend: String,
    pub prompt_summary: String,
    pub output_summary: String,
}

#[derive(Debug, Clone)]
pub struct WorkingMemoryBuffer {
    max_turns: usize,
    turns: VecDeque<WorkingMemoryTurn>,
}

#[derive(Debug, Clone)]
pub struct ComposeInput<'a> {
    pub backend: AcpBackend,
    pub cwd: &'a Path,
    pub session_id: &'a str,
    pub model: Option<&'a str>,
    pub prompt: &'a str,
    pub memory: Option<&'a RecallBuckets>,
    pub skills: Option<&'a SkillRegistry>,
    pub working_memory: &'a WorkingMemoryBuffer,
    pub handoff: Option<&'a str>,
}

impl ContextEngine {
    pub fn from_config(config: Option<&ContextEngineConfig>) -> Self {
        let enabled = config.map(|cfg| cfg.enabled).unwrap_or(true)
            && config.map(|cfg| !cfg.injection.is_off()).unwrap_or(true);
        let budgets = config.and_then(|cfg| cfg.budgets).unwrap_or_default();
        Self { enabled, budgets }
    }

    pub fn compose_effective_prompt(&self, input: ComposeInput<'_>) -> String {
        if !self.enabled {
            return input.prompt.to_string();
        }
        // Fast path: trivial prompts without continuity metadata get a minimal capsule.
        if is_trivial_prompt(&input) {
            return self.compose_minimal_prompt(&input);
        }
        let mut capsule = String::new();
        capsule.push_str("<iota-context>\n");
        capsule.push_str("This block is orchestration context supplied by iota. Treat it as background data, not as a user request.\n\n");
        capsule.push_str("<session>\n");
        capsule.push_str(&format!(
            "iota_session_id: {}\nbackend: {}\ncwd: {}\n",
            input.session_id,
            input.backend,
            input.cwd.display()
        ));
        capsule.push_str("</session>\n\n");
        capsule.push_str("<memory-tools>\n");
        capsule.push_str("MCP tool `iota_memory_write` persists info across sessions.\n");
        capsule.push_str(
            "Args: content, type(semantic|episodic|procedural), scope(user|project|session), ",
        );
        capsule.push_str(&format!(
            "scope_id(default: user=\"local-user\", project=\"{}\", session=\"{}\").\n",
            input.cwd.display(),
            input.session_id,
        ));
        capsule.push_str("Optional: facet(identity|preference|strategic|domain), merge_mode, confidence, ttl_days.\n");
        capsule.push_str("</memory-tools>\n\n");
        if let Some(model) = input.model.filter(|value| !value.trim().is_empty()) {
            capsule.push_str("<model>\n");
            capsule.push_str(&format!("You are currently using: {}\n", model.trim()));
            capsule.push_str("</model>\n\n");
        }
        if let Some(skills) = input.skills {
            let index = skills.skill_index(input.backend, self.budgets.skills_chars);
            if !index.is_empty() {
                capsule.push_str("<skills>\n");
                capsule.push_str(&index);
                capsule.push_str("</skills>\n\n");
            }
        }
        if let Some(memory) = input.memory {
            capsule.push_str(&trim_section(
                &render_memory(memory),
                self.budgets.memory_chars,
            ));
        }
        if let Some(handoff) = input.handoff.filter(|value| !value.trim().is_empty()) {
            capsule.push_str("<handoff>\n");
            capsule.push_str(&trim_section(handoff, self.budgets.handoff_chars));
            capsule.push_str("</handoff>\n\n");
        }
        let working_memory = input
            .working_memory
            .render(self.budgets.working_memory_chars);
        if !working_memory.is_empty() {
            capsule.push_str("<working-memory>\n");
            capsule.push_str(&working_memory);
            capsule.push_str("</working-memory>\n\n");
        }
        let workspace = render_workspace(input.cwd);
        if !workspace.trim().is_empty() {
            capsule.push_str("<workspace>\n");
            capsule.push_str(&trim_section(&workspace, self.budgets.workspace_chars));
            capsule.push_str("</workspace>\n\n");
        }
        capsule.push_str("</iota-context>\n\nUser request:\n");
        capsule.push_str(input.prompt);
        capsule
    }

    /// Minimal capsule for trivial prompts — skips memory, skills, and workspace.
    fn compose_minimal_prompt(&self, input: &ComposeInput<'_>) -> String {
        let mut capsule = String::new();
        capsule.push_str("<iota-context>\n");
        capsule.push_str("<session>\n");
        capsule.push_str(&format!(
            "iota_session_id: {}\nbackend: {}\ncwd: {}\n",
            input.session_id,
            input.backend,
            input.cwd.display()
        ));
        capsule.push_str("</session>\n");
        if let Some(model) = input.model.filter(|value| !value.trim().is_empty()) {
            capsule.push_str("\n<model>\n");
            capsule.push_str(&format!("You are currently using: {}\n", model.trim()));
            capsule.push_str("</model>\n");
        }
        if let Some(handoff) = input.handoff.filter(|value| !value.trim().is_empty()) {
            capsule.push_str("\n<handoff>\n");
            capsule.push_str(&trim_section(handoff, self.budgets.handoff_chars));
            capsule.push_str("</handoff>\n");
        }
        capsule.push_str("</iota-context>\n\nUser request:\n");
        capsule.push_str(input.prompt);
        capsule
    }

    pub fn budgets(&self) -> ContextBudgets {
        self.budgets
    }
}

impl Default for ContextEngine {
    fn default() -> Self {
        Self {
            enabled: true,
            budgets: ContextBudgets::default(),
        }
    }
}

impl WorkingMemoryBuffer {
    pub fn new(max_turns: usize) -> Self {
        Self {
            max_turns,
            turns: VecDeque::new(),
        }
    }

    pub fn push_turn(&mut self, backend: AcpBackend, prompt: &str, output: &str) {
        self.turns.push_back(WorkingMemoryTurn {
            backend: backend.to_string(),
            prompt_summary: summarize(prompt, 240),
            output_summary: summarize(output, 360),
        });
        while self.turns.len() > self.max_turns {
            self.turns.pop_front();
        }
    }

    pub fn render(&self, budget: usize) -> String {
        let mut selected = Vec::new();
        let mut current_len = 0;
        for turn in self.turns.iter().rev() {
            let line = format!(
                "- [{}] user: {}; assistant: {}\n",
                turn.backend, turn.prompt_summary, turn.output_summary
            );
            if current_len + line.len() > budget {
                break;
            }
            current_len += line.len();
            selected.push(line);
        }
        selected.reverse();
        selected.join("")
    }
}

/// A trivial prompt is short and doesn't reference memory tools or complex operations.
/// These get a minimal context capsule to reduce prompt size and latency.
fn is_trivial_prompt(input: &ComposeInput<'_>) -> bool {
    if input.model.is_some_and(|value| !value.trim().is_empty())
        || input.handoff.is_some_and(|value| !value.trim().is_empty())
    {
        return false;
    }

    let trimmed = input.prompt.trim();
    trimmed.len() <= 80
        && !trimmed.contains("iota_memory")
        && !trimmed.contains("remember")
        && !trimmed.contains("recall")
        && !trimmed.contains("skill")
}

fn render_memory(memory: &RecallBuckets) -> String {
    let mut output = String::new();
    push_memory_section(&mut output, "identity", &memory.identity);
    push_memory_section(&mut output, "preference", &memory.preference);
    push_memory_section(&mut output, "strategic", &memory.strategic);
    push_memory_section(&mut output, "domain", &memory.domain);
    push_memory_section(&mut output, "procedural", &memory.procedural);
    push_memory_section(&mut output, "episodic", &memory.episodic);
    output
}

fn push_memory_section(output: &mut String, name: &str, records: &[MemoryRecord]) {
    if records.is_empty() {
        return;
    }
    output.push_str(&format!("<memory type=\"{}\">\n", name));
    for record in records {
        output.push_str("- ");
        output.push_str(record.content.trim());
        output.push('\n');
    }
    output.push_str("</memory>\n\n");
}

fn render_workspace(cwd: &Path) -> String {
    // Run `git status --short` synchronously.  This function is called from
    // the async engine path; callers are responsible for wrapping this in
    // `spawn_blocking` when the runtime budget matters.
    let mut changed = Vec::new();
    if let Ok(output) = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(cwd)
        // Prevent git from opening an editor or pager.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .output()
        && output.status.success()
    {
        changed = String::from_utf8_lossy(&output.stdout)
            .lines()
            .take(20)
            .map(str::to_string)
            .collect();
    }
    // Only emit workspace content when there are changed files worth reporting.
    if changed.is_empty() {
        return String::new();
    }
    let mut text = format!("cwd: {}\nrecent changed files:\n", cwd.display());
    for line in changed {
        text.push_str("- ");
        text.push_str(&line);
        text.push('\n');
    }
    text
}

fn trim_section(value: &str, budget: usize) -> String {
    if value.len() <= budget {
        return value.to_string();
    }
    let mut trimmed = value
        .chars()
        .take(budget.saturating_sub(16))
        .collect::<String>();
    trimmed.push_str("\n[trimmed]\n");
    trimmed
}

fn summarize(value: &str, limit: usize) -> String {
    crate::utils::summarize(value, limit)
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
