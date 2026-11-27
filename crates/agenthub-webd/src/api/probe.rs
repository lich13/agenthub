use super::{api_error, ok, ApiResponse};
use crate::{
    linux_adapter,
    state::{AppState, CachedProbeStatus},
};
use agenthub_core::{
    config::Config,
    probe::{redact_probe_event_for_output, ProbeRuntime},
    services::{
        probe::{self as probe_service},
        settings as settings_service,
        use_cases::{AgentHubUseCases, LimitU32Request},
    },
};
use serde_json::{json, Value};

pub(crate) use probe_service::ProbeStatusRequest as ProbeStatusQuery;

pub(crate) async fn get_probe_status(state: AppState, query: ProbeStatusQuery) -> ApiResponse {
    ok(probe_status_cached_value(state, query.refresh.unwrap_or(false)).await)
}

pub(crate) async fn get_probe_settings(state: AppState) -> ApiResponse {
    ok(probe_settings_value(&state)?)
}

pub(crate) async fn patch_probe_settings(
    state: AppState,
    request: settings_service::ProbeSettingsSaveRequest,
) -> ApiResponse {
    let config_path = state.config_path().to_path_buf();
    if !config_path.exists() {
        return Err(api_error(format!(
            "config file not found: {}",
            config_path.display()
        )));
    }
    let config = state.config();
    let plan = AgentHubUseCases::with_config(&config, state.platform())
        .settings()
        .and_then(|settings| settings.save_probe_settings(request))?;
    let response_config =
        linux_adapter::apply_probe_settings_save_plan(&state, &config_path, plan)?;
    ok(probe_settings_value_for_config(&state, &response_config)?)
}

pub(crate) async fn get_probe_events(state: AppState, query: LimitU32Request) -> ApiResponse {
    let limit = query
        .limit
        .unwrap_or(state.config().probe.recent_limit as u32)
        .clamp(1, 500);
    let events = state
        .db
        .list_probe_events(limit)?
        .into_iter()
        .map(redact_probe_event_for_output)
        .collect::<Vec<_>>();
    ok(json!({
        "events": events,
        "limit": limit,
    }))
}

pub(crate) async fn start_probe_action(
    state: AppState,
    action: probe_service::ProbeAction,
) -> ApiResponse {
    let config_path = state.config_path().to_path_buf();
    let plan = probe_service::plan_probe_action(action)?;
    let id = linux_adapter::start_probe_action_plan(&state, plan, &config_path)?;
    ok(json!({"job_id": id}))
}

fn probe_runtime(state: &AppState) -> anyhow::Result<ProbeRuntime> {
    Ok(ProbeRuntime::new(
        state.refresh_config()?,
        state.platform().clone(),
    ))
}

async fn probe_status_cached_value(state: AppState, force_refresh: bool) -> Value {
    if force_refresh {
        let value = probe_status_fresh_value(state.clone(), true).await;
        store_probe_status_snapshot(&state, value.clone());
        return probe_service::probe_status_snapshot_view(
            value,
            0,
            false,
            probe_service::ProbeSnapshotStatus::Fresh,
        );
    }

    if let Some(snapshot) = current_probe_status_snapshot(&state) {
        let now = jiff::Timestamp::now().as_second();
        let age = now.saturating_sub(snapshot.refreshed_at_unix).max(0);
        let refreshing = if age >= 2 {
            spawn_probe_status_refresh(state);
            true
        } else {
            is_probe_status_refreshing(&state)
        };
        return probe_service::probe_status_snapshot_view(
            snapshot.value,
            age,
            refreshing,
            probe_service::ProbeSnapshotStatus::Cached,
        );
    }

    let value = probe_status_fresh_value(state.clone(), false).await;
    store_probe_status_snapshot(&state, value.clone());
    probe_service::probe_status_snapshot_view(
        value,
        0,
        false,
        probe_service::ProbeSnapshotStatus::Initial,
    )
}

async fn probe_status_fresh_value(state: AppState, force_refresh: bool) -> Value {
    match async {
        let status = probe_runtime(&state)?
            .status_with_db(&state.db, force_refresh)
            .await?;
        Ok::<_, anyhow::Error>(json!(status))
    }
    .await
    {
        Ok(status) => status,
        Err(err) => json!({
            "label": "Probe",
            "enabled": state.config().probe.enabled,
            "available": false,
            "flavor": "builtin",
            "error": err.to_string(),
        }),
    }
}

fn current_probe_status_snapshot(state: &AppState) -> Option<CachedProbeStatus> {
    state
        .probe_status_cache
        .lock()
        .expect("probe status cache")
        .snapshot
        .clone()
}

fn is_probe_status_refreshing(state: &AppState) -> bool {
    state
        .probe_status_cache
        .lock()
        .expect("probe status cache")
        .refreshing
}

fn store_probe_status_snapshot(state: &AppState, value: Value) {
    let mut cache = state.probe_status_cache.lock().expect("probe status cache");
    cache.snapshot = Some(CachedProbeStatus {
        value,
        refreshed_at_unix: jiff::Timestamp::now().as_second(),
    });
    cache.refreshing = false;
}

pub(crate) fn spawn_probe_status_refresh(state: AppState) {
    {
        let mut cache = state.probe_status_cache.lock().expect("probe status cache");
        if cache.refreshing {
            return;
        }
        cache.refreshing = true;
    }
    tokio::spawn(async move {
        let value = probe_status_fresh_value(state.clone(), false).await;
        store_probe_status_snapshot(&state, value);
    });
}

fn probe_settings_value(state: &AppState) -> anyhow::Result<Value> {
    probe_settings_value_for_config(state, &state.refresh_config()?)
}

fn probe_settings_value_for_config(state: &AppState, config: &Config) -> anyhow::Result<Value> {
    let secret_state = settings_service::ProbeSecretState::from_secret_bytes(
        state
            .db
            .get_secret_setting_bytes(settings_service::PROBE_BARK_DEVICE_KEY_SETTING)?
            .as_deref(),
    );
    let plan = AgentHubUseCases::with_config(config, state.platform())
        .settings()?
        .probe_settings_view(secret_state)?;
    serde_json::to_value(plan).map_err(anyhow::Error::from)
}
