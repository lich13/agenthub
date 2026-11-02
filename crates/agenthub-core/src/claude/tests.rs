use super::*;
use serde_json::json;
use std::io::Write;

struct Fixture {
    root: PathBuf,
    paths: ClaudePaths,
    file: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = env::temp_dir().join(format!("agenthub-claude-test-{}", uuid::Uuid::new_v4()));
        let config = root.join("config");
        let paths = ClaudePaths {
            projects: config.join("projects"),
            config,
        };
        let file = paths.projects.join("project/native-fixture.jsonl");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::create_dir_all(root.join("workspace")).unwrap();
        Self { root, paths, file }
    }
    fn row(&self, id: &str, parent: Option<&str>, kind: &str, content: Value) -> Value {
        json!({"type":kind,"uuid":id,"parentUuid":parent,"sessionId":"native-fixture","cwd":self.root.join("workspace"),"version":"2.1.284","timestamp":"2026-09-29T00:00:00Z","message":{"role":kind,"content":content,"stop_reason":if kind=="assistant" {"end_turn"} else {""}}})
    }
    fn write(&self, rows: &[Value]) {
        fs::write(
            &self.file,
            rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
    }
    fn append(&self, row: &Value) {
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(&self.file)
                .unwrap(),
            "{row}"
        )
        .unwrap();
    }
    fn complete(&self) {
        self.write(&[
            self.row("u1", None, "user", json!("Example request")),
            self.row(
                "a1",
                Some("u1"),
                "assistant",
                json!([{"type":"text","text":"Example complete"}]),
            ),
        ]);
    }
    fn key(&self) -> String {
        key_for(
            &self.paths.projects.canonicalize().unwrap(),
            &self
                .paths
                .projects
                .canonicalize()
                .unwrap()
                .join("project/native-fixture.jsonl"),
            "native-fixture",
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn lists_sort_by_conversation_time_and_ignore_metadata_and_tools() {
    let f = Fixture::new();
    let mut first = f.row("u1", None, "user", json!("Fixture request"));
    first["timestamp"] = json!("2026-10-01T11:00:00+08:00");
    f.write(&[first]);
    let second_path = f.file.with_file_name("other-fixture.jsonl");
    let mut second = f.row(
        "a2",
        None,
        "assistant",
        json!([{"type":"text","text":"Fixture answer"}]),
    );
    second["sessionId"] = json!("other-fixture");
    second["timestamp"] = json!("2026-10-01T04:00:00Z");
    fs::write(&second_path, format!("{second}\n")).unwrap();
    f.append(&json!({"type":"custom-title","timestamp":"2026-12-01T00:00:00Z","customTitle":"Renamed fixture"}));
    let mut tool = f.row(
        "tool",
        Some("u1"),
        "assistant",
        json!([{"type":"tool_use","id":"call","name":"Read","input":{}}]),
    );
    tool["timestamp"] = json!("2026-12-02T00:00:00Z");
    f.append(&tool);
    let first = list_claude_sessions(&f.paths, 1, None).unwrap();
    assert_eq!(first[0].id, "other-fixture");
    let mut follow_up = f.row("u2", Some("tool"), "user", json!("Follow up"));
    follow_up["timestamp"] = json!("2026-10-01T05:00:00Z");
    f.append(&follow_up);
    let latest = list_claude_sessions(&f.paths, 1, None).unwrap();
    assert_eq!(latest[0].id, "native-fixture");
    assert_eq!(
        latest[0].updated_at.as_deref(),
        Some("2026-10-01T05:00:00Z")
    );
    assert_eq!(
        claude_session_detail(
            &f.paths,
            &ClaudeDetailRequest {
                session_key: latest[0].session_key.clone(),
                limit: None,
                before: None
            }
        )
        .unwrap()
        .summary
        .updated_at,
        latest[0].updated_at
    );
}

#[test]
fn native_commands_and_automatic_context_preserve_the_real_prompt_and_pending_tools() {
    let f = Fixture::new();
    let args = "Keep line one\n  keep indentation 🙂";
    let mut request = f.row("request", None, "user", json!(format!("<command-name>/plan</command-name>\n <command-message>plan</command-message>\n <command-args>{args}</command-args>")));
    request["promptId"] = json!("native-prompt");
    let mut echo = f.row(
        "echo",
        Some("request"),
        "user",
        json!("<local-command-stdout>Enabled plan mode</local-command-stdout>"),
    );
    echo["promptId"] = json!("native-prompt");
    let mut meta = f.row(
        "meta",
        Some("echo"),
        "user",
        json!("Automatic context fixture"),
    );
    meta["isMeta"] = json!(true);
    let call = f.row(
        "call",
        Some("meta"),
        "assistant",
        json!([{"type":"tool_use","id":"read-fixture","name":"Read","input":{}}]),
    );
    let notice = f.row(
        "notice",
        Some("call"),
        "user",
        json!(
            "<task-notification><summary>Background fixture complete</summary></task-notification>"
        ),
    );
    let result = f.row(
        "result",
        Some("notice"),
        "user",
        json!([{"type":"tool_result","tool_use_id":"read-fixture","content":"Fixture output"}]),
    );
    let answer = f.row(
        "answer",
        Some("result"),
        "assistant",
        json!("Finished fixture"),
    );
    let mut compact = f.row(
        "compact",
        Some("answer"),
        "user",
        json!("Long summary fixture"),
    );
    compact["isCompactSummary"] = json!(true);
    f.write(&[request, echo, meta, call, notice, result, answer, compact]);
    let parsed = reader::read(&f.file).unwrap();
    let users: Vec<_> = parsed
        .events
        .iter()
        .filter(|e| e.kind == "user_message")
        .collect();
    assert_eq!(users.len(), 1);
    assert_eq!(
        users[0].user_message.as_ref().unwrap().text,
        format!("/plan\n{args}")
    );
    assert!(parsed
        .events
        .iter()
        .all(|e| e.turn_id.as_deref() == Some("request")));
    assert!(parsed
        .events
        .iter()
        .any(|e| e.kind == "tool_call" && e.status.as_deref() == Some("completed")));
    assert!(parsed.events.iter().any(|e| e.kind == "compaction"));
    assert_eq!(parsed.prompts.len(), 1);
    assert!(!parsed.prompts["native-prompt"].ambiguous);
    assert_eq!(parsed.native_events.len(), 1);
    assert!(!parsed.turn_open);
}

#[test]
fn automatic_only_sessions_do_not_open_turns_and_command_examples_stay_literal() {
    let f = Fixture::new();
    f.write(&[
        f.row("context", None, "user", json!("<system-reminder>Fixture reminder</system-reminder>")),
        json!({"type":"attachment","attachment":{"type":"instructions","files":[{"content":"Preserved instruction fixture"}]}}),
    ]);
    let parsed = reader::read(&f.file).unwrap();
    assert!(!parsed.turn_open);
    assert!(parsed.prompts.is_empty());
    assert!(parsed
        .events
        .iter()
        .all(|e| e.kind != "user_message" && e.turn_id.is_none()));
    let instructions = parsed.events.last().unwrap().user_message.as_ref().unwrap();
    assert_eq!(
        instructions.context_blocks[0].kind,
        crate::user_message::UserContextKind::Instructions
    );
    assert_eq!(
        instructions.context_blocks[0].text,
        "Preserved instruction fixture"
    );
    for body in [
        "```xml\n<command-name>/plan</command-name><command-message>plan</command-message><command-args>Example</command-args>\n```",
        "> <local-command-stdout>Example</local-command-stdout>",
        "    <command-name>/plan</command-name><command-message>plan</command-message><command-args>Indented example</command-args>",
        "<local-command-stdout>Incomplete",
        "<command-name>/plan</command-name><command-args>Incomplete command</command-args>",
    ] {
        f.write(&[f.row("user", None, "user", json!(body))]);
        let parsed = reader::read(&f.file).unwrap();
        assert_eq!(parsed.events[0].kind, "user_message");
        assert_eq!(parsed.events[0].user_message.as_ref().unwrap().text, body);
    }
    f.write(&[f.row(
        "mixed",
        None,
        "user",
        json!(
            "<system-reminder>Automatic fixture</system-reminder>\nActual request\n  exact spaces"
        ),
    )]);
    let parsed = reader::read(&f.file).unwrap();
    let user = parsed.events[0].user_message.as_ref().unwrap();
    assert_eq!(user.text, "\nActual request\n  exact spaces");
    assert_eq!(user.context_blocks.len(), 1);
}

#[test]
fn discovers_multiple_projects_and_duplicate_native_ids_without_subagents() {
    let f = Fixture::new();
    f.complete();
    let duplicate = f.paths.projects.join("another/same.jsonl");
    fs::create_dir_all(duplicate.parent().unwrap()).unwrap();
    fs::copy(&f.file, &duplicate).unwrap();
    let child = f
        .paths
        .projects
        .join("project/native-fixture/subagents/child.jsonl");
    fs::create_dir_all(child.parent().unwrap()).unwrap();
    fs::copy(&f.file, child).unwrap();
    let all = list_claude_sessions(&f.paths, 100, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, all[1].id);
    assert_ne!(all[0].session_key, all[1].session_key);
    assert!(all.iter().all(|s| s.status == "recent"));
    assert!(list_claude_sessions(&f.paths, 100, Some("not-present"))
        .unwrap()
        .is_empty());
    assert!(resolve(&f.paths, "../../secret").is_err());
    let absent = ClaudePaths {
        projects: f.root.join("absent"),
        config: f.root.clone(),
    };
    assert!(!absent.available());
    assert!(list_claude_sessions(&absent, 100, None).unwrap().is_empty());
}

#[test]
fn preserves_tools_text_plan_and_native_metadata_with_bounded_paging() {
    let f = Fixture::new();
    let mut rows = vec![f.row("u1", None, "user", json!("Start"))];
    let mut parent = "u1".to_string();
    for i in 0..90 {
        let aid = format!("a{i}");
        let uid = format!("r{i}");
        let call = format!("call{i}");
        rows.push(f.row(&aid,Some(&parent),"assistant",json!([{"type":"tool_use","id":call,"name":"Read","input":{"file_path":"/workspace/AGENTS.md"}}])));
        rows.push(f.row(&uid,Some(&aid),"user",json!([{"type":"tool_result","tool_use_id":call,"content":"# AGENTS.md\nExample instructions","is_error":i==1}])));
        parent = uid;
    }
    rows.push(f.row("plan",Some(&parent),"assistant",json!([{"type":"tool_use","id":"plan-call","name":"ExitPlanMode","input":{"plan":"# Example plan\nDo work"}}])));
    rows.push(f.row(
        "plan-result",
        Some("plan"),
        "user",
        json!([{"type":"tool_result","tool_use_id":"plan-call","content":"approved"}]),
    ));
    rows.push(f.row(
        "final",
        Some("plan-result"),
        "assistant",
        json!([{"type":"text","text":"Done <oai-mem-citation>internal</oai-mem-citation>"}]),
    ));
    rows.push(json!({"type":"cost-state","sessionId":"native-fixture","totalCostUSD":0}));
    f.write(&rows);
    let parsed = reader::read(&f.file).unwrap();
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    assert_eq!(
        parsed
            .events
            .iter()
            .filter(|e| e.kind == "tool_call")
            .count(),
        90
    );
    assert_eq!(
        parsed
            .events
            .iter()
            .filter(|e| e.status.as_deref() == Some("failed"))
            .count(),
        1
    );
    assert_eq!(
        parsed
            .events
            .iter()
            .filter(|e| e.kind == "user_message")
            .count(),
        1
    );
    assert!(parsed.events.iter().any(|e| e.kind == "plan"));
    let mut before = None;
    let mut event_ids = Vec::new();
    loop {
        let page = claude_session_detail(
            &f.paths,
            &ClaudeDetailRequest {
                session_key: f.key(),
                limit: Some(20),
                before,
            },
        )
        .unwrap();
        assert!(page.events.len() <= 20);
        event_ids.extend(page.events.iter().map(|e| e.id.clone()));
        if !page.has_more {
            break;
        }
        before = page.before_cursor;
    }
    assert_eq!(event_ids.len(), parsed.events.len());
    event_ids.sort();
    event_ids.dedup();
    assert_eq!(event_ids.len(), parsed.events.len());
}

#[test]
fn cache_recovers_partial_tail_truncation_replacement_and_keeps_unknown_records() {
    let f = Fixture::new();
    f.complete();
    let first = reader::read(&f.file).unwrap();
    assert_eq!(first.events.len(), 2);
    let tail = json!({"type":"custom-title","sessionId":"native-fixture","customTitle":"Renamed"})
        .to_string();
    let half = tail.len() / 2;
    fs::OpenOptions::new()
        .append(true)
        .open(&f.file)
        .unwrap()
        .write_all(&tail.as_bytes()[..half])
        .unwrap();
    let partial = reader::read(&f.file).unwrap();
    assert_eq!(partial.events.len(), 2);
    assert!(!partial.issues.is_empty());
    let mut file = fs::OpenOptions::new().append(true).open(&f.file).unwrap();
    writeln!(file, "{}", &tail[half..]).unwrap();
    let complete = reader::read(&f.file).unwrap();
    assert!(complete.issues.is_empty());
    assert_eq!(complete.title, "Renamed");
    f.append(&json!({"type":"future-record","sessionId":"native-fixture","example":"visible"}));
    let unknown = reader::read(&f.file).unwrap();
    assert_eq!(unknown.events.last().unwrap().kind, "unknown");
    assert!(unknown.issues.is_empty());
    assert!(!unknown.warnings.is_empty());
    f.complete();
    assert!(reader::read(&f.file).unwrap().issues.is_empty());
    fs::remove_file(&f.file).unwrap();
    f.write(&[f.row("new", None, "user", json!("Replacement"))]);
    assert_eq!(reader::read(&f.file).unwrap().title, "Replacement");
    fs::OpenOptions::new()
        .append(true)
        .open(&f.file)
        .unwrap()
        .write_all(b"broken\n")
        .unwrap();
    assert!(!reader::read(&f.file).unwrap().issues.is_empty());
}

#[test]
fn follows_rewound_branch_and_never_reports_tool_failure_as_terminal_failure() {
    let f = Fixture::new();
    f.write(&[
        f.row("u1", None, "user", json!("start")),
        f.row(
            "a1",
            Some("u1"),
            "assistant",
            json!([{"type":"text","text":"abandoned"}]),
        ),
        f.row("u2", Some("a1"), "user", json!("abandoned question")),
        f.row(
            "new",
            Some("u1"),
            "assistant",
            json!([{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"false"}}]),
        ),
        f.row(
            "tr",
            Some("new"),
            "user",
            json!([{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"exit 1"}]),
        ),
    ]);
    let parsed = reader::read(&f.file).unwrap();
    assert!(!parsed
        .events
        .iter()
        .any(|e| e.text.as_deref().is_some_and(|s| s.contains("abandoned"))));
    assert!(parsed.native_events.is_empty());
    assert!(parsed.turn_open);
}

#[test]
fn explicit_question_waits_for_result_and_does_not_infer_questions_from_prose() {
    let f = Fixture::new();
    f.write(&[f.row("u1",None,"user",json!("start")),f.row("q",Some("u1"),"assistant",json!([{"type":"tool_use","id":"question","name":"AskUserQuestion","input":{"questions":[{"question":"Choose a color","options":[{"label":"Blue"},{"label":"Green"}]}]}}]))]);
    let waiting = reader::read(&f.file).unwrap();
    assert_eq!(waiting.native_events.len(), 1);
    assert_eq!(waiting.native_events[0].kind, "reply_needed");
    assert!(waiting.native_events[0].body.contains("Blue"));
    f.append(&f.row(
        "answered",
        Some("q"),
        "user",
        json!([{"type":"tool_result","tool_use_id":"question","content":"Blue"}]),
    ));
    assert!(reader::read(&f.file).unwrap().native_events.is_empty());
    f.append(&f.row(
        "a1",
        Some("answered"),
        "assistant",
        json!([{"type":"text","text":"Which color?"}]),
    ));
    assert!(reader::read(&f.file)
        .unwrap()
        .native_events
        .iter()
        .all(|e| e.kind == "completion"));
}

#[test]
fn unfinished_turn_with_verified_inactive_owner_can_be_managed() {
    let f = Fixture::new();
    f.write(&[f.row("u1", None, "user", json!("Unfinished request"))]);
    let summary = claude_session_summary(&f.paths, &f.key()).unwrap();
    assert_eq!(summary.status, "unknown");
    assert!(summary.can_rename && summary.can_delete);
    assert!(rename_claude_session(&f.paths, &f.key(), "Renamed").is_ok());
    assert!(preview_claude_delete(&f.paths, &f.key()).is_ok());
}

#[test]
fn rename_preserves_native_records_and_delete_ignores_changed_fingerprint() {
    let f = Fixture::new();
    f.complete();
    let sibling = f.file.with_file_name("other.jsonl");
    fs::copy(&f.file, &sibling).unwrap();
    let original = fs::read(&f.file).unwrap();
    let renamed = rename_claude_session(&f.paths, &f.key(), "A new title").unwrap();
    assert_eq!(renamed.title, "A new title");
    assert!(fs::read(&f.file).unwrap().starts_with(&original));
    assert_eq!(fs::read(&sibling).unwrap(), original);
    let preview = preview_claude_delete(&f.paths, &f.key()).unwrap();
    f.append(
        &json!({"type":"custom-title","sessionId":"native-fixture","customTitle":"Changed again"}),
    );
    let result = execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: f.key(),
            confirmed: true,
            fingerprint: preview.fingerprint,
        },
    )
    .unwrap();
    assert!(result.deleted);
    assert!(!f.file.exists());
    assert!(sibling.exists());
    assert!(f.root.join("workspace").exists());
}

