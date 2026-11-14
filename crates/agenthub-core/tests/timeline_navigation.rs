use agenthub_core::{
    claude::{claude_session_detail, list_claude_sessions, ClaudeDetailRequest, ClaudePaths},
    codex::{thread_detail_page_from_summary, ThreadDetail, ThreadStatus, ThreadSummary},
    grok::{grok_session_page, GrokPaths},
    history_navigation::{after_cursor, seek_window, timeline},
    user_message::{parse_user_message, UserMessage},
};
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

fn user_message(id: &str, text: &str) -> UserMessage {
    parse_user_message(id, &Value::String(text.to_owned()))
}

#[test]
fn timeline_deduplicates_native_ids_and_excludes_context_or_metadata_only_prompts() {
    let first = user_message("user-1", "重复的正文");
    let second = user_message("user-2", "重复的正文");
    let duplicate = user_message("user-1", "同一 ID 的重复记录");
    let context_text =
        "<environment_context>\n  <cwd>/synthetic/workspace</cwd>\n</environment_context>";
    let context_only = user_message("context-only", context_text);
    let memory_metadata = user_message(
        "memory-metadata-only",
        "<oai-mem-citation>fixture citation</oai-mem-citation>",
    );
    let page_metadata = user_message(
        "page-metadata-only",
        "<external_codex_apps_open_page>{\"page_id\":\"fixture-page\"}</external_codex_apps_open_page>",
    );

    let entries = timeline(
        vec![
            ("user-1", Some(&first), Some("重复的正文")),
            ("user-2", Some(&second), Some("重复的正文")),
            ("user-1", Some(&duplicate), Some("同一 ID 的重复记录")),
            ("context-only", Some(&context_only), Some(context_text)),
            (
                "memory-metadata-only",
                Some(&memory_metadata),
                Some("<oai-mem-citation>fixture citation</oai-mem-citation>"),
            ),
            (
                "page-metadata-only",
                Some(&page_metadata),
                Some("<external_codex_apps_open_page>{\"page_id\":\"fixture-page\"}</external_codex_apps_open_page>"),
            ),
        ]
        .into_iter(),
    );

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["user-1", "user-2"],
        "identical prompt text with different native IDs stays distinct"
    );
    assert!(entries.iter().all(|entry| entry.title == "重复的正文"));
}

#[test]
fn timeline_truncates_unicode_by_character_without_splitting_text() {
    let text = "你好🧭".repeat(100);
    let message = user_message("unicode-user", &text);
    let entries = timeline(std::iter::once((
        "unicode-user",
        Some(&message),
        Some(text.as_str()),
    )));

    let entry = entries.first().expect("non-empty Unicode user prompt");
    assert_eq!(entry.title.chars().count(), 80);
    assert_eq!(entry.preview.as_deref().unwrap().chars().count(), 180);
    assert_eq!(
        format!("{}{}", entry.title, entry.preview.as_deref().unwrap()),
        text.chars().take(260).collect::<String>()
    );
}

#[test]
fn stable_seek_cursors_page_after_the_target_and_survive_appends() {
    let ids = (0..8)
        .map(|index| format!("user-{index}"))
        .collect::<Vec<_>>();
    let jump = seek_window(ids.iter().map(String::as_str), Some("j:user-3"), 4)
        .unwrap()
        .unwrap();
    assert_eq!(jump, (2, 6));
    assert!(ids[jump.0..jump.1].iter().any(|id| id == "user-3"));

    let cursor = after_cursor(jump.1, ids.len(), ids.get(jump.1 - 1).map(String::as_str));
    assert_eq!(cursor.as_deref(), Some("n:user-5"));

    let mut appended = ids.clone();
    appended.extend(["user-8".to_owned(), "user-9".to_owned()]);
    let next = seek_window(appended.iter().map(String::as_str), cursor.as_deref(), 3)
        .unwrap()
        .unwrap();
    assert_eq!(next, (6, 9));
    assert_eq!(
        appended[next.0..next.1]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["user-6", "user-7", "user-8"]
    );

    assert_eq!(
        seek_window(ids.iter().map(String::as_str), Some("b:4"), 4).unwrap(),
        None,
        "provider-specific legacy cursors remain outside stable seek handling"
    );
    let error = seek_window(ids.iter().map(String::as_str), Some("j:missing"), 3).unwrap_err();
    assert!(error.to_string().contains("历史位置已变化"));
}

struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("agenthub-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create isolated test fixture");
        Self { root }
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct CodexFixture {
    _temp: TempFixture,
    summary: ThreadSummary,
}

