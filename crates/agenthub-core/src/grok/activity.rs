use super::{GrokDeletionEligibility, GrokPaths, GrokSessionSummary};
use crate::{
    file_stamp::FileStamp,
    platform::{resolve_kernel_exe, same_canonical_dir},
};
use serde_json::Value;
#[cfg(test)]
use std::collections::VecDeque;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ownership {
    Inactive,
    Open,
    Unknown,
}

#[derive(Clone, Debug)]
struct Registration {
    id: String,
    pid: Option<u32>,
    opened_at: Option<i64>,
    cwd: Option<String>,
}

#[derive(Clone, Debug)]
struct Process {
    started_at: Option<i64>,
    zombie: bool,
    native: Option<bool>,
}

#[derive(Clone)]
pub(super) struct Snapshot {
    registrations: Option<Vec<Registration>>,
    processes: Option<HashMap<u32, Process>>,
}

type ReadonlyGrokSnapshotKey = (PathBuf, Option<(u64, Option<SystemTime>)>);
type ReadonlyGrokSnapshotEntry = (ReadonlyGrokSnapshotKey, Snapshot, std::time::Instant);

static READONLY_SNAPSHOT_CACHE: Mutex<Option<ReadonlyGrokSnapshotEntry>> = Mutex::new(None);

#[cfg(test)]
pub(crate) fn invalidate_readonly_snapshot_cache() {
    if let Ok(mut guard) = READONLY_SNAPSHOT_CACHE.lock() {
        *guard = None;
    }
}

pub(crate) fn evict_idle_cache(max_idle: std::time::Duration) {
    if let Ok(mut guard) = READONLY_SNAPSHOT_CACHE.lock() {
        if guard
            .as_ref()
            .is_some_and(|(_, _, ts)| ts.elapsed() >= max_idle)
        {
            *guard = None;
            crate::note_cache_eviction();
        }
    }
    TURN_CACHE
        .0
        .evict_idle(max_idle, |entry| entry.last_accessed, |_| 0);
}

#[cfg(test)]
thread_local! {
    static TEST_SNAPSHOT: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
    static TEST_SNAPSHOT_QUEUE: std::cell::RefCell<VecDeque<Option<(String, String)>>> = const { std::cell::RefCell::new(VecDeque::new()) };
}

#[cfg(test)]
pub(super) fn test_owner(id: Option<&str>, cwd: &str) {
    invalidate_readonly_snapshot_cache();
    TEST_SNAPSHOT.with(|slot| {
        *slot.borrow_mut() = id.map(|id| (id.to_owned(), cwd.to_owned()));
    });
}

#[cfg(test)]
pub(super) fn test_owner_sequence(seq: Vec<Option<(String, String)>>) {
    invalidate_readonly_snapshot_cache();
    TEST_SNAPSHOT_QUEUE.with(|q| {
        *q.borrow_mut() = VecDeque::from(seq);
    });
}

