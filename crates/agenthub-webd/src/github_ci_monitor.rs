use super::*;

pub(super) fn spawn(config_path: PathBuf, state: Option<AppState>) {
    tokio::spawn(async move {
        let Ok(mut monitor) = agenthub_core::github_ci::Monitor::new() else {
            tracing::warn!("GitHub CI client initialization failed");
            return;
        };
        let mut cache = monitor_start::DaemonMonitorCache::default();
        let mut check_db_request = true;
        loop {
            let result = async {
                match &state {
                    Some(state) => {
                        let load_config = || state.refresh_config();
                        let events = monitor
                            .tick_in_process(&state.db, &load_config, check_db_request)
                            .await?;
                        deliver_events(&state.db, events, &load_config).await?;
                    }
                    None => {
                        let config = cache.load_config(&config_path)?;
                        let db = cache.panel_db(&config)?;
                        let load_config = || cache.load_config(&config_path);
                        let events = monitor.tick(&db, &load_config).await?;
                        deliver_events(&db, events, &load_config).await?;
                    }
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if result.is_err() {
                tracing::warn!("GitHub CI monitor state unavailable");
            }
            match &state {
                Some(state) => {
                    let delay = if result.is_err() {
                        Duration::from_secs(2)
                    } else {
                        monitor.in_process_wait_duration()
                    };
                    check_db_request = tokio::select! {
                        _ = time::sleep(delay) => result.is_err(),
                        _ = state.ci_wake.notified() => true,
                    };
                }
                None => {
                    let delay = monitor.wait_duration();
                    time::sleep(if result.is_err() {
                        Duration::from_secs(2)
                    } else {
                        delay
                    })
                    .await;
                }
            }
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
