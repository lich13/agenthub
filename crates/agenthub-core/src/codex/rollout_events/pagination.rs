//! Mechanical extraction of the existing Codex pagination path.
use super::*;

#[derive(Default)]
struct DetailState {
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

fn json_value_heap_bytes(v: &Value) -> u64 {
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) => 0,
        Value::String(s) => s.capacity() as u64,
        Value::Array(items) => {
            (items.capacity() * std::mem::size_of::<Value>()) as u64
                + items.iter().map(json_value_heap_bytes).sum::<u64>()
        }
        Value::Object(map) => {
            32 + map
                .iter()
                .map(|(k, val)| 24 + k.capacity() as u64 + json_value_heap_bytes(val))
                .sum::<u64>()
        }
    }
}

fn detail_state_weight(state: &DetailState) -> u64 {
    fn opt_str_cap(s: &Option<String>) -> usize {
        s.as_ref().map_or(0, String::capacity)
    }
    fn block_weight(block: &MessageBlock) -> u64 {
        (512 + block.id.capacity()
            + block.kind.capacity()
            + block.role.capacity()
            + opt_str_cap(&block.phase)
            + opt_str_cap(&block.display_kind)
            + opt_str_cap(&block.summary)
            + opt_str_cap(&block.text)
            + opt_str_cap(&block.tool_name)
            + opt_str_cap(&block.call_id)
            + opt_str_cap(&block.input)
            + opt_str_cap(&block.group_id)
            + opt_str_cap(&block.status)
            + opt_str_cap(&block.created_at)
            + opt_str_cap(&block.turn_id)
            + opt_str_cap(&block.item_id)
            + opt_str_cap(&block.plan_status)) as u64
            + block
                .questions
                .iter()
                .map(crate::codex::UserInputQuestion::heap_bytes)
                .sum::<u64>()
            + (block.answers.capacity() * std::mem::size_of::<crate::codex::UserInputAnswer>())
                as u64
            + block
                .answers
                .iter()
                .map(|a| {
                    (a.question_id.capacity()
                        + opt_str_cap(&a.note)
                        + a.answers.iter().map(String::capacity).sum::<usize>())
                        as u64
                })
                .sum::<u64>()
            + block.subagent.as_ref().map_or(0, |s| {
                (160 + s.name.capacity()
                    + opt_str_cap(&s.event_id)
                    + opt_str_cap(&s.agent_id)
                    + opt_str_cap(&s.role)
                    + opt_str_cap(&s.unavailable_reason)
                    + opt_str_cap(&s.delegation)) as u64
            })
            + block.payload.as_ref().map_or(0, json_value_heap_bytes)
            + block
                .user_message
                .as_ref()
                .map_or(0, crate::user_message::UserMessage::weight)
    }
    opt_str_cap(&state.turn_context) as u64
        + state.blocks.iter().map(|b| block_weight(b)).sum::<u64>()
        + (state.blocks.capacity() + state.compacted.capacity()) as u64
            * std::mem::size_of::<Arc<MessageBlock>>() as u64
        + crate::history_navigation::weight(&state.timeline)
}

fn load_legacy_messages(path: &Path) -> Result<Vec<CodexMessage>> {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(crate::codex::rollout_file::open_reader(path)?);
    let mut line = Vec::new();
    let mut messages = Vec::new();
    let mut turn_context = None;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if let Ok(mut value) = serde_json::from_slice::<Value>(&line) {
            normalize_canonical_turn(&mut value, &mut turn_context);
            if let Some(message) = parse_message_event(&value) {
                messages.push(message);
            }
        }
    }
    Ok(messages)
}

fn with_detail_state<R>(path: &Path, project: impl FnOnce(&DetailState) -> Result<R>) -> Result<R> {
    crate::codex::rollout_file::read_cached(
        &DETAIL_CACHE,
        path,
        true,
        |state, line, _, _| {
            if line.iter().all(u8::is_ascii_whitespace) {
                return Ok(());
            }
            state.raw_event_count += 1;
            if let Ok(mut value) = serde_json::from_slice::<Value>(line) {
                normalize_canonical_turn(&mut value, &mut state.turn_context);
                state.builder.push_event(&value, state.raw_event_count);
            }
            Ok(())
        },
        |state, info| {
            if info.changed {
                let blocks = state.builder.snapshot_uncompacted();
                let previous: HashMap<_, _> = state
                    .blocks
                    .iter()
                    .map(|block| (&block.id, block))
                    .collect();
                let compacted = compact_chat_history(&blocks, CHAT_HISTORY_LIMIT);
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
            project(state)
        },
        detail_state_weight,
    )
}

pub(crate) fn visit_search_blocks(
    summary: &ThreadSummary,
    mut visit: impl FnMut(&MessageBlock) -> bool,
) -> Result<()> {
    let Some(path) = summary.rollout_path.as_deref() else {
        return Ok(());
    };
    with_detail_state(path, |state| {
        for block in &state.blocks {
            if !visit(block) {
                break;
            }
        }
        Ok(())
    })
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
    with_detail_state(&path, |state| {
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
                load_legacy_messages(&path)?
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
    })
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
