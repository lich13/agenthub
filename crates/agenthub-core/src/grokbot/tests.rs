use super::*;
use serde_json::json;
struct Fixture {
    root: PathBuf,
    paths: GrokBotPaths,
    scope: String,
}
impl Fixture {
    fn new(account: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("agenthub-grokbot-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let f = Self {
            paths: GrokBotPaths {
                persistence: root.clone(),
            },
            root,
            scope: encode_scope(account),
        };
        f.write(
            &format!("{PREFIX}client-meta.account-slot"),
            1,
            json!(account),
        );
        f.rows(json!([{"id":"bot-a","name":"Fixture","lastActivityAt":1000}]));
        f
    }
    fn write(&self, k: &str, schema: u64, value: Value) {
        let p = slice_path(&self.root, k);
        let replacement = p.with_extension("replacement");
        let body = json!({"schemaVersion":schema,"value":value}).to_string();
        fs::write(
            &replacement,
            if p.extension().unwrap() == "kblob" {
                format!("{}\n{body}", json!(k))
            } else {
                body
            },
        )
        .unwrap();
        fs::rename(replacement, p).unwrap();
    }
    fn rows(&self, rows: Value) {
        self.write(
            &key(&self.scope, "roster.last-roster"),
            4,
            json!({"rows":rows}),
        );
    }
    fn replica(&self, entries: Value) {
        self.write(
            &replica_key(&self.scope, "bot-a"),
            1,
            json!({"entries":entries,"epochHint":"fixture","persistedAt":2000}),
        );
    }
    fn session(&self) -> String {
        session_key(&self.scope, "bot-a")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn cache_formats_account_isolation_missing_path_and_replacement() {
    for account in ["fixture.account".to_owned(), "a".repeat(200)] {
        let f = Fixture::new(&account);
        let rows = list(&f.paths, &Default::default()).unwrap();
        assert!(rows[0].path.as_os_str().is_empty());
        f.replica(json!([{"kind":"message","id":"u","role":"user","content":"fixture request","timestampMs":1000}]));
        let first = load_all_events(&f.paths, &f.session()).unwrap();
        assert_eq!(first[0].text.as_deref(), Some("fixture request"));
        let path = session_summary(&f.paths, &f.session()).unwrap().path;
        assert!(path.is_file());
        assert_eq!(
            path.extension().unwrap(),
            if account.len() > 100 { "kblob" } else { "blob" }
        );
        f.replica(json!([{"kind":"message","id":"u","role":"user","content":"changed request","timestampMs":1000}]));
        assert_eq!(
            load_all_events(&f.paths, &f.session()).unwrap()[0]
                .text
                .as_deref(),
            Some("changed request")
        );
        f.write(
            &format!("{PREFIX}client-meta.account-slot"),
            1,
            json!("different"),
        );
        assert!(session_summary(&f.paths, &f.session()).is_err());
    }
}
#[test]
fn explicit_requests_only_and_no_terminal_inference() {
    let f = Fixture::new("fixture");
    f.replica(json!([
 {"kind":"message","id":"a","role":"assistant","content":"done","isStreaming":false},
 {"kind":"send-message","id":"widget","message":{"type":"widget","widget":{}}},
 {"kind":"tool-call","id":"failed-tool","status":"failed"},
 {"kind":"send-message","id":"permission","timestampMs":2000,"message":{"type":"local-tool-permission","ask":{"status":"pending","requestId":"request","title":"Fixture permission"}}}
 ]));
    let scan = scan(&f.paths).unwrap();
    assert_eq!(scan.streams[0].events.len(), 1);
    assert_eq!(scan.streams[0].events[0].kind, "authorization");
    f.replica(json!([{"kind":"send-message","id":"permission","timestampMs":2000,"message":{"type":"local-tool-permission","ask":{"status":"allowed","requestId":"request"}}}]));
    assert!(scan_events(&f).is_empty());
}
fn scan_events(f: &Fixture) -> Vec<NativeTurnEvent> {
    scan(&f.paths).unwrap().streams.remove(0).events
}
#[test]
fn baseline_and_reloaded_cache_do_not_replay_requests() {
    let f = Fixture::new("fixture");
    let now = chrono::Utc::now().timestamp_millis();
    let entry = |id: &str, ts: i64| json!({"kind":"send-message","id":id,"timestampMs":ts,"message":{"type":"local-tool-permission","ask":{"status":"pending","requestId":id}}});
    f.replica(json!([entry("old", now - 1000)]));
    let mut config = crate::config::Config::default();
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_grokbot = true;
    let db = crate::db::PanelDb::open(":memory:").unwrap();
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &scan(&f.paths).unwrap(), &config)
            .unwrap(),
        0
    );
    f.replica(json!([entry("old", now - 1000), entry("new", now + 1000)]));
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &scan(&f.paths).unwrap(), &config)
            .unwrap(),
        1
    );
    f.replica(json!([]));
    db.stage_native_notifications(NativeProvider::GrokBot, &scan(&f.paths).unwrap(), &config)
        .unwrap();
    f.replica(json!([entry("new", now + 1000)]));
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &scan(&f.paths).unwrap(), &config)
            .unwrap(),
        0
    );
    assert_eq!(db.pending_native_deliveries(100, None).unwrap().len(), 1);
}

