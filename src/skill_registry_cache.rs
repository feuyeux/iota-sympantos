use anyhow::{Context, Result};
use std::path::PathBuf;

pub async fn pull_skill(source: &str, name: Option<&str>) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let cache = home.join(".i6").join("skills").join("registry-cache");
    std::fs::create_dir_all(&cache)
        .with_context(|| format!("Failed to create {}", cache.display()))?;
    let file_name = name
        .map(str::to_string)
        .or_else(|| source.rsplit('/').next().map(str::to_string))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "skill.md".to_string());
    let path = cache.join(file_name);
    let content = if source.starts_with("http://") || source.starts_with("https://") {
        reqwest::get(source)
            .await?
            .error_for_status()?
            .text()
            .await?
    } else {
        std::fs::read_to_string(source).with_context(|| format!("Failed to read {}", source))?
    };
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(path)
}
