use super::{GrokDeletionEligibility, GrokPaths, GrokSessionSummary};
use chrono::{DateTime, NaiveDateTime};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
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
        }
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SNAPSHOT: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn test_owner(id: Option<&str>, cwd: &str) {
    invalidate_readonly_snapshot_cache();
    TEST_SNAPSHOT.with(|slot| {
        *slot.borrow_mut() = id.map(|id| (id.to_owned(), cwd.to_owned()));
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
            Phase::Empty if owner == Ownership::Inactive => "recent",
            _ => "unknown",
        }
    }

    pub(super) fn update_summary(&self, session: &mut GrokSessionSummary) {
        let owner = self.ownership(session);
        let (phase, complete) = lifecycle(session);
        session.status = Self::display_status(owner, phase).into();
        session.deletion_eligibility = Some(Self::eligibility(owner, phase, complete));
    }

    fn eligibility(_owner: Ownership, _phase: Phase, _complete: bool) -> GrokDeletionEligibility {
        GrokDeletionEligibility {
            allowed: true,
            reason: None,
        }
    }

    #[cfg(test)]
    pub(super) fn deletion_eligibility(
        &self,
        session: &GrokSessionSummary,
    ) -> GrokDeletionEligibility {
        let (phase, complete) = lifecycle(session);
        Self::eligibility(self.ownership(session), phase, complete)
    }

    #[cfg(test)]
    pub(super) fn ensure_deletable(&self, session: &GrokSessionSummary) -> anyhow::Result<()> {
        let eligibility = self.deletion_eligibility(session);
        anyhow::ensure!(
            eligibility.allowed,
            "{}",
            eligibility.reason.unwrap_or_default()
        );
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn ensure_not_open(&self, session: &GrokSessionSummary) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.ownership(session) == Ownership::Inactive,
            "Grok session is still open in a native process or its ownership cannot be confirmed"
        );
        Ok(())
    }
}

fn same_path(left: &str, right: &str) -> bool {
    left == right
        || fs::canonicalize(left)
            .ok()
            .zip(fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
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
                opened_at: row
                    .get("opened_at")
                    .or_else(|| row.get("openedAt"))
                    .and_then(Value::as_str)
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.timestamp()),
                cwd: row.get("cwd").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect()
}

fn collect_processes(paths: &GrokPaths, pids: &HashSet<u32>) -> Option<HashMap<u32, Process>> {
    let output = Command::new("ps")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .args(["-axo", "pid=,lstart=,stat=,comm="])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 8 * 1024 * 1024 {
        return None;
    }
    let executables: HashSet<_> = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join("grok"))
        .chain([paths.home.join("bin/grok")])
        .chain(dirs::home_dir().map(|home| home.join(".grok/bin/grok")))
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    let mut processes = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|value| value.parse().ok()) else {
            continue;
        };
        if !pids.contains(&pid) {
            continue;
        }
        let start = fields.by_ref().take(5).collect::<Vec<_>>().join(" ");
        let started_at = NaiveDateTime::parse_from_str(&start, "%a %b %e %T %Y")
            .ok()
            .map(|time| time.and_utc().timestamp());
        let zombie = fields.next().is_some_and(|state| state.starts_with('Z'));
        let command = fields.collect::<Vec<_>>().join(" ");
        #[cfg(target_os = "linux")]
        let executable = {
            let _ = command;
            fs::read_link(format!("/proc/{pid}/exe")).ok()
        };
        #[cfg(not(target_os = "linux"))]
        let executable = Path::new(&command).canonicalize().ok();
        let native = executable.and_then(|path| {
            if executables.contains(&path) {
                Some(true)
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("grok"))
            {
                // A running CLI can retain an old/deleted executable after an upgrade.
                // An unverified Grok binary must not release deletion protection.
                None
            } else {
                Some(false)
            }
        });
        processes.insert(
            pid,
            Process {
                started_at,
                zombie,
                native,
            },
        );
    }
    Some(processes)
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
                            Some("completed" | "cancelled" | "error" | "failed")
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

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64),
    #[cfg(unix)]
    changed: (i64, i64),
}

impl Stamp {
    fn read(file: &File) -> Option<Self> {
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok()?,
            #[cfg(unix)]
            identity: (metadata.dev(), metadata.ino()),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
    fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.identity == other.identity
        }
        #[cfg(not(unix))]
        {
            let _ = other;
            false
        }
    }
}

struct Cursor {
    path: PathBuf,
    id: String,
    stamp: Stamp,
    offset: u64,
    anchor: Vec<u8>,
    turns: Turns,
    caught_up: bool,
}

struct TurnCache(Mutex<VecDeque<Cursor>>);
static TURN_CACHE: TurnCache = TurnCache(Mutex::new(VecDeque::new()));
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
        let stamp =
            Stamp::read(&file).ok_or_else(|| std::io::Error::other("invalid activity file"))?;
        let old = self.0.lock().ok().and_then(|mut entries| {
            let index = entries
                .iter()
                .position(|entry| entry.path == path && entry.id == id)?;
            entries.remove(index)
        });
        let mut cursor = match old {
            Some(cursor) if cursor.stamp == stamp && cursor.caught_up => {
                let state = (cursor.turns.phase(), cursor.offset == stamp.len);
                self.publish(cursor);
                return Ok(state);
            }
            Some(cursor)
                if cursor.stamp.same_file(&stamp)
                    && (stamp.len > cursor.stamp.len
                        || (cursor.stamp == stamp && !cursor.caught_up))
                    && anchor(&mut file, cursor.offset)? == cursor.anchor =>
            {
                cursor
            }
            _ => Cursor {
                path: path.to_owned(),
                id: id.to_owned(),
                stamp: stamp.clone(),
                offset: 0,
                anchor: Vec::new(),
                turns: Turns::default(),
                caught_up: false,
            },
        };
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
        if Stamp::read(&file).as_ref() != Some(&stamp) {
            return Ok((Phase::Unknown, false));
        }
        cursor.anchor = anchor(&mut file, cursor.offset)?;
        let complete = cursor.caught_up && cursor.offset == stamp.len;
        cursor.stamp = stamp;
        self.publish(cursor);
        Ok((phase, complete))
    }
    fn publish(&self, cursor: Cursor) {
        if let Ok(mut entries) = self.0.lock() {
            entries.retain(|entry| entry.path != cursor.path);
            while entries.len() >= 256 {
                entries.pop_front();
            }
            entries.push_back(cursor);
        }
    }
}

fn anchor(file: &mut File, offset: u64) -> std::io::Result<Vec<u8>> {
    let len = offset.min(64);
    let mut value = vec![0; len as usize];
    file.seek(SeekFrom::Start(offset - len))?;
    file.read_exact(&mut value)?;
    Ok(value)
}

#[cfg(test)]
mod tests;