#[test]
fn history_paging_mapping_and_uncached_rows_keep_native_identity() {
    let f = Fixture::new("fixture");
    let mut entries: Vec<_> = (0..180).map(|i| json!({"kind":"message", "id":format!("message-{i}"), "role":if i % 2 == 0 {"user"} else {"assistant"}, "content":format!("fixture {i}"), "timestampMs":1000+i})).collect();
    entries.extend([
        json!({"kind":"tool-call", "id":"tool", "name":"Read", "summary":"Read fixture", "status":"failed", "input":{"path":"/workspace/fixture"}, "result":{"error":"fixture failure"}}),
        json!({"kind":"user-attachment", "id":"file", "file_path":"/nonexistent-fixture/report.txt", "file_name":"report.txt"}),
        json!({"kind":"send-message", "id":"card", "message":{"type":"widget", "widget":{"prompt":"fixture question"}}, "respondedValue":"fixture answer"}),
    ]);
    f.replica(json!(entries));
    let latest = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            limit: Some(3),
            before: None,
        },
    )
    .unwrap();
    assert_eq!(
        latest
            .events
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["tool", "file", "card"]
    );
    assert_eq!(latest.events[0].text.as_deref(), Some("Read fixture"));
    assert!(latest.events[0]
        .result
        .as_deref()
        .unwrap()
        .contains("fixture failure"));
    assert_eq!(
        latest.events[1].user_message.as_ref().unwrap().attachments[0].name,
        "report.txt"
    );
    assert_eq!(latest.events[2].result.as_deref(), Some("fixture answer"));
    let earlier = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            limit: Some(10),
            before: latest.before_cursor,
        },
    )
    .unwrap();
    assert_eq!(earlier.events.first().unwrap().id, "message-170");
    assert_eq!(earlier.events.last().unwrap().id, "message-179");
    assert_eq!(latest.timeline.as_ref().unwrap().len(), 91);
    fs::remove_file(slice_path(&f.root, &replica_key(&f.scope, "bot-a"))).unwrap();
    let absent = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            limit: Some(10),
            before: None,
        },
    )
    .unwrap();
    assert!(absent.summary.path.as_os_str().is_empty());
    assert!(absent.events.is_empty());
    assert_eq!(list(&f.paths, &Default::default()).unwrap().len(), 1);
    fs::write(
        slice_path(&f.root, &replica_key(&f.scope, "bot-a")),
        "invalid",
    )
    .unwrap();
    assert!(list(&f.paths, &Default::default()).is_ok());
    assert!(load_all_events(&f.paths, &f.session()).is_err());
}

