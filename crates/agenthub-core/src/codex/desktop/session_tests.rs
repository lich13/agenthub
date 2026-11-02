use super::super::{
    commands::{CollaborationMode, CommandResult, DesktopCommand, DesktopReply, MessageInput},
    framing::{write_frame, FrameReader, FrameWriter},
    references::NativeReference,
    state::{NativeState, RuntimeState},
    DesktopError, DesktopEvent, DesktopReadSession,
};
use super::DesktopSession;
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::{
    net::UnixStream,
    time::{timeout, Duration},
};

const THREAD_ID: &str = "11111111-1111-4111-8111-111111111111";
const CLIENT_ID: &str = "55555555-5555-4555-8555-555555555555";
const OWNER_ID: &str = "66666666-6666-4666-8666-666666666666";
const OTHER_OWNER_ID: &str = "77777777-7777-4777-8777-777777777777";
const TURN_ID: &str = "33333333-3333-4333-8333-333333333333";

struct ReferenceFixture(PathBuf);

impl ReferenceFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-desktop-session-references-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("synthetic reference fixture directory is created");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("synthetic reference fixture directory is private");
        Self(root)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("synthetic reference fixture file is written");
        path
    }
}

impl Drop for ReferenceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn idle_state() -> Value {
    json!({
        "id": THREAD_ID,
        "hostId": "local",
        "parentThreadId": null,
        "archived": false,
        "source": {},
        "threadSource": {},
        "threadRuntimeStatus": {"type": "idle"},
        "turns": [],
        "requests": []
    })
}

fn session_pair() -> (DesktopSession, UnixStream) {
    session_pair_with_state(idle_state())
}

fn active_state() -> Value {
    json!({
        "id": THREAD_ID,
        "hostId": "local",
        "parentThreadId": null,
        "archived": false,
        "source": {},
        "threadSource": {},
        "threadRuntimeStatus": {"type": "active"},
        "turns": [{"id": TURN_ID, "status": "inProgress", "items": []}],
        "requests": [],
        "cwd": "/fixture/workspace"
    })
}

fn active_state_with_request(method: &str, params: Value) -> Value {
    let mut state = active_state();
    state["requests"] = json!([{
        "id": "fixture-pending-request",
        "method": method,
        "params": params
    }]);
    state
}

fn reply_command_for_state(
    initial_state: &Value,
    method: &str,
    response: DesktopReply,
) -> DesktopCommand {
    let mut state = NativeState::new(THREAD_ID).expect("fixture thread ID is valid");
    state
        .apply(DesktopEvent::Snapshot {
            revision: 1,
            state: initial_state.clone(),
        })
        .expect("fixture snapshot is accepted");
    let request_id = json!("fixture-pending-request");
    let request = state
        .pending_request(&request_id, method)
        .expect("fixture request is pending");
    DesktopCommand::Reply {
        request_id,
        expected_request_key: super::super::projection::state_request_key(&state, request),
        method: method.into(),
        expected_turn_id: Some(TURN_ID.into()),
        response,
    }
}

fn session_pair_with_state(initial_state: Value) -> (DesktopSession, UnixStream) {
    let (client, broker) = UnixStream::pair().expect("fixture socket pair is created");
    let mut state = NativeState::new(THREAD_ID).expect("fixture thread ID is valid");
    state
        .apply(DesktopEvent::Snapshot {
            revision: 1,
            state: initial_state,
        })
        .expect("fixture snapshot is accepted");
    let connection = DesktopReadSession {
        stream: Some(client),
        reader: FrameReader::default(),
        writer: FrameWriter::default(),
        conversation_id: THREAD_ID.into(),
        client_id: CLIENT_ID.into(),
        owner_id: OWNER_ID.into(),
        revision: Some(1),
    };
    (
        DesktopSession {
            connection,
            state,
            codex_home: None,
        },
        broker,
    )
}

async fn read_frame(stream: &mut UnixStream) -> std::result::Result<Value, DesktopError> {
    FrameReader::default().read(stream).await
}

async fn send_owner_response(stream: &mut UnixStream, request: &Value, owner_id: &str) {
    let response = json!({
        "type": "response",
        "requestId": request["requestId"],
        "resultType": "success",
        "handledByClientId": owner_id
    });
    write_frame(stream, &response)
        .await
        .expect("fixture owner response is written");
}

