//! Offline cleanup of the standard DeepSeek Harness file store.
pub mod notifications;
pub mod sessions;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

use crate::{archive::HiddenThreadItemResult, session_delete::Outcome};

const INDEX_LIMIT: u64 = 16 * 1024 * 1024;
const HEADER_LIMIT: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct DshPaths {
    pub home: PathBuf,
}
impl DshPaths {
    pub fn default_for_user() -> Self {
        Self {
            home: std::env::var_os("DSH_HOME")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".dsh")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshArchiveCandidate {
    pub id: String,
    pub archive_id: String,
    pub parent_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshArchiveDeletePlan {
    pub available: bool,
    pub archived_threads: usize,
    pub candidates: Vec<DshArchiveCandidate>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DshArchiveSelection {
    pub id: String,
    pub archive_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DshArchiveDeleteRequest {
    pub confirmed: bool,
    pub targets: Vec<DshArchiveSelection>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshArchiveDeleteResult {
    pub items: Vec<HiddenThreadItemResult>,
    pub deleted_threads: usize,
    pub deleted_bytes: u64,
}

struct Registry {
    path: PathBuf,
    bytes: Vec<u8>,
    value: Value,
    archived: BTreeSet<String>,
    stamp: FileStamp,
}
#[derive(Default)]
struct Session {
    directories: Vec<PathBuf>,
    stamps: Vec<(PathBuf, FileStamp)>,
    parent: Option<String>,
    reason: Option<String>,
}
struct Inventory {
    registry: Option<Registry>,
    sessions: BTreeMap<String, Session>,
    plan: DshArchiveDeletePlan,
}

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
fn file_stamp(path: &Path) -> Result<FileStamp> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(FileStamp {
        len: meta.len(),
        modified: meta.modified()?,
        #[cfg(unix)]
        identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
    })
}

fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "不支持的 DSH 线程身份"
    );
    Ok(())
}

fn directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "DSH 存储目录不可核验"
            );
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => anyhow::bail!("无法读取 DSH 存储目录"),
    }
}

fn regular_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).context("无法读取 DSH 文件")?;
    ensure!(file.metadata()?.is_file(), "DSH 文件不是普通文件");
    Ok(file)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    regular_file(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "DSH 索引或配置超出读取上限");
    Ok(bytes)
}

fn standard_layout(paths: &DshPaths) -> Result<()> {
    // User overlays are data, never executable YAML. A storage override requires
    // an explicit future adapter; do not guess another backend's ownership.
    let mut configs = vec![paths.home.join("cordis.patch.yml")];
    for legacy in ["config.yaml", "settings.yaml"] {
        ensure!(
            !paths.home.join(legacy).try_exists()?,
            "DSH 旧版自定义配置暂不支持归档清理"
        );
    }
    let profiles = paths.home.join("profiles");
    if directory(&profiles)? {
        for entry in fs::read_dir(&profiles)? {
            let entry = entry?;
            if entry.file_type()?.is_symlink() {
                anyhow::bail!("DSH profile 目录不可核验");
            }
            if entry.file_type()?.is_dir() {
                configs.push(entry.path().join("cordis.patch.yml"));
            }
        }
    }
    for path in configs {
        if !path.try_exists()? {
            continue;
        }
        let bytes = read_bounded(&path, HEADER_LIMIT)?;
        let text = std::str::from_utf8(&bytes).context("DSH 配置不是 UTF-8")?;
        ensure!(
            !text
                .lines()
                .map(str::trim)
                .filter(|line| !line.starts_with('#'))
                .any(|line| {
                    [
                        "session-persistence",
                        "storage-",
                        "session-query-sqlite",
                        "session-projection-cache",
                    ]
                    .iter()
                    .any(|key| line.contains(key))
                }),
            "DSH 自定义持久化后端暂不支持归档清理"
        );
    }
    Ok(())
}