impl CodexFixture {
    fn new(events: &[Value]) -> Self {
        let temp = TempFixture::new("timeline-codex");
        let rollout = temp.root.join("rollout.jsonl");
        let summary = ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "fixture-thread".to_owned(),
            title: "Synthetic timeline fixture".to_owned(),
            status: ThreadStatus::Recent,
            updated_at: None,
            archived_at: None,
            message_count: events.len(),
            latest_message: None,
            cwd: Some("/synthetic/workspace".to_owned()),
            model: None,
            rollout_path: Some(rollout),
            active_turn_id: None,
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: None,
        };
        let fixture = Self {
            _temp: temp,
            summary,
        };
        fixture.write(events);
        fixture
    }

    fn rollout_path(&self) -> &Path {
        self.summary.rollout_path.as_deref().unwrap()
    }

    fn write(&self, events: &[Value]) {
        let body = events
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(self.rollout_path(), format!("{body}\n")).unwrap();
    }

    fn append(&self, events: &[Value]) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(self.rollout_path())
            .unwrap();
        let body = events
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        writeln!(file, "{body}").unwrap();
    }

    fn read(
        &self,
        compact: bool,
        limit: Option<usize>,
        before: Option<&str>,
    ) -> Result<ThreadDetail> {
        thread_detail_page_from_summary(self.summary.clone(), compact, limit, before, None)
    }
}

fn codex_message(id: &str, role: &str, text: &str) -> Value {
    let part_type = if role == "user" {
        "input_text"
    } else {
        "output_text"
    };
    json!({
        "type":"response_item",
        "payload":{
            "type":"message",
            "id":id,
            "role":role,
            "content":[{"type":part_type,"text":text}]
        }
    })
}

fn codex_block_ids(detail: &ThreadDetail) -> Vec<String> {
    detail.blocks.iter().map(|block| block.id.clone()).collect()
}

#[test]
fn codex_latest_page_keeps_full_timeline_and_stable_navigation_across_compaction() {
    let mut events = vec![
        codex_message("user-1", "user", "相同的请求"),
        codex_message("assistant-1", "assistant", "回答一"),
        codex_message("user-2", "user", "相同的请求"),
        codex_message("assistant-2", "assistant", "回答二"),
        codex_message(
            "context-only",
            "user",
            "<environment_context>\n  <cwd>/synthetic/workspace</cwd>\n</environment_context>",
        ),
        codex_message("assistant-3", "assistant", "回答三"),
    ];
    events.extend((0..80).map(|index| {
        codex_message(
            &format!("filler-{index}"),
            "assistant",
            &format!("Synthetic filler {index}"),
        )
    }));
    let fixture = CodexFixture::new(&events);

    let folded = fixture.read(true, None, None).unwrap();
    assert!(folded
        .blocks
        .iter()
        .any(|block| block.kind == "chat_history_collapsed"));

    let latest = fixture.read(true, Some(1), None).unwrap();
    assert!(latest.blocks.iter().all(|block| block.role != "user"));
    let timeline_ids = latest
        .timeline
        .as_ref()
        .expect("latest page has the full user timeline")
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(timeline_ids, ["user-1", "user-2"]);
    assert!(latest.before_cursor.is_some());

    let older = fixture
        .read(true, Some(3), latest.before_cursor.as_deref())
        .unwrap();
    assert!(
        !older.blocks.is_empty(),
        "before cursor still pages backward"
    );
    assert!(older.timeline.is_none(), "seek pages omit the full index");

    let jump = fixture.read(true, Some(3), Some("j:user-2")).unwrap();
    assert_eq!(
        codex_block_ids(&jump),
        ["user-2", "assistant-2", "context-only"]
    );
    assert!(jump.timeline.is_none());
    assert_eq!(jump.before_cursor.as_deref(), Some("p:user-2"));
    let previous = fixture
        .read(true, Some(2), jump.before_cursor.as_deref())
        .unwrap();
    assert_eq!(codex_block_ids(&previous), ["user-1", "assistant-1"]);
    assert!(previous.timeline.is_none());

    let next_cursor = jump.after_cursor.as_deref().unwrap();
    assert_eq!(next_cursor, "n:context-only");

    let next = fixture.read(true, Some(3), Some(next_cursor)).unwrap();
    assert_eq!(
        codex_block_ids(&next),
        ["assistant-3", "filler-0", "filler-1"]
    );
    let mut visited = codex_block_ids(&jump);
    visited.extend(codex_block_ids(&next));
    let mut unique = visited.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        visited.len(),
        unique.len(),
        "n cursor must not repeat a block"
    );

    let new_events = [
        codex_message("user-3", "user", "第三个请求"),
        codex_message("assistant-4", "assistant", "第四个回答"),
    ];
    fixture.append(&new_events);
    events.extend(new_events);
    let appended_latest = fixture.read(true, Some(1), None).unwrap();
    let appended_timeline = appended_latest
        .timeline
        .as_ref()
        .unwrap()
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(appended_timeline, ["user-1", "user-2", "user-3"]);
    let stable_next = fixture.read(true, Some(3), Some(next_cursor)).unwrap();
    assert_eq!(codex_block_ids(&stable_next), codex_block_ids(&next));

    let without_target = events
        .iter()
        .filter(|event| event.pointer("/payload/id").and_then(Value::as_str) != Some("user-2"))
        .cloned()
        .collect::<Vec<_>>();
    fixture.write(&without_target);
    let error = fixture.read(true, Some(3), Some("j:user-2")).unwrap_err();
    assert!(error.to_string().contains("历史位置已变化"));
}