#[test]
fn warnings_allow_management_but_links_and_identity_conflicts_do_not() {
    let f = Fixture::new();
    f.complete();
    f.append(&json!({"type":"future","sessionId":"native-fixture"}));
    assert!(preview_claude_delete(&f.paths, &f.key()).is_ok());
    assert!(rename_claude_session(&f.paths, &f.key(), "Renamed").is_ok());
    f.complete();
    f.append(&json!({"type":"custom-title","sessionId":"other-id","customTitle":"wrong"}));
    assert!(rename_claude_session(&f.paths, &f.key(), "blocked").is_err());
    #[cfg(unix)]
    {
        fs::remove_file(&f.file).unwrap();
        let target = f.root.join("outside.jsonl");
        fs::write(&target, "protected").unwrap();
        std::os::unix::fs::symlink(&target, &f.file).unwrap();
        assert!(resolve(&f.paths, &f.key()).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "protected");
    }
}

#[test]
fn preferred_identity_and_version_allow_rename_delete_and_notification_scans() {
    for camel in [Some("native-fixture"), Some(""), None] {
        let f = Fixture::new();
        let mut rows = vec![
            f.row("u1", None, "user", json!("Start")),
            f.row(
                "a1",
                Some("u1"),
                "assistant",
                json!([{"type":"text","text":"Finished"}]),
            ),
        ];
        for row in &mut rows {
            if let Some(id) = camel {
                row["sessionId"] = json!(id);
                row["version"] = json!(if id.is_empty() { "" } else { "2.1.284" });
            } else {
                row.as_object_mut().unwrap().remove("sessionId");
                row.as_object_mut().unwrap().remove("version");
            }
            row["session_id"] = json!(if camel == Some("native-fixture") {
                "different-fixture"
            } else {
                "native-fixture"
            });
            row["claude_code_version"] = json!("2.1.285");
        }
        f.write(&rows);
        let parsed = reader::read(&f.file).unwrap();
        assert!(parsed.issues.is_empty());
        assert_eq!(parsed.id, "native-fixture");
        assert_eq!(
            parsed.version,
            if camel == Some("native-fixture") {
                "2.1.284"
            } else {
                "2.1.285"
            }
        );
        assert_eq!(
            parsed.warnings.len(),
            if camel == Some("native-fixture") {
                2
            } else {
                0
            }
        );
        let scan = notification_snapshots(&f.paths).unwrap();
        assert_eq!(scan.errors, 0);
        assert_eq!(scan.streams[0].events.len(), 1);
        let summary = claude_session_summary(&f.paths, &f.key()).unwrap();
        assert!(summary.can_rename && summary.can_delete);
        assert_eq!(
            rename_claude_session(&f.paths, &f.key(), "Renamed")
                .unwrap()
                .title,
            "Renamed"
        );
        let plan = preview_claude_delete(&f.paths, &f.key()).unwrap();
        let result = execute_claude_delete(
            &f.paths,
            ClaudeDeleteRequest {
                session_key: f.key(),
                confirmed: true,
                fingerprint: plan.fingerprint,
            },
        )
        .unwrap();
        assert!(result.deleted);
        assert!(!f.file.exists());
    }
}

