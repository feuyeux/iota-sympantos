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

/// The default `SqliteKanbanStore` database path (`~/.i6/kanban/iota.db`).
///
/// Kept alongside `StorePaths` so every `~/.i6`-rooted path this crate
/// resolves goes through one module instead of each caller re-deriving
/// `dirs::home_dir().join(".i6")...` independently. `iota-kanban` is a
/// separate, dependency-free published crate and cannot use this helper
/// directly; it resolves the same path with its own local
/// `crate::paths::default_shadows_dir` for that reason.
pub fn kanban_db_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".i6").join("kanban").join("iota.db"))
}

/// The default Kanban shadow-workspace directory (`~/.i6/kanban/shadows`).
pub fn kanban_shadows_dir() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".i6").join("kanban").join("shadows"))
}