fn start_command() -> DesktopCommand {
    DesktopCommand::Start(MessageInput {
        native_mentions: false,
        reference_spans: Vec::new(),
        client_id: "88888888-8888-4888-8888-888888888888".into(),
        text: "fixture start".into(),
        references: vec![],
    })
}

fn settings_command() -> DesktopCommand {
    DesktopCommand::Settings {
        model: "fixture-model".into(),
        reasoning_effort: Some("medium".into()),
        mode: CollaborationMode::Default,
    }
}

fn steer_command() -> DesktopCommand {
    DesktopCommand::Steer {
        message: MessageInput {
            native_mentions: false,
            reference_spans: Vec::new(),
            client_id: "88888888-8888-4888-8888-888888888888".into(),
            text: "fixture steer".into(),
            references: vec![],
        },
        expected_turn_id: TURN_ID.into(),
    }
}

async fn execute_with_native_result(
    command: DesktopCommand,
    initial_state: Value,
    native_result: Value,
) -> std::result::Result<CommandResult, DesktopError> {
    let (mut session, mut broker) = session_pair_with_state(initial_state);
    let broker_task = tokio::spawn(async move {
        let recheck = read_frame(&mut broker)
            .await
            .expect("owner recheck is sent");
        assert_eq!(recheck["method"], "thread-owner-discovery");
        send_owner_response(&mut broker, &recheck, OWNER_ID).await;

        let mutation = read_frame(&mut broker)
            .await
            .expect("mutation is sent to the verified owner");
        let response = json!({
            "type": "response",
            "requestId": mutation["requestId"],
            "resultType": "success",
            "handledByClientId": OWNER_ID,
            "result": native_result
        });
        write_frame(&mut broker, &response)
            .await
            .expect("fixture mutation response is written");
    });

    let result = session.execute(&command).await;
    broker_task.await.expect("fixture broker task completes");
    result
}

async fn verified_mutation(stream: &mut UnixStream, method: &str) -> Value {
    let recheck = read_frame(stream).await.expect("owner recheck is sent");
    assert_eq!(recheck["method"], "thread-owner-discovery");
    send_owner_response(stream, &recheck, OWNER_ID).await;

    let mutation = read_frame(stream)
        .await
        .expect("mutation is sent after the owner recheck");
    assert_eq!(mutation["method"], method);
    mutation
}

async fn send_native_error(stream: &mut UnixStream, request: &Value, error: &str) {
    write_frame(
        stream,
        &json!({
            "type":"response",
            "requestId":request["requestId"],
            "resultType":"error",
            "handledByClientId":OWNER_ID,
            "error":error
        }),
    )
    .await
    .expect("fixture native error is written");
}

async fn send_native_success(stream: &mut UnixStream, request: &Value, result: Value) {
    write_frame(
        stream,
        &json!({
            "type":"response",
            "requestId":request["requestId"],
            "resultType":"success",
            "handledByClientId":OWNER_ID,
            "result":result
        }),
    )
    .await
    .expect("fixture native success is written");
}

#[tokio::test]
async fn owner_change_during_recheck_prevents_any_mutation_request() {
    let (mut session, mut broker) = session_pair();
    let broker_task = tokio::spawn(async move {
        let request = read_frame(&mut broker)
            .await
            .expect("owner recheck is sent");
        assert_eq!(request["method"], "thread-owner-discovery");
        assert!(request.get("targetClientId").is_none());
        send_owner_response(&mut broker, &request, OTHER_OWNER_ID).await;

        let next = timeout(Duration::from_secs(2), read_frame(&mut broker))
            .await
            .expect("session closes instead of sending a mutation")
            .expect_err("no mutation frame is sent after owner changes");
        assert_eq!(next, DesktopError::Disconnected);
    });

    let result = session.execute(&start_command()).await;
    assert!(matches!(result, Err(DesktopError::OwnerUnavailable)));
    broker_task.await.expect("fixture broker task completes");
}

