//! Serialized owner of a live desktop session and AgentHub's outgoing queue.
//! Dropping UI callbacks never replays a command. Native execution remains in
//! the existing desktop process; this worker only follows and submits to it.
use super::{
    commands::{CollaborationMode, DesktopCommand, DesktopReply, MessageInput},
    projection::{self, bounded, state_request_key},
    references::NativeReference,
    DesktopError, DesktopSession, Result,
};
use crate::{
    db::desktop_submissions::{DesktopLedger, Submission},
    services::codex_desktop::{DesktopAction, DesktopMode, DesktopQueuedMessage, DesktopView},
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch};

struct ActionRequest {
    action: Option<DesktopAction>,
    expected_session: String,
    references: Vec<NativeReference>,
    response: oneshot::Sender<Result<DesktopView>>,
}

#[derive(Clone)]
pub struct DesktopWorker {
    actions: mpsc::Sender<ActionRequest>,
    view: watch::Receiver<DesktopView>,
    project_directory: Option<std::path::PathBuf>,
}

impl DesktopWorker {
    #[cfg(test)]
    pub async fn spawn(
        session: DesktopSession,
        ledger: DesktopLedger,
        source: String,
    ) -> Result<Self> {
        Self::spawn_inner(session, ledger, source, None).await
    }

    pub async fn spawn_checked(
        session: DesktopSession,
        ledger: DesktopLedger,
        source: String,
        paths: crate::codex::CodexPaths,
    ) -> Result<Self> {
        Self::spawn_inner(session, ledger, source, Some(paths)).await
    }

    async fn spawn_inner(
        session: DesktopSession,
        ledger: DesktopLedger,
        source: String,
        paths: Option<crate::codex::CodexPaths>,
    ) -> Result<Self> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let project_directory = session.state().project_directory().ok();
        let thread_id = session.state().thread_id().to_owned();
        let list_source = source.clone();
        let list_thread = thread_id.clone();
        let pending = storage(&ledger, move |ledger| {
            ledger.list(&list_source, &list_thread)
        })
        .await?;
        let initial = projection::view(session.state(), &session_id, queue_view(&pending));
        let (view_tx, view) = watch::channel(initial);
        let (actions, action_rx) = mpsc::channel(16);
        let session_owner = session.owner_id().to_owned();
        tokio::spawn(
            WorkerLoop {
                session,
                ledger,
                source,
                thread_id,
                session_id,
                pending,
                paths,
                wait_for_turn: None,
                last_error: None,
                connection_phase: "connected",
                confirmed_owner: session_owner,
                resync_used: false,
                reconnect_attempts: 0,
                view: view_tx,
                actions: action_rx,
            }
            .run(),
        );
        Ok(Self {
            actions,
            view,
            project_directory,
        })
    }

    pub fn current(&self) -> DesktopView {
        self.view.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<DesktopView> {
        self.view.clone()
    }
    pub fn is_closed(&self) -> bool {
        self.actions.is_closed()
    }
    pub fn project_directory(&self) -> Option<&std::path::Path> {
        self.project_directory.as_deref()
    }

    pub async fn act(&self, action: DesktopAction) -> Result<DesktopView> {
        self.act_with_references(action, Vec::new()).await
    }

    pub async fn act_with_references(
        &self,
        action: DesktopAction,
        references: Vec<NativeReference>,
    ) -> Result<DesktopView> {
        self.act_in_session(action, references, self.current().session_id)
            .await
    }

    pub async fn act_in_session(
        &self,
        action: DesktopAction,
        references: Vec<NativeReference>,
        expected_session: String,
    ) -> Result<DesktopView> {
        if self.current().session_id != expected_session {
            return Err(DesktopError::RequestExpired);
        }
        if !self.current().writable {
            return Err(DesktopError::ReadOnly);
        }
        validate_action(&action)?;
        let count = match &action {
            DesktopAction::Send { references, .. } | DesktopAction::Steer { references, .. } => {
                references.len()
            }
            _ => 0,
        };
        if count != references.len() {
            return Err(DesktopError::InvalidInput);
        }
        self.request(Some(action), references, expected_session)
            .await
    }

    /// Only the explicit host Connect entrance may confirm a recovered owner.
    pub async fn confirm_owner(&self) -> Result<DesktopView> {
        self.request(None, Vec::new(), self.current().session_id)
            .await
    }

    async fn request(
        &self,
        action: Option<DesktopAction>,
        references: Vec<NativeReference>,
        expected_session: String,
    ) -> Result<DesktopView> {
        let (response, receive) = oneshot::channel();
        self.actions
            .try_send(ActionRequest {
                action,
                references,
                expected_session,
                response,
            })
            .map_err(|_| DesktopError::Busy)?;
        receive.await.map_err(|_| DesktopError::Disconnected)?
    }
}

