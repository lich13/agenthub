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

    pub fn without_ctime(mut self) -> Self {
        #[cfg(unix)]
        {
            self.identity.2 = 0;
            self.identity.3 = 0;
        }
        self
    }

    pub fn grew_into(&self, next: &Self) -> bool {
        self.same_inode(next) && next.len >= self.len
    }

    pub fn sqlite_pair(db_path: &Path) -> Option<(Self, Option<Self>)> {
        let main = Self::of_nofollow(db_path)?;
        let wal = Self::of_nofollow(&wal_sidecar_path(db_path));
        Some((main, wal))
    }

    pub fn sqlite_read_pair(db_path: &Path) -> Option<SqliteReadStampPair> {
        let main = Self::of_nofollow(db_path)?;
        let wal = WalReadStamp::of_nofollow(&wal_sidecar_path(db_path));
        Some((main, wal))
    }

    pub fn sqlite_read_pair_followed(db_path: &Path) -> Option<SqliteReadStampPair> {
        let main = Self::of_followed(db_path)?;
        let wal = WalReadStamp::of_followed(&wal_sidecar_path(db_path));
        Some((main, wal))
    }
}

/// Read-cache WAL stamp omitting `ctime` so root read-only SQLite opens (`fchown` on `-wal`)
/// do not invalidate unchanged caches while writes still update `len` and/or `modified`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalReadStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
    #[cfg(unix)]
    pub inode: (u64, u64),
}

pub type SqliteReadStampPair = (FileStamp, Option<WalReadStamp>);

impl WalReadStamp {
    pub fn from_metadata(meta: &Metadata) -> Self {
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino())
        };
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            #[cfg(unix)]
            inode,
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
}

pub(crate) fn wal_sidecar_path(db_path: &Path) -> PathBuf {
    let mut wal_os = db_path.as_os_str().to_os_string();
    wal_os.push("-wal");
    PathBuf::from(wal_os)
}

#[cfg(unix)]
pub(crate) fn probe_advisory_lock(path: &Path, mode: libc::c_int) -> std::io::Result<bool> {
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "锁路径不是普通文件",
        ));
    }
    // SAFETY: `file` owns this live descriptor throughout the non-blocking advisory lock probe.
    loop {
        let rc = unsafe { libc::flock(file.as_raw_fd(), mode | libc::LOCK_NB) };
        if rc == 0 {
            // SAFETY: `file` still owns the descriptor whose advisory lock is being released.
            unsafe {
                let _ = libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
            return Ok(false);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if matches!(
            err.raw_os_error(),
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
        ) {
            return Ok(true);
        }
        return Err(err);
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        io::{Seek, SeekFrom, Write},
        os::unix::{fs::PermissionsExt, io::AsRawFd},
    };

    #[test]
    fn sqlite_read_pair_ignores_wal_ctime_only_change_and_detects_writes() {
        let dir = std::env::temp_dir().join(format!(
            "agenthub-file-stamp-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let db = dir.join("sample.sqlite");
        let wal = wal_sidecar_path(&db);
        fs::write(&db, b"main-db").unwrap();
        fs::write(&wal, b"wal-frame-1").unwrap();
        fs::set_permissions(&wal, fs::Permissions::from_mode(0o644)).unwrap();

        let full_initial = FileStamp::sqlite_pair(&db).unwrap();
        let read_initial = FileStamp::sqlite_read_pair(&db).unwrap();
        let read_followed_initial = FileStamp::sqlite_read_pair_followed(&db).unwrap();

        // Metadata-only touch on `-wal` advances `ctime` without changing `len` or `mtime`.
        std::thread::sleep(Duration::from_millis(5));
        fs::set_permissions(&wal, fs::Permissions::from_mode(0o600)).unwrap();
        let full_after_chmod = FileStamp::sqlite_pair(&db).unwrap();
        let read_after_chmod = FileStamp::sqlite_read_pair(&db).unwrap();
        let read_followed_after_chmod = FileStamp::sqlite_read_pair_followed(&db).unwrap();
        assert_ne!(full_initial, full_after_chmod);
        assert_eq!(read_initial, read_after_chmod);
        assert_eq!(read_followed_initial, read_followed_after_chmod);

        // Appending to `-wal` changes `len` and invalidates `sqlite_read_pair`.
        {
            let mut f = fs::OpenOptions::new().append(true).open(&wal).unwrap();
            f.write_all(b"-extra").unwrap();
            f.sync_all().unwrap();
        }
        let read_after_append = FileStamp::sqlite_read_pair(&db).unwrap();
        assert_ne!(read_initial, read_after_append);

        // Same-length in-place rewrite advancing `mtime` invalidates `sqlite_read_pair`.
        {
            let mut f = fs::OpenOptions::new().write(true).open(&wal).unwrap();
            f.seek(SeekFrom::Start(0)).unwrap();
            f.write_all(b"WAL-FRAME-2-EXTRA").unwrap();
            let future_mtime = SystemTime::now() + Duration::from_secs(2);
            f.set_times(fs::FileTimes::new().set_modified(future_mtime))
                .unwrap();
        }
        let read_after_rewrite = FileStamp::sqlite_read_pair(&db).unwrap();
        assert_ne!(read_after_append, read_after_rewrite);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn probe_advisory_lock_distinguishes_missing_unlocked_locked_and_errors() {
        let dir = std::env::temp_dir().join(format!(
            "agenthub-lock-probe-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();

        assert!(!probe_advisory_lock(&dir.join("missing.lock"), libc::LOCK_SH).unwrap());

        let lock_file = dir.join("active.lock");
        let holder = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_file)
            .unwrap();
        assert!(!probe_advisory_lock(&lock_file, libc::LOCK_SH).unwrap());
        assert!(!probe_advisory_lock(&lock_file, libc::LOCK_EX).unwrap());

        assert_eq!(
            unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert!(probe_advisory_lock(&lock_file, libc::LOCK_SH).unwrap());
        assert!(probe_advisory_lock(&lock_file, libc::LOCK_EX).unwrap());
        drop(holder);
        assert!(!probe_advisory_lock(&lock_file, libc::LOCK_EX).unwrap());

        let symlink_lock = dir.join("symlink.lock");
        std::os::unix::fs::symlink(&lock_file, &symlink_lock).unwrap();
        assert!(probe_advisory_lock(&symlink_lock, libc::LOCK_SH).is_err());

        let dir_lock = dir.join("dir.lock");
        fs::create_dir_all(&dir_lock).unwrap();
        assert!(probe_advisory_lock(&dir_lock, libc::LOCK_SH).is_err());

        fs::remove_dir_all(dir).unwrap();
    }
}