impl Snapshot {
    pub(super) fn capture_readonly(paths: &GrokPaths) -> Self {
        let reg_path = paths.home.join("active_sessions.json");
        let stamp = fs::metadata(&reg_path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        let key = (paths.home.clone(), stamp);
        if let Ok(guard) = READONLY_SNAPSHOT_CACHE.lock() {
            if let Some((ref cached_key, ref snap, ts)) = *guard {
                if *cached_key == key && ts.elapsed() < std::time::Duration::from_secs(2) {
                    return snap.clone();
                }
            }
        }
        let snap = Self::capture(paths);
        if let Ok(mut guard) = READONLY_SNAPSHOT_CACHE.lock() {
            *guard = Some((key, snap.clone(), std::time::Instant::now()));
        }
        snap
    }

    pub(super) fn capture(paths: &GrokPaths) -> Self {
        #[cfg(test)]
        if let Some(queued) = TEST_SNAPSHOT_QUEUE.with(|q| q.borrow_mut().pop_front()) {
            return match queued {
                Some((id, cwd)) => {
                    let mut processes = HashMap::new();
                    processes.insert(
                        42,
                        Process {
                            started_at: Some(100),
                            zombie: false,
                            native: Some(true),
                        },
                    );
                    Self {
                        registrations: Some(vec![Registration {
                            id,
                            pid: Some(42),
                            opened_at: Some(200),
                            cwd: Some(cwd),
                        }]),
                        processes: Some(processes),
                    }
                }
                None => Self {
                    registrations: Some(Vec::new()),
                    processes: Some(HashMap::new()),
                },
            };
        }
        #[cfg(test)]
        if let Some((id, cwd)) = TEST_SNAPSHOT.with(|slot| slot.borrow().clone()) {
            let mut processes = HashMap::new();
            processes.insert(
                42,
                Process {
                    started_at: Some(100),
                    zombie: false,
                    native: Some(true),
                },
            );
            return Self {
                registrations: Some(vec![Registration {
                    id,
                    pid: Some(42),
                    opened_at: Some(200),
                    cwd: Some(cwd),
                }]),
                processes: Some(processes),
            };
        }
        let registrations = read_registrations(&paths.home.join("active_sessions.json"));
        let pids: HashSet<_> = registrations
            .iter()
            .flatten()
            .filter_map(|entry| entry.pid)
            .collect();
        // A list or notification scan shares this one process snapshot.
        let processes = if pids.is_empty() {
            Some(HashMap::new())
        } else {
            collect_processes(paths, &pids)
        };
        Self {
            registrations,
            processes,
        }
    }

    pub(super) fn ownership(&self, session: &GrokSessionSummary) -> Ownership {
        let Some(registrations) = &self.registrations else {
            return Ownership::Unknown;
        };
        let mut result = Ownership::Inactive;
        for entry in registrations.iter().filter(|entry| entry.id == session.id) {
            let state = self.owner(entry, &session.cwd);
            if state == Ownership::Open {
                return state;
            }
            if state == Ownership::Unknown {
                result = state;
            }
        }
        result
    }

    fn owner(&self, entry: &Registration, cwd: &str) -> Ownership {
        let (Some(pid), Some(processes)) = (entry.pid, &self.processes) else {
            return Ownership::Unknown;
        };
        let Some(process) = processes.get(&pid) else {
            return Ownership::Inactive;
        };
        if process.zombie || process.native == Some(false) {
            return Ownership::Inactive;
        }
        let (Some(started), Some(opened)) = (process.started_at, entry.opened_at) else {
            return Ownership::Unknown;
        };
        // ps records whole seconds; a newly reused PID cannot own an older entry.
        if started > opened {
            return Ownership::Inactive;
        }
        if process.native != Some(true)
            || entry
                .cwd
                .as_deref()
                .is_none_or(|path| !same_path(path, cwd))
        {
            return Ownership::Unknown;
        }
        Ownership::Open
    }

    #[cfg(test)]
    pub(super) fn status(&self, session: &GrokSessionSummary) -> &'static str {
        Self::display_status(self.ownership(session), lifecycle(session).0)
    }

    fn display_status(owner: Ownership, phase: Phase) -> &'static str {
        match phase {
            Phase::Ended => "recent",
            Phase::Running if owner == Ownership::Open => "running",
            Phase::Empty if matches!(owner, Ownership::Inactive | Ownership::Open) => "recent",
            _ => "unknown",
        }
    }

    pub(super) fn update_summary(&self, session: &mut GrokSessionSummary) {
        let owner = self.ownership(session);
        let (phase, _) = lifecycle(session);
        session.status = Self::display_status(owner, phase).into();
        session.deletion_eligibility = Some(GrokDeletionEligibility {
            allowed: true,
            reason: None,
        });
    }
}

fn same_path(left: &str, right: &str) -> bool {
    same_canonical_dir(Path::new(left), Path::new(right))
}

