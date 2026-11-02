use std::collections::{HashMap, HashSet};

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};

use crate::{
    codex::{MessageBlock, ThreadDetail, ThreadStatus, ThreadSummary},
    db::JobRecord,
    platform::PlatformPaths,
    services::system::{require_capability, Capability},
};

pub use crate::services::cleanup::{
    plan_cleanup_action as plan_thread_cleanup_action, CleanupAction as ThreadCleanupAction,
    CleanupActionPlan as ThreadCleanupPlan, CleanupTarget as ThreadCleanupTarget,
};

pub const THREAD_DETAIL_DEFAULT_BLOCK_LIMIT: usize = 120;
pub const THREAD_DETAIL_MAX_BLOCK_LIMIT: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    pub q: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadsOverview {
    pub threads: Vec<ThreadSummary>,
    pub query: ThreadsQuery,
    pub fetch_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListPlan {
    pub required_capability: Capability,
    pub query: ThreadsQuery,
    pub fetch_limit: usize,
    pub response_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListReadPlan {
    pub list: ThreadListPlan,
    pub include_hidden_thread_ids: bool,
    pub include_archived_thread_ids: bool,
    pub include_running_jobs: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetailRequest {
    pub id: String,
    pub limit: Option<usize>,
    pub full: Option<bool>,
    pub before: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetailPlan {
    pub required_capability: Capability,
    pub thread_id: String,
    pub block_limit: Option<usize>,
    pub full: bool,
    pub before: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadDetailResponseKind {
    Detail,
    Blocks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetailReadPlan {
    pub detail: ThreadDetailPlan,
    pub response_kind: ThreadDetailResponseKind,
    pub include_active_job: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadBlocksPage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<crate::jsonl_cache::HistoryMetadata>,
    pub thread_id: String,
    pub blocks: Vec<MessageBlock>,
    pub total_blocks: usize,
    pub has_more_blocks: bool,
    pub before_cursor: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ThreadReadModelInputs {
    pub threads: Vec<ThreadSummary>,
    pub running_jobs: Vec<JobRecord>,
    pub hidden_thread_ids: HashSet<String>,
    pub archived_thread_ids: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListReadModelView {
    pub overview: ThreadsOverview,
    pub threads: Vec<ThreadSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetailReadModelView {
    pub detail: ThreadDetail,
}

#[derive(Debug, Clone, Copy)]
pub struct ThreadListRuntimeState<'a> {
    pub running_jobs: &'a [JobRecord],
    pub hidden_thread_ids: &'a HashSet<String>,
    pub archived_thread_ids: &'a HashSet<String>,
}

pub fn build_threads_overview(
    threads: Vec<ThreadSummary>,
    running_jobs: Vec<JobRecord>,
    query: ThreadsQuery,
    hidden_thread_ids: &HashSet<String>,
    archived_thread_ids: &HashSet<String>,
) -> ThreadsOverview {
    let response_limit = requested_thread_limit(query.limit);
    let fetch_limit = thread_list_fetch_limit(query.limit);
    let threads = apply_thread_list_runtime_state(
        threads,
        ThreadListRuntimeState {
            running_jobs: &running_jobs,
            hidden_thread_ids,
            archived_thread_ids,
        },
    );
    let threads = filter_thread_summaries(
        threads,
        query.archived.unwrap_or(false),
        query.q.as_deref(),
        response_limit,
    );

    ThreadsOverview {
        threads,
        query,
        fetch_limit,
    }
}

pub fn thread_list_read_model(
    platform: &PlatformPaths,
    inputs: ThreadReadModelInputs,
    query: ThreadsQuery,
) -> anyhow::Result<ThreadListReadModelView> {
    require_capability(platform, Capability::Threads)?;
    let overview = build_threads_overview(
        inputs.threads,
        inputs.running_jobs,
        query,
        &inputs.hidden_thread_ids,
        &inputs.archived_thread_ids,
    );
    Ok(ThreadListReadModelView {
        threads: overview.threads.clone(),
        overview,
    })
}

pub fn thread_detail_read_model(
    platform: &PlatformPaths,
    detail: ThreadDetail,
    active_job: Option<JobRecord>,
) -> anyhow::Result<ThreadDetailReadModelView> {
    require_capability(platform, Capability::Threads)?;
    let detail = apply_thread_detail_runtime_state(detail, active_job.as_ref());
    Ok(ThreadDetailReadModelView { detail })
}

pub fn plan_threads_list_request(
    platform: &PlatformPaths,
    mut query: ThreadsQuery,
) -> anyhow::Result<ThreadListPlan> {
    require_capability(platform, Capability::Threads)?;
    query.q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let response_limit = requested_thread_limit(query.limit);
    let fetch_limit = thread_list_fetch_limit(query.limit);
    Ok(ThreadListPlan {
        required_capability: Capability::Threads,
        query,
        fetch_limit,
        response_limit,
    })
}

pub fn plan_thread_list_read(
    platform: &PlatformPaths,
    query: ThreadsQuery,
) -> anyhow::Result<ThreadListReadPlan> {
    Ok(ThreadListReadPlan {
        list: plan_threads_list_request(platform, query)?,
        include_hidden_thread_ids: true,
        include_archived_thread_ids: true,
        include_running_jobs: true,
    })
}

pub fn plan_thread_detail_request(
    platform: &PlatformPaths,
    request: ThreadDetailRequest,
) -> anyhow::Result<ThreadDetailPlan> {
    require_capability(platform, Capability::Threads)?;
    let thread_id = required_thread_id(&request.id)?;
    let full = request.full.unwrap_or(false);
    Ok(ThreadDetailPlan {
        required_capability: Capability::Threads,
        thread_id,
        block_limit: detail_block_limit(request.limit, Some(full)),
        full,
        before: request.before,
    })
}

pub fn plan_thread_detail_read(
    platform: &PlatformPaths,
    request: ThreadDetailRequest,
) -> anyhow::Result<ThreadDetailReadPlan> {
    Ok(ThreadDetailReadPlan {
        detail: plan_thread_detail_request(platform, request)?,
        response_kind: ThreadDetailResponseKind::Detail,
        include_active_job: true,
    })
}

pub fn plan_thread_blocks_request(
    platform: &PlatformPaths,
    id: &str,
    limit: Option<usize>,
    before: Option<String>,
) -> anyhow::Result<ThreadDetailPlan> {
    plan_thread_detail_request(
        platform,
        ThreadDetailRequest {
            id: id.to_string(),
            limit: Some(block_page_limit(limit)),
            full: Some(false),
            before,
        },
    )
}

pub fn plan_thread_blocks_read(
    platform: &PlatformPaths,
    id: &str,
    limit: Option<usize>,
    before: Option<String>,
) -> anyhow::Result<ThreadDetailReadPlan> {
    Ok(ThreadDetailReadPlan {
        detail: plan_thread_blocks_request(platform, id, limit, before)?,
        response_kind: ThreadDetailResponseKind::Blocks,
        include_active_job: true,
    })
}

pub fn window_thread_detail_for_plan(
    detail: ThreadDetail,
    plan: &ThreadDetailPlan,
) -> ThreadDetail {
    crate::codex::window_thread_detail(detail, plan.block_limit, plan.before.as_deref())
}

pub fn thread_blocks_page_for_plan(
    detail: ThreadDetail,
    plan: &ThreadDetailPlan,
) -> ThreadBlocksPage {
    let window = window_thread_detail_for_plan(detail, plan);
    ThreadBlocksPage {
        history: window.history,
        thread_id: plan.thread_id.clone(),
        blocks: window.blocks,
        total_blocks: window.total_blocks,
        has_more_blocks: window.has_more_blocks,
        before_cursor: window.before_cursor,
    }
}

pub fn apply_thread_list_runtime_state(
    threads: Vec<ThreadSummary>,
    runtime: ThreadListRuntimeState<'_>,
) -> Vec<ThreadSummary> {
    let mut threads = prune_hidden_thread_summaries(threads, runtime.hidden_thread_ids);
    merge_running_jobs(
        &mut threads,
        runtime.running_jobs,
        runtime.archived_thread_ids,
    );
    prune_hidden_thread_summaries(threads, runtime.hidden_thread_ids)
}

pub fn apply_thread_detail_runtime_state(
    mut detail: ThreadDetail,
    active_job: Option<&JobRecord>,
) -> ThreadDetail {
    if let Some(job) = active_job {
        if job
            .thread_id
            .as_deref()
            .is_some_and(|thread_id| thread_id == detail.summary.id)
        {
            apply_running_job_to_summary(&mut detail.summary, job);
        }
    }
    detail
}

pub fn merge_running_jobs(
    threads: &mut Vec<ThreadSummary>,
    running_jobs: &[JobRecord],
    archived_thread_ids: &HashSet<String>,
) {
    let mut by_thread: HashMap<&str, &JobRecord> = HashMap::new();
    for job in running_jobs {
        if let Some(thread_id) = job.thread_id.as_deref() {
            by_thread.entry(thread_id).or_insert(job);
        }
    }

    for thread in threads.iter_mut() {
        if let Some(job) = by_thread.get(thread.id.as_str()) {
            apply_running_job_to_summary(thread, job);
        }
    }

    for job in by_thread.values() {
        apply_running_job_to_thread_list(threads, job, archived_thread_ids);
    }
}

pub fn apply_running_job_to_summary(summary: &mut ThreadSummary, job: &JobRecord) {
    if matches!(summary.status, ThreadStatus::Archived) {
        return;
    }
    summary.status = ThreadStatus::Running;
    summary.active_job_id = Some(job.id.clone());
    if summary.active_turn_id.is_none() {
        summary.active_turn_id = job.turn_id.clone();
    }
    if summary.latest_message.is_none() {
        summary.latest_message = Some(job.title.clone());
    }
}

pub fn thread_list_fetch_limit(limit: Option<usize>) -> usize {
    requested_thread_limit(limit)
}

pub fn filter_thread_summaries(
    mut rows: Vec<ThreadSummary>,
    archived: bool,
    q: Option<&str>,
    limit: usize,
) -> Vec<ThreadSummary> {
    let needle = q
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    rows.retain(|row| {
        if matches!(row.status, ThreadStatus::Archived) != archived {
            return false;
        }
        if let Some(needle) = &needle {
            row.id.to_ascii_lowercase().contains(needle)
                || row.title.to_ascii_lowercase().contains(needle)
                || row
                    .latest_message
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(needle)
        } else {
            true
        }
    });
    ThreadSummary::sort_by_message_time(&mut rows);
    rows.truncate(limit.max(1));
    rows
}

pub fn prune_hidden_thread_summaries(
    rows: Vec<ThreadSummary>,
    hidden_thread_ids: &HashSet<String>,
) -> Vec<ThreadSummary> {
    if hidden_thread_ids.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| !hidden_thread_ids.contains(&row.id))
        .collect()
}

pub fn normalize_thread_detail_block_limit(limit: Option<usize>, full: bool) -> Option<usize> {
    if full {
        None
    } else {
        Some(normalize_thread_block_limit(limit))
    }
}

pub fn normalize_thread_block_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(THREAD_DETAIL_DEFAULT_BLOCK_LIMIT)
        .clamp(1, THREAD_DETAIL_MAX_BLOCK_LIMIT)
}

fn apply_running_job_to_thread_list(
    threads: &mut Vec<ThreadSummary>,
    job: &JobRecord,
    archived_thread_ids: &HashSet<String>,
) {
    let Some(thread_id) = job.thread_id.as_deref() else {
        return;
    };
    if archived_thread_ids.contains(thread_id) {
        return;
    }
    if let Some(thread) = threads.iter_mut().find(|thread| thread.id == thread_id) {
        apply_running_job_to_summary(thread, job);
        return;
    }
    threads.push(thread_summary_from_running_job(job));
}

fn thread_summary_from_running_job(job: &JobRecord) -> ThreadSummary {
    ThreadSummary {
        activity_updated_at: timestamp_to_rfc3339(job.started_at),
        storage_size: None,
        id: job.thread_id.clone().unwrap_or_else(|| job.id.clone()),
        title: "未命名线程".to_string(),
        status: ThreadStatus::Running,
        updated_at: None,
        archived_at: None,
        message_count: 0,
        latest_message: Some(job.title.clone()),
        cwd: None,
        model: None,
        rollout_path: None,
        active_turn_id: job.turn_id.clone(),
        active_job_id: Some(job.id.clone()),
        pending_elicitation: None,
        last_event_kind: None,
    }
}

fn requested_thread_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(80).clamp(1, 500)
}

fn required_thread_id(value: &str) -> anyhow::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("thread_id is required");
    }
    Ok(trimmed.to_string())
}

fn detail_block_limit(limit: Option<usize>, full: Option<bool>) -> Option<usize> {
    normalize_thread_detail_block_limit(limit, full.unwrap_or(false))
}

fn block_page_limit(limit: Option<usize>) -> usize {
    normalize_thread_block_limit(limit)
}

fn timestamp_to_rfc3339(timestamp: i64) -> Option<String> {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::{
        codex::{ThreadDetail, ThreadStatus, ThreadSummary},
        db::JobRecord,
        platform::{PlatformKind, PlatformPaths},
        services::threads::{
            apply_thread_detail_runtime_state, build_threads_overview, filter_thread_summaries,
            merge_running_jobs, plan_thread_blocks_read, plan_thread_detail_read,
            plan_thread_list_read, thread_list_fetch_limit, ThreadDetailResponseKind, ThreadsQuery,
        },
    };

    #[test]
    fn overview_filters_hidden_and_archived_threads_and_merges_running_jobs() {
        let threads = vec![
            thread(
                "visible",
                ThreadStatus::Recent,
                Some("2026-06-18T10:00:00Z"),
            ),
            thread("hidden", ThreadStatus::Recent, Some("2026-06-18T11:00:00Z")),
            thread(
                "archived",
                ThreadStatus::Archived,
                Some("2026-06-18T12:00:00Z"),
            ),
        ];
        let jobs = vec![
            running_job("job-visible", "visible", Some("turn-visible"), 30),
            running_job("job-hidden", "hidden", Some("turn-hidden"), 40),
            running_job("job-new", "new-running", Some("turn-new"), 50),
            running_job("job-archived", "archived", Some("turn-archived"), 60),
        ];
        let hidden = HashSet::from(["hidden".to_string()]);
        let archived = HashSet::from(["archived".to_string()]);

        let overview = build_threads_overview(
            threads.clone(),
            jobs.clone(),
            ThreadsQuery {
                archived: Some(false),
                q: None,
                limit: Some(10),
            },
            &hidden,
            &archived,
        );

        assert_eq!(overview.query.archived, Some(false));
        assert_eq!(overview.fetch_limit, 10);
        assert_eq!(
            overview
                .threads
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<Vec<_>>(),
            vec!["visible", "new-running"]
        );
        let visible = overview
            .threads
            .iter()
            .find(|thread| thread.id == "visible")
            .expect("visible running thread");
        assert_eq!(visible.status, ThreadStatus::Running);
        assert_eq!(visible.active_job_id.as_deref(), Some("job-visible"));
        assert_eq!(visible.active_turn_id.as_deref(), Some("turn-visible"));
        assert!(!overview.threads.iter().any(|thread| thread.id == "hidden"));
        assert!(!overview
            .threads
            .iter()
            .any(|thread| thread.id == "archived"));

        let archived_overview = build_threads_overview(
            threads,
            jobs,
            ThreadsQuery {
                archived: Some(true),
                q: None,
                limit: Some(10),
            },
            &hidden,
            &archived,
        );
        assert_eq!(
            archived_overview
                .threads
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<Vec<_>>(),
            vec!["archived"]
        );
    }

    #[test]
    fn runtime_state_helper_applies_running_jobs_hidden_and_archived_rules_for_probe() {
        let rows = vec![
            thread(
                "visible",
                ThreadStatus::Recent,
                Some("2026-06-18T10:00:00Z"),
            ),
            thread("hidden", ThreadStatus::Recent, Some("2026-06-18T11:00:00Z")),
            thread(
                "archived",
                ThreadStatus::Archived,
                Some("2026-06-18T12:00:00Z"),
            ),
        ];
        let jobs = vec![
            running_job("job-visible", "visible", Some("turn-visible"), 30),
            running_job("job-hidden", "hidden", Some("turn-hidden"), 40),
            running_job("job-new", "new-running", Some("turn-new"), 50),
            running_job("job-archived", "archived", Some("turn-archived"), 60),
        ];
        let hidden = HashSet::from(["hidden".to_string()]);
        let archived = HashSet::from(["archived".to_string()]);

        let rows = super::apply_thread_list_runtime_state(
            rows,
            super::ThreadListRuntimeState {
                running_jobs: &jobs,
                hidden_thread_ids: &hidden,
                archived_thread_ids: &archived,
            },
        );
        let filtered = filter_thread_summaries(rows, false, None, 10);

        assert_eq!(
            filtered
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<Vec<_>>(),
            vec!["visible", "new-running"]
        );
        assert_eq!(filtered[0].active_job_id.as_deref(), Some("job-visible"));
    }

    #[test]
    fn merge_running_jobs_preserves_existing_active_turn_and_uses_job_title_fallback() {
        let mut rows = vec![ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            active_turn_id: Some("turn-from-rollout".to_string()),
            latest_message: None,
            ..thread("thread-a", ThreadStatus::ReplyNeeded, None)
        }];
        let jobs = vec![running_job("job-a", "thread-a", Some("turn-from-job"), 10)];

        merge_running_jobs(&mut rows, &jobs, &HashSet::new());

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, ThreadStatus::Running);
        assert_eq!(rows[0].active_job_id.as_deref(), Some("job-a"));
        assert_eq!(rows[0].active_turn_id.as_deref(), Some("turn-from-rollout"));
        assert_eq!(rows[0].latest_message.as_deref(), Some("Job job-a"));
    }

    #[test]
    fn fetch_limit_clamps_to_requested_page_size() {
        assert_eq!(thread_list_fetch_limit(Some(25)), 25);
        assert_eq!(thread_list_fetch_limit(None), 80);
    }

    #[test]
    fn read_plans_capture_shared_thread_sources_for_linux_and_macos_only() {
        let linux = PlatformPaths::for_kind(PlatformKind::Linux);
        let macos = PlatformPaths::for_kind(PlatformKind::Macos);
        let windows = PlatformPaths::for_kind(PlatformKind::Windows);

        let list = plan_thread_list_read(
            &linux,
            ThreadsQuery {
                archived: Some(true),
                q: Some("  task  ".to_string()),
                limit: Some(25),
            },
        )
        .unwrap();

        assert!(list.include_hidden_thread_ids);
        assert!(list.include_archived_thread_ids);
        assert!(list.include_running_jobs);
        assert_eq!(list.list.query.archived, Some(true));
        assert_eq!(list.list.query.q.as_deref(), Some("task"));
        assert_eq!(list.list.fetch_limit, 25);
        assert_eq!(list.list.response_limit, 25);

        let detail = plan_thread_detail_read(
            &macos,
            super::ThreadDetailRequest {
                id: " thread-a ".to_string(),
                limit: Some(600),
                full: Some(false),
                before: None,
            },
        )
        .unwrap();
        assert!(detail.include_active_job);
        assert_eq!(detail.response_kind, ThreadDetailResponseKind::Detail);
        assert_eq!(detail.detail.thread_id, "thread-a");
        assert_eq!(
            detail.detail.block_limit,
            Some(super::THREAD_DETAIL_MAX_BLOCK_LIMIT)
        );

        let blocks = plan_thread_blocks_read(&macos, " thread-a ", Some(0), None).unwrap();
        assert!(blocks.include_active_job);
        assert_eq!(blocks.response_kind, ThreadDetailResponseKind::Blocks);
        assert_eq!(blocks.detail.block_limit, Some(1));

        assert!(plan_thread_list_read(&windows, ThreadsQuery::default()).is_err());
        assert!(plan_thread_blocks_read(&windows, "thread-a", None, None).is_err());
    }

    #[test]
    fn detail_runtime_state_applies_running_job_once_in_core() {
        let detail = ThreadDetail {
            timeline: None,
            after_cursor: None,
            history_subagent_blocks: Vec::new(),
            history: None,
            subagents: None,
            subagent_updates: Default::default(),
            summary: thread(
                "thread-a",
                ThreadStatus::Recent,
                Some("2026-06-18T10:00:00Z"),
            ),
            messages: Vec::new(),
            blocks: Vec::new(),
            raw_event_count: 0,
            total_blocks: 0,
            has_more_blocks: false,
            before_cursor: None,
        };
        let job = running_job("job-a", "thread-a", Some("turn-a"), 30);

        let detail = apply_thread_detail_runtime_state(detail, Some(&job));

        assert_eq!(detail.summary.status, ThreadStatus::Running);
        assert_eq!(detail.summary.active_job_id.as_deref(), Some("job-a"));
        assert_eq!(detail.summary.active_turn_id.as_deref(), Some("turn-a"));
        assert_eq!(
            detail.summary.latest_message.as_deref(),
            Some("latest thread-a")
        );
    }

    fn thread(id: &str, status: ThreadStatus, updated_at: Option<&str>) -> ThreadSummary {
        ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: id.to_string(),
            title: format!("Thread {id}"),
            status,
            updated_at: updated_at.map(str::to_string),
            archived_at: None,
            message_count: 1,
            latest_message: Some(format!("latest {id}")),
            cwd: None,
            model: None,
            rollout_path: None,
            active_turn_id: None,
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: None,
        }
    }

    fn running_job(id: &str, thread_id: &str, turn_id: Option<&str>, started_at: i64) -> JobRecord {
        JobRecord {
            id: id.to_string(),
            kind: "codex_chat".to_string(),
            status: "running".to_string(),
            title: format!("Job {id}"),
            thread_id: Some(thread_id.to_string()),
            turn_id: turn_id.map(str::to_string),
            started_at,
            finished_at: None,
            exit_code: None,
            output: String::new(),
            error: None,
        }
    }
}
