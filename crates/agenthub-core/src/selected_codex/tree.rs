//! Direct deletion of selected native storage. Reading and activity never gate removal.
use super::*;
use crate::{
    codex::rollout_file::{self, RolloutName},
    session_delete,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct TreePreview {
    pub selected: SelectedCodexPreview,
    pub ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
pub struct DeleteOutcome {
    pub deleted_ids: Vec<String>,
    pub residuals: Vec<String>,
    pub error: Option<String>,
    pub deleted_bytes: u64,
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub(crate) fn spawn_edges(paths: &CodexPaths) -> Result<BTreeMap<String, BTreeSet<String>>> {
    if !paths.state_db().exists() {
        return Ok(BTreeMap::new());
    }
    let conn = Connection::open_with_flags(paths.state_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    spawn_edges_with_connection(&conn)
}

pub(crate) fn spawn_edges_with_connection(
    conn: &Connection,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let available = crate::archive::table_columns(conn, "threads")?;
    let selected = ["id", "parent_thread_id", "parentThreadId", "source"]
        .into_iter()
        .filter(|column| available.contains(*column))
        .map(quoted)
        .collect::<Vec<_>>();
    let mut statement = conn.prepare(&format!("SELECT {} FROM threads", selected.join(",")))?;
    let names: Vec<_> = statement.column_names().map(str::to_owned).collect();
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let value = read_row_value(row, &names)?;
        let Some(id) = value["id"].as_str() else {
            continue;
        };
        let source = value["source"]
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or(Value::Null);
        for parent in [
            value.get("parent_thread_id"),
            value.get("parentThreadId"),
            source.pointer("/subagent/thread_spawn/parent_thread_id"),
            source.pointer("/parent_thread_id"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        {
            edges.entry(parent.into()).or_default().insert(id.into());
        }
    }
    append_spawn_table_edges(conn, &mut edges)?;
    Ok(edges)
}

pub(crate) fn append_spawn_table_edges(
    conn: &Connection,
    edges: &mut BTreeMap<String, BTreeSet<String>>,
) -> Result<()> {
    let columns = crate::archive::table_columns(conn, "thread_spawn_edges")?;
    if columns.contains("parent_thread_id") && columns.contains("child_thread_id") {
        let mut stmt =
            conn.prepare("SELECT parent_thread_id, child_thread_id FROM thread_spawn_edges")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (parent, child) = row?;
            edges.entry(parent).or_default().insert(child);
        }
    }
    Ok(())
}

pub(crate) fn descendants_from_edges(
    edges: &BTreeMap<String, BTreeSet<String>>,
    roots: &[String],
) -> Result<Vec<String>> {
    for id in roots {
        session_delete::valid_id(id)?;
    }
    let mut pending = roots.to_vec();
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        session_delete::valid_id(&id)?;
        if seen.insert(id.clone()) {
            if let Some(children) = edges.get(&id) {
                pending.extend(children.iter().cloned());
            }
        }
    }
    Ok(seen.into_iter().collect())
}

pub(crate) fn descendants(paths: &CodexPaths, roots: &[String]) -> Result<Vec<String>> {
    for id in roots {
        session_delete::valid_id(id)?;
    }
    descendants_from_edges(&spawn_edges(paths)?, roots)
}

struct Inventory {
    files: BTreeSet<PathBuf>,
    errors: Vec<String>,
}

struct BatchInventory {
    files_by_id: BTreeMap<String, BTreeSet<PathBuf>>,
    errors: Vec<String>,
}

fn collect_inventory_by_id(paths: &CodexPaths, ids: &BTreeSet<String>) -> BatchInventory {
    let mut result = BatchInventory {
        files_by_id: BTreeMap::new(),
        errors: Vec::new(),
    };
    if ids.is_empty() {
        return result;
    }
    let roots = [paths.sessions_dir(), paths.home.join("archived_sessions")];
    let mut add_for_id = |id: &str, path: PathBuf| {
        if (path.extension().is_some_and(|ext| ext == "jsonl")
            || rollout_file::is_compressed(&path))
            && roots.iter().any(|root| session_delete::within(root, &path))
        {
            let entry = result.files_by_id.entry(id.to_owned()).or_default();
            for sibling in [
                rollout_file::plain_path(&path),
                rollout_file::compressed_path(&path),
            ] {
                entry.insert(sibling);
            }
        }
    };
    if paths.state_db().exists() {
        let read = (|| -> Result<()> {
            let conn =
                Connection::open_with_flags(paths.state_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            if crate::archive::table_columns(&conn, "threads")?.contains("rollout_path") {
                let ids_json = serde_json::to_string(ids)?;
                let mut stmt = conn.prepare(
                    "SELECT id, rollout_path FROM threads WHERE rollout_path IS NOT NULL AND id IN (SELECT value FROM json_each(?1))",
                )?;
                for row in stmt.query_map([&ids_json], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })? {
                    let (id, path) = row?;
                    add_for_id(&id, PathBuf::from(path));
                }
            }
            Ok(())
        })();
        if let Err(e) = read {
            result.errors.push(format!("原生索引: {e}"));
        }
    }
    if let Ok(file) = fs::File::open(paths.session_index()) {
        for line in BufReader::new(file).lines() {
            match line {
                Ok(line) => {
                    if let Ok(row) = serde_json::from_str::<Value>(&line) {
                        if let Some(id) = row["id"].as_str().filter(|id| ids.contains(*id)) {
                            if let Some(path) = row
                                .get("path")
                                .or_else(|| row.get("rollout_path"))
                                .and_then(Value::as_str)
                            {
                                add_for_id(id, path.into());
                            }
                        }
                    }
                }
                Err(e) => {
                    result.errors.push(format!("会话名称索引: {e}"));
                    break;
                }
            }
        }
    }
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .follow_root_links(false)
        {
            match entry {
                Ok(entry) => {
                    if let Some(name) = RolloutName::parse(entry.path()) {
                        if ids.contains(&name.thread_id) {
                            result
                                .files_by_id
                                .entry(name.thread_id)
                                .or_default()
                                .insert(entry.into_path());
                        }
                    }
                }
                Err(e) => result.errors.push(e.to_string()),
            }
        }
    }
    for id in ids {
        result.files_by_id.entry(id.clone()).or_default().insert(
            paths
                .home
                .join("thread-writer-locks")
                .join(format!("{id}.lock")),
        );
    }
    let snapshots = paths.home.join("shell_snapshots");
    match fs::read_dir(&snapshots) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        if let Some(name) = entry.file_name().to_str() {
                            for id in ids {
                                if name
                                    .strip_prefix(id.as_str())
                                    .is_some_and(|suffix| suffix.starts_with('.'))
                                {
                                    result
                                        .files_by_id
                                        .entry(id.clone())
                                        .or_default()
                                        .insert(entry.path());
                                }
                            }
                        }
                    }
                    Err(error) => result
                        .errors
                        .push(format!("{}: {error}", snapshots.display())),
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => result
            .errors
            .push(format!("{}: {error}", snapshots.display())),
    }
    let visualizations = paths.home.join("visualizations");
    if visualizations.exists() {
        let mut entries = walkdir::WalkDir::new(&visualizations)
            .follow_links(false)
            .follow_root_links(false)
            .into_iter();
        while let Some(entry) = entries.next() {
            match entry {
                Ok(entry) => {
                    if let Some(id) = entry.file_name().to_str().filter(|id| ids.contains(*id)) {
                        let id_owned = id.to_owned();
                        result
                            .files_by_id
                            .entry(id_owned)
                            .or_default()
                            .insert(entry.into_path());
                        entries.skip_current_dir();
                    }
                }
                Err(e) => result.errors.push(e.to_string()),
            }
        }
    }
    result
}

