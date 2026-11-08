use super::*;

pub(super) fn spawn(config_path: PathBuf, state: Option<AppState>) {
    tokio::spawn(async move {
        let Ok(mut monitor) = agenthub_core::github_ci::Monitor::new() else {
            tracing::warn!("GitHub CI client initialization failed");
            return;
        };
        let mut cache = monitor_start::DaemonMonitorCache::default();
        loop {
            let result = async {
                let config = match &state {
                    Some(state) => state.config(),
                    None => cache.load_config(&config_path)?,
                };
                let db = cache.panel_db(&config)?;
                let events = monitor.tick(&db, &config).await?;
                deliver_events(&db, events, &|| match &state {
                    Some(state) => Ok(state.config()),
                    None => cache.load_config(&config_path),
                })
                .await?;
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

pub(super) async fn deliver_events(
    db: &PanelDb,
    events: Vec<agenthub_core::github_ci::Event>,
    load_config: &notification_delivery::ConfigLoader<'_>,
) -> Result<()> {
    let mut acknowledged = Vec::new();
    let mut delivery_result = Ok(());
    for event in events {
        let key = event.key();
        let result = notification_delivery::github_ci(load_config, db, event).await;
        if let Err(error) = result {
            delivery_result = Err(error);
            break;
        }
        acknowledged.push(key);
    }
    agenthub_core::github_ci::acknowledge(db, &acknowledged)?;
    delivery_result
}
