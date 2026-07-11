# iota-core

`iota-core` is the reusable Rust runtime behind iota-sympantos. It provides ACP backend orchestration, configuration, context assembly, memory, skills, MCP support, daemon protocol types, storage, telemetry, and normalized runtime events without depending on the CLI, TUI, or desktop application.

## Add the dependency

For a second-party consumer that uses this repository as a git dependency:

```toml
[dependencies]
iota-core = { git = "<repository-url>", package = "iota-core" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional Kanban MCP tools only when the consuming project also needs the iota Kanban integration:

```toml
[dependencies]
iota-core = { git = "<repository-url>", package = "iota-core", features = ["kanban"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

For a local multi-crate project, use a path dependency instead:

```toml
[dependencies]
iota-core = { path = "../iota-sympantos/crates/iota-core" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Minimal usage

```rust,no_run
use iota_core::config::read_config;
use iota_core::{AcpBackend, IotaEngine, DEFAULT_TIMEOUT_MS};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = read_config()?;
    let mut engine = IotaEngine::create_session(
        config,
        false,
        DEFAULT_TIMEOUT_MS,
        None,
    );
    let output = engine
        .run_with_timing(
            AcpBackend::Codex,
            PathBuf::from("."),
            "Summarize this project",
        )
        .await?;
    println!("{}", output.text);
    Ok(())
}
```

The configuration loader intentionally reads `~/.i6/nimia.yaml`, matching the iota runtime. Consumers that need explicit configuration can construct `NimiaConfig` directly and pass it to `IotaEngine::create_session`.

## Features

| Feature | Default | Effect |
| :------ | :------ | :----- |
| `kanban` | No | Enables `iota-kanban` and the `iota_kanban_*` MCP tools. |

## Compatibility

The crate supports Windows, macOS, and Linux. It requires Rust 1.95.0 or newer and Rust edition 2024.
