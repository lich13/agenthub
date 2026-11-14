//! File operations for explicitly selected native sessions. No transcript parsing or activity gates.
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Seek, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct Outcome {
    pub bytes: u64,
    pub files: u64,
    pub residuals: Vec<String>,
}

pub(crate) fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && Path::new(id).components().count() == 1
            && matches!(
                Path::new(id).components().next(),
                Some(Component::Normal(_))
            )
            && !id.chars().any(|c| c.is_control() || c == '/' || c == '\\'),
        "无效的线程定位键"
    );
    Ok(())
}

/// A database path is a location hint, never permission to traverse another storage root.
pub(crate) fn within(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return false;
    }
    let mut parent = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for part in components.iter().take(components.len().saturating_sub(1)) {
        parent.push(part);
        if fs::symlink_metadata(&parent).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return false;
        }
    }
    true
}

pub(crate) fn size(paths: &[PathBuf]) -> (usize, u64) {
    let mut count = 0;
    let mut bytes = 0u64;
    for path in paths {
        for entry in walkdir::WalkDir::new(path)
            .follow_links(false)
            .follow_root_links(false)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() || entry.file_type().is_symlink() {
                count += 1;
                bytes =
                    bytes.saturating_add(fs::symlink_metadata(entry.path()).map_or(0, |m| m.len()));
            }
        }
    }
    (count, bytes)
}

impl Outcome {
    pub fn attempt(&mut self, path: &Path, action: impl FnOnce() -> Result<()>) {
        if let Err(error) = action() {
            self.residuals.push(format!("{}: {error}", path.display()));
        }
    }

    pub fn remove_strict(&mut self, path: &Path) -> bool {
        if fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            return true;
        }
        for entry in walkdir::WalkDir::new(path)
            .follow_links(false)
            .follow_root_links(false)
            .contents_first(true)
        {
            match entry {
                Ok(entry) => {
                    let bytes = fs::symlink_metadata(entry.path()).map_or(0, |m| m.len());
                    let directory = entry.file_type().is_dir();
                    let result = if directory {
                        fs::remove_dir(entry.path())
                    } else {
                        fs::remove_file(entry.path())
                    };
                    match result {
                        Ok(()) if !directory => {
                            self.files += 1;
                            self.bytes = self.bytes.saturating_add(bytes);
                        }
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            self.residuals
                                .push(format!("{}: {e}", entry.path().display()));
                            return false;
                        }
                    }
                }
                Err(e)
                    if e.io_error()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
                Err(e) => {
                    self.residuals.push(e.to_string());
                    return false;
                }
            }
        }
        true
    }

    pub fn finish(&mut self) {
        self.residuals.sort();
        self.residuals.dedup();
    }
}

fn same_source(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if !after.is_file()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || before.permissions() != after.permissions()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (
            before.dev(),
            before.ino(),
            before.ctime(),
            before.ctime_nsec(),
            before.uid(),
            before.gid(),
        ) == (
            after.dev(),
            after.ino(),
            after.ctime(),
            after.ctime_nsec(),
            after.uid(),
            after.gid(),
        )
    }
    #[cfg(not(unix))]
    {
        before.created().ok() == after.created().ok()
    }
}

