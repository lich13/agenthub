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
    pub deleted_files: u64,
    pub deleted_bytes: u64,
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub(crate) fn spawn_edges(paths: &CodexPaths) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if !paths.state_db().exists() {
        return Ok(edges);
    }
    let conn = Connection::open_with_flags(paths.state_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let available = crate::archive::table_columns(&conn, "threads")?;
    let selected = ["id", "parent_thread_id", "parentThreadId", "source"]
        .into_iter()
        .filter(|column| available.contains(*column))
        .map(quoted)
        .collect::<Vec<_>>();
    let mut statement = conn.prepare(&format!("SELECT {} FROM threads", selected.join(",")))?;
    let names: Vec<_> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
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
    let columns = crate::archive::table_columns(&conn, "thread_spawn_edges")?;
    if columns.contains("parent_thread_id") && columns.contains("child_thread_id") {
        let mut stmt =
            conn.prepare("SELECT parent_thread_id, child_thread_id FROM thread_spawn_edges")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (parent, child) = row?;
            edges.entry(parent).or_default().insert(child);
        }
    }
    Ok(edges)
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
    rollouts: BTreeSet<String>,
    errors: Vec<String>,
}

struct BatchInventory {
    files_by_id: BTreeMap<String, BTreeSet<PathBuf>>,
    rollouts: BTreeSet<String>,
    errors: Vec<String>,
}

fn collect_inventory_by_id(paths: &CodexPaths, ids: &BTreeSet<String>) -> BatchInventory {
    let mut result = BatchInventory {
        files_by_id: BTreeMap::new(),
        rollouts: ids.clone(),
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
            if let Some(name) = RolloutName::parse(&path) {
                result.rollouts.insert(name.rollout_id);
            }
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
                    if !ids.iter().any(|id| line.contains(id.as_str())) {
                        continue;
                    }
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
                            result.rollouts.insert(name.rollout_id);
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
        rollouts: batch.rollouts,
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

pub(crate) fn ensure_tree_not_running(paths: &CodexPaths, root_id: &str) -> Result<()> {
    let tree_ids = descendants(paths, &[root_id.to_string()])?;
    let conn = paths
        .state_db()
        .exists()
        .then(|| Connection::open_with_flags(paths.state_db(), OpenFlags::SQLITE_OPEN_READ_ONLY))
        .transpose()?;
    ensure_ids_not_running(paths, &tree_ids, conn.as_ref())
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
    let _guard = MUTATION_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("线程管理锁不可用"))?;
    for id in selected {
        session_delete::valid_id(id)?;
    }
    let ids: BTreeSet<_> = selected.iter().cloned().collect();
    let inventory = inventory(paths, &ids);
    let mut outcome = session_delete::Outcome {
        residuals: inventory.errors,
        ..Default::default()
    };
    let files: Vec<_> = inventory.files.iter().cloned().collect();
    for (idx, path) in files.iter().enumerate() {
        if session_delete::within(&paths.home, path) {
            if !outcome.remove_strict(path) {
                for rem in &files[idx + 1..] {
                    if fs::symlink_metadata(rem).is_ok() {
                        outcome.residuals.push(rem.display().to_string());
                    }
                }
                break;
            }
        } else {
            outcome
                .residuals
                .push(format!("无法定位线程文件: {}", path.display()));
            for rem in &files[idx + 1..] {
                if fs::symlink_metadata(rem).is_ok() {
                    outcome.residuals.push(rem.display().to_string());
                }
            }
            break;
        }
    }
    if outcome.residuals.is_empty() {
        let index = paths.session_index();
        outcome.attempt(&index, || {
            session_delete::filter_jsonl(&index, |row| {
                row["id"].as_str().is_some_and(|id| ids.contains(id))
            })
        });
    }
    if outcome.residuals.is_empty() {
        let history = paths.home.join("history.jsonl");
        outcome.attempt(&history, || {
            session_delete::filter_jsonl(&history, |row| session_delete::has_id(row, &ids))
        });
    }
    if outcome.residuals.is_empty() {
        let database = paths.state_db();
        outcome.attempt(&database, || {
            delete_database(&database, &ids, &inventory.rollouts, &inventory.files)
        });
    }
    let database = paths.state_db();
    if outcome.residuals.is_empty() {
        match fs::read_dir(&paths.home) {
            Ok(entries) => {
                for entry in entries {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(error) => {
                            outcome.residuals.push(error.to_string());
                            break;
                        }
                    };
                    if entry.file_name().to_str().is_some_and(|name| {
                        name.starts_with("thread_history_") && name.ends_with(".sqlite")
                    }) {
                        outcome.attempt(&entry.path(), || {
                            delete_database(
                                &entry.path(),
                                &ids,
                                &inventory.rollouts,
                                &inventory.files,
                            )
                        });
                        if !outcome.residuals.is_empty() {
                            break;
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => outcome.residuals.push(e.to_string()),
        }
    }
    outcome.finish();
    let deleted_ids: Vec<String> = if database.exists() {
        Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()
            .map(|conn| {
                ids.iter()
                    .filter(|id| {
                        row_value(&conn, "threads", "id", id).is_ok_and(|row| row.is_none())
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    } else {
        ids.into_iter().collect()
    };
    if outcome.residuals.is_empty() {
        update_delete_residuals(paths, &[], &deleted_ids);
    } else if !deleted_ids.is_empty() {
        update_delete_residuals(paths, &deleted_ids, &[]);
    }
    Ok(DeleteOutcome {
        deleted_ids,
        error: (!outcome.residuals.is_empty()).then(|| "部分内容未删除，请查看逐项结果".into()),
        residuals: outcome.residuals,
        deleted_files: outcome.files,
        deleted_bytes: outcome.bytes,
    })
}

#[cfg(test)]
mod tests;
