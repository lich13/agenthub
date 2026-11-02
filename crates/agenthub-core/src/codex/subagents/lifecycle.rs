use super::*;
use crate::codex::MessageBlock;

pub(in crate::codex) fn native_activity_block(value: &Value) -> Option<MessageBlock> {
    let payload = value.get("payload").unwrap_or(value);
    if !matches!(
        payload.get("type")?.as_str()?,
        "item_completed" | "item.completed"
    ) {
        return None;
    }
    let item = payload.get("item")?;
    if item.get("type")?.as_str()? != "SubAgentActivity" {
        return None;
    }
    let kind = match item.get("kind")?.as_str()? {
        "started" => SubagentEventKind::Started,
        "completed" => SubagentEventKind::Completed,
        "interrupted" => SubagentEventKind::Interrupted,
        "interacted" => SubagentEventKind::Interacted,
        _ => return None,
    };
    let event_id = field(item, &["id"])?;
    let agent_id = field(item, &["agent_thread_id"])?;
    if validate_key(&event_id).is_err() || validate_key(&agent_id).is_err() {
        return None;
    }
    Some(MessageBlock {
        subagent: Some(SubagentActivity {
            event_kind: Some(kind),
            event_id: Some(event_id.clone()),
            agent_id: Some(agent_id.clone()),
            name: "子智能体".into(),
            role: None,
            status: SubagentStatus::Unknown,
            available: false,
            unavailable_reason: Some("子智能体关联尚未确认或记录已清理".into()),
            delegation: None,
        }),
        user_message: None,
        id: format!("subagent:{event_id}"),
        role: "tool".into(),
        kind: "subagent_activity".into(),
        display_kind: Some("subagent".into()),
        status: None,
        text: None,
        summary: None,
        input: None,
        truncated: Some(false),
        resolved: None,
        answers: Vec::new(),
        plan_status: None,
        group_id: None,
        tool_name: None,
        call_id: None,
        turn_id: field(payload, &["turn_id", "turnId"]).or_else(|| field(value, &["turn_id"])),
        item_id: Some(event_id),
        created_at: field(value, &["timestamp", "created_at"]),
        questions: Vec::new(),
        payload: Some(serde_json::json!({
            "parent_thread_id": field(payload, &["thread_id", "threadId"]),
            "agent_thread_id": agent_id,
            "agent_path": field(item, &["agent_path"])
        })),
    })
}

pub(super) fn output_identity(block: &MessageBlock) -> (Option<String>, Option<String>) {
    let output = block
        .text
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null);
    (
        field(&output, &["agent_id", "thread_id", "id"]),
        field(&output, &["task_name", "agent_path"]),
    )
}

// Keep native chronology and retain the creation anchor used by search. This runs
// before pagination, for both search and the detail reader.
pub(in crate::codex) fn merge_creation_events(blocks: &mut Vec<std::sync::Arc<MessageBlock>>) {
    let mut seen = HashSet::new();
    blocks.retain(|block| {
        block
            .subagent
            .as_ref()
            .and_then(|a| a.event_id.as_ref())
            .is_none_or(|id| seen.insert(id.clone()))
    });
    let mut consumed = HashSet::new();
    let mut removed = HashSet::new();
    for index in 0..blocks.len() {
        let creation = &blocks[index];
        let Some(agent) = creation
            .subagent
            .as_ref()
            .filter(|a| a.event_kind.is_none())
        else {
            continue;
        };
        let (id, path) = output_identity(creation);
        if id.is_none() && path.is_none() {
            continue;
        }
        let matches: Vec<_> = blocks
            .iter()
            .enumerate()
            .filter(|(position, block)| {
                !consumed.contains(position)
                    && block.turn_id == creation.turn_id
                    && block.subagent.as_ref().is_some_and(|a| {
                        a.event_kind == Some(SubagentEventKind::Started)
                            && if let Some(id) = &id {
                                a.agent_id.as_ref() == Some(id)
                            } else {
                                block
                                    .payload
                                    .as_ref()
                                    .and_then(|p| field(p, &["agent_path"]))
                                    == path
                            }
                    })
            })
            .map(|(position, _)| position)
            .collect();
        // Multiple starts in one parent turn are separate work rounds. Only the
        // first start following this creation can belong to it.
        let Some(target) = matches.into_iter().find(|position| *position >= index) else {
            continue;
        };
        let name = agent.name.clone();
        let delegation = agent.delegation.clone();
        let anchor = creation.id.clone();
        let input = creation.input.clone();
        let call = creation.call_id.clone();
        let native = std::sync::Arc::make_mut(&mut blocks[target]);
        native.id = anchor;
        native.input = input;
        native.call_id = call;
        let activity = native.subagent.as_mut().expect("native activity");
        activity.name = name;
        activity.delegation = delegation;
        consumed.insert(target);
        removed.insert(index);
    }
    let mut position = 0;
    blocks.retain(|_| {
        let keep = !removed.contains(&position);
        position += 1;
        keep
    });
}

