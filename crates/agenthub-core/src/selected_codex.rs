//! Native Codex archive/restore rules and direct selected deletion.
use crate::codex::{
    rollout_file, rollout_has_confirmed_inactive_state, rollout_has_running_signal, CodexPaths,
};
use anyhow::{ensure, Context, Result};
use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::Mutex,
};

pub mod tree;

const ACTIVE_STATUSES: &[&str] = &[
    "running",
    "active",
    "in_progress",
    "inprogress",
    "pending",
    "submitting",
];

fn is_active_status(status: &str) -> bool {
    ACTIVE_STATUSES
        .iter()
        .any(|s| s.eq_ignore_ascii_case(status))
}

fn has_active_job(conn: &Connection, table: &str, key: &str, id: &str) -> rusqlite::Result<bool> {
    // Table and key come from the native schema inventory. Status literals are static.
    static VALUES: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| ACTIVE_STATUSES.join("','"));
    conn.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM {table} WHERE {key}=?1 AND lower(status) IN ('{}'))",
            *VALUES
        ),
        [id],
        |row| row.get(0),
    )
}

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

struct ArchiveInventory {
    columns: HashSet<String>,
    job_tables: Vec<(&'static str, &'static str)>,
    index_rows: Vec<Value>,
    files: Vec<PathBuf>,
    database_owners: HashMap<PathBuf, HashSet<String>>,
    index_owners: HashMap<PathBuf, HashSet<Option<String>>>,
}

impl ArchiveInventory {
    fn read(paths: &CodexPaths, conn: &Connection) -> Result<Self> {
        let columns = crate::archive::table_columns(conn, "threads")?;
        let mut job_tables = Vec::new();
        for (table, key) in [
            ("jobs", "thread_id"),
            ("agent_job_items", "assigned_thread_id"),
        ] {
            let fields = crate::archive::table_columns(conn, table)?;
            if fields.contains(key) && fields.contains("status") {
                job_tables.push((table, key));
            }
        }
        let index_rows: Vec<Value> = read_index(paths)?
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let mut index_owners: HashMap<PathBuf, HashSet<Option<String>>> = HashMap::new();
        for row in &index_rows {
            if let Some(path) = row
                .get("path")
                .or_else(|| row.get("rollout_path"))
                .and_then(Value::as_str)
            {
                if let Ok(actual) = fs::canonicalize(existing_rollout(Path::new(path))) {
                    index_owners
                        .entry(actual)
                        .or_default()
                        .insert(row.get("id").and_then(Value::as_str).map(str::to_owned));
                }
            }
        }
        let mut database_owners: HashMap<PathBuf, HashSet<String>> = HashMap::new();
        if columns.contains("rollout_path") {
            let mut statement =
                conn.prepare("SELECT id,rollout_path FROM threads WHERE rollout_path IS NOT NULL")?;
            for row in
                statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            {
                let (id, path) = row?;
                if let Ok(actual) = fs::canonicalize(existing_rollout(Path::new(&path))) {
                    database_owners.entry(actual).or_default().insert(id);
                }
            }
        }
        let mut files = Vec::new();
        for root in [paths.sessions_dir(), paths.home.join("archived_sessions")] {
            if !root.exists() {
                continue;
            }
            ensure_safe_path(&paths.home, &root)?;
            for entry in walkdir::WalkDir::new(root).follow_links(false) {
                let entry = entry?;
                if entry.file_name().to_string_lossy().starts_with("rollout-") {
                    files.push(entry.into_path());
                }
            }
        }
        Ok(Self {
            columns,
            job_tables,
            index_rows,
            files,
            database_owners,
            index_owners,
        })
    }
}

/// Codex compresses an idle rollout in place while its records keep the plain path.
fn existing_rollout(path: &Path) -> PathBuf {
    if fs::symlink_metadata(path).is_err() {
        for sibling in [
            rollout_file::plain_path(path),
            rollout_file::compressed_path(path),
        ] {
            if fs::symlink_metadata(&sibling).is_ok() {
                return sibling;
            }
        }
    }
    path.to_path_buf()
}

