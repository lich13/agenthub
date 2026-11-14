use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;

use super::{thread_rows::table_columns, CodexPaths};

pub fn set_thread_archived(paths: &CodexPaths, id: &str, archived: bool) -> Result<()> {
    let db = paths.state_db();
    let conn = Connection::open(&db).with_context(|| format!("open {}", db.display()))?;
    let columns = table_columns(&conn, "threads")?;
    if !columns.contains("archived_at") {
        anyhow::bail!("threads.archived_at column not found");
    }
    let archived_at = if archived {
        Some(Utc::now().timestamp())
    } else {
        None
    };
    if columns.contains("archived") {
        conn.execute(
            "UPDATE threads SET archived=?2, archived_at=?3 WHERE id=?1",
            rusqlite::params![id, if archived { 1 } else { 0 }, archived_at],
        )?;
    } else {
        conn.execute(
            "UPDATE threads SET archived_at=?2 WHERE id=?1",
            rusqlite::params![id, archived_at],
        )?;
    }
    Ok(())
}
