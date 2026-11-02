use agenthub_core::{
    codex::{thread_detail_from_summary, ThreadStatus, ThreadSummary},
    user_message::{parse_user_message, UserAttachment, UserMessage},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN1cAAAAASUVORK5CYII=";
const ALLOWED_CONTEXT_KINDS: [&str; 4] = ["instructions", "environment", "runtime", "files"];

fn parse_text(id: &str, text: &str) -> UserMessage {
    parse_user_message(id, &Value::String(text.to_owned()))
}

fn context_pairs(message: &UserMessage) -> Vec<(String, String)> {
    message
        .context_blocks
        .iter()
        .map(|block| {
            let kind = serde_json::to_value(block.kind).unwrap();
            (
                kind.as_str()
                    .expect("context kind must serialize as a string")
                    .to_owned(),
                block.text.clone(),
            )
        })
        .collect()
}

fn context_kind_for(message: &UserMessage, text: &str) -> Option<String> {
    context_pairs(message)
        .into_iter()
        .find_map(|(kind, block)| (block == text).then_some(kind))
}

fn image() -> Value {
    json!({
        "type": "input_image",
        "image_url": format!("data:image/png;base64,{PNG}")
    })
}

fn native_images_at_same_path(path: &str, second_image_data: &str) -> UserMessage {
    let wrapper = format!(
        "# Files mentioned by the user:\n\n## shared.png: {path}\nImage attachment: true\n\n## shared.png: {path}\nImage attachment: true\n\n## My request:\nInspect both native references."
    );
    parse_user_message(
        "fixture-same-native-path",
        &json!([
            {"type":"input_text", "text":wrapper},
            {"type":"input_text", "text":format!("<image name=[Image #1] path=\"{path}\">")},
            image(),
            {"type":"input_text", "text":"</image>"},
            {"type":"input_text", "text":format!("<image name=[Image #2] path=\"{path}\">")},
            {"type":"input_image", "image_url":format!("data:image/png;base64,{second_image_data}")},
            {"type":"input_text", "text":"</image>"}
        ]),
    )
}

struct TempFixtureDir {
    path: PathBuf,
}

impl TempFixtureDir {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!(
            "agenthub-user-context-regression-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&path).expect("create isolated /tmp fixture directory");
        Self { path }
    }

    fn create_then_remove(&self, name: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, b"synthetic attachment placeholder").unwrap();
        fs::remove_file(&path).unwrap();
        path
    }
}

impl Drop for TempFixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn automatic_context_whitelist_preserves_verbatim_order_and_duplicates() {
    let repeated_environment =
        "<environment_context>\n  <cwd>/workspace/agenthub</cwd>\n</environment_context>";
    let repeated_additional =
        "<additional_context>\nSynthetic repeated context.\n</additional_context>";
    let wrappers = vec![
        repeated_environment.to_owned(),
        "<permissions instructions>\nSynthetic permission scope.\n</permissions instructions>"
            .to_owned(),
        "<app-context>\nSynthetic application runtime.\n</app-context>".to_owned(),
        "<collaboration_mode>\nSynthetic collaboration mode.\n</collaboration_mode>".to_owned(),
        "<skills_instructions>\nSynthetic skill instructions.\n</skills_instructions>".to_owned(),
        "<plugins_instructions>\nSynthetic plugin instructions.\n</plugins_instructions>".to_owned(),
        "<subagent_notification>\n{\"agent_id\":\"fake-agent\"}\n</subagent_notification>"
            .to_owned(),
        "<subagent_context>\n- /workspace/child: synthetic worker\n</subagent_context>"
            .to_owned(),
        "<codex_internal_context source=\"fixture\" owner=\"fake-agent\">\nSynthetic internal context.\n</codex_internal_context>"
            .to_owned(),
        "<goal_context>\nSynthetic goal context.\n</goal_context>".to_owned(),
        repeated_additional.to_owned(),
        repeated_additional.to_owned(),
        "<user_instructions>\nSynthetic user instructions.\n</user_instructions>".to_owned(),
        "<legacy_unified_exec_process_limit_warning>\nSynthetic warning.\n</legacy_unified_exec_process_limit_warning>"
            .to_owned(),
        "<legacy_apply_patch_exec_command_warning>\nSynthetic warning.\n</legacy_apply_patch_exec_command_warning>"
            .to_owned(),
        "<legacy_model_mismatch_warning>\nSynthetic warning.\n</legacy_model_mismatch_warning>"
            .to_owned(),
        repeated_environment.to_owned(),
    ];
    let prefix = "Keep this user-authored prefix.\n";
    let suffix = " Keep this user-authored suffix.";
    let input = format!("{prefix}{}{suffix}", wrappers.concat());

