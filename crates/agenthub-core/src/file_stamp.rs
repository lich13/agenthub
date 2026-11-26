use anyhow::{ensure, Result};
use std::{
    collections::VecDeque,
    fs::{self, File, Metadata},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
    #[cfg(unix)]
    pub identity: (u64, u64, i64, i64),
}

impl FileStamp {
    pub fn from_metadata(meta: &Metadata) -> Self {
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
        };
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            #[cfg(unix)]
            identity,
        }
    }

    pub fn of_nofollow(path: &Path) -> Option<Self> {
        let meta = fs::symlink_metadata(path).ok()?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return None;
        }
        Some(Self::from_metadata(&meta))
    }

    pub fn of_followed(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        Some(Self::from_metadata(&meta))
    }

    pub fn read_regular_nofollow(path: &Path, err_msg: &'static str) -> Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "{err_msg}"
        );
        let _ = meta.modified()?;
        Ok(Self::from_metadata(&meta))
    }

    pub fn from_open_file(file: &File) -> Option<Self> {
        let meta = file.metadata().ok()?;
        if !meta.is_file() {
            return None;
        }
        let _ = meta.modified().ok()?;
        Some(Self::from_metadata(&meta))
    }

    pub fn same_inode(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.identity.0 == other.identity.0 && self.identity.1 == other.identity.1
        }
        #[cfg(not(unix))]
        {
            let _ = other;
            true
        }
    }

    pub fn grew_into(&self, next: &Self) -> bool {
        self.same_inode(next) && next.len >= self.len
    }

    pub fn sqlite_pair(db_path: &Path) -> Option<(Self, Option<Self>)> {
        let main = Self::of_nofollow(db_path)?;
        let mut wal_os = db_path.as_os_str().to_os_string();
        wal_os.push("-wal");
        let wal = Self::of_nofollow(&PathBuf::from(wal_os));
        Some((main, wal))
    }
}

pub fn open_regular_nofollow(path: &Path) -> Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(file.metadata()?.is_file(), "目标不是普通文件");
    Ok(file)
}

struct StampedEntry<S, V> {
    path: PathBuf,
    stamp: S,
    value: V,
    bytes: u64,
    last_accessed: Instant,
}

pub(crate) struct StampedCache<S, V> {
    entries: Mutex<VecDeque<StampedEntry<S, V>>>,
    max_entries: usize,
    max_bytes: u64,
}

impl<S: Clone + PartialEq, V: Clone> StampedCache<S, V> {
    pub(crate) const fn new(max_entries: usize, max_bytes: u64) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_entries,
            max_bytes,
        }
    }

    pub(crate) fn get(&self, path: &Path, expected_stamp: &S) -> Option<V> {
        let mut entries = self.entries.lock().ok()?;
        let mut entry = crate::lru::take_first(&mut entries, |e| e.path == path)?;
        if &entry.stamp == expected_stamp {
            entry.last_accessed = Instant::now();
            let val = entry.value.clone();
            entries.push_back(entry);
            Some(val)
        } else {
            crate::note_cache_eviction();
            None
        }
    }

    pub(crate) fn insert(&self, path: PathBuf, stamp: S, value: V, weight_bytes: u64) {
        let bytes = weight_bytes.max(64);
        if bytes > self.max_bytes {
            return;
        }
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        entries.retain(|e| e.path != path);
        let total = entries.iter().map(|e| e.bytes).sum::<u64>() + bytes;
        entries.push_back(StampedEntry {
            path,
            stamp,
            value,
            bytes,
            last_accessed: Instant::now(),
        });
        crate::lru::trim(
            &mut entries,
            self.max_entries,
            total,
            self.max_bytes,
            |_| true,
            |e| e.bytes,
        );
    }

    pub(crate) fn evict_idle(&self, max_idle: Duration) -> usize {
        let Ok(mut entries) = self.entries.lock() else {
            return 0;
        };
        let before = entries.len();
        let now = Instant::now();
        entries.retain(|e| now.saturating_duration_since(e.last_accessed) < max_idle);
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