#[test]
fn roster_requests_share_native_identity_and_resolved_interactions_win() {
    let f = Fixture::new("fixture");
    f.rows(json!([{"id":"bot-a", "lastActivityAt":2000, "awaitingUserResponse":{"id":"entry", "prompt":"fixture approval"}}]));
    f.replica(json!([{"id":"entry","kind":"send-message","timestampMs":1000,"message":{"type":"auto-review-approval","approval":{"status":"pending","requestId":"approval","summary":"fixture approval"}}}]));
    assert_eq!(scan_events(&f).len(), 1);
    assert_eq!(scan_events(&f)[0].turn_id, "approval");
    assert_eq!(scan_events(&f)[0].kind, "authorization");
    f.replica(json!([{"id":"entry","kind":"send-message","timestampMs":1000,"message":{"type":"auto-review-approval","approval":{"status":"approved","requestId":"approval"}}}]));
    assert!(scan_events(&f).is_empty());
    f.rows(json!([{"id":"bot-a", "lastActivityAt":2000, "awaitingUserResponse":{"id":"form", "prompt":"fixture reply"}}]));
    f.replica(json!([{"id":"form","kind":"send-message","timestampMs":1500,"formResolution":"submitted","message":{"type":"user-form","formRequest":{"title":"fixture reply","fields":[]}}}]));
    assert!(scan_events(&f).is_empty());
    let detail = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            limit: None,
            before: None,
        },
    )
    .unwrap();
    assert_eq!(detail.summary.status, "unknown");
    for response in [json!("fixture answer"), json!(false), json!({})] {
        f.replica(json!([{"id":"form","kind":"send-message","timestampMs":1500,"respondedValue":response,"message":{"type":"user-form","formRequest":{"title":"fixture reply","fields":[]}}}]));
        assert!(scan_events(&f).is_empty());
    }
    f.replica(json!([{"id":"form","kind":"send-message","timestampMs":1500,"message":{"type":"user-form","formRequest":{"title":"fixture reply","fields":[]},"phoneCallQuestion":{"deadlineAtMs":1}}}]));
    assert!(scan_events(&f).is_empty());
}

#[test]
fn unknown_interactions_and_schema_do_not_create_notifications() {
    let f = Fixture::new(&"a".repeat(200));
    f.replica(json!([
        {"id":"unknown","kind":"send-message","timestampMs":1000,"message":{"type":"future-interaction","status":"pending"}},
        {"id":"permission","kind":"send-message","timestampMs":1000,"message":{"type":"local-tool-permission","ask":{"requestId":"fixture","status":"unknown"}}},
        {"id":"failed-send","kind":"send-message","timestampMs":1000,"draftSendState":"failed","message":{"type":"text","content":"fixture"}}
    ]));
    assert!(scan_events(&f).is_empty());
    let k = replica_key(&f.scope, "bot-a");
    f.write(&k, 99, json!({"entries":[]}));
    assert!(load_all_events(&f.paths, &f.session()).is_err());
    assert_eq!(scan(&f.paths).unwrap().errors, 1);
    fs::write(
        slice_path(&f.root, &k),
        "\"different-key\"\n{\"schemaVersion\":1,\"value\":{\"entries\":[]}}",
    )
    .unwrap();
    assert!(load_all_events(&f.paths, &f.session()).is_err());
}

#[test]
fn reenable_and_restart_do_not_replay_pending_or_evicted_requests() {
    let f = Fixture::new("fixture");
    let db_path = f.root.join("fixture.sqlite");
    let config = |enabled| {
        let mut c = crate::config::Config::default();
        c.probe.notifications.enabled = true;
        c.probe.notifications.notify_grokbot = enabled;
        c
    };
    let db = crate::db::PanelDb::open(&db_path).unwrap();
    db.stage_native_notifications(
        NativeProvider::GrokBot,
        &scan(&f.paths).unwrap(),
        &config(true),
    )
    .unwrap();
    let entry = json!({"kind":"send-message","id":"request","timestampMs":chrono::Utc::now().timestamp_millis()+1000,"message":{"type":"local-tool-permission","ask":{"status":"pending","requestId":"request"}}});
    f.replica(json!([entry.clone()]));
    assert_eq!(
        db.stage_native_notifications(
            NativeProvider::GrokBot,
            &scan(&f.paths).unwrap(),
            &config(true)
        )
        .unwrap(),
        1
    );
    db.stage_native_notifications(
        NativeProvider::GrokBot,
        &scan(&f.paths).unwrap(),
        &config(false),
    )
    .unwrap();
    assert_eq!(
        db.stage_native_notifications(
            NativeProvider::GrokBot,
            &scan(&f.paths).unwrap(),
            &config(true)
        )
        .unwrap(),
        0
    );
    drop(db);
    f.replica(json!([]));
    let db = crate::db::PanelDb::open(&db_path).unwrap();
    db.stage_native_notifications(
        NativeProvider::GrokBot,
        &scan(&f.paths).unwrap(),
        &config(true),
    )
    .unwrap();
    let path = slice_path(&f.root, &replica_key(&f.scope, "bot-a"));
    fs::rename(&path, f.root.join("old-fixture")).unwrap();
    f.replica(json!([entry]));
    assert_eq!(
        db.stage_native_notifications(
            NativeProvider::GrokBot,
            &scan(&f.paths).unwrap(),
            &config(true)
        )
        .unwrap(),
        0
    );
    assert!(db.pending_native_deliveries(100, None).unwrap().is_empty());
}

