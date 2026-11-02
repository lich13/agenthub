use super::super::{
    references::{NativeReference, ReferenceSpan},
    DesktopError,
};
use super::MessageInput;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::{fs, path::PathBuf};

const CLIENT_ID: &str = "55555555-5555-4555-8555-555555555555";
const SOURCE_DERIVED_FIXTURE: &str =
    include_str!("../../../tests/fixtures/desktop-native-input/source-derived.json");
const MENTION_LINK_SOURCE_FIXTURE: &str =
    include_str!("../../../tests/fixtures/desktop-native-input/mention-links-source-derived.json");

struct SyntheticFiles(PathBuf);

impl SyntheticFiles {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-desktop-native-input-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("synthetic input fixture directory is created");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("synthetic input fixture directory is private");
        Self(root)
    }

    fn write(&self, filename: &str, contents: &str) -> PathBuf {
        let path = self.0.join(filename);
        fs::write(&path, contents).expect("synthetic input fixture file is written");
        path
    }
}

impl Drop for SyntheticFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source_fixture() -> Value {
    serde_json::from_str(SOURCE_DERIVED_FIXTURE)
        .expect("source-derived synthetic fixture is valid JSON")
}

fn mention_link_source_fixture() -> Value {
    serde_json::from_str(MENTION_LINK_SOURCE_FIXTURE)
        .expect("mention-link source-derived synthetic fixture is valid JSON")
}

fn fixture_case(name: &str) -> Value {
    source_fixture()["cases"][name].clone()
}

fn mention_link_fixture_case(name: &str) -> Value {
    mention_link_source_fixture()["cases"][name].clone()
}

fn reference_from_fixture(value: &Value, files: &SyntheticFiles) -> NativeReference {
    match value["kind"]
        .as_str()
        .expect("fixture reference kind exists")
    {
        "file" => NativeReference::selected_file(
            files.write(
                value["filename"]
                    .as_str()
                    .expect("fixture file name exists"),
                value["contents"]
                    .as_str()
                    .expect("fixture file contents exist"),
            ),
        )
        .expect("synthetic fixture file is selected"),
        "skill" => NativeReference::skill(
            value["name"]
                .as_str()
                .expect("fixture skill name exists")
                .into(),
            files.write(
                value["filename"]
                    .as_str()
                    .expect("fixture skill file name exists"),
                value["contents"]
                    .as_str()
                    .expect("fixture skill contents exist"),
            ),
        )
        .expect("synthetic fixture skill is accepted")
        .capture()
        .expect("synthetic fixture skill identity is captured"),
        "app" => NativeReference::app(
            value["name"]
                .as_str()
                .expect("fixture app name exists")
                .into(),
            value["uri"]
                .as_str()
                .expect("fixture app URI exists")
                .into(),
        )
        .expect("synthetic fixture app is accepted"),
        "plugin" => NativeReference::plugin(
            value["name"]
                .as_str()
                .expect("fixture plugin name exists")
                .into(),
            value["marketplace"]
                .as_str()
                .expect("fixture marketplace exists"),
        )
        .expect("synthetic fixture plugin is accepted"),
        other => panic!("unsupported synthetic fixture reference kind: {other}"),
    }
}

fn message_from_fixture(case: &Value, files: &SyntheticFiles) -> MessageInput {
    message_from_fixture_with_mode(case, files, false)
}

fn message_from_fixture_with_mode(
    case: &Value,
    files: &SyntheticFiles,
    native_mentions: bool,
) -> MessageInput {
    let references = case["references"]
        .as_array()
        .expect("fixture references are an array")
        .iter()
        .map(|reference| reference_from_fixture(reference, files))
        .collect();
    let reference_spans = serde_json::from_value(case["spans"].clone())
        .expect("fixture spans use the strict ReferenceSpan schema");
    MessageInput {
        native_mentions,
        client_id: CLIENT_ID.into(),
        text: case["text"]
            .as_str()
            .expect("fixture request text exists")
            .into(),
        references,
        reference_spans,
    }
}

