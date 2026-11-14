use super::*;
use serde_json::json;
const ROOT: &str = "11111111-1111-4111-8111-111111111111";
const CHILD: &str = "22222222-2222-4222-8222-222222222222";
const OTHER: &str = "33333333-3333-4333-8333-333333333333";
struct Fixture {
    paths: CodexPaths,
}
impl Fixture {
    fn new() -> Self {
        let home =
            std::env::temp_dir().join(format!("agenthub-direct-delete-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(home.join("sessions")).unwrap();
        let paths = CodexPaths { home };
        let conn = Connection::open(paths.state_db()).unwrap();
        conn.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT, parent_thread_id TEXT, source TEXT, status TEXT, archived INTEGER, archived_at INTEGER);
            CREATE TABLE thread_spawn_edges(parent_thread_id TEXT, child_thread_id TEXT);
            CREATE TABLE thread_attachments(thread_id TEXT, attachment_id TEXT);
            CREATE TABLE agent_job_items(id TEXT, assigned_thread_id TEXT, status TEXT);").unwrap();
        let result = Self { paths };
        for id in [ROOT, CHILD, OTHER] {
            fs::write(result.file(id), "broken transcript\n{").unwrap();
            conn.execute(
                "INSERT INTO threads VALUES(?1,?2,?3,'cli','running',0,NULL)",
                rusqlite::params![
                    id,
                    result.file(id).to_string_lossy(),
                    (id == CHILD).then_some(ROOT)
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO thread_attachments VALUES(?1,'attachment')",
                [id],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO thread_spawn_edges VALUES(?1,?2)",
            [ROOT, CHILD],
        )
        .unwrap();
        fs::write(
            result.paths.session_index(),
            [ROOT, CHILD, OTHER]
                .map(|id| format!("{}\n", json!({"id":id})))
                .concat()
                + "broken index line\n",
        )
        .unwrap();
        result
    }
    fn file(&self, id: &str) -> PathBuf {
        self.paths
            .sessions_dir()
            .join(format!("rollout-2026-01-01T00-00-00-{id}.jsonl"))
    }
    fn conn(&self) -> Connection {
        Connection::open(self.paths.state_db()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.paths.home);
    }
}

#[test]
fn batch_delete_handles_overlapping_trees_and_keeps_unselected_rows() {
    let f = Fixture::new();
    let results = execute_batch(
        &f.paths,
        &[
            vec![ROOT.into(), CHILD.into()],
            vec![CHILD.into()],
            vec!["../outside".into()],
        ],
    );
    assert_eq!(results.len(), 3);
    assert!(results[0].as_ref().unwrap().residuals.is_empty());
    assert_eq!(results[1].as_ref().unwrap().deleted_ids, [CHILD]);
    assert!(results[2].is_err());
    assert!(!f.file(ROOT).exists());
    assert!(!f.file(CHILD).exists());
    assert!(f.file(OTHER).exists());
    assert_eq!(
        f.conn()
            .query_row("SELECT count(*) FROM threads", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(fs::read_to_string(f.paths.session_index())
        .unwrap()
        .contains(OTHER));
}

#[cfg(unix)]
#[test]
fn batch_delete_continues_unrelated_targets_after_file_failure() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let blocked = f.paths.sessions_dir().join("blocked");
    fs::create_dir(&blocked).unwrap();
    let moved = blocked.join(f.file(ROOT).file_name().unwrap());
    fs::rename(f.file(ROOT), &moved).unwrap();
    f.conn()
        .execute(
            "UPDATE threads SET rollout_path=?1 WHERE id=?2",
            rusqlite::params![moved.to_string_lossy(), ROOT],
        )
        .unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o555)).unwrap();
    let results = execute_batch(&f.paths, &[vec![ROOT.into()], vec![OTHER.into()]]);
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!results[0].as_ref().unwrap().residuals.is_empty());
    assert!(results[1].as_ref().unwrap().residuals.is_empty());
    assert!(moved.exists());
    assert!(!f.file(OTHER).exists());
    let index = fs::read_to_string(f.paths.session_index()).unwrap();
    assert!(index.contains(ROOT) && index.contains(CHILD));
    assert!(!index.contains(OTHER));
}

#[test]
fn direct_delete_removes_corrupt_active_tree_compressed_files_indexes_and_sidecars() {
    let f = Fixture::new();
    let history = Connection::open(f.paths.home.join("thread_history_2.sqlite")).unwrap();
    history
        .execute_batch(
            "CREATE TABLE thread_items(thread_id TEXT); CREATE TABLE future_rows(thread_id TEXT)",
        )
        .unwrap();
    for table in ["thread_items", "future_rows"] {
        for id in [ROOT, OTHER] {
            history
                .execute(&format!("INSERT INTO {table} VALUES(?1)"), [id])
                .unwrap();
        }
    }
    for directory in [
        "thread-writer-locks",
        "shell_snapshots",
        "visualizations/day",
    ] {
        fs::create_dir_all(f.paths.home.join(directory)).unwrap();
    }
    let lock = f
        .paths
        .home
        .join(format!("thread-writer-locks/{ROOT}.lock"));
    fs::write(&lock, "").unwrap();
    let open_lock = fs::File::open(&lock).unwrap();
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        assert_eq!(
            unsafe { libc::flock(open_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
    }
    fs::write(
        f.paths.home.join(format!("shell_snapshots/{ROOT}.sh")),
        "fixture",
    )
    .unwrap();
    let visual = f.paths.home.join("visualizations/day").join(ROOT);
    fs::create_dir_all(&visual).unwrap();
    fs::write(visual.join("view.html"), "fixture").unwrap();
    fs::write(rollout_file::compressed_path(&f.file(ROOT)), "invalid zstd").unwrap();
    let preview = preview(&f.paths, ROOT, true).unwrap();
    assert_eq!(preview.ids.len(), 2);
    let outcome = execute_ids(&f.paths, &preview.ids).unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.residuals);
    assert!(!f.file(ROOT).exists());
    assert!(!f.file(CHILD).exists());
    assert!(f.file(OTHER).exists());
    assert!(!visual.exists());
    assert!(!lock.exists());
    for table in ["thread_items", "future_rows"] {
        assert_eq!(
            history
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    assert!(fs::read_to_string(f.paths.session_index())
        .unwrap()
        .contains("broken index line"));
    assert!(execute_ids(&f.paths, &preview.ids).unwrap().error.is_none());
}

#[test]
fn frozen_ids_do_not_expand_to_new_descendants_or_reverse_references() {
    let f = Fixture::new();
    let preview = preview(&f.paths, ROOT, true).unwrap();
    f.conn()
        .execute(
            "UPDATE threads SET parent_thread_id=?1, rollout_path=?2 WHERE id=?3",
            rusqlite::params![ROOT, f.file(ROOT).to_string_lossy(), OTHER],
        )
        .unwrap();
    let outcome = execute_ids(&f.paths, &preview.ids).unwrap();
    assert!(outcome.error.is_none());
    assert!(row_value(&f.conn(), "threads", "id", OTHER)
        .unwrap()
        .is_some());
    assert!(f.file(OTHER).exists());
}

#[test]
fn cycles_and_missing_child_rows_are_deduplicated() {
    let f = Fixture::new();
    f.conn()
        .execute(
            "INSERT INTO thread_spawn_edges VALUES(?1,?2)",
            [CHILD, ROOT],
        )
        .unwrap();
    f.conn()
        .execute(
            "INSERT INTO thread_spawn_edges VALUES(?1,'missing-child')",
            [CHILD],
        )
        .unwrap();
    let ids = descendants(&f.paths, &[ROOT.into()]).unwrap();
    assert_eq!(ids.len(), 3);
    assert!(execute_ids(&f.paths, &ids).unwrap().error.is_none());
}

#[cfg(unix)]
#[test]
fn symlinks_are_unlinked_without_following_their_targets() {
    let f = Fixture::new();
    fs::remove_file(f.file(ROOT)).unwrap();
    std::os::unix::fs::symlink(f.file(OTHER), f.file(ROOT)).unwrap();
    let outcome = execute_ids(&f.paths, &[ROOT.into()]).unwrap();
    assert!(outcome.error.is_none());
    assert!(fs::symlink_metadata(f.file(ROOT)).is_err());
    assert!(f.file(OTHER).exists());
}

#[test]
fn database_failure_reports_partial_deletion_and_other_targets_continue() {
    let f = Fixture::new();
    f.conn().execute_batch(&format!("CREATE TRIGGER fail_delete BEFORE DELETE ON threads WHEN OLD.id='{ROOT}' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")).unwrap();
    let result = execute_ids(&f.paths, &[ROOT.into()]).unwrap();
    assert!(result.error.is_some());
    assert!(!result.residuals.is_empty());
    assert!(!f.file(ROOT).exists());
    assert!(execute_ids(&f.paths, &[OTHER.into()])
        .unwrap()
        .error
        .is_none());
    assert!(!f.file(OTHER).exists());
}

#[test]
fn missing_rollout_still_removes_rollout_keyed_history_and_exact_prompt_rows() {
    let f = Fixture::new();
    let rollout = "44444444-4444-4444-8444-444444444444";
    let path = f.paths.home.join("archived_sessions").join(format!(
        "rollout-2026-01-01T00-00-00-{ROOT}_{rollout}.jsonl.zst"
    ));
    f.conn()
        .execute(
            "UPDATE threads SET rollout_path=?1 WHERE id=?2",
            rusqlite::params![path.to_string_lossy(), ROOT],
        )
        .unwrap();
    fs::remove_file(f.file(ROOT)).unwrap();
    let history = Connection::open(f.paths.home.join("thread_history_2.sqlite")).unwrap();
    history
        .execute_batch("CREATE TABLE thread_turns(rollout_id TEXT, content TEXT)")
        .unwrap();
    history
        .execute(
            "INSERT INTO thread_turns VALUES(?1,'fixture'),(?2,'retained')",
            [rollout, OTHER],
        )
        .unwrap();
    fs::write(
        f.paths.home.join("history.jsonl"),
        format!(
            "{}\n{}\n",
            json!({"session_id":ROOT,"text":"fixture"}),
            json!({"session_id":OTHER,"text":"retained"})
        ),
    )
    .unwrap();
    let result = execute_ids(&f.paths, &[ROOT.into()]).unwrap();
    assert!(result.error.is_none(), "{:?}", result.residuals);
    assert_eq!(
        history
            .query_row("SELECT rollout_id FROM thread_turns", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        OTHER
    );
    assert!(!fs::read_to_string(f.paths.home.join("history.jsonl"))
        .unwrap()
        .contains(ROOT));
    assert!(fs::read_to_string(f.paths.home.join("history.jsonl"))
        .unwrap()
        .contains(OTHER));
}

#[test]
fn archive_and_restore_still_require_a_matching_preview() {
    let f = Fixture::new();
    Connection::open(f.paths.state_db())
        .unwrap()
        .execute("UPDATE threads SET status='completed'", [])
        .unwrap();
    fs::write(
        f.file(ROOT),
        format!(
            "{}\n{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":ROOT}}),
            json!({"type":"turn_started","turn_id":"turn"}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn"}})
        ),
    )
    .unwrap();
    // Archive validation still rejects a partial/unreadable shared index.
    assert!(super::super::preview_archive(&f.paths, ROOT, true).is_err());
    fs::write(f.paths.session_index(), format!("{}\n", json!({"id":ROOT}))).unwrap();
    let p = super::super::preview_archive(&f.paths, ROOT, true).unwrap();
    assert!(super::super::execute_archive(&f.paths, ROOT, true, "stale").is_err());
    super::super::execute_archive(&f.paths, ROOT, true, &p.fingerprint).unwrap();
    let p = super::super::preview_archive(&f.paths, ROOT, false).unwrap();
    super::super::execute_archive(&f.paths, ROOT, false, &p.fingerprint).unwrap();
}

#[test]
fn estimate_bytes_batch_matches_single_preview_across_rollouts_locks_snapshots_and_visualizations()
{
    let f = Fixture::new();
    let locks_dir = f.paths.home.join("thread-writer-locks");
    let snapshots_dir = f.paths.home.join("shell_snapshots");
    let viz_dir = f.paths.home.join("visualizations").join(ROOT);
    fs::create_dir_all(&locks_dir).unwrap();
    fs::create_dir_all(&snapshots_dir).unwrap();
    fs::create_dir_all(&viz_dir).unwrap();
    fs::write(locks_dir.join(format!("{ROOT}.lock")), b"lock-bytes").unwrap();
    fs::write(snapshots_dir.join(format!("{ROOT}.sh")), b"snapshot-bytes").unwrap();
    fs::write(viz_dir.join("chart.json"), b"{\"chart\":true}").unwrap();

    let single_root = preview(&f.paths, ROOT, false).unwrap();
    let single_child = preview(&f.paths, CHILD, false).unwrap();
    let single_other = preview(&f.paths, OTHER, false).unwrap();

    let batch = estimate_bytes_batch(
        &f.paths,
        &[
            ROOT.into(),
            CHILD.into(),
            OTHER.into(),
            "../invalid-id".into(),
            "00000000-0000-4000-8000-999999999999".into(),
        ],
    );

    assert_eq!(batch.get(ROOT).copied(), Some(single_root.selected.bytes));
    assert_eq!(batch.get(CHILD).copied(), Some(single_child.selected.bytes));
    assert_eq!(batch.get(OTHER).copied(), Some(single_other.selected.bytes));
    assert!(!batch.contains_key("../invalid-id"));
    assert!(!batch.contains_key("00000000-0000-4000-8000-999999999999"));
}
