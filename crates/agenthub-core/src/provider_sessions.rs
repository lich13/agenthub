//! Shared session list, detail, pagination, and mutation DTOs for native providers
//! (Antigravity App and DeepSeek Harness).
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::{path::PathBuf, sync::Arc};

pub(crate) static HISTORY_CACHE: crate::read_cache::ReadCache<Arc<ProviderHistorySnapshot>> =
    crate::read_cache::ReadCache::new(crate::resource_limits::PROVIDER_HISTORY_CACHE_BYTES)
        .with_native_metadata();

pub(crate) struct ProviderHistorySnapshot {
    pub events: Arc<Vec<ProviderHistoryEvent>>,
    timeline: crate::history_navigation::Timeline,
    revision: String,
}

impl ProviderHistorySnapshot {
    pub fn new(events: Vec<ProviderHistoryEvent>) -> Arc<Self> {
        let timeline = crate::history_navigation::timeline(
            events
                .iter()
                .filter(|event| event.kind == "user_message")
                .map(|event| {
                    (
                        event.id.as_str(),
                        event.user_message.as_ref(),
                        event.text.as_deref(),
                    )
                }),
        );
        // Calculate once per parsed snapshot, never on a cache hit or page read.
        let mut digest = DefaultHasher::new();
        events.hash(&mut digest);
        Arc::new(Self {
            events: Arc::new(events),
            timeline,
            revision: format!("{:016x}", digest.finish()),
        })
    }

    fn timeline(&self) -> crate::history_navigation::Timeline {
        self.timeline.clone()
    }
}

pub(crate) fn history_weight(snapshot: &Arc<ProviderHistorySnapshot>, _: u64) -> u64 {
    let events = &snapshot.events;
    std::mem::size_of::<ProviderHistorySnapshot>() as u64
        + (events.capacity() * std::mem::size_of::<ProviderHistoryEvent>()) as u64
        + snapshot.revision.capacity() as u64
        + (snapshot.timeline.capacity()
            * std::mem::size_of::<crate::history_navigation::TimelineEntry>()) as u64
        + snapshot
            .timeline
            .iter()
            .map(|entry| {
                (entry.id.capacity()
                    + entry.title.capacity()
                    + entry.preview.as_ref().map_or(0, String::capacity)) as u64
            })
            .sum::<u64>()
        + events
            .iter()
            .map(|event| {
                (event.id.capacity()
                    + event.kind.capacity()
                    + [
                        &event.timestamp,
                        &event.role,
                        &event.text,
                        &event.call_id,
                        &event.turn_id,
                        &event.status,
                        &event.detail,
                        &event.result,
                    ]
                    .iter()
                    .map(|value| value.as_ref().map_or(0, String::capacity))
                    .sum::<usize>()) as u64
                    + event
                        .user_message
                        .as_ref()
                        .map_or(0, crate::user_message::UserMessage::weight)
            })
            .sum::<u64>()
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
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

pub(crate) fn paginate_provider_detail(
    summary: ProviderSessionSummary,
    snapshot: &ProviderHistorySnapshot,
    request: &ProviderDetailRequest,
) -> Result<ProviderSessionDetail> {
    let all_events = &snapshot.events;
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
    let timeline = request.before.is_none().then(|| snapshot.timeline());
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
            revision: snapshot.revision.clone(),
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

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn content_edits_change_revision_without_count_or_timestamp_changes() {
        let event = ProviderHistoryEvent {
            id: "fixture".into(),
            timestamp: Some("2026-01-01T00:00:00Z".into()),
            kind: "user_message".into(),
            role: Some("user".into()),
            text: Some("原文".into()),
            call_id: None,
            turn_id: None,
            status: None,
            detail: None,
            result: None,
            user_message: None,
        };
        let first = ProviderHistorySnapshot::new(vec![event.clone()]);
        let again = ProviderHistorySnapshot::new(vec![event.clone()]);
        let mut edited = event;
        edited.text = Some("新文".into());
        let second = ProviderHistorySnapshot::new(vec![edited]);
        assert_eq!(first.revision, again.revision);
        assert_ne!(first.revision, second.revision);
        assert_eq!(first.timeline.len(), 1);
        assert_ne!(first.timeline, second.timeline);
    }
}