#[tokio::test]
async fn each_dispatch_revalidates_references_before_owner_recheck() {
    let fixture = ReferenceFixture::new();
    let reference_path = fixture.file("fixture-reference.txt", b"synthetic selected reference");
    let reference = NativeReference::selected_file(reference_path.clone())
        .expect("synthetic file reference is selected and captured");
    fs::write(&reference_path, b"x").expect("synthetic reference is truncated after capture");

    let (mut session, mut broker) = session_pair();
    let broker_task = tokio::spawn(async move {
        let next = timeout(Duration::from_secs(2), read_frame(&mut broker))
            .await
            .expect("reference guard closes the fixture session without sending a request")
            .expect_err("no owner or mutation request follows an invalid reference");
        assert_eq!(next, DesktopError::Disconnected);
    });

    for client_id in [
        "88888888-8888-4888-8888-888888888888",
        "99999999-9999-4999-8999-999999999999",
    ] {
        let command = DesktopCommand::Start(MessageInput {
            native_mentions: false,
            reference_spans: Vec::new(),
            client_id: client_id.into(),
            text: "fixture reference validation".into(),
            references: vec![reference.clone()],
        });
        assert!(matches!(
            session.execute(&command).await,
            Err(DesktopError::RequestExpired)
        ));
    }

    drop(session);
    broker_task.await.expect("fixture broker task completes");
}

#[tokio::test]
async fn start_and_steer_require_nested_native_result_envelopes() {
    let start = execute_with_native_result(
        start_command(),
        idle_state(),
        json!({"result": {"turn": {"id": TURN_ID}}}),
    )
    .await
    .expect("nested native Start result is recognized");
    assert_eq!(start.turn_id.as_deref(), Some(TURN_ID));

    let steer = execute_with_native_result(
        steer_command(),
        active_state(),
        json!({"result": {"turnId": TURN_ID}}),
    )
    .await
    .expect("nested native Steer result is recognized");
    assert_eq!(steer.turn_id.as_deref(), Some(TURN_ID));

    let single_layer_start = execute_with_native_result(
        start_command(),
        idle_state(),
        json!({"turn": {"id": TURN_ID}}),
    )
    .await;
    assert!(matches!(
        single_layer_start,
        Err(DesktopError::OutcomeUnknown)
    ));

    let single_layer_steer =
        execute_with_native_result(steer_command(), active_state(), json!({"turnId": TURN_ID}))
            .await;
    assert!(matches!(
        single_layer_steer,
        Err(DesktopError::OutcomeUnknown)
    ));
}

#[tokio::test]
async fn state_patches_received_while_waiting_for_response_are_applied() {
    let (mut session, mut broker) = session_pair();
    let broker_task = tokio::spawn(async move {
        let recheck = read_frame(&mut broker)
            .await
            .expect("owner recheck is sent");
        assert_eq!(recheck["method"], "thread-owner-discovery");
        send_owner_response(&mut broker, &recheck, OWNER_ID).await;

        let mutation = read_frame(&mut broker)
            .await
            .expect("start request is sent after the owner recheck");
        assert_eq!(mutation["method"], "thread-follower-update-thread-settings");
        assert_eq!(mutation["targetClientId"], OWNER_ID);

        let patch = json!({
            "type": "broadcast",
            "sourceClientId": OWNER_ID,
            "method": "thread-stream-state-changed",
            "version": 11,
            "params": {
                "conversationId": THREAD_ID,
                "change": {
                    "type": "patches",
                    "revision": 2,
                    "baseRevision": 1,
                    "patches": [
                        {"op": "replace", "path": ["threadRuntimeStatus"], "value": {"type": "active"}},
                        {"op": "replace", "path": ["turns"], "value": [{"id": TURN_ID, "status": "inProgress", "items": []}]},
                        {"op": "replace", "path": ["requests"], "value": [{
                            "id": "fixture-pending-request",
                            "method": "item/commandExecution/requestApproval",
                            "params": {"threadId": THREAD_ID, "turnId": TURN_ID}
                        }]}
                    ]
                }
            }
        });
        write_frame(&mut broker, &patch)
            .await
            .expect("fixture state patch is written before the response");

        let response = json!({
            "type": "response",
            "requestId": mutation["requestId"],
            "resultType": "success",
            "handledByClientId": OWNER_ID,
            "result": {"applied": true}
        });
        write_frame(&mut broker, &response)
            .await
            .expect("fixture mutation response is written");
    });

    let result = session
        .execute(&settings_command())
        .await
        .expect("known native settings response is returned");
    assert!(result.turn_id.is_none());
    assert_eq!(session.state().revision(), Some(2));
    assert_eq!(session.state().runtime(), RuntimeState::Waiting);
    assert_eq!(
        session
            .state()
            .pending_request(
                &json!("fixture-pending-request"),
                "item/commandExecution/requestApproval"
            )
            .expect("request from interleaved patch remains available")["params"]["turnId"],
        TURN_ID
    );
    broker_task.await.expect("fixture broker task completes");
}

