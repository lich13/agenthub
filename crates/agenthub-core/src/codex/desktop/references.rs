//! References selected by native Rust controls/catalogs. This is an encrypted
//! internal queue format, never a request DTO and never an arbitrary UI path.
use super::{DesktopError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    io::{Read, Seek},
    path::{Path, PathBuf},
};

/// Encrypted queue metadata uses a resolved reference index, not an expiring
/// UI grant. Ranges remain relative to the user's text until wire construction.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceSpan {
    pub reference: usize,
    pub start: usize,
    pub end: usize,
}

pub(super) fn resolve_spans(
    handles: &[String],
    spans: &[crate::services::codex_desktop::DesktopReferenceSpan],
) -> Result<Vec<ReferenceSpan>> {
    if spans.len() > 32 {
        return Err(DesktopError::InvalidInput);
    }
    spans
        .iter()
        .map(|span| {
            Ok(ReferenceSpan {
                reference: handles
                    .iter()
                    .position(|handle| handle == &span.handle)
                    .ok_or(DesktopError::InvalidInput)?,
                start: span.start,
                end: span.end,
            })
        })
        .collect()
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified: i64,
    modified_ns: i64,
    changed: i64,
    changed_ns: i64,
    // Nested in the existing identity: old readers ignore this optional field,
    // and old queue entries serialize exactly as before. Only native file
    // selection may establish a proof; validation never blesses new content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_proof: Option<FileContentProof>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FileContentProof {
    sha256: String,
    uid: u32,
    gid: u32,
    mode: u32,
}

