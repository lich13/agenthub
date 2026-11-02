use super::*;
use serde_json::json;

fn question(call: &str) -> Value {
    json!({"type":"response_item","timestamp":"2026-01-01T00:00:00Z","turn_id":"turn-a","payload":{"type":"function_call","name":"request_user_input_async","call_id":call,"arguments":json!({"questions":[{"title":"请确认结果。","options":["通过","失败"]},{"title":"请选择保留项。"}]}).to_string()}})
}

fn answer(call: &str, index: usize) -> Value {
    let text = format!(
        "<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",
        json!([{"questionItemId":json!(["request_user_input_async",call,index]).to_string(),"question":"请确认结果。","answer":"通过"}])
    );
    json!({"type":"response_item","turn_id":"turn-a","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}})
}

#[test]
fn normalizes_both_native_shapes_and_rejects_bad_questions() {
    let async_args = question_arguments(&question("call-a")).unwrap();
    let parsed = normalize_user_input_questions(&async_args).unwrap();
    assert_eq!(parsed[0].question, "请确认结果。");
    assert_eq!(parsed[0].options[1].label, "失败");
    assert_eq!(parsed[1].id, "q2");
    let sync = normalize_user_input_questions(&json!({"questions":[{"id":"mode","question":"Choose?","header":"Mode","options":[{"label":"One","description":"First"}]}]})).unwrap();
    assert_eq!(sync[0].id, "mode");
    assert_eq!(sync[0].options[0].description.as_deref(), Some("First"));
    assert!(normalize_user_input_questions(&json!({"questions":[{"title":"  "}]})).is_none());
    assert!(normalize_user_input_questions(
        &json!({"questions":[{"title":"Valid","options":[42]}]})
    )
    .is_none());
}

#[test]
fn acceptance_progress_and_completion_do_not_answer_async_questions() {
    let mut tracker = AsyncQuestionTracker::default();
    tracker.push(&question("call-a"), 1);
    for event in [
        json!({"payload":{"type":"function_call_output","call_id":"call-a","output":"{\"accepted\":true}"}}),
        json!({"payload":{"type":"function_call","name":"exec","call_id":"work-a"}}),
        json!({"payload":{"type":"function_call_output","call_id":"work-a","output":"done"}}),
        json!({"payload":{"type":"message","role":"assistant","content":[{"text":"继续检查其他内容。"}]}}),
        json!({"payload":{"type":"task_complete","turn_id":"turn-a","status":"completed"}}),
    ] {
        tracker.push(&event, 2);
    }
    assert_eq!(tracker.pending().count(), 1);
    assert_eq!(tracker.calls[0].pending().unwrap().questions.len(), 2);
}

#[test]
fn answers_match_original_call_and_index_without_clearing_other_calls() {
    let mut tracker = AsyncQuestionTracker::default();
    tracker.push(&question("call-a"), 1);
    tracker.push(&question("call-b"), 2);
    let hash = tracker.calls[0].content_hash.clone();
    tracker.push(&answer("call-a", 0), 3);
    tracker.push(&answer("call-a", 0), 4);
    tracker.push(&question("call-a"), 5);
    assert_eq!(tracker.calls.len(), 2);
    assert_eq!(tracker.calls[0].pending().unwrap().questions[0].id, "q2");
    assert_eq!(tracker.calls[1].pending().unwrap().questions.len(), 2);
    assert_eq!(tracker.calls[0].content_hash, hash);
    tracker.push(&answer("call-a", 1), 6);
    assert!(tracker.calls[0].pending().is_none());
    assert!(tracker.calls[1].pending().is_some());
}

#[test]
fn literals_invalid_envelopes_and_wrong_identities_do_not_resolve() {
    let text = answer("call-a", 0)["payload"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    for candidate in [
        format!("```xml\n{text}\n```"),
        format!("`{text}`"),
        format!("> {text}"),
        format!("Example:\n{text}"),
        text.replace("</send_user_message_question_reply>", ""),
        text.replace("[", "broken["),
    ] {
        assert!(native_question_answers(&candidate).is_none());
    }
    let mut tracker = AsyncQuestionTracker::default();
    tracker.push(&question("call-a"), 1);
    tracker.push(&answer("other-call", 0), 2);
    tracker.push(&answer("call-a", 99), 3);
    let mut wrong_turn = answer("call-a", 0);
    wrong_turn["turn_id"] = json!("other-turn");
    tracker.push(&wrong_turn, 4);
    assert_eq!(tracker.calls[0].pending().unwrap().questions.len(), 2);
}

#[test]
fn cancellation_and_replacement_end_only_relevant_pending_work() {
    for event in [
        json!({"payload":{"type":"turn_aborted","turn_id":"turn-a"}}),
        json!({"payload":{"type":"task_started","turn_id":"turn-b"}}),
        json!({"turn_id":"turn-a","payload":{"type":"function_call_output","call_id":"call-a","output":"{\"accepted\":false}"}}),
    ] {
        let mut tracker = AsyncQuestionTracker::default();
        tracker.push(&question("call-a"), 1);
        tracker.push(&event, 2);
        assert_eq!(tracker.pending().count(), 0);
    }
}

#[test]
fn reader_rechecks_answers_and_refuses_incomplete_or_replaced_sources() {
    let path = std::env::temp_dir().join(format!("agenthub-question-{}", uuid::Uuid::new_v4()));
    let q = question("call-a");
    std::fs::write(&path, format!("{q}\n")).unwrap();
    assert_eq!(
        rollout_async_questions(&path).unwrap()[0]
            .pending()
            .unwrap()
            .questions
            .len(),
        2
    );
    std::fs::write(&path, format!("{q}\n{}\n", answer("call-a", 0))).unwrap();
    assert_eq!(
        rollout_async_questions(&path).unwrap()[0]
            .pending()
            .unwrap()
            .questions
            .len(),
        1
    );
    let unfinished = answer("call-a", 1).to_string();
    std::fs::write(
        &path,
        format!("{q}\n{}", &unfinished[..unfinished.len() - 1]),
    )
    .unwrap();
    assert!(rollout_async_questions(&path).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, "{}\n").unwrap();
    assert!(rollout_async_questions(&path).unwrap().is_empty());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn reader_and_message_timeline_agree_after_ack_and_partial_answer() {
    let events = vec![
        json!({"payload":{"type":"task_started","turn_id":"turn-a"}}),
        question("call-a"),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-a","output":"{\"accepted\":true}"}}),
        answer("call-a", 0),
    ];
    let blocks = crate::codex::message_blocks_from_events(&events);
    let question = blocks
        .iter()
        .find(|b| b.call_id.as_deref() == Some("call-a"))
        .unwrap();
    assert_eq!(question.resolved, Some(false));
    assert_eq!(question.questions.len(), 1);
    assert_eq!(question.questions[0].id, "q2");
    assert!(blocks.iter().any(|b| b.role == "user"));
}
