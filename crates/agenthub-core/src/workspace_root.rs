//! Lightweight single-file `.git` worktree root resolver (`gitdir: .../worktrees/<name>`).
//! Never spawns `git` subprocesses and stays safe under `ProtectHome=read-only`.

use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

type CacheEntry = (Option<String>, Instant);

static WORKSPACE_ROOT_CACHE: Mutex<Option<HashMap<String, CacheEntry>>> = Mutex::new(None);

const CACHE_TTL: Duration = Duration::from_secs(30);
const MAX_CACHE_ENTRIES: usize = 2_048;

pub(crate) fn evict_idle_cache(max_idle: Duration) {
    if let Ok(mut guard) = WORKSPACE_ROOT_CACHE.lock() {
        if let Some(map) = guard.as_mut() {
            let before = map.len();
            let now = Instant::now();
            map.retain(|_, (_, ts)| now.saturating_duration_since(*ts) < max_idle);
            let evicted = map.len() < before;
            if map.is_empty() {
                *guard = None;
            }
            if evicted {
                crate::note_cache_eviction();
            }
        }
    }
}

/// When `cwd` resides inside a Git worktree whose `.git` file points to
/// `<main_repo>/.git/worktrees/<name>` (or `commondir`), returns `<main_repo>`.
/// Returns `None` when `cwd` is empty, relative, or already a normal directory/repo root.
pub(crate) fn resolve_workspace_root(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || !Path::new(trimmed).is_absolute() {
        return None;
    }
    if let Ok(guard) = WORKSPACE_ROOT_CACHE.lock() {
        if let Some((cached, ts)) = guard.as_ref().and_then(|map| map.get(trimmed)) {
            if ts.elapsed() < CACHE_TTL {
                return cached.clone();
            }
        }
    }
    let resolved = resolve_workspace_root_uncached(Path::new(trimmed));
    if let Ok(mut guard) = WORKSPACE_ROOT_CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() >= MAX_CACHE_ENTRIES && !map.contains_key(trimmed) {
            map.clear();
        }
        map.insert(trimmed.to_owned(), (resolved.clone(), Instant::now()));
    }
    resolved
}

fn resolve_workspace_root_uncached(cwd: &Path) -> Option<String> {
    let normalized_cwd = normalize_lexical(cwd);
    for dir in normalized_cwd.ancestors().take(8) {
        if dir.parent().is_none() {
            break;
        }
        let dot_git = dir.join(".git");
        let Ok(meta) = fs::symlink_metadata(&dot_git) else {
            continue;
        };
        if meta.is_dir() {
            if dir.is_absolute() && dir != normalized_cwd && dir.parent().is_some() {
                return dir.to_str().map(str::to_owned);
            }
            return None;
        }
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 4_096 {
            return None;
        }
        let gitdir = read_gitdir_pointer(&dot_git, dir)?;
        let main_root = main_repo_from_worktree_gitdir(&gitdir)?;
        if main_root.is_absolute() && main_root != normalized_cwd && main_root.parent().is_some() {
            return main_root.to_str().map(str::to_owned);
        }
        return None;
    }
    None
}

fn read_gitdir_pointer(dot_git_file: &Path, base_dir: &Path) -> Option<PathBuf> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut buf = Vec::new();
    options
        .open(dot_git_file)
        .ok()?
        .take(4_096)
        .read_to_end(&mut buf)
        .ok()?;
    let text = std::str::from_utf8(&buf).ok()?;
    let first_line = text.lines().next()?.trim();
    let raw_target = first_line.strip_prefix("gitdir:")?.trim();
    if raw_target.is_empty() {
        return None;
    }
    let target = Path::new(raw_target);
    Some(if target.is_absolute() {
        normalize_lexical(target)
    } else {
        normalize_lexical(&base_dir.join(target))
    })
}

fn main_repo_from_worktree_gitdir(gitdir: &Path) -> Option<PathBuf> {
    let worktrees_dir = gitdir.parent()?;
    if worktrees_dir.file_name().and_then(|n| n.to_str()) != Some("worktrees") {
        return None;
    }
    let common_git = worktrees_dir.parent()?;
    if common_git.file_name().and_then(|n| n.to_str()) == Some(".git") {
        return common_git.parent().map(normalize_lexical);
    }
    let commondir_file = gitdir.join("commondir");
    if let Ok(meta) = fs::symlink_metadata(&commondir_file) {
        if meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 4_096 {
            let mut raw = String::new();
            fs::File::open(&commondir_file)
                .ok()?
                .take(4_096)
                .read_to_string(&mut raw)
                .ok()?;
            let rel = raw.lines().next()?.trim();
            if !rel.is_empty() {
                let resolved_common = if Path::new(rel).is_absolute() {
                    normalize_lexical(Path::new(rel))
                } else {
                    normalize_lexical(&gitdir.join(rel))
                };
                if resolved_common.file_name().and_then(|n| n.to_str()) == Some(".git") {
                    return resolved_common.parent().map(normalize_lexical);
                }
            }
        }
    }
    None
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir) | None
                ) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_absolute_and_relative_git_worktree_roots_and_ignores_submodules() {
        let base = std::env::temp_dir().join(format!(
            "agenthub-worktree-root-test-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        let main_repo = base.join("repos/AgentHub");
        let worktree_a = base.join("orca/worktrees/feat-alpha");
        let worktree_b = main_repo.join(".claude/worktrees/fix-beta");
        let nested = worktree_a.join("crates/agenthub-core");
        let submodule = main_repo.join("vendor/sub");
        fs::create_dir_all(main_repo.join(".git/worktrees/feat-alpha")).unwrap();
        fs::create_dir_all(main_repo.join(".git/worktrees/fix-beta")).unwrap();
        fs::create_dir_all(main_repo.join(".git/modules/sub")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&worktree_b).unwrap();
        fs::create_dir_all(&submodule).unwrap();

        fs::write(
            worktree_a.join(".git"),
            format!(
                "gitdir: {}\n",
                main_repo.join(".git/worktrees/feat-alpha").display()
            ),
        )
        .unwrap();
        fs::write(
            worktree_b.join(".git"),
            "gitdir: ../../../.git/worktrees/fix-beta\n",
        )
        .unwrap();
        fs::write(submodule.join(".git"), "gitdir: ../../.git/modules/sub\n").unwrap();

        let main_subdir = main_repo.join("macos/Sources");
        fs::create_dir_all(&main_subdir).unwrap();

        let expected = main_repo.to_string_lossy().into_owned();
        assert_eq!(
            resolve_workspace_root(worktree_a.to_str().unwrap()).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(
            resolve_workspace_root(nested.to_str().unwrap()).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(
            resolve_workspace_root(worktree_b.to_str().unwrap()).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(
            resolve_workspace_root(main_subdir.to_str().unwrap()).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(resolve_workspace_root(main_repo.to_str().unwrap()), None);
        assert_eq!(resolve_workspace_root(submodule.to_str().unwrap()), None);
        assert_eq!(resolve_workspace_root(""), None);

        let _ = fs::remove_dir_all(&base);
    }
}
