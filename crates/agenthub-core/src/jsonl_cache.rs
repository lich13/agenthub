//! Shared, bounded native JSONL reader. Only complete records enter a snapshot.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMetadata {
    pub epoch: String,
    pub revision: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_ids: Vec<String>,
    #[serde(default)]
    pub reset: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<HistoryChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryChange {
    pub revision: String,
    pub changed_ids: Vec<String>,
    pub reset: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Stamp {
    fn read(path: &Path) -> Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "会话必须是普通文件"
        );
        Self::from_metadata(&meta)
    }

    fn from_metadata(meta: &fs::Metadata) -> Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            size: meta.len(),
            modified: meta.modified()?,
            #[cfg(unix)]
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        })
    }

    fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.identity.0 == other.identity.0 && self.identity.1 == other.identity.1
        }
        #[cfg(not(unix))]
        {
            self.modified == other.modified
        }
    }
}

fn open(path: &Path) -> Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    Ok(options.open(path)?)
}

fn anchor(file: &mut File, offset: u64) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    let mut bytes = vec![0; offset.min(4096) as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut bytes)?;
    digest.update(&bytes);
    file.seek(SeekFrom::Start(offset.saturating_sub(bytes.len() as u64)))?;
    file.read_exact(&mut bytes)?;
    digest.update(&bytes);
    Ok(digest.finalize().into())
}

pub(crate) struct ReadInfo<'a> {
    pub epoch: &'a str,
    pub offset: u64,
    #[cfg(test)]
    pub lines: usize,
    pub partial: bool,
    pub changed: bool,
}

impl ReadInfo<'_> {
    pub fn revision(&self) -> String {
        format!("{}:{}", self.epoch, self.offset)
    }
}

struct Snapshot<S> {
    stamp: Stamp,
    offset: u64,
    lines: usize,
    epoch: String,
    anchor: [u8; 32],
    partial: bool,
    terminated: bool,
    state: S,
}

