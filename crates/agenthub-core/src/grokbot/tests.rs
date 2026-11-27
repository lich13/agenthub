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
fn scan_events(f: &Fixture) -> Arc<Vec<NativeTurnEvent>> {
    scan(&f.paths).unwrap().streams.remove(0).events
}
#[test]
fn baseline_and_reloaded_cache_do_not_replay_requests() {
    let f = Fixture::new("fixture");
    let now = jiff::Timestamp::now().as_millisecond();
    let entry = |id: &str, ts: i64| json!({"kind":"send-message","id":id,"timestampMs":ts,"message":{"type":"local-tool-permission","ask":{"status":"pending","requestId":id}}});
    f.replica(json!([entry("old", now - 1000)]));
    let mut config = crate::config::Config::current_defaults().unwrap();
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_grokbot = true;
    let db = crate::db::PanelDb::open_with_secret_box(
        ":memory:",
        crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let stage_cfg = |db: &crate::db::PanelDb,
                     scan: &crate::native_probe::NativeScan,
                     c: &crate::config::Config| {
        db.stage_native_notifications_from_config(
            NativeProvider::GrokBot,
            scan,
            &|| Ok(c.clone()),
            false,
            true,
        )
        .unwrap()
    };
    assert_eq!(stage_cfg(&db, &scan(&f.paths).unwrap(), &config), 0);
    f.replica(json!([entry("old", now - 1000), entry("new", now + 1000)]));
    assert_eq!(stage_cfg(&db, &scan(&f.paths).unwrap(), &config), 1);
    f.replica(json!([]));
    stage_cfg(&db, &scan(&f.paths).unwrap(), &config);
    f.replica(json!([entry("new", now + 1000)]));
    assert_eq!(stage_cfg(&db, &scan(&f.paths).unwrap(), &config), 0);
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
            agent_id: None,
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
            agent_id: None,
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
            agent_id: None,
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
            agent_id: None,
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
        let mut c = crate::config::Config::current_defaults().unwrap();
        c.probe.notifications.enabled = true;
        c.probe.notifications.notify_grokbot = enabled;
        c
    };
    let db = crate::db::PanelDb::open_with_secret_box(
        &db_path,
        crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let stage_cfg =
        |db: &crate::db::PanelDb, scan: &crate::native_probe::NativeScan, enabled: bool| {
            let c = config(enabled);
            db.stage_native_notifications_from_config(
                NativeProvider::GrokBot,
                scan,
                &|| Ok(c.clone()),
                false,
                true,
            )
            .unwrap()
        };
    stage_cfg(&db, &scan(&f.paths).unwrap(), true);
    let entry = json!({"kind":"send-message","id":"request","timestampMs":jiff::Timestamp::now().as_millisecond()+1000,"message":{"type":"local-tool-permission","ask":{"status":"pending","requestId":"request"}}});
    f.replica(json!([entry.clone()]));
    assert_eq!(stage_cfg(&db, &scan(&f.paths).unwrap(), true), 1);
    stage_cfg(&db, &scan(&f.paths).unwrap(), false);
    assert_eq!(stage_cfg(&db, &scan(&f.paths).unwrap(), true), 0);
    drop(db);
    f.replica(json!([]));
    let db = crate::db::PanelDb::open_with_secret_box(
        &db_path,
        crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    stage_cfg(&db, &scan(&f.paths).unwrap(), true);
    let path = slice_path(&f.root, &replica_key(&f.scope, "bot-a"));
    fs::rename(&path, f.root.join("old-fixture")).unwrap();
    f.replica(json!([entry]));
    assert_eq!(stage_cfg(&db, &scan(&f.paths).unwrap(), true), 0);
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
    let now = jiff::Timestamp::now().as_millisecond();
    let msg = |id: &str, ts: i64, content: &str| {
        json!({
            "kind": "send-message",
            "id": id,
            "timestampMs": ts,
            "message": { "type": "text", "content": content }
        })
    };

    // 1. Existing stream from pre-v2.1.11 upgrade with cursor = 0 and historical message older than prior_scanned_at
    let mut config = crate::config::Config::current_defaults().unwrap();
    config.probe.enabled = true;
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_grokbot = true;
    let db_path = f.root.join("upgrade.sqlite");
    let db = crate::db::PanelDb::open_with_secret_box(
        &db_path,
        crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();

    let stage_cfg = |db: &crate::db::PanelDb, scan: &crate::native_probe::NativeScan| {
        db.stage_native_notifications_from_config(
            NativeProvider::GrokBot,
            scan,
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap()
    };

    // Baseline with empty replica (simulates v2.1.10 where settled_count was 0 -> cursor = 0)
    f.replica(json!([]));
    assert_eq!(stage_cfg(&db, &scan(&f.paths).unwrap()), 0);
    assert_eq!(
        db.native_probe_stream_cursor(NativeProvider::GrokBot, &f.session())
            .unwrap(),
        Some(0)
    );

    // The prior scan stores seconds; all milliseconds in that same second belong to its baseline.
    let hist_ts = (now / 1000) * 1000 + 999;
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE native_probe_providers SET activated_ms=?1 WHERE provider='grokbot'",
            [hist_ts - 60_000],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM settings WHERE key='grokbot_resume_cutoff_ms'",
            [],
        )
        .unwrap();
    }
    f.replica(json!([msg("hist-1", hist_ts, "Historical reply")]));
    assert_eq!(
        stage_cfg(&db, &scan(&f.paths).unwrap()),
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
    assert_eq!(stage_cfg(&db, &s1), 1);
    // Repeated scan with same completion does not re-queue
    assert_eq!(stage_cfg(&db, &s1), 0);

    // Detail status remains "unknown" when no pending interaction exists
    let d = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            agent_id: None,
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
    assert_eq!(stage_cfg(&db, &s_perm), 1);
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
    assert_eq!(s_attach.streams[0].events[0].body, "report.pdf");

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

#[test]
fn restored_notifications_keep_the_suspended_message_baseline() {
    let f = Fixture::new("fixture");
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/grokbot-1.1.0-messages.json"
    ))
    .unwrap();
    let now = jiff::Timestamp::now().as_millisecond();
    let mut entries = fixture["entries"].as_array().unwrap()[1..].to_vec();
    for (index, entry) in entries.iter_mut().enumerate() {
        entry["timestampMs"] = json!(now + index as i64 + 1000);
    }
    f.replica(json!(entries));
    let mut config = crate::config::Config::current_defaults().unwrap();
    config.probe.enabled = true;
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_grokbot = true;
    let db_path = f.root.join("suspended.sqlite");
    let open = || {
        crate::db::PanelDb::open_with_secret_box(
            &db_path,
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap()
    };
    let db = open();
    let stage_cfg = |db: &crate::db::PanelDb, scan: &crate::native_probe::NativeScan| {
        db.stage_native_notifications_from_config(
            NativeProvider::GrokBot,
            scan,
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap()
    };
    // v2.1.17 advanced message positions without creating completion candidates.
    let mut suspended = scan(&f.paths).unwrap();
    std::sync::Arc::make_mut(&mut suspended.streams[0].events).clear();
    assert_eq!(stage_cfg(&db, &suspended), 0);
    drop(db);
    crate::provider_sessions::HISTORY_CACHE.evict_idle(std::time::Duration::ZERO);
    let db = open();
    assert_eq!(
        stage_cfg(&db, &scan(&f.paths).unwrap()),
        0,
        "restoring notifications must not replay the paused version's last message"
    );
    let mut fresh = entries[0].clone();
    fresh["id"] = json!("fixture-restored-message");
    fresh["timestampMs"] = json!(now + 2000);
    entries.push(fresh);
    f.replica(json!(entries));
    let restored = scan(&f.paths).unwrap();
    assert_eq!(stage_cfg(&db, &restored), 1);
    assert_eq!(stage_cfg(&db, &restored), 0);
    for kind in ["completion", "reply_needed", "authorization"] {
        assert!(NativeProvider::GrokBot.event_enabled(&config, kind));
    }
}

#[test]
fn later_message_still_notifies_while_an_earlier_interaction_is_pending() {
    let f = Fixture::new("fixture");
    f.replica(json!([
        {"id":"request","kind":"send-message","timestampMs":1000,"message":{"type":"local-tool-permission","ask":{"status":"pending","requestId":"approval"}}},
        {"id":"later","kind":"send-message","timestampMs":2000,"message":{"type":"attachment","file_name":"report(final).pdf","source_path":"/fixture/private/report(final).pdf","url":"https://example.invalid/report(final).pdf"}}
    ]));
    let events = scan_events(&f);
    assert!(events.iter().any(|event| event.kind == "authorization"));
    let message = events
        .iter()
        .find(|event| event.kind == "completion")
        .unwrap();
    assert_eq!(message.turn_id, "later");
    assert_eq!(message.body, "report(final).pdf");
    assert!(load_all_events(&f.paths, &f.session())
        .unwrap()
        .iter()
        .any(|event| event
            .text
            .as_deref()
            .is_some_and(|text| text.contains("/fixture/private/report(final).pdf"))));
}

#[test]
fn search_prevents_cross_account_metadata_leak_on_mid_read_or_mid_search_switch_and_truncates_roster_ids(
) {
    use crate::services::{
        search::{SearchScope, SearchUseCases, SessionSearchRequest},
        sessions::SessionProvider,
    };

    struct HookReset;
    impl Drop for HookReset {
        fn drop(&mut self) {
            TEST_MID_ROSTER_READ_HOOK.with(|c| *c.borrow_mut() = None);
            TEST_MID_SEARCH_LOAD_HOOK.with(|c| *c.borrow_mut() = None);
        }
    }
    let _reset = HookReset;

    let f = Fixture::new("acct-a");
    let scope_a = encode_scope("acct-a");
    let scope_b = encode_scope("acct-b");

    // 1. Post-truncate ID set only: 4 rows in roster, max_sessions = 2 -> only top 2 IDs retained.
    f.rows(json!([
        {"id": "bot-1", "title": "Oldest", "lastActivityAt": 1000},
        {"id": "bot-2", "title": "Second", "lastActivityAt": 2000},
        {"id": "bot-3", "title": "Third", "lastActivityAt": 3000},
        {"id": "bot-4", "title": "Newest", "lastActivityAt": 4000}
    ]));
    let (trunc_summaries, trunc_roster) = prepare_search(&f.paths, None, 2).unwrap();
    assert_eq!(trunc_summaries.len(), 2);
    assert_eq!(trunc_roster.thread_ids.len(), 2);
    assert!(trunc_roster.thread_ids.contains("bot-4"));
    assert!(trunc_roster.thread_ids.contains("bot-3"));
    assert!(!trunc_roster.thread_ids.contains("bot-1"));

    // Setup Account A and Account B slices with a shared query token + account-private titles/messages.
    f.rows(json!([
        {"id": "a-meta", "title": "SharedToken SecretAccountA_Title", "lastActivityAt": 5000},
        {"id": "a-body", "title": "PlainA", "lastActivityAt": 4000}
    ]));
    f.write(
        &replica_key(&scope_a, "a-meta"),
        1,
        json!({"entries": [{"kind":"message","id":"m1","role":"user","content":"body a1","timestampMs":5000}]}),
    );
    f.write(
        &replica_key(&scope_a, "a-body"),
        1,
        json!({"entries": [{"kind":"message","id":"m2","role":"user","content":"SharedToken SecretAccountA_Body","timestampMs":4000}]}),
    );
    f.write(
        &key(&scope_b, "roster.last-roster"),
        4,
        json!({"rows": [{"id": "b-1", "title": "SharedToken AccountB_Title", "lastActivityAt": 6000}]}),
    );
    f.write(
        &replica_key(&scope_b, "b-1"),
        1,
        json!({"entries": [{"kind":"message","id":"mb1","role":"assistant","content":"SharedToken AccountB_Body","isStreaming":false,"timestampMs":6000}]}),
    );

    let service = SearchUseCases::with_grokbot_for_test(&f.root, f.paths.clone());

    // 2. Account switches from A -> B mid-`read_verified_roster` (after reading A's roster):
    // must NOT return unverified Account A summaries; retries and returns only Account B!
    let root_for_hook = f.root.clone();
    let switched_once = std::rc::Rc::new(std::cell::Cell::new(false));
    let switched_flag = switched_once.clone();
    TEST_MID_ROSTER_READ_HOOK.with(|c| {
        *c.borrow_mut() = Some(Box::new(move || {
            if !switched_flag.get() {
                switched_flag.set(true);
                let p = slice_path(&root_for_hook, &format!("{PREFIX}client-meta.account-slot"));
                let repl = p.with_extension("replacement");
                fs::write(
                    &repl,
                    json!({"schemaVersion": 1, "value": "acct-b"}).to_string(),
                )
                .unwrap();
                fs::rename(repl, p).unwrap();
            }
        }));
    });
    let mid_roster_res = service
        .search(SessionSearchRequest {
            provider: SessionProvider::Grokbot,
            scope: SearchScope::Provider,
            session_key: None,
            query: "SharedToken".into(),
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert!(switched_once.get());
    assert!(!mid_roster_res.results.is_empty());
    assert!(mid_roster_res.results.iter().all(|r| {
        r.native_id == "b-1"
            && !r.title.contains("SecretAccountA")
            && !r.snippet.contains("SecretAccountA")
    }));
    TEST_MID_ROSTER_READ_HOOK.with(|c| *c.borrow_mut() = None);

    // 3. Switch back to Account A, then switch A -> B mid-search (DURING `load_all_events_for_search`,
    // AFTER `a-meta`'s title match was already pushed to `attempt_matches`!):
    // must discard Account A metadata/events and return ONLY Account B results!
    f.write(
        &format!("{PREFIX}client-meta.account-slot"),
        1,
        json!("acct-a"),
    );
    let root_for_load = f.root.clone();
    let switched_mid_load = std::rc::Rc::new(std::cell::Cell::new(false));
    let load_flag = switched_mid_load.clone();
    TEST_MID_SEARCH_LOAD_HOOK.with(|c| {
        *c.borrow_mut() = Some(Box::new(move || {
            if !load_flag.get() {
                load_flag.set(true);
                let p = slice_path(&root_for_load, &format!("{PREFIX}client-meta.account-slot"));
                let repl = p.with_extension("replacement");
                fs::write(
                    &repl,
                    json!({"schemaVersion": 1, "value": "acct-b"}).to_string(),
                )
                .unwrap();
                fs::rename(repl, p).unwrap();
            }
        }));
    });
    let mid_search_res = service
        .search(SessionSearchRequest {
            provider: SessionProvider::Grokbot,
            scope: SearchScope::Provider,
            session_key: None,
            query: "SharedToken".into(),
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert!(switched_mid_load.get());
    assert!(!mid_search_res.results.is_empty());
    assert!(mid_search_res.results.iter().all(|r| {
        r.native_id == "b-1"
            && !r.title.contains("SecretAccountA")
            && !r.snippet.contains("SecretAccountA")
    }));
    TEST_MID_SEARCH_LOAD_HOOK.with(|c| *c.borrow_mut() = None);

    // 3b. `SearchScope::Thread` on Account A when active account switches A -> B mid-`load_all_events_for_search`
    // (after `a-meta` title match was already pushed to `attempt_matches`) discards stale matches and errors.
    f.write(
        &format!("{PREFIX}client-meta.account-slot"),
        1,
        json!("acct-a"),
    );
    let root_for_thread = f.root.clone();
    let switched_mid_thread = std::rc::Rc::new(std::cell::Cell::new(false));
    let thread_flag = switched_mid_thread.clone();
    TEST_MID_SEARCH_LOAD_HOOK.with(|c| {
        *c.borrow_mut() = Some(Box::new(move || {
            if !thread_flag.get() {
                thread_flag.set(true);
                let p = slice_path(
                    &root_for_thread,
                    &format!("{PREFIX}client-meta.account-slot"),
                );
                let repl = p.with_extension("replacement");
                fs::write(
                    &repl,
                    json!({"schemaVersion": 1, "value": "acct-b"}).to_string(),
                )
                .unwrap();
                fs::rename(repl, p).unwrap();
            }
        }));
    });
    let thread_switch_err = service
        .search(SessionSearchRequest {
            provider: SessionProvider::Grokbot,
            scope: SearchScope::Thread,
            session_key: Some(session_key(&scope_a, "a-meta")),
            query: "SharedToken".into(),
            cursor: None,
            limit: None,
        })
        .unwrap_err()
        .to_string();
    assert!(switched_mid_thread.get());
    assert!(
        thread_switch_err.contains("不在当前账户中"),
        "{thread_switch_err}"
    );
    TEST_MID_SEARCH_LOAD_HOOK.with(|c| *c.borrow_mut() = None);

    // 3c. Same-account `roster.last-roster` mutation mid-`read_verified_roster` retries and drops removed rows.
    let root_for_roster_mut = f.root.clone();
    let scope_b_for_hook = scope_b.clone();
    let mutated_roster_once = std::rc::Rc::new(std::cell::Cell::new(false));
    let roster_mut_flag = mutated_roster_once.clone();
    TEST_MID_ROSTER_READ_HOOK.with(|c| {
        *c.borrow_mut() = Some(Box::new(move || {
            if !roster_mut_flag.get() {
                roster_mut_flag.set(true);
                let p = slice_path(
                    &root_for_roster_mut,
                    &key(&scope_b_for_hook, "roster.last-roster"),
                );
                let repl = p.with_extension("replacement");
                fs::write(
                    &repl,
                    json!({
                        "schemaVersion": 4,
                        "value": {"rows": [{"id": "b-fresh", "title": "SharedToken FreshOnlyTitle", "lastActivityAt": 7000}]}
                    })
                    .to_string(),
                )
                .unwrap();
                fs::rename(repl, p).unwrap();
            }
        }));
    });
    let roster_mut_res = service
        .search(SessionSearchRequest {
            provider: SessionProvider::Grokbot,
            scope: SearchScope::Provider,
            session_key: None,
            query: "SharedToken".into(),
            cursor: None,
            limit: None,
        })
        .unwrap();
    assert!(mutated_roster_once.get());
    assert!(!roster_mut_res.results.is_empty());
    assert!(roster_mut_res
        .results
        .iter()
        .all(|r| r.native_id == "b-fresh" && !r.title.contains("AccountB_Title")));
    TEST_MID_ROSTER_READ_HOOK.with(|c| *c.borrow_mut() = None);

    // 4. Continuous thrashing across all bounded attempts returns an explicit error instead of stale data.
    let root_thrash = f.root.clone();
    let toggle = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let toggle_ref = toggle.clone();
    TEST_MID_ROSTER_READ_HOOK.with(|c| {
        *c.borrow_mut() = Some(Box::new(move || {
            let next = toggle_ref.get() + 1;
            toggle_ref.set(next);
            let acct = if next.is_multiple_of(2) {
                "acct-a"
            } else {
                "acct-b"
            };
            let p = slice_path(&root_thrash, &format!("{PREFIX}client-meta.account-slot"));
            let repl = p.with_extension("replacement");
            fs::write(
                &repl,
                json!({"schemaVersion": 1, "value": acct}).to_string(),
            )
            .unwrap();
            fs::rename(repl, p).unwrap();
        }));
    });
    assert!(service
        .search(SessionSearchRequest {
            provider: SessionProvider::Grokbot,
            scope: SearchScope::Provider,
            session_key: None,
            query: "SharedToken".into(),
            cursor: None,
            limit: None,
        })
        .is_err());
}

#[test]
fn list_and_detail_share_authorization_and_resolved_awaiting_status() {
    let f = Fixture::new("fixture");
    // 1. Pending permission in replica surfaces "authorization" in both list() and detail().
    f.rows(json!([{"id":"bot-a", "lastActivityAt":2000}]));
    f.replica(json!([{
        "id": "perm-1",
        "kind": "send-message",
        "timestampMs": 2000,
        "message": {
            "type": "local-tool-permission",
            "ask": { "status": "pending", "requestId": "req-auth", "title": "Allow tool" }
        }
    }]));
    let listed = list(&f.paths, &Default::default()).unwrap();
    assert_eq!(listed[0].status, "authorization");
    let d1 = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(d1.summary.status, "authorization");

    // 2. Stale awaitingUserResponse pointing to a resolved form or widget card settles to "unknown" in both list() and detail().
    f.rows(
        json!([{"id":"bot-a", "lastActivityAt":3000, "awaitingUserResponse":{"id":"widget-1"}}]),
    );
    f.replica(json!([{
        "id": "widget-1",
        "kind": "send-message",
        "timestampMs": 3000,
        "message": { "type": "widget", "widget": { "text": "card" } }
    }]));
    let listed_resolved = list(&f.paths, &Default::default()).unwrap();
    assert_eq!(listed_resolved[0].status, "unknown");
    let d2 = detail(
        &f.paths,
        &ProviderDetailRequest {
            session_key: f.session(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(d2.summary.status, "unknown");
}
