use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::kanban::{
    AdvancedBridge, KanbanStore, SqliteKanbanStore, default_pull_source, export_event_bundle,
    import_event_bundle, pull_event_bundle, push_event_bundle, read_event_bundle,
    serve_event_sync, write_event_bundle,
};

pub(super) fn run_kanban_command(args: &[String]) -> Result<()> {
    let store_path = default_kanban_dir().join("iota.db");
    let shadows_dir = default_kanban_dir().join("shadows");
    let store = Arc::new(SqliteKanbanStore::open(&store_path)?);
    let bridge = AdvancedBridge::new(PathBuf::from("hermes"), shadows_dir);

    for line in execute_kanban_command(args, store.as_ref(), &bridge)? {
        println!("{line}");
    }
    Ok(())
}

fn execute_kanban_command(
    args: &[String],
    store: &dyn KanbanStore,
    bridge: &AdvancedBridge,
) -> Result<Vec<String>> {
    match args.first().map(String::as_str) {
        Some("specify") => {
            let task_id = parse_task_id(args.get(1), "Usage: iota kanban specify <id>")?;
            ensure_bridge_available(bridge)?;
            let result = bridge
                .specify(task_id, store)
                .with_context(|| format!("failed to specify task #{task_id}"))?;
            Ok(vec![format!(
                "Specified task #{} ({} chars).",
                result.task_id,
                result.spec_body.chars().count()
            )])
        }
        Some("decompose") => {
            let task_id = parse_task_id(args.get(1), "Usage: iota kanban decompose <id>")?;
            ensure_bridge_available(bridge)?;
            let result = bridge
                .decompose(task_id, store)
                .with_context(|| format!("failed to decompose task #{task_id}"))?;
            Ok(vec![format!(
                "Decomposed task #{} into {} child task(s): {}",
                result.parent_id,
                result.child_ids.len(),
                result
                    .child_ids
                    .iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )])
        }
        Some("export") | Some("export-events") => {
            let path = parse_path_arg(args.get(1), "Usage: iota kanban export <path> [cursor]")?;
            let cursor = args
                .get(2)
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("invalid event cursor")?
                .unwrap_or(0);
            let source = hostname::get()
                .ok()
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| "local".to_string());
            let bundle = export_event_bundle(store, cursor, source)?;
            write_event_bundle(&path, &bundle)?;
            Ok(vec![format!(
                "Exported {} kanban event(s) to {} (cursor {}).",
                bundle.events.len(),
                path.display(),
                bundle.cursor
            )])
        }
        Some("import") | Some("import-events") => {
            let path = parse_path_arg(args.get(1), "Usage: iota kanban import <path>")?;
            let bundle = read_event_bundle(&path)?;
            let concrete = store
                .as_any()
                .downcast_ref::<SqliteKanbanStore>()
                .context("kanban event import requires SqliteKanbanStore")?;
            let report = import_event_bundle(concrete, &bundle)?;
            Ok(vec![format!(
                "Imported {}/{} kanban event(s) from {} ({} skipped, cursor {}).",
                report.events_applied,
                report.events_seen,
                report.source,
                report.events_skipped,
                report.cursor
            )])
        }
        Some("serve-sync") => {
            let addr = args.get(1).map(String::as_str).unwrap_or("127.0.0.1:47662");
            let concrete = store
                .as_any()
                .downcast_ref::<SqliteKanbanStore>()
                .context("kanban sync server requires SqliteKanbanStore")?;
            println!("Serving kanban sync on {addr}");
            serve_event_sync(Arc::new(concrete.clone()), addr)?;
            Ok(Vec::new())
        }
        Some("pull") => {
            let addr = args
                .get(1)
                .map(String::as_str)
                .context("Usage: iota kanban pull <addr> [cursor]")?;
            let cursor = args
                .get(2)
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("invalid event cursor")?
                .unwrap_or(0);
            let source = default_pull_source(addr);
            let bundle = pull_event_bundle(addr, cursor, source)?;
            let concrete = store
                .as_any()
                .downcast_ref::<SqliteKanbanStore>()
                .context("kanban sync pull requires SqliteKanbanStore")?;
            let report = import_event_bundle(concrete, &bundle)?;
            Ok(vec![format!(
                "Pulled and imported {}/{} kanban event(s) from {} ({} skipped, cursor {}).",
                report.events_applied,
                report.events_seen,
                report.source,
                report.events_skipped,
                report.cursor
            )])
        }
        Some("push") => {
            let addr = args
                .get(1)
                .map(String::as_str)
                .context("Usage: iota kanban push <addr> [cursor]")?;
            let cursor = args
                .get(2)
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("invalid event cursor")?
                .unwrap_or(0);
            let source = hostname::get()
                .ok()
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| "local".to_string());
            let bundle = export_event_bundle(store, cursor, source)?;
            let report = push_event_bundle(addr, bundle)?;
            Ok(vec![format!(
                "Pushed {}/{} kanban event(s) to {} ({} skipped, cursor {}).",
                report.events_applied,
                report.events_seen,
                addr,
                report.events_skipped,
                report.cursor
            )])
        }
        Some("help") | Some("-h") | Some("--help") | None => Ok(vec![usage().to_string()]),
        Some(other) => bail!("unknown kanban command: {other}\n{}", usage()),
    }
}

