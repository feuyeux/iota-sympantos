use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

use super::NimiaConfig;

pub fn config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".i6").join("nimia.yaml"))
}

pub fn read_config() -> Result<NimiaConfig> {
    let path = config_path()?;
    if !path.exists() {
        bail!("Backend config not found: {}", path.display());
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_yaml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
}
