#![cfg(unix)]

use super::framing::{FrameReader, FrameWriter};
use super::{socket, state::NativeState, DesktopError, DesktopEvent, DesktopReadSession};
use crate::resource_limits::CODEX_DESKTOP_FRAME_BYTES;
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    time::timeout,
};

const THREAD_ID: &str = "11111111-1111-4111-8111-111111111111";
const CLIENT_ID: &str = "agenthub-test-client";
const OWNER_ID: &str = "codex-desktop-owner";
const WAIT: Duration = Duration::from_secs(2);

fn goal_replica(state: Value) -> NativeState {
    let mut replica = NativeState::new(THREAD_ID).expect("fixture thread ID is valid");
    replica
        .apply(DesktopEvent::Snapshot { revision: 1, state })
        .expect("fixture snapshot is accepted");
    replica
}

fn projected_goal(replica: &NativeState) -> Option<crate::services::codex_desktop::DesktopGoal> {
    super::projection::view(replica, "fixture-session", Vec::new()).goal
}

#[test]
fn goal_projection_tracks_updates_clearing_and_completed_goal() {
    let mut replica = goal_replica(json!({
        "id": THREAD_ID,
        "threadGoal": {"objective": "first fixture", "status": "active"}
    }));
    let active = projected_goal(&replica).expect("active goal is projected");
    assert_eq!(active.objective, "first fixture");
    assert_eq!(active.status, "active");
    assert!(!active.truncated);

    replica
        .apply(DesktopEvent::Patches {
            revision: 2,
            change: json!({
                "baseRevision": 1,
                "patches": [{
                    "op": "replace",
                    "path": ["threadGoal"],
                    "value": {"objective": "updated fixture", "status": "paused"}
                }]
            }),
        })
        .expect("goal update patch is accepted");
    let updated = projected_goal(&replica).expect("updated goal is projected");
    assert_eq!(updated.objective, "updated fixture");
    assert_eq!(updated.status, "paused");

    replica
        .apply(DesktopEvent::Patches {
            revision: 3,
            change: json!({
                "baseRevision": 2,
                "patches": [{"op": "remove", "path": ["threadGoal"]}]
            }),
        })
        .expect("goal removal patch is accepted");
    assert!(projected_goal(&replica).is_none());

    replica
        .apply(DesktopEvent::Patches {
            revision: 4,
            change: json!({
                "baseRevision": 3,
                "patches": [{
                    "op": "add",
                    "path": ["completedThreadGoal"],
                    "value": {"objective": "completed fixture", "status": "complete"}
                }]
            }),
        })
        .expect("completed goal patch is accepted");
    let completed = projected_goal(&replica).expect("completed goal is projected");
    assert_eq!(completed.objective, "completed fixture");
    assert_eq!(completed.status, "complete");
}

#[test]
fn goal_projection_handles_missing_and_unknown_status() {
    let missing = goal_replica(json!({"id": THREAD_ID}));
    assert!(projected_goal(&missing).is_none());

    let unknown = goal_replica(json!({
        "id": THREAD_ID,
        "threadGoal": {"objective": "fixture", "status": "futureStatus"}
    }));
    assert_eq!(projected_goal(&unknown).unwrap().status, "unknown");
}

#[test]
fn goal_objective_respects_byte_cap_at_utf8_boundaries() {
    let at_limit = format!("{}中", "a".repeat(4093));
    let replica = goal_replica(json!({
        "id": THREAD_ID,
        "threadGoal": {"objective": at_limit, "status": "active"}
    }));
    let goal = projected_goal(&replica).unwrap();
    assert_eq!(goal.objective.len(), 4096);
    assert!(!goal.truncated);

    let over_limit = format!("{}中", "a".repeat(4094));
    let replica = goal_replica(json!({
        "id": THREAD_ID,
        "threadGoal": {"objective": over_limit, "status": "active"}
    }));
    let goal = projected_goal(&replica).unwrap();
    assert!(goal.truncated);
    assert!(goal.objective.is_char_boundary(goal.objective.len()));
    assert!(goal.objective.ends_with('…'));
    assert!(
        goal.objective.len() <= 4096,
        "{} bytes",
        goal.objective.len()
    );
}

