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
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            size: meta.len(),
            modified: meta.modified()?,
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        })
    }

    fn same_file(&self, other: &Self) -> bool {
        self.identity.0 == other.identity.0 && self.identity.1 == other.identity.1
    }
}

fn open(path: &Path) -> Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(libc::O_NOFOLLOW);
    Ok(options.open(path)?)
}

pub(crate) fn open_regular(path: &Path) -> Result<File> {
    let before = Stamp::read(path)?;
    let file = open(path)?;
    ensure!(
        Stamp::from_metadata(&file.metadata()?)? == before,
        "会话文件身份已变化，请重试"
    );
    Ok(file)
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
    rewrite_gen: u64,
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

fn strip_line_ending(mut line: &[u8]) -> &[u8] {
    if line.ends_with(b"\n") {
        line = &line[..line.len() - 1];
    }
    if line.ends_with(b"\r") {
        line = &line[..line.len() - 1];
    }
    line
}

fn derive_epoch(
    stamp: &Stamp,
    scope: &str,
    compressed: bool,
    start_offset: u64,
    rewrite_gen: u64,
    first_record_digest: Option<[u8; 32]>,
    compressed_stream: Option<(u64, [u8; 32])>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"agenthub-jsonl-epoch-v1\0");
    hasher.update(stamp.identity.0.to_le_bytes());
    hasher.update(stamp.identity.1.to_le_bytes());
    hasher.update((scope.len() as u64).to_le_bytes());
    hasher.update(scope.as_bytes());
    hasher.update([u8::from(compressed)]);
    hasher.update(start_offset.to_le_bytes());
    hasher.update(rewrite_gen.to_le_bytes());
    if let Some((size, stream_anchor)) = compressed_stream {
        hasher.update([2u8]);
        hasher.update(size.to_le_bytes());
        hasher.update(stream_anchor);
    } else if let Some(first_digest) = first_record_digest {
        hasher.update([1u8]);
        hasher.update(first_digest);
    } else {
        hasher.update([0u8]);
    }
    let full = hasher.finalize();
    hex::encode(&full[..16])
}

struct Lineage {
    identity: (u64, u64),
    scope: String,
    last_offset: u64,
    last_anchor: [u8; 32],
    terminated: bool,
    rewrite_gen: u64,
}

struct Entry<S> {
    path: PathBuf,
    scope: String,
    snapshot: Mutex<Option<Snapshot<S>>>,
    weight: std::sync::atomic::AtomicU64,
    last_accessed_ms: std::sync::atomic::AtomicU64,
}

pub(crate) struct JsonlCache<S> {
    entries: Mutex<VecDeque<Arc<Entry<S>>>>,
    lineage: Mutex<VecDeque<Lineage>>,
    max_bytes: u64,
    max_entries: usize,
    max_line_bytes: Option<u64>,
}

