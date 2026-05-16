# utils — Shared Utilities

Common helper functions used across multiple modules.

## Functions

| Function | Purpose |
|----------|---------|
| `elapsed_ms(Instant)` | Wall-clock milliseconds since a start instant |
| `now_ts()` | Current Unix timestamp in seconds |
| `summarize(str, limit)` | Collapse whitespace and truncate with "..." |
| `lock_or_recover(Mutex)` | Lock mutex, recovering gracefully from poisoned state |
