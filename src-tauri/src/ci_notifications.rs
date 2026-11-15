use agenthub_core::github_ci::{Event, Request, Snapshot};
use agenthub_core::services::remote::{MachineTarget, RemoteInvokeRequest};
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};
use tauri::Manager;

#[cfg(target_os = "macos")]
mod native;

#[derive(Default)]
struct Observer {
    source: Option<(u64, u64, i64)>,
    baseline_ms: i64,
    last_scan_ms: i64,
    repositories: BTreeSet<u64>,
    runs: HashMap<(u64, u64), (u64, bool)>,
}

impl Observer {
    fn observe(&mut self, revision: u64, snapshot: &Snapshot) -> Vec<Event> {
        let repositories = &snapshot.monitored_repositories;
        let filter_bots = snapshot.filter_bots;
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
        if !initial && snapshot.last_scan_ms < self.last_scan_ms {
            return Vec::new();
        }
        if initial {
            self.source = Some(source);
            self.baseline_ms = snapshot.last_scan_ms;
            self.runs.clear();
            self.repositories.clear();
        }
        let mut events = Vec::new();
        let runs = snapshot
            .runs
            .iter()
            .map(|run| {
                let key = (run.repository_id, run.id);
                let eligible = run.notification_kind().is_some();
                let source_baseline = snapshot
                    .repository_baselines_ms
                    .get(&run.repository_id)
                    .copied()
                    .unwrap_or(0)
                    .max(if run.is_bot {
                        snapshot.bot_baseline_ms
                    } else {
                        0
                    });
                let finished_ms = chrono::DateTime::parse_from_rfc3339(&run.updated_at)
                    .map_or(0, |time| time.timestamp_millis());
                if !initial
                    && self.repositories.contains(&run.repository_id)
                    && eligible
                    && agenthub_core::github_ci::matches_run_filter(run, repositories, filter_bots)
                    && self.runs.get(&key).map_or_else(
                        || finished_ms > self.baseline_ms.max(source_baseline),
                        |(attempt, completed)| {
                            (*attempt != run.run_attempt || !completed)
                                && (self.last_scan_ms >= source_baseline
                                    || finished_ms > source_baseline)
                        },
                    )
                {
                    events.push(Event {
                        observed_ms: snapshot.last_scan_ms,
                        account: account.clone(),
                        run: run.clone(),
                    });
                }
                (key, (run.run_attempt, eligible))
            })
            .collect();
        self.repositories = snapshot.scanned_repository_ids.clone();
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
        let mut maintenance_at = tokio::time::Instant::now();
        let mut interval = tokio::time::interval(Duration::from_secs(
            agenthub_core::github_ci::ACTIVE_POLL_SECONDS as u64,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if tokio::time::Instant::now() >= maintenance_at {
                let _ = agenthub_core::read_tasks::spawn_blocking(|| {
                    agenthub_core::evict_idle_detail_caches(
                        agenthub_core::resource_limits::DETAIL_CACHE_IDLE_TTL,
                    );
                })
                .await;
                maintenance_at = tokio::time::Instant::now() + Duration::from_secs(60);
            }
            let remote = app.state::<crate::remote::RemoteConnections>();
            let source = remote.view();
            let state = app.state::<crate::DesktopState>().inner().clone();
            let config = state.config();
            let result = if source.target == MachineTarget::Remote {
                Some(
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
                        }),
                )
            } else if !agenthub_core::github_ci::enabled(&config) {
                observer = Observer::default();
                None
            } else {
                Some(
                    tauri::async_runtime::spawn_blocking(move || {
                        agenthub_core::services::use_cases::AgentHubUseCases::with_config_for_surface(
                            &config,
                            state.platform(),
                            state.host_surface(),
                        )
                        .github_ci(&state.db, Request { cached_only: true, ..Default::default() })
                        .map_err(|_| "CI 状态不可用".to_string())
                    })
                    .await
                    .unwrap_or_else(|_| Err("CI 状态不可用".into())),
                )
            };
            if remote.is_current(source.revision) {
                if let Some(Ok(snapshot)) = result {
                    let machine = source
                        .active_machine_name
                        .as_deref()
                        .filter(|_| source.target == MachineTarget::Remote)
                        .unwrap_or("本机");
                    let events = observer.observe(source.revision, &snapshot);
                    #[cfg(target_os = "macos")]
                    if !events.is_empty() {
                        native::send_authorized(machine.to_owned(), events);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests;
