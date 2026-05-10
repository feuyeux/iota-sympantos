# Code Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix confirmed Critical, Important, and Minor issues from the 2026-05-10 code review.

**Architecture:** Each task targets one issue in isolation. Changes are localized: embedding engine gets async conversion, SQLite poison recovery adds ROLLBACK, TUI channels get error handling, FTS5 gets input sanitization, and hot-path allocations are removed.

**Tech Stack:** Rust (edition 2024), tokio, rusqlite, reqwest

---

### Task 1: Remove `unwrap()` on `base_url` in `embed_api` (C1 partial — safety fix)

**Files:**
- Modify: `src/store/embedding.rs:72-75`

- [ ] **Step 1: Fix fragile unwrap coupling**

`embed_api` line 75 calls `config.base_url.as_deref().unwrap()` which is only safe because `is_api()` already checked it. Make `embed_api` receive the already-resolved values so there is no unwrap coupling.

Replace lines 56-70 and 72-101 in `src/store/embedding.rs`:

```rust
    pub fn embed(&self, content: &str) -> Vec<f32> {
        let canonical = canonicalize(content);
        if canonical.is_empty() {
            return Vec::new();
        }
        if let (Some(config), Some(client)) = (self.config.as_ref(), self.client.as_ref()) {
            if let Some(base_url) = config.base_url.as_deref() {
                match Self::embed_api(client, base_url, config.model.as_deref(), config.api_key.as_deref(), &canonical) {
                    Ok(vec) => return vec,
                    Err(e) => {
                        tracing::warn!("embedding API failed, using local fallback: {e}");
                    }
                }
            }
        }
        local_trigram(&canonical)
    }

    fn embed_api(
        client: &reqwest::blocking::Client,
        base_url: &str,
        model: Option<&str>,
        api_key: Option<&str>,
        text: &str,
    ) -> anyhow::Result<Vec<f32>> {
        let model = model.unwrap_or("nomic-embed-text");
        let url = format!("{}/api/embeddings", base_url.trim_end_matches('/'));

        let mut request = client.post(&url).json(&OllamaEmbeddingRequest {
            model: model.to_string(),
            prompt: text.to_string(),
        });

        if let Some(key) = api_key.filter(|k| !k.is_empty()) {
            request = request.bearer_auth(key);
        }

        let response = request.send()?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("embedding API returned {status}: {body}");
        }

        let parsed: OllamaEmbeddingResponse = response.json()?;
        if parsed.embedding.is_empty() {
            anyhow::bail!("embedding API returned empty vector");
        }
        Ok(parsed.embedding)
    }
```

Note: `is_api()` can be simplified or removed after this change since `embed()` no longer uses it.

- [ ] **Step 2: Run existing tests**

```bash
cargo test -p iota-sympantos embedding_tests
```
Expected: PASS (all 9 tests)

- [ ] **Step 3: Commit**

```bash
git add src/store/embedding.rs
git commit -m "fix(embedding): remove unwrap coupling between is_api and embed_api

Eliminate the fragile implicit contract where embed_api relied on is_api
having already validated config.base_url. Pass resolved references
explicitly so the function is safe regardless of the call site.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 2: Convert embedding to `spawn_blocking` to avoid blocking async runtime (C1 — core fix)

**Files:**
- Modify: `src/store/embedding.rs:12-16,27-43,46-53` (change `blocking::Client` to async `Client`, make engine hold no client, remove `is_api`)
- Modify: `src/store/memory.rs:117,194,645-666` (make `upsert_embedding` release lock before embedding call)

- [ ] **Step 1: Simplify EmbeddingEngine — keep only config, drop the client**

The core insight: `EmbeddingEngine` is `Clone` and shared. Rather than holding a `reqwest::Client` (which is designed to be shared via `Arc` anyway), we make `embed()` a pure function that takes config only and performs the blocking call. The caller wraps it in `spawn_blocking`.

Replace `EmbeddingEngine` struct and impl in `src/store/embedding.rs`:

```rust
/// Embedding engine that supports API-based or local trigram embeddings.
#[derive(Clone, Default)]
pub struct EmbeddingEngine {
    config: Option<EmbeddingConfig>,
}