fn assert_native_mention_case(case: &Value, files: &SyntheticFiles) -> (MessageInput, Value) {
    let message = message_from_fixture_with_mode(case, files, true);
    assert!(message.validate().is_ok());
    let input = message.input();
    let skill_path = input
        .as_array()
        .expect("native input is an array")
        .iter()
        .find(|element| element["type"] == "skill")
        .and_then(|element| element["path"].as_str())
        .unwrap_or("");
    let file_attachments = message.attachments();
    let file_path = file_attachments
        .first()
        .and_then(|attachment| attachment["path"].as_str())
        .unwrap_or("");
    let expected_prompt = case["expectedPrompt"]
        .as_str()
        .expect("fixture expected prompt exists")
        .replace("<skill-path>", skill_path)
        .replace("<file-path>", file_path);

    assert_eq!(message.prompt(), expected_prompt);
    assert_eq!(input[0]["text"], expected_prompt);
    assert_eq!(input[0]["text_elements"], json!([]));

    let expected_placeholders: Vec<_> = case["expectedPlaceholdersInTextOrder"]
        .as_array()
        .expect("fixture expected placeholders are an array")
        .iter()
        .map(|value| value.as_str().expect("fixture placeholder is a string"))
        .collect();
    let mut spans: Vec<_> = message.reference_spans.iter().collect();
    spans.sort_unstable_by_key(|span| span.start);
    assert_eq!(spans.len(), expected_placeholders.len());
    for (span, placeholder) in spans.iter().zip(expected_placeholders) {
        assert_eq!(
            message.text.get(span.start..span.end),
            Some(placeholder),
            "source-derived span must address the exact UTF-8 placeholder"
        );
        assert_eq!(
            message.references[span.reference].placeholder().as_deref(),
            Some(placeholder)
        );
    }

    (message, input)
}

fn assert_text_elements(message: &MessageInput, case: &Value, prefix_offset: usize) -> Value {
    let input = message.input();
    assert_eq!(input[0]["type"], "text");
    assert_eq!(input[0]["text"], message.prompt());

    let expected_placeholders: Vec<_> = case["expectedPlaceholdersInTextOrder"]
        .as_array()
        .expect("fixture expected placeholders are an array")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("fixture placeholder is a string")
                .to_owned()
        })
        .collect();
    let mut spans: Vec<_> = message.reference_spans.iter().collect();
    spans.sort_unstable_by_key(|span| span.start);
    assert_eq!(spans.len(), expected_placeholders.len());

    let expected_elements: Vec<_> = spans
        .iter()
        .zip(expected_placeholders.iter())
        .map(|(span, placeholder)| {
            assert_eq!(
                message.references[span.reference].placeholder().as_deref(),
                Some(placeholder.as_str())
            );
            assert_eq!(
                message.text.get(span.start..span.end),
                Some(placeholder.as_str())
            );
            json!({
                "byteRange": {
                    "start": prefix_offset + span.start,
                    "end": prefix_offset + span.end
                },
                "placeholder": placeholder
            })
        })
        .collect();
    assert_eq!(input[0]["text_elements"], json!(expected_elements));
    input
}

fn assert_invalid(message: &MessageInput) {
    assert!(matches!(
        message.validate(),
        Err(DesktopError::InvalidInput)
    ));
}

fn app_reference(name: &str, identity: &str) -> NativeReference {
    NativeReference::app(name.into(), format!("app://fixture-{identity}"))
        .expect("synthetic app reference is accepted")
}

