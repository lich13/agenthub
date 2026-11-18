use super::*;
use std::time::SystemTime;

const THREAD: &str = "11111111-1111-4111-8111-111111111111";
const CHILD: &str = "22222222-2222-4222-8222-222222222222";
const REVERT: &str = "33333333-3333-4333-8333-333333333333";

struct Home {
    root: PathBuf,
    paths: CodexPaths,
}

impl Home {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("agenthub-account-scope-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("sessions/2026/01/01")).unwrap();
        Self {
            paths: CodexPaths::new(&root),
            root,
        }
    }

    fn rollout(&self, thread: &str, rollout: &str) -> PathBuf {
        let ids = if thread == rollout {
            thread.to_owned()
        } else {
            format!("{thread}_{rollout}")
        };
        self.root
            .join("sessions/2026/01/01")
            .join(format!("rollout-2026-01-01T00-00-00-{ids}.jsonl"))
    }

    fn plain(&self, thread: &str, rollout: &str, scoped: bool) -> PathBuf {
        let path = self.rollout(thread, rollout);
        fs::write(&path, body(thread, scoped)).unwrap();
        age(&path);
        path
    }

    fn compressed(&self, thread: &str, rollout: &str, scoped: bool) -> PathBuf {
        let path = rollout_file::compressed_path(&self.rollout(thread, rollout));
        fs::write(
            &path,
            zstd::stream::encode_all(body(thread, scoped).as_bytes(), 3).unwrap(),
        )
        .unwrap();
        age(&path);
        path
    }

    fn lock(&self, relative: &str) -> File {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(path).unwrap();
        file.lock().unwrap();
        file
    }

