use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::Connection;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use super::{
    rollout_events::{
        choose_initial_thread_title, is_hidden_thread_metadata, is_internal_thread_metadata,
        is_subagent_metadata, source_text_contains_subagent, thread_source_label,
        ThreadVisibilityMetadata,
    },
    CodexPaths, ThreadStatus, ThreadSummary,
};

pub fn hidden_thread_ids(paths: &CodexPaths) -> Result<HashSet<String>> {
    let db = paths.state_db();
    if !db.exists() {
        return Ok(HashSet::new());
    }
    let conn = Connection::open(&db).with_context(|| format!("open {}", db.display()))?;
    let has_threads = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='threads'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_threads {
        return Ok(HashSet::new());
    }

    let columns = table_columns(&conn, "threads")?;
    let thread_source_expr = first_existing(&columns, &["thread_source", "source_kind"])
        .or_else(|| first_existing(&columns, &["source"]))
        .unwrap_or("NULL")
        .to_string();
    let source_expr = first_existing(&columns, &["source"])
        .unwrap_or("NULL")
        .to_string();
    let parent_thread_expr = first_existing(&columns, &["parent_thread_id", "parentThreadId"])
        .unwrap_or("NULL")
        .to_string();
    let agent_path_expr = first_existing(&columns, &["agent_path", "agentPath"])
        .unwrap_or("NULL")
        .to_string();
    let agent_nickname_expr = first_existing(&columns, &["agent_nickname", "agentNickname"])
        .unwrap_or("NULL")
        .to_string();
    let agent_role_expr = first_existing(&columns, &["agent_role", "agentRole"])
        .unwrap_or("NULL")
        .to_string();
    let has_user_event_expr = first_existing(&columns, &["has_user_event"])
        .unwrap_or("NULL")
        .to_string();
    let title_expr = first_existing(&columns, &["title", "name"])
        .unwrap_or("NULL")
        .to_string();
    let first_user_message_expr = first_existing(&columns, &["first_user_message"])
        .unwrap_or("NULL")
        .to_string();
    let preview_expr = first_existing(&columns, &["preview"])
        .unwrap_or("NULL")
        .to_string();
    let sql = format!(
        "SELECT id, {thread_source_expr}, {source_expr}, {parent_thread_expr}, {agent_path_expr}, {agent_nickname_expr}, {agent_role_expr}, {has_user_event_expr}, {title_expr}, {first_user_message_expr}, {preview_expr} FROM threads"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let thread_source: Option<String> = row.get(1).ok();
        let source: Option<String> = row.get(2).ok();
        let parent_thread_id: Option<String> = row.get(3).ok();
        let agent_path: Option<String> = row.get(4).ok();
        let agent_nickname: Option<String> = row.get(5).ok();
        let agent_role: Option<String> = row.get(6).ok();
        let has_user_event: Option<i64> = row.get(7).ok();
        let title: Option<String> = row.get(8).ok();
        let first_user_message: Option<String> = row.get(9).ok();
        let preview: Option<String> = row.get(10).ok();
        let hidden = is_hidden_thread_metadata(ThreadVisibilityMetadata {
            thread_source: thread_source.as_deref(),
            source: source.as_deref(),
            parent_thread_id: parent_thread_id.as_deref(),
            agent_path: agent_path.as_deref(),
            agent_nickname: agent_nickname.as_deref(),
            agent_role: agent_role.as_deref(),
            has_user_event,
            title: title.as_deref(),
            first_user_message: first_user_message.as_deref(),
            preview: preview.as_deref(),
        });
        Ok(hidden.then_some(id))
    })?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

pub fn archived_thread_ids(paths: &CodexPaths) -> Result<HashSet<String>> {
    let db = paths.state_db();
    if !db.exists() {
        return Ok(HashSet::new());
    }
    let conn = Connection::open(&db).with_context(|| format!("open {}", db.display()))?;
    let has_threads = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='threads'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_threads {
        return Ok(HashSet::new());
    }

    let columns = table_columns(&conn, "threads")?;
    let archived_expr = first_existing(&columns, &["archived"])
        .unwrap_or("0")
        .to_string();
    let archived_at_expr = first_existing(&columns, &["archived_at"])
        .unwrap_or("NULL")
        .to_string();
    let sql = format!("SELECT id, {archived_expr}, {archived_at_expr} FROM threads");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let archived_flag: i64 = row.get(1).unwrap_or(0);
        let archived_at: Option<ValueCell> = row.get(2).ok();
        let archived =
            archived_flag != 0 || archived_at.as_ref().and_then(format_cell_time).is_some();
        Ok(archived.then_some(id))
    })?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

