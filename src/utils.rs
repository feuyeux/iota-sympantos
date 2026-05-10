//! Shared utility functions used across multiple modules.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

/// Returns the current Unix timestamp in seconds.
pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Summarizes a string to at most `limit` characters, collapsing whitespace.
/// Appends "..." if the value was truncated.
pub fn summarize(value: &str, limit: usize) -> String {
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

/// Lock a `std::sync::Mutex` and recover gracefully from a poisoned state.
///
/// If the previous lock-holder panicked, the mutex is considered poisoned.
/// Rather than propagating a secondary panic (which would kill the daemon),
/// we recover the inner value — the underlying data is still accessible and
/// often consistent enough to continue.  A warning is printed to stderr so
/// operators are aware of the prior panic.
///
/// Prefer [`lock_sqlite_conn`] for SQLite connections — it additionally runs
/// ROLLBACK to clear any dangling transaction.
#[allow(dead_code)]
pub fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|err: PoisonError<MutexGuard<'_, T>>| {
            eprintln!(
                "[iota] warning: mutex was poisoned by a previous panic; recovering inner value"
            );
            err.into_inner()
        })
}

/// Lock a SQLite connection mutex and recover from poison with ROLLBACK.
///
/// If the previous lock-holder panicked mid-transaction, we execute ROLLBACK
/// to clear any dangling transaction before returning the connection.
pub fn lock_sqlite_conn(conn: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    conn.lock()
        .unwrap_or_else(|err: PoisonError<MutexGuard<'_, Connection>>| {
            eprintln!(
                "[iota] warning: SQLite connection mutex was poisoned by a previous panic; rolling back"
            );
            let conn = err.into_inner();
            if let Err(e) = conn.execute_batch("ROLLBACK") {
                eprintln!("[iota] warning: ROLLBACK after poison recovery failed: {e}");
            }
            conn
        })
}

#[cfg(test)]
#[path = "utils_tests.rs"]
mod tests;