impl<S: Default> JsonlCache<S> {
    pub const fn new(max_bytes: u64) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            lineage: Mutex::new(VecDeque::new()),
            max_bytes,
            max_entries: crate::resource_limits::SOURCE_CACHE_ENTRIES,
            max_line_bytes: None,
        }
    }

    pub const fn with_entry_limit(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
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
        consume: impl FnMut(&mut S, &[u8], usize, u64) -> Result<()>,
        finish: impl FnOnce(&mut S, ReadInfo<'_>) -> Result<R>,
        weight: impl FnOnce(&S) -> u64,
    ) -> Result<R> {
        self.read_scoped(
            (path, ""),
            start,
            accept_complete_tail,
            consume,
            finish,
            weight,
        )
    }

    /// A verified reader context can share the byte budget without sharing parser state.
    pub fn read_scoped<R>(
        &self,
        source: (&Path, &str),
        start: impl FnOnce(u64) -> Result<u64>,
        accept_complete_tail: bool,
        consume: impl FnMut(&mut S, &[u8], usize, u64) -> Result<()>,
        finish: impl FnOnce(&mut S, ReadInfo<'_>) -> Result<R>,
        weight: impl FnOnce(&S) -> u64,
    ) -> Result<R> {
        self.read_source(
            (source.0, source.1, false),
            start,
            accept_complete_tail,
            consume,
            finish,
            weight,
        )
    }

    /// Compressed files share this cache's budget. Any source change rebuilds
    /// the decoded snapshot; unchanged files need no decompression or parsing.
    pub fn read_compressed<R>(
        &self,
        path: &Path,
        accept_complete_tail: bool,
        consume: impl FnMut(&mut S, &[u8], usize, u64) -> Result<()>,
        finish: impl FnOnce(&mut S, ReadInfo<'_>) -> Result<R>,
        weight: impl FnOnce(&S) -> u64,
    ) -> Result<R> {
        self.read_source(
            (path, "", true),
            |_| Ok(0),
            accept_complete_tail,
            consume,
            finish,
            weight,
        )
    }

    fn read_source<R>(
        &self,
        source: (&Path, &str, bool),
        start: impl FnOnce(u64) -> Result<u64>,
        accept_complete_tail: bool,
        mut consume: impl FnMut(&mut S, &[u8], usize, u64) -> Result<()>,
        finish: impl FnOnce(&mut S, ReadInfo<'_>) -> Result<R>,
        weight: impl FnOnce(&S) -> u64,
    ) -> Result<R> {
        use std::sync::atomic::Ordering;
        let (path, scope, compressed) = source;
        let now_ms = monotonic_ms();
        let entry = {
            let mut entries = self
                .entries
                .lock()
                .map_err(|_| anyhow::anyhow!("会话缓存不可用"))?;
            let entry = crate::lru::take_first(&mut entries, |entry| {
                entry.path == path && entry.scope == scope
            })
            .unwrap_or_else(|| {
                Arc::new(Entry {
                    path: path.into(),
                    scope: scope.into(),
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
                    !compressed
                        && before.size > value.stamp.size
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
                let file_id = (before.identity.0, before.identity.1);
                let prior_lineage = self.lineage.lock().ok().and_then(|guard| {
                    guard
                        .iter()
                        .find(|item| item.identity == file_id && item.scope == scope)
                        .map(|item| {
                            (
                                item.last_offset,
                                item.last_anchor,
                                item.terminated,
                                item.rewrite_gen,
                            )
                        })
                });
                let prev_cached_gen = cached
                    .as_ref()
                    .filter(|value| before.same_file(&value.stamp))
                    .map(|value| value.rewrite_gen);
                let had_cached = cached.is_some();
                let mut snapshot = if append {
                    cached.take().expect("append snapshot")
                } else {
                    let offset = initial_offset;
                    Snapshot {
                        stamp: before.clone(),
                        offset,
                        lines: 0,
                        epoch: String::new(),
                        rewrite_gen: 0,
                        anchor: [0; 32],
                        partial: false,
                        terminated: true,
                        state: S::default(),
                    }
                };
                let rewrite_gen = if append {
                    snapshot.rewrite_gen
                } else if let Some(prev_gen) = prev_cached_gen {
                    prev_gen + 1
                } else if let Some((prev_offset, prev_anchor, prev_terminated, prev_gen)) =
                    prior_lineage
                {
                    let valid_cold_continuation = !compressed
                        && !had_cached
                        && before.size >= prev_offset
                        && anchor(&mut file, prev_offset).ok() == Some(prev_anchor)
                        && (prev_terminated || before.size == prev_offset || {
                            let mut bytes = [0; 2];
                            let count = (before.size - prev_offset).min(2) as usize;
                            file.seek(SeekFrom::Start(prev_offset))
                                .and_then(|_| file.read_exact(&mut bytes[..count]))
                                .is_ok()
                                && (bytes[0] == b'\n' || (count == 2 && bytes == *b"\r\n"))
                        });
                    let unchanged_compressed = compressed
                        && !had_cached
                        && before.size == prev_offset
                        && anchor(&mut file, before.size).ok() == Some(prev_anchor);
                    if valid_cold_continuation || unchanged_compressed {
                        prev_gen
                    } else {
                        prev_gen + 1
                    }
                } else {
                    0
                };
                snapshot.rewrite_gen = rewrite_gen;
                let was_empty = snapshot.lines == 0 && snapshot.offset == 0;
                snapshot.offset += separator;
                if separator > 0 {
                    snapshot.terminated = true;
                }
                file.seek(SeekFrom::Start(snapshot.offset))?;
                let input = (&mut file).take(before.size - snapshot.offset);
                let input: Box<dyn Read + '_> = if compressed {
                    Box::new(zstd::stream::read::Decoder::new(input)?)
                } else {
                    Box::new(input)
                };
                let mut input = BufReader::with_capacity(64 * 1024, input);
                let mut line = Vec::new();
                let read_line = |input: &mut BufReader<Box<dyn Read + '_>>,
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
                let effective_start_offset = snapshot.offset;
                let mut first_record_digest: Option<[u8; 32]> = None;
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
                    if (!append || was_empty) && !compressed && first_record_digest.is_none() {
                        first_record_digest = Some(Sha256::digest(strip_line_ending(&line)).into());
                    }
                    snapshot.lines += 1;
                    consume(&mut snapshot.state, &line, snapshot.lines, snapshot.offset)?;
                    snapshot.offset += length as u64;
                    snapshot.terminated = line.ends_with(b"\n");
                }
                drop(input);
                if compressed {
                    let stream_anchor = anchor(&mut file, before.size)?;
                    snapshot.anchor = stream_anchor;
                    snapshot.epoch = derive_epoch(
                        &before,
                        scope,
                        true,
                        effective_start_offset,
                        rewrite_gen,
                        None,
                        Some((before.size, stream_anchor)),
                    );
                } else {
                    snapshot.anchor = anchor(&mut file, snapshot.offset)?;
                    if !append || (was_empty && first_record_digest.is_some()) {
                        snapshot.epoch = derive_epoch(
                            &before,
                            scope,
                            false,
                            effective_start_offset,
                            rewrite_gen,
                            first_record_digest,
                            None,
                        );
                    }
                }
                let after = Stamp::read(path)?;
                ensure!(
                    after == before
                        || (!compressed && after.same_file(&before) && after.size > before.size),
                    "会话文件已变化，请重试"
                );
                if let Ok(mut guard) = self.lineage.lock() {
                    let recorded_offset = if compressed {
                        before.size
                    } else {
                        snapshot.offset
                    };
                    if let Some(pos) = guard
                        .iter()
                        .position(|item| item.identity == file_id && item.scope == scope)
                    {
                        guard.remove(pos);
                    }
                    while guard.len() >= 256 {
                        guard.pop_front();
                    }
                    guard.push_back(Lineage {
                        identity: file_id,
                        scope: scope.into(),
                        last_offset: recorded_offset,
                        last_anchor: snapshot.anchor,
                        terminated: snapshot.terminated,
                        rewrite_gen,
                    });
                }
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
        let bytes = entries
            .iter()
            .map(|entry| entry.weight.load(Ordering::Relaxed))
            .sum::<u64>();
        crate::lru::trim(
            &mut entries,
            self.max_entries,
            bytes,
            self.max_bytes,
            |entry| Arc::strong_count(entry) == 1,
            |entry| entry.weight.load(Ordering::Relaxed),
        );
        // An oversize active snapshot is not retained once its reader releases it.
        if entry.weight.load(Ordering::Relaxed) > self.max_bytes {
            entries.retain(|candidate| !Arc::ptr_eq(candidate, &entry));
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn cached_weight(&self, path: &Path) -> u64 {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.path == path)
            .map(|entry| entry.weight.load(std::sync::atomic::Ordering::Relaxed))
            .sum()
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