    fn threads(&self, rows: &[(&str, &Path)], edges: &[(&str, &str)]) {
        let conn = rusqlite::Connection::open(self.paths.state_db()).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT);
             CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT);",
        )
        .unwrap();
        for (id, path) in rows {
            conn.execute(
                "INSERT INTO threads VALUES (?1, ?2)",
                [*id, path.to_str().unwrap()],
            )
            .unwrap();
        }
        for edge in edges {
            conn.execute("INSERT INTO thread_spawn_edges VALUES (?1, ?2)", *edge)
                .unwrap();
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn header(thread: &str, scoped: bool) -> String {
    let marker = if scoped {
        r#""account_scoped":true,"#
    } else {
        ""
    };
    format!(
        r#"{{"timestamp":"2026-01-01T00:00:00.000Z","type":"session_meta","payload":{{{marker}"id":"{thread}","cwd":"/fixture","originator":"fixture"}}}}"#
    ) + "\n"
}

fn body(thread: &str, scoped: bool) -> String {
    header(thread, scoped)
        + "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"fixture\"}}\n"
}

fn cleared(thread: &str) -> String {
    body(thread, true).replace(r#""account_scoped":true,"#, &" ".repeat(MARKER.len()))
}

fn age(path: &Path) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap();
}

fn decoded(path: &Path) -> Vec<u8> {
    zstd::stream::decode_all(File::open(path).unwrap()).unwrap()
}

fn reasons(result: &CodexAccountScopeResult) -> Vec<(&str, CodexAccountScopeReason)> {
    result
        .items
        .iter()
        .map(|item| (item.thread_id.as_str(), item.reason))
        .collect()
}

#[test]
fn header_inspection_only_accepts_the_exact_native_marker() {
    let line = header(THREAD, true);
    let Header::Marked(offset) = inspect(line.as_bytes(), THREAD) else {
        panic!("native header must be rewritable");
    };
    assert_eq!(
        &line.as_bytes()[offset..offset + MARKER.len()],
        MARKER,
        "offset must address the marker"
    );
    assert_eq!(
        inspect(header(THREAD, false).as_bytes(), THREAD),
        Header::Clear
    );
    assert_eq!(
        inspect(
            line.replace("\"account_scoped\":true", "\"account_scoped\":false")
                .as_bytes(),
            THREAD
        ),
        Header::Clear
    );
    for unsupported in [
        // Another serializer's spacing.
        line.replace("\"account_scoped\":true,", "\"account_scoped\": true,"),
        // The marker is the last field, so no trailing comma can be blanked.
        format!(r#"{{"type":"session_meta","payload":{{"id":"{THREAD}","account_scoped":true}}}}"#),
        // The first textual match belongs to another object.
        format!(
            r#"{{"type":"session_meta","extra":{{"account_scoped":true,"kept":1}},"payload":{{"account_scoped":true,"id":"{THREAD}"}}}}"#
        ),
    ] {
        assert_eq!(
            inspect(unsupported.as_bytes(), THREAD),
            Header::Unsupported,
            "{unsupported}"
        );
    }
    // The header belongs to another thread: never rewritten, and never proof of a clear original.
    assert_eq!(inspect(line.as_bytes(), CHILD), Header::Unsupported);
    assert_eq!(
        inspect(header(THREAD, false).as_bytes(), CHILD),
        Header::Unreadable
    );
    // Quoted marker text inside a string is escaped and never matches the raw bytes.
    let quoted = format!(
        r#"{{"type":"session_meta","payload":{{"note":"\"account_scoped\":true,","id":"{THREAD}"}}}}"#
    );
    assert_eq!(inspect(quoted.as_bytes(), THREAD), Header::Clear);
    for unreadable in ["", "{\"type\":\"event_msg\"}", "{\"type\":\"session_meta\""] {
        assert_eq!(inspect(unreadable.as_bytes(), THREAD), Header::Unreadable);
    }
}

#[cfg(unix)]
#[test]
fn plain_rollout_is_rewritten_in_place_under_an_open_appender() {
    use std::os::unix::fs::MetadataExt;
    let home = Home::new();
    let path = home.plain(THREAD, THREAD, true);
    let before = fs::metadata(&path).unwrap();
    let mut appender = File::options().append(true).open(&path).unwrap();
    assert_eq!(
        status(&home.paths),
        CodexAccountScopeStatus {
            scoped_threads: 1,
            scoped_files: 1
        }
    );

    let result = clear(&home.paths).unwrap();
    assert_eq!(
        (
            result.cleared_threads,
            result.cleared_files,
            result.remaining_files
        ),
        (1, 1, 0)
    );
    assert!(result.items.is_empty());
    let after = fs::metadata(&path).unwrap();
    assert_eq!(
        (after.ino(), after.len(), after.modified().unwrap()),
        (before.ino(), before.len(), before.modified().unwrap())
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), cleared(THREAD));
    assert_eq!(status(&home.paths), CodexAccountScopeStatus::default());
    // No lock file or directory may be created on Codex's behalf.
    assert!(!home.root.join("thread-writer-locks").exists());
    assert!(!home.root.join(".tmp").exists());

    let again = clear(&home.paths).unwrap();
    assert_eq!(again, CodexAccountScopeResult::default());
    appender.write_all(b"{\"type\":\"appended\"}\n").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        cleared(THREAD) + "{\"type\":\"appended\"}\n"
    );
}

#[test]
fn rewritten_header_rebuilds_cached_snapshots() {
    let home = Home::new();
    let path = home.plain(THREAD, THREAD, true);
    let cache = crate::jsonl_cache::JsonlCache::<Vec<String>>::new(1024 * 1024);
    let read = || {
        rollout_file::read_cached(
            &cache,
            &path,
            true,
            |lines, line, _, _| {
                lines.push(String::from_utf8_lossy(line).into_owned());
                Ok(())
            },
            |lines, _| Ok(lines.clone()),
            |lines| lines.iter().map(|line| line.len() as u64).sum(),
        )
        .unwrap()
    };
    assert!(read()[0].contains("account_scoped"));
    clear(&home.paths).unwrap();
    let lines = read();
    assert_eq!(lines.len(), 2);
    assert!(!lines[0].contains("account_scoped"));
}

