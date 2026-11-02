use super::super::framing::{write_frame, FrameReader};
use super::super::{DesktopError, DesktopSession};
use super::DesktopWorker;
use crate::{
    crypto::SecretBox,
    db::{desktop_submissions::DesktopLedger, PanelDb},
    services::codex_desktop::{DesktopAction, DesktopAnswer, DesktopReferenceSpan, DesktopView},
};
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{mpsc, oneshot},
    time::timeout,
};

const THREAD_ID: &str = "11111111-1111-4111-8111-111111111111";
const CLIENT_ID: &str = "55555555-5555-4555-8555-555555555555";
const OWNER_ID: &str = "66666666-6666-4666-8666-666666666666";
const OTHER_OWNER_ID: &str = "77777777-7777-4777-8777-777777777777";
const TURN_ID: &str = "33333333-3333-4333-8333-333333333333";
const OTHER_TURN_ID: &str = "44444444-4444-4444-8444-444444444444";
const NEXT_TURN_ID: &str = "77777777-7777-4777-8777-777777777777";
const FIRST_MESSAGE_ID: &str = "88888888-8888-4888-8888-888888888888";
const SECOND_MESSAGE_ID: &str = "99999999-9999-4999-8999-999999999999";

struct Fixture {
    root: PathBuf,
    codex_home: PathBuf,
    socket: PathBuf,
    database: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from("/tmp").join(format!("nh-desk-worker-{}", uuid::Uuid::new_v4()));
        let codex_home = root.join("codex-home");
        let ipc = codex_home.join("ipc");
        fs::create_dir_all(&ipc).expect("temporary IPC directories are created");
        set_private_directory(&root);
        set_private_directory(&codex_home);
        set_private_directory(&ipc);
        Self {
            socket: ipc.join("ipc.sock"),
            database: root.join("panel.sqlite"),
            root,
            codex_home,
        }
    }

    fn bind(&self) -> UnixListener {
        let listener = UnixListener::bind(&self.socket).expect("fake IPC socket is bound");
        fs::set_permissions(&self.socket, fs::Permissions::from_mode(0o600))
            .expect("fake IPC socket permissions are restricted");
        listener
    }

    fn ledger(&self) -> DesktopLedger {
        let crypto = SecretBox::deterministic_dev();
        let db = PanelDb::open_with_secret_box(&self.database, crypto.clone())
            .expect("temporary PanelDb is initialized");
        DesktopLedger::new(db, crypto)
    }

    fn enqueue_pair(&self, ledger: &DesktopLedger, source: &str) {
        ledger
            .enqueue(
                source,
                THREAD_ID,
                FIRST_MESSAGE_ID,
                &json!({"kind":"queue","text":"fixture first queued message"}),
            )
            .expect("first fixture message is queued");
        ledger
            .enqueue(
                source,
                THREAD_ID,
                SECOND_MESSAGE_ID,
                &json!({"kind":"queue","text":"fixture second queued message"}),
            )
            .expect("second fixture message is queued");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn set_private_directory(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("temporary fixture directory permissions are restricted");
}

fn source_id() -> String {
    "ab".repeat(32)
}

#[test]
fn user_messages_are_fixed_safe_and_keep_desktop_failures_distinct() {
    let messages = [
        (
            DesktopError::OwnerUnavailable,
            "请先在 Codex 桌面打开同一线程，再重试连接",
        ),
        (DesktopError::RequestExpired, "该请求已失效，请查看最新状态"),
        (
            DesktopError::OutcomeUnknown,
            "发送结果未确认，请先在 Codex 中核对，勿重复发送",
        ),
        (
            DesktopError::UnsupportedProtocol,
            "此操作与当前 Codex 桌面协议不兼容，请在 Codex 中继续处理",
        ),
        (
            DesktopError::OutOfSync,
            "Codex 桌面状态已失步，请重新连接；仍可继续阅读",
        ),
        (DesktopError::UnsafeSocket, "无法确认 Codex 桌面或线程身份"),
    ];

    for (error, expected) in messages {
        let message = error.user_message();
        assert_eq!(message, expected);
        assert!(!message.contains("fixture"));
        assert!(!message.contains("/tmp/"));
    }
    for left in 0..messages.len() {
        for right in left + 1..messages.len() {
            assert_ne!(messages[left].1, messages[right].1);
        }
    }
}

#[test]
fn empty_spans_preserve_legacy_ledger_bytes_and_nonempty_spans_bind_identity() {
    use super::super::references::{NativeReference, ReferenceSpan};
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let references = vec![NativeReference::app("Fixture".into(), "app://fixture".into()).unwrap()];
    let legacy = json!({"kind":"queue","text":"@Fixture","references":references});
    let body = super::submission_body("queue", "@Fixture", &references, &[]);
    assert!(body.get("nativeMentions").is_none());
    assert_eq!(
        serde_json::to_vec(&legacy).unwrap(),
        serde_json::to_vec(&body).unwrap()
    );
    ledger
        .enqueue(&source, THREAD_ID, FIRST_MESSAGE_ID, &legacy)
        .unwrap();
    ledger
        .enqueue(&source, THREAD_ID, FIRST_MESSAGE_ID, &body)
        .unwrap();
    let body = super::submission_body(
        "queue",
        "@Fixture",
        &references,
        &[ReferenceSpan {
            reference: 0,
            start: 0,
            end: 8,
        }],
    );
    assert_eq!(body["nativeMentions"], true);
    assert!(ledger
        .enqueue(&source, THREAD_ID, FIRST_MESSAGE_ID, &body)
        .is_err());
    ledger
        .enqueue(&source, THREAD_ID, SECOND_MESSAGE_ID, &body)
        .unwrap();
    let reopened = fixture.ledger();
    let items = reopened.list(&source, THREAD_ID).unwrap();
    assert!(items[0]
        .body
        .as_ref()
        .and_then(|body| body.get("nativeMentions"))
        .is_none());
    assert_eq!(
        serde_json::to_vec(items[0].body.as_ref().unwrap()).unwrap(),
        serde_json::to_vec(&legacy).unwrap()
    );
    assert_eq!(items[1].body.as_ref(), Some(&body));
    assert_eq!(items[1].body.as_ref().unwrap()["nativeMentions"], true);
}

#[tokio::test]
async fn native_mentions_reach_owner_for_start_and_steer_and_persist_the_flag() {
    use super::super::references::NativeReference;

    let fixture = Fixture::new();
    let source = source_id();
    let ledger = fixture.ledger();
    let app = NativeReference::app("search".into(), "app://fixture-search".into())
        .expect("synthetic app reference is accepted");
    let listener = fixture.bind();
    let (start_seen_tx, start_seen_rx) = oneshot::channel();
    let (release_start_tx, release_start_rx) = oneshot::channel();
    let (steer_seen_tx, steer_seen_rx) = oneshot::channel();
    let (release_steer_tx, release_steer_rx) = oneshot::channel();

    let broker = tokio::spawn(async move {
        let mut stream = accept_session(&listener, thread_state("idle", Vec::new())).await;
        let owner_check = expect_request(&mut stream, "thread-owner-discovery").await;
        send_success(&mut stream, &owner_check, json!({}), Some(OWNER_ID)).await;

        let start = expect_request(&mut stream, "thread-follower-start-turn").await;
        assert_eq!(
            start["params"]["turnStart"]["request"]["input"],
            json!([
                {
                    "type":"text",
                    "text":"[$search](app://fixture-search)😀",
                    "text_elements":[]
                },
                {"type":"mention","name":"search","path":"app://fixture-search"}
            ])
        );
        assert_eq!(
            start["params"]["turnStart"]["context"]["attachments"],
            json!([])
        );
        assert!(start["params"]["turnStart"]["request"]
            .get("attachments")
            .is_none());
        start_seen_tx
            .send(())
            .expect("test inspects queued native-mention bytes before owner acceptance");
        release_start_rx
            .await
            .expect("test releases the native start response");
        send_success(
            &mut stream,
            &start,
            json!({"result":{"turn":{"id":TURN_ID}}}),
            Some(OWNER_ID),
        )
        .await;

        let mut active = thread_state("active", vec![turn(TURN_ID, "inProgress")]);
        active["cwd"] = json!("/fixture/workspace");
        send_snapshot(&mut stream, 2, active).await;
        let owner_check = expect_request(&mut stream, "thread-owner-discovery").await;
        send_success(&mut stream, &owner_check, json!({}), Some(OWNER_ID)).await;

        let steer = expect_request(&mut stream, "thread-follower-steer-turn").await;
        assert_eq!(
            steer["params"]["input"],
            json!([
                {
                    "type":"text",
                    "text":"[$search](app://fixture-search)继续",
                    "text_elements":[]
                },
                {"type":"mention","name":"search","path":"app://fixture-search"}
            ])
        );
        assert_eq!(steer["params"]["attachments"], json!([]));
        assert_eq!(
            steer["params"]["restoreMessage"]["context"]["fileAttachments"],
            json!([])
        );
        steer_seen_tx
            .send(())
            .expect("test inspects immediate steer bytes before owner acceptance");
        release_steer_rx
            .await
            .expect("test releases the native steer response");
        send_success(
            &mut stream,
            &steer,
            json!({"result":{"turnId":TURN_ID}}),
            Some(OWNER_ID),
        )
        .await;
        expect_unfollow_then_close(&mut stream).await;
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to fake desktop socket");
    let worker = DesktopWorker::spawn(session, ledger.clone(), source.clone())
        .await
        .expect("worker starts with the temporary encrypted queue");
    worker
        .act_with_references(
            DesktopAction::Send {
                message_id: FIRST_MESSAGE_ID.into(),
                text: "@search😀".into(),
                references: vec!["fixture-search-handle".into()],
                reference_spans: vec![DesktopReferenceSpan {
                    handle: "fixture-search-handle".into(),
                    start: 0,
                    end: 7,
                }],
            },
            vec![app.clone()],
        )
        .await
        .expect("new native mention message enters the encrypted queue");
    timeout(Duration::from_secs(5), start_seen_rx)
        .await
        .expect("owner receives native start within the test bound")
        .expect("fake owner signals receipt of native start");
    let stored_start = ledger.list(&source, THREAD_ID).unwrap();
    assert_eq!(
        stored_start[0].body.as_ref().unwrap()["nativeMentions"],
        true
    );
    assert_eq!(
        stored_start[0].body.as_ref().unwrap()["referenceSpans"],
        json!([{"reference":0,"start":0,"end":7}])
    );
    release_start_tx
        .send(())
        .expect("native start response is released after persistence is checked");
    wait_for_view(&worker, |view| {
        view.active_turn_id.as_deref() == Some(TURN_ID)
    })
    .await;

    let steer_worker = worker.clone();
    let steer_task = tokio::spawn(async move {
        steer_worker
            .act_with_references(
                DesktopAction::Steer {
                    message_id: SECOND_MESSAGE_ID.into(),
                    text: "@search继续".into(),
                    references: vec!["fixture-search-handle".into()],
                    reference_spans: vec![DesktopReferenceSpan {
                        handle: "fixture-search-handle".into(),
                        start: 0,
                        end: 7,
                    }],
                    turn_id: TURN_ID.into(),
                },
                vec![app],
            )
            .await
    });
    timeout(Duration::from_secs(5), steer_seen_rx)
        .await
        .expect("owner receives native steer within the test bound")
        .expect("fake owner signals receipt of native steer");
    let stored_steer = ledger.list(&source, THREAD_ID).unwrap();
    let steer_body = stored_steer
        .iter()
        .find(|item| item.id == SECOND_MESSAGE_ID)
        .and_then(|item| item.body.as_ref())
        .expect("steer remains encrypted until the owner accepts it");
    assert_eq!(steer_body["nativeMentions"], true);
    assert_eq!(
        steer_body["referenceSpans"],
        json!([{"reference":0,"start":0,"end":7}])
    );
    release_steer_tx
        .send(())
        .expect("native steer response is released after persistence is checked");
    steer_task
        .await
        .expect("steer action task completes")
        .expect("owner accepts native steer");

    drop(worker);
    broker.await.expect("fake owner assertions pass");
}

#[tokio::test]
async fn legacy_spans_without_native_mentions_flag_keep_text_elements_on_dispatch() {
    use super::super::references::NativeReference;

    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let reference = NativeReference::app("search".into(), "app://fixture-search".into())
        .expect("synthetic app reference is accepted");
    let legacy = json!({
        "kind":"queue",
        "text":"@search",
        "references":[reference],
        "referenceSpans":[{"reference":0,"start":0,"end":7}]
    });
    assert!(legacy.get("nativeMentions").is_none());
    ledger
        .enqueue(&source, THREAD_ID, FIRST_MESSAGE_ID, &legacy)
        .expect("legacy encrypted queue item is inserted");
    let listener = fixture.bind();
    let broker = tokio::spawn(async move {
        let mut stream = accept_session(&listener, thread_state("idle", Vec::new())).await;
        let owner_check = expect_request(&mut stream, "thread-owner-discovery").await;
        send_success(&mut stream, &owner_check, json!({}), Some(OWNER_ID)).await;
        let start = expect_request(&mut stream, "thread-follower-start-turn").await;
        assert_eq!(
            start["params"]["turnStart"]["request"]["input"],
            json!([
                {
                    "type":"text",
                    "text":"@search",
                    "text_elements":[{
                        "byteRange":{"start":0,"end":7},
                        "placeholder":"@search"
                    }]
                },
                {"type":"mention","name":"search","path":"app://fixture-search"}
            ])
        );
        send_success(
            &mut stream,
            &start,
            json!({"result":{"turn":{"id":TURN_ID}}}),
            Some(OWNER_ID),
        )
        .await;
        expect_unfollow_then_close(&mut stream).await;
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to fake desktop socket");
    let worker = DesktopWorker::spawn(session, ledger, source)
        .await
        .expect("worker starts with the legacy encrypted queue");
    worker
        .act(DesktopAction::ResumeQueue)
        .await
        .expect("legacy queue resume action is accepted");
    wait_for_view(&worker, |view| view.queue.is_empty()).await;
    drop(worker);
    broker.await.expect("fake owner assertions pass");
}

fn turn(id: &str, status: &str) -> Value {
    json!({"id":id,"status":status,"items":[]})
}

fn thread_state(runtime: &str, turns: Vec<Value>) -> Value {
    json!({
        "id": THREAD_ID,
        "hostId": "local",
        "parentThreadId": null,
        "archived": false,
        "source": {},
        "threadSource": {},
        "threadRuntimeStatus": {"type": runtime},
        "turns": turns,
        "requests": []
    })
}

fn thread_state_with_submission(client_id: &str) -> Value {
    let mut state = thread_state("active", vec![turn(TURN_ID, "inProgress")]);
    state["turns"][0]["items"] = json!([{
        "id":"fixture-user-message",
        "type":"userMessage",
        "clientId":client_id,
        "content":[]
    }]);
    state
}

async fn read_frame(stream: &mut UnixStream) -> std::result::Result<Value, DesktopError> {
    FrameReader::default().read(stream).await
}

async fn send_success(
    stream: &mut UnixStream,
    request: &Value,
    result: Value,
    handled_by: Option<&str>,
) {
    let mut response = json!({
        "type":"response",
        "requestId":request["requestId"],
        "resultType":"success",
        "result":result
    });
    if let Some(owner) = handled_by {
        response["handledByClientId"] = json!(owner);
    }
    write_frame(stream, &response)
        .await
        .expect("fixture IPC response is written");
}

async fn send_snapshot(stream: &mut UnixStream, revision: u64, state: Value) {
    send_snapshot_as(stream, revision, state, OWNER_ID).await;
}

async fn send_snapshot_as(stream: &mut UnixStream, revision: u64, state: Value, owner_id: &str) {
    write_frame(
        stream,
        &json!({
            "type":"broadcast",
            "sourceClientId":owner_id,
            "method":"thread-stream-state-changed",
            "version":11,
            "params":{
                "conversationId":THREAD_ID,
                "change":{"type":"snapshot","revision":revision,"conversationState":state}
            }
        }),
    )
    .await
    .expect("fixture state snapshot is written");
}

async fn accept_session(listener: &UnixListener, state: Value) -> UnixStream {
    accept_session_as(listener, state, OWNER_ID).await
}

async fn accept_session_as(listener: &UnixListener, state: Value, owner_id: &str) -> UnixStream {
    let (mut stream, _) = listener
        .accept()
        .await
        .expect("fake IPC accepts the local test session");

    let initialize = read_frame(&mut stream)
        .await
        .expect("session initialization is received");
    assert_eq!(initialize["method"], "initialize");
    send_success(
        &mut stream,
        &initialize,
        json!({"clientId":CLIENT_ID}),
        None,
    )
    .await;

    let discovery = read_frame(&mut stream)
        .await
        .expect("initial owner discovery is received");
    assert_eq!(discovery["method"], "thread-owner-discovery");
    send_success(&mut stream, &discovery, json!({}), Some(owner_id)).await;

    let following = read_frame(&mut stream)
        .await
        .expect("session follow broadcast is received");
    assert_eq!(following["method"], "thread-stream-following-changed");
    assert_eq!(following["params"]["following"], true);
    send_snapshot_as(&mut stream, 1, state, owner_id).await;
    stream
}

async fn send_out_of_sync_patch(stream: &mut UnixStream) {
    write_frame(
        stream,
        &json!({
            "type":"broadcast",
            "sourceClientId":OWNER_ID,
            "method":"thread-stream-state-changed",
            "version":11,
            "params":{
                "conversationId":THREAD_ID,
                "change":{"type":"patches","revision":2,"baseRevision":0,"patches":[]}
            }
        }),
    )
    .await
    .expect("fixture sends a revision-gap patch");
}

async fn send_connection_reset(stream: &mut UnixStream) {
    write_frame(
        stream,
        &json!({"type":"broadcast","method":"ipc-connection-reset"}),
    )
    .await
    .expect("fixture sends a connection-reset broadcast");
}

async fn expect_request(stream: &mut UnixStream, method: &str) -> Value {
    let request = read_frame(stream)
        .await
        .unwrap_or_else(|error| panic!("expected fixture request {method}, got {error}"));
    assert_eq!(request["type"], "request");
    assert_eq!(request["method"], method);
    request
}

async fn expect_no_frame(stream: &mut UnixStream) {
    match timeout(Duration::from_millis(250), read_frame(stream)).await {
        Err(_) => {}
        Ok(Ok(frame)) => {
            panic!("unexpected native IPC frame before the allowed transition: {frame}")
        }
        Ok(Err(error)) => panic!("fixture IPC closed before the expected transition: {error}"),
    }
}

async fn expect_unfollow_then_close(stream: &mut UnixStream) {
    let following = timeout(Duration::from_secs(3), read_frame(stream))
        .await
        .expect("session close broadcasts that following stopped")
        .expect("session close broadcast is framed");
    assert_eq!(following["method"], "thread-stream-following-changed");
    assert_eq!(following["params"]["following"], false);
    assert_eq!(
        read_frame(stream).await,
        Err(DesktopError::Disconnected),
        "session closes its transport after unfollowing"
    );
}

async fn wait_for_view(
    worker: &DesktopWorker,
    matches: impl Fn(&DesktopView) -> bool,
) -> DesktopView {
    let mut view = worker.subscribe();
    timeout(Duration::from_secs(5), async {
        loop {
            let current = view.borrow().clone();
            if matches(&current) {
                return current;
            }
            view.changed()
                .await
                .expect("worker view remains available while actor is running");
        }
    })
    .await
    .unwrap_or_else(|_| {
        let current = view.borrow().clone();
        let queue: Vec<_> = current
            .queue
            .iter()
            .map(|item| (item.id.as_str(), item.status.as_str()))
            .collect();
        panic!(
            "expected desktop worker view before timeout; session={} phase={:?} runtime={} connected={} writable={} error={:?} queue={queue:?}",
            current.session_id,
            current.connection_phase,
            current.runtime,
            current.connected,
            current.writable,
            current.error
        )
    })
}

async fn wait_for_view_by_yield(
    worker: &DesktopWorker,
    matches: impl Fn(&DesktopView) -> bool,
) -> DesktopView {
    for _ in 0..10_000 {
        let current = worker.current();
        if matches(&current) {
            return current;
        }
        tokio::task::yield_now().await;
    }
    panic!("expected desktop worker view did not arrive");
}

async fn wait_for_retry_attempt(attempts: &mut mpsc::UnboundedReceiver<usize>, expected: usize) {
    for _ in 0..10_000 {
        if let Ok(actual) = attempts.try_recv() {
            assert_eq!(actual, expected);
            for _ in 0..100 {
                tokio::task::yield_now().await;
            }
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("expected retry attempt {expected} did not arrive");
}

enum BrokerCommand {
    Snapshot { revision: u64, state: Value },
    CheckNoFrame(oneshot::Sender<()>),
}

#[tokio::test]
async fn queued_dispatch_waits_for_the_exact_turn_to_finish() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    fixture.enqueue_pair(&ledger, &source);
    let listener = fixture.bind();
    let (broker_commands, mut commands) = mpsc::unbounded_channel();
    let (first_started_tx, first_started_rx) = oneshot::channel();
    let (second_started_tx, second_started_rx) = oneshot::channel();

    let broker = tokio::spawn(async move {
        let mut stream = accept_session(&listener, thread_state("idle", Vec::new())).await;

        let owner_check = expect_request(&mut stream, "thread-owner-discovery").await;
        assert!(owner_check.get("targetClientId").is_none());
        send_success(&mut stream, &owner_check, json!({}), Some(OWNER_ID)).await;

        let first = expect_request(&mut stream, "thread-follower-start-turn").await;
        assert_eq!(
            first["params"]["turnStart"]["request"]["clientUserMessageId"],
            FIRST_MESSAGE_ID
        );
        send_success(
            &mut stream,
            &first,
            json!({"result":{"turn":{"id":TURN_ID}}}),
            Some(OWNER_ID),
        )
        .await;
        first_started_tx
            .send(())
            .expect("test receives the first start acknowledgement");

        let BrokerCommand::Snapshot { revision, state } = commands
            .recv()
            .await
            .expect("test supplies a nonmatching completion snapshot")
        else {
            panic!("expected a nonmatching completion snapshot");
        };
        send_snapshot(&mut stream, revision, state).await;

        let BrokerCommand::CheckNoFrame(done) = commands
            .recv()
            .await
            .expect("test asks for the no-resend observation")
        else {
            panic!("expected a no-resend observation request");
        };
        expect_no_frame(&mut stream).await;
        done.send(()).expect("no-resend observation is reported");

        let BrokerCommand::Snapshot { revision, state } = commands
            .recv()
            .await
            .expect("test supplies the exact completed turn")
        else {
            panic!("expected the exact completed-turn snapshot");
        };
        send_snapshot(&mut stream, revision, state).await;

        let owner_check = expect_request(&mut stream, "thread-owner-discovery").await;
        send_success(&mut stream, &owner_check, json!({}), Some(OWNER_ID)).await;
        let second = expect_request(&mut stream, "thread-follower-start-turn").await;
        assert_eq!(
            second["params"]["turnStart"]["request"]["clientUserMessageId"],
            SECOND_MESSAGE_ID
        );
        send_success(
            &mut stream,
            &second,
            json!({"result":{"turn":{"id":NEXT_TURN_ID}}}),
            Some(OWNER_ID),
        )
        .await;
        second_started_tx
            .send(())
            .expect("test receives the second start acknowledgement");

        let following = timeout(Duration::from_secs(3), read_frame(&mut stream))
            .await
            .expect("worker closes its session after the test finishes")
            .expect("session close broadcasts are framed");
        assert_eq!(following["method"], "thread-stream-following-changed");
        assert_eq!(following["params"]["following"], false);
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to the fake desktop socket");
    let worker = DesktopWorker::spawn(session, ledger, source)
        .await
        .expect("worker starts with the temporary encrypted queue");
    worker
        .act(DesktopAction::ResumeQueue)
        .await
        .expect("fixture queue resume action is accepted");
    first_started_rx
        .await
        .expect("first message reaches the fake owner");

    wait_for_view(&worker, |view| {
        view.queue.len() == 1
            && view.queue[0].id == SECOND_MESSAGE_ID
            && view.queue[0].status == "queued"
    })
    .await;

    broker_commands
        .send(BrokerCommand::Snapshot {
            revision: 2,
            state: thread_state("idle", vec![turn(OTHER_TURN_ID, "completed")]),
        })
        .expect("nonmatching completion snapshot reaches fake owner");
    wait_for_view(&worker, |view| view.revision == 2).await;
    let (no_frame_tx, no_frame_rx) = oneshot::channel();
    broker_commands
        .send(BrokerCommand::CheckNoFrame(no_frame_tx))
        .expect("worker dispatch is checked before the exact completion");
    no_frame_rx
        .await
        .expect("fake owner completes the no-resend observation");

    broker_commands
        .send(BrokerCommand::Snapshot {
            revision: 3,
            state: thread_state("idle", vec![turn(TURN_ID, "completed")]),
        })
        .expect("exact completed turn reaches fake owner");
    second_started_rx
        .await
        .expect("second message starts only after its predecessor finishes");
    wait_for_view(&worker, |view| view.queue.is_empty()).await;

    drop(worker);
    broker.await.expect("fake broker assertions pass");
}

#[tokio::test]
async fn outcome_unknown_is_not_replayed_after_worker_restart_and_resume() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    fixture.enqueue_pair(&ledger, &source);
    let listener = fixture.bind();
    let (check_no_replay_tx, mut check_no_replay_rx) =
        mpsc::unbounded_channel::<oneshot::Sender<()>>();
    let (exact_snapshot_tx, mut exact_snapshot_rx) =
        mpsc::unbounded_channel::<oneshot::Sender<()>>();
    let (second_session_tx, second_session_rx) = oneshot::channel();
    let (first_closed_tx, first_closed_rx) = oneshot::channel();
    let (close_first_tx, close_first_rx) = oneshot::channel();

    let broker = tokio::spawn(async move {
        let mut first_stream = accept_session(&listener, thread_state("idle", Vec::new())).await;
        let owner_check = expect_request(&mut first_stream, "thread-owner-discovery").await;
        send_success(&mut first_stream, &owner_check, json!({}), Some(OWNER_ID)).await;
        let first = expect_request(&mut first_stream, "thread-follower-start-turn").await;
        assert_eq!(
            first["params"]["turnStart"]["request"]["clientUserMessageId"],
            FIRST_MESSAGE_ID
        );
        write_frame(
            &mut first_stream,
            &json!({
                "type":"response",
                "requestId":first["requestId"],
                "resultType":"error",
                "error":"fixture-native-error"
            }),
        )
        .await
        .expect("ambiguous native error is returned");
        close_first_rx
            .await
            .expect("test has observed the unknown item before closing its worker");
        let following = timeout(Duration::from_secs(3), read_frame(&mut first_stream))
            .await
            .expect("first worker closes after the test drops it")
            .expect("first worker unsubscribes from its session");
        assert_eq!(following["method"], "thread-stream-following-changed");
        assert_eq!(following["params"]["following"], false);
        first_closed_tx
            .send(())
            .expect("test observes the first worker's completed shutdown");

        let mut second_stream =
            accept_session(&listener, thread_state_with_submission(SECOND_MESSAGE_ID)).await;
        second_session_tx
            .send(())
            .expect("test observes the restarted worker's handshake");
        let done = check_no_replay_rx
            .recv()
            .await
            .expect("test checks that unknown work is not replayed after resume");
        expect_no_frame(&mut second_stream).await;
        done.send(())
            .expect("a different native message ID does not replay unknown work");

        let exact = exact_snapshot_rx
            .recv()
            .await
            .expect("test requests a native snapshot with the exact uncertain ID");
        send_snapshot(
            &mut second_stream,
            2,
            thread_state_with_submission(FIRST_MESSAGE_ID),
        )
        .await;
        exact
            .send(())
            .expect("exact matching snapshot is delivered");
        let done = check_no_replay_rx
            .recv()
            .await
            .expect("test checks that confirming the uncertain item sends nothing");
        expect_no_frame(&mut second_stream).await;
        done.send(())
            .expect("snapshot confirmation does not resubmit the item");

        let following = timeout(Duration::from_secs(3), read_frame(&mut second_stream))
            .await
            .expect("restarted worker closes its test session")
            .expect("restarted session unsubscribes");
        assert_eq!(following["method"], "thread-stream-following-changed");
        assert_eq!(following["params"]["following"], false);
    });

    let first_session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("first worker attaches to fake desktop socket");
    let first_worker = DesktopWorker::spawn(first_session, ledger.clone(), source.clone())
        .await
        .expect("first worker starts");
    first_worker
        .act(DesktopAction::ResumeQueue)
        .await
        .expect("fixture queue is eligible for its first dispatch");

    wait_for_view(&first_worker, |view| {
        view.queue
            .iter()
            .any(|item| item.id == FIRST_MESSAGE_ID && item.status == "outcome_unknown")
            && view
                .queue
                .iter()
                .any(|item| item.id == SECOND_MESSAGE_ID && item.status == "queued")
    })
    .await;

    let unknown = wait_for_view(&first_worker, |view| {
        view.queue
            .iter()
            .any(|item| item.id == FIRST_MESSAGE_ID && item.status == "outcome_unknown")
            && view
                .queue
                .iter()
                .any(|item| item.id == SECOND_MESSAGE_ID && item.status == "queued")
    })
    .await;
    assert!(
        unknown.writable,
        "connected={} phase={:?} runtime={} error={:?}",
        unknown.connected, unknown.connection_phase, unknown.runtime, unknown.error
    );
    close_first_tx
        .send(())
        .expect("fake broker is ready to observe explicit worker shutdown");
    drop(first_worker);
    first_closed_rx
        .await
        .expect("first worker has fully unsubscribed before the restart");

    ledger
        .recover(&source)
        .expect("restart recovery preserves the ambiguous status");
    let second_session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("replacement worker attaches to the fake desktop socket");
    second_session_rx
        .await
        .expect("fake broker observes the restarted worker session");
    let second_worker = DesktopWorker::spawn(second_session, ledger.clone(), source.clone())
        .await
        .expect("replacement worker loads the same encrypted ledger");
    second_worker
        .act(DesktopAction::ResumeQueue)
        .await
        .expect("user may resume definite queued work");
    wait_for_view(&second_worker, |view| {
        view.queue
            .iter()
            .any(|item| item.id == FIRST_MESSAGE_ID && item.status == "outcome_unknown")
            && view
                .queue
                .iter()
                .any(|item| item.id == SECOND_MESSAGE_ID && item.status == "queued")
    })
    .await;
    let (done_tx, done_rx) = oneshot::channel();
    check_no_replay_tx
        .send(done_tx)
        .expect("restarted session is checked for replay");
    done_rx
        .await
        .expect("restarted session confirms no replay request");

    let before_exact = ledger
        .list(&source, THREAD_ID)
        .expect("unknown record is available before exact ID confirmation");
    assert_eq!(before_exact[0].id, FIRST_MESSAGE_ID);
    assert_eq!(before_exact[0].status, "outcome_unknown");

    let (exact_done_tx, exact_done_rx) = oneshot::channel();
    exact_snapshot_tx
        .send(exact_done_tx)
        .expect("fake owner is asked to publish the exact native message ID");
    exact_done_rx
        .await
        .expect("fake owner sent the exact-ID snapshot");
    let reconciled = wait_for_view(&second_worker, |view| {
        !view.queue.iter().any(|item| item.id == FIRST_MESSAGE_ID)
            && view
                .queue
                .iter()
                .any(|item| item.id == SECOND_MESSAGE_ID && item.status == "queued")
    })
    .await;
    assert!(reconciled
        .queue
        .iter()
        .all(|item| item.id != FIRST_MESSAGE_ID));
    let (done_tx, done_rx) = oneshot::channel();
    check_no_replay_tx
        .send(done_tx)
        .expect("confirmed uncertain item is checked for accidental resend");
    done_rx
        .await
        .expect("exact-ID confirmation does not resend the item");
    let records = ledger
        .list(&source, THREAD_ID)
        .expect("reconciled and queued records remain readable");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, SECOND_MESSAGE_ID);
    assert_eq!(records[0].status, "queued");

    drop(second_worker);
    broker.await.expect("fake broker assertions pass");
}

#[tokio::test]
async fn stop_holds_and_preserves_encrypted_queue_items() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    fixture.enqueue_pair(&ledger, &source);
    let listener = fixture.bind();
    let (no_start_tx, no_start_rx) = oneshot::channel();

    let broker = tokio::spawn(async move {
        let mut stream = accept_session(
            &listener,
            thread_state("active", vec![turn(TURN_ID, "inProgress")]),
        )
        .await;
        let owner_check = expect_request(&mut stream, "thread-owner-discovery").await;
        send_success(&mut stream, &owner_check, json!({}), Some(OWNER_ID)).await;
        let stop = expect_request(&mut stream, "thread-follower-interrupt-turn").await;
        assert_eq!(stop["params"]["expectedTurnId"], TURN_ID);
        send_snapshot(
            &mut stream,
            2,
            thread_state("idle", vec![turn(TURN_ID, "interrupted")]),
        )
        .await;
        send_success(
            &mut stream,
            &stop,
            json!({"ok":true,"interruptedTurnId":TURN_ID}),
            Some(OWNER_ID),
        )
        .await;
        expect_no_frame(&mut stream).await;
        no_start_tx
            .send(())
            .expect("fake owner confirms Stop did not drain the queue");

        let following = timeout(Duration::from_secs(3), read_frame(&mut stream))
            .await
            .expect("worker closes its test session")
            .expect("session close is framed");
        assert_eq!(following["method"], "thread-stream-following-changed");
        assert_eq!(following["params"]["following"], false);
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to active fake desktop thread");
    let worker = DesktopWorker::spawn(session, ledger.clone(), source.clone())
        .await
        .expect("worker starts with queued items");
    worker
        .act(DesktopAction::Stop {
            turn_id: TURN_ID.into(),
        })
        .await
        .expect("matching active turn is stopped");
    let view = wait_for_view(&worker, |view| {
        view.queue.len() == 2 && view.queue.iter().all(|item| item.status == "held")
    })
    .await;
    assert_eq!(view.queue[0].id, FIRST_MESSAGE_ID);
    assert_eq!(view.queue[1].id, SECOND_MESSAGE_ID);
    no_start_rx
        .await
        .expect("fake owner observes no queue dispatch after Stop");

    let records = ledger
        .list(&source, THREAD_ID)
        .expect("held queue records remain decryptable");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].status, "held");
    assert_eq!(
        records[0].body.as_ref().unwrap()["text"],
        "fixture first queued message"
    );
    assert_eq!(records[1].status, "held");
    assert_eq!(
        records[1].body.as_ref().unwrap()["text"],
        "fixture second queued message"
    );

    drop(worker);
    broker.await.expect("fake broker assertions pass");
}

#[tokio::test]
async fn out_of_sync_holds_queue_rotates_session_and_resubscribes_to_snapshot() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    fixture.enqueue_pair(&ledger, &source);
    let listener = fixture.bind();
    let (recovered_tx, recovered_rx) = oneshot::channel();

    let broker = tokio::spawn(async move {
        let mut old_stream = accept_session(
            &listener,
            thread_state("active", vec![turn(TURN_ID, "inProgress")]),
        )
        .await;
        send_out_of_sync_patch(&mut old_stream).await;
        assert_eq!(
            timeout(Duration::from_secs(3), read_frame(&mut old_stream))
                .await
                .expect("out-of-sync stream closes before recovery"),
            Err(DesktopError::Disconnected)
        );

        let mut recovered_state = thread_state("idle", Vec::new());
        recovered_state["latestModel"] = json!("fixture-recovered-model");
        let mut recovered_stream = accept_session(&listener, recovered_state).await;
        expect_no_frame(&mut recovered_stream).await;
        recovered_tx
            .send(())
            .expect("recovered worker sends no mutation before test action");

        expect_unfollow_then_close(&mut recovered_stream).await;
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to the initial fake desktop session");
    let worker = DesktopWorker::spawn(session, ledger, source)
        .await
        .expect("worker starts with queued work");
    let old_session_id = worker.current().session_id;
    recovered_rx
        .await
        .expect("fake broker observes the automatic resubscription");
    let recovered = wait_for_view(&worker, |view| {
        view.session_id != old_session_id
            && view.connection_phase.as_deref() == Some("connected")
            && view.model.as_deref() == Some("fixture-recovered-model")
            && view.queue.len() == 2
            && view.queue.iter().all(|item| item.status == "held")
    })
    .await;
    assert!(recovered.connected);
    assert!(recovered.writable);

    let stale_action = worker
        .act_in_session(settings_action(), Vec::new(), old_session_id)
        .await;
    assert!(matches!(stale_action, Err(DesktopError::RequestExpired)));

    drop(worker);
    broker.await.expect("fake broker assertions pass");
}

#[tokio::test]
async fn changed_owner_requires_explicit_confirmation_before_writes() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let listener = fixture.bind();
    let (new_owner_tx, new_owner_rx) = oneshot::channel();
    let (check_no_write_tx, check_no_write_rx) = oneshot::channel();
    let (no_write_checked_tx, no_write_checked_rx) = oneshot::channel();

    let broker = tokio::spawn(async move {
        let mut old_stream = accept_session(&listener, thread_state("idle", Vec::new())).await;
        send_out_of_sync_patch(&mut old_stream).await;
        assert_eq!(
            timeout(Duration::from_secs(3), read_frame(&mut old_stream))
                .await
                .expect("out-of-sync stream closes before changed-owner recovery"),
            Err(DesktopError::Disconnected)
        );

        let mut new_stream =
            accept_session_as(&listener, thread_state("idle", Vec::new()), OTHER_OWNER_ID).await;
        new_owner_tx
            .send(())
            .expect("worker attaches to the changed native owner");
        check_no_write_rx
            .await
            .expect("test checks that unconfirmed owner receives no request");
        expect_no_frame(&mut new_stream).await;
        no_write_checked_tx
            .send(())
            .expect("test observes the no-write interval before owner confirmation");

        let confirmation = expect_request(&mut new_stream, "thread-owner-discovery").await;
        send_success(
            &mut new_stream,
            &confirmation,
            json!({}),
            Some(OTHER_OWNER_ID),
        )
        .await;
        let owner_check = expect_request(&mut new_stream, "thread-owner-discovery").await;
        send_success(
            &mut new_stream,
            &owner_check,
            json!({}),
            Some(OTHER_OWNER_ID),
        )
        .await;
        let settings =
            expect_request(&mut new_stream, "thread-follower-update-thread-settings").await;
        send_success(
            &mut new_stream,
            &settings,
            json!({"applied":true}),
            Some(OTHER_OWNER_ID),
        )
        .await;

        expect_unfollow_then_close(&mut new_stream).await;
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to the initial fake desktop session");
    let worker = DesktopWorker::spawn(session, ledger, source)
        .await
        .expect("worker starts against the initial owner");
    new_owner_rx
        .await
        .expect("fake broker observes the changed-owner attachment");
    let read_only = wait_for_view(&worker, |view| {
        view.connection_phase.as_deref() == Some("owner_confirmation_required")
    })
    .await;
    assert!(read_only.connected);
    assert!(!read_only.writable);
    assert!(matches!(
        worker
            .act_in_session(settings_action(), Vec::new(), read_only.session_id)
            .await,
        Err(DesktopError::ReadOnly)
    ));
    check_no_write_tx
        .send(())
        .expect("fake broker is ready to verify no unconfirmed request was sent");
    no_write_checked_rx
        .await
        .expect("fake broker observed no request before explicit confirmation");

    let confirmed = worker
        .confirm_owner()
        .await
        .expect("explicit confirmation rechecks the replacement owner");
    assert_eq!(confirmed.connection_phase.as_deref(), Some("connected"));
    assert!(confirmed.writable);
    let written = worker
        .act(settings_action())
        .await
        .expect("confirmed replacement owner accepts a user mutation");
    assert!(written.writable);

    drop(worker);
    broker.await.expect("fake broker assertions pass");
}

#[tokio::test(start_paused = true)]
async fn reconnect_uses_only_five_bounded_backoff_attempts() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let listener = fixture.bind();
    let (attempt_tx, mut attempt_rx) = mpsc::unbounded_channel();
    let (trigger_reset_tx, trigger_reset_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let (clock_stop_tx, clock_stop_rx) = oneshot::channel();
    let clock_driver = tokio::spawn(async move {
        let mut stop = clock_stop_rx;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                _ = &mut stop => break,
            }
        }
    });

    let broker = tokio::spawn(async move {
        let mut initial = accept_session(&listener, thread_state("idle", Vec::new())).await;
        trigger_reset_rx
            .await
            .expect("worker is ready before the fake IPC disconnects");
        send_connection_reset(&mut initial).await;
        let closed = read_frame(&mut initial)
            .await
            .expect_err("connection-reset discards the previous stream");
        assert_eq!(closed, DesktopError::Disconnected);

        for attempt in 1..=5 {
            let (mut stream, _) = listener
                .accept()
                .await
                .expect("fake listener accepts a bounded recovery attempt");
            let initialize = read_frame(&mut stream)
                .await
                .expect("recovery attempt sends initialize");
            assert_eq!(initialize["method"], "initialize");
            drop(stream);
            attempt_tx
                .send(attempt)
                .expect("test observes each failed recovery attempt");
        }

        let mut finish_rx = finish_rx;
        let mut next_attempt = 6;
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (mut stream, _) = accepted.expect("fake listener accepts any excess retry");
                    let initialize = read_frame(&mut stream)
                        .await
                        .expect("excess recovery attempt sends initialize");
                    assert_eq!(initialize["method"], "initialize");
                    attempt_tx
                        .send(next_attempt)
                        .expect("test observes an out-of-bound retry");
                    next_attempt += 1;
                    drop(stream);
                }
                _ = &mut finish_rx => break,
            }
        }
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to the initial fake desktop session");
    let worker = DesktopWorker::spawn(session, ledger, source)
        .await
        .expect("worker starts before the fake IPC disconnects");
    let original_session = worker.current().session_id;
    trigger_reset_tx
        .send(())
        .expect("fake broker disconnects only after the worker is ready");
    let reconnecting = wait_for_view_by_yield(&worker, |view| {
        view.session_id != original_session
            && view.connection_phase.as_deref() == Some("reconnecting")
    })
    .await;
    assert!(!reconnecting.writable);
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }

    for (attempt, delay) in [1_u64, 2, 4, 8, 16].into_iter().enumerate() {
        assert!(
            attempt_rx.try_recv().is_err(),
            "retry {} waits for its scheduled backoff",
            attempt + 1
        );
        tokio::time::advance(Duration::from_secs(delay)).await;
        wait_for_retry_attempt(&mut attempt_rx, attempt + 1).await;
    }
    let disconnected = wait_for_view_by_yield(&worker, |view| {
        view.connection_phase.as_deref() == Some("disconnected")
    })
    .await;
    assert!(!disconnected.connected);
    assert!(worker.is_closed());

    tokio::time::advance(Duration::from_secs(60)).await;
    for _ in 0..1_000 {
        tokio::task::yield_now().await;
    }
    assert!(attempt_rx.try_recv().is_err(), "no sixth retry is started");
    clock_stop_tx
        .send(())
        .expect("virtual clock driver stops after the retry bound is verified");
    clock_driver.await.expect("virtual clock driver exits");
    finish_tx
        .send(())
        .expect("fake broker stops waiting after the retry bound is verified");
    broker.await.expect("fake broker assertions pass");
}

#[tokio::test]
async fn native_request_removal_clears_worker_projection_and_stales_old_key() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let listener = fixture.bind();
    let (request_visible_tx, request_visible_rx) = oneshot::channel();
    let (no_replay_tx, no_replay_rx) = oneshot::channel();

    let broker = tokio::spawn(async move {
        let mut state = thread_state("active", vec![turn(TURN_ID, "inProgress")]);
        state["requests"] = json!([{
            "id":"fixture-question-request",
            "method":"item/tool/requestUserInput",
            "params":{
                "threadId":THREAD_ID,
                "turnId":TURN_ID,
                "questions":[{"id":"fixture-question","question":"Fixture question"}]
            }
        }]);
        let mut stream = accept_session(&listener, state).await;
        request_visible_tx
            .send(())
            .expect("worker received the synthetic pending request");

        let owner_check = expect_request(&mut stream, "thread-owner-discovery").await;
        send_success(&mut stream, &owner_check, json!({}), Some(OWNER_ID)).await;
        let answer = expect_request(&mut stream, "thread-follower-submit-user-input").await;
        assert_eq!(answer["params"]["requestId"], "fixture-question-request");
        write_frame(
            &mut stream,
            &json!({
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
            }),
        )
        .await
        .expect("native owner removes the pending request");
        send_success(&mut stream, &answer, json!({"ok":true}), Some(OWNER_ID)).await;
        expect_no_frame(&mut stream).await;
        no_replay_tx
            .send(())
            .expect("test observes the stale request key sends no IPC frame");

        expect_unfollow_then_close(&mut stream).await;
    });

    let session = DesktopSession::attach(&fixture.codex_home, THREAD_ID)
        .await
        .expect("worker attaches to the synthetic pending request");
    let worker = DesktopWorker::spawn(session, ledger, source)
        .await
        .expect("worker starts with a visible request");
    request_visible_rx
        .await
        .expect("fake broker sent the request snapshot");
    let request_key = wait_for_view(&worker, |view| !view.requests.is_empty())
        .await
        .requests[0]
        .key
        .clone();
    let answer = || DesktopAction::Answer {
        request_key: request_key.clone(),
        answers: vec![DesktopAnswer {
            question_id: "fixture-question".into(),
            answers: vec!["fixture answer".into()],
        }],
    };

    let result = worker
        .act(answer())
        .await
        .expect("answer is accepted natively");
    assert!(result.requests.is_empty());
    assert_eq!(result.revision, 2);
    assert!(matches!(
        worker.act(answer()).await,
        Err(DesktopError::RequestExpired)
    ));
    no_replay_rx
        .await
        .expect("fake owner observed no frame for the stale request key");

    drop(worker);
    broker.await.expect("fake broker assertions pass");
}

fn settings_action() -> DesktopAction {
    DesktopAction::Settings {
        model: "fixture-model".into(),
        reasoning_effort: None,
        mode: crate::services::codex_desktop::DesktopMode::Default,
    }
}
