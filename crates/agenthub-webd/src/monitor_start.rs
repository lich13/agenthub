//! Existing monitor start implementation, split without changing dispatch order.
use super::*;

pub(super) fn spawn_probe_error_monitor(state: AppState, config_path: PathBuf) {
    tokio::spawn(async move {
        let cache = DaemonMonitorCache::default();
        loop {
            let config = state.config();
            match run_probe_error_monitor_once(&state.db, &|| cache.load_config(&config_path)).await
            {
                Ok(outcome) if outcome.new_incidents > 0 => tracing::info!(
                    new_incidents = outcome.new_incidents,
                    "Probe error monitor processed Codex turn errors"
                ),
                Ok(_) => tracing::debug!("Probe error monitor scan completed"),
                Err(err) => tracing::warn!("Probe error monitor scan failed: {err}"),
            }
            wait_for_config_change(config.probe.poll_seconds.clamp(5, 3_600), || {
                state.probe_settings_changed(&config.probe)
            })
            .await;
        }
    });
}

async fn wait_for_config_change(seconds: u64, changed: impl Fn() -> bool) {
    let deadline = time::Instant::now() + Duration::from_secs(seconds);
    while !changed() && time::Instant::now() < deadline {
        time::sleep_until(deadline.min(time::Instant::now() + Duration::from_secs(2))).await;
    }
}

#[derive(PartialEq, Eq)]
struct FileStamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStamp {
        modified: meta.modified().ok(),
        len: meta.len(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
        },
    })
}

type ConfigFileStamp = (FileStamp, Option<FileStamp>);
fn config_stamp(path: &Path) -> Option<ConfigFileStamp> {
    Some((
        file_stamp(path)?,
        path.parent()
            .and_then(|parent| file_stamp(&parent.join("env"))),
    ))
}

#[derive(Default)]
pub(super) struct DaemonMonitorCache {
    config: std::sync::Mutex<Option<(ConfigFileStamp, Config)>>,
    db: Option<(PathBuf, PanelDb)>,
}

impl DaemonMonitorCache {
    pub(super) fn load_config(&self, config_path: &Path) -> Result<Config> {
        let mut cached = self
            .config
            .lock()
            .map_err(|_| anyhow::anyhow!("configuration cache unavailable"))?;
        let stamp = config_stamp(config_path);
        if let (Some(current_stamp), Some((cached_stamp, cached_cfg))) = (&stamp, &*cached) {
            if current_stamp == cached_stamp {
                return Ok(cached_cfg.clone());
            }
        }
        let loaded = Config::load_runtime(config_path)?;
        // A replacement during loading must not label the loaded value with a
        // different file's identity. The next scan will load it normally.
        *cached = stamp
            .filter(|before| Some(before) == config_stamp(config_path).as_ref())
            .map(|stamp| (stamp, loaded.clone()));
        Ok(loaded)
    }

    pub(super) fn panel_db(&mut self, config: &Config) -> Result<PanelDb> {
        if let Some((cached_path, cached_db)) = &self.db {
            if cached_path == &config.paths.db_path {
                return Ok(cached_db.clone());
            }
        }
        let db = open_panel_db(config)?;
        self.db = Some((config.paths.db_path.clone(), db.clone()));
        Ok(db)
    }
}