#[cfg(unix)]
#[test]
fn compressed_rollout_is_replaced_by_a_verified_copy() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let home = Home::new();
    let path = home.compressed(THREAD, THREAD, true);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    age(&path);
    let before = fs::metadata(&path).unwrap();
    let _coordination = File::create({
        fs::create_dir_all(home.root.join("thread-writer-locks")).unwrap();
        home.root.join("thread-writer-locks/.coordination.lock")
    })
    .unwrap();

    let result = clear(&home.paths).unwrap();
    assert_eq!((result.cleared_threads, result.cleared_files), (1, 1));
    assert!(result.items.is_empty());
    assert_eq!(decoded(&path), cleared(THREAD).into_bytes());
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        zstd::zstd_safe::get_frame_content_size(&bytes).unwrap(),
        Some(cleared(THREAD).len() as u64)
    );
    let after = fs::metadata(&path).unwrap();
    assert_ne!(after.ino(), before.ino());
    assert_eq!(
        (
            after.mode(),
            after.uid(),
            after.gid(),
            after.modified().unwrap()
        ),
        (
            before.mode(),
            before.uid(),
            before.gid(),
            before.modified().unwrap()
        )
    );
    assert_eq!(
        fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1,
        "temporary copy must not remain"
    );
    assert_eq!(status(&home.paths), CodexAccountScopeStatus::default());
}

#[test]
fn compressed_rollout_is_left_alone_while_codex_may_use_it() {
    let home = Home::new();
    let path = home.compressed(THREAD, THREAD, true);
    let original = fs::read(&path).unwrap();
    let unchanged = |reason| {
        let result = clear(&home.paths).unwrap();
        assert_eq!(result.cleared_files, 0);
        assert_eq!(reasons(&result), [(THREAD, reason)]);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    };
    // Without Codex's coordination lock the replacement cannot be serialized.
    unchanged(CodexAccountScopeReason::Busy);
    assert!(!home.root.join("thread-writer-locks").exists());

    let coordination = home.lock("thread-writer-locks/.coordination.lock");
    drop(coordination);
    let writer = home.lock(&format!("thread-writer-locks/{THREAD}.lock"));
    unchanged(CodexAccountScopeReason::Busy);
    drop(writer);

    // A plain sibling means Codex already materialized the thread; the archive copy is stale.
    let plain = home.plain(THREAD, THREAD, false);
    assert_eq!(
        clear(&home.paths).unwrap(),
        CodexAccountScopeResult::default()
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_file(plain).unwrap();

    assert_eq!(clear(&home.paths).unwrap().cleared_files, 1);
    assert_eq!(decoded(&path), cleared(THREAD).into_bytes());
}

#[test]
fn maintenance_in_progress_rejects_the_pass() {
    let home = Home::new();
    let path = home.plain(THREAD, THREAD, true);
    home.threads(&[(THREAD, &path)], &[]);
    let maintenance = home.lock(".tmp/rollout-maintenance.lock");
    assert!(clear(&home.paths)
        .unwrap_err()
        .to_string()
        .contains("正在维护"));
    assert_eq!(clear_thread_tree(&home.paths, THREAD).unwrap(), 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), body(THREAD, true));
    drop(maintenance);
    assert_eq!(clear(&home.paths).unwrap().cleared_files, 1);
}

