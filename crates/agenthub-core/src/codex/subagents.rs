//! Parent-bound, read-only access to native Codex child sessions.
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use super::{CodexPaths, ThreadDetail, ThreadStatus, ThreadSummary};

const MAX_DEPTH: usize = 32;
const MAX_RECORDS: usize = 20_000;

mod lifecycle;
pub(super) use lifecycle::{merge_creation_events, native_activity_block};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Creating,
    Running,
    Completed,
    Failed,
    Interrupted,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentEventKind {
    Started,
    Completed,
    Interrupted,
    Interacted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentCounts {
    pub creating: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub interrupted: usize,
    pub unknown: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentCollection {
    pub agents: Vec<SubagentActivity>,
    pub counts: SubagentCounts,
    pub complete: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentActivity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<SubagentEventKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub agent_id: Option<String>,
    pub name: String,
    pub role: Option<String>,
    pub status: SubagentStatus,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubagentDetailRequest {
    pub root_thread_id: String,
    pub agent_id: String,
    pub limit: Option<usize>,
    pub before: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentDetailResponse {
    pub root_thread_id: String,
    pub parent_thread_id: String,
    pub agent: SubagentActivity,
    pub detail: ThreadDetail,
}

#[derive(Clone)]
struct AgentRecord {
    id: String,
    parents: HashSet<String>,
    path: Option<String>,
    title: String,
    nickname: Option<String>,
    role: Option<String>,
    rollout: PathBuf,
    cwd: Option<String>,
    updated_at: Option<String>,
}

fn field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn spawn_source(value: &Value) -> &Value {
    value.pointer("/subagent/thread_spawn").unwrap_or(value)
}

fn columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    Ok(conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<_>>()?)
}

fn read_graph(paths: &CodexPaths) -> Result<HashMap<String, AgentRecord>> {
    let conn = Connection::open_with_flags(paths.state_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("子智能体索引暂时不可用")?;
    let cols = columns(&conn, "threads")?;
    ensure!(
        cols.contains("id") && cols.contains("rollout_path"),
        "子智能体索引格式不支持"
    );
    let select = |names: &[&str]| {
        names
            .iter()
            .find(|name| cols.contains(**name))
            .map(|s| s.to_string())
            .unwrap_or_else(|| "NULL".into())
    };
    let sql = format!(
        "SELECT id, rollout_path, {}, {}, {}, {}, {}, {}, {}, {} FROM threads LIMIT {}",
        select(&["source"]),
        select(&["parent_thread_id", "parentThreadId"]),
        select(&["agent_path", "agentPath"]),
        select(&["title", "name"]),
        select(&["agent_nickname", "agentNickname"]),
        select(&["agent_role", "agentRole"]),
        select(&["cwd"]),
        select(&["updated_at", "created_at"]),
        MAX_RECORDS + 1
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |row| {
            let source: Option<String> = row.get(2)?;
            let source: Value = source
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null);
            let source = spawn_source(&source);
            let mut parents = HashSet::new();
            if let Some(id) = row.get::<_, Option<String>>(3)?.filter(|s| !s.is_empty()) {
                parents.insert(id);
            }
            if let Some(id) = field(source, &["parent_thread_id", "parentThreadId"]) {
                parents.insert(id);
            }
            Ok(AgentRecord {
                id: row.get(0)?,
                rollout: row.get::<_, Option<String>>(1)?.unwrap_or_default().into(),
                parents,
                path: row
                    .get::<_, Option<String>>(4)?
                    .or_else(|| field(source, &["agent_path", "agentPath"])),
                title: row
                    .get::<_, Option<String>>(5)?
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_default(),
                nickname: row
                    .get::<_, Option<String>>(6)?
                    .or_else(|| field(source, &["agent_nickname"])),
                role: row
                    .get::<_, Option<String>>(7)?
                    .or_else(|| field(source, &["agent_role"])),
                cwd: row.get(8)?,
                updated_at: row.get::<_, Option<String>>(9).unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        rows.len() <= MAX_RECORDS,
        "子智能体索引过大，暂时无法验证关联"
    );
    let mut graph: HashMap<_, _> = rows.into_iter().map(|r| (r.id.clone(), r)).collect();
    let edge_cols = columns(&conn, "thread_spawn_edges")?;
    if edge_cols.contains("parent_thread_id") && edge_cols.contains("child_thread_id") {
        let mut stmt =
            conn.prepare("SELECT parent_thread_id, child_thread_id FROM thread_spawn_edges")?;
        let mut rows = stmt.query([])?;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            count += 1;
            ensure!(count <= MAX_RECORDS, "子智能体关联过多");
            let parent: String = row.get(0)?;
            if let Some(child) = graph.get_mut(&row.get::<_, String>(1)?) {
                child.parents.insert(parent);
            }
        }
    }
    Ok(graph)
}

fn validate_key(key: &str) -> Result<()> {
    ensure!(
        !key.is_empty()
            && key.len() <= 256
            && !key.chars().any(|c| c.is_control() || c == '/' || c == '\\'),
        "无效的线程身份"
    );
    Ok(())
}

fn validate_rollout(paths: &CodexPaths, record: &AgentRecord) -> Result<PathBuf> {
    let home = fs::canonicalize(&paths.home).context("会话根目录不可用")?;
    let path = fs::canonicalize(&record.rollout).context("子智能体记录已清理或不存在")?;
    ensure!(
        path.starts_with(home.join("sessions")) || path.starts_with(home.join("archived_sessions")),
        "子智能体记录不在会话目录内"
    );
    let mut component = record.rollout.as_path();
    while component != paths.home && component != home {
        ensure!(
            !fs::symlink_metadata(component)?.file_type().is_symlink(),
            "子智能体记录包含符号链接"
        );
        component = component.parent().context("子智能体路径无法验证")?;
    }
    ensure!(path.is_file(), "子智能体记录不是文件");
    let file = File::open(&path)?;
    let mut reader = BufReader::new(file.take(512 * 1024));
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            let meta = if value.get("type").and_then(Value::as_str) == Some("session_meta") {
                value.get("payload")
            } else {
                value.pointer("/session_meta/payload")
            };
            if let Some(meta) = meta {
                ensure!(
                    field(meta, &["id", "session_id"]).as_deref() == Some(&record.id),
                    "子智能体文件身份已变化"
                );
                if let Some(parent) = meta
                    .get("source")
                    .map(spawn_source)
                    .and_then(|s| field(s, &["parent_thread_id", "parentThreadId"]))
                    .or_else(|| field(meta, &["parent_thread_id", "parentThreadId"]))
                {
                    ensure!(
                        record.parents.len() == 1 && record.parents.contains(&parent),
                        "子智能体父线程身份冲突"
                    );
                }
                return Ok(path);
            }
        }
        line.clear();
    }
    bail!("子智能体文件缺少可验证身份")
}

fn validate_relation(
    paths: &CodexPaths,
    graph: &HashMap<String, AgentRecord>,
    root: &str,
    agent: &str,
) -> Result<()> {
    validate_key(root)?;
    validate_key(agent)?;
    ensure!(root != agent, "请选择子智能体");
    ensure!(
        super::local_thread_summary(paths, root)?.is_some(),
        "主线程已变化或不可读取"
    );
    let mut visited = HashSet::new();
    let mut id = agent;
    for _ in 0..MAX_DEPTH {
        if id == root {
            return Ok(());
        }
        ensure!(visited.insert(id), "子智能体关联存在循环");
        let record = graph.get(id).context("子智能体关联记录不存在")?;
        ensure!(record.parents.len() == 1, "子智能体父线程关联不明确");
        validate_rollout(paths, record)?;
        id = record.parents.iter().next().expect("one parent");
    }
    bail!("子智能体嵌套层级过深")
}

struct NativeState {
    status: SubagentStatus,
    turn: Option<String>,
    pending: HashSet<String>,
    invalid: bool,
}
impl Default for NativeState {
    fn default() -> Self {
        Self {
            status: SubagentStatus::Unknown,
            turn: None,
            pending: HashSet::new(),
            invalid: false,
        }
    }
}
fn native_status(path: &Path) -> Result<SubagentStatus> {
    static CACHE: crate::jsonl_cache::JsonlCache<NativeState> =
        crate::jsonl_cache::JsonlCache::new(crate::resource_limits::SUBAGENT_CACHE_BYTES);
    CACHE.read(
        path,
        |_| Ok(0),
        false,
        |state, line, _, _| {
            if state.invalid || line.iter().all(u8::is_ascii_whitespace) {
                return Ok(());
            }
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                state.invalid = true;
                return Ok(());
            };
            let mut status = state.status;
            let mut turn = state.turn.take();
            let pending = &mut state.pending;
            let event = value.get("payload").unwrap_or(&value);
            let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
            let event_turn = field(event, &["turn_id", "turnId"]);
            match kind {
                "task_started" | "turn_started" | "turn/started" => {
                    turn = event_turn;
                    pending.clear();
                    status = SubagentStatus::Running;
                }
                "task_complete" | "turn_completed" | "turn/completed" | "turn_aborted"
                | "turn/aborted" | "turn_error" | "turn_failed"
                    if event_turn.is_none() || turn.is_none() || event_turn == turn =>
                {
                    let terminal = field(event, &["status", "turn_status"]).unwrap_or_default();
                    status = if kind.contains("aborted")
                        || matches!(terminal.as_str(), "cancelled" | "canceled" | "interrupted")
                    {
                        SubagentStatus::Interrupted
                    } else if matches!(kind, "turn_error" | "turn_failed")
                        || matches!(terminal.as_str(), "failed" | "error")
                    {
                        SubagentStatus::Failed
                    } else if pending.is_empty() {
                        SubagentStatus::Completed
                    } else {
                        SubagentStatus::Unknown
                    };
                    pending.clear();
                }
                "function_call" | "custom_tool_call" => {
                    if let Some(call) = field(event, &["call_id", "callId"]) {
                        pending.insert(call);
                    }
                }
                "function_call_output" | "custom_tool_call_output" => {
                    if let Some(call) = field(event, &["call_id", "callId"]) {
                        pending.remove(&call);
                    }
                }
                "message"
                    if event.get("role").and_then(Value::as_str) == Some("user")
                        && status != SubagentStatus::Running =>
                {
                    status = SubagentStatus::Unknown;
                }
                _ => {}
            }

            state.status = status;
            state.turn = turn;
            Ok(())
        },
        |state, info| {
            Ok(if info.partial || state.invalid {
                SubagentStatus::Unknown
            } else {
                state.status
            })
        },
        |state| 256 + state.pending.len() as u64 * 128,
    )
}

fn activity(
    record: &AgentRecord,
    status: SubagentStatus,
    delegation: Option<String>,
) -> SubagentActivity {
    SubagentActivity {
        event_kind: None,
        event_id: None,
        agent_id: Some(record.id.clone()),
        name: display_name(None, record),
        role: record.role.clone(),
        status,
        available: true,
        unavailable_reason: None,
        delegation,
    }
}

fn tool_leaf(name: &str) -> &str {
    name.rsplit(['.', ':', '/']).next().unwrap_or(name)
}

// Capture structured delegation before the generic tool preview truncates JSON.
pub(super) fn creation_activity(name: &str, payload: &Value) -> Option<SubagentActivity> {
    if tool_leaf(name) != "spawn_agent" {
        return None;
    }
    let raw = payload
        .get("arguments")
        .or_else(|| payload.get("input"))
        .or_else(|| payload.get("params"))?;
    let parsed;
    let input = if let Some(text) = raw.as_str() {
        parsed = serde_json::from_str::<Value>(text).ok()?;
        &parsed
    } else {
        raw
    };
    Some(SubagentActivity {
        event_kind: None,
        event_id: None,
        agent_id: None,
        name: field(input, &["task_name", "name"])
            .map(|name| humanize_task(&name))
            .unwrap_or_else(|| "子智能体".into()),
        role: field(input, &["agent_type"]),
        status: SubagentStatus::Creating,
        available: false,
        unavailable_reason: Some("子智能体关联尚未确认或记录已清理".into()),
        delegation: field(input, &["message", "prompt"]),
    })
}

fn humanize_task(task: &str) -> String {
    let name = task.trim().replace('_', " ");
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn display_name(task: Option<&str>, record: &AgentRecord) -> String {
    if let Some(task) = task.filter(|name| !name.trim().is_empty() && *name != "子智能体") {
        return humanize_task(task);
    }
    if let Some(path) = record
        .path
        .as_deref()
        .and_then(|path| path.rsplit('/').find(|s| !s.is_empty()))
    {
        return humanize_task(path);
    }
    if !record.title.trim().is_empty() {
        return record.title.trim().to_owned();
    }
    record
        .nickname
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "子智能体".into())
}

fn record_activity(
    paths: &CodexPaths,
    record: &AgentRecord,
    task: Option<&str>,
) -> SubagentActivity {
    let mut agent = activity(record, SubagentStatus::Unknown, None);
    agent.name = display_name(task, record);
    let verified = (|| -> Result<SubagentStatus> {
        ensure!(record.parents.len() == 1, "子智能体父线程关联不明确");
        let path = validate_rollout(paths, record)?;
        let before = fs::metadata(&path)?;
        let status = native_status(&path)?;
        let after = fs::metadata(validate_rollout(paths, record)?)?;
        ensure!(
            before.len() == after.len() && before.modified()? == after.modified()?,
            "子智能体正在更新"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                (
                    before.dev(),
                    before.ino(),
                    before.ctime(),
                    before.ctime_nsec()
                ) == (after.dev(), after.ino(), after.ctime(), after.ctime_nsec()),
                "子智能体身份已变化"
            );
        }
        Ok(status)
    })();
    match verified {
        Ok(status) => agent.status = status,
        Err(_) => {
            agent.available = false;
            agent.unavailable_reason = Some("子智能体记录缺失、正在更新或身份无法验证".into());
        }
    }
    agent
}

/// Recompute direct children independently of the parent's message window or stamp.
/// Lifecycle fields belong to an event; status belongs to the current child turn.
pub fn enrich_subagent_blocks(paths: &CodexPaths, detail: &mut ThreadDetail) {
    let visible: HashSet<_> = detail.blocks.iter().map(|block| block.id.clone()).collect();
    detail.blocks.extend(
        std::mem::take(&mut detail.history_subagent_blocks)
            .into_iter()
            .filter(|block| !visible.contains(&block.id)),
    );
    let graph = read_graph(paths);
    let mut collection = SubagentCollection {
        agents: Vec::new(),
        counts: SubagentCounts::default(),
        complete: graph.is_ok(),
        warning: None,
    };
    let names = lifecycle::CreationNames::from_blocks(&detail.blocks);
    let mut current = HashMap::new();
    if let Ok(graph) = &graph {
        let mut records: Vec<_> = graph
            .values()
            .filter(|r| r.parents.contains(&detail.summary.id))
            .collect();
        records.sort_by(|a, b| a.id.cmp(&b.id));
        for record in records {
            if record.parents.len() != 1 {
                collection.complete = false;
                continue;
            }
            let agent = record_activity(paths, record, names.for_record(record));
            collection.complete &= agent.available && agent.status != SubagentStatus::Unknown;
            current.insert(record.id.clone(), agent.clone());
            collection.agents.push(agent);
        }
    }
    let mut pending_calls = HashSet::new();
    for block in &mut detail.blocks {
        let Some(mut view) = block.subagent.clone() else {
            continue;
        };
        let native = view.event_kind.is_some();
        let (explicit, task_path) = if native {
            (
                block
                    .payload
                    .as_ref()
                    .and_then(|p| field(p, &["agent_thread_id"]))
                    .or_else(|| view.agent_id.clone()),
                block
                    .payload
                    .as_ref()
                    .and_then(|p| field(p, &["agent_path"])),
            )
        } else {
            lifecycle::output_identity(block)
        };
        let source_parent = block
            .payload
            .as_ref()
            .and_then(|p| field(p, &["parent_thread_id"]));
        let mut matched = None;
        if source_parent
            .as_deref()
            .is_none_or(|id| id == detail.summary.id)
        {
            if let Ok(graph) = &graph {
                let records: Vec<_> = graph
                    .values()
                    .filter(|record| {
                        record.parents.contains(&detail.summary.id)
                            && record.parents.len() == 1
                            && if let Some(id) = &explicit {
                                record.id == *id
                            } else {
                                task_path
                                    .as_ref()
                                    .is_some_and(|path| record.path.as_ref() == Some(path))
                            }
                    })
                    .collect();
                if records.len() == 1 {
                    let record = records[0];
                    // An explicit ID wins resolution, but contradictory path evidence
                    // must not silently bind a different child.
                    if task_path.is_none() || record.path == task_path {
                        matched = current.get(&record.id).cloned();
                    }
                }
            }
        }
        if let Some(mut agent) = matched {
            if !agent.available {
                agent.agent_id = None;
            }
            agent.event_kind = view.event_kind;
            agent.event_id = view.event_id;
            agent.delegation = view.delegation;
            view = agent;
        } else {
            view.agent_id = None;
            view.available = false;
            view.status =
                if !native && block.text.is_none() && block.status.as_deref() == Some("running") {
                    SubagentStatus::Creating
                } else {
                    SubagentStatus::Unknown
                };
            view.name = humanize_task(&view.name);
            view.unavailable_reason = Some(if view.status == SubagentStatus::Creating {
                "子智能体记录尚未就绪".into()
            } else {
                "子智能体记录缺失或关联无法确认".into()
            });
            if view.status == SubagentStatus::Creating {
                let key = block.call_id.clone().unwrap_or_else(|| block.id.clone());
                if pending_calls.insert(key.clone()) {
                    let mut pending = view.clone();
                    pending.event_id = Some(key);
                    collection.agents.push(pending);
                }
            } else {
                collection.complete = false;
            }
        }
        block.subagent = Some(view);
    }
    for agent in &collection.agents {
        match agent.status {
            SubagentStatus::Creating => collection.counts.creating += 1,
            SubagentStatus::Running => collection.counts.running += 1,
            SubagentStatus::Completed => collection.counts.completed += 1,
            SubagentStatus::Failed => collection.counts.failed += 1,
            SubagentStatus::Interrupted => collection.counts.interrupted += 1,
            SubagentStatus::Unknown => collection.counts.unknown += 1,
        }
    }
    if !collection.complete {
        collection.warning = Some("部分子智能体记录无法确认，统计可能不完整".into());
    }
    detail.subagents = Some(collection);
    detail.subagent_updates = detail
        .blocks
        .iter()
        .filter_map(|block| {
            block.subagent.clone().map(|mut agent| {
                agent.delegation = None;
                (block.id.clone(), agent)
            })
        })
        .collect();
    detail.blocks.retain(|block| visible.contains(&block.id));
}

fn verified_detail(
    paths: &CodexPaths,
    request: &SubagentDetailRequest,
    paged: bool,
) -> Result<SubagentDetailResponse> {
    let graph = read_graph(paths)?;
    validate_relation(paths, &graph, &request.root_thread_id, &request.agent_id)?;
    let record = graph.get(&request.agent_id).context("子智能体记录不存在")?;
    let path = validate_rollout(paths, record)?;
    let status = native_status(&path)?;
    let parent = record.parents.iter().next().expect("verified parent");
    let names = graph
        .get(parent)
        .and_then(|parent| validate_rollout(paths, parent).ok())
        .and_then(|path| lifecycle::creation_names(&path).ok())
        .unwrap_or_default();
    let mut current_agent = activity(record, status, None);
    current_agent.name = display_name(names.for_record(record), record);
    let before = fs::metadata(&path)?;
    let summary = ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: record.id.clone(),
        title: current_agent.name.clone(),
        status: if status == SubagentStatus::Running {
            ThreadStatus::Running
        } else {
            ThreadStatus::Recent
        },
        updated_at: record.updated_at.clone(),
        archived_at: None,
        message_count: 0,
        latest_message: None,
        cwd: record.cwd.clone(),
        model: None,
        rollout_path: Some(path.clone()),
        active_turn_id: None,
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: None,
    };
    let mut detail = super::rollout_events::thread_detail_page_from_summary(
        summary,
        false,
        paged.then(|| request.limit.unwrap_or(120).clamp(1, 500)),
        request.before.as_deref(),
        None,
    )?;
    let after = fs::metadata(validate_rollout(paths, record)?)?;
    ensure!(
        before.len() == after.len() && before.modified()? == after.modified()?,
        "子智能体记录已变化，请重试"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec(),
            "子智能体文件身份已变化，请重试"
        );
    }
    let current_graph = read_graph(paths)?;
    validate_relation(
        paths,
        &current_graph,
        &request.root_thread_id,
        &request.agent_id,
    )?;
    let current = current_graph
        .get(&request.agent_id)
        .context("子智能体关联已变化")?;
    ensure!(
        current.parents == record.parents && validate_rollout(paths, current)? == path,
        "子智能体关联已变化，请重试"
    );
    enrich_subagent_blocks(paths, &mut detail);
    Ok(SubagentDetailResponse {
        root_thread_id: request.root_thread_id.clone(),
        parent_thread_id: record
            .parents
            .iter()
            .next()
            .expect("verified parent")
            .clone(),
        agent: current_agent,
        detail,
    })
}