type ReferenceResult<T> = std::result::Result<T, (DesktopError, &'static str)>;
const UNREADABLE: &str = "无法读取所选文件，请重新选择";
const CHANGED: &str = "所选文件内容或身份已变化，请重新选择";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeReference {
    File {
        name: String,
        path: PathBuf,
        identity: FileIdentity,
        image: bool,
    },
    Skill {
        name: String,
        path: PathBuf,
        identity: Option<FileIdentity>,
    },
    App {
        name: String,
        path: String,
    },
    Plugin {
        name: String,
        path: String,
    },
}

impl NativeReference {
    pub fn selected_file(path: PathBuf) -> Result<Self> {
        Self::selected_file_detailed(path).map_err(|(error, _)| error)
    }

    pub fn selected_file_detailed(path: PathBuf) -> ReferenceResult<Self> {
        let path = path
            .canonicalize()
            .map_err(|_| (DesktopError::InvalidInput, "无法读取所选文件，请重新选择"))?;
        safe_path(&path).map_err(|error| (error, "文件名或路径格式无效"))?;
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or((DesktopError::InvalidInput, "文件名或路径格式无效"))?
            .to_owned();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|_| (DesktopError::InvalidInput, "无法读取所选文件，请重新选择"))?;
        let metadata = file
            .metadata()
            .map_err(|_| (DesktopError::InvalidInput, UNREADABLE))?;
        let mut identity = FileIdentity::read(&metadata)
            .map_err(|error| (error, "仅支持普通文件，不能添加目录"))?;
        let mut header = [0_u8; 16];
        let read = file
            .read(&mut header)
            .map_err(|_| (DesktopError::InvalidInput, "无法读取所选文件，请重新选择"))?;
        let header = &header[..read];
        let image = header.starts_with(b"\x89PNG\r\n\x1a\n")
            || header.starts_with(b"\xff\xd8\xff")
            || header.starts_with(b"GIF87a")
            || header.starts_with(b"GIF89a")
            || (header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP"));
        if image && identity.size > super::attachments::MAX_IMAGE_BYTES as u64 {
            return Err((
                DesktopError::InvalidInput,
                "图片超过 20MiB，请选择较小的图片",
            ));
        }
        // Every supported image fits this budget; small ordinary files benefit
        // too. Large files retain the original metadata-only strict policy.
        if identity.size <= super::attachments::MAX_IMAGE_BYTES as u64 {
            let sha256 = content_digest(&mut file, identity.size)?;
            let after = file
                .metadata()
                .map_err(|_| (DesktopError::RequestExpired, UNREADABLE))?;
            if FileIdentity::read(&after).map_err(|e| (e, CHANGED))? != identity
                || metadata.uid() != after.uid()
                || metadata.gid() != after.gid()
                || metadata.mode() != after.mode()
            {
                return Err((DesktopError::RequestExpired, CHANGED));
            }
            identity.content_proof = Some(FileContentProof {
                sha256,
                uid: metadata.uid(),
                gid: metadata.gid(),
                mode: metadata.mode(),
            });
        }
        let reference = Self::File {
            name,
            path,
            identity,
            image,
        };
        reference.validate_detailed()?;
        Ok(reference)
    }

    pub fn skill(name: String, path: PathBuf) -> Result<Self> {
        safe_name(&name)?;
        safe_path(&path)?;
        Ok(Self::Skill {
            name,
            path,
            identity: None,
        })
    }

    pub fn app(name: String, path: String) -> Result<Self> {
        safe_name(&name)?;
        reference_uri(&path, "app://")?;
        Ok(Self::App { name, path })
    }

    pub fn plugin(name: String, marketplace: &str) -> Result<Self> {
        safe_name(&name)?;
        safe_name(marketplace)?;
        if name.contains('@') || marketplace.contains('@') {
            return Err(DesktopError::InvalidInput);
        }
        let path = format!("plugin://{name}@{marketplace}");
        reference_uri(&path, "plugin://")?;
        Ok(Self::Plugin { name, path })
    }

    /// Catalog references are stamped only after the user selects them. Already
    /// selected files keep their original identity; capture cannot bless a swap.
    pub fn capture(self) -> Result<Self> {
        self.capture_detailed().map_err(|(error, _)| error)
    }

    pub fn capture_detailed(mut self) -> ReferenceResult<Self> {
        if let Self::Skill { path, identity, .. } = &mut self {
            if identity.is_none() {
                *path = path
                    .canonicalize()
                    .map_err(|_| (DesktopError::InvalidInput, "无法读取所选技能，请重新选择"))?;
                *identity = Some(
                    file_identity(path).map_err(|error| (error, "所选技能已失效，请重新选择"))?,
                );
            }
        }
        self.validate_detailed()?;
        Ok(self)
    }

    pub fn validate_detailed(&self) -> ReferenceResult<()> {
        if let Self::File {
            name,
            path,
            identity,
            ..
        } = self
        {
            safe_name(name).map_err(|error| (error, "文件名或路径格式无效"))?;
            return verify_file_detailed(path, identity);
        }
        self.validate()
            .map_err(|error| (error, "所选技能或应用引用已失效，请重新选择"))
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::File {
                name,
                path,
                identity,
                ..
            } => {
                safe_name(name)?;
                verify_file(path, identity)
            }
            Self::Skill {
                name,
                path,
                identity,
            } => {
                safe_name(name)?;
                verify_file(path, identity.as_ref().ok_or(DesktopError::InvalidInput)?)
            }
            Self::App { name, path } => {
                safe_name(name)?;
                reference_uri(path, "app://")
            }
            Self::Plugin { name, path } => {
                safe_name(name)?;
                reference_uri(path, "plugin://")?;
                if path
                    .strip_prefix("plugin://")
                    .and_then(|path| path.split_once('@'))
                    .is_none_or(|(plugin, market)| {
                        plugin.is_empty() || market.is_empty() || market.contains('@')
                    })
                {
                    return Err(DesktopError::InvalidInput);
                }
                Ok(())
            }
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::File { image: true, .. } => "image",
            Self::File { .. } => "file",
            Self::Skill { .. } => "skill",
            Self::App { .. } => "app",
            Self::Plugin { .. } => "plugin",
        }
    }

    /// Deduplicate only native-selected files, never names or caller paths.
    pub fn same_selected_file(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::File { path, identity, .. },
                Self::File {
                    path: other_path,
                    identity: other_identity,
                    ..
                },
            ) => {
                path == other_path
                    && identity.same_content_metadata(other_identity)
                    && match (&identity.content_proof, &other_identity.content_proof) {
                        (Some(left), Some(right)) => left == right,
                        _ => identity.same_metadata(other_identity),
                    }
            }
            _ => false,
        }
    }

    pub(super) fn input(&self) -> Option<Value> {
        match self {
            Self::File {
                path, image: true, ..
            } => Some(json!({"type":"localImage","path":path})),
            Self::Skill { name, path, .. } => Some(json!({"type":"skill","name":name,"path":path})),
            Self::App { name, path } | Self::Plugin { name, path } => {
                Some(json!({"type":"mention","name":name,"path":path}))
            }
            Self::File { .. } => None,
        }
    }

    pub(super) fn placeholder(&self) -> Option<String> {
        match self {
            Self::Skill { name, .. } => Some(format!("${name}")),
            Self::App { name, .. } | Self::Plugin { name, .. } => Some(format!("@{name}")),
            Self::File { .. } => None,
        }
    }

    /// Build 20586's composer dG/uG serializes resolved mentions as Markdown
    /// links. Only catalog-authorized paths are used; no UI path is accepted.
    pub(super) fn prompt_link(&self) -> Option<String> {
        let (label, path) = match self {
            Self::Skill { name, path, .. } => (format!("${name}"), path.to_str()?),
            Self::App { name, path } => (format!("${name}"), path.as_str()),
            Self::Plugin { name, path } => (format!("@{name}"), path.as_str()),
            Self::File { .. } => return None,
        };
        // Match the native prompt-link serializer, including its label boundary
        // escape. Paths retain spaces and opening parentheses as native identity;
        // the native parser unescapes backslashes and the closing delimiter.
        let label = label
            .replace('\\', "\\\\")
            .replace('[', "\\[")
            .replace("](", "]\\(")
            .replace(']', "\\]");
        let destination = path.replace('\\', "\\\\").replace(')', "\\)");
        Some(format!("[{label}]({destination})"))
    }

    pub(super) fn attachment(&self) -> Option<Value> {
        if let Self::File {
            name, path, image, ..
        } = self
        {
            let mut value = json!({"label":name,"path":path,"fsPath":path});
            if *image {
                value["isImageAttachment"] = json!(true);
            }
            Some(value)
        } else {
            None
        }
    }

    pub(super) fn file_description(&self) -> Option<String> {
        if let Self::File {
            name, path, image, ..
        } = self
        {
            Some(format!(
                "## {name}: {}{}",
                path.to_str()?,
                if *image {
                    "\nImage attachment: true"
                } else {
                    ""
                }
            ))
        } else {
            None
        }
    }
}

