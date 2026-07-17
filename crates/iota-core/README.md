# iota-sympantos-core

`iota-sympantos-core` is the publishable package for the reusable Rust runtime behind iota-sympantos. Its library target remains `iota_core`, so Rust source continues to import it with `use iota_core::...`. It provides ACP backend orchestration, configuration, context assembly, memory, skills, MCP support, daemon protocol types, storage, telemetry, and normalized runtime events without depending on the CLI, TUI, or desktop application.

## Add the dependency

Consume the independently versioned package from the registry while retaining the concise local dependency/import name:

```toml
[dependencies]
iota-core = { package = "iota-sympantos-core", version = "0.1.0" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional Kanban MCP tools only when the consuming project also needs the iota Kanban integration:

```toml
[dependencies]
iota-core = { package = "iota-sympantos-core", version = "0.1.0", features = ["kanban"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

For development in a combined checkout, add a path while keeping the version and package contract explicit:

```toml
[dependencies]
iota-core = { package = "iota-sympantos-core", version = "0.1.0", path = "../iota-sympantos/crates/iota-core" }
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
