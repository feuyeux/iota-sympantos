//! Shared utility functions used across multiple modules.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ts_is_positive() {
        assert!(now_ts() > 0);
    }

    #[test]
    fn summarize_short_string_unchanged() {
        assert_eq!(summarize("hello world", 20), "hello world");
    }

    #[test]
    fn summarize_truncates_and_appends_ellipsis() {
        let result = summarize("hello world foo bar", 10);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 10);
    }

    #[test]
    fn summarize_collapses_whitespace() {
        assert_eq!(summarize("hello   world", 20), "hello world");
    }
}
