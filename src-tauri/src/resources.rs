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
    let bytes = std::fs::read(path)?;
    Ok(bytes.starts_with(AGENTHUB_WEBD_HELPER_PLACEHOLDER))
}

fn sync_agenthub_webd_helper_file(source: &Path, target: &Path) -> std::io::Result<()> {
    static NEXT_TEMP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let source_bytes = std::fs::read(source)?;
    let should_copy = match std::fs::read(target) {
        Ok(target_bytes) => source_bytes != target_bytes,
        Err(_) => true,
    };
    if !should_copy {
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
        let mut file = std::fs::File::create(&temp_path)?;
        std::io::Write::write_all(&mut file, &source_bytes)?;
        file.sync_all()?;
        drop(file);
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

pub(crate) fn retire_legacy_desktop_web_service(
    platform: &agenthub_core::platform::PlatformPaths,
) -> anyhow::Result<()> {
    let pid_path = platform.data_dir.join("desktop-webui.pid");
    if let Ok(text) = std::fs::read_to_string(&pid_path) {
        let pid = text.trim().parse::<u32>()?;
        anyhow::ensure!(pid > 1, "invalid legacy desktop WebUI PID");
        let output = std::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "args="])
            .output()?;
        if output.status.success() {
            let arguments = String::from_utf8(output.stdout)?;
            anyhow::ensure!(
                legacy_web_service_matches(arguments.trim(), platform),
                "legacy PID identity mismatch; process preserved"
            );
            let status = std::process::Command::new("/bin/kill")
                .args(["-TERM", &pid.to_string()])
                .status()?;
            anyhow::ensure!(
                status.success(),
                "could not stop verified legacy desktop WebUI"
            );
        }
        std::fs::remove_file(&pid_path)?;
    }
    for name in ["desktop-assets", "webui"] {
        let path = platform.data_dir.join(name);
        if audited_static_copy(&path)? {
            std::fs::remove_dir_all(&path)?;
        }
    }
    Ok(())
}

fn legacy_web_service_matches(
    arguments: &str,
    platform: &agenthub_core::platform::PlatformPaths,
) -> bool {
    arguments
        == format!(
            "{} --config {} serve --surface desktop-lan-webui",
            platform.daemon_binary().display(),
            platform.config_file.display()
        )
}

fn audited_static_copy(path: &Path) -> std::io::Result<bool> {
    if !path.try_exists()? {
        return Ok(false);
    }
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Ok(false);
    }
    let index = path.join("index.html");
    let Ok(text) = std::fs::read_to_string(&index) else {
        return Ok(false);
    };
    if !text.contains("<title>AgentHub</title>") && !text.contains("<title>AgentHub</title>") {
        return Ok(false);
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        if entry.file_type()?.is_symlink() {
            return Ok(false);
        }
        if name == "index.html" && entry.file_type()?.is_file() {
            continue;
        }
        if name != "assets" || !entry.file_type()?.is_dir() {
            return Ok(false);
        }
        for asset in std::fs::read_dir(entry.path())? {
            let asset = asset?;
            if !asset.file_type()?.is_file() {
                return Ok(false);
            }
            let name = asset.file_name().to_string_lossy().into_owned();
            if !name.starts_with("index-") || !(name.ends_with(".js") || name.ends_with(".css")) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod retirement_tests {
    use super::*;
    #[test]
    fn desktop_retirement_preserves_unknown_files_and_monitor() {
        let home = tempfile::tempdir().unwrap();
        let platform = agenthub_core::platform::PlatformPaths::for_kind_with_home(
            agenthub_core::platform::PlatformKind::Macos,
            home.path(),
        );
        let assets = platform.data_dir.join("desktop-assets");
        std::fs::create_dir_all(assets.join("assets")).unwrap();
        std::fs::write(assets.join("index.html"), "<title>AgentHub</title>").unwrap();
        std::fs::write(assets.join("assets/index-hash.js"), "generated").unwrap();
        assert!(audited_static_copy(&assets).unwrap());
        std::fs::write(assets.join("user.txt"), "preserve").unwrap();
        assert!(!audited_static_copy(&assets).unwrap());
        let command = format!(
            "{} --config {} probe monitor-errors",
            platform.daemon_binary().display(),
            platform.config_file.display()
        );
        assert!(!legacy_web_service_matches(&command, &platform));
        let command = format!(
            "{} --config {} serve --surface desktop-lan-webui",
            platform.daemon_binary().display(),
            platform.config_file.display()
        );
        assert!(legacy_web_service_matches(&command, &platform));
        retire_legacy_desktop_web_service(&platform).unwrap();
        assert!(assets.join("user.txt").exists());
    }
}
