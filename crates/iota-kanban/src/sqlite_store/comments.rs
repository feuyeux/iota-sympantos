//! Comment CRUD for [`super::SqliteKanbanStore`].

use anyhow::Result;
use rusqlite::params;

use crate::types::{Comment, CommentId, TaskId};
use crate::utils::now_ts;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn add_comment_on_conn(
        conn: &rusqlite::Connection,
        task_id: TaskId,
        author: &str,
        body: &str,
    ) -> Result<CommentId> {
        let now = now_ts();
        conn.execute(
            "INSERT INTO comments (task_id, author, body, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![task_id as i64, author, body, now],
        )?;
        Ok(conn.last_insert_rowid() as u64)
    }

    pub(super) fn list_comments_impl(&self, task_id: TaskId) -> Result<Vec<Comment>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, author, body, created_at
             FROM comments WHERE task_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![task_id as i64], row_to_comment)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

pub(super) fn row_to_comment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: row.get::<_, i64>(0)? as u64,
        task_id: row.get::<_, i64>(1)? as u64,
        author: row.get(2)?,
        body: row.get(3)?,
        created_at: row.get(4)?,
    })
}