pub fn thread_source_counts(paths: &CodexPaths) -> Result<HashMap<String, usize>> {
    let db = paths.state_db();
    if !db.exists() {
        return Ok(HashMap::new());
    }
    let conn = Connection::open(&db).with_context(|| format!("open {}", db.display()))?;
    let has_threads = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='threads'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_threads {
        return Ok(HashMap::new());
    }

    let columns = table_columns(&conn, "threads")?;
    let thread_source_expr = first_existing(&columns, &["thread_source", "source_kind"])
        .unwrap_or("NULL")
        .to_string();
    let source_expr = first_existing(&columns, &["source"])
        .unwrap_or("NULL")
        .to_string();
    let parent_thread_expr = first_existing(&columns, &["parent_thread_id", "parentThreadId"])
        .unwrap_or("NULL")
        .to_string();
    let agent_path_expr = first_existing(&columns, &["agent_path", "agentPath"])
        .unwrap_or("NULL")
        .to_string();
    let agent_nickname_expr = first_existing(&columns, &["agent_nickname", "agentNickname"])
        .unwrap_or("NULL")
        .to_string();
    let agent_role_expr = first_existing(&columns, &["agent_role", "agentRole"])
        .unwrap_or("NULL")
        .to_string();
    let has_user_event_expr = first_existing(&columns, &["has_user_event"])
        .unwrap_or("NULL")
        .to_string();
    let title_expr = first_existing(&columns, &["title", "name"])
        .unwrap_or("NULL")
        .to_string();
    let first_user_message_expr = first_existing(&columns, &["first_user_message"])
        .unwrap_or("NULL")
        .to_string();
    let preview_expr = first_existing(&columns, &["preview"])
        .unwrap_or("NULL")
        .to_string();
    let archived_expr = first_existing(&columns, &["archived"])
        .unwrap_or("0")
        .to_string();
    let archived_at_expr = first_existing(&columns, &["archived_at"])
        .unwrap_or("NULL")
        .to_string();
    let sql = format!(
        "SELECT {thread_source_expr}, {source_expr}, {parent_thread_expr}, {agent_path_expr}, {agent_nickname_expr}, {agent_role_expr}, {has_user_event_expr}, {title_expr}, {first_user_message_expr}, {preview_expr}, {archived_expr}, {archived_at_expr} FROM threads"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let thread_source: Option<String> = row.get(0).ok();
        let source: Option<String> = row.get(1).ok();
        let parent_thread_id: Option<String> = row.get(2).ok();
        let agent_path: Option<String> = row.get(3).ok();
        let agent_nickname: Option<String> = row.get(4).ok();
        let agent_role: Option<String> = row.get(5).ok();
        let has_user_event: Option<i64> = row.get(6).ok();
        let title: Option<String> = row.get(7).ok();
        let first_user_message: Option<String> = row.get(8).ok();
        let preview: Option<String> = row.get(9).ok();
        let archived_flag: i64 = row.get(10).unwrap_or(0);
        let archived_at: Option<ValueCell> = row.get(11).ok();
        Ok((
            thread_source,
            source,
            parent_thread_id,
            agent_path,
            agent_nickname,
            agent_role,
            has_user_event,
            title,
            first_user_message,
            preview,
            archived_flag,
            archived_at,
        ))
    })?;
    let mut counts = HashMap::new();
    for row in rows {
        let (
            thread_source,
            source,
            parent_thread_id,
            agent_path,
            agent_nickname,
            agent_role,
            has_user_event,
            title,
            first_user_message,
            preview,
            archived_flag,
            archived_at,
        ) = row?;
        let metadata = ThreadVisibilityMetadata {
            thread_source: thread_source.as_deref(),
            source: source.as_deref(),
            parent_thread_id: parent_thread_id.as_deref(),
            agent_path: agent_path.as_deref(),
            agent_nickname: agent_nickname.as_deref(),
            agent_role: agent_role.as_deref(),
            has_user_event,
            title: title.as_deref(),
            first_user_message: first_user_message.as_deref(),
            preview: preview.as_deref(),
        };
        let key = if is_internal_thread_metadata(metadata) {
            "internal".to_string()
        } else if is_subagent_metadata(
            thread_source.as_deref(),
            parent_thread_id.as_deref(),
            agent_path.as_deref(),
            agent_nickname.as_deref(),
            agent_role.as_deref(),
        ) || source.as_deref().is_some_and(source_text_contains_subagent)
        {
            "subagent".to_string()
        } else {
            thread_source_label(thread_source.as_deref().or(source.as_deref()))
        };
        *counts.entry(key).or_insert(0) += 1;
        if archived_flag != 0 || archived_at.as_ref().and_then(format_cell_time).is_some() {
            *counts.entry("archived".to_string()).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

pub(super) struct LocalThreadRow {
    pub(super) summary: ThreadSummary,
    pub(super) db_title: Option<String>,
    pub(super) first_user_message: Option<String>,
}

fn thread_title_expression(columns: &HashSet<String>) -> String {
    match (columns.contains("name"), columns.contains("title")) {
        (true, true) if columns.contains("history_mode") => {
            "CASE WHEN history_mode = 'legacy' THEN title ELSE COALESCE(NULLIF(TRIM(name), ''), title) END"
        }
        (true, true) => "COALESCE(NULLIF(TRIM(name), ''), title)",
        (true, false) => "name",
        (false, true) => "title",
        (false, false) => "NULL",
    }
    .to_string()
}

pub(super) fn read_thread_rows(paths: &CodexPaths) -> Result<Vec<LocalThreadRow>> {
    read_thread_rows_matching(paths, None)
}

pub(super) fn read_thread_rows_matching(
    paths: &CodexPaths,
    id: Option<&str>,
) -> Result<Vec<LocalThreadRow>> {
    let db = paths.state_db();
    if !db.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open(&db).with_context(|| format!("open {}", db.display()))?;
    let has_threads = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='threads'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_threads {
        return Ok(Vec::new());
    }

    let columns = table_columns(&conn, "threads")?;
    let title_expr = thread_title_expression(&columns);
    let updated_expr = first_existing(&columns, &["updated_at", "last_activity_at", "created_at"])
        .unwrap_or("NULL")
        .to_string();
    let archived_expr = first_existing(&columns, &["archived_at"])
        .unwrap_or("NULL")
        .to_string();
    let archived_flag_expr = first_existing(&columns, &["archived"])
        .unwrap_or("0")
        .to_string();
    let rollout_expr = first_existing(&columns, &["rollout_path"])
        .unwrap_or("NULL")
        .to_string();
    let cwd_expr = first_existing(&columns, &["cwd"])
        .unwrap_or("NULL")
        .to_string();
    let model_expr = first_existing(&columns, &["model"])
        .unwrap_or("NULL")
        .to_string();
    let thread_source_expr = first_existing(&columns, &["thread_source", "source_kind"])
        .or_else(|| first_existing(&columns, &["source"]))
        .unwrap_or("NULL")
        .to_string();
    let source_expr = first_existing(&columns, &["source"])
        .unwrap_or("NULL")
        .to_string();
    let parent_thread_expr = first_existing(&columns, &["parent_thread_id", "parentThreadId"])
        .unwrap_or("NULL")
        .to_string();
    let agent_path_expr = first_existing(&columns, &["agent_path", "agentPath"])
        .unwrap_or("NULL")
        .to_string();
    let agent_nickname_expr = first_existing(&columns, &["agent_nickname", "agentNickname"])
        .unwrap_or("NULL")
        .to_string();
    let agent_role_expr = first_existing(&columns, &["agent_role", "agentRole"])
        .unwrap_or("NULL")
        .to_string();
    let has_user_event_expr = first_existing(&columns, &["has_user_event"])
        .unwrap_or("NULL")
        .to_string();
    let first_user_message_expr = first_existing(&columns, &["first_user_message"])
        .unwrap_or("NULL")
        .to_string();
    let preview_expr = first_existing(&columns, &["preview"])
        .unwrap_or("NULL")
        .to_string();
    let sql = format!(
        "SELECT id, {title_expr}, {updated_expr}, {archived_expr}, {archived_flag_expr}, {rollout_expr}, {cwd_expr}, {model_expr}, {thread_source_expr}, {source_expr}, {parent_thread_expr}, {agent_path_expr}, {agent_nickname_expr}, {agent_role_expr}, {has_user_event_expr}, {first_user_message_expr}, {preview_expr} FROM threads WHERE (?1 IS NULL OR id = ?1)"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([id], |row| {
        let id: String = row.get(0)?;
        let title: Option<String> = row.get(1).ok();
        let updated_raw: Option<ValueCell> = row.get(2).ok();
        let archived_raw: Option<ValueCell> = row.get(3).ok();
        let archived_flag: i64 = row.get(4).unwrap_or(0);
        let rollout_path: Option<String> = row.get(5).ok();
        let cwd: Option<String> = row.get(6).ok();
        let model: Option<String> = row.get(7).ok();
        let thread_source: Option<String> = row.get(8).ok();
        let source: Option<String> = row.get(9).ok();
        let parent_thread_id: Option<String> = row.get(10).ok();
        let agent_path: Option<String> = row.get(11).ok();
        let agent_nickname: Option<String> = row.get(12).ok();
        let agent_role: Option<String> = row.get(13).ok();
        let has_user_event: Option<i64> = row.get(14).ok();
        let first_user_message: Option<String> = row.get(15).ok();
        let preview: Option<String> = row.get(16).ok();
        if is_hidden_thread_metadata(ThreadVisibilityMetadata {
            thread_source: thread_source.as_deref(),
            source: source.as_deref(),
            parent_thread_id: parent_thread_id.as_deref(),
            agent_path: agent_path.as_deref(),
            agent_nickname: agent_nickname.as_deref(),
            agent_role: agent_role.as_deref(),
            has_user_event,
            title: title.as_deref(),
            first_user_message: first_user_message.as_deref(),
            preview: preview.as_deref(),
        }) {
            return Ok(None);
        }
        let archived_at = archived_raw.as_ref().and_then(format_cell_time);
        let status = if archived_at.is_some() || archived_flag != 0 {
            ThreadStatus::Archived
        } else {
            ThreadStatus::Recent
        };
        Ok(Some(LocalThreadRow {
            summary: ThreadSummary {
                activity_updated_at: updated_raw.as_ref().and_then(format_cell_time),
                storage_size: None,
                id,
                title: choose_initial_thread_title(title.as_deref(), None),
                status,
                updated_at: updated_raw.as_ref().and_then(format_cell_time),
                archived_at,
                message_count: 0,
                latest_message: None,
                cwd,
                model,
                rollout_path: rollout_path
                    .filter(|p| !p.trim().is_empty())
                    .map(PathBuf::from),
                active_turn_id: None,
                active_job_id: None,
                pending_elicitation: None,
                last_event_kind: None,
            },
            db_title: title,
            first_user_message,
        }))
    })?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

#[derive(Debug)]
enum ValueCell {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

impl rusqlite::types::FromSql for ValueCell {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(match value {
            rusqlite::types::ValueRef::Null => Self::Null,
            rusqlite::types::ValueRef::Integer(v) => Self::Integer(v),
            rusqlite::types::ValueRef::Real(v) => Self::Real(v),
            rusqlite::types::ValueRef::Text(v) => {
                Self::Text(String::from_utf8_lossy(v).to_string())
            }
            rusqlite::types::ValueRef::Blob(_) => Self::Null,
        })
    }
}

fn format_cell_time(cell: &ValueCell) -> Option<String> {
    match cell {
        ValueCell::Null => None,
        ValueCell::Integer(v) => format_timestamp(*v),
        ValueCell::Real(v) => format_timestamp(*v as i64),
        ValueCell::Text(v) => {
            if v.trim().is_empty() {
                None
            } else if let Ok(n) = v.parse::<i64>() {
                format_timestamp(n)
            } else {
                Some(v.clone())
            }
        }
    }
}

fn format_timestamp(value: i64) -> Option<String> {
    if value <= 0 {
        return None;
    }
    let seconds = if value > 10_000_000_000 {
        value / 1000
    } else {
        value
    };
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(|dt| DateTime::<Local>::from(dt).to_rfc3339())
}

pub(crate) fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}

pub(crate) fn first_existing<'a>(columns: &HashSet<String>, names: &'a [&str]) -> Option<&'a str> {
    names.iter().copied().find(|name| columns.contains(*name))
}
