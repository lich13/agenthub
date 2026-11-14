use super::*;
use std::{
    collections::{BTreeSet, HashSet},
    fs::OpenOptions,
    io::{Read, Write},
    sync::Mutex,
};
static MUTATIONS: Mutex<()> = Mutex::new(());

fn open_regular(path: &Path) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(file.metadata()?.is_file(), "Claude 专属内容不是普通文件");
    Ok(file)
}
fn checked_path(paths: &ClaudePaths, path: &Path) -> Result<()> {
    let root = paths.projects.canonicalize()?;
    checked_path_with_root(&root, path)
}
fn checked_path_with_root(root: &Path, path: &Path) -> Result<()> {
    ensure!(path.starts_with(root), "Claude 会话路径越界");
    let mut current = root.to_path_buf();
    for part in path.strip_prefix(root)?.components() {
        current.push(part);
        ensure!(
            !fs::symlink_metadata(&current)?.file_type().is_symlink(),
            "Claude 会话路径不能包含符号链接"
        );
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(metadata.is_file(), "Claude 会话不是普通文件");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.nlink() == 1,
            "Claude 会话存在硬链接，管理操作已禁用"
        );
    }
    Ok(())
}
pub(super) fn check_rename_path(paths: &ClaudePaths, path: &Path) -> Result<()> {
    let root = paths.projects.canonicalize()?;
    check_rename_path_with_root(&root, path)
}
pub(super) fn check_rename_path_with_root(root: &Path, path: &Path) -> Result<()> {
    checked_path_with_root(root, path)?;
    ensure!(
        !fs::metadata(path)?.permissions().readonly(),
        "Claude 会话只读，无法追加名称"
    );
    Ok(())
}
fn fingerprint(path: &Path) -> Result<String> {
    fingerprint_prefix(path, u64::MAX)
}
fn fingerprint_prefix(path: &Path, limit: u64) -> Result<String> {
    let before = reader::stamp(path)?;
    let file = open_regular(path)?;
    ensure!(
        file.metadata()?.len() <= 512 * 1024 * 1024,
        "Claude 会话超出管理大小上限"
    );
    let mut hash = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata()?;
        ensure!(m.nlink() == 1, "Claude 专属文件存在硬链接");
        hash.update(m.dev().to_le_bytes());
        hash.update(m.ino().to_le_bytes());
    }
    let mut file = file.take(limit.min(512 * 1024 * 1024 + 1));
    let mut buffer = [0; 65536];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= 512 * 1024 * 1024, "Claude 会话超出管理大小上限");
        hash.update(&buffer[..n]);
    }
    ensure!(
        before == reader::stamp(path)?,
        "Claude 会话正在变化，请重新预览"
    );
    Ok(hex::encode(hash.finalize()))
}
fn ensure_inactive(paths: &ClaudePaths, parsed: &Parsed, path: &Path) -> Result<()> {
    ensure!(
        parsed.issues.is_empty() && parsed.version.starts_with("2.") && !parsed.cwd.is_empty(),
        "Claude 会话格式或身份不明，管理操作已禁用"
    );
    if let Some(reason) = activity::Snapshot::capture(paths)
        .assess(parsed, path)
        .block_reason()
    {
        anyhow::bail!(reason);
    }
    Ok(())
}
pub(super) fn deletion_assessment(
    parsed: &Parsed,
    path: &Path,
    snapshot: &activity::Snapshot,
) -> Result<activity::Assessment> {
    let mut files = vec![path.to_path_buf()];
    for entry in walkdir::WalkDir::new(path.with_extension(""))
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    Ok(snapshot.assess_owned(parsed, &files, &HashSet::new()))
}

fn delete_paths(paths: &ClaudePaths, main: &Path, id: &str) -> (Vec<PathBuf>, Vec<String>) {
    let mut errors = Vec::new();
    let mut result = vec![main.into(), main.with_extension("")];
    for group in ["file-history", "tasks", "session-env"] {
        result.push(paths.config.join(group).join(id));
    }
    result.push(paths.config.join("debug").join(format!("{id}.txt")));
    let todos = paths.config.join("todos");
    match fs::read_dir(&todos) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        if entry.file_name().to_str().is_some_and(|name| {
                            name.starts_with(&format!("{id}-")) && name.ends_with(".json")
                        }) {
                            result.push(entry.path());
                        }
                    }
                    Err(error) => errors.push(format!("{}: {error}", todos.display())),
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => errors.push(format!("{}: {error}", todos.display())),
    }
    (result, errors)
}

