//! Native rollout filenames and their physical representations.
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RolloutName {
    pub thread_id: String,
    pub rollout_id: String,
}

impl RolloutName {
    pub fn parse(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let name = name.strip_suffix(".zst").unwrap_or(name);
        let core = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
        jiff::civil::DateTime::strptime("%Y-%m-%dT%H-%M-%S", core.get(..19)?).ok()?;
        if core.get(19..20)? != "-" {
            return None;
        }
        let ids = core.get(20..)?;
        let (thread, rollout) = ids.split_once('_').unwrap_or((ids, ids));
        uuid::Uuid::parse_str(thread).ok()?;
        uuid::Uuid::parse_str(rollout).ok()?;
        Some(Self {
            thread_id: thread.into(),
            rollout_id: rollout.into(),
        })
    }
}

pub fn is_compressed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
}

pub(crate) fn plain_path(path: &Path) -> PathBuf {
    if is_compressed(path) {
        path.with_extension("")
    } else {
        path.into()
    }
}

pub(crate) fn compressed_path(path: &Path) -> PathBuf {
    plain_path(path).with_extension("jsonl.zst")
}

/// Decode the native representation in memory without writing a second history.
pub(crate) fn open_reader(path: &Path) -> anyhow::Result<Box<dyn std::io::Read>> {
    let file = crate::jsonl_cache::open_regular(path)?;
    if is_compressed(path) {
        Ok(Box::new(zstd::stream::read::Decoder::new(file)?))
    } else {
        Ok(Box::new(file))
    }
}

/// Keep ordinary and compressed rollout snapshots in the same bounded pool.
pub(crate) fn read_cached<S: Default, R>(
    cache: &crate::jsonl_cache::JsonlCache<S>,
    path: &Path,
    accept_complete_tail: bool,
    consume: impl FnMut(&mut S, &[u8], usize, u64) -> anyhow::Result<()>,
    finish: impl FnOnce(&mut S, crate::jsonl_cache::ReadInfo<'_>) -> anyhow::Result<R>,
    weight: impl FnOnce(&S) -> u64,
) -> anyhow::Result<R> {
    if is_compressed(path) {
        cache.read_compressed(path, accept_complete_tail, consume, finish, weight)
    } else {
        cache.read(
            path,
            |_| Ok(0),
            accept_complete_tail,
            consume,
            finish,
            weight,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_names_distinguish_thread_and_rollout_without_merging_other_paths() {
        let thread = "11111111-1111-4111-8111-111111111111";
        let rollout = "22222222-2222-4222-8222-222222222222";
        let name = format!("rollout-2026-01-01T00-00-00-{thread}_{rollout}.jsonl.zst");
        let parsed = RolloutName::parse(Path::new(&name)).unwrap();
        assert_eq!(parsed.thread_id, thread);
        assert_eq!(parsed.rollout_id, rollout);
        assert_eq!(
            compressed_path(&plain_path(Path::new(&name))),
            Path::new(&name)
        );
        assert!(RolloutName::parse(Path::new(&name.replace("2026-01-01", "invalid"))).is_none());
    }
}
