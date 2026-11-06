use super::*;

pub(super) fn spawn(config_path: PathBuf) {
    tokio::spawn(async move {
        let Ok(mut monitor) = agenthub_core::github_ci::Monitor::new() else {
            tracing::warn!("GitHub CI client initialization failed");
            return;
        };
        let mut cache = monitor_start::DaemonMonitorCache::default();
        loop {
            let result = async {
                let config = cache.load_config(&config_path)?;
                let db = cache.panel_db(&config)?;
                if !monitor.tick(&db, &config).await? {
                    return Ok(());
                }
                for event in agenthub_core::github_ci::pending(&db)? {
                    let current = cache.load_config(&config_path)?;
                    let key = event.key();
                    notification_delivery::github_ci(&current, &db, event).await?;
                    agenthub_core::github_ci::acknowledge(&db, &key)?;
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if result.is_err() {
                tracing::warn!("GitHub CI monitor state unavailable");
            }
            time::sleep(Duration::from_secs(2)).await;
        }
    });
}
