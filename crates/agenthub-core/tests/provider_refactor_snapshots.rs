use agenthub_core::{
    claude::{claude_session_detail, list_claude_sessions, ClaudeDetailRequest, ClaudePaths},
    codex::{thread_detail_page_from_summary, ThreadStatus, ThreadSummary},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

const CODEX_HISTORY: &str =
    include_str!("fixtures/provider_refactor_snapshots/codex-rollout.jsonl");
const CLAUDE_HISTORY: &str =
    include_str!("fixtures/provider_refactor_snapshots/claude-transcript.jsonl");
const CLAUDE_DAMAGED_HISTORY: &str =
    include_str!("fixtures/provider_refactor_snapshots/claude-damaged-transcript.jsonl");
const CODEX_ID: &str = "codex-refactor-snapshot";
const CLAUDE_ID: &str = "claude-refactor-snapshot";
const CLAUDE_DAMAGED_ID: &str = "claude-damaged-snapshot";
const WORKSPACE: &str = "/synthetic/provider-refactor/workspace";

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "agenthub-provider-refactor-snapshot-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create isolated provider snapshot fixture");
        Self(fs::canonicalize(root).expect("canonicalize provider snapshot fixture"))
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.0).is_err() {
            eprintln!("PROVIDER_REFACTOR_SNAPSHOT_FIXTURE_CLEANUP_FAILED");
        }
    }
}