struct ClaudeFixture {
    _temp: TempFixture,
    paths: ClaudePaths,
}

impl ClaudeFixture {
    fn new() -> Self {
        let temp = TempFixture::new("timeline-claude");
        let config = temp.root.join("config");
        let paths = ClaudePaths {
            projects: config.join("projects"),
            config,
        };
        fs::create_dir_all(paths.projects.join("project")).unwrap();
        let fixture = Self { _temp: temp, paths };
        fs::create_dir_all(fixture.root().join("workspace")).unwrap();
        fixture
    }

    fn root(&self) -> &Path {
        &self._temp.root
    }

    fn transcript(&self) -> PathBuf {
        self.paths.projects.join("project/native.jsonl")
    }

    fn row(&self, id: &str, parent: Option<&str>, role: &str, text: &str) -> Value {
        json!({
            "type":role,
            "uuid":id,
            "parentUuid":parent,
            "sessionId":"timeline-fixture",
            "cwd":self.root().join("workspace"),
            "version":"2.1.284",
            "timestamp":"2026-10-15T00:00:00Z",
            "message":{
                "role":role,
                "content":text,
                "stop_reason":if role == "assistant" {"end_turn"} else {""}
            }
        })
    }

    fn write(&self, rows: &[Value]) {
        let body = rows
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(self.transcript(), format!("{body}\n")).unwrap();
    }

    fn key(&self) -> String {
        list_claude_sessions(&self.paths, 10, None)
            .unwrap()
            .into_iter()
            .find(|summary| summary.id == "timeline-fixture")
            .expect("fixture is discoverable")
            .session_key
    }
}

#[test]
fn claude_pages_keep_user_message_markers_and_only_latest_page_has_timeline() {
    let fixture = ClaudeFixture::new();
    let rows = [
        fixture.row("user-1", None, "user", "相同请求"),
        fixture.row("assistant-1", Some("user-1"), "assistant", "回答一"),
        fixture.row("user-2", Some("assistant-1"), "user", "相同请求"),
        fixture.row("assistant-2", Some("user-2"), "assistant", "回答二"),
    ];
    fixture.write(&rows);
    let key = fixture.key();

    let latest = claude_session_detail(
        &fixture.paths,
        &ClaudeDetailRequest {
            agent_id: None,
            session_key: key.clone(),
            limit: Some(2),
            before: None,
        },
    )
    .unwrap();
    assert_eq!(
        latest
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        ["user-2", "assistant-2:0"]
    );
    assert!(latest.events[0].user_message.is_some());
    assert_eq!(
        latest
            .timeline
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["user-1", "user-2"]
    );

    let older = claude_session_detail(
        &fixture.paths,
        &ClaudeDetailRequest {
            agent_id: None,
            session_key: key,
            limit: Some(2),
            before: latest.before_cursor,
        },
    )
    .unwrap();
    assert_eq!(
        older
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        ["user-1", "assistant-1:0"]
    );
    assert!(older.timeline.is_none());
    assert!(older.events[0].user_message.is_some());
    assert!(!older.has_more);
}

struct GrokFixture {
    _temp: TempFixture,
    paths: GrokPaths,
    id: String,
    updates: PathBuf,
}