#[test]
fn cached_ordinary_card_does_not_become_roster_reply_request() {
    let f = Fixture::new("fixture");
    f.rows(json!([{"id":"bot-a","lastActivityAt":2000,"awaitingUserResponse":{"id":"widget"}}]));
    f.replica(json!([{"id":"widget","kind":"send-message","timestampMs":2000,"message":{"type":"widget","widget":{"text":"ordinary card"}}}]));
    assert!(scan_events(&f).is_empty());
}

#[test]
fn targeted_notification_scan_does_not_read_other_bot_transcripts() {
    let f = Fixture::new("fixture.account");
    f.rows(json!([
        {"id":"bot-a","name":"Target","lastActivityAt":2000,"awaitingUserResponse":{"id":"question-a"}},
        {"id":"bot-b","name":"Other","lastActivityAt":2000}
    ]));
    f.replica(json!([]));
    f.write(
        &replica_key(&f.scope, "bot-b"),
        1,
        json!({"entries":"broken"}),
    );
    let selected = scan_selected(&f.paths, Some("bot-a")).unwrap();
    assert_eq!(selected.errors, 0);
    assert_eq!(selected.streams.len(), 1);
    assert_eq!(selected.streams[0].session_key, f.session());
    assert_eq!(selected.streams[0].events.len(), 1);
    assert_eq!(scan(&f.paths).unwrap().errors, 1);
}