pub fn preview_claude_delete(paths: &ClaudePaths, key: &str) -> Result<ClaudeDeletePreview> {
    let path = locate(paths, key)?;
    let id = path
        .file_stem()
        .context("会话文件名缺失")?
        .to_string_lossy()
        .into_owned();
    let (targets, warnings) = delete_paths(paths, &path, &id);
    let (file_count, bytes) = crate::session_delete::size(&targets);
    Ok(ClaudeDeletePreview {
        session_key: key.into(),
        title: id.clone(),
        id,
        path,
        paths: targets,
        fingerprint: String::new(),
        file_count,
        bytes,
        warnings: (!warnings.is_empty()).then_some(warnings),
    })
}

pub fn rename_claude_session(
    paths: &ClaudePaths,
    key: &str,
    title: &str,
) -> Result<ClaudeSessionSummary> {
    let _lock = MUTATIONS
        .lock()
        .map_err(|_| anyhow::anyhow!("Claude 管理锁不可用"))?;
    let title = title.trim();
    ensure!(
        !title.is_empty() && title.chars().count() <= 200 && !title.chars().any(char::is_control),
        "名称应为 1 至 200 字符"
    );
    let (path, parsed) = resolve(paths, key)?;
    check_rename_path(paths, &path)?;
    ensure_inactive(paths, &parsed, &path)?;
    let before = fingerprint(&path)?;
    let original_len = fs::metadata(&path)?.len();
    let mut options = OpenOptions::new();
    options.read(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&path)
        .context("Claude 改名无法写入，请核对会话目录权限和 systemd 写入白名单")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let fd = file.metadata()?;
        let current = fs::symlink_metadata(&path)?;
        ensure!(
            fd.dev() == current.dev() && fd.ino() == current.ino(),
            "Claude 文件身份已变化"
        );
    }
    ensure!(
        before == fingerprint(&path)?,
        "Claude 会话已变化，请刷新后重试"
    );
    let current = reader::read(&path)?;
    ensure!(
        current.id == parsed.id && current.cwd == parsed.cwd,
        "Claude 会话身份已变化"
    );
    ensure_inactive(paths, &current, &path)?;
    let bytes = serde_json::to_vec(
        &serde_json::json!({"type":"custom-title","customTitle":title,"sessionId":parsed.id}),
    )?;
    let mut record = bytes;
    record.push(b'\n');
    // One append, no rewrite/truncation of native history. On failure leave the
    // original plus any appended bytes for recovery; never roll back new writer data.
    file.write_all(&record)
        .with_context(|| format!("Claude 改名未完成，原文件保留在 {}", path.display()))?;
    file.sync_all()?;
    ensure!(
        fs::metadata(&path)?.len() == original_len + record.len() as u64
            && fingerprint_prefix(&path, original_len)? == before,
        "Claude 文件在改名期间发生变化，原文件保留在 {}",
        path.display()
    );
    let result = claude_session_summary(paths, key)?;
    ensure!(
        result.id == parsed.id && result.title == title && result.read_error.is_none(),
        "Claude 改名读回未通过，文件已保留在 {}",
        path.display()
    );
    Ok(result)
}
pub(crate) fn find_empty_inactive_sessions(
    paths: &ClaudePaths,
) -> Result<Vec<(ClaudeSessionSummary, String, u64)>> {
    if !paths.available() {
        return Ok(Vec::new());
    }
    let snapshot = activity::Snapshot::capture(paths);
    let mut out = Vec::new();
    for path in files(paths)? {
        if checked_path(paths, &path).is_err() {
            continue;
        }
        let Ok(parsed) = reader::read(&path) else {
            continue;
        };
        if !parsed.issues.is_empty()
            || parsed.turn_open
            || parsed.message_count != 0
            || !parsed.events.is_empty()
        {
            continue;
        }
        let Ok(assessment) = deletion_assessment(&parsed, &path, &snapshot) else {
            continue;
        };
        if assessment.ownership != activity::Ownership::Inactive {
            continue;
        }
        let Ok(fp) = fingerprint(&path) else {
            continue;
        };
        let Ok(sum) = summary(paths, &path, &parsed, &snapshot) else {
            continue;
        };
        let file_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| sum.id.clone());
        let (targets, warnings) = delete_paths(paths, &path, &file_id);
        if !warnings.is_empty() {
            continue;
        }
        let (_, bytes) = crate::session_delete::size(&targets);
        out.push((sum, fp, bytes));
    }
    Ok(out)
}

