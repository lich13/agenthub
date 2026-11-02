use super::{HistoryJournal, JsonlCache, ReadInfo};
use anyhow::Result;
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(5);
const CACHE_BYTES: u64 = 1024 * 1024;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-jsonl-cache-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("create isolated cache fixture");
        Self { root }
    }

    fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, contents).expect("write synthetic JSONL fixture");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.root).is_err() {
            eprintln!("JSONL_CACHE_FIXTURE_CLEANUP_FAILED");
        }
    }
}

fn append(path: &Path, contents: &[u8]) {
    OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open synthetic fixture for append")
        .write_all(contents)
        .expect("append synthetic JSONL records");
}

fn rewrite(path: &Path, contents: &[u8]) {
    let modified = fs::metadata(path).unwrap().modified().unwrap();
    fs::write(path, contents).expect("rewrite synthetic JSONL fixture");
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified + Duration::from_secs(1))
        .expect("give fixture rewrite a distinct timestamp");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct View {
    ids: Vec<u64>,
    epoch: String,
    revision: String,
    offset: u64,
    lines: usize,
    partial: bool,
    changed: bool,
}

fn view(state: &mut [u64], info: ReadInfo<'_>) -> Result<View> {
    Ok(View {
        ids: state.to_vec(),
        epoch: info.epoch.to_owned(),
        revision: info.revision(),
        offset: info.offset,
        lines: info.lines,
        partial: info.partial,
        changed: info.changed,
    })
}

fn consume(state: &mut Vec<u64>, line: &[u8], count: &AtomicUsize) -> Result<()> {
    #[derive(Deserialize)]
    struct Row {
        id: u64,
    }
    count.fetch_add(1, Ordering::SeqCst);
    state.push(serde_json::from_slice::<Row>(line)?.id);
    Ok(())
}

fn read_weighted(
    cache: &JsonlCache<Vec<u64>>,
    path: &Path,
    count: &AtomicUsize,
    accept_complete_tail: bool,
    weight: u64,
) -> Result<View> {
    cache.read(
        path,
        |_| Ok(0),
        accept_complete_tail,
        |state, line, _, _| consume(state, line, count),
        |state, info| view(state, info),
        |_| weight,
    )
}

fn read(cache: &JsonlCache<Vec<u64>>, path: &Path, count: &AtomicUsize) -> Result<View> {
    read_weighted(cache, path, count, false, 0)
}

