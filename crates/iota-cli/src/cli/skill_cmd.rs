use anyhow::{Context, Result};

use iota_core::skill;

pub(super) async fn run_skill_command(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("pull") => {
            let source = args
                .get(1)
                .context("skill pull requires a source path or URL")?;
            let name = args.get(2).map(String::as_str);
            let path = skill::cache::pull_skill(source, name).await?;
            println!(
                "{}",
                serde_json::json!({"path": path.display().to_string()})
            );
            Ok(())
        }
        _ => anyhow::bail!("Usage: iota skill pull <source> [name]"),
    }
}