struct WorkerLoop {
    session: DesktopSession,
    ledger: DesktopLedger,
    source: String,
    thread_id: String,
    session_id: String,
    pending: Vec<Submission>,
    paths: Option<crate::codex::CodexPaths>,
    wait_for_turn: Option<String>,
    last_error: Option<DesktopError>,
    connection_phase: &'static str,
    confirmed_owner: String,
    resync_used: bool,
    reconnect_attempts: usize,
    view: watch::Sender<DesktopView>,
    actions: mpsc::Receiver<ActionRequest>,
}

impl WorkerLoop {
    async fn run(mut self) {
        if let Err(error) = self.reconcile().await {
            self.publish(Some(error));
        }
        loop {
            let event = tokio::select! {
                event = self.session.next_change() => {
                    if event.is_ok() { self.reconnect_attempts = 0; }
                    event
                },
                action = self.actions.recv() => {
                    let Some(action) = action else {break};
                    let result = if action.expected_session != self.session_id { Err(DesktopError::RequestExpired) }
                        else if let Some(command) = action.action { self.action(command, action.references).await }
                        else { self.confirm().await };
                    if result.is_ok() { self.last_error = None; }
                    let error = result.as_ref().err().copied();
                    self.publish(error);
                    let _ = action.response.send(result.map(|()|self.view.borrow().clone()));
                    if !self.session.is_connected() { Err(error.unwrap_or(DesktopError::Disconnected)) } else { Ok(()) }
                },
            };
            if let Err(error) = event {
                if !self.recover(error).await {
                    break;
                }
            }
            if let Err(error) = self.reconcile().await {
                self.publish(Some(error));
            }
            if let Err(error) = self.dispatch_queued().await {
                self.publish(Some(error));
            } else {
                self.publish(None);
            }
        }
        self.session.close().await;
    }

    fn make_view(&self) -> DesktopView {
        let mut view = projection::view(
            self.session.state(),
            &self.session_id,
            queue_view(&self.pending),
        );
        view.connection_phase = Some(self.connection_phase.into());
        view.connected = matches!(
            self.connection_phase,
            "connected" | "owner_confirmation_required"
        );
        view.writable &=
            self.connection_phase == "connected" && self.session.owner_id() == self.confirmed_owner;
        if !view.connected {
            view.requests.clear();
        }
        view
    }

    async fn confirm(&mut self) -> Result<()> {
        if !self.session.is_connected() {
            return Err(DesktopError::Disconnected);
        }
        self.session.verify_owner().await?;
        self.session.state().check_writable()?;
        self.confirmed_owner = self.session.owner_id().to_owned();
        self.connection_phase = "connected";
        self.resync_used = false;
        Ok(())
    }

