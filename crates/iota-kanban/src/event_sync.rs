use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;

/// Read/write timeout for each event-sync TCP connection.
const EVENT_SYNC_IO_TIMEOUT_SECS: u64 = 30;
const MAX_EVENT_SYNC_MESSAGE_BYTES: usize = 10 * 1024 * 1024;
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
    anyhow::ensure!(
        cursor <= i64::MAX as u64,
        "kanban export cursor exceeds SQLite integer range"
    );
    let source = source.into();
    anyhow::ensure!(
        !source.trim().is_empty() && source.len() <= MAX_EVENT_SYNC_SOURCE_BYTES,
        "kanban event bundle source must be 1..={} bytes",
        MAX_EVENT_SYNC_SOURCE_BYTES
    );
    let events = store.events_since(cursor)?;
    let next_cursor = events.last().map(|event| event.id).unwrap_or(cursor);
    Ok(KanbanEventBundle {
        format_version: FORMAT_VERSION,
        source,
        cursor: next_cursor,
        events,
    })
}

pub fn write_event_bundle(path: &Path, bundle: &KanbanEventBundle) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating kanban event bundle dir {}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(bundle)?;
    anyhow::ensure!(
        json.len() <= MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban event bundle exceeds {} byte limit",
        MAX_EVENT_SYNC_MESSAGE_BYTES
    );
    fs::write(path, json).with_context(|| format!("writing kanban event bundle {}", path.display()))
}

pub fn read_event_bundle(path: &Path) -> Result<KanbanEventBundle> {
    let file = fs::File::open(path)
        .with_context(|| format!("opening kanban event bundle {}", path.display()))?;
    let size = file
        .metadata()
        .with_context(|| format!("reading kanban event bundle metadata {}", path.display()))?
        .len();
    anyhow::ensure!(
        size <= MAX_EVENT_SYNC_MESSAGE_BYTES as u64,
        "kanban event bundle exceeds {} byte limit",
        MAX_EVENT_SYNC_MESSAGE_BYTES
    );
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_EVENT_SYNC_MESSAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading kanban event bundle {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() <= MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban event bundle exceeds {} byte limit",
        MAX_EVENT_SYNC_MESSAGE_BYTES
    );
    let bundle: KanbanEventBundle = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing kanban event bundle {}", path.display()))?;
    anyhow::ensure!(
        bundle.format_version == FORMAT_VERSION,
        "unsupported kanban event bundle version: {}",
        bundle.format_version
    );
    Ok(bundle)
}

const MAX_EVENT_SYNC_SOURCE_BYTES: usize = 256;

