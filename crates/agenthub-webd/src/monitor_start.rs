//! Existing monitor start implementation, split without changing dispatch order.
use super::*;

pub(super) fn spawn_probe_error_monitor(state: AppState) {
    tokio::spawn(async move {
        loop {
            let stamp = config_stamp(state.config_path());
            let poll_seconds = match state.refresh_config() {
                Ok(config) => {
                    match run_probe_error_monitor_once(&state.db, &|| state.refresh_config()).await
                    {
                        Ok(outcome) if outcome.new_incidents > 0 => tracing::info!(
                            new_incidents = outcome.new_incidents,
                            "Probe error monitor processed Codex turn errors"
                        ),
                        Ok(_) => tracing::debug!("Probe error monitor scan completed"),
                        Err(err) => tracing::warn!("Probe error monitor scan failed: {err}"),
                    }
                    config.probe.poll_seconds.clamp(5, 3_600)
                }
                Err(err) => {
                    tracing::warn!("Probe error monitor config load failed: {err}");
                    15
                }
            };
            wait_for_config_change(poll_seconds, || config_stamp(state.config_path()) != stamp)
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

use agenthub_core::config::{config_stamp, ConfigCache};

#[derive(Default)]
pub(super) struct DaemonMonitorCache {
    config: ConfigCache,
    db: Option<(PathBuf, PanelDb)>,
}

impl DaemonMonitorCache {
    pub(super) fn load_config(&self, config_path: &Path) -> Result<Config> {
        self.config.load(config_path)
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
}
