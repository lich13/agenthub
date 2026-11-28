use super::*;
use std::{
    collections::{BTreeSet, HashSet},
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ownership {
    Inactive,
    Open,
    Unknown,
}
#[derive(Clone, Debug)]
struct Process {
    pid: u32,
    started_ms: Option<i64>,
    cwd: Option<PathBuf>,
    args: Vec<String>,
    files: HashSet<PathBuf>,
    inodes: HashSet<(u64, u64)>,
    role: ProcessRole,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessRole {
    Session,
    Daemon,
    PtyHost,
    Unknown,
}
impl ProcessRole {
    fn name(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Daemon => "daemon",
            Self::PtyHost => "pty_host",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) struct Assessment {
    pub ownership: Ownership,
    reasons: BTreeSet<&'static str>,
    processes: Vec<ClaudeOwnershipProcess>,
    native_status: Option<String>,
    native_status_updated_at: Option<i64>,
}
impl Assessment {
    fn reason(&mut self, code: &'static str) {
        self.reasons.insert(code);
    }
    /// Live state published by the verified native owner of this session.
    /// `busy` also covers delegated agents and workflows after the main turn.
    pub fn native_status(&self) -> Option<&str> {
        self.native_status.as_deref()
    }
    pub fn native_status_updated_at(&self) -> Option<i64> {
        self.native_status_updated_at
    }
    pub fn block_reason(&self) -> Option<String> {
        if self.ownership == Ownership::Inactive {
            return None;
        }
        Some(
            self.reasons
                .iter()
                .map(|code| {
                    let label = match *code {
                        "registered_owner" => "会话仍由已核验的原生进程持有",
                        "open_file" => "会话文件仍被原生进程打开",
                        "resume_owner" => "原生进程仍持有此会话的恢复身份",
                        "registry_unverified" => "会话进程登记未通过核验",
                        "foreign_domain" => "相关登记属于其他进程域",
                        "cwd_unknown" => "相关进程的工作目录不可读",
                        "same_cwd_unregistered" => "同工作目录存在未登记的会话进程",
                        "process_list_unavailable" => "原生进程列表不可用",
                        _ => "进程归属证据冲突",
                    };
                    format!("{label} [{code}]")
                })
                .collect::<Vec<_>>()
                .join("；"),
        )
    }
    pub fn diagnostics(self) -> ClaudeOwnershipDiagnostics {
        ClaudeOwnershipDiagnostics {
            ownership: match self.ownership {
                Ownership::Inactive => "inactive",
                Ownership::Open => "open",
                Ownership::Unknown => "unknown",
            }
            .into(),
            reason_codes: self.reasons.into_iter().map(str::to_owned).collect(),
            processes: self.processes,
        }
    }
}
#[derive(Clone)]
pub(super) struct Snapshot {
    processes: Option<Vec<Process>>,
    process_canonical_cwds: Vec<Option<PathBuf>>,
    registrations: Vec<Registration>,
    registration_canonical_cwds: Vec<Option<PathBuf>>,
    registrations_by_pid: HashMap<u32, Vec<usize>>,
    domain: Option<String>,
}

#[cfg(test)]
thread_local! { static TEST_SNAPSHOT: std::cell::RefCell<Option<Snapshot>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(super) fn test_owner(id: Option<&str>, cwd: &Path) {
    test_owner_with_status(id, cwd, None);
}
#[cfg(test)]
pub(super) fn test_owner_with_status(id: Option<&str>, cwd: &Path, status: Option<&str>) {
    let date = "Tue Oct 20 08:00:00 2026";
    invalidate_readonly_snapshot_cache();
    TEST_SNAPSHOT.with(|slot| {
        *slot.borrow_mut() = id.map(|id| {
            let domain = pid_domain();
            let canonical = cwd.canonicalize().ok();
            Snapshot {
                processes: Some(vec![Process {
                    pid: 42,
                    started_ms: started_ms(date),
                    cwd: Some(cwd.into()),
                    args: vec!["claude".into()],
                    files: HashSet::new(),
                    inodes: HashSet::new(),
                    role: ProcessRole::Session,
                }]),
                process_canonical_cwds: vec![canonical.clone()],
                registrations: vec![Registration {
                    pid: 42,
                    session_id: id.into(),
                    cwd: cwd.into(),
                    proc_start: date.into(),
                    started_at: started_ms(date).unwrap() + 500,
                    pid_domain: domain.clone().unwrap(),
                    status: status.map(str::to_owned),
                    status_updated_at: None,
                    updated_at: None,
                    file_matches_pid: true,
                }],
                registration_canonical_cwds: vec![canonical],
                registrations_by_pid: HashMap::from([(42, vec![0])]),
                domain,
            }
        })
    });
}

fn default_file_matches_pid() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registration {
    pid: u32,
    session_id: String,
    cwd: PathBuf,
    #[serde(default)]
    proc_start: String,
    started_at: i64,
    #[serde(default)]
    pid_domain: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    status_updated_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
    #[serde(skip, default = "default_file_matches_pid")]
    file_matches_pid: bool,
}

fn pid_domain() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        Some("darwin".into())
    }
    #[cfg(target_os = "linux")]
    {
        Some(format!(
            "linux:{}:{}",
            fs::read_to_string("/etc/machine-id").ok()?.trim(),
            fs::read_link("/proc/self/ns/pid").ok()?.to_str()?
        ))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

fn started_ms(value: &str) -> Option<i64> {
    jiff::civil::DateTime::strptime(
        "%a %b %e %T %Y",
        value.split_whitespace().collect::<Vec<_>>().join(" "),
    )
    .ok()
    .and_then(|dt| jiff::tz::Offset::UTC.to_timestamp(dt).ok())
    .map(|ts| ts.as_millisecond())
}

type ReadonlySnapshotCacheEntry = ((PathBuf, PathBuf), Snapshot, std::time::Instant);

static READONLY_SNAPSHOT_CACHE: std::sync::Mutex<Option<ReadonlySnapshotCacheEntry>> =
    std::sync::Mutex::new(None);
static CAPTURE_SINGLEFLIGHT: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
}

impl Snapshot {
    pub(super) fn capture_readonly(paths: &ClaudePaths) -> Self {
        #[cfg(test)]
        if let Some(snapshot) = TEST_SNAPSHOT.with(|slot| slot.borrow().clone()) {
            return snapshot;
        }
        let key = (paths.config.clone(), paths.projects.clone());
        if let Ok(guard) = READONLY_SNAPSHOT_CACHE.lock() {
            if let Some((ref cached_key, ref snap, ts)) = *guard {
                if *cached_key == key && ts.elapsed() < std::time::Duration::from_secs(2) {
                    return snap.clone();
                }
            }
        }
        let _flight = CAPTURE_SINGLEFLIGHT
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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

    pub(super) fn capture(paths: &ClaudePaths) -> Self {
        #[cfg(test)]
        if let Some(snapshot) = TEST_SNAPSHOT.with(|slot| slot.borrow().clone()) {
            return snapshot;
        }
        let root = paths.config.join("sessions");
        let mut registrations = Vec::new();
        let _ = (|| -> Result<()> {
            if !root.try_exists()? {
                return Ok(());
            }
            ensure!(
                !fs::symlink_metadata(&root)?.file_type().is_symlink(),
                "会话登记目录不可核验"
            );
            for (index, entry) in fs::read_dir(&root)?.enumerate() {
                ensure!(index < 20_000, "会话登记超出核验上限");
                let path = entry?.path();
                if path.extension().is_none_or(|s| s != "json") {
                    continue;
                }
                let registration = (|| -> Result<Registration> {
                    let meta = fs::symlink_metadata(&path)?;
                    ensure!(
                        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 262_144,
                        "会话登记无法核验"
                    );
                    let mut registration: Registration = serde_json::from_slice(&fs::read(&path)?)?;
                    ensure!(
                        registration.pid > 1 && !registration.session_id.is_empty(),
                        "会话登记身份缺失"
                    );
                    registration.file_matches_pid = path.file_stem().and_then(|s| s.to_str())
                        == Some(&registration.pid.to_string());
                    Ok(registration)
                })();
                if let Ok(registration) = registration {
                    registrations.push(registration);
                }
            }
            Ok(())
        })();
        Self::new(collect(), registrations)
    }
    fn new(processes: Option<Vec<Process>>, registrations: Vec<Registration>) -> Self {
        let mut registrations_by_pid: HashMap<u32, Vec<usize>> = HashMap::new();
        for (idx, reg) in registrations.iter().enumerate() {
            registrations_by_pid.entry(reg.pid).or_default().push(idx);
        }
        let process_canonical_cwds = processes
            .as_ref()
            .map(|list| {
                list.iter()
                    .map(|p| p.cwd.as_ref().and_then(|c| c.canonicalize().ok()))
                    .collect()
            })
            .unwrap_or_default();
        let registration_canonical_cwds = registrations
            .iter()
            .map(|r| r.cwd.canonicalize().ok())
            .collect();
        Self {
            processes,
            process_canonical_cwds,
            registrations,
            registration_canonical_cwds,
            registrations_by_pid,
            domain: pid_domain(),
        }
    }
    #[cfg(test)]
    pub(super) fn ownership(&self, parsed: &Parsed, path: &Path) -> Ownership {
        self.assess(parsed, path).ownership
    }
    pub(super) fn assess(&self, parsed: &Parsed, path: &Path) -> Assessment {
        self.assess_owned(parsed, &[path.to_path_buf()], &HashSet::new())
    }
    pub(super) fn assess_owned(
        &self,
        parsed: &Parsed,
        paths: &[PathBuf],
        children: &HashSet<String>,
    ) -> Assessment {
        let mut result = Assessment {
            ownership: Ownership::Inactive,
            reasons: BTreeSet::new(),
            processes: Vec::new(),
            native_status: None,
            native_status_updated_at: None,
        };
        let Some(processes) = &self.processes else {
            result.ownership = Ownership::Unknown;
            result.reason("process_list_unavailable");
            return result;
        };
        let parsed_canonical_cwds: std::cell::OnceCell<Vec<PathBuf>> = std::cell::OnceCell::new();
        let matches_session_cwd = |raw: &Path, precomputed_canon: Option<&Path>| -> bool {
            if raw == Path::new(&parsed.cwd)
                || parsed
                    .cwds
                    .iter()
                    .any(|seen| seen != &parsed.cwd && raw == Path::new(seen))
            {
                return true;
            }
            let Some(canon) = precomputed_canon else {
                return false;
            };
            let session_canons = parsed_canonical_cwds.get_or_init(|| {
                let mut out = Vec::new();
                if !parsed.cwd.is_empty() {
                    if let Ok(c) = Path::new(&parsed.cwd).canonicalize() {
                        out.push(c);
                    }
                }
                for seen in &parsed.cwds {
                    if seen != &parsed.cwd && !seen.is_empty() {
                        if let Ok(c) = Path::new(seen).canonicalize() {
                            if !out.contains(&c) {
                                out.push(c);
                            }
                        }
                    }
                }
                out
            });
            session_canons.iter().any(|c| c == canon)
        };
        let proc_canon = |p_idx: usize, p: &Process| -> Option<PathBuf> {
            match self.process_canonical_cwds.get(p_idx) {
                Some(cached) => cached.clone(),
                None => p.cwd.as_ref().and_then(|c| c.canonicalize().ok()),
            }
        };
        let reg_canon = |r_idx: usize, r: &Registration| -> Option<PathBuf> {
            match self.registration_canonical_cwds.get(r_idx) {
                Some(cached) => cached.clone(),
                None => r.cwd.canonicalize().ok(),
            }
        };
        let same_with_canon =
            |left: &Path, left_canon: Option<&Path>, right: &Path, right_canon: Option<&Path>| {
                left == right || left_canon.zip(right_canon).is_some_and(|(a, b)| a == b)
            };
        let target_id = |id: &str| id == parsed.id || children.contains(id);
        let domain = &self.domain;
        let domain_matches = |r: &Registration| {
            if r.pid_domain.is_empty() {
                domain.is_some()
            } else {
                domain.as_deref() == Some(r.pid_domain.as_str())
            }
        };
        for (r_idx, registration) in self.registrations.iter().enumerate() {
            let r_can = reg_canon(r_idx, registration);
            if !registration.pid_domain.is_empty()
                && domain.as_deref() != Some(&registration.pid_domain)
                && (target_id(&registration.session_id)
                    || matches_session_cwd(&registration.cwd, r_can.as_deref()))
            {
                result.reason("foreign_domain");
                result.processes.push(ClaudeOwnershipProcess {
                    pid: registration.pid,
                    role: "unknown".into(),
                    registration_verified: false,
                    mismatched_fields: vec!["pidDomain".into()],
                });
            }
        }
        let target_inodes: HashSet<(u64, u64)> = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                paths
                    .iter()
                    .filter_map(|p| fs::symlink_metadata(p).ok())
                    .filter(|m| m.is_file())
                    .map(|m| (m.dev(), m.ino()))
                    .collect()
            }
            #[cfg(not(unix))]
            {
                HashSet::new()
            }
        };
        let is_within_dir = |child: &Path,
                             child_canon: Option<&Path>,
                             parent: &Path,
                             parent_canon: Option<&Path>| {
            child.starts_with(parent)
                || child_canon
                    .zip(parent_canon)
                    .is_some_and(|(c, p)| c.starts_with(p))
        };
        for (p_idx, p) in processes.iter().enumerate() {
            let p_can = proc_canon(p_idx, p);
            let open_file = paths.iter().any(|path| p.files.contains(path))
                || (!target_inodes.is_empty()
                    && target_inodes.iter().any(|ino| p.inodes.contains(ino)));
            let cwd_matches = p
                .cwd
                .as_ref()
                .is_some_and(|cwd| matches_session_cwd(cwd, p_can.as_deref()));
            // Only OS-verified leading startup modes are exempt. Child argv
            // after `--` belongs to a separate process, checked independently.
            if matches!(p.role, ProcessRole::Daemon | ProcessRole::PtyHost) && !open_file {
                if cwd_matches {
                    result.processes.push(ClaudeOwnershipProcess {
                        pid: p.pid,
                        role: p.role.name().into(),
                        registration_verified: false,
                        mismatched_fields: Vec::new(),
                    });
                }
                continue;
            }
            let current_ids: Vec<_> = p
                .args
                .windows(2)
                .filter(|pair| pair[0] == "--session-id")
                .map(|pair| pair[1].as_str())
                .chain(
                    p.args
                        .iter()
                        .filter_map(|arg| arg.strip_prefix("--session-id=")),
                )
                .collect();
            let registrations: Vec<(usize, &Registration, Option<PathBuf>)> = self
                .registrations_by_pid
                .get(&p.pid)
                .into_iter()
                .flatten()
                .map(|&idx| {
                    (
                        idx,
                        &self.registrations[idx],
                        reg_canon(idx, &self.registrations[idx]),
                    )
                })
                .filter(|(_, r, _)| domain_matches(r))
                .collect();
            let proc_start_verified = |r: &Registration, r_can: Option<&Path>| {
                if r.proc_start.is_empty() {
                    p.started_ms.is_some_and(|actual| r.started_at >= actual)
                        && (open_file
                            || p.cwd.as_ref().is_some_and(|cwd| {
                                same_with_canon(cwd, p_can.as_deref(), &r.cwd, r_can)
                            }))
                } else {
                    started_ms(&r.proc_start).zip(p.started_ms).is_some_and(
                        |(registered, actual)| registered == actual && r.started_at >= actual,
                    )
                }
            };
            let cwd_verified_for_reg = |r: &Registration, r_can: Option<&Path>| {
                p.cwd.as_ref().is_none_or(|cwd| {
                    same_with_canon(cwd, p_can.as_deref(), &r.cwd, r_can)
                        || (r.file_matches_pid
                            && !r.proc_start.is_empty()
                            && !r.pid_domain.is_empty()
                            && domain.as_deref() == Some(r.pid_domain.as_str())
                            && target_id(&r.session_id)
                            && !current_ids.iter().any(|id| *id != r.session_id)
                            && matches_session_cwd(&r.cwd, r_can)
                            && is_within_dir(cwd, p_can.as_deref(), &r.cwd, r_can))
                })
            };
            let verified: Vec<&(usize, &Registration, Option<PathBuf>)> = registrations
                .iter()
                .filter(|(_, r, r_can)| {
                    p.role == ProcessRole::Session
                        && proc_start_verified(r, r_can.as_deref())
                        && cwd_verified_for_reg(r, r_can.as_deref())
                })
                .collect();
            let owns = verified.len() == 1
                && verified.iter().any(|(_, r, r_can)| {
                    children.contains(&r.session_id)
                        || (r.session_id == parsed.id
                            && matches_session_cwd(&r.cwd, r_can.as_deref()))
                });
            let explicit = p.args.windows(2).any(|pair| {
                matches!(pair[0].as_str(), "--resume" | "-r" | "--session-id")
                    && target_id(&pair[1])
            }) || p.args.iter().any(|a| {
                a.strip_prefix("--resume=")
                    .or_else(|| a.strip_prefix("--session-id="))
                    .is_some_and(target_id)
            });
            let relevant = owns
                || open_file
                || cwd_matches
                || p.cwd.is_none()
                || explicit
                || registrations
                    .iter()
                    .any(|(_, r, _)| target_id(&r.session_id));
            if !relevant {
                continue;
            }
            let mut mismatched = BTreeSet::new();
            for (_, r, r_can) in &registrations {
                if (!r.proc_start.is_empty() && started_ms(&r.proc_start) != p.started_ms)
                    || p.started_ms.is_none()
                {
                    mismatched.insert("procStart");
                }
                if p.started_ms.is_none_or(|start| r.started_at < start) {
                    mismatched.insert("startedAt");
                }
                if !cwd_verified_for_reg(r, r_can.as_deref()) {
                    mismatched.insert("cwd");
                }
            }
            if registrations.len() > 1 {
                mismatched.insert("sessionId");
            }
            let conflicting_cwd = verified.iter().any(|(_, r, r_can)| {
                r.session_id == parsed.id && !matches_session_cwd(&r.cwd, r_can.as_deref())
            });
            let conflicting_id =
                verified.len() == 1 && current_ids.iter().any(|id| *id != verified[0].1.session_id);
            if conflicting_id {
                mismatched.insert("sessionId");
                result.reason("evidence_conflict");
            }
            if conflicting_cwd {
                mismatched.insert("cwd");
            }
            if p.role == ProcessRole::Unknown {
                mismatched.insert("executable");
            }
            result.processes.push(ClaudeOwnershipProcess {
                pid: p.pid,
                role: p.role.name().into(),
                registration_verified: verified.len() == 1,
                mismatched_fields: mismatched.into_iter().map(str::to_owned).collect(),
            });
            if owns && !conflicting_id {
                result.native_status = verified[0].1.status.clone();
                result.native_status_updated_at =
                    verified[0].1.status_updated_at.or(verified[0].1.updated_at);
            }
            if explicit
                && p.role == ProcessRole::Session
                && registrations.is_empty()
                && (cwd_matches || open_file)
            {
                if p.started_ms
                    .zip(parsed.last_turn_at)
                    .is_some_and(|(start, turn)| start <= turn)
                {
                    result.ownership = Ownership::Open;
                }
                if open_file {
                    result.reason("open_file");
                }
                result.reason("resume_owner");
                continue;
            }
            if open_file || (owns && !conflicting_id) {
                result.ownership = Ownership::Open;
                result.reason(if open_file {
                    "open_file"
                } else {
                    "registered_owner"
                });
                if open_file && verified.len() == 1 && !owns {
                    result.reason("evidence_conflict");
                }
                continue;
            }
            // Exact registry identity is positive evidence of another owner;
            // absence of open files or resume flags is never inactivity proof.
            if conflicting_cwd {
                result.reason("evidence_conflict");
            }
            if verified.len() == 1 && !conflicting_id {
                continue;
            }
            if verified.len() > 1 {
                result.reason("evidence_conflict");
            }
            if !registrations.is_empty() {
                result.reason("registry_unverified");
            }
            if p.cwd.is_none() {
                result.reason("cwd_unknown");
            }
            if cwd_matches {
                result.reason("same_cwd_unregistered");
            }
            if explicit
                || registrations
                    .iter()
                    .any(|(_, r, _)| target_id(&r.session_id))
            {
                result.reason("registry_unverified");
            }
        }
        // A malformed registry must not poison targets whose cwd is known to
        // have no possible native process, or a process verified to another ID.
        if result.ownership != Ownership::Open && !result.reasons.is_empty() {
            result.ownership = Ownership::Unknown;
        }
        result
    }
}
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}
fn is_claude_code_script(arg: &str) -> bool {
    (arg.contains("/@anthropic-ai/claude-code/") || arg.contains(r"\@anthropic-ai\claude-code\"))
        && (arg.ends_with("cli.js") || arg.ends_with("claude.exe"))
}

/// Finds `@anthropic-ai/claude-code/cli.js` after optional `node`/`bun` flags.
/// If `has_argv0` is true (`args[0]` is `node`/`bun`), scanning starts at `args[1]`;
/// otherwise (`parse_process_table` where `comm` is separate), scanning starts at `args[0]`.
fn find_claude_script_index(args: &[String], has_argv0: bool) -> Option<usize> {
    let start = usize::from(has_argv0);
    let mut skip_next_flag_value = false;
    for (idx, arg) in args.iter().enumerate().skip(start) {
        if skip_next_flag_value {
            skip_next_flag_value = false;
            continue;
        }
        if arg == "--" {
            return args
                .get(idx + 1)
                .filter(|next| is_claude_code_script(next))
                .map(|_| idx + 1);
        }
        if arg.starts_with('-') {
            if matches!(
                arg.as_str(),
                "-r" | "--require"
                    | "--import"
                    | "--loader"
                    | "--experimental-loader"
                    | "-C"
                    | "--conditions"
                    | "--env-file"
                    | "--env-file-if-exists"
                    | "--config"
            ) {
                skip_next_flag_value = true;
            }
            continue;
        }
        return is_claude_code_script(arg).then_some(idx);
    }
    None
}

fn native(command: &str, args: &[String]) -> bool {
    let name = Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    matches!(name, "claude" | "claude.exe")
        || (matches!(name, "node" | "bun")
            && (find_claude_script_index(args, false).is_some()
                || find_claude_script_index(args, true).is_some()))
}

fn version_named(name: &str) -> bool {
    let parts: Vec<_> = name.split('.').collect();
    (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn is_verified_claude_version_binary(
    executable: &Path,
    executable_name: &str,
    home: Option<&Path>,
) -> bool {
    if home.is_some_and(|home| {
        executable
            .parent()
            .is_some_and(|parent| same_path(parent, &home.join(".local/share/claude/versions")))
            || home
                .join(".local/bin/claude")
                .canonicalize()
                .ok()
                .as_deref()
                == Some(executable)
    }) {
        return true;
    }
    if !version_named(executable_name) {
        return false;
    }
    let exec_canon = executable.canonicalize().ok();
    let matches_exec = |candidate: &Path| {
        candidate
            .canonicalize()
            .ok()
            .is_some_and(|canon| Some(&canon) == exec_canon.as_ref() || canon == executable)
    };
    for fixed in [
        "/opt/homebrew/bin/claude",
        "/usr/local/bin/claude",
        "/usr/bin/claude",
    ] {
        if matches_exec(Path::new(fixed)) {
            return true;
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if matches_exec(&dir.join("claude")) {
                return true;
            }
        }
    }
    if let Some(parent) = executable.parent() {
        let parent_is_versions = parent.file_name().and_then(|s| s.to_str()) == Some("versions");
        let owner_is_claude = parent
            .parent()
            .and_then(|gp| gp.file_name())
            .and_then(|s| s.to_str())
            .is_some_and(|s| matches!(s, "claude" | "claude-code"));
        let is_linuxbrew_prefix = {
            let mut comps = executable.components();
            matches!(
                (comps.next(), comps.next(), comps.next()),
                (
                    Some(std::path::Component::RootDir),
                    Some(std::path::Component::Normal(root_dir)),
                    Some(std::path::Component::Normal(brew_user)),
                ) if root_dir == "home" && brew_user == "linuxbrew"
            )
        };
        let trusted_prefix = ["/opt/homebrew", "/usr/local", "/usr/lib"]
            .iter()
            .any(|p| executable.starts_with(p))
            || is_linuxbrew_prefix
            || home.is_some_and(|h| executable.starts_with(h));
        if parent_is_versions && owner_is_claude && trusted_prefix {
            return true;
        }
    }
    false
}

fn process_role(executable: &Path, args: &[String]) -> ProcessRole {
    process_role_at_home(executable, args, dirs::home_dir().as_deref())
}
fn process_role_at_home(executable: &Path, args: &[String], home: Option<&Path>) -> ProcessRole {
    let executable_name = executable
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let script_idx = matches!(executable_name, "node" | "bun")
        .then(|| find_claude_script_index(args, true))
        .flatten();
    let installed = is_verified_claude_version_binary(executable, executable_name, home);
    if !installed && !matches!(executable_name, "claude" | "claude.exe") && script_idx.is_none() {
        return ProcessRole::Unknown;
    }
    let option_start = script_idx.map_or(1, |idx| idx + 1);
    let Some(options) = args.get(option_start..) else {
        return ProcessRole::Unknown;
    };
    if options.first().is_some_and(|s| s == "--bg-pty-host") {
        ProcessRole::PtyHost
    } else if options
        .get(..2)
        .is_some_and(|pair| pair == ["daemon", "run"])
    {
        ProcessRole::Daemon
    } else {
        // --bg-spare can become a session in this same PID after being claimed.
        ProcessRole::Session
    }
}

fn process_identity(pid: u32) -> Option<(PathBuf, Vec<String>)> {
    crate::platform::resolve_kernel_process_identity(pid)
}

fn collect() -> Option<Vec<Process>> {
    // macOS truncates `comm` paths to 16 characters even with -ww. `ucomm`
    // comes from the native process name, so long CLI installation paths do
    // not hide a live Claude process or rely on a caller-controlled argv[0].
    #[cfg(target_os = "macos")]
    let format = "pid=,stat=,lstart=,ucomm=,args=";
    #[cfg(not(target_os = "macos"))]
    let format = "pid=,stat=,lstart=,comm=,args=";
    let output = Command::new("/bin/ps")
        .args(["-ww", "-axo", format])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut processes = parse_process_table(&output.stdout)?;
    if processes.is_empty() {
        return Some(processes);
    }
    #[cfg(not(target_os = "linux"))]
    let mut missing_cwd_pids = Vec::new();
    for p in &mut processes {
        p.cwd = crate::platform::resolve_kernel_cwd(p.pid);
        if let Some((files, inodes)) = crate::platform::collect_kernel_open_vnodes(p.pid) {
            p.files = files;
            p.inodes = inodes;
        }
        #[cfg(not(target_os = "linux"))]
        if p.cwd.is_none() {
            missing_cwd_pids.push(p.pid);
        }
    }
    #[cfg(not(target_os = "linux"))]
    if !missing_cwd_pids.is_empty() {
        let pids = missing_cwd_pids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        if let Ok(output) =
            crate::platform::sanitized_lsof_command(crate::platform::default_lsof_bin())
                .args(["-nP", "-p", &pids, "-Fpnf"])
                .output()
        {
            let mut current = None;
            let mut descriptor = "";
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if let Some(pid) = line.strip_prefix('p').and_then(|s| s.parse::<u32>().ok()) {
                    current = processes.iter().position(|p| p.pid == pid);
                } else if let Some(fd) = line.strip_prefix('f') {
                    descriptor = fd;
                } else if let (Some(index), Some(name)) = (current, line.strip_prefix('n')) {
                    let decoded = crate::platform::decode_lsof_path(name);
                    if descriptor == "cwd" && processes[index].cwd.is_none() {
                        processes[index].cwd = Some(decoded);
                    } else if name.starts_with('/') {
                        processes[index].files.insert(decoded);
                    }
                }
            }
        }
    }
    #[cfg(unix)]
    processes.retain(|p| {
        p.cwd.is_some()
            || !p.files.is_empty()
            || !p.inodes.is_empty()
            || crate::platform::pid_is_alive(p.pid)
    });
    Some(processes)
}

fn parse_process_table(output: &[u8]) -> Option<Vec<Process>> {
    let mut processes = Vec::new();
    for line in String::from_utf8_lossy(output).lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 9 || fields[1].contains('Z') {
            continue;
        }
        let Ok(pid) = fields[0].parse::<u32>() else {
            continue;
        };
        let Some(started) =
            jiff::civil::DateTime::strptime("%a %b %e %T %Y", fields[2..7].join(" "))
                .ok()
                .and_then(|dt| jiff::tz::Offset::UTC.to_timestamp(dt).ok())
                .map(|ts| ts.as_millisecond())
        else {
            continue;
        };
        let mut args = fields[8..]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let direct_named = native(fields[7], &args);
        let mut candidate = version_named(fields[7]);
        // Process names (`ucomm`/`comm`) or wrapper directories can contain spaces (e.g.
        // `/opt/Application Support/@anthropic-ai/claude-code/cli.js`).
        // Space-split nominations are only candidates and must pass kernel `process_identity`.
        if !direct_named && !candidate && fields.len() > 9 {
            for split_end in 8..fields.len().min(12) {
                let joined_comm = fields[7..=split_end].join(" ");
                let rem_args: Vec<String> = fields[split_end + 1..]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                if native(&joined_comm, &rem_args) || version_named(fields[split_end]) {
                    candidate = true;
                    break;
                }
            }
        }
        if !direct_named && !candidate {
            continue;
        }
        let role = if let Some((executable, native_args)) = process_identity(pid) {
            let role = process_role(&executable, &native_args);
            args = native_args;
            role
        } else {
            ProcessRole::Unknown
        };
        // A native installation runs as its version file (or space-split wrapper path),
        // so only the kernel-verified executable admits non-direct candidates.
        if !direct_named && role == ProcessRole::Unknown {
            continue;
        }
        processes.push(Process {
            pid,
            started_ms: Some(started),
            cwd: None,
            args,
            files: HashSet::new(),
            inodes: HashSet::new(),
            role,
        });
    }
    Some(processes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_invalid_utf8_does_not_hide_native_process_candidates() {
        let output = b"99 S Tue Oct 20 08:00:00 2026 unrelated /fixture/\xff\n999999 S Tue Oct 20 08:00:00 2026 claude claude --resume fixture\n";
        let processes = parse_process_table(output).unwrap();
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].pid, 999999);
        assert_eq!(processes[0].role, ProcessRole::Unknown);
    }

    #[cfg(unix)]
    #[test]
    fn relocated_version_directory_keeps_verified_native_role() {
        let root =
            env::temp_dir().join(format!("agenthub-claude-version-{}", uuid::Uuid::new_v4()));
        let versions = root.join("moved-versions");
        let home = root.join("home");
        fs::create_dir_all(&versions).unwrap();
        fs::create_dir_all(home.join(".local/share/claude")).unwrap();
        std::os::unix::fs::symlink(&versions, home.join(".local/share/claude/versions")).unwrap();
        let executable = versions.join("2.1.322");
        fs::write(&executable, "fixture").unwrap();
        assert_eq!(
            process_role_at_home(&executable, &["claude".into()], Some(&home)),
            ProcessRole::Session
        );
        assert_eq!(
            process_role_at_home(&root.join("2.1.322"), &["claude".into()], Some(&home)),
            ProcessRole::Unknown
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_rechecks_unknown_ownership_after_an_inactive_preview() {
        let root =
            env::temp_dir().join(format!("agenthub-claude-unknown-{}", uuid::Uuid::new_v4()));
        let paths = ClaudePaths {
            projects: root.join("projects"),
            config: root.clone(),
        };
        let file = paths.projects.join("fixture/empty.jsonl");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "").unwrap();
        TEST_SNAPSHOT.with(|slot| *slot.borrow_mut() = Some(Snapshot::new(Some(vec![]), vec![])));
        let candidates = super::super::mutations::find_empty_inactive_sessions(&paths).unwrap();
        assert_eq!(candidates.len(), 1);
        TEST_SNAPSHOT.with(|slot| *slot.borrow_mut() = Some(Snapshot::new(None, vec![])));
        let (summary, fingerprint, _) = &candidates[0];
        let result = super::super::mutations::execute_claude_delete(
            &paths,
            ClaudeDeleteRequest {
                session_key: summary.session_key.clone(),
                confirmed: true,
                fingerprint: fingerprint.clone(),
            },
        );
        TEST_SNAPSHOT.with(|slot| *slot.borrow_mut() = None);
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("process_list_unavailable"));
        assert!(file.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_roles_use_leading_native_arguments_and_never_exempt_spares() {
        for (args, expected) in [
            (
                vec!["claude", "daemon", "run", "--resume", "target"],
                ProcessRole::Daemon,
            ),
            (
                vec![
                    "claude bg-pty-host",
                    "--bg-pty-host",
                    "--",
                    "claude",
                    "--resume",
                    "target",
                ],
                ProcessRole::PtyHost,
            ),
            (vec!["claude bg-spare", "--bg-spare"], ProcessRole::Session),
            (
                vec!["claude", "--resume", "target", "--bg-pty-host"],
                ProcessRole::Session,
            ),
            (
                vec!["claude", "--print", "daemon run"],
                ProcessRole::Session,
            ),
        ] {
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(process_role(Path::new("/fixture/claude"), &args), expected);
            assert_eq!(
                process_role(Path::new("/fixture/unverified"), &args),
                ProcessRole::Unknown
            );
        }
    }

    #[test]
    fn version_named_processes_are_candidates_and_registry_status_needs_a_verified_owner() {
        for name in ["2.1.322", "3.0", "10.20.30.40"] {
            assert!(version_named(name), "{name}");
        }
        for name in [
            "claude",
            "2",
            "2.1.x",
            "2..1",
            ".2.1",
            "1.2.3.4.5",
            "v2.1.3",
            "",
        ] {
            assert!(!version_named(name), "{name}");
        }
        // The name only nominates a process; an unverified executable is never a session.
        assert_eq!(
            process_role(Path::new("/fixture/2.1.322"), &["claude".to_owned()]),
            ProcessRole::Unknown
        );

        let parsed = Parsed {
            id: "target".into(),
            cwd: "/fixture/project".into(),
            ..Default::default()
        };
        let file = Path::new("/fixture/target.jsonl");
        test_owner_with_status(Some("target"), Path::new(&parsed.cwd), Some("busy"));
        let mut snapshot = TEST_SNAPSHOT.with(|slot| slot.borrow().clone().unwrap());
        test_owner(None, Path::new("/fixture"));
        assert_eq!(snapshot.assess(&parsed, file).native_status(), Some("busy"));
        let other = Parsed {
            id: "other".into(),
            ..parsed.clone()
        };
        assert_eq!(snapshot.assess(&other, file).native_status(), None);
        // A session that changed directory still belongs to the owner that
        // registered its launch directory.
        let moved = Parsed {
            cwd: "/fixture/project/nested".into(),
            cwds: vec!["/fixture/project".into(), "/fixture/project/nested".into()],
            ..parsed.clone()
        };
        let assessment = snapshot.assess(&moved, file);
        assert_eq!(assessment.ownership, Ownership::Open);
        assert_eq!(assessment.native_status(), Some("busy"));
        let elsewhere = Parsed {
            cwd: "/fixture/elsewhere".into(),
            cwds: vec!["/fixture/elsewhere".into()],
            ..parsed.clone()
        };
        assert_eq!(
            snapshot.assess(&elsewhere, file).ownership,
            Ownership::Unknown
        );
        // A registration that fails verification publishes nothing.
        snapshot.registrations[0].proc_start = "invalid".into();
        assert_eq!(snapshot.assess(&parsed, file).native_status(), None);
        let registration: Registration = serde_json::from_value(serde_json::json!({
            "pid": 42, "sessionId": "target", "cwd": "/fixture/project",
            "procStart": "Tue Oct 20 08:00:00 2026", "startedAt": 1, "pidDomain": "darwin"
        }))
        .unwrap();
        assert_eq!(registration.status, None);
    }

    #[test]
    fn verified_hosts_do_not_claim_child_arguments_but_children_and_spares_are_protected() {
        let parsed = Parsed {
            id: "target".into(),
            cwd: "/fixture/project".into(),
            last_turn_at: Some(2000),
            ..Default::default()
        };
        let file = Path::new("/fixture/target.jsonl");
        let host = Process {
            pid: 40,
            started_ms: Some(1000),
            cwd: Some(parsed.cwd.clone().into()),
            args: vec![
                "claude".into(),
                "--bg-pty-host".into(),
                "--".into(),
                "claude".into(),
                "--resume".into(),
                "target".into(),
            ],
            files: HashSet::new(),
            inodes: HashSet::new(),
            role: ProcessRole::PtyHost,
        };
        let mut snapshot = Snapshot::new(
            Some(vec![
                host.clone(),
                Process {
                    pid: 41,
                    role: ProcessRole::Daemon,
                    ..host.clone()
                },
            ]),
            vec![],
        );
        assert_eq!(snapshot.ownership(&parsed, file), Ownership::Inactive);
        snapshot.processes.as_mut().unwrap().push(Process {
            pid: 42,
            role: ProcessRole::Session,
            args: vec!["claude".into(), "--resume".into(), "target".into()],
            ..host.clone()
        });
        assert_eq!(snapshot.ownership(&parsed, file), Ownership::Open);
        snapshot.processes.as_mut().unwrap()[2].args =
            vec!["claude bg-spare".into(), "--bg-spare".into()];
        assert!(snapshot
            .assess(&parsed, file)
            .block_reason()
            .unwrap()
            .contains("same_cwd_unregistered"));
        snapshot.processes.as_mut().unwrap().truncate(2);
        snapshot.processes.as_mut().unwrap()[0]
            .files
            .insert(file.into());
        assert!(snapshot
            .assess(&parsed, file)
            .block_reason()
            .unwrap()
            .contains("open_file"));
    }

    #[test]
    fn diagnostics_explain_mismatches_without_exporting_native_values() {
        let parsed = Parsed {
            id: "target-secret".into(),
            cwd: "/fixture/private-project".into(),
            ..Default::default()
        };
        test_owner(Some(&parsed.id), Path::new(&parsed.cwd));
        let mut snapshot = TEST_SNAPSHOT.with(|s| s.borrow().clone().unwrap());
        test_owner(None, Path::new("/fixture"));
        snapshot.registrations[0].proc_start = "invalid".into();
        snapshot.registrations[0].started_at = 0;
        let assessment = snapshot.assess(&parsed, Path::new("/fixture/private-transcript.jsonl"));
        assert_eq!(assessment.ownership, Ownership::Unknown);
        assert!(assessment
            .block_reason()
            .unwrap()
            .contains("registry_unverified"));
        let diagnostics = assessment.diagnostics();
        assert_eq!(
            diagnostics.processes[0].mismatched_fields,
            ["procStart", "startedAt"]
        );
        let json = serde_json::to_string(&diagnostics).unwrap();
        for private in [
            "target-secret",
            "private-project",
            "private-transcript",
            "--resume",
            "proc_start",
        ] {
            assert!(!json.contains(private));
        }
        snapshot.processes.as_mut().unwrap()[0].cwd = None;
        assert!(snapshot
            .assess(&parsed, Path::new("/fixture/file"))
            .reasons
            .contains("cwd_unknown"));
        snapshot.registrations[0].pid_domain = "elsewhere".into();
        assert!(snapshot
            .assess(&parsed, Path::new("/fixture/file"))
            .reasons
            .contains("foreign_domain"));
        snapshot.processes = None;
        assert!(snapshot
            .assess(&parsed, Path::new("/fixture/file"))
            .reasons
            .contains("process_list_unavailable"));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn kernel_arguments_preserve_this_test_process_identity() {
        let (executable, args) = process_identity(std::process::id()).unwrap();
        assert!(!args.is_empty());
        assert_eq!(
            executable.canonicalize().unwrap(),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        assert_eq!(process_role(&executable, &args), ProcessRole::Unknown);
    }
    #[test]
    fn verified_registry_scopes_same_cwd_and_survives_unavailable_lsof() {
        let parsed = Parsed {
            id: "historical".into(),
            cwd: "/fixture/project".into(),
            ..Default::default()
        };
        let date = "Tue Oct 20 08:00:00 2026";
        let process = Process {
            pid: 42,
            started_ms: started_ms(date),
            cwd: None,
            args: vec!["claude".into()],
            files: HashSet::new(),
            inodes: HashSet::new(),
            role: ProcessRole::Session,
        };
        let registration = Registration {
            pid: 42,
            session_id: "current".into(),
            cwd: parsed.cwd.clone().into(),
            proc_start: date.into(),
            started_at: started_ms(date).unwrap() + 500,
            pid_domain: pid_domain().unwrap(),
            status: None,
            status_updated_at: None,
            updated_at: None,
            file_matches_pid: true,
        };
        let snapshot = Snapshot::new(Some(vec![process.clone()]), vec![registration.clone()]);
        assert_eq!(
            snapshot.ownership(&parsed, Path::new("/fixture/history.jsonl")),
            Ownership::Inactive
        );
        assert_eq!(
            snapshot.ownership(
                &Parsed {
                    id: "current".into(),
                    ..parsed.clone()
                },
                Path::new("/fixture/current.jsonl")
            ),
            Ownership::Open
        );
        for registration in [
            Registration {
                proc_start: "Mon Oct 19 08:00:00 2026".into(),
                ..registration.clone()
            },
            Registration {
                pid_domain: "foreign-domain".into(),
                ..registration
            },
        ] {
            let snapshot = Snapshot::new(Some(vec![process.clone()]), vec![registration]);
            assert_eq!(
                snapshot.ownership(&parsed, Path::new("/fixture/history.jsonl")),
                Ownership::Unknown
            );
        }
    }

    #[test]
    fn corrupt_registry_only_blocks_targets_a_live_unmapped_process_could_own() {
        let parsed = Parsed {
            id: "fixture".into(),
            cwd: "/fixture/project".into(),
            ..Default::default()
        };
        let process = Process {
            pid: 42,
            started_ms: Some(1000),
            cwd: Some("/fixture/elsewhere".into()),
            args: vec!["claude".into()],
            files: HashSet::new(),
            inodes: HashSet::new(),
            role: ProcessRole::Session,
        };
        let mut snapshot = Snapshot::new(Some(vec![process]), vec![]);
        assert_eq!(
            snapshot.ownership(&parsed, Path::new("/fixture/history.jsonl")),
            Ownership::Inactive
        );
        snapshot.processes.as_mut().unwrap()[0].cwd = Some(parsed.cwd.clone().into());
        assert_eq!(
            snapshot.ownership(&parsed, Path::new("/fixture/history.jsonl")),
            Ownership::Unknown
        );
    }
    #[test]
    fn inactive_reused_idle_and_unknown_ownership_do_not_prove_running() {
        let p = Parsed {
            id: "native-id".into(),
            cwd: "/tmp/example".into(),
            last_turn_at: Some(2000),
            ..Default::default()
        };
        let file = Path::new("/tmp/example.jsonl");
        assert_eq!(
            Snapshot::new(Some(vec![]), vec![]).ownership(&p, file),
            Ownership::Inactive
        );
        let owner = Process {
            pid: 5,
            started_ms: Some(1000),
            cwd: Some(PathBuf::from(&p.cwd)),
            args: vec!["claude".into(), "--resume".into(), p.id.clone()],
            files: HashSet::new(),
            inodes: HashSet::new(),
            role: ProcessRole::Session,
        };
        assert_eq!(
            Snapshot::new(Some(vec![owner.clone()]), vec![]).ownership(&p, file),
            Ownership::Open
        );
        let idle_resume = Snapshot::new(
            Some(vec![Process {
                started_ms: Some(3000),
                ..owner.clone()
            }]),
            vec![],
        );
        let idle_assessment = idle_resume.assess(&p, file);
        assert_eq!(idle_assessment.ownership, Ownership::Unknown);
        assert!(idle_assessment.reasons.contains("resume_owner"));
        assert!(!idle_assessment.reasons.contains("registry_unverified"));
        // Even if `--resume` holds the main JSONL fd open while sitting idle at
        // the prompt (`started_ms > last_turn_at`), it blocks rename without
        // claiming active turn execution (`Ownership::Open`).
        let idle_resume_open_fd = Snapshot::new(
            Some(vec![Process {
                started_ms: Some(3000),
                files: HashSet::from([file.to_path_buf()]),
                ..owner.clone()
            }]),
            vec![],
        );
        let idle_fd_assessment = idle_resume_open_fd.assess(&p, file);
        assert_eq!(idle_fd_assessment.ownership, Ownership::Unknown);
        assert!(idle_fd_assessment.reasons.contains("open_file"));
        assert!(idle_fd_assessment.reasons.contains("resume_owner"));
        // Once a turn executes after `--resume` (`started_ms <= last_turn_at`),
        // open fd proves `Ownership::Open` even if `cwd` moved into a subdirectory.
        let active_resume_open_fd = Snapshot::new(
            Some(vec![Process {
                started_ms: Some(1500),
                cwd: Some(PathBuf::from("/tmp/example/subdir")),
                files: HashSet::from([file.to_path_buf()]),
                ..owner.clone()
            }]),
            vec![],
        );
        assert_eq!(active_resume_open_fd.ownership(&p, file), Ownership::Open);
        assert_eq!(
            Snapshot::new(
                Some(vec![Process {
                    args: vec!["claude".into()],
                    ..owner
                }]),
                vec![],
            )
            .ownership(&p, file),
            Ownership::Unknown
        );
        assert_eq!(
            Snapshot::new(None, vec![]).ownership(&p, file),
            Ownership::Unknown
        );
        assert!(!native("zsh", &["claude".into()]));
    }

    #[test]
    fn chinese_paths_subdir_drift_and_node_flags_keep_verified_claude_owner() {
        let date = "Tue Oct 20 08:00:00 2026";
        let chinese_cwd = PathBuf::from("/Users/example/Documents/程序开发/AgentHub");
        let subdir_cwd = chinese_cwd.join("crates/agenthub-core");
        let parsed = Parsed {
            id: "1102f538-0000-4000-8000-000000000000".into(),
            cwd: chinese_cwd.display().to_string(),
            ..Default::default()
        };
        let proc_in_subdir = Process {
            pid: 42,
            started_ms: started_ms(date),
            cwd: Some(subdir_cwd),
            args: vec![
                "node".into(),
                "--no-warnings".into(),
                "/opt/lib/node_modules/@anthropic-ai/claude-code/cli.js".into(),
            ],
            files: HashSet::new(),
            inodes: HashSet::new(),
            role: process_role(
                Path::new("/usr/local/bin/node"),
                &[
                    "node".into(),
                    "--no-warnings".into(),
                    "/opt/lib/node_modules/@anthropic-ai/claude-code/cli.js".into(),
                ],
            ),
        };
        assert_eq!(proc_in_subdir.role, ProcessRole::Session);
        assert!(native(
            "/usr/local/bin/node",
            &[
                "--no-warnings".into(),
                "/opt/lib/node_modules/@anthropic-ai/claude-code/cli.js".into(),
            ]
        ));
        let reg = Registration {
            pid: 42,
            session_id: parsed.id.clone(),
            cwd: chinese_cwd,
            proc_start: date.into(),
            started_at: started_ms(date).unwrap() + 200,
            pid_domain: pid_domain().unwrap(),
            status: Some("busy".into()),
            status_updated_at: None,
            updated_at: None,
            file_matches_pid: true,
        };
        let snapshot = Snapshot::new(Some(vec![proc_in_subdir]), vec![reg]);
        let assessment = snapshot.assess(&parsed, Path::new("/fixture/1102f538.jsonl"));
        assert_eq!(assessment.ownership, Ownership::Open);
        assert_eq!(assessment.native_status(), Some("busy"));
        assert!(!assessment.reasons.contains("registry_unverified"));
    }
}
