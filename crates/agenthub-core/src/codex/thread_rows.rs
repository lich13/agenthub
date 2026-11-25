use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use super::{
    rollout_events::{
        choose_initial_thread_title, is_hidden_thread_metadata, ThreadVisibilityMetadata,
    },
    CodexPaths, ThreadStatus, ThreadSummary,
};

pub fn hidden_and_archived_thread_ids(
    paths: &CodexPaths,
) -> Result<(HashSet<String>, HashSet<String>)> {
    let db = paths.state_db();
    if !db.exists() {
        return Ok((HashSet::new(), HashSet::new()));
    }
    let conn = Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {}", db.display()))?;
    let has_threads = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='threads'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_threads {
        return Ok((HashSet::new(), HashSet::new()));
    }

    let columns = table_columns(&conn, "threads")?;
    let thread_source_expr = first_existing(&columns, &["thread_source", "source_kind"])
        .or_else(|| first_existing(&columns, &["source"]))
        .unwrap_or("NULL")
        .to_string();
    let source_expr = first_existing(&columns, &["source"])
        .unwrap_or("NULL")
        .to_string();
    let parent_thread_expr = coalesce_non_empty(
        &columns,
        &[
            "parent_thread_id",
            "parentThreadId",
            "attached_thread_parent_id",
            "attachedThreadParentId",
        ],
    );
    let agent_path_expr = first_existing(&columns, &["agent_path", "agentPath"])
        .unwrap_or("NULL")
        .to_string();
    let agent_nickname_expr = first_existing(&columns, &["agent_nickname", "agentNickname"])
        .unwrap_or("NULL")
        .to_string();
    let agent_role_expr = first_existing(&columns, &["agent_role", "agentRole"])
        .unwrap_or("NULL")
        .to_string();
    let archived_expr = first_existing(&columns, &["archived"])
        .unwrap_or("0")
        .to_string();
    let archived_at_expr = first_existing(&columns, &["archived_at"])
        .unwrap_or("NULL")
        .to_string();
    let spawned_children = read_spawn_edge_children(&conn);
    let sql = format!(
        "SELECT id, {thread_source_expr}, {source_expr}, {parent_thread_expr}, {agent_path_expr}, {agent_nickname_expr}, {agent_role_expr}, {archived_expr}, {archived_at_expr} FROM threads"
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
        let archived_flag: i64 = row.get(7).unwrap_or(0);
        let archived_at: Option<ValueCell> = row.get(8).ok();
        let hidden = spawned_children.contains(&id)
            || is_hidden_thread_metadata(ThreadVisibilityMetadata {
                thread_source: thread_source.as_deref(),
                source: source.as_deref(),
                parent_thread_id: parent_thread_id.as_deref(),
                agent_path: agent_path.as_deref(),
                agent_nickname: agent_nickname.as_deref(),
                agent_role: agent_role.as_deref(),
            });
        let archived =
            archived_flag != 0 || archived_at.as_ref().and_then(format_cell_time).is_some();
        Ok((id, hidden, archived))
    })?;
    let mut hidden_ids = HashSet::new();
    let mut archived_ids = HashSet::new();
    for row in rows {
        let (id, hidden, archived) = row?;
        if hidden {
            hidden_ids.insert(id.clone());
        }
        if archived {
            archived_ids.insert(id);
        }
    }
    hidden_ids.extend(spawned_children);
    Ok((hidden_ids, archived_ids))
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
    let conn = Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {}", db.display()))?;
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
    let parent_thread_expr = coalesce_non_empty(
        &columns,
        &[
            "parent_thread_id",
            "parentThreadId",
            "attached_thread_parent_id",
            "attachedThreadParentId",
        ],
    );
    let agent_path_expr = first_existing(&columns, &["agent_path", "agentPath"])
        .unwrap_or("NULL")
        .to_string();
    let agent_nickname_expr = first_existing(&columns, &["agent_nickname", "agentNickname"])
        .unwrap_or("NULL")
        .to_string();
    let agent_role_expr = first_existing(&columns, &["agent_role", "agentRole"])
        .unwrap_or("NULL")
        .to_string();
    let first_user_message_expr = first_existing(&columns, &["first_user_message"])
        .unwrap_or("NULL")
        .to_string();
    let spawned_children = read_spawn_edge_children(&conn);
    let filter = if id.is_some() { " WHERE id = ?1" } else { "" };
    let sql = format!(
        "SELECT id, {title_expr}, {updated_expr}, {archived_expr}, {archived_flag_expr}, {rollout_expr}, {cwd_expr}, {model_expr}, {thread_source_expr}, {source_expr}, {parent_thread_expr}, {agent_path_expr}, {agent_nickname_expr}, {agent_role_expr}, {first_user_message_expr} FROM threads{filter}"
    );
    let mut workspace_root_memo: HashMap<String, Option<String>> = HashMap::new();
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
        let thread_source: Option<String> = row.get(8).ok();
        let source: Option<String> = row.get(9).ok();
        let parent_thread_id: Option<String> = row.get(10).ok();
        let agent_path: Option<String> = row.get(11).ok();
        let agent_nickname: Option<String> = row.get(12).ok();
        let agent_role: Option<String> = row.get(13).ok();
        let first_user_message: Option<String> = row.get(14).ok();
        if spawned_children.contains(&id)
            || is_hidden_thread_metadata(ThreadVisibilityMetadata {
                thread_source: thread_source.as_deref(),
                source: source.as_deref(),
                parent_thread_id: parent_thread_id.as_deref(),
                agent_path: agent_path.as_deref(),
                agent_nickname: agent_nickname.as_deref(),
                agent_role: agent_role.as_deref(),
            })
        {
            return Ok(None);
        }
        let archived_at = archived_raw.as_ref().and_then(format_cell_time);
        let status = if archived_at.is_some() || archived_flag != 0 {
            ThreadStatus::Archived
        } else {
            ThreadStatus::Recent
        };
        let workspace_root = cwd.as_deref().and_then(|c| {
            if let Some(cached) = workspace_root_memo.get(c) {
                return cached.clone();
            }
            let resolved = crate::workspace_root::resolve_workspace_root(c);
            workspace_root_memo.insert(c.to_owned(), resolved.clone());
            resolved
        });
        let formatted_updated = updated_raw.as_ref().and_then(format_cell_time);
        Ok(Some(LocalThreadRow {
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
                workspace_root,
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

pub(super) fn format_cell_time(cell: &ValueCell) -> Option<String> {
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
    jiff::Timestamp::from_second(seconds).ok().map(|ts| {
        ts.display_with_offset(jiff::tz::TimeZone::system().to_offset(ts))
            .to_string()
    })
}

pub(crate) use crate::archive::{first_existing, table_columns};

pub(crate) fn coalesce_non_empty(columns: &HashSet<String>, names: &[&str]) -> String {
    let existing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| columns.contains(*name))
        .collect();
    match existing.as_slice() {
        [] => "NULL".to_string(),
        [single] => (*single).to_string(),
        many => {
            let parts = many
                .iter()
                .map(|col| format!("NULLIF(TRIM({col}), '')"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("COALESCE({parts})")
        }
    }
}

fn read_spawn_edge_children(conn: &Connection) -> HashSet<String> {
    let has_table = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_table {
        return HashSet::new();
    }
    let Ok(mut stmt) = conn.prepare("SELECT child_thread_id FROM thread_spawn_edges") else {
        return HashSet::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return HashSet::new();
    };
    rows.flatten()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}