#[test]
fn provider_history_serialization_matches_refactor_snapshots() {
    let fixture = FixtureRoot::new();
    let codex_path = fixture.0.join("codex/rollout.jsonl");
    fs::create_dir_all(codex_path.parent().unwrap()).expect("create Codex fixture directory");
    fs::write(&codex_path, CODEX_HISTORY).expect("write synthetic Codex rollout");

    let codex_summary = ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: CODEX_ID.into(),
        title: "Synthetic provider snapshot".into(),
        status: ThreadStatus::Recent,
        updated_at: Some("2026-01-02T03:04:14Z".into()),
        archived_at: None,
        message_count: CODEX_HISTORY.lines().count(),
        latest_message: Some("Confirmed.".into()),
        cwd: Some(WORKSPACE.into()),
        model: Some("fixture-model".into()),
        rollout_path: Some(codex_path),
        active_turn_id: None,
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: Some("task_complete".into()),
    };
    let codex_full = thread_detail_page_from_summary(codex_summary.clone(), true, None, None, None)
        .expect("read complete synthetic Codex history");
    let codex_latest =
        thread_detail_page_from_summary(codex_summary.clone(), true, Some(3), None, None)
            .expect("read latest Codex page");
    let codex_before = codex_latest
        .before_cursor
        .clone()
        .expect("latest Codex page has an older cursor");
    let codex_older =
        thread_detail_page_from_summary(codex_summary, true, Some(3), Some(&codex_before), None)
            .expect("read older Codex page");

    assert_eq!(
        codex_full.raw_event_count,
        CODEX_HISTORY.lines().count(),
        "a malformed rollout row is counted but must not abort later history"
    );
    assert!(codex_full
        .messages
        .iter()
        .any(|message| message.role == "assistant" && message.text == "Confirmed."));
    assert!(codex_full.blocks.iter().any(|block| {
        block.call_id.as_deref() == Some("codex-call-read")
            && block.status.as_deref() == Some("completed")
            && block
                .text
                .as_deref()
                .is_some_and(|text| text.contains("Synthetic file contents"))
    }));
    assert!(codex_full.blocks.iter().any(|block| {
        block.call_id.as_deref() == Some("codex-call-question")
            && block
                .questions
                .iter()
                .any(|question| question.question == "Keep the synthetic result?")
    }));
    assert!(codex_latest.has_more_blocks);
    assert_eq!(codex_latest.blocks.len(), 3);
    assert!(!codex_older.blocks.is_empty());

    let claude_root = fixture.0.join("claude");
    let config = claude_root.join("config");
    let projects = config.join("projects");
    let claude_path = projects.join("synthetic-project/transcript.jsonl");
    fs::create_dir_all(claude_path.parent().unwrap()).expect("create Claude fixture directory");
    fs::write(&claude_path, CLAUDE_HISTORY).expect("write synthetic Claude transcript");
    let damaged_path = projects.join("synthetic-damaged-project/transcript.jsonl");
    fs::create_dir_all(damaged_path.parent().unwrap())
        .expect("create damaged Claude fixture directory");
    fs::write(&damaged_path, CLAUDE_DAMAGED_HISTORY)
        .expect("write damaged synthetic Claude transcript");
    let claude_paths = ClaudePaths { config, projects };
    let claude_summaries =
        list_claude_sessions(&claude_paths, 10, None).expect("list synthetic Claude session");
    let claude_summary = claude_summaries
        .iter()
        .find(|summary| summary.id == CLAUDE_ID)
        .cloned()
        .expect("find synthetic Claude session");
    let claude_damaged_summary = claude_summaries
        .iter()
        .find(|summary| summary.id == CLAUDE_DAMAGED_ID)
        .cloned()
        .expect("find damaged synthetic Claude session");
    assert_eq!(claude_summary.status, "recent");
    assert!(claude_summary.read_error.is_none());
    assert!(claude_summary.can_rename && claude_summary.can_delete);
    assert_eq!(claude_damaged_summary.status, "unknown");
    assert_eq!(
        claude_damaged_summary.read_error.as_deref(),
        Some("会话含损坏记录，改名已禁用"),
        "a malformed Claude row disables rename without hiding later records or blocking deletion"
    );
    assert!(!claude_damaged_summary.can_rename && claude_damaged_summary.can_delete);

    let claude_full = claude_session_detail(
        &claude_paths,
        &ClaudeDetailRequest {
            agent_id: None,
            session_key: claude_summary.session_key.clone(),
            limit: None,
            before: None,
        },
    )
    .expect("read complete synthetic Claude history");
    let claude_latest = claude_session_detail(
        &claude_paths,
        &ClaudeDetailRequest {
            agent_id: None,
            session_key: claude_summary.session_key.clone(),
            limit: Some(4),
            before: None,
        },
    )
    .expect("read latest Claude page");
    let claude_before = claude_latest
        .before_cursor
        .clone()
        .expect("latest Claude page has an older cursor");
    let claude_older = claude_session_detail(
        &claude_paths,
        &ClaudeDetailRequest {
            agent_id: None,
            session_key: claude_summary.session_key.clone(),
            limit: Some(4),
            before: Some(claude_before),
        },
    )
    .expect("read older Claude page");

    assert!(claude_full.events.iter().any(|event| {
        event.call_id.as_deref() == Some("claude-call-read")
            && event.status.as_deref() == Some("completed")
            && event
                .result
                .as_deref()
                .is_some_and(|result| result.contains("Synthetic file contents"))
    }));
    assert!(claude_full.events.iter().any(|event| {
        event.call_id.as_deref() == Some("claude-call-question")
            && event.status.as_deref() == Some("completed")
            && event.kind == "tool_call"
    }));
    assert!(claude_full.events.iter().any(|event| {
        event.kind == "assistant_message" && event.text.as_deref() == Some("Confirmed.")
    }));
    assert!(claude_latest.has_more);
    assert_eq!(claude_latest.events.len(), 4);
    assert!(!claude_older.events.is_empty());
    assert!(claude_full.total_events > claude_latest.events.len());
    assert_eq!(claude_full.summary.status, "recent");

    let claude_damaged = claude_session_detail(
        &claude_paths,
        &ClaudeDetailRequest {
            agent_id: None,
            session_key: claude_damaged_summary.session_key.clone(),
            limit: None,
            before: None,
        },
    )
    .expect("read damaged synthetic Claude history");
    assert!(claude_damaged.events.iter().any(|event| {
        event.kind == "assistant_message"
            && event.text.as_deref() == Some("Recovered after malformed row.")
    }));
    assert_eq!(claude_damaged.summary.status, "unknown");
    assert!(claude_damaged.summary.read_error.is_some());

    let mut actual = json!({
        "codex": {
            "full": serde_json::to_value(&codex_full).expect("serialize full Codex detail"),
            "latestPage": serde_json::to_value(&codex_latest).expect("serialize latest Codex page"),
            "olderPage": serde_json::to_value(&codex_older).expect("serialize older Codex page"),
        },
        "claude": {
            "listedSummary": serde_json::to_value(&claude_summary).expect("serialize Claude summary"),
            "full": serde_json::to_value(&claude_full).expect("serialize full Claude detail"),
            "latestPage": serde_json::to_value(&claude_latest).expect("serialize latest Claude page"),
            "olderPage": serde_json::to_value(&claude_older).expect("serialize older Claude page"),
            "damagedSummary": serde_json::to_value(&claude_damaged_summary).expect("serialize damaged Claude summary"),
            "damagedDetail": serde_json::to_value(&claude_damaged).expect("serialize damaged Claude detail"),
        },
    });
    let mut epochs = BTreeMap::new();
    normalize_snapshot(&mut actual, &fixture.0, false, &mut epochs);
    assert!(
        !epochs.is_empty(),
        "history metadata should retain its non-empty runtime epoch"
    );
    compare_or_update_snapshot(actual);
}

