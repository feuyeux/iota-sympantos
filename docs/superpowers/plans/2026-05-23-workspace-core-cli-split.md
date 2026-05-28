# Workspace Core/CLI Split Implementation Plan

> Archive note: this is a historical implementation plan. For current behavior and commands, see [../../iota book.md](../../iota%20book.md), [../../architecture.md](../../architecture.md), and [../../command.md](../../command.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the repository into a Cargo workspace with reusable `iota-core` and terminal app `iota-cli`.

**Architecture:** The workspace root only coordinates member crates and shared dependency versions. `iota-core` owns runtime/domain modules and exports them from `lib.rs`; `iota-cli` owns `main.rs`, `cli`, and `tui`, and imports reusable modules through `iota_core`.

**Tech Stack:** Rust 2024, Cargo workspace resolver 3, Tokio, ratatui/crossterm for the CLI TUI, rusqlite-backed stores, existing inline `#[path = "..._tests.rs"]` test convention.

---

## File Structure

- Modify: `Cargo.toml`
  Root workspace manifest with `[workspace]`, `[workspace.package]`, and `[workspace.dependencies]`.
- Create: `crates/iota-core/Cargo.toml`
  Library crate manifest. Package name `iota-core`, library crate name `iota_core`.
- Create: `crates/iota-cli/Cargo.toml`
  App crate manifest. Package name `iota-cli`, binary name `iota`.
- Move: `src/acp` -> `crates/iota-core/src/acp`
- Move: `src/config` -> `crates/iota-core/src/config`
- Move: `src/context` -> `crates/iota-core/src/context`
- Move: `src/daemon` -> `crates/iota-core/src/daemon`
- Move: `src/engine` -> `crates/iota-core/src/engine`
- Move: `src/kanban` -> `crates/iota-core/src/kanban`
- Move: `src/mcp` -> `crates/iota-core/src/mcp`
- Move: `src/memory` -> `crates/iota-core/src/memory`
- Move: `src/runtime_event` -> `crates/iota-core/src/runtime_event`
- Move: `src/skill` -> `crates/iota-core/src/skill`
- Move: `src/store` -> `crates/iota-core/src/store`
- Move: `src/telemetry` -> `crates/iota-core/src/telemetry`
- Move: `src/utils` -> `crates/iota-core/src/utils`
- Move: `src/lib.rs` -> `crates/iota-core/src/lib.rs`, then replace its contents with the full core module export list.
- Move: `src/main.rs` -> `crates/iota-cli/src/main.rs`, then reduce its contents to app-local modules only.
- Move: `src/cli` -> `crates/iota-cli/src/cli`
- Move: `src/tui` -> `crates/iota-cli/src/tui`
- Modify: `README.md`, `README-zh.md`, `docs/architecture.md`, `docs/code-call-chains.md`, `docs/command.md`, `docs/observability.md`, and module `SKILL.md` files only if they contain source paths that become actively misleading after the move.

## Task 1: Create Workspace Manifests

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/iota-core/Cargo.toml`
- Create: `crates/iota-cli/Cargo.toml`

- [x] **Step 1: Inspect current manifest before editing**

Run:

```bash
sed -n '1,220p' Cargo.toml
```

Expected: The manifest has `[package]`, `[dependencies]`, `[build-dependencies]`, `[lib]`, and `[[bin]]`.

- [x] **Step 2: Replace root `Cargo.toml` with workspace manifest**

Set `Cargo.toml` to:

```toml
[workspace]
members = [
    "crates/iota-core",
    "crates/iota-cli",
]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.95.0"

[workspace.dependencies]
anyhow = "1.0"
chrono = "0.4"
crossterm = { version = "0.28", features = ["bracketed-paste", "event-stream"] }
dirs = "6.0.0"
futures-util = "0.3"
hex = "0.4"
hostname = "0.4"
opentelemetry = "0.29"
opentelemetry-appender-tracing = "0.29"
opentelemetry-otlp = { version = "0.29", features = ["grpc-tonic", "trace", "metrics", "logs"] }
opentelemetry_sdk = { version = "0.29", features = ["rt-tokio"] }
pulldown-cmark = "0.12"
ratatui = { version = "0.29", features = ["scrolling-regions", "unstable-backend-writer", "unstable-rendered-line-info", "unstable-widget-ref"] }
reqwest = { version = "0.13.3", features = ["json", "blocking"] }
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
sha2 = "0.10"
tokio = { version = "1.0", features = ["full"] }
tokio-util = "0.7"
tracing = "0.1"
tracing-opentelemetry = "0.30"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "registry"] }
unicode-segmentation = "1.12"
unicode-width = "0.2"
urlencoding = "2.1"
uuid = { version = "1.8", features = ["v4", "serde"] }
```

- [x] **Step 3: Create `crates/iota-core/Cargo.toml`**

Set `crates/iota-core/Cargo.toml` to:

```toml
[package]
name = "iota-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
anyhow.workspace = true
chrono.workspace = true
dirs.workspace = true
futures-util.workspace = true
hex.workspace = true
hostname.workspace = true
opentelemetry.workspace = true
opentelemetry-appender-tracing.workspace = true
opentelemetry-otlp.workspace = true
opentelemetry_sdk.workspace = true
reqwest.workspace = true
rusqlite.workspace = true
serde.workspace = true
serde_json.workspace = true
serde_yaml.workspace = true
sha2.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
tracing-opentelemetry.workspace = true
tracing-subscriber.workspace = true
urlencoding.workspace = true
uuid.workspace = true

