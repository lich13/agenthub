//! Existing hooks implementation, split without changing dispatch order.
use super::*;

pub(super) async fn handle_hook_stop_command(
    config: &Config,
    db: &PanelDb,
    thread_id: Option<String>,
    turn_id: Option<String>,
    kind: String,
) -> Result<HookStopResult> {
    let stdin_payload = read_optional_stdin_json()?;
    let event_input = hook_stop_event_input(
        config,
        stdin_payload.as_ref(),
        thread_id.as_deref(),
        turn_id.as_deref(),
        &kind,
    )?;
    let event = probe_runtime(config).build_event(event_input);
    handle_built_probe_event(config, db, event).await
}

pub(super) async fn handle_hook_request_user_input_command(config_path: &Path) -> Result<()> {
    let stdin_payload = read_optional_stdin_json()?.context("PreToolUse stdin is required")?;
    let config = Config::load(config_path)?;
    validate_hook_request_user_input_envelope(&stdin_payload)?;
    if hook_context_suppression_reason(&config, &stdin_payload).is_some() {
        return Ok(());
    }
    validate_hook_request_user_input_payload(&stdin_payload)?;
    schedule_hook_request_user_input_confirmation(config_path, &stdin_payload)?;
    Ok(())
}

pub(super) async fn handle_hook_request_user_input_confirm_command(
    config_path: &Path,
) -> Result<()> {
    let stdin_payload = read_optional_stdin_json()?.context("PreToolUse stdin is required")?;
    let config = Config::load(config_path)?;
    validate_hook_request_user_input_envelope(&stdin_payload)?;
    if hook_context_suppression_reason(&config, &stdin_payload).is_some() {
        return Ok(());
    }
    let hook_payload = validate_hook_request_user_input_payload(&stdin_payload)?;
    time::sleep(Duration::from_millis(QUESTION_CONFIRMATION_DELAY_MS)).await;
    let Some(transcript_path) =
        read_string_field(&stdin_payload, &["transcript_path", "transcriptPath"])
    else {
        return Ok(());
    };
    let state = rollout_request_user_input_state(
        Path::new(&transcript_path),
        hook_payload.turn_id.trim(),
        hook_payload.tool_use_id.trim(),
    )
    .unwrap_or(RolloutRequestUserInputState::Missing);
    if state != RolloutRequestUserInputState::Pending {
        return Ok(());
    }

    let mut diagnostics = question_confirmation_diagnostics();
    diagnostics.insert("question_tool".into(), json!(hook_payload.tool_name));
    let event_input = hook_request_user_input_event_input(&config, stdin_payload)?
        .with_body_selection_diagnostics(diagnostics);
    let db = open_panel_db(&config)?;
    let event = probe_runtime(&config).build_event(event_input);
    record_probe_event_with_bark_timeout(&config, &db, event, std::time::Duration::from_secs(3))
        .await?;
    Ok(())
}

