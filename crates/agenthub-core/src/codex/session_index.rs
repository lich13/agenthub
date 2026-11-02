use anyhow::Result;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use super::{
    rollout_events::{first_user_message_title, thread_title_candidate},
    CodexPaths,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionIndexEntry {
    pub(crate) path: Option<PathBuf>,
    pub(crate) title: Option<String>,
    pub(crate) first_user_message: Option<String>,
}

impl SessionIndexEntry {
    pub(crate) fn title_candidate(&self) -> Option<String> {
        self.title
            .as_deref()
            .and_then(thread_title_candidate)
            .or_else(|| {
                self.first_user_message
                    .as_deref()
                    .and_then(first_user_message_title)
            })
    }
}

pub(crate) fn read_session_index(
    paths: &CodexPaths,
) -> Result<Arc<HashMap<String, SessionIndexEntry>>> {
    static CACHE: crate::jsonl_cache::JsonlCache<Arc<HashMap<String, SessionIndexEntry>>> =
        crate::jsonl_cache::JsonlCache::new(crate::resource_limits::CODEX_INDEX_CACHE_BYTES);
    CACHE.read(
        &paths.session_index(),
        |_| Ok(0),
        true,
        |map, line, _, _| {
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                return Ok(());
            };
            let Some(id) = value.get("id").and_then(Value::as_str) else {
                return Ok(());
            };
            let mut entry = SessionIndexEntry {
                path: session_index_path(paths, &value),
                title: session_index_title(&value),
                first_user_message: session_index_string_field(&value, "first_user_message")
                    .or_else(|| session_index_string_field(&value, "firstUserMessage")),
            };
            let map = Arc::make_mut(map);
            if let Some(existing) = map.get_mut(id) {
                if entry.title.is_some() {
                    existing.title = entry.title.take();
                }
                if existing.path.is_none() {
                    existing.path = entry.path.take();
                }
                if existing.first_user_message.is_none() {
                    existing.first_user_message = entry.first_user_message.take();
                }
            } else {
                map.insert(id.to_string(), entry);
            }

            Ok(())
        },
        |map, _| Ok(map.clone()),
        |map| map.len() as u64 * 2048,
    )
}

fn session_index_path(paths: &CodexPaths, value: &Value) -> Option<PathBuf> {
    value
        .get("path")
        .or_else(|| value.get("rollout_path"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| paths.contains_path(path))
}

fn session_index_title(value: &Value) -> Option<String> {
    [
        "title",
        "name",
        "thread_name",
        "threadName",
        "thread_title",
        "threadTitle",
        "session_title",
        "sessionTitle",
    ]
    .into_iter()
    .find_map(|field| session_index_string_field(value, field))
}

fn session_index_string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}
