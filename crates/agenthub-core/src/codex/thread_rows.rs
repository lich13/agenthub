use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::{collections::HashSet, path::PathBuf};

use super::{
    identity::{classify_codex_task_identity, CodexTaskIdentity},
    rollout_events::{
        choose_initial_thread_title, is_hidden_thread_metadata, ThreadVisibilityMetadata,
    },
    CodexPaths, ThreadStatus, ThreadSummary,
};

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

pub(super) fn read_thread_rows_filtered(
    paths: &CodexPaths,
    archived_filter: Option<bool>,
) -> Result<Vec<LocalThreadRow>> {
    read_thread_rows_inner(paths, None, archived_filter, true)
        .map(|rows| rows.into_iter().map(|(r, _)| r).collect())
}

pub(super) fn read_thread_rows_matching(
    paths: &CodexPaths,
    id: Option<&str>,
) -> Result<Vec<LocalThreadRow>> {
    read_thread_rows_inner(paths, id, None, true)
        .map(|rows| rows.into_iter().map(|(r, _)| r).collect())
}

pub(super) fn read_notification_thread_headers_with_identities(
    paths: &CodexPaths,
) -> Result<Vec<(ThreadSummary, CodexTaskIdentity)>> {
    Ok(read_thread_rows_inner(paths, None, Some(false), false)?
        .into_iter()
        .map(|(row, identity)| (row.summary, identity))
        .collect())
}

fn read_thread_rows_inner(
    paths: &CodexPaths,
    id: Option<&str>,
    archived_filter: Option<bool>,
    include_list_extras: bool,
) -> Result<Vec<(LocalThreadRow, CodexTaskIdentity)>> {
    let db = paths.state_db();
    match std::fs::metadata(&db) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("stat {}", db.display())),
    }
    let conn = Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {}", db.display()))?;
    let has_threads = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='threads'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
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
    let origin_expr = coalesce_non_empty(&columns, &["thread_source", "source_kind"]);
    let source_expr = coalesce_non_empty(&columns, &["source"]);
    let parent_thread_expr = coalesce_non_empty(
        &columns,
        &[
            "parent_thread_id",
            "parentThreadId",
            "attached_thread_parent_id",
            "attachedThreadParentId",
        ],
    );
    let strict_parent_expr = coalesce_non_empty(&columns, &["parent_thread_id", "parentThreadId"]);
    let agent_path_expr = coalesce_non_empty(&columns, &["agent_path", "agentPath"]);
    let agent_nickname_expr = coalesce_non_empty(&columns, &["agent_nickname", "agentNickname"]);
    let agent_role_expr = coalesce_non_empty(&columns, &["agent_role", "agentRole"]);
    let first_user_message_expr = if include_list_extras {
        first_existing(&columns, &["first_user_message"]).unwrap_or("NULL")
    } else {
        "NULL"
    };
    let spawned_children = match id {
        Some(target_id) => {
            if is_spawned_child(&conn, target_id)? {
                return Ok(Vec::new());
            }
            HashSet::new()
        }
        None => read_spawn_edge_children(&conn)?,
    };
    let filter = if id.is_some() { " WHERE id = ?1" } else { "" };
    let sql = format!(
        "SELECT id, {title_expr}, {updated_expr}, {archived_expr}, {archived_flag_expr}, {rollout_expr}, {cwd_expr}, {model_expr}, {origin_expr}, {source_expr}, {parent_thread_expr}, {strict_parent_expr}, {agent_path_expr}, {agent_nickname_expr}, {agent_role_expr}, {first_user_message_expr} FROM threads{filter}"
    );
    let tz = jiff::tz::TimeZone::system();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(id), |row| {
        let id: String = row.get(0)?;
        let title: Option<String> = row.get(1).ok();
        let updated_raw: Option<ValueCell> = row.get(2).ok();
        let archived_raw: Option<ValueCell> = row.get(3).ok();
        let archived_flag: i64 = row.get(4).unwrap_or(0);
        let rollout_path: Option<String> = row.get(5).ok();
        let cwd: Option<String> = row.get(6).ok();
        let model: Option<String> = row.get(7).ok();
        let origin_raw: Option<String> = row.get(8).ok();
        let source: Option<String> = row.get(9).ok();
        let parent_thread_id: Option<String> = row.get(10).ok();
        let strict_parent_id: Option<String> = row.get(11).ok();
        let agent_path: Option<String> = row.get(12).ok();
        let agent_nickname: Option<String> = row.get(13).ok();
        let agent_role: Option<String> = row.get(14).ok();
        let first_user_message: Option<String> = row.get(15).ok();
        let thread_source = origin_raw.as_deref().or(source.as_deref());
        if spawned_children.contains(&id)
            || is_hidden_thread_metadata(ThreadVisibilityMetadata {
                thread_source,
                source: source.as_deref(),
                parent_thread_id: parent_thread_id.as_deref(),
                agent_path: agent_path.as_deref(),
                agent_nickname: agent_nickname.as_deref(),
                agent_role: agent_role.as_deref(),
            })
        {
            return Ok(None);
        }
        let archived_at = archived_raw
            .as_ref()
            .and_then(|c| format_cell_time_with_tz(c, &tz));
        let status = if archived_at.is_some() || archived_flag != 0 {
            ThreadStatus::Archived
        } else {
            ThreadStatus::Recent
        };
        if let Some(want_archived) = archived_filter {
            if matches!(status, ThreadStatus::Archived) != want_archived {
                return Ok(None);
            }
        }
        let identity = classify_codex_task_identity(
            source.as_deref(),
            origin_raw.as_deref(),
            [
                strict_parent_id.as_deref(),
                agent_path.as_deref(),
                agent_nickname.as_deref(),
                agent_role.as_deref(),
            ],
        );
        let formatted_updated = updated_raw
            .as_ref()
            .and_then(|c| format_cell_time_with_tz(c, &tz));
        Ok(Some((
            LocalThreadRow {
                summary: ThreadSummary {
                    activity_updated_at: formatted_updated.clone(),
                    storage_size: None,
                    id,
                    title: choose_initial_thread_title(title.as_deref(), None),
                    status,
                    updated_at: formatted_updated,
                    archived_at,
                    message_count: 0,
                    latest_message: None,
                    cwd,
                    workspace_root: None,
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
            },
            identity,
        )))
    })?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