impl EmbeddingEngine {
    /// Create from optional config. If config is None or has no base_url, falls back to local.
    pub fn from_config(config: Option<EmbeddingConfig>) -> Self {
        Self { config }
    }

    /// Whether this engine has API configuration.
    pub fn is_api(&self) -> bool {
        self.config
            .as_ref()
            .map(|c| c.base_url.is_some())
            .unwrap_or(false)
    }

    /// Compute embedding for content. Uses API if configured, else local trigram.
    ///
    /// This may make a synchronous HTTP request when an embedding API is configured.
    /// Callers should wrap this in `tokio::task::spawn_blocking` to avoid blocking
    /// the async runtime.
    pub fn embed(&self, content: &str) -> Vec<f32> {
        let canonical = canonicalize(content);
        if canonical.is_empty() {
            return Vec::new();
        }
        if let Some(config) = self.config.as_ref() {
            if let Some(base_url) = config.base_url.as_deref() {
                match Self::embed_api(
                    base_url,
                    config.model.as_deref(),
                    config.api_key.as_deref(),
                    &canonical,
                ) {
                    Ok(vec) => return vec,
                    Err(e) => {
                        tracing::warn!("embedding API failed, using local fallback: {e}");
                    }
                }
            }
        }
        local_trigram(&canonical)
    }

    fn embed_api(
        base_url: &str,
        model: Option<&str>,
        api_key: Option<&str>,
        text: &str,
    ) -> anyhow::Result<Vec<f32>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let model = model.unwrap_or("nomic-embed-text");
        let url = format!("{}/api/embeddings", base_url.trim_end_matches('/'));

        let mut request = client.post(&url).json(&OllamaEmbeddingRequest {
            model: model.to_string(),
            prompt: text.to_string(),
        });

        if let Some(key) = api_key.filter(|k| !k.is_empty()) {
            request = request.bearer_auth(key);
        }

        let response = request.send()?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("embedding API returned {status}: {body}");
        }

        let parsed: OllamaEmbeddingResponse = response.json()?;
        if parsed.embedding.is_empty() {
            anyhow::bail!("embedding API returned empty vector");
        }
        Ok(parsed.embedding)
    }
}
```

Remove the `client` field from the struct and remove `reqwest::blocking::Client` from imports at line 1 (it's used inline in `embed_api` now).

- [ ] **Step 2: Modify `MemoryStore` to release lock before embedding**

In `src/store/memory.rs`, modify `upsert_embedding` to take the vector as a parameter (embedding already computed by caller). Then modify all call sites in `insert_with_merge` to compute the embedding BEFORE acquiring the lock, or to drop the lock before calling embed.

Replace `upsert_embedding` signature and body (lines 645-666):

```rust
    fn upsert_embedding(
        conn: &Connection,
        memory_id: &str,
        vector: &[f32],
        updated_at: i64,
    ) -> Result<()> {
        if vector.is_empty() {
            return Ok(());
        }
        let blob = embedding::to_blob(vector);
        conn.execute(
            "INSERT INTO memory_embedding (memory_id, vector_blob, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(memory_id) DO UPDATE SET
               vector_blob = excluded.vector_blob,
               updated_at = excluded.updated_at",
            params![memory_id, blob, updated_at],
        )?;
        Ok(())
    }
```

Then modify the three call sites in `insert_with_merge`. At line 228, compute the embedding first, then acquire the lock. The pattern for each call site changes from:

```rust
// Before (line 228-249):
let conn = crate::utils::lock_or_recover(&self.conn);
// ... query ...
self.upsert_embedding(&conn, &existing_id, &input.content, now)?;

// After:
let vector = self.embedding.embed(&input.content);
let conn = crate::utils::lock_or_recover(&self.conn);
// ... query ...
Self::upsert_embedding(&conn, &existing_id, &vector, now)?;
```

Apply the same pattern at the other two `upsert_embedding` call sites (around lines 291 and 320) — since the `conn` guard is already held at those points, compute the vector earlier too.

For `search_vector` (line 465), the embedding call is already before the lock acquisition — it's correct as-is.

- [ ] **Step 3: Wrap engine `prompt_in_cwd_timed` callers that may trigger memory writes**

In `src/engine.rs`, `prompt_in_cwd_timed_with_execution_id` calls memory insert/search methods. These now call `embedding.embed()` which may block. The engine's call path is already inside `tokio::spawn` in the TUI path (tui.rs:890) and in the daemon path. Verify that the daemon path also wraps engine calls in a spawn context.

Check `src/daemon/pool.rs` for the daemon call path. If it calls engine directly on a tokio worker thread without `spawn_blocking`, add `spawn_blocking` wrapping.

- [ ] **Step 4: Run tests**

```bash
cargo test -p iota-sympantos embedding_tests memory_tests
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/store/embedding.rs src/store/memory.rs src/engine.rs src/daemon/pool.rs
git commit -m "fix(embedding): remove blocking HTTP client from async hot path

