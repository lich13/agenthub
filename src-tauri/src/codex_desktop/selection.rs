//! Native selection grants: the WebUI receives opaque handles, never a path it
//! can modify and ask Rust to read. Grants are bound to the machine revision and
//! (for attachments) the current desktop selection.
use agenthub_core::{codex::desktop::DesktopError, crypto::SecretBox, security::hash_token};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, process::Command, sync::Semaphore, time::timeout};

const MAX_GRANTS: usize = 256;
const GRANT_AGE: Duration = Duration::from_secs(30 * 60);
static CHOOSER: OnceLock<Semaphore> = OnceLock::new();

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

/// A fixed native OS chooser, invoked only by an explicit UI action. There is
/// no script interpolation, Apple Event to Codex, or arbitrary path argument.
pub(super) async fn choose(directory: bool) -> Result<Option<PathBuf>, String> {
    let _permit = CHOOSER
        .get_or_init(|| Semaphore::new(1))
        .try_acquire()
        .map_err(|_| "文件选择窗口已打开")?;
    let script = if directory {
        "POSIX path of (choose folder with prompt \"选择 Codex 工作目录\")"
    } else {
        "POSIX path of (choose file with prompt \"选择要引用的文件\")"
    };
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "无法打开文件选择窗口")?;
    let operation = async {
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or("无法读取选择结果")?
            .take(8193)
            .read_to_end(&mut output)
            .await
            .map_err(|_| "无法读取选择结果")?;
        if output.len() > 8192 {
            return Err("所选路径超出限制");
        }
        let status = child.wait().await.map_err(|_| "无法读取选择结果")?;
        if !status.success() {
            return Ok(None);
        }
        let output = String::from_utf8(output).map_err(|_| "所选路径无法处理")?;
        let path = output.strip_suffix('\n').unwrap_or(&output);
        if path.is_empty()
            || path.chars().any(char::is_control)
            || !std::path::Path::new(path).is_absolute()
        {
            return Err("所选路径无法处理");
        }
        Ok(Some(PathBuf::from(path)))
    };
    timeout(Duration::from_secs(300), operation)
        .await
        .map_err(|_| super::desktop_error(DesktopError::Timeout))?
        .map_err(str::to_owned)
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
