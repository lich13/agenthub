//! Local-only, allowlisted access to the already running Antigravity App.
use agenthub_core::{
    antigravity,
    native_probe::{NativeScan, NativeStreamSnapshot},
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};

const SERVICE: &str = "/exa.language_server_pb.LanguageServerService/";
const LIMIT: usize = agenthub_core::resource_limits::ANTIGRAVITY_LOCAL_RPC_BYTES;
#[derive(Clone, Debug, PartialEq, Eq)]
struct SlimExecMeta {
    execution_id: String,
    last_step_idx: Option<u64>,
    termination_reason: String,
    proceeded_with_auto_continue: bool,
}

impl SlimExecMeta {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            execution_id: value["executionId"].as_str()?.to_owned(),
            last_step_idx: value["lastStepIdx"].as_u64(),
            termination_reason: value["terminationReason"].as_str().unwrap_or("").to_owned(),
            proceeded_with_auto_continue: value["proceededWithAutoContinue"].as_bool()
                == Some(true),
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "executionId": self.execution_id,
            "lastStepIdx": self.last_step_idx,
            "terminationReason": self.termination_reason,
            "proceededWithAutoContinue": self.proceeded_with_auto_continue,
        })
    }
}

type SnapshotCacheEntry = (String, String, NativeStreamSnapshot, std::time::Instant);
type MetaCacheEntry = (String, String, Vec<SlimExecMeta>, std::time::Instant);

static CACHE: tokio::sync::Mutex<VecDeque<SnapshotCacheEntry>> =
    tokio::sync::Mutex::const_new(VecDeque::new());
static META_CACHE: tokio::sync::Mutex<VecDeque<MetaCacheEntry>> =
    tokio::sync::Mutex::const_new(VecDeque::new());

pub async fn evict_idle_cache(max_idle: Duration) {
    let now = std::time::Instant::now();
    {
        let mut cache = CACHE.lock().await;
        cache.retain(|(_, _, _, touched)| now.saturating_duration_since(*touched) < max_idle);
    }
    {
        let mut meta_cache = META_CACHE.lock().await;
        meta_cache.retain(|(_, _, _, touched)| now.saturating_duration_since(*touched) < max_idle);
    }
}
type Endpoint = antigravity::sessions::DiscoveredAntigravityEndpoint;

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?)
}
async fn query(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    method: &str,
    body: Value,
) -> Result<Value> {
    ensure!(
        matches!(
            method,
            "GetAllCascadeTrajectories"
                | "GetCascadeTrajectory"
                | "GetCascadeTrajectorySteps"
                | "GetCascadeTrajectoryExecutorMetadatas"
        ),
        "unsupported Antigravity read method"
    );
    let url = format!("http://127.0.0.1:{port}{SERVICE}{method}");
    let mut response = client
        .post(url)
        .header("x-codeium-csrf-token", token)
        .header("Connect-Protocol-Version", "1")
        .json(&body)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Antigravity read unavailable"))?;
    ensure!(
        response.status().is_success()
            && response.content_length().is_none_or(|n| n <= LIMIT as u64),
        "Antigravity response unavailable"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Antigravity read unavailable"))?
    {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= LIMIT,
            "Antigravity response exceeds limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid Antigravity response"))
}
async fn connect() -> Result<(Endpoint, reqwest::Client, u16, Value)> {
    if !cfg!(target_os = "macos") {
        anyhow::bail!("Antigravity notifications require the local Mac App");
    }
    let client = client()?;
    match antigravity::sessions::get_cached_live_endpoint() {
        Some(Ok(endpoint)) => {
            for &port in &endpoint.ports {
                if let Ok(value) = query(
                    &client,
                    port,
                    &endpoint.token,
                    "GetAllCascadeTrajectories",
                    json!({"excludeSubtrajectories":false}),
                )
                .await
                {
                    if value["trajectorySummaries"].is_object() {
                        return Ok((endpoint, client, port, value));
                    }
                }
            }
        }
        Some(Err(())) => {
            anyhow::bail!("Antigravity native API unavailable");
        }
        None => {}
    }
    let candidates =
        tokio::task::spawn_blocking(antigravity::sessions::discover_host_endpoints).await?;
    if candidates.is_empty() {
        antigravity::sessions::record_endpoint_failure(false);
        anyhow::bail!("Antigravity native owner unavailable or ambiguous");
    }
    if candidates.len() > 1 {
        antigravity::sessions::record_endpoint_failure(true);
        anyhow::bail!("Antigravity native owner unavailable or ambiguous");
    }
    let endpoint = candidates.into_iter().next().unwrap();
    for &port in &endpoint.ports {
        if let Ok(value) = query(
            &client,
            port,
            &endpoint.token,
            "GetAllCascadeTrajectories",
            json!({"excludeSubtrajectories":false}),
        )
        .await
        {
            if value["trajectorySummaries"].is_object() {
                antigravity::sessions::record_verified_live_endpoint(
                    endpoint.pid,
                    port,
                    endpoint.token.clone(),
                );
                return Ok((endpoint, client, port, value));
            }
        }
    }
    antigravity::sessions::record_endpoint_failure(true);
    anyhow::bail!("Antigravity native API unavailable")
}

