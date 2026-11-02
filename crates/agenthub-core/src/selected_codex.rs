//! Native Codex archive/restore rules and direct selected deletion.
use crate::codex::{rollout_has_confirmed_inactive_state, rollout_has_running_signal, CodexPaths};
use anyhow::{ensure, Context, Result};
use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::Mutex,
};

pub mod tree;

static MUTATION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedCodexPreview {
    pub id: String,
    pub title: String,
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
    pub fingerprint: String,
}

pub(crate) fn open(paths: &CodexPaths, writable: bool) -> Result<Connection> {
    ensure_safe_path(&paths.home, &paths.state_db())?;
    let conn = Connection::open_with_flags(
        paths.state_db(),
        if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

fn snapshot(
    paths: &CodexPaths,
    conn: &Connection,
    id: &str,
    expected_archived: Option<bool>,
) -> Result<SelectedCodexPreview> {
    let columns = crate::archive::table_columns(conn, "threads")?;
    let row = row_value(conn, "threads", "id", id)?.context("线程不存在")?;
    let archived = row.get("archived").and_then(Value::as_i64).unwrap_or(0) != 0
        || row.get("archived_at").and_then(Value::as_i64).unwrap_or(0) > 0;
    if let Some(expected_archived) = expected_archived {
        ensure!(
            archived == expected_archived,
            "线程归档状态与操作不匹配，请刷新列表"
        );
    }
    for field in [
        "active_turn_id",
        "activeTurnId",
        "active_job_id",
        "activeJobId",
        "running_job_id",
        "runningJobId",
    ] {
        ensure!(
            row.get(field)
                .is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty())),
            "线程仍有活动任务，暂不能归档或恢复"
        );
    }
    for field in ["status", "state"] {
        if let Some(status) = row.get(field).and_then(Value::as_str) {
            ensure!(
                matches!(
                    status.to_ascii_lowercase().as_str(),
                    "idle"
                        | "recent"
                        | "completed"
                        | "complete"
                        | "done"
                        | "failed"
                        | "cancelled"
                        | "canceled"
                        | "aborted"
                        | "archived"
                        | "stopped"
                        | "running"
                        | "active"
                        | "in_progress"
                        | "inprogress"
                        | "pending"
                        | "submitting"
                ),
                "线程活动状态无法确认，暂不能归档或恢复"
            );
        }
        ensure!(
            !row.get(field)
                .and_then(Value::as_str)
                .is_some_and(|s| matches!(
                    s.to_ascii_lowercase().as_str(),
                    "running" | "active" | "in_progress" | "inprogress" | "pending" | "submitting"
                )),
            "线程仍在运行，暂不能归档或恢复"
        );
    }
    for (table, key) in [
        ("jobs", "thread_id"),
        ("agent_job_items", "assigned_thread_id"),
    ] {
        let cols = crate::archive::table_columns(conn, table)?;
        if cols.contains(key) && cols.contains("status") {
            let active: bool = conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {key}=?1 AND lower(status) IN ('running','active','in_progress','pending','submitting'))"), [id], |r| r.get(0))?;
            ensure!(!active, "线程仍有关联的运行任务，暂不能归档或恢复");
        }
    }
    let index_rows = &read_index(paths)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    let mut files = BTreeSet::new();
    if let Some(path) = row
        .get("rollout_path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        files.insert(PathBuf::from(path));
    }
    for entry in index_rows {
        if entry.get("id").and_then(Value::as_str) == Some(id) {
            if let Some(path) = entry
                .get("path")
                .or_else(|| entry.get("rollout_path"))
                .and_then(Value::as_str)
            {
                files.insert(PathBuf::from(path));
            }
        }
    }
    // Only known native stores and exact filename suffixes may supply additional files.
    for root in [paths.sessions_dir(), paths.home.join("archived_sessions")] {
        if !root.exists() {
            continue;
        }
        ensure_safe_path(&paths.home, &root)?;
        for entry in walkdir::WalkDir::new(&root).follow_links(false) {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy();
            if name.starts_with("rollout-") && name.ends_with(&format!("-{id}.jsonl")) {
                files.insert(entry.path().to_path_buf());
            }
        }
    }
    ensure!(!files.is_empty(), "缺少可验证的线程文件，暂不能归档或恢复");
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(&row)?);
    let mut bytes = 0;
    for entry in index_rows {
        if entry.get("id").and_then(Value::as_str) == Some(id) {
            digest.update(serde_json::to_vec(entry)?);
        }
    }
    for file in &files {
        ensure!(
            file.starts_with(paths.sessions_dir())
                || file.starts_with(paths.home.join("archived_sessions")),
            "线程文件超出原生会话目录"
        );
        ensure_safe_path(&paths.home, file)?;
        let actual = fs::canonicalize(file)?;

        ensure!(
            session_header_id(file)?.as_deref() == Some(id),
            "线程文件身份与选中 ID 不一致"
        );
        ensure!(
            !rollout_has_running_signal(file)?,
            "线程文件仍有运行信号，暂不能归档或恢复"
        );
        ensure!(
            rollout_has_confirmed_inactive_state(file)?,
            "线程终态无法确认，暂不能归档或恢复"
        );
        if columns.contains("rollout_path") {
            let mut stmt = conn.prepare(
                "SELECT rollout_path FROM threads WHERE id != ?1 AND rollout_path IS NOT NULL",
            )?;
            for path in stmt.query_map([id], |r| r.get::<_, String>(0))? {
                ensure!(
                    !fs::canonicalize(path?).is_ok_and(|p| p == actual),
                    "文件仍被其他线程引用"
                );
            }
        }
        for entry in index_rows {
            if entry.get("id").and_then(Value::as_str) == Some(id) {
                continue;
            }
            if let Some(path) = entry
                .get("path")
                .or_else(|| entry.get("rollout_path"))
                .and_then(Value::as_str)
            {
                ensure!(
                    !fs::canonicalize(path).is_ok_and(|p| p == actual),
                    "文件仍被其他线程索引引用"
                );
            }
        }
        let metadata = fs::metadata(file)?;
        ensure!(metadata.is_file(), "线程目标不是普通文件");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.nlink() == 1,
                "线程文件有其他硬链接，暂不能归档或恢复"
            );
            digest.update(metadata.ino().to_le_bytes());
            digest.update(metadata.dev().to_le_bytes());
            digest.update(metadata.ctime().to_le_bytes());
            digest.update(metadata.ctime_nsec().to_le_bytes());
        }
        digest.update(file.as_os_str().as_encoded_bytes());
        let hash = content_hash(file)?;
        let after = fs::metadata(file)?;
        ensure!(
            metadata.len() == after.len() && metadata.modified()? == after.modified()?,
            "线程文件在扫描期间变化，请重新扫描"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec()
                ) == (after.dev(), after.ino(), after.ctime(), after.ctime_nsec()),
                "线程文件在扫描期间变化，请重新扫描"
            );
        }
        digest.update(hash);
        bytes += metadata.len();
    }
    Ok(SelectedCodexPreview {
        id: id.to_string(),
        title: row
            .get("name")
            .or_else(|| row.get("title"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(id)
            .to_string(),
        paths: files.into_iter().collect(),
        bytes,
        fingerprint: hex::encode(digest.finalize()),
    })
}

pub fn preview_archive(
    paths: &CodexPaths,
    id: &str,
    archive: bool,
) -> Result<SelectedCodexPreview> {
    snapshot(paths, &open(paths, false)?, id, Some(!archive))
}

pub fn execute_archive(
    paths: &CodexPaths,
    id: &str,
    archive: bool,
    fingerprint: &str,
) -> Result<()> {
    let _guard = MUTATION_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("线程管理锁不可用"))?;
    let mut conn = open(paths, true)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure!(
        snapshot(paths, &tx, id, Some(!archive))?.fingerprint == fingerprint,
        "线程或文件已变化，请重新预览"
    );
    let columns = crate::archive::table_columns(&tx, "threads")?;
    ensure!(columns.contains("archived_at"), "原生归档字段不可用");
    let at = archive.then(|| chrono::Utc::now().timestamp());
    let changed = if columns.contains("archived") {
        tx.execute(
            "UPDATE threads SET archived=?2,archived_at=?3 WHERE id=?1",
            rusqlite::params![id, archive, at],
        )?
    } else {
        tx.execute(
            "UPDATE threads SET archived_at=?2 WHERE id=?1",
            rusqlite::params![id, at],
        )?
    };
    ensure!(changed == 1, "线程身份已变化");
    tx.commit()?;
    Ok(())
}