#[derive(Clone, Default)]
pub(super) struct CreationNames {
    names: Vec<(Option<String>, Option<String>, String)>,
}

impl CreationNames {
    pub(super) fn from_blocks(blocks: &[MessageBlock]) -> Self {
        Self {
            names: blocks
                .iter()
                .filter_map(|block| {
                    let agent = block.subagent.as_ref()?;
                    if agent.name == "子智能体" {
                        return None;
                    }
                    if agent.event_kind.is_some() && block.call_id.is_none() {
                        return None;
                    }
                    let (id, path) = if agent.event_kind.is_some() {
                        (
                            block
                                .payload
                                .as_ref()
                                .and_then(|p| field(p, &["agent_thread_id"])),
                            None,
                        )
                    } else {
                        output_identity(block)
                    };
                    Some((id, path, agent.name.clone()))
                })
                .collect(),
        }
    }
    pub(super) fn for_record(&self, record: &AgentRecord) -> Option<&str> {
        let matches: HashSet<_> = self
            .names
            .iter()
            .filter(|(id, path, _)| {
                if let Some(id) = id {
                    id == &record.id
                } else {
                    path.is_some() && path == &record.path
                }
            })
            .map(|(_, _, name)| name.as_str())
            .collect();
        (matches.len() == 1).then(|| *matches.iter().next().expect("single name"))
    }
}

#[derive(Default)]
struct CreationState {
    names: CreationNames,
    pending: HashMap<String, String>,
    stopped: bool,
}
pub(super) fn creation_names(path: &Path) -> Result<CreationNames> {
    static CACHE: crate::jsonl_cache::JsonlCache<CreationState> =
        crate::jsonl_cache::JsonlCache::new(crate::resource_limits::SUBAGENT_LIFECYCLE_CACHE_BYTES);
    CACHE.read(
        path,
        |_| Ok(0),
        false,
        |state, line, _, _| {
            state.stopped |= line.len() > 20 * 1024 * 1024;
            if state.stopped {
                return Ok(());
            }
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                return Ok(());
            };
            let names = &mut state.names;
            let pending = &mut state.pending;
            let payload = value.get("payload").unwrap_or(&value);
            match payload.get("type").and_then(Value::as_str) {
                Some("function_call" | "custom_tool_call") => {
                    if let Some(agent) =
                        field(payload, &["name"]).and_then(|name| creation_activity(&name, payload))
                    {
                        if let Some(call) = field(payload, &["call_id"]) {
                            ensure!(pending.len() < MAX_RECORDS, "子智能体创建记录过多");
                            pending.insert(call, agent.name);
                        }
                    }
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    if let Some(name) =
                        field(payload, &["call_id"]).and_then(|call| pending.remove(&call))
                    {
                        let raw = payload.get("output").unwrap_or(&Value::Null);
                        let parsed = raw
                            .as_str()
                            .and_then(|s| serde_json::from_str::<Value>(s).ok());
                        let output = parsed.as_ref().unwrap_or(raw);
                        ensure!(names.names.len() < MAX_RECORDS, "子智能体创建记录过多");
                        names.names.push((
                            field(output, &["agent_id", "thread_id", "id"]),
                            field(output, &["task_name", "agent_path"]),
                            name,
                        ));
                    }
                }
                _ => {}
            }

            Ok(())
        },
        |state, _| Ok(state.names.clone()),
        |state| {
            state
                .names
                .names
                .iter()
                .map(|(id, path, name)| {
                    (id.as_ref().map_or(0, String::len)
                        + path.as_ref().map_or(0, String::len)
                        + name.len()
                        + 128) as u64
                })
                .sum::<u64>()
                + state
                    .pending
                    .iter()
                    .map(|(id, name)| (id.len() + name.len() + 128) as u64)
                    .sum::<u64>()
        },
    )
}