fn read_registrations(path: &Path) -> Option<Vec<Registration>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return None,
    };
    if file.metadata().ok()?.len() > 1024 * 1024 {
        return None;
    }
    let value: Value = serde_json::from_reader(file.take(1024 * 1024 + 1)).ok()?;
    let rows: Vec<_> = match &value {
        Value::Array(rows) => rows.iter().map(|row| (None, row)).collect(),
        Value::Object(rows) => rows
            .iter()
            .map(|(key, row)| (Some(key.as_str()), row))
            .collect(),
        _ => return None,
    };
    rows.into_iter()
        .map(|(key, row)| {
            row.as_object()?;
            let id = row
                .get("session_id")
                .or_else(|| row.get("sessionId"))
                .and_then(Value::as_str)
                .or(key.filter(|key| uuid::Uuid::parse_str(key).is_ok()))?;
            Some(Registration {
                id: id.to_owned(),
                pid: row.get("pid").and_then(Value::as_u64).and_then(|pid| {
                    u32::try_from(pid)
                        .ok()
                        .filter(|pid| *pid > 0 && *pid <= i32::MAX as u32)
                }),
                opened_at: parse_opened_at_seconds(
                    row.get("opened_at").or_else(|| row.get("openedAt")),
                ),
                cwd: row.get("cwd").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect()
}

fn parse_opened_at_seconds(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(s) = value.as_str() {
        return s.parse::<jiff::Timestamp>().ok().map(|ts| ts.as_second());
    }
    let raw = value.as_i64().filter(|&n| n > 0)?;
    if raw >= 1_000_000_000_000 {
        Some(raw / 1000)
    } else {
        Some(raw)
    }
}

fn grok_allowlist(paths: &GrokPaths) -> (HashSet<PathBuf>, HashSet<PathBuf>) {
    let user_home = crate::platform::current_home().ok().or_else(dirs::home_dir);
    let candidate_dirs = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .chain([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            paths.home.join("bin"),
        ])
        .chain(
            user_home
                .into_iter()
                .flat_map(|home| [home.join(".local/bin"), home.join(".grok/bin")]),
        );

    let mut allowlisted_dirs = HashSet::new();
    let mut executables = HashSet::new();
    for dir in candidate_dirs {
        if dir.is_dir() {
            allowlisted_dirs.insert(dir.clone());
            if let Ok(canon_dir) = dir.canonicalize() {
                allowlisted_dirs.insert(canon_dir);
            }
        }
        let bin = dir.join("grok");
        if let Ok(canon_bin) = bin.canonicalize() {
            if let Some(parent) = canon_bin.parent() {
                allowlisted_dirs.insert(parent.to_path_buf());
            }
            executables.insert(canon_bin);
        }
    }
    (executables, allowlisted_dirs)
}

fn is_allowlisted_grok_binary(
    path: &Path,
    executables: &HashSet<PathBuf>,
    allowlisted_dirs: &HashSet<PathBuf>,
) -> bool {
    if executables.contains(path) {
        return true;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !(name == "grok" || name.starts_with("grok-")) {
        return false;
    }
    path.parent()
        .is_some_and(|parent| allowlisted_dirs.contains(parent))
}

fn collect_processes(paths: &GrokPaths, pids: &HashSet<u32>) -> Option<HashMap<u32, Process>> {
    if pids.is_empty() {
        return Some(HashMap::new());
    }
    let live_pids: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|&pid| crate::platform::pid_is_alive(pid))
        .collect();
    if live_pids.is_empty() {
        return Some(HashMap::new());
    }
    let (executables, allowlisted_dirs) = grok_allowlist(paths);
    let mut processes = HashMap::new();
    for chunk in live_pids.chunks(256) {
        let pid_list = chunk
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let output = Command::new("/bin/ps")
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .args(["-ww", "-p", &pid_list, "-o", "pid=,lstart=,stat=,args="])
            .output()
            .ok()?;
        if output.stdout.len() > 8 * 1024 * 1024 {
            return None;
        }
        if !output.status.success() {
            if output.status.code() == Some(1)
                && output.stdout.trim_ascii().is_empty()
                && output.stderr.trim_ascii().is_empty()
            {
                continue;
            }
            return None;
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut fields = line.split_whitespace();
            let Some(pid) = fields.next().and_then(|value| value.parse().ok()) else {
                continue;
            };
            if !pids.contains(&pid) {
                continue;
            };
            let start = fields.by_ref().take(5).collect::<Vec<_>>().join(" ");
            let started_at = jiff::civil::DateTime::strptime("%a %b %e %T %Y", &start)
                .ok()
                .and_then(|dt| jiff::tz::Offset::UTC.to_timestamp(dt).ok())
                .map(|ts| ts.as_second());
            let zombie = fields.next().is_some_and(|state| state.starts_with('Z'));
            let args: Vec<&str> = fields.collect();
            let native = classify_grok_process(pid, &args, &executables, &allowlisted_dirs);
            processes.insert(
                pid,
                Process {
                    started_at,
                    zombie,
                    native,
                },
            );
        }
    }
    Some(processes)
}

fn classify_grok_process(
    pid: u32,
    args: &[&str],
    executables: &HashSet<PathBuf>,
    allowlisted_dirs: &HashSet<PathBuf>,
) -> Option<bool> {
    let kernel_exe = resolve_kernel_exe(pid);
    if let Some(ref exe) = kernel_exe {
        if is_allowlisted_grok_binary(exe, executables, allowlisted_dirs) {
            return Some(true);
        }
        if let Ok(canonical) = exe.canonicalize() {
            if is_allowlisted_grok_binary(&canonical, executables, allowlisted_dirs) {
                return Some(true);
            }
        }
    }
    let mut saw_grok_name = kernel_exe
        .as_ref()
        .and_then(|p| p.file_name())
        .is_some_and(|name| name.to_string_lossy().contains("grok"));
    for (idx, token) in args.iter().copied().take(3).enumerate() {
        if token.starts_with('-') {
            continue;
        }
        let token_path = Path::new(token);
        if token_path.is_absolute() {
            if is_allowlisted_grok_binary(token_path, executables, allowlisted_dirs) {
                return Some(true);
            }
            if let Ok(canonical) = token_path.canonicalize() {
                if is_allowlisted_grok_binary(&canonical, executables, allowlisted_dirs) {
                    return Some(true);
                }
            }
            if token_path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("grok"))
            {
                saw_grok_name = true;
            }
        } else if idx == 0 && (token == "grok" || token.starts_with("grok-")) {
            if !executables.is_empty() {
                return Some(true);
            }
            saw_grok_name = true;
        }
    }
    if saw_grok_name {
        // A running CLI can retain an old/deleted executable after an upgrade.
        // An unverified Grok binary must not release deletion protection.
        None
    } else if kernel_exe.is_some() || !args.is_empty() {
        Some(false)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Empty,
    Running,
    Ended,
    Unknown,
}

#[derive(Clone, Default)]
struct Turns {
    phase: Option<Phase>,
    stack: Vec<bool>,
    corrupt: bool,
}

impl Turns {
    fn accept(&mut self, row: &[u8], id: &str) {
        let value: Value = match serde_json::from_slice(row) {
            Ok(value) => value,
            Err(_) => {
                self.corrupt = true;
                return;
            }
        };
        if !value.is_object() || value.get("type").and_then(Value::as_str).is_none() {
            self.corrupt = true;
            return;
        }
        match value.get("type").and_then(Value::as_str) {
            Some("turn_started") => {
                let relationship = value.get("session_relationship").and_then(Value::as_str);
                let session_id = value.get("session_id").and_then(Value::as_str);
                let primary = session_id == Some(id) && relationship == Some("primary");
                if relationship.is_none()
                    || (relationship == Some("primary") && session_id != Some(id))
                {
                    self.corrupt = true;
                }
                if primary {
                    if self.stack.contains(&true)
                        || value
                            .get("schema_version")
                            .and_then(Value::as_str)
                            .is_some_and(|v| v != "1.0")
                    {
                        self.corrupt = true;
                    }
                    self.phase = Some(Phase::Running);
                }
                if self.stack.len() >= 32 {
                    self.corrupt = true;
                } else {
                    self.stack.push(primary);
                }
            }
            Some("turn_ended") => {
                let primary = self.stack.pop();
                if primary.is_none() {
                    self.corrupt = true;
                }
                if primary == Some(true) {
                    self.phase = Some(
                        if matches!(
                            value.get("outcome").and_then(Value::as_str),
                            Some(
                                "completed"
                                    | "cancelled"
                                    | "canceled"
                                    | "error"
                                    | "failed"
                                    | "interrupted"
                                    | "aborted"
                                    | "timeout"
                            )
                        ) {
                            Phase::Ended
                        } else {
                            Phase::Unknown
                        },
                    );
                }
            }
            _ => {}
        }
    }
    fn phase(&self) -> Phase {
        if self.corrupt {
            Phase::Unknown
        } else {
            self.phase.unwrap_or(Phase::Empty)
        }
    }
}

fn is_valid_primary_turn_start(row: &[u8], id: &str) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(row) else {
        return false;
    };
    value.is_object()
        && value.get("type").and_then(Value::as_str) == Some("turn_started")
        && value.get("session_relationship").and_then(Value::as_str) == Some("primary")
        && value.get("session_id").and_then(Value::as_str) == Some(id)
        && value
            .get("schema_version")
            .and_then(Value::as_str)
            .is_none_or(|v| v == "1.0")
}

fn try_reconstruct_from_tail_window(
    file: &mut File,
    stamp_len: u64,
    id: &str,
) -> std::io::Result<Option<(Turns, u64)>> {
    let tail_start = stamp_len - READ_BUDGET;
    file.seek(SeekFrom::Start(tail_start - 1))?;
    let mut prev = [0u8; 1];
    file.read_exact(&mut prev)?;
    let mut reader = BufReader::new(file.take(READ_BUDGET));
    let mut offset = tail_start;
    if prev[0] != b'\n' {
        let mut discard = Vec::new();
        let skipped = reader.read_until(b'\n', &mut discard)?;
        if skipped == 0 || discard.last() != Some(&b'\n') {
            return Ok(None);
        }
        offset += skipped as u64;
    }
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut last_primary_idx: Option<usize> = None;
    loop {
        let mut line = Vec::new();
        let bytes = reader
            .by_ref()
            .take(LINE_LIMIT + 1)
            .read_until(b'\n', &mut line)?;
        if bytes == 0 {
            break;
        }
        if line.len() as u64 > LINE_LIMIT {
            return Ok(None);
        }
        if line.last() != Some(&b'\n') {
            break;
        }
        offset += bytes as u64;
        if line.iter().any(|byte| !byte.is_ascii_whitespace()) {
            if is_valid_primary_turn_start(&line, id) {
                last_primary_idx = Some(lines.len());
            }
            lines.push(line);
        }
    }
    let Some(start_idx) = last_primary_idx else {
        return Ok(None);
    };
    let mut turns = Turns::default();
    for line in &lines[start_idx..] {
        turns.accept(line, id);
        if turns.corrupt {
            return Ok(None);
        }
    }
    Ok(Some((turns, offset)))
}

struct Cursor {
    path: PathBuf,
    id: String,
    stamp: FileStamp,
    offset: u64,
    anchor: [u8; 32],
    turns: Turns,
    caught_up: bool,
    last_accessed: std::time::Instant,
}

struct TurnCache(crate::lru::ByteBoundedLru<Cursor>);
static TURN_CACHE: TurnCache = TurnCache(crate::lru::ByteBoundedLru::new(256, u64::MAX));
const READ_BUDGET: u64 = 8 * 1024 * 1024;
const LINE_LIMIT: u64 = 256 * 1024;

fn lifecycle(session: &GrokSessionSummary) -> (Phase, bool) {
    match TURN_CACHE.read_state(&session.path.join("events.jsonl"), &session.id) {
        Ok(phase) => phase,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Phase::Empty, true),
        Err(_) => (Phase::Unknown, false),
    }
}

impl TurnCache {
    #[cfg(test)]
    fn read(&self, path: &Path, id: &str) -> std::io::Result<Phase> {
        self.read_state(path, id).map(|state| state.0)
    }

    fn read_state(&self, path: &Path, id: &str) -> std::io::Result<(Phase, bool)> {
        let mut file = File::open(path)?;
        let stamp = FileStamp::from_open_file(&file)
            .ok_or_else(|| std::io::Error::other("invalid activity file"))?;
        let old = self
            .0
            .take(|entry| entry.path == path && entry.id == id, |_| 0);
        let mut cursor = match old {
            Some(mut cursor) if cursor.stamp == stamp && cursor.caught_up => {
                cursor.last_accessed = std::time::Instant::now();
                let state = (cursor.turns.phase(), cursor.offset == stamp.len);
                self.publish(cursor);
                return Ok(state);
            }
            Some(cursor)
                if cursor.stamp.same_inode(&stamp)
                    && (stamp.len > cursor.stamp.len
                        || (cursor.stamp == stamp && !cursor.caught_up))
                    && crate::jsonl_cache::file_anchor(&mut file, cursor.offset)?
                        == cursor.anchor =>
            {
                cursor
            }
            _ => Cursor {
                path: path.to_owned(),
                id: id.to_owned(),
                stamp,
                offset: 0,
                anchor: [0; 32],
                turns: Turns::default(),
                caught_up: false,
                last_accessed: std::time::Instant::now(),
            },
        };
        if cursor.offset == 0 && stamp.len > READ_BUDGET {
            if let Some((turns, tail_offset)) =
                try_reconstruct_from_tail_window(&mut file, stamp.len, id)?
            {
                if FileStamp::from_open_file(&file).as_ref() != Some(&stamp) {
                    return Ok((Phase::Unknown, false));
                }
                cursor.turns = turns;
                cursor.offset = tail_offset;
                cursor.caught_up = true;
                cursor.anchor = crate::jsonl_cache::file_anchor(&mut file, cursor.offset)?;
                let phase = cursor.turns.phase();
                let complete = cursor.offset == stamp.len;
                cursor.stamp = stamp;
                cursor.last_accessed = std::time::Instant::now();
                self.publish(cursor);
                return Ok((phase, complete));
            }
        }
        file.seek(SeekFrom::Start(cursor.offset))?;
        let available = stamp.len - cursor.offset;
        let mut reader = BufReader::new((&mut file).take(available.min(READ_BUDGET)));
        let mut scanned = 0;
        loop {
            let mut line = Vec::new();
            let bytes = reader
                .by_ref()
                .take(LINE_LIMIT + 1)
                .read_until(b'\n', &mut line)?;
            if bytes == 0 {
                break;
            }
            scanned += bytes as u64;
            if line.len() as u64 > LINE_LIMIT {
                cursor.turns.corrupt = true;
                cursor.offset += bytes as u64;
            } else if line.last() != Some(&b'\n') {
                break;
            } else {
                cursor.offset += bytes as u64;
                if line.iter().any(|byte| !byte.is_ascii_whitespace()) {
                    cursor.turns.accept(&line, id);
                }
            }
        }
        cursor.caught_up = scanned == available;
        // A half-written final line is ignored until its terminating newline arrives.
        let phase = if cursor.caught_up {
            cursor.turns.phase()
        } else {
            Phase::Unknown
        };
        if FileStamp::from_open_file(&file).as_ref() != Some(&stamp) {
            return Ok((Phase::Unknown, false));
        }
        cursor.anchor = crate::jsonl_cache::file_anchor(&mut file, cursor.offset)?;
        let complete = cursor.caught_up && cursor.offset == stamp.len;
        cursor.stamp = stamp;
        cursor.last_accessed = std::time::Instant::now();
        self.publish(cursor);
        Ok((phase, complete))
    }
    fn publish(&self, cursor: Cursor) {
        let target_path = cursor.path.clone();
        self.0
            .insert(cursor, |entry| entry.path == target_path, |_| 0);
    }
}

#[cfg(test)]
mod tests;