    let message = parse_text("fixture-context-whitelist", &input);

    assert_eq!(message.text, format!("{prefix}{suffix}"));
    let contexts = context_pairs(&message);
    assert_eq!(
        contexts
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<_>>(),
        wrappers
    );
    assert!(contexts
        .iter()
        .all(|(kind, _)| ALLOWED_CONTEXT_KINDS.contains(&kind.as_str())));
    assert_eq!(
        context_kind_for(&message, repeated_environment),
        Some("environment".into())
    );
    assert_eq!(
        context_kind_for(
            &message,
            "<permissions instructions>\nSynthetic permission scope.\n</permissions instructions>"
        ),
        Some("runtime".into())
    );
    assert_eq!(
        contexts
            .iter()
            .filter(|(_, text)| text == repeated_environment)
            .count(),
        2,
        "repeated environment blocks must not be deduplicated"
    );
    assert_eq!(
        contexts
            .iter()
            .filter(|(_, text)| text == repeated_additional)
            .count(),
        2,
        "repeated additional-context blocks must not be deduplicated"
    );
}

#[test]
fn equivalent_sanitized_context_shapes_keep_the_same_user_body() {
    let body = "Please inspect the synthetic fixture.";
    let environment =
        "<environment_context>\n  <cwd>/workspace/project</cwd>\n</environment_context>";
    let permissions =
        "<permissions instructions>\nOnly use synthetic fixture data.\n</permissions instructions>";
    let agents = "# AGENTS.md instructions\n\n<INSTRUCTIONS>\nUse synthetic fixture data only.\n</INSTRUCTIONS>";

    // Equivalent to a prompt with separate runtime and permissions wrappers.
    let first = parse_text(
        "fixture-layout-one",
        &format!("{environment}{permissions}{body}"),
    );
    // Equivalent to the complete AGENTS.md wrapper followed immediately by
    // environment context on the same line as the closing INSTRUCTIONS tag.
    let second = parse_text(
        "fixture-layout-two",
        &format!("{agents}{environment}{body}"),
    );

    assert_eq!(first.text, body);
    assert_eq!(second.text, body);
    assert_eq!(
        context_kind_for(&first, environment),
        Some("environment".into())
    );
    assert_eq!(
        context_kind_for(&second, environment),
        Some("environment".into())
    );
    assert_eq!(
        context_kind_for(&first, permissions),
        Some("runtime".into())
    );
    assert!(context_pairs(&second)
        .iter()
        .any(|(kind, text)| kind == "instructions" && text.contains(agents)));
    assert!(!first.text.contains("environment_context"));
    assert!(!second.text.contains("AGENTS.md"));
}

#[test]
fn user_text_with_code_quotes_native_answers_or_incomplete_tags_is_retained() {
    let input = concat!(
        "Question: Should the literal example remain?\n",
        "Answer: Yes, keep this native user response.\n",
        "<user_shell_command>pwd</user_shell_command>\n",
        "<turn_aborted>synthetic interrupted turn</turn_aborted>\n",
        "Explain <environment_context>x</environment_context> literally.\n",
        "Inline example: `<environment_context>literal</environment_context>`.\n",
        "```xml\n",
        "<skills_instructions>literal code sample</skills_instructions>\n",
        "```\n",
        "> <additional_context>quoted user text</additional_context>\n",
        "    <app-context>indented example</app-context>\n",
        "<environment_context>incomplete wrapper"
    );
    let content = json!([
        {"type": "input_text", "text": "Question: Should the literal example remain?\n"},
        {"type": "input_text", "text": "Answer: Yes, keep this native user response.\n<user_shell_command>pwd</user_shell_command>\n<turn_aborted>synthetic interrupted turn</turn_aborted>\nExplain <environment_context>x</environment_context> literally.\nInline example: `<environment_context>literal</environment_context>`.\n```xml\n<skills_instructions>literal code sample</skills_instructions>\n```\n> <additional_context>quoted user text</additional_context>\n    <app-context>indented example</app-context>\n<environment_context>incomplete wrapper"}
    ]);

    let message = parse_user_message("fixture-literals", &content);

    assert_eq!(message.text, input);
    assert!(message.context_blocks.is_empty());
    assert!(message.attachments.is_empty());
}

