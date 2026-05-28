//! Retry utilities for transient failure handling.
//!
//! Provides a simple exponential-backoff retry loop that wraps any fallible
//! operation, logging each attempt and the final error. Designed to be
//! generic over any `T: std::error::Error + Send + Sync + 'static`.

use anyhow::Result;
use std::time::Duration;

/// Retry a fallible operation up to `max_attempts` times with exponential back-off.
///
/// `base_delay` is the initial wait between retries; each subsequent attempt
/// waits `base_delay * 2^(attempt-1)`. The first attempt is made immediately,
/// so `max_attempts=1` means "try once and don't retry".
///
/// On exhaustion of attempts the last error is returned.
pub fn with_backoff<F, T>(mut f: F, max_attempts: u32, base_delay: Duration) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut attempt = 0;
    loop {
        match f() {
            Ok(val) => return Ok(val),
            Err(err) if attempt + 1 >= max_attempts => return Err(err),
            Err(err) => {
                let delay = base_delay * 2_u32.pow(attempt);
                tracing::warn!(
                    attempt = attempt + 1,
                    max_attempts,
                    delay_secs = delay.as_secs_f32(),
                    error = %err,
                    "storage operation failed, retrying with backoff"
                );
                std::thread::sleep(delay);
                attempt += 1;
            }
        }
    }
}
