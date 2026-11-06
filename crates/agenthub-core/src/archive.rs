use crate::codex::{hidden_thread_metadata_category, CodexPaths, ThreadVisibilityMetadata};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

pub mod hidden_scan;

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

pub(crate) fn list_archived_threads(paths: &CodexPaths) -> Result<Vec<HiddenThreadCandidate>> {
    let conn = crate::selected_codex::open(paths, false)?;
    conn.execute_batch("BEGIN DEFERRED")?;
    let rows = hidden_visibility_rows(&conn, &crate::scan_work::ScanWork::default())?;
    let roots = rows
        .iter()
        .filter(|r| r.archived)
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    let edges = visibility_edges(&conn, &rows)?;
    let ids = crate::selected_codex::tree::descendants_from_edges(&edges, &roots)?;
    let by_id: BTreeMap<_, _> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
    Ok(ids
        .into_iter()
        .map(|id| {
            let row = by_id.get(id.as_str());
            HiddenThreadCandidate {
                title: row.map_or_else(|| id.clone(), |r| r.title.clone()),
                fingerprint: row.map(|r| r.fingerprint.clone()),
                id,
                allowed: true,
                reason: None,
            }
        })
        .collect())
}

pub(crate) fn plan_delete_hidden_candidates(paths: &CodexPaths) -> Result<HiddenThreadDeletePlan> {
    hidden_delete_plan_with_work(paths, &crate::scan_work::ScanWork::default())
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
    let edges = visibility_edges(&conn, &rows)?;
    let hidden_ids = crate::selected_codex::tree::descendants_from_edges(&edges, &hidden_ids)?;
    let plan = HiddenThreadDeletePlan {
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

fn visibility_edges(
    conn: &Connection,
    rows: &[HiddenVisibilityRow],
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        for parent in &row.parent_ids {
            edges
                .entry(parent.clone())
                .or_default()
                .insert(row.id.clone());
        }
    }
    crate::selected_codex::tree::append_spawn_table_edges(conn, &mut edges)?;
    Ok(edges)
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
    let alternate_parent_expr = first_existing(columns, &["parentThreadId"]).unwrap_or("NULL");
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
         {first_user_message_expr}, {preview_expr}, CAST({updated_expr} AS TEXT), {alternate_parent_expr}"
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
    let alternate_parent: Option<String> = row.get(13).ok();
    let parent_thread_id: Option<String> = row.get(4).ok().or_else(|| alternate_parent.clone());
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
    let mut parent_ids = extract_row_parents(parent_thread_id.as_deref(), source.as_deref());
    if let Some(parent) =
        alternate_parent.filter(|parent| !parent.is_empty() && !parent_ids.contains(parent))
    {
        parent_ids.push(parent);
    }
    Ok(HiddenVisibilityRow {
        title: title.unwrap_or_else(|| id.clone()),
        id,
        archived,
        hidden_category,
        parent_ids,
        fingerprint,
    })
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

#[cfg(test)]
mod tests {
    use super::hidden_delete_plan_with_work;
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
    fn one_visibility_snapshot_preserves_every_parent_source() {
        let root = hidden_cleanup_fixture("parent-sources");
        let paths = CodexPaths::new(&root);
        let conn = Connection::open(paths.state_db()).unwrap();
        conn.execute_batch("ALTER TABLE threads ADD COLUMN parentThreadId TEXT;")
            .unwrap();
        conn.execute("UPDATE threads SET parent_thread_id='snake-parent', parentThreadId='camel-parent', source=?1 WHERE id='hidden-subagent'",
            [json!({"parent_thread_id":"json-parent", "subagent":{"thread_spawn":{"parent_thread_id":"spawn-parent"}}}).to_string()]).unwrap();
        let rows =
            super::hidden_visibility_rows(&conn, &crate::scan_work::ScanWork::default()).unwrap();
        let edges = super::visibility_edges(&conn, &rows).unwrap();
        for parent in [
            "snake-parent",
            "camel-parent",
            "json-parent",
            "spawn-parent",
            "main-visible",
        ] {
            assert!(edges[parent].contains("hidden-subagent"));
        }
        drop(conn);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_delete_hidden_selects_non_archived_hidden_threads() {
        let root = hidden_cleanup_fixture("plan-hidden");

        let plan = hidden_delete_plan_with_work(
            &CodexPaths::new(&root),
            &crate::scan_work::ScanWork::default(),
        )
        .unwrap();

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
    fn archived_preview_reuses_titles_fingerprints_and_descendant_scope() {
        let root = hidden_cleanup_fixture("archived-batch");
        let paths = CodexPaths::new(&root);
        let conn = Connection::open(paths.state_db()).unwrap();
        conn.execute(
            "UPDATE threads SET title='Archived title' WHERE id='archived-hidden'",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_spawn_edges VALUES('archived-hidden','hidden-internal')",
            [],
        )
        .unwrap();
        let rows = super::list_archived_threads(&paths).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .find(|r| r.id == "archived-hidden")
                .unwrap()
                .title,
            "Archived title"
        );
        for row in rows {
            let fingerprint = super::verify_archived_target_fingerprint(
                &conn,
                &super::table_columns(&conn, "threads").unwrap(),
                true,
                &row.id,
            )
            .unwrap();
            assert_eq!(row.fingerprint, fingerprint);
        }
        drop(conn);
        fs::remove_dir_all(root).unwrap();
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

    fn session_index_line(root: &Path, id: &str) -> String {
        json!({
            "id": id,
            "path": root.join("sessions").join(format!("rollout-{id}.jsonl")),
        })
        .to_string()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "agenthub-archive-{label}-{}-{counter}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }
}
