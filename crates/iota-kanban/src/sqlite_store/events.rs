//! Raw event-log append/read for [`super::SqliteKanbanStore`].
//!
//! This is the low-level events table accessor used by the [`KanbanStore`]
//! trait's `append_event`/`events_since`. Event *sourcing* (replaying a
//! [`crate::types::KanbanEvent`] to rebuild state) lives in
//! [`super::apply_event`], not here.

use anyhow::Result;
use rusqlite::params;

use crate::types::{EventId, KanbanEvent};
use crate::utils::now_ts;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn append_event_impl(&self, event_type: &str, payload: &str) -> Result<EventId> {
        let now = now_ts();
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO events (event_type, payload, created_at) VALUES (?1, ?2, ?3)",
            params![event_type, payload, now],
        )?;
        Ok(conn.last_insert_rowid() as u64)
    }

    pub(super) fn events_since_impl(&self, cursor: EventId) -> Result<Vec<KanbanEvent>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, event_type, payload, created_at
             FROM events WHERE id > ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![cursor as i64], row_to_event)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

pub(super) fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<KanbanEvent> {
    Ok(KanbanEvent {
        id: row.get::<_, i64>(0)? as u64,
        event_type: row.get(1)?,
        payload: row.get(2)?,
        created_at: row.get(3)?,
    })
}