#[test]
fn source_derived_fixture_is_explicitly_synthetic_and_contains_no_native_snapshot() {
    let fixture = source_fixture();
    assert_eq!(fixture["fixtureKind"], "syntheticSourceDerived");
    assert!(fixture["notice"]
        .as_str()
        .is_some_and(|notice| notice.contains("not a native snapshot")));
    assert_eq!(fixture["sourceEvidence"]["nativeBuild"], 20586);
    assert_eq!(
        fixture["sourceEvidence"]["rangeContract"],
        json!({"unit":"UTF-8 bytes","endExclusive":true})
    );
}

#[test]
fn mention_link_fixture_is_explicitly_synthetic_and_tracks_build_20586_sources() {
    let fixture = mention_link_source_fixture();
    assert_eq!(fixture["fixtureKind"], "syntheticSourceDerived");
    assert!(fixture["notice"]
        .as_str()
        .is_some_and(|notice| { notice.contains("not a native snapshot or live execution") }));
    assert_eq!(fixture["sourceEvidence"]["nativeBuild"], 20586);
    assert!(fixture["sourceEvidence"]["composer"]
        .as_str()
        .is_some_and(|evidence| evidence.contains("dG/uG and getPersistedText")));
    assert!(fixture["sourceEvidence"]["start"]
        .as_str()
        .is_some_and(|evidence| evidence.contains("operation.context.attachments")));
}

#[test]
fn native_mentions_project_repeated_chinese_emoji_spans_to_markdown_links() {
    let case = mention_link_fixture_case("multiReferenceUtf8");
    let files = SyntheticFiles::new();
    let (message, input) = assert_native_mention_case(&case, &files);

    assert!(message.native_mentions);
    assert_eq!(message.text, case["text"]);
    assert_eq!(input.as_array().map(Vec::len), Some(5));
    assert_eq!(input[1]["type"], "skill");
    assert_eq!(input[1]["name"], "outline");
    assert!(input[1]["path"]
        .as_str()
        .is_some_and(|path| path.ends_with("fixture-outline.md")));
    assert_eq!(
        input[2],
        json!({"type":"mention","name":"search","path":"app://fixture-search-a"})
    );
    assert_eq!(
        input[3],
        json!({"type":"mention","name":"search","path":"app://fixture-search-b"})
    );
    assert_eq!(
        input[4],
        json!({"type":"mention","name":"review","path":"plugin://review@fixture-market"})
    );
}

#[test]
fn native_mention_markdown_escapes_label_and_destination_delimiters() {
    let case = mention_link_fixture_case("markdownEscaping");
    let files = SyntheticFiles::new();
    let (message, input) = assert_native_mention_case(&case, &files);

    assert_eq!(message.references.len(), 1);
    assert_eq!(input[1]["type"], "mention");
    assert_eq!(input[1]["name"], "tool [one] ](tail) (open)\\two");
    assert_eq!(input[1]["path"], "app://fixture/[branch](target)\\v1");
    assert_eq!(
        message.prompt(),
        "请打开 [$tool \\[one\\] \\]\\(tail) (open)\\\\two](app://fixture/[branch](target\\)\\\\v1) 后继续。"
    );
}

#[test]
fn native_mention_markdown_preserves_skill_destination_spaces() {
    let case = mention_link_fixture_case("markdownPathSpaces");
    let files = SyntheticFiles::new();
    let (message, input) = assert_native_mention_case(&case, &files);

    assert_eq!(input[1]["type"], "skill");
    let path = input[1]["path"]
        .as_str()
        .expect("synthetic skill path is a string");
    assert!(path.ends_with("/fixture skill (native.md"));
    assert_eq!(
        message.prompt(),
        format!("请使用 [$spacePath]({path}) 后继续。")
    );
}