use antigravity::sessions::{
    collect_subagents_by_root, decode_file_uri, is_subtrajectory,
    live_item_is_running as summary_is_active,
};

fn decode_local_file_uri(uri: &str) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(decode_file_uri(uri));
    path.is_absolute().then_some(path)
}

fn has_open_file_handle(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        let bin = if std::path::Path::new("/usr/sbin/lsof").is_file() {
            "/usr/sbin/lsof"
        } else {
            "lsof"
        };
        std::process::Command::new(bin)
            .args(["-Fp", "--"])
            .arg(path)
            .output()
            .is_ok_and(|out| out.status.success() && out.stdout.contains(&b'p'))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

fn inspect_task_log(log_uri: &str) -> Option<antigravity::TaskLiveness> {
    let path = decode_local_file_uri(log_uri)?;
    let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
    let last_activity_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);
    Some(antigravity::TaskLiveness {
        active: has_open_file_handle(&path),
        last_activity_ms,
    })
}

fn parse_ps_etime_ms(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (days, rest) = match trimmed.split_once('-') {
        Some((d, r)) => (d.parse::<i64>().ok()?, r),
        None => (0, trimmed),
    };
    let parts: Vec<i64> = rest
        .split(':')
        .map(|p| p.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let (hours, mins, secs) = match parts.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if days < 0 || hours < 0 || !(0..60).contains(&mins) || !(0..60).contains(&secs) {
        return None;
    }
    let total_secs = days
        .checked_mul(86_400)?
        .checked_add(hours.checked_mul(3_600)?)?
        .checked_add(mins.checked_mul(60)?)?
        .checked_add(secs)?;
    total_secs.checked_mul(1_000)
}

fn read_ps_process_table() -> Option<String> {
    #[cfg(unix)]
    {
        let bin = if std::path::Path::new("/bin/ps").is_file() {
            "/bin/ps"
        } else {
            "ps"
        };
        let out = std::process::Command::new(bin)
            .args(["-ww", "-axo", "pid=,ppid=,etime=,command="])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).ok())
            .flatten()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn normalize_ws(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn command_looks_like_daemon(norm_cmd: &str) -> bool {
    let lower = norm_cmd.to_ascii_lowercase();
    lower.starts_with("nohup ")
        || lower.contains(" nohup ")
        || lower.contains(" --daemon")
        || lower.ends_with(" &")
}

fn process_command_matches_spec(proc_cmd: &str, spec_cmd: &str) -> bool {
    let norm_proc = normalize_ws(proc_cmd);
    let norm_spec = normalize_ws(spec_cmd);
    if norm_proc.is_empty() || norm_spec.is_empty() || command_looks_like_daemon(&norm_proc) {
        return false;
    }
    if norm_proc.contains(&norm_spec) {
        return true;
    }
    // Compound shell pipelines (`cd dir && cargo test -p foo`) spawn subcommands as descendants.
    norm_spec
        .split(['&', ';', '|', '\n'])
        .map(normalize_ws)
        .filter(|seg| seg.len() >= 6 && !seg.starts_with("cd "))
        .any(|seg| norm_proc.contains(&seg))
}

fn ps_table_matches_untracked_commands(
    table: &str,
    root_pid: u32,
    specs: &[antigravity::UntrackedCommandSpec],
    now_ms: i64,
) -> bool {
    if root_pid == 0 || specs.is_empty() {
        return false;
    }
    const MAX_COMMAND_AGE_MS: i64 = 4 * 3_600 * 1_000;
    const EARLY_CLOCK_SLACK_MS: i64 = 5_000;
    const LATE_SPAWN_WINDOW_MS: i64 = 15_000;

    let mut rows: Vec<(u32, u32, i64, &str)> = Vec::new();
    for line in table.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let Some((pid_str, rest)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let Some(pid) = pid_str.parse::<u32>().ok() else {
            continue;
        };
        let rest = rest.trim_start();
        let Some((ppid_str, rest)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let Some(ppid) = ppid_str.parse::<u32>().ok() else {
            continue;
        };
        let rest = rest.trim_start();
        let Some((etime_str, cmd)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let Some(elapsed_ms) = parse_ps_etime_ms(etime_str) else {
            continue;
        };
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            rows.push((pid, ppid, elapsed_ms, cmd));
        }
    }

    let mut descendants = BTreeSet::new();
    let mut frontier = BTreeSet::from([root_pid]);
    for _ in 0..16 {
        let mut next_frontier = BTreeSet::new();
        for &(pid, ppid, _, _) in &rows {
            if pid != root_pid && frontier.contains(&ppid) && descendants.insert(pid) {
                next_frontier.insert(pid);
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }
    if descendants.is_empty() {
        return false;
    }

    rows.into_iter().any(|(pid, _, elapsed_ms, cmd)| {
        if !descendants.contains(&pid) || elapsed_ms > MAX_COMMAND_AGE_MS {
            return false;
        }
        let start_ms = now_ms.saturating_sub(elapsed_ms);
        specs.iter().any(|spec| {
            spec.spawn_ms > 0
                && start_ms >= spec.spawn_ms.saturating_sub(EARLY_CLOCK_SLACK_MS)
                && start_ms <= spec.spawn_ms.saturating_add(LATE_SPAWN_WINDOW_MS)
                && process_command_matches_spec(cmd, &spec.command_line)
        })
    })
}

fn has_matching_active_child_process(
    root_pid: u32,
    specs: &[antigravity::UntrackedCommandSpec],
    now_ms: i64,
) -> bool {
    if root_pid == 0 || specs.is_empty() {
        return false;
    }
    let Some(table) = read_ps_process_table() else {
        return false;
    };
    ps_table_matches_untracked_commands(&table, root_pid, specs, now_ms)
}

async fn fetch_incremental_executor_metadatas(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    id: &str,
    trajectory_id: &str,
) -> Result<Option<Vec<Value>>> {
    let mut cached: Vec<SlimExecMeta> = {
        let cache = META_CACHE.lock().await;
        cache
            .iter()
            .find(|(cid, tid, _, _)| cid == id && tid == trajectory_id)
            .map(|(_, _, metas, _)| metas.clone())
            .unwrap_or_default()
    };
    let mut start_offset = cached.len().saturating_sub(1);
    let mut retried_from_zero = false;

    loop {
        cached.truncate(start_offset);
        let mut cursor = start_offset;
        let mut diverged = false;
        loop {
            if cached.len() >= 20_000 {
                return Ok(None);
            }
            let page = query(
                client,
                port,
                token,
                "GetCascadeTrajectoryExecutorMetadatas",
                json!({"cascadeId": id, "executorMetadataOffset": cursor}),
            )
            .await?;
            let Some(batch) = page["executorMetadata"]
                .as_array()
                .or_else(|| page["executorMetadatas"].as_array())
                .filter(|b| !b.is_empty())
            else {
                if cursor == start_offset && start_offset > 0 && !retried_from_zero {
                    diverged = true;
                }
                break;
            };
            let mut parsed_batch = Vec::with_capacity(batch.len());
            for raw in batch {
                let Some(slim) = SlimExecMeta::from_value(raw) else {
                    return Ok(None);
                };
                parsed_batch.push(slim);
            }
            if cursor == start_offset && start_offset > 0 && !retried_from_zero {
                // Verify overlap continuity when possible by ensuring no duplicate pre-anchor ID.
                if cached
                    .last()
                    .is_some_and(|prev| prev.execution_id == parsed_batch[0].execution_id)
                {
                    diverged = true;
                    break;
                }
            }
            cursor += parsed_batch.len();
            cached.extend(parsed_batch);
        }
        if diverged {
            retried_from_zero = true;
            start_offset = 0;
            cached.clear();
            continue;
        }
        break;
    }

    let serialized: Vec<Value> = cached.iter().map(SlimExecMeta::to_value).collect();
    {
        let mut cache = META_CACHE.lock().await;
        cache.retain(|(cid, _, _, _)| cid != id);
        cache.push_back((
            id.to_owned(),
            trajectory_id.to_owned(),
            cached,
            std::time::Instant::now(),
        ));
        while cache.len() > 32 || cache.iter().map(|(_, _, m, _)| m.len()).sum::<usize>() > 32_768 {
            cache.pop_front();
        }
    }
    Ok(Some(serialized))
}

async fn fetch_incremental_turn_trajectory(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    id: &str,
    summary: &Value,
) -> Result<Option<Value>> {
    let Some(total) = summary["stepCount"]
        .as_u64()
        .filter(|&n| n > 0 && n <= 200_000)
    else {
        return Ok(None);
    };
    let Some(turn_offset) = summary["lastUserInputStepIndex"]
        .as_u64()
        .filter(|&off| off < total)
    else {
        return Ok(None);
    };
    let cached_unsettled_offset = {
        let cache = CACHE.lock().await;
        cache
            .iter()
            .find(|(key, _, _, _)| key == id)
            .map(|(_, _, s, _)| s.settled_count)
            .filter(|&settled| settled < turn_offset && turn_offset - settled <= 256)
    };
    let start_offset = cached_unsettled_offset.unwrap_or(turn_offset);
    let total_usize = total as usize;
    let offset_usize = start_offset as usize;
    let mut steps = Vec::new();
    while offset_usize + steps.len() < total_usize && steps.len() < 50_000 {
        let more = query(
            client,
            port,
            token,
            "GetCascadeTrajectorySteps",
            json!({"cascadeId": id, "stepOffset": offset_usize + steps.len()}),
        )
        .await?;
        let Some(batch) = more["steps"].as_array().filter(|b| !b.is_empty()) else {
            break;
        };
        steps.extend(batch.iter().cloned());
    }
    if offset_usize + steps.len() != total_usize {
        return Ok(None);
    }
    let trajectory_id = summary["trajectoryId"].as_str().unwrap_or("");
    let Some(metas) =
        fetch_incremental_executor_metadatas(client, port, token, id, trajectory_id).await?
    else {
        return Ok(None);
    };
    Ok(Some(json!({
        "status": summary["status"],
        "stepOffset": start_offset,
        "numTotalSteps": total,
        "trajectory": {
            "cascadeId": id,
            "trajectoryId": summary["trajectoryId"],
            "steps": steps,
            "executorMetadatas": metas,
        }
    })))
}

async fn fetch_probe_trajectory(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    id: &str,
    summary: &Value,
) -> Result<Value> {
    if let Ok(Some(incremental)) =
        fetch_incremental_turn_trajectory(client, port, token, id, summary).await
    {
        return Ok(incremental);
    }
    fetch_complete_trajectory(client, port, token, id).await
}

async fn fetch_complete_trajectory(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    id: &str,
) -> Result<Value> {
    let mut response = query(
        client,
        port,
        token,
        "GetCascadeTrajectory",
        json!({"cascadeId": id, "disableRehydration": true}),
    )
    .await?;
    let total = response["numTotalSteps"].as_u64().unwrap_or_else(|| {
        response["trajectory"]["steps"]
            .as_array()
            .map_or(0, Vec::len) as u64
    }) as usize;
    let mut current_len = response["trajectory"]["steps"]
        .as_array()
        .map_or(0, Vec::len);
    while current_len < total && current_len < 50_000 {
        let more = query(
            client,
            port,
            token,
            "GetCascadeTrajectorySteps",
            json!({"cascadeId": id, "stepOffset": current_len}),
        )
        .await?;
        let Some(batch) = more["steps"].as_array().filter(|b| !b.is_empty()) else {
            break;
        };
        if let Some(steps) = response["trajectory"]["steps"].as_array_mut() {
            steps.extend(batch.iter().cloned());
            current_len = steps.len();
        } else {
            break;
        }
    }
    Ok(response)
}
pub async fn scan() -> Result<NativeScan> {
    let (endpoint, client, port, summaries) = connect().await?;
    scan_at(
        &client,
        port,
        &endpoint.token,
        endpoint.pid,
        &summaries,
        None,
    )
    .await
}

pub async fn scan_one(cascade_id: &str) -> Result<NativeScan> {
    let (endpoint, client, port, mut summaries) = connect().await?;
    let mut last_scan = NativeScan::default();
    for attempt in 0..4 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            summaries = query(
                &client,
                port,
                &endpoint.token,
                "GetAllCascadeTrajectories",
                json!({"excludeSubtrajectories": false}),
            )
            .await?;
        }
        let scan = scan_at(
            &client,
            port,
            &endpoint.token,
            endpoint.pid,
            &summaries,
            Some(cascade_id),
        )
        .await?;
        let settled = scan
            .streams
            .first()
            .is_some_and(|s| !s.events.is_empty() || s.settled_count == s.record_count);
        last_scan = scan;
        if settled {
            break;
        }
    }
    Ok(last_scan)
}

async fn load_trajectory_with_context(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    pid: u32,
    id: &str,
    summary: &Value,
    subagents: BTreeMap<String, antigravity::SubagentLiveness>,
    now_ms: i64,
    inspect_background: bool,
) -> Result<(Value, antigravity::TrajectoryLivenessContext)> {
    let has_active_subagents = subagents.values().any(|s| s.active);
    let response = fetch_probe_trajectory(client, port, token, id, summary).await?;
    let mut tasks = BTreeMap::new();
    let mut has_active_child_processes = false;
    if inspect_background {
        if let Some(steps) = response["trajectory"]["steps"].as_array() {
            let summary_traj_id = summary["trajectoryId"].as_str().unwrap_or("");
            let pending_logs = antigravity::collect_pending_task_logs(id, summary_traj_id, steps);
            if !pending_logs.is_empty() {
                tasks = tokio::task::spawn_blocking(move || {
                    pending_logs
                        .into_iter()
                        .filter_map(|(tid, uri)| inspect_task_log(&uri).map(|l| (tid, l)))
                        .collect()
                })
                .await
                .unwrap_or_default();
            }
            let untracked_specs =
                antigravity::collect_untracked_task_commands(id, summary_traj_id, steps, &tasks);
            if !untracked_specs.is_empty() {
                has_active_child_processes = tokio::task::spawn_blocking(move || {
                    has_matching_active_child_process(pid, &untracked_specs, now_ms)
                })
                .await
                .unwrap_or(false);
            }
        }
    }
    let ctx = antigravity::TrajectoryLivenessContext {
        has_active_subagents,
        has_active_child_processes,
        now_ms: Some(now_ms),
        subagents,
        tasks,
    };
    Ok((response, ctx))
}

pub async fn diagnose(cascade_id: &str, db: &agenthub_core::db::PanelDb) -> Result<Value> {
    let (endpoint, client, port, value) = connect().await?;
    let summaries = value["trajectorySummaries"]
        .as_object()
        .context("missing Antigravity summaries")?;
    let summary = summaries
        .get(cascade_id)
        .with_context(|| format!("missing Antigravity summary for {cascade_id}"))?;
    let subagents = collect_subagents_by_root(summaries)
        .get(cascade_id)
        .cloned()
        .unwrap_or_default();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let (response, ctx) = load_trajectory_with_context(
        &client,
        port,
        &endpoint.token,
        endpoint.pid,
        cascade_id,
        summary,
        subagents,
        now_ms,
        true,
    )
    .await?;
    let db_cursor = db.native_probe_stream_cursor(
        agenthub_core::native_probe::NativeProvider::Antigravity,
        cascade_id,
    )?;
    antigravity::diagnose_trajectory(cascade_id, summary, &response, &ctx, db_cursor)
}
pub async fn source_current(delivery: &agenthub_core::db::NativeDelivery) -> Result<bool> {
    let (endpoint, client, port, summaries) = connect().await?;
    let scan = scan_at(
        &client,
        port,
        &endpoint.token,
        endpoint.pid,
        &summaries,
        Some(&delivery.thread_id),
    )
    .await?;
    super::provider_monitor::local_event_current(&scan, delivery)
}
async fn scan_at(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    pid: u32,
    value: &Value,
    only: Option<&str>,
) -> Result<NativeScan> {
    let summaries = value["trajectorySummaries"]
        .as_object()
        .context("missing Antigravity summaries")?;
    ensure!(
        summaries.len() <= 5000,
        "Antigravity source inventory exceeds limit"
    );
    let subagents_by_root = collect_subagents_by_root(summaries);
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut scan = NativeScan::default();
    for (id, summary) in summaries {
        if is_subtrajectory(id, summary) {
            continue;
        }
        if only.is_some_and(|target| target != id) {
            continue;
        }
        let subagents = subagents_by_root.get(id).cloned().unwrap_or_default();
        let has_active_subagents = subagents.values().any(|s| s.active);
        let signature = hex::encode(Sha256::digest(serde_json::to_vec(&json!([
            pid,
            summary,
            has_active_subagents
        ]))?));
        let is_active = summary_is_active(summary) || has_active_subagents;
        // Revalidation (`only.is_some()`) always obtains fresh native terminal / interaction state.
        // Settled idle streams (including idle trailing questions / awaiting /plan) reuse CACHE until
        // `summary` or subagent liveness changes `signature`.
        let cached = if only.is_none() && !is_active {
            let mut cache = CACHE.lock().await;
            cache
                .iter_mut()
                .find(|(key, sig, s, _)| {
                    key == id
                        && sig == &signature
                        && s.record_count == s.settled_count
                        && !s.events.iter().any(|e| e.kind == "authorization")
                })
                .map(|(_, _, s, touched)| {
                    *touched = std::time::Instant::now();
                    s.clone()
                })
        } else {
            None
        };
        if let Some(snapshot) = cached {
            scan.streams.push(snapshot);
            continue;
        }
        if antigravity::identity(id, summary).is_err() {
            scan.errors += 1;
            continue;
        }
        let result = load_trajectory_with_context(
            client, port, token, pid, id, summary, subagents, now_ms, !is_active,
        )
        .await
        .and_then(|(response, ctx)| {
            antigravity::snapshot_with_context(id, summary, &response, &ctx)
        });
        match result {
            Ok(snapshot) => {
                let mut cache = CACHE.lock().await;
                cache.retain(|(key, _, _, _)| key != id);
                cache.push_back((
                    id.clone(),
                    signature,
                    snapshot.clone(),
                    std::time::Instant::now(),
                ));
                while cache.len() > 64
                    || cache
                        .iter()
                        .map(|(_, _, s, _)| {
                            s.events
                                .iter()
                                .map(|e| e.body.len() + e.turn_id.len() + 256)
                                .sum::<usize>()
                        })
                        .sum::<usize>()
                        > 32 * 1024 * 1024
                {
                    cache.pop_front();
                }
                scan.streams.push(snapshot);
            }
            Err(_) => scan.errors += 1,
        }
    }
    Ok(scan)
}

#[cfg(test)]
mod tests;
