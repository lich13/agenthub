//! Mechanical extraction of the existing Codex pagination path.
use super::*;

#[derive(Default)]
struct DetailState {
    messages: Vec<CodexMessage>,
    builder: MessageBlockBuilder,
    raw_event_count: usize,
    turn_context: Option<String>,
    blocks: Vec<Arc<MessageBlock>>,
    compacted: Vec<Arc<MessageBlock>>,
    journal: crate::jsonl_cache::HistoryJournal,
    timeline: crate::history_navigation::Timeline,
}

static DETAIL_CACHE: crate::jsonl_cache::JsonlCache<DetailState> =
    crate::jsonl_cache::JsonlCache::new(crate::resource_limits::CODEX_DETAIL_CACHE_BYTES);

pub(crate) fn evict_idle_detail_cache(max_idle: std::time::Duration) {
    DETAIL_CACHE.evict_idle(max_idle);
}

pub fn thread_detail_page_from_summary(
    summary: ThreadSummary,
    compact: bool,
    limit: Option<usize>,
    before: Option<&str>,
    since: Option<&str>,
) -> Result<ThreadDetail> {
    let Some(path) = summary.rollout_path.clone() else {
        return Ok(ThreadDetail {
            timeline: Some(Default::default()),
            after_cursor: None,
            history_subagent_blocks: Vec::new(),
            summary,
            subagents: None,
            subagent_updates: Default::default(),
            messages: Vec::new(),
            blocks: Vec::new(),
            raw_event_count: 0,
            total_blocks: 0,
            has_more_blocks: false,
            before_cursor: None,
            history: None,
        });
    };
    crate::codex::rollout_file::read_cached(
        &DETAIL_CACHE,
        &path,
        true,
        |state, line, _, _| {
            if line.iter().all(u8::is_ascii_whitespace) {
                return Ok(());
            }
            state.raw_event_count += 1;
            if let Ok(mut value) = serde_json::from_slice::<Value>(line) {
                normalize_canonical_turn(&mut value, &mut state.turn_context);
                if let Some(message) = parse_message_event(&value) {
                    state.messages.push(message);
                }
                state.builder.push_event(&value, state.raw_event_count);
            }
            Ok(())
        },
        |state, info| {
            if info.changed {
                let blocks = state.builder.clone().finish_uncompacted();
                let previous: HashMap<_, _> = state
                    .blocks
                    .iter()
                    .map(|block| (&block.id, block))
                    .collect();
                let compacted = compact_chat_history(blocks.clone(), CHAT_HISTORY_LIMIT);
                let old_markers: HashMap<_, _> = state
                    .compacted
                    .iter()
                    .filter(|block| block.kind == "chat_history_collapsed")
                    .map(|block| (&block.id, block))
                    .collect();
                let mut changed: Vec<_> = blocks
                    .iter()
                    .filter(|block| {
                        previous
                            .get(&block.id)
                            .is_some_and(|old| !Arc::ptr_eq(old, block) && ***old != ***block)
                    })
                    .map(|block| block.id.clone())
                    .collect();
                changed.extend(
                    compacted
                        .iter()
                        .filter(|block| {
                            old_markers
                                .get(&block.id)
                                .is_some_and(|old| ***old != ***block)
                        })
                        .map(|block| block.id.clone()),
                );
                let current_ids: HashSet<_> = blocks.iter().map(|block| &block.id).collect();
                let removed = state
                    .blocks
                    .iter()
                    .any(|block| !current_ids.contains(&block.id));
                state.journal.record_changes(&info, changed, removed);
                state.timeline = crate::history_navigation::update_timeline(
                    &state.timeline,
                    blocks
                        .iter()
                        .filter(|block| block.role == "user")
                        .map(|block| {
                            (
                                block.id.as_str(),
                                block.user_message.as_ref(),
                                block.text.as_deref(),
                                previous
                                    .get(&block.id)
                                    .is_some_and(|old| Arc::ptr_eq(old, block)),
                            )
                        }),
                );
                state.compacted = compacted;
                state.blocks = blocks;
            }
            let seeking = before.is_some_and(|cursor| {
                cursor.starts_with("j:")
                    || cursor.starts_with("n:")
                    || cursor.starts_with("w:")
                    || cursor.starts_with("p:")
            });
            let source = if compact && !seeking {
                &state.compacted
            } else {
                &state.blocks
            };
            let total_blocks = source.len();
            let end = history_end(source, before);
            let start = limit
                .filter(|n| *n > 0)
                .map_or(0, |n| end.saturating_sub(n));
            let (start, end) = crate::history_navigation::seek_window(
                source.iter().map(|block| block.id.as_str()),
                before,
                limit.unwrap_or(120).clamp(1, 500),
            )?
            .unwrap_or((start, end));
            let mut blocks: Vec<_> = source[start..end]
                .iter()
                .map(|block| (**block).clone())
                .collect();
            for message in blocks
                .iter_mut()
                .filter_map(|block| block.user_message.as_mut())
            {
                message.refresh_files(summary.cwd.as_deref());
            }
            Ok(ThreadDetail {
                timeline: before.is_none().then(|| state.timeline.clone()),
                after_cursor: crate::history_navigation::after_cursor(
                    end,
                    total_blocks,
                    source
                        .get(end.saturating_sub(1))
                        .map(|block| block.id.as_str()),
                ),
                history_subagent_blocks: if limit.is_some() {
                    source
                        .iter()
                        .filter(|block| block.subagent.is_some())
                        .map(|block| (**block).clone())
                        .collect()
                } else {
                    Vec::new()
                },
                summary,
                subagents: None,
                subagent_updates: Default::default(),
                messages: if limit.is_none() {
                    state.messages.clone()
                } else {
                    Vec::new()
                },
                blocks,
                raw_event_count: state.raw_event_count,
                total_blocks,
                has_more_blocks: start > 0,
                before_cursor: (start > 0).then(|| {
                    if seeking {
                        format!("p:{}", source[start].id)
                    } else {
                        format!("b:{start}")
                    }
                }),
                history: Some(state.journal.metadata(since)),
            })
        },
        |state| {
            fn weight(block: &MessageBlock) -> u64 {
                1024 + block.text.as_ref().map_or(0, String::len) as u64
                    + block.input.as_ref().map_or(0, String::len) as u64
                    + block
                        .user_message
                        .as_ref()
                        .map_or(0, crate::user_message::UserMessage::weight)
            }
            state
                .messages
                .iter()
                .map(|m| m.text.len() as u64 + 128)
                .sum::<u64>()
                + state.blocks.iter().map(|block| weight(block)).sum::<u64>()
                + state.blocks.len() as u64 * 64
                + crate::history_navigation::weight(&state.timeline)
        },
    )
}

pub(crate) fn valid_history_cursor(cursor: &str) -> bool {
    let Some((kind, value)) = cursor.split_once(':') else {
        return false;
    };
    match kind {
        "b" => {
            let (position, id) = value
                .split_once(':')
                .map_or((value, None), |(pos, id)| (pos, Some(id)));
            position.parse::<usize>().is_ok() && id.is_none_or(|id| !id.is_empty())
        }
        "a" | "j" | "n" | "w" | "p" => !value.is_empty(),
        _ => false,
    }
}

fn history_end(blocks: &[Arc<MessageBlock>], before: Option<&str>) -> usize {
    let Some(cursor) = before else {
        return blocks.len();
    };
    if let Some(id) = cursor.strip_prefix("a:") {
        return blocks
            .iter()
            .position(|block| block.id == id)
            .map_or(0, |index| index + 1);
    }
    let Some(value) = cursor.strip_prefix("b:") else {
        return blocks.len();
    };
    if let Some((_, id)) = value.split_once(':') {
        return blocks.iter().position(|block| block.id == id).unwrap_or(0);
    }
    value
        .parse::<usize>()
        .unwrap_or(blocks.len())
        .min(blocks.len())
}