#[test]
fn native_mention_projection_keeps_the_file_prefix_and_structured_references() {
    let case = mention_link_fixture_case("filePrefix");
    let files = SyntheticFiles::new();
    let (message, input) = assert_native_mention_case(&case, &files);

    assert!(input[0]["text"].as_str().is_some_and(
        |text| text.starts_with("# Files mentioned by the user:\n\n## fixture-note.txt: ")
    ));
    assert_eq!(input.as_array().map(Vec::len), Some(4));
    assert_eq!(input[1]["type"], "skill");
    assert_eq!(input[1]["name"], "outline");
    assert_eq!(input[2]["type"], "mention");
    assert_eq!(input[2]["name"], "search");
    assert_eq!(input[3]["type"], "mention");
    assert_eq!(input[3]["name"], "review");
    let attachments = message.attachments();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["label"], "fixture-note.txt");
    assert_eq!(attachments[0]["path"], attachments[0]["fsPath"]);
}

#[test]
fn native_mentions_do_not_rewrite_plain_text_without_selected_spans() {
    let case = mention_link_fixture_case("plainTextWithoutSpans");
    let files = SyntheticFiles::new();
    let (message, input) = assert_native_mention_case(&case, &files);

    assert!(message.native_mentions);
    assert!(message.reference_spans.is_empty());
    assert_eq!(message.prompt(), message.text);
    assert_eq!(input[0]["text"], message.text);
    assert_eq!(input[0]["text_elements"], json!([]));
    assert_eq!(input[1]["type"], "skill");
    assert_eq!(input[2]["type"], "mention");
    assert_eq!(input[3]["type"], "mention");
}

#[test]
fn invalid_native_mention_spans_reject_overlap_utf8_and_forged_ranges() {
    let overlap = MessageInput {
        native_mentions: true,
        client_id: CLIENT_ID.into(),
        text: "@same".into(),
        references: vec![
            app_reference("same", "overlap-a"),
            app_reference("same", "overlap-b"),
        ],
        reference_spans: vec![
            ReferenceSpan {
                reference: 0,
                start: 0,
                end: 5,
            },
            ReferenceSpan {
                reference: 1,
                start: 0,
                end: 5,
            },
        ],
    };
    assert_invalid(&overlap);

    let text = "😀@calendar";
    let non_character_boundary = MessageInput {
        native_mentions: true,
        client_id: CLIENT_ID.into(),
        text: text.into(),
        references: vec![app_reference("calendar", "utf8-boundary")],
        reference_spans: vec![ReferenceSpan {
            reference: 0,
            start: 1,
            end: 10,
        }],
    };
    assert!(text.get(1..10).is_none());
    assert_invalid(&non_character_boundary);

    let forged_reference = MessageInput {
        native_mentions: true,
        client_id: CLIENT_ID.into(),
        text: "@calendar".into(),
        references: vec![app_reference("calendar", "forged-reference")],
        reference_spans: vec![ReferenceSpan {
            reference: 1,
            start: 0,
            end: "@calendar".len(),
        }],
    };
    assert_invalid(&forged_reference);

    let forged_range = MessageInput {
        native_mentions: true,
        client_id: CLIENT_ID.into(),
        text: "@calendar".into(),
        references: vec![app_reference("calendar", "forged-range")],
        reference_spans: vec![ReferenceSpan {
            reference: 0,
            start: 0,
            end: "@calendar".len() + 1,
        }],
    };
    assert_invalid(&forged_range);
}

#[test]
fn native_mention_prompt_expansion_obeys_the_maximum_prompt_size() {
    let reference = NativeReference::app("x".into(), format!("app://{}", "p".repeat(4090)))
        .expect("synthetic maximum-length app URI is accepted");
    let mut text = String::new();
    let mut reference_spans = Vec::new();
    for _ in 0..32 {
        if !text.is_empty() {
            text.push(' ');
        }
        let start = text.len();
        text.push_str("@x");
        reference_spans.push(ReferenceSpan {
            reference: 0,
            start,
            end: text.len(),
        });
    }
    text.push_str(&"a".repeat(131_000));
    assert!(text.len() < 256 * 1024);

    let message = MessageInput {
        native_mentions: true,
        client_id: CLIENT_ID.into(),
        text,
        references: vec![reference],
        reference_spans,
    };
    assert!(message.prompt().len() > 256 * 1024);
    assert_invalid(&message);
}