pub(super) fn schedule_hook_request_user_input_confirmation(
    config_path: &Path,
    payload: &Value,
) -> Result<()> {
    let executable = env::current_exe().context("resolve current helper executable")?;
    let mut child = ProcessCommand::new(executable)
        .args([
            "--config",
            config_path
                .to_str()
                .context("config path is not valid UTF-8")?,
            "probe",
            "hook-request-user-input-confirm",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("start question confirmation helper")?;
    let payload = serde_json::to_vec(payload).context("serialize PreToolUse payload")?;
    child
        .stdin
        .take()
        .context("open question confirmation stdin")?
        .write_all(&payload)
        .context("send PreToolUse payload to confirmation helper")?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub(super) struct HookRequestUserInputPayload {
    pub(super) hook_event_name: String,
    pub(super) session_id: String,
    pub(super) turn_id: String,
    pub(super) tool_name: String,
    pub(super) tool_input: HookRequestUserInputToolInput,
    pub(super) tool_use_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct HookRequestUserInputToolInput {
    pub(super) questions: Vec<Value>,
}

pub(super) fn validate_hook_request_user_input_envelope(payload: &Value) -> Result<()> {
    anyhow::ensure!(
        read_string_field(payload, &["hook_event_name"]).as_deref() == Some("PreToolUse"),
        "unexpected hook_event_name"
    );
    anyhow::ensure!(
        read_string_field(payload, &["tool_name"])
            .as_deref()
            .is_some_and(agenthub_core::codex::is_question_tool),
        "unexpected tool_name"
    );
    Ok(())
}

pub(super) fn validate_hook_request_user_input_payload(
    payload: &Value,
) -> Result<HookRequestUserInputPayload> {
    let hook_payload: HookRequestUserInputPayload =
        serde_json::from_value(payload.clone()).context("parse PreToolUse payload")?;
    anyhow::ensure!(
        hook_payload.hook_event_name == "PreToolUse",
        "unexpected hook_event_name"
    );
    anyhow::ensure!(
        agenthub_core::codex::is_question_tool(&hook_payload.tool_name),
        "unexpected tool_name"
    );
    required_hook_field(&hook_payload.session_id, "session_id")?;
    required_hook_field(&hook_payload.turn_id, "turn_id")?;
    required_hook_field(&hook_payload.tool_use_id, "tool_use_id")?;
    agenthub_core::codex::normalize_user_input_questions(
        &json!({"questions": hook_payload.tool_input.questions}),
    )
    .context("invalid native questions")?;
    Ok(hook_payload)
}

pub(super) fn hook_request_user_input_event_input(
    config: &Config,
    payload: Value,
) -> Result<ProbeEventInput> {
    validate_hook_request_user_input_envelope(&payload)?;
    if let Some(reason) = hook_context_suppression_reason(config, &payload) {
        let session_id = read_string_field(&payload, &["session_id", "sessionId"]);
        let turn_id = read_string_field(&payload, &["turn_id", "turnId"]);
        let tool_use_id = read_string_field(&payload, &["tool_use_id", "toolUseId"]);
        return Ok(ProbeEventInput::hook_stop_with_context(
            session_id.as_deref(),
            turn_id.as_deref(),
            session_id.as_deref(),
            None,
            None,
            "reply-needed",
        )
        .with_body_source(Some("request_user_input"))
        .with_pre_tool_use_scan_source()
        .with_call_id(tool_use_id.as_deref())
        .with_suppression_reason(Some(reason)));
    }
    let hook_payload = validate_hook_request_user_input_payload(&payload)?;
    let session_id = required_hook_field(&hook_payload.session_id, "session_id")?;
    let turn_id = required_hook_field(&hook_payload.turn_id, "turn_id")?;
    let tool_use_id = required_hook_field(&hook_payload.tool_use_id, "tool_use_id")?;
    let questions = agenthub_core::codex::normalize_user_input_questions(
        &json!({"questions":hook_payload.tool_input.questions}),
    )
    .context("invalid native questions")?;
    let elicitation = PendingElicitation {
        turn_id: Some(turn_id.to_string()),
        item_id: Some(tool_use_id.to_string()),
        questions,
    };
    let body = probe_service::format_probe_pending_elicitation(&elicitation);
    let thread_title = hook_thread_title(config, Some(&payload), Some(session_id));
    let transcript_path = read_string_field(&payload, &["transcript_path", "transcriptPath"]);

    Ok(ProbeEventInput::hook_stop_with_context(
        Some(session_id),
        Some(turn_id),
        Some(session_id),
        transcript_path.as_deref(),
        Some(&body),
        "reply-needed",
    )
    .with_body_source(Some("request_user_input"))
    .with_pre_tool_use_scan_source()
    .with_call_id(Some(tool_use_id))
    .with_thread_title(thread_title.as_deref()))
}

pub(super) fn question_confirmation_diagnostics() -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            "question_confirmation_strategy".to_string(),
            json!("rollout_unresolved_after_grace"),
        ),
        (
            "question_confirmation_delay_ms".to_string(),
            json!(QUESTION_CONFIRMATION_DELAY_MS),
        ),
        ("question_confirmed_pending".to_string(), json!(true)),
    ])
}

