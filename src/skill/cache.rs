use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

/// Sanitize a candidate file name so it is safe to use as a single path
/// component inside the skill cache directory.
///
/// Rules:
/// - Use only the final path component (strips any directory prefix).
/// - Keep only alphanumeric characters, hyphens, underscores and dots.
/// - Reject names that are empty, consist solely of dots (`..`, `.`), or
///   exceed 128 characters after sanitization.
fn sanitize_file_name(raw: &str) -> Result<String> {
    let raw_path = Path::new(raw);
    if raw_path.is_absolute()
        || raw_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("skill file name '{}' must not contain path traversal", raw);
    }
    // Take only the final segment — rejects embedded `/` and `\`.
    let base = raw_path
        .file_name()
        .and_then(|os| os.to_str())
        .unwrap_or(raw);

    // Filter to a safe character set.
    let sanitized: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.is_empty() {
        bail!("skill file name is empty after sanitization");
    }
    // Reject pure-dot names (`.`, `..`) that are directory references.
    if sanitized.chars().all(|c| c == '.') {
        bail!(
            "skill file name '{}' is a reserved path component",
            sanitized
        );
    }
    if sanitized.len() > 128 {
        bail!(
            "skill file name is too long ({} chars, max 128)",
            sanitized.len()
        );
    }
    Ok(sanitized)
}

pub async fn pull_skill(source: &str, name: Option<&str>) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let cache = home.join(".i6").join("skills").join("registry-cache");
    std::fs::create_dir_all(&cache)
        .with_context(|| format!("Failed to create {}", cache.display()))?;

    // Derive the raw candidate name from the explicit argument or URL tail.
    let raw_name = name
        .map(str::to_string)
        .or_else(|| source.rsplit('/').next().map(str::to_string))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "skill.md".to_string());

    let file_name = sanitize_file_name(&raw_name)
        .with_context(|| format!("Invalid skill file name derived from source '{}'", source))?;

    let path = cache.join(&file_name);

    // Verify the resolved path is still inside the cache directory (defence in depth).
    let resolved = path.canonicalize().unwrap_or_else(|_| path.clone()); // file may not exist yet — check parent instead
    let resolved_parent = resolved.parent().unwrap_or(&resolved);
    let cache_canonical = cache.canonicalize().unwrap_or_else(|_| cache.clone());
    if !resolved_parent.starts_with(&cache_canonical) {
        bail!(
            "skill file name '{}' would escape the cache directory",
            file_name
        );
    }

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

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
