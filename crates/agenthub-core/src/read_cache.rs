use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    edge_hash: [u8; 32],
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Stamp {
    fn read(path: &Path, hash_edges: bool) -> Option<Self> {
        // Coarse filesystem clocks can give a same-sized rewrite the same stamp.
        // Read bounded edges so an appended/replaced native record cannot reuse
        // a snapshot merely because its metadata timestamps share one clock tick.
        let (metadata, edge_hash) = if hash_edges {
            let mut file = fs::File::open(path).ok()?;
            let metadata = file.metadata().ok()?;
            let mut digest = Sha256::new();
            let mut edge = [0_u8; 4096];
            let head = file.read(&mut edge).ok()?;
            digest.update(&edge[..head]);
            if metadata.len() > edge.len() as u64 {
                file.seek(SeekFrom::End(-(edge.len() as i64))).ok()?;
                let tail = file.read(&mut edge).ok()?;
                digest.update(&edge[..tail]);
            }
            (metadata, digest.finalize().into())
        } else {
            let metadata = fs::symlink_metadata(path).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return None;
            }
            (metadata, [0; 32])
        };
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            size: metadata.len(),
            modified: metadata.modified().ok()?,
            edge_hash,
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        })
    }
}

struct Entry<T> {
    path: PathBuf,
    stamp: Stamp,
    value: Arc<T>,
    bytes: u64,
    last_accessed: std::time::Instant,
}

pub(crate) struct ReadCache<T> {
    entries: Mutex<VecDeque<Entry<T>>>,
    max_bytes: u64,
    max_entries: usize,
    hash_edges: bool,
}

impl<T: Clone> ReadCache<T> {
    pub(crate) const fn new(max_bytes: u64) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_bytes,
            max_entries: crate::resource_limits::SOURCE_CACHE_ENTRIES,
            hash_edges: true,
        }
    }

    pub(crate) const fn with_entry_limit(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
    }

    /// Derived JSONL summaries use the same native identity and nanosecond
    /// mtime/ctime invalidation as their source cache. Rehashing both file edges
    /// before and after each summary duplicates the source reader's cold I/O.
    /// Keep edge validation on platforms without a native change timestamp.
    pub(crate) const fn with_native_metadata(mut self) -> Self {
        self.hash_edges = !cfg!(unix);
        self
    }

    pub(crate) fn read(
        &self,
        path: &Path,
        weight: impl Fn(&T, u64) -> u64,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let before = Stamp::read(path, self.hash_edges);
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(index) = entries.iter().position(|entry| entry.path == path) {
                let mut entry = entries.remove(index).expect("known cache entry");
                if Some(&entry.stamp) == before.as_ref() {
                    entry.last_accessed = std::time::Instant::now();
                    let value = entry.value.clone();
                    entries.push_back(entry);
                    drop(entries);
                    return Ok((*value).clone());
                }
            }
        }
        let value = Arc::new(load()?);
        // Never publish a snapshot while its source is being appended or replaced.
        if let Some(stamp) =
            before.filter(|stamp| Some(stamp) == Stamp::read(path, self.hash_edges).as_ref())
        {
            let bytes = weight(&value, stamp.size);
            if bytes <= self.max_bytes {
                if let Ok(mut entries) = self.entries.lock() {
                    entries.retain(|entry| entry.path != path);
                    while entries.len() >= self.max_entries
                        || entries.iter().map(|entry| entry.bytes).sum::<u64>() + bytes
                            > self.max_bytes
                    {
                        if entries.pop_front().is_none() {
                            break;
                        }
                    }
                    entries.push_back(Entry {
                        path: path.to_path_buf(),
                        stamp,
                        value: value.clone(),
                        bytes,
                        last_accessed: std::time::Instant::now(),
                    });
                }
            }
        }
        Ok(Arc::unwrap_or_clone(value))
    }

    pub(crate) fn evict_idle(&self, max_idle: std::time::Duration) {
        if let Ok(mut entries) = self.entries.lock() {
            let now = std::time::Instant::now();
            entries.retain(|entry| now.saturating_duration_since(entry.last_accessed) < max_idle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn rollout_read_cache_reuses_only_unchanged_files_and_is_bounded() {
        let path = std::env::temp_dir().join(format!("agenthub-cache-{}", uuid::Uuid::new_v4()));
        fs::write(&path, "one").unwrap();
        let cache = ReadCache::new(6);
        let reads = Cell::new(0);
        let read = || {
            cache
                .read(
                    &path,
                    |_, size| size,
                    || {
                        reads.set(reads.get() + 1);
                        Ok(fs::read_to_string(&path)?)
                    },
                )
                .unwrap()
        };
        assert_eq!(read(), "one");
        assert_eq!(read(), "one");
        assert_eq!(reads.get(), 1);
        cache.evict_idle(std::time::Duration::from_secs(900));
        assert_eq!(read(), "one");
        assert_eq!(reads.get(), 1);
        cache.evict_idle(std::time::Duration::ZERO);
        assert_eq!(read(), "one");
        assert_eq!(reads.get(), 2);
        fs::write(&path, "two").unwrap();
        assert_eq!(read(), "two");
        assert_eq!(reads.get(), 3);
        fs::write(&path, "too large").unwrap();
        read();
        read();
        assert_eq!(reads.get(), 5);
        fs::remove_file(&path).unwrap();
        assert!(cache
            .read(&path, |_, size| size, || Ok(fs::read_to_string(&path)?))
            .is_err());
    }
}