#[test]
fn old_and_new_file_envelopes_keep_multiple_attachments_and_embedded_images() {
    let temp = TempFixtureDir::new();
    let old_one = temp.create_then_remove("old-one.png");
    let old_two = temp.create_then_remove("old-two.png");
    let old_note = temp.create_then_remove("old-note.txt");
    let old_wrapper = format!(
        "# Files mentioned by the user:\n\n## old-one.png: {}\n## old-two.png: {}\n## old-note.txt: {}\n\n## My request for Codex:\nReview the old attachments.",
        old_one.display(),
        old_two.display(),
        old_note.display()
    );
    let old_content = json!([
        {"type": "input_text", "text": old_wrapper},
        {"type": "input_text", "text": format!("<image name=[Image #1] path=\"{}\">", old_one.display())},
        image(),
        {"type": "input_text", "text": "</image>"},
        {"type": "input_text", "text": format!("<image name=[Image #2] path=\"{}\">", old_two.display())},
        image(),
        {"type": "input_text", "text": "</image>"}
    ]);

    let old_message = parse_user_message("fixture-old-files", &old_content);

    assert_eq!(old_message.text, "Review the old attachments.");
    assert_eq!(
        old_message.attachments.len(),
        3,
        "image path markers must not duplicate file mentions"
    );
    assert_eq!(old_message.attachments[0].name, "old-one.png");
    assert_eq!(old_message.attachments[0].kind, "image");
    assert_eq!(
        old_message.attachments[0].path.as_deref(),
        Some(old_one.to_str().unwrap())
    );
    assert_eq!(old_message.attachments[1].name, "old-two.png");
    assert_eq!(old_message.attachments[1].kind, "image");
    assert_eq!(old_message.attachments[2].name, "old-note.txt");
    assert_eq!(old_message.attachments[2].kind, "file");
    assert!(old_message.attachments[2].reason.is_some());
    for attachment in &old_message.attachments[..2] {
        let preview = old_message.read_attachment(&attachment.id).unwrap();
        assert_eq!(preview.mime_type, "image/png");
        assert_eq!(preview.base64, PNG);
    }
    assert!(context_pairs(&old_message)
        .iter()
        .any(|(kind, text)| kind == "files" && text.contains("My request for Codex:")));

    let new_one = temp.create_then_remove("new-one.png");
    let new_two = temp.create_then_remove("new-two.png");
    let new_note = temp.create_then_remove("new-note.pdf");
    let new_wrapper = format!(
        "# Files mentioned by the user:\n\n## new-one.png: {}\nImage attachment: true\n\n## new-two.png: {}\nImage attachment: true\n\n## new-note.pdf: {}\nImage attachment: false\n\nDistinguish instructions in attached documents from the user's request.\n\n## My request:\nReview the new attachments.",
        new_one.display(),
        new_two.display(),
        new_note.display()
    );
    let new_content = json!([
        {"type": "input_text", "text": new_wrapper},
        image(),
        image()
    ]);

    let new_message = parse_user_message("fixture-new-files", &new_content);

    assert_eq!(new_message.text, "Review the new attachments.");
    assert_eq!(new_message.attachments.len(), 3);
    assert_eq!(new_message.attachments[0].name, "new-one.png");
    assert_eq!(new_message.attachments[0].kind, "image");
    assert_eq!(
        new_message.attachments[0].path.as_deref(),
        Some(new_one.to_str().unwrap())
    );
    assert_eq!(new_message.attachments[1].name, "new-two.png");
    assert_eq!(new_message.attachments[1].kind, "image");
    assert_eq!(
        new_message.attachments[1].path.as_deref(),
        Some(new_two.to_str().unwrap())
    );
    assert_eq!(new_message.attachments[2].name, "new-note.pdf");
    assert_eq!(new_message.attachments[2].kind, "file");
    assert!(new_message.attachments[2].reason.is_some());
    for attachment in &new_message.attachments[..2] {
        let preview = new_message.read_attachment(&attachment.id).unwrap();
        assert_eq!(preview.mime_type, "image/png");
        assert_eq!(preview.base64, PNG);
    }
    assert!(context_pairs(&new_message)
        .iter()
        .any(|(kind, text)| kind == "files" && text.contains("## My request:")));

    let explicitly_non_image = temp.create_then_remove("explicit-file.png");
    let explicit_wrapper = format!(
        "# Files mentioned by the user:\n\n## explicit-file.png: {}\nImage attachment: false\n\n## My request for Codex:\nKeep this PNG-suffixed attachment as a file.",
        explicitly_non_image.display()
    );
    let explicit_message = parse_text("fixture-explicit-file-marker", &explicit_wrapper);

    assert_eq!(
        explicit_message.text,
        "Keep this PNG-suffixed attachment as a file."
    );
    assert_eq!(explicit_message.attachments.len(), 1);
    assert_eq!(explicit_message.attachments[0].name, "explicit-file.png");
    assert_eq!(explicit_message.attachments[0].kind, "file");
    assert_eq!(
        explicit_message.attachments[0].path.as_deref(),
        explicitly_non_image.to_str()
    );
}