    async fn recover(&mut self, error: DesktopError) -> bool {
        let source = self.source.clone();
        let thread = self.thread_id.clone();
        if storage(&self.ledger, move |ledger| ledger.hold(&source, &thread))
            .await
            .is_err()
            || self.refresh_queue().await.is_err()
        {
            self.connection_phase = "disconnected";
            self.publish(Some(DesktopError::Unavailable));
            return false;
        }
        let Some(home) = self.session.codex_home().map(std::path::Path::to_owned) else {
            self.connection_phase = "disconnected";
            self.publish(Some(error));
            return false;
        };
        let resync = error == DesktopError::OutOfSync && !self.resync_used;
        let reconnect = matches!(
            error,
            DesktopError::Disconnected
                | DesktopError::OwnerUnavailable
                | DesktopError::Timeout
                | DesktopError::OutcomeUnknown
        );
        if !resync && !reconnect {
            self.connection_phase = "disconnected";
            self.publish(Some(error));
            return false;
        }
        if resync {
            self.resync_used = true;
        }
        self.connection_phase = if resync { "resyncing" } else { "reconnecting" };
        // Rotate before waiting so old actions, grants and UI responses cannot
        // be accepted during recovery even if the new snapshot has revision 1.
        self.session_id = uuid::Uuid::new_v4().to_string();
        self.publish(Some(error));
        loop {
            let delay = if resync {
                0
            } else {
                let Some(delay) = [1, 2, 4, 8, 16].get(self.reconnect_attempts) else {
                    break;
                };
                self.reconnect_attempts += 1;
                *delay
            };
            let home = home.clone();
            let thread = self.thread_id.clone();
            let attempt = async move {
                if delay > 0 {
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                }
                DesktopSession::attach(&home, &thread).await
            };
            tokio::pin!(attempt);
            let attempt = loop {
                tokio::select! {
                    result = &mut attempt => break result,
                    request = self.actions.recv() => {
                        let Some(request) = request else { return false; };
                        let _ = request.response.send(Err(DesktopError::ReadOnly));
                    }
                }
            };
            match attempt {
                Ok(session) => {
                    let old = std::mem::replace(&mut self.session, session);
                    old.close().await;
                    self.project_after_recovery();
                    return true;
                }
                Err(failure) => {
                    super::diagnostics::record(
                        super::diagnostics::Source::Ipc,
                        "connect",
                        None,
                        None,
                        None,
                        failure,
                    );
                    if resync
                        || !matches!(
                            failure,
                            DesktopError::Unavailable
                                | DesktopError::Disconnected
                                | DesktopError::OwnerUnavailable
                                | DesktopError::Timeout
                        )
                    {
                        break;
                    }
                }
            }
        }
        self.connection_phase = "disconnected";
        self.publish(Some(error));
        false
    }

    fn project_after_recovery(&mut self) {
        self.connection_phase = if self.session.owner_id() == self.confirmed_owner {
            "connected"
        } else {
            "owner_confirmation_required"
        };
        self.last_error = None;
        self.publish(None);
    }

    fn publish(&mut self, error: Option<DesktopError>) {
        if error.is_some() {
            self.last_error = error;
        }
        let mut view = self.make_view();
        view.error = self.last_error.map(|error| error.user_message().into());
        self.view.send_replace(view);
    }
    async fn refresh_queue(&mut self) -> Result<()> {
        let source = self.source.clone();
        let thread = self.thread_id.clone();
        self.pending = storage(&self.ledger, move |ledger| ledger.list(&source, &thread)).await?;
        Ok(())
    }

