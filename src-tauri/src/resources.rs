use std::path::Path;

const AGENTHUB_WEBD_RESOURCE_NAME: &str = "agenthub-webd";
const AGENTHUB_WEBD_HELPER_PLACEHOLDER: &[u8] = b"AGENTHUB_HELPER_PLACEHOLDER";

pub(crate) fn sync_agenthub_webd_helper_from_resource(resource_dir: &Path) -> Result<(), String> {
    let source = resource_dir.join(AGENTHUB_WEBD_RESOURCE_NAME);
    if !source.is_file() {
        return Ok(());
    }
    if is_agenthub_webd_helper_placeholder(&source).map_err(|err| err.to_string())? {
        return Ok(());
    }
    let platform = agenthub_core::platform::PlatformPaths::desktop_current();
    let target = platform.daemon_binary();
    sync_agenthub_webd_helper_file(&source, &target).map_err(|err| err.to_string())
}

pub(crate) fn repair_probe_error_monitor_launch_agent(
    config: &agenthub_core::config::Config,
    platform: &agenthub_core::platform::PlatformPaths,
) -> Result<agenthub_core::probe_error_monitor::ProbeErrorMonitorLaunchAgentStatus, String> {
    if !platform.config_file.is_file() {
        if let Some(parent) = platform.config_file.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let text = toml::to_string_pretty(config).map_err(|err| err.to_string())?;
        std::fs::write(&platform.config_file, text).map_err(|err| err.to_string())?;
    }
    agenthub_core::probe_error_monitor::ensure_probe_error_monitor_launch_agent(
        platform,
        config.probe.enabled
            && (config.probe.error_monitor.enabled || config.probe.notifications.enabled),
    )
    .map_err(|err| err.to_string())
}

fn is_agenthub_webd_helper_placeholder(path: &Path) -> std::io::Result<bool> {
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    let mut prefix = Vec::with_capacity(AGENTHUB_WEBD_HELPER_PLACEHOLDER.len());
    file.take(AGENTHUB_WEBD_HELPER_PLACEHOLDER.len() as u64)
        .read_to_end(&mut prefix)?;
    Ok(prefix == AGENTHUB_WEBD_HELPER_PLACEHOLDER)
}

fn files_have_same_len_and_bytes(source: &Path, target: &Path) -> bool {
    use std::io::Read;

    let (Ok(source_meta), Ok(target_meta)) = (std::fs::metadata(source), std::fs::metadata(target))
    else {
        return false;
    };
    if source_meta.len() != target_meta.len() {
        return false;
    }
    let (Ok(mut src), Ok(mut dst)) = (std::fs::File::open(source), std::fs::File::open(target))
    else {
        return false;
    };
    let mut src_buf = [0u8; 64 * 1024];
    let mut dst_buf = [0u8; 64 * 1024];
    loop {
        let Ok(n) = src.read(&mut src_buf) else {
            return false;
        };
        if n == 0 {
            return true;
        }
        if dst.read_exact(&mut dst_buf[..n]).is_err() || src_buf[..n] != dst_buf[..n] {
            return false;
        }
    }
}

fn sync_agenthub_webd_helper_file(source: &Path, target: &Path) -> std::io::Result<()> {
    static NEXT_TEMP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    if files_have_same_len_and_bytes(source, target) {
        ensure_executable(target)?;
        return Ok(());
    }
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "helper target has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let seq = NEXT_TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp_path = parent.join(format!(
        ".{}.tmp-{}-{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(AGENTHUB_WEBD_RESOURCE_NAME),
        std::process::id(),
        seq,
    ));
    let result = (|| -> std::io::Result<()> {
        let mut src_file = std::fs::File::open(source)?;
        let mut dst_file = std::fs::File::create(&temp_path)?;
        std::io::copy(&mut src_file, &mut dst_file)?;
        dst_file.sync_all()?;
        drop(dst_file);
        ensure_executable(&temp_path)?;
        std::fs::rename(&temp_path, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_agenthub_webd_helper_file_copies_and_marks_executable() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("agenthub-webd");
        let target = temp
            .path()
            .join("Application Support/AgentHub/bin/agenthub-webd");
        std::fs::write(&source, b"#!/bin/sh\nexit 0\n").unwrap();

        sync_agenthub_webd_helper_file(&source, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"#!/bin/sh\nexit 0\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let first_meta = std::fs::metadata(&target).unwrap();
            assert_ne!(
                first_meta.permissions().mode() & 0o111,
                0,
                "helper must be executable"
            );
            let first_ino = first_meta.ino();

            // Repeat sync with identical bytes keeps existing inode untouched.
            sync_agenthub_webd_helper_file(&source, &target).unwrap();
            assert_eq!(std::fs::metadata(&target).unwrap().ino(), first_ino);

            // Updating helper bytes replaces via a temporary file + rename (new inode),
            // avoiding macOS arm64 vnode signature cache invalidation (SIGKILL 137).
            std::fs::write(&source, b"#!/bin/sh\necho v2\nexit 0\n").unwrap();
            sync_agenthub_webd_helper_file(&source, &target).unwrap();
            let second_meta = std::fs::metadata(&target).unwrap();
            assert_eq!(
                std::fs::read(&target).unwrap(),
                b"#!/bin/sh\necho v2\nexit 0\n"
            );
            assert_ne!(
                second_meta.ino(),
                first_ino,
                "updated helper must be replaced with a new inode via atomic rename"
            );

            // Same-length content update also replaces via atomic rename.
            let second_ino = second_meta.ino();
            std::fs::write(&source, b"#!/bin/sh\necho v3\nexit 0\n").unwrap();
            sync_agenthub_webd_helper_file(&source, &target).unwrap();
            let third_meta = std::fs::metadata(&target).unwrap();
            assert_eq!(
                std::fs::read(&target).unwrap(),
                b"#!/bin/sh\necho v3\nexit 0\n"
            );
            assert_ne!(third_meta.ino(), second_ino);

            // Unreadable same-length target is treated as different and replaced atomically.
            let third_ino = third_meta.ino();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000)).unwrap();
            sync_agenthub_webd_helper_file(&source, &target).unwrap();
            let fourth_meta = std::fs::metadata(&target).unwrap();
            assert_ne!(fourth_meta.ino(), third_ino);
            assert_eq!(
                std::fs::read(&target).unwrap(),
                b"#!/bin/sh\necho v3\nexit 0\n"
            );
        }
    }

    #[test]
    fn helper_placeholder_detection_prevents_dev_resource_sync() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("agenthub-webd");
        std::fs::write(&source, b"AGENTHUB_HELPER_PLACEHOLDER\nnot a binary\n").unwrap();

        assert!(is_agenthub_webd_helper_placeholder(&source).unwrap());
    }

    #[test]
    fn error_monitor_launch_agent_contract_uses_helper_only_and_has_no_listener() {
        let plist = agenthub_core::probe_error_monitor::probe_error_monitor_launch_agent_plist(
            Path::new("/Users/test/Library/Application Support/AgentHub/bin/agenthub-webd"),
            Path::new("/Users/test/Library/Application Support/AgentHub/config.toml"),
            Path::new("/Users/test/Library/Logs/AgentHub/probe-error-monitor.out.log"),
            Path::new("/Users/test/Library/Logs/AgentHub/probe-error-monitor.err.log"),
        );
        assert!(plist.contains("<string>monitor-errors</string>"));
        assert!(!plist.contains("<string>serve</string>"));
        assert!(!plist.contains("15742"));
        assert!(!plist.contains("Sockets"));
    }
}