fn snapshot_from_inventory(
    paths: &CodexPaths,
    conn: &Connection,
    id: &str,
    expected_archived: Option<bool>,
    inventory: &ArchiveInventory,
) -> Result<SelectedCodexPreview> {
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
            ensure!(!is_active_status(status), "线程仍在运行，暂不能归档或恢复");
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
                ),
                "线程活动状态无法确认，暂不能归档或恢复"
            );
        }
    }
    for (table, key) in &inventory.job_tables {
        let active = has_active_job(conn, table, key, id)?;
        ensure!(!active, "线程仍有关联的运行任务，暂不能归档或恢复");
    }
    let index_rows = &inventory.index_rows;
    let mut files = BTreeSet::new();
    if let Some(path) = row
        .get("rollout_path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        files.insert(existing_rollout(Path::new(path)));
    }
    for entry in index_rows {
        if entry.get("id").and_then(Value::as_str) == Some(id) {
            if let Some(path) = entry
                .get("path")
                .or_else(|| entry.get("rollout_path"))
                .and_then(Value::as_str)
            {
                files.insert(existing_rollout(Path::new(path)));
            }
        }
    }
    let suffix = format!("-{id}.jsonl");
    files.extend(
        inventory
            .files
            .iter()
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.strip_suffix(".zst")
                        .unwrap_or(&name)
                        .ends_with(&suffix)
                })
            })
            .cloned(),
    );
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
        ensure!(
            inventory
                .database_owners
                .get(&actual)
                .is_none_or(|owners| owners.iter().all(|owner| owner == id)),
            "文件仍被其他线程引用"
        );
        ensure!(
            inventory
                .index_owners
                .get(&actual)
                .is_none_or(|owners| owners.iter().all(|owner| owner.as_deref() == Some(id))),
            "文件仍被其他线程索引引用"
        );
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

pub fn preview_archives(
    paths: &CodexPaths,
    ids: &[String],
    archive: bool,
) -> Vec<Result<SelectedCodexPreview>> {
    let context = (|| -> Result<_> {
        let conn = open(paths, false)?;
        let inventory = ArchiveInventory::read(paths, &conn)?;
        Ok((conn, inventory))
    })();
    ids.iter()
        .map(|id| {
            let (conn, inventory) = context
                .as_ref()
                .map_err(|error| anyhow::anyhow!("{error:#}"))?;
            ensure!(
                crate::codex::codex_task_identity_from_connection(conn, &inventory.columns, id)?
                    == crate::codex::CodexTaskIdentity::Main,
                "线程身份无法确认为主任务"
            );
            snapshot_from_inventory(paths, conn, id, Some(!archive), inventory)
        })
        .collect()
}