    async fn action(
        &mut self,
        action: DesktopAction,
        resolved: Vec<NativeReference>,
    ) -> Result<()> {
        if self.connection_phase != "connected" || self.session.owner_id() != self.confirmed_owner {
            return Err(DesktopError::ReadOnly);
        }
        self.session.state().check_writable()?;
        let spans = match &action {
            DesktopAction::Send {
                message_id,
                text,
                references,
                reference_spans,
            }
            | DesktopAction::Steer {
                message_id,
                text,
                references,
                reference_spans,
                ..
            } => {
                let spans = super::references::resolve_spans(references, reference_spans)?;
                MessageInput {
                    native_mentions: true,
                    client_id: message_id.clone(),
                    text: text.clone(),
                    references: resolved.clone(),
                    reference_spans: spans.clone(),
                }
                .validate()?;
                spans
            }
            _ => Vec::new(),
        };
        match action {
            DesktopAction::Send {
                message_id, text, ..
            } => {
                let source = self.source.clone();
                let thread = self.thread_id.clone();
                storage(&self.ledger, move |ledger| {
                    ledger.enqueue(
                        &source,
                        &thread,
                        &message_id,
                        &submission_body("queue", &text, &resolved, &spans),
                    )
                })
                .await?;
                self.refresh_queue().await?;
            }
            DesktopAction::Steer {
                message_id,
                text,
                turn_id,
                ..
            } => {
                if self.session.state().active_turn_id()? != turn_id {
                    return Err(DesktopError::RequestExpired);
                }
                let source = self.source.clone();
                let thread = self.thread_id.clone();
                let id = message_id.clone();
                let body_text = text.clone();
                let body_references = resolved.clone();
                let body_spans = spans.clone();
                storage(&self.ledger, move |ledger| {
                    ledger.enqueue(
                        &source,
                        &thread,
                        &id,
                        &submission_body("steer", &body_text, &body_references, &body_spans),
                    )
                })
                .await?;
                self.refresh_queue().await?;
                let source = self.source.clone();
                let thread = self.thread_id.clone();
                let id = message_id.clone();
                if !storage(&self.ledger, move |ledger| {
                    ledger.claim_immediate(&source, &thread, &id)
                })
                .await?
                {
                    let source = self.source.clone();
                    let thread = self.thread_id.clone();
                    let id = message_id.clone();
                    storage(&self.ledger, move |ledger| {
                        ledger.hold_queued(&source, &thread, &id)
                    })
                    .await?;
                    self.refresh_queue().await?;
                    return Err(DesktopError::Busy);
                }
                self.submit(
                    &message_id,
                    DesktopCommand::Steer {
                        message: MessageInput {
                            native_mentions: true,
                            client_id: message_id.clone(),
                            text,
                            references: resolved,
                            reference_spans: spans,
                        },
                        expected_turn_id: turn_id,
                    },
                )
                .await?;
            }
            DesktopAction::Stop { turn_id } => {
                let source = self.source.clone();
                let thread = self.thread_id.clone();
                storage(&self.ledger, move |ledger| ledger.hold(&source, &thread)).await?;
                self.refresh_queue().await?;
                self.execute(&DesktopCommand::Stop {
                    expected_turn_id: turn_id,
                })
                .await?;
            }
            DesktopAction::CancelQueued { message_id } => {
                let source = self.source.clone();
                let thread = self.thread_id.clone();
                if !storage(&self.ledger, move |ledger| {
                    ledger.cancel(&source, &thread, &message_id)
                })
                .await?
                {
                    return Err(DesktopError::RequestExpired);
                }
                self.refresh_queue().await?;
            }
            DesktopAction::ResumeQueue => {
                let source = self.source.clone();
                let thread = self.thread_id.clone();
                let ids: Vec<_> = self
                    .pending
                    .iter()
                    .filter(|item| {
                        item.status == "held"
                            && item
                                .body
                                .as_ref()
                                .is_some_and(|body| body["kind"] == "queue")
                    })
                    .map(|item| item.id.clone())
                    .collect();
                storage(&self.ledger, move |ledger| {
                    ledger.resume_selected(&source, &thread, &ids)
                })
                .await?;
                self.refresh_queue().await?;
            }
            DesktopAction::DismissUnknown { message_id } => {
                let source = self.source.clone();
                let thread = self.thread_id.clone();
                if !storage(&self.ledger, move |ledger| {
                    ledger.dismiss_unknown(&source, &thread, &message_id)
                })
                .await?
                {
                    return Err(DesktopError::RequestExpired);
                }
                self.refresh_queue().await?;
            }
            DesktopAction::Settings {
                model,
                reasoning_effort,
                mode,
            } => {
                self.execute(&DesktopCommand::Settings {
                    model,
                    reasoning_effort,
                    mode: match mode {
                        DesktopMode::Default => CollaborationMode::Default,
                        DesktopMode::Plan => CollaborationMode::Plan,
                    },
                })
                .await?;
            }
            DesktopAction::Decide {
                request_key,
                allow,
                decision,
            } => {
                self.reply(
                    &request_key,
                    DesktopReply::decision(allow, decision.as_deref())?,
                )
                .await?;
            }
            DesktopAction::Answer {
                request_key,
                answers,
            } => {
                self.reply(
                    &request_key,
                    DesktopReply::Answers {
                        answers: answers
                            .into_iter()
                            .map(|answer| (answer.question_id, answer.answers))
                            .collect(),
                    },
                )
                .await?;
            }
            DesktopAction::Elicitation {
                request_key,
                accept,
                content,
            } => {
                self.reply(&request_key, DesktopReply::Elicitation { accept, content })
                    .await?;
            }
        }
        Ok(())
    }

