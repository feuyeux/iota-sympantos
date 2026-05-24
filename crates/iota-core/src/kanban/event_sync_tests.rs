use anyhow::Context;
use crate::kanban::event_sync::{
    default_pull_source, export_event_bundle, handle_event_sync_stream,
    import_event_bundle, pull_event_bundle, push_event_bundle, read_event_bundle,
    write_event_bundle, KanbanEventBundle,
};
use crate::kanban::{CreateTaskRequest, SqliteKanbanStore, KanbanStore, Status};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn serve_event_sync_until_shutdown(
    store: Arc<SqliteKanbanStore>,
    listener: std::net::TcpListener,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => handle_event_sync_stream(store.as_ref(), stream)?,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => return Err(err).context("accepting kanban sync connection"),
        }
    }
    Ok(())
}

#[test]
fn event_bundle_round_trips_state_with_stable_ids() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = source.create_board("dev", "Development").unwrap();
    let task_id = source
        .create_task(CreateTaskRequest {
            board_id,
            title: "Sync me".to_string(),
            body: Some("body".to_string()),
            status: Some(Status::Todo),
            assignee: Some("alice".to_string()),
            priority: Some(7),
            tags: vec!["sync".to_string()],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    source.add_comment(task_id, "bob", "comment").unwrap();

    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let report = import_event_bundle(&target, &bundle).unwrap();

    assert_eq!(report.source, "node-a");
    assert_eq!(report.events_seen, bundle.events.len());
    assert_eq!(report.events_skipped, 0);
    assert_eq!(target.get_board("dev").unwrap().id, board_id);
    assert_eq!(target.get_task(task_id).unwrap().title, "Sync me");
    assert_eq!(target.list_comments(task_id).unwrap()[0].body, "comment");
}

#[test]
fn import_skips_events_at_or_below_stored_source_cursor() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();

    let first = import_event_bundle(&target, &bundle).unwrap();
    let second = import_event_bundle(&target, &bundle).unwrap();

    assert_eq!(first.events_applied, 1);
    assert_eq!(second.events_applied, 0);
    assert_eq!(second.events_skipped, 1);
}

#[test]
fn unapplicable_event_is_skipped_and_cursor_still_advances() {
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let bundle = KanbanEventBundle {
        format_version: 1,
        source: "node-a".to_string(),
        cursor: 2,
        events: vec![crate::kanban::KanbanEvent {
            id: 2,
            event_type: "task_updated".to_string(),
            payload: serde_json::json!({
                "task_id": 999,
                "patch": {
                    "title": "missing"
                }
            })
            .to_string(),
            created_at: 0,
        }],
    };

    let report = import_event_bundle(&target, &bundle).unwrap();
    assert_eq!(report.events_seen, 1);
    assert_eq!(report.events_applied, 0);
    assert_eq!(target.sync_cursor("node-a").unwrap(), 2);
}

#[test]
fn default_pull_source_is_stable_per_peer_addr() {
    assert_eq!(
        default_pull_source("127.0.0.1:47662"),
        "peer:127.0.0.1:47662"
    );
    assert_ne!(
        default_pull_source("127.0.0.1:47662"),
        default_pull_source("127.0.0.1:47663")
    );
}

#[test]
fn import_cursors_are_isolated_by_peer_source() {
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();

    target
        .set_sync_cursor(&default_pull_source("127.0.0.1:47662"), 10)
        .unwrap();

    assert_eq!(
        target
            .sync_cursor(&default_pull_source("127.0.0.1:47662"))
            .unwrap(),
        10
    );
    assert_eq!(
        target
            .sync_cursor(&default_pull_source("127.0.0.1:47663"))
            .unwrap(),
        0
    );
}

#[test]
fn imported_events_are_re_exportable_for_multihop_sync() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let relay = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    import_event_bundle(&relay, &bundle).unwrap();

    let forwarded = export_event_bundle(&relay, 0, "node-b").unwrap();

    assert_eq!(forwarded.events.len(), 1);
    assert_eq!(forwarded.events[0].event_type, "board_created");
}

#[test]
fn tcp_sync_pull_and_push_round_trip() {
    let remote = Arc::new(SqliteKanbanStore::open(Path::new(":memory:")).unwrap());
    remote.create_board("remote", "Remote").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_store = remote.clone();
    let server_shutdown = shutdown.clone();
    let handle = std::thread::spawn(move || {
        serve_event_sync_until_shutdown(server_store, listener, server_shutdown).unwrap();
    });

    let local = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let pulled = pull_event_bundle(addr, 0, "remote-node").unwrap();
    import_event_bundle(&local, &pulled).unwrap();
    assert_eq!(local.get_board("remote").unwrap().name, "Remote");

    local.create_board("local", "Local").unwrap();
    let outgoing = export_event_bundle(&local, 0, "local-node").unwrap();
    let report = push_event_bundle(addr, outgoing).unwrap();
    assert!(report.events_applied > 0);
    assert_eq!(remote.get_board("local").unwrap().name, "Local");

    shutdown.store(true, Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(addr);
    handle.join().unwrap();
}

#[test]
fn write_and_read_event_bundle_file() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let tmp =
        std::env::temp_dir().join(format!("iota-kanban-events-{}.json", uuid::Uuid::new_v4()));

    write_event_bundle(&tmp, &bundle).unwrap();
    let loaded = read_event_bundle(&tmp).unwrap();

    assert_eq!(loaded.source, "node-a");
    assert_eq!(loaded.events.len(), 1);
    let _ = std::fs::remove_file(tmp);
}