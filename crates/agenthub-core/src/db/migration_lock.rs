//! Serialize upgrades across App/helper/server processes, including VACUUM.
//! A private sidecar avoids conflicting with SQLite byte-range locks on macOS.
use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    time::{Duration, Instant},
};

pub(super) struct MigrationLock(Option<File>);
impl MigrationLock {
    pub(super) fn acquire(path: &Path) -> Result<Self> {
        // Independent in-memory databases never share a migration target.
        if path == Path::new(":memory:") {
            return Ok(Self(None));
        }
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".migration.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(lock_path)
            .context("open database migration lock")?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let started = Instant::now();
            loop {
                // SAFETY: the file owns a valid descriptor throughout this guard.
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::WouldBlock {
                    return Err(error).context("lock database migration");
                }
                anyhow::ensure!(
                    started.elapsed() < Duration::from_secs(10),
                    "database migration is busy; retry after the current upgrade finishes"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        Ok(Self(Some(file)))
    }
}
impl Drop for MigrationLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: this descriptor is still owned by the guard.
            unsafe {
                if let Some(file) = &self.0 {
                    libc::flock(file.as_raw_fd(), libc::LOCK_UN);
                }
            }
        }
    }
}