#[test]
fn cross_record_conflicts_block_rename_and_notifications_but_allow_deletion() {
    let f = Fixture::new();
    f.complete();
    f.append(&json!({"type":"system","subtype":"init","sessionId":"other-fixture","session_id":"native-fixture"}));
    assert!(reader::read(&f.file)
        .unwrap()
        .issues
        .iter()
        .any(|issue| issue.contains("会话身份冲突")));
    assert!(rename_claude_session(&f.paths, &f.key(), "Blocked").is_err());
    assert!(preview_claude_delete(&f.paths, &f.key()).is_ok());
    assert!(notification_snapshots(&f.paths).unwrap().streams.is_empty());
    assert!(f.file.exists());
}

#[test]
fn embedded_images_load_lazily_and_invalid_ids_are_rejected() {
    let f = Fixture::new();
    let data="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j8ioAAAAASUVORK5CYII=";
    f.write(&[f.row("u1",None,"user",json!([{"type":"text","text":"Example image"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":data}}]))]);
    let detail = claude_session_detail(
        &f.paths,
        &ClaudeDetailRequest {
            session_key: f.key(),
            before: None,
            limit: None,
        },
    )
    .unwrap();
    let user = detail.events[0].user_message.as_ref().unwrap();
    assert_eq!(user.attachments.len(), 1);
    assert!(!serde_json::to_string(&detail).unwrap().contains(data));
    let mut request = crate::user_message::SessionAttachmentRequest {
        root_thread_id: None,
        provider: crate::services::sessions::SessionProvider::Claude,
        session_key: f.key(),
        message_id: user.id.clone(),
        attachment_id: user.attachments[0].id.clone(),
    };
    assert_eq!(
        read_attachment(&f.paths, &request).unwrap().mime_type,
        "image/png"
    );
    request.attachment_id = "../../outside".into();
    assert!(read_attachment(&f.paths, &request).is_err());
}

#[test]
fn batch_delete_has_partial_results_and_never_removes_an_unselected_sibling() {
    use crate::services::sessions::*;
    let f = Fixture::new();
    f.complete();
    let owned = f.file.with_extension("");
    fs::create_dir(&owned).unwrap();
    fs::write(owned.join("unknown.fixture"), "synthetic owned content").unwrap();
    let single = preview_claude_delete(&f.paths, &f.key()).unwrap();
    let sibling = f.file.with_file_name("keep.jsonl");
    fs::copy(&f.file, &sibling).unwrap();
    let cases = SessionUseCases {
        platform: crate::platform::PlatformPaths::desktop_current(),
        codex: crate::codex::CodexPaths::new(f.root.join("codex")),
        grok: crate::grok::GrokPaths::default_for_user(),
        claude: f.paths.clone(),
        antigravity: crate::antigravity::sessions::AntigravityPaths::offline(
            f.root.join("antigravity"),
        ),
        dsh: crate::dsh::DshPaths {
            home: f.root.join("dsh"),
        },
    };
    let unknown = format!("claude:{}", "f".repeat(64));
    let preview = cases
        .bulk_preview(SessionBatchRequest {
            include_descendants: false,
            provider: SessionProvider::Claude,
            operation: SessionOperation::Delete,
            session_keys: vec![f.key(), unknown.clone()],
        })
        .unwrap();
    assert!(preview.items[0].allowed);
    assert_eq!(
        preview.items[0].warnings,
        single.warnings.unwrap_or_default()
    );
    assert!(!preview.items[1].allowed);
    let result = cases
        .bulk_execute(SessionBatchExecuteRequest {
            include_descendants: false,
            provider: SessionProvider::Claude,
            operation: SessionOperation::Delete,
            confirmed: true,
            items: vec![
                SessionBatchSelection {
                    descendants: vec![],
                    session_key: f.key(),
                    fingerprint: preview.items[0].fingerprint.clone().unwrap(),
                },
                SessionBatchSelection {
                    descendants: vec![],
                    session_key: unknown,
                    fingerprint: "invalid".into(),
                },
            ],
        })
        .unwrap();
    assert_eq!(result.items[0].status, "succeeded");
    assert_eq!(result.items[1].status, "failed");
    assert!(sibling.exists());
}

#[test]
fn claude_notifications_baseline_dedupe_and_terminal_evidence_survive_restart() {
    use crate::{
        config::Config,
        db::PanelDb,
        native_probe::{NativeProvider, NativeScan},
    };
    let f = Fixture::new();
    f.complete();
    let mut config = Config::default();
    config.probe.notifications.enabled = true;
    let dbpath = f.root.join("panel.sqlite");
    let db = PanelDb::open(&dbpath).unwrap();
    let scan = notification_snapshots(&f.paths).unwrap();
    assert_eq!(
        db.stage_native_notifications(NativeProvider::Claude, &scan, &config)
            .unwrap(),
        0
    );
    let mut user = f.row("next", Some("a1"), "user", json!("Next request"));
    user["timestamp"] = json!((chrono::Utc::now() + chrono::Duration::seconds(1)).to_rfc3339());
    f.append(&user);
    let mut final_row = f.row(
        "done",
        Some("next"),
        "assistant",
        json!([{"type":"text","text":"Finished"}]),
    );
    final_row["timestamp"] = user["timestamp"].clone();
    f.append(&final_row);
    let scan = notification_snapshots(&f.paths).unwrap();
    assert_eq!(
        db.stage_native_notifications(NativeProvider::Claude, &scan, &config)
            .unwrap(),
        1
    );
    let pending = db.pending_native_deliveries(10).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(db.claim_native_delivery(&pending[0].event_key).unwrap());
    db.finish_native_delivery(&pending[0], json!({"sent":true}))
        .unwrap();
    drop(db);
    let db = PanelDb::open(&dbpath).unwrap();
    assert_eq!(
        db.stage_native_notifications(NativeProvider::Claude, &scan, &config)
            .unwrap(),
        0
    );
    assert!(db.pending_native_deliveries(10).unwrap().is_empty());
    let mut failure = json!({"type":"result","sessionId":"native-fixture","subtype":"error_max_turns","is_error":true,"errors":["Terminal fixture failure"],"timestamp":user["timestamp"]});
    user["uuid"] = json!("failure-turn");
    user["parentUuid"] = json!("done");
    f.append(&user);
    failure["timestamp"] = user["timestamp"].clone();
    f.append(&failure);
    assert!(notification_snapshots(&f.paths).unwrap().streams[0]
        .events
        .iter()
        .any(|e| e.kind == "failure"));
    config.probe.notifications.notify_claude = false;
    assert_eq!(
        db.stage_native_notifications(NativeProvider::Claude, &NativeScan::default(), &config)
            .unwrap(),
        0
    );
}

#[test]
fn native_saved_plans_and_bound_permission_requests_are_read_without_guessing() {
    let f = Fixture::new();
    let user = f.row("u1", None, "user", json!("Start"));
    let call = f.row(
        "a1",
        Some("u1"),
        "assistant",
        json!([{"type":"tool_use","id":"call","name":"Bash","input":{"command":"pwd"}}]),
    );
    let permission = json!({"type":"permission_request","request_id":"approval","tool_use_id":"call","tool_name":"Bash","description":"Allow this command","sessionId":"native-fixture","timestamp":"2026-09-29T00:00:01Z"});
    f.write(&[user, call, permission.clone()]);
    let waiting = reader::read(&f.file).unwrap();
    assert!(waiting.issues.is_empty());
    assert_eq!(waiting.native_events.len(), 1);
    assert_eq!(waiting.native_events[0].kind, "authorization");
    f.append(&json!({"type":"permission_response","request_id":"approval","subtype":"error","sessionId":"native-fixture"}));
    assert!(reader::read(&f.file).unwrap().native_events.is_empty());
    f.append(&json!({"type":"attachment","uuid":"plan","sessionId":"native-fixture","attachment":{"type":"plan_file_reference","planContent":"# Saved plan\nKeep content"}}));
    let p = reader::read(&f.file).unwrap();
    assert_eq!(p.events.last().unwrap().kind, "plan");
    assert_eq!(
        p.events.last().unwrap().text.as_deref(),
        Some("# Saved plan\nKeep content")
    );
    let mut unbound = permission;
    unbound["tool_use_id"] = json!("another-call");
    f.append(&unbound);
    assert!(reader::read(&f.file).unwrap().native_events.is_empty());
}

#[test]
fn oversized_unrelated_file_does_not_hide_readable_sessions_and_cache_is_reused() {
    let f = Fixture::new();
    f.complete();
    let first = reader::read(&f.file).unwrap();
    let second = reader::read(&f.file).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    let huge = f.file.with_file_name("oversized.jsonl");
    fs::File::create(huge)
        .unwrap()
        .set_len(512 * 1024 * 1024 + 1)
        .unwrap();
    let all = list_claude_sessions(&f.paths, 100, None).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|s| s.id == "native-fixture" && s.can_rename));
    let blocked = all.iter().find(|s| s.id == "oversized").unwrap();
    assert!(blocked.read_error.is_some());
    assert!(blocked.can_delete);
    assert_eq!(resolve(&f.paths, &f.key()).unwrap().1.id, "native-fixture");
}

#[test]
fn claude_summary_list_caches_many_transcripts_and_refreshes_one_append() {
    const SESSION_COUNT: usize = 70;

    let f = Fixture::new();
    let write_rows = |path: &Path, rows: &[Value]| {
        fs::write(
            path,
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
    };
    let mut sessions = Vec::with_capacity(SESSION_COUNT);
    for index in 0..SESSION_COUNT {
        let id = format!("cache-fixture-{index:03}");
        let path = f.paths.projects.join("project").join(format!("{id}.jsonl"));
        let user_id = format!("{id}-user");
        let assistant_id = format!("{id}-assistant");
        let mut user = f.row(
            &user_id,
            None,
            "user",
            json!(format!("Initial request {id}")),
        );
        user["sessionId"] = json!(id.as_str());
        let mut assistant = f.row(
            &assistant_id,
            Some(&user_id),
            "assistant",
            json!([{"type":"text","text":format!("Initial response {id}")}]),
        );
        assistant["sessionId"] = json!(id.as_str());
        write_rows(&path, &[user, assistant]);
        sessions.push((id, path, assistant_id));
    }

    reader::BODY_RECORDS.with(|records| records.set(0));
    let first = list_claude_sessions(&f.paths, 100, None).unwrap();
    assert_eq!(first.len(), SESSION_COUNT);
    let first_reads = reader::BODY_RECORDS.with(|records| records.get());
    assert_eq!(first_reads, SESSION_COUNT * 2);

    let target_id = &sessions.last().unwrap().0;
    let initial_title = format!("Initial request {target_id}");
    let initial_last_message = format!("Initial response {target_id}");
    let first_target = first.iter().find(|row| &row.id == target_id).unwrap();
    assert_eq!(first_target.title, initial_title);
    assert_eq!(first_target.message_count, 2);
    assert_eq!(
        first_target.last_message.as_deref(),
        Some(initial_last_message.as_str())
    );
    assert_eq!(first_target.status, "recent");

    let second = list_claude_sessions(&f.paths, 100, None).unwrap();
    assert_eq!(
        reader::BODY_RECORDS.with(|records| records.get()),
        first_reads,
        "a repeated list must use the lightweight summary cache"
    );
    assert_eq!(second.len(), SESSION_COUNT);

    let (target_id, target_path, last_assistant_id) = sessions.last().unwrap();
    let follow_up_id = format!("{target_id}-follow-up");
    let mut follow_up = f.row(
        &follow_up_id,
        Some(last_assistant_id),
        "user",
        json!(format!("Follow-up request {target_id}")),
    );
    follow_up["sessionId"] = json!(target_id.as_str());
    follow_up["timestamp"] = json!("2026-09-30T00:00:00Z");
    let custom_title = json!({
        "type": "custom-title",
        "sessionId": target_id,
        "customTitle": "Refreshed title"
    });
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(target_path)
        .unwrap();
    file.write_all(format!("{follow_up}\n{custom_title}\n").as_bytes())
        .unwrap();

    let updated = list_claude_sessions(&f.paths, 100, None).unwrap();
    let reads_after_update = reader::BODY_RECORDS.with(|records| records.get());
    let update_reads = reads_after_update - first_reads;
    assert!(
        matches!(update_reads, 2 | 4),
        "only the changed transcript should be parsed (2 appended records, or 4 if its body cache entry was evicted); read {update_reads}"
    );
    assert_eq!(updated.len(), SESSION_COUNT);
    let updated_target = updated.iter().find(|row| row.id == *target_id).unwrap();
    let expected_last_message = format!("Follow-up request {target_id}");
    assert_eq!(updated_target.title, "Refreshed title");
    assert_eq!(updated_target.message_count, 3);
    assert_eq!(
        updated_target.last_message.as_deref(),
        Some(expected_last_message.as_str())
    );
    assert_eq!(updated_target.status, "unknown");

    for before in first.iter().filter(|row| row.id != *target_id) {
        let after = updated.iter().find(|row| row.id == before.id).unwrap();
        assert_eq!(after.title, before.title);
        assert_eq!(after.message_count, before.message_count);
        assert_eq!(after.last_message, before.last_message);
        assert_eq!(after.status, before.status);
        assert_eq!(after.updated_at, before.updated_at);
    }

    let detail = claude_session_detail(
        &f.paths,
        &ClaudeDetailRequest {
            session_key: updated_target.session_key.clone(),
            limit: None,
            before: None,
        },
    )
    .unwrap();
    assert_eq!(detail.summary.title, updated_target.title);
    assert_eq!(detail.summary.message_count, updated_target.message_count);
    assert_eq!(detail.summary.last_message, updated_target.last_message);
    assert_eq!(detail.summary.status, updated_target.status);
    assert_eq!(
        detail.events.last().and_then(|event| event.text.as_deref()),
        Some(expected_last_message.as_str())
    );
}

#[cfg(unix)]
#[test]
fn claude_summary_cache_revalidates_rewrites_truncation_and_atomic_replacement() {
    use std::os::unix::fs::MetadataExt;

    let f = Fixture::new();
    f.complete();
    let key = f.key();
    let list_summary = || {
        list_claude_sessions(&f.paths, 100, None)
            .unwrap()
            .into_iter()
            .find(|summary| summary.session_key == key)
            .expect("fixture summary")
    };
    let check = |listed: &ClaudeSessionSummary,
                 title: &str,
                 message_count: usize,
                 last_message: &str,
                 status: &str,
                 can_manage: bool| {
        assert_eq!(listed.title, title);
        assert_eq!(listed.message_count, message_count);
        assert_eq!(listed.last_message.as_deref(), Some(last_message));
        assert_eq!(listed.status, status);
        assert_eq!(listed.can_rename, can_manage);
        assert!(listed.can_delete);
        assert_eq!(listed.rename_block_reason.is_some(), !can_manage);
        assert!(listed.delete_block_reason.is_none());

        let detail = claude_session_detail(
            &f.paths,
            &ClaudeDetailRequest {
                session_key: key.clone(),
                limit: None,
                before: None,
            },
        )
        .unwrap();
        assert_eq!(detail.summary.id, listed.id);
        assert_eq!(detail.summary.session_key, listed.session_key);
        assert_eq!(detail.summary.title, listed.title);
        assert_eq!(detail.summary.updated_at, listed.updated_at);
        assert_eq!(detail.summary.message_count, listed.message_count);
        assert_eq!(detail.summary.last_message, listed.last_message);
        assert_eq!(detail.summary.status, listed.status);
        assert_eq!(detail.summary.can_rename, listed.can_rename);
        assert_eq!(detail.summary.can_delete, listed.can_delete);
        assert_eq!(
            detail.summary.rename_block_reason,
            listed.rename_block_reason
        );
        assert_eq!(
            detail.summary.delete_block_reason,
            listed.delete_block_reason
        );
        assert_eq!(detail.summary.read_error, listed.read_error);
        assert_eq!(detail.summary.read_warning, listed.read_warning);
        assert_eq!(detail.total_events, message_count);
        assert_eq!(
            detail.events.last().and_then(|event| event.text.as_deref()),
            Some(last_message)
        );

        if can_manage {
            assert!(preview_claude_delete(&f.paths, &key).is_ok());
        } else {
            assert!(rename_claude_session(&f.paths, &key, "Blocked").is_err());
            assert!(preview_claude_delete(&f.paths, &key).is_ok());
        }
    };

    let initial = list_summary();
    check(
        &initial,
        "Example request",
        2,
        "Example complete",
        "recent",
        true,
    );

    let before_rewrite = fs::metadata(&f.file).unwrap();
    let original = fs::read(&f.file).unwrap();
    let rewritten = String::from_utf8(original.clone()).unwrap().replacen(
        "Example request",
        "Changed request",
        1,
    );
    assert_ne!(rewritten.as_bytes(), original.as_slice());
    assert_eq!(rewritten.len() as u64, before_rewrite.len());
    fs::write(&f.file, rewritten.as_bytes()).unwrap();
    fs::File::options()
        .write(true)
        .open(&f.file)
        .unwrap()
        .set_modified(before_rewrite.modified().unwrap())
        .unwrap();
    let after_rewrite = fs::metadata(&f.file).unwrap();
    assert_eq!(after_rewrite.len(), before_rewrite.len());
    assert_eq!(
        after_rewrite.modified().unwrap(),
        before_rewrite.modified().unwrap()
    );
    assert_eq!(
        (after_rewrite.dev(), after_rewrite.ino()),
        (before_rewrite.dev(), before_rewrite.ino())
    );
    assert_ne!(
        (after_rewrite.ctime(), after_rewrite.ctime_nsec()),
        (before_rewrite.ctime(), before_rewrite.ctime_nsec())
    );
    let rewritten_summary = list_summary();
    check(
        &rewritten_summary,
        "Changed request",
        2,
        "Example complete",
        "recent",
        true,
    );

    let before_replace = fs::metadata(&f.file).unwrap();
    let replaced = String::from_utf8(fs::read(&f.file).unwrap())
        .unwrap()
        .replacen("Changed request", "Swapped request", 1);
    assert_eq!(replaced.len() as u64, before_replace.len());
    let replacement = f
        .file
        .with_file_name(format!(".atomic-replacement-{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&replacement, replaced.as_bytes()).unwrap();
    fs::File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(before_replace.modified().unwrap())
        .unwrap();
    let replacement_metadata = fs::metadata(&replacement).unwrap();
    assert_eq!(replacement_metadata.len(), before_replace.len());
    assert_eq!(
        replacement_metadata.modified().unwrap(),
        before_replace.modified().unwrap()
    );
    assert_ne!(
        (replacement_metadata.dev(), replacement_metadata.ino()),
        (before_replace.dev(), before_replace.ino())
    );
    fs::rename(&replacement, &f.file).unwrap();
    let after_replace = fs::metadata(&f.file).unwrap();
    assert_eq!(after_replace.len(), before_replace.len());
    assert_eq!(
        after_replace.modified().unwrap(),
        before_replace.modified().unwrap()
    );
    assert_ne!(
        (after_replace.dev(), after_replace.ino()),
        (before_replace.dev(), before_replace.ino())
    );
    let replaced_summary = list_summary();
    check(
        &replaced_summary,
        "Swapped request",
        2,
        "Example complete",
        "recent",
        true,
    );

    let bytes = fs::read(&f.file).unwrap();
    let first_record_len = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    fs::File::options()
        .write(true)
        .open(&f.file)
        .unwrap()
        .set_len(first_record_len as u64)
        .unwrap();
    let truncated_summary = list_summary();
    check(
        &truncated_summary,
        "Swapped request",
        1,
        "Swapped request",
        "unknown",
        true,
    );
}

#[test]
fn reads_stream_init_aliases_tool_references_and_empty_result_falls_back() {
    let f = Fixture::new();
    f.write(&[
        json!({
            "type": "system",
            "subtype": "init",
            "uuid": "init",
            "session_id": "snake-session",
            "claude_code_version": "2.1.284",
            "cwd": f.root.join("workspace"),
            "timestamp": "2026-09-29T00:00:00Z"
        }),
        json!({
            "type": "user",
            "uuid": "u1",
            "session_id": "snake-session",
            "cwd": f.root.join("workspace"),
            "claude_code_version": "2.1.284",
            "timestamp": "2026-09-29T00:00:01Z",
            "message": {"role":"user","content":"Start","stop_reason":""}
        }),
        json!({
            "type": "assistant",
            "uuid": "a1",
            "parentUuid": "u1",
            "session_id": "snake-session",
            "timestamp": "2026-09-29T00:00:02Z",
            "message": {
                "role":"assistant",
                "content":[
                    {"type":"tool_reference","tool_name":"mcp__fixture__search"},
                    {"type":"text","text":"Fallback answer"}
                ],
                "stop_reason":"end_turn"
            }
        }),
        json!({
            "type": "result",
            "uuid": "result",
            "session_id": "snake-session",
            "subtype": "success",
            "is_error": false,
            "result": "",
            "timestamp": "2026-09-29T00:00:03Z"
        }),
    ]);
    let parsed = reader::read(&f.file).unwrap();
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.id, "snake-session");
    assert_eq!(parsed.version, "2.1.284");
    assert!(parsed.events.iter().any(|event| {
        event.kind == "tool_reference" && event.role.as_deref() == Some("mcp__fixture__search")
    }));
    assert_eq!(parsed.native_events.len(), 1);
    assert_eq!(parsed.native_events[0].body, "Fallback answer");
    assert!(!parsed.turn_open);
}

#[test]
fn success_result_does_not_finish_with_pending_tool_and_unknown_system_is_warning() {
    let f = Fixture::new();
    f.write(&[
        f.row("u1", None, "user", json!("Start")),
        f.row(
            "a1",
            Some("u1"),
            "assistant",
            json!([{"type":"tool_use","id":"call","name":"Search","input":{}}]),
        ),
        json!({
            "type":"result",
            "uuid":"result",
            "sessionId":"native-fixture",
            "subtype":"success",
            "is_error":false,
            "result":"Finished too early",
            "timestamp":"2026-09-29T00:00:01Z"
        }),
        json!({
            "type":"system",
            "subtype":"future_activity",
            "uuid":"future",
            "sessionId":"native-fixture",
            "timestamp":"2026-09-29T00:00:02Z"
        }),
    ]);
    let parsed = reader::read(&f.file).unwrap();
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    assert!(parsed
        .warnings
        .iter()
        .any(|warning| warning.contains("新格式系统活动")));
    assert!(parsed.turn_open);
    assert!(parsed.native_events.is_empty());
    let summary = list_claude_sessions(&f.paths, 10, None)
        .unwrap()
        .into_iter()
        .find(|summary| summary.id == "native-fixture")
        .expect("fixture summary");
    assert_eq!(summary.status, "unknown");
    assert!(summary.read_error.is_none());
    assert!(summary.read_warning.is_some());
    assert!(summary.can_rename && summary.can_delete);
    let scan = notification_snapshots(&f.paths).unwrap();
    assert_eq!(scan.errors, 0);
    assert_eq!(scan.streams.len(), 1);
}

#[test]
fn compatible_warnings_do_not_block_verified_inactive_management() {
    let f = Fixture::new();
    f.complete();
    for kind in [
        "mode",
        "permission-mode",
        "file-history-delta",
        "future-fixture-record",
    ] {
        f.append(&json!({"type":kind,"sessionId":"native-fixture"}));
    }
    f.append(&json!({"type":"system","subtype":"informational","session_id":"native-fixture"}));
    let detail = claude_session_detail(
        &f.paths,
        &ClaudeDetailRequest {
            session_key: f.key(),
            limit: Some(200),
            before: None,
        },
    )
    .unwrap();
    assert!(detail.summary.read_error.is_none());
    assert!(detail.summary.read_warning.is_some());
    assert_eq!(detail.summary.status, "recent");
    assert!(detail.summary.can_rename && detail.summary.can_delete);
    assert!(rename_claude_session(&f.paths, &f.key(), "Renamed").is_ok());
    assert!(preview_claude_delete(&f.paths, &f.key()).is_ok());
    let scan = notification_snapshots(&f.paths).unwrap();
    assert_eq!(scan.errors, 0);
    assert_eq!(scan.streams[0].events.len(), 1);
    assert_eq!(scan.streams[0].events[0].kind, "completion");
}

#[test]
fn auxiliary_api_metadata_is_not_displayed_or_treated_as_terminal() {
    let f = Fixture::new();
    f.complete();
    for kind in [
        "api-request-shape",
        "api-request-blob",
        "api-request",
        "agent-setting",
        "continued-in",
    ] {
        f.append(
            &json!({"type":kind,"sessionId":"native-fixture","data":"synthetic-internal-content"}),
        );
    }
    let parsed = reader::read(&f.file).unwrap();
    assert!(parsed.warnings.is_empty());
    assert!(!parsed.events.iter().any(|e| e
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("synthetic-internal-content")));
    assert_eq!(parsed.native_events.len(), 1);
}

fn owned_sidecars(f: &Fixture) -> PathBuf {
    let directory = f.file.with_extension("");
    fs::create_dir_all(directory.join("subagents")).unwrap();
    fs::create_dir_all(directory.join("tool-results")).unwrap();
    fs::write(directory.join("subagents/agent-fixture.jsonl"),format!("{}\n{}\n",json!({"type":"fork-context-ref","parentSessionId":"native-fixture"}),json!({"type":"user","sessionId":"child-fixture","uuid":"child-user","message":{"content":"Synthetic child"}}))).unwrap();
    fs::write(
        directory.join("subagents/agent-fixture.meta.json"),
        "{\"agentType\":\"fixture\"}",
    )
    .unwrap();
    fs::write(
        directory.join("tool-results/output.txt"),
        "synthetic tool result",
    )
    .unwrap();
    directory
}

#[test]
fn deletion_covers_verified_sidecars_without_following_continuations() {
    let f = Fixture::new();
    f.complete();
    let directory = owned_sidecars(&f);
    f.append(&json!({"type":"continued-in","sessionId":"native-fixture","continuationSessionId":"other-fixture"}));
    let other = f.file.with_file_name("other.jsonl");
    fs::write(&other, "synthetic unrelated").unwrap();
    let plan = preview_claude_delete(&f.paths, &f.key()).unwrap();
    assert_eq!(plan.file_count, 4);
    assert!(plan.paths.contains(&directory.canonicalize().unwrap()));
    execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: f.key(),
            confirmed: true,
            fingerprint: plan.fingerprint,
        },
    )
    .unwrap();
    assert!(!f.file.exists());
    assert!(!directory.exists());
    assert!(other.exists());
}

#[test]
fn dedicated_children_are_deleted_even_with_conflicting_content_identity() {
    for conflict in [false, true] {
        let f = Fixture::new();
        f.complete();
        let directory = owned_sidecars(&f);
        let child = directory.join("subagents/agent-fixture.jsonl");
        let mut rows = vec![
            json!({"type":"fork-context-ref","parentSessionId":"native-fixture"}),
            json!({"type":"user","sessionId":"child-fixture","session_id":"other-fixture"}),
            json!({"type":"assistant","sessionId":"","session_id":"child-fixture"}),
            json!({"type":"system","session_id":"child-fixture"}),
        ];
        if conflict {
            rows.push(
                json!({"type":"system","sessionId":"different-child","session_id":"child-fixture"}),
            );
        }
        fs::write(
            &child,
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let preview = preview_claude_delete(&f.paths, &f.key());
        let result = execute_claude_delete(
            &f.paths,
            ClaudeDeleteRequest {
                session_key: f.key(),
                confirmed: true,
                fingerprint: preview.unwrap().fingerprint,
            },
        )
        .unwrap();
        assert!(result.deleted);
        assert!(!directory.exists());
    }
}

#[test]
fn changed_sidecars_are_deleted_without_repreview() {
    let f = Fixture::new();
    f.complete();
    let directory = owned_sidecars(&f);
    let plan = preview_claude_delete(&f.paths, &f.key()).unwrap();
    fs::write(directory.join("tool-results/new.txt"), "fixture").unwrap();
    fs::write(
        directory.join("subagents/agent-fixture.jsonl"),
        "broken child",
    )
    .unwrap();
    let result = execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: f.key(),
            confirmed: true,
            fingerprint: plan.fingerprint,
        },
    )
    .unwrap();
    assert!(result.deleted);
    assert!(!directory.exists());
    assert!(!f.file.exists());
}

#[cfg(unix)]
#[test]
fn sidecar_links_are_unlinked_and_rename_delete_capabilities_are_independent() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    for hard in [false, true] {
        let f = Fixture::new();
        f.complete();
        let directory = owned_sidecars(&f);
        let external = f.root.join("external");
        fs::write(&external, "fixture").unwrap();
        let link = directory.join("tool-results/link");
        if hard {
            fs::hard_link(&external, &link).unwrap();
        } else {
            symlink(&external, &link).unwrap();
        }
        let result = execute_claude_delete(
            &f.paths,
            ClaudeDeleteRequest {
                session_key: f.key(),
                confirmed: true,
                fingerprint: String::new(),
            },
        )
        .unwrap();
        assert!(result.deleted);
        assert!(!directory.exists());
        assert!(external.exists());
    }
    let f = Fixture::new();
    f.complete();
    fs::set_permissions(&f.file, fs::Permissions::from_mode(0o400)).unwrap();
    let summary = claude_session_summary(&f.paths, &f.key()).unwrap();
    assert!(!summary.can_rename);
    assert!(summary.can_delete);
}

#[test]
fn renamed_title_is_a_complete_native_record_in_a_64_kib_tail_window() {
    use std::io::{Read, Seek, SeekFrom};
    let f = Fixture::new();
    f.complete();
    f.append(&json!({"type":"file-history-snapshot","snapshot":{"fixture":"x".repeat(80 * 1024)}}));
    let original = fs::read(&f.file).unwrap();
    let summary = rename_claude_session(&f.paths, &f.key(), "新的测试标题").unwrap();
    assert_eq!(summary.title, "新的测试标题");
    let all = fs::read(&f.file).unwrap();
    assert_eq!(&all[..original.len()], original);
    assert_eq!(
        all[original.len()..]
            .iter()
            .filter(|b| **b == b'\n')
            .count(),
        1
    );
    let mut file = fs::File::open(&f.file).unwrap();
    file.seek(SeekFrom::End(-65536)).unwrap();
    let mut tail = String::new();
    file.read_to_string(&mut tail).unwrap();
    let row: Value = serde_json::from_str(tail.lines().last().unwrap()).unwrap();
    assert_eq!(row["type"], "custom-title");
    assert_eq!(row["customTitle"], summary.title);
    assert!(all.ends_with(b"\n"));
}

#[test]
fn unknown_owned_content_empty_and_metadata_only_children_are_previewed_and_deleted() {
    let f = Fixture::new();
    f.complete();
    let directory = owned_sidecars(&f);
    fs::create_dir_all(directory.join("future/empty")).unwrap();
    fs::write(directory.join("future/opaque.bin"), [0, 1, 255]).unwrap();
    fs::write(directory.join("subagents/agent-empty.jsonl"), "").unwrap();
    fs::write(
        directory.join("subagents/agent-metadata.jsonl"),
        "{\"type\":\"progress\"}\n",
    )
    .unwrap();
    fs::write(
        directory.join("subagents/agent-empty.meta.json"),
        "{\"agentType\":\"fixture\"}",
    )
    .unwrap();
    let preview = preview_claude_delete(&f.paths, &f.key()).unwrap();
    assert_eq!(preview.file_count, 8);
    assert!(preview.paths.contains(&directory.canonicalize().unwrap()));
    assert!(preview.warnings.is_none());
    execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: f.key(),
            confirmed: true,
            fingerprint: preview.fingerprint,
        },
    )
    .unwrap();
    assert!(!directory.exists());
    assert!(list_claude_sessions(&f.paths, 200, None)
        .unwrap()
        .is_empty());
}