#[tokio::test]
async fn native_handling_removes_pending_approvals_permissions_questions_and_elicitations() {
    let mut file_state = active_state();
    file_state["turns"][0]["items"] = json!([{
        "id": "fixture-file-change-item",
        "type": "fileChange",
        "changes": [{
            "path": "fixture.txt",
            "diff": "--- before\n+++ fixture content\n",
            "kind": "update"
        }]
    }]);
    file_state["requests"] = json!([{
        "id": "fixture-pending-request",
        "method": "item/fileChange/requestApproval",
        "params": {
            "threadId": THREAD_ID,
            "turnId": TURN_ID,
            "itemId": "fixture-file-change-item"
        }
    }]);

    let cases = vec![
        (
            "item/commandExecution/requestApproval",
            "thread-follower-command-approval-decision",
            active_state_with_request(
                "item/commandExecution/requestApproval",
                json!({"threadId":THREAD_ID,"turnId":TURN_ID,"command":"fixture command"}),
            ),
            DesktopReply::Decision { allow: true },
        ),
        (
            "item/fileChange/requestApproval",
            "thread-follower-file-approval-decision",
            file_state,
            DesktopReply::Decision { allow: true },
        ),
        (
            "item/permissions/requestApproval",
            "thread-follower-permissions-request-approval-response",
            active_state_with_request(
                "item/permissions/requestApproval",
                json!({
                    "threadId":THREAD_ID,
                    "turnId":TURN_ID,
                    "permissions":{"network":true}
                }),
            ),
            DesktopReply::Decision { allow: true },
        ),
        (
            "item/tool/requestUserInput",
            "thread-follower-submit-user-input",
            active_state_with_request(
                "item/tool/requestUserInput",
                json!({
                    "threadId":THREAD_ID,
                    "turnId":TURN_ID,
                    "questions":[{"id":"fixture-question","question":"Fixture question"}]
                }),
            ),
            DesktopReply::Answers {
                answers: vec![("fixture-question".into(), vec!["fixture answer".into()])],
            },
        ),
        (
            "mcpServer/elicitation/request",
            "thread-follower-submit-mcp-server-elicitation-response",
            active_state_with_request(
                "mcpServer/elicitation/request",
                json!({
                    "threadId":THREAD_ID,
                    "turnId":TURN_ID,
                    "mode":"form",
                    "message":"Fixture request",
                    "requestedSchema":{"type":"object","properties":{"name":{"type":"string"}}}
                }),
            ),
            DesktopReply::Elicitation {
                accept: true,
                content: Some(json!({"name":"fixture answer"})),
            },
        ),
    ];

    for (request_method, native_method, initial_state, response) in cases {
        let command = reply_command_for_state(&initial_state, request_method, response);
        let (mut session, mut broker) = session_pair_with_state(initial_state);
        let broker_task = tokio::spawn(async move {
            let recheck = read_frame(&mut broker)
                .await
                .expect("owner recheck is sent");
            assert_eq!(recheck["method"], "thread-owner-discovery");
            send_owner_response(&mut broker, &recheck, OWNER_ID).await;

            let mutation = read_frame(&mut broker)
                .await
                .expect("pending request response is sent to the verified owner");
            assert_eq!(mutation["method"], native_method);
            assert_eq!(mutation["params"]["requestId"], "fixture-pending-request");

            let patch = json!({
                "type":"broadcast",
                "sourceClientId":OWNER_ID,
                "method":"thread-stream-state-changed",
                "version":11,
                "params":{
                    "conversationId":THREAD_ID,
                    "change":{
                        "type":"patches",
                        "revision":2,
                        "baseRevision":1,
                        "patches":[{"op":"replace","path":["requests"],"value":[]}]
                    }
                }
            });
            write_frame(&mut broker, &patch)
                .await
                .expect("native handling removes the pending request");
            write_frame(
                &mut broker,
                &json!({
                    "type":"response",
                    "requestId":mutation["requestId"],
                    "resultType":"success",
                    "handledByClientId":OWNER_ID,
                    "result":{"ok":true}
                }),
            )
            .await
            .expect("native handling confirmation is written");
        });

        let result = session
            .execute(&command)
            .await
            .expect("native reply is confirmed");
        assert!(result.turn_id.is_none());
        broker_task.await.expect("fixture broker task completes");
        assert_eq!(session.state().revision(), Some(2));
        assert_eq!(session.state().pending_requests().count(), 0);
        assert!(matches!(
            session
                .state()
                .pending_request(&json!("fixture-pending-request"), request_method),
            Err(DesktopError::RequestExpired)
        ));
    }
}

