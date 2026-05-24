use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;

/// Read/write timeout for each event-sync TCP connection.
const EVENT_SYNC_IO_TIMEOUT_SECS: u64 = 30;
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
    let events_applied = store.replay_events(&new_events)?;
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
    let source = if trimmed.is_empty() {
        "unknown"
    } else {
        trimmed
    };
    format!("peer:{source}")
}

pub fn serve_event_sync<A: ToSocketAddrs>(store: Arc<SqliteKanbanStore>, addr: A) -> Result<()> {
    let listener = TcpListener::bind(addr).context("binding kanban event sync listener")?;
    for stream in listener.incoming() {
        let stream = stream.context("accepting kanban event sync connection")?;
        // Guard against a slow/hung peer blocking the server thread indefinitely.
        let timeout = Some(std::time::Duration::from_secs(EVENT_SYNC_IO_TIMEOUT_SECS));
        let _ = stream.set_read_timeout(timeout);
        let _ = stream.set_write_timeout(timeout);
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
    // Graceful half-close: signal EOF to the client so it can finish reading
    // before the OS drops the connection.
    let _ = stream.shutdown(Shutdown::Write);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "event_sync_tests.rs"]
mod tests;