fn validate_event_bundle(bundle: &KanbanEventBundle, stored_cursor: u64) -> Result<()> {
    anyhow::ensure!(
        !bundle.source.trim().is_empty()
            && bundle.source.len() <= MAX_EVENT_SYNC_SOURCE_BYTES,
        "kanban event bundle source must be 1..={} bytes",
        MAX_EVENT_SYNC_SOURCE_BYTES
    );
    anyhow::ensure!(
        bundle.cursor <= i64::MAX as u64,
        "kanban event bundle cursor exceeds SQLite integer range"
    );

    let mut previous = None;
    for event in &bundle.events {
        anyhow::ensure!(
            event.id > 0 && event.id <= i64::MAX as u64,
            "kanban event id {} is outside SQLite integer range",
            event.id
        );
        if let Some(previous) = previous {
            anyhow::ensure!(
                event.id > previous,
                "kanban event ids must be strictly increasing"
            );
        }
        previous = Some(event.id);
    }

    if let Some(last) = bundle.events.last() {
        anyhow::ensure!(
            bundle.cursor == last.id,
            "kanban event bundle cursor {} does not match last event {}",
            bundle.cursor,
            last.id
        );
    } else {
        anyhow::ensure!(
            bundle.cursor <= stored_cursor,
            "empty kanban event bundle cannot advance cursor"
        );
    }

    let mut previous_new = stored_cursor;
    for event in bundle.events.iter().filter(|event| event.id > stored_cursor) {
        let expected = previous_new
            .checked_add(1)
            .context("kanban sync cursor overflow")?;
        anyhow::ensure!(
            event.id == expected,
            "kanban event gap: expected {}, received {}",
            expected,
            event.id
        );
        previous_new = event.id;
    }
    Ok(())
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
    anyhow::ensure!(
        !bundle.source.trim().is_empty()
            && bundle.source.len() <= MAX_EVENT_SYNC_SOURCE_BYTES,
        "kanban event bundle source must be 1..={} bytes",
        MAX_EVENT_SYNC_SOURCE_BYTES
    );
    let events_seen = bundle.events.len();
    let stored_cursor = store.sync_cursor(&bundle.source)?;
    validate_event_bundle(bundle, stored_cursor)?;
    let new_events: Vec<KanbanEvent> = bundle
        .events
        .iter()
        .filter(|event| event.id > stored_cursor)
        .cloned()
        .collect();
    let events_skipped = events_seen.saturating_sub(new_events.len());
    let committed_cursor = new_events
        .last()
        .map(|event| event.id)
        .unwrap_or(stored_cursor);
    let events_applied =
        store.import_event_bundle_atomic(&new_events, &bundle.source, committed_cursor)?;
    Ok(EventImportReport {
        source: bundle.source.clone(),
        events_seen,
        events_applied,
        events_skipped,
        cursor: committed_cursor,
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
    let bind_addr = addr
        .to_socket_addrs()
        .context("resolving kanban event sync address")?
        .next()
        .context("kanban event sync address did not resolve")?;
    anyhow::ensure!(
        bind_addr.ip().is_loopback(),
        "refusing to expose unauthenticated kanban sync outside loopback"
    );
    let listener = TcpListener::bind(bind_addr).context("binding kanban event sync listener")?;
    for stream in listener.incoming() {
        let stream = stream.context("accepting kanban event sync connection")?;
        // Guard against a slow/hung peer blocking the server thread indefinitely.
        let timeout = Some(std::time::Duration::from_secs(EVENT_SYNC_IO_TIMEOUT_SECS));
        let _ = stream.set_read_timeout(timeout);
        let _ = stream.set_write_timeout(timeout);
        if let Err(error) = handle_event_sync_stream(store.as_ref(), stream) {
            eprintln!("kanban sync connection failed: {error:#}");
        }
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
    let timeout = std::time::Duration::from_secs(EVENT_SYNC_IO_TIMEOUT_SECS);
    let mut last_error = None;
    let mut connected = None;
    for peer in addr
        .to_socket_addrs()
        .context("resolving kanban sync peer")?
    {
        match TcpStream::connect_timeout(&peer, timeout) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let mut stream = connected.with_context(|| match last_error {
        Some(error) => format!("connecting to kanban sync peer: {error}"),
        None => "kanban sync peer address did not resolve".to_string(),
    })?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let request_json = serde_json::to_vec(request)?;
    anyhow::ensure!(
        request_json.len() < MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban sync request exceeded {} byte limit",
        MAX_EVENT_SYNC_MESSAGE_BYTES
    );
    stream.write_all(&request_json)?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let line = read_limited_line(BufReader::new(stream))?;
    serde_json::from_str(&line).context("parsing kanban sync peer response")
}

fn handle_event_sync_stream(store: &SqliteKanbanStore, mut stream: TcpStream) -> Result<()> {
    let line = read_limited_line(BufReader::new(stream.try_clone()?))?;
    let response = match serde_json::from_str::<EventSyncRequest>(&line) {
        Ok(request) => handle_event_sync_request(store, request),
        Err(err) => EventSyncResponse {
            ok: false,
            bundle: None,
            report: None,
            error: Some(format!("invalid request: {err}")),
        },
    };
    let mut response_json = serde_json::to_vec(&response)?;
    if response_json.len() >= MAX_EVENT_SYNC_MESSAGE_BYTES {
        response_json = serde_json::to_vec(&EventSyncResponse {
            ok: false,
            bundle: None,
            report: None,
            error: Some("kanban sync response exceeded message limit".to_string()),
        })?;
    }
    stream.write_all(&response_json)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    // Graceful half-close: signal EOF to the client so it can finish reading
    // before the OS drops the connection.
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

fn read_limited_line<R: BufRead>(mut reader: R) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_EVENT_SYNC_MESSAGE_BYTES as u64 + 1)
        .read_until(b'\n', &mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban sync message exceeded {} byte limit",
        MAX_EVENT_SYNC_MESSAGE_BYTES
    );
    String::from_utf8(bytes).context("kanban sync message was not valid UTF-8")
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