impl FileIdentity {
    fn read(metadata: &std::fs::Metadata) -> Result<Self> {
        if !metadata.is_file() {
            return Err(DesktopError::InvalidInput);
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
            modified: metadata.mtime(),
            modified_ns: metadata.mtime_nsec(),
            changed: metadata.ctime(),
            changed_ns: metadata.ctime_nsec(),
            content_proof: None,
        })
    }

    fn same_content_metadata(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.size == other.size
            && self.modified == other.modified
            && self.modified_ns == other.modified_ns
    }

    fn same_metadata(&self, other: &Self) -> bool {
        self.same_content_metadata(other)
            && self.changed == other.changed
            && self.changed_ns == other.changed_ns
    }
}

fn safe_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
        return Err(DesktopError::InvalidInput);
    }
    Ok(())
}
fn reference_uri(path: &str, prefix: &str) -> Result<()> {
    if path.strip_prefix(prefix).is_none_or(str::is_empty)
        || path.len() > 4096
        || path.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(DesktopError::InvalidInput);
    }
    Ok(())
}
fn safe_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .to_str()
            .is_none_or(|path| path.len() > 8192 || path.chars().any(char::is_control))
    {
        return Err(DesktopError::InvalidInput);
    }
    Ok(())
}
fn file_identity(path: &Path) -> Result<FileIdentity> {
    safe_path(path)?;
    if path.canonicalize().ok().as_deref() != Some(path) {
        return Err(DesktopError::RequestExpired);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| DesktopError::RequestExpired)?;
    FileIdentity::read(&metadata)
}
fn verify_file(path: &Path, expected: &FileIdentity) -> Result<()> {
    verify_file_detailed(path, expected).map_err(|(error, _)| error)
}

fn verify_file_detailed(path: &Path, expected: &FileIdentity) -> ReferenceResult<()> {
    let current = file_identity(path).map_err(|error| {
        (
            error,
            if path.try_exists().unwrap_or(false) {
                CHANGED
            } else {
                UNREADABLE
            },
        )
    })?;
    if !current.same_content_metadata(expected) {
        return Err((DesktopError::RequestExpired, CHANGED));
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| (DesktopError::RequestExpired, UNREADABLE))?;
    let before = file
        .metadata()
        .map_err(|_| (DesktopError::RequestExpired, UNREADABLE))?;
    if FileIdentity::read(&before).map_err(|e| (e, CHANGED))? != current {
        return Err((DesktopError::RequestExpired, CHANGED));
    }
    if let Some(proof) = &expected.content_proof {
        if proof.uid != before.uid() || proof.gid != before.gid() || proof.mode != before.mode() {
            return Err((DesktopError::RequestExpired, CHANGED));
        }
    }
    if !current.same_metadata(expected) {
        let proof = expected
            .content_proof
            .as_ref()
            .ok_or((DesktopError::RequestExpired, CHANGED))?;
        if current.size > super::attachments::MAX_IMAGE_BYTES as u64
            || content_digest(&mut file, current.size)? != proof.sha256
        {
            return Err((DesktopError::RequestExpired, CHANGED));
        }
    }
    let after = file
        .metadata()
        .map_err(|_| (DesktopError::RequestExpired, UNREADABLE))?;
    if FileIdentity::read(&after).map_err(|e| (e, CHANGED))? != current
        || before.uid() != after.uid()
        || before.gid() != after.gid()
        || before.mode() != after.mode()
        || file_identity(path).map_err(|e| (e, CHANGED))? != current
    {
        return Err((DesktopError::RequestExpired, CHANGED));
    }
    Ok(())
}

fn content_digest(file: &mut std::fs::File, size: u64) -> ReferenceResult<String> {
    file.rewind()
        .map_err(|_| (DesktopError::RequestExpired, UNREADABLE))?;
    let mut bounded = file.take(size + 1);
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = bounded
            .read(&mut buffer)
            .map_err(|_| (DesktopError::RequestExpired, UNREADABLE))?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        hash.update(&buffer[..count]);
    }
    if bytes != size {
        return Err((DesktopError::RequestExpired, CHANGED));
    }
    Ok(hex::encode(hash.finalize()))
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod tests;
