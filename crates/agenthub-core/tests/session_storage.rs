use agenthub_core::{
    claude::{list_claude_sessions, ClaudePaths},
    codex::{list_threads, CodexPaths},
    grok::{list_grok_sessions, GrokPaths},
    session_storage::{directory_size, file_size, SessionStorageSize, StorageScope, StorageStatus},
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = env::temp_dir().join(format!("agenthub-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_for_complete(
    root: &Path,
    directory: &Path,
    primary: Option<&Path>,
    owner: &str,
) -> SessionStorageSize {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value = directory_size(root, directory, primary, owner);
        if value.status == StorageStatus::Complete {
            return value;
        }
        assert_eq!(value.status, StorageStatus::Pending);
        assert!(
            Instant::now() < deadline,
            "directory size remained pending: {value:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn storage_size_uses_the_public_lowercase_wire_shape() {
    let value = SessionStorageSize {
        bytes: Some(42),
        scope: StorageScope::Directory,
        status: StorageStatus::Complete,
    };
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        json!({"bytes": 42, "scope": "directory", "status": "complete"})
    );
    assert_eq!(
        serde_json::to_value(StorageScope::File).unwrap(),
        json!("file")
    );
    assert_eq!(
        serde_json::to_value(StorageStatus::Partial).unwrap(),
        json!("partial")
    );
    assert_eq!(
        serde_json::to_value(StorageStatus::Pending).unwrap(),
        json!("pending")
    );
    assert_eq!(
        serde_json::to_value(StorageStatus::Unavailable).unwrap(),
        json!("unavailable")
    );
}

#[test]
fn file_size_counts_metadata_without_reading_content_or_following_links() {
    let temp = TempDir::new("storage-file");
    let file = temp.path().join("session.jsonl");
    fs::write(&file, b"metadata-only").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    }

    let size = file_size(temp.path(), &file);
    assert_eq!(size.bytes, Some(13));
    assert_eq!(size.scope, StorageScope::File);
    assert_eq!(size.status, StorageStatus::Complete);

    let missing = file_size(temp.path(), &temp.path().join("missing.jsonl"));
    assert_eq!(missing, SessionStorageSize::unavailable(StorageScope::File));

    #[cfg(unix)]
    {
        let target = temp.path().join("target.jsonl");
        let link = temp.path().join("link.jsonl");
        fs::write(&target, b"outside link target").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            file_size(temp.path(), &link),
            SessionStorageSize::unavailable(StorageScope::File)
        );
    }
}

#[test]
fn directory_size_is_pending_then_counts_logical_files_once() {
    let temp = TempDir::new("storage-directory");
    let directory = temp.path().join("native-id");
    let nested = directory.join("nested");
    let primary = temp.path().join("native-id.jsonl");
    fs::create_dir_all(&nested).unwrap();
    fs::write(&primary, b"primary").unwrap();
    fs::write(nested.join("events.jsonl"), b"nested events").unwrap();
    let unreadable = nested.join("metadata-only.bin");
    fs::write(&unreadable, b"unreadable bytes").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        let outside = temp
            .path()
            .with_file_name(format!("agenthub-storage-outside-{}", uuid::Uuid::new_v4()));
        fs::write(&outside, vec![b'x'; 4096]).unwrap();
        std::os::unix::fs::symlink(&outside, nested.join("external-link")).unwrap();
        let _ = fs::remove_file(&outside);
    }

    let first = directory_size(temp.path(), &directory, Some(&primary), "fixture-owner");
    assert_eq!(first.bytes, None);
    assert_eq!(first.scope, StorageScope::Directory);
    assert_eq!(first.status, StorageStatus::Pending);

    let expected = fs::metadata(&primary).unwrap().len()
        + fs::metadata(nested.join("events.jsonl")).unwrap().len()
        + fs::metadata(&unreadable).unwrap().len();
    let ready = wait_for_complete(temp.path(), &directory, Some(&primary), "fixture-owner");
    assert_eq!(ready.bytes, Some(expected));
    assert_eq!(ready.scope, StorageScope::Directory);
    assert_eq!(ready.status, StorageStatus::Complete);
    assert_eq!(
        directory_size(temp.path(), &directory, Some(&primary), "fixture-owner"),
        ready
    );
}

