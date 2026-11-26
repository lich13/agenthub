use crate::file_stamp::{open_regular_nofollow, FileStamp};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    file: FileStamp,
    edge_hash: [u8; 32],
}

impl Stamp {
    fn read(path: &Path, hash_edges: bool) -> Option<Self> {
        // Coarse filesystem clocks can give a same-sized rewrite the same stamp.
        // Read bounded edges so an appended/replaced native record cannot reuse
        // a snapshot merely because its metadata timestamps share one clock tick.
        if hash_edges {
            let mut f = open_regular_nofollow(path).ok()?;
            let stamp = FileStamp::from_open_file(&f)?;
            let mut digest = Sha256::new();
            let mut edge = [0_u8; 4096];
            let head = f.read(&mut edge).ok()?;
            digest.update(&edge[..head]);
            if stamp.len > edge.len() as u64 {
                f.seek(SeekFrom::End(-(edge.len() as i64))).ok()?;
                let tail = f.read(&mut edge).ok()?;
                digest.update(&edge[..tail]);
            }
            Some(Self {
                file: stamp,
                edge_hash: digest.finalize().into(),
            })
        } else {
            let stamp = FileStamp::of_nofollow(path)?;
            stamp.modified?;
            Some(Self {
                file: stamp,
                edge_hash: [0; 32],
            })
        }
    }
}

struct Entry<T> {
    path: PathBuf,
    stamp: SourceStamp,
    value: Arc<T>,
    bytes: u64,
    last_accessed: std::time::Instant,
}

#[derive(Clone, PartialEq, Eq)]
enum SourceStamp {
    Files(Vec<Option<Stamp>>),
    Revision(Vec<u8>),
}

impl SourceStamp {
    fn size(&self) -> u64 {
        match self {
            Self::Files(files) => files.iter().flatten().map(|stamp| stamp.file.len).sum(),
            Self::Revision(_) => 0,
        }
    }
}

type InflightMap = std::collections::HashMap<PathBuf, Arc<Mutex<()>>>;

pub(crate) struct ReadCache<T> {
    entries: Mutex<VecDeque<Entry<T>>>,
    inflight: Mutex<Option<InflightMap>>,
    max_bytes: u64,
    max_entries: usize,
    hash_edges: bool,
}

