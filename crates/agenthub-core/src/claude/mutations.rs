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

pub fn execute_claude_delete(
    paths: &ClaudePaths,
    request: ClaudeDeleteRequest,
) -> Result<ClaudeDeleteResult> {
    let _lock = MUTATIONS
        .lock()
        .map_err(|_| anyhow::anyhow!("Claude 管理锁不可用"))?;
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
    let snapshot = activity::Snapshot::capture(paths);
    match fs::symlink_metadata(&main) {
        Ok(_) => {
            checked_path(paths, &main)?;
            if !request.fingerprint.is_empty() {
                ensure!(
                    fingerprint(&main)? == request.fingerprint,
                    "Claude 会话已变化，请重新预览"
                );
            }
            let parsed = read_or_warning(&main, false);
            let assessment = deletion_assessment(&parsed, &main, &snapshot)?;
            if assessment.ownership == activity::Ownership::Open {
                anyhow::bail!(
                    "{}",
                    assessment
                        .block_reason()
                        .unwrap_or_else(|| "Claude 会话正在运行，无法删除".into())
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                request.fingerprint.is_empty(),
                "Claude 会话文件已不存在，请重新预览"
            );
            let fallback = Parsed {
                id: id.clone(),
                ..Default::default()
            };
            let assessment = deletion_assessment(&fallback, &main, &snapshot)?;
            if assessment.ownership == activity::Ownership::Open {
                anyhow::bail!(
                    "{}",
                    assessment
                        .block_reason()
                        .unwrap_or_else(|| "Claude 会话正在运行，无法删除".into())
                );
            }
        }
        Err(error) => return Err(error.into()),
    }

    let ids = BTreeSet::from([id.clone()]);
    let mut outcome = crate::session_delete::Outcome::default();
    let (targets, errors) = delete_paths(paths, &main, &id);
    if !errors.is_empty() {
        outcome.residuals.extend(errors);
        outcome.finish();
        return Ok(ClaudeDeleteResult {
            session_key: request.session_key,
            deleted: false,
            bytes: 0,
            residuals: outcome.residuals,
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
    if outcome.residuals.is_empty() {
        let index = main
            .parent()
            .context("会话项目目录缺失")?
            .join("sessions-index.json");
        outcome.attempt(&index, || {
            crate::session_delete::edit_json(&index, |value| {
                if let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) {
                    entries.retain(|row| !crate::session_delete::has_id(row, &ids));
                }
            })
        });
    }
    if outcome.residuals.is_empty() {
        let history = paths.config.join("history.jsonl");
        outcome.attempt(&history, || {
            crate::session_delete::filter_jsonl(&history, |row| {
                crate::session_delete::has_id(row, &ids)
            })
        });
    }
    if outcome.residuals.is_empty() {
        let registrations = paths.config.join("sessions");
        match fs::read_dir(&registrations).and_then(|entries| {
            if fs::symlink_metadata(&registrations)?
                .file_type()
                .is_symlink()
            {
                Err(std::io::Error::other("会话登记目录是链接"))
            } else {
                Ok(entries)
            }
        }) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) if entry.path().extension().is_some_and(|ext| ext == "json") => {
                            let path = entry.path();
                            match open_regular(&path)
                                .and_then(|file| Ok(serde_json::from_reader::<_, Value>(file)?))
                            {
                                Ok(row) if crate::session_delete::has_id(&row, &ids) => {
                                    if !outcome.remove_strict(&path) {
                                        break;
                                    }
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    outcome
                                        .residuals
                                        .push(format!("{}: {error}", path.display()));
                                    break;
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            outcome
                                .residuals
                                .push(format!("{}: {error}", registrations.display()));
                            break;
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => outcome
                .residuals
                .push(format!("{}: {error}", registrations.display())),
        }
    }
    outcome.finish();
    Ok(ClaudeDeleteResult {
        session_key: request.session_key,
        deleted: outcome.residuals.is_empty(),
        bytes: outcome.bytes,
        residuals: outcome.residuals,
    })
}