    async fn reply(&mut self, key: &str, response: DesktopReply) -> Result<()> {
        let request = self
            .session
            .state()
            .pending_requests()
            .find(|request| state_request_key(self.session.state(), request) == key)
            .ok_or(DesktopError::RequestExpired)?;
        let command = DesktopCommand::Reply {
            request_id: request["id"].clone(),
            expected_request_key: key.into(),
            method: request["method"]
                .as_str()
                .ok_or(DesktopError::InvalidFrame)?
                .into(),
            expected_turn_id: request["params"]["turnId"].as_str().map(str::to_owned),
            response,
        };
        self.execute(&command).await?;
        Ok(())
    }

    async fn reconcile(&mut self) -> Result<()> {
        let confirmed: Vec<_> = self
            .pending
            .iter()
            .filter(|item| matches!(item.status.as_str(), "dispatching" | "outcome_unknown"))
            .filter_map(|item| {
                self.session
                    .state()
                    .submission_turn_id(&item.id)
                    .map(|turn| (item.id.clone(), turn))
            })
            .collect();
        if confirmed.is_empty() {
            return Ok(());
        }
        let source = self.source.clone();
        let thread = self.thread_id.clone();
        storage(&self.ledger, move |ledger| {
            for (id, turn) in confirmed {
                ledger.accepted(&source, &thread, &id, &turn)?;
            }
            Ok(())
        })
        .await?;
        self.refresh_queue().await
    }

