use super::{message_blocks_from_events, rollout_hook_stop_message_selection};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(events: &[Value]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agenthub-rollout-accuracy-{}-{}.jsonl",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(
            &path,
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn start() -> Value {
    json!({"type":"turn_context","payload":{"turn_id":"root-turn"}})
}

fn assistant(text: &str, phase: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":phase,"content":[{"type":"output_text","text":text}],"internal_chat_message_metadata_passthrough":{"turn_id":"internal-model-turn"}}})
}

#[test]
fn rollout_accuracy_screenshot_1147_selects_final_despite_internal_turn_metadata() {
    let final_answer = "Reduced AGENTS.md from 75 lines / 7158 bytes to 41 lines / 4483 bytes.";
    let fixture = Fixture::new(&[
        start(),
        json!({"type":"event_msg","payload":{"type":"agent_message","phase":"commentary","message":"I will review and shorten AGENTS.md."}}),
        assistant("I will review and shorten AGENTS.md.", "commentary"),
        assistant(final_answer, "final_answer"),
    ]);
    let selection = rollout_hook_stop_message_selection(&fixture.0, Some("root-turn"))
        .unwrap()
        .unwrap();
    assert_eq!(selection.message, final_answer);
    assert_eq!(selection.selected_turn_id.as_deref(), Some("root-turn"));
    assert_eq!(selection.selected_line, Some(4));
}

#[test]
fn rollout_accuracy_screenshot_1136_never_uses_inter_agent_final_answer() {
    let internal = json!({"type":"response_item","payload":{"type":"agent_message","author":"/root/config_compatibility","recipient":"/root","content":[{"type":"input_text","text":"Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/config_compatibility\nPayload: internal audit"}]}});
    let fixture = Fixture::new(&[start(), internal.clone()]);
    assert!(
        rollout_hook_stop_message_selection(&fixture.0, Some("root-turn"))
            .unwrap()
            .is_none()
    );
    assert!(message_blocks_from_events([&internal]).is_empty());
}

#[test]
fn rollout_accuracy_does_not_promote_commentary_or_user_message_to_completion() {
    for event in [
        json!({"type":"event_msg","payload":{"type":"agent_message","phase":"commentary","message":"Work is in progress."}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"Please finish this work."}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"Unconfirmed legacy message."}]}}),
    ] {
        let fixture = Fixture::new(&[start(), event]);
        assert!(
            rollout_hook_stop_message_selection(&fixture.0, Some("root-turn"))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn rollout_accuracy_explicit_other_turn_final_does_not_leak() {
    let fixture = Fixture::new(&[
        start(),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","turn_id":"other-turn","phase":"final_answer","content":[{"text":"Another task result."}]}}),
    ]);
    assert!(
        rollout_hook_stop_message_selection(&fixture.0, Some("root-turn"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn rollout_accuracy_completed_body_is_authoritative_and_words_are_not_filters() {
    let final_answer = "The document mentions FINAL_ANSWER and memory consolidation.";
    let fixture = Fixture::new(&[
        start(),
        assistant("Work is in progress.", "commentary"),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"root-turn","last_agent_message":final_answer}}),
    ]);
    let selection = rollout_hook_stop_message_selection(&fixture.0, Some("root-turn"))
        .unwrap()
        .unwrap();
    assert_eq!(selection.message, final_answer);
    assert_eq!(selection.source, "task_complete.last_agent_message");
}

#[test]
fn user_images_survive_without_text_and_native_replays_are_not_duplicate_bubbles() {
    let first = json!({"type":"response_item","payload":{"id":"user-one","type":"message","role":"user","content":[{"type":"input_image","image_url":"https://example.com/image.png"}]}});
    let second = json!({"type":"response_item","payload":{"id":"user-two","type":"message","role":"user","content":[{"type":"input_image","image_url":"https://example.com/image.png"}]}});
    let blocks = message_blocks_from_events([&first, &first, &second]);
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        blocks[0].user_message.as_ref().unwrap().attachments.len(),
        1
    );
    assert_eq!(blocks[1].user_message.as_ref().unwrap().id, "user-two");
}

#[test]
fn user_plan_examples_are_never_promoted_to_assistant_plan_cards() {
    let value = json!({"type":"response_item","payload":{"id":"user","type":"message","role":"user","content":[{"type":"input_text","text":"<proposed_plan># literal</proposed_plan>"}]}});
    let blocks = message_blocks_from_events([&value]);
    assert_eq!(blocks[0].kind, "message");
    assert!(blocks[0]
        .user_message
        .as_ref()
        .unwrap()
        .text
        .contains("<proposed_plan>"));
}

#[test]
fn commentary_phase_is_preserved_for_process_folding_without_affecting_final_answers_or_plans() {
    let user = json!({"type":"response_item","turn_id":"turn-1","payload":{"id":"u-1","type":"message","role":"user","content":[{"type":"input_text","text":"Audit commentary folding."}]}});
    let commentary = json!({"type":"response_item","turn_id":"turn-1","payload":{"id":"a-mid","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"Checking workspace files first."}]}});
    let call = json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":{"cmd":"rg phase"}}});
    let output = json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"function_call_output","call_id":"call-1","output":"ok"}});
    let plan = json!({"type":"response_item","turn_id":"turn-1","payload":{"id":"a-plan","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"<proposed_plan># Plan\n1. Verify</proposed_plan>"}]}});
    let final_msg = json!({"type":"response_item","turn_id":"turn-1","payload":{"id":"a-final","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"All checks passed."}]}});
    let blocks =
        message_blocks_from_events([&user, &commentary, &call, &output, &plan, &final_msg]);
    assert_eq!(blocks[0].phase, None);
    assert_eq!(blocks[1].id, "a-mid");
    assert_eq!(blocks[1].phase.as_deref(), Some("commentary"));
    assert_eq!(blocks[2].call_id.as_deref(), Some("call-1"));
    assert_eq!(blocks[2].phase, None);
    assert_eq!(blocks[3].kind, "plan");
    assert_eq!(blocks[3].phase, None);
    assert_eq!(blocks[4].id, "a-final");
    assert_eq!(blocks[4].phase, None);
    let serialized = serde_json::to_value(&blocks[4]).unwrap();
    assert!(serialized.get("phase").is_none());
}
