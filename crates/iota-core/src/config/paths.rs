use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct StorePaths {
    root: PathBuf,
}

impl StorePaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn resolve() -> Result<Self> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(Self::new(home.join(".i6").join("context")))
    }

    pub fn events_db(&self) -> PathBuf {
        self.root.join("events.sqlite")
    }

    pub fn memory_db(&self) -> PathBuf {
        self.root.join("memory.sqlite")
    }

    pub fn store_db(&self) -> PathBuf {
        self.root.join("store.sqlite")
    }
}
