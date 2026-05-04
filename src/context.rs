use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::acp::AcpBackend;
use crate::config::ContextEngineConfig;
use crate::memory::{MemoryRecord, RecallBuckets};
use crate::skills::SkillRegistry;

#[derive(Debug, Clone)]
pub struct ContextEngine {
    pub enabled: bool,
    budgets: ContextBudgets,
}

#[derive(Debug, Clone, Copy)]
pub struct ContextBudgets {
    pub memory_chars: usize,
    pub skills_chars: usize,
    pub dialogue_chars: usize,
    pub workspace_chars: usize,
    pub handoff_chars: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueTurn {
    pub backend: String,
    pub prompt_summary: String,
    pub output_summary: String,
}

#[derive(Debug, Clone)]
pub struct DialogueBuffer {
    max_turns: usize,
    turns: VecDeque<DialogueTurn>,
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
    pub dialogue: &'a DialogueBuffer,
    pub handoff: Option<&'a str>,
}

impl ContextEngine {
    pub fn from_config(config: Option<&ContextEngineConfig>) -> Self {
        let enabled = config.map(|cfg| cfg.enabled).unwrap_or(true)
            && config
                .map(|cfg| cfg.injection.as_str() != "off")
                .unwrap_or(true);
        let budgets = config
            .and_then(|cfg| cfg.budgets.as_ref())
            .map(|budgets| ContextBudgets {
                memory_chars: budgets.memory_chars,
                skills_chars: budgets.skills_chars,
                dialogue_chars: budgets.dialogue_chars,
                workspace_chars: budgets.workspace_chars,
                handoff_chars: 800,
            })
            .unwrap_or_default();
        Self { enabled, budgets }
    }

    pub fn compose_effective_prompt(&self, input: ComposeInput<'_>) -> String {
        if !self.enabled {
            return input.prompt.to_string();
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
        if let Some(model) = input.model.filter(|value| !value.trim().is_empty()) {
            capsule.push_str("<model>\n");
            capsule.push_str(&format!("You are currently using: {}\n", model.trim()));
            capsule.push_str("</model>\n\n");
        }
        if let Some(memory) = input.memory {
            capsule.push_str(&trim_section(
                &render_memory(memory),
                self.budgets.memory_chars,
            ));
        }
        let dialogue = input.dialogue.render(self.budgets.dialogue_chars);
        if !dialogue.is_empty() {
            capsule.push_str("<dialogue>\n");
            capsule.push_str(&dialogue);
            capsule.push_str("</dialogue>\n\n");
        }
        capsule.push_str("<workspace>\n");
        capsule.push_str(&trim_section(
            &render_workspace(input.cwd),
            self.budgets.workspace_chars,
        ));
        capsule.push_str("</workspace>\n\n");
        if let Some(skills) = input.skills {
            let index = skills.skill_index(input.backend, self.budgets.skills_chars);
            if !index.is_empty() {
                capsule.push_str("<skills>\n");
                capsule.push_str(&index);
                capsule.push_str("</skills>\n\n");
            }
        }
        if let Some(handoff) = input.handoff.filter(|value| !value.trim().is_empty()) {
            capsule.push_str("<handoff>\n");
            capsule.push_str(&trim_section(handoff, self.budgets.handoff_chars));
            capsule.push_str("</handoff>\n\n");
        }
        capsule.push_str("</iota-context>\n\nUser request:\n");
        capsule.push_str(input.prompt);
        capsule
    }
}

impl Default for ContextBudgets {
    fn default() -> Self {
        Self {
            memory_chars: 2000,
            skills_chars: 1200,
            dialogue_chars: 1500,
            workspace_chars: 800,
            handoff_chars: 800,
        }
    }
}

impl DialogueBuffer {
    pub fn new(max_turns: usize) -> Self {
        Self {
            max_turns,
            turns: VecDeque::new(),
        }
    }

    pub fn push_turn(&mut self, backend: AcpBackend, prompt: &str, output: &str) {
        self.turns.push_back(DialogueTurn {
            backend: backend.to_string(),
            prompt_summary: summarize(prompt, 240),
            output_summary: summarize(output, 360),
        });
        while self.turns.len() > self.max_turns {
            self.turns.pop_front();
        }
    }

    pub fn render(&self, budget: usize) -> String {
        let mut output = String::new();
        for turn in self.turns.iter().rev() {
            let line = format!(
                "- [{}] user: {}; assistant: {}\n",
                turn.backend, turn.prompt_summary, turn.output_summary
            );
            if output.len() + line.len() > budget {
                break;
            }
            output.push_str(&line);
        }
        output
    }
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
    let mut changed = Vec::new();
    if let Ok(output) = std::process::Command::new("git")
        .arg("status")
        .arg("--short")
        .current_dir(cwd)
        .output()
    {
        if output.status.success() {
            changed = String::from_utf8_lossy(&output.stdout)
                .lines()
                .take(20)
                .map(str::to_string)
                .collect();
        }
    }
    let mut text = format!("cwd: {}\n", cwd.display());
    if !changed.is_empty() {
        text.push_str("recent changed files:\n");
        for line in changed {
            text.push_str("- ");
            text.push_str(&line);
            text.push('\n');
        }
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
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= limit {
        compact
    } else {
        let mut text = compact
            .chars()
            .take(limit.saturating_sub(3))
            .collect::<String>();
        text.push_str("...");
        text
    }
}

#[allow(dead_code)]
fn _normalize_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_context_returns_prompt_unchanged() {
        let engine = ContextEngine {
            enabled: false,
            budgets: ContextBudgets::default(),
        };
        let dialogue = DialogueBuffer::new(2);
        let prompt = engine.compose_effective_prompt(ComposeInput {
            backend: AcpBackend::Codex,
            cwd: Path::new("."),
            session_id: "s",
            model: None,
            prompt: "ping",
            memory: None,
            skills: None,
            dialogue: &dialogue,
            handoff: None,
        });
        assert_eq!(prompt, "ping");
    }

    #[test]
    fn enabled_context_wraps_prompt() {
        let engine = ContextEngine {
            enabled: true,
            budgets: ContextBudgets::default(),
        };
        let dialogue = DialogueBuffer::new(2);
        let prompt = engine.compose_effective_prompt(ComposeInput {
            backend: AcpBackend::Codex,
            cwd: Path::new("."),
            session_id: "s",
            model: Some("m"),
            prompt: "ping",
            memory: None,
            skills: None,
            dialogue: &dialogue,
            handoff: None,
        });
        assert!(prompt.contains("<iota-context>"));
        assert!(prompt.ends_with("ping"));
    }
}