/// Executes `(id, fingerprint)` targets in order. The inventory is read once
/// outside any write transaction on the native database; every target is then
/// rechecked and updated in its own short transaction, so one failure never
/// rolls back another target.
pub fn execute_archives(
    paths: &CodexPaths,
    items: &[(&str, &str)],
    archive: bool,
) -> Vec<Result<()>> {
    let context = (|| -> Result<_> {
        let guard = MUTATION_LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("线程管理锁不可用"))?;
        let conn = open(paths, true)?;
        let inventory = ArchiveInventory::read(paths, &conn)?;
        Ok((guard, conn, inventory))
    })();
    let (_guard, mut conn, inventory) = match context {
        Ok(context) => context,
        Err(error) => {
            return items
                .iter()
                .map(|_| Err(anyhow::anyhow!("{error:#}")))
                .collect()
        }
    };
    items
        .iter()
        .map(|&(id, fingerprint)| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            ensure!(
                snapshot_from_inventory(paths, &tx, id, Some(!archive), &inventory)?.fingerprint
                    == fingerprint,
                "线程或文件已变化，请重新预览"
            );
            ensure!(
                inventory.columns.contains("archived_at"),
                "原生归档字段不可用"
            );
            let at = archive.then(|| jiff::Timestamp::now().as_second());
            let changed = if inventory.columns.contains("archived") {
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
        })
        .collect()
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
    BufReader::new(rollout_file::open_reader(path)?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot(
        paths: &CodexPaths,
        conn: &Connection,
        id: &str,
        expected_archived: Option<bool>,
    ) -> Result<SelectedCodexPreview> {
        let inventory = ArchiveInventory::read(paths, conn)?;
        snapshot_from_inventory(paths, conn, id, expected_archived, &inventory)
    }

    #[test]
    fn archive_and_restore_follow_a_natively_compressed_rollout() {
        let home = std::env::temp_dir().join(format!(
            "agenthub-archive-compressed-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = CodexPaths::new(&home);
        fs::create_dir_all(paths.sessions_dir()).unwrap();
        let conn = Connection::open(paths.state_db()).unwrap();
        conn.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,rollout_path TEXT,source TEXT,status TEXT,archived INTEGER,archived_at INTEGER);").unwrap();
        // The native records keep the plain path; only the compressed file exists.
        // Every case owns its file, so no cached scan can outlive a rewritten one.
        let thread = |rows: &dyn Fn(&str) -> Vec<Value>| {
            let id = uuid::Uuid::new_v4().to_string();
            let plain = paths
                .sessions_dir()
                .join(format!("rollout-fixture-{id}.jsonl"));
            let text: String = rows(&id).iter().map(|row| format!("{row}\n")).collect();
            fs::write(
                rollout_file::compressed_path(&plain),
                zstd::stream::encode_all(text.as_bytes(), 0).unwrap(),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO threads VALUES(?1,'Fixture',?2,'cli','completed',0,NULL)",
                rusqlite::params![id, plain.to_str().unwrap()],
            )
            .unwrap();
            (id, plain)
        };
        let meta = |id: &str| json!({"type":"session_meta","payload":{"id":id}});
        let event =
            |kind: &str| json!({"type":"event_msg","payload":{"type":kind,"turn_id":"turn"}});
        let preview = |id: &String, archive| {
            preview_archives(&paths, std::slice::from_ref(id), archive).remove(0)
        };

        let (id, plain) = thread(&|id| vec![meta(id), event("task_complete")]);
        let compressed = rollout_file::compressed_path(&plain);
        fs::write(
            paths.session_index(),
            format!("{}\n", json!({"id":id,"path":plain})),
        )
        .unwrap();
        for archive in [true, false] {
            let item = preview(&id, archive).unwrap();
            assert_eq!(item.paths, std::slice::from_ref(&compressed));
            assert_eq!(item.bytes, fs::metadata(&compressed).unwrap().len());
            execute_archives(&paths, &[(&id, &item.fingerprint)], archive)
                .remove(0)
                .unwrap();
            let archived: bool = conn
                .query_row("SELECT archived FROM threads WHERE id=?1", [&id], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(archived, archive);
        }
        assert!(!plain.exists());

        // Without a recorded path, only the directory walk finds the compressed file.
        let (walked, walked_plain) = thread(&|id| vec![meta(id), event("task_complete")]);
        conn.execute(
            "UPDATE threads SET rollout_path=NULL WHERE id=?1",
            [&walked],
        )
        .unwrap();
        assert_eq!(
            preview(&walked, true).unwrap().paths,
            [rollout_file::compressed_path(&walked_plain)]
        );

        // Every existing rule still guards a compressed file.
        let blocked = |rows: &dyn Fn(&str) -> Vec<Value>| {
            let (id, _) = thread(rows);
            format!("{:#}", preview(&id, true).unwrap_err())
        };
        assert!(
            blocked(&|_| vec![meta("another-id"), event("task_complete")])
                .contains("线程文件身份与选中 ID 不一致")
        );
        assert!(
            blocked(&|id| vec![meta(id), event("task_started")]).contains("线程文件仍有运行信号")
        );
        assert!(blocked(&|id| vec![meta(id)]).contains("线程终态无法确认"));
        conn.execute(
            "INSERT INTO threads VALUES('another-thread','Other',?1,'cli','completed',0,NULL)",
            [plain.to_str().unwrap()],
        )
        .unwrap();
        assert!(format!("{:#}", preview(&id, true).unwrap_err()).contains("文件仍被其他线程引用"));
        drop(conn);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn archive_batch_matches_single_preview_and_rechecks_changes_before_execution() {
        let home =
            std::env::temp_dir().join(format!("agenthub-archive-batch-{}", uuid::Uuid::new_v4()));
        let paths = CodexPaths::new(&home);
        fs::create_dir_all(paths.sessions_dir()).unwrap();
        let conn = Connection::open(paths.state_db()).unwrap();
        conn.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,rollout_path TEXT,source TEXT,status TEXT,archived INTEGER,archived_at INTEGER); CREATE TABLE jobs(thread_id TEXT,status TEXT);").unwrap();
        let ids: Vec<_> = (0..2).map(|_| uuid::Uuid::new_v4().to_string()).collect();
        let mut index = String::new();
        let mut files = Vec::new();
        for id in &ids {
            let path = paths
                .sessions_dir()
                .join(format!("rollout-fixture-{id}.jsonl"));
            fs::write(
                &path,
                format!(
                    "{}\n{}\n",
                    json!({"type":"session_meta","payload":{"id":id}}),
                    json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn"}})
                ),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO threads VALUES(?1,'Fixture',?2,'cli','completed',0,NULL)",
                rusqlite::params![id, path.to_str().unwrap()],
            )
            .unwrap();
            index.push_str(&format!("{}\n", json!({"id":id,"path":path})));
            files.push(path);
        }
        fs::write(paths.session_index(), index).unwrap();
        for archive in [true, false] {
            conn.execute(
                "UPDATE threads SET archived=?1,archived_at=CASE WHEN ?1 THEN 1 ELSE NULL END",
                [!archive],
            )
            .unwrap();
            let results = preview_archives(&paths, &ids, archive);
            for (id, result) in ids.iter().zip(results) {
                assert_eq!(
                    serde_json::to_value(result.unwrap()).unwrap(),
                    serde_json::to_value(snapshot(&paths, &conn, id, Some(!archive)).unwrap())
                        .unwrap()
                );
            }
        }
        conn.execute("UPDATE threads SET archived=0,archived_at=NULL", [])
            .unwrap();
        let preview = preview_archives(&paths, &ids, true).remove(0).unwrap();
        let mut text = fs::read_to_string(&files[0]).unwrap();
        text.push_str("{}\n");
        fs::write(&files[0], text).unwrap();
        assert!(
            execute_archives(&paths, &[(&ids[0], &preview.fingerprint)], true)
                .remove(0)
                .is_err()
        );
        for status in ["running", "inprogress"] {
            conn.execute(
                "INSERT INTO jobs VALUES(?1,?2)",
                rusqlite::params![ids[0], status],
            )
            .unwrap();
            assert!(
                preview_archives(&paths, &ids[..1], true)[0].is_err(),
                "{status}"
            );
            conn.execute("DELETE FROM jobs", []).unwrap();
        }
        conn.execute("INSERT INTO jobs VALUES(?1,'inprogress')", [&ids[0]])
            .unwrap();
        let result = preview_archives(
            &paths,
            &[ids[0].clone(), ids[1].clone(), "missing".into()],
            true,
        );
        assert!(result[0].is_err());
        assert!(result[1].is_ok());
        assert!(result[2].is_err());
        conn.execute("DELETE FROM jobs", []).unwrap();
        conn.execute(
            "UPDATE threads SET rollout_path=?1 WHERE id=?2",
            rusqlite::params![files[0].to_str().unwrap(), ids[1]],
        )
        .unwrap();
        assert!(preview_archives(&paths, &ids, true)
            .iter()
            .all(Result::is_err));
        drop(conn);
        fs::remove_dir_all(home).unwrap();
    }
}
