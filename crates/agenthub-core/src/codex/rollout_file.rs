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
        chrono::NaiveDateTime::parse_from_str(core.get(..19)?, "%Y-%m-%dT%H-%M-%S").ok()?;
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

pub(crate) fn is_compressed(path: &Path) -> bool {
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
