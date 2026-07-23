//! SQLite connection initialization and standard configurations.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

/// Opens an SQLite database connection at the specified path and applies standard configuration pragmas:
/// - Write-Ahead Logging (WAL) mode for better concurrency.
/// - NORMAL synchronization for robust writes without full-flush performance cost.
/// - 5000ms busy timeout to prevent transient write locks.
/// - Foreign Key constraint enforcement.
///
/// The parent directory and the database file (plus its `-wal`/`-shm`
/// sidecar files once SQLite creates them) are locked to owner-only
/// permissions (`0700`/`0600` on Unix) — these databases can hold
/// conversation/memory content and must not be readable by other local
/// users (result.md S-03).
pub fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        crate::fs_secure::create_missing_dir_owner_only(parent)
            .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
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

    lock_down_db_files(path)?;

    Ok(conn)
}

/// Applies owner-only permissions to the main database and any WAL/SHM
/// sidecars already created by SQLite. Permission failures are fatal because
/// continuing would expose sensitive local data contrary to the store's
/// security contract.
fn lock_down_db_files(path: &Path) -> Result<()> {
    for candidate in db_sidecar_paths(path) {
        if candidate.exists() {
            crate::fs_secure::set_file_owner_only(&candidate).with_context(|| {
                format!(
                    "Failed to lock down SQLite file permissions: {}",
                    candidate.display()
                )
            })?;
        }
    }
    Ok(())
}

fn db_sidecar_paths(path: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    let file_name = path.file_name().and_then(|n| n.to_str());
    if let (Some(parent), Some(file_name)) = (path.parent(), file_name) {
        paths.push(parent.join(format!("{file_name}-wal")));
        paths.push(parent.join(format!("{file_name}-shm")));
    }
    paths
}