fn ensure_bridge_available(bridge: &AdvancedBridge) -> Result<()> {
    if bridge.is_available() {
        Ok(())
    } else {
        bail!("advanced kanban bridge is not available: hermes command failed")
    }
}

fn parse_task_id(value: Option<&String>, usage: &str) -> Result<u64> {
    let Some(value) = value else {
        bail!("{usage}");
    };
    let value = value.strip_prefix('#').unwrap_or(value);
    value
        .parse::<u64>()
        .with_context(|| format!("invalid task id: {value}"))
}

fn parse_path_arg(value: Option<&String>, usage: &str) -> Result<PathBuf> {
    let Some(value) = value else {
        bail!("{usage}");
    };
    Ok(PathBuf::from(value))
}

fn default_kanban_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .join(".i6")
        .join("kanban")
}

fn usage() -> &'static str {
    "Usage:\n  iota kanban specify <id>\n  iota kanban decompose <id>\n  iota kanban export <path> [cursor]\n  iota kanban import <path>\n  iota kanban serve-sync [addr]\n  iota kanban pull <addr> [cursor]\n  iota kanban push <addr> [cursor]"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kanban::{CreateTaskRequest, Status};

    fn fake_hermes_echo_spec(tmp: &Path) -> PathBuf {
        if cfg!(windows) {
            let path = tmp.join("fake-hermes.cmd");
            std::fs::write(&path, "@echo off\r\necho {\"spec\":\"cli spec\"}\r\n").unwrap();
            path
        } else {
            let path = tmp.join("fake-hermes.sh");
            std::fs::write(&path, "#!/bin/sh\necho '{\"spec\":\"cli spec\"}'\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&path).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&path, perms).unwrap();
            }
            path
        }
    }

    #[test]
    fn specify_updates_task_body() {
        let tmp = std::env::temp_dir().join(format!("iota-cli-kanban-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let board_id = store.create_board("dev", "Development").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Vague task".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        let bridge = AdvancedBridge::new(fake_hermes_echo_spec(&tmp), tmp.join("shadows"));
        let args = vec!["specify".to_string(), task_id.to_string()];

        let out = execute_kanban_command(&args, &store, &bridge).unwrap();

        assert!(out[0].contains("Specified task"));
        assert_eq!(
            store.get_task(task_id).unwrap().body.as_deref(),
            Some("cli spec")
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn export_and_import_events_round_trip() {
        let tmp = std::env::temp_dir().join(format!("iota-cli-kanban-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let board_id = source.create_board("dev", "Development").unwrap();
        let task_id = source
            .create_task(CreateTaskRequest {
                board_id,
                title: "Exported task".to_string(),
                body: None,
                status: Some(Status::Todo),
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        let bridge = AdvancedBridge::new(fake_hermes_echo_spec(&tmp), tmp.join("shadows"));
        let bundle_path = tmp.join("events.json");

        let export_out = execute_kanban_command(
            &["export".to_string(), bundle_path.display().to_string()],
            &source,
            &bridge,
        )
        .unwrap();
        assert!(export_out[0].contains("Exported"));

        let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
        let import_out = execute_kanban_command(
            &["import".to_string(), bundle_path.display().to_string()],
            &target,
            &bridge,
        )
        .unwrap();

        assert!(import_out[0].contains("Imported"));
        assert_eq!(target.get_task(task_id).unwrap().title, "Exported task");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
