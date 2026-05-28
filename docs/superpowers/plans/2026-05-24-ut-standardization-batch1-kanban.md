# UT Standardization: Inline Test Extraction

> Archive note: this is a historical implementation plan. For current behavior and commands, see [../../iota book.md](../../iota%20book.md), [../../architecture.md](../../architecture.md), and [../../command.md](../../command.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract inline test modules from source files into standalone `*_tests.rs` files, following the established `#[path = "..."] mod tests;` pattern.

**Architecture:** Each test module (`mod tests {}`) is extracted to a corresponding `*_tests.rs` file in the same directory. The source file retains only the `#[cfg(test)] #[path = "..."] mod tests;` declaration.

**Tech Stack:** Rust `#[cfg(test)]`, `#[tokio::test]`, `#[test]`

---

## Batch 1: `crates/iota-core/src/kanban/*`

### File Map

| Source File | Test File | Inline Test Lines |
|------------|-----------|-------------------|
| `kanban/event_sourcing.rs` | `kanban/event_sourcing_tests.rs` | 125-215 |
| `kanban/state_machine.rs` | `kanban/state_machine_tests.rs` | 28-57 |
| `kanban/bridge.rs` | `kanban/bridge_tests.rs` | 36-50, 259-275 |
| `kanban/worker.rs` | `kanban/worker_tests.rs` | 169-235 |
| `kanban/event_sync.rs` | `kanban/event_sync_tests.rs` | 9-11, 263-295 |
| `kanban/dispatcher.rs` | `kanban/dispatcher_tests.rs` | 4-96 (inner), 449-500 |
| `kanban/shadow.rs` | `kanban/shadow_tests.rs` | 453-500 |
| `kanban/mod.rs` | `kanban/mod_tests.rs` | (uses path already) |

---

### Task 1: Extract `kanban/event_sourcing.rs` tests

**Files:**
- Modify: `crates/iota-core/src/kanban/event_sourcing.rs:125-215`
- Create: `crates/iota-core/src/kanban/event_sourcing_tests.rs`

- [ ] **Step 1: Read source file to find test module boundaries**

```bash
sed -n '125,215p' crates/iota-core/src/kanban/event_sourcing.rs
```

- [ ] **Step 2: Create `event_sourcing_tests.rs`**

Extract the `#[cfg(test)] mod tests { ... }` block into a new file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ... copy all test functions here
}
```

- [ ] **Step 3: Replace inline tests with path reference**

In `event_sourcing.rs`, remove the inline test block and add:

```rust
#[cfg(test)]
#[path = "event_sourcing_tests.rs"]
mod tests;
```

- [ ] **Step 4: Verify compilation**

```bash
cargo test -p iota-core --lib kanban::event_sourcing::tests 2>&1 | head -30
```

- [ ] **Step 5: Commit**

```bash
git add crates/iota-core/src/kanban/event_sourcing_tests.rs crates/iota-core/src/kanban/event_sourcing.rs
git commit -m "refactor: extract inline tests from kanban/event_sourcing.rs"
```

---

### Task 2: Extract `kanban/state_machine.rs` tests

**Files:**
- Modify: `crates/iota-core/src/kanban/state_machine.rs:28-57`
- Create: `crates/iota-core/src/kanban/state_machine_tests.rs`

- [ ] **Step 1: Read test module**

```bash
sed -n '28,57p' crates/iota-core/src/kanban/state_machine.rs
```

- [ ] **Step 2: Create `state_machine_tests.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions() { ... }

    #[test]
    fn invalid_transitions() { ... }

    #[test]
    fn blocked_to_done_is_valid() { ... }
}
```

- [ ] **Step 3: Replace inline tests with path reference**

```rust
#[cfg(test)]
#[path = "state_machine_tests.rs"]
mod tests;
```

- [ ] **Step 4: Verify compilation**

```bash
cargo test -p iota-core --lib kanban::state_machine::tests 2>&1 | head -30
```

- [ ] **Step 5: Commit**

```bash
git add crates/iota-core/src/kanban/state_machine_tests.rs crates/iota-core/src/kanban/state_machine.rs
git commit -m "refactor: extract inline tests from kanban/state_machine.rs"
```

---

### Task 3: Extract `kanban/bridge.rs` tests

**Files:**
- Modify: `crates/iota-core/src/kanban/bridge.rs:36-50, 259-275`
- Create: `crates/iota-core/src/kanban/bridge_tests.rs`

Note: Has TWO inline test blocks.

- [ ] **Step 1: Read both test modules**

```bash
sed -n '36,50p' crates/iota-core/src/kanban/bridge.rs
sed -n '259,275p' crates/iota-core/src/kanban/bridge.rs
```

- [ ] **Step 2: Create `bridge_tests.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ... inner mod tests { ... } block at line 36
    // ... #[cfg(test)] mod tests { ... } block at line 259
}
```

- [ ] **Step 3: Replace both inline blocks with path reference**

Add at bottom of file:

```rust
#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
```

- [ ] **Step 4: Verify compilation**

```bash
cargo test -p iota-core --lib kanban::bridge::tests 2>&1 | head -30
```

- [ ] **Step 5: Commit**

```bash
git add crates/iota-core/src/kanban/bridge_tests.rs crates/iota-core/src/kanban/bridge.rs
git commit -m "refactor: extract inline tests from kanban/bridge.rs"
```

---

### Task 4: Extract `kanban/worker.rs` tests

**Files:**
- Modify: `crates/iota-core/src/kanban/worker.rs:169-235`
- Create: `crates/iota-core/src/kanban/worker_tests.rs`

- [ ] **Step 1: Read test modules**

```bash
sed -n '169,235p' crates/iota-core/src/kanban/worker.rs
```

- [ ] **Step 2: Create `worker_tests.rs`**

- [ ] **Step 3: Replace inline tests with path reference**

```rust
#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
```

- [ ] **Step 4: Verify compilation**

```bash
cargo test -p iota-core --lib kanban::worker::tests 2>&1 | head -30
```

- [ ] **Step 5: Commit**

---

### Task 5: Extract `kanban/event_sync.rs` tests

**Files:**
- Modify: `crates/iota-core/src/kanban/event_sync.rs`
- Create: `crates/iota-core/src/kanban/event_sync_tests.rs`

Note: Has `mod tests` inside `impl SqliteEventStore` AND a top-level `#[cfg(test)] mod tests`.

