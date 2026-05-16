use anyhow::{Context, Result};

use crate::acp;
use crate::config;
use crate::memory::MemoryStore;
use crate::native;
use crate::skill::SkillRegistry;

pub(super) fn run_native_materialize(args: &[String]) -> Result<()> {
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let backend = args
        .windows(2)
        .find_map(|pair| (pair[0] == "--backend").then_some(pair[1].as_str()))
        .map(acp::AcpBackend::parse)
        .transpose()?;
    let positional = args
        .iter()
        .enumerate()
        .filter(|(index, arg)| {
            arg.as_str() != "--dry-run"
                && arg.as_str() != "--all"
                && arg.as_str() != "--backend"
                && index
                    .checked_sub(1)
                    .is_none_or(|prev| args[prev] != "--backend")
        })
        .map(|(_, arg)| arg)
        .collect::<Vec<_>>();
    let body = positional
        .get(1)
        .map(|value| value.as_str())
        .unwrap_or("iota native overlay");
    if args.iter().any(|arg| arg == "--all") {
        let backend = backend.context("native-materialize --all requires --backend <name>")?;
        let workspace = positional
            .first()
            .map(std::path::PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        let config = config::read_config()?;
        let roots = config::context_skill_roots(&config);
        let skills = SkillRegistry::load(&workspace, &roots);
        let memory = config::context_memory_db_path(&config)
            .ok()
            .and_then(|path| MemoryStore::open(&path).ok());
        let previews = native::dry_run_backend_projection(
            backend,
            &workspace,
            memory.as_ref(),
            Some(&skills),
        )?;
        if dry_run {
            println!(
                "{}",
                serde_json::json!({
                    "previews": previews.iter().map(|preview| serde_json::json!({
                        "path": preview.path.display().to_string(),
                        "changed": preview.changed,
                        "content": preview.content,
                    })).collect::<Vec<_>>()
                })
            );
        } else {
            let changed = previews
                .iter()
                .map(|preview| native::apply(&preview.path, &preview.content))
                .collect::<Result<Vec<_>>>()?;
            println!("{}", serde_json::json!({"changed": changed}));
        }
        return Ok(());
    }
    let path = if let Some(backend) = backend {
        let workspace = positional
            .first()
            .map(std::path::PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        native::backend_memory_path(backend, &workspace)?
            .context("native materialization for this backend is deferred")?
    } else {
        positional
            .first()
            .map(std::path::PathBuf::from)
            .context("native-materialize requires a target path or --backend <name>")?
    };
    if dry_run {
        let preview = native::dry_run(&path, body)?;
        println!(
            "{}",
            serde_json::json!({
                "path": preview.path.display().to_string(),
                "changed": preview.changed,
                "content": preview.content,
            })
        );
    } else {
        let changed = native::apply(&path, body)?;
        println!(
            "{}",
            serde_json::json!({"path": path.display().to_string(), "changed": changed})
        );
    }
    Ok(())
}