[build-dependencies]
chrono.workspace = true

[lib]
name = "iota_core"
path = "src/lib.rs"
```

- [x] **Step 4: Create `crates/iota-cli/Cargo.toml`**

Set `crates/iota-cli/Cargo.toml` to:

```toml
[package]
name = "iota-cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
anyhow.workspace = true
chrono.workspace = true
crossterm.workspace = true
futures-util.workspace = true
iota-core = { path = "../iota-core" }
pulldown-cmark.workspace = true
ratatui.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
unicode-segmentation.workspace = true
unicode-width.workspace = true

[[bin]]
name = "iota"
path = "src/main.rs"
```

- [x] **Step 5: Run metadata to verify manifest shape**

Run:

```bash
cargo metadata --no-deps --format-version 1
```

Expected: FAIL is acceptable before source moves if Cargo complains about missing `src/lib.rs` or `src/main.rs` under the new crate directories. Manifest syntax errors are not acceptable.

- [x] **Step 6: Commit manifest setup**

Run:

```bash
git add Cargo.toml crates/iota-core/Cargo.toml crates/iota-cli/Cargo.toml
git commit -m "build: add core and cli workspace manifests"
```

Expected: One commit containing only manifest additions/changes.

## Task 2: Move Source Files Into Member Crates

**Files:**
- Move core directories from `src/` to `crates/iota-core/src/`
- Move app directories from `src/` to `crates/iota-cli/src/`
- Modify: `crates/iota-core/src/lib.rs`
- Modify: `crates/iota-cli/src/main.rs`

- [x] **Step 1: Create member source directories**

Run:

```bash
mkdir -p crates/iota-core/src crates/iota-cli/src
```

Expected: Both source directories exist.

- [x] **Step 2: Move core modules**

Run:

```bash
git mv src/acp crates/iota-core/src/acp
git mv src/config crates/iota-core/src/config
git mv src/context crates/iota-core/src/context
git mv src/daemon crates/iota-core/src/daemon
git mv src/engine crates/iota-core/src/engine
git mv src/kanban crates/iota-core/src/kanban
git mv src/mcp crates/iota-core/src/mcp
git mv src/memory crates/iota-core/src/memory
git mv src/runtime_event crates/iota-core/src/runtime_event
git mv src/skill crates/iota-core/src/skill
git mv src/store crates/iota-core/src/store
git mv src/telemetry crates/iota-core/src/telemetry
git mv src/utils crates/iota-core/src/utils
git mv src/lib.rs crates/iota-core/src/lib.rs
```

Expected: Core directories and their adjacent `*_tests.rs` files move together.

- [x] **Step 3: Move app modules**

Run:

```bash
git mv src/main.rs crates/iota-cli/src/main.rs
git mv src/cli crates/iota-cli/src/cli
git mv src/tui crates/iota-cli/src/tui
```

Expected: App directories and their adjacent `*_tests.rs` files move together.

- [x] **Step 4: Remove the now-empty root `src` directory if empty**

Run:

```bash
rmdir src
```

Expected: PASS if `src` is empty. If it fails, run `find src -maxdepth 2 -type f | sort` and classify any remaining files before moving them.

- [x] **Step 5: Replace `crates/iota-core/src/lib.rs` module exports**

Set `crates/iota-core/src/lib.rs` to:

```rust
pub mod acp;
pub mod config;
pub mod context;
pub mod daemon;
pub mod engine;
pub mod kanban;
pub mod mcp;
pub mod memory;
pub mod runtime_event;
pub mod skill;
pub mod store;
pub mod telemetry;
pub mod utils;
```

- [x] **Step 6: Replace `crates/iota-cli/src/main.rs` app entry**

Set `crates/iota-cli/src/main.rs` to:

```rust
mod cli;
mod tui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
```

- [x] **Step 7: Verify no source files remain in root `src`**

Run:

```bash
find src -type f
```

Expected: FAIL with `find: src: No such file or directory`, or no output if an empty `src` directory remains.

- [x] **Step 8: Commit source moves**

Run:

```bash
git add -A
git commit -m "refactor: move core and cli sources into workspace crates"
```

Expected: One commit primarily showing renames.

## Task 3: Update CLI/TUI Core Imports

**Files:**
- Modify: `crates/iota-cli/src/cli/*.rs`
- Modify: `crates/iota-cli/src/tui/*.rs`
- Modify: `crates/iota-cli/src/tui/*_tests.rs`

- [x] **Step 1: List app files that still import core through `crate::`**

Run:

```bash
rg -n "crate::(acp|config|context|daemon|engine|kanban|mcp|memory|runtime_event|skill|store|telemetry|utils)" crates/iota-cli/src
```

Expected: Matches in CLI/TUI files before replacement.

- [x] **Step 2: Mechanically replace core import roots**

Run:

```bash
perl -pi -e 's/crate::(acp|config|context|daemon|engine|kanban|mcp|memory|runtime_event|skill|store|telemetry|utils)/iota_core::$1/g' $(rg -l "crate::(acp|config|context|daemon|engine|kanban|mcp|memory|runtime_event|skill|store|telemetry|utils)" crates/iota-cli/src)
```

Expected: App-local imports such as `crate::tui` and `super::...` remain unchanged.

- [x] **Step 3: Re-run the search**

Run:

```bash
rg -n "crate::(acp|config|context|daemon|engine|kanban|mcp|memory|runtime_event|skill|store|telemetry|utils)" crates/iota-cli/src
```

Expected: No matches.

- [x] **Step 4: Run check and capture remaining compile errors**

Run:

```bash
cargo check --workspace
```

Expected: Initial run may fail. Remaining failures should be unresolved imports, private module visibility, or missing dependencies.

- [x] **Step 5: Fix public API visibility only where needed**

If `cargo check --workspace` reports private-module errors from `iota-cli`, expose the smallest required item from `iota-core`. For example, if `iota_core::store::observability` is private but used by `iota-cli`, update `crates/iota-core/src/store/mod.rs` with:

```rust
pub mod approvals;
pub mod cache;
pub mod ledger;
pub mod observability;
```

Expected: Existing public modules remain public; do not expose unrelated internals unless a compile error proves the app uses them.

- [x] **Step 6: Add missing app dependencies only if compile errors require them**

If `cargo check --workspace` reports an unresolved external crate in `iota-cli`, add that dependency to `crates/iota-cli/Cargo.toml` using workspace dependency syntax. Example:

```toml
serde = { workspace = true }
```

Expected: Dependencies used only by core remain only in `iota-core`; dependencies used by CLI/TUI are listed in `iota-cli`.

- [x] **Step 7: Run check until it passes**

Run:

```bash
cargo check --workspace
```

Expected: PASS.

- [x] **Step 8: Commit import and visibility fixes**

Run:

```bash
git add crates/iota-core crates/iota-cli Cargo.toml
git commit -m "refactor: import core modules from cli crate"
```

Expected: One commit containing import rewrites and any minimal visibility/dependency fixes.

## Task 4: Update Path-Sensitive Documentation

**Files:**
- Modify only if needed: `README.md`
- Modify only if needed: `README-zh.md`
- Modify only if needed: `docs/architecture.md`
- Modify only if needed: `docs/code-call-chains.md`
- Modify only if needed: `docs/command.md`
- Modify only if needed: `docs/observability.md`
- Modify only if needed: `crates/iota-core/src/**/SKILL.md`
- Modify only if needed: `crates/iota-cli/src/**/SKILL.md`

- [x] **Step 1: Search for old source path references**

Run:

```bash
rg -n "src/(main|cli|tui|acp|config|context|daemon|engine|kanban|mcp|memory|runtime_event|skill|store|telemetry|utils)|cargo run --" README.md README-zh.md docs crates
```

Expected: Matches identify docs that mention old paths or root-package Cargo run commands.

- [x] **Step 2: Update source path examples**

For docs that list the old tree, rewrite path examples to the new workspace shape. Use this exact convention for path references:

```text
crates/iota-cli/src/main.rs
crates/iota-cli/src/cli/
crates/iota-cli/src/tui/
crates/iota-core/src/engine/
crates/iota-core/src/acp/
```

Expected: Docs no longer imply that app and core modules still live directly under root `src/`.

- [x] **Step 3: Update root Cargo run examples**

Replace root-package examples:

```bash
cargo run -- check
```

with package-qualified examples:

```bash
cargo run -p iota-cli -- check
```

Expected: Installed binary examples such as `iota check` remain unchanged.

- [x] **Step 4: Preserve unrelated user changes**

Run:

```bash
git diff -- README.md
```

Expected: If `README.md` had pre-existing unrelated edits, keep them intact and only add path or command updates required by this split.

- [x] **Step 5: Stage only path-related documentation updates**

Run:

```bash
git diff -- README.md README-zh.md docs crates
```

Expected: The diff contains only workspace path or package-qualified Cargo command updates. If `README.md` contains unrelated pre-existing edits, do not stage those hunks.

Run:

```bash
git add -p README.md README-zh.md docs crates
```

Expected: Only hunks related to this workspace split are staged.

- [x] **Step 6: Commit documentation updates if any hunks were staged**

Run:

```bash
git diff --cached --quiet || git commit -m "docs: update paths for workspace layout"
```

Expected: Commit is created only when documentation changes for this split were staged.

## Task 5: Full Verification And Cleanup

**Files:**
- Modify only if verification reveals required fixes.

- [x] **Step 1: Format all crates**

Run:

```bash
cargo fmt --all --check
```

Expected: PASS. If it fails, run `cargo fmt --all`, inspect the diff, and commit formatting with the relevant code changes.

- [x] **Step 2: Run workspace tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS. If a test fails due to moved paths, fix the test or code path using `PathBuf` and workspace-relative assumptions only where the test owns those assumptions.

- [x] **Step 3: Run clippy**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [x] **Step 4: Run CLI smoke test**

Run:

```bash
cargo run -p iota-cli -- check
```

Expected: Command runs through the `iota-cli` binary. It may report backend availability according to local machine setup, but it must not fail because Cargo cannot find the package or binary.

- [x] **Step 5: Inspect final status**

Run:

```bash
git status --short
```

Expected: Only intentional changes remain. Pre-existing unrelated files, such as a user-modified `README.md`, must not be reverted.

- [x] **Step 6: Commit final verification fixes if any**

Run:

```bash
git add -A
git diff --cached --quiet || git commit -m "test: verify workspace core cli split"
```

Expected: Commit is created only if verification required additional fixes.

## Self-Review

- Spec coverage: The plan covers workspace manifests, `iota-core` and `iota-cli` boundaries, source moves, import rules, path-sensitive docs, and the specified verification commands.
- Placeholder scan: No red-flag placeholders or unspecified implementation steps remain.
- Type consistency: Crate names are consistent: package `iota-core` exposes library crate `iota_core`; package `iota-cli` exposes binary `iota`.
