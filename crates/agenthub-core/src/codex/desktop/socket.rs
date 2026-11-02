use super::{DesktopError, Result};
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Component, Path},
};
use tokio::net::UnixStream;

#[derive(PartialEq, Eq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

pub(super) async fn connect(codex_home: &Path) -> Result<UnixStream> {
    if !codex_home.is_absolute()
        || codex_home
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(DesktopError::UnsafeSocket);
    }
    // SAFETY: geteuid has no pointer arguments or mutable process state.
    let uid = unsafe { libc::geteuid() };
    let root = codex_home.to_path_buf();
    let before = crate::read_tasks::spawn_blocking(move || validate(&root, uid))
        .await
        .map_err(|_| DesktopError::Unavailable)??;
    let stream = UnixStream::connect(codex_home.join("ipc/ipc.sock"))
        .await
        .map_err(|_| DesktopError::Unavailable)?;
    if stream
        .peer_cred()
        .map_err(|_| DesktopError::UnsafeSocket)?
        .uid()
        != uid
    {
        return Err(DesktopError::UnsafeSocket);
    }
    let root = codex_home.to_path_buf();
    let after = crate::read_tasks::spawn_blocking(move || validate(&root, uid))
        .await
        .map_err(|_| DesktopError::Unavailable)??;
    if before != after {
        return Err(DesktopError::UnsafeSocket);
    }
    Ok(stream)
}

fn validate(codex_home: &Path, uid: u32) -> Result<SocketIdentity> {
    // Unlike the separate app-server daemon rendezvous path, desktop IPC is a
    // direct socket. Never follow a replacement symlink at these boundaries.
    for directory in [codex_home.to_path_buf(), codex_home.join("ipc")] {
        let metadata = fs::symlink_metadata(directory).map_err(|_| DesktopError::Unavailable)?;
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
            return Err(DesktopError::UnsafeSocket);
        }
    }
    let metadata = fs::symlink_metadata(codex_home.join("ipc/ipc.sock"))
        .map_err(|_| DesktopError::Unavailable)?;
    if !metadata.file_type().is_socket() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(DesktopError::UnsafeSocket);
    }
    Ok(SocketIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}