#[test]
fn context_blocks_round_trip_and_old_client_wire_shapes_remain_compatible() {
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OldClientMessage {
        id: String,
        text: String,
        attachments: Vec<UserAttachment>,
    }

    let context = "<environment_context>\n  <cwd>/workspace/project</cwd>\n</environment_context>";
    let message = parse_text("fixture-wire-format", &format!("{context}Visible request."));
    let wire = serde_json::to_value(&message).unwrap();

    assert_eq!(wire["contextBlocks"][0]["kind"], "environment");
    assert_eq!(wire["contextBlocks"][0]["text"], context);
    let old_client: OldClientMessage = serde_json::from_value(wire).unwrap();
    assert_eq!(old_client.id, "fixture-wire-format");
    assert_eq!(old_client.text, "Visible request.");
    assert!(old_client.attachments.is_empty());

    let legacy_wire = json!({
        "id": "fixture-legacy-wire",
        "text": "Visible legacy request.",
        "attachments": []
    });
    let current_client: UserMessage = serde_json::from_value(legacy_wire).unwrap();
    assert_eq!(current_client.text, "Visible legacy request.");
    assert!(current_client.context_blocks.is_empty());
}

#[test]
fn user_message_cache_weight_includes_context_bytes() {
    let body = "same visible request";
    let without_context = parse_text("fixture-weight-no-context", body);
    let context = "<environment_context>\n  <cwd>/workspace/project</cwd>\n</environment_context>";
    let with_context = parse_text("fixture-weight-with-context", &format!("{context}{body}"));

    assert_eq!(with_context.text, without_context.text);
    assert!(
        with_context.weight() >= without_context.weight() + context.len() as u64,
        "the context payload must contribute to the cached message weight"
    );
}

#[test]
fn public_codex_reader_keeps_context_only_blocks_and_summarizes_clean_user_text() {
    let temp = TempFixtureDir::new();
    let rollout = temp.path.join("rollout-context-fixture.jsonl");
    let context_only =
        "<environment_context>\n  <cwd>/workspace/project</cwd>\n</environment_context>";
    let mixed_context =
        "<permissions instructions>\nSynthetic read-only fixture.\n</permissions instructions>";
    let user_body = "Please summarize the synthetic fixture.";
    let assistant_body = "The fixture contains one visible request.";
    let events = [
        json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": context_only}]
            }
        }),
        json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": format!("{mixed_context}{user_body}")}]
            }
        }),
        json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": assistant_body}]
            }
        }),
    ];
    let jsonl = events
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&rollout, format!("{jsonl}\n")).unwrap();
    let summary = ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: format!("fixture-thread-{}", uuid::Uuid::new_v4()),
        title: "Synthetic context fixture".into(),
        status: ThreadStatus::Recent,
        updated_at: None,
        archived_at: None,
        message_count: events.len(),
        latest_message: None,
        cwd: Some("/workspace/project".into()),
        model: None,
        rollout_path: Some(rollout),
        active_turn_id: None,
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: None,
    };

    let detail = thread_detail_from_summary(summary).unwrap();

    let context_only_block = detail
        .blocks
        .iter()
        .find(|block| {
            block.role == "user"
                && block
                    .user_message
                    .as_ref()
                    .is_some_and(|m| m.text.is_empty())
        })
        .expect("context-only user block must remain in the public Codex history");
    assert!(!context_only_block
        .user_message
        .as_ref()
        .unwrap()
        .context_blocks
        .is_empty());
    assert_eq!(context_only_block.text.as_deref(), Some(""));

    let visible_user_block = detail
        .blocks
        .iter()
        .find(|block| block.role == "user" && block.text.as_deref() == Some(user_body))
        .expect("mixed context must retain the real user-authored body");
    assert!(visible_user_block
        .user_message
        .as_ref()
        .is_some_and(|message| !message.context_blocks.is_empty()));

    let user_summaries = detail
        .messages
        .iter()
        .filter(|message| message.role == "user")
        .collect::<Vec<_>>();
    assert_eq!(user_summaries.len(), 1);
    assert_eq!(user_summaries[0].text, user_body);
    assert!(detail
        .messages
        .iter()
        .all(|message| !message.text.contains("<environment_context>")
            && !message.text.contains("<permissions instructions>")));
    assert_eq!(detail.summary.title, "Synthetic context fixture");
}