    async fn dispatch_queued(&mut self) -> Result<()> {
        if self.connection_phase != "connected"
            || self.session.owner_id() != self.confirmed_owner
            || self.session.state().check_idle().is_err()
        {
            return Ok(());
        }
        if let Some(turn) = &self.wait_for_turn {
            // The start response may arrive before the active-state broadcast.
            // Do not interpret that old idle snapshot as permission to send twice.
            if !self.session.state().turn_finished(turn) {
                return Ok(());
            }
            self.wait_for_turn = None;
        }
        let Some(item) = self.pending.first() else {
            return Ok(());
        };
        if item.status != "queued" {
            return Ok(());
        }
        let body = item.body.as_ref().ok_or(DesktopError::InvalidInput)?;
        if body["kind"] != "queue" {
            return Ok(());
        }
        let text = body["text"]
            .as_str()
            .ok_or(DesktopError::InvalidInput)?
            .to_owned();
        let references =
            serde_json::from_value(body.get("references").cloned().unwrap_or_else(|| json!([])))
                .map_err(|_| DesktopError::InvalidInput)?;
        let reference_spans = serde_json::from_value(
            body.get("referenceSpans")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .map_err(|_| DesktopError::InvalidInput)?;
        let native_mentions = match body.get("nativeMentions") {
            None => false,
            Some(Value::Bool(value)) => *value,
            _ => return Err(DesktopError::InvalidInput),
        };
        let id = item.id.clone();
        let claimed_id = id.clone();
        let source = self.source.clone();
        let thread = self.thread_id.clone();
        if !storage(&self.ledger, move |ledger| {
            ledger.claim(&source, &thread, &claimed_id)
        })
        .await?
        {
            return Ok(());
        }
        self.submit(
            &id,
            DesktopCommand::Start(MessageInput {
                native_mentions,
                client_id: id.clone(),
                text,
                references,
                reference_spans,
            }),
        )
        .await
    }

    async fn submit(&mut self, id: &str, command: DesktopCommand) -> Result<()> {
        self.refresh_queue().await?;
        self.publish(None);
        let result = self.execute(&command).await;
        let source = self.source.clone();
        let thread = self.thread_id.clone();
        let id = id.to_owned();
        match result {
            Ok(result) => {
                let turn = result.turn_id.ok_or(DesktopError::OutcomeUnknown)?;
                self.wait_for_turn = Some(turn.clone());
                storage(&self.ledger, move |ledger| {
                    ledger.accepted(&source, &thread, &id, &turn)
                })
                .await?;
                self.refresh_queue().await?;
                Ok(())
            }
            Err(error) => {
                storage(&self.ledger, move |ledger| {
                    if matches!(
                        error,
                        DesktopError::OutcomeUnknown
                            | DesktopError::Timeout
                            | DesktopError::Disconnected
                    ) {
                        ledger.outcome_unknown(&source, &thread, &id)
                    } else {
                        ledger.not_sent(&source, &thread, &id)
                    }
                })
                .await?;
                self.refresh_queue().await?;
                Err(error)
            }
        }
    }

    async fn execute(
        &mut self,
        command: &DesktopCommand,
    ) -> Result<super::commands::CommandResult> {
        if let Some(paths) = self.paths.clone() {
            let thread = self.thread_id.clone();
            crate::read_tasks::spawn_blocking(move || {
                let summary = crate::codex::local_thread_summary(&paths, &thread)
                    .map_err(|_| DesktopError::Unavailable)?
                    .ok_or(DesktopError::ReadOnly)?;
                if summary.status == crate::codex::ThreadStatus::Archived {
                    return Err(DesktopError::ReadOnly);
                }
                Ok(())
            })
            .await
            .map_err(|_| DesktopError::Unavailable)??;
        }
        let session_id = &self.session_id;
        let queue = queue_view(&self.pending);
        let sender = &self.view;
        self.session
            .execute_observing(command, |state| {
                let mut view = projection::view(state, session_id, queue.clone());
                view.connection_phase = Some("connected".into());
                sender.send_replace(view);
            })
            .await
    }
}

fn queue_view(pending: &[Submission]) -> Vec<DesktopQueuedMessage> {
    pending
        .iter()
        .map(|item| DesktopQueuedMessage {
            id: item.id.clone(),
            status: item.status.clone(),
            kind: item
                .body
                .as_ref()
                .and_then(|body| body["kind"].as_str())
                .unwrap_or("queue")
                .into(),
            preview: bounded(
                item.body
                    .as_ref()
                    .and_then(|body| body["text"].as_str())
                    .unwrap_or(""),
                2048,
            ),
            created_at: item.created_at,
        })
        .collect()
}

async fn storage<T: Send + 'static>(
    ledger: &DesktopLedger,
    work: impl FnOnce(DesktopLedger) -> anyhow::Result<T> + Send + 'static,
) -> Result<T> {
    let ledger = ledger.clone();
    crate::read_tasks::spawn_blocking(move || work(ledger))
        .await
        .map_err(|_| DesktopError::Unavailable)?
        .map_err(|_| DesktopError::Unavailable)
}

fn submission_body(
    kind: &str,
    text: &str,
    references: &[NativeReference],
    spans: &[super::references::ReferenceSpan],
) -> serde_json::Value {
    let mut body = json!({"kind":kind,"text":text,"references":references});
    // Ledger identity compares decrypted bytes. Empty metadata MUST preserve
    // the exact 1.3.0 body, including retries after an application upgrade.
    if !spans.is_empty() {
        body["referenceSpans"] = json!(spans);
        body["nativeMentions"] = json!(true);
    }
    body
}

fn validate_action(action: &DesktopAction) -> Result<()> {
    // Bound the channel before taking ownership; input validation is repeated
    // by the native command builder immediately before dispatch.
    let bytes = serde_json::to_vec(action).map_err(|_| DesktopError::InvalidInput)?;
    if bytes.len() > 1024 * 1024 {
        return Err(DesktopError::InvalidInput);
    }
    match action {
        DesktopAction::Send {
            message_id,
            text,
            references,
            ..
        }
        | DesktopAction::Steer {
            message_id,
            text,
            references,
            ..
        } if uuid::Uuid::parse_str(message_id).is_err()
            || (text.trim().is_empty() && references.is_empty())
            || text.len() > 256 * 1024
            || text.contains('\0')
            || references.len() > 32 =>
        {
            return Err(DesktopError::InvalidInput);
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