#[test]
fn unchanged_file_reuses_snapshot_without_consuming_records() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n{\"id\":2}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);

    let initial = read(&cache, &path, &count).unwrap();
    let warm = read(&cache, &path, &count).unwrap();

    assert_eq!(initial.ids, vec![1, 2]);
    assert!(initial.changed);
    assert!(!warm.changed);
    assert_eq!(warm.ids, initial.ids);
    assert_eq!(warm.epoch, initial.epoch);
    assert_eq!(warm.revision, initial.revision);
    assert_eq!(warm.lines, 2);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn append_consumes_only_new_records_and_preserves_epoch() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n{\"id\":2}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let initial = read(&cache, &path, &count).unwrap();

    append(&path, b"{\"id\":3}\n");
    let updated = read(&cache, &path, &count).unwrap();

    assert_eq!(updated.ids, vec![1, 2, 3]);
    assert_eq!(updated.epoch, initial.epoch);
    assert_ne!(updated.revision, initial.revision);
    assert_eq!(updated.offset, fs::metadata(&path).unwrap().len());
    assert_eq!(updated.lines, 3);
    assert!(!updated.partial);
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn partial_record_waits_for_completion_and_newline() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n{\"id\":");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let initial = read(&cache, &path, &count).unwrap();
    assert_eq!(initial.ids, vec![1]);
    assert_eq!(initial.offset, b"{\"id\":1}\n".len() as u64);
    assert!(initial.partial);

    let unchanged = read(&cache, &path, &count).unwrap();
    assert_eq!(unchanged.revision, initial.revision);
    assert!(!unchanged.changed);
    append(&path, b"2}");
    let waiting = read(&cache, &path, &count).unwrap();
    assert_eq!(waiting.ids, vec![1]);
    assert!(waiting.partial);
    assert_eq!(waiting.revision, initial.revision);
    assert_eq!(count.load(Ordering::SeqCst), 1);

    append(&path, b"\n");
    let completed = read(&cache, &path, &count).unwrap();
    assert_eq!(completed.ids, vec![1, 2]);
    assert_eq!(completed.epoch, initial.epoch);
    assert_ne!(completed.revision, initial.revision);
    assert!(!completed.partial);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn valid_final_json_record_is_accepted_only_when_enabled() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n{\"id\":2}");
    let strict = JsonlCache::new(CACHE_BYTES);
    let permissive = JsonlCache::new(CACHE_BYTES);
    let strict_count = AtomicUsize::new(0);
    let permissive_count = AtomicUsize::new(0);

    let waiting = read_weighted(&strict, &path, &strict_count, false, 0).unwrap();
    let accepted = read_weighted(&permissive, &path, &permissive_count, true, 0).unwrap();
    let warm = read_weighted(&permissive, &path, &permissive_count, true, 0).unwrap();

    assert_eq!(waiting.ids, vec![1]);
    assert!(waiting.partial);
    assert_eq!(strict_count.load(Ordering::SeqCst), 1);
    assert_eq!(accepted.ids, vec![1, 2]);
    assert_eq!(accepted.lines, 2);
    assert_eq!(accepted.offset, fs::metadata(&path).unwrap().len());
    assert!(!accepted.partial);
    assert_eq!(warm.revision, accepted.revision);
    assert!(!warm.changed);
    assert_eq!(permissive_count.load(Ordering::SeqCst), 2);
}

#[test]
fn accepted_tail_followed_by_newline_and_record_preserves_line_numbers() {
    for separator in [b"\n".as_slice(), b"\r\n".as_slice()] {
        let fixture = Fixture::new();
        let contents = b"{\"id\":1}\n{\"id\":2}";
        let path = fixture.file("history.jsonl", contents);
        let cache = JsonlCache::new(CACHE_BYTES);
        let count = AtomicUsize::new(0);
        let initial = read_weighted(&cache, &path, &count, true, 0).unwrap();
        assert_eq!(initial.ids, vec![1, 2]);

        append(&path, separator);
        append(&path, b"{\"id\":3}\n");
        let mut positions = Vec::new();
        let updated = cache
            .read(
                &path,
                |_| Ok(0),
                true,
                |state, line, number, offset| {
                    positions.push((number, offset));
                    consume(state, line, &count)
                },
                |state, info| view(state, info),
                |_| 0,
            )
            .unwrap();

        assert_eq!(updated.ids, vec![1, 2, 3]);
        assert_eq!(updated.lines, 3);
        assert_eq!(updated.epoch, initial.epoch);
        assert_ne!(updated.revision, initial.revision);
        assert_eq!(updated.offset, fs::metadata(&path).unwrap().len());
        assert!(!updated.partial);
        assert_eq!(
            positions,
            vec![(3, (contents.len() + separator.len()) as u64)]
        );
        assert_eq!(count.load(Ordering::SeqCst), 3);

        let warm = read_weighted(&cache, &path, &count, true, 0).unwrap();
        assert_eq!(warm.ids, updated.ids);
        assert_eq!(warm.revision, updated.revision);
        assert!(!warm.changed);
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn extending_accepted_tail_without_newline_reparses_the_whole_record() {
    fn read_numbers(cache: &JsonlCache<Vec<u64>>, path: &Path) -> Result<View> {
        cache.read(
            path,
            |_| Ok(0),
            true,
            |state, line, _, _| {
                state.push(serde_json::from_slice::<u64>(line)?);
                Ok(())
            },
            |state, info| view(state, info),
            |_| 0,
        )
    }

    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"1\n2");
    let cache = JsonlCache::new(CACHE_BYTES);
    let initial = read_numbers(&cache, &path).unwrap();
    assert_eq!(initial.ids, vec![1, 2]);

    append(&path, b"3");
    let updated = read_numbers(&cache, &path).unwrap();
    assert_eq!(updated.ids, vec![1, 23]);
    assert_eq!(updated.lines, 2);
    assert_eq!(updated.offset, fs::metadata(&path).unwrap().len());
    assert_ne!(updated.epoch, initial.epoch);
    assert!(!updated.partial);

    let warm = read_numbers(&cache, &path).unwrap();
    assert_eq!(warm.ids, updated.ids);
    assert_eq!(warm.revision, updated.revision);
    assert!(!warm.changed);
}

#[test]
fn failed_finish_discards_mutated_snapshot_before_unchanged_file_retry() {
    for warm_cache in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.file("history.jsonl", b"{\"id\":1}\n{\"id\":2}\n");
        let cache = JsonlCache::new(CACHE_BYTES);
        let count = AtomicUsize::new(0);
        if warm_cache {
            read(&cache, &path, &count).unwrap();
        }

        let failed = cache.read(
            &path,
            |_| Ok(0),
            false,
            |state, line, _, _| consume(state, line, &count),
            |state, _| {
                state.push(999);
                Err::<View, _>(anyhow::anyhow!("synthetic finish failure"))
            },
            |_| 0,
        );
        assert_eq!(failed.unwrap_err().to_string(), "synthetic finish failure");
        let parsed_before_retry = count.load(Ordering::SeqCst);

        let recovered = read(&cache, &path, &count).unwrap();
        assert_eq!(recovered.ids, vec![1, 2]);
        assert_eq!(recovered.lines, 2);
        assert!(recovered.changed);
        assert_eq!(count.load(Ordering::SeqCst), parsed_before_retry + 2);

        let warm = read(&cache, &path, &count).unwrap();
        assert_eq!(warm.ids, recovered.ids);
        assert_eq!(warm.revision, recovered.revision);
        assert!(!warm.changed);
        assert_eq!(count.load(Ordering::SeqCst), parsed_before_retry + 2);
    }
}

#[test]
fn line_limit_rejects_oversized_records_and_recovers_after_truncation() {
    for terminated in [false, true] {
        for warm_cache in [false, true] {
            let fixture = Fixture::new();
            let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
            let cache = JsonlCache::new(CACHE_BYTES).with_line_limit(32);
            let count = AtomicUsize::new(0);
            if warm_cache {
                read_weighted(&cache, &path, &count, true, 0).unwrap();
            }

            let mut oversized = b"{\"id\":2,\"padding\":\"".to_vec();
            oversized.extend(std::iter::repeat_n(b'x', 4096));
            oversized.extend_from_slice(b"\"}");
            if terminated {
                oversized.push(b'\n');
            }
            append(&path, &oversized);

            let error = read_weighted(&cache, &path, &count, true, 0).unwrap_err();
            assert_eq!(error.to_string(), "会话记录超出读取上限");
            assert_eq!(count.load(Ordering::SeqCst), 1);

            rewrite(&path, b"{\"id\":3}\n");
            let recovered = read_weighted(&cache, &path, &count, true, 0).unwrap();
            assert_eq!(recovered.ids, vec![3]);
            assert_eq!(recovered.lines, 1);
            assert_eq!(recovered.offset, fs::metadata(&path).unwrap().len());
            assert!(!recovered.partial);
            assert_eq!(count.load(Ordering::SeqCst), 2);

            let warm = read_weighted(&cache, &path, &count, true, 0).unwrap();
            assert_eq!(warm.ids, recovered.ids);
            assert_eq!(warm.revision, recovered.revision);
            assert!(!warm.changed);
            assert_eq!(count.load(Ordering::SeqCst), 2);
        }
    }
}

#[test]
fn line_limit_accepts_records_at_the_exact_byte_limit() {
    for contents in [b"{\"id\":1}".as_slice(), b"{\"id\":1}\n".as_slice()] {
        let fixture = Fixture::new();
        let path = fixture.file("history.jsonl", contents);
        let cache = JsonlCache::new(CACHE_BYTES).with_line_limit(contents.len() as u64);
        let count = AtomicUsize::new(0);

        let accepted = read_weighted(&cache, &path, &count, true, 0).unwrap();
        assert_eq!(accepted.ids, vec![1]);
        assert_eq!(accepted.lines, 1);
        assert_eq!(accepted.offset, contents.len() as u64);
        assert!(!accepted.partial);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn permissive_tail_still_waits_for_valid_json() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n{\"id\":");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let initial = read_weighted(&cache, &path, &count, true, 0).unwrap();
    assert_eq!(initial.ids, vec![1]);
    assert!(initial.partial);

    append(&path, b"2}");
    let completed = read_weighted(&cache, &path, &count, true, 0).unwrap();
    assert_eq!(completed.ids, vec![1, 2]);
    assert_eq!(completed.epoch, initial.epoch);
    assert!(!completed.partial);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn replacement_starts_new_epoch_even_with_same_size_and_timestamp() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let initial = read(&cache, &path, &count).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let replacement = fixture.file("replacement.jsonl", b"{\"id\":9}\n");
    File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    fs::rename(&replacement, &path).expect("replace synthetic history");

    let updated = read(&cache, &path, &count).unwrap();
    assert_eq!(updated.ids, vec![9]);
    assert_ne!(updated.epoch, initial.epoch);
    assert_ne!(updated.revision, initial.revision);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[cfg(unix)]
#[test]
fn cached_file_replaced_with_symlink_cannot_return_its_snapshot_or_target() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
    let target = fixture.file("other-history.jsonl", b"{\"id\":9}\n");
    let original = fixture.root.join("original.jsonl");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    read(&cache, &path, &count).unwrap();

    fs::rename(&path, &original).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let finish_count = AtomicUsize::new(0);
    let result = cache.read(
        &path,
        |_| Ok(0),
        false,
        |state, line, _, _| consume(state, line, &count),
        |state, info| {
            finish_count.fetch_add(1, Ordering::SeqCst);
            view(state, info)
        },
        |_| 0,
    );

    assert!(result.is_err());
    assert_eq!(finish_count.load(Ordering::SeqCst), 0);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    fs::remove_file(&path).unwrap();
    fs::rename(&original, &path).unwrap();
    assert_eq!(read(&cache, &path, &count).unwrap().ids, vec![1]);
}

#[cfg(unix)]
#[test]
fn source_replaced_during_parse_is_not_published_and_retry_uses_new_identity() {
    for warm_cache in [false, true] {
        for replace_with_symlink in [false, true] {
            let fixture = Fixture::new();
            let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
            let cache = JsonlCache::new(CACHE_BYTES);
            let count = AtomicUsize::new(0);
            let initial = warm_cache.then(|| read(&cache, &path, &count).unwrap());
            append(&path, b"{\"id\":2}\n");
            let replacement = fixture.file("replacement.jsonl", b"{\"id\":7}\n{\"id\":8}\n");
            File::options()
                .write(true)
                .open(&replacement)
                .unwrap()
                .set_modified(fs::metadata(&path).unwrap().modified().unwrap())
                .unwrap();
            let mut replaced = false;
            let finish_count = AtomicUsize::new(0);

            let result = cache.read(
                &path,
                |_| Ok(0),
                false,
                |state, line, _, _| {
                    consume(state, line, &count)?;
                    if !replaced {
                        if replace_with_symlink {
                            fs::rename(&path, fixture.root.join("original.jsonl"))?;
                            std::os::unix::fs::symlink(&replacement, &path)?;
                        } else {
                            fs::rename(&replacement, &path)?;
                        }
                        replaced = true;
                    }
                    Ok(())
                },
                |state, info| {
                    finish_count.fetch_add(1, Ordering::SeqCst);
                    view(state, info)
                },
                |_| 0,
            );

            assert!(replaced);
            assert!(result.is_err());
            assert_eq!(finish_count.load(Ordering::SeqCst), 0);
            if replace_with_symlink {
                assert!(read(&cache, &path, &count).is_err());
                fs::remove_file(&path).unwrap();
                fs::rename(&replacement, &path).unwrap();
            }
            let recovered = read(&cache, &path, &count).unwrap();
            assert_eq!(recovered.ids, vec![7, 8]);
            assert_eq!(recovered.lines, 2);
            assert!(!recovered.partial);
            if let Some(initial) = initial {
                assert_ne!(recovered.epoch, initial.epoch);
            }
            let warm = read(&cache, &path, &count).unwrap();
            assert_eq!(warm.revision, recovered.revision);
            assert_eq!(warm.ids, recovered.ids);
            assert!(!warm.changed);
        }
    }
}

#[test]
fn truncation_discards_cached_records_and_starts_new_epoch() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n{\"id\":2}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let initial = read(&cache, &path, &count).unwrap();
    rewrite(&path, b"{\"id\":3}\n");

    let updated = read(&cache, &path, &count).unwrap();
    assert_eq!(updated.ids, vec![3]);
    assert_ne!(updated.epoch, initial.epoch);
    assert_eq!(updated.lines, 1);
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn same_size_rewrite_discards_cached_records_and_starts_new_epoch() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let initial = read(&cache, &path, &count).unwrap();
    rewrite(&path, b"{\"id\":9}\n");

    let updated = read(&cache, &path, &count).unwrap();
    assert_eq!(updated.ids, vec![9]);
    assert_eq!(updated.offset, initial.offset);
    assert_ne!(updated.epoch, initial.epoch);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn same_file_concurrent_reads_consume_each_record_once() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    thread::scope(|scope| {
        let cache_ref = &cache;
        let path_ref = &path;
        let count_ref = &count;
        let leader = scope.spawn(move || {
            cache_ref.read(
                path_ref,
                |_| Ok(0),
                false,
                |state, line, _, _| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(WAIT).expect("release first reader");
                    consume(state, line, count_ref)
                },
                |state, info| view(state, info),
                |_| 0,
            )
        });
        entered_rx
            .recv_timeout(WAIT)
            .expect("first reader entered consume");
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let followers: Vec<_> = (0..4)
            .map(|_| {
                let started = started_tx.clone();
                let finished = finished_tx.clone();
                let cache = &cache;
                let path = &path;
                let count = &count;
                scope.spawn(move || {
                    started.send(()).unwrap();
                    let result = read(cache, path, count);
                    finished.send(()).unwrap();
                    result
                })
            })
            .collect();
        for _ in 0..followers.len() {
            started_rx.recv_timeout(WAIT).expect("follower started");
        }
        let finished_before_release = finished_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        release_tx.send(()).unwrap();
        let first = leader.join().expect("first reader thread").unwrap();
        for follower in followers {
            let result = follower.join().expect("follower thread").unwrap();
            assert_eq!(result.ids, vec![1]);
            assert_eq!(result.epoch, first.epoch);
            assert_eq!(result.revision, first.revision);
            assert!(!result.changed);
        }
        assert!(
            !finished_before_release,
            "same-file reader bypassed active parse"
        );
    });
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn blocked_reader_does_not_block_other_files() {
    let fixture = Fixture::new();
    let slow_path = fixture.file("slow.jsonl", b"{\"id\":1}\n");
    let fast_path = fixture.file("fast.jsonl", b"{\"id\":2}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    thread::scope(|scope| {
        let cache_ref = &cache;
        let slow_path_ref = &slow_path;
        let count_ref = &count;
        let slow = scope.spawn(move || {
            cache_ref.read(
                slow_path_ref,
                |_| Ok(0),
                false,
                |state, line, _, _| {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .recv_timeout(WAIT * 2)
                        .expect("release blocked reader");
                    consume(state, line, count_ref)
                },
                |state, info| view(state, info),
                |_| 0,
            )
        });
        entered_rx
            .recv_timeout(WAIT)
            .expect("slow reader entered consume");
        let (finished_tx, finished_rx) = mpsc::channel();
        let fast_path_ref = &fast_path;
        let fast = scope.spawn(move || {
            let result = read(cache_ref, fast_path_ref, count_ref);
            finished_tx.send(result).unwrap();
        });
        let completed_while_blocked = finished_rx.recv_timeout(WAIT);
        release_tx.send(()).unwrap();
        let slow_result = slow.join().expect("slow reader thread").unwrap();
        fast.join().expect("fast reader thread");
        let fast_result = completed_while_blocked
            .expect("another file must finish while the first reader is blocked")
            .unwrap();
        assert_eq!(slow_result.ids, vec![1]);
        assert_eq!(fast_result.ids, vec![2]);
    });
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn entry_limit_evicts_least_recently_read_file() {
    let fixture = Fixture::new();
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let mut paths = Vec::new();
    let mut views = Vec::new();
    for index in 0..64 {
        let path = fixture.file(&format!("history-{index}.jsonl"), b"{\"id\":1}\n");
        views.push(read(&cache, &path, &count).unwrap());
        paths.push(path);
    }
    assert_eq!(
        read(&cache, &paths[0], &count).unwrap().epoch,
        views[0].epoch
    );
    let newest = fixture.file("history-newest.jsonl", b"{\"id\":2}\n");
    read(&cache, &newest, &count).unwrap();

    assert_eq!(
        read(&cache, &paths[0], &count).unwrap().epoch,
        views[0].epoch
    );
    assert_eq!(count.load(Ordering::SeqCst), 65);
    let rebuilt = read(&cache, &paths[1], &count).unwrap();
    assert_ne!(rebuilt.epoch, views[1].epoch);
    assert_eq!(rebuilt.ids, vec![1]);
    assert_eq!(count.load(Ordering::SeqCst), 66);
}

#[test]
fn byte_budget_evicts_cached_state_before_entry_limit() {
    let fixture = Fixture::new();
    let first_path = fixture.file("first.jsonl", b"{\"id\":1}\n");
    let second_path = fixture.file("second.jsonl", b"{\"id\":2}\n");
    let cache = JsonlCache::new(5 * 1024);
    let count = AtomicUsize::new(0);
    let first = read_weighted(&cache, &first_path, &count, false, 4 * 1024).unwrap();
    let second = read_weighted(&cache, &second_path, &count, false, 4 * 1024).unwrap();

    let retained = read_weighted(&cache, &second_path, &count, false, 4 * 1024).unwrap();
    assert_eq!(retained.epoch, second.epoch);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let rebuilt = read_weighted(&cache, &first_path, &count, false, 4 * 1024).unwrap();
    assert_ne!(rebuilt.epoch, first.epoch);
    assert_eq!(rebuilt.ids, vec![1]);
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn byte_budget_preserves_in_use_snapshot_and_evicts_it_after_release() {
    let fixture = Fixture::new();
    let active_path = fixture.file("active.jsonl", b"{\"id\":1}\n");
    let other_path = fixture.file("other.jsonl", b"{\"id\":2}\n");
    let budget = 5 * 1024;
    let weight = 4 * 1024;
    let cache = JsonlCache::new(budget);
    let count = AtomicUsize::new(0);
    let initial = read_weighted(&cache, &active_path, &count, false, weight).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    thread::scope(|scope| {
        let cache_ref = &cache;
        let path_ref = &active_path;
        let count_ref = &count;
        let active = scope.spawn(move || {
            cache_ref.read(
                path_ref,
                |_| Ok(0),
                false,
                |state, line, _, _| consume(state, line, count_ref),
                |state, info| {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .recv_timeout(WAIT)
                        .expect("release active snapshot");
                    view(state, info)
                },
                |_| weight,
            )
        });
        entered_rx.recv_timeout(WAIT).expect("snapshot is in use");
        let other = read_weighted(&cache, &other_path, &count, false, weight);
        let retained_while_active = {
            let entries = cache.entries.lock().unwrap();
            (
                entries.iter().any(|entry| entry.path == active_path),
                entries
                    .iter()
                    .map(|entry| entry.weight.load(Ordering::Relaxed))
                    .sum::<u64>(),
            )
        };
        release_tx.send(()).unwrap();
        let active = active.join().expect("active reader thread").unwrap();

        assert_eq!(other.unwrap().ids, vec![2]);
        assert!(retained_while_active.0);
        // This is a retention budget: in-use snapshots may exceed it temporarily.
        assert!(retained_while_active.1 > budget);
        assert_eq!(active.epoch, initial.epoch);
        assert_eq!(active.ids, initial.ids);
        assert!(!active.changed);
    });

    let retained = read_weighted(&cache, &active_path, &count, false, weight).unwrap();
    assert_eq!(retained.epoch, initial.epoch);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    read_weighted(&cache, &other_path, &count, false, weight).unwrap();
    let rebuilt = read_weighted(&cache, &active_path, &count, false, weight).unwrap();
    assert_eq!(rebuilt.ids, initial.ids);
    assert_ne!(rebuilt.epoch, initial.epoch);
    assert_eq!(count.load(Ordering::SeqCst), 4);
}

#[test]
fn oversize_snapshot_is_returned_but_not_retained() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
    let cache = JsonlCache::new(1024);
    let count = AtomicUsize::new(0);
    let first = read_weighted(&cache, &path, &count, false, 8 * 1024).unwrap();
    let rebuilt = read_weighted(&cache, &path, &count, false, 8 * 1024).unwrap();

    assert_eq!(first.ids, vec![1]);
    assert_eq!(rebuilt.ids, first.ids);
    assert_ne!(rebuilt.epoch, first.epoch);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn idle_eviction_releases_stale_snapshots_and_retains_recent_entries() {
    let fixture = Fixture::new();
    let path = fixture.file("history.jsonl", b"{\"id\":1}\n");
    let cache = JsonlCache::new(CACHE_BYTES);
    let count = AtomicUsize::new(0);
    let first = read_weighted(&cache, &path, &count, false, 64).unwrap();
    cache.evict_idle(std::time::Duration::from_secs(900));
    let retained = read_weighted(&cache, &path, &count, false, 64).unwrap();
    assert_eq!(retained.epoch, first.epoch);
    assert_eq!(count.load(Ordering::SeqCst), 1);

    cache.evict_idle(std::time::Duration::ZERO);
    let rebuilt = read_weighted(&cache, &path, &count, false, 64).unwrap();
    assert_ne!(rebuilt.epoch, first.epoch);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

fn record(journal: &mut HistoryJournal, epoch: &str, offset: u64, ids: Vec<String>) -> String {
    let info = ReadInfo {
        epoch,
        offset,
        lines: offset as usize,
        partial: false,
        changed: true,
    };
    journal.record(&info, ids);
    info.revision()
}

#[test]
fn journal_returns_only_changes_after_requested_revision_without_duplicates() {
    let mut journal = HistoryJournal::default();
    let initial = record(&mut journal, "fixture-epoch", 1, vec!["initial".into()]);
    record(
        &mut journal,
        "fixture-epoch",
        2,
        vec!["b".into(), "a".into()],
    );
    let latest = record(
        &mut journal,
        "fixture-epoch",
        3,
        vec!["b".into(), "c".into()],
    );
    record(&mut journal, "fixture-epoch", 3, vec!["ignored".into()]);

    let changes = journal.metadata(Some(&initial));
    assert_eq!(changes.epoch, "fixture-epoch");
    assert_eq!(changes.revision, latest);
    assert_eq!(changes.changed_ids, vec!["a", "b", "c"]);
    assert!(!changes.reset);
    for since in [None, Some(latest.as_str())] {
        let unchanged = journal.metadata(since);
        assert_eq!(unchanged.revision, latest);
        assert!(unchanged.changed_ids.is_empty());
        assert!(!unchanged.reset);
    }
}

#[test]
fn expired_or_unknown_journal_revision_requests_safe_reset() {
    let mut journal = HistoryJournal::default();
    let expired = record(&mut journal, "fixture-epoch", 1, vec!["old".into()]);
    for offset in 2..128 {
        record(
            &mut journal,
            "fixture-epoch",
            offset,
            vec![format!("row-{offset}")],
        );
    }
    let recent = journal.metadata(None).revision;
    let latest = record(&mut journal, "fixture-epoch", 128, vec!["newest".into()]);

    for since in [expired.as_str(), "unknown-revision"] {
        let metadata = journal.metadata(Some(since));
        assert!(metadata.reset);
        assert!(metadata.changed_ids.is_empty());
        assert_eq!(metadata.revision, latest);
    }
    let available = journal.metadata(Some(&recent));
    assert!(!available.reset);
    assert_eq!(available.changed_ids, vec!["newest"]);
}

#[test]
fn changed_epoch_invalidates_old_journal_revision() {
    let mut journal = HistoryJournal::default();
    let old = record(&mut journal, "fixture-before", 1, vec!["old".into()]);
    let current = record(&mut journal, "fixture-after", 1, vec!["replacement".into()]);

    let metadata = journal.metadata(Some(&old));
    assert_eq!(metadata.epoch, "fixture-after");
    assert_eq!(metadata.revision, current);
    assert!(metadata.reset);
    assert!(metadata.changed_ids.is_empty());
    let unchanged = journal.metadata(Some(&current));
    assert!(!unchanged.reset);
    assert!(unchanged.changed_ids.is_empty());
}

#[test]
fn excessive_changed_ids_request_reset_instead_of_partial_changes() {
    let mut journal = HistoryJournal::default();
    let initial = record(&mut journal, "fixture-epoch", 1, vec![]);
    record(
        &mut journal,
        "fixture-epoch",
        2,
        (0..5000).map(|index| format!("row-{index}")).collect(),
    );

    let metadata = journal.metadata(Some(&initial));
    assert!(metadata.reset);
    assert!(metadata.changed_ids.is_empty());
}