#[test]
fn code_and_quote_close_tags_do_not_truncate_complete_agents_context() {
    let agents = concat!(
        "# AGENTS.md instructions\n",
        "\n",
        "<INSTRUCTIONS>\n",
        "Preserve closing tags shown in code:\n",
        "```xml\n",
        "</INSTRUCTIONS>\n",
        "```\n",
        "Preserve closing tags shown as a quote:\n",
        "> </INSTRUCTIONS>\n",
        "These lines remain inside the complete instruction block.\n",
        "</INSTRUCTIONS>"
    );
    let message = parse_text(
        "fixture-agents-closing-tag-literals",
        &format!("{agents}\nVisible request."),
    );

    assert_eq!(message.text, "\nVisible request.");
    let contexts = context_pairs(&message);
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0].0, "instructions");
    assert_eq!(contexts[0].1, agents);
    assert_eq!(contexts[0].1.matches("</INSTRUCTIONS>").count(), 3);
}

#[test]
fn known_context_tags_inside_ordinary_xml_parent_remain_user_text() {
    let input = concat!(
        "<document>\n",
        "<environment_context>literal user XML</environment_context>\n",
        "<section><user_instructions>also literal</user_instructions></section>\n",
        "</document>\n",
        "Visible request."
    );

    let message = parse_text("fixture-ordinary-xml-parent", input);

    assert_eq!(message.text, input);
    assert!(message.context_blocks.is_empty());
}

#[test]
fn context_like_tags_inside_complete_or_unfinished_xml_comments_remain_user_text() {
    for (id, input) in [
        (
            "fixture-complete-xml-comment",
            "<!--\n<environment_context>comment literal</environment_context>\n-->\nVisible request.",
        ),
        (
            "fixture-unfinished-xml-comment",
            "<!--\n<environment_context>comment literal</environment_context>\nVisible request.",
        ),
    ] {
        let message = parse_text(id, input);

        assert_eq!(message.text, input);
        assert!(message.context_blocks.is_empty());
    }
}

#[test]
fn incomplete_files_wrapper_does_not_extract_environment_context_from_request() {
    let input = concat!(
        "# Files mentioned by the user:\n\n",
        "## readme.md: /fixture/readme.md\n\n",
        "## My request:\n",
        "<environment_context>literal request content</environment_context>"
    );

    let message = parse_text("fixture-incomplete-files-wrapper", input);

    assert_eq!(message.text, input);
    assert!(message.context_blocks.is_empty());
    assert!(message.attachments.is_empty());
}

#[test]
fn partial_environment_opening_tag_does_not_extract_a_complete_child_tag() {
    let input = "<environment_context\n<app-context>x</app-context>";

    let message = parse_text("fixture-partial-environment-opening", input);

    assert_eq!(message.text, input);
    assert!(message.context_blocks.is_empty());
}

#[test]
fn long_single_line_with_ordinary_xml_keeps_nested_context_like_text() {
    let input = format!(
        "<document>{}<environment_context>long-line literal</environment_context></document>",
        "ordinary-xml-content-".repeat(4096)
    );

    let message = parse_text("fixture-long-single-line-xml", &input);

    assert_eq!(message.text, input);
    assert!(message.context_blocks.is_empty());
}

#[test]
fn same_native_path_deduplicates_identical_embedded_content() {
    let temp = TempFixtureDir::new();
    let path = temp.create_then_remove("shared.png");
    let message = native_images_at_same_path(path.to_str().unwrap(), PNG);

    assert_eq!(message.attachments.len(), 1);
    assert_eq!(message.attachments[0].name, "shared.png");
    assert_eq!(message.attachments[0].path.as_deref(), path.to_str());
    assert_eq!(
        message
            .read_attachment(&message.attachments[0].id)
            .unwrap()
            .base64,
        PNG
    );
}

#[test]
fn same_native_path_keeps_distinct_embedded_content() {
    const OTHER_PNG: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j8ioAAAAASUVORK5CYII=";
    let temp = TempFixtureDir::new();
    let path = temp.create_then_remove("shared.png");
    let message = native_images_at_same_path(path.to_str().unwrap(), OTHER_PNG);

    assert_eq!(message.attachments.len(), 2);
    assert_eq!(message.attachments[0].path.as_deref(), path.to_str());
    assert_eq!(message.attachments[1].path.as_deref(), path.to_str());
    assert_ne!(message.attachments[0].id, message.attachments[1].id);
    assert_eq!(
        message
            .read_attachment(&message.attachments[0].id)
            .unwrap()
            .base64,
        PNG
    );
    assert_eq!(
        message
            .read_attachment(&message.attachments[1].id)
            .unwrap()
            .base64,
        OTHER_PNG
    );
}
