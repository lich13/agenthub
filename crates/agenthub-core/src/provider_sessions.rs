//! Shared session list, detail, pagination, and mutation DTOs for native providers
//! (Antigravity App and DeepSeek Harness).
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDetailRequest {
    pub session_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRenameRequest {
    pub session_key: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSessionRequest {
    pub session_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSessionSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_size: Option<crate::session_storage::SessionStorageSize>,
    pub id: String,
    pub session_key: String,
    pub title: String,
    pub cwd: String,
    pub path: PathBuf,
    pub archived: bool,
    pub updated_at: Option<String>,
    pub message_count: usize,
    pub last_message: Option<String>,
    pub status: String,
    pub format_version: String,
    pub can_rename: bool,
    pub rename_block_reason: Option<String>,
    pub can_archive: bool,
    pub archive_block_reason: Option<String>,
    pub can_delete: bool,
    pub delete_block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHistoryEvent {
    pub id: String,
    pub timestamp: Option<String>,
    pub kind: String,
    pub role: Option<String>,
    pub text: Option<String>,
    pub call_id: Option<String>,
    pub turn_id: Option<String>,
    pub status: Option<String>,
    pub detail: Option<String>,
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_message: Option<crate::user_message::UserMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSessionDetail {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<crate::history_navigation::Timeline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<crate::jsonl_cache::HistoryMetadata>,
    pub summary: ProviderSessionSummary,
    pub events: Vec<ProviderHistoryEvent>,
    pub total_events: usize,
    pub has_more: bool,
    pub before_cursor: Option<String>,
}

pub fn matches_query(summary: &ProviderSessionSummary, needle: Option<&str>) -> bool {
    let Some(q) = needle.map(str::trim).filter(|q| !q.is_empty()) else {
        return true;
    };
    let lower = q.to_lowercase();
    summary.title.to_lowercase().contains(&lower)
        || summary.id.to_lowercase().contains(&lower)
        || summary.cwd.to_lowercase().contains(&lower)
        || summary
            .last_message
            .as_deref()
            .is_some_and(|msg| msg.to_lowercase().contains(&lower))
}

pub fn paginate_provider_detail(
    summary: ProviderSessionSummary,
    all_events: &[ProviderHistoryEvent],
    request: &ProviderDetailRequest,
) -> Result<ProviderSessionDetail> {
    let limit = request.limit.unwrap_or(160).clamp(1, 500);
    let (start, end) = match crate::history_navigation::seek_window(
        all_events.iter().map(|event| event.id.as_str()),
        request.before.as_deref(),
        limit,
    )? {
        Some(window) => window,
        None => {
            let end = match &request.before {
                None => all_events.len(),
                Some(id) => all_events
                    .iter()
                    .position(|event| &event.id == id)
                    .context("历史位置已变化，请刷新会话")?,
            };
            (end.saturating_sub(limit), end)
        }
    };
    let mut events = all_events[start..end].to_vec();
    for event in &mut events {
        if let Some(user) = &mut event.user_message {
            user.refresh_files((!summary.cwd.is_empty()).then_some(summary.cwd.as_str()));
        }
    }
    let timeline = request.before.is_none().then(|| {
        crate::history_navigation::timeline(
            all_events
                .iter()
                .filter(|event| event.kind == "user_message")
                .map(|event| {
                    (
                        event.id.as_str(),
                        event.user_message.as_ref(),
                        event.text.as_deref(),
                    )
                }),
        )
    });
    let revision = hex::encode(Sha256::digest(format!(
        "{}:{}:{}",
        summary.session_key,
        all_events.len(),
        summary.updated_at.as_deref().unwrap_or("")
    )));
    Ok(ProviderSessionDetail {
        timeline,
        after_cursor: crate::history_navigation::after_cursor(
            end,
            all_events.len(),
            all_events
                .get(end.saturating_sub(1))
                .map(|event| event.id.as_str()),
        ),
        history: Some(crate::jsonl_cache::HistoryMetadata {
            epoch: summary.session_key.clone(),
            revision,
            changed_ids: Vec::new(),
            reset: false,
            changes: Vec::new(),
        }),
        summary,
        events,
        total_events: all_events.len(),
        has_more: start > 0,
        before_cursor: (start > 0).then(|| all_events[start].id.clone()),
    })
}