#[test]
fn send_message_completion_cursor_dedupe_and_upgrade_baseline() {
    let f = Fixture::new("fixture");
    let now = chrono::Utc::now().timestamp_millis();
    let msg = |id: &str, ts: i64, content: &str| {
        json!({
            "kind": "send-message",
            "id": id,
            "timestampMs": ts,
            "message": { "type": "text", "content": content }
        })
    };

    // 1. Existing stream from pre-v2.1.11 upgrade with cursor = 0 and historical message older than prior_scanned_at
    let mut config = crate::config::Config::default();
    config.probe.enabled = true;
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_grokbot = true;
    let db_path = f.root.join("upgrade.sqlite");
    let db = crate::db::PanelDb::open(&db_path).unwrap();

    // Baseline with empty replica (simulates v2.1.10 where settled_count was 0 -> cursor = 0)
    f.replica(json!([]));
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &scan(&f.paths).unwrap(), &config)
            .unwrap(),
        0
    );
    assert_eq!(
        db.native_probe_stream_cursor(NativeProvider::GrokBot, &f.session())
            .unwrap(),
        Some(0)
    );

    // Upgrade scan sees a message from 5s before prior_scanned_at (but after activated_ms if activated earlier) -> skipped, cursor advanced
    let hist_ts = (now / 1000 - 5) * 1000;
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE native_probe_providers SET activated_ms=?1 WHERE provider='grokbot'",
            [hist_ts - 60_000],
        )
        .unwrap();
    }
    f.replica(json!([msg("hist-1", hist_ts, "Historical reply")]));
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &scan(&f.paths).unwrap(), &config)
            .unwrap(),
        0,
        "pre-upgrade historical message on cursor=0 stream must be baselined as skipped"
    );
    assert_eq!(
        db.native_probe_stream_cursor(NativeProvider::GrokBot, &f.session())
            .unwrap(),
        Some(hist_ts as u64)
    );

    // 2. New send-message completion arrives -> queued exactly once and cursor advances
    let new_ts = now + 2_000;
    f.replica(json!([
        msg("hist-1", hist_ts, "Historical reply"),
        msg("reply-1", new_ts, "Bot finished task")
    ]));
    let s1 = scan(&f.paths).unwrap();
    assert_eq!(s1.streams[0].settled_count, new_ts as u64);
    assert_eq!(s1.streams[0].events.len(), 1);
    assert_eq!(s1.streams[0].events[0].kind, "completion");
    assert_eq!(s1.streams[0].events[0].turn_id, "reply-1");
    assert_eq!(s1.streams[0].events[0].body, "Bot finished task");
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &s1, &config)
            .unwrap(),
        1
    );
    // Repeated scan with same completion does not re-queue
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &s1, &config)
            .unwrap(),
        0
    );

    // Detail status remains "unknown" when no pending interaction exists
    let d = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            limit: None,
            before: None,
        },
    )
    .unwrap();
    assert_eq!(d.summary.status, "unknown");

    // 3. Pending interaction newer than completion suppresses completion event while preserving settled_count,
    // and resolving that interaction does not resurrect the pre-interaction message as a new completion
    let pre_perm_ts = new_ts + 500;
    let perm_ts = new_ts + 1_000;
    f.replica(json!([
        msg("pre-perm", pre_perm_ts, "Running tool now"),
        {
            "kind": "send-message",
            "id": "perm-1",
            "timestampMs": perm_ts,
            "message": {
                "type": "local-tool-permission",
                "ask": { "status": "pending", "requestId": "req-1", "title": "Need tool access" }
            }
        }
    ]));
    let s_perm = scan(&f.paths).unwrap();
    assert_eq!(s_perm.streams[0].settled_count, pre_perm_ts as u64);
    assert_eq!(s_perm.streams[0].events.len(), 1);
    assert_eq!(s_perm.streams[0].events[0].kind, "authorization");
    assert_eq!(
        db.stage_native_notifications(NativeProvider::GrokBot, &s_perm, &config)
            .unwrap(),
        1
    );
    assert_eq!(
        db.native_probe_stream_cursor(NativeProvider::GrokBot, &f.session())
            .unwrap(),
        Some(pre_perm_ts as u64)
    );

    // User approves permission while tool is still running: pre-perm message must not resurrect as completion
    f.replica(json!([
        msg("pre-perm", pre_perm_ts, "Running tool now"),
        {
            "kind": "send-message",
            "id": "perm-1",
            "timestampMs": perm_ts,
            "message": {
                "type": "local-tool-permission",
                "ask": { "status": "allowed", "requestId": "req-1", "title": "Need tool access" }
            }
        }
    ]));
    let s_allowed = scan(&f.paths).unwrap();
    assert!(
        s_allowed.streams[0].events.is_empty(),
        "resolved interaction must not resurrect earlier send-message as a completion"
    );

    // 4. Roster lastEntry fallback and attachment completion (including empty file_name/url with alt fallback)
    let attach_ts = perm_ts + 2_000;
    f.rows(json!([{
        "id": "bot-a",
        "name": "Fixture",
        "lastActivityAt": attach_ts,
        "lastEntry": {
            "id": "roster-attach",
            "kind": "send-message",
            "timestampMs": attach_ts,
            "message": {
                "type": "attachment",
                "file_name": "report.pdf",
                "url": "https://example.invalid/report.pdf"
            }
        }
    }]));
    f.replica(json!([]));
    let s_attach = scan(&f.paths).unwrap();
    assert_eq!(s_attach.streams[0].events.len(), 1);
    assert_eq!(s_attach.streams[0].events[0].kind, "completion");
    assert_eq!(s_attach.streams[0].events[0].turn_id, "roster-attach");
    assert_eq!(
        s_attach.streams[0].events[0].body,
        "[report.pdf](<https://example.invalid/report.pdf>)"
    );

    let alt_attach_ts = attach_ts + 1_000;
    f.replica(json!([{
        "id": "alt-attach",
        "kind": "send-message",
        "timestampMs": alt_attach_ts,
        "message": {
            "type": "attachment",
            "file_name": "",
            "alt": "Architecture diagram",
            "url": ""
        }
    }]));
    let s_alt = scan(&f.paths).unwrap();
    assert_eq!(s_alt.streams[0].events.len(), 1);
    assert_eq!(s_alt.streams[0].events[0].turn_id, "alt-attach");
    assert_eq!(s_alt.streams[0].events[0].body, "Architecture diagram");
}