#[test]
fn metadata_conflicts_do_not_change_the_filename_owned_inventory() {
    for record in [
        json!({"parentSessionId":"other-parent"}),
        json!({"sessionId":"other-child"}),
        json!({"sessionId":123}),
    ] {
        let f = Fixture::new();
        f.complete();
        let directory = owned_sidecars(&f);
        fs::write(
            directory.join("subagents/agent-fixture.meta.json"),
            record.to_string(),
        )
        .unwrap();
        assert!(preview_claude_delete(&f.paths, &f.key()).is_ok());
        assert!(
            claude_session_summary(&f.paths, &f.key())
                .unwrap()
                .can_delete
        );
        assert!(f.file.exists());
    }
}

#[test]
fn active_dedicated_child_does_not_block_parent_deletion() {
    let f = Fixture::new();
    f.complete();
    owned_sidecars(&f);
    activity::test_owner(Some("child-fixture"), &f.root.join("child-worktree"));
    let summary = claude_session_summary(&f.paths, &f.key()).unwrap();
    let preview = preview_claude_delete(&f.paths, &f.key());
    let diagnostics = claude_ownership_diagnostics(&f.paths, &f.key()).unwrap();
    activity::test_owner(None, Path::new("/fixture"));
    assert!(summary.can_rename);
    assert!(summary.can_delete);
    assert!(preview.is_ok());
    let _ = diagnostics;
    assert!(f.file.exists());
}