fn monotonic_ms() -> u64 {
    use std::{sync::OnceLock, time::Instant};
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

struct Entry<S> {
    path: PathBuf,
    snapshot: Mutex<Option<Snapshot<S>>>,
    weight: std::sync::atomic::AtomicU64,
    last_accessed_ms: std::sync::atomic::AtomicU64,
}

pub(crate) struct JsonlCache<S> {
    entries: Mutex<VecDeque<Arc<Entry<S>>>>,
    max_bytes: u64,
    max_line_bytes: Option<u64>,
}

impl<S: Default> JsonlCache<S> {
    pub const fn new(max_bytes: u64) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_bytes,
            max_line_bytes: None,
        }
    }

    pub const fn with_line_limit(mut self, max_bytes: u64) -> Self {
        self.max_line_bytes = Some(max_bytes);
        self
    }

    /// The per-file lock coalesces readers. No global lock is held during I/O,
    /// parsing, or page construction, and only the requested result is copied.
    pub fn read<R>(
        &self,
        path: &Path,
        start: impl FnOnce(u64) -> Result<u64>,
        accept_complete_tail: bool,
        mut consume: impl FnMut(&mut S, &[u8], usize, u64) -> Result<()>,
        finish: impl FnOnce(&mut S, ReadInfo<'_>) -> Result<R>,
        weight: impl FnOnce(&S) -> u64,
    ) -> Result<R> {
        use std::sync::atomic::Ordering;
        let now_ms = monotonic_ms();
        let entry = {
            let mut entries = self
                .entries
                .lock()
                .map_err(|_| anyhow::anyhow!("会话缓存不可用"))?;
            let entry = entries
                .iter()
                .position(|entry| entry.path == path)
                .and_then(|index| entries.remove(index))
                .unwrap_or_else(|| {
                    Arc::new(Entry {
                        path: path.into(),
                        snapshot: Mutex::new(None),
                        weight: 0.into(),
                        last_accessed_ms: now_ms.into(),
                    })
                });
            entry.last_accessed_ms.store(now_ms, Ordering::Relaxed);
            entries.push_back(entry.clone());
            entry
        };
        let result = (|| {
            let mut cached = entry
                .snapshot
                .lock()
                .map_err(|_| anyhow::anyhow!("会话缓存不可用"))?;
            let before = Stamp::read(path).context("无法读取会话文件")?;
            let changed = cached.as_ref().is_none_or(|value| value.stamp != before);
            if changed {
                let initial_offset = start(before.size)?.min(before.size);
                let mut file = open(path)?;
                ensure!(
                    Stamp::from_metadata(&file.metadata()?)? == before,
                    "会话文件身份已变化，请重试"
                );
                let mut separator = 0;
                let append = cached.as_ref().is_some_and(|value| {
                    before.size > value.stamp.size
                        && before.same_file(&value.stamp)
                        && anchor(&mut file, value.offset).ok() == Some(value.anchor)
                        && (value.terminated || {
                            // A complete JSON value without a final newline is
                            // provisional until its next separator is known.
                            // Extending that same line requires a fresh parse.
                            let mut bytes = [0; 2];
                            let count = (before.size - value.offset).min(2) as usize;
                            let read = file
                                .seek(SeekFrom::Start(value.offset))
                                .and_then(|_| file.read_exact(&mut bytes[..count]));
                            if read.is_err() {
                                false
                            } else if bytes[0] == b'\n' {
                                separator = 1;
                                true
                            } else if count == 2 && bytes == *b"\r\n" {
                                separator = 2;
                                true
                            } else {
                                false
                            }
                        })
                });
                let mut snapshot = if append {
                    cached.take().expect("append snapshot")
                } else {
                    let offset = initial_offset;
                    Snapshot {
                        stamp: before.clone(),
                        offset,
                        lines: 0,
                        epoch: uuid::Uuid::new_v4().to_string(),
                        anchor: [0; 32],
                        partial: false,
                        terminated: true,
                        state: S::default(),
                    }
                };
                snapshot.offset += separator;
                if separator > 0 {
                    snapshot.terminated = true;
                }
                file.seek(SeekFrom::Start(snapshot.offset))?;
                let mut input =
                    BufReader::with_capacity(64 * 1024, file.take(before.size - snapshot.offset));
                let mut line = Vec::new();
                let read_line = |input: &mut BufReader<std::io::Take<File>>,
                                 line: &mut Vec<u8>|
                 -> Result<usize> {
                    if let Some(limit) = self.max_line_bytes {
                        let length = input.by_ref().take(limit + 1).read_until(b'\n', line)?;
                        ensure!(length as u64 <= limit, "会话记录超出读取上限");
                        Ok(length)
                    } else {
                        Ok(input.read_until(b'\n', line)?)
                    }
                };
                if !append && snapshot.offset > 0 {
                    snapshot.offset += read_line(&mut input, &mut line)? as u64;
                    line.clear();
                }
                snapshot.partial = false;
                loop {
                    line.clear();
                    let length = read_line(&mut input, &mut line)?;
                    if length == 0 {
                        break;
                    }
                    if !(line.ends_with(b"\n")
                        || accept_complete_tail
                            && serde_json::from_slice::<serde_json::Value>(&line).is_ok())
                    {
                        snapshot.partial = true;
                        break;
                    }
                    snapshot.lines += 1;
                    consume(&mut snapshot.state, &line, snapshot.lines, snapshot.offset)?;
                    snapshot.offset += length as u64;
                    snapshot.terminated = line.ends_with(b"\n");
                }
                let mut file = input.into_inner().into_inner();
                snapshot.anchor = anchor(&mut file, snapshot.offset)?;
                let after = Stamp::read(path)?;
                ensure!(
                    after == before || (after.same_file(&before) && after.size > before.size),
                    "会话文件已变化，请重试"
                );
                snapshot.stamp = before;
                *cached = Some(snapshot);
            }
            let snapshot = cached.as_mut().context("会话快照不可用")?;
            let info = ReadInfo {
                epoch: &snapshot.epoch,
                offset: snapshot.offset,
                #[cfg(test)]
                lines: snapshot.lines,
                partial: snapshot.partial,
                changed,
            };
            let result = finish(&mut snapshot.state, info);
            entry.weight.store(
                weight(&snapshot.state).saturating_add(1024),
                Ordering::Relaxed,
            );
            if result.is_err() {
                // A reducer may have failed while building a derived snapshot.
                // Retrying the unchanged file must rebuild, not reuse that state.
                *cached = None;
                entry.weight.store(0, Ordering::Relaxed);
            }
            result
        })();
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("会话缓存不可用"))?;
        let mut bytes = entries
            .iter()
            .map(|entry| entry.weight.load(Ordering::Relaxed))
            .sum::<u64>();
        while entries.len() > crate::resource_limits::SOURCE_CACHE_ENTRIES || bytes > self.max_bytes
        {
            let Some(index) = entries
                .iter()
                .position(|entry| Arc::strong_count(entry) == 1)
            else {
                break;
            };
            let evicted = entries.remove(index).expect("cache entry");
            bytes = bytes.saturating_sub(evicted.weight.load(Ordering::Relaxed));
        }
        // An oversize active snapshot is not retained once its reader releases it.
        if entry.weight.load(Ordering::Relaxed) > self.max_bytes {
            entries.retain(|candidate| !Arc::ptr_eq(candidate, &entry));
        }
        result
    }

    pub fn evict_idle(&self, max_idle: std::time::Duration) {
        use std::sync::atomic::Ordering;
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let now_ms = monotonic_ms();
        let max_idle_ms = max_idle.as_millis() as u64;
        entries.retain(|entry| {
            Arc::strong_count(entry) > 1
                || now_ms.saturating_sub(entry.last_accessed_ms.load(Ordering::Relaxed))
                    < max_idle_ms
        });
    }
}

