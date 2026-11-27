use serde::{Deserialize, Serialize};

use crate::codex::{MessageBlock, ThreadDetail, ThreadSummary};

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

pub fn plan_threads_list_request(mut query: ThreadsQuery) -> anyhow::Result<ThreadListPlan> {
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
    request: ThreadDetailRequest,
) -> anyhow::Result<ThreadDetailPlan> {
    let thread_id = required_thread_id(&request.id)?;
    if let Some(cursor) = request.before.as_deref() {
        anyhow::ensure!(crate::codex::valid_history_cursor(cursor), "无效的历史游标");
    }
    let full = request.full.unwrap_or(false);
    Ok(ThreadDetailPlan {
        thread_id,
        block_limit: (!full).then(|| clamp_block_limit(request.limit)),
        before: request.before,
    })
}

pub fn plan_thread_blocks_request(
    id: &str,
    limit: Option<usize>,
    before: Option<String>,
) -> anyhow::Result<ThreadDetailPlan> {
    plan_thread_detail_request(ThreadDetailRequest {
        id: id.to_string(),
        limit: Some(clamp_block_limit(limit)),
        full: Some(false),
        before,
    })
}

pub fn read_window_thread_detail(
    paths: &crate::codex::CodexPaths,
    plan: &ThreadDetailPlan,
) -> anyhow::Result<Option<ThreadDetail>> {
    let Some(mut detail) = crate::codex::thread_detail_page(
        paths,
        &plan.thread_id,
        plan.block_limit,
        plan.before.as_deref(),
        None,
    )?
    else {
        return Ok(None);
    };
    crate::codex::subagents::enrich_subagent_blocks(paths, &mut detail);
    Ok(Some(detail))
}

pub fn read_thread_blocks_page(
    paths: &crate::codex::CodexPaths,
    plan: &ThreadDetailPlan,
) -> anyhow::Result<Option<ThreadBlocksPage>> {
    Ok(
        read_window_thread_detail(paths, plan)?.map(|detail| ThreadBlocksPage {
            history: detail.history,
            thread_id: plan.thread_id.clone(),
            blocks: detail.blocks,
            total_blocks: detail.total_blocks,
            has_more_blocks: detail.has_more_blocks,
            before_cursor: detail.before_cursor,
        }),
    )
}

fn clamp_block_limit(limit: Option<usize>) -> usize {
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

#[cfg(test)]
mod tests {

    use crate::services::threads::{
        plan_thread_blocks_request, plan_thread_detail_request, plan_threads_list_request,
        requested_thread_limit, ThreadsQuery,
    };

    #[test]
    fn fetch_limit_clamps_to_requested_page_size() {
        assert_eq!(requested_thread_limit(Some(25)), 25);
        assert_eq!(requested_thread_limit(None), 80);
    }

    #[test]
    fn read_plans_capture_shared_thread_sources_for_linux_and_macos_only() {
        let list = plan_threads_list_request(ThreadsQuery {
            archived: Some(true),
            q: Some("  task  ".to_string()),
            limit: Some(25),
        })
        .unwrap();

        assert_eq!(list.query.archived, Some(true));
        assert_eq!(list.query.q.as_deref(), Some("task"));
        assert_eq!(list.fetch_limit, 25);

        let detail = plan_thread_detail_request(super::ThreadDetailRequest {
            id: " thread-a ".to_string(),
            limit: Some(600),
            full: Some(false),
            before: None,
        })
        .unwrap();
        assert_eq!(detail.thread_id, "thread-a");
        assert_eq!(
            detail.block_limit,
            Some(super::THREAD_DETAIL_MAX_BLOCK_LIMIT)
        );

        let blocks = plan_thread_blocks_request(" thread-a ", Some(0), None).unwrap();
        assert_eq!(blocks.block_limit, Some(1));

        for valid in [
            "b:12",
            "b:12:message:12",
            "a:message:3",
            "p:message:3",
            "j:m1",
            "n:m1",
            "w:m1",
        ] {
            assert!(
                plan_thread_blocks_request("thread-a", Some(10), Some(valid.to_string())).is_ok()
            );
        }
        for invalid in ["not-a-cursor", "b:abc", "b:12:", "a:", "p:", "x:12"] {
            let err = plan_thread_blocks_request("thread-a", Some(10), Some(invalid.to_string()))
                .unwrap_err();
            assert!(
                err.to_string().contains("无效的历史游标"),
                "unexpected error for {invalid}: {err}"
            );
        }
    }
}
