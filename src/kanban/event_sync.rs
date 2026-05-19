use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{EventId, KanbanEvent, KanbanStore, SqliteKanbanStore};

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanEventBundle {
    pub format_version: u32,
    pub source: String,
    pub cursor: EventId,
    pub events: Vec<KanbanEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventImportReport {
    pub source: String,
    pub events_seen: usize,
    pub events_applied: usize,
    pub events_skipped: usize,
    pub cursor: EventId,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum EventSyncRequest {
    EventsSince { cursor: EventId, source: String },
    ImportBundle { bundle: KanbanEventBundle },
}

#[derive(Debug, Serialize, Deserialize)]
struct EventSyncResponse {
    ok: bool,
    bundle: Option<KanbanEventBundle>,
    report: Option<EventImportReport>,
    error: Option<String>,
}

pub fn export_event_bundle(
    store: &dyn KanbanStore,
    cursor: EventId,
    source: impl Into<String>,
) -> Result<KanbanEventBundle> {
    let events = store.events_since(cursor)?;
    let next_cursor = events.last().map(|event| event.id).unwrap_or(cursor);
    Ok(KanbanEventBundle {
        format_version: FORMAT_VERSION,
        source: source.into(),
        cursor: next_cursor,
        events,
    })
}

pub fn write_event_bundle(path: &Path, bundle: &KanbanEventBundle) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating kanban event bundle dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(bundle)?;
    fs::write(path, json).with_context(|| format!("writing kanban event bundle {}", path.display()))
}

pub fn read_event_bundle(path: &Path) -> Result<KanbanEventBundle> {
    let bytes = fs::read(path)
        .with_context(|| format!("reading kanban event bundle {}", path.display()))?;
    let bundle: KanbanEventBundle = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing kanban event bundle {}", path.display()))?;
    anyhow::ensure!(
        bundle.format_version == FORMAT_VERSION,
        "unsupported kanban event bundle version: {}",
        bundle.format_version
    );
    Ok(bundle)
}

pub fn import_event_bundle(
    store: &SqliteKanbanStore,
    bundle: &KanbanEventBundle,
) -> Result<EventImportReport> {
    anyhow::ensure!(
        bundle.format_version == FORMAT_VERSION,
        "unsupported kanban event bundle version: {}",
        bundle.format_version
    );
    let events_seen = bundle.events.len();
    let stored_cursor = store.sync_cursor(&bundle.source)?;
    let new_events: Vec<KanbanEvent> = bundle
        .events
        .iter()
        .filter(|event| event.id > stored_cursor)
        .cloned()
        .collect();
    let events_skipped = events_seen.saturating_sub(new_events.len());
    let events_applied = store.replay_events_strict(&new_events)?;
    for event in &new_events {
        store.append_event(&event.event_type, &event.payload)?;
    }
    store.set_sync_cursor(&bundle.source, bundle.cursor)?;
    Ok(EventImportReport {
        source: bundle.source.clone(),
        events_seen,
        events_applied,
        events_skipped,
        cursor: bundle.cursor,
    })
}

pub fn default_pull_source(addr: &str) -> String {
    let trimmed = addr.trim();
    let source = if trimmed.is_empty() { "unknown" } else { trimmed };
    format!("peer:{source}")
}

pub fn serve_event_sync<A: ToSocketAddrs>(store: Arc<SqliteKanbanStore>, addr: A) -> Result<()> {
    let listener = TcpListener::bind(addr).context("binding kanban event sync listener")?;
    for stream in listener.incoming() {
        let stream = stream.context("accepting kanban event sync connection")?;
        handle_event_sync_stream(store.as_ref(), stream)?;
    }
    Ok(())
}

pub fn pull_event_bundle<A: ToSocketAddrs>(
    addr: A,
    cursor: EventId,
    source: impl Into<String>,
) -> Result<KanbanEventBundle> {
    let request = EventSyncRequest::EventsSince {
        cursor,
        source: source.into(),
    };
    let response = send_event_sync_request(addr, &request)?;
    if response.ok {
        response
            .bundle
            .context("kanban sync peer did not return an event bundle")
    } else {
        anyhow::bail!(
            "kanban sync pull failed: {}",
            response
                .error
                .unwrap_or_else(|| "unknown error".to_string())
        )
    }
}

pub fn push_event_bundle<A: ToSocketAddrs>(
    addr: A,
    bundle: KanbanEventBundle,
) -> Result<EventImportReport> {
    let response = send_event_sync_request(addr, &EventSyncRequest::ImportBundle { bundle })?;
    if response.ok {
        response
            .report
            .context("kanban sync peer did not return an import report")
    } else {
        anyhow::bail!(
            "kanban sync push failed: {}",
            response
                .error
                .unwrap_or_else(|| "unknown error".to_string())
        )
    }
}

fn send_event_sync_request<A: ToSocketAddrs>(
    addr: A,
    request: &EventSyncRequest,
) -> Result<EventSyncResponse> {
    let mut stream = TcpStream::connect(addr).context("connecting to kanban sync peer")?;
    let request_json = serde_json::to_string(request)?;
    stream.write_all(request_json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).context("parsing kanban sync peer response")
}

fn handle_event_sync_stream(store: &SqliteKanbanStore, mut stream: TcpStream) -> Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let response = match serde_json::from_str::<EventSyncRequest>(&line) {
        Ok(request) => handle_event_sync_request(store, request),
        Err(err) => EventSyncResponse {
            ok: false,
            bundle: None,
            report: None,
            error: Some(format!("invalid request: {err}")),
        },
    };
    let response_json = serde_json::to_string(&response)?;
    stream.write_all(response_json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn handle_event_sync_request(
    store: &SqliteKanbanStore,
    request: EventSyncRequest,
) -> EventSyncResponse {
    match request {
        EventSyncRequest::EventsSince { cursor, source } => {
            match export_event_bundle(store, cursor, source) {
                Ok(bundle) => EventSyncResponse {
                    ok: true,
                    bundle: Some(bundle),
                    report: None,
                    error: None,
                },
                Err(err) => EventSyncResponse {
                    ok: false,
                    bundle: None,
                    report: None,
                    error: Some(err.to_string()),
                },
            }
        }
        EventSyncRequest::ImportBundle { bundle } => match import_event_bundle(store, &bundle) {
            Ok(report) => EventSyncResponse {
                ok: true,
                bundle: None,
                report: Some(report),
                error: None,
            },
            Err(err) => EventSyncResponse {
                ok: false,
                bundle: None,
                report: None,
                error: Some(err.to_string()),
            },
        },
    }
}

#[cfg(test)]
fn serve_event_sync_until_shutdown(
    store: Arc<SqliteKanbanStore>,
    listener: TcpListener,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kanban::{CreateTaskRequest, Status};

    fn store() -> SqliteKanbanStore {
        SqliteKanbanStore::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn event_bundle_round_trips_state_with_stable_ids() {
        let source = store();
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
        let target = store();
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
        let source = store();
        source.create_board("dev", "Development").unwrap();
        let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
        let target = store();

        let first = import_event_bundle(&target, &bundle).unwrap();
        let second = import_event_bundle(&target, &bundle).unwrap();

        assert_eq!(first.events_applied, 1);
        assert_eq!(second.events_applied, 0);
        assert_eq!(second.events_skipped, 1);
    }

    #[test]
    fn failed_import_does_not_advance_source_cursor() {
        let target = store();
        let bundle = KanbanEventBundle {
            format_version: FORMAT_VERSION,
            source: "node-a".to_string(),
            cursor: 2,
            events: vec![KanbanEvent {
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

        let err = import_event_bundle(&target, &bundle).unwrap_err();

        assert!(
            err.to_string().contains("applying kanban event 2"),
            "expected strict replay context in error, got: {err}"
        );
        assert_eq!(target.sync_cursor("node-a").unwrap(), 0);
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
        let target = store();

        target.set_sync_cursor(&default_pull_source("127.0.0.1:47662"), 10).unwrap();

        assert_eq!(target.sync_cursor(&default_pull_source("127.0.0.1:47662")).unwrap(), 10);
        assert_eq!(target.sync_cursor(&default_pull_source("127.0.0.1:47663")).unwrap(), 0);
    }

    #[test]
    fn imported_events_are_re_exportable_for_multihop_sync() {
        let source = store();
        source.create_board("dev", "Development").unwrap();
        let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
        let relay = store();
        import_event_bundle(&relay, &bundle).unwrap();

        let forwarded = export_event_bundle(&relay, 0, "node-b").unwrap();

        assert_eq!(forwarded.events.len(), 1);
        assert_eq!(forwarded.events[0].event_type, "board_created");
    }

    #[test]
    fn tcp_sync_pull_and_push_round_trip() {
        let remote = Arc::new(store());
        remote.create_board("remote", "Remote").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_store = remote.clone();
        let server_shutdown = shutdown.clone();
        let handle = std::thread::spawn(move || {
            serve_event_sync_until_shutdown(server_store, listener, server_shutdown).unwrap();
        });

        let local = store();
        let pulled = pull_event_bundle(addr, 0, "remote-node").unwrap();
        import_event_bundle(&local, &pulled).unwrap();
        assert_eq!(local.get_board("remote").unwrap().name, "Remote");

        local.create_board("local", "Local").unwrap();
        let outgoing = export_event_bundle(&local, 0, "local-node").unwrap();
        let report = push_event_bundle(addr, outgoing).unwrap();
        assert!(report.events_applied > 0);
        assert_eq!(remote.get_board("local").unwrap().name, "Local");

        shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(addr);
        handle.join().unwrap();
    }

    #[test]
    fn write_and_read_event_bundle_file() {
        let source = store();
        source.create_board("dev", "Development").unwrap();
        let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
        let tmp =
            std::env::temp_dir().join(format!("iota-kanban-events-{}.json", uuid::Uuid::new_v4()));

        write_event_bundle(&tmp, &bundle).unwrap();
        let loaded = read_event_bundle(&tmp).unwrap();

        assert_eq!(loaded.source, "node-a");
        assert_eq!(loaded.events.len(), 1);
        let _ = fs::remove_file(tmp);
    }
}
