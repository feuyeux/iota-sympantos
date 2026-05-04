use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::acp::AcpBackend;

#[derive(Debug, Clone, Serialize)]
pub struct SkillRegistry {
    roots: Vec<PathBuf>,
    skills: BTreeMap<String, Skill>,
    diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    pub metadata: SkillMetadata,
    pub body: String,
    pub path: PathBuf,
    pub priority: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    #[serde(default)]
    pub version: Option<serde_yaml::Value>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub backends: Vec<String>,
    #[serde(default)]
    pub execution: SkillExecution,
    #[serde(default)]
    pub output: SkillOutput,
    #[serde(default, rename = "failurePolicy")]
    pub failure_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl SkillTool {
    pub fn label(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExecution {
    #[serde(default = "default_execution_mode")]
    pub mode: String,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default, deserialize_with = "deserialize_skill_tools")]
    pub tools: Vec<SkillTool>,
}

impl Default for SkillExecution {
    fn default() -> Self {
        Self {
            mode: default_execution_mode(),
            server: None,
            parallel: false,
            tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillOutput {
    #[serde(default)]
    pub template: String,
}

impl SkillRegistry {
    pub fn load(workspace: &Path, configured_roots: &[PathBuf]) -> Self {
        let mut roots = Vec::new();
        roots.push(workspace.join(".iota").join("skills"));
        roots.extend(configured_roots.iter().cloned());
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".i6").join("skills"));
        }
        let signature = roots_signature(&roots);
        let cache = skill_cache();
        if let Ok(cache) = cache.lock() {
            if let Some((cached_signature, registry)) = cache.as_ref() {
                if *cached_signature == signature {
                    return registry.clone();
                }
            }
        }

        let mut registry = Self {
            roots,
            skills: BTreeMap::new(),
            diagnostics: Vec::new(),
        };
        registry.reload();
        if let Ok(mut cache) = cache.lock() {
            *cache = Some((signature, registry.clone()));
        }
        registry
    }

    pub fn reload(&mut self) {
        self.skills.clear();
        self.diagnostics.clear();
        let roots = self.roots.clone();
        for (priority, root) in roots.iter().enumerate().rev() {
            if !root.exists() {
                continue;
            }
            if let Err(err) = self.load_root(root, priority) {
                self.diagnostics
                    .push(format!("{}: {}", root.display(), err));
            }
        }
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn compatible_skills(&self, backend: AcpBackend) -> Vec<&Skill> {
        self.skills
            .values()
            .filter(|skill| skill.supports_backend(backend))
            .collect()
    }

    pub fn skill_index(&self, backend: AcpBackend, budget: usize) -> String {
        let mut output = String::new();
        for skill in self.compatible_skills(backend) {
            let summary = skill
                .metadata
                .summary
                .as_deref()
                .or(skill.metadata.description.as_deref())
                .unwrap_or("");
            let line = format!("- {}: {}\n", skill.metadata.name, summary.trim());
            if output.len() + line.len() > budget {
                break;
            }
            output.push_str(&line);
        }
        output
    }

    pub fn match_skill(&self, backend: AcpBackend, prompt: &str) -> Option<&Skill> {
        let normalized = prompt.to_lowercase();
        self.compatible_skills(backend).into_iter().find(|skill| {
            skill
                .metadata
                .triggers
                .iter()
                .any(|trigger| normalized.contains(&trigger.to_lowercase()))
        })
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    fn load_root(&mut self, root: &Path, priority: usize) -> Result<()> {
        let mut files = collect_skill_files(root)?;
        files.sort();
        let mut seen_in_root = BTreeSet::new();
        for path in files {
            match parse_skill_file(&path, priority) {
                Ok(skill) => {
                    if !seen_in_root.insert(skill.metadata.name.clone()) {
                        self.diagnostics.push(format!(
                            "duplicate skill '{}' in {}; kept first sorted item",
                            skill.metadata.name,
                            root.display()
                        ));
                        continue;
                    }
                    self.skills.insert(skill.metadata.name.clone(), skill);
                }
                Err(err) => self
                    .diagnostics
                    .push(format!("{}: {}", path.display(), err)),
            }
        }
        Ok(())
    }
}

impl Skill {
    pub fn supports_backend(&self, backend: AcpBackend) -> bool {
        self.metadata.backends.is_empty()
            || self
                .metadata
                .backends
                .iter()
                .any(|name| AcpBackend::parse(name).is_ok_and(|value| value == backend))
    }
}

pub fn render_template(skill: &Skill, prompt: &str) -> String {
    let template = skill.metadata.output.template.trim();
    if template.is_empty() {
        return skill.body.clone();
    }
    template
        .replace("{{prompt}}", prompt)
        .replace("{{skill.name}}", &skill.metadata.name)
}

fn collect_skill_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(root).with_context(|| format!("Failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_skill_files(&path)?);
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| matches!(ext, "md" | "yaml" | "yml"))
        {
            files.push(path);
        }
    }
    Ok(files)
}

fn parse_skill_file(path: &Path, priority: usize) -> Result<Skill> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read skill {}", path.display()))?;
    let (frontmatter, body) = split_frontmatter(&content)
        .with_context(|| format!("Skill {} is missing YAML frontmatter", path.display()))?;
    let metadata: SkillMetadata = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("Invalid skill YAML in {}", path.display()))?;
    Ok(Skill {
        metadata,
        body: body.trim().to_string(),
        path: path.to_path_buf(),
        priority,
    })
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let (frontmatter, after) = rest.split_at(end);
    let body = after.strip_prefix("\n---").unwrap_or(after);
    let body = body.strip_prefix('\n').unwrap_or(body);
    Some((frontmatter, body))
}

fn default_execution_mode() -> String {
    "advisory".to_string()
}

fn deserialize_skill_tools<'de, D>(deserializer: D) -> Result<Vec<SkillTool>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<serde_yaml::Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|value| match value {
            serde_yaml::Value::String(name) => Ok(SkillTool { name, alias: None }),
            serde_yaml::Value::Mapping(mapping) => {
                let name = mapping
                    .get(serde_yaml::Value::String("name".to_string()))
                    .and_then(serde_yaml::Value::as_str)
                    .ok_or_else(|| serde::de::Error::custom("skill tool object requires name"))?
                    .to_string();
                let alias = mapping
                    .get(serde_yaml::Value::String("as".to_string()))
                    .and_then(serde_yaml::Value::as_str)
                    .map(str::to_string);
                Ok(SkillTool { name, alias })
            }
            _ => Err(serde::de::Error::custom(
                "skill tools must be strings or {name, as} objects",
            )),
        })
        .collect()
}

type SkillCache = Option<(String, SkillRegistry)>;

fn skill_cache() -> &'static Mutex<SkillCache> {
    static CACHE: OnceLock<Mutex<SkillCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn roots_signature(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| format!("{}:{}", root.display(), latest_mtime(root).unwrap_or(0)))
        .collect::<Vec<_>>()
        .join("|")
}

fn latest_mtime(path: &Path) -> Option<u128> {
    if !path.exists() {
        return Some(0);
    }
    let mut latest = path
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_ms)
        .unwrap_or(0);
    if path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                latest = latest.max(latest_mtime(&entry.path()).unwrap_or(0));
            }
        }
    }
    Some(latest)
}

fn system_time_ms(time: SystemTime) -> Option<u128> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}
