use super::*;
use serde_json::json;

struct Fixture {
    paths: DshPaths,
}
impl Fixture {
    fn new(ids: &[&str]) -> Self {
        let home = std::env::temp_dir().join(format!("agenthub-dsh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(home.join("storages/session_projcache/sessions")).unwrap();
        fs::create_dir_all(home.join("sessions/project")).unwrap();
        let f = Self {
            paths: DshPaths { home },
        };
        f.index(json!({"unit":{"name":"workspace","version":2},"global":{"archivedSessionIds":ids,"pinnedSessionIds":["keep"],"unknown":"preserved"},"tables":{"workspaces":{"workspace":{"sessionIds":["root","child","keep","fork"],"title":"Fixture","path":"/fixture"}}}}));
        f
    }
    fn index_path(&self) -> PathBuf {
        self.paths.home.join("storages/workspace.json")
    }
    fn index(&self, value: Value) {
        fs::write(self.index_path(), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn read_index(&self) -> Value {
        serde_json::from_slice(&fs::read(self.index_path()).unwrap()).unwrap()
    }
    fn session(&self, id: &str, parent: Option<&str>, subagent: bool, compressed: bool) -> PathBuf {
        let dir = self.paths.home.join("sessions/project").join(id);
        fs::create_dir_all(&dir).unwrap();
        let mut header =
            json!({"type":"session","version":4,"id":id,"createdAt":1,"isSeeded":false});
        if let Some(parent) = parent {
            header["parentSession"] = json!(parent);
        }
        if subagent {
            header["origin"] = json!("subagent");
        }
        let data = format!("{header}\n{{\"type\":\"fixture-event\"}}\n");
        if compressed {
            fs::write(
                dir.join("session.v4.jsonl.zstd"),
                zstd::stream::encode_all(data.as_bytes(), 1).unwrap(),
            )
            .unwrap();
        } else {
            fs::write(dir.join("session.v4.jsonl"), data).unwrap();
        }
        fs::write(dir.join("session.lock"), "fixture lock").unwrap();
        fs::write(
            self.paths
                .home
                .join("storages/session_projcache/sessions")
                .join(format!("{id}.json")),
            "{}",
        )
        .unwrap();
        dir
    }
    fn request(&self, ids: &[&str]) -> DshArchiveDeleteRequest {
        let plan = preview(&self.paths).unwrap();
        DshArchiveDeleteRequest {
            confirmed: true,
            targets: ids
                .iter()
                .map(|id| DshArchiveSelection {
                    id: (*id).into(),
                    archive_id: plan
                        .candidates
                        .iter()
                        .find(|c| c.id == *id)
                        .map(|c| c.archive_id.clone())
                        .unwrap_or_else(|| (*id).into()),
                })
                .collect(),
        }
    }
    fn execute(&self, ids: &[&str]) -> DshArchiveDeleteResult {
        execute_checked(&self.paths, self.request(ids), || Ok(())).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.paths.home);
    }
}

#[test]
fn removes_archived_tree_and_exact_references_but_preserves_forks_and_shared_data() {
    let f = Fixture::new(&["root"]);
    let root = f.session("root", None, false, false);
    let child = f.session("child", Some("root"), true, true);
    let keep = f.session("keep", None, false, false);
    let fork = f.session("fork", Some("root"), false, true);
    fs::create_dir_all(f.paths.home.join("attachments")).unwrap();
    fs::write(f.paths.home.join("attachments/shared"), "keep").unwrap();
    let plan = preview(&f.paths).unwrap();
    assert_eq!(plan.archived_threads, 1);
    assert_eq!(
        plan.candidates
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["child", "root"]
    );
    let result = f.execute(&["root", "child", "root"]);
    assert_eq!(result.deleted_threads, 2);
    assert!(result.deleted_bytes > 0);
    assert!(!root.exists() && !child.exists());
    assert!(keep.exists() && fork.exists() && f.paths.home.join("attachments/shared").exists());
    let index = f.read_index();
    assert_eq!(index["global"]["archivedSessionIds"], json!([]));
    assert_eq!(index["global"]["unknown"], "preserved");
    assert_eq!(
        index["tables"]["workspaces"]["workspace"]["sessionIds"],
        json!(["keep", "fork"])
    );
    assert!(!f
        .paths
        .home
        .join("storages/session_projcache/sessions/root.json")
        .exists());
}

#[test]
fn restored_targets_are_skipped_and_new_archives_are_not_added_to_execution() {
    let f = Fixture::new(&["root"]);
    let root = f.session("root", None, false, false);
    let new = f.session("new", None, false, false);
    preview(&f.paths).unwrap();
    let mut index = f.read_index();
    index["global"]["archivedSessionIds"] = json!(["new"]);
    f.index(index.clone());
    let result = f.execute(&["root"]);
    assert_eq!(result.items[0].status, "skipped");
    assert!(root.exists() && new.exists());
    assert_eq!(f.read_index(), index);
}

#[test]
fn later_descendants_and_partial_failures_keep_the_archived_root_retryable() {
    let f = Fixture::new(&["root", "other"]);
    f.session("root", None, false, false);
    f.session("other", None, false, false);
    preview(&f.paths).unwrap();
    let child = f.session("child", Some("root"), true, false);
    let result = f.execute(&["root", "other"]);
    assert_eq!(result.deleted_threads, 1);
    assert!(child.exists());
    assert_eq!(
        f.read_index()["global"]["archivedSessionIds"],
        json!(["root"])
    );
    assert_eq!(f.execute(&["root", "child"]).deleted_threads, 2);
}

#[test]
fn corrupt_session_fails_individually_and_missing_files_can_be_cleaned_again() {
    let f = Fixture::new(&["root", "broken", "missing"]);
    f.session("root", None, false, true);
    let broken = f.session("broken", None, false, false);
    fs::write(broken.join("session.v4.jsonl"), "broken\n").unwrap();
    let result = f.execute(&["root", "broken", "missing"]);
    assert_eq!(result.deleted_threads, 2);
    assert_eq!(
        result
            .items
            .iter()
            .find(|i| i.id == "broken")
            .unwrap()
            .status,
        "failed"
    );
    assert_eq!(
        f.read_index()["global"]["archivedSessionIds"],
        json!(["broken"])
    );
    let before = fs::read(f.index_path()).unwrap();
    let meta = fs::metadata(f.index_path()).unwrap();
    f.execute(&["root", "missing"]);
    assert_eq!(before, fs::read(f.index_path()).unwrap());
    assert_eq!(
        meta.modified().unwrap(),
        fs::metadata(f.index_path()).unwrap().modified().unwrap()
    );
}

#[test]
fn running_dsh_or_unconfirmed_or_path_input_never_mutates_storage() {
    let f = Fixture::new(&["root"]);
    let root = f.session("root", None, false, false);
    for (confirmed, id) in [(false, "root"), (true, "../root"), (true, "root")] {
        assert!(execute_checked(
            &f.paths,
            DshArchiveDeleteRequest {
                confirmed,
                targets: vec![DshArchiveSelection {
                    id: id.into(),
                    archive_id: "root".into()
                }]
            },
            || anyhow::bail!("DSH running")
        )
        .is_err());
        assert!(root.exists());
    }
    assert!(serde_json::from_value::<DshArchiveDeleteRequest>(
        json!({"confirmed":true,"targets":[{"id":"root","archiveId":"root"}],"path":"/fixture"})
    )
    .is_err());
}

#[test]
fn concurrent_index_change_is_preserved_and_reported_for_retry() {
    use std::cell::Cell;
    let f = Fixture::new(&["root"]);
    f.session("root", None, false, false);
    let checks = Cell::new(0);
    let result = execute_checked(&f.paths, f.request(&["root"]), || {
        checks.set(checks.get() + 1);
        if checks.get() == 3 {
            let mut value = f.read_index();
            value["global"]["unknown"] = json!("external edit");
            f.index(value);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(result.deleted_threads, 0);
    assert_eq!(result.items[0].status, "failed");
    assert_eq!(f.read_index()["global"]["unknown"], "external edit");
    assert_eq!(
        f.read_index()["global"]["archivedSessionIds"],
        json!(["root"])
    );
    assert_eq!(f.execute(&["root"]).deleted_threads, 1);
    assert!(!fs::read_dir(f.paths.home.join("storages"))
        .unwrap()
        .any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".agenthub-index-")));
}

#[test]
fn absent_data_is_empty_and_unknown_or_pending_storage_is_rejected() {
    let f = Fixture::new(&[]);
    assert!(
        !preview(&DshPaths {
            home: f.paths.home.join("absent")
        })
        .unwrap()
        .available
    );
    let mut index = f.read_index();
    index["global"]["pendingMutation"] = json!({"fixture":true});
    f.index(index);
    assert!(preview(&f.paths)
        .unwrap_err()
        .to_string()
        .contains("变更未完成"));
    fs::write(f.index_path(), "invalid").unwrap();
    assert!(preview(&f.paths).is_err());
    fs::write(
        f.paths.home.join("cordis.patch.yml"),
        "- id: storage-json\n  config:\n    root: /fixture/custom\n",
    )
    .unwrap();
    assert!(preview(&f.paths)
        .unwrap_err()
        .to_string()
        .contains("自定义持久化"));
}

#[cfg(unix)]
#[test]
fn directory_links_are_never_followed_and_shared_cache_links_remain_protected() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new(&["root"]);
    let outside = f.paths.home.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "safe").unwrap();
    symlink(&outside, f.paths.home.join("sessions/project/root")).unwrap();
    assert_eq!(f.execute(&["root"]).deleted_threads, 1);
    assert!(outside.join("keep").exists());
}

#[test]
fn dsh_process_recognition_distinguishes_native_runtimes_from_shell_text() {
    for command in [
        "/Applications/DeepSeek Harness.app/Contents/MacOS/DeepSeek Harness",
        "dsh --profile headless",
        "node /fixture/node_modules/@deepseek-ai/cli/lib/index.js",
    ] {
        assert!(dsh_command(command));
    }
    for command in [
        "/bin/bash -lc inspect /Applications/DeepSeek Harness.app/Contents/Resources",
        "node /fixture/another-app.js",
        "cargo test dsh",
    ] {
        assert!(!dsh_command(command));
    }
}

#[test]
fn restored_parent_and_newly_archived_child_do_not_reuse_old_confirmation() {
    let f = Fixture::new(&["root"]);
    f.session("root", None, false, false);
    let child = f.session("child", Some("root"), true, false);
    let request = f.request(&["root", "child"]);
    let mut index = f.read_index();
    index["global"]["archivedSessionIds"] = json!(["child"]);
    f.index(index.clone());
    let result = execute_checked(&f.paths, request, || Ok(())).unwrap();
    assert_eq!(result.deleted_threads, 0);
    assert!(child.exists());
    assert_eq!(f.read_index(), index);
}

#[test]
fn concurrent_session_activity_skips_only_the_changed_target() {
    use std::cell::Cell;
    let f = Fixture::new(&["root", "z-last"]);
    let root = f.session("root", None, false, false);
    let later = f.session("z-last", None, false, false);
    let request = f.request(&["root", "z-last"]);
    let checks = Cell::new(0);
    let result = execute_checked(&f.paths, request, || {
        checks.set(checks.get() + 1);
        if checks.get() == 2 {
            use std::io::Write;
            writeln!(
                OpenOptions::new()
                    .append(true)
                    .open(root.join("session.v4.jsonl"))
                    .unwrap(),
                "{{\"type\":\"new-event\"}}"
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(result.deleted_threads, 1);
    assert_eq!(result.items[0].status, "skipped");
    assert_eq!(result.items[1].status, "deleted");
    assert!(root.join("session.lock").exists());
    assert!(!later.exists());
    assert_eq!(
        f.read_index()["global"]["archivedSessionIds"],
        json!(["root"])
    );
}

#[cfg(unix)]
#[test]
fn projection_permission_boundary_preserves_child_identity_for_retry() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new(&["root"]);
    f.session("root", None, false, false);
    let child = f.session("child", Some("root"), true, true);
    let cache = f.paths.home.join("storages/session_projcache/sessions");
    fs::rename(&cache, cache.with_file_name("retained")).unwrap();
    symlink(cache.with_file_name("retained"), &cache).unwrap();
    let result = f.execute(&["root", "child"]);
    assert_eq!(result.deleted_threads, 0);
    assert!(child.join("session.v4.jsonl.zstd").exists());
    fs::remove_file(&cache).unwrap();
    fs::rename(cache.with_file_name("retained"), &cache).unwrap();
    assert_eq!(f.execute(&["root", "child"]).deleted_threads, 2);
}