#[cfg(unix)]
#[test]
fn special_files_and_filename_owned_directories_are_deleted() {
    let f = Fixture::new();
    f.complete();
    let directory = owned_sidecars(&f);
    let socket = directory.join("fixture.fifo");
    let name = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(preview_claude_delete(&f.paths, &f.key()).is_ok());
    fs::remove_file(socket).unwrap();
    let alias = f.file.with_file_name("unrelated.jsonl");
    fs::rename(&f.file, &alias).unwrap();
    fs::rename(directory, alias.with_extension("")).unwrap();
    let key = key_for(
        &f.paths.projects.canonicalize().unwrap(),
        &alias.canonicalize().unwrap(),
        "native-fixture",
    );
    let result = execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: key,
            confirmed: true,
            fingerprint: String::new(),
        },
    )
    .unwrap();
    assert!(result.deleted);
    assert!(!alias.exists());
    assert!(!alias.with_extension("").exists());
}

#[test]
fn empty_metadata_session_deletes_file_history_and_exact_shared_records() {
    let f = Fixture::new();
    f.write(&[
        json!({"type":"ai-title","sessionId":"native-fixture","aiTitle":"Empty"}),
        json!({"type":"agent-name","sessionId":"native-fixture"}),
    ]);
    let detail = claude_session_detail(
        &f.paths,
        &ClaudeDetailRequest {
            session_key: f.key(),
            limit: Some(10),
            before: None,
        },
    )
    .unwrap();
    assert!(detail.events.is_empty());
    assert!(detail.summary.read_error.is_none());
    assert!(detail.summary.can_delete);
    let side = f.paths.config.join("file-history/native-fixture");
    fs::create_dir_all(&side).unwrap();
    fs::write(side.join("fixture@v1"), "fixture").unwrap();
    for group in ["tasks", "session-env"] {
        let directory = f.paths.config.join(group).join("native-fixture");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("fixture"), "fixture").unwrap();
    }
    for directory in ["debug", "todos", "sessions"] {
        fs::create_dir_all(f.paths.config.join(directory)).unwrap();
    }
    fs::write(f.paths.config.join("debug/native-fixture.txt"), "fixture").unwrap();
    fs::write(
        f.paths
            .config
            .join("todos/native-fixture-agent-fixture.json"),
        "fixture",
    )
    .unwrap();
    fs::write(
        f.paths.config.join("todos/retained-agent-fixture.json"),
        "retained",
    )
    .unwrap();
    fs::write(
        f.paths.config.join("sessions/123.json"),
        json!({"sessionId":"native-fixture"}).to_string(),
    )
    .unwrap();
    fs::write(
        f.paths.config.join("sessions/456.json"),
        json!({"sessionId":"retained"}).to_string(),
    )
    .unwrap();
    let index = f.file.parent().unwrap().join("sessions-index.json");
    fs::write(
        &index,
        json!({"version":1,"entries":[{"sessionId":"native-fixture"},{"sessionId":"retained"}]})
            .to_string(),
    )
    .unwrap();
    fs::write(
        f.paths.config.join("history.jsonl"),
        "{\"sessionId\":\"native-fixture\"}\n{\"sessionId\":\"retained\"}\nmalformed\n",
    )
    .unwrap();
    let result = execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: f.key(),
            confirmed: true,
            fingerprint: String::new(),
        },
    )
    .unwrap();
    assert!(result.deleted);
    assert!(!side.exists());
    assert!(!f.file.exists());
    for path in [
        "tasks/native-fixture",
        "session-env/native-fixture",
        "debug/native-fixture.txt",
        "todos/native-fixture-agent-fixture.json",
        "sessions/123.json",
    ] {
        assert!(!f.paths.config.join(path).exists(), "{path}");
    }
    assert!(f.paths.config.join("sessions/456.json").exists());
    assert!(f
        .paths
        .config
        .join("todos/retained-agent-fixture.json")
        .exists());
    assert_eq!(
        serde_json::from_str::<Value>(&fs::read_to_string(index).unwrap()).unwrap(),
        json!({"version":1,"entries":[{"sessionId":"retained"}]})
    );
    assert_eq!(
        fs::read_to_string(f.paths.config.join("history.jsonl")).unwrap(),
        "{\"sessionId\":\"retained\"}\nmalformed\n"
    );
}