#[test]
fn directory_size_invalidates_a_truncated_or_replaced_primary() {
    let temp = TempDir::new("storage-invalidation");
    let directory = temp.path().join("native-id");
    let primary = temp.path().join("native-id.jsonl");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&primary, b"original-primary").unwrap();
    fs::write(directory.join("state"), b"state").unwrap();

    let first = wait_for_complete(
        temp.path(),
        &directory,
        Some(&primary),
        "invalidation-owner",
    );
    let original = first.bytes.unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&primary)
        .unwrap()
        .set_len(1)
        .unwrap();
    let pending = directory_size(
        temp.path(),
        &directory,
        Some(&primary),
        "invalidation-owner",
    );
    assert_eq!(pending.status, StorageStatus::Pending);
    assert_eq!(pending.bytes, None);

    let refreshed = wait_for_complete(
        temp.path(),
        &directory,
        Some(&primary),
        "invalidation-owner",
    );
    assert_eq!(refreshed.bytes, Some(original - 15));
    assert_ne!(refreshed.bytes, Some(original));
}

#[test]
fn directory_size_keeps_completed_cache_for_primary_append() {
    let temp = TempDir::new("storage-append-cache");
    let directory = temp.path().join("native-id");
    let primary = temp.path().join("native-id.jsonl");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&primary, b"original-primary").unwrap();
    fs::write(directory.join("state"), b"state").unwrap();

    let owner = "append-cache-owner";
    let first = wait_for_complete(temp.path(), &directory, Some(&primary), owner);
    let original = first.bytes.unwrap();

    let mut file = fs::OpenOptions::new().append(true).open(&primary).unwrap();
    file.write_all(b"-appended").unwrap();
    drop(file);
    assert!(fs::metadata(&primary).unwrap().len() > original);
    let appended = directory_size(temp.path(), &directory, Some(&primary), owner);
    assert_eq!(appended.scope, StorageScope::Directory);
    assert_eq!(appended.status, StorageStatus::Complete);
    assert_eq!(appended.bytes, Some(original));

    fs::OpenOptions::new()
        .write(true)
        .open(&primary)
        .unwrap()
        .set_len(1)
        .unwrap();
    let truncated = directory_size(temp.path(), &directory, Some(&primary), owner);
    assert_eq!(truncated.status, StorageStatus::Pending);
    assert_eq!(truncated.bytes, None);
    let _ = wait_for_complete(temp.path(), &directory, Some(&primary), owner);

    let replacement = temp.path().join("replacement.jsonl");
    fs::write(&replacement, b"replacement-primary").unwrap();
    #[cfg(unix)]
    fs::rename(&replacement, &primary).unwrap();
    #[cfg(not(unix))]
    {
        fs::remove_file(&primary).unwrap();
        fs::rename(&replacement, &primary).unwrap();
    }
    let replaced = directory_size(temp.path(), &directory, Some(&primary), owner);
    assert_eq!(replaced.status, StorageStatus::Pending);
    assert_eq!(replaced.bytes, None);
}

