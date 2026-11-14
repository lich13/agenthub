use serde::{Deserialize, Serialize};

use crate::{
    codex::{MessageBlock, ThreadDetail, ThreadStatus, ThreadSummary},
    platform::PlatformPaths,
    services::system::{require_capability, Capability},
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListPlan {
    pub query: ThreadsQuery,
    pub fetch_limit: usize,
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
    pub thread_id: String,
    pub block_limit: Option<usize>,
    pub before: Option<String>,
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

pub fn thread_list_read_model(
    threads: Vec<ThreadSummary>,
    query: ThreadsQuery,
) -> Vec<ThreadSummary> {
    filter_thread_summaries(
        threads,
        query.archived.unwrap_or(false),
        query.q.as_deref(),
        requested_thread_limit(query.limit),
    )
}

pub fn load_thread_read_model_inputs(
    paths: &crate::codex::CodexPaths,
    plan: &ThreadListPlan,
) -> anyhow::Result<Vec<ThreadSummary>> {
    crate::codex::list_threads(
        paths,
        plan.query.archived.unwrap_or(false),
        plan.query.q.as_deref(),
        plan.fetch_limit,
    )
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
    let fetch_limit = requested_thread_limit(query.limit);
    Ok(ThreadListPlan { query, fetch_limit })
}

pub fn plan_thread_detail_request(
    platform: &PlatformPaths,
    request: ThreadDetailRequest,
) -> anyhow::Result<ThreadDetailPlan> {
    require_capability(platform, Capability::Threads)?;
    let thread_id = required_thread_id(&request.id)?;
    let full = request.full.unwrap_or(false);
    Ok(ThreadDetailPlan {
        thread_id,
        block_limit: detail_block_limit(request.limit, Some(full)),
        before: request.before,
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

#[cfg(test)]
mod tests {

    use crate::{
        platform::{PlatformKind, PlatformPaths},
        services::threads::{
            plan_thread_blocks_request, plan_thread_detail_request, plan_threads_list_request,
            requested_thread_limit, ThreadsQuery,
        },
    };

    #[test]
    fn fetch_limit_clamps_to_requested_page_size() {
        assert_eq!(requested_thread_limit(Some(25)), 25);
        assert_eq!(requested_thread_limit(None), 80);
    }

    #[test]
    fn read_plans_capture_shared_thread_sources_for_linux_and_macos_only() {
        let linux = PlatformPaths::for_kind(PlatformKind::Linux);
        let macos = PlatformPaths::for_kind(PlatformKind::Macos);
        let windows = PlatformPaths::for_kind(PlatformKind::Windows);

        let list = plan_threads_list_request(
            &linux,
            ThreadsQuery {
                archived: Some(true),
                q: Some("  task  ".to_string()),
                limit: Some(25),
            },
        )
        .unwrap();

        assert_eq!(list.query.archived, Some(true));
        assert_eq!(list.query.q.as_deref(), Some("task"));
        assert_eq!(list.fetch_limit, 25);

        let detail = plan_thread_detail_request(
            &macos,
            super::ThreadDetailRequest {
                id: " thread-a ".to_string(),
                limit: Some(600),
                full: Some(false),
                before: None,
            },
        )
        .unwrap();
        assert_eq!(detail.thread_id, "thread-a");
        assert_eq!(
            detail.block_limit,
            Some(super::THREAD_DETAIL_MAX_BLOCK_LIMIT)
        );

        let blocks = plan_thread_blocks_request(&macos, " thread-a ", Some(0), None).unwrap();
        assert_eq!(blocks.block_limit, Some(1));

        assert!(plan_threads_list_request(&windows, ThreadsQuery::default()).is_err());
        assert!(plan_thread_blocks_request(&windows, "thread-a", None, None).is_err());
    }
}