fn normalize_snapshot(
    value: &mut Value,
    fixture_root: &Path,
    in_history: bool,
    epochs: &mut BTreeMap<String, String>,
) {
    match value {
        Value::String(text) => {
            *text = text.replace(
                &fixture_root.to_string_lossy().to_string(),
                "<FIXTURE_ROOT>",
            );
        }
        Value::Array(values) => {
            for value in values {
                normalize_snapshot(value, fixture_root, in_history, epochs);
            }
        }
        Value::Object(fields) => {
            if in_history && fields.contains_key("epoch") {
                let original = fields
                    .get("epoch")
                    .and_then(Value::as_str)
                    .expect("history epoch is a string");
                assert!(!original.is_empty(), "history epoch must be present");
                let next = epochs.len() + 1;
                epochs
                    .entry(original.to_owned())
                    .or_insert_with(|| format!("runtime-epoch-{next}"));
            }
            for (key, value) in fields {
                if in_history && key == "epoch" {
                    let original = value.as_str().expect("history epoch is a string");
                    let stable = epochs.get(original).unwrap().clone();
                    *value = Value::String(stable);
                } else if in_history && key == "revision" {
                    if let Some(revision) = value.as_str() {
                        if let Some((epoch, offset)) = revision.split_once(':') {
                            if let Some(stable) = epochs.get(epoch) {
                                *value = Value::String(format!("{stable}:{offset}"));
                            }
                        }
                    }
                } else {
                    normalize_snapshot(value, fixture_root, in_history || key == "history", epochs);
                }
            }
        }
        _ => {}
    }
}

fn compare_or_update_snapshot(actual: Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/provider_refactor_snapshots/expected.json");
    let serialized = format!("{}\n", serde_json::to_string_pretty(&actual).unwrap());
    if env::var("AGENTHUB_UPDATE_REFACTOR_SNAPSHOTS")
        .map(|value| value == "1")
        .unwrap_or(false)
    {
        fs::write(&path, serialized).expect("write provider refactor snapshot baseline");
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "provider refactor snapshot is missing at {}; generate it from the pre-refactor source with AGENTHUB_UPDATE_REFACTOR_SNAPSHOTS=1 ({error})",
            path.display()
        )
    });
    assert_eq!(
        serialized, expected,
        "provider history serialization changed; compare the pre-refactor baseline before updating"
    );
}
