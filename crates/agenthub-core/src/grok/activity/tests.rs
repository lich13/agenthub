use super::*;
use std::io::Write;

struct Fixture(GrokPaths, GrokSessionSummary);

impl Fixture {
    fn new() -> Self {
        let (paths, id, path) = super::super::tests::fixture();
        let session = super::super::read_summary(&path).unwrap().unwrap();
        assert_eq!(session.id, id);
        Self(paths, session)
    }
    fn events(&self, text: &str) {
        fs::write(self.1.path.join("events.jsonl"), text).unwrap();
    }
    fn start(&self) -> String {
        format!(
            "{}\n",
            serde_json::json!({"type":"turn_started", "session_id":self.1.id,
            "session_relationship":"primary", "schema_version":"1.0"})
        )
    }
    fn snapshot(&self, process: Option<Process>) -> Snapshot {
        Snapshot {
            registrations: Some(vec![Registration {
                id: self.1.id.clone(),
                pid: Some(123),
                opened_at: Some(100),
                cwd: Some(self.1.cwd.clone()),
            }]),
            processes: Some(
                process
                    .map(|p| HashMap::from([(123, p)]))
                    .unwrap_or_default(),
            ),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0.home).unwrap();
    }
}
fn live() -> Process {
    Process {
        started_at: Some(90),
        zombie: false,
        native: Some(true),
    }
}
const END: &str = "{\"type\":\"turn_ended\",\"outcome\":\"completed\"}\n";

#[test]
fn status_needs_live_native_owner_and_unfinished_primary_turn() {
    let f = Fixture::new();
    f.events(&f.start());
    let active = f.snapshot(Some(live()));
    assert_eq!(active.status(&f.1), "running");
    assert!(active.ensure_not_open(&f.1).is_err());
    assert!(active.ensure_deletable(&f.1).is_ok());
    assert_eq!(f.snapshot(None).status(&f.1), "unknown");
    for end in [END, "{\"type\":\"turn_ended\",\"outcome\":\"cancelled\"}\n"] {
        f.events(&(f.start() + end));
        assert_eq!(active.status(&f.1), "recent");
        // An idle but open native session must still block deletion.
        assert!(active.ensure_not_open(&f.1).is_err());
        assert!(active.ensure_deletable(&f.1).is_ok());
        let exited = f.snapshot(None);
        assert_eq!(exited.status(&f.1), "recent");
        assert!(exited.ensure_not_open(&f.1).is_ok());
        assert!(exited.ensure_deletable(&f.1).is_ok());
    }
    f.events(&(f.start() + END + &f.start()));
    assert_eq!(active.status(&f.1), "running");
}

#[test]
fn process_identity_start_time_and_permissions_gate_ownership() {
    let f = Fixture::new();
    for process in [
        None,
        Some(Process {
            zombie: true,
            ..live()
        }),
        Some(Process {
            started_at: Some(101),
            ..live()
        }),
        Some(Process {
            native: Some(false),
            ..live()
        }),
    ] {
        assert_eq!(f.snapshot(process).ownership(&f.1), Ownership::Inactive);
    }
    for process in [
        Process {
            started_at: None,
            ..live()
        },
        Process {
            native: None,
            ..live()
        },
    ] {
        let snapshot = f.snapshot(Some(process));
        assert_eq!(snapshot.ownership(&f.1), Ownership::Unknown);
        assert!(snapshot.ensure_deletable(&f.1).is_ok());
    }
    let mut snapshot = f.snapshot(Some(live()));
    snapshot.processes = None;
    assert_eq!(snapshot.ownership(&f.1), Ownership::Unknown);
    assert!(snapshot.ensure_deletable(&f.1).is_ok());
    snapshot = f.snapshot(Some(live()));
    snapshot.registrations.as_mut().unwrap()[0].cwd = Some("/different-workspace".into());
    assert_eq!(snapshot.ownership(&f.1), Ownership::Unknown);
    snapshot.registrations = None;
    assert!(snapshot.ensure_not_open(&f.1).is_err());
}

#[test]
fn valid_running_activity_without_a_native_owner_is_deletable_while_status_stays_unknown() {
    let f = Fixture::new();
    f.events(&f.start());
    let exited = f.snapshot(None);

    assert_eq!(exited.status(&f.1), "unknown");
    assert_eq!(
        exited.deletion_eligibility(&f.1),
        GrokDeletionEligibility {
            allowed: true,
            reason: None,
        }
    );
    assert!(exited.ensure_deletable(&f.1).is_ok());
}