pub(super) async fn run_probe_error_monitor_daemon(config_path: PathBuf) -> Result<()> {
    super::github_ci_monitor::spawn(config_path.clone(), None);
    let mut upgraded_hooks = false;
    let mut cache = DaemonMonitorCache::default();
    loop {
        let stamp = config_stamp(&config_path);
        let poll_seconds = match cache.load_config(&config_path) {
            Ok(config) => {
                if !upgraded_hooks {
                    upgrade_managed_question_hooks_on_start(&config);
                    upgraded_hooks = true;
                }
                match cache.panel_db(&config) {
                    Ok(db) => {
                        if let Err(err) =
                            run_probe_error_monitor_once(&db, &|| cache.load_config(&config_path))
                                .await
                        {
                            tracing::warn!("Probe error monitor scan failed: {err}");
                        }
                    }
                    Err(err) => tracing::warn!("Probe error monitor DB open failed: {err}"),
                }
                config.probe.poll_seconds.clamp(5, 3_600)
            }
            Err(err) => {
                tracing::warn!("Probe error monitor config load failed: {err}");
                15
            }
        };
        wait_for_config_change(poll_seconds, || config_stamp(&config_path) != stamp).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn config_changes_wake_long_sleeps_without_increasing_scan_frequency() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let changed = Arc::new(AtomicBool::new(false));
        let signal = changed.clone();
        let start = time::Instant::now();
        let wait = tokio::spawn(async move {
            wait_for_config_change(3600, || signal.load(Ordering::SeqCst)).await;
        });
        tokio::task::yield_now().await;
        time::advance(Duration::from_secs(1)).await;
        assert!(!wait.is_finished());
        changed.store(true, Ordering::SeqCst);
        time::advance(Duration::from_secs(1)).await;
        wait.await.unwrap();
        assert_eq!(start.elapsed(), Duration::from_secs(2));
        let start = time::Instant::now();
        wait_for_config_change(5, || false).await;
        assert_eq!(start.elapsed(), Duration::from_secs(5));
        let start = time::Instant::now();
        wait_for_config_change(3600, || true).await;
        assert!(start.elapsed().is_zero());
    }

    #[test]
    fn daemon_monitor_cache_reuses_unchanged_config_and_db_until_modified() {
        let root =
            std::env::temp_dir().join(format!("agenthub-daemon-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg_path = root.join("config.toml");
        let db_a = root.join("panel_a.sqlite");
        let db_b = root.join("panel_b.sqlite");
        let mut cfg = Config::default();
        cfg.paths.data_dir = root.clone();
        cfg.paths.db_path = db_a.clone();
        cfg.security.secret_key = agenthub_core::crypto::SecretBox::random_key_material();
        cfg.probe.poll_seconds = 25;
        std::fs::write(&cfg_path, toml::to_string_pretty(&cfg).unwrap()).unwrap();

        let mut cache = DaemonMonitorCache::default();
        let loaded_1 = cache.load_config(&cfg_path).unwrap();
        assert_eq!(loaded_1.probe.poll_seconds, 25);
        let db_1 = cache.panel_db(&loaded_1).unwrap();
        db_1.set_setting("daemon_cache_probe", "ok").unwrap();

        // Unchanged config & db_path reuse cached instances.
        let loaded_2 = cache.load_config(&cfg_path).unwrap();
        let db_2 = cache.panel_db(&loaded_2).unwrap();
        assert_eq!(
            db_2.get_setting("daemon_cache_probe").unwrap().as_deref(),
            Some("ok")
        );

        // Changing config file size/content invalidates cached Config and switches DB when db_path changes.
        cfg.probe.poll_seconds = 45;
        cfg.paths.db_path = db_b.clone();
        std::fs::write(&cfg_path, toml::to_string_pretty(&cfg).unwrap()).unwrap();
        let loaded_3 = cache.load_config(&cfg_path).unwrap();
        assert_eq!(loaded_3.probe.poll_seconds, 45);
        let db_3 = cache.panel_db(&loaded_3).unwrap();
        assert!(db_3.get_setting("daemon_cache_probe").unwrap().is_none());

        drop(db_1);
        drop(db_2);
        drop(db_3);
        drop(cache);
        let _ = std::fs::remove_dir_all(&root);
    }
    #[cfg(unix)]
    #[test]
    fn daemon_config_and_env_atomic_replacements_invalidate_same_size_and_mtime() {
        let root =
            std::env::temp_dir().join(format!("agenthub-daemon-stamp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("config.toml");
        let env = root.join("env");
        let mut cfg = Config::default();
        cfg.security.secret_key = agenthub_core::crypto::SecretBox::random_key_material();
        cfg.probe.poll_seconds = 25;
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        let cache = DaemonMonitorCache::default();
        assert_eq!(cache.load_config(&path).unwrap().probe.poll_seconds, 25);
        let replace = |path: &Path, text: &str| {
            let before = std::fs::metadata(path).unwrap();
            let next = path.with_extension("replacement");
            std::fs::write(&next, text).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&next)
                .unwrap()
                .set_modified(before.modified().unwrap())
                .unwrap();
            assert_eq!(std::fs::metadata(&next).unwrap().len(), before.len());
            std::fs::rename(next, path).unwrap();
        };
        cfg.probe.poll_seconds = 45;
        replace(&path, &toml::to_string(&cfg).unwrap());
        assert_eq!(cache.load_config(&path).unwrap().probe.poll_seconds, 45);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        cfg.probe.poll_seconds = 65;
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(cache.load_config(&path).unwrap().probe.poll_seconds, 65);
        // Creation is part of the cache identity even when TOML has precedence.
        let first = agenthub_core::crypto::SecretBox::random_key_material();
        let second = agenthub_core::crypto::SecretBox::random_key_material();
        std::fs::write(&env, format!("AGENTHUB_SECRET_KEY={first}\n")).unwrap();
        assert!(cache.load_config(&path).unwrap().security.secret_key == cfg.security.secret_key);
        assert!(
            cache
                .config
                .lock()
                .unwrap()
                .as_ref()
                .map(|(stamp, _)| stamp)
                == config_stamp(&path).as_ref()
        );
        cfg.security.secret_key.clear();
        std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
        assert!(cache.load_config(&path).unwrap().security.secret_key == first);
        replace(&env, &format!("AGENTHUB_SECRET_KEY={second}\n"));
        assert!(cache.load_config(&path).unwrap().security.secret_key == second);
        std::fs::remove_file(&env).unwrap();
        assert!(cache.load_config(&path).unwrap().security.secret_key != second);
        std::fs::remove_dir_all(root).unwrap();
    }
}