#[test]
fn legacy_desktop_view_without_goal_deserializes() {
    let view: crate::services::codex_desktop::DesktopView = serde_json::from_value(json!({
        "sessionId": "fixture-session",
        "threadId": THREAD_ID,
        "revision": 1,
        "connected": true,
        "writable": false,
        "runtime": "idle",
        "activeTurnId": null,
        "model": null,
        "reasoningEffort": null,
        "mode": null,
        "requests": [],
        "queue": [],
        "error": null
    }))
    .expect("legacy DTO without goal remains readable");

    assert!(view.goal.is_none());
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("nh-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).expect("create isolated desktop IPC fixture");
        set_mode(&root, 0o700);
        Self { root }
    }

    fn create_ipc_dir(&self) -> PathBuf {
        let ipc = self.root.join("ipc");
        fs::create_dir(&ipc).expect("create private IPC directory");
        set_mode(&ipc, 0o700);
        ipc
    }

    fn bind_socket(&self) -> (UnixListener, PathBuf) {
        let socket_path = self.create_ipc_dir().join("ipc.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fixture IPC socket");
        set_mode(&socket_path, 0o600);
        (listener, socket_path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn set_mode(path: &Path, mode: u32) {
    let mut permissions = fs::symlink_metadata(path)
        .expect("read fixture permissions")
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions).expect("set fixture permissions");
}

fn test_session(stream: UnixStream, thread_id: &str) -> DesktopReadSession {
    DesktopReadSession {
        stream: Some(stream),
        reader: FrameReader::default(),
        writer: FrameWriter::default(),
        conversation_id: thread_id.to_owned(),
        client_id: CLIENT_ID.to_owned(),
        owner_id: OWNER_ID.to_owned(),
        revision: None,
    }
}

fn session_pair(thread_id: &str) -> (DesktopReadSession, UnixStream) {
    let (stream, peer) = UnixStream::pair().expect("create isolated IPC stream pair");
    (test_session(stream, thread_id), peer)
}

async fn write_wire(stream: &mut UnixStream, value: &Value) {
    super::framing::write_frame(stream, value)
        .await
        .expect("write fixture frame");
}

async fn read_wire(reader: &mut FrameReader, stream: &mut UnixStream) -> Value {
    timeout(WAIT, reader.read(stream))
        .await
        .expect("fixture frame read completes")
        .expect("fixture frame is valid")
}

async fn assert_peer_closed(stream: &mut UnixStream) {
    let mut byte = [0; 1];
    let count = timeout(WAIT, stream.read(&mut byte))
        .await
        .expect("peer observes stream closure")
        .expect("read peer closure");
    assert_eq!(count, 0, "expected EOF after the client rejected the event");
}

fn snapshot(owner: &str, thread_id: &str, revision: u64, version: u64) -> Value {
    json!({
        "type": "broadcast",
        "sourceClientId": owner,
        "method": "thread-stream-state-changed",
        "version": version,
        "params": {
            "hostId": "local",
            "conversationId": thread_id,
            "change": {
                "type": "snapshot",
                "revision": revision,
                "conversationState": {
                    "id": thread_id,
                    "title": "fixture snapshot"
                }
            }
        }
    })
}

fn patches(owner: &str, thread_id: &str, base: u64, revision: u64) -> Value {
    json!({
        "type": "broadcast",
        "sourceClientId": owner,
        "method": "thread-stream-state-changed",
        "version": super::STREAM_VERSION,
        "params": {
            "hostId": "local",
            "conversationId": thread_id,
            "change": {
                "type": "patches",
                "baseRevision": base,
                "revision": revision,
                "patches": [{"op": "replace", "path": ["title"], "value": "fixture patch"}]
            }
        }
    })
}

fn expect_error<T>(result: std::result::Result<T, DesktopError>) -> DesktopError {
    match result {
        Ok(_) => panic!("expected a protocol error"),
        Err(error) => error,
    }
}

async fn assert_unsafe_socket(path: &Path) {
    assert_eq!(
        expect_error(socket::connect(path).await),
        DesktopError::UnsafeSocket
    );
}

#[tokio::test]
async fn attach_discovers_the_existing_owner_and_declines_client_discovery() {
    let fixture = Fixture::new();
    let (listener, _) = fixture.bind_socket();
    let server = tokio::spawn(async move {
        let (mut stream, _) = timeout(WAIT, listener.accept())
            .await
            .expect("client connects to fixture")
            .expect("accept fixture connection");
        let mut reader = FrameReader::default();

        let initialize = read_wire(&mut reader, &mut stream).await;
        assert_eq!(initialize["method"], "initialize");
        assert_eq!(initialize["version"], 0);
        assert_eq!(initialize["params"]["clientType"], "agenthub");
        assert!(initialize["sourceClientId"].is_null());

        write_wire(
            &mut stream,
            &json!({
                "type": "client-discovery-request",
                "requestId": "fixture-discovery-request"
            }),
        )
        .await;
        let discovery_response = read_wire(&mut reader, &mut stream).await;
        assert_eq!(discovery_response["type"], "client-discovery-response");
        assert_eq!(discovery_response["requestId"], "fixture-discovery-request");
        assert_eq!(discovery_response["response"]["canHandle"], false);

        write_wire(
            &mut stream,
            &json!({
                "type": "response",
                "requestId": initialize["requestId"].clone(),
                "resultType": "success",
                "result": {"clientId": CLIENT_ID}
            }),
        )
        .await;

        let discovery = read_wire(&mut reader, &mut stream).await;
        assert_eq!(discovery["method"], "thread-owner-discovery");
        assert_eq!(discovery["version"], 1);
        assert_eq!(discovery["sourceClientId"], CLIENT_ID);
        assert_eq!(discovery["params"]["hostId"], "local");
        assert_eq!(discovery["params"]["conversationId"], THREAD_ID);
        write_wire(
            &mut stream,
            &json!({
                "type": "response",
                "requestId": discovery["requestId"].clone(),
                "resultType": "success",
                "handledByClientId": OWNER_ID
            }),
        )
        .await;

        let follow = read_wire(&mut reader, &mut stream).await;
        assert_eq!(follow["type"], "broadcast");
        assert_eq!(follow["method"], "thread-stream-following-changed");
        assert_eq!(follow["sourceClientId"], CLIENT_ID);
        assert_eq!(follow["targetClientIds"], json!([OWNER_ID]));
        assert_eq!(follow["params"]["conversationId"], THREAD_ID);
        assert_eq!(follow["params"]["following"], true);

        let unsubscribe = read_wire(&mut reader, &mut stream).await;
        assert_eq!(unsubscribe["method"], "thread-stream-following-changed");
        assert_eq!(unsubscribe["params"]["following"], false);
    });

    let session = DesktopReadSession::attach(&fixture.root, THREAD_ID)
        .await
        .expect("attach to the fixture's verified owner");
    assert_eq!(session.client_id, CLIENT_ID);
    assert_eq!(session.owner_id, OWNER_ID);
    session.close().await;
    timeout(WAIT, server)
        .await
        .expect("fixture server completes")
        .expect("fixture server assertions pass");
}

#[tokio::test]
async fn stream_ignores_other_owners_and_threads_then_accepts_snapshot_and_batch_patch() {
    let (mut session, mut peer) = session_pair(THREAD_ID);

    write_wire(
        &mut peer,
        &snapshot("another-owner", THREAD_ID, 90, super::STREAM_VERSION),
    )
    .await;
    write_wire(
        &mut peer,
        &snapshot(
            OWNER_ID,
            "22222222-2222-4222-8222-222222222222",
            91,
            super::STREAM_VERSION,
        ),
    )
    .await;
    write_wire(
        &mut peer,
        &snapshot(OWNER_ID, THREAD_ID, 40, super::STREAM_VERSION),
    )
    .await;

    match timeout(WAIT, session.next_event())
        .await
        .expect("snapshot arrives")
        .expect("verified snapshot is accepted")
    {
        DesktopEvent::Snapshot { revision, state } => {
            assert_eq!(revision, 40);
            assert_eq!(state["id"], THREAD_ID);
            assert_eq!(state["title"], "fixture snapshot");
        }
        _ => panic!("expected a snapshot after unrelated broadcasts were ignored"),
    }

    write_wire(&mut peer, &patches(OWNER_ID, THREAD_ID, 40, 45)).await;
    match timeout(WAIT, session.next_event())
        .await
        .expect("batch patch arrives")
        .expect("base-matched patch batch is accepted")
    {
        DesktopEvent::Patches { revision, change } => {
            assert_eq!(revision, 45);
            assert_eq!(change["baseRevision"], 40);
            assert_eq!(change["patches"].as_array().map(Vec::len), Some(1));
        }
        _ => panic!("expected a patch event"),
    }
}

#[tokio::test]
async fn patch_base_gaps_and_non_advanced_revisions_fail_closed() {
    for (base, revision) in [(39, 41), (40, 40), (40, 39)] {
        let (mut session, mut peer) = session_pair(THREAD_ID);
        write_wire(
            &mut peer,
            &snapshot(OWNER_ID, THREAD_ID, 40, super::STREAM_VERSION),
        )
        .await;
        match timeout(WAIT, session.next_event())
            .await
            .expect("initial snapshot arrives")
            .expect("initial snapshot is valid")
        {
            DesktopEvent::Snapshot { revision: 40, .. } => {}
            _ => panic!("expected the initial snapshot"),
        }

        write_wire(&mut peer, &patches(OWNER_ID, THREAD_ID, base, revision)).await;
        assert_eq!(
            expect_error(
                timeout(WAIT, session.next_event())
                    .await
                    .expect("patch is handled")
            ),
            DesktopError::OutOfSync
        );
        assert_peer_closed(&mut peer).await;
    }
}

#[tokio::test]
async fn unsupported_versions_and_native_error_text_are_not_exposed() {
    const SENTINEL: &str = "example.invalid/native-error-sentinel";

    let (mut session, mut peer) = session_pair(THREAD_ID);
    let server = tokio::spawn(async move {
        let mut reader = FrameReader::default();

        let future_request = read_wire(&mut reader, &mut peer).await;
        assert_eq!(future_request["method"], "thread-owner-discovery");
        assert_eq!(future_request["version"], 1);
        write_wire(
            &mut peer,
            &json!({
                "type": "response",
                "requestId": future_request["requestId"].clone(),
                "resultType": "error",
                "error": "unsupported-version",
                "message": SENTINEL
            }),
        )
        .await;

        let hostile_request = read_wire(&mut reader, &mut peer).await;
        assert_eq!(hostile_request["method"], "thread-owner-discovery");
        write_wire(
            &mut peer,
            &json!({
                "type": "response",
                "requestId": hostile_request["requestId"].clone(),
                "resultType": "error",
                "error": SENTINEL,
                "message": SENTINEL
            }),
        )
        .await;

        write_wire(
            &mut peer,
            &json!({
                "type": "broadcast",
                "sourceClientId": OWNER_ID,
                "method": "thread-stream-state-changed",
                "version": 99,
                "params": {
                    "hostId": "local",
                    "conversationId": THREAD_ID
                }
            }),
        )
        .await;
        assert_peer_closed(&mut peer).await;
    });

    let unsupported = expect_error(
        session
            .request(super::protocol::Method::DiscoverOwner, json!({}))
            .await,
    );
    assert_eq!(unsupported, DesktopError::UnsupportedProtocol);
    assert!(!unsupported.to_string().contains(SENTINEL));
    assert!(!format!("{unsupported:?}").contains(SENTINEL));

    let sanitized = expect_error(
        session
            .request(super::protocol::Method::DiscoverOwner, json!({}))
            .await,
    );
    assert_eq!(sanitized, DesktopError::Unavailable);
    assert!(!sanitized.to_string().contains(SENTINEL));
    assert!(!format!("{sanitized:?}").contains(SENTINEL));

    assert_eq!(
        expect_error(session.next_event().await),
        DesktopError::UnsupportedProtocol
    );
    timeout(WAIT, server)
        .await
        .expect("fixture server completes")
        .expect("fixture server assertions pass");
}

#[tokio::test]
async fn frame_reader_resumes_fragmented_header_and_body_after_cancellation() {
    let (mut receiver, mut sender) = UnixStream::pair().expect("create fixture stream pair");
    let value = json!({"event": "fragmented", "revision": 23});
    let body = serde_json::to_vec(&value).expect("encode fixture JSON");
    let header = (body.len() as u32).to_le_bytes();
    let mut reader = FrameReader::default();

    sender
        .write_all(&header[..2])
        .await
        .expect("write partial header");
    assert!(
        timeout(Duration::from_millis(25), reader.read(&mut receiver))
            .await
            .is_err()
    );

    sender
        .write_all(&header[2..])
        .await
        .expect("write rest of header");
    sender
        .write_all(&body[..3])
        .await
        .expect("write partial body");
    assert!(
        timeout(Duration::from_millis(25), reader.read(&mut receiver))
            .await
            .is_err()
    );

    sender
        .write_all(&body[3..])
        .await
        .expect("write rest of body");
    assert_eq!(
        timeout(WAIT, reader.read(&mut receiver))
            .await
            .expect("resumed read completes")
            .expect("fragmented JSON frame is valid"),
        value
    );
}

#[tokio::test]
async fn frame_writer_flushes_a_cancelled_partial_write_without_duplication() {
    let (mut writer_stream, mut peer) = tokio::io::duplex(4);
    let value = json!({"payload": "a frame larger than the duplex buffer"});
    let body = serde_json::to_vec(&value).expect("encode fixture JSON");
    let expected_len = 4 + body.len();
    let mut writer = FrameWriter::default();

    assert!(timeout(
        Duration::from_millis(25),
        writer.send(&mut writer_stream, &value)
    )
    .await
    .is_err());

    let mut received = vec![0; expected_len];
    peer.read_exact(&mut received[..4])
        .await
        .expect("read the frame header written before cancellation");
    assert_eq!(
        u32::from_le_bytes(received[..4].try_into().expect("four-byte header")) as usize,
        body.len()
    );

    let (flush_result, read_result) = timeout(WAIT, async {
        tokio::join!(
            writer.flush(&mut writer_stream),
            peer.read_exact(&mut received[4..])
        )
    })
    .await
    .expect("resumed write and peer read complete");
    flush_result.expect("writer resumes from its saved offset");
    read_result.expect("read the remainder of the frame");
    assert_eq!(
        serde_json::from_slice::<Value>(&received[4..]).expect("decode one complete frame"),
        value
    );

    writer
        .flush(&mut writer_stream)
        .await
        .expect("empty flush does not replay the frame");
    let mut extra = [0; 1];
    assert!(timeout(Duration::from_millis(25), peer.read(&mut extra))
        .await
        .is_err());
}

async fn frame_error(bytes: &[u8]) -> DesktopError {
    let (mut receiver, mut sender) = UnixStream::pair().expect("create fixture stream pair");
    if !bytes.is_empty() {
        sender
            .write_all(bytes)
            .await
            .expect("write malformed frame");
    }
    drop(sender);
    expect_error(FrameReader::default().read(&mut receiver).await)
}

fn raw_frame(body: &[u8]) -> Vec<u8> {
    let mut frame = (body.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(body);
    frame
}

#[tokio::test]
async fn frame_reader_rejects_empty_oversized_invalid_and_truncated_frames() {
    assert_eq!(frame_error(&[]).await, DesktopError::Disconnected);
    assert_eq!(
        frame_error(&0_u32.to_le_bytes()).await,
        DesktopError::InvalidFrame
    );
    assert_eq!(
        frame_error(&((CODEX_DESKTOP_FRAME_BYTES as u32) + 1).to_le_bytes()).await,
        DesktopError::FrameTooLarge
    );
    assert_eq!(
        frame_error(&raw_frame(b"{not-json")).await,
        DesktopError::InvalidFrame
    );
    assert_eq!(frame_error(&[2, 0]).await, DesktopError::Disconnected);

    let mut truncated_body = 6_u32.to_le_bytes().to_vec();
    truncated_body.extend_from_slice(b"{}");
    assert_eq!(
        frame_error(&truncated_body).await,
        DesktopError::Disconnected
    );
}

#[tokio::test]
async fn connect_rejects_symlinked_home_ipc_and_socket_paths() {
    {
        let fixture = Fixture::new();
        let target = fixture.root.join("real-home");
        fs::create_dir(&target).expect("create target home");
        set_mode(&target, 0o700);
        let target_ipc = target.join("ipc");
        fs::create_dir(&target_ipc).expect("create target IPC directory");
        set_mode(&target_ipc, 0o700);
        let alias = fixture.root.join("home-link");
        symlink(&target, &alias).expect("create home symlink");
        assert_unsafe_socket(&alias).await;
    }

    {
        let fixture = Fixture::new();
        let target = fixture.root.join("external-ipc");
        fs::create_dir(&target).expect("create IPC symlink target");
        set_mode(&target, 0o700);
        symlink(&target, fixture.root.join("ipc")).expect("create IPC directory symlink");
        assert_unsafe_socket(&fixture.root).await;
    }

    {
        let fixture = Fixture::new();
        let ipc = fixture.create_ipc_dir();
        let real_socket = fixture.root.join("real.sock");
        let listener = UnixListener::bind(&real_socket).expect("bind real fixture socket");
        set_mode(&real_socket, 0o600);
        symlink(&real_socket, ipc.join("ipc.sock")).expect("create socket symlink");
        assert_unsafe_socket(&fixture.root).await;
        drop(listener);
    }
}

#[tokio::test]
async fn connect_rejects_insecure_permissions_and_non_socket_files() {
    {
        let fixture = Fixture::new();
        let (listener, _) = fixture.bind_socket();
        set_mode(&fixture.root, 0o770);
        assert_unsafe_socket(&fixture.root).await;
        drop(listener);
    }

    {
        let fixture = Fixture::new();
        let (listener, _) = fixture.bind_socket();
        set_mode(&fixture.root.join("ipc"), 0o770);
        assert_unsafe_socket(&fixture.root).await;
        drop(listener);
    }

    {
        let fixture = Fixture::new();
        let (listener, socket_path) = fixture.bind_socket();
        set_mode(&socket_path, 0o640);
        assert_unsafe_socket(&fixture.root).await;
        drop(listener);
    }

    {
        let fixture = Fixture::new();
        let socket_path = fixture.create_ipc_dir().join("ipc.sock");
        fs::write(&socket_path, b"regular fixture file").expect("create non-socket endpoint");
        assert_unsafe_socket(&fixture.root).await;
    }
}

#[tokio::test]
async fn attach_timeout_closes_the_accepted_socket() {
    let fixture = Fixture::new();
    let (listener, _) = fixture.bind_socket();
    let server = tokio::spawn(async move {
        let (mut stream, _) = timeout(WAIT, listener.accept())
            .await
            .expect("client reaches the fixture socket")
            .expect("accept fixture connection");
        let mut reader = FrameReader::default();
        let initialize = read_wire(&mut reader, &mut stream).await;
        assert_eq!(initialize["method"], "initialize");
        assert_peer_closed(&mut stream).await;
    });

    assert_eq!(
        expect_error(
            DesktopReadSession::attach_with_timeout(
                &fixture.root,
                THREAD_ID,
                Duration::from_millis(500)
            )
            .await
        ),
        DesktopError::Timeout
    );
    timeout(WAIT, server)
        .await
        .expect("fixture server completes")
        .expect("fixture server assertions pass");
}
