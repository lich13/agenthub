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
}
impl Assessment {
    fn reason(&mut self, code: &'static str) {
        self.reasons.insert(code);
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
    registrations: Vec<Registration>,
}

#[cfg(test)]
thread_local! { static TEST_SNAPSHOT: std::cell::RefCell<Option<Snapshot>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(super) fn test_owner(id: Option<&str>, cwd: &Path) {
    let date = "Tue Oct 20 08:00:00 2026";
    invalidate_readonly_snapshot_cache();
    TEST_SNAPSHOT.with(|slot| {
        *slot.borrow_mut() = id.map(|id| Snapshot {
            processes: Some(vec![Process {
                pid: 42,
                started_ms: started_ms(date),
                cwd: Some(cwd.into()),
                args: vec!["claude".into()],
                files: HashSet::new(),
                role: ProcessRole::Session,
            }]),
            registrations: vec![Registration {
                pid: 42,
                session_id: id.into(),
                cwd: cwd.into(),
                proc_start: date.into(),
                started_at: started_ms(date).unwrap() + 500,
                pid_domain: pid_domain().unwrap(),
            }],
        })
    });
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registration {
    pid: u32,
    session_id: String,
    cwd: PathBuf,
    proc_start: String,
    started_at: i64,
    pid_domain: String,
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
    chrono::NaiveDateTime::parse_from_str(
        &value.split_whitespace().collect::<Vec<_>>().join(" "),
        "%a %b %e %T %Y",
    )
    .ok()
    .map(|d| d.and_utc().timestamp_millis())
}

type ReadonlySnapshotCacheEntry = ((PathBuf, PathBuf), Snapshot, std::time::Instant);

static READONLY_SNAPSHOT_CACHE: std::sync::Mutex<Option<ReadonlySnapshotCacheEntry>> =
    std::sync::Mutex::new(None);

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

impl Snapshot {
    pub(super) fn capture_readonly(paths: &ClaudePaths) -> Self {
        let key = (paths.config.clone(), paths.projects.clone());
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
                    let registration: Registration = serde_json::from_slice(&fs::read(&path)?)?;
                    ensure!(
                        registration.pid > 1 && !registration.session_id.is_empty(),
                        "会话登记身份缺失"
                    );
                    Ok(registration)
                })();
                if let Ok(registration) = registration {
                    registrations.push(registration);
                }
            }
            Ok(())
        })();
        Self {
            processes: collect(),
            registrations,
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
        };
        let Some(processes) = &self.processes else {
            result.ownership = Ownership::Unknown;
            result.reason("process_list_unavailable");
            return result;
        };
        let target_id = |id: &str| id == parsed.id || children.contains(id);
        let domain = pid_domain();
        for registration in &self.registrations {
            if domain.as_deref() != Some(&registration.pid_domain)
                && (target_id(&registration.session_id)
                    || same_path(&registration.cwd, Path::new(&parsed.cwd)))
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
        for p in processes {
            let open_file = paths.iter().any(|path| p.files.contains(path));
            let cwd_matches = p
                .cwd
                .as_ref()
                .is_some_and(|cwd| same_path(cwd, Path::new(&parsed.cwd)));
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
            let registrations: Vec<_> = self
                .registrations
                .iter()
                .filter(|r| r.pid == p.pid && domain.as_deref() == Some(r.pid_domain.as_str()))
                .collect();
            let verified: Vec<_> = registrations
                .iter()
                .filter(|r| {
                    p.role == ProcessRole::Session
                        && started_ms(&r.proc_start).zip(p.started_ms).is_some_and(
                            |(registered, actual)| registered == actual && r.started_at >= actual,
                        )
                        && p.cwd.as_ref().is_none_or(|cwd| same_path(cwd, &r.cwd))
                })
                .collect();
            let owns = verified.len() == 1
                && verified.iter().any(|r| {
                    children.contains(&r.session_id)
                        || (r.session_id == parsed.id && same_path(&r.cwd, Path::new(&parsed.cwd)))
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
                || registrations.iter().any(|r| target_id(&r.session_id));
            if !relevant {
                continue;
            }
            let mut mismatched = BTreeSet::new();
            for r in &registrations {
                if started_ms(&r.proc_start) != p.started_ms || p.started_ms.is_none() {
                    mismatched.insert("procStart");
                }
                if p.started_ms.is_none_or(|start| r.started_at < start) {
                    mismatched.insert("startedAt");
                }
                if p.cwd.as_ref().is_some_and(|cwd| !same_path(cwd, &r.cwd)) {
                    mismatched.insert("cwd");
                }
            }
            if registrations.len() > 1 {
                mismatched.insert("sessionId");
            }
            let conflicting_cwd = verified
                .iter()
                .any(|r| r.session_id == parsed.id && !same_path(&r.cwd, Path::new(&parsed.cwd)));
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
            let conflicting_id =
                verified.len() == 1 && current_ids.iter().any(|id| *id != verified[0].session_id);
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
            if open_file || owns {
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
            if verified.len() == 1 {
                continue;
            }
            result.reason("registry_unverified");
            if verified.len() > 1 {
                result.reason("evidence_conflict");
            }
            if cwd_matches
                && explicit
                && p.role == ProcessRole::Session
                && registrations.is_empty()
                && p.started_ms
                    .zip(parsed.last_turn_at)
                    .is_some_and(|(start, turn)| start <= turn)
            {
                result.ownership = Ownership::Open;
                result.reason("resume_owner");
                continue;
            }
            if p.cwd.is_none() {
                result.reason("cwd_unknown");
            }
            if cwd_matches {
                result.reason("same_cwd_unregistered");
            }
            if explicit || registrations.iter().any(|r| target_id(&r.session_id)) {
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
fn native(command: &str, args: &[String]) -> bool {
    let name = Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    matches!(name, "claude" | "claude.exe")
        || (matches!(name, "node" | "bun")
            && args.iter().any(|a| {
                a.contains("/@anthropic-ai/claude-code/")
                    && (a.ends_with("cli.js") || a.ends_with("claude.exe"))
            }))
}

fn process_role(executable: &Path, args: &[String]) -> ProcessRole {
    let executable_name = executable
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let script = matches!(executable_name, "node" | "bun")
        .then(|| args.get(1))
        .flatten()
        .filter(|arg| arg.contains("/@anthropic-ai/claude-code/") && arg.ends_with("cli.js"));
    let installed = dirs::home_dir().is_some_and(|home| {
        executable.parent() == Some(home.join(".local/share/claude/versions").as_path())
            || home
                .join(".local/bin/claude")
                .canonicalize()
                .ok()
                .as_deref()
                == Some(executable)
    });
    if !installed && !matches!(executable_name, "claude" | "claude.exe") && script.is_none() {
        return ProcessRole::Unknown;
    }
    let Some(options) = args.get(if script.is_some() { 2.. } else { 1.. }) else {
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

#[cfg(target_os = "macos")]
fn process_identity(pid: u32) -> Option<(PathBuf, Vec<String>)> {
    let mut executable = [0_u8; 4096];
    // Kernel-provided executable and NUL-separated argv keep a PTY host's
    // argv[0] label distinct from arguments forwarded to its child.
    let count = unsafe {
        libc::proc_pidpath(
            pid.try_into().ok()?,
            executable.as_mut_ptr().cast(),
            executable.len() as u32,
        )
    };
    if count <= 0 {
        return None;
    }
    let end = executable.iter().position(|byte| *byte == 0)?;
    let executable = PathBuf::from(std::str::from_utf8(&executable[..end]).ok()?);
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid.try_into().ok()?];
    let mut size = 0;
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || !(4..=262_144).contains(&size)
    {
        return None;
    }
    let mut buffer = vec![0_u8; size];
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    buffer.truncate(size);
    let argc = i32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?);
    if !(1..=16_384).contains(&argc) {
        return None;
    }
    let data = buffer.get(4..)?;
    let mut offset = data.iter().position(|byte| *byte == 0)?;
    while data.get(offset) == Some(&0) {
        offset += 1;
    }
    let mut args = Vec::new();
    for _ in 0..argc {
        let remaining = data.get(offset..)?;
        let end = remaining.iter().position(|byte| *byte == 0)?;
        args.push(std::str::from_utf8(&remaining[..end]).ok()?.into());
        offset += end + 1;
    }
    Some((executable, args))
}
#[cfg(target_os = "linux")]
fn process_identity(pid: u32) -> Option<(PathBuf, Vec<String>)> {
    use std::io::Read;
    let root = PathBuf::from(format!("/proc/{pid}"));
    let mut bytes = Vec::new();
    fs::File::open(root.join("cmdline"))
        .ok()?
        .take(262_145)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 262_144 || !bytes.ends_with(&[0]) {
        return None;
    }
    let args = bytes[..bytes.len() - 1]
        .split(|byte| *byte == 0)
        .map(|arg| std::str::from_utf8(arg).map(str::to_owned))
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    Some((fs::read_link(root.join("exe")).ok()?, args))
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_identity(_pid: u32) -> Option<(PathBuf, Vec<String>)> {
    None
}

fn collect() -> Option<Vec<Process>> {
    // macOS truncates `comm` paths to 16 characters even with -ww. `ucomm`
    // comes from the native process name, so long CLI installation paths do
    // not hide a live Claude process or rely on a caller-controlled argv[0].
    #[cfg(target_os = "macos")]
    let format = "pid=,stat=,lstart=,ucomm=,args=";
    #[cfg(not(target_os = "macos"))]
    let format = "pid=,stat=,lstart=,comm=,args=";
    let output = Command::new("ps")
        .args(["-ww", "-axo", format])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut processes = Vec::new();
    for line in String::from_utf8(output.stdout).ok()?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 9 || fields[1].contains('Z') {
            continue;
        }
        let mut args = fields[8..]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        if !native(fields[7], &args) {
            continue;
        }
        let pid = fields[0].parse().ok()?;
        let role = if let Some((executable, native_args)) = process_identity(pid) {
            let role = process_role(&executable, &native_args);
            args = native_args;
            role
        } else {
            ProcessRole::Unknown
        };
        let started =
            chrono::NaiveDateTime::parse_from_str(&fields[2..7].join(" "), "%a %b %e %T %Y")
                .ok()
                .map(|d| d.and_utc().timestamp_millis());
        processes.push(Process {
            pid,
            started_ms: started,
            cwd: None,
            args,
            files: HashSet::new(),
            role,
        });
    }
    if processes.is_empty() {
        return Some(processes);
    }
    #[cfg(target_os = "linux")]
    for p in &mut processes {
        let root = PathBuf::from(format!("/proc/{}", p.pid));
        p.cwd = fs::read_link(root.join("cwd")).ok();
        if let Ok(files) = fs::read_dir(root.join("fd")) {
            p.files = files
                .filter_map(|e| fs::read_link(e.ok()?.path()).ok())
                .collect();
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let pids = processes
            .iter()
            .map(|p| p.pid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if let Ok(output) = Command::new("/usr/sbin/lsof")
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
                    if descriptor == "cwd" {
                        processes[index].cwd = Some(PathBuf::from(name));
                    } else if name.starts_with('/') {
                        processes[index].files.insert(PathBuf::from(name));
                    }
                }
            }
        }
    }
    Some(processes)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            role: ProcessRole::PtyHost,
        };
        let mut snapshot = Snapshot {
            processes: Some(vec![
                host.clone(),
                Process {
                    pid: 41,
                    role: ProcessRole::Daemon,
                    ..host.clone()
                },
            ]),
            registrations: vec![],
        };
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
            role: ProcessRole::Session,
        };
        let registration = Registration {
            pid: 42,
            session_id: "current".into(),
            cwd: parsed.cwd.clone().into(),
            proc_start: date.into(),
            started_at: started_ms(date).unwrap() + 500,
            pid_domain: pid_domain().unwrap(),
        };
        let snapshot = Snapshot {
            processes: Some(vec![process.clone()]),
            registrations: vec![registration.clone()],
        };
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
            let snapshot = Snapshot {
                processes: Some(vec![process.clone()]),
                registrations: vec![registration],
            };
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
            role: ProcessRole::Session,
        };
        let mut snapshot = Snapshot {
            processes: Some(vec![process]),
            registrations: vec![],
        };
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
            Snapshot {
                registrations: vec![],
                processes: Some(vec![])
            }
            .ownership(&p, file),
            Ownership::Inactive
        );
        let owner = Process {
            pid: 5,
            started_ms: Some(1000),
            cwd: Some(PathBuf::from(&p.cwd)),
            args: vec!["claude".into(), "--resume".into(), p.id.clone()],
            files: HashSet::new(),
            role: ProcessRole::Session,
        };
        assert_eq!(
            Snapshot {
                registrations: vec![],
                processes: Some(vec![owner.clone()])
            }
            .ownership(&p, file),
            Ownership::Open
        );
        assert_eq!(
            Snapshot {
                registrations: vec![],
                processes: Some(vec![Process {
                    started_ms: Some(3000),
                    ..owner.clone()
                }])
            }
            .ownership(&p, file),
            Ownership::Unknown
        );
        assert_eq!(
            Snapshot {
                registrations: vec![],
                processes: Some(vec![Process {
                    args: vec!["claude".into()],
                    ..owner
                }])
            }
            .ownership(&p, file),
            Ownership::Unknown
        );
        assert_eq!(
            Snapshot {
                registrations: vec![],
                processes: None
            }
            .ownership(&p, file),
            Ownership::Unknown
        );
        assert!(!native("zsh", &["claude".into()]));
    }
}
