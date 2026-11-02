//! Read-only, metadata-only session sizes. Callers establish native ownership first.
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, Metadata},
    path::{Component, Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::{Instant, SystemTime},
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl Stamp {
    fn new(meta: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            #[cfg(unix)]
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        }
    }
    fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            (self.identity.0, self.identity.1) == (other.identity.0, other.identity.1)
        }
        #[cfg(not(unix))]
        {
            self == other
        }
    }
}

// Reject links in every component below the configured root, including the leaf.
// Canonicalizing a path alone would silently count a link's external target.
fn metadata(root: &Path, path: &Path) -> Option<(PathBuf, Metadata)> {
    if fs::symlink_metadata(root).ok()?.file_type().is_symlink() {
        return None;
    }
    let canonical_root = root.canonicalize().ok()?;
    let relative = path
        .strip_prefix(root)
        .or_else(|_| path.strip_prefix(&canonical_root))
        .ok()?;
    let mut resolved = canonical_root;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        resolved.push(name);
        if fs::symlink_metadata(&resolved)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return None;
        }
    }
    let meta = fs::symlink_metadata(&resolved).ok()?;
    Some((resolved, meta))
}

pub fn file_size(root: &Path, file: &Path) -> SessionStorageSize {
    match metadata(root, file).filter(|(_, meta)| meta.is_file()) {
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
    directory_stamp: Stamp,
    primary: Option<(PathBuf, Stamp)>,
    owner: String,
}
impl Source {
    fn current(&self) -> bool {
        metadata(&self.root, &self.directory)
            .is_some_and(|(_, meta)| Stamp::new(&meta) == self.directory_stamp)
            && self.primary.as_ref().is_none_or(|(path, stamp)| {
                metadata(&self.root, path).is_some_and(|(_, meta)| Stamp::new(&meta) == *stamp)
            })
    }
    fn same_slot(&self, other: &Self) -> bool {
        self.root == other.root && self.directory == other.directory && self.owner == other.owner
    }
    fn can_reuse_size(&self, next: &Self) -> bool {
        self.same_slot(next)
            && self.directory_stamp.same_file(&next.directory_stamp)
            && match (&self.primary, &next.primary) {
                (None, None) => true,
                (Some((path, stamp)), Some((next_path, next_stamp))) => {
                    path == next_path && stamp.same_file(next_stamp) && next_stamp.len >= stamp.len
                }
                _ => false,
            }
    }
}
struct Entry {
    source: Source,
    value: SessionStorageSize,
    checked: Instant,
    queued: bool,
}
struct Cache {
    entries: Arc<Mutex<VecDeque<Entry>>>,
    queue: mpsc::SyncSender<Source>,
}
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
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let mut entry = if let Some(index) = entries
            .iter()
            .position(|entry| entry.source.same_slot(&source))
        {
            entries.remove(index).expect("existing entry")
        } else {
            Entry {
                source: source.clone(),
                value: pending_size(),
                checked: Instant::now(),
                queued: false,
            }
        };
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
        while entries.len() >= CACHE_ENTRIES {
            if let Some(index) = entries.iter().position(|entry| !entry.queued) {
                entries.remove(index);
            } else {
                entries.pop_front();
            }
        }
        entries.push_back(entry);
        value
    }
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
    let Some((directory, meta)) = metadata(root, directory).filter(|(_, meta)| meta.is_dir())
    else {
        return unavailable();
    };
    let primary = match primary {
        Some(file) => match metadata(root, file).filter(|(_, meta)| meta.is_file()) {
            Some((path, meta)) => Some((path, Stamp::new(&meta))),
            None => return unavailable(),
        },
        None => None,
    };
    let Some(root) = root.canonicalize().ok() else {
        return unavailable();
    };
    let source = Source {
        root,
        directory,
        directory_stamp: Stamp::new(&meta),
        primary,
        owner: owner.to_owned(),
    };
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Cache::new).get(source)
}

fn scan(source: &Source) -> SessionStorageSize {
    let mut bytes = 0u64;
    let mut partial = false;
    let mut seen = HashSet::new();
    let started = Instant::now();
    let mut add = |path: &Path, meta: Metadata| {
        #[cfg(unix)]
        let key = {
            use std::os::unix::fs::MetadataExt;
            format!("{}:{}", meta.dev(), meta.ino())
        };
        #[cfg(not(unix))]
        let key = path.to_string_lossy().into_owned();
        let _ = path;
        if seen.insert(key) {
            bytes = bytes.saturating_add(meta.len());
        }
    };
    if let Some((path, _)) = &source.primary {
        if let Some((path, meta)) = metadata(&source.root, path) {
            add(&path, meta);
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
        let Some((path, meta)) = metadata(&source.root, entry.path()) else {
            partial = true;
            continue;
        };
        if meta.is_file() {
            add(&path, meta);
        } else if !meta.is_dir() || entry.depth() == SCAN_DEPTH {
            partial = true;
        }
    }
    // A replaced or changing source cannot publish a result under an old identity.
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
