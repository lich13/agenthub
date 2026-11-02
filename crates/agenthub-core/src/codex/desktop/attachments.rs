//! Private pasted-image storage, guarded by the existing desktop worker lease.
//! Images must remain readable by the native owner, so only the ownership index
//! is encrypted. A durable retention marker precedes every queue handoff.
use super::references::NativeReference;
use crate::{crypto::SecretBox, security::hash_token};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_STORE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_IMAGES: usize = 256;
pub const DRAFT_SECONDS: u64 = 30 * 60;
const INDEX_LIMIT: u64 = 1024 * 1024;
const INDEX: &str = "ownership.enc";
const ERROR: &str = "粘贴附件存储无法安全读取，已保留现有文件";
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    reference: NativeReference,
    created: u64,
    state: Retention,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Retention {
    Draft,
    Retained,
    Released,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u32,
    root: PathBuf,
    entries: Vec<Entry>,
}

pub struct AttachmentStore {
    root: PathBuf,
    directory: (u64, u64),
    index_identity: Option<(u64, u64, u64, i64, i64)>,
    crypto: SecretBox,
    index: Index,
    faulted: bool,
}

pub fn root_for_database(database: &Path) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(".desktop-attachments");
    path.into()
}

impl AttachmentStore {
    /// Call only while holding DesktopWorkerLease, including recovery/cleanup.
    pub fn open(root: PathBuf, crypto: SecretBox, now: u64) -> Result<Self> {
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&root)
                    .map_err(|_| ERROR)?;
            }
            Err(_) => return Err(ERROR),
            Ok(_) => {}
        }
        let directory = private_directory(&root)?;
        let root = root.canonicalize().map_err(|_| ERROR)?;
        let (index, index_identity) = match private_file(&root.join(INDEX)) {
            Ok(mut file) => {
                let metadata = file.metadata().map_err(|_| ERROR)?;
                if metadata.len() > INDEX_LIMIT || metadata.len() < 12 {
                    return Err(ERROR);
                }
                let mut bytes = Vec::new();
                Read::by_ref(&mut file)
                    .take(INDEX_LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| ERROR)?;
                if bytes.len() as u64 > INDEX_LIMIT {
                    return Err(ERROR);
                }
                let plaintext = crypto
                    .decrypt(&bytes[12..], &bytes[..12])
                    .map_err(|_| ERROR)?;
                let index: Index = serde_json::from_slice(&plaintext).map_err(|_| ERROR)?;
                (index, Some(identity(&metadata)))
            }
            Err(_)
                if fs::symlink_metadata(root.join(INDEX))
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                (
                    Index {
                        version: 1,
                        root: root.clone(),
                        entries: Vec::new(),
                    },
                    None,
                )
            }
            Err(_) => return Err(ERROR),
        };
        if index.version != 1 || index.root != root || index.entries.len() > MAX_IMAGES {
            return Err(ERROR);
        }
        let mut store = Self {
            root,
            directory,
            index_identity,
            crypto,
            index,
            faulted: false,
        };
        store.validate_inventory()?;
        store.expire(now)?;
        Ok(store)
    }

    fn check(&self) -> Result<()> {
        if self.faulted || private_directory(&self.root)? != self.directory {
            return Err(ERROR);
        }
        let current = match private_file(&self.root.join(INDEX)) {
            Ok(file) => Some(identity(&file.metadata().map_err(|_| ERROR)?)),
            Err(_)
                if fs::symlink_metadata(self.root.join(INDEX))
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(_) => return Err(ERROR),
        };
        if current != self.index_identity {
            return Err(ERROR);
        }
        Ok(())
    }

    fn path<'a>(&self, reference: &'a NativeReference) -> Result<&'a Path> {
        let NativeReference::File {
            path, image: true, ..
        } = reference
        else {
            return Err(ERROR);
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ERROR)?;
        if path.parent() != Some(self.root.as_path())
            || name.len() != 68
            || !name.ends_with(".png")
            || !name[..64].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ERROR);
        }
        Ok(path)
    }

    fn validate_inventory(&self) -> Result<u64> {
        self.check()?;
        let mut known = std::collections::HashSet::new();
        let mut bytes = 0u64;
        for entry in &self.index.entries {
            let path = self.path(&entry.reference)?;
            if !known.insert(path.to_path_buf()) {
                return Err(ERROR);
            }
            if entry.state == Retention::Released && !path.try_exists().map_err(|_| ERROR)? {
                continue;
            }
            let file = private_file(path)?;
            entry.reference.validate().map_err(|_| ERROR)?;
            bytes = bytes
                .checked_add(file.metadata().map_err(|_| ERROR)?.len())
                .ok_or(ERROR)?;
        }
        for file in fs::read_dir(&self.root).map_err(|_| ERROR)? {
            let path = file.map_err(|_| ERROR)?.path();
            // An interrupted, unindexed write is deliberately preserved. Never
            // guess ownership or silently evict history after a crash.
            if path.file_name().is_some_and(|name| name == INDEX) || known.contains(&path) {
                continue;
            }
            return Err(ERROR);
        }
        if bytes > MAX_STORE_BYTES {
            return Err(ERROR);
        }
        Ok(bytes)
    }

    fn persist(&mut self, next: Index) -> Result<()> {
        self.check()?;
        let bytes = serde_json::to_vec(&next).map_err(|_| ERROR)?;
        let (encrypted, nonce) = self.crypto.encrypt(&bytes).map_err(|_| ERROR)?;
        if encrypted.len() + nonce.len() > INDEX_LIMIT as usize {
            return Err(ERROR);
        }
        let temporary = self.root.join(format!(
            "{}.tmp",
            hash_token(&SecretBox::random_key_material())
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)
                .map_err(|_| ERROR)?;
            file.write_all(&nonce)
                .and_then(|()| file.write_all(&encrypted))
                .and_then(|()| file.sync_all())
                .map_err(|_| ERROR)?;
            self.check()?;
            fs::rename(&temporary, self.root.join(INDEX)).map_err(|_| ERROR)?;
            File::open(&self.root)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| ERROR)?;
            self.index_identity = Some(identity(&file.metadata().map_err(|_| ERROR)?));
            self.index = next;
            Ok(())
        })();
        if result.is_err() {
            // A failed rename/fsync has an unknown durable outcome. Stop all
            // mutations until the encrypted index is reopened under the lease.
            self.faulted = true;
        }
        result
    }

    pub fn import_png(&mut self, bytes: &[u8], now: u64) -> Result<NativeReference> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("粘贴图片超过 20MiB");
        }
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err("粘贴图片格式无效");
        }
        self.expire(now)?;
        let size = self.validate_inventory()?;
        if self.index.entries.len() >= MAX_IMAGES || size + bytes.len() as u64 > MAX_STORE_BYTES {
            return Err("粘贴附件存储已满，请使用文件选择器添加图片");
        }
        let path = self.root.join(format!(
            "{}.png",
            hash_token(&SecretBox::random_key_material())
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|_| ERROR)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| ERROR)?;
        let mut reference = NativeReference::selected_file(path).map_err(|_| ERROR)?;
        if let NativeReference::File { name, .. } = &mut reference {
            *name = "粘贴图片.png".into();
        }
        let mut next = self.index.clone();
        next.entries.push(Entry {
            reference: reference.clone(),
            created: now,
            state: Retention::Draft,
        });
        self.persist(next)?;
        Ok(reference)
    }

    /// Must finish and fsync before calling the existing worker/ledger. Even an
    /// enqueue failure or unknown result is retained conservatively forever.
    pub fn retain_before_send(&mut self, references: &[NativeReference]) -> Result<()> {
        self.check()?;
        let mut next = self.index.clone();
        let mut changed = false;
        for reference in references {
            let NativeReference::File { path, .. } = reference else {
                continue;
            };
            if path.parent() != Some(self.root.as_path()) {
                continue;
            }
            let entry = next
                .entries
                .iter_mut()
                .find(|entry| entry.reference == *reference)
                .ok_or(ERROR)?;
            if entry.state == Retention::Released {
                return Err(ERROR);
            }
            reference.validate().map_err(|_| ERROR)?;
            changed |= entry.state != Retention::Retained;
            entry.state = Retention::Retained;
        }
        if changed {
            self.persist(next)?;
        }
        Ok(())
    }

    pub fn release(&mut self, references: &[NativeReference]) -> Result<()> {
        self.check()?;
        let mut next = self.index.clone();
        for entry in &mut next.entries {
            if entry.state == Retention::Draft && references.contains(&entry.reference) {
                entry.state = Retention::Released;
            }
        }
        self.remove_released(next)
    }

    pub fn expire(&mut self, now: u64) -> Result<()> {
        self.check()?;
        let mut next = self.index.clone();
        for entry in &mut next.entries {
            if entry.state == Retention::Draft && now.saturating_sub(entry.created) >= DRAFT_SECONDS
            {
                entry.state = Retention::Released;
            }
        }
        self.remove_released(next)
    }

    fn remove_released(&mut self, next: Index) -> Result<()> {
        if !next
            .entries
            .iter()
            .any(|entry| entry.state == Retention::Released)
        {
            return Ok(());
        }
        // Mark first: recovery can finish a deletion interrupted on either side
        // of unlink without confusing a retained native attachment with a draft.
        self.persist(next)?;
        for entry in self
            .index
            .entries
            .iter()
            .filter(|entry| entry.state == Retention::Released)
        {
            let path = self.path(&entry.reference)?;
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(ERROR),
                Ok(_) => {}
            }
            private_file(path)?;
            entry.reference.validate().map_err(|_| ERROR)?;
            self.check()?;
            fs::remove_file(path).map_err(|_| ERROR)?;
        }
        let mut next = self.index.clone();
        next.entries
            .retain(|entry| entry.state != Retention::Released);
        self.persist(next)
    }
}

fn private_directory(path: &Path) -> Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ERROR)?;
    // SAFETY: geteuid only reads this process's effective UID.
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(ERROR);
    }
    Ok((metadata.dev(), metadata.ino()))
}
fn private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| ERROR)?;
    let metadata = file.metadata().map_err(|_| ERROR)?;
    // SAFETY: geteuid only reads this process's effective UID.
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
    {
        return Err(ERROR);
    }
    Ok(file)
}
fn identity(metadata: &fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

#[cfg(test)]
#[path = "attachments_tests.rs"]
mod tests;
