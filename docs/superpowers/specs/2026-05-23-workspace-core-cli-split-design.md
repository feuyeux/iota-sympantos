# Workspace Core/CLI Split Design

> Archive note: this is a historical design spec. For current behavior and commands, see [../../iota book.md](../../iota%20book.md), [../../architecture.md](../../architecture.md), and [../../command.md](../../command.md).

## Goal

Split the current single package into a Cargo workspace with a reusable core crate and a CLI/TUI application crate.

The immediate target is:

- `iota-core`: reusable runtime and domain modules.
- `iota-cli`: terminal application that owns command dispatch and the ratatui TUI.

This prepares the repository for a future Tauri desktop app that can depend on the same core crate without depending on CLI or TUI code.

## Non-Goals

- Do not add the Tauri app in this change.
- Do not redesign module internals.
- Do not change the installed binary name; it remains `iota`.
- Do not change runtime behavior, ACP protocol behavior, config format, or daemon semantics.

## Workspace Layout

The repository root becomes a workspace-only manifest:

```text
iota-sympantos/
├── Cargo.toml
├── crates/
│   ├── iota-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── acp/
│   │       ├── config/
│   │       ├── context/
│   │       ├── daemon/
│   │       ├── engine/
│   │       ├── kanban/
│   │       ├── mcp/
│   │       ├── memory/
│   │       ├── runtime_event/
│   │       ├── skill/
│   │       ├── store/
│   │       ├── telemetry/
│   │       └── utils/
│   └── iota-cli/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── cli/
│           └── tui/
└── docs/
```

The root `Cargo.toml` should define workspace members and shared package metadata:

```toml
[workspace]
members = [
  "crates/iota-core",
  "crates/iota-cli",
]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.95.0"
version = "0.1.0"
```

Shared dependency versions should live under `[workspace.dependencies]`. Member crates should reference shared dependencies with `{ workspace = true }` where practical.

## Crate Boundaries

`iota-core` owns reusable runtime functionality:

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

`iota-cli` owns application-level terminal behavior:

```rust
mod cli;
mod tui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
```

Dependency direction:

```text
iota-cli          -> iota-core
future Tauri app  -> iota-core
iota-core         -> no dependency on app crates
```

`daemon` remains in `iota-core`. Although the current daemon path is driven by CLI commands, it is a reusable runtime connection-pooling mechanism and may be useful to future app frontends.

## Import Rules

Inside `iota-core`, existing internal imports should continue to use `crate::...` because those modules remain in the same crate.

Inside `iota-cli`, imports of core modules must use the dependency crate:

```rust
use iota_core::config::NimiaConfig;
use iota_core::engine::IotaEngine;
```

CLI/TUI local imports should remain app-local:

```rust
use crate::tui;
```

## Migration Plan

1. Convert the root manifest into a workspace manifest.
2. Create `crates/iota-core/Cargo.toml` and `crates/iota-cli/Cargo.toml`.
3. Move reusable modules from `src/` into `crates/iota-core/src/`.
4. Move `main.rs`, `cli/`, and `tui/` into `crates/iota-cli/src/`.
5. Move test files with their modules, preserving the existing `#[path = "..._tests.rs"]` convention.
6. Update app imports from `crate::...` to `iota_core::...` where they reference core modules.
7. Update docs that reference old source paths only where needed to keep repository navigation accurate.

## Verification

The implementation is considered valid when these commands pass:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
```

After the split, package-qualified commands are required from the workspace root for direct Cargo runs:

```bash
cargo run -p iota-cli -- check
```

The installed binary name remains `iota`, so user-facing CLI usage is unchanged.

## Risks And Constraints

- `iota-cli` may initially have many `crate::...` imports that must be carefully separated into app-local and core imports.
- Any scripts or docs using `cargo run -- ...` from the root may need package qualification.
- The future Tauri app should depend on `iota-core`, not `iota-cli`.
- The change must preserve cross-platform path handling and the existing test-file convention.