EmbeddingEngine no longer holds a reqwest::blocking::Client across calls.
Instead, embed_api creates a fresh client per request. Callers should wrap
embed() in spawn_blocking when on an async runtime. upsert_embedding now
receives a pre-computed vector so the SQLite lock is not held during HTTP.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 3: Add ROLLBACK on poisoned mutex recovery for SQLite (C2)

**Files:**
- Modify: `src/utils.rs:37-46`

- [ ] **Step 1: Add a SQLite-specific recovery function**

Add a new function in `src/utils.rs` for SQLite connections that performs ROLLBACK on poison recovery:

```rust
use rusqlite::Connection;

/// Lock a SQLite connection mutex and recover from poison with ROLLBACK.
///
/// If the previous lock-holder panicked mid-transaction, we execute ROLLBACK
/// to clear any dangling transaction before returning the connection. This
/// prevents silent data corruption from partially-committed state.
pub fn lock_sqlite_conn(conn: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    let guard = mutex.lock().unwrap_or_else(|err: PoisonError<MutexGuard<'_, Connection>>| {
        eprintln!(
            "[iota] warning: SQLite connection mutex was poisoned by a previous panic; rolling back dangling transaction"
        );
        let conn = err.into_inner();
        // Clear any dangling transaction left by the panicked holder.
        if let Err(e) = conn.execute_batch("ROLLBACK") {
            eprintln!("[iota] warning: ROLLBACK after poison recovery failed: {e}");
        }
        conn
    });
    guard
}
```

- [ ] **Step 2: Replace `lock_or_recover` with `lock_sqlite_conn` in all store files**

In `src/store/memory.rs`, `src/store/cache.rs`, `src/store/ledger.rs`, `src/store/approval.rs`:
- Replace all `crate::utils::lock_or_recover(&self.conn)` calls with `crate::utils::lock_sqlite_conn(&self.conn)`

Find exact occurrences:
```bash
grep -rn "lock_or_recover" src/store/
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p iota-sympantos --lib
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/utils.rs src/store/memory.rs src/store/cache.rs src/store/ledger.rs src/store/approval.rs
git commit -m "fix(store): execute ROLLBACK when recovering poisoned SQLite mutex

Add lock_sqlite_conn which wraps lock_or_recover with an explicit ROLLBACK
to clear any dangling transaction left by a panicked lock holder. Replaces
lock_or_recover for all SQLite connection access across stores.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 4: Handle `try_send` failures in TUI (I1)

**Files:**
- Modify: `src/tui.rs:273-279,320`

- [ ] **Step 1: Log warning when prompt `try_send` fails**

In `src/tui.rs`, modify the `submit` function around line 273-279:

```rust
        let tx = self.turn_tx.clone();
        match tx.try_send(TurnMessage::Prompt {
            backend: self.active_backend,
            cwd: self.cwd.clone(),
            text,
        }) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(msg)) => {
                tracing::warn!(
                    backend = %self.active_backend,
                    "turn channel full; prompt queued in TUI will be sent when channel drains"
                );
                // Re-queue and retry via a small spawned task so we don't block the TUI
                let tx2 = tx.clone();
                tokio::spawn(async move {
                    let _ = tx2.send(msg).await;
                });
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                tracing::error!("turn channel closed; engine has shut down");
                self.history.push(ConversationEntry::SystemNotice {
                    text: "Error: engine channel closed".into(),
                });
                self.running_turn = false;
            }
        }