#[test]
fn registry_supports_native_arrays_objects_and_rejects_uncertain_rows() {
    let f = Fixture::new();
    let path = f.0.home.join("active_sessions.json");
    for value in [
        serde_json::json!([{"session_id":f.1.id,"pid":123,"cwd":"/work","opened_at":"2026-01-01T00:00:00Z"}]),
        serde_json::json!({&f.1.id:{"pid":123,"cwd":"/work","openedAt":"2026-01-01T00:00:00Z"}}),
        serde_json::json!({"entry":{"sessionId":f.1.id,"pid":123,"cwd":"/work","opened_at":"2026-01-01T00:00:00Z"}}),
    ] {
        fs::write(&path, value.to_string()).unwrap();
        let rows = read_registrations(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, f.1.id);
        assert_eq!(rows[0].pid, Some(123));
        assert!(rows[0].opened_at.is_some());
    }
    for text in ["null", "{", "[{}]", "{\"entry\":true}"] {
        fs::write(&path, text).unwrap();
        assert!(read_registrations(&path).is_none());
    }
    fs::write(&path, format!("[{{\"session_id\":\"{}\"}}]", f.1.id)).unwrap();
    assert_eq!(Snapshot::capture(&f.0).ownership(&f.1), Ownership::Unknown);
}

#[test]
fn background_turns_do_not_replace_primary_state_and_corrupt_records_are_unknown() {
    let f = Fixture::new();
    let active = f.snapshot(Some(live()));
    let background = "{\"type\":\"turn_started\",\"session_relationship\":\"background\"}\n";
    f.events(&(f.start() + background + END));
    assert_eq!(active.status(&f.1), "running");
    f.events(&(f.start() + END + background + END));
    assert_eq!(active.status(&f.1), "recent");
    for invalid in ["broken\n", "42\n", "{}\n", "{\"type\":\"turn_started\"}\n",
        "{\"type\":\"turn_started\",\"session_relationship\":\"primary\",\"session_id\":\"other\"}\n"] {
        f.events(&(f.start() + invalid + END));
        assert_eq!(active.status(&f.1), "unknown");
    }
    f.events(&(f.start() + "{\"type\":\"turn_ended\"}\n"));
    assert_eq!(active.status(&f.1), "unknown");
}

#[test]
fn append_cache_ignores_partial_tail_and_resets_on_truncate_or_replace() {
    let f = Fixture::new();
    let path = f.1.path.join("events.jsonl");
    let cache = TurnCache(Mutex::new(VecDeque::new()));
    f.events(&f.start());
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Running);
    let offset = cache.0.lock().unwrap()[0].offset;
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&END.as_bytes()[..20]).unwrap();
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Running);
    assert_eq!(cache.0.lock().unwrap()[0].offset, offset);
    file.write_all(&END.as_bytes()[20..]).unwrap();
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Ended);
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Ended);
    f.events("");
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Empty);
    f.events(&(f.start() + END));
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Ended);
    let replacement = f.1.path.join("replacement");
    fs::write(&replacement, f.start()).unwrap();
    fs::rename(replacement, &path).unwrap();
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Running);
}

#[test]
fn bounded_reads_resume_without_poisoning_a_line_at_the_budget_boundary() {
    let f = Fixture::new();
    let cache = TurnCache(Mutex::new(VecDeque::new()));
    let padding = format!(
        "{{\"type\":\"other\",\"text\":\"{}\"}}\n",
        "x".repeat(60_000)
    );
    f.events(&(f.start() + &padding.repeat(150) + END));
    let path = f.1.path.join("events.jsonl");
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Unknown);
    assert_eq!(cache.read(&path, &f.1.id).unwrap(), Phase::Ended);
    assert!(cache.0.lock().unwrap()[0].caught_up);
}

#[test]
fn list_and_detail_share_exited_registration_result_without_changing_native_files() {
    let f = Fixture::new();
    f.events(&(f.start() + END));
    let before = super::super::tests::directory_snapshot(&f.1.path).unwrap();
    fs::write(f.0.home.join("active_sessions.json"), serde_json::json!([{
        "session_id":f.1.id, "pid":std::process::id(), "opened_at":"2000-01-01T00:00:00Z", "cwd":"/work"
    }]).to_string()).unwrap();
    assert_eq!(
        super::super::list_grok_sessions(&f.0, 10, None).unwrap()[0].status,
        "recent"
    );
    assert_eq!(
        super::super::grok_session_detail(&f.0, &f.1.id, None)
            .unwrap()
            .summary
            .status,
        "recent"
    );
    assert!(super::super::preview_grok_delete(&f.0, &f.1.id).is_ok());
    assert_eq!(
        super::super::tests::directory_snapshot(&f.1.path).unwrap(),
        before
    );
}