fn background_jobs(paths: &ClaudePaths) -> Result<HashMap<String, BTreeSet<String>>> {
    let jobs = paths.config.join("jobs");
    let entries = match fs::read_dir(&jobs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error).context("无法读取 Claude 后台会话登记"),
    };
    let mut matching = HashMap::<String, BTreeSet<String>>::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(native_id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        matching
            .entry(native_id.clone())
            .or_default()
            .insert(native_id.clone());
        if let Ok(row) = open_regular(&entry.path().join("state.json"))
            .and_then(|file| Ok(serde_json::from_reader::<_, Value>(file)?))
        {
            for id in ["id", "sessionId", "resumeSessionId"]
                .iter()
                .filter_map(|field| row.get(field).and_then(Value::as_str))
            {
                matching
                    .entry(id.to_owned())
                    .or_default()
                    .insert(native_id.clone());
            }
        }
    }
    Ok(matching)
}

const NATIVE_HELP_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
const NATIVE_RM_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
const NATIVE_HELP_BYTES: usize = 64 * 1024;

fn run_native_rm(
    executable: &Path,
    paths: &ClaudePaths,
    argument: &str,
    help: bool,
    timeout: std::time::Duration,
) -> Result<(std::process::ExitStatus, Vec<u8>)> {
    use std::{
        os::fd::AsRawFd,
        process::{Command, Stdio},
        time::Instant,
    };
    let mut child = Command::new(executable)
        .args(["rm", argument])
        .env("CLAUDE_CONFIG_DIR", &paths.config)
        .stdin(Stdio::null())
        .stdout(if help { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动 Claude 原生清理命令，历史文件尚未删除")?;
    let deadline = Instant::now() + timeout;
    let result = (|| -> Result<_> {
        let mut output = child.stdout.take();
        if let Some(stdout) = &output {
            let fd = stdout.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags == -1
                || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let mut bytes = Vec::new();
        let mut eof = output.is_none();
        loop {
            ensure!(
                Instant::now() < deadline,
                "原生命令超时（{} 秒），请检查后台会话后重试",
                timeout.as_secs()
            );
            let status = child.try_wait()?;
            if let Some(stdout) = output.as_mut().filter(|_| !eof) {
                let mut buffer = [0; 4096];
                loop {
                    ensure!(
                        Instant::now() < deadline,
                        "原生命令超时（{} 秒），请检查后台会话后重试",
                        timeout.as_secs()
                    );
                    match stdout.read(&mut buffer) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(count) => {
                            ensure!(bytes.len() + count <= NATIVE_HELP_BYTES, "原生帮助输出过大");
                            bytes.extend_from_slice(&buffer[..count]);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            if let Some(status) = status.filter(|_| eof) {
                return Ok((status, bytes));
            }
            std::thread::sleep(
                std::time::Duration::from_millis(10)
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    })();
    if result.is_err() {
        // Only the helper spawned above belongs to this operation. Never signal
        // the native session or its process group, and always reap this child.
        let _ = child.kill();
        child
            .wait()
            .context("无法回收 Claude 清理 helper，历史文件尚未删除")?;
    }
    result.map_err(|error| anyhow::anyhow!("Claude 原生清理失败，历史文件尚未删除：{error}"))
}

fn check_native_rm(executable: &Path, paths: &ClaudePaths) -> Result<()> {
    let (status, output) = run_native_rm(executable, paths, "--help", true, NATIVE_HELP_WAIT)?;
    ensure!(
        status.success()
            && String::from_utf8_lossy(&output)
                .lines()
                .any(|line| line.starts_with("Usage: claude rm <id>")),
        "当前 Claude CLI 不支持原生 rm 命令，历史文件尚未删除"
    );
    Ok(())
}

fn remove_background_job(executable: &Path, paths: &ClaudePaths, native_id: &str) -> Result<()> {
    ensure!(
        !native_id.starts_with('-'),
        "Claude 后台会话标识不能作为命令选项"
    );
    let (status, _) = run_native_rm(executable, paths, native_id, false, NATIVE_RM_WAIT)?;
    ensure!(status.success(), "Claude 原生清理未完成（{status}），请先使用 claude rm {native_id} 检查后台会话；历史文件尚未删除");
    Ok(())
}

#[derive(Default)]
struct NativeCleanup {
    executable: Option<PathBuf>,
    removed: BTreeSet<String>,
}

impl NativeCleanup {
    fn remove(&mut self, paths: &ClaudePaths, jobs: &[&String]) -> Result<()> {
        if jobs.is_empty() {
            return Ok(());
        }
        if self.executable.is_none() {
            let executable = env::var_os("PATH")
                .into_iter()
                .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
                .map(|path| path.join("claude"))
                .chain(dirs::home_dir().map(|home| home.join(".local/bin/claude")))
                .find(|path| path.is_file())
                .context(
                    "此会话有关联的 Claude 后台记录，但当前机器找不到 Claude CLI；历史文件尚未删除",
                )?;
            check_native_rm(&executable, paths)?;
            self.executable = Some(executable);
        }
        let executable = self.executable.as_ref().expect("verified native rm");
        for native_id in jobs {
            remove_background_job(executable, paths, native_id)?;
            self.removed.insert((*native_id).clone());
        }
        Ok(())
    }
}

pub fn execute_claude_delete(
    paths: &ClaudePaths,
    request: ClaudeDeleteRequest,
) -> Result<ClaudeDeleteResult> {
    execute_claude_delete_batch(paths, vec![request]).remove(0)
}

struct PendingDelete {
    request: ClaudeDeleteRequest,
    main: PathBuf,
    id: String,
    outcome: crate::session_delete::Outcome,
}

fn delete_exclusive(
    paths: &ClaudePaths,
    request: ClaudeDeleteRequest,
    jobs: &Result<HashMap<String, BTreeSet<String>>>,
    native: &mut NativeCleanup,
) -> Result<PendingDelete> {
    ensure!(request.confirmed, "删除需要明确确认");
    let main = locate(paths, &request.session_key)?;
    let id = main
        .file_stem()
        .context("会话文件名缺失")?
        .to_string_lossy()
        .into_owned();
    let projects = paths
        .projects
        .canonicalize()
        .unwrap_or_else(|_| paths.projects.clone());
    ensure!(
        crate::session_delete::within(&projects, &main),
        "Claude 会话路径越界"
    );
    // Unified cleanup supplies a fingerprint; ordinary deletion does not read
    // transcript contents or inspect/stop the native session's process.
    if !request.fingerprint.is_empty() {
        checked_path(paths, &main)?;
        ensure!(
            fingerprint(&main)? == request.fingerprint,
            "Claude 会话已变化，请重新预览"
        );
        let snapshot = activity::Snapshot::capture(paths);
        let parsed = read_or_warning(&main, false);
        let assessment = deletion_assessment(&parsed, &main, &snapshot)?;
        if assessment.ownership == activity::Ownership::Open {
            anyhow::bail!(assessment
                .block_reason()
                .unwrap_or_else(|| "Claude 会话正在运行，无法清理".into()));
        }
    }
    let jobs = jobs.as_ref().map_err(|e| anyhow::anyhow!("{e:#}"))?;
    let matching: Vec<_> = jobs
        .get(&id)
        .into_iter()
        .flatten()
        .filter(|native_id| !native.removed.contains(*native_id))
        .collect();
    native.remove(paths, &matching)?;

    let mut outcome = crate::session_delete::Outcome::default();
    let (targets, errors) = delete_paths(paths, &main, &id);
    if !errors.is_empty() {
        outcome.residuals.extend(errors);
        return Ok(PendingDelete {
            request,
            main,
            id,
            outcome,
        });
    }
    for (idx, path) in targets.iter().enumerate() {
        if crate::session_delete::within(&paths.config, path)
            || crate::session_delete::within(&projects, path)
        {
            if !outcome.remove_strict(path) {
                for rem in &targets[idx + 1..] {
                    if fs::symlink_metadata(rem).is_ok() {
                        outcome.residuals.push(rem.display().to_string());
                    }
                }
                break;
            }
        } else {
            outcome
                .residuals
                .push(format!("无法定位会话文件: {}", path.display()));
            for rem in &targets[idx + 1..] {
                if fs::symlink_metadata(rem).is_ok() {
                    outcome.residuals.push(rem.display().to_string());
                }
            }
            break;
        }
    }
    Ok(PendingDelete {
        request,
        main,
        id,
        outcome,
    })
}

pub(crate) fn execute_claude_delete_batch(
    paths: &ClaudePaths,
    requests: Vec<ClaudeDeleteRequest>,
) -> Vec<Result<ClaudeDeleteResult>> {
    let _lock = match MUTATIONS.lock() {
        Ok(lock) => lock,
        Err(_) => {
            return requests
                .into_iter()
                .map(|_| Err(anyhow::anyhow!("Claude 管理锁不可用")))
                .collect()
        }
    };
    let jobs = background_jobs(paths);
    let mut native = NativeCleanup::default();
    let mut pending: Vec<_> = requests
        .into_iter()
        .map(|request| delete_exclusive(paths, request, &jobs, &mut native))
        .collect();
    let mut indexes = std::collections::BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for item in pending
        .iter()
        .filter_map(|item| item.as_ref().ok())
        .filter(|item| item.outcome.residuals.is_empty())
    {
        indexes
            .entry(item.main.with_file_name("sessions-index.json"))
            .or_default()
            .insert(item.id.clone());
    }
    for (index, ids) in indexes {
        if let Err(error) = crate::session_delete::edit_json(&index, |value| {
            if let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) {
                entries.retain(|row| !crate::session_delete::has_id(row, &ids));
            }
        }) {
            for item in pending.iter_mut().filter_map(|item| item.as_mut().ok()) {
                if item.outcome.residuals.is_empty()
                    && item.main.with_file_name("sessions-index.json") == index
                {
                    item.outcome
                        .residuals
                        .push(format!("{}: {error}", index.display()));
                }
            }
        }
    }
    let ids: BTreeSet<_> = pending
        .iter()
        .filter_map(|item| item.as_ref().ok())
        .filter(|item| item.outcome.residuals.is_empty())
        .map(|item| item.id.clone())
        .collect();
    if !ids.is_empty() {
        let history = paths.config.join("history.jsonl");
        if let Err(error) = crate::session_delete::filter_jsonl(&history, |row| {
            crate::session_delete::has_id(row, &ids)
        }) {
            for item in pending
                .iter_mut()
                .filter_map(|item| item.as_mut().ok())
                .filter(|item| item.outcome.residuals.is_empty())
            {
                item.outcome
                    .residuals
                    .push(format!("{}: {error}", history.display()));
            }
        }
    }
    let mut eligible: Vec<_> = pending
        .iter_mut()
        .filter_map(|item| item.as_mut().ok())
        .filter(|item| item.outcome.residuals.is_empty())
        .collect();
    if !eligible.is_empty() {
        let registrations = paths.config.join("sessions");
        let scan = (|| -> Result<()> {
            let entries = match fs::read_dir(&registrations) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            ensure!(
                !fs::symlink_metadata(&registrations)?
                    .file_type()
                    .is_symlink(),
                "会话登记目录是链接"
            );
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if !path.extension().is_some_and(|ext| ext == "json") {
                    continue;
                }
                let Ok(row) = open_regular(&path)
                    .and_then(|file| Ok(serde_json::from_reader::<_, Value>(file)?))
                else {
                    continue;
                };
                for item in &mut eligible {
                    if item.outcome.residuals.is_empty()
                        && crate::session_delete::has_id(&row, &BTreeSet::from([item.id.clone()]))
                    {
                        item.outcome.remove_strict(&path);
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = scan {
            for item in eligible {
                if item.outcome.residuals.is_empty() {
                    item.outcome
                        .residuals
                        .push(format!("{}: {error}", registrations.display()));
                }
            }
        }
    }
    pending
        .into_iter()
        .map(|item| {
            item.map(|mut item| {
                item.outcome.finish();
                ClaudeDeleteResult {
                    session_key: item.request.session_key,
                    deleted: item.outcome.residuals.is_empty(),
                    bytes: item.outcome.bytes,
                    residuals: item.outcome.residuals,
                }
            })
        })
        .collect()
}

#[cfg(all(test, unix))]
mod native_delete_tests {
    use super::*;

    #[test]
    fn native_delete_entrypoints_reap_helpers_keep_history_and_release_management() {
        use std::os::unix::fs::PermissionsExt;
        const CASE: &str = "AGENTHUB_NATIVE_DELETE_CASE";
        let Ok(case) = env::var(CASE) else {
            for case in ["help-hang", "rm-hang", "overflow", "reject", "normal"] {
                let output = std::process::Command::new(env::current_exe().unwrap())
                    .args(["--exact", "claude::mutations::native_delete_tests::native_delete_entrypoints_reap_helpers_keep_history_and_release_management", "--nocapture"])
                    .env(CASE, case).output().unwrap();
                assert!(
                    output.status.success(),
                    "{case}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            return;
        };
        let root = env::temp_dir().join(format!("agenthub-native-entry-{}", uuid::Uuid::new_v4()));
        let paths = ClaudePaths {
            config: root.clone(),
            projects: root.join("projects"),
        };
        fs::create_dir_all(paths.projects.join("fixture")).unwrap();
        let bin = root.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = r#"#!/bin/sh
printf '%s\n' "$$" >> "$CLAUDE_CONFIG_DIR/pids"
case "$2" in
  --help)
    printf 'help\n' >> "$CLAUDE_CONFIG_DIR/calls"
    case "$AGENTHUB_NATIVE_DELETE_CASE" in
      help-hang) exec /bin/sleep 120 ;;
      overflow) while :; do printf '%04096d' 0; done ;;
    esac
    printf 'Usage: claude rm <id>\n'
    ;;
  *)
    printf 'rm:%s\n' "$2" >> "$CLAUDE_CONFIG_DIR/calls"
    if [ "$2" = job-1 ]; then
      case "$AGENTHUB_NATIVE_DELETE_CASE" in
        rm-hang) exec /bin/sleep 120 ;;
        reject) exit 6 ;;
      esac
    fi
    ;;
esac
"#;
        let cli = bin.join("claude");
        fs::write(&cli, script).unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        // This test process is isolated, so PATH changes cannot affect other tests.
        env::set_var(
            "PATH",
            env::join_paths(
                std::iter::once(bin).chain(env::split_paths(&env::var_os("PATH").unwrap())),
            )
            .unwrap(),
        );
        let mut requests = Vec::new();
        let mut history = Vec::new();
        for i in 1..=3 {
            let id = format!("00000000-0000-4000-8000-{i:012}");
            let path = paths.projects.join("fixture").join(format!("{id}.jsonl"));
            fs::write(&path, "").unwrap();
            let key = super::super::key_for(
                &paths.projects.canonicalize().unwrap(),
                &path.canonicalize().unwrap(),
                &id,
            );
            requests.push(ClaudeDeleteRequest {
                session_key: key,
                confirmed: true,
                fingerprint: String::new(),
            });
            history.push(path);
            if i <= 2 {
                let job = root.join("jobs").join(format!("job-{i}"));
                fs::create_dir_all(&job).unwrap();
                fs::write(
                    job.join("state.json"),
                    serde_json::json!({"sessionId":id}).to_string(),
                )
                .unwrap();
            }
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let batch_paths = paths.clone();
        let ordinary = requests.pop().unwrap();
        let next_batch = requests[0].clone();
        if case == "normal" || case == "reject" {
            let results = runtime
                .block_on(crate::read_tasks::spawn_blocking(move || {
                    execute_claude_delete_batch(&batch_paths, requests)
                }))
                .unwrap();
            assert_eq!(results[0].is_ok(), case == "normal");
            assert!(results[1].as_ref().unwrap().deleted);
            assert_eq!(history[0].exists(), case != "normal");
            assert!(!history[1].exists());
            assert_eq!(
                fs::read_to_string(root.join("calls")).unwrap(),
                "help\nrm:job-1\nrm:job-2\n"
            );
        } else {
            // The right-click single-delete entry uses the same helper and lock.
            let request = requests.remove(0);
            let result = runtime
                .block_on(crate::read_tasks::spawn_blocking(move || {
                    execute_claude_delete(&batch_paths, request)
                }))
                .unwrap();
            let message = result.unwrap_err().to_string();
            assert!(message.contains(if case == "overflow" {
                "输出过大"
            } else {
                "超时"
            }));
            assert!(history[0].exists());
            assert!(history[1].exists());
        }
        let calls_before_ordinary = fs::read_to_string(root.join("calls")).unwrap();
        // A later real management request gets both the read permit and mutation lock.
        let later_paths = paths.clone();
        let result = runtime.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                crate::read_tasks::spawn_blocking(move || {
                    execute_claude_delete(&later_paths, ordinary)
                }),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        });
        assert!(result.deleted);
        assert!(!history[2].exists());
        assert_eq!(
            fs::read_to_string(root.join("calls")).unwrap(),
            calls_before_ordinary
        );
        if case == "normal" {
            fs::write(&history[0], "").unwrap();
            assert!(execute_claude_delete(&paths, next_batch).unwrap().deleted);
            assert_eq!(
                fs::read_to_string(root.join("calls")).unwrap(),
                "help\nrm:job-1\nrm:job-2\nhelp\nrm:job-1\n"
            );
        }
        for pid in fs::read_to_string(root.join("pids")).unwrap().lines() {
            let pid: i32 = pid.parse().unwrap();
            assert_eq!(
                unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
                -1,
                "helper already reaped"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ECHILD)
            );
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "helper no longer running"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
        assert!(
            root.join("jobs/job-1/state.json").exists(),
            "AgentHub never deletes native job storage itself"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn background_mapping_uses_native_and_resumed_identity_and_native_rm_owns_its_files() {
        let root = env::temp_dir().join(format!("agenthub-native-rm-{}", uuid::Uuid::new_v4()));
        let paths = ClaudePaths {
            config: root.clone(),
            projects: root.join("projects"),
        };
        for (name, row) in [
            (
                "original",
                serde_json::json!({"sessionId":"selected","resumeSessionId":"resumed"}),
            ),
            ("other", serde_json::json!({"sessionId":"unrelated"})),
        ] {
            fs::create_dir_all(root.join("jobs").join(name)).unwrap();
            fs::write(
                root.join("jobs").join(name).join("state.json"),
                row.to_string(),
            )
            .unwrap();
        }
        let jobs = background_jobs(&paths).unwrap();
        for id in ["selected", "resumed", "original"] {
            assert_eq!(jobs[id], BTreeSet::from(["original".to_owned()]));
        }
        assert!(!jobs.contains_key("absent"));
        let cli = root.join("claude");
        crate::write_executable_fixture(&cli, "#!/bin/sh\n[ \"$#\" = 2 ] && [ \"$1\" = rm ] && [ \"$2\" = original ] || exit 4\n[ -f \"$CLAUDE_CONFIG_DIR/jobs/original/state.json\" ] || exit 5\n[ ! -f \"$CLAUDE_CONFIG_DIR/reject\" ] || exit 6\nprintf '%s\\n' \"$2\" > \"$CLAUDE_CONFIG_DIR/removed\"\n");
        // Unsupported rm must be rejected before any deletion is attempted.
        assert!(check_native_rm(&cli, &paths).is_err());
        assert!(remove_background_job(&cli, &paths, "--force").is_err());
        let supported = root.join("supported-claude");
        crate::write_executable_fixture(&supported, "#!/bin/sh\n[ \"$1\" = rm ] && [ \"$2\" = --help ] || exit 4\nprintf 'Usage: claude rm <id>\\n'\n");
        check_native_rm(&supported, &paths).unwrap();
        fs::write(root.join("reject"), b"").unwrap();
        assert!(remove_background_job(&cli, &paths, "original").is_err());
        assert!(!root.join("removed").exists());
        fs::remove_file(root.join("reject")).unwrap();
        remove_background_job(&cli, &paths, "original").unwrap();
        assert_eq!(
            fs::read_to_string(root.join("removed")).unwrap(),
            "original\n"
        );
        // AgentHub leaves job storage to Claude; it never removes it itself.
        assert!(root.join("jobs/original/state.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