#[test]
fn reverted_copies_follow_their_original() {
    // Both marked: the original goes first, then the copy.
    let home = Home::new();
    let original = home.plain(THREAD, THREAD, true);
    let copy = home.plain(THREAD, REVERT, true);
    let result = clear(&home.paths).unwrap();
    assert_eq!((result.cleared_threads, result.cleared_files), (1, 2));
    assert_eq!(fs::read_to_string(&original).unwrap(), cleared(THREAD));
    assert_eq!(fs::read_to_string(&copy).unwrap(), cleared(THREAD));

    // An already unmarked original allows the copy.
    let home = Home::new();
    home.plain(THREAD, THREAD, false);
    let copy = home.plain(THREAD, REVERT, true);
    assert_eq!(clear(&home.paths).unwrap().cleared_files, 1);
    assert_eq!(fs::read_to_string(&copy).unwrap(), cleared(THREAD));

    // Without an original the unmarked copy would hide the thread from everyone.
    let home = Home::new();
    let copy = home.plain(THREAD, REVERT, true);
    let result = clear(&home.paths).unwrap();
    assert_eq!(
        reasons(&result),
        [(THREAD, CodexAccountScopeReason::OriginalMissing)]
    );
    assert_eq!((result.cleared_threads, result.cleared_files), (0, 0));
    assert_eq!(fs::read_to_string(&copy).unwrap(), body(THREAD, true));

    // Codex rejects an original whose header names another thread, so it does not count either.
    let home = Home::new();
    fs::write(home.rollout(THREAD, THREAD), body(CHILD, false)).unwrap();
    let copy = home.plain(THREAD, REVERT, true);
    assert_eq!(
        reasons(&clear(&home.paths).unwrap()),
        [(THREAD, CodexAccountScopeReason::OriginalMissing)]
    );
    assert_eq!(fs::read_to_string(&copy).unwrap(), body(THREAD, true));

    // An original that could not be cleared keeps its copies marked and reports its own reason.
    let home = Home::new();
    let original = home.rollout(THREAD, THREAD);
    fs::write(
        &original,
        body(THREAD, true).replace("\"account_scoped\":true,", "\"account_scoped\": true,"),
    )
    .unwrap();
    let copy = home.plain(THREAD, REVERT, true);
    let result = clear(&home.paths).unwrap();
    assert_eq!(
        reasons(&result),
        [(THREAD, CodexAccountScopeReason::Format)]
    );
    assert_eq!(result.cleared_files, 0);
    assert_eq!(fs::read_to_string(&copy).unwrap(), body(THREAD, true));
    assert_eq!(
        status(&home.paths),
        CodexAccountScopeStatus {
            scoped_threads: 1,
            scoped_files: 2
        }
    );
}

#[cfg(unix)]
#[test]
fn symlinked_rollouts_and_directories_are_ignored() {
    let home = Home::new();
    let outside = Home::new();
    let target = outside.plain(THREAD, THREAD, true);
    std::os::unix::fs::symlink(&target, home.rollout(THREAD, THREAD)).unwrap();
    std::os::unix::fs::symlink(
        outside.root.join("sessions"),
        home.root.join("archived_sessions"),
    )
    .unwrap();
    // A recorded path that only resolves through the linked directory is not followed either.
    let linked = outside.plain(CHILD, CHILD, true);
    let recorded = home
        .root
        .join("archived_sessions/2026/01/01")
        .join(linked.file_name().unwrap());
    assert!(recorded.is_file());
    home.threads(
        &[(THREAD, &home.rollout(THREAD, THREAD)), (CHILD, &recorded)],
        &[],
    );
    assert_eq!(status(&home.paths), CodexAccountScopeStatus::default());
    assert_eq!(
        clear(&home.paths).unwrap(),
        CodexAccountScopeResult::default()
    );
    assert_eq!(clear_thread_tree(&home.paths, THREAD).unwrap(), 0);
    assert_eq!(clear_thread_tree(&home.paths, CHILD).unwrap(), 0);
    assert_eq!(fs::read_to_string(&target).unwrap(), body(THREAD, true));
    assert_eq!(fs::read_to_string(&linked).unwrap(), body(CHILD, true));
}