pub(super) fn required_hook_field<'a>(value: &'a str, name: &str) -> Result<&'a str> {
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "missing {name}");
    Ok(value)
}
pub(super) fn hook_thread_title(
    config: &Config,
    payload: Option<&Value>,
    thread_id: Option<&str>,
) -> Option<String> {
    payload
        .and_then(|value| read_string_field(value, &["thread_title", "threadTitle", "title"]))
        .or_else(|| {
            thread_id.and_then(|thread_id| local_thread_title(config, thread_id).ok().flatten())
        })
}

pub(super) fn hook_stop_event_input(
    config: &Config,
    stdin_payload: Option<&Value>,
    cli_thread_id: Option<&str>,
    cli_turn_id: Option<&str>,
    kind: &str,
) -> Result<ProbeEventInput> {
    let payload_thread_id = stdin_payload.as_ref().and_then(|value| {
        read_string_field(value, &["thread_id", "threadId", "session_id", "sessionId"])
    });
    let payload_turn_id = stdin_payload
        .as_ref()
        .and_then(|value| read_string_field(value, &["turn_id", "turnId"]));
    let payload_session_id = stdin_payload
        .as_ref()
        .and_then(|value| read_string_field(value, &["session_id", "sessionId"]));
    let payload_transcript_path = stdin_payload
        .as_ref()
        .and_then(|value| read_string_field(value, &["transcript_path", "transcriptPath"]));
    let payload_last_assistant_message = stdin_payload.as_ref().and_then(|value| {
        read_string_field(value, &["last_assistant_message", "lastAssistantMessage"])
    });
    let event_thread_id = cli_thread_id
        .map(str::to_string)
        .or(payload_thread_id.clone())
        .or(payload_session_id.clone());
    let event_turn_id = cli_turn_id.map(str::to_string).or(payload_turn_id.clone());
    let event_kind = stdin_payload
        .as_ref()
        .and_then(|value| read_string_field(value, &["kind", "event_kind", "eventKind"]))
        .unwrap_or_else(|| kind.to_string());
    if let Some(reason) = stdin_payload
        .and_then(|payload| hook_memory_suppression_reason(config, payload))
        .or_else(|| task_notification_suppression_reason(config, event_thread_id.as_deref()))
    {
        return Ok(ProbeEventInput::hook_stop_with_context(
            event_thread_id.as_deref(),
            event_turn_id.as_deref(),
            payload_session_id.as_deref(),
            None,
            None,
            &event_kind,
        )
        .with_suppression_reason(Some(reason)));
    }
    let resolved_last_assistant_message = hook_stop_last_assistant_message(
        payload_transcript_path.as_deref(),
        event_turn_id.as_deref(),
        payload_last_assistant_message.as_deref(),
    )?;
    if resolved_last_assistant_message.is_none() {
        return Ok(ProbeEventInput::hook_stop_with_context(
            event_thread_id.as_deref(),
            event_turn_id.as_deref(),
            payload_session_id.as_deref(),
            None,
            None,
            &event_kind,
        )
        .with_suppression_reason(Some("unconfirmed_final_body")));
    }
    let thread_title = hook_thread_title(config, stdin_payload, event_thread_id.as_deref());
    Ok(ProbeEventInput::hook_stop_with_context(
        event_thread_id.as_deref(),
        event_turn_id.as_deref(),
        payload_session_id.as_deref(),
        payload_transcript_path.as_deref(),
        resolved_last_assistant_message
            .as_ref()
            .map(|selection| selection.message.as_str()),
        &event_kind,
    )
    .with_body_source(
        resolved_last_assistant_message
            .as_ref()
            .map(|selection| selection.source.as_str()),
    )
    .with_body_selection_diagnostics(selection_diagnostics(
        resolved_last_assistant_message.as_ref(),
    ))
    .with_thread_title(thread_title.as_deref()))
}