#[test]
fn empty_file_is_readable_and_missing_main_file_does_not_prevent_sidecar_deletion() {
    let f = Fixture::new();
    fs::write(&f.file, "").unwrap();
    let detail = claude_session_detail(
        &f.paths,
        &ClaudeDetailRequest {
            session_key: f.key(),
            limit: None,
            before: None,
        },
    )
    .unwrap();
    assert!(detail.events.is_empty());
    assert!(detail.summary.read_error.is_none());
    let preview = preview_claude_delete(&f.paths, &f.key()).unwrap();
    fs::remove_file(&f.file).unwrap();
    let side = f.paths.config.join("file-history/native-fixture");
    fs::create_dir_all(&side).unwrap();
    fs::write(side.join("fixture"), "fixture").unwrap();
    let result = execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: preview.session_key,
            confirmed: true,
            fingerprint: String::new(),
        },
    )
    .unwrap();
    assert!(result.deleted);
    assert!(!side.exists());
}

#[test]
fn assistant_display_phase_uses_native_stop_evidence_without_changing_notification_identity() {
    let f = Fixture::new();
    let user = f.row("u1", None, "user", json!("Fixture request"));
    let mut intermediate = f.row(
        "a1",
        Some("u1"),
        "assistant",
        json!([
            {"type":"thinking","thinking":"Reasoning fixture"},
            {"type":"text","text":"Temporary explanation"},
            {"type":"tool_use","id":"call-1","name":"Read","input":{"file_path":"/fixture/input"}}
        ]),
    );
    intermediate["message"]["stop_reason"] = json!("tool_use");
    let result = f.row(
        "r1",
        Some("a1"),
        "user",
        json!([{"type":"tool_result","tool_use_id":"call-1","content":"Synthetic output"}]),
    );
    let final_reply = f.row(
        "a2",
        Some("r1"),
        "assistant",
        json!([{"type":"text","text":"Final reply"}]),
    );
    f.write(&[user, intermediate, result, final_reply]);
    let parsed = reader::read(&f.file).unwrap();
    let texts: Vec<_> = parsed
        .events
        .iter()
        .filter(|e| e.kind == "assistant_message")
        .collect();
    assert_eq!(
        texts[0].assistant_phase,
        Some(ClaudeAssistantPhase::Intermediate)
    );
    assert_eq!(texts[1].assistant_phase, Some(ClaudeAssistantPhase::Final));
    assert!(parsed
        .events
        .iter()
        .all(|e| e.turn_id.as_deref() == Some("u1")));
    assert_eq!(parsed.native_events.len(), 1);
    assert_eq!(parsed.native_events[0].turn_id, "u1");
    assert_eq!(parsed.native_events[0].kind, "completion");
    assert_eq!(parsed.native_events[0].body, "Final reply");
}