#[derive(Debug)]
pub(super) enum ValueCell {
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

pub(super) fn format_cell_time_with_tz(
    cell: &ValueCell,
    tz: &jiff::tz::TimeZone,
) -> Option<String> {
    match cell {
        ValueCell::Null => None,
        ValueCell::Integer(v) => format_timestamp(*v, tz),
        ValueCell::Real(v) => format_timestamp(*v as i64, tz),
        ValueCell::Text(v) => {
            if v.trim().is_empty() {
                None
            } else if let Ok(n) = v.parse::<i64>() {
                format_timestamp(n, tz)
            } else {
                Some(v.clone())
            }
        }
    }
}

fn format_timestamp(value: i64, tz: &jiff::tz::TimeZone) -> Option<String> {
    if value <= 0 {
        return None;
    }
    let seconds = if value > 10_000_000_000 {
        value / 1000
    } else {
        value
    };
    jiff::Timestamp::from_second(seconds)
        .ok()
        .map(|ts| ts.display_with_offset(tz.to_offset(ts)).to_string())
}

pub(crate) use crate::{archive::first_existing, db::table_columns};

pub(crate) fn coalesce_non_empty(columns: &HashSet<String>, names: &[&str]) -> String {
    let existing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| columns.contains(*name))
        .collect();
    match existing.as_slice() {
        [] => "NULL".to_string(),
        [single] => format!("NULLIF(TRIM({single}, char(32, 9, 10, 13)), '')"),
        many => {
            let parts = many
                .iter()
                .map(|col| format!("NULLIF(TRIM({col}, char(32, 9, 10, 13)), '')"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("COALESCE({parts})")
        }
    }
}

pub(crate) fn first_non_empty<'a>(
    values: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn is_spawned_child(conn: &Connection, target_id: &str) -> Result<bool> {
    let trimmed = target_id.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    let has_table = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
    if !has_table {
        return Ok(false);
    }
    let cols = table_columns(conn, "thread_spawn_edges")?;
    if !cols.contains("child_thread_id") {
        return Ok(false);
    }
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM thread_spawn_edges WHERE TRIM(child_thread_id) = ?1)",
        [trimmed],
        |row| row.get(0),
    )?;
    Ok(exists != 0)
}

fn read_spawn_edge_children(conn: &Connection) -> Result<HashSet<String>> {
    let has_table = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
    if !has_table {
        return Ok(HashSet::new());
    }
    let cols = table_columns(conn, "thread_spawn_edges")?;
    if !cols.contains("child_thread_id") {
        return Ok(HashSet::new());
    }
    let mut stmt = conn.prepare("SELECT child_thread_id FROM thread_spawn_edges")?;
    let rows = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?;
    let mut out = HashSet::new();
    for row in rows {
        if let Some(id) = row? {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                out.insert(trimmed.to_string());
            }
        }
    }
    Ok(out)
}