fn registry(paths: &DshPaths) -> Result<Option<Registry>> {
    ensure!(paths.home.is_absolute(), "DSH_HOME 必须是绝对路径");
    if !directory(&paths.home)? {
        return Ok(None);
    }
    standard_layout(paths)?;
    let storages = paths.home.join("storages");
    if !directory(&storages)? {
        return Ok(None);
    }
    let path = storages.join("workspace.json");
    if !path.try_exists()? {
        return Ok(None);
    }
    let stamp = file_stamp(&path)?;
    let bytes = read_bounded(&path, INDEX_LIMIT)?;
    ensure!(stamp == file_stamp(&path)?, "DSH 归档索引正在变化，请重试");
    let value: Value = serde_json::from_slice(&bytes).context("DSH 归档索引损坏")?;
    ensure!(
        value.pointer("/unit/name").and_then(Value::as_str) == Some("workspace")
            && value.pointer("/unit/version").and_then(Value::as_u64) == Some(2),
        "不支持的 DSH 归档索引格式"
    );
    ensure!(
        value
            .pointer("/global/pendingMutation")
            .is_none_or(Value::is_null),
        "DSH 工作区变更未完成，请先在 DSH 中恢复后重试"
    );
    let ids = value
        .pointer("/global/archivedSessionIds")
        .and_then(Value::as_array)
        .context("DSH 归档名单损坏")?;
    let mut archived = BTreeSet::new();
    for value in ids {
        let id = value.as_str().context("DSH 归档线程身份损坏")?;
        valid_id(id)?;
        archived.insert(id.to_owned());
    }
    ensure!(
        value
            .pointer("/tables/workspaces")
            .is_some_and(Value::is_object),
        "DSH 工作区索引损坏"
    );
    let valid_references = |rows: &Value| {
        rows.as_array().is_some_and(|rows| {
            rows.iter()
                .all(|v| v.as_str().is_some_and(|id| valid_id(id).is_ok()))
        })
    };
    ensure!(
        value
            .pointer("/global/pinnedSessionIds")
            .is_none_or(valid_references),
        "DSH 置顶索引损坏"
    );
    for row in value["tables"]["workspaces"].as_object().unwrap().values() {
        ensure!(
            row.get("sessionIds").is_some_and(valid_references),
            "DSH 工作区线程索引损坏"
        );
    }
    Ok(Some(Registry {
        path,
        bytes,
        value,
        archived,
        stamp,
    }))
}

fn header(path: &Path, compressed: bool, id: &str) -> Result<Option<String>> {
    let file = regular_file(path)?;
    let reader: Box<dyn Read> = if compressed {
        Box::new(zstd::stream::read::Decoder::new(file)?)
    } else {
        Box::new(file)
    };
    let mut line = Vec::new();
    BufReader::new(reader.take(HEADER_LIMIT + 1)).read_until(b'\n', &mut line)?;
    ensure!(
        line.len() as u64 <= HEADER_LIMIT && line.ends_with(b"\n"),
        "DSH 会话头不完整或过大"
    );
    let value: Value = serde_json::from_slice(&line).context("DSH 会话头损坏")?;
    ensure!(
        value["type"] == "session"
            && value["id"] == id
            && value["version"]
                .as_u64()
                .is_some_and(|v| (1..=4).contains(&v)),
        "不支持或不匹配的 DSH 会话身份"
    );
    if value["origin"] == "subagent" {
        let parent = value["parentSession"]
            .as_str()
            .context("DSH 子线程缺少父线程身份")?;
        valid_id(parent)?;
        Ok(Some(parent.into()))
    } else {
        ensure!(value.get("origin").is_none(), "无法确认 DSH 会话来源");
        Ok(None)
    }
}

fn session(path: &Path, id: &str) -> Result<Option<String>> {
    let mut generations = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(version) = name
            .strip_prefix("session.v")
            .and_then(|n| n.split('.').next())
            .and_then(|v| v.parse::<u32>().ok())
        {
            if name == format!("session.v{version}.jsonl")
                || name == format!("session.v{version}.jsonl.zstd")
            {
                generations.push((version, name.ends_with(".zstd"), entry.path()));
            } else if name.ends_with(".jsonl") || name.ends_with(".zstd") {
                anyhow::bail!("不支持的 DSH 会话文件格式");
            }
        } else if name.ends_with(".jsonl") || name.ends_with(".zstd") {
            anyhow::bail!("不支持的 DSH 会话文件格式");
        }
    }
    generations.sort_by_key(|g| g.0);
    let Some((version, compressed, path)) = generations.pop() else {
        return Ok(None); // Header-only/partially removed directories still have exact ownership.
    };
    ensure!(version <= 4, "不支持的 DSH 会话格式");
    ensure!(
        !generations.iter().any(|g| g.0 == version),
        "DSH 会话存在冲突的存储格式"
    );
    header(&path, compressed, id)
}