fn wait_for_partial(root: &Path, directory: &Path, owner: &str) -> SessionStorageSize {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let value = directory_size(root, directory, None, owner);
        if value.status == StorageStatus::Partial {
            return value;
        }
        assert_eq!(value.status, StorageStatus::Pending);
        assert!(
            Instant::now() < deadline,
            "directory size remained pending: {value:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn directory_size_marks_depth_and_entry_bounds_partial() {
    let deep = TempDir::new("storage-depth-bound");
    let deep_directory = deep.path().join("thread");
    fs::create_dir_all(&deep_directory).unwrap();
    let mut cursor = deep_directory.clone();
    for index in 0..17 {
        cursor = cursor.join(format!("level-{index}"));
        fs::create_dir_all(&cursor).unwrap();
    }
    fs::write(cursor.join("too-deep"), b"x").unwrap();
    let deep_size = wait_for_partial(deep.path(), &deep_directory, "depth-bound");
    assert_eq!(deep_size.scope, StorageScope::Directory);
    assert!(deep_size.bytes.is_some());

    let many = TempDir::new("storage-entry-bound");
    let many_directory = many.path().join("thread");
    fs::create_dir_all(&many_directory).unwrap();
    for index in 0..10_001 {
        fs::write(many_directory.join(format!("entry-{index}")), b"x").unwrap();
    }
    let many_size = wait_for_partial(many.path(), &many_directory, "entry-bound");
    assert_eq!(many_size.scope, StorageScope::Directory);
    assert!(many_size.bytes.is_some_and(|bytes| bytes <= 10_001));
}

#[cfg(unix)]
#[test]
fn storage_size_rejects_symlink_roots_and_directories() {
    let temp = TempDir::new("storage-links");
    let real_root = temp.path().join("real");
    fs::create_dir_all(real_root.join("thread")).unwrap();
    let file = real_root.join("thread.jsonl");
    fs::write(&file, b"primary").unwrap();
    let root_link = temp.path().join("root-link");
    std::os::unix::fs::symlink(&real_root, &root_link).unwrap();
    assert_eq!(
        file_size(&root_link, &root_link.join("thread.jsonl")),
        SessionStorageSize::unavailable(StorageScope::File)
    );

    let directory_link = temp.path().join("thread-link");
    std::os::unix::fs::symlink(real_root.join("thread"), &directory_link).unwrap();
    assert_eq!(
        directory_size(&real_root, &directory_link, Some(&file), "fixture-owner"),
        SessionStorageSize::unavailable(StorageScope::Directory)
    );
}

#[cfg(unix)]
#[test]
fn directory_size_rejects_cached_sources_replaced_with_symlinks() {
    for replace_primary in [true, false] {
        let temp = TempDir::new("storage-cached-link");
        let directory = temp.path().join("native-id");
        let primary = temp.path().join("native-id.jsonl");
        fs::create_dir(&directory).unwrap();
        fs::write(&primary, b"primary").unwrap();
        fs::write(directory.join("state"), b"state").unwrap();
        let owner = "cached-link-owner";
        let initial = wait_for_complete(temp.path(), &directory, Some(&primary), owner);
        assert_eq!(initial.bytes, Some(12));

        let target = temp.path().join("unrelated-source");
        let source = if replace_primary {
            fs::write(&target, vec![b'x'; 4096]).unwrap();
            &primary
        } else {
            fs::create_dir(&target).unwrap();
            fs::write(target.join("unrelated-state"), vec![b'x'; 4096]).unwrap();
            &directory
        };
        fs::rename(source, temp.path().join("original-source")).unwrap();
        std::os::unix::fs::symlink(&target, source).unwrap();

        assert_eq!(
            directory_size(temp.path(), &directory, Some(&primary), owner),
            SessionStorageSize::unavailable(StorageScope::Directory)
        );
        if replace_primary {
            assert_eq!(
                file_size(temp.path(), &primary),
                SessionStorageSize::unavailable(StorageScope::File)
            );
        }

        fs::remove_file(source).unwrap();
        fs::rename(&target, source).unwrap();
        let pending = directory_size(temp.path(), &directory, Some(&primary), owner);
        assert_eq!(pending.status, StorageStatus::Pending);
        assert_eq!(pending.bytes, None);
        let refreshed = wait_for_complete(temp.path(), &directory, Some(&primary), owner);
        assert_eq!(
            refreshed.bytes,
            Some(4096 + if replace_primary { 5 } else { 7 })
        );
    }
}

#[cfg(unix)]
#[test]
fn directory_size_invalidates_replaced_directory_with_unchanged_timestamp() {
    let temp = TempDir::new("storage-directory-identity");
    let directory = temp.path().join("native-id");
    let primary = temp.path().join("native-id.jsonl");
    fs::create_dir(&directory).unwrap();
    fs::write(&primary, b"primary").unwrap();
    fs::write(directory.join("state"), b"old").unwrap();
    let owner = "directory-identity-owner";
    let initial = wait_for_complete(temp.path(), &directory, Some(&primary), owner);
    assert_eq!(initial.bytes, Some(10));

    let modified = fs::metadata(&directory).unwrap().modified().unwrap();
    let replacement = temp.path().join("replacement");
    fs::create_dir(&replacement).unwrap();
    fs::write(replacement.join("state"), b"new-directory-state").unwrap();
    fs::File::open(&replacement)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    fs::rename(&directory, temp.path().join("original-directory")).unwrap();
    fs::rename(&replacement, &directory).unwrap();

    let pending = directory_size(temp.path(), &directory, Some(&primary), owner);
    assert_eq!(pending.status, StorageStatus::Pending);
    assert_eq!(pending.bytes, None);
    let refreshed = wait_for_complete(temp.path(), &directory, Some(&primary), owner);
    assert_eq!(
        refreshed.bytes,
        Some(7 + b"new-directory-state".len() as u64)
    );
    assert_ne!(refreshed.bytes, initial.bytes);
}

#[test]
fn grok_summary_reports_session_directory_size_after_polling() {
    let temp = TempDir::new("storage-grok");
    let id = uuid::Uuid::new_v4().to_string();
    let directory = temp.path().join("sessions/%2Fwork").join(&id);
    fs::create_dir_all(directory.join("nested")).unwrap();
    fs::write(temp.path().join("active_sessions.json"), b"{}").unwrap();
    fs::write(
        directory.join("summary.json"),
        serde_json::to_vec(&json!({
            "info": {"id": id, "cwd": "/work"},
            "generated_title": "Fixture",
            "num_messages": 1
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(directory.join("updates.jsonl"), b"updates").unwrap();
    fs::write(directory.join("nested/state"), b"state").unwrap();
    let paths = GrokPaths {
        home: temp.path().to_path_buf(),
    };

    let first = list_grok_sessions(&paths, 10, None).unwrap();
    assert_eq!(first.len(), 1);
    let first_size = first[0].storage_size.as_ref().unwrap();
    assert_eq!(first_size.scope, StorageScope::Directory);
    assert_eq!(first_size.status, StorageStatus::Pending);

    let expected = fs::metadata(directory.join("summary.json")).unwrap().len()
        + fs::metadata(directory.join("updates.jsonl")).unwrap().len()
        + fs::metadata(directory.join("nested/state")).unwrap().len();
    let deadline = Instant::now() + Duration::from_secs(3);
    let ready = loop {
        let summary = &list_grok_sessions(&paths, 10, None).unwrap()[0];
        let size = summary.storage_size.as_ref().unwrap();
        if size.status == StorageStatus::Complete {
            break size.clone();
        }
        assert_eq!(size.status, StorageStatus::Pending);
        assert!(
            Instant::now() < deadline,
            "Grok storage remained pending: {size:?}"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(ready.bytes, Some(expected));
    assert_eq!(ready.scope, StorageScope::Directory);
    let wire = serde_json::to_value(&list_grok_sessions(&paths, 10, None).unwrap()[0]).unwrap();
    assert!(wire.get("storageSize").is_some());
}

fn claude_row(session_id: &str, uuid: &str, kind: &str, cwd: &Path, content: Value) -> Value {
    json!({
        "type": kind,
        "uuid": uuid,
        "parentUuid": null,
        "sessionId": session_id,
        "cwd": cwd,
        "version": "2.1.284",
        "timestamp": "2026-09-29T00:00:00Z",
        "message": {
            "role": kind,
            "content": content,
            "stop_reason": if kind == "assistant" { "end_turn" } else { "" }
        }
    })
}

fn write_claude_session(path: &Path, session_id: &str, cwd: &Path) {
    let rows = [
        claude_row(session_id, "user-1", "user", cwd, json!("Example request")),
        claude_row(
            session_id,
            "assistant-1",
            "assistant",
            cwd,
            json!([{"type": "text", "text": "Example answer"}]),
        ),
    ];
    fs::write(
        path,
        rows.iter()
            .map(|row| format!("{row}\n"))
            .collect::<Vec<_>>()
            .concat(),
    )
    .unwrap();
}

#[test]
fn claude_summary_owns_only_a_unique_native_id_directory() {
    let temp = TempDir::new("storage-claude");
    let projects = temp.path().join("config/projects");
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let project = projects.join("project");
    let session_id = "native-fixture";
    let file = project.join(format!("{session_id}.jsonl"));
    let directory = project.join(session_id);
    fs::create_dir_all(directory.join("nested")).unwrap();
    write_claude_session(&file, session_id, &cwd);
    fs::write(directory.join("events.bin"), b"directory event bytes").unwrap();
    fs::write(directory.join("nested/state"), b"state").unwrap();
    let paths = ClaudePaths {
        config: temp.path().join("config"),
        projects,
    };

    let first = list_claude_sessions(&paths, 10, None).unwrap();
    assert_eq!(first.len(), 1);
    let first_size = first[0].storage_size.as_ref().unwrap();
    assert_eq!(first_size.scope, StorageScope::Directory);
    assert_eq!(first_size.status, StorageStatus::Pending);

    let expected = fs::metadata(&file).unwrap().len()
        + fs::metadata(directory.join("events.bin")).unwrap().len()
        + fs::metadata(directory.join("nested/state")).unwrap().len();
    let deadline = Instant::now() + Duration::from_secs(3);
    let ready = loop {
        let summary = &list_claude_sessions(&paths, 10, None).unwrap()[0];
        let size = summary.storage_size.as_ref().unwrap();
        if size.status == StorageStatus::Complete {
            break size.clone();
        }
        assert_eq!(size.status, StorageStatus::Pending);
        assert!(
            Instant::now() < deadline,
            "Claude storage remained pending: {size:?}"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(ready.bytes, Some(expected));
    let wire = serde_json::to_value(&list_claude_sessions(&paths, 10, None).unwrap()[0]).unwrap();
    assert!(wire.get("storageSize").is_some());
}

#[test]
fn claude_duplicate_native_ids_do_not_inherit_sibling_directories() {
    let temp = TempDir::new("storage-claude-duplicate");
    let projects = temp.path().join("config/projects");
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let session_id = "duplicate-native-id";
    for project_name in ["first", "second"] {
        let project = projects.join(project_name);
        let file = project.join(format!("{session_id}.jsonl"));
        let directory = project.join(session_id);
        fs::create_dir_all(&directory).unwrap();
        write_claude_session(&file, session_id, &cwd);
        fs::write(
            directory.join("native-state"),
            format!("{project_name}-only-bytes").as_bytes(),
        )
        .unwrap();
    }
    let renamed = projects.join("first/renamed.jsonl");
    write_claude_session(&renamed, session_id, &cwd);
    let paths = ClaudePaths {
        config: temp.path().join("config"),
        projects,
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let summaries = loop {
        let summaries = list_claude_sessions(&paths, 10, None).unwrap();
        assert_eq!(summaries.len(), 3);
        let all_directories_ready = summaries
            .iter()
            .filter(|summary| {
                summary
                    .path
                    .file_name()
                    .is_some_and(|name| name != "renamed.jsonl")
            })
            .all(|summary| {
                summary
                    .storage_size
                    .as_ref()
                    .is_some_and(|size| size.status == StorageStatus::Complete)
            });
        if all_directories_ready {
            break summaries;
        }
        assert!(
            Instant::now() < deadline,
            "Claude duplicate storage remained pending"
        );
        thread::sleep(Duration::from_millis(10));
    };
    for summary in summaries {
        let size = summary.storage_size.expect("Claude storage size");
        let filename = summary.path.file_name().unwrap().to_string_lossy();
        if filename == "renamed.jsonl" {
            assert_eq!(size.scope, StorageScope::File);
            assert_eq!(size.status, StorageStatus::Complete);
            assert_eq!(size.bytes, Some(fs::metadata(summary.path).unwrap().len()));
            continue;
        }
        assert_eq!(size.scope, StorageScope::Directory);
        assert_eq!(size.status, StorageStatus::Complete);
        let directory = summary.path.with_extension("");
        let expected = fs::metadata(&summary.path).unwrap().len()
            + fs::metadata(directory.join("native-state")).unwrap().len();
        assert_eq!(size.bytes, Some(expected));
    }
}

fn write_codex_thread(home: &Path, id: &str, metadata_id: &str) -> PathBuf {
    let rollout = home.join(format!("{id}.jsonl"));
    fs::write(
        &rollout,
        [
            json!({"session_meta": {"payload": {"id": metadata_id}}}).to_string(),
            json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"text": "example"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    rollout
}

fn codex_fixture(home: &Path, rows: &[(&str, &str, i64)]) {
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::create_dir_all(home.join("app-server-control")).unwrap();
    fs::write(home.join("session_index.jsonl"), b"").unwrap();
    let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            archived INTEGER NOT NULL DEFAULT 0,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    for (id, metadata_id, updated_at) in rows {
        let rollout = write_codex_thread(home, id, metadata_id);
        conn.execute(
            "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, preview)
             VALUES(?1, ?2, 1, ?3, 'codex', '', '/tmp', ?4, '', '', 0, '')",
            (id, rollout.display().to_string(), updated_at, format!("Fixture {id}")),
        )
        .unwrap();
    }
}

#[test]
fn codex_summary_requires_a_validated_rollout_identity() {
    let temp = TempDir::new("storage-codex");
    let valid_id = "codex-valid";
    let invalid_id = "codex-invalid";
    codex_fixture(
        temp.path(),
        &[(valid_id, valid_id, 1), (invalid_id, "other-id", 2)],
    );
    let paths = CodexPaths::new(temp.path());
    let summaries = list_threads(&paths, false, None, 10).unwrap();
    assert_eq!(summaries.len(), 2);
    let valid = summaries
        .iter()
        .find(|summary| summary.id == valid_id)
        .unwrap();
    let valid_size = valid.storage_size.as_ref().unwrap();
    assert_eq!(valid_size.scope, StorageScope::File);
    assert_eq!(valid_size.status, StorageStatus::Complete);
    assert_eq!(
        valid_size.bytes,
        Some(
            fs::metadata(valid.rollout_path.as_ref().unwrap())
                .unwrap()
                .len()
        )
    );

    let invalid = summaries
        .iter()
        .find(|summary| summary.id == invalid_id)
        .unwrap();
    assert_eq!(
        invalid.storage_size,
        Some(SessionStorageSize::unavailable(StorageScope::File))
    );
    let wire = serde_json::to_value(valid).unwrap();
    assert!(wire.get("storageSize").is_some());
}

#[test]
fn archived_codex_summary_keeps_validated_file_size() {
    let temp = TempDir::new("storage-codex-archived");
    let id = "codex-archived";
    codex_fixture(temp.path(), &[(id, id, 1)]);
    let conn = Connection::open(temp.path().join("state_5.sqlite")).unwrap();
    conn.execute("UPDATE threads SET archived = 1 WHERE id = ?1", [id])
        .unwrap();
    drop(conn);

    let rows = list_threads(&CodexPaths::new(temp.path()), true, None, 10).unwrap();
    assert_eq!(rows.len(), 1);
    let size = rows[0].storage_size.as_ref().unwrap();
    assert_eq!(size.scope, StorageScope::File);
    assert_eq!(size.status, StorageStatus::Complete);
    assert!(size.bytes.is_some());
}