#[test]
fn result_phase_updates_are_visible_to_pagination_without_guessing_across_context_boundaries() {
    for boundary in [false, true] {
        let f = Fixture::new();
        let mut assistant = f.row(
            "a1",
            Some("u1"),
            "assistant",
            json!([{"type":"text","text":"Candidate reply"}]),
        );
        assistant["message"]["stop_reason"] = Value::Null;
        f.write(&[
            f.row("u1", None, "user", json!("Fixture request")),
            assistant,
        ]);
        let initial = reader::read(&f.file).unwrap();
        assert_eq!(initial.events.last().unwrap().assistant_phase, None);
        if boundary {
            f.append(&json!({"type":"system","subtype":"compact_boundary","uuid":"context-boundary","content":"Context fixture"}));
        }
        f.append(&json!({"type":"result","session_id":"native-fixture","subtype":"success","is_error":false,"result":"Candidate reply","timestamp":"2026-09-29T00:00:02Z"}));
        let updated = reader::read(&f.file).unwrap();
        let reply = updated.events.iter().find(|e| e.id == "a1:0").unwrap();
        assert_eq!(
            reply.assistant_phase,
            if boundary {
                None
            } else {
                Some(ClaudeAssistantPhase::Final)
            }
        );
        assert_eq!(initial.events.last().unwrap().assistant_phase, None);
        if !boundary {
            let metadata = updated
                .history
                .metadata(Some(&initial.history.metadata(None).revision));
            assert!(metadata.changed_ids.iter().any(|id| id == "a1:0"));
        }
    }
    let old: ClaudeHistoryEvent =
        serde_json::from_value(json!({"id":"legacy","kind":"assistant_message"})).unwrap();
    assert_eq!(old.assistant_phase, None);
    assert!(serde_json::to_value(old)
        .unwrap()
        .get("assistantPhase")
        .is_none());
    let old_preview: ClaudeDeletePreview = serde_json::from_value(json!({"sessionKey":"fixture","id":"fixture","title":"Fixture","path":"/fixture/session.jsonl","fingerprint":"fixture","fileCount":1,"bytes":0})).unwrap();
    assert!(old_preview.warnings.is_none());
}

#[test]
fn split_assistant_records_use_native_message_identity_for_process_text() {
    for shared_identity in [true, false] {
        let f = Fixture::new();
        let mut explanation = f.row(
            "a1",
            Some("u1"),
            "assistant",
            json!([{"type":"text","text":"Process explanation"}]),
        );
        explanation["message"]["id"] = json!("message-1");
        explanation["message"]["stop_reason"] = Value::Null;
        f.write(&[
            f.row("u1", None, "user", json!("Fixture request")),
            explanation,
        ]);
        let initial = reader::read(&f.file).unwrap();
        let mut call = f.row(
            "a2",
            Some("a1"),
            "assistant",
            json!([{"type":"tool_use","id":"call-1","name":"Read","input":{}}]),
        );
        call["message"]["id"] = json!(if shared_identity {
            "message-1"
        } else {
            "message-2"
        });
        call["message"]["stop_reason"] = Value::Null;
        f.append(&call);
        let updated = reader::read(&f.file).unwrap();
        let explanation = updated
            .events
            .iter()
            .find(|event| event.id == "a1:0")
            .unwrap();
        assert_eq!(
            explanation.assistant_phase,
            shared_identity.then_some(ClaudeAssistantPhase::Intermediate)
        );
        assert_eq!(initial.events.last().unwrap().assistant_phase, None);
        assert!(updated.native_events.is_empty());
        if shared_identity {
            assert!(updated
                .history
                .metadata(Some(&initial.history.metadata(None).revision))
                .changed_ids
                .contains(&"a1:0".to_owned()));
        }
    }
}

#[test]
fn permission_resolution_and_failures_update_display_without_replacing_native_events() {
    let f = Fixture::new();
    let mut call = f.row(
        "a1",
        Some("u1"),
        "assistant",
        json!([{"type":"tool_use","id":"call-1","name":"Bash","input":{"command":"fixture"}}]),
    );
    call["message"]["stop_reason"] = json!("tool_use");
    f.write(&[f.row("u1", None, "user", json!("Fixture request")), call]);
    f.append(&json!({"type":"permission_request","uuid":"permission-1","sessionId":"native-fixture","tool_use_id":"call-1","request_id":"request-1","description":"Approve fixture","tool_name":"Bash","timestamp":"2026-09-29T00:00:01Z"}));
    let pending = reader::read(&f.file).unwrap();
    assert_eq!(
        pending
            .events
            .iter()
            .find(|e| e.kind == "permission_request")
            .unwrap()
            .status
            .as_deref(),
        Some("in_progress")
    );
    f.append(&json!({"type":"permission_response","request_id":"request-1"}));
    f.append(&json!({"type":"result","subtype":"error_during_execution","is_error":true,"errors":["Synthetic failure"],"timestamp":"2026-09-29T00:00:02Z"}));
    let failed = reader::read(&f.file).unwrap();
    assert_eq!(
        failed
            .events
            .iter()
            .find(|e| e.kind == "permission_request")
            .unwrap()
            .status
            .as_deref(),
        Some("completed")
    );
    assert_eq!(
        failed
            .events
            .iter()
            .find(|e| e.kind == "tool_call")
            .unwrap()
            .status
            .as_deref(),
        Some("cancelled")
    );
    assert_eq!(failed.events.last().unwrap().kind, "failure");
    assert_eq!(failed.native_events.len(), 1);
    assert_eq!(failed.native_events[0].turn_id, "u1");
    assert_eq!(failed.native_events[0].kind, "failure");
}

#[test]
fn empty_success_result_uses_this_turn_text_and_never_tool_use_or_cancelled_turns() {
    let f = Fixture::new();
    let user = f.row("u1", None, "user", json!("Fixture request"));
    let mut answer = f.row(
        "a1",
        Some("u1"),
        "assistant",
        json!([{"type":"text","text":"Fixture final reply"}]),
    );
    answer["message"]["stop_reason"] = Value::Null;
    f.write(&[user.clone(), answer.clone()]);
    assert!(reader::read(&f.file).unwrap().native_events.is_empty());
    let result = json!({"type":"result","session_id":"native-fixture","subtype":"success","is_error":false,"result":"","timestamp":"2026-09-29T00:00:02Z"});
    f.append(&result);
    assert_eq!(
        reader::read(&f.file).unwrap().native_events[0].body,
        "Fixture final reply"
    );
    for reason in ["tool_use", "pause_turn", "refusal"] {
        for scenario in ["implicit", "explicit", "override-completion"] {
            // Each behavior has its own native file identity. In-place writes
            // of tool_use/end_turn have equal lengths and can share filesystem
            // timestamps; cache rewrite behavior has separate regression tests.
            let f = Fixture::new();
            let user = f.row("u1", None, "user", json!("Fixture request"));
            let mut answer = f.row(
                "a1",
                Some("u1"),
                "assistant",
                json!([{"type":"text","text":"Fixture reply"}]),
            );
            let prior_terminal = scenario == "override-completion";
            answer["message"]["stop_reason"] =
                json!(if prior_terminal { "end_turn" } else { reason });
            f.write(&[user, answer]);
            if prior_terminal {
                let initial = reader::read(&f.file).unwrap();
                assert_eq!(initial.native_events.len(), 1);
                assert_eq!(
                    initial.events.last().unwrap().assistant_phase,
                    Some(ClaudeAssistantPhase::Final)
                );
            }
            let mut terminal = result.clone();
            if scenario != "implicit" {
                terminal["stop_reason"] = json!(reason);
            }
            f.append(&terminal);
            let parsed = reader::read(&f.file).unwrap();
            assert!(parsed.native_events.is_empty(), "{scenario}: {reason}");
            assert_eq!(
                parsed
                    .events
                    .iter()
                    .find(|event| event.id == "a1:0")
                    .unwrap()
                    .assistant_phase,
                if !prior_terminal && reason == "tool_use" {
                    Some(ClaudeAssistantPhase::Intermediate)
                } else {
                    None
                },
                "{scenario}: {reason}"
            );
        }
    }
    let f = Fixture::new();
    f.write(&[
        f.row("u1", None, "user", json!("Cancelled request")),
        f.row(
            "a1",
            Some("u1"),
            "assistant",
            json!([{"type":"text","text":"Cancelled reply"}]),
        ),
        json!({"type":"system","subtype":"cancelled"}),
        result,
    ]);
    assert!(reader::read(&f.file).unwrap().native_events.is_empty());
    let f = Fixture::new();
    f.complete();
    f.append(&f.row("u2", Some("a1"), "user", json!("New request")));
    assert_eq!(reader::read(&f.file).unwrap().native_events.len(), 1);
}

#[test]
fn terminal_evidence_respects_pending_tools_identity_and_record_integrity() {
    let f = Fixture::new();
    for reason in ["end_turn", "stop_sequence"] {
        let mut answer = f.row(
            "a1",
            Some("u1"),
            "assistant",
            json!([{"type":"text","text":"Finished"}]),
        );
        answer["message"]["stop_reason"] = json!(reason);
        f.write(&[f.row("u1", None, "user", json!("Start")), answer]);
        assert_eq!(
            notification_snapshots(&f.paths).unwrap().streams[0]
                .events
                .len(),
            1
        );
        let mut conflict = json!({"type":"system","subtype":"init","sessionId":"other-fixture","session_id":"native-fixture"});
        f.append(&conflict);
        assert!(notification_snapshots(&f.paths).unwrap().streams.is_empty());
        f.complete();
        conflict["sessionId"] = json!("native-fixture");
        conflict["version"] = json!("2.1.284");
        conflict["claude_code_version"] = json!("3.0.0");
        f.append(&conflict);
        assert_eq!(
            notification_snapshots(&f.paths).unwrap().streams[0]
                .events
                .len(),
            1
        );
    }
    f.write(&[
        f.row("u1", None, "user", json!("Start")),
        f.row(
            "a1",
            Some("u1"),
            "assistant",
            json!([{"type":"tool_use","id":"call","name":"Bash","input":{"command":"pwd"}}]),
        ),
        f.row(
            "a2",
            Some("a1"),
            "assistant",
            json!([{"type":"text","text":"Premature answer"}]),
        ),
    ]);
    assert!(notification_snapshots(&f.paths).unwrap().streams[0]
        .events
        .is_empty());
    fs::OpenOptions::new()
        .append(true)
        .open(&f.file)
        .unwrap()
        .write_all(b"{\"type\":")
        .unwrap();
    assert!(notification_snapshots(&f.paths).unwrap().streams.is_empty());
}