/// A bounded revision journal lets clients refresh only historical rows which
/// actually changed. An expired revision requests a safe refresh, never silence.
#[derive(Default, Clone)]
pub(crate) struct HistoryJournal {
    epoch: String,
    revision: String,
    changes: VecDeque<HistoryChange>,
}

impl HistoryJournal {
    pub fn record(&mut self, info: &ReadInfo<'_>, changed_ids: Vec<String>) {
        self.record_changes(info, changed_ids, false);
    }

    pub fn record_changes(&mut self, info: &ReadInfo<'_>, changed_ids: Vec<String>, reset: bool) {
        let revision = info.revision();
        if self.epoch != info.epoch {
            self.changes.clear();
            self.epoch = info.epoch.to_owned();
        }
        if self.revision != revision {
            let reset = reset || changed_ids.len() > crate::resource_limits::HISTORY_CHANGED_IDS;
            self.changes.push_back(HistoryChange {
                revision: revision.clone(),
                changed_ids: if reset { Vec::new() } else { changed_ids },
                reset,
            });
            while self.changes.len() > crate::resource_limits::HISTORY_REVISIONS
                || self
                    .changes
                    .iter()
                    .map(|change| change.changed_ids.len())
                    .sum::<usize>()
                    > crate::resource_limits::HISTORY_CHANGED_IDS
            {
                self.changes.pop_front();
            }
            self.revision = revision;
        }
    }

    pub fn metadata(&self, since: Option<&str>) -> HistoryMetadata {
        let mut changed_ids = std::collections::BTreeSet::new();
        let mut reset = false;
        if let Some(since) = since.filter(|since| *since != self.revision) {
            if let Some(index) = self
                .changes
                .iter()
                .position(|change| change.revision == since)
            {
                for change in self.changes.iter().skip(index + 1) {
                    reset |= change.reset;
                    changed_ids.extend(change.changed_ids.iter().cloned());
                }
                if changed_ids.len() > crate::resource_limits::HISTORY_CHANGED_IDS {
                    reset = true;
                    changed_ids.clear();
                }
            } else {
                reset = true;
            }
        }
        HistoryMetadata {
            epoch: self.epoch.clone(),
            revision: self.revision.clone(),
            changed_ids: changed_ids.into_iter().collect(),
            reset,
            changes: self.changes.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests;
