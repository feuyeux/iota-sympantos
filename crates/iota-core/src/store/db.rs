//! SQLite connection initialization and standard configurations.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

/// Opens an SQLite database connection at the specified path and applies standard configuration pragmas:
/// - Write-Ahead Logging (WAL) mode for better concurrency.
/// - NORMAL synchronization for robust writes without full-flush performance cost.
/// - 5000ms busy timeout to prevent transient write locks.
/// - Foreign Key constraint enforcement.
pub fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }
    }

    let conn = Connection::open(path)
        .with_context(|| format!("Failed to open SQLite database: {}", path.display()))?;

    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA busy_timeout=5000; \
         PRAGMA foreign_keys=ON;",
    )
    .with_context(|| {
        format!(
            "Failed to configure SQLite database pragmas for: {}",
            path.display()
        )
    })?;

    Ok(conn)
}