/// Page only after checking the full parent chain against the current machine.
pub fn read_subagent_detail(
    paths: &CodexPaths,
    request: &SubagentDetailRequest,
) -> Result<SubagentDetailResponse> {
    if let Some(cursor) = &request.before {
        ensure!(
            cursor
                .strip_prefix("b:")
                .and_then(|s| s.parse::<usize>().ok())
                .is_some(),
            "无效的历史游标"
        );
    }
    verified_detail(paths, request, true)
}

/// Attachment identity is resolved from the verified native message, never a caller path.
pub(crate) fn read_attachment(
    paths: &CodexPaths,
    request: &crate::user_message::SessionAttachmentRequest,
) -> Result<crate::user_message::SessionAttachmentResponse> {
    let root = request
        .root_thread_id
        .as_ref()
        .context("缺少主线程上下文")?;
    let response = verified_detail(
        paths,
        &SubagentDetailRequest {
            root_thread_id: root.clone(),
            agent_id: request.session_key.clone(),
            limit: None,
            before: None,
        },
        false,
    )?;
    let message = response
        .detail
        .blocks
        .into_iter()
        .filter_map(|block| block.user_message)
        .find(|message| message.id == request.message_id)
        .context("子智能体附件已变化或不存在")?;
    message.read_attachment(&request.attachment_id)
}