impl<T> ReadCache<T> {
    pub(crate) const fn new(max_bytes: u64) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            inflight: Mutex::new(None),
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
    ) -> Result<Arc<T>> {
        self.read_stamped(
            path,
            || {
                Stamp::read(path, self.hash_edges)
                    .map(|stamp| SourceStamp::Files(vec![Some(stamp)]))
            },
            weight,
            load,
        )
    }

    pub(crate) fn read_sources(
        &self,
        key: &Path,
        sources: &[PathBuf],
        weight: impl Fn(&T, u64) -> u64,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<Arc<T>> {
        self.read_stamped(
            key,
            || {
                let stamps: Vec<_> = sources
                    .iter()
                    .map(|path| Stamp::read(path, self.hash_edges))
                    .collect();
                stamps
                    .iter()
                    .any(Option::is_some)
                    .then_some(SourceStamp::Files(stamps))
            },
            weight,
            load,
        )
    }

    /// The caller verifies the native identity and revision on each request.
    /// None invalidates a previous snapshot and reads without retaining it.
    pub(crate) fn read_revision(
        &self,
        key: &Path,
        revision: Option<Vec<u8>>,
        weight: impl Fn(&T, u64) -> u64,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<Arc<T>> {
        self.read_stamped(
            key,
            || revision.clone().map(SourceStamp::Revision),
            weight,
            load,
        )
    }

    fn read_stamped(
        &self,
        path: &Path,
        stamp: impl Fn() -> Option<SourceStamp>,
        weight: impl Fn(&T, u64) -> u64,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<Arc<T>> {
        let before = stamp();
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(mut entry) =
                crate::lru::take_first(&mut entries, |entry| entry.path == path)
            {
                if Some(&entry.stamp) == before.as_ref() {
                    entry.last_accessed = std::time::Instant::now();
                    let value = entry.value.clone();
                    entries.push_back(entry);
                    drop(entries);
                    return Ok(value);
                }
            }
        }
        let flight = self
            .inflight
            .lock()
            .ok()
            .map(|mut guard| {
                guard
                    .get_or_insert_with(std::collections::HashMap::new)
                    .entry(path.to_path_buf())
                    .or_insert_with(|| Arc::new(Mutex::new(())))
                    .clone()
            })
            .unwrap_or_else(|| Arc::new(Mutex::new(())));
        struct InflightCleanup<'a> {
            inflight: &'a Mutex<Option<InflightMap>>,
            path: &'a Path,
            flight: Arc<Mutex<()>>,
        }
        impl Drop for InflightCleanup<'_> {
            fn drop(&mut self) {
                if let Ok(mut guard) = self.inflight.lock() {
                    if let Some(map) = guard.as_mut() {
                        if map.get(self.path).is_some_and(|slot| {
                            Arc::ptr_eq(slot, &self.flight) && Arc::strong_count(slot) <= 2
                        }) {
                            map.remove(self.path);
                        }
                        if map.is_empty() {
                            *guard = None;
                        }
                    }
                }
            }
        }
        let cleanup = InflightCleanup {
            inflight: &self.inflight,
            path,
            flight,
        };
        let _flight_guard = cleanup.flight.lock().unwrap_or_else(|e| e.into_inner());
        let before = stamp();
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(mut entry) =
                crate::lru::take_first(&mut entries, |entry| entry.path == path)
            {
                if Some(&entry.stamp) == before.as_ref() {
                    entry.last_accessed = std::time::Instant::now();
                    let value = entry.value.clone();
                    entries.push_back(entry);
                    drop(entries);
                    return Ok(value);
                }
            }
        }
        let value = Arc::new(load()?);
        // Never publish a snapshot while its source is being appended or replaced.
        if let Some(stamp) = before.filter(|before| Some(before) == stamp().as_ref()) {
            let bytes = weight(&value, stamp.size());
            if bytes <= self.max_bytes {
                if let Ok(mut entries) = self.entries.lock() {
                    entries.retain(|entry| entry.path != path);
                    let total = entries.iter().map(|entry| entry.bytes).sum::<u64>() + bytes;
                    entries.push_back(Entry {
                        path: path.to_path_buf(),
                        stamp,
                        value: value.clone(),
                        bytes,
                        last_accessed: std::time::Instant::now(),
                    });
                    crate::lru::trim(
                        &mut entries,
                        self.max_entries,
                        total,
                        self.max_bytes,
                        |_| true,
                        |entry| entry.bytes,
                    );
                }
            }
        }
        Ok(value)
    }

    #[cfg(test)]
    pub(crate) fn cached_weight(&self, path: &Path) -> u64 {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.path == path)
            .map(|entry| entry.bytes)
            .sum()
    }

    pub(crate) fn evict_idle(&self, max_idle: std::time::Duration) -> usize {
        if let Ok(mut entries) = self.entries.lock() {
            let before = entries.len();
            let now = std::time::Instant::now();
            entries.retain(|entry| now.saturating_duration_since(entry.last_accessed) < max_idle);
            let evicted = before - entries.len();
            if evicted > 0 {
                if entries.capacity() > entries.len().saturating_mul(4).max(16) {
                    entries.shrink_to_fit();
                }
                crate::note_cache_eviction();
            }
            evicted
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, fs};

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
        assert_eq!(read().as_str(), "one");
        assert_eq!(read().as_str(), "one");
        assert_eq!(reads.get(), 1);
        cache.evict_idle(std::time::Duration::from_secs(900));
        assert_eq!(read().as_str(), "one");
        assert_eq!(reads.get(), 1);
        cache.evict_idle(std::time::Duration::ZERO);
        assert_eq!(read().as_str(), "one");
        assert_eq!(reads.get(), 2);
        fs::write(&path, "two").unwrap();
        assert_eq!(read().as_str(), "two");
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
