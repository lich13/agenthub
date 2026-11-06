use agenthub_core::github_ci::{Event, Request, Snapshot};
use agenthub_core::services::remote::{MachineTarget, RemoteInvokeRequest};
use std::{collections::HashMap, time::Duration};
use tauri::Manager;

#[cfg(target_os = "macos")]
mod native;

#[derive(Default)]
struct Observer {
    source: Option<(u64, u64, i64)>,
    baseline_ms: i64,
    last_scan_ms: i64,
    runs: HashMap<(u64, u64), (u64, bool)>,
}

impl Observer {
    fn observe(&mut self, revision: u64, snapshot: &Snapshot) -> Vec<Event> {
        let Some(account) = snapshot
            .account
            .as_ref()
            .filter(|_| snapshot.enabled && snapshot.baseline_ms > 0)
        else {
            *self = Self::default();
            return Vec::new();
        };
        let source = (revision, account.id, snapshot.baseline_ms);
        let initial = self.source != Some(source);
        if !initial && snapshot.last_scan_ms <= self.last_scan_ms {
            return Vec::new();
        }
        if initial {
            self.source = Some(source);
            self.baseline_ms = snapshot.last_scan_ms;
            self.runs.clear();
        }
        let mut events = Vec::new();
        let runs = snapshot
            .runs
            .iter()
            .map(|run| {
                let key = (run.repository_id, run.id);
                let eligible = run.notification_kind().is_some();
                let changed = self.runs.get(&key).map_or_else(
                    || {
                        chrono::DateTime::parse_from_rfc3339(&run.updated_at)
                            .is_ok_and(|time| time.timestamp_millis() > self.baseline_ms)
                    },
                    |(attempt, completed)| *attempt != run.run_attempt || !completed,
                );
                if !initial && eligible && changed {
                    events.push(Event {
                        observed_ms: snapshot.last_scan_ms,
                        account: account.clone(),
                        run: run.clone(),
                    });
                }
                (key, (run.run_attempt, eligible))
            })
            .collect();
        self.runs = runs;
        self.last_scan_ms = snapshot.last_scan_ms;
        events
    }
}

pub(crate) fn start(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    native::install(app.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut observer = Observer::default();
        loop {
            let remote = app.state::<crate::remote::RemoteConnections>();
            let source = remote.view();
            if remote.is_current(source.revision) {
                let request = Request {
                    cached_only: true,
                    ..Default::default()
                };
                let result = if source.target == MachineTarget::Remote {
                    remote
                        .invoke(&RemoteInvokeRequest {
                            revision: source.revision,
                            command: "github.ci.status".into(),
                            args: serde_json::json!({ "cached_only": true }),
                        })
                        .await
                        .and_then(|value| {
                            serde_json::from_value::<Snapshot>(value)
                                .map_err(|_| "CI 状态不可用".into())
                        })
                } else {
                    let state = app.state::<crate::DesktopState>().inner().clone();
                    if state.config().probe.monitor_enabled() {
                        *state.github_ci.lock().await = None;
                    }
                    tauri::async_runtime::spawn_blocking(move || {
                        let config = state.config();
                        agenthub_core::services::use_cases::AgentHubUseCases::with_config_for_surface(
                            &config, state.platform(), state.host_surface(),
                        ).github_ci(&state.db, request).map_err(|_| "CI 状态不可用".to_string())
                    })
                    .await
                    .unwrap_or_else(|_| Err("CI 状态不可用".into()))
                };
                if remote.is_current(source.revision) {
                    if let Ok(snapshot) = result {
                        let machine = source
                            .active_machine_name
                            .as_deref()
                            .filter(|_| source.target == MachineTarget::Remote)
                            .unwrap_or("本机");
                        for event in observer.observe(source.revision, &snapshot) {
                            #[cfg(target_os = "macos")]
                            native::send(&app, source.revision, machine, &event);
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(
                agenthub_core::github_ci::POLL_SECONDS as u64,
            ))
            .await;
        }
    });
}

#[cfg(test)]
mod tests;