#[tokio::test]
async fn business_rejections_for_stop_and_all_reply_types_keep_the_session_usable() {
    let mut file_state = active_state();
    file_state["turns"][0]["items"] = json!([{
        "id":"fixture-file-change-item",
        "type":"fileChange",
        "changes":[{"path":"fixture.txt","diff":"--- before\n+++ after\n","kind":"update"}]
    }]);
    file_state["requests"] = json!([{
        "id":"fixture-pending-request",
        "method":"item/fileChange/requestApproval",
        "params":{"threadId":THREAD_ID,"turnId":TURN_ID,"itemId":"fixture-file-change-item"}
    }]);

    let cases = vec![
        (
            DesktopCommand::Stop {
                expected_turn_id: TURN_ID.into(),
            },
            active_state(),
            "thread-follower-interrupt-turn",
        ),
        (
            reply_command_for_state(
                &active_state_with_request(
                    "item/commandExecution/requestApproval",
                    json!({"threadId":THREAD_ID,"turnId":TURN_ID,"command":"fixture command"}),
                ),
                "item/commandExecution/requestApproval",
                DesktopReply::Decision { allow: true },
            ),
            active_state_with_request(
                "item/commandExecution/requestApproval",
                json!({"threadId":THREAD_ID,"turnId":TURN_ID,"command":"fixture command"}),
            ),
            "thread-follower-command-approval-decision",
        ),
        (
            reply_command_for_state(
                &file_state,
                "item/fileChange/requestApproval",
                DesktopReply::Decision { allow: true },
            ),
            file_state,
            "thread-follower-file-approval-decision",
        ),
        (
            reply_command_for_state(
                &active_state_with_request(
                    "item/permissions/requestApproval",
                    json!({"threadId":THREAD_ID,"turnId":TURN_ID,"permissions":{"network":true}}),
                ),
                "item/permissions/requestApproval",
                DesktopReply::Decision { allow: true },
            ),
            active_state_with_request(
                "item/permissions/requestApproval",
                json!({"threadId":THREAD_ID,"turnId":TURN_ID,"permissions":{"network":true}}),
            ),
            "thread-follower-permissions-request-approval-response",
        ),
        (
            reply_command_for_state(
                &active_state_with_request(
                    "item/tool/requestUserInput",
                    json!({"threadId":THREAD_ID,"turnId":TURN_ID,"questions":[{"id":"fixture-question","question":"Fixture question"}]}),
                ),
                "item/tool/requestUserInput",
                DesktopReply::Answers {
                    answers: vec![("fixture-question".into(), vec!["fixture answer".into()])],
                },
            ),
            active_state_with_request(
                "item/tool/requestUserInput",
                json!({"threadId":THREAD_ID,"turnId":TURN_ID,"questions":[{"id":"fixture-question","question":"Fixture question"}]}),
            ),
            "thread-follower-submit-user-input",
        ),
        (
            reply_command_for_state(
                &active_state_with_request(
                    "mcpServer/elicitation/request",
                    json!({
                        "threadId":THREAD_ID,
                        "turnId":TURN_ID,
                        "mode":"form",
                        "message":"Fixture request",
                        "requestedSchema":{"type":"object","properties":{"name":{"type":"string"}}}
                    }),
                ),
                "mcpServer/elicitation/request",
                DesktopReply::Elicitation {
                    accept: true,
                    content: Some(json!({"name":"fixture answer"})),
                },
            ),
            active_state_with_request(
                "mcpServer/elicitation/request",
                json!({
                    "threadId":THREAD_ID,
                    "turnId":TURN_ID,
                    "mode":"form",
                    "message":"Fixture request",
                    "requestedSchema":{"type":"object","properties":{"name":{"type":"string"}}}
                }),
            ),
            "thread-follower-submit-mcp-server-elicitation-response",
        ),
    ];

    for (command, initial_state, first_method) in cases {
        let (mut session, mut broker) = session_pair_with_state(initial_state);
        let broker_task = tokio::spawn(async move {
            let rejected = verified_mutation(&mut broker, first_method).await;
            send_native_error(&mut broker, &rejected, "fixture-handler-rejection").await;

            let next =
                verified_mutation(&mut broker, "thread-follower-update-thread-settings").await;
            send_native_success(&mut broker, &next, json!({"applied":true})).await;
        });

        assert!(matches!(
            session.execute(&command).await,
            Err(DesktopError::Rejected)
        ));
        assert!(
            session.is_connected(),
            "{first_method} rejection keeps IPC open"
        );
        session
            .execute(&settings_command())
            .await
            .expect("next explicit request succeeds on the same session");
        broker_task.await.expect("fixture broker assertions pass");
    }
}

