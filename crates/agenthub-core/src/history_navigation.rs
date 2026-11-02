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
    update_timeline(
        &[],
        messages.map(|(id, message, raw)| (id, message, raw, false)),
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
                let text = crate::user_message::summary_text(
                    message.map_or(raw.unwrap_or_default(), |message| &message.text),
                );
                let text = crate::services::search::strip_memory_metadata(&text);
                let text: String = text
                    .split_whitespace()
                    .flat_map(|word| word.chars().chain(std::iter::once(' ')))
                    .take(260)
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