fn replace(
    path: &Path,
    source: &fs::Metadata,
    write: impl FnOnce(&mut fs::File) -> Result<()>,
) -> Result<()> {
    let temporary = path.with_file_name(format!(".agenthub-index-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        write(&mut file)?;
        #[cfg(unix)]
        {
            use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
            let current = file.metadata()?;
            if (current.uid(), current.gid()) != (source.uid(), source.gid()) {
                let result = unsafe { libc::fchown(file.as_raw_fd(), source.uid(), source.gid()) };
                if result != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
        }
        // chown may clear mode bits, so restore permissions afterwards.
        file.set_permissions(source.permissions())?;
        file.sync_all()?;
        ensure!(
            same_source(source, &fs::symlink_metadata(path)?),
            "共享索引已变化，请重试"
        );
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn open_shared(path: &Path) -> Result<Option<fs::File>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    ensure!(file.metadata()?.is_file(), "共享索引不是普通文件");
    Ok(Some(file))
}

pub(crate) fn filter_jsonl(path: &Path, remove: impl Fn(&Value) -> bool) -> Result<()> {
    let Some(source) = open_shared(path)? else {
        return Ok(());
    };
    let metadata = source.metadata()?;
    let mut reader = BufReader::new(source);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        if serde_json::from_slice::<Value>(&line).is_ok_and(|value| remove(&value)) {
            break;
        }
    }
    reader.rewind()?;
    replace(path, &metadata, |output| {
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if serde_json::from_slice::<Value>(&line).is_ok_and(|value| remove(&value)) {
                continue;
            }
            output.write_all(&line)?;
        }
        Ok(())
    })
}

pub(crate) fn edit_json(path: &Path, edit: impl FnOnce(&mut Value)) -> Result<()> {
    let Some(source) = open_shared(path)? else {
        return Ok(());
    };
    let metadata = source.metadata()?;
    let mut value: Value =
        serde_json::from_reader(BufReader::new(source)).context("无法读取共享索引")?;
    let original = value.clone();
    edit(&mut value);
    if value == original {
        return Ok(());
    }
    replace(path, &metadata, |output| {
        serde_json::to_writer(&mut *output, &value)?;
        output.write_all(b"\n")?;
        Ok(())
    })
}

pub(crate) fn has_id(value: &Value, ids: &std::collections::BTreeSet<String>) -> bool {
    ["id", "sessionId", "session_id", "thread_id"]
        .iter()
        .any(|key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|id| ids.contains(id))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_index_concurrent_change_is_retained_and_temporary_is_removed() {
        let root =
            std::env::temp_dir().join(format!("agenthub-index-race-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("index.json");
        fs::write(&path, b"[1,2]").unwrap();
        let error = edit_json(&path, |value| {
            *value = serde_json::json!([2]);
            fs::write(&path, b"[1,2,3]").unwrap();
        })
        .unwrap_err();
        assert!(error.to_string().contains("已变化"));
        assert_eq!(fs::read(&path).unwrap(), b"[1,2,3]");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn shared_index_replacement_preserves_owner_group_and_mode() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root =
            std::env::temp_dir().join(format!("agenthub-index-owner-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("index.json");
        fs::write(&path, b"[1,2]").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        if unsafe { libc::geteuid() } == 0 {
            use std::os::fd::AsRawFd;
            let file = fs::File::open(&path).unwrap();
            assert_eq!(unsafe { libc::fchown(file.as_raw_fd(), 65534, 65534) }, 0);
        }
        let before = fs::metadata(&path).unwrap();
        edit_json(&path, |value| *value = serde_json::json!([2])).unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_eq!(
            (after.uid(), after.gid(), after.mode()),
            (before.uid(), before.gid(), before.mode())
        );
        assert_ne!(after.ino(), before.ino());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_indexes_only_replace_when_selected_records_exist() {
        let root = std::env::temp_dir().join(format!("agenthub-index-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let json = root.join("active_sessions.json");
        let jsonl = root.join("history.jsonl");
        let json_bytes = b"[ {\"id\": \"other\"}, {\"id\": \"selected\"} ]\n";
        let jsonl_bytes = b"{\"id\":\"other\"}\r\n{\"id\":\"selected\"}\npartial fixture";
        fs::write(&json, json_bytes).unwrap();
        fs::write(&jsonl, jsonl_bytes).unwrap();
        let before = [fs::metadata(&json).unwrap(), fs::metadata(&jsonl).unwrap()];
        edit_json(&json, |_| {}).unwrap();
        filter_jsonl(&jsonl, |row| row["id"] == "absent").unwrap();
        assert_eq!(fs::read(&json).unwrap(), json_bytes);
        assert_eq!(fs::read(&jsonl).unwrap(), jsonl_bytes);
        for (path, metadata) in [&json, &jsonl].into_iter().zip(before) {
            let after = fs::metadata(path).unwrap();
            assert_eq!(metadata.modified().unwrap(), after.modified().unwrap());
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(metadata.ino(), after.ino());
            }
        }
        edit_json(&json, |value| {
            value
                .as_array_mut()
                .unwrap()
                .retain(|row| row["id"] != "selected")
        })
        .unwrap();
        filter_jsonl(&jsonl, |row| row["id"] == "selected").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&json).unwrap()).unwrap(),
            serde_json::json!([{ "id": "other" }])
        );
        assert_eq!(
            fs::read(&jsonl).unwrap(),
            b"{\"id\":\"other\"}\r\npartial fixture"
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deletion_has_no_transcript_size_depth_or_file_count_limit() {
        let root =
            std::env::temp_dir().join(format!("agenthub-delete-size-{}", uuid::Uuid::new_v4()));
        let mut deep = root.clone();
        for _ in 0..40 {
            deep.push("nested");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::File::create(deep.join("sparse.jsonl"))
            .unwrap()
            .set_len(513 * 1024 * 1024)
            .unwrap();
        for index in 0..10_001 {
            fs::File::create(root.join(format!("{index}.fixture"))).unwrap();
        }
        let mut outcome = Outcome::default();
        assert!(outcome.remove_strict(&root));
        assert!(outcome.residuals.is_empty(), "{:?}", outcome.residuals);
        assert_eq!(outcome.files, 10_002);
        assert_eq!(outcome.bytes, 513 * 1024 * 1024);
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn root_directory_link_is_unlinked_without_entering_target() {
        let root =
            std::env::temp_dir().join(format!("agenthub-delete-link-{}", uuid::Uuid::new_v4()));
        let target = root.join("retained");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("fixture"), "retained").unwrap();
        let link = root.join("selected");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut outcome = Outcome::default();
        assert!(outcome.remove_strict(&link));
        assert!(outcome.residuals.is_empty());
        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(
            fs::read_to_string(target.join("fixture")).unwrap(),
            "retained"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_is_reported_and_other_paths_are_removed() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "agenthub-delete-permission-{}",
            uuid::Uuid::new_v4()
        ));
        let locked = root.join("locked");
        fs::create_dir_all(&locked).unwrap();
        let retained = locked.join("fixture");
        let removable = root.join("removable");
        fs::write(&retained, "fixture").unwrap();
        fs::write(&removable, "fixture").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        let mut outcome = Outcome::default();
        assert!(!outcome.remove_strict(&retained));
        assert!(outcome.remove_strict(&removable));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(outcome.residuals.len(), 1);
            assert!(retained.exists());
        }
        assert!(!removable.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
