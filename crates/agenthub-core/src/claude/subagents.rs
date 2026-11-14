//! Read-only child transcripts, scoped to a verified main Claude session.
use super::*;
use crate::codex::subagents::{
    SubagentActivity, SubagentCollection, SubagentCounts, SubagentStatus,
};
use std::path::Component;

fn valid_agent_id(id: &str) -> bool {
    !id.is_empty()
        && !matches!(id, "." | "..")
        && !id
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        && Path::new(id)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn verified_path(root: &Path, path: &Path, directory: bool) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .context("子智能体记录不在会话目录内")?;
    let mut current = root.to_path_buf();
    let mut identity = Sha256::new();
    let mut inspect = |path: &Path| -> Result<()> {
        let meta = fs::symlink_metadata(path)?;
        ensure!(!meta.file_type().is_symlink(), "子智能体记录包含符号链接");
        identity.update(path.as_os_str().as_encoded_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            identity.update(meta.dev().to_le_bytes());
            identity.update(meta.ino().to_le_bytes());
        }
        Ok(())
    };
    inspect(root)?;
    for component in relative.components() {
        ensure!(
            matches!(component, Component::Normal(_)),
            "子智能体路径无法验证"
        );
        current.push(component);
        inspect(&current)?;
    }
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        if directory {
            meta.is_dir()
        } else {
            meta.is_file()
        },
        "子智能体记录类型不匹配"
    );
    Ok(hex::encode(identity.finalize()))
}

fn parent_root(paths: &ClaudePaths, parent: &Path, parsed: &Parsed) -> Result<PathBuf> {
    ensure!(
        !fs::symlink_metadata(&paths.projects)?
            .file_type()
            .is_symlink(),
        "Claude 会话根不能是符号链接"
    );
    let root = paths.projects.canonicalize()?;
    verified_path(&root, parent, false)?;
    ensure!(
        parsed.issues.is_empty() && parent.file_stem().and_then(|s| s.to_str()) == Some(&parsed.id),
        "父会话身份无法验证"
    );
    Ok(root)
}

fn activity(id: &str, parsed: &Parsed) -> SubagentActivity {
    SubagentActivity {
        event_kind: None,
        event_id: None,
        agent_id: Some(id.into()),
        name: parsed
            .agent_names
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.into()),
        role: None,
        status: SubagentStatus::Unknown,
        available: true,
        unavailable_reason: None,
        delegation: None,
    }
}

pub(super) fn collection(
    paths: &ClaudePaths,
    parent: &Path,
    parsed: &Parsed,
) -> SubagentCollection {
    let mut result = SubagentCollection {
        agents: vec![],
        counts: SubagentCounts::default(),
        complete: true,
        warning: None,
    };
    let scan = (|| -> Result<()> {
        let root = parent_root(paths, parent, parsed)?;
        let parent_before = reader::stamp(parent)?;
        let directory = parent.with_extension("").join("subagents");
        let directory_before = match fs::symlink_metadata(&directory) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => verified_path(&root, &directory, true)?,
        };
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|name| name.strip_prefix("agent-"))
                .and_then(|name| name.strip_suffix(".jsonl"))
                .filter(|id| valid_agent_id(id))
            else {
                continue;
            };
            let mut agent = activity(id, parsed);
            if verified_path(&root, &entry.path(), false).is_err() {
                agent.available = false;
                agent.unavailable_reason = Some("子智能体记录已清理或无法读取".into());
                result.complete = false;
            }
            result.agents.push(agent);
        }
        parent_root(paths, parent, parsed)?;
        ensure!(
            directory_before == verified_path(&root, &directory, true)?
                && parent_before == reader::stamp(parent)?,
            "父会话来源已变化"
        );
        Ok(())
    })();
    if scan.is_err() {
        result.agents.clear();
        result.complete = false;
        result.warning = Some("子智能体记录暂时无法读取".into());
    }
    result.agents.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    result.counts.unknown = result.agents.len();
    result
}