impl GrokFixture {
    fn new() -> Self {
        let temp = TempFixture::new("timeline-grok");
        let id = uuid::Uuid::new_v4().to_string();
        let path = temp.root.join("sessions/%2Fsynthetic").join(&id);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("summary.json"),
            serde_json::to_vec(&json!({
                "info":{"id":id,"cwd":"/synthetic/workspace"},
                "generated_title":"Synthetic timeline fixture",
                "num_messages":4
            }))
            .unwrap(),
        )
        .unwrap();
        let updates = path.join("updates.jsonl");
        fs::write(&updates, "").unwrap();
        fs::write(temp.root.join("active_sessions.json"), "{}").unwrap();
        Self {
            paths: GrokPaths {
                home: temp.root.clone(),
            },
            _temp: temp,
            id,
            updates,
        }
    }

    fn update(kind: &str, message_id: &str, text: &str) -> Value {
        json!({
            "params":{
                "update":{
                    "sessionUpdate":kind,
                    "messageId":message_id,
                    "content":{"type":"text","text":text}
                }
            }
        })
    }

    fn write(&self, rows: &[Value]) {
        let body = rows
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&self.updates, format!("{body}\n")).unwrap();
    }

    fn write_over_64_mib_history(&self) -> String {
        const TARGET_BYTES: u64 = 65 * 1024 * 1024;

        let mut writer = BufWriter::new(fs::File::create(&self.updates).unwrap());
        let early_user = Self::update("user_message_chunk", "early-user", "早期请求").to_string();
        writeln!(writer, "{early_user}").unwrap();
        let mut written = early_user.len() as u64 + 1;

        let padding = json!({
            "params": {
                "update": {
                    "sessionUpdate": "fixture_padding",
                    "content": {"type": "text", "text": "x".repeat(768)}
                }
            }
        })
        .to_string();
        let padding_bytes = padding.len() as u64 + 1;
        while written < TARGET_BYTES {
            writer.write_all(padding.as_bytes()).unwrap();
            writer.write_all(b"\n").unwrap();
            written += padding_bytes;
        }

        let tail = Self::update("agent_message_chunk", "", "末尾回答").to_string();
        writeln!(writer, "{tail}").unwrap();
        writer.flush().unwrap();
        "grok:0".to_owned()
    }
}

#[test]
fn grok_pages_keep_user_message_markers_and_only_latest_page_has_timeline() {
    let fixture = GrokFixture::new();
    fixture.write(&[
        GrokFixture::update("user_message_chunk", "user-1", "相同请求"),
        GrokFixture::update("agent_message_chunk", "", "回答一"),
        GrokFixture::update("user_message_chunk", "user-2", "相同请求"),
        GrokFixture::update("agent_message_chunk", "", "回答二"),
    ]);

    let latest = grok_session_page(&fixture.paths, &fixture.id, None, Some(2), None, None).unwrap();
    assert_eq!(latest.events.len(), 2);
    assert_eq!(latest.events[0].kind, "user_message_chunk");
    assert!(latest.events[0].user_message.is_some());
    let timeline_ids = latest
        .timeline
        .as_ref()
        .unwrap()
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(timeline_ids.len(), 2);
    assert_ne!(timeline_ids[0], timeline_ids[1]);
    assert_eq!(timeline_ids[1], latest.events[0].id);

    let older = grok_session_page(
        &fixture.paths,
        &fixture.id,
        None,
        Some(2),
        latest.before_cursor.as_deref(),
        None,
    )
    .unwrap();
    assert_eq!(older.events.len(), 2);
    assert_eq!(older.events[0].kind, "user_message_chunk");
    assert!(older.events[0].user_message.is_some());
    assert!(older.timeline.is_none());
    assert!(!older.has_more);
}

#[test]
fn grok_latest_page_indexes_and_jumps_to_user_prompt_before_64_mib_tail_window() {
    let fixture = GrokFixture::new();
    let early_user_id = fixture.write_over_64_mib_history();
    assert!(fs::metadata(&fixture.updates).unwrap().len() > 64 * 1024 * 1024);

    let latest = grok_session_page(&fixture.paths, &fixture.id, None, Some(1), None, None).unwrap();
    assert_eq!(latest.events.len(), 1);
    assert_eq!(latest.events[0].kind, "agent_message_chunk");
    let timeline_ids = latest
        .timeline
        .as_ref()
        .expect("latest page has the full user timeline")
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(timeline_ids, [early_user_id.as_str()]);

    let cursor = format!("j:{early_user_id}");
    let jump = grok_session_page(
        &fixture.paths,
        &fixture.id,
        None,
        Some(1),
        Some(&cursor),
        None,
    )
    .unwrap();
    assert_eq!(jump.events.len(), 1);
    assert_eq!(jump.events[0].id, early_user_id);
    assert!(jump.events[0].user_message.is_some());
    assert!(jump.timeline.is_none());
}
