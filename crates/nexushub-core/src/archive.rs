use crate::codex::{hidden_thread_metadata_category, CodexPaths, ThreadVisibilityMetadata};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    fs,
    path::Path,
};
use walkdir::WalkDir;

pub mod hidden_scan;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchiveDeletePlan {
    pub total_threads: u64,
    pub active_threads: u64,
    pub archived_threads: u64,
    pub session_index_lines: u64,
    pub rollout_files: u64,
    pub archived_ids: Vec<String>,
    pub integrity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveDeleteResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<HiddenThreadItemResult>,
    pub before: ArchiveDeletePlan,
    pub after_total_threads: u64,
    pub after_active_threads: u64,
    pub after_archived_threads: u64,
    pub after_integrity: String,
    pub deleted_rollout_files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiddenThreadSelection {
    pub id: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenThreadCandidate {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenThreadItemResult {
    pub id: String,
    pub status: String,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub residuals: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HiddenThreadDeletePlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan: Option<hidden_scan::HiddenScanProgress>,
    pub total_threads: u64,
    pub visible_threads: u64,
    pub hidden_threads: u64,
    pub archived_threads: u64,
    pub session_index_lines: u64,
    pub rollout_files: u64,
    pub hidden_ids: Vec<String>,
    pub candidates: Vec<HiddenThreadCandidate>,
    #[serde(rename = "hidden_source_counts")]
    pub source_counts: BTreeMap<String, u64>,
    pub integrity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenThreadDeleteResult {
    pub skipped_threads: u64,
    pub failed_threads: u64,
    pub items: Vec<HiddenThreadItemResult>,
    pub before: HiddenThreadDeletePlan,
    pub deleted_threads: u64,
    pub after_total_threads: u64,
    pub after_visible_threads: u64,
    pub after_hidden_threads: u64,
    pub after_archived_threads: u64,
    pub after_integrity: String,
    pub visible_threads: u64,
    pub hidden_threads: u64,
    pub integrity: String,
    pub deleted_rollout_files: u64,
}

pub fn plan_delete_archived(paths: &CodexPaths) -> Result<ArchiveDeletePlan> {
    let db = paths.state_db();
    let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", db.display()))?;
    let integrity = "not_checked".to_string();
    let columns = table_columns(&conn, "threads")?;
    let archived_condition = archived_sql_condition(&columns);
    let (total, active, archived) = count_threads(&conn)?;
    let updated_expr =
        first_existing(&columns, &["updated_at", "last_activity_at", "created_at"]).unwrap_or("id");
    let sql = format!(
        "SELECT id FROM threads WHERE {archived_condition} ORDER BY {updated_expr} DESC, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let archived_ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let archived_ids = crate::selected_codex::tree::descendants(paths, &archived_ids)?;
    Ok(ArchiveDeletePlan {
        total_threads: total,
        active_threads: active,
        archived_threads: archived,
        session_index_lines: count_lines(&paths.session_index()).unwrap_or(0),
        rollout_files: count_rollout_files(&paths.sessions_dir()).unwrap_or(0),
        archived_ids,
        integrity,
    })
}

fn thread_row_exists(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )?)
}

pub fn execute_delete_archived(
    paths: &CodexPaths,
    candidates: &[HiddenThreadSelection],
) -> Result<ArchiveDeleteResult> {
    let before = plan_delete_archived(paths)?;
    let db = paths.state_db();
    let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", db.display()))?;
    let thread_cols = table_columns(&conn, "threads")?;
    let edge_cols = table_columns(&conn, "thread_spawn_edges")?;
    let has_spawn_edges =
        edge_cols.contains("parent_thread_id") && edge_cols.contains("child_thread_id");
    let (items, deleted_rollout_files) =
        delete_candidates(paths, candidates, |candidate| -> Result<()> {
            crate::session_delete::valid_id(&candidate.id)?;
            if let Some(cur_fp) = verify_archived_target_fingerprint(
                &conn,
                &thread_cols,
                has_spawn_edges,
                &candidate.id,
            )? {
                if let Some(req_fp) = candidate.fingerprint.as_deref().filter(|s| !s.is_empty()) {
                    anyhow::ensure!(req_fp == cur_fp, "归档线程指纹已变化，请重新预览");
                }
                return Ok(());
            }
            if !thread_row_exists(&conn, &candidate.id)?
                && crate::selected_codex::tree::is_recorded_delete_residual(paths, &candidate.id)
                && crate::selected_codex::tree::has_orphan_artifacts(paths, &candidate.id)
            {
                return Ok(());
            }
            anyhow::bail!("目标不属于已归档线程允许集合或不存在")
        });
    let counts =
        Connection::open_with_flags(paths.state_db(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(anyhow::Error::from)
            .and_then(|conn| count_threads(&conn));
    let after_integrity = if counts.is_ok() {
        "not_checked"
    } else {
        "unavailable"
    }
    .into();
    let (after_total_threads, after_active_threads, after_archived_threads) =
        counts.unwrap_or_default();
    Ok(ArchiveDeleteResult {
        before,
        items,
        after_total_threads,
        after_active_threads,
        after_archived_threads,
        after_integrity,
        deleted_rollout_files,
    })
}

fn delete_candidates(
    paths: &CodexPaths,
    candidates: &[HiddenThreadSelection],
    authorize: impl Fn(&HiddenThreadSelection) -> Result<()>,
) -> (Vec<HiddenThreadItemResult>, u64) {
    let mut items = Vec::new();
    let mut files = 0;
    let mut seen = HashSet::new();
    for candidate in candidates {
        if !seen.insert(&candidate.id) {
            continue;
        }
        if let Err(error) = authorize(candidate).and_then(|()| {
            crate::selected_codex::tree::ensure_tree_not_running(paths, &candidate.id)
        }) {
            items.push(HiddenThreadItemResult {
                id: candidate.id.clone(),
                status: "failed".into(),
                reason: Some(error.to_string()),
                residuals: vec![],
            });
            break;
        }
        match crate::selected_codex::tree::execute_ids(paths, std::slice::from_ref(&candidate.id)) {
            Ok(result) => {
                files += result.deleted_files;
                let failed = result.error.is_some() || !result.residuals.is_empty();
                items.push(HiddenThreadItemResult {
                    id: candidate.id.clone(),
                    status: if failed { "failed" } else { "deleted" }.into(),
                    reason: result.error,
                    residuals: result.residuals,
                });
                if failed {
                    break;
                }
            }
            Err(error) => {
                items.push(HiddenThreadItemResult {
                    id: candidate.id.clone(),
                    status: "failed".into(),
                    reason: Some(error.to_string()),
                    residuals: vec![],
                });
                break;
            }
        }
    }
    (items, files)
}

pub fn plan_delete_hidden(paths: &CodexPaths) -> Result<HiddenThreadDeletePlan> {
    hidden_delete_plan(paths)
}

fn hidden_summary_counts(paths: &CodexPaths) -> Result<HiddenThreadDeletePlan> {
    let work = crate::scan_work::ScanWork::default();
    let conn = crate::selected_codex::open(paths, false)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    let rows = hidden_visibility_rows(&conn, &work)?;
    let mut visible_threads = 0;
    let mut hidden_ids = Vec::new();
    let mut source_counts = BTreeMap::new();
    let mut archived_threads = 0;
    for row in &rows {
        if row.archived {
            archived_threads += 1;
        } else if let Some(category) = &row.hidden_category {
            hidden_ids.push(row.id.clone());
            *source_counts.entry(category.clone()).or_insert(0) += 1;
        } else {
            visible_threads += 1;
        }
    }
    let hidden_ids = crate::selected_codex::tree::descendants(paths, &hidden_ids)?;
    Ok(HiddenThreadDeletePlan {
        scan: None,
        total_threads: rows.len() as u64,
        visible_threads,
        hidden_threads: hidden_ids.len() as u64,
        archived_threads,
        session_index_lines: 0,
        rollout_files: 0,
        hidden_ids,
        candidates: Vec::new(),
        source_counts,
        integrity: "not_checked".into(),
    })
}

pub(crate) fn plan_delete_hidden_candidates(paths: &CodexPaths) -> Result<HiddenThreadDeletePlan> {
    let work = crate::scan_work::ScanWork::default();
    let conn = crate::selected_codex::open(paths, false)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    let rows = hidden_visibility_rows(&conn, &work)?;
    let mut visible_threads = 0;
    let mut hidden_ids = Vec::new();
    let mut source_counts = BTreeMap::new();
    let mut archived_threads = 0;
    for row in &rows {
        if row.archived {
            archived_threads += 1;
        } else if let Some(category) = &row.hidden_category {
            hidden_ids.push(row.id.clone());
            *source_counts.entry(category.clone()).or_insert(0) += 1;
        } else {
            visible_threads += 1;
        }
    }
    let hidden_ids = crate::selected_codex::tree::descendants(paths, &hidden_ids)?;
    let row_by_id: BTreeMap<&str, &HiddenVisibilityRow> =
        rows.iter().map(|row| (row.id.as_str(), row)).collect();
    let candidates = hidden_ids
        .iter()
        .map(|id| {
            let row = row_by_id.get(id.as_str()).copied();
            HiddenThreadCandidate {
                id: id.clone(),
                title: row.map_or_else(|| id.clone(), |r| r.title.clone()),
                fingerprint: row.map(|r| r.fingerprint.clone()),
                allowed: true,
                reason: None,
            }
        })
        .collect();
    Ok(HiddenThreadDeletePlan {
        scan: None,
        total_threads: rows.len() as u64,
        visible_threads,
        hidden_threads: hidden_ids.len() as u64,
        archived_threads,
        session_index_lines: 0,
        rollout_files: 0,
        hidden_ids,
        candidates,
        source_counts,
        integrity: "not_checked".into(),
    })
}

pub fn execute_delete_hidden(
    paths: &CodexPaths,
    candidates: &[HiddenThreadSelection],
) -> Result<HiddenThreadDeleteResult> {
    let before = hidden_summary_counts(paths)?;
    let conn = crate::selected_codex::open(paths, false)?;
    let thread_cols = table_columns(&conn, "threads")?;
    let edge_cols = table_columns(&conn, "thread_spawn_edges")?;
    let has_spawn_edges =
        edge_cols.contains("parent_thread_id") && edge_cols.contains("child_thread_id");
    let (items, deleted_rollout_files) =
        delete_candidates(paths, candidates, |candidate| -> Result<()> {
            crate::session_delete::valid_id(&candidate.id)?;
            if let Some(cur_fp) = verify_hidden_target_fingerprint(
                &conn,
                &thread_cols,
                has_spawn_edges,
                &candidate.id,
            )? {
                if let Some(req_fp) = candidate.fingerprint.as_deref().filter(|s| !s.is_empty()) {
                    anyhow::ensure!(req_fp == cur_fp, "隐藏线程指纹已变化，请重新预览");
                }
                return Ok(());
            }
            if !thread_row_exists(&conn, &candidate.id)?
                && crate::selected_codex::tree::is_recorded_delete_residual(paths, &candidate.id)
                && crate::selected_codex::tree::has_orphan_artifacts(paths, &candidate.id)
            {
                return Ok(());
            }
            anyhow::bail!("目标不属于隐藏线程允许集合或不存在")
        });
    drop(conn);
    let after = hidden_summary_counts(paths).unwrap_or_else(|_| HiddenThreadDeletePlan {
        integrity: "unavailable".into(),
        ..Default::default()
    });
    Ok(HiddenThreadDeleteResult {
        deleted_threads: items.iter().filter(|i| i.status == "deleted").count() as u64,
        skipped_threads: items.iter().filter(|i| i.status == "skipped").count() as u64,
        failed_threads: items.iter().filter(|i| i.status == "failed").count() as u64,
        items,
        before,
        after_total_threads: after.total_threads,
        after_visible_threads: after.visible_threads,
        after_hidden_threads: after.hidden_threads,
        after_archived_threads: after.archived_threads,
        after_integrity: after.integrity.clone(),
        visible_threads: after.visible_threads,
        hidden_threads: after.hidden_threads,
        integrity: after.integrity,
        deleted_rollout_files,
    })
}

fn count_threads(conn: &Connection) -> Result<(u64, u64, u64)> {
    let columns = table_columns(conn, "threads")?;
    let archived_condition = archived_sql_condition(&columns);
    let sql = format!(
        "SELECT count(*),
                coalesce(sum(CASE WHEN NOT ({archived_condition}) THEN 1 ELSE 0 END), 0),
                coalesce(sum(CASE WHEN {archived_condition} THEN 1 ELSE 0 END), 0)
         FROM threads"
    );
    Ok(conn.query_row(&sql, [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?)
}

fn hidden_delete_plan(paths: &CodexPaths) -> Result<HiddenThreadDeletePlan> {
    let work = crate::scan_work::ScanWork::default();
    hidden_delete_plan_with_work(paths, &work)
}

pub(crate) fn hidden_delete_plan_with_work(
    paths: &CodexPaths,
    work: &crate::scan_work::ScanWork,
) -> Result<HiddenThreadDeletePlan> {
    let mut candidates = Vec::new();
    let mut plan = scan_hidden(paths, work, |_, candidate| {
        if let Some(candidate) = candidate {
            candidates.push(candidate);
        }
        Ok(())
    })?;
    plan.candidates = candidates;
    Ok(plan)
}

pub(crate) fn scan_hidden(
    paths: &CodexPaths,
    work: &crate::scan_work::ScanWork,
    mut emit: impl FnMut(&HiddenThreadDeletePlan, Option<HiddenThreadCandidate>) -> Result<()>,
) -> Result<HiddenThreadDeletePlan> {
    work.check()?;
    let conn = crate::selected_codex::open(paths, false)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    let cancellation = work.clone();
    conn.progress_handler(1000, Some(move || cancellation.check().is_err()));
    conn.execute_batch("BEGIN DEFERRED")?;
    let integrity = "not_checked".to_string();

    let rows = hidden_visibility_rows(&conn, work)?;
    let mut visible_threads = 0;
    let mut hidden_ids = Vec::new();
    let mut source_counts = BTreeMap::new();
    let mut archived_threads = 0;
    for row in &rows {
        if row.archived {
            archived_threads += 1;
        } else if let Some(category) = &row.hidden_category {
            hidden_ids.push(row.id.clone());
            *source_counts.entry(category.clone()).or_insert(0) += 1;
        } else {
            visible_threads += 1;
        }
    }
    let hidden_ids = crate::selected_codex::tree::descendants(paths, &hidden_ids)?;
    let mut plan = HiddenThreadDeletePlan {
        scan: None,
        total_threads: rows.len() as u64,
        visible_threads,
        hidden_threads: hidden_ids.len() as u64,
        archived_threads,
        session_index_lines: 0,
        rollout_files: 0,
        hidden_ids,
        candidates: Vec::new(),
        source_counts,
        integrity,
    };
    emit(&plan, None)?;
    plan.session_index_lines = count_lines(&paths.session_index()).unwrap_or(0);
    plan.rollout_files = count_rollout_files(&paths.sessions_dir()).unwrap_or(0);
    emit(&plan, None)?;
    let row_by_id: BTreeMap<&str, &HiddenVisibilityRow> =
        rows.iter().map(|row| (row.id.as_str(), row)).collect();
    for id in &plan.hidden_ids {
        work.check()?;
        let row = row_by_id.get(id.as_str()).copied();
        let title = row.map_or_else(|| id.clone(), |r| r.title.clone());
        let fingerprint = row.map(|r| r.fingerprint.clone());
        emit(
            &plan,
            Some(HiddenThreadCandidate {
                id: id.clone(),
                title,
                fingerprint,
                allowed: true,
                reason: None,
            }),
        )?;
    }
    Ok(plan)
}

#[derive(Debug)]
struct HiddenVisibilityRow {
    id: String,
    title: String,
    archived: bool,
    hidden_category: Option<String>,
    parent_ids: Vec<String>,
    fingerprint: String,
}

#[allow(clippy::too_many_arguments)]
fn codex_row_fingerprint(
    id: &str,
    archived: bool,
    updated_tag: &str,
    thread_source: Option<&str>,
    source: Option<&str>,
    parent_thread_id: Option<&str>,
    has_user_event: Option<i64>,
    title: Option<&str>,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"codex:v1\0");
    for part in [
        id,
        if archived { "1" } else { "0" },
        updated_tag,
        thread_source.unwrap_or(""),
        source.unwrap_or(""),
        parent_thread_id.unwrap_or(""),
        title.unwrap_or(""),
    ] {
        hash.update(part.as_bytes());
        hash.update(b"\0");
    }
    match has_user_event {
        Some(v) => {
            hash.update(b"1");
            hash.update(v.to_le_bytes());
        }
        None => hash.update(b"0"),
    }
    hex::encode(hash.finalize())
}

fn extract_row_parents(parent_thread_id: Option<&str>, source: Option<&str>) -> Vec<String> {
    let mut parents = Vec::new();
    if let Some(p) = parent_thread_id.filter(|s| !s.trim().is_empty()) {
        parents.push(p.to_string());
    }
    if let Some(parsed) = source.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) {
        for ptr in [
            "/subagent/thread_spawn/parent_thread_id",
            "/parent_thread_id",
        ] {
            if let Some(p) = parsed
                .pointer(ptr)
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                if !parents.iter().any(|existing| existing == p) {
                    parents.push(p.to_string());
                }
            }
        }
    }
    parents
}

struct VisibilitySelectSql {
    select_cols: String,
    updated_expr: String,
}

fn build_visibility_select_sql(columns: &HashSet<String>) -> VisibilitySelectSql {
    let archived_condition = archived_sql_condition(columns);
    let updated_expr = first_existing(columns, &["updated_at", "last_activity_at", "created_at"])
        .unwrap_or("id")
        .to_string();
    let thread_source_expr = first_existing(columns, &["thread_source", "source_kind"])
        .or_else(|| first_existing(columns, &["source"]))
        .unwrap_or("NULL");
    let source_expr = first_existing(columns, &["source"]).unwrap_or("NULL");
    let parent_thread_expr =
        first_existing(columns, &["parent_thread_id", "parentThreadId"]).unwrap_or("NULL");
    let agent_path_expr = first_existing(columns, &["agent_path", "agentPath"]).unwrap_or("NULL");
    let agent_nickname_expr =
        first_existing(columns, &["agent_nickname", "agentNickname"]).unwrap_or("NULL");
    let agent_role_expr = first_existing(columns, &["agent_role", "agentRole"]).unwrap_or("NULL");
    let has_user_event_expr = first_existing(columns, &["has_user_event"]).unwrap_or("NULL");
    let title_expr = first_existing(columns, &["title", "name"]).unwrap_or("NULL");
    let first_user_message_expr =
        first_existing(columns, &["first_user_message"]).unwrap_or("NULL");
    let preview_expr = first_existing(columns, &["preview"]).unwrap_or("NULL");
    let select_cols = format!(
        "id, CASE WHEN {archived_condition} THEN 1 ELSE 0 END, \
         {thread_source_expr}, {source_expr}, {parent_thread_expr}, {agent_path_expr}, \
         {agent_nickname_expr}, {agent_role_expr}, {has_user_event_expr}, {title_expr}, \
         {first_user_message_expr}, {preview_expr}, CAST({updated_expr} AS TEXT)"
    );
    VisibilitySelectSql {
        select_cols,
        updated_expr,
    }
}

fn decode_visibility_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HiddenVisibilityRow> {
    let id: String = row.get(0)?;
    let archived_flag: i64 = row.get(1)?;
    let archived = archived_flag != 0;
    let thread_source: Option<String> = row.get(2).ok();
    let source: Option<String> = row.get(3).ok();
    let parent_thread_id: Option<String> = row.get(4).ok();
    let agent_path: Option<String> = row.get(5).ok();
    let agent_nickname: Option<String> = row.get(6).ok();
    let agent_role: Option<String> = row.get(7).ok();
    let has_user_event: Option<i64> = row.get(8).ok();
    let title: Option<String> = row.get(9).ok();
    let first_user_message: Option<String> = row.get(10).ok();
    let preview: Option<String> = row.get(11).ok();
    let updated_tag: Option<String> = row.get(12).ok();
    let hidden_category = hidden_thread_metadata_category(ThreadVisibilityMetadata {
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
    let fingerprint = codex_row_fingerprint(
        &id,
        archived,
        updated_tag.as_deref().unwrap_or(""),
        thread_source.as_deref(),
        source.as_deref(),
        parent_thread_id.as_deref(),
        has_user_event,
        title.as_deref(),
    );
    let parent_ids = extract_row_parents(parent_thread_id.as_deref(), source.as_deref());
    Ok(HiddenVisibilityRow {
        title: title.unwrap_or_else(|| id.clone()),
        id,
        archived,
        hidden_category,
        parent_ids,
        fingerprint,
    })
}

pub(crate) fn codex_thread_fingerprint(paths: &CodexPaths, id: &str) -> Result<Option<String>> {
    let db = paths.state_db();
    let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", db.display()))?;
    let columns = table_columns(&conn, "threads")?;
    let spec = build_visibility_select_sql(&columns);
    let sql = format!("SELECT {} FROM threads WHERE id = ?1", spec.select_cols);
    let row = conn
        .query_row(&sql, [id], decode_visibility_row)
        .optional()?;
    Ok(row.map(|r| r.fingerprint))
}

/// Targeted `O(depth)` point lookup verifying `id` belongs to the current archived thread closure.
pub(crate) fn verify_archived_target_fingerprint(
    conn: &Connection,
    thread_columns: &HashSet<String>,
    has_spawn_edges: bool,
    id: &str,
) -> Result<Option<String>> {
    verify_closure_target_fingerprint(conn, thread_columns, has_spawn_edges, id, |row| {
        row.archived
    })
}

/// Targeted `O(depth)` point lookup verifying `id` belongs to the current non-archived hidden
/// thread closure in `state_5.sqlite` without scanning all threads or rollout files on disk.
pub(crate) fn verify_hidden_target_fingerprint(
    conn: &Connection,
    thread_columns: &HashSet<String>,
    has_spawn_edges: bool,
    id: &str,
) -> Result<Option<String>> {
    verify_closure_target_fingerprint(conn, thread_columns, has_spawn_edges, id, |row| {
        !row.archived && row.hidden_category.is_some()
    })
}

fn verify_closure_target_fingerprint(
    conn: &Connection,
    thread_columns: &HashSet<String>,
    has_spawn_edges: bool,
    id: &str,
    matches_root: impl Fn(&HiddenVisibilityRow) -> bool,
) -> Result<Option<String>> {
    let spec = build_visibility_select_sql(thread_columns);
    let row_sql = format!("SELECT {} FROM threads WHERE id = ?1", spec.select_cols);
    let Some(target_row) = conn
        .query_row(&row_sql, [id], decode_visibility_row)
        .optional()?
    else {
        return Ok(None);
    };
    let target_fp = target_row.fingerprint.clone();
    if matches_root(&target_row) {
        return Ok(Some(target_fp));
    }
    let mut queue: VecDeque<String> = target_row.parent_ids.into_iter().collect();
    if has_spawn_edges {
        let mut stmt = conn.prepare(
            "SELECT parent_thread_id FROM thread_spawn_edges WHERE child_thread_id = ?1",
        )?;
        for parent in stmt.query_map([id], |r| r.get::<_, String>(0))? {
            queue.push_back(parent?);
        }
    }
    let mut visited = BTreeSet::from([id.to_string()]);
    while let Some(parent_id) = queue.pop_front() {
        if !visited.insert(parent_id.clone()) {
            continue;
        }
        let Some(anc) = conn
            .query_row(&row_sql, [&parent_id], decode_visibility_row)
            .optional()?
        else {
            continue;
        };
        if matches_root(&anc) {
            return Ok(Some(target_fp));
        }
        for p in anc.parent_ids {
            queue.push_back(p);
        }
        if has_spawn_edges {
            let mut stmt = conn.prepare(
                "SELECT parent_thread_id FROM thread_spawn_edges WHERE child_thread_id = ?1",
            )?;
            for parent in stmt.query_map([&parent_id], |r| r.get::<_, String>(0))? {
                queue.push_back(parent?);
            }
        }
    }
    Ok(None)
}

fn hidden_visibility_rows(
    conn: &Connection,
    work: &crate::scan_work::ScanWork,
) -> Result<Vec<HiddenVisibilityRow>> {
    let columns = table_columns(conn, "threads")?;
    let spec = build_visibility_select_sql(&columns);
    let sql = format!(
        "SELECT {} FROM threads ORDER BY {} DESC, id",
        spec.select_cols, spec.updated_expr
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], decode_visibility_row)?;
    let mut result = Vec::new();
    for row in rows {
        let row = row?;
        work.check()?;
        result.push(row);
    }
    Ok(result)
}

pub(crate) fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}

fn first_existing<'a>(columns: &HashSet<String>, names: &'a [&str]) -> Option<&'a str> {
    names.iter().copied().find(|name| columns.contains(*name))
}

fn archived_sql_condition(columns: &HashSet<String>) -> String {
    let mut conditions = Vec::new();
    if columns.contains("archived") {
        conditions.push("coalesce(archived, 0) != 0".to_string());
    }
    if columns.contains("archived_at") {
        conditions.push("archived_at IS NOT NULL".to_string());
    }
    if conditions.is_empty() {
        "0".to_string()
    } else {
        conditions.join(" OR ")
    }
}

fn count_lines(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    Ok(fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?
        .lines()
        .count() as u64)
}

fn count_rollout_files(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    Ok(WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
                    .unwrap_or(false)
        })
        .count() as u64)
}