pub(super) fn detail(
    paths: &ClaudePaths,
    request: &ClaudeDetailRequest,
    id: &str,
) -> Result<ClaudeSessionDetail> {
    ensure!(valid_agent_id(id), "无效的子智能体 ID");
    let (parent, parent_parsed) = resolve(paths, &request.session_key)?;
    let root = parent_root(paths, &parent, &parent_parsed)?;
    let parent_stamp = reader::stamp(&parent)?;
    let parent_source = verified_path(&root, &parent, false)?;
    let path = parent
        .with_extension("")
        .join("subagents")
        .join(format!("agent-{id}.jsonl"));
    let source = verified_path(&root, &path, false).context("子智能体记录已清理或无法读取")?;
    let before = reader::stamp(&path)?;
    let prefix = format!(
        "{}:",
        digest(format!(
            "{}\0{id}\0{source}\0{parent_source}",
            request.session_key
        ))
    );

    let parsed = reader::read_context(
        &path,
        &reader::ReadContext::Subagent {
            parent_id: parent_parsed.id.clone(),
            agent_id: id.into(),
            prefix: prefix.clone(),
        },
    )?;
    ensure!(
        parsed.issues.is_empty() && parsed.id == parent_parsed.id,
        "子智能体记录身份无法验证"
    );
    let native_cursor = request
        .before
        .as_deref()
        .map(|cursor| -> Result<String> {
            let (mode, position) = cursor
                .split_once(':')
                .filter(|(kind, _)| matches!(*kind, "j" | "n" | "w" | "p"))
                .map_or(("", cursor), |(kind, id)| (kind, id));
            let position = position
                .strip_prefix(&prefix)
                .context("子智能体历史来源已变化，请重新打开")?;
            Ok(if mode.is_empty() {
                position.into()
            } else {
                format!("{mode}:{position}")
            })
        })
        .transpose()?;
    let limit = request.limit.unwrap_or(160).clamp(1, 500);
    let (start, end) = match crate::history_navigation::seek_window(
        parsed.events.iter().map(|event| event.id.as_str()),
        native_cursor.as_deref(),
        limit,
    )? {
        Some(window) => window,
        None => {
            let end = native_cursor
                .as_ref()
                .map(|id| {
                    parsed
                        .events
                        .iter()
                        .position(|e| &e.id == id)
                        .context("历史位置已变化，请刷新会话")
                })
                .transpose()?
                .unwrap_or(parsed.events.len());
            (end.saturating_sub(limit), end)
        }
    };
    let events = parsed.events[start..end]
        .iter()
        .map(|event| {
            let mut event = (**event).clone();
            event.id = format!("{prefix}{}", event.id);
            if let Some(user) = &mut event.user_message {
                user.id = format!("{prefix}{}", user.id);
            }
            event
        })
        .collect();
    let mut history = parsed.history.metadata(None);
    history.epoch = format!("{prefix}{}", history.epoch);
    for id in &mut history.changed_ids {
        *id = format!("{prefix}{id}");
    }
    for change in &mut history.changes {
        for id in &mut change.changed_ids {
            *id = format!("{prefix}{id}");
        }
    }
    let timeline = request.before.is_none().then(|| parsed.timeline.clone());
    let summary = summary(
        paths,
        &parent,
        &parent_parsed,
        &activity::Snapshot::capture_readonly(paths),
    )?;
    ensure!(
        source == verified_path(&root, &path, false)?,
        "子智能体来源已变化，请重试"
    );
    let (_, current_parent) = resolve(paths, &request.session_key)?;
    parent_root(paths, &parent, &current_parent)?;
    ensure!(
        parent_stamp == reader::stamp(&parent)? && before == reader::stamp(&path)?,
        "子智能体来源已变化，请重试"
    );
    Ok(ClaudeSessionDetail {
        summary,
        agent: Some(activity(id, &parent_parsed)),
        subagents: None,
        events,
        timeline,
        history: Some(history),
        total_events: parsed.events.len(),
        has_more: start > 0,
        before_cursor: (start > 0).then(|| format!("{prefix}{}", parsed.events[start].id)),
        after_cursor: crate::history_navigation::after_cursor(
            end,
            parsed.events.len(),
            parsed
                .events
                .get(end.saturating_sub(1))
                .map(|e| e.id.as_str()),
        )
        .map(|cursor| {
            format!(
                "n:{prefix}{}",
                cursor.strip_prefix("n:").expect("next cursor")
            )
        }),
    })
}
