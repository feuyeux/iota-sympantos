use anyhow::{Context, Result};
use std::io::{self, Write as _};

use crate::acp;
use crate::config::NimiaConfig;
use crate::engine::IotaEngine;

pub async fn run(config: NimiaConfig) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut engine = IotaEngine::new(config, false, acp::DEFAULT_TIMEOUT_MS);

    println!("ACP backends start lazily on first use.");
    println!("Enter '<backend> <prompt>' or 'exit'. Example: codex ping");

    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
            break;
        }

        let Some((backend_name, prompt)) = line.split_once(char::is_whitespace) else {
            eprintln!("Expected '<backend> <prompt>'");
            continue;
        };
        let backend = match acp::AcpBackend::parse(backend_name) {
            Ok(backend) => backend,
            Err(err) => {
                eprintln!("{}", err);
                continue;
            }
        };

        match engine
            .prompt_in_cwd(backend, cwd.clone(), prompt.trim())
            .await
        {
            Ok(text) => {
                if !text.is_empty() {
                    println!("{}", text);
                }
            }
            Err(err) => eprintln!("{}", err),
        }
    }

    engine.shutdown().await;
    Ok(())
}