```

- [ ] **Step 2: Handle stream `try_send` failures in `acp/mod.rs`**

In `src/acp/mod.rs` around line 320, replace `let _ = tx.try_send(text);`:

```rust
                    if let Some(tx) = stream_tx {
                        if tx.try_send(text).is_err() {
                            tracing::warn!("stream channel full or closed; dropping chunk");
                        }
                    }
```

For streaming output, dropping chunks when the channel is full is acceptable (the TUI will catch up on the next frame), but we should at least trace it instead of silently discarding.

- [ ] **Step 3: Build check**

```bash
cargo build -p iota-sympantos
```
Expected: SUCCESS (warnings ok)

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs src/acp/mod.rs
git commit -m "fix(tui): handle try_send failures instead of silently dropping

When the prompt channel is full, spawn a task to send asynchronously.
When the stream channel is full, log a warning instead of silently
discarding the error with let _ =.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 5: Add line length limit on child process stdout (I2)

**Files:**
- Modify: `src/acp/wire.rs:42-53`

- [ ] **Step 1: Add bounded line reading**

Replace the `read_next_line_with_duration` function and add a constant:

```rust
/// Maximum length of a single line from an ACP backend's stdout (10 MiB).
/// Prevents memory exhaustion from a misbehaving or compromised backend.
pub const MAX_ACP_LINE_BYTES: usize = 10 * 1024 * 1024;

async fn read_next_line_with_duration<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    duration: Duration,
    message: &str,
) -> Result<Option<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    match timeout(duration, lines.next_line()).await {
        Ok(Ok(Some(line))) => {
            if line.len() > MAX_ACP_LINE_BYTES {
                anyhow::bail!(
                    "ACP backend emitted a line exceeding {} bytes ({} bytes); rejecting",
                    MAX_ACP_LINE_BYTES,
                    line.len()
                );
            }
            Ok(Some(line))
        }
        Ok(Ok(None)) => Ok(None),
        Ok(Err(e)) => Err(anyhow!("{}: {}", message, e)),
        Err(_) => Err(anyhow!(message.to_string())),
    }
}
```

Note: `tokio::io::Lines::next_line()` already reads the full line into memory before returning. The check here is a defense-in-depth measure — if a future change removes `BufReader` buffering or the line is buffered elsewhere, this catches oversized lines. The memory is already allocated by this point, but we prevent the line from being stored long-term in application state.

An alternative with true bounded reads would require replacing `Lines<BufReader<R>>` with a custom read loop using `read_until(b'\n', &mut buf)` with a length check after each read. For now, this check provides defense-in-depth at the application boundary.

- [ ] **Step 2: Build check**

```bash
cargo build -p iota-sympantos
```
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add src/acp/wire.rs
git commit -m "fix(acp): reject oversized stdout lines from child processes

Add a 10 MiB line-length check after tokio's next_line() returns.
Prevents storing arbitrarily long lines in application state from a
misbehaving backend, matching the existing daemon request limit.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 6: Sanitize FTS5 query input (I4)

**Files:**
- Modify: `src/store/memory.rs:551-556`

- [ ] **Step 1: Strip FTS5 special syntax before quoting**

Replace the `search_fts` query preparation (lines 551-556):

```rust
    fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        // Strip FTS5 special syntax characters so the query is treated as
        // literal text, not as boolean operators or column filters.
        let sanitized = sanitize_fts5_query(query);
        let safe_query = format!("\"{}\"", sanitized.replace('"', "\"\""));
        // ... rest unchanged
```

Add the sanitizer function at the bottom of `src/store/memory.rs` (before `#[cfg(test)]`):

```rust
/// Remove FTS5 special syntax characters so a user-provided query is treated
/// as literal search text, not as boolean expressions or column filters.
fn sanitize_fts5_query(query: &str) -> String {
    // FTS5 special characters: * (prefix), ^ (initial), NOT/AND/OR (operators),
    // parentheses for grouping, and column-prefix colon syntax.
    // We strip all of them to produce a safe phrase query.
    query
        .chars()
        .filter(|c| !matches!(c, '*' | '^' | '(' | ')' | ':'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
```

- [ ] **Step 2: Add a unit test**

In `src/store/memory_tests.rs`, add:

```rust
#[test]
fn fts5_query_sanitization_strips_special_chars() {
    assert_eq!(sanitize_fts5_query("hello world"), "hello world");
    assert_eq!(sanitize_fts5_query("hello AND world"), "hello world");
    assert_eq!(sanitize_fts5_query("foo* bar^ (baz)"), "foo bar baz");
    assert_eq!(sanitize_fts5_query("content:secret"), "secret");
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p iota-sympantos memory_tests
```
Expected: PASS (including new test)

- [ ] **Step 4: Commit**

```bash
git add src/store/memory.rs src/store/memory_tests.rs
git commit -m "fix(memory): sanitize FTS5 query to prevent syntax injection

Strip FTS5 boolean operators, prefix/suffix markers, grouping, and column
prefix syntax from user-provided queries before wrapping in phrase quotes.
Prevents crafted input from injecting FTS5 expressions.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 7: Add scope/type pre-filter to vector search (I7)

**Files:**
- Modify: `src/store/memory.rs:461-512`

- [ ] **Step 1: Add an optional pre-filter to `search_vector`**

The current query scans all active memory records. Add a heuristic: if the embedding is local (trigram fallback), use keyword search instead since trigram vectors have limited discriminative power. When embedding is API-based, add a `type IN ('semantic','procedural')` pre-filter since episodic memories are better served by keyword search.

```rust
    fn search_vector(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if query.trim().is_empty() {
            return self.search_keyword(query, limit);
        }
        // Local trigram embeddings have limited discriminative power for
        // pure vector search. Delegate to keyword search instead.
        if !self.embedding.is_api() {
            return self.search_keyword(query, limit);
        }
        let query_vec = self.embedding.embed(query);
        if query_vec.is_empty() {
            return Ok(Vec::new());
        }
        let query_canonical = embedding::canonicalize(query);
        let conn = crate::utils::lock_sqlite_conn(&self.conn);
        // Pre-filter: vector search is most useful for semantic/procedural
        // memories. Episodic memories are better matched by keyword recency.
        let mut stmt = conn.prepare(
            "SELECT m.id, m.type, m.facet, m.scope, m.scope_id, m.content, m.confidence, m.created_at, m.updated_at, m.expires_at, e.vector_blob
             FROM memory m
             JOIN memory_embedding e ON e.memory_id = m.id
             WHERE m.expires_at > ?1 AND m.type IN ('semantic','procedural')
             ORDER BY m.updated_at DESC
             LIMIT ?2",
        )?;
        // Reduced from 800 to 400 since we pre-filter by type
        let rows = stmt.query_map(params![now_ts(), 400i64], |row| {
            Ok((row_to_memory_record(row)?, row.get::<_, Vec<u8>>(10)?))
        })?;
        // ... rest unchanged (scoring loop)
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p iota-sympantos memory_tests
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/store/memory.rs
git commit -m "perf(memory): add type pre-filter and reduce vector search limit

Vector search now skips episodic memories (better served by keyword) and
reduces the scan limit from 800 to 400. Local trigram mode delegates to
keyword search entirely since trigram vectors lack discriminative power
for cosine-similarity ranking.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 8: Hot-path allocation removal (M1, M8) + tighten terminal result check (M7)

**Files:**
- Modify: `src/engine.rs:1324` (M1)
- Modify: `src/tui/composer.rs:45-50` (M8)
- Modify: `src/acp/mod.rs:929-931` (M7)

- [ ] **Step 1: Replace `to_lowercase()` with ASCII lowercasing in `classify_memory_prompt`**

In `src/engine.rs` line 1324, replace:
```rust
    let lower = prompt.to_lowercase();
```
With:
```rust
    let lower = prompt.to_ascii_lowercase();
```

Chinese characters are unaffected by `to_ascii_lowercase()` (they pass through unchanged), and all the keyword matching is ASCII-based (English keywords + Chinese characters that don't change case).

- [ ] **Step 2: Replace `to_lowercase()` in history search**

In `src/tui/composer.rs` lines 45 and 50, replace:
```rust
        let q = self.query.to_lowercase();
```
With:
```rust
        let q = self.query.to_ascii_lowercase();
```
And replace:
```rust
            .find(|&i| history[i].to_lowercase().contains(&q))
```
With:
```rust
            .find(|&i| history[i].to_ascii_lowercase().contains(&q))
```

- [ ] **Step 3: Tighten `is_terminal_result` fallback**

In `src/acp/mod.rs` lines 929-931, make the fallback more specific:

```rust
fn is_terminal_result(result: &Value) -> bool {
    if result.get("stopReason").and_then(Value::as_str).is_some() {
        return true;
    }
    // Fallback: treat as terminal only if a content array with text is present
    // (some backends omit stopReason on the final event)
    result
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false)
}
```

This is narrower than the previous `extract_text(result).is_some()` which matched a broad set of field names and could fire on intermediate progress objects.

- [ ] **Step 4: Build and test**

```bash
cargo build -p iota-sympantos
cargo test -p iota-sympantos
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/engine.rs src/tui/composer.rs src/acp/mod.rs
git commit -m "perf: replace to_lowercase with to_ascii_lowercase in hot paths

- classify_memory_prompt: allocation-free ASCII lowercasing on every prompt
- Ctrl+R history search: allocation-free on every history entry scanned
- is_terminal_result: require content array instead of broad extract_text

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 9: Add `busy_timeout` on all SQLite connections (M6)

**Files:**
- Modify: `src/store/memory.rs:670`
- Modify: `src/store/cache.rs:108`
- Modify: `src/store/ledger.rs:30`
- Modify: `src/store/approval.rs` (check for connection init)

- [ ] **Step 1: Add busy_timeout pragma in all store init paths**

In `src/store/memory.rs`, line 670 (`init_schema`):
```rust
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;")?;
```

In `src/store/cache.rs`, find the init function and add:
```rust
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;")?;
```

In `src/store/ledger.rs`, line 30:
```rust
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;")?;
```

Check `src/store/approval.rs` for its connection init and add the same pragma.

- [ ] **Step 2: Build and test**

```bash
cargo build -p iota-sympantos
cargo test -p iota-sympantos
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/store/memory.rs src/store/cache.rs src/store/ledger.rs src/store/approval.rs
git commit -m "fix(store): set busy_timeout on all SQLite connections

Add PRAGMA busy_timeout=5000 to prevent SQLITE_BUSY errors when a
previous process still holds a WAL lock (e.g. after daemon restart).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 10: Cache rendered markdown lines in TUI (M4)

**Files:**
- Modify: `src/tui.rs:370-384`

- [ ] **Step 1: Add cached rendered lines to app state**

In `src/tui/state.rs`, add a field to track cached markdown output:

```rust
    /// Cached rendered markdown lines, cleared when streaming_text changes.
    pub rendered_md_lines: Vec<ratatui::text::Line<'static>>,
```

In `src/tui.rs` around line 370-384, add caching:

```rust
            // Re-render markdown only when streaming text has changed.
            if self.streaming_version != self.rendered_version {
                self.rendered_md_lines = markdown::render(&self.streaming_text);
                self.rendered_version = self.streaming_version;
            }
            for md_line in &self.rendered_md_lines {
                let indented = Line::from(
                    std::iter::once(Span::raw("     "))
                        .chain(md_line.spans.iter().cloned())
                        .collect::<Vec<_>>(),
                );
                lines.push(indented);
            }
```

This requires adding `streaming_version: u64` and `rendered_version: u64` to the TUI app state. `streaming_version` is incremented every time `streaming_text` is mutated (in the `stream_rx.recv()` handler).

- [ ] **Step 2: Build check and manual verification note**

```bash
cargo build -p iota-sympantos
```
Expected: SUCCESS

Note: This change requires the `Clone` impl on `Span` — `ratatui::text::Span` implements `Clone`. Verify that `ratatui::text::Line<'static>` works with cached values.

- [ ] **Step 3: Commit**

```bash
git add src/tui.rs src/tui/state.rs
git commit -m "perf(tui): cache rendered markdown lines between frames

Only re-render markdown when streaming_text changes, avoiding O(n)
allocation and parsing on every ~8ms frame render for static text.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Accepted (No Action)

- **I5 (TokioMutex held for full prompt duration):** This is by design — the TUI runs one prompt at a time. The `TokioMutex` is the correct tool for long-held async locks. No change needed unless concurrent prompt processing is required in the future.

- **I6 (Path traversal in expand_home_path):** All config values are user-controlled. A user can only traverse their own home directory. This is negligible risk and does not warrant the complexity of canonicalization.