#[test]
fn reference_span_uses_the_strict_camel_case_wire_shape() {
    let value: ReferenceSpan = serde_json::from_value(json!({
        "reference": 2,
        "start": 5,
        "end": 12
    }))
    .expect("strict reference span shape is accepted");
    assert_eq!(
        serde_json::to_value(value).expect("reference span serializes"),
        json!({"reference":2,"start":5,"end":12})
    );
    assert!(serde_json::from_value::<ReferenceSpan>(json!({
        "reference": 2,
        "start": 5,
        "end": 12,
        "unexpectedField": true
    }))
    .is_err());
}

#[test]
fn app_and_plugin_spans_keep_duplicate_names_bound_to_distinct_identities() {
    let case = fixture_case("sameNameDifferentIdentity");
    let files = SyntheticFiles::new();
    let message = message_from_fixture(&case, &files);

    assert!(message.validate().is_ok());
    let input = assert_text_elements(&message, &case, 0);
    let structured: Vec<_> = input
        .as_array()
        .expect("native input is an array")
        .iter()
        .skip(1)
        .cloned()
        .collect();
    assert_eq!(
        structured,
        vec![
            json!({"type":"mention","name":"search","path":"app://fixture-connector-a/search"}),
            json!({"type":"mention","name":"search","path":"app://fixture-connector-b/search"}),
            json!({"type":"mention","name":"review","path":"plugin://review@fixture-marketplace-a"}),
            json!({"type":"mention","name":"review","path":"plugin://review@fixture-marketplace-b"})
        ]
    );
}

#[test]
fn utf8_ranges_are_half_open_byte_offsets_after_chinese_and_emoji() {
    let case = fixture_case("utf8HalfOpen");
    let files = SyntheticFiles::new();
    let message = message_from_fixture(&case, &files);
    let span: ReferenceSpan =
        serde_json::from_value(case["spans"][0].clone()).expect("UTF-8 fixture span is valid");

    assert!(message.validate().is_ok());
    assert_eq!(span.start, 14);
    assert_eq!(span.end, 23);
    assert_eq!(span.end - span.start, "@calendar".len());
    assert_eq!(message.text.get(span.start..span.end), Some("@calendar"));
    assert_text_elements(&message, &case, 0);
}

#[test]
fn file_prefix_offsets_preserve_inline_spans_and_structured_references() {
    let case = fixture_case("filePrefix");
    let files = SyntheticFiles::new();
    let message = message_from_fixture(&case, &files);

    assert!(message.validate().is_ok());
    let prompt = message.prompt();
    let prefix_offset = prompt.len() - message.text.len();
    assert!(prefix_offset > 0);
    assert!(prompt.ends_with(&message.text));
    let input = assert_text_elements(&message, &case, prefix_offset);
    assert_eq!(input.as_array().map(Vec::len), Some(4));
    assert_eq!(input[1]["type"], "skill");
    assert_eq!(input[1]["name"], "outline");
    assert!(input[1]["path"]
        .as_str()
        .is_some_and(|path| path.ends_with("fixture-outline.md")));
    assert_eq!(
        input[2],
        json!({
            "type":"mention",
            "name":"search",
            "path":"app://fixture-search"
        })
    );
    assert_eq!(
        input[3],
        json!({
            "type":"mention",
            "name":"review",
            "path":"plugin://review@fixture-marketplace"
        })
    );

    let attachments = message.attachments();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["label"], "fixture-note.txt");
    assert_eq!(attachments[0]["path"], attachments[0]["fsPath"]);
}