#[tokio::test]
async fn missing_mutation_response_is_reported_as_outcome_unknown() {
    let (mut session, mut broker) = session_pair();
    let broker_task = tokio::spawn(async move {
        let recheck = read_frame(&mut broker)
            .await
            .expect("owner recheck is sent");
        assert_eq!(recheck["method"], "thread-owner-discovery");
        send_owner_response(&mut broker, &recheck, OWNER_ID).await;

        let mutation = read_frame(&mut broker)
            .await
            .expect("mutation request is sent to the verified owner");
        assert_eq!(mutation["method"], "thread-follower-start-turn");
        // Dropping the broker side leaves the mutation outcome unconfirmed.
    });

    let result = session.execute(&start_command()).await;
    assert!(matches!(result, Err(DesktopError::OutcomeUnknown)));
    broker_task.await.expect("fixture broker task completes");
}

#[tokio::test]
async fn request_timeout_ms_matches_start_steer_and_default_waits() {
    let cases = [
        (
            start_command(),
            idle_state(),
            "thread-follower-start-turn",
            90_000,
            json!({"result":{"turn":{"id":TURN_ID}}}),
        ),
        (
            steer_command(),
            active_state(),
            "thread-follower-steer-turn",
            30_000,
            json!({"result":{"turnId":TURN_ID}}),
        ),
        (
            settings_command(),
            idle_state(),
            "thread-follower-update-thread-settings",
            10_000,
            json!({"applied":true}),
        ),
    ];

    for (command, state, method, timeout_ms, native_result) in cases {
        let (mut session, mut broker) = session_pair_with_state(state);
        let broker_task = tokio::spawn(async move {
            let mutation = verified_mutation(&mut broker, method).await;
            assert_eq!(mutation["timeoutMs"], timeout_ms);
            send_native_success(&mut broker, &mutation, native_result).await;
        });

        session
            .execute(&command)
            .await
            .expect("fixture mutation receives its native result");
        broker_task.await.expect("fixture broker assertions pass");
    }
}

#[tokio::test]
async fn request_timeout_is_classified_as_timeout_and_is_not_retried() {
    let (mut session, mut broker) = session_pair_with_state(idle_state());
    let broker_task = tokio::spawn(async move {
        let mutation =
            verified_mutation(&mut broker, "thread-follower-update-thread-settings").await;
        send_native_error(&mut broker, &mutation, "request-timeout").await;
        let closed = timeout(Duration::from_secs(2), read_frame(&mut broker))
            .await
            .expect("request-timeout closes the session without a retry")
            .expect_err("request-timeout does not send a second mutation");
        assert_eq!(closed, DesktopError::Disconnected);
    });

    let result = session.execute(&settings_command()).await;
    assert!(matches!(result, Err(DesktopError::Timeout)));
    assert!(
        !session.is_connected(),
        "request-timeout closes the connection"
    );
    broker_task.await.expect("fixture broker assertions pass");
}

#[tokio::test(start_paused = true)]
async fn start_succeeds_after_fifteen_virtual_seconds_within_owner_deadline() {
    let (mut session, mut broker) = session_pair();
    let (clock_stop_tx, clock_stop_rx) = tokio::sync::oneshot::channel();
    let clock_driver = tokio::spawn(async move {
        let mut stop = clock_stop_rx;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                _ = &mut stop => break,
            }
        }
    });
    let broker_task = tokio::spawn(async move {
        let mutation = verified_mutation(&mut broker, "thread-follower-start-turn").await;
        assert_eq!(mutation["timeoutMs"], 90_000);
        tokio::time::sleep(Duration::from_secs(15)).await;
        send_native_success(
            &mut broker,
            &mutation,
            json!({"result":{"turn":{"id":TURN_ID}}}),
        )
        .await;
    });
    let result = session.execute(&start_command()).await;
    clock_stop_tx
        .send(())
        .expect("virtual clock driver stops after the request completes");
    clock_driver.await.expect("virtual clock driver exits");
    let result = result.expect("Start remains valid after a fifteen-second owner response");
    assert_eq!(result.turn_id.as_deref(), Some(TURN_ID));
    broker_task.await.expect("fixture broker task completes");
}

