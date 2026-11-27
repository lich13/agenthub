//! Read-only, metadata-only session sizes. Callers establish native ownership first.
use crate::file_stamp::FileStamp;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, Metadata},
    path::{Component, Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::Instant,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageScope {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageStatus {
    Complete,
    Partial,
    Pending,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionStorageSize {
    pub bytes: Option<u64>,
    pub scope: StorageScope,
    pub status: StorageStatus,
}

impl SessionStorageSize {
    pub fn unavailable(scope: StorageScope) -> Self {
        Self {
            bytes: None,
            scope,
            status: StorageStatus::Unavailable,
        }
    }
}

// Reject links in every component below the configured root, including the leaf.
// Canonicalizing a path alone would silently count a link's external target.
fn metadata_with_canonical_root(
    root: &Path,
    canonical_root: Option<&Path>,
    path: &Path,
) -> Option<(PathBuf, Metadata)> {
    let owned_canonical: PathBuf;
    let canonical = match canonical_root {
        Some(prevalidated) => prevalidated,
        None => {
            if fs::symlink_metadata(root).ok()?.file_type().is_symlink() {
                return None;
            }
            owned_canonical = root.canonicalize().ok()?;
            &owned_canonical
        }
    };
    let relative = path
        .strip_prefix(root)
        .or_else(|_| path.strip_prefix(canonical))
        .ok()?;
    let mut resolved = canonical.to_path_buf();
    let mut last_meta: Option<Metadata> = None;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        resolved.push(name);
        let meta = fs::symlink_metadata(&resolved).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        last_meta = Some(meta);
    }
    let meta = match last_meta {
        Some(meta) => meta,
        None => fs::symlink_metadata(&resolved).ok()?,
    };
    Some((resolved, meta))
}

#[cfg(test)]
fn metadata(root: &Path, path: &Path) -> Option<(PathBuf, Metadata)> {
    metadata_with_canonical_root(root, None, path)
}

pub fn file_size(root: &Path, file: &Path) -> SessionStorageSize {
    file_size_with_canonical_root(root, None, file)
}

pub(crate) fn file_size_with_canonical_root(
    root: &Path,
    canonical_root: Option<&Path>,
    file: &Path,
) -> SessionStorageSize {
    match metadata_with_canonical_root(root, canonical_root, file)
        .filter(|(_, meta)| meta.is_file())
    {
        Some((_, meta)) => SessionStorageSize {
            bytes: Some(meta.len()),
            scope: StorageScope::File,
            status: StorageStatus::Complete,
        },
        None => SessionStorageSize::unavailable(StorageScope::File),
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    root: PathBuf,
    directory: PathBuf,
    directory_stamp: FileStamp,
    primary: Option<(PathBuf, FileStamp)>,
    owner: String,
}
impl Source {
    /// Re-reads both stamps under the reuse rule of `can_reuse_size`: the same
    /// directory and primary file, where the primary may only have grown.
    fn current(&self) -> bool {
        metadata_with_canonical_root(&self.root, Some(&self.root), &self.directory).is_some_and(
            |(_, meta)| {
                self.directory_stamp
                    .same_inode(&FileStamp::from_metadata(&meta))
            },
        ) && self.primary.as_ref().is_none_or(|(path, stamp)| {
            metadata_with_canonical_root(&self.root, Some(&self.root), path)
                .is_some_and(|(_, meta)| stamp.grew_into(&FileStamp::from_metadata(&meta)))
        })
    }
    fn same_slot(&self, other: &Self) -> bool {
        self.root == other.root && self.directory == other.directory && self.owner == other.owner
    }
    fn can_reuse_size(&self, next: &Self) -> bool {
        self.same_slot(next)
            && self.directory_stamp.same_inode(&next.directory_stamp)
            && match (&self.primary, &next.primary) {
                (None, None) => true,
                (Some((path, stamp)), Some((next_path, next_stamp))) => {
                    path == next_path && stamp.grew_into(next_stamp)
                }
                _ => false,
            }
    }
}
struct Entry {
    source: Source,
    value: SessionStorageSize,
    checked: Instant,
    last_accessed: Instant,
    queued: bool,
}
struct Cache {
    entries: Arc<Mutex<VecDeque<Entry>>>,
    queue: mpsc::SyncSender<Source>,
}
static CACHE: OnceLock<Cache> = OnceLock::new();
use crate::resource_limits::{
    STORAGE_CACHE_ENTRIES as CACHE_ENTRIES, STORAGE_CACHE_TTL as TTL,
    STORAGE_SCAN_DEPTH as SCAN_DEPTH, STORAGE_SCAN_ENTRIES as SCAN_ENTRIES,
};

impl Cache {
    fn new() -> Self {
        let entries = Arc::new(Mutex::new(VecDeque::<Entry>::new()));
        let (queue, receiver) =
            mpsc::sync_channel::<Source>(crate::resource_limits::STORAGE_QUEUE_ENTRIES);
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..crate::resource_limits::STORAGE_WORKERS {
            let receiver = receiver.clone();
            let entries = entries.clone();
            let pending = queue.clone();
            let _ = std::thread::Builder::new()
                .name("session-size".into())
                .spawn(move || loop {
                    let Ok(job) = receiver.lock().unwrap_or_else(|e| e.into_inner()).recv() else {
                        break;
                    };
                    // A queued directory may have changed while waiting. Scan its latest
                    // verified source, with only one queued/running job per directory.
                    let source = {
                        let entries = entries.lock().unwrap_or_else(|e| e.into_inner());
                        entries
                            .iter()
                            .find(|entry| entry.source.same_slot(&job))
                            .map(|entry| entry.source.clone())
                    };
                    let Some(source) = source else {
                        continue;
                    };
                    let value = scan(&source);
                    let mut entries = entries.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(entry) = entries
                        .iter_mut()
                        .find(|entry| entry.source.same_slot(&source))
                    {
                        if entry.source == source {
                            entry.value = value;
                            entry.checked = Instant::now();
                            entry.queued = false;
                        } else {
                            // Changes during the scan invalidate that result. Coalesce
                            // them into one follow-up instead of publishing stale bytes.
                            entry.queued = pending.try_send(entry.source.clone()).is_ok();
                        }
                    }
                });
        }
        Self { entries, queue }
    }
    fn get(&self, source: Source) -> SessionStorageSize {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let mut entry =
            crate::lru::take_first(&mut entries, |entry| entry.source.same_slot(&source))
                .unwrap_or_else(|| Entry {
                    source: source.clone(),
                    value: pending_size(),
                    checked: now,
                    last_accessed: now,
                    queued: false,
                });
        entry.last_accessed = now;
        if entry.source != source {
            // Ordinary appends use the bounded TTL. Resetting on every mtime
            // change would leave an actively written session permanently pending.
            if !entry.source.can_reuse_size(&source) {
                entry.value = pending_size();
            }
            entry.source = source;
        }
        if !entry.queued
            && (entry.value.status == StorageStatus::Pending || entry.checked.elapsed() >= TTL)
        {
            // Hold the entry lock until enqueued so a fast worker cannot lose its result.
            match self.queue.try_send(entry.source.clone()) {
                Ok(()) => entry.queued = true,
                Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    entry.value = SessionStorageSize::unavailable(StorageScope::Directory);
                    entry.checked = Instant::now();
                }
            }
        }
        // Keep the last verified value while refreshing an unchanged source.
        let value = entry.value.clone();
        if entries.len() >= CACHE_ENTRIES {
            let has_idle = entries.iter().any(|entry| !entry.queued);
            crate::lru::trim(
                &mut entries,
                CACHE_ENTRIES - 1,
                0,
                0,
                |entry| !has_idle || !entry.queued,
                |_| 0,
            );
        }
        entries.push_back(entry);
        value
    }

    fn evict_idle(&self, max_idle: std::time::Duration) -> usize {
        let Ok(mut entries) = self.entries.lock() else {
            return 0;
        };
        let before = entries.len();
        let now = Instant::now();
        entries.retain(|entry| {
            entry.queued || now.saturating_duration_since(entry.last_accessed) < max_idle
        });
        let evicted = before - entries.len();
        if evicted > 0 {
            if entries.capacity() > entries.len().saturating_mul(4).max(16) {
                entries.shrink_to_fit();
            }
            crate::note_cache_eviction();
        }
        evicted
    }
}

pub fn evict_idle_cache(max_idle: std::time::Duration) -> usize {
    CACHE.get().map_or(0, |cache| cache.evict_idle(max_idle))
}

fn pending_size() -> SessionStorageSize {
    SessionStorageSize {
        bytes: None,
        scope: StorageScope::Directory,
        status: StorageStatus::Pending,
    }
}

/// Returns immediately; the next list poll collects a completed directory scan.
pub fn directory_size(
    root: &Path,
    directory: &Path,
    primary: Option<&Path>,
    owner: &str,
) -> SessionStorageSize {
    let unavailable = || SessionStorageSize::unavailable(StorageScope::Directory);
    if fs::symlink_metadata(root)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(true)
    {
        return unavailable();
    }
    let Some(canonical_root) = root.canonicalize().ok() else {
        return unavailable();
    };
    let Some((directory, meta)) =
        metadata_with_canonical_root(root, Some(&canonical_root), directory)
            .filter(|(_, meta)| meta.is_dir())
    else {
        return unavailable();
    };
    let primary = match primary {
        Some(file) => match metadata_with_canonical_root(root, Some(&canonical_root), file)
            .filter(|(_, meta)| meta.is_file())
        {
            Some((path, meta)) => Some((path, FileStamp::from_metadata(&meta))),
            None => return unavailable(),
        },
        None => None,
    };
    let source = Source {
        root: canonical_root,
        directory,
        directory_stamp: FileStamp::from_metadata(&meta),
        primary,
        owner: owner.to_owned(),
    };
    CACHE.get_or_init(Cache::new).get(source)
}

fn scan(source: &Source) -> SessionStorageSize {
    let mut bytes = 0u64;
    let mut partial = false;
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let started = Instant::now();
    let mut add = |meta: &Metadata| {
        #[cfg(unix)]
        let key = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino())
        };
        #[cfg(not(unix))]
        let key = (0u64, bytes);
        if seen.insert(key) {
            bytes = bytes.saturating_add(meta.len());
        }
    };
    if let Some((path, _)) = &source.primary {
        if let Some((_, meta)) =
            metadata_with_canonical_root(&source.root, Some(&source.root), path)
        {
            add(&meta);
        } else {
            return SessionStorageSize::unavailable(StorageScope::Directory);
        }
    }
    for (count, entry) in walkdir::WalkDir::new(&source.directory)
        .follow_links(false)
        .max_depth(SCAN_DEPTH)
        .into_iter()
        .enumerate()
    {
        if count >= SCAN_ENTRIES || started.elapsed() > crate::resource_limits::STORAGE_SCAN_TIME {
            partial = true;
            break;
        }
        let Ok(entry) = entry else {
            partial = true;
            continue;
        };
        if entry.file_type().is_symlink() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            partial = true;
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            add(&meta);
        } else if !meta.is_dir() || entry.depth() == SCAN_DEPTH {
            partial = true;
        }
    }
    // A replaced or truncated source cannot publish a result under an old identity.
    // An append during the scan keeps it, as the request side does until the TTL.
    if !source.current() {
        return SessionStorageSize::unavailable(StorageScope::Directory);
    }
    SessionStorageSize {
        bytes: Some(bytes),
        scope: StorageScope::Directory,
        status: if partial {
            StorageStatus::Partial
        } else {
            StorageStatus::Complete
        },
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;

    // The stamps a request captures before its scan is queued.
    fn requested(root: &Path, directory: &Path, primary: &Path) -> Source {
        let (directory, directory_meta) = metadata(root, directory).unwrap();
        let (primary, primary_meta) = metadata(root, primary).unwrap();
        Source {
            root: root.canonicalize().unwrap(),
            directory,
            directory_stamp: FileStamp::from_metadata(&directory_meta),
            primary: Some((primary, FileStamp::from_metadata(&primary_meta))),
            owner: "fixture-owner".into(),
        }
    }

    #[test]
    fn scan_keeps_a_completed_size_for_appends_and_rejects_changed_identities() {
        let root =
            std::env::temp_dir().join(format!("agenthub-storage-scan-{}", uuid::Uuid::new_v4()));
        let directory = root.join("native-id");
        let primary = root.join("native-id.jsonl");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&primary, b"original-primary").unwrap();
        fs::write(directory.join("state"), b"state").unwrap();
        let unavailable = SessionStorageSize::unavailable(StorageScope::Directory);

        let source = requested(&root, &directory, &primary);
        fs::OpenOptions::new()
            .append(true)
            .open(&primary)
            .unwrap()
            .write_all(b"-appended")
            .unwrap();
        fs::write(directory.join("later"), b"later").unwrap();
        assert_eq!(
            scan(&source),
            SessionStorageSize {
                bytes: Some(16 + 9 + 5 + 5),
                scope: StorageScope::Directory,
                status: StorageStatus::Complete,
            }
        );

        fs::OpenOptions::new()
            .write(true)
            .open(&primary)
            .unwrap()
            .set_len(1)
            .unwrap();
        assert_eq!(scan(&source), unavailable);

        // The replacement is larger, so only the changed identity rejects it.
        let source = requested(&root, &directory, &primary);
        let replacement = root.join("replacement.jsonl");
        fs::write(&replacement, b"replacement-primary").unwrap();
        fs::rename(&replacement, &primary).unwrap();
        assert_eq!(scan(&source), unavailable);

        let source = requested(&root, &directory, &primary);
        fs::rename(&directory, root.join("original-directory")).unwrap();
        fs::create_dir(&directory).unwrap();
        assert_eq!(scan(&source), unavailable);
        fs::remove_dir_all(root).unwrap();
    }
}