#[test]
fn absent_spans_emit_an_empty_text_elements_array_and_keep_structured_inputs() {
    let case = fixture_case("structuredOnly");
    let files = SyntheticFiles::new();
    let message = message_from_fixture(&case, &files);

    assert!(message.validate().is_ok());
    let input = message.input();
    assert_eq!(input[0]["text"], message.text);
    assert_eq!(input[0]["text_elements"], json!([]));
    assert_eq!(input[1]["type"], "skill");
    assert_eq!(input[1]["name"], "outline");
    assert_eq!(
        input[2],
        json!({
            "type":"mention",
            "name":"search",
            "path":"app://fixture-search"
        })
    );
    assert_eq!(
        input[3],
        json!({
            "type":"mention",
            "name":"review",
            "path":"plugin://review@fixture-marketplace"
        })
    );
}

#[test]
fn overlapping_and_out_of_bounds_spans_are_rejected() {
    let overlapping = MessageInput {
        native_mentions: false,
        client_id: CLIENT_ID.into(),
        text: "@same".into(),
        references: vec![
            app_reference("same", "first-identity"),
            app_reference("same", "second-identity"),
        ],
        reference_spans: vec![
            ReferenceSpan {
                reference: 0,
                start: 0,
                end: 5,
            },
            ReferenceSpan {
                reference: 1,
                start: 0,
                end: 5,
            },
        ],
    };
    assert_invalid(&overlapping);

    let text = "@calendar";
    let out_of_bounds = MessageInput {
        native_mentions: false,
        client_id: CLIENT_ID.into(),
        text: text.into(),
        references: vec![app_reference("calendar", "calendar")],
        reference_spans: vec![ReferenceSpan {
            reference: 0,
            start: 0,
            end: text.len() + 1,
        }],
    };
    assert_invalid(&out_of_bounds);
}

#[test]
fn non_character_boundaries_and_mismatched_placeholders_are_rejected() {
    let text = "😀@calendar";
    let non_character_boundary = MessageInput {
        native_mentions: false,
        client_id: CLIENT_ID.into(),
        text: text.into(),
        references: vec![app_reference("calendar", "calendar")],
        reference_spans: vec![ReferenceSpan {
            reference: 0,
            start: 3,
            end: text.len(),
        }],
    };
    assert!(text.get(3..text.len()).is_none());
    assert_invalid(&non_character_boundary);

    let wrong_name = MessageInput {
        native_mentions: false,
        client_id: CLIENT_ID.into(),
        text: "@wrong".into(),
        references: vec![app_reference("right", "right")],
        reference_spans: vec![ReferenceSpan {
            reference: 0,
            start: 0,
            end: "@wrong".len(),
        }],
    };
    assert_invalid(&wrong_name);
}

#[test]
fn file_references_cannot_be_used_as_inline_text_spans() {
    let files = SyntheticFiles::new();
    let file = NativeReference::selected_file(files.write("fixture-file.txt", "synthetic file"))
        .expect("synthetic file reference is accepted");
    let message = MessageInput {
        native_mentions: false,
        client_id: CLIENT_ID.into(),
        text: "@fixture-file".into(),
        references: vec![file],
        reference_spans: vec![ReferenceSpan {
            reference: 0,
            start: 0,
            end: "@fixture-file".len(),
        }],
    };

    assert_invalid(&message);
}

#[test]
fn span_count_is_limited_to_32_and_accepts_the_boundary() {
    fn repeated_message(count: usize) -> MessageInput {
        let mut text = String::new();
        let mut reference_spans = Vec::new();
        for index in 0..count {
            if index > 0 {
                text.push(' ');
            }
            let start = text.len();
            text.push_str("@tag");
            reference_spans.push(ReferenceSpan {
                reference: 0,
                start,
                end: text.len(),
            });
        }
        MessageInput {
            native_mentions: false,
            client_id: CLIENT_ID.into(),
            text,
            references: vec![app_reference("tag", "shared-identity")],
            reference_spans,
        }
    }

    assert!(repeated_message(32).validate().is_ok());
    assert_invalid(&repeated_message(33));
}
