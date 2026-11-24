//! Parent-bound, read-only access and inline enrichment for Antigravity subagent sessions.
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

use crate::{
    codex::subagents::{
        SubagentActivity, SubagentCollection, SubagentCounts, SubagentEventKind, SubagentStatus,
    },
    provider_sessions::{
        paginate_provider_detail, ProviderDetailRequest, ProviderHistoryEvent,
        ProviderHistorySnapshot, ProviderSessionDetail, ProviderSessionSummary,
    },
};

use super::sessions::{
    attach_storage_size, decode_file_uri, descendant_ids, evaluate_offline_tree_status_with_lock,
    extract_transcript_spawned_subagents, has_conversation_parents_table,
    has_primary_session_storage, inspect_conv_db_metadata, is_presence_locked_shared,
    is_subtrajectory, list_summaries_from_source, live_item_status, load_store_auxiliary_index,
    make_summary_with_status, resolve_target_store_and_endpoint, subagent_states_by_root,
    valid_cascade_id, AntigravityPaths, LiveRpc,
};

const MAX_SUBAGENT_DEPTH: usize = 32;

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}

fn ensure_no_symlink_under_store(store_home: &Path, target: &Path) -> Result<Option<fs::Metadata>> {
    if store_home.exists() {
        let root_meta = fs::symlink_metadata(store_home)?;
        ensure!(
            root_meta.is_dir() && !root_meta.file_type().is_symlink(),
            "Antigravity 会话根目录不能是符号链接"
        );
    }
    let rel = target
        .strip_prefix(store_home)
        .context("子智能体路径不在当前存储目录内")?;
    let mut curr = store_home.to_path_buf();
    let mut last_meta = None;
    for comp in rel.components() {
        ensure!(
            matches!(comp, Component::Normal(_)),
            "子智能体路径包含非法组件"
        );
        curr.push(comp);
        match fs::symlink_metadata(&curr) {
            Ok(meta) => {
                ensure!(!meta.file_type().is_symlink(), "子智能体记录包含符号链接");
                last_meta = Some(meta);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        }
    }
    Ok(last_meta)
}

fn inspect_node_storage(
    store: &AntigravityPaths,
    id: &str,
) -> Result<Option<(String, Vec<FileStamp>)>> {
    let logs = store.brain_dir().join(id).join(".system_generated/logs");
    let candidates = [
        store.conversations_dir().join(format!("{id}.db")),
        store.conversations_dir().join(format!("{id}.pb")),
        store.brain_dir().join(id),
        logs.join("transcript_full.jsonl"),
        logs.join("transcript.jsonl"),
    ];
    let mut hasher = Sha256::new();
    let mut stamps = Vec::new();
    for path in candidates {
        if let Some(meta) = ensure_no_symlink_under_store(&store.home, &path)? {
            hasher.update(path.as_os_str().as_encoded_bytes());
            #[cfg(unix)]
            let identity = {
                use std::os::unix::fs::MetadataExt;
                hasher.update(meta.dev().to_le_bytes());
                hasher.update(meta.ino().to_le_bytes());
                (meta.dev(), meta.ino())
            };
            stamps.push(FileStamp {
                path,
                len: meta.len(),
                modified: meta.modified().ok(),
                #[cfg(unix)]
                identity,
            });
        }
    }
    if stamps.is_empty() {
        Ok(None)
    } else {
        let hex_full = hex::encode(hasher.finalize());
        Ok(Some((hex_full[..16].to_owned(), stamps)))
    }
}

#[derive(Default)]
struct SubagentNodeMeta {
    parents: BTreeSet<String>,
    title: Option<String>,
    cwd: String,
    updated_at: Option<String>,
    sqlite_status: Option<&'static str>,
    sidecar_killed: bool,
    live_item: Option<Value>,
}

struct StoreSubagentGraph {
    nodes: BTreeMap<String, SubagentNodeMeta>,
    children_map: BTreeMap<String, Vec<String>>,
    sqlite_statuses: BTreeMap<String, &'static str>,
    raw_summaries: BTreeMap<String, super::sessions::annotations::ParsedRawSummary>,
}

impl StoreSubagentGraph {
    fn add_edge(&mut self, parent: String, child: String) {
        if parent.is_empty()
            || child.is_empty()
            || parent == child
            || uuid::Uuid::parse_str(&parent).is_err()
            || uuid::Uuid::parse_str(&child).is_err()
        {
            return;
        }
        self.nodes
            .entry(child.clone())
            .or_default()
            .parents
            .insert(parent.clone());
        let kids = self.children_map.entry(parent).or_default();
        if !kids.contains(&child) {
            kids.push(child);
        }
    }
}

fn build_store_graph(
    store: &AntigravityPaths,
    endpoint: Option<&(LiveRpc, Value)>,
    seed_roots: &[&str],
    extra_events: Option<(&str, &[ProviderHistoryEvent])>,
) -> Result<StoreSubagentGraph> {
    let mut graph = StoreSubagentGraph {
        nodes: BTreeMap::new(),
        children_map: BTreeMap::new(),
        sqlite_statuses: BTreeMap::new(),
        raw_summaries: BTreeMap::new(),
    };
    let db_path = store.summaries_db();
    let open_conn =
        if ensure_no_symlink_under_store(&store.home, &db_path)?.is_some() && db_path.is_file() {
            Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .ok()
        } else {
            None
        };
    let aux = load_store_auxiliary_index(store, open_conn.as_ref());
    let mut fallback_root_edges: Vec<(String, String)> = Vec::new();
    for (pid, kids) in &aux.sidecar_children {
        for cid in kids {
            graph.add_edge(pid.clone(), cid.clone());
        }
    }
    for (cid, s_meta) in &aux.sidecar_meta {
        let node = graph.nodes.entry(cid.clone()).or_default();
        node.sidecar_killed |= s_meta.killed;
        if node.title.is_none() {
            node.title = s_meta.title.clone();
        }
    }
    for (cid, c_meta) in &aux.conv_meta {
        if let Some(pid) = c_meta.parent_id.as_ref() {
            graph.add_edge(pid.clone(), cid.clone());
        } else if let Some(rid) = c_meta.root_id.as_ref() {
            fallback_root_edges.push((rid.clone(), cid.clone()));
        }
        let node = graph.nodes.entry(cid.clone()).or_default();
        if node.title.is_none() {
            node.title = c_meta.title.clone();
        }
        if node.cwd.is_empty() {
            if let Some(cwd) = &c_meta.cwd {
                node.cwd = cwd.clone();
            }
        }
    }

    if let Some(conn) = &open_conn {
        if has_conversation_parents_table(conn) {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT conversation_id, COALESCE(parent_id, '') FROM conversation_parents",
            ) {
                if let Ok(rows) =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                {
                    for (cid, pid) in rows.flatten() {
                        graph.add_edge(pid, cid);
                    }
                }
            }
        }
        let cols: BTreeSet<String> = conn
            .prepare("PRAGMA table_info(conversation_summaries)")
            .ok()
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|it| it.flatten().collect())
            })
            .unwrap_or_default();
        let status_expr = if cols.contains("status") {
            "COALESCE(status, '')"
        } else {
            "''"
        };
        let idle_expr = if cols.contains("not_fully_idle") {
            "not_fully_idle"
        } else {
            "0"
        };
        let raw_expr = if cols.contains("raw_summary") {
            "raw_summary"
        } else {
            "NULL"
        };
        let sql = format!(
            "SELECT conversation_id, COALESCE(title, ''), COALESCE(preview, ''), \
             COALESCE(last_modified_time, ''), COALESCE(workspace_uris, ''), \
             COALESCE(parent_conversation_id, ''), {status_expr}, {idle_expr}, {raw_expr} \
             FROM conversation_summaries"
        );
        if let Ok(mut stmt) = conn.prepare(&sql) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, super::sessions::ValueOrString>(7)
                        .map(|v| v.0)
                        .unwrap_or(false),
                    r.get::<_, Option<Vec<u8>>>(8).ok().flatten(),
                ))
            }) {
                for (
                    cid,
                    title,
                    preview,
                    modified,
                    ws_raw,
                    parent,
                    status_str,
                    not_idle,
                    raw_blob,
                ) in rows.flatten()
                {
                    if uuid::Uuid::parse_str(&cid).is_err() {
                        continue;
                    }
                    let parsed_raw = raw_blob
                        .as_deref()
                        .map(super::sessions::annotations::parse_raw_summary);
                    if !parent.is_empty() {
                        graph.add_edge(parent, cid.clone());
                    } else if let Some(pid) = parsed_raw
                        .as_ref()
                        .and_then(|r| r.trajectory_meta.parent_conversation_id.as_deref())
                        .filter(|p| !p.is_empty() && *p != cid)
                    {
                        graph.add_edge(pid.to_owned(), cid.clone());
                    } else if let Some(rid) = parsed_raw
                        .as_ref()
                        .and_then(|r| r.trajectory_meta.root_conversation_id.as_deref())
                        .filter(|r| !r.is_empty() && *r != cid)
                    {
                        fallback_root_edges.push((rid.to_owned(), cid.clone()));
                    }
                    let st = super::sessions::offline_summary_row_status(&status_str, not_idle)
                        .or_else(|| {
                            parsed_raw.as_ref().and_then(
                                super::sessions::annotations::ParsedRawSummary::active_status,
                            )
                        });
                    if let Some(s) = st {
                        graph.sqlite_statuses.insert(cid.clone(), s);
                    }
                    let node = graph.nodes.entry(cid.clone()).or_default();
                    node.sqlite_status = st;
                    if let Some(raw_title) = parsed_raw
                        .as_ref()
                        .and_then(|r| r.annotations.title.as_deref())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        node.title = Some(raw_title.to_owned());
                    } else if !title.trim().is_empty() {
                        node.title = Some(title.trim().to_owned());
                    } else if !preview.trim().is_empty() {
                        node.title = Some(preview.trim().chars().take(80).collect());
                    }
                    if let Ok(uris) = serde_json::from_str::<Vec<String>>(&ws_raw) {
                        if let Some(first) = uris.into_iter().next() {
                            node.cwd = decode_file_uri(&first);
                        }
                    }
                    if node.cwd.is_empty() {
                        if let Some(u) = parsed_raw.as_ref().and_then(|r| {
                            r.workspaces
                                .first()
                                .or_else(|| r.trajectory_meta.workspace_uris.first())
                        }) {
                            node.cwd = decode_file_uri(u);
                        }
                    }
                    if !modified.trim().is_empty() && !modified.trim().starts_with("0001-") {
                        node.updated_at = Some(modified);
                    }
                    if let Some(pr) = parsed_raw {
                        graph.raw_summaries.insert(cid, pr);
                    }
                }
            }
        }
    }

    if let Some((_, payload)) = endpoint {
        if let Some(map) = payload["trajectorySummaries"].as_object() {
            for (cid, item) in map {
                if uuid::Uuid::parse_str(cid).is_err() {
                    continue;
                }
                let parent = item["parentConversationId"]
                    .as_str()
                    .or_else(|| item["trajectoryMetadata"]["parentConversationId"].as_str())
                    .filter(|s| !s.is_empty() && *s != cid);
                if let Some(pid) = parent {
                    graph.add_edge(pid.to_owned(), cid.clone());
                } else if is_subtrajectory(cid, item) {
                    if let Some(root) = item["rootConversationId"]
                        .as_str()
                        .or_else(|| item["trajectoryMetadata"]["rootConversationId"].as_str())
                        .filter(|s| !s.is_empty() && *s != cid)
                    {
                        fallback_root_edges.push((root.to_owned(), cid.clone()));
                    }
                }
                let node = graph.nodes.entry(cid.clone()).or_default();
                node.live_item = Some(item.clone());
                if let Some(t) = item["annotations"]["title"]
                    .as_str()
                    .or_else(|| item["summary"].as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    node.title = Some(t.to_owned());
                }
                if node.cwd.is_empty() {
                    if let Some(uri) = item["workspaces"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|w| w["workspaceFolderAbsoluteUri"].as_str())
                        .or_else(|| {
                            item["trajectoryMetadata"]["workspaceUris"]
                                .as_array()
                                .and_then(|a| a.first())
                                .and_then(Value::as_str)
                        })
                    {
                        node.cwd = decode_file_uri(uri);
                    }
                }
                if let Some(ts) = item["lastModifiedTime"]
                    .as_str()
                    .or_else(|| item["createdTime"].as_str())
                    .filter(|s| !s.is_empty())
                {
                    node.updated_at = Some(ts.to_owned());
                }
            }
        }
    }
    if let Some((parent_id, events)) = extra_events {
        for ev in events {
            let tool = ev.role.as_deref().or(ev.text.as_deref()).unwrap_or("");
            if tool == "invoke_subagent" {
                let mut spawned = BTreeSet::new();
                if let Some(res) = &ev.result {
                    super::collect_spawned_subagent_ids(&Value::String(res.clone()), &mut spawned);
                }
                for cid in spawned {
                    graph.add_edge(parent_id.to_owned(), cid);
                }
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<(String, usize)> =
        seed_roots.iter().map(|&r| (r.to_owned(), 0)).collect();
    while let Some((curr, depth)) = queue.pop_front() {
        if depth > MAX_SUBAGENT_DEPTH || !visited.insert(curr.clone()) {
            continue;
        }
        if inspect_node_storage(store, &curr).ok().flatten().is_some() {
            if let Some(db_meta) = inspect_conv_db_metadata(store, &curr) {
                if !db_meta.parent_ids.is_empty() {
                    for pid in db_meta.parent_ids {
                        graph.add_edge(pid, curr.clone());
                    }
                } else if let Some(rid) = db_meta.root_id {
                    fallback_root_edges.push((rid, curr.clone()));
                }
                if let Some(cwd) = db_meta.cwd {
                    let node = graph.nodes.entry(curr.clone()).or_default();
                    if node.cwd.is_empty() {
                        node.cwd = cwd;
                    }
                }
            }
            for cid in extract_transcript_spawned_subagents(store, &curr) {
                graph.add_edge(curr.clone(), cid);
            }
        }
        if let Some(kids) = graph.children_map.get(&curr).cloned() {
            for kid in kids {
                queue.push_back((kid, depth + 1));
            }
        }
    }

    for (root_id, child_id) in fallback_root_edges {
        if graph
            .nodes
            .get(&child_id)
            .is_none_or(|n| n.parents.is_empty())
        {
            graph.add_edge(root_id, child_id.clone());
            if visited.insert(child_id.clone()) {
                if let Some(db_meta) = inspect_conv_db_metadata(store, &child_id) {
                    for pid in db_meta.parent_ids {
                        graph.add_edge(pid, child_id.clone());
                    }
                }
                for grand_cid in extract_transcript_spawned_subagents(store, &child_id) {
                    graph.add_edge(child_id.clone(), grand_cid);
                }
            }
        }
    }

    Ok(graph)
}

fn verify_unique_parent_chain(
    graph: &StoreSubagentGraph,
    root_id: &str,
    agent_id: &str,
) -> Result<()> {
    ensure!(agent_id != root_id, "子智能体不能指向主会话自身");
    let mut visited = BTreeSet::from([agent_id.to_owned()]);
    let mut curr = agent_id;
    for _ in 0..MAX_SUBAGENT_DEPTH {
        let node = graph
            .nodes
            .get(curr)
            .context("子智能体不属于当前主会话或记录缺失")?;
        ensure!(
            node.parents.len() == 1,
            "子智能体父会话关联不唯一，拒绝访问"
        );
        let parent = node.parents.iter().next().expect("single parent");
        if parent == root_id {
            return Ok(());
        }
        ensure!(
            visited.insert(parent.clone()),
            "子智能体父链存在循环，拒绝访问"
        );
        curr = parent;
    }
    bail!("子智能体嵌套层级超过上限或未关联至主会话")
}

#[derive(Default, Clone)]
struct InvocationHints {
    name: Option<String>,
    role: Option<String>,
    delegation: Option<String>,
}

fn parse_tool_args_json(raw: Option<&str>) -> Value {
    raw.and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null)
}

fn first_str_field(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn humanize_label(raw: &str) -> String {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("子智能体");
    let count = line.chars().count();
    if count <= 60 {
        line.to_owned()
    } else {
        let mut s: String = line.chars().take(60).collect();
        s.push('…');
        s
    }
}

fn extract_invocation_hints(args: &Value) -> InvocationHints {
    let name = first_str_field(
        args,
        &[
            "Name",
            "name",
            "SubagentName",
            "subagentName",
            "Title",
            "title",
        ],
    );
    let role = first_str_field(
        args,
        &[
            "TypeName",
            "typeName",
            "SubagentType",
            "subagentType",
            "Role",
            "role",
            "Subagent",
            "subagent",
            "Agent",
            "agent",
        ],
    );
    let delegation = first_str_field(
        args,
        &[
            "Task",
            "task",
            "Prompt",
            "prompt",
            "Instructions",
            "instructions",
            "Message",
            "message",
        ],
    );
    InvocationHints {
        name,
        role,
        delegation,
    }
}

#[allow(clippy::too_many_arguments)]
fn child_activity_in_store(
    store: &AntigravityPaths,
    endpoint: Option<&(LiveRpc, Value)>,
    graph: &StoreSubagentGraph,
    sub_states: &BTreeMap<String, (bool, bool)>,
    child_id: &str,
    hints: Option<&InvocationHints>,
    inherited_lock: bool,
    now_ms: i64,
) -> SubagentActivity {
    let node = graph.nodes.get(child_id);
    let unique_parent = node.is_some_and(|n| n.parents.len() == 1);
    let live_item = node.and_then(|n| n.live_item.as_ref());
    let storage_inspect = inspect_node_storage(store, child_id);
    let sidecar_killed = node.is_some_and(|n| n.sidecar_killed);
    let raw_killed = graph
        .raw_summaries
        .get(child_id)
        .is_some_and(super::sessions::annotations::ParsedRawSummary::is_user_canceled_or_killed);

    let (available, status, unavailable_reason) = match storage_inspect {
        Err(_) => (
            false,
            SubagentStatus::Unknown,
            Some("子智能体记录包含符号链接或无法验证".into()),
        ),
        Ok(_) if !unique_parent => (
            false,
            SubagentStatus::Unknown,
            Some("子智能体父会话关联不明确".into()),
        ),
        Ok(None) if live_item.is_none() => (
            false,
            SubagentStatus::Unknown,
            Some("子智能体记录已清理或无法读取".into()),
        ),
        Ok(_) => {
            if let Some(item) = live_item {
                let (exec_subs, wait_subs) =
                    sub_states.get(child_id).copied().unwrap_or((false, false));
                let st = live_item_status(store, child_id, item, exec_subs, wait_subs, now_ms);
                let term = item["latestExecutorTerminationReason"]
                    .as_str()
                    .or_else(|| {
                        item["trajectoryMetadata"]["latestExecutorTerminationReason"].as_str()
                    })
                    .unwrap_or("");
                let mapped = match st {
                    "running" | "waiting" => SubagentStatus::Running,
                    "failed" => SubagentStatus::Failed,
                    _ if sidecar_killed
                        || raw_killed
                        || matches!(
                            term,
                            "EXECUTOR_TERMINATION_REASON_USER_CANCELED"
                                | "EXECUTOR_TERMINATION_REASON_INTERRUPTED"
                        ) =>
                    {
                        SubagentStatus::Interrupted
                    }
                    _ => SubagentStatus::Completed,
                };
                (true, mapped, None)
            } else if has_primary_session_storage(store, child_id) {
                let descendants = descendant_ids(&graph.children_map, child_id);
                let st = evaluate_offline_tree_status_with_lock(
                    store,
                    child_id,
                    &descendants,
                    &graph.sqlite_statuses,
                    Some(&graph.raw_summaries),
                    inherited_lock,
                    now_ms,
                );
                let mapped = match st {
                    "running" | "waiting" => SubagentStatus::Running,
                    "failed" => SubagentStatus::Failed,
                    _ if sidecar_killed || raw_killed => SubagentStatus::Interrupted,
                    _ => SubagentStatus::Completed,
                };
                let _ = endpoint;
                (true, mapped, None)
            } else {
                (
                    false,
                    SubagentStatus::Unknown,
                    Some("子智能体记录已清理或无法读取".into()),
                )
            }
        }
    };

    let node_title = node
        .and_then(|n| n.title.as_deref())
        .filter(|t| !t.is_empty() && *t != "未命名会话" && *t != child_id);
    let name = node_title
        .map(str::to_owned)
        .or_else(|| hints.and_then(|h| h.name.clone()))
        .or_else(|| {
            hints
                .and_then(|h| h.delegation.as_deref())
                .map(humanize_label)
        })
        .or_else(|| hints.and_then(|h| h.role.clone()))
        .unwrap_or_else(|| child_id.to_owned());
    let role = hints.and_then(|h| h.role.clone()).filter(|r| r != &name);
    let delegation = hints.and_then(|h| h.delegation.clone());

    SubagentActivity {
        event_kind: None,
        event_id: None,
        agent_id: available.then(|| child_id.to_owned()),
        name,
        role,
        status,
        available,
        unavailable_reason,
        delegation,
    }
}

fn ancestor_tree_locked(
    store: &AntigravityPaths,
    graph: &StoreSubagentGraph,
    start_id: &str,
) -> bool {
    if is_presence_locked_shared(store, start_id) {
        return true;
    }
    let mut curr = start_id;
    let mut seen = BTreeSet::from([start_id]);
    for _ in 0..MAX_SUBAGENT_DEPTH {
        let Some(node) = graph.nodes.get(curr) else {
            break;
        };
        if node.parents.len() != 1 {
            break;
        }
        let parent = node.parents.iter().next().expect("single parent").as_str();
        if !seen.insert(parent) {
            break;
        }
        if is_presence_locked_shared(store, parent) {
            return true;
        }
        curr = parent;
    }
    false
}

fn enrich_with_graph(
    store: &AntigravityPaths,
    endpoint: Option<&(LiveRpc, Value)>,
    graph: &StoreSubagentGraph,
    focus_id: &str,
    all_events: &[ProviderHistoryEvent],
    detail: &mut ProviderSessionDetail,
) {
    let now_ms = super::sessions::now_epoch_ms();
    let sub_states = endpoint
        .and_then(|(_, payload)| payload["trajectorySummaries"].as_object())
        .map(subagent_states_by_root)
        .unwrap_or_default();

    let mut hints_by_child: BTreeMap<String, InvocationHints> = BTreeMap::new();
    let mut anchor_actions: BTreeMap<
        String,
        (SubagentEventKind, Option<String>, InvocationHints, bool),
    > = BTreeMap::new();

    for ev in all_events {
        if !matches!(ev.kind.as_str(), "tool_call" | "tool_result") {
            continue;
        }
        let tool = ev.role.as_deref().or(ev.text.as_deref()).unwrap_or("");
        if !matches!(
            tool,
            "invoke_subagent" | "send_message" | "manage_subagents"
        ) {
            continue;
        }
        let combined_result = ev.result.as_deref();
        let args = parse_tool_args_json(ev.detail.as_deref());

        match tool {
            "invoke_subagent" => {
                let hints = extract_invocation_hints(&args);
                let mut spawned = BTreeSet::new();
                if let Some(res) = combined_result {
                    super::collect_spawned_subagent_ids(
                        &Value::String(res.to_owned()),
                        &mut spawned,
                    );
                }
                spawned.remove(focus_id);
                let child_id = spawned.into_iter().next();
                if let Some(ref cid) = child_id {
                    hints_by_child
                        .entry(cid.clone())
                        .or_insert_with(|| hints.clone());
                }
                let in_flight = child_id.is_none()
                    && combined_result.is_none()
                    && matches!(ev.status.as_deref(), None | Some("running"));
                anchor_actions.insert(
                    ev.id.clone(),
                    (SubagentEventKind::Started, child_id, hints, in_flight),
                );
            }
            "send_message" => {
                let recipient = first_str_field(&args, &["Recipient", "recipient"]);
                let is_direct_child = recipient.as_deref().is_some_and(|cid| {
                    graph
                        .nodes
                        .get(cid)
                        .is_some_and(|n| n.parents.len() == 1 && n.parents.contains(focus_id))
                });
                if !is_direct_child {
                    continue;
                }
                let msg = first_str_field(&args, &["Message", "message"]);
                let hints = InvocationHints {
                    name: None,
                    role: None,
                    delegation: msg,
                };
                anchor_actions.insert(
                    ev.id.clone(),
                    (SubagentEventKind::Interacted, recipient, hints, false),
                );
            }
            "manage_subagents" => {
                let action = first_str_field(&args, &["Action", "action"]).unwrap_or_default();
                let target_child = args["ConversationIds"]
                    .as_array()
                    .or_else(|| args["conversationIds"].as_array())
                    .and_then(|arr| arr.iter().filter_map(Value::as_str).next())
                    .filter(|&cid| {
                        graph
                            .nodes
                            .get(cid)
                            .is_some_and(|n| n.parents.len() == 1 && n.parents.contains(focus_id))
                    })
                    .map(str::to_owned);
                let Some(target_child) = target_child else {
                    continue;
                };
                let kind = if matches!(action.as_str(), "kill" | "kill_all") {
                    SubagentEventKind::Interrupted
                } else {
                    SubagentEventKind::Interacted
                };
                anchor_actions.insert(
                    ev.id.clone(),
                    (kind, Some(target_child), InvocationHints::default(), false),
                );
            }
            _ => {}
        }
    }

    let mut direct_ids: Vec<String> = graph
        .nodes
        .iter()
        .filter(|(_, meta)| meta.parents.contains(focus_id))
        .map(|(cid, _)| cid.clone())
        .collect();
    direct_ids.sort();

    let mut collection = SubagentCollection {
        agents: Vec::new(),
        counts: SubagentCounts::default(),
        complete: true,
        warning: None,
    };
    let inherited_lock = ancestor_tree_locked(store, graph, focus_id);
    let mut resolved_children: BTreeMap<String, SubagentActivity> = BTreeMap::new();
    for cid in direct_ids {
        let meta_unique = graph.nodes.get(&cid).is_some_and(|n| n.parents.len() == 1);
        if !meta_unique {
            collection.complete = false;
            continue;
        }
        let agent = child_activity_in_store(
            store,
            endpoint,
            graph,
            &sub_states,
            &cid,
            hints_by_child.get(&cid),
            inherited_lock,
            now_ms,
        );
        collection.complete &= agent.available && agent.status != SubagentStatus::Unknown;
        resolved_children.insert(cid, agent.clone());
        collection.agents.push(agent);
    }

    let mut updates: BTreeMap<String, SubagentActivity> = BTreeMap::new();
    for (event_id, (event_kind, child_id, call_hints, in_flight)) in &anchor_actions {
        let view = if let Some(cid) = child_id.as_deref() {
            if let Some(mut base) = resolved_children.get(cid).cloned() {
                base.event_kind = Some(*event_kind);
                base.event_id = Some(event_id.clone());
                if call_hints.delegation.is_some() {
                    base.delegation = call_hints.delegation.clone();
                }
                base
            } else {
                collection.complete = false;
                SubagentActivity {
                    event_kind: Some(*event_kind),
                    event_id: Some(event_id.clone()),
                    agent_id: None,
                    name: call_hints
                        .name
                        .clone()
                        .or_else(|| call_hints.delegation.as_deref().map(humanize_label))
                        .unwrap_or_else(|| cid.to_owned()),
                    role: call_hints.role.clone(),
                    status: SubagentStatus::Unknown,
                    available: false,
                    unavailable_reason: Some("子智能体记录缺失或关联无法确认".into()),
                    delegation: call_hints.delegation.clone(),
                }
            }
        } else if *in_flight {
            let pending = SubagentActivity {
                event_kind: Some(*event_kind),
                event_id: Some(event_id.clone()),
                agent_id: None,
                name: call_hints
                    .name
                    .clone()
                    .or_else(|| call_hints.delegation.as_deref().map(humanize_label))
                    .unwrap_or_else(|| "子智能体".into()),
                role: call_hints.role.clone(),
                status: SubagentStatus::Creating,
                available: false,
                unavailable_reason: Some("子智能体记录尚未就绪".into()),
                delegation: call_hints.delegation.clone(),
            };
            collection.agents.push(pending.clone());
            pending
        } else {
            collection.complete = false;
            SubagentActivity {
                event_kind: Some(*event_kind),
                event_id: Some(event_id.clone()),
                agent_id: None,
                name: call_hints
                    .name
                    .clone()
                    .or_else(|| call_hints.delegation.as_deref().map(humanize_label))
                    .unwrap_or_else(|| "子智能体".into()),
                role: call_hints.role.clone(),
                status: SubagentStatus::Unknown,
                available: false,
                unavailable_reason: Some("子智能体记录缺失或关联无法确认".into()),
                delegation: call_hints.delegation.clone(),
            }
        };
        let mut update_view = view.clone();
        update_view.delegation = None;
        updates.insert(event_id.clone(), update_view);
        if let Some(ev) = detail.events.iter_mut().find(|e| &e.id == event_id) {
            ev.subagent = Some(view);
        }
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
    if !collection.agents.is_empty() || !collection.complete {
        detail.subagents = Some(collection);
    }
    if !updates.is_empty() {
        detail.subagent_updates = Some(updates);
    }
}

pub(crate) fn enrich_session_detail(
    store: &AntigravityPaths,
    endpoint: Option<&(LiveRpc, Value)>,
    focus_id: &str,
    all_events: &[ProviderHistoryEvent],
    detail: &mut ProviderSessionDetail,
) {
    let Ok(graph) = build_store_graph(store, endpoint, &[focus_id], Some((focus_id, all_events)))
    else {
        return;
    };
    enrich_with_graph(store, endpoint, &graph, focus_id, all_events, detail);
}

struct VerifiedSubagentLoad {
    store: AntigravityPaths,
    endpoint: Option<(LiveRpc, Value)>,
    graph: StoreSubagentGraph,
    agent_activity: SubagentActivity,
    child_summary: ProviderSessionSummary,
    snapshot: std::sync::Arc<ProviderHistorySnapshot>,
    prefix: String,
    before_stamps: Option<Vec<FileStamp>>,
}

fn load_verified_subagent(
    paths: &AntigravityPaths,
    root_session_key: &str,
    agent_id: &str,
) -> Result<VerifiedSubagentLoad> {
    valid_cascade_id(root_session_key)?;
    valid_cascade_id(agent_id)?;
    ensure!(agent_id != root_session_key, "子智能体不能指向主会话自身");

    let (store, endpoint) = resolve_target_store_and_endpoint(paths, root_session_key)?;
    let root_summary = list_summaries_from_source(
        &store,
        Some(root_session_key),
        endpoint.as_ref().map(|(_, v)| v),
    )?
    .into_iter()
    .next()
    .context("未找到所属 Antigravity 主会话")?;
    let root_inspect = inspect_node_storage(&store, root_session_key)?;
    let graph = build_store_graph(&store, endpoint.as_ref(), &[root_session_key], None)?;
    verify_unique_parent_chain(&graph, root_session_key, agent_id)?;

    let child_inspect = inspect_node_storage(&store, agent_id)?;
    let in_live = endpoint
        .as_ref()
        .and_then(|(_, v)| v["trajectorySummaries"].get(agent_id))
        .is_some();
    ensure!(
        child_inspect.is_some() || in_live,
        "子智能体记录已清理或不属于当前存储"
    );
    ensure!(
        paths
            .store_for_id(agent_id, endpoint.as_ref().map(|(_, v)| v))
            .home
            == store.home,
        "拒绝跨存储读取 Antigravity 子智能体"
    );

    let parent_id = graph
        .nodes
        .get(agent_id)
        .and_then(|n| n.parents.iter().next())
        .cloned()
        .unwrap_or_else(|| root_session_key.to_owned());
    let parent_hints =
        super::sessions::load_events_from_source(&store, &parent_id, endpoint.as_ref())
            .ok()
            .and_then(|parent_snap| {
                for ev in parent_snap.events.iter() {
                    if ev.role.as_deref().or(ev.text.as_deref()) == Some("invoke_subagent") {
                        let mut spawned = BTreeSet::new();
                        if let Some(res) = &ev.result {
                            super::collect_spawned_subagent_ids(
                                &Value::String(res.clone()),
                                &mut spawned,
                            );
                        }
                        if spawned.contains(agent_id) {
                            return Some(extract_invocation_hints(&parse_tool_args_json(
                                ev.detail.as_deref(),
                            )));
                        }
                    }
                }
                None
            });

    let now_ms = super::sessions::now_epoch_ms();
    let sub_states = endpoint
        .as_ref()
        .and_then(|(_, payload)| payload["trajectorySummaries"].as_object())
        .map(subagent_states_by_root)
        .unwrap_or_default();
    let inherited_lock = is_presence_locked_shared(&store, root_session_key)
        || ancestor_tree_locked(&store, &graph, agent_id);
    let agent_activity = child_activity_in_store(
        &store,
        endpoint.as_ref(),
        &graph,
        &sub_states,
        agent_id,
        parent_hints.as_ref(),
        inherited_lock,
        now_ms,
    );
    ensure!(
        agent_activity.available,
        "{}",
        agent_activity
            .unavailable_reason
            .as_deref()
            .unwrap_or("子智能体记录当前不可读")
    );

    let snapshot = super::sessions::load_events_from_source(&store, agent_id, endpoint.as_ref())?;
    let node_meta = graph.nodes.get(agent_id);
    let cwd = node_meta
        .map(|n| n.cwd.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| root_summary.cwd.clone());
    let updated_at = node_meta
        .and_then(|n| n.updated_at.clone())
        .or_else(|| root_summary.updated_at.clone());
    let raw_status = match agent_activity.status {
        SubagentStatus::Running => "running",
        SubagentStatus::Failed => "failed",
        _ => "idle",
    };
    let mut child_summary = make_summary_with_status(
        &store,
        agent_id.to_owned(),
        agent_activity.name.clone(),
        cwd,
        false,
        updated_at,
        snapshot.events.len(),
        None,
        raw_status,
        if in_live {
            "cascade-v2"
        } else {
            "sqlite-offline"
        },
        false,
    );
    attach_storage_size(&store, &mut child_summary);

    let root_id_hex = root_inspect
        .as_ref()
        .map(|(h, _)| h.as_str())
        .unwrap_or("live");
    let (child_id_hex, before_stamps) = match child_inspect {
        Some((h, stamps)) => (h, Some(stamps)),
        None => ("live".to_owned(), None),
    };
    let full_digest = hex::encode(Sha256::digest(format!(
        "{root_session_key}\0{agent_id}\0{root_id_hex}\0{child_id_hex}"
    )));
    let prefix = format!("{}:", &full_digest[..16]);

    Ok(VerifiedSubagentLoad {
        store,
        endpoint,
        graph,
        agent_activity,
        child_summary,
        snapshot,
        prefix,
        before_stamps,
    })
}

fn stamps_compatible_after_read(before: &[FileStamp], after: &[FileStamp]) -> bool {
    before.iter().all(|b| {
        after.iter().any(|a| {
            if a.path != b.path {
                return false;
            }
            #[cfg(unix)]
            if a.identity != b.identity {
                return false;
            }
            let is_dir = a.path.file_name().is_some_and(|n| {
                a.path
                    .parent()
                    .and_then(|p| p.file_name())
                    .is_some_and(|pn| pn == "brain")
                    && uuid::Uuid::parse_str(&n.to_string_lossy()).is_ok()
            });
            is_dir || a.len >= b.len
        })
    })
}

fn verify_after_read(
    load: &VerifiedSubagentLoad,
    root_session_key: &str,
    agent_id: &str,
) -> Result<()> {
    let after = inspect_node_storage(&load.store, agent_id)?;
    if let Some(before_stamps) = &load.before_stamps {
        let Some((_, after_stamps)) = after else {
            bail!("子智能体记录在读取期间已被移除，请重试");
        };
        ensure!(
            stamps_compatible_after_read(before_stamps, &after_stamps),
            "子智能体来源已变化，请重试"
        );
    }
    let current_graph = build_store_graph(
        &load.store,
        load.endpoint.as_ref(),
        &[root_session_key],
        None,
    )?;
    verify_unique_parent_chain(&current_graph, root_session_key, agent_id)?;
    Ok(())
}

pub(crate) fn subagent_detail(
    paths: &AntigravityPaths,
    request: &ProviderDetailRequest,
    agent_id: &str,
) -> Result<ProviderSessionDetail> {
    let mut loaded = load_verified_subagent(paths, &request.session_key, agent_id)?;
    let prefix = loaded.prefix.clone();
    let native_cursor = request
        .before
        .as_deref()
        .map(|cursor| -> Result<String> {
            let (mode, position) = cursor
                .split_once(':')
                .filter(|(kind, _)| matches!(*kind, "j" | "n" | "w" | "p"))
                .map_or(("", cursor), |(kind, id)| (kind, id));
            let position = position
                .strip_prefix(&prefix)
                .context("子智能体历史来源已变化，请重新打开")?;
            Ok(if mode.is_empty() {
                position.into()
            } else {
                format!("{mode}:{position}")
            })
        })
        .transpose()?;

    let inner_req = ProviderDetailRequest {
        session_key: agent_id.to_owned(),
        agent_id: None,
        limit: request.limit,
        before: native_cursor,
    };
    let mut detail =
        paginate_provider_detail(loaded.child_summary.clone(), &loaded.snapshot, &inner_req)?;
    for ev in loaded.snapshot.events.iter() {
        let tool = ev.role.as_deref().or(ev.text.as_deref()).unwrap_or("");
        if tool == "invoke_subagent" {
            let mut spawned = BTreeSet::new();
            if let Some(res) = &ev.result {
                super::collect_spawned_subagent_ids(&Value::String(res.clone()), &mut spawned);
            }
            for cid in spawned {
                loaded.graph.add_edge(agent_id.to_owned(), cid);
            }
        }
    }
    enrich_with_graph(
        &loaded.store,
        loaded.endpoint.as_ref(),
        &loaded.graph,
        agent_id,
        &loaded.snapshot.events,
        &mut detail,
    );

    for ev in &mut detail.events {
        ev.id = format!("{prefix}{}", ev.id);
        if let Some(um) = &mut ev.user_message {
            um.id = format!("{prefix}{}", um.id);
        }
        if let Some(sub) = &mut ev.subagent {
            if let Some(eid) = &mut sub.event_id {
                *eid = format!("{prefix}{eid}");
            }
        }
    }
    if let Some(updates) = detail.subagent_updates.take() {
        detail.subagent_updates = Some(
            updates
                .into_iter()
                .map(|(k, mut v)| {
                    if let Some(eid) = &mut v.event_id {
                        *eid = format!("{prefix}{eid}");
                    }
                    (format!("{prefix}{k}"), v)
                })
                .collect(),
        );
    }
    if let Some(tl) = &mut detail.timeline {
        for entry in std::sync::Arc::make_mut(tl) {
            entry.id = format!("{prefix}{}", entry.id);
        }
    }
    if let Some(hist) = &mut detail.history {
        hist.epoch = format!("{prefix}{}", hist.epoch);
        for cid in &mut hist.changed_ids {
            *cid = format!("{prefix}{cid}");
        }
        for ch in &mut hist.changes {
            for cid in &mut ch.changed_ids {
                *cid = format!("{prefix}{cid}");
            }
        }
    }
    if let Some(bc) = &mut detail.before_cursor {
        *bc = format!("{prefix}{bc}");
    }
    if let Some(ac) = &mut detail.after_cursor {
        let rest = ac.strip_prefix("n:").unwrap_or(ac);
        *ac = format!("n:{prefix}{rest}");
    }
    detail.agent = Some(loaded.agent_activity.clone());

    verify_after_read(&loaded, &request.session_key, agent_id)?;
    let _ = &loaded.graph;
    Ok(detail)
}

pub(crate) fn read_attachment(
    paths: &AntigravityPaths,
    request: &crate::user_message::SessionAttachmentRequest,
) -> Result<crate::user_message::SessionAttachmentResponse> {
    let root_session_key = request
        .root_thread_id
        .as_deref()
        .context("缺少主会话上下文")?;
    let agent_id = &request.session_key;
    let loaded = load_verified_subagent(paths, root_session_key, agent_id)?;
    let raw_msg_id = request
        .message_id
        .strip_prefix(&loaded.prefix)
        .unwrap_or(&request.message_id);
    let mut message = loaded
        .snapshot
        .events
        .iter()
        .filter_map(|event| event.user_message.as_ref())
        .find(|msg| msg.id == raw_msg_id)
        .cloned()
        .context("子智能体附件已变化或不存在，请刷新会话")?;
    verify_after_read(&loaded, root_session_key, agent_id)?;
    message.refresh_files(
        (!loaded.child_summary.cwd.is_empty()).then_some(loaded.child_summary.cwd.as_str()),
    );
    message.read_attachment(&request.attachment_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::antigravity::sessions::{
        execute_batch_item, list_sessions, live_item_is_failed, live_item_is_running,
        live_item_is_waiting, preview_batch_item, session_detail, AntigravityPaths,
    };
    use crate::provider_sessions::ProviderListRequest;
    use crate::services::sessions::SessionOperation;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[cfg(unix)]
    #[test]
    fn unindexed_pb_lock_steps_status_and_proc_net_tcp_convergence() {
        use std::os::unix::io::AsRawFd;
        let tcp_sample = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 0100007F:C351 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 91001 1 0000000000000000 100 0 0 10 0\n\
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 91002 1 0000000000000000 100 0 0 10 0\n\
   2: 0100007F:C352 0100007F:D001 01 00000000:00000000 00:00000000 00000000  1000        0 91003 1 0000000000000000 100 0 0 10 0\n";
        let inodes = BTreeSet::from([91001_u64, 91002, 91003]);
        assert_eq!(
            super::super::sessions::parse_proc_net_tcp_loopback_ports(tcp_sample, &inodes),
            vec![50001]
        );

        let tcp6_sample = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 0000000000000000FFFF00000100007F:C359 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 91009 1 0000000000000000 100 0 0 10 0\n\
   1: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 91010 1 0000000000000000 100 0 0 10 0\n";
        let inodes6 = BTreeSet::from([91009_u64, 91010]);
        assert_eq!(
            super::super::sessions::parse_proc_net_tcp_loopback_ports(tcp6_sample, &inodes6),
            vec![50009]
        );
        assert_eq!(
            super::super::sessions::parse_loopback_listener_port("n[::ffff:127.0.0.1]:50009"),
            Some(50009)
        );
        assert_eq!(
            super::super::sessions::parse_loopback_listener_port("n[::]:50009"),
            None
        );

        let waiting_item = json!({"status": "CASCADE_RUN_STATUS_WAITING", "notFullyIdle": true, "waitingSteps": [{"stepIdx": 1}]});
        assert!(live_item_is_waiting(&waiting_item));
        assert!(live_item_is_running(&waiting_item));
        assert!(!live_item_is_failed(&waiting_item));
        let failed_item = json!({
            "status": "CASCADE_RUN_STATUS_IDLE",
            "latestExecutorTerminationReason": "EXECUTOR_TERMINATION_REASON_MAX_INVOCATIONS"
        });
        assert!(live_item_is_failed(&failed_item));

        let root =
            std::env::temp_dir().join(format!("agenthub-ag-pb-status-{}", uuid::Uuid::new_v4()));
        for d in ["conversations", "presence", "brain"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let indexed_gen_id = uuid::Uuid::new_v4().to_string();
        let indexed_wait_id = uuid::Uuid::new_v4().to_string();
        let indexed_err_id = uuid::Uuid::new_v4().to_string();
        let pb_root_id = uuid::Uuid::new_v4().to_string();
        let pb_sub_id = uuid::Uuid::new_v4().to_string();
        let orphan_brain_id = uuid::Uuid::new_v4().to_string();
        fs::create_dir_all(root.join("brain").join(&orphan_brain_id)).unwrap();

        let make_steps_db = |cid: &str, status_code: i64| {
            let p = root.join("conversations").join(format!("{cid}.db"));
            let c = Connection::open(&p).unwrap();
            c.execute_batch("CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB);").unwrap();
            c.execute(
                "INSERT INTO steps VALUES (0, 14, 3, X''), (1, 15, ?1, X'')",
                rusqlite::params![status_code],
            )
            .unwrap();
        };
        make_steps_db(&indexed_gen_id, 8); // GENERATING -> running
        make_steps_db(&indexed_wait_id, 9); // WAITING -> waiting
        make_steps_db(&indexed_err_id, 7); // ERROR -> failed

        let sconn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        sconn
            .execute_batch(
                "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY, title TEXT, preview TEXT, step_count INTEGER,
                last_modified_time TEXT, workspace_uris TEXT, status TEXT,
                parent_conversation_id TEXT, nesting_depth INTEGER, not_fully_idle NUMERIC
            );",
            )
            .unwrap();
        for (cid, title) in [
            (&indexed_gen_id, "Generating DB"),
            (&indexed_wait_id, "Waiting DB"),
            (&indexed_err_id, "Failed DB"),
        ] {
            sconn.execute(
                "INSERT INTO conversation_summaries VALUES (?1, ?2, '', 2, '2026-11-24T01:00:00Z', '', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0)",
                rusqlite::params![cid, title],
            ).unwrap();
        }
        drop(sconn);

        // Unindexed .pb root + unindexed .pb spawned subagent
        fs::write(
            root.join("conversations").join(format!("{pb_root_id}.pb")),
            b"pb-root",
        )
        .unwrap();
        fs::write(
            root.join("conversations").join(format!("{pb_sub_id}.pb")),
            b"pb-sub",
        )
        .unwrap();
        let now_iso = crate::timestamps::format_utc(jiff::Timestamp::now(), true);
        let pb_logs = root
            .join("brain")
            .join(&pb_root_id)
            .join(".system_generated/logs");
        fs::create_dir_all(&pb_logs).unwrap();
        fs::write(
            pb_logs.join("transcript_full.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":now_iso,"content":"CLI 无索引任务"}),
                json!({"step_index":1,"type":"GENERIC","status":"RUNNING","created_at":now_iso,"content":"old running step"}),
                json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"正在派发子代理","tool_calls":[{"id":"c1","name":"invoke_subagent","arguments":{"Name":"Worker","Task":"Inspect"}}]}),
            ),
        ).unwrap();

        let hold_lock = |cid: &str| {
            let f = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(root.join("presence").join(format!("{cid}.lock")))
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            f
        };
        let gen_lock = hold_lock(&indexed_gen_id);
        let wait_lock = hold_lock(&indexed_wait_id);
        let pb_lock = hold_lock(&pb_root_id);

        // Append subagent spawn result to pb_root transcript so pb_sub_id is recognized as child
        let mut f_append = fs::OpenOptions::new()
            .append(true)
            .open(pb_logs.join("transcript_full.jsonl"))
            .unwrap();
        writeln!(
            f_append,
            "{}",
            json!({"step_index":3,"type":"GENERIC","status":"DONE","created_at":now_iso,"content":format!("Created the following subagent:\n{{\"conversationId\":\"{pb_sub_id}\"}}")})
        ).unwrap();
        drop(f_append);

        let paths = AntigravityPaths::offline(root.clone());
        let list = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert!(
            !list.iter().any(|s| s.id == orphan_brain_id),
            "orphan brain dir must not resurrect"
        );
        assert!(
            !list.iter().any(|s| s.id == pb_sub_id),
            "unindexed spawned subagent must not appear as root"
        );

        let by_id: BTreeMap<_, _> = list.iter().map(|s| (s.id.as_str(), s)).collect();
        assert_eq!(by_id[indexed_gen_id.as_str()].status, "running");
        assert_eq!(by_id[indexed_wait_id.as_str()].status, "waiting");
        assert!(!by_id[indexed_wait_id.as_str()].can_delete);
        assert_eq!(by_id[indexed_err_id.as_str()].status, "failed");
        assert_eq!(by_id[pb_root_id.as_str()].status, "running");
        assert_eq!(by_id[pb_root_id.as_str()].title, "CLI 无索引任务");

        // Verify stale RUNNING row in transcript_full.jsonl was normalized to completed because later step settled
        let pb_detail = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: pb_root_id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(pb_detail.events[1].status.as_deref(), Some("completed"));

        // Yielding turn via PLANNER_RESPONSE (no tool_calls) while pb_sub_id is still unreturned keeps pb_root running
        let mut f_wait = fs::OpenOptions::new()
            .append(true)
            .open(pb_logs.join("transcript_full.jsonl"))
            .unwrap();
        writeln!(
            f_wait,
            "{}",
            json!({"step_index":4,"type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"等待子代理回报"})
        )
        .unwrap();
        drop(f_wait);
        let waiting_sub_list = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(
            waiting_sub_list
                .iter()
                .find(|s| s.id == pb_root_id)
                .unwrap()
                .status,
            "running"
        );

        // Subagent wakeup SYSTEM_MESSAGE (transcript content sender=<uuid>) clears pending_subagents, then PLANNER_RESPONSE + FINISH settles to idle
        let mut f_fin = fs::OpenOptions::new()
            .append(true)
            .open(pb_logs.join("transcript_full.jsonl"))
            .unwrap();
        writeln!(
            f_fin,
            "{}",
            json!({"step_index":5,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":now_iso,"content":format!("<SYSTEM_MESSAGE>\n[Message] timestamp={now_iso} sender={pb_sub_id} priority=MESSAGE_PRIORITY_HIGH content=Subagent done\n</SYSTEM_MESSAGE>")})
        )
        .unwrap();
        writeln!(
            f_fin,
            "{}",
            json!({"step_index":6,"type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"全部任务已完成。"})
        )
        .unwrap();
        writeln!(
            f_fin,
            "{}",
            json!({"step_index":7,"type":"FINISH","status":"DONE","created_at":now_iso})
        )
        .unwrap();
        drop(f_fin);
        let settled_list = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        let pb_after = settled_list.iter().find(|s| s.id == pb_root_id).unwrap();
        assert_eq!(pb_after.status, "idle");

        for l in [gen_lock, wait_lock, pb_lock] {
            unsafe {
                let _ = libc::flock(l.as_raw_fd(), libc::LOCK_UN);
            }
        }
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn nested_subagents_cache_dynamic_refresh_guards_and_attachments() {
        use std::os::unix::io::AsRawFd;
        let base =
            std::env::temp_dir().join(format!("agenthub-ag-nested-{}", uuid::Uuid::new_v4()));
        let app_home = base.join("app");
        let cli_home = base.join("cli");
        for home in [&app_home, &cli_home] {
            for d in ["conversations", "brain", "presence"] {
                fs::create_dir_all(home.join(d)).unwrap();
            }
        }
        let l0 = "10000000-0000-4000-8000-000000000000";
        let l1 = "20000000-0000-4000-8000-000000000001";
        let l2 = "30000000-0000-4000-8000-000000000002";
        let other_root = "40000000-0000-4000-8000-000000000004";
        let cli_child = "50000000-0000-4000-8000-000000000005";

        for cid in [l0, l1, l2, other_root] {
            fs::write(
                app_home.join("conversations").join(format!("{cid}.pb")),
                b"pb",
            )
            .unwrap();
        }
        fs::write(
            cli_home
                .join("conversations")
                .join(format!("{cli_child}.pb")),
            b"cli-pb",
        )
        .unwrap();

        let img_path = base.join("subagent-shot.png");
        let one_px_png: [u8; 67] = [
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H',
            b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, b'I', b'D', b'A', b'T', 0x78,
            0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ];
        fs::write(&img_path, one_px_png).unwrap();

        let write_transcript = |home: &Path, cid: &str, rows: &[Value]| {
            let dir = home.join("brain").join(cid).join(".system_generated/logs");
            fs::create_dir_all(&dir).unwrap();
            let text = rows
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            fs::write(dir.join("transcript_full.jsonl"), text).unwrap();
        };

        let now_iso = crate::timestamps::format_utc(jiff::Timestamp::now(), true);
        write_transcript(
            &app_home,
            l0,
            &[
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-24T01:00:00Z","content":"主任务"}),
                json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-24T01:00:01Z","tool_calls":[
                    {"id":"call-spawn-l1","name":"invoke_subagent","arguments":{"Name":"Explorer L1","Role":"research","Task":"Audit backend"}}
                ]}),
                json!({"step_index":2,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:02Z","content":format!("Created the following subagent:\n{{\"conversationId\":\"{l1}\"}}")}),
                json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-24T01:00:03Z","tool_calls":[
                {"id":"call-msg-l1","name":"send_message","arguments":{"Recipient":l1,"Message":"Focus on subagents.rs"}},
                {"id":"call-kill-l1","name":"manage_subagents","arguments":{"Action":"kill","ConversationIds":[l1]}}
            ],"content":"已协调子代理"}),
            ],
        );
        write_transcript(
            &app_home,
            l1,
            &[
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-24T01:00:02Z","content":"核查 L1 附件","media":[{"uri":format!("file://{}", img_path.display()),"mime_type":"image/png"}]}),
                json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-24T01:00:03Z","tool_calls":[
                {"id":"call-spawn-l2","name":"invoke_subagent","arguments":{"Name":"Worker L2","Task":"Check leaf"}},
                {"id":"call-reply-parent","name":"send_message","arguments":{"Recipient":l0,"Message":"Done reporting to parent"}}
            ],"content":"L1 完成"}),
                json!({"step_index":2,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:04Z","content":format!("Created the following subagent:\n{{\"conversationId\":\"{l2}\"}}")}),
                json!({"step_index":3,"type":"FINISH","status":"DONE","created_at":"2026-11-24T01:00:05Z"}),
            ],
        );
        write_transcript(
            &app_home,
            l2,
            &[
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-24T01:00:04Z","content":"L2 叶子任务"}),
                json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-24T01:00:05Z","content":"L2 完毕"}),
            ],
        );
        write_transcript(
            &app_home,
            other_root,
            &[
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-24T01:00:00Z","content":"无关主会话"}),
            ],
        );

        let mut paths = AntigravityPaths::offline(app_home.clone());
        paths.cli_home = Some(cli_home.clone());

        // 1. L0 detail enriches L1 collection, 3 inline cards (Started, Interacted, Interrupted), and subagent_updates
        let d0 = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let subs0 = d0.subagents.as_ref().unwrap();
        assert_eq!(subs0.agents.len(), 1);
        assert_eq!(subs0.agents[0].agent_id.as_deref(), Some(l1));
        assert_eq!(subs0.agents[0].name, "Explorer L1");
        assert_eq!(subs0.agents[0].status, SubagentStatus::Completed);
        let inline_kinds: Vec<_> = d0
            .events
            .iter()
            .filter_map(|e| e.subagent.as_ref().and_then(|s| s.event_kind))
            .collect();
        assert_eq!(
            inline_kinds,
            vec![
                SubagentEventKind::Started,
                SubagentEventKind::Interacted,
                SubagentEventKind::Interrupted
            ]
        );

        // 2. Dynamic refresh OUTSIDE HISTORY_CACHE: L0 transcript is untouched, L1 becomes locked & running!
        let l1_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(app_home.join("presence").join(format!("{l1}.lock")))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(l1_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let mut f_l1 = fs::OpenOptions::new()
            .append(true)
            .open(
                app_home
                    .join("brain")
                    .join(l1)
                    .join(".system_generated/logs/transcript_full.jsonl"),
            )
            .unwrap();
        writeln!(f_l1, "{}", json!({"step_index":4,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":now_iso,"content":"继续运行"})).unwrap();
        drop(f_l1);

        let d0_live = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            d0_live.subagents.as_ref().unwrap().agents[0].status,
            SubagentStatus::Running
        );
        assert!(d0_live
            .subagent_updates
            .as_ref()
            .unwrap()
            .values()
            .all(|u| u.status == SubagentStatus::Running));
        unsafe {
            let _ = libc::flock(l1_lock.as_raw_fd(), libc::LOCK_UN);
        }

        // 3. L1 subagent_detail returns direct child L2 (L0 -> L1 -> L2!), isolated prefix, and does NOT treat send_message(L0) as child subagent!
        let d1 = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: Some(l1.into()),
                limit: Some(2),
                before: None,
            },
        )
        .unwrap();
        assert_eq!(d1.agent.as_ref().unwrap().agent_id.as_deref(), Some(l1));
        assert_eq!(d1.subagents.as_ref().unwrap().agents.len(), 1);
        assert_eq!(
            d1.subagents.as_ref().unwrap().agents[0].agent_id.as_deref(),
            Some(l2)
        );
        assert!(d1.history.as_ref().unwrap().epoch.contains(':'));
        assert!(d1.before_cursor.is_some());
        let d1_older = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: Some(l1.into()),
                limit: Some(10),
                before: d1.before_cursor.clone(),
            },
        )
        .unwrap();
        for ev in d1.events.iter().chain(d1_older.events.iter()) {
            if let Some(sub) = &ev.subagent {
                assert_ne!(
                    sub.agent_id.as_deref(),
                    Some(l0),
                    "send_message to parent must never become child subagent card"
                );
            }
        }
        assert!(session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: Some(l1.into()),
                limit: Some(10),
                before: Some("badprefix:step-1".into()),
            }
        )
        .is_err());

        // 4. L2 reachable from L0 via 2-hop verified chain; unrelated root, cross-store CLI child, and symlink rejected!
        let d2 = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: Some(l2.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(d2.agent.as_ref().unwrap().agent_id.as_deref(), Some(l2));
        assert!(session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: other_root.into(),
                agent_id: Some(l1.into()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: Some(cli_child.into()),
                ..Default::default()
            }
        )
        .is_err());

        // Subagent attachment read via SessionUseCases with root_thread_id
        let d1_full = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: Some(l1.into()),
                limit: Some(50),
                before: None,
            },
        )
        .unwrap();
        let user_msg = d1_full
            .events
            .iter()
            .find_map(|e| e.user_message.as_ref())
            .unwrap();
        let att_id = user_msg.attachments[0].id.clone();
        let att_resp = super::read_attachment(
            &paths,
            &crate::user_message::SessionAttachmentRequest {
                provider: crate::services::sessions::SessionProvider::Antigravity,
                session_key: l1.into(),
                root_thread_id: Some(l0.into()),
                message_id: user_msg.id.clone(),
                attachment_id: att_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(att_resp.mime_type, "image/png");
        assert!(!att_resp.base64.is_empty());
        assert!(super::read_attachment(
            &paths,
            &crate::user_message::SessionAttachmentRequest {
                provider: crate::services::sessions::SessionProvider::Antigravity,
                session_key: l1.into(),
                root_thread_id: Some(other_root.into()),
                message_id: user_msg.id.clone(),
                attachment_id: att_id,
            }
        )
        .is_err());

        // Replace L2 brain dir with symlink -> must reject!
        let l2_brain = app_home.join("brain").join(l2);
        let moved_l2 = base.join("moved-l2");
        fs::rename(&l2_brain, &moved_l2).unwrap();
        std::os::unix::fs::symlink(&moved_l2, &l2_brain).unwrap();
        assert!(session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: Some(l2.into()),
                ..Default::default()
            }
        )
        .is_err());

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn topological_leaf_first_delete_and_unhydrated_404_cleanup() {
        let root =
            std::env::temp_dir().join(format!("agenthub-ag-topo-del-{}", uuid::Uuid::new_v4()));
        for d in ["conversations", "brain"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        // Choose UUIDs where L1 > L2 lexicographically so naive reverse-UUID order would wrongly delete L1 before L2!
        let l0 = "50000000-0000-4000-8000-000000000000";
        let l1 = "90000000-0000-4000-8000-000000000001";
        let l2 = "10000000-0000-4000-8000-000000000002";
        for cid in [l0, l1, l2] {
            fs::write(root.join("conversations").join(format!("{cid}.db")), b"db").unwrap();
            fs::create_dir_all(root.join("brain").join(cid)).unwrap();
        }
        let conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, parent_conversation_id TEXT);
             CREATE TABLE conversations_fts (conversation_id TEXT, chunk_index INTEGER, archived INTEGER, title TEXT);
             CREATE TABLE conversation_parents (conversation_id TEXT PRIMARY KEY, parent_id TEXT);"
        ).unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries VALUES (?1, ''), (?2, ?1), (?3, ?2)",
            rusqlite::params![l0, l1, l2],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_parents VALUES (?1, ?2), (?3, ?1)",
            rusqlite::params![l1, l0, l2],
        )
        .unwrap();
        drop(conn);

        let delete_order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let server_order = delete_order.clone();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            while !server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let req_json: Value = serde_json::from_slice(&body).unwrap_or_default();
                if req_line.contains("GetAllCascadeTrajectories") {
                    // Only L0 and L1 are hydrated in live server; L2 is unhydrated on disk only!
                    let resp = json!({
                        "trajectorySummaries": {
                            (l0): {"summary": "Root", "status": "CASCADE_RUN_STATUS_IDLE", "lastModifiedTime": "2026-11-24T01:00:00Z"},
                            (l1): {"summary": "Child", "status": "CASCADE_RUN_STATUS_IDLE", "parentConversationId": l0, "rootConversationId": l0}
                        }
                    });
                    let b = serde_json::to_vec(&resp).unwrap();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).unwrap();
                    stream.write_all(&b).unwrap();
                } else if req_line.contains("DeleteCascadeTrajectory") {
                    let cid = req_json["cascadeId"].as_str().unwrap_or("").to_owned();
                    server_order.lock().unwrap().push(cid.clone());
                    if cid == l2 {
                        let err = json!({"code": "not_found", "message": "trajectory not found"});
                        let b = serde_json::to_vec(&err).unwrap();
                        write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).unwrap();
                        stream.write_all(&b).unwrap();
                    } else {
                        stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                            .unwrap();
                    }
                }
            }
        });

        let paths = AntigravityPaths::with_fixed_rpc(root.clone(), port, "tok");
        let preview = preview_batch_item(&paths, l0, SessionOperation::Delete, true).unwrap();
        assert_eq!(preview.descendants, vec![l1.to_owned(), l2.to_owned()]);
        let res = execute_batch_item(
            &paths,
            l0,
            SessionOperation::Delete,
            &preview.descendants,
            true,
        )
        .unwrap();
        assert_eq!(res.status, "succeeded");
        assert!(res.residuals.is_empty());
        assert_eq!(
            *delete_order.lock().unwrap(),
            vec![l2.to_owned(), l1.to_owned(), l0.to_owned()]
        );
        for cid in [l0, l1, l2] {
            assert!(!root
                .join("conversations")
                .join(format!("{cid}.db"))
                .exists());
            assert!(!root.join("brain").join(cid).exists());
        }
        let check_conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        let l2_left: i64 = check_conn
            .query_row(
                "SELECT count(*) FROM conversation_summaries WHERE conversation_id = ?1",
                rusqlite::params![l2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            l2_left, 0,
            "unhydrated 404 child row in SQLite must also be cleaned"
        );
        drop(check_conn);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", port));
        let _ = handle.join();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn consecutive_subagent_calls_pagination_search_jump_and_unhydrated_root_or_concurrent_l3_guards(
    ) {
        use crate::antigravity::sessions::{load_all_events, rename_session, set_archived};
        use std::os::unix::io::AsRawFd;

        let root =
            std::env::temp_dir().join(format!("agenthub-ag-paged-sub-{}", uuid::Uuid::new_v4()));
        for d in ["conversations", "brain", "presence"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let l0 = "10000000-0000-4000-8000-000000000010";
        let l1 = "20000000-0000-4000-8000-000000000011";
        let l2 = "30000000-0000-4000-8000-000000000012";
        let l3 = "40000000-0000-4000-8000-000000000013";
        for cid in [l0, l1, l2] {
            fs::write(root.join("conversations").join(format!("{cid}.pb")), b"pb").unwrap();
            let dir = root.join("brain").join(cid).join(".system_generated/logs");
            fs::create_dir_all(&dir).unwrap();
        }
        // Build L0 transcript with older turn + 1 invoke_subagent + 4 consecutive send_message/manage_subagents + 5 GENERIC results
        let l0_rows = [
            json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-24T01:00:00Z","content":"第一轮历史请求"}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-24T01:00:01Z","content":"第一轮历史回复"}),
            json!({"step_index":2,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-24T01:00:02Z","content":"第二轮连续子代理交互"}),
            json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-24T01:00:03Z","tool_calls":[
                {"id":"c-spawn","name":"invoke_subagent","arguments":{"Name":"Worker L1","Task":"Batch task"}},
                {"id":"c-m1","name":"send_message","arguments":{"Recipient":l1,"Message":"Step A"}},
                {"id":"c-m2","name":"send_message","arguments":{"Recipient":l1,"Message":"Step B"}},
                {"id":"c-m3","name":"manage_subagents","arguments":{"Action":"status","ConversationIds":[l1]}},
                {"id":"c-m4","name":"manage_subagents","arguments":{"Action":"kill","ConversationIds":[l1]}}
            ]}),
            json!({"step_index":4,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:04Z","content":format!("Created the following subagent:\n{{\"conversationId\":\"{l1}\",\"marker\":\"searchable-subagent-output\"}}")}),
            json!({"step_index":5,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:05Z","content":"sent A"}),
            json!({"step_index":6,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:06Z","content":"sent B"}),
            json!({"step_index":7,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:07Z","content":"status ok"}),
            json!({"step_index":8,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:08Z","content":"killed"}),
        ];
        fs::write(
            root.join("brain")
                .join(l0)
                .join(".system_generated/logs/transcript_full.jsonl"),
            l0_rows
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(
            root.join("brain")
                .join(l1)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n",
                json!({"step_index":0,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-24T01:00:04Z","tool_calls":[{"id":"c-l2","name":"invoke_subagent","arguments":{"Name":"Leaf L2","Task":"Leaf"}}]}),
                json!({"step_index":1,"type":"GENERIC","status":"DONE","created_at":"2026-11-24T01:00:05Z","content":format!("Created the following subagent:\n{{\"conversationId\":\"{l2}\"}}")})
            ),
        )
        .unwrap();

        let offline = AntigravityPaths::offline(root.clone());

        // 1. Reverse pagination with small limit (2) across consecutive subagent tool results never yields an empty page or truncates turn 0!
        let mut collected = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = session_detail(
                &offline,
                &ProviderDetailRequest {
                    session_key: l0.into(),
                    agent_id: None,
                    limit: Some(2),
                    before: cursor.clone(),
                },
            )
            .unwrap();
            assert!(
                !page.events.is_empty(),
                "page across subagent results must never be empty"
            );
            let mut batch = page.events.clone();
            batch.extend(collected);
            collected = batch;
            if !page.has_more {
                assert!(page.before_cursor.is_none());
                break;
            }
            assert!(page.before_cursor.is_some());
            cursor = page.before_cursor;
        }
        assert_eq!(collected.len(), 8);
        assert_eq!(collected[0].text.as_deref(), Some("第一轮历史请求"));

        // 2. Searchable load_all_events IDs match jump window j:<id> without 404!
        let all_evs = load_all_events(&offline, l0).unwrap();
        let spawn_hit = all_evs
            .iter()
            .find(|e| {
                e.result
                    .as_deref()
                    .unwrap_or("")
                    .contains("searchable-subagent-output")
            })
            .expect("folded invoke_subagent event retains searchable result");
        let jumped = session_detail(
            &offline,
            &ProviderDetailRequest {
                session_key: l0.into(),
                agent_id: None,
                limit: Some(3),
                before: Some(format!("j:{}", spawn_hit.id)),
            },
        )
        .unwrap();
        assert!(jumped
            .events
            .iter()
            .any(|e| e.id == spawn_hit.id && e.subagent.is_some()));

        // 3. Offline .pb rename/archive must reject when transcript-spawned child L1 holds presence lock!
        let l1_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("presence").join(format!("{l1}.lock")))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(l1_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert!(rename_session(&offline, l0, "试图改名").is_err());
        assert!(set_archived(&offline, l0, true).is_err());
        unsafe {
            let _ = libc::flock(l1_lock.as_raw_fd(), libc::LOCK_UN);
        }

        // 4. Online Connect-RPC: concurrent running L3 blocks delete; once L3 gone, unhydrated root L0 + children returning 404 delete cleanly!
        let spawn_running_l3 = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let server_l3 = spawn_running_l3.clone();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            while !server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                if req_line.contains("GetAllCascadeTrajectories") {
                    let summaries = if server_l3.load(std::sync::atomic::Ordering::Relaxed) {
                        json!({
                            (l3): {"summary": "Concurrent L3", "status": "CASCADE_RUN_STATUS_RUNNING", "parentConversationId": l2}
                        })
                    } else {
                        json!({})
                    };
                    let b = serde_json::to_vec(&json!({"trajectorySummaries": summaries})).unwrap();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).unwrap();
                    stream.write_all(&b).unwrap();
                } else if req_line.contains("DeleteCascadeTrajectory") {
                    let b = serde_json::to_vec(
                        &json!({"code": "not_found", "message": "trajectory not found"}),
                    )
                    .unwrap();
                    write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).unwrap();
                    stream.write_all(&b).unwrap();
                }
            }
        });

        let online = AntigravityPaths::with_fixed_rpc(root.clone(), port, "tok");
        // Concurrent running L3 spawned under unhydrated L2 must block execute_batch_item even though confirmed set was [L1, L2]!
        assert!(execute_batch_item(
            &online,
            l0,
            SessionOperation::Delete,
            &[l1.into(), l2.into()],
            true
        )
        .is_err());
        assert!(root.join("conversations").join(format!("{l2}.pb")).exists());

        // 4b. Unhydrated session holding presence lock must NOT have its disk files deleted on 404 fallback!
        spawn_running_l3.store(false, std::sync::atomic::Ordering::Relaxed);
        let l2_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("presence").join(format!("{l2}.lock")))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(l2_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let locked_404 = execute_batch_item(
            &online,
            l0,
            SessionOperation::Delete,
            &[l1.into(), l2.into()],
            true,
        )
        .unwrap();
        assert_eq!(locked_404.status, "failed");
        assert!(root.join("conversations").join(format!("{l2}.pb")).exists());
        unsafe {
            let _ = libc::flock(l2_lock.as_raw_fd(), libc::LOCK_UN);
        }

        // Once L3 is gone and L2 unlocked, unhydrated L0 + L1 + L2 all returning 404 not_found are cleanly deleted from disk!
        let ok = execute_batch_item(
            &online,
            l0,
            SessionOperation::Delete,
            &[l1.into(), l2.into()],
            true,
        )
        .unwrap();
        assert_eq!(ok.status, "succeeded");
        assert!(ok.residuals.is_empty());
        for cid in [l0, l1, l2] {
            assert!(!root
                .join("conversations")
                .join(format!("{cid}.pb"))
                .exists());
            assert!(!root.join("brain").join(cid).exists());
        }

        // 5. Offline indexed SQLite store with unlocked root + locked active child whose parent_conversation_id is empty (linked via transcript) reports root "running"!
        let sq_root =
            std::env::temp_dir().join(format!("agenthub-ag-unparented-{}", uuid::Uuid::new_v4()));
        for d in ["conversations", "brain", "presence"] {
            fs::create_dir_all(sq_root.join(d)).unwrap();
        }
        let r_id = "60000000-0000-4000-8000-000000000016";
        let c_id = "70000000-0000-4000-8000-000000000017";
        fs::write(
            sq_root.join("conversations").join(format!("{r_id}.pb")),
            b"pb",
        )
        .unwrap();
        let c_db =
            Connection::open(sq_root.join("conversations").join(format!("{c_id}.db"))).unwrap();
        c_db.execute_batch("CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB); INSERT INTO steps VALUES (0, 15, 8, X'');").unwrap();
        drop(c_db);
        let r_logs = sq_root
            .join("brain")
            .join(r_id)
            .join(".system_generated/logs");
        fs::create_dir_all(&r_logs).unwrap();
        fs::write(
            r_logs.join("transcript_full.jsonl"),
            format!("{}\n", json!({"step_index":0,"type":"GENERIC","status":"DONE","content":format!("Created the following subagent:\n{{\"conversationId\":\"{c_id}\"}}")})),
        ).unwrap();
        let s_db = Connection::open(sq_root.join("conversation_summaries.db")).unwrap();
        s_db.execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, title TEXT, preview TEXT, step_count INTEGER, last_modified_time TEXT, workspace_uris TEXT, status TEXT, parent_conversation_id TEXT, nesting_depth INTEGER, not_fully_idle NUMERIC);",
        ).unwrap();
        s_db.execute(
            "INSERT INTO conversation_summaries VALUES (?1, 'Root', '', 1, '2026-11-24T01:00:00Z', '', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0), (?2, 'Child', '', 1, '2026-11-24T01:00:01Z', '', 'CASCADE_RUN_STATUS_IDLE', '', 1, 0)",
            rusqlite::params![r_id, c_id],
        ).unwrap();
        drop(s_db);
        let c_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(sq_root.join("presence").join(format!("{c_id}.lock")))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(c_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let sq_list = list_sessions(
            &AntigravityPaths::offline(sq_root.clone()),
            &ProviderListRequest::default(),
        )
        .unwrap();
        assert_eq!(sq_list.len(), 1);
        assert_eq!(sq_list[0].id, r_id);
        assert_eq!(sq_list[0].status, "running");
        unsafe {
            let _ = libc::flock(c_lock.as_raw_fd(), libc::LOCK_UN);
        }
        let _ = fs::remove_dir_all(sq_root);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", port));
        let _ = handle.join();
        let _ = fs::remove_dir_all(root);
    }
}