fn inventory(paths: &DshPaths) -> Result<Inventory> {
    let registry = registry(paths)?;
    let mut sessions = BTreeMap::<String, Session>::new();
    let mut plan = DshArchiveDeletePlan {
        available: registry.is_some(),
        ..Default::default()
    };
    let Some(index) = &registry else {
        return Ok(Inventory {
            registry,
            sessions,
            plan,
        });
    };
    let root = paths.home.join("sessions");
    if directory(&root)? {
        for project in fs::read_dir(&root)? {
            let project = project?;
            if project.file_type()?.is_symlink() {
                anyhow::bail!("DSH 会话项目目录包含链接，无法核验归档范围");
            }
            if !project.file_type()?.is_dir() {
                continue;
            }
            for entry in fs::read_dir(project.path())? {
                let entry = entry?;
                let name = entry.file_name();
                let Some(id) = name.to_str().filter(|id| valid_id(id).is_ok()) else {
                    continue;
                };
                let ty = entry.file_type()?;
                if !ty.is_dir() && !ty.is_symlink() {
                    continue;
                }
                let found = sessions.entry(id.into()).or_default();
                found.directories.push(entry.path());
                found
                    .stamps
                    .push((entry.path(), file_stamp(&entry.path())?));
                if ty.is_symlink() {
                    continue;
                } // A selected link is unlinked, never followed.
                for file in fs::read_dir(entry.path())? {
                    let file = file?;
                    if file.file_name().to_string_lossy().starts_with("session.") {
                        found.stamps.push((file.path(), file_stamp(&file.path())?));
                    }
                }
                match session(&entry.path(), id) {
                    Ok(parent) => {
                        if found.directories.len() > 1 && found.parent != parent {
                            found.reason = Some("DSH 同一身份的父子关系冲突".into());
                        } else {
                            found.parent = parent;
                        }
                    }
                    Err(error) => {
                        found.reason = Some(error.to_string());
                        plan.issues
                            .push(format!("{id}：无法确认会话头或子线程归属"));
                    }
                }
            }
        }
    }
    plan.archived_threads = index.archived.len();
    let mut selected: BTreeMap<String, String> = index
        .archived
        .iter()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    loop {
        let next: Vec<_> = sessions
            .iter()
            .filter(|(id, s)| {
                !selected.contains_key(*id)
                    && s.reason.is_none()
                    && s.parent.as_ref().is_some_and(|p| selected.contains_key(p))
            })
            .map(|(id, s)| (id.clone(), selected[s.parent.as_ref().unwrap()].clone()))
            .collect();
        if next.is_empty() {
            break;
        }
        selected.extend(next);
    }
    plan.candidates = selected
        .into_iter()
        .map(|(id, archive_id)| {
            let s = sessions.get(&id);
            DshArchiveCandidate {
                archive_id,
                parent_id: s.and_then(|s| s.parent.clone()),
                reason: s.and_then(|s| s.reason.clone()),
                id,
            }
        })
        .collect();
    plan.issues.sort();
    plan.issues.dedup();
    Ok(Inventory {
        registry,
        sessions,
        plan,
    })
}

pub fn preview(paths: &DshPaths) -> Result<DshArchiveDeletePlan> {
    Ok(inventory(paths)?.plan)
}

pub(crate) fn preview_with_bytes(
    paths: &DshPaths,
) -> Result<(DshArchiveDeletePlan, BTreeMap<String, u64>)> {
    let inv = inventory(paths)?;
    let mut sizes = BTreeMap::new();
    let cache_dir = paths.home.join("storages/session_projcache/sessions");
    for candidate in &inv.plan.candidates {
        let mut target_paths = Vec::new();
        if let Some(session) = inv.sessions.get(&candidate.id) {
            target_paths.extend(session.directories.iter().cloned());
        }
        let cache_file = cache_dir.join(format!("{}.json", candidate.id));
        if cache_file.exists() {
            target_paths.push(cache_file);
        }
        let (_, bytes) = crate::session_delete::size(&target_paths);
        sizes.insert(candidate.id.clone(), bytes);
    }
    Ok((inv.plan, sizes))
}

