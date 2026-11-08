use super::*;
use serde_json::json;
fn state() -> State {
    let mut s = State::default();
    s.consume(
        format!(
            "{}",
            json!({"type":"session","version":4,"id":"fixture","isSeeded":false})
        )
        .as_bytes(),
    )
    .unwrap();
    s
}
fn event(s: &mut State, kind: &str, data: Value) {
    s.consume(json!({"type":kind,"seq":s.next_seq,"time":1_800_000_000_000_i64+s.next_seq as i64,"data":data}).to_string().as_bytes()).unwrap();
}
fn kinds(s: &State) -> Vec<String> {
    s.snapshot("fixture", "project/fixture", "source")
        .unwrap()
        .unwrap()
        .events
        .into_iter()
        .map(|e| e.kind)
        .collect()
}
#[test]
fn completion_failure_cancel_and_resume_follow_main_turn_boundaries() {
    let mut s = state();
    event(&mut s, "turn/start", json!({"turn":1}));
    event(
        &mut s,
        "assistant/message",
        json!({"turn":1,"message":{"content":[{"type":"text","text":"Fixture answer"},{"type":"reasoning","text":"private"}]}}),
    );
    event(
        &mut s,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    assert_eq!(kinds(&s), ["completion"]);
    assert_eq!(s.events[0].body, "Fixture answer");
    event(&mut s, "turn/start", json!({"turn":2}));
    event(
        &mut s,
        "turn/end",
        json!({"turn":2,"reason":{"kind":"error"}}),
    );
    assert_eq!(kinds(&s), ["completion", "failure"]);
    event(&mut s, "turn/start", json!({"turn":3}));
    assert_eq!(kinds(&s), ["completion"]);
    event(
        &mut s,
        "turn/end",
        json!({"turn":3,"reason":{"kind":"aborted"}}),
    );
    assert_eq!(kinds(&s), ["completion"]);
}
#[test]
fn approvals_need_an_unanswered_native_request_and_clear_on_decision() {
    let mut s = state();
    event(&mut s, "turn/start", json!({"turn":1}));
    event(
        &mut s,
        "tool/call",
        json!({"turn":1,"callId":"tool","name":"shell","arguments":"{}"}),
    );
    assert!(kinds(&s).is_empty());
    event(
        &mut s,
        "approval/asked",
        json!({"id":"approval","toolName":"shell"}),
    );
    assert_eq!(kinds(&s), ["authorization"]);
    event(
        &mut s,
        "approval/decided",
        json!({"id":"approval","outcome":"allowed-once"}),
    );
    assert!(kinds(&s).is_empty());
}
#[test]
fn timed_question_survives_pending_results_turn_end_and_other_user_messages() {
    let mut s = state();
    event(
        &mut s,
        "request/header",
        json!({"header":{"tools":[{"name":"ask_user_question","parameters":{"properties":{"timeout":{}}}}]}}),
    );
    event(&mut s, "turn/start", json!({"turn":1}));
    event(
        &mut s,
        "tool/call",
        json!({"turn":1,"name":"ask_user_question","callId":"ask","arguments":"{\"questions\":[{\"question\":\"Continue?\"}]}"}),
    );
    let original = s.pending["question:ask"].0.clone();
    event(
        &mut s,
        "tool/result",
        json!({"message":{"toolCallId":"ask","content":[{"type":"text","text":"{\"pending\":true}"}]}}),
    );
    event(
        &mut s,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    event(
        &mut s,
        "user/message",
        json!({"content":[],"source":{"kind":"user"}}),
    );
    assert!(kinds(&s).contains(&"reply_needed".into()));
    assert_eq!(s.pending["question:ask"].0, original);
    event(
        &mut s,
        "user/message",
        json!({"content":[],"source":{"kind":"user-question-reply","callId":"ask"}}),
    );
    assert_eq!(kinds(&s), ["completion"]);
}
#[test]
fn blocking_questions_clear_on_tool_result_and_subagents_are_excluded() {
    let mut s = state();
    event(&mut s, "turn/start", json!({"turn":1}));
    event(
        &mut s,
        "tool/call",
        json!({"turn":1,"name":"ask_user_question","callId":"ask","arguments":{"questions":[{"question":"Choose?"}]}}),
    );
    assert_eq!(kinds(&s), ["reply_needed"]);
    event(
        &mut s,
        "tool/result",
        json!({"message":{"toolCallId":"ask","content":[],"isError":true}}),
    );
    assert!(kinds(&s).is_empty());
    s.header.as_mut().unwrap()["origin"] = json!("subagent");
    assert!(s
        .snapshot("fixture", "project/fixture", "source")
        .unwrap()
        .is_none());
}
#[test]
fn fork_inherited_events_do_not_produce_notifications() {
    let mut s = State::default();
    s.consume(json!({"type":"session","version":4,"id":"fixture","isSeeded":true,"inheritedEventCount":2}).to_string().as_bytes()).unwrap();
    event(&mut s, "turn/start", json!({"turn":1}));
    event(
        &mut s,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    assert!(kinds(&s).is_empty());
    event(&mut s, "turn/start", json!({"turn":2}));
    event(
        &mut s,
        "turn/end",
        json!({"turn":2,"reason":{"kind":"completed"}}),
    );
    assert_eq!(kinds(&s), ["completion"]);
}
struct FileFixture(PathBuf);
impl FileFixture {
    fn new(extension: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "agenthub-dsh-notifications-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir.join(format!("session.v4.{extension}")))
    }
}
impl Drop for FileFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}
fn lines() -> Vec<Vec<u8>> {
    vec![
    json!({"type":"session","version":4,"id":"fixture","isSeeded":false}),
    json!({"type":"turn/start","seq":0,"time":1800000000000_i64,"data":{"turn":1}}),
    json!({"type":"turn/end","seq":1,"time":1800000001000_i64,"data":{"turn":1,"reason":{"kind":"completed"}}})
].into_iter().map(|v|format!("{v}\n").into_bytes()).collect()
}
#[test]
fn compressed_frames_incrementally_accept_only_complete_records_and_replacement_resets() {
    let f = FileFixture::new("jsonl.zstd");
    let lines = lines();
    let frames = lines
        .iter()
        .map(|s| zstd::stream::encode_all(s.as_slice(), 1).unwrap())
        .collect::<Vec<_>>();
    fs::write(&f.0, [frames[0].clone(), frames[1].clone()].concat()).unwrap();
    assert!(read(&f.0, "fixture", "p/fixture")
        .unwrap()
        .unwrap()
        .events
        .is_empty());
    let mut append = OpenOptions::new().append(true).open(&f.0).unwrap();
    let cut = frames[2].len() / 2;
    append.write_all(&frames[2][..cut]).unwrap();
    assert!(read(&f.0, "fixture", "p/fixture")
        .unwrap()
        .unwrap()
        .events
        .is_empty());
    append.write_all(&frames[2][cut..]).unwrap();
    let done = read(&f.0, "fixture", "p/fixture").unwrap().unwrap();
    assert_eq!(done.events.len(), 1);
    assert_eq!(
        read(&f.0, "fixture", "p/fixture").unwrap().unwrap().events,
        done.events
    );
    fs::write(&f.0, &frames[0]).unwrap();
    assert!(read(&f.0, "fixture", "p/fixture")
        .unwrap()
        .unwrap()
        .events
        .is_empty());
}
#[test]
fn plaintext_partial_tail_and_unknown_required_events_are_not_published() {
    let f = FileFixture::new("jsonl");
    let l = lines();
    fs::write(
        &f.0,
        [l[0].clone(), l[1].clone(), l[2][..l[2].len() - 1].to_vec()].concat(),
    )
    .unwrap();
    assert!(read(&f.0, "fixture", "p/fixture")
        .unwrap()
        .unwrap()
        .events
        .is_empty());
    OpenOptions::new()
        .append(true)
        .open(&f.0)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    assert_eq!(
        read(&f.0, "fixture", "p/fixture")
            .unwrap()
            .unwrap()
            .events
            .len(),
        1
    );
    let mut s = state();
    assert!(s
        .consume(br#"{"type":"future-required","seq":0,"time":1800000000000,"data":{}}"#)
        .is_err());
}

#[test]
fn targeted_scan_preserves_identity_and_cache_covers_more_than_64_sessions() {
    let root = std::env::temp_dir().join(format!("agenthub-dsh-cache-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let paths = DshPaths {
        home: root.as_path().to_path_buf(),
    };
    for i in 0..80 {
        let id = format!("fixture-{i}");
        let dir = root.as_path().join("sessions/project").join(&id);
        fs::create_dir_all(&dir).unwrap();
        let mut rows = lines();
        rows[0] = format!(
            "{}\n",
            json!({"type":"session","version":4,"id":id,"isSeeded":false,"cwd":"/workspace/fixture"})
        )
        .into_bytes();
        fs::write(dir.join("session.v4.jsonl"), rows.concat()).unwrap();
    }
    let full = scan(&paths).unwrap();
    assert_eq!(full.streams.len(), 80);
    {
        let cache = CACHE.lock().unwrap();
        assert_eq!(
            cache
                .entries
                .iter()
                .filter(|e| e.path.starts_with(root.as_path()))
                .count(),
            80
        );
        assert_eq!(
            cache.bytes,
            cache.entries.iter().map(|e| e.bytes).sum::<usize>()
        );
    }
    let request = crate::provider_sessions::ProviderListRequest {
        limit: Some(1),
        q: Some("fixture-0".into()),
        ..Default::default()
    };
    let (items, candidates) =
        crate::dsh::sessions::probe_sessions(&paths, &request, |_| true).unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].storage_size.is_none());
    assert_eq!(candidates.streams.len(), 1);
    assert!(
        crate::dsh::sessions::list_sessions(&paths, &request).unwrap()[0]
            .storage_size
            .is_some()
    );
    let unrelated = root
        .as_path()
        .join("sessions/project/fixture-1/session.v4.jsonl");
    fs::write(&unrelated, "broken\n").unwrap();
    let (_, empty) = crate::dsh::sessions::probe_sessions(&paths, &request, |_| false).unwrap();
    assert!(empty.streams.is_empty());
    assert_eq!(empty.errors, 0);
    let broken = scan_selected(&paths, Some("fixture-1")).unwrap();
    assert!(broken.streams.is_empty());
    assert_eq!(broken.errors, 1);
    let targeted = scan_selected(&paths, Some("fixture-0")).unwrap();
    assert_eq!(targeted.errors, 0);
    assert_eq!(targeted.streams.len(), 1);
    assert_eq!(
        targeted.streams[0].events,
        full.streams
            .iter()
            .find(|s| s.id == "fixture-0")
            .unwrap()
            .events
    );
    let duplicate = root.as_path().join("sessions/other/fixture-0");
    fs::create_dir_all(&duplicate).unwrap();
    fs::copy(
        root.as_path()
            .join("sessions/project/fixture-0/session.v4.jsonl"),
        duplicate.join("session.v4.jsonl"),
    )
    .unwrap();
    let ambiguous = scan_selected(&paths, Some("fixture-0")).unwrap();
    assert!(ambiguous.streams.is_empty());
    assert_eq!(ambiguous.errors, 1);
    fs::remove_dir_all(root).unwrap();
}