pub(super) fn hook_stop_last_assistant_message(
    transcript_path: Option<&str>,
    turn_id: Option<&str>,
    stdin_message: Option<&str>,
) -> Result<Option<TranscriptMessageSelection>> {
    if let Some(message) = stdin_message
        .filter(|message| agenthub_core::probe::is_probe_machine_control_payload(message))
    {
        return Ok(Some(TranscriptMessageSelection::from_stdin(
            message,
            "internal_control_payload",
        )));
    }
    if let Some(path) = transcript_path
        .filter(|path| !path.trim().is_empty())
        .map(Path::new)
    {
        if let Some(selection) =
            stable_transcript_selection(path, turn_id, TranscriptSelectionKind::HookStop)?
        {
            return Ok(Some(selection));
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TranscriptSelectionKind {
    HookStop,
    Completion,
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptMessageSelection {
    pub(super) message: String,
    pub(super) source: String,
    pub(super) strategy: String,
    pub(super) selected_turn_id: Option<String>,
    pub(super) selected_line: Option<usize>,
    pub(super) candidate_count: usize,
    pub(super) transcript_stabilized: bool,
    pub(super) transcript_size_before: Option<u64>,
    pub(super) transcript_size_after: Option<u64>,
}

impl TranscriptMessageSelection {
    fn from_rollout(selection: RolloutMessageSelection, stability: TranscriptStability) -> Self {
        Self {
            message: selection.message,
            source: selection.source,
            strategy: selection.strategy,
            selected_turn_id: selection.selected_turn_id,
            selected_line: selection.selected_line,
            candidate_count: selection.candidate_count,
            transcript_stabilized: stability.stabilized,
            transcript_size_before: stability.size_before,
            transcript_size_after: stability.size_after,
        }
    }

    fn from_stdin(message: &str, source: &str) -> Self {
        Self {
            message: message.to_string(),
            source: source.to_string(),
            strategy: source.to_string(),
            selected_turn_id: None,
            selected_line: None,
            candidate_count: 1,
            transcript_stabilized: false,
            transcript_size_before: None,
            transcript_size_after: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TranscriptStability {
    pub(super) stabilized: bool,
    pub(super) size_before: Option<u64>,
    pub(super) size_after: Option<u64>,
}

pub(super) fn stable_transcript_selection(
    path: &Path,
    turn_id: Option<&str>,
    kind: TranscriptSelectionKind,
) -> Result<Option<TranscriptMessageSelection>> {
    let size_before = fs::metadata(path).ok().map(|metadata| metadata.len());
    let mut last_size = size_before;
    let mut stable_reads = 0usize;
    let mut latest_selection = transcript_selection(path, turn_id, kind)?;
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(50));
        let current_size = fs::metadata(path).ok().map(|metadata| metadata.len());
        let changed = current_size != last_size;
        if !changed {
            stable_reads += 1;
        } else {
            stable_reads = 0;
        }
        last_size = current_size;
        if changed {
            latest_selection = transcript_selection(path, turn_id, kind)?;
        }
        if stable_reads >= 2 && latest_selection.is_some() {
            break;
        }
    }
    let stability = TranscriptStability {
        stabilized: stable_reads >= 2,
        size_before,
        size_after: last_size,
    };
    Ok(latest_selection
        .map(|selection| TranscriptMessageSelection::from_rollout(selection, stability)))
}

pub(super) fn transcript_selection(
    path: &Path,
    turn_id: Option<&str>,
    kind: TranscriptSelectionKind,
) -> Result<Option<RolloutMessageSelection>> {
    match kind {
        TranscriptSelectionKind::HookStop => rollout_hook_stop_message_selection(path, turn_id),
        TranscriptSelectionKind::Completion => {
            rollout_completion_last_agent_message_selection(path, turn_id)
        }
    }
}

pub(super) fn selection_diagnostics(
    selection: Option<&TranscriptMessageSelection>,
) -> BTreeMap<String, Value> {
    let mut diagnostics = BTreeMap::new();
    diagnostics.insert(
        "body_selection_strategy".to_string(),
        selection
            .map(|value| json!(value.strategy))
            .unwrap_or(Value::Null),
    );
    diagnostics.insert(
        "body_selected_turn_id".to_string(),
        selection
            .and_then(|value| value.selected_turn_id.as_deref())
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    diagnostics.insert(
        "body_selected_line".to_string(),
        selection
            .and_then(|value| value.selected_line)
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    diagnostics.insert(
        "body_candidate_count".to_string(),
        selection
            .map(|value| json!(value.candidate_count))
            .unwrap_or_else(|| json!(0)),
    );
    diagnostics.insert(
        "transcript_stabilized".to_string(),
        selection
            .map(|value| json!(value.transcript_stabilized))
            .unwrap_or(Value::Null),
    );
    diagnostics.insert(
        "transcript_size_before".to_string(),
        selection
            .and_then(|value| value.transcript_size_before)
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    diagnostics.insert(
        "transcript_size_after".to_string(),
        selection
            .and_then(|value| value.transcript_size_after)
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
    );
    diagnostics
}

pub(super) fn read_optional_stdin_json() -> Result<Option<Value>> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }
    let mut buffer = String::new();
    let mut lock = stdin.lock();
    lock.read_to_string(&mut buffer)
        .context("read hook stop stdin")?;
    if buffer.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&buffer)
        .map(Some)
        .context("parse stdin json")
}

pub(super) fn read_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

pub(super) fn hook_context_suppression_reason(
    config: &Config,
    payload: &Value,
) -> Option<&'static str> {
    hook_memory_suppression_reason(config, payload).or_else(|| {
        let id = read_string_field(
            payload,
            &["thread_id", "threadId", "session_id", "sessionId"],
        );
        task_notification_suppression_reason(config, id.as_deref())
    })
}

pub(super) fn hook_memory_suppression_reason(
    config: &Config,
    payload: &Value,
) -> Option<&'static str> {
    let transcript_path = hook_transcript_path_for_identity(payload).flatten();
    let cwd = read_string_field(payload, &["cwd"]);
    probe_runtime(config)
        .hook_context_suppression_reason(cwd.as_deref(), transcript_path.as_deref())
        .filter(|_| hook_transcript_path_for_identity(payload).is_some())
}

pub(super) fn task_notification_suppression_reason(
    config: &Config,
    thread_id: Option<&str>,
) -> Option<&'static str> {
    let Some(id) = thread_id else {
        return Some("unconfirmed_task_identity");
    };
    let paths = resolve_codex_paths(&config.codex.home).codex_paths();
    let identity = match agenthub_core::codex::codex_task_identity(&paths, id) {
        Ok(identity) => identity,
        Err(_) => return Some("unconfirmed_task_identity"),
    };
    identity.suppression_reason()
}

pub(super) fn hook_transcript_path_for_identity(payload: &Value) -> Option<Option<String>> {
    let mut transcript_path = None;
    for key in ["transcript_path", "transcriptPath"] {
        let Some(value) = payload.get(key) else {
            continue;
        };
        match value {
            Value::Null => {}
            Value::String(path) if path.trim().is_empty() => {}
            Value::String(path) => transcript_path = Some(path.trim().to_string()),
            _ => return None,
        }
    }
    Some(transcript_path)
}

pub(super) fn notify_completion_context(
    config: &Config,
    payload: Option<&Value>,
    cli_thread_id: Option<&str>,
    cli_turn_id: Option<&str>,
) -> Result<ProbeEventInput> {
    let payload_thread_id = payload.and_then(|value| {
        read_string_field(value, &["thread_id", "threadId", "session_id", "sessionId"])
    });
    let payload_turn_id =
        payload.and_then(|value| read_string_field(value, &["turn_id", "turnId"]));
    let thread_id = cli_thread_id.map(str::to_string).or(payload_thread_id);
    let turn_id = cli_turn_id.map(str::to_string).or(payload_turn_id);
    let session_id =
        payload.and_then(|value| read_string_field(value, &["session_id", "sessionId"]));
    if let Some(reason) = payload
        .and_then(|value| hook_memory_suppression_reason(config, value))
        .or_else(|| {
            task_notification_suppression_reason(
                config,
                thread_id.as_deref().or(session_id.as_deref()),
            )
        })
    {
        return Ok(ProbeEventInput::notify_completion_with_context(
            thread_id.as_deref().or(session_id.as_deref()),
            turn_id.as_deref(),
            session_id.as_deref(),
            None,
            None,
            None,
        )
        .with_suppression_reason(Some(reason)));
    }
    let transcript_path =
        payload.and_then(|value| read_string_field(value, &["transcript_path", "transcriptPath"]));
    let payload_thread_title = payload
        .and_then(|value| read_string_field(value, &["thread_title", "threadTitle", "title"]));
    let resolved_thread = if transcript_path.is_none() && thread_id.is_some() {
        notify_completion_thread_summary(config, thread_id.as_deref().unwrap())
            .ok()
            .flatten()
    } else {
        None
    };
    let resolved_transcript_path = transcript_path.or_else(|| {
        resolved_thread
            .as_ref()
            .and_then(|thread| thread.rollout_path.as_ref())
            .map(|path| path.to_string_lossy().to_string())
    });
    let thread_title = payload_thread_title.or_else(|| {
        resolved_thread
            .as_ref()
            .map(|thread| thread.title.clone())
            .filter(|title| !title.trim().is_empty())
    });
    let transcript_message = if let Some(path) = resolved_transcript_path.as_deref() {
        stable_transcript_selection(
            Path::new(path),
            turn_id.as_deref(),
            TranscriptSelectionKind::Completion,
        )?
    } else {
        None
    };
    let (message, body_source, diagnostics) = if let Some(selection) = transcript_message {
        (
            Some(selection.message.clone()),
            Some(selection.source.clone()),
            selection_diagnostics(Some(&selection)),
        )
    } else {
        return Ok(ProbeEventInput::notify_completion_with_context(
            thread_id.as_deref().or(session_id.as_deref()),
            turn_id.as_deref(),
            session_id.as_deref(),
            None,
            None,
            None,
        )
        .with_suppression_reason(Some("unconfirmed_final_body")));
    };
    if body_source
        .as_deref()
        .is_some_and(|source| matches!(source, "proposed_plan" | "request_user_input"))
    {
        return Ok(ProbeEventInput::hook_stop_with_context(
            thread_id.as_deref().or(session_id.as_deref()),
            turn_id.as_deref(),
            session_id.as_deref(),
            resolved_transcript_path.as_deref(),
            message.as_deref(),
            "reply-needed",
        )
        .with_body_source(body_source.as_deref())
        .with_body_selection_diagnostics(diagnostics)
        .with_thread_title(thread_title.as_deref()));
    }
    Ok(ProbeEventInput::notify_completion_with_context(
        thread_id.as_deref().or(session_id.as_deref()),
        turn_id.as_deref(),
        session_id.as_deref(),
        resolved_transcript_path.as_deref(),
        message.as_deref(),
        body_source.as_deref(),
    )
    .with_body_selection_diagnostics(diagnostics)
    .with_thread_title(thread_title.as_deref()))
}

pub(super) fn notify_completion_thread_summary(
    config: &Config,
    thread_id: &str,
) -> Result<Option<agenthub_core::codex::ThreadSummary>> {
    let resolved = resolve_codex_paths(&config.codex.home);
    agenthub_core::codex::local_thread_summary(&resolved.codex_paths(), thread_id)
}

pub(super) fn local_thread_title(config: &Config, thread_id: &str) -> Result<Option<String>> {
    Ok(notify_completion_thread_summary(config, thread_id)?
        .map(|thread| thread.title)
        .filter(|title| !title.trim().is_empty() && title.trim() != "未命名线程"))
}