fn content_hash(path: &Path) -> Result<Vec<u8>> {
    let mut reader = fs::File::open(path)?;
    let mut buffer = [0; 65536];
    let mut digest = Sha256::new();
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(digest.finalize().to_vec())
}

fn session_header_id(path: &Path) -> Result<Option<String>> {
    let mut line = String::new();
    BufReader::new(fs::File::open(path)?)
        .take(1024 * 1024)
        .read_line(&mut line)?;
    let header: Value = serde_json::from_str(&line).context("线程文件头损坏")?;
    ensure!(
        header.get("type").and_then(Value::as_str) == Some("session_meta"),
        "未知线程文件格式"
    );
    Ok(header
        .pointer("/payload/id")
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn ensure_safe_path(root: &Path, path: &Path) -> Result<()> {
    let relative = path.strip_prefix(root).context("目标超出 Codex 数据目录")?;
    let mut current = root.to_path_buf();
    ensure!(
        !fs::symlink_metadata(&current)?.file_type().is_symlink(),
        "Codex 数据目录不能是符号链接"
    );
    for component in relative.components() {
        ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "路径包含不安全分量"
        );
        current.push(component);
        ensure!(
            !fs::symlink_metadata(&current)?.file_type().is_symlink(),
            "线程管理不接受符号链接"
        );
    }
    ensure!(
        fs::canonicalize(path)?.starts_with(fs::canonicalize(root)?),
        "目标超出 Codex 数据目录"
    );
    Ok(())
}

fn read_index(paths: &CodexPaths) -> Result<String> {
    let path = paths.session_index();
    if !path.try_exists()? {
        return Ok(String::new());
    }
    ensure_safe_path(&paths.home, &path)?;
    let text = fs::read_to_string(path)?;
    for line in text.lines().filter(|s| !s.trim().is_empty()) {
        serde_json::from_str::<Value>(line).context("会话索引损坏，暂不能归档或恢复")?;
    }
    Ok(text)
}

fn row_value(conn: &Connection, table: &str, key: &str, id: &str) -> Result<Option<Value>> {
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table} WHERE {key}=?1"))?;
    let names = stmt
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    Ok(stmt
        .query_row([id], |r| read_row_value(r, &names))
        .optional()?)
}

fn read_row_value(row: &rusqlite::Row<'_>, names: &[String]) -> rusqlite::Result<Value> {
    let mut values = serde_json::Map::new();
    for (i, name) in names.iter().enumerate() {
        let value = match row.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => Value::from(n),
            ValueRef::Real(n) => Value::from(n),
            ValueRef::Text(s) => Value::from(String::from_utf8_lossy(s).into_owned()),
            ValueRef::Blob(b) => serde_json::json!({"hex":hex::encode(b)}),
        };
        values.insert(name.clone(), value);
    }
    Ok(Value::Object(values))
}