- [ ] **Step 1: Read test modules**

```bash
sed -n '9,11p' crates/iota-core/src/kanban/event_sync.rs
sed -n '263,295p' crates/iota-core/src/kanban/event_sync.rs
```

- [ ] **Step 2: Create `event_sync_tests.rs`**

- [ ] **Step 3: Replace inline tests with path reference**

- [ ] **Step 4: Verify compilation**

- [ ] **Step 5: Commit**

---

### Task 6: Extract `kanban/dispatcher.rs` tests

**Files:**
- Modify: `crates/iota-core/src/kanban/dispatcher.rs`
- Create: `crates/iota-core/src/kanban/dispatcher_tests.rs`

Note: Has `mod tests { ... }` inside `impl Dispatcher` AND a top-level `#[cfg(test)] mod tests`.

- [ ] **Step 1: Read test modules**

```bash
sed -n '4,96p' crates/iota-core/src/kanban/dispatcher.rs
sed -n '449,500p' crates/iota-core/src/kanban/dispatcher.rs
```

- [ ] **Step 2: Create `dispatcher_tests.rs`**

- [ ] **Step 3: Replace inline tests with path reference**

- [ ] **Step 4: Verify compilation**

- [ ] **Step 5: Commit**

---

### Task 7: Extract `kanban/shadow.rs` tests

**Files:**
- Modify: `crates/iota-core/src/kanban/shadow.rs:453-500`
- Create: `crates/iota-core/src/kanban/shadow_tests.rs`

- [ ] **Step 1-5: Same pattern**

---

### Task 8: Extract `kanban/mod.rs` (already using path pattern)

**Files:**
- Modify: `crates/iota-core/src/kanban/mod.rs:12-13`
- Create: `crates/iota-core/src/kanban/sqlite_store_tests.rs`

Note: `mod.rs` uses `#[path = "sqlite_store_tests.rs"]` but that file doesn't exist yet - `sqlite_store.rs` has inline tests.

- [ ] **Step 1: Read `sqlite_store.rs` test module**

- [ ] **Step 2: Extract to `sqlite_store_tests.rs`**

- [ ] **Step 3: Verify `mod.rs` path already correct**

```rust
#[cfg(test)]
#[path = "sqlite_store_tests.rs"]
mod sqlite_store_tests;
```

- [ ] **Step 4: Verify compilation**

- [ ] **Step 5: Commit**

---

### Task 9: Final verification for Batch 1

- [ ] **Step 1: Run all kanban tests**

```bash
cargo test -p iota-core --lib kanban 2>&1 | tail -20
```

- [ ] **Step 2: Verify no inline test modules remain**

```bash
grep -n '#[cfg(test)]' crates/iota-core/src/kanban/*.rs | grep -v '_tests.rs'
```

Expected: only `mod tests;` path declarations, no `mod tests {` blocks.

- [ ] **Step 3: Push and create PR**

```bash
git push origin HEAD && gh pr create --title "refactor(batch1): extract kanban inline tests" --body "Extract inline test modules from kanban/* to *_tests.rs files"
```