#[cfg(unix)]
#[test]
fn a_linked_home_is_the_configured_root_for_both_entries() {
    let home = Home::new();
    let holder = Home::new();
    let link = holder.root.join("home");
    std::os::unix::fs::symlink(&home.root, &link).unwrap();
    let paths = CodexPaths::new(&link);
    let first = home.plain(THREAD, THREAD, true);
    let second = home.plain(CHILD, CHILD, true);
    home.threads(
        &[(THREAD, &link.join(first.strip_prefix(&home.root).unwrap()))],
        &[],
    );
    assert_eq!(clear_thread_tree(&paths, THREAD).unwrap(), 1);
    assert_eq!(fs::read_to_string(&first).unwrap(), cleared(THREAD));
    assert_eq!(clear(&paths).unwrap().cleared_files, 1);
    assert_eq!(fs::read_to_string(&second).unwrap(), cleared(CHILD));
}

#[test]
fn abandoned_copies_are_removed_unless_a_pass_still_holds_them() {
    let home = Home::new();
    let directory = home.root.join("sessions/2026/01/01");
    let abandoned = directory.join(format!("{COPY_PREFIX}abandoned"));
    let held = directory.join(format!("{COPY_PREFIX}held"));
    fs::write(&abandoned, b"fixture").unwrap();
    let holder = File::create(&held).unwrap();
    holder.lock().unwrap();

    // Checking never removes anything.
    assert_eq!(status(&home.paths), CodexAccountScopeStatus::default());
    assert!(abandoned.exists());
    assert_eq!(
        clear(&home.paths).unwrap(),
        CodexAccountScopeResult::default()
    );
    assert!(!abandoned.exists());
    assert!(held.exists());
    drop(holder);
    clear(&home.paths).unwrap();
    assert!(!held.exists());
}

#[test]
fn exhausted_budget_still_progresses_and_reports_the_rest() {
    let home = Home::new();
    let first = home.plain(THREAD, THREAD, true);
    let second = home.plain(CHILD, CHILD, true);
    let result = clear_within(&home.paths, Duration::ZERO).unwrap();
    assert_eq!(
        (
            result.cleared_threads,
            result.cleared_files,
            result.remaining_files
        ),
        (1, 1, 1)
    );
    assert_eq!(fs::read_to_string(&first).unwrap(), cleared(THREAD));
    assert_eq!(fs::read_to_string(&second).unwrap(), body(CHILD, true));
    let result = clear_within(&home.paths, Duration::ZERO).unwrap();
    assert_eq!((result.cleared_files, result.remaining_files), (1, 0));
    assert_eq!(fs::read_to_string(&second).unwrap(), cleared(CHILD));
}

#[test]
fn stop_hook_entry_covers_the_thread_and_its_descendants_only() {
    let home = Home::new();
    let parent = home.plain(THREAD, THREAD, true);
    let child = home.plain(CHILD, CHILD, true);
    let unrelated = home.plain(REVERT, REVERT, true);
    let outside = Home::new();
    let foreign = outside.plain(THREAD, THREAD, true);
    assert_eq!(clear_thread_tree(&home.paths, THREAD).unwrap(), 0);

    home.threads(
        &[(THREAD, &parent), (CHILD, &child), (REVERT, &unrelated)],
        &[(THREAD, CHILD)],
    );
    assert_eq!(clear_thread_tree(&home.paths, THREAD).unwrap(), 2);
    assert_eq!(fs::read_to_string(&parent).unwrap(), cleared(THREAD));
    assert_eq!(fs::read_to_string(&child).unwrap(), cleared(CHILD));
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), body(REVERT, true));
    assert_eq!(clear_thread_tree(&home.paths, THREAD).unwrap(), 0);

    // A recorded path outside the session roots is never followed.
    outside.threads(&[(THREAD, &foreign)], &[]);
    let conn = rusqlite::Connection::open(home.paths.state_db()).unwrap();
    conn.execute(
        "UPDATE threads SET rollout_path = ?1 WHERE id = ?2",
        [foreign.to_str().unwrap(), REVERT],
    )
    .unwrap();
    assert_eq!(clear_thread_tree(&home.paths, REVERT).unwrap(), 0);
    assert_eq!(fs::read_to_string(&foreign).unwrap(), body(THREAD, true));
}
