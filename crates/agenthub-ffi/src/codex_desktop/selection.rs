//! Native selection grants: Swift receives opaque handles, never a path it
//! can modify and ask Rust to read. Grants are bound to the machine revision and
//! (for attachments) the current desktop selection.
use crate::host::HostBridge;
use agenthub_core::{crypto::SecretBox, security::hash_token};
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

const MAX_GRANTS: usize = 256;
const GRANT_AGE: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, PartialEq, Eq)]
pub(super) enum GrantValue {
    Project {
        path: PathBuf,
        device: u64,
        inode: u64,
    },
    Reference(agenthub_core::codex::desktop::references::NativeReference),
}

struct Grant {
    revision: u64,
    scope: Option<String>,
    value: GrantValue,
    created: Instant,
}

#[derive(Default)]
pub(super) struct Grants {
    entries: HashMap<String, Grant>,
}

impl Grants {
    pub fn insert(&mut self, revision: u64, scope: Option<String>, value: GrantValue) -> String {
        self.entries
            .retain(|_, entry| entry.created.elapsed() < GRANT_AGE && entry.revision == revision);
        if let Some((handle, _)) = self
            .entries
            .iter()
            .find(|(_, entry)| entry.scope == scope && entry.value == value)
        {
            return handle.clone();
        }
        if self.entries.len() >= MAX_GRANTS {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.created)
                .map(|(key, _)| key.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        let handle = hash_token(&SecretBox::random_key_material());
        self.entries.insert(
            handle.clone(),
            Grant {
                revision,
                scope,
                value,
                created: Instant::now(),
            },
        );
        handle
    }

    pub fn resolve(
        &self,
        revision: u64,
        scope: Option<&str>,
        handle: &str,
    ) -> Result<GrantValue, String> {
        self.entries
            .get(handle)
            .filter(|entry| {
                entry.revision == revision
                    && entry.scope.as_deref() == scope
                    && entry.created.elapsed() < GRANT_AGE
            })
            .map(|entry| entry.value.clone())
            .ok_or_else(|| "所选项目或引用已失效，请重新选择".into())
    }
}

pub(super) fn project(path: PathBuf) -> Result<GrantValue, String> {
    use std::os::unix::fs::MetadataExt;
    let path = path.canonicalize().map_err(|_| "无法读取所选目录")?;
    let metadata = path.metadata().map_err(|_| "无法读取所选目录")?;
    if !metadata.is_dir() {
        return Err("请选择目录".into());
    }
    Ok(GrantValue::Project {
        path,
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(super) fn project_path(value: GrantValue) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    if let GrantValue::Project {
        path,
        device,
        inode,
    } = value
    {
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| "所选目录已不可用")?;
        if metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.dev() == device
            && metadata.ino() == inode
            && path.canonicalize().ok().as_ref() == Some(&path)
        {
            return Ok(path);
        }
    }
    Err("所选目录已变化，请重新选择".into())
}

pub(super) async fn choose(host: &HostBridge, directory: bool) -> Result<Option<PathBuf>, String> {
    let prompt = if directory {
        "选择 Codex 工作目录"
    } else {
        "选择要引用的文件"
    };
    host.choose_local_path(directory, prompt).await
}

#[cfg(test)]
#[path = "../../../../src-tauri/src/codex_desktop/selection_tests.rs"]
mod tests;