#[cfg(test)]
mod tests {
    use super::{
        execute_delete_hidden, plan_delete_hidden, ArchiveDeletePlan, HiddenThreadDeleteResult,
        HiddenThreadSelection,
    };
    use crate::codex::CodexPaths;
    use rusqlite::Connection;
    use serde_json::json;
    use std::path::PathBuf;
    use std::{
        env, fs,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn plan_serializes_counts() {
        let plan = ArchiveDeletePlan {
            total_threads: 1,
            active_threads: 1,
            archived_threads: 0,
            session_index_lines: 1,
            rollout_files: 1,
            archived_ids: Vec::new(),
            integrity: "ok".to_string(),
        };
        assert!(serde_json::to_string(&plan)
            .unwrap()
            .contains("total_threads"));
    }

    #[test]
    fn plan_delete_hidden_selects_non_archived_hidden_threads() {
        let root = hidden_cleanup_fixture("plan-hidden");

        let plan = plan_delete_hidden(&CodexPaths::new(&root)).unwrap();

        assert_eq!(plan.total_threads, 4);
        assert_eq!(plan.visible_threads, 1);
        assert_eq!(plan.hidden_threads, 2);
        assert_eq!(plan.archived_threads, 1);
        assert_eq!(
            plan.hidden_ids,
            vec!["hidden-internal".to_string(), "hidden-subagent".to_string()]
        );
        assert_eq!(plan.source_counts.get("internal").copied(), Some(1));
        assert_eq!(plan.source_counts.get("subagent").copied(), Some(1));
        assert!(!plan.hidden_ids.contains(&"archived-hidden".to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn execute_delete_hidden_removes_only_hidden_and_preserves_visible_count() {
        let root = hidden_cleanup_fixture("execute-hidden");
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let candidates = selections_for_plan(&plan);

        let result = execute_delete_hidden(&paths, &candidates).unwrap();

        assert_hidden_result_preserved_visible(&result);
        assert_eq!(result.before.hidden_threads, 2);
        assert_eq!(result.after_hidden_threads, 0);
        assert_eq!(result.after_archived_threads, 1);
        assert_eq!(result.deleted_rollout_files, 2);

        let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
        assert!(thread_exists(&conn, "main-visible"));
        assert!(!thread_exists(&conn, "hidden-subagent"));
        assert!(!thread_exists(&conn, "hidden-internal"));
        assert!(thread_exists(&conn, "archived-hidden"));
        assert_eq!(count_rows(&conn, "thread_dynamic_tools"), 1);
        assert_eq!(count_rows(&conn, "thread_spawn_edges"), 0);
        let assigned: Option<String> = conn
            .query_row(
                "SELECT assigned_thread_id FROM agent_job_items WHERE id='job-hidden-subagent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assigned, None);
        let index = fs::read_to_string(root.join("session_index.jsonl")).unwrap();
        assert!(index.contains("main-visible"));
        assert!(index.contains("archived-hidden"));
        assert!(!index.contains("hidden-subagent"));
        assert!(!index.contains("hidden-internal"));
        assert!(root
            .join("sessions")
            .join("rollout-main-visible.jsonl")
            .exists());
        assert!(root
            .join("sessions")
            .join("rollout-archived-hidden.jsonl")
            .exists());
        assert!(!root
            .join("sessions")
            .join("rollout-hidden-subagent.jsonl")
            .exists());
        assert!(!root
            .join("sessions")
            .join("rollout-hidden-internal.jsonl")
            .exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn execute_delete_hidden_removes_running_and_other_confirmed_candidates() {
        let root = hidden_cleanup_fixture("running-hidden");
        let paths = CodexPaths::new(&root);
        fs::write(
            root.join("sessions/rollout-hidden-subagent.jsonl"),
            "{\"type\":\"turn_started\"}\n{",
        )
        .unwrap();
        let plan = plan_delete_hidden(&paths).unwrap();
        assert!(candidate(&plan, "hidden-subagent").allowed);
        let result = execute_delete_hidden(&paths, &selections_for_plan(&plan)).unwrap();
        assert_eq!(result.deleted_threads, 2);
        assert_eq!(result.failed_threads, 0);
        assert_eq!(result.skipped_threads, 0);
        assert_hidden_result_preserved_visible(&result);
        assert!(!root.join("sessions/rollout-hidden-subagent.jsonl").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn execute_delete_hidden_removes_stale_running_rollout() {
        let root = hidden_cleanup_fixture("running-hidden");
        let paths = CodexPaths::new(&root);
        fs::write(
            root.join("sessions/rollout-hidden-subagent.jsonl"),
            "{\"type\":\"turn_started\"}\n{",
        )
        .unwrap();
        let plan = plan_delete_hidden(&paths).unwrap();
        assert!(candidate(&plan, "hidden-subagent").allowed);
        let result = execute_delete_hidden(&paths, &selections_for_plan(&plan)).unwrap();
        assert_eq!(result.deleted_threads, 2);
        assert_eq!(result.failed_threads, 0);
        assert_eq!(result.skipped_threads, 0);
        assert_hidden_result_preserved_visible(&result);
        assert!(!root.join("sessions/rollout-hidden-subagent.jsonl").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn execute_delete_hidden_allows_aborted_candidate() {
        let root = hidden_cleanup_fixture("aborted-hidden");
        fs::write(
            root.join("sessions").join("rollout-hidden-subagent.jsonl"),
            [
                json!({"type":"session_meta","payload":{"id":"hidden-subagent"}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-abort"}}).to_string(),
                json!({"type":"turn_started","turn_id":"turn-abort"}).to_string(),
                json!({"type":"response_item","turn_id":"turn-abort","payload":{"type":"function_call","name":"exec_command","call_id":"call-abort","arguments":{"cmd":"sleep 10"}}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-abort","reason":"interrupted"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let candidates = selections_for_plan(&plan);

        let result = execute_delete_hidden(&paths, &candidates).unwrap();

        assert_hidden_result_preserved_visible(&result);
        assert_eq!(result.before.hidden_threads, 2);
        assert_eq!(result.after_hidden_threads, 0);
        let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
        assert!(!thread_exists(&conn, "hidden-subagent"));
        assert!(!root
            .join("sessions")
            .join("rollout-hidden-subagent.jsonl")
            .exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hidden_delete_removes_changed_files_without_repreview() {
        let root = hidden_cleanup_fixture("changed-fingerprint");
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let candidates = selections_for_plan(&plan);
        fs::write(
            root.join("sessions/rollout-hidden-internal.jsonl"),
            "changed fixture",
        )
        .unwrap();
        let result = execute_delete_hidden(&paths, &candidates).unwrap();
        assert_eq!(result.deleted_threads, 2);
        assert_eq!(result.failed_threads, 0);
        assert_hidden_result_preserved_visible(&result);
        assert!(!root.join("sessions/rollout-hidden-internal.jsonl").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hidden_delete_rejects_unindexed_orphans_but_allows_recorded_partial_delete_residual_retry() {
        let root = hidden_cleanup_fixture("orphan-vs-residual");
        let paths = CodexPaths::new(&root);

        // 1. Pre-migration / unindexed orphan rollout (absent from `threads` and never partially
        //    deleted by NexusHub) must be strictly rejected by both archived and hidden delete.
        let orphan_id = "unindexed-pre-migration";
        let orphan_rollout = root
            .join("sessions")
            .join(format!("rollout-{orphan_id}.jsonl"));
        fs::write(&orphan_rollout, valid_completed_rollout(orphan_id)).unwrap();
        let orphan_sel = [HiddenThreadSelection {
            id: orphan_id.into(),
            fingerprint: None,
        }];
        let archived_orphan = super::execute_delete_archived(&paths, &orphan_sel).unwrap();
        assert_eq!(archived_orphan.items[0].status, "failed");
        assert!(orphan_rollout.exists());

        let hidden_orphan = execute_delete_hidden(&paths, &orphan_sel).unwrap();
        assert_eq!(hidden_orphan.failed_threads, 1);
        assert!(orphan_rollout.exists());

        // 2. Genuine partial failure where `state_5.sqlite` row is deleted but secondary
        //    `thread_history_1.sqlite` fails records a controlled residual, allowing clean retry.
        let plan = plan_delete_hidden(&paths).unwrap();
        let selected = selection(&plan, "hidden-subagent");
        let history_db = root.join("thread_history_1.sqlite");
        let hconn = Connection::open(&history_db).unwrap();
        hconn
            .execute_batch(
                "CREATE TABLE history(thread_id TEXT NOT NULL);
                 INSERT INTO history(thread_id) VALUES('hidden-subagent');
                 CREATE TRIGGER fail_secondary BEFORE DELETE ON history
                 WHEN OLD.thread_id='hidden-subagent'
                 BEGIN SELECT RAISE(ABORT,'secondary history locked'); END;",
            )
            .unwrap();
        drop(hconn);

        let first = execute_delete_hidden(&paths, std::slice::from_ref(&selected)).unwrap();
        assert_eq!(first.failed_threads, 1);
        assert!(!first.items[0].residuals.is_empty());
        assert!(!thread_exists(
            &Connection::open(paths.state_db()).unwrap(),
            "hidden-subagent"
        ));
        assert!(crate::selected_codex::tree::is_recorded_delete_residual(
            &paths,
            "hidden-subagent"
        ));

        // Recreate a residual artifact and drop the secondary failure trigger, then retry.
        let residual_lock = root.join("thread-writer-locks/hidden-subagent.lock");
        fs::create_dir_all(residual_lock.parent().unwrap()).unwrap();
        fs::write(&residual_lock, b"").unwrap();
        Connection::open(&history_db)
            .unwrap()
            .execute_batch("DROP TRIGGER fail_secondary;")
            .unwrap();

        let retry = execute_delete_hidden(&paths, &[selected]).unwrap();
        assert_eq!(retry.deleted_threads, 1);
        assert_eq!(retry.failed_threads, 0);
        assert!(!residual_lock.exists());
        assert!(!crate::selected_codex::tree::is_recorded_delete_residual(
            &paths,
            "hidden-subagent"
        ));
        assert!(orphan_rollout.exists());
        assert!(thread_exists(
            &Connection::open(paths.state_db()).unwrap(),
            "hidden-internal"
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hidden_delete_removes_missing_corrupt_and_externally_referenced_rollouts() {
        for case in ["missing", "corrupt", "shared"] {
            let root = hidden_cleanup_fixture(case);
            let paths = CodexPaths::new(&root);
            let plan = plan_delete_hidden(&paths).unwrap();
            let rollout = root.join("sessions/rollout-hidden-subagent.jsonl");
            if case == "missing" {
                fs::remove_file(&rollout).unwrap();
            } else if case == "corrupt" {
                fs::write(&rollout, "{broken jsonl").unwrap();
            } else {
                Connection::open(paths.state_db())
                    .unwrap()
                    .execute(
                        "UPDATE threads SET rollout_path=?1 WHERE id='main-visible'",
                        [rollout.to_string_lossy()],
                    )
                    .unwrap();
            }
            let result =
                execute_delete_hidden(&paths, &[selection(&plan, "hidden-subagent")]).unwrap();
            assert_eq!(result.deleted_threads, 1, "{case}");
            assert_eq!(result.failed_threads, 0, "{case}");
            assert!(!rollout.exists());
            assert!(thread_exists(
                &Connection::open(paths.state_db()).unwrap(),
                "main-visible"
            ));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn hidden_delete_unlinks_symlink_rollouts_without_touching_the_target() {
        let root = hidden_cleanup_fixture("symlink");
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let rollout = root.join("sessions/rollout-hidden-subagent.jsonl");
        let target = root.join("sessions/rollout-main-visible.jsonl");
        fs::remove_file(&rollout).unwrap();
        std::os::unix::fs::symlink(&target, &rollout).unwrap();
        let result = execute_delete_hidden(&paths, &[selection(&plan, "hidden-subagent")]).unwrap();
        assert_eq!(result.deleted_threads, 1);
        assert_eq!(result.failed_threads, 0);
        assert!(fs::symlink_metadata(rollout).is_err());
        assert!(target.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hidden_delete_does_not_add_threads_created_after_preview_to_candidates() {
        let root = hidden_cleanup_fixture("late-hidden");
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let candidates = selections_for_plan(&plan);
        append_hidden_thread(&root, "late-hidden", 50);

        let result = execute_delete_hidden(&paths, &candidates).unwrap();

        assert_eq!(result.deleted_threads, 2);
        assert_eq!(result.after_hidden_threads, 1);
        let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
        assert!(thread_exists(&conn, "late-hidden"));
        assert!(root
            .join("sessions")
            .join("rollout-late-hidden.jsonl")
            .exists());
        assert!(fs::read_to_string(root.join("session_index.jsonl"))
            .unwrap()
            .contains("late-hidden"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hidden_delete_accepts_more_than_one_hundred_preview_candidates() {
        let root = hidden_cleanup_fixture("over-one-hundred");
        for index in 0..101 {
            append_hidden_thread(&root, &format!("extra-{index:03}"), 100 + index as i64);
        }
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let candidates = selections_for_plan(&plan);
        assert!(candidates.len() > 100);

        let result = execute_delete_hidden(&paths, &candidates).unwrap();

        assert_eq!(result.deleted_threads as usize, candidates.len());
        assert_eq!(result.skipped_threads, 0);
        assert_eq!(result.failed_threads, 0);
        assert_eq!(result.after_hidden_threads, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hidden_delete_stops_after_a_single_item_database_failure() {
        let root = hidden_cleanup_fixture("item-database-failure");
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let candidates = vec![
            selection(&plan, "hidden-internal"),
            selection(&plan, "hidden-subagent"),
        ];
        Connection::open(root.join("state_5.sqlite"))
            .unwrap()
            .execute_batch("CREATE TRIGGER fail_one_hidden BEFORE DELETE ON threads WHEN OLD.id='hidden-internal' BEGIN SELECT RAISE(ABORT,'injected item failure'); END;")
            .unwrap();

        let result = execute_delete_hidden(&paths, &candidates).unwrap();

        assert_eq!(result.failed_threads, 1);
        assert_eq!(result.deleted_threads, 0);
        assert!(!result.items[0].residuals.is_empty());
        let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
        assert!(thread_exists(&conn, "hidden-internal"));
        assert!(thread_exists(&conn, "hidden-subagent"));
        assert!(!root
            .join("sessions")
            .join("rollout-hidden-internal.jsonl")
            .exists());
        assert!(root
            .join("sessions")
            .join("rollout-hidden-subagent.jsonl")
            .exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn execute_delete_archived_and_hidden_reject_out_of_bounds_and_running_targets_and_stop_early()
    {
        let root = hidden_cleanup_fixture("execution-side-guards");
        let paths = CodexPaths::new(&root);

        // 1. Out-of-bounds / visible ID rejected and stops subsequent archived deletion.
        let bad_archived = super::execute_delete_archived(
            &paths,
            &[
                HiddenThreadSelection {
                    id: "main-visible".into(),
                    fingerprint: None,
                },
                HiddenThreadSelection {
                    id: "archived-hidden".into(),
                    fingerprint: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(bad_archived.items.len(), 1);
        assert_eq!(bad_archived.items[0].status, "failed");
        let conn = Connection::open(paths.state_db()).unwrap();
        assert!(thread_exists(&conn, "main-visible"));
        assert!(thread_exists(&conn, "archived-hidden"));

        // 2. Nonexistent forged ID rejected in hidden delete without touching subsequent valid item.
        let bad_hidden = execute_delete_hidden(
            &paths,
            &[
                HiddenThreadSelection {
                    id: "forged-nonexistent-id".into(),
                    fingerprint: None,
                },
                HiddenThreadSelection {
                    id: "hidden-internal".into(),
                    fingerprint: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(bad_hidden.failed_threads, 1);
        assert_eq!(bad_hidden.deleted_threads, 0);
        assert!(thread_exists(&conn, "hidden-internal"));

        // 3. Running target (active agent_job_items) rejected and stops subsequent deletion.
        conn.execute(
            "UPDATE agent_job_items SET status='running' WHERE id='job-hidden-subagent'",
            [],
        )
        .unwrap();
        let running_res = execute_delete_hidden(
            &paths,
            &[
                HiddenThreadSelection {
                    id: "hidden-subagent".into(),
                    fingerprint: None,
                },
                HiddenThreadSelection {
                    id: "hidden-internal".into(),
                    fingerprint: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(running_res.failed_threads, 1);
        assert_eq!(running_res.deleted_threads, 0);
        assert!(thread_exists(&conn, "hidden-subagent"));
        assert!(thread_exists(&conn, "hidden-internal"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hidden_delete_reports_database_failures_without_rolling_back_other_candidates() {
        let root = hidden_cleanup_fixture("system-database-failure");
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let candidates = vec![
            selection(&plan, "hidden-internal"),
            selection(&plan, "hidden-subagent"),
        ];
        Connection::open(root.join("state_5.sqlite"))
            .unwrap()
            .execute_batch("CREATE TRIGGER rollback_hidden_batch BEFORE DELETE ON threads WHEN OLD.id='hidden-subagent' BEGIN SELECT RAISE(ROLLBACK,'injected batch failure'); END;")
            .unwrap();

        let result = execute_delete_hidden(&paths, &candidates).unwrap();
        assert_eq!(result.deleted_threads, 1);
        assert_eq!(result.failed_threads, 1);
        assert!(!result.items[1].residuals.is_empty());
        let conn = Connection::open(paths.state_db()).unwrap();
        assert!(!thread_exists(&conn, "hidden-internal"));
        assert!(thread_exists(&conn, "hidden-subagent"));
        assert!(!root.join("sessions/rollout-hidden-internal.jsonl").exists());
        assert!(!root.join("sessions/rollout-hidden-subagent.jsonl").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_rejects_missing_state_db_even_for_empty_candidates() {
        let root = unique_temp_dir("missing-state-db");
        let paths = CodexPaths::new(&root);

        assert!(super::execute_delete_archived(&paths, &[]).is_err());
        assert!(execute_delete_hidden(&paths, &[]).is_err());

        let candidate = [super::HiddenThreadSelection {
            id: "11111111-1111-4111-8111-111111111111".into(),
            fingerprint: None,
        }];
        assert!(super::execute_delete_archived(&paths, &candidate).is_err());
        assert!(execute_delete_hidden(&paths, &candidate).is_err());
        let _ = fs::remove_dir_all(root);
    }

    fn assert_hidden_result_preserved_visible(result: &HiddenThreadDeleteResult) {
        assert_eq!(
            result.after_visible_threads, result.before.visible_threads,
            "visible thread count changed"
        );
    }

    fn hidden_cleanup_fixture(label: &str) -> PathBuf {
        let root = unique_temp_dir(label);
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        for id in [
            "main-visible",
            "hidden-subagent",
            "hidden-internal",
            "archived-hidden",
        ] {
            fs::write(
                sessions.join(format!("rollout-{id}.jsonl")),
                valid_completed_rollout(id),
            )
            .unwrap();
        }
        fs::write(
            root.join("session_index.jsonl"),
            [
                session_index_line(&root, "main-visible"),
                session_index_line(&root, "hidden-subagent"),
                session_index_line(&root, "hidden-internal"),
                session_index_line(&root, "archived-hidden"),
            ]
            .join("\n"),
        )
        .unwrap();
        let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads(
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                thread_source TEXT NOT NULL,
                parent_thread_id TEXT,
                agent_path TEXT,
                agent_nickname TEXT,
                agent_role TEXT,
                has_user_event INTEGER NOT NULL,
                first_user_message TEXT NOT NULL,
                preview TEXT NOT NULL,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0,
                archived_at INTEGER
            );
            CREATE TABLE thread_dynamic_tools(thread_id TEXT NOT NULL);
            CREATE TABLE thread_spawn_edges(parent_thread_id TEXT NOT NULL, child_thread_id TEXT NOT NULL);
            CREATE TABLE agent_job_items(id TEXT PRIMARY KEY, assigned_thread_id TEXT, status TEXT NOT NULL);",
        )
        .unwrap();
        insert_thread(
            &conn,
            &root,
            ThreadFixtureRow {
                id: "main-visible",
                source: "codex",
                thread_source: "user",
                parent_thread_id: None,
                updated_at: 10,
                archived: 0,
            },
        );
        insert_thread(
            &conn,
            &root,
            ThreadFixtureRow {
                id: "hidden-subagent",
                source: "codex",
                thread_source: "subagent",
                parent_thread_id: Some("main-visible"),
                updated_at: 20,
                archived: 0,
            },
        );
        insert_thread(
            &conn,
            &root,
            ThreadFixtureRow {
                id: "hidden-internal",
                source: "exec",
                thread_source: "user",
                parent_thread_id: None,
                updated_at: 30,
                archived: 0,
            },
        );
        insert_thread(
            &conn,
            &root,
            ThreadFixtureRow {
                id: "archived-hidden",
                source: "codex",
                thread_source: "subagent",
                parent_thread_id: Some("main-visible"),
                updated_at: 40,
                archived: 1,
            },
        );
        conn.execute(
            "INSERT INTO thread_dynamic_tools(thread_id) VALUES('main-visible'), ('hidden-subagent'), ('hidden-internal')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_spawn_edges(parent_thread_id, child_thread_id) VALUES('main-visible', 'hidden-subagent'), ('hidden-subagent', 'hidden-internal')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_job_items(id, assigned_thread_id, status) VALUES('job-hidden-subagent', 'hidden-subagent', 'completed')",
            [],
        )
        .unwrap();
        root
    }

    struct ThreadFixtureRow<'a> {
        id: &'a str,
        source: &'a str,
        thread_source: &'a str,
        parent_thread_id: Option<&'a str>,
        updated_at: i64,
        archived: i64,
    }

    fn insert_thread(conn: &Connection, root: &Path, row: ThreadFixtureRow<'_>) {
        let rollout = root
            .join("sessions")
            .join(format!("rollout-{}.jsonl", row.id));
        let first_user_message = if row.id == "hidden-internal" {
            "read-only subagent verification"
        } else {
            "user request"
        };
        conn.execute(
            "INSERT INTO threads(
                id, rollout_path, created_at, updated_at, source, thread_source, parent_thread_id,
                agent_path, agent_nickname, agent_role, has_user_event, first_user_message,
                preview, cwd, title, archived, archived_at
             ) VALUES(?1, ?2, 1, ?3, ?4, ?5, ?6, NULL, NULL, NULL, ?7, ?8, ?8, '/tmp', ?1, ?9, ?10)",
            (
                row.id,
                rollout.display().to_string(),
                row.updated_at,
                row.source,
                row.thread_source,
                row.parent_thread_id,
                if row.id == "hidden-internal" { 0 } else { 1 },
                first_user_message,
                row.archived,
                if row.archived == 0 {
                    None
                } else {
                    Some(1_i64)
                },
            ),
        )
        .unwrap();
    }

    fn valid_completed_rollout(id: &str) -> String {
        [
            json!({"type":"session_meta","payload":{"id":id}}).to_string(),
            json!({"type":"turn_started","turn_id":"turn-fixture"}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-fixture"}})
                .to_string(),
        ]
        .join("\n")
    }

    fn append_hidden_thread(root: &Path, id: &str, updated_at: i64) {
        let rollout = root.join("sessions").join(format!("rollout-{id}.jsonl"));
        fs::write(&rollout, valid_completed_rollout(id)).unwrap();
        let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
        insert_thread(
            &conn,
            root,
            ThreadFixtureRow {
                id,
                source: "codex",
                thread_source: "subagent",
                parent_thread_id: Some("main-visible"),
                updated_at,
                archived: 0,
            },
        );
        let index_path = root.join("session_index.jsonl");
        let mut index = fs::read_to_string(&index_path).unwrap();
        if !index.is_empty() {
            index.push('\n');
        }
        index.push_str(&session_index_line(root, id));
        fs::write(index_path, index).unwrap();
    }

    fn candidate<'a>(
        plan: &'a super::HiddenThreadDeletePlan,
        id: &str,
    ) -> &'a super::HiddenThreadCandidate {
        plan.candidates
            .iter()
            .find(|candidate| candidate.id == id)
            .unwrap()
    }

    fn selection(plan: &super::HiddenThreadDeletePlan, id: &str) -> HiddenThreadSelection {
        let candidate = candidate(plan, id);
        HiddenThreadSelection {
            id: candidate.id.clone(),
            fingerprint: candidate.fingerprint.clone(),
        }
    }

    fn selections_for_plan(plan: &super::HiddenThreadDeletePlan) -> Vec<HiddenThreadSelection> {
        plan.candidates
            .iter()
            .map(|candidate| HiddenThreadSelection {
                id: candidate.id.clone(),
                fingerprint: candidate.fingerprint.clone(),
            })
            .collect()
    }

    fn session_index_line(root: &Path, id: &str) -> String {
        json!({
            "id": id,
            "path": root.join("sessions").join(format!("rollout-{id}.jsonl")),
        })
        .to_string()
    }

    fn thread_exists(conn: &Connection, id: &str) -> bool {
        conn.query_row("SELECT count(*) FROM threads WHERE id=?1", [id], |row| {
            row.get::<_, u64>(0)
        })
        .unwrap()
            > 0
    }

    fn count_rows(conn: &Connection, table: &str) -> u64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "nexushub-archive-{label}-{}-{counter}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn execute_delete_hidden_rejects_unlisted_or_visible_ids_and_reports_partial_failure() {
        let root = hidden_cleanup_fixture("ok-delete-hidden-bounds");
        let paths = CodexPaths::new(&root);
        let plan = plan_delete_hidden(&paths).unwrap();
        let valid_subagent = selection(&plan, "hidden-subagent");
        let valid_internal = selection(&plan, "hidden-internal");

        // First candidate succeeds; second is a visible main thread (rejected by O(K) target set check) and stops early before third
        let mixed = vec![
            valid_subagent.clone(),
            HiddenThreadSelection {
                id: "main-visible".to_string(),
                fingerprint: valid_subagent.fingerprint.clone(),
            },
            valid_internal,
        ];

        let result = execute_delete_hidden(&paths, &mixed).unwrap();
        assert_eq!(result.deleted_threads, 1);
        assert_eq!(result.failed_threads, 1);
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].id, "hidden-subagent");
        assert_eq!(result.items[0].status, "deleted");
        assert_eq!(result.items[1].id, "main-visible");
        assert_eq!(result.items[1].status, "failed");
        assert_hidden_result_preserved_visible(&result);
        assert!(!root.join("sessions/rollout-hidden-subagent.jsonl").exists());
        assert!(root.join("sessions/rollout-hidden-internal.jsonl").exists());

        let _ = fs::remove_dir_all(root);
    }
}
