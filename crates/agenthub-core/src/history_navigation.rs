use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineEntry {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

pub type Timeline = Arc<Vec<TimelineEntry>>;
pub(crate) const TIMELINE_TEXT_CHARS: usize = 260;

/// A small index independent of the rendered body window and history folding.
/// Repeated prompts are distinct; only duplicate native message identities merge.
pub fn timeline<'a>(
    messages: impl Iterator<
        Item = (
            &'a str,
            Option<&'a crate::user_message::UserMessage>,
            Option<&'a str>,
        ),
    >,
) -> Timeline {
    update_timeline_inner(
        &[],
        messages.map(|(id, message, raw)| (id, message, raw, false)),
        false,
    )
}

pub fn literal_timeline<'a>(
    messages: impl Iterator<
        Item = (
            &'a str,
            Option<&'a crate::user_message::UserMessage>,
            Option<&'a str>,
        ),
    >,
) -> Timeline {
    update_timeline_inner(
        &[],
        messages.map(|(id, message, raw)| (id, message, raw, false)),
        true,
    )
}

pub fn update_timeline<'a>(
    previous: &[TimelineEntry],
    messages: impl Iterator<
        Item = (
            &'a str,
            Option<&'a crate::user_message::UserMessage>,
            Option<&'a str>,
            bool,
        ),
    >,
) -> Timeline {
    update_timeline_inner(previous, messages, false)
}

fn update_timeline_inner<'a>(
    previous: &[TimelineEntry],
    messages: impl Iterator<
        Item = (
            &'a str,
            Option<&'a crate::user_message::UserMessage>,
            Option<&'a str>,
            bool,
        ),
    >,
    literal_user: bool,
) -> Timeline {
    let previous: HashMap<_, _> = previous
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let mut seen = HashSet::new();
    Arc::new(
        messages
            .filter_map(|(id, message, raw, unchanged)| {
                let identity = message.map_or(id, |message| message.id.as_str());
                if !seen.insert(identity) {
                    return None;
                }
                if unchanged {
                    return previous.get(id).map(|entry| (*entry).clone());
                }
                let raw_or_msg = message.map_or(raw.unwrap_or_default(), |message| &message.text);
                let text = if literal_user {
                    crate::user_message::literal_summary_text(raw_or_msg)
                } else {
                    crate::user_message::summary_text(raw_or_msg)
                };
                let text = crate::services::search::strip_memory_metadata(&text);
                let text: String = text
                    .split_whitespace()
                    .flat_map(|word| word.chars().chain(std::iter::once(' ')))
                    .take(TIMELINE_TEXT_CHARS)
                    .collect();
                let text = text.trim_end();
                let attachments = message.is_some_and(|message| !message.attachments.is_empty());
                if text.is_empty() && !attachments {
                    return None;
                }
                let mut chars = text.chars();
                let title: String = chars.by_ref().take(80).collect();
                let preview: String = chars.take(180).collect();
                Some(TimelineEntry {
                    id: id.into(),
                    title: if title.is_empty() {
                        "附件".into()
                    } else {
                        title
                    },
                    preview: (!preview.is_empty()).then_some(preview),
                })
            })
            .collect(),
    )
}

pub fn weight(entries: &[TimelineEntry]) -> u64 {
    entries
        .iter()
        .map(|entry| {
            96 + entry.id.len() as u64
                + entry.title.len() as u64
                + entry.preview.as_ref().map_or(0, String::len) as u64
        })
        .sum()
}

/// New cursors use stable identities, so appends cannot move the selected prompt.
/// Legacy before cursors remain the responsibility of each provider.
pub fn seek_window<'a>(
    ids: impl ExactSizeIterator<Item = &'a str>,
    cursor: Option<&str>,
    limit: usize,
) -> Result<Option<(usize, usize)>> {
    let Some((kind, id)) = cursor
        .and_then(|cursor| cursor.split_once(':'))
        .filter(|(kind, _)| matches!(*kind, "j" | "n" | "w" | "p"))
    else {
        return Ok(None);
    };
    let total = ids.len();
    let index = ids
        .into_iter()
        .position(|candidate| candidate == id)
        .context("历史位置已变化，请刷新会话")?;
    if kind == "p" {
        return Ok(Some((index.saturating_sub(limit), index)));
    }
    let start = if kind == "n" {
        index + 1
    } else if kind == "w" {
        index
    } else {
        index.saturating_sub(limit / 4)
    };
    Ok(Some((start, start.saturating_add(limit).min(total))))
}

pub fn after_cursor(end: usize, total: usize, last_id: Option<&str>) -> Option<String> {
    (end < total)
        .then(|| last_id.map(|id| format!("n:{id}")))
        .flatten()
}

pub(crate) struct PageWindow {
    pub start: usize,
    pub end: usize,
    pub total: usize,
    pub has_more: bool,
    pub before_cursor: Option<String>,
    pub after_cursor: Option<String>,
}

pub(crate) fn paginate_window<T>(
    items: &[T],
    id_of: impl Fn(&T) -> &str,
    before: Option<&str>,
    limit: Option<usize>,
    unbounded_when_none: bool,
) -> Result<PageWindow> {
    let total = items.len();
    let clamped_limit = limit.unwrap_or(160).clamp(1, 500);
    let (start, end) = match seek_window(items.iter().map(&id_of), before, clamped_limit)? {
        Some(window) => window,
        None => {
            let end = match before {
                None => total,
                Some(id) => items
                    .iter()
                    .position(|item| id_of(item) == id)
                    .context("历史位置已变化，请刷新会话")?,
            };
            let start = if unbounded_when_none && limit.is_none() {
                0
            } else {
                end.saturating_sub(clamped_limit)
            };
            (start, end)
        }
    };
    Ok(PageWindow {
        start,
        end,
        total,
        has_more: start > 0,
        before_cursor: (start > 0).then(|| id_of(&items[start]).to_owned()),
        after_cursor: after_cursor(end, total, items.get(end.saturating_sub(1)).map(&id_of)),
    })
}

pub(crate) fn strip_subagent_cursor_prefix(
    cursor: &str,
    prefix: &str,
    mismatch_error: &'static str,
) -> Result<String> {
    let (mode, position) = cursor
        .split_once(':')
        .filter(|(kind, _)| matches!(*kind, "j" | "n" | "w" | "p"))
        .map_or(("", cursor), |(kind, id)| (kind, id));
    let position = position
        .strip_prefix(prefix)
        .with_context(|| mismatch_error)?;
    Ok(if mode.is_empty() {
        position.into()
    } else {
        format!("{mode}:{position}")
    })
}

pub(crate) fn prefix_history_metadata(
    history: &mut crate::jsonl_cache::HistoryMetadata,
    prefix: &str,
) {
    history.epoch = format!("{prefix}{}", history.epoch);
    for id in &mut history.changed_ids {
        *id = format!("{prefix}{id}");
    }
    for change in &mut history.changes {
        for id in &mut change.changed_ids {
            *id = format!("{prefix}{id}");
        }
    }
}

pub(crate) fn prefix_after_cursor(after_cursor: &mut Option<String>, prefix: &str) {
    if let Some(ac) = after_cursor {
        let rest = ac.strip_prefix("n:").unwrap_or(ac);
        *ac = format!("n:{prefix}{rest}");
    }
}
