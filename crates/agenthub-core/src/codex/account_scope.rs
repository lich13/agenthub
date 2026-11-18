//! Codex hides `account_scoped` rollouts from every account except their creator. Visibility is
//! decided by the first rollout line alone, so the field is blanked in place: content, creator
//! identity, file length and every byte offset stay untouched.
use super::{
    rollout_file::{self, RolloutName},
    CodexPaths,
};
use crate::{selected_codex, session_delete};
use anyhow::{ensure, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MARKER: &[u8] = br#""account_scoped":true,"#;
/// Not a `.tmp` name, which Codex sweeps as its own stale temporary file.
const COPY_PREFIX: &str = ".agenthub-rollout-";
const HEADER_LIMIT: u64 = 4 * 1024 * 1024;
/// Remote requests time out after 60 seconds and writes are never replayed.
const CLEAR_BUDGET: Duration = Duration::from_secs(20);
const COORDINATION_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountScopeStatus {
    pub scoped_threads: u64,
    pub scoped_files: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountScopeResult {
    pub cleared_threads: u64,
    pub cleared_files: u64,
    /// Scoped files left for the next call once the time budget ran out.
    pub remaining_files: u64,
    pub items: Vec<CodexAccountScopeItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountScopeItem {
    pub thread_id: String,
    pub reason: CodexAccountScopeReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexAccountScopeReason {
    Format,
    Busy,
    Changed,
    OriginalMissing,
    Failed,
}

#[derive(Debug)]
enum Skip {
    Format,
    Busy,
    Changed,
    OriginalMissing,
    Failed(String),
}

impl From<io::Error> for Skip {
    fn from(error: io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<anyhow::Error> for Skip {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

impl Skip {
    fn item(self, thread_id: String) -> CodexAccountScopeItem {
        let (reason, detail) = match self {
            Self::Format => (CodexAccountScopeReason::Format, None),
            Self::Busy => (CodexAccountScopeReason::Busy, None),
            Self::Changed => (CodexAccountScopeReason::Changed, None),
            Self::OriginalMissing => (CodexAccountScopeReason::OriginalMissing, None),
            Self::Failed(detail) => (CodexAccountScopeReason::Failed, Some(detail)),
        };
        CodexAccountScopeItem {
            thread_id,
            reason,
            detail,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Header {
    /// This thread's session header without the marker.
    Clear,
    /// The marker starts at this byte and can be blanked without changing anything else.
    Marked(usize),
    /// Scoped, but not in the exact form Codex writes.
    Unsupported,
    Unreadable,
}

impl Header {
    fn scoped(self) -> bool {
        matches!(self, Self::Marked(_) | Self::Unsupported)
    }
}

fn blank(line: &mut [u8], offset: usize) {
    line[offset..offset + MARKER.len()].fill(b' ');
}

fn inspect(line: &[u8], thread_id: &str) -> Header {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Header::Unreadable;
    };
    if value["type"] != "session_meta" {
        return Header::Unreadable;
    }
    if value["payload"]["account_scoped"] != true {
        // Codex rejects an original whose header names another thread, so it is no proof that
        // a reverted copy may lose its marker.
        return if value["payload"]["id"] == thread_id {
            Header::Clear
        } else {
            Header::Unreadable
        };
    }
    let Some(offset) = line
        .windows(MARKER.len())
        .position(|window| window == MARKER)
    else {
        return Header::Unsupported;
    };
    let mut patched = line.to_vec();
    blank(&mut patched, offset);
    let mut expected = value;
    if let Some(payload) = expected.get_mut("payload").and_then(Value::as_object_mut) {
        payload.remove("account_scoped");
    }
    if expected["payload"]["id"] == thread_id
        && serde_json::from_slice::<Value>(&patched).is_ok_and(|patched| patched == expected)
    {
        Header::Marked(offset)
    } else {
        Header::Unsupported
    }
}

fn first_line(reader: impl Read) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    BufReader::new(reader.take(HEADER_LIMIT)).read_until(b'\n', &mut line)?;
    Ok(line)
}

struct Rollout {
    path: PathBuf,
    /// Reverted copies carry another rollout ID and defer to this file when unmarked.
    original: bool,
    header: Header,
}

#[derive(Default)]
struct Scan {
    threads: BTreeMap<String, Vec<Rollout>>,
    /// Replacement copies that a killed pass neither published nor removed.
    copies: Vec<PathBuf>,
}

/// The configured home is the root; a rollout directory that is a link is never entered.
fn roots(paths: &CodexPaths) -> impl Iterator<Item = PathBuf> {
    [paths.sessions_dir(), paths.home.join("archived_sessions")]
        .into_iter()
        .filter(|root| fs::symlink_metadata(root).is_ok_and(|meta| meta.is_dir()))
}

fn scan(paths: &CodexPaths) -> Scan {
    let mut scan = Scan::default();
    for root in roots(paths) {
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .follow_root_links(false)
            .sort_by_file_name()
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(COPY_PREFIX))
            {
                scan.copies.push(entry.into_path());
                continue;
            }
            let path = entry.path();
            let Some(name) = RolloutName::parse(path) else {
                continue;
            };
            // Codex reads the plain file whenever both representations exist.
            if rollout_file::is_compressed(path)
                && fs::symlink_metadata(rollout_file::plain_path(path)).is_ok()
            {
                continue;
            }
            let header = rollout_file::open_reader(path)
                .and_then(|reader| Ok(first_line(reader)?))
                .map_or(Header::Unreadable, |line| inspect(&line, &name.thread_id));
            scan.threads
                .entry(name.thread_id.clone())
                .or_default()
                .push(Rollout {
                    path: entry.into_path(),
                    original: name.rollout_id == name.thread_id,
                    header,
                });
        }
    }
    scan
}

enum Lock {
    /// Dropping the file releases the lock.
    Held(#[allow(dead_code)] File),
    Absent,
    Busy,
}

/// Codex owns these files. Creating one as the service user would lock Codex out of its own
/// lock, so only an existing file is opened, read-only.
fn try_lock(path: &Path) -> io::Result<Lock> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Lock::Absent),
        Err(error) => return Err(error),
    };
    match file.try_lock() {
        Ok(()) => Ok(Lock::Held(file)),
        Err(TryLockError::WouldBlock) => Ok(Lock::Busy),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Held for a whole pass so Codex's compaction worker never reads a header mid-rewrite.
fn maintenance_lock(paths: &CodexPaths) -> io::Result<Lock> {
    try_lock(&paths.home.join(".tmp").join("rollout-maintenance.lock"))
}

fn writer_lock(paths: &CodexPaths, thread_id: &str) -> Result<Lock, Skip> {
    let path = paths
        .home
        .join("thread-writer-locks")
        .join(format!("{thread_id}.lock"));
    match try_lock(&path) {
        Ok(lock @ (Lock::Held(_) | Lock::Absent)) => Ok(lock),
        Ok(Lock::Busy) | Err(_) => Err(Skip::Busy),
    }
}

/// Codex writers wait on this lock, so it only covers a thread-lock probe or the final recheck
/// and rename. Without the lock file a replacement cannot be serialized with Codex at all.
fn coordination_lock(paths: &CodexPaths) -> Result<Lock, Skip> {
    let path = paths
        .home
        .join("thread-writer-locks")
        .join(".coordination.lock");
    let started = Instant::now();
    loop {
        match try_lock(&path)? {
            lock @ Lock::Held(_) => return Ok(lock),
            Lock::Busy if started.elapsed() < COORDINATION_WAIT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Lock::Busy | Lock::Absent => return Err(Skip::Busy),
        }
    }
}

/// Codex only appends to a plain rollout, so the header can be rewritten under a live writer.
fn clear_plain(thread_id: &str, path: &Path) -> Result<bool, Skip> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let before = file.metadata()?;
    let mut line = first_line(&file)?;
    let offset = match inspect(&line, thread_id) {
        Header::Marked(offset) => offset,
        Header::Clear => return Ok(false),
        Header::Unsupported | Header::Unreadable => return Err(Skip::Format),
    };
    file.seek(SeekFrom::Start(offset as u64))?;
    file.write_all(&[b' '; MARKER.len()])?;
    blank(&mut line, offset);
    let mut written = vec![0; line.len()];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut written)?;
    if written != line {
        return Err(Skip::Changed);
    }
    // Codex orders history and ages rollouts for compaction by mtime; an append in between wins.
    // The marker is already gone, so a failure here must not report the file as still scoped.
    if let (Ok(after), Ok(modified)) = (file.metadata(), before.modified()) {
        if after.len() == before.len() {
            let _ = file.set_modified(modified);
        }
    }
    Ok(true)
}

/// A compressed rollout is immutable, so an identical copy without the marker replaces it.
fn clear_compressed(paths: &CodexPaths, thread_id: &str, path: &Path) -> Result<bool, Skip> {
    let before = fs::symlink_metadata(path)?;
    let mut reader = BufReader::new(rollout_file::open_reader(path)?);
    let mut line = Vec::new();
    reader
        .by_ref()
        .take(HEADER_LIMIT)
        .read_until(b'\n', &mut line)?;
    let offset = match inspect(&line, thread_id) {
        Header::Marked(offset) => offset,
        Header::Clear => return Ok(false),
        Header::Unsupported | Header::Unreadable => return Err(Skip::Format),
    };
    // Do not encode a copy that cannot be published. Codex probes a thread lock only while it
    // holds the coordination lock; the same order keeps this probe from failing a writer that
    // is opening the thread.
    {
        let _coordination = coordination_lock(paths)?;
        writer_lock(paths, thread_id)?;
    }
    blank(&mut line, offset);
    let mut expected = Sha256::new();
    expected.update(&line);
    let length = line.len() as u64 + io::copy(&mut reader, &mut expected)?;
    let expected = expected.finalize();

    let temporary = path.with_file_name(format!("{COPY_PREFIX}{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        // Held until the copy is published or removed; a later pass removes an unlocked one.
        if matches!(file.try_lock(), Err(TryLockError::WouldBlock)) {
            return Err(Skip::Busy);
        }
        let mut reader = BufReader::new(rollout_file::open_reader(path)?);
        io::copy(
            &mut reader.by_ref().take(line.len() as u64),
            &mut io::sink(),
        )?;
        let mut encoder = zstd::stream::write::Encoder::new(&file, 3)?;
        encoder.set_pledged_src_size(Some(length))?;
        encoder.write_all(&line)?;
        io::copy(&mut reader, &mut encoder)?;
        encoder.finish()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = file.metadata()?;
            if (current.uid(), current.gid()) != (before.uid(), before.gid()) {
                std::os::unix::fs::fchown(&file, Some(before.uid()), Some(before.gid()))?;
            }
        }
        // chown may clear mode bits, so restore permissions afterwards.
        file.set_permissions(before.permissions())?;
        file.set_modified(before.modified()?)?;
        file.sync_all()?;
        file.seek(SeekFrom::Start(0))?;
        let mut actual = Sha256::new();
        io::copy(&mut zstd::stream::read::Decoder::new(&file)?, &mut actual)?;
        if actual.finalize() != expected {
            return Err(Skip::Changed);
        }
        let _coordination = coordination_lock(paths)?;
        let _writer = writer_lock(paths, thread_id)?;
        if !session_delete::same_source(&before, &fs::symlink_metadata(path)?)
            || fs::symlink_metadata(rollout_file::plain_path(path)).is_ok()
        {
            return Err(Skip::Changed);
        }
        fs::rename(&temporary, path)?;
        Ok(true)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn clear_file(paths: &CodexPaths, thread_id: &str, path: &Path) -> Result<bool, Skip> {
    if rollout_file::is_compressed(path) {
        clear_compressed(paths, thread_id, path)
    } else {
        clear_plain(thread_id, path)
    }
}

/// Returns the rewritten file count and the first reason the thread is still scoped.
fn clear_thread(paths: &CodexPaths, thread_id: &str, files: &mut [Rollout]) -> (u64, Option<Skip>) {
    let mut cleared = 0;
    let mut skipped = None;
    let mut attempt = |file: &mut Rollout, skipped: &mut Option<Skip>| match clear_file(
        paths, thread_id, &file.path,
    ) {
        Ok(rewritten) => {
            cleared += u64::from(rewritten);
            file.header = Header::Clear;
        }
        Err(skip) => {
            skipped.get_or_insert(skip);
        }
    };
    for file in files.iter_mut().filter(|file| file.original) {
        if file.header.scoped() {
            attempt(file, &mut skipped);
        }
    }
    // An unmarked reverted copy makes Codex consult the original; without a readable, unmarked
    // original the thread would disappear for every account.
    let mut originals = files.iter().filter(|file| file.original).peekable();
    let originals_clear =
        originals.peek().is_some() && originals.all(|file| file.header == Header::Clear);
    for file in files.iter_mut().filter(|file| !file.original) {
        if !file.header.scoped() {
            continue;
        }
        if originals_clear {
            attempt(file, &mut skipped);
        } else {
            skipped.get_or_insert(Skip::OriginalMissing);
        }
    }
    (cleared, skipped)
}

pub fn status(paths: &CodexPaths) -> CodexAccountScopeStatus {
    let mut status = CodexAccountScopeStatus::default();
    for files in scan(paths).threads.values() {
        let scoped = files.iter().filter(|file| file.header.scoped()).count() as u64;
        status.scoped_files += scoped;
        status.scoped_threads += u64::from(scoped > 0);
    }
    status
}

pub fn clear(paths: &CodexPaths) -> Result<CodexAccountScopeResult> {
    clear_within(paths, CLEAR_BUDGET)
}

fn clear_within(paths: &CodexPaths, budget: Duration) -> Result<CodexAccountScopeResult> {
    let maintenance = maintenance_lock(paths)?;
    ensure!(
        !matches!(maintenance, Lock::Busy),
        "Codex 正在维护会话文件，稍后重试"
    );
    let scan = scan(paths);
    for copy in &scan.copies {
        if let Ok(Lock::Held(_unused)) = try_lock(copy) {
            let _ = fs::remove_file(copy);
        }
    }
    // The budget bounds rewriting; listing a large history must not consume it.
    let started = Instant::now();
    let mut result = CodexAccountScopeResult::default();
    let mut attempted = false;
    for (thread_id, mut files) in scan.threads {
        let scoped = files.iter().filter(|file| file.header.scoped()).count() as u64;
        if scoped == 0 {
            continue;
        }
        if attempted && started.elapsed() >= budget {
            result.remaining_files += scoped;
            continue;
        }
        attempted = true;
        let (cleared, skipped) = clear_thread(paths, &thread_id, &mut files);
        result.cleared_files += cleared;
        match skipped {
            None => result.cleared_threads += 1,
            Some(skip) => result.items.push(skip.item(thread_id)),
        }
    }
    Ok(result)
}

/// Stop-hook entry: the finished thread and its spawned descendants, plain originals only.
/// Everything else is left to the explicit action.
pub fn clear_thread_tree(paths: &CodexPaths, thread_id: &str) -> Result<u64> {
    if !paths.state_db().exists() {
        return Ok(0);
    }
    let maintenance = maintenance_lock(paths)?;
    if matches!(maintenance, Lock::Busy) {
        return Ok(0);
    }
    let conn = Connection::open_with_flags(paths.state_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let edges = selected_codex::tree::spawn_edges_with_connection(&conn)?;
    let ids = selected_codex::tree::descendants_from_edges(&edges, &[thread_id.to_owned()])?;
    let roots: Vec<PathBuf> = roots(paths).collect();
    let mut statement = conn.prepare("SELECT rollout_path FROM threads WHERE id = ?1")?;
    let mut cleared = 0;
    for id in ids {
        let Some(path) = statement
            .query_row([&id], |row| row.get::<_, Option<String>>(0))
            .optional()?
            .flatten()
            .map(PathBuf::from)
        else {
            continue;
        };
        if RolloutName::parse(&path)
            .is_some_and(|name| name.thread_id == id && name.rollout_id == id)
            && !rollout_file::is_compressed(&path)
            && roots.iter().any(|root| session_delete::within(root, &path))
            && clear_plain(&id, &path).unwrap_or(false)
        {
            cleared += 1;
        }
    }
    Ok(cleared)
}

#[cfg(test)]
mod tests;