#[test]
fn notification_prompts_are_native_prompt_ids_and_completed_turns_survive_later_turns() {
    let f = Fixture::new();
    let mut user = f.row("message-one", None, "user", json!("First prompt"));
    user["promptId"] = json!("prompt-one");
    let mut answer = f.row(
        "answer-one",
        Some("message-one"),
        "assistant",
        json!("First completion"),
    );
    answer["message"]["stop_reason"] = json!("end_turn");
    let mut second = f.row(
        "message-two",
        Some("answer-one"),
        "user",
        json!("Second prompt"),
    );
    second["promptId"] = json!("prompt-two");
    f.write(&[user, answer, second]);
    let context = notification_context(&f.paths, &f.file).unwrap();
    assert_eq!(context.stream.events.len(), 1);
    assert_eq!(
        context.stream.events[0].prompt_id.as_deref(),
        Some("prompt-one")
    );
    assert!(!context.prompts.contains_key("message-one"));
    assert_eq!(context.prompts["prompt-two"].turn_id, "message-two");
    assert_eq!(
        context.hook_state("prompt-one", "failure", i64::MAX, None),
        ClaudeNotificationState::Obsolete
    );
    assert_eq!(
        context.hook_state("prompt-one", "completion", i64::MAX, None),
        ClaudeNotificationState::Current
    );
    assert_eq!(
        context.hook_state("prompt-two", "completion", i64::MAX, None),
        ClaudeNotificationState::Pending
    );
    let mut partial = f.row(
        "partial-two",
        Some("message-two"),
        "assistant",
        json!("Second partial"),
    );
    partial["message"]["stop_reason"] = Value::Null;
    f.append(&partial);
    assert_eq!(reader::read(&f.file).unwrap().native_events.len(), 1);
    f.append(&json!({"type":"system","subtype":"cancelled"}));
    assert_eq!(reader::read(&f.file).unwrap().native_events.len(), 1);
}

#[test]
fn failure_hooks_expire_on_execution_but_not_metadata_and_error_text_is_not_completion() {
    let f = Fixture::new();
    let mut user = f.row("request", None, "user", json!("Request"));
    user["promptId"] = json!("prompt");
    let mut error = f.row(
        "api-error",
        Some("request"),
        "assistant",
        json!("API error fixture"),
    );
    error["isApiErrorMessage"] = json!(true);
    f.write(&[user, error]);
    let context = notification_context(&f.paths, &f.file).unwrap();
    assert!(context.stream.events.is_empty());
    let cutoff = Some(context.stream.record_count);
    assert_eq!(
        context.hook_state("prompt", "failure", i64::MAX, cutoff),
        ClaudeNotificationState::Current
    );
    f.append(&json!({"type":"system","subtype":"turn_duration","durationMs":10}));
    assert_eq!(
        notification_context(&f.paths, &f.file).unwrap().hook_state(
            "prompt",
            "failure",
            i64::MAX,
            cutoff
        ),
        ClaudeNotificationState::Current
    );
    let mut resumed = f.row(
        "resumed",
        Some("api-error"),
        "assistant",
        json!("Working again"),
    );
    resumed["message"]["stop_reason"] = Value::Null;
    f.append(&resumed);
    let context = notification_context(&f.paths, &f.file).unwrap();
    assert_eq!(
        context.hook_state("prompt", "failure", i64::MAX, cutoff),
        ClaudeNotificationState::Obsolete
    );
    assert_eq!(
        context.hook_state("prompt", "failure", i64::MAX, None),
        ClaudeNotificationState::Obsolete
    );
    f.append(&f.row("completed", Some("resumed"), "assistant", json!("Done")));
    let context = notification_context(&f.paths, &f.file).unwrap();
    assert_eq!(
        context.hook_state("prompt", "completion", i64::MAX, None),
        ClaudeNotificationState::Current
    );
    assert_eq!(
        context.hook_state("prompt", "failure", i64::MAX, None),
        ClaudeNotificationState::Obsolete
    );
    f.append(&json!({"type":"system","subtype":"cancelled"}));
    assert_eq!(
        notification_context(&f.paths, &f.file).unwrap().hook_state(
            "prompt",
            "completion",
            i64::MAX,
            None
        ),
        ClaudeNotificationState::Obsolete
    );
}

#[test]
fn polling_drops_old_failures_when_a_new_prompt_starts() {
    let f = Fixture::new();
    let mut user = f.row("request", None, "user", json!("Request"));
    user["promptId"] = json!("prompt");
    f.write(&[user, json!({"type":"result","subtype":"error_during_execution","is_error":true,"errors":["fixture error"],"timestamp":"2026-09-29T00:00:01Z"})]);
    assert_eq!(
        notification_context(&f.paths, &f.file)
            .unwrap()
            .stream
            .events[0]
            .kind,
        "failure"
    );
    let mut next = f.row("next", Some("request"), "user", json!("Continue"));
    next["promptId"] = json!("next-prompt");
    f.append(&next);
    assert!(notification_context(&f.paths, &f.file)
        .unwrap()
        .stream
        .events
        .is_empty());
}

#[test]
fn failure_evidence_written_after_the_hook_is_not_resumed_execution() {
    let f = Fixture::new();
    let mut user = f.row("request", None, "user", json!("Request"));
    user["promptId"] = json!("prompt");
    f.write(&[user]);
    let initial = notification_context(&f.paths, &f.file).unwrap();
    let received = initial.prompts["prompt"].timestamp_ms + 1;
    let mut error = f.row(
        "error",
        Some("request"),
        "assistant",
        json!("Fixture API error"),
    );
    error["isApiErrorMessage"] = json!(true);
    error["timestamp"] = json!(chrono::DateTime::from_timestamp_millis(received + 10)
        .unwrap()
        .to_rfc3339());
    f.append(&error);
    let context = notification_context(&f.paths, &f.file).unwrap();
    assert_eq!(
        context.hook_state(
            "prompt",
            "failure",
            received,
            Some(initial.stream.record_count)
        ),
        ClaudeNotificationState::Current
    );
    assert!(context.stream.events.is_empty());
}

#[test]
fn execute_claude_delete_rejects_out_of_bounds_running_and_stops_on_partial_failure() {
    let f = Fixture::new();
    f.complete();

    // 1. Out-of-bounds / forged session_key is rejected.
    assert!(execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: "claude:0000000000000000000000000000000000000000000000000000000000000000"
                .into(),
            confirmed: true,
            fingerprint: String::new(),
        },
    )
    .is_err());

    // 2. Actively running Claude session is rejected without deleting files.
    let cwd = f.root.join("workspace");
    activity::test_owner(Some("native-fixture"), &cwd);
    let err = execute_claude_delete(
        &f.paths,
        ClaudeDeleteRequest {
            session_key: f.key(),
            confirmed: true,
            fingerprint: String::new(),
        },
    );
    activity::test_owner(None, Path::new("/fixture"));
    assert!(err.is_err());
    assert!(f.file.exists());

    // 3. Partial failure stops immediately and preserves subsequent sidecar / shared index targets.
    #[cfg(unix)]
    if unsafe { libc::geteuid() } != 0 {
        use std::os::unix::fs::PermissionsExt;
        let locked_sidecar = f.file.with_extension("");
        fs::create_dir_all(&locked_sidecar).unwrap();
        let locked_child = locked_sidecar.join("locked.txt");
        fs::write(&locked_child, "locked").unwrap();
        let later_debug = f.paths.config.join("debug/native-fixture.txt");
        fs::create_dir_all(later_debug.parent().unwrap()).unwrap();
        fs::write(&later_debug, "preserved-after-stop").unwrap();
        let history = f.paths.config.join("history.jsonl");
        fs::write(&history, "{\"sessionId\":\"native-fixture\"}\n").unwrap();
        fs::set_permissions(&locked_sidecar, fs::Permissions::from_mode(0o500)).unwrap();

        let partial = execute_claude_delete(
            &f.paths,
            ClaudeDeleteRequest {
                session_key: f.key(),
                confirmed: true,
                fingerprint: String::new(),
            },
        )
        .unwrap();
        fs::set_permissions(&locked_sidecar, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!partial.deleted);
        assert!(!partial.residuals.is_empty());
        assert!(locked_child.exists());
        assert!(later_debug.exists());
        assert!(fs::read_to_string(&history)
            .unwrap()
            .contains("native-fixture"));
    }
}

#[test]
fn notification_snapshots_reuse_lightweight_probe_cache_without_rereading_or_touching_detail_cache()
{
    let f = Fixture::new();
    f.complete();

    reader::BODY_RECORDS.with(|c| c.set(0));
    let first = notification_snapshots(&f.paths).unwrap();
    assert_eq!(first.errors, 0);
    assert_eq!(first.streams.len(), 1);
    assert_eq!(first.streams[0].events.len(), 1);
    let parsed_first = reader::BODY_RECORDS.with(|c| c.get());
    assert!(parsed_first >= 2);

    // Unchanged transcript on subsequent 15s scan: 0 JSONL lines parsed!
    reader::BODY_RECORDS.with(|c| c.set(0));
    let second = notification_snapshots(&f.paths).unwrap();
    assert_eq!(reader::BODY_RECORDS.with(|c| c.get()), 0);
    assert_eq!(second.streams[0].events, first.streams[0].events);

    // Appending a new turn invalidates the lightweight probe cache entry and detects the update.
    f.append(&f.row("u2", Some("a1"), "user", json!("Follow-up question")));
    f.append(&f.row(
        "a2",
        Some("u2"),
        "assistant",
        json!([{"type":"text","text":"Second reply"}]),
    ));
    let third = notification_snapshots(&f.paths).unwrap();
    assert!(reader::BODY_RECORDS.with(|c| c.get()) >= 4);
    assert_eq!(third.streams[0].events.len(), 2);
}