fn dsh_command(command: &str) -> bool {
    let command = command.trim();
    let mut words = command.split_whitespace();
    let program = words
        .next()
        .and_then(|s| Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if matches!(
        program,
        "sh" | "bash" | "zsh" | "fish" | "python" | "python3"
    ) {
        return false;
    }
    if command.contains("DeepSeek Harness.app/Contents/") {
        return true;
    }
    matches!(program, "dsh" | "deepseek-harness" | "DeepSeekHarness")
        || (matches!(program, "node" | "bun")
            && words.any(|s| {
                s.contains("/@deepseek-ai/dsh/")
                    || s.contains("/@deepseek-ai/cli/")
                    || s.contains("/deepseek-harness/")
                    || s.contains("/dsh/node_modules/")
            }))
}

fn require_stopped() -> Result<()> {
    let output = Command::new("ps")
        .args(["-axo", "command="])
        .output()
        .context("无法核验 DSH 运行状态")?;
    ensure!(output.status.success(), "无法核验 DSH 运行状态");
    let output = std::str::from_utf8(&output.stdout).context("无法核验 DSH 运行状态")?;
    ensure!(
        !output.lines().any(dsh_command),
        "请先退出当前机器的 DSH，再清理归档线程"
    );
    Ok(())
}

fn unchanged(index: &Registry) -> Result<()> {
    let storages = index.path.parent().context("DSH 索引目录不可核验")?;
    ensure!(
        directory(storages)? && directory(storages.parent().context("DSH 数据目录不可核验")?)?,
        "DSH 数据目录已变化"
    );
    ensure!(
        file_stamp(&index.path)? == index.stamp
            && read_bounded(&index.path, INDEX_LIMIT)? == index.bytes,
        "DSH 归档索引已被修改，请重新预览"
    );
    Ok(())
}

fn remove_references(value: &mut Value, ids: &BTreeSet<String>) {
    fn filter(value: Option<&mut Value>, ids: &BTreeSet<String>) {
        if let Some(rows) = value.and_then(Value::as_array_mut) {
            rows.retain(|v| v.as_str().is_none_or(|id| !ids.contains(id)));
        }
    }
    filter(value.pointer_mut("/global/archivedSessionIds"), ids);
    filter(value.pointer_mut("/global/pinnedSessionIds"), ids);
    if let Some(workspaces) = value
        .pointer_mut("/tables/workspaces")
        .and_then(Value::as_object_mut)
    {
        for workspace in workspaces.values_mut() {
            filter(workspace.get_mut("sessionIds"), ids);
        }
    }
}

fn save_registry(
    index: &Registry,
    value: &Value,
    check_stopped: &impl Fn() -> Result<()>,
) -> Result<()> {
    if value == &index.value {
        return Ok(());
    }
    let tmp = index
        .path
        .with_file_name(format!(".agenthub-index-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.set_permissions(fs::metadata(&index.path)?.permissions())?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        check_stopped()?;
        unchanged(index)?;
        fs::rename(&tmp, &index.path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn remove_cache(paths: &DshPaths, id: &str, outcome: &mut Outcome) -> Result<bool> {
    let root = paths.home.join("storages/session_projcache");
    if !directory(&root)? {
        return Ok(true);
    }
    let rows = root.join("sessions");
    if !directory(&rows)? {
        return Ok(true);
    }
    let name = format!("{id}.json");
    let backup = format!("{name}.bak.");
    for entry in fs::read_dir(rows)? {
        let entry = entry?;
        let filename = entry.file_name();
        let filename = filename.to_string_lossy();
        if (filename == name
            || filename
                .strip_prefix(&backup)
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())))
            && !outcome.remove_strict(&entry.path())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn remove_dsh_cache_strict(paths: &DshPaths, id: &str, outcome: &mut Outcome) -> bool {
    let root = paths.home.join("storages/session_projcache");
    let rows = root.join("sessions");
    remove_cache(paths, id, outcome).unwrap_or_else(|err| {
        let dir = if root.is_dir() { rows } else { root };
        outcome.residuals.push(format!("{}: {err}", dir.display()));
        false
    })
}

fn remove_session(path: &Path, outcome: &mut Outcome) -> Result<bool> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error.into()),
    };
    if meta.file_type().is_symlink() {
        return Ok(outcome.remove_strict(path));
    }
    let mut headers = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("session.v") {
            headers.push(entry.path());
        } else if !outcome.remove_strict(&entry.path()) {
            return Ok(false);
        }
    }
    for header in headers {
        if !outcome.remove_strict(&header) {
            return Ok(false);
        }
    }
    let before = outcome.residuals.len();
    outcome.attempt(path, || Ok(fs::remove_dir(path)?));
    Ok(outcome.residuals.len() == before)
}

pub(super) fn remove_dsh_session_strict(path: &Path, outcome: &mut Outcome) -> bool {
    remove_session(path, outcome).unwrap_or_else(|err| {
        outcome.residuals.push(format!("{}: {err}", path.display()));
        false
    })
}

pub fn execute(
    paths: &DshPaths,
    request: DshArchiveDeleteRequest,
) -> Result<DshArchiveDeleteResult> {
    execute_checked(paths, request, require_stopped)
}

fn execute_checked(
    paths: &DshPaths,
    request: DshArchiveDeleteRequest,
    check_stopped: impl Fn() -> Result<()>,
) -> Result<DshArchiveDeleteResult> {
    ensure!(request.confirmed, "DSH 归档清理需要确认");
    let mut targets = BTreeMap::new();
    for target in request.targets {
        valid_id(&target.id)?;
        valid_id(&target.archive_id)?;
        if let Some(previous) = targets.insert(target.id, target.archive_id.clone()) {
            ensure!(
                previous == target.archive_id,
                "DSH 归档身份冲突，请重新预览"
            );
        }
    }
    check_stopped()?;
    let inventory = inventory(paths)?;
    let mut result = DshArchiveDeleteResult::default();
    let Some(index) = inventory.registry else {
        result.items = targets
            .into_keys()
            .map(|id| HiddenThreadItemResult {
                id,
                status: "skipped".into(),
                reason: Some("未发现 DSH 归档索引".into()),
                residuals: vec![],
            })
            .collect();
        return Ok(result);
    };
    let allowed: BTreeMap<_, _> = inventory
        .plan
        .candidates
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();
    let mut deleted = BTreeSet::new();
    let mut stopped = None;
    for (id, archive_id) in targets {
        let mut item = HiddenThreadItemResult {
            id: id.clone(),
            status: "skipped".into(),
            reason: None,
            residuals: vec![],
        };
        if stopped.is_none() {
            if let Err(error) = check_stopped().and_then(|_| unchanged(&index)) {
                stopped = Some(error.to_string());
            }
        }
        if let Some(reason) = &stopped {
            item.reason = Some(reason.clone());
        } else if let Some(candidate) = allowed
            .get(id.as_str())
            .filter(|c| c.archive_id == archive_id)
        {
            if let Some(reason) = &candidate.reason {
                item.status = "failed".into();
                item.reason = Some(reason.clone());
            } else {
                let mut outcome = Outcome::default();
                let session = inventory.sessions.get(&id);
                let changed = session.is_some_and(|session| {
                    session.stamps.iter().any(|(path, stamp)| {
                        file_stamp(path).is_ok_and(|current| &current != stamp)
                            || !crate::session_delete::within(&paths.home, path)
                    })
                });
                if changed {
                    stopped = Some("DSH 会话已被修改，请退出 DSH 后重新预览".into());
                    item.reason = stopped.clone();
                    result.items.push(item);
                    continue;
                }
                if let Err(error) = remove_cache(paths, &id, &mut outcome) {
                    outcome.residuals.push(format!("投影缓存：{error}"));
                }
                if outcome.residuals.is_empty() {
                    if let Some(session) = session {
                        for path in &session.directories {
                            if let Err(error) = remove_session(path, &mut outcome) {
                                outcome.residuals.push(format!("会话文件：{error}"));
                            }
                        }
                    }
                }
                outcome.finish();
                result.deleted_bytes += outcome.bytes;
                if outcome.residuals.is_empty() {
                    item.status = "deleted".into();
                    deleted.insert(id);
                } else {
                    item.status = "failed".into();
                    item.reason = Some("部分 DSH 线程数据未清理，请重试".into());
                    item.residuals = outcome.residuals;
                    stopped = item.reason.clone();
                }
            }
        } else {
            item.reason = Some("线程已恢复或不再属于本次归档范围".into());
        }
        result.items.push(item);
    }
    // Keep an archived root discoverable until every confirmed descendant is
    // removed. A descendant created after preview is never silently included.
    for root in &index.archived {
        if !deleted.contains(root) {
            continue;
        }
        let remaining = inventory.plan.candidates.iter().any(|candidate| {
            if deleted.contains(&candidate.id) {
                return false;
            }
            let mut parent = candidate.parent_id.as_ref();
            let mut seen = BTreeSet::new();
            while let Some(id) = parent {
                if id == root {
                    return true;
                }
                if !seen.insert(id) {
                    break;
                }
                parent = inventory.sessions.get(id).and_then(|s| s.parent.as_ref());
            }
            false
        });
        if remaining {
            deleted.remove(root);
            if let Some(item) = result.items.iter_mut().find(|i| &i.id == root) {
                item.status = "failed".into();
                item.reason = Some("仍有未确认或未清理的子线程，请重新预览后重试".into());
            }
        }
    }
    let mut value = index.value.clone();
    remove_references(&mut value, &deleted);
    if let Err(error) = save_registry(&index, &value, &check_stopped) {
        for item in result.items.iter_mut().filter(|i| deleted.contains(&i.id)) {
            item.status = "failed".into();
            item.reason = Some("DSH 归档索引未清理，请重新预览后重试".into());
            item.residuals.push(error.to_string());
        }
    }
    result.deleted_threads = result
        .items
        .iter()
        .filter(|i| i.status == "deleted")
        .count();
    Ok(result)
}

#[cfg(test)]
mod tests;