fn inventory(paths: &CodexPaths, ids: &BTreeSet<String>) -> Inventory {
    let batch = collect_inventory_by_id(paths, ids);
    Inventory {
        files: batch.files_by_id.into_values().flatten().collect(),
        errors: batch.errors,
    }
}

const DELETE_RESIDUALS_FILE: &str = ".agenthub-delete-residuals.json";

fn delete_residuals_path(paths: &CodexPaths) -> PathBuf {
    paths.home.join(DELETE_RESIDUALS_FILE)
}

fn load_recorded_delete_residuals(paths: &CodexPaths) -> BTreeSet<String> {
    let path = delete_residuals_path(paths);
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BTreeSet<String>>(&bytes).ok())
        .unwrap_or_default()
}

pub(crate) fn is_recorded_delete_residual(paths: &CodexPaths, id: &str) -> bool {
    if session_delete::valid_id(id).is_err() {
        return false;
    }
    load_recorded_delete_residuals(paths).contains(id)
}

fn update_delete_residuals(paths: &CodexPaths, add: &[String], remove: &[String]) {
    let mut current = load_recorded_delete_residuals(paths);
    let mut changed = false;
    for id in remove {
        if current.remove(id) {
            changed = true;
        }
    }
    for id in add {
        if session_delete::valid_id(id).is_ok() && current.insert(id.clone()) {
            changed = true;
        }
    }
    let target = delete_residuals_path(paths);
    if current.is_empty() {
        if target.exists() {
            let _ = fs::remove_file(target);
        }
        return;
    }
    if !changed && target.exists() {
        return;
    }
    if let Ok(payload) = serde_json::to_vec(&current) {
        let tmp = paths.home.join(format!(
            ".agenthub-delete-residuals.{}.tmp",
            uuid::Uuid::new_v4()
        ));
        if fs::write(&tmp, payload).is_ok() && fs::rename(&tmp, &target).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}

pub(crate) fn has_orphan_artifacts(paths: &CodexPaths, id: &str) -> bool {
    if session_delete::valid_id(id).is_err() {
        return false;
    }
    let ids = BTreeSet::from([id.to_string()]);
    let inv = inventory(paths, &ids);
    if inv
        .files
        .iter()
        .any(|path| fs::symlink_metadata(path).is_ok())
    {
        return true;
    }
    for shared in [paths.session_index(), paths.home.join("history.jsonl")] {
        if let Ok(file) = fs::File::open(shared) {
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                if let Ok(row) = serde_json::from_str::<Value>(&line) {
                    if session_delete::has_id(&row, &ids) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub(crate) fn is_writer_locked(paths: &CodexPaths, id: &str) -> bool {
    let lock_path = paths
        .home
        .join("thread-writer-locks")
        .join(format!("{id}.lock"));
    if !lock_path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;
        let Ok(file) = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
        else {
            return false;
        };
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
        if rc == 0 {
            unsafe {
                let _ = libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
            false
        } else {
            let err = std::io::Error::last_os_error();
            matches!(
                err.raw_os_error(),
                Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
            )
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub(crate) fn ensure_ids_not_running(
    paths: &CodexPaths,
    tree_ids: &[String],
    conn: Option<&Connection>,
) -> Result<()> {
    for id in tree_ids {
        ensure!(
            !is_writer_locked(paths, id),
            "线程 {id} 仍持有写入锁，无法清理"
        );
    }
    let Some(conn) = conn else {
        return Ok(());
    };
    let active_job_tables: Vec<(&str, &str)> = [
        ("jobs", "thread_id"),
        ("agent_job_items", "assigned_thread_id"),
    ]
    .into_iter()
    .filter(|(table, key)| {
        crate::archive::table_columns(conn, table)
            .is_ok_and(|cols| cols.contains(*key) && cols.contains("status"))
    })
    .collect();
    for id in tree_ids {
        if let Some(row) = row_value(conn, "threads", "id", id)? {
            for field in [
                "active_turn_id",
                "activeTurnId",
                "active_job_id",
                "activeJobId",
                "running_job_id",
                "runningJobId",
            ] {
                ensure!(
                    row.get(field).is_none_or(|v| {
                        v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty())
                    }),
                    "线程 {id} 仍有活动任务，无法清理"
                );
            }
            for field in ["status", "state"] {
                ensure!(
                    !row.get(field).and_then(Value::as_str).is_some_and(|s| {
                        matches!(
                            s.to_ascii_lowercase().as_str(),
                            "running"
                                | "active"
                                | "in_progress"
                                | "inprogress"
                                | "pending"
                                | "submitting"
                        )
                    }),
                    "线程 {id} 正在运行，无法清理"
                );
            }
        }
        for &(table, key) in &active_job_tables {
            let active: bool = conn.query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM {table} WHERE {key}=?1 AND lower(status) IN ('running','active','in_progress','pending','submitting'))"
                ),
                [id],
                |r| r.get(0),
            )?;
            ensure!(!active, "线程 {id} 仍有关联的运行任务，无法清理");
        }
    }
    Ok(())
}

pub fn preview(paths: &CodexPaths, id: &str, include_descendants: bool) -> Result<TreePreview> {
    session_delete::valid_id(id)?;
    let (ids, relation_error) = if include_descendants {
        match descendants(paths, &[id.into()]) {
            Ok(ids) => (ids, None),
            Err(error) => (vec![id.into()], Some(format!("父子关联无法读取: {error}"))),
        }
    } else {
        (vec![id.into()], None)
    };
    let mut found = inventory(paths, &ids.iter().cloned().collect());
    found.errors.extend(relation_error);
    let files: Vec<_> = found
        .files
        .into_iter()
        .filter(|p| fs::symlink_metadata(p).is_ok())
        .collect();
    let (_, bytes) = session_delete::size(&files);
    Ok(TreePreview {
        selected: SelectedCodexPreview {
            id: id.into(),
            title: id.into(),
            paths: files,
            bytes,
            fingerprint: String::new(),
        },
        ids,
        warnings: found.errors,
    })
}

/// Single-pass batch size estimator for `ids` (avoids `O(K * N)` directory walks during dry-run).
pub(crate) fn estimate_bytes_batch(paths: &CodexPaths, ids: &[String]) -> BTreeMap<String, u64> {
    let target_set: BTreeSet<String> = ids
        .iter()
        .filter(|id| session_delete::valid_id(id).is_ok())
        .cloned()
        .collect();
    if target_set.is_empty() {
        return BTreeMap::new();
    }
    collect_inventory_by_id(paths, &target_set)
        .files_by_id
        .into_iter()
        .filter_map(|(id, files)| {
            let existing: Vec<PathBuf> = files
                .into_iter()
                .filter(|p| fs::symlink_metadata(p).is_ok())
                .collect();
            if existing.is_empty() {
                None
            } else {
                let (_, bytes) = session_delete::size(&existing);
                Some((id, bytes))
            }
        })
        .collect()
}

fn delete_database(
    path: &Path,
    ids: &BTreeSet<String>,
    rollouts: &BTreeSet<String>,
    files: &BTreeSet<PathBuf>,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "原生数据库不能是链接"
    );
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut tables = tx
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    tables.sort_by_key(|name| name == "threads");
    let ids_json = serde_json::to_string(ids)?;
    let rollouts_json = serde_json::to_string(rollouts)?;
    let files_json = serde_json::to_string(files)?;
    for table in tables {
        if table.starts_with("sqlite_") || table == "_sqlx_migrations" {
            continue;
        }
        let columns = crate::archive::table_columns(&tx, &quoted(&table))?;
        if table == "agent_job_items" && columns.contains("assigned_thread_id") {
            tx.execute("UPDATE agent_job_items SET assigned_thread_id=NULL WHERE assigned_thread_id IN (SELECT value FROM json_each(?1))",[&ids_json])?;
            continue;
        }
        let mut predicates = Vec::new();
        for column in &columns {
            if column == "rollout_id" {
                predicates.push(format!(
                    "{} IN (SELECT value FROM json_each(?2))",
                    quoted(column)
                ));
            } else if column == "thread_id"
                || (table == "threads" && column == "id")
                || (table == "thread_spawn_edges"
                    && matches!(column.as_str(), "parent_thread_id" | "child_thread_id"))
            {
                predicates.push(format!(
                    "{} IN (SELECT value FROM json_each(?1))",
                    quoted(column)
                ));
            } else if table == "rollout_migration_skipped_rollouts" && column == "rollout_path" {
                predicates.push(format!(
                    "{} IN (SELECT value FROM json_each(?3))",
                    quoted(column)
                ));
            }
        }
        if !predicates.is_empty() {
            // Numbered parameters have a fixed arity, including identities absent from this table.
            tx.execute(&format!("DELETE FROM {} WHERE ({}) AND (?1 IS NOT NULL AND ?2 IS NOT NULL AND ?3 IS NOT NULL)",quoted(&table),predicates.join(" OR ")),rusqlite::params![ids_json,rollouts_json,files_json])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn execute_ids(paths: &CodexPaths, selected: &[String]) -> Result<DeleteOutcome> {
    execute_batch(paths, &[selected.to_vec()]).remove(0)
}

struct PendingDelete {
    ids: BTreeSet<String>,
    files: BTreeSet<PathBuf>,
    rollouts: BTreeSet<String>,
    outcome: session_delete::Outcome,
}

fn mutate_shared(
    pending: &mut [Result<PendingDelete>],
    path: &Path,
    action: impl FnOnce(&BTreeSet<String>, &BTreeSet<String>, &BTreeSet<PathBuf>) -> Result<()>,
) {
    let eligible: Vec<_> = pending
        .iter_mut()
        .filter_map(|item| item.as_mut().ok())
        .filter(|item| item.outcome.residuals.is_empty())
        .collect();
    if eligible.is_empty() {
        return;
    }
    let ids = eligible
        .iter()
        .flat_map(|item| item.ids.iter().cloned())
        .collect();
    let rollouts = eligible
        .iter()
        .flat_map(|item| item.rollouts.iter().cloned())
        .collect();
    let files = eligible
        .iter()
        .flat_map(|item| item.files.iter().cloned())
        .collect();
    if let Err(error) = action(&ids, &rollouts, &files) {
        for item in eligible {
            item.outcome
                .residuals
                .push(format!("{}: {error}", path.display()));
        }
    }
}

pub(crate) fn execute_batch(
    paths: &CodexPaths,
    selections: &[Vec<String>],
) -> Vec<Result<DeleteOutcome>> {
    if selections.is_empty() {
        return vec![];
    }
    let _guard = match MUTATION_LOCK.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return selections
                .iter()
                .map(|_| Err(anyhow::anyhow!("线程管理锁不可用")))
                .collect()
        }
    };
    let mut pending: Vec<_> = selections
        .iter()
        .map(|selected| -> Result<_> {
            for id in selected {
                session_delete::valid_id(id)?;
            }
            Ok(PendingDelete {
                ids: selected.iter().cloned().collect(),
                files: BTreeSet::new(),
                rollouts: BTreeSet::new(),
                outcome: session_delete::Outcome::default(),
            })
        })
        .collect();
    let ids = pending
        .iter()
        .filter_map(|item| item.as_ref().ok())
        .flat_map(|item| item.ids.iter().cloned())
        .collect();
    let inventory = collect_inventory_by_id(paths, &ids);
    let mut file_results = BTreeMap::<PathBuf, Vec<String>>::new();
    for item in pending.iter_mut().filter_map(|item| item.as_mut().ok()) {
        item.files = item
            .ids
            .iter()
            .filter_map(|id| inventory.files_by_id.get(id))
            .flatten()
            .cloned()
            .collect();
        item.rollouts = item.ids.clone();
        item.rollouts.extend(
            item.files
                .iter()
                .filter_map(|path| RolloutName::parse(path).map(|name| name.rollout_id)),
        );
        item.outcome.residuals.clone_from(&inventory.errors);
        let files: Vec<_> = item.files.iter().collect();
        for (idx, path) in files.iter().enumerate() {
            let residuals = file_results.entry((*path).clone()).or_insert_with(|| {
                let mut outcome = session_delete::Outcome::default();
                if session_delete::within(&paths.home, path) {
                    outcome.remove_strict(path);
                } else {
                    outcome
                        .residuals
                        .push(format!("无法定位线程文件: {}", path.display()));
                }
                item.outcome.bytes += outcome.bytes;
                outcome.residuals
            });
            if !residuals.is_empty() {
                item.outcome.residuals.extend(residuals.iter().cloned());
                item.outcome.residuals.extend(
                    files[idx + 1..]
                        .iter()
                        .filter(|path| fs::symlink_metadata(path).is_ok())
                        .map(|path| path.display().to_string()),
                );
                break;
            }
        }
    }
    let index = paths.session_index();
    mutate_shared(&mut pending, &index, |ids, _, _| {
        session_delete::filter_jsonl(&index, |row| {
            row["id"].as_str().is_some_and(|id| ids.contains(id))
        })
    });
    let history = paths.home.join("history.jsonl");
    mutate_shared(&mut pending, &history, |ids, _, _| {
        session_delete::filter_jsonl(&history, |row| session_delete::has_id(row, ids))
    });
    let database = paths.state_db();
    mutate_shared(&mut pending, &database, |ids, rollouts, files| {
        delete_database(&database, ids, rollouts, files)
    });
    if pending
        .iter()
        .filter_map(|item| item.as_ref().ok())
        .any(|item| item.outcome.residuals.is_empty())
    {
        let history_databases = (|| -> Result<Vec<PathBuf>> {
            let entries = match fs::read_dir(&paths.home) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
                Err(error) => return Err(error.into()),
            };
            let mut databases = Vec::new();
            for entry in entries {
                let entry = entry?;
                if entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("thread_history_") && name.ends_with(".sqlite")
                }) {
                    databases.push(entry.path());
                }
            }
            Ok(databases)
        })();
        match history_databases {
            Ok(databases) => {
                for database in databases {
                    mutate_shared(&mut pending, &database, |ids, rollouts, files| {
                        delete_database(&database, ids, rollouts, files)
                    });
                }
            }
            Err(error) => mutate_shared(&mut pending, &paths.home, |_, _, _| Err(error)),
        }
    }
    let conn = if database.exists() {
        Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
    } else {
        None
    };
    let mut record = BTreeSet::new();
    let mut clear = BTreeSet::new();
    let result = pending
        .into_iter()
        .map(|item| {
            item.map(|mut item| {
                item.outcome.finish();
                let deleted_ids: Vec<_> = item
                    .ids
                    .into_iter()
                    .filter(|id| {
                        !database.exists()
                            || conn.as_ref().is_some_and(|conn| {
                                row_value(conn, "threads", "id", id).is_ok_and(|row| row.is_none())
                            })
                    })
                    .collect();
                if item.outcome.residuals.is_empty() {
                    clear.extend(deleted_ids.iter().cloned());
                } else {
                    record.extend(deleted_ids.iter().cloned());
                }
                DeleteOutcome {
                    deleted_ids,
                    error: (!item.outcome.residuals.is_empty())
                        .then(|| "部分内容未删除，请查看逐项结果".into()),
                    residuals: item.outcome.residuals,
                    deleted_bytes: item.outcome.bytes,
                }
            })
        })
        .collect();
    // A shared target can belong to several selected trees. Keep any outstanding residual.
    clear.retain(|id| !record.contains(id));
    update_delete_residuals(
        paths,
        &record.into_iter().collect::<Vec<_>>(),
        &clear.into_iter().collect::<Vec<_>>(),
    );
    result
}

#[cfg(test)]
mod tests;