#[tokio::test]
async fn protocol_error_aliases_are_unsupported_and_leave_session_open() {
    let (mut session, mut broker) = session_pair();
    let broker_task = tokio::spawn(async move {
        for error in ["request-version-mismatch", "no-handler-for-request"] {
            let mutation =
                verified_mutation(&mut broker, "thread-follower-update-thread-settings").await;
            send_native_error(&mut broker, &mutation, error).await;
        }

        let mutation =
            verified_mutation(&mut broker, "thread-follower-update-thread-settings").await;
        send_native_success(&mut broker, &mutation, json!({"applied":true})).await;
    });

    for _ in 0..2 {
        assert!(matches!(
            session.execute(&settings_command()).await,
            Err(DesktopError::UnsupportedProtocol)
        ));
        assert!(session.is_connected());
    }
    session
        .execute(&settings_command())
        .await
        .expect("a later native request succeeds on the same session");
    assert!(session.is_connected());
    broker_task.await.expect("fixture broker assertions pass");
}

#[tokio::test]
async fn handler_errors_are_unknown_for_messages_rejected_for_settings_and_do_not_retry() {
    let cases = [
        (
            start_command(),
            idle_state(),
            "thread-follower-start-turn",
            true,
        ),
        (
            steer_command(),
            active_state(),
            "thread-follower-steer-turn",
            true,
        ),
        (
            settings_command(),
            idle_state(),
            "thread-follower-update-thread-settings",
            false,
        ),
    ];

    for (command, state, method, outcome_unknown) in cases {
        let (mut session, mut broker) = session_pair_with_state(state);
        let broker_task = tokio::spawn(async move {
            let mutation = verified_mutation(&mut broker, method).await;
            send_native_error(&mut broker, &mutation, "fixture-handler-exception").await;

            let later =
                verified_mutation(&mut broker, "thread-follower-update-thread-settings").await;
            send_native_success(&mut broker, &later, json!({"applied":true})).await;
        });

        let result = session.execute(&command).await;
        if outcome_unknown {
            assert!(matches!(result, Err(DesktopError::OutcomeUnknown)));
        } else {
            assert!(matches!(result, Err(DesktopError::Rejected)));
        }
        assert!(session.is_connected());
        session
            .execute(&settings_command())
            .await
            .expect("a subsequent explicit command succeeds without a session restart");
        broker_task.await.expect("fixture broker assertions pass");
    }
}

#[tokio::test]
async fn transport_and_owner_identity_failures_are_not_rejected_responses() {
    for (native_error, handled_by, expected_identity_error) in [
        (Some("client-disconnected"), OWNER_ID, false),
        (None, OTHER_OWNER_ID, true),
    ] {
        let (mut session, mut broker) = session_pair();
        let broker_task = tokio::spawn(async move {
            let mutation =
                verified_mutation(&mut broker, "thread-follower-update-thread-settings").await;
            let mut response = json!({
                "type":"response",
                "requestId":mutation["requestId"],
                "resultType":if native_error.is_some() { "error" } else { "success" },
                "handledByClientId":handled_by
            });
            if let Some(error) = native_error {
                response["error"] = json!(error);
            }
            write_frame(&mut broker, &response)
                .await
                .expect("transport or identity fixture response is written");
            let closed = timeout(Duration::from_secs(2), read_frame(&mut broker))
                .await
                .expect("failed session closes after transport or identity failure")
                .expect_err("session does not send another request after a failed exchange");
            assert_eq!(closed, DesktopError::Disconnected);
        });

        let result = session.execute(&settings_command()).await;
        if expected_identity_error {
            assert!(matches!(result, Err(DesktopError::InvalidIdentity)));
        } else {
            assert!(matches!(result, Err(DesktopError::Disconnected)));
        }
        assert!(!session.is_connected());
        broker_task.await.expect("fixture broker assertions pass");
    }
}
