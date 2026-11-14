use crate::{overview::DesktopState, services::actions::DesktopActionResponse};
use agenthub_core::services::use_cases::AgentHubUseCases;
use agenthub_core::{
    config::Config,
    probe::redact_probe_event_for_output,
    services::{
        jobs as job_service, probe as probe_service,
        settings::{self as settings_service, ProbeSettingsSaveRequest},
    },
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopProbeSettings {
    pub codex: Value,
    pub probe: agenthub_core::config::ProbeConfig,
    pub notifications: Value,
}

impl From<settings_service::SettingsView> for DesktopProbeSettings {
    fn from(view: settings_service::SettingsView) -> Self {
        Self {
            codex: serde_json::to_value(view.codex).unwrap_or_else(|_| json!({})),
            probe: view.probe,
            notifications: serde_json::to_value(view.notifications).unwrap_or_else(|_| json!({})),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopProbeEventsResponse {
    pub events: Vec<agenthub_core::db::ProbeEvent>,
    pub limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopProbeEventsRequest {
    pub limit: Option<u32>,
}

pub(crate) fn probe_settings_with_state(state: &DesktopState) -> Result<DesktopProbeSettings> {
    let config = state.config();
    let secret_state = settings_service::ProbeSecretState::from_secret_bytes(
        state
            .db
            .get_secret_setting_bytes(settings_service::PROBE_BARK_DEVICE_KEY_SETTING)?
            .as_deref(),
    );
    let plan = AgentHubUseCases::with_config(&config, state.platform())
        .settings()?
        .probe_settings_view(secret_state)?;
    Ok(DesktopProbeSettings::from(plan))
}

pub(crate) fn probe_save_settings_with_state(
    state: &DesktopState,
    request: ProbeSettingsSaveRequest,
) -> Result<DesktopProbeSettings> {
    probe_save_settings_with_state_and_repair(state, request, |config, platform| {
        crate::resources::repair_probe_error_monitor_launch_agent(config, platform)
            .map(|_| ())
            .map_err(anyhow::Error::msg)
    })
}

fn probe_save_settings_with_state_and_repair(
    state: &DesktopState,
    request: ProbeSettingsSaveRequest,
    repair_error_monitor: impl FnOnce(&Config, &agenthub_core::platform::PlatformPaths) -> Result<()>,
) -> Result<DesktopProbeSettings> {
    let config = state.config();
    let plan = AgentHubUseCases::with_config(&config, state.platform())
        .settings()?
        .save_probe_settings(request)?;
    let config_path = state.platform().config_file.clone();
    if !config_path.exists() {
        anyhow::bail!("config file not found: {}", config_path.display());
    }
    let response_config =
        settings_service::apply_probe_settings_save_plan(&config_path, &state.db, &config, plan)?;
    state.replace_config(response_config.clone());
    repair_error_monitor(&response_config, state.platform())?;
    probe_settings_with_state(state)
}

#[cfg(test)]
pub(crate) fn test_probe_save_settings_with_state(
    state: &DesktopState,
    request: ProbeSettingsSaveRequest,
) -> Result<DesktopProbeSettings> {
    probe_save_settings_with_state_and_repair(state, request, |_, _| Ok(()))
}

pub(crate) fn probe_action_with_state(
    state: &DesktopState,
    action: probe_service::ProbeAction,
) -> Result<DesktopActionResponse> {
    let plan = probe_service::ProbeUseCases::new(state.platform()).action(action)?;
    let command = action.as_desktop_command();
    let job = &plan.job;
    let binary = state.platform().daemon_binary();
    if !binary.is_file() {
        return Ok(unavailable_action(
            command,
            &format!(
                "Probe action requires local agenthub-webd binary; plan is available but job cannot start: {}",
                binary.display()
            ),
        ));
    }
    let process = probe_service::probe_process_command(
        state.platform(),
        &state.platform().config_file,
        action,
    );
    let job_id = if let Some(group) = job.exclusive_group.as_deref() {
        state
            .jobs
            .start_exclusive_process_job(&job.kind, &job.title, process, group)?
    } else {
        state
            .jobs
            .start_process_job(&job.kind, &job.title, process)?
    };
    Ok(ok_action(
        command,
        "started local Probe job",
        None,
        Some(job_id),
        None,
    ))
}

pub(crate) fn probe_events_with_state(
    state: &DesktopState,
    request: DesktopProbeEventsRequest,
) -> Result<DesktopProbeEventsResponse> {
    let limit = request
        .limit
        .unwrap_or(state.config().probe.recent_limit as u32)
        .clamp(1, 500);
    let events = state
        .db
        .list_probe_events(limit)?
        .into_iter()
        .map(redact_probe_event_for_output)
        .collect();
    Ok(DesktopProbeEventsResponse { events, limit })
}

fn ok_action(
    command: &str,
    message: &str,
    thread_id: Option<String>,
    job_id: Option<String>,
    data: Option<Value>,
) -> DesktopActionResponse {
    job_service::action_ok(command, message, thread_id, job_id, data).into()
}

fn unavailable_action(command: &str, message: &str) -> DesktopActionResponse {
    job_service::action_unavailable(command, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenthub_core::{
        crypto::SecretBox,
        db::PanelDb,
        platform::{PlatformKind, PlatformPaths},
        services::settings::{ProbeNotificationsSavePatch, ProbeSettingsSavePatch},
    };

    fn command_test_state(kind: PlatformKind) -> (tempfile::TempDir, DesktopState, PlatformPaths) {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::for_platform_kind_with_home(kind, temp.path());
        config.paths.data_dir = temp.path().join("data");
        config.paths.db_path = temp.path().join("panel.sqlite");
        config.paths.log_dir = temp.path().join("logs");
        config.codex.home = temp.path().join("codex-home");
        config.codex.workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&config.paths.data_dir).unwrap();
        std::fs::create_dir_all(&config.paths.log_dir).unwrap();
        std::fs::create_dir_all(&config.codex.home).unwrap();
        std::fs::create_dir_all(config.codex.home.join("sessions")).unwrap();
        std::fs::write(config.codex.home.join("session_index.jsonl"), "").unwrap();
        let status = std::process::Command::new("sqlite3")
            .arg(config.codex.home.join("state_5.sqlite"))
            .arg(
                "CREATE TABLE threads(
                    id TEXT PRIMARY KEY,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0,
                    archived_at INTEGER
                );
                CREATE TABLE thread_dynamic_tools(thread_id TEXT NOT NULL);
                CREATE TABLE thread_spawn_edges(parent_thread_id TEXT NOT NULL, child_thread_id TEXT NOT NULL);
                CREATE TABLE agent_job_items(id TEXT PRIMARY KEY, assigned_thread_id TEXT, status TEXT NOT NULL);",
            )
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::create_dir_all(&config.codex.workspace).unwrap();
        let mut platform = PlatformPaths::for_kind_with_home(PlatformKind::Macos, temp.path());
        platform.kind = kind;
        std::fs::create_dir_all(platform.config_file.parent().unwrap()).unwrap();
        std::fs::write(&platform.config_file, toml::to_string(&config).unwrap()).unwrap();
        let db = PanelDb::open_with_secret_box(
            &config.paths.db_path,
            SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let state = DesktopState::new(config, db, platform.clone());
        (temp, state, platform)
    }

    #[test]
    fn probe_settings_save_command_uses_core_plan_for_normalization_and_secret_write() {
        let (_temp, state, _platform) = command_test_state(PlatformKind::Macos);

        let saved = test_probe_save_settings_with_state(
            &state,
            ProbeSettingsSaveRequest {
                probe: Some(ProbeSettingsSavePatch {
                    poll_seconds: Some(1),
                    recent_limit: Some(999),
                    notifications: Some(ProbeNotificationsSavePatch {
                        server_url: Some(" https://api.day.app ".to_string()),
                        group: Some("  ".to_string()),
                        device_key: Some(" nested-key ".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                notifications: Some(ProbeNotificationsSavePatch {
                    device_key: Some(" top-key ".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(saved.probe.poll_seconds, 5);
        assert_eq!(saved.probe.recent_limit, 500);
        assert_eq!(saved.probe.notifications.server_url, "https://api.day.app");
        assert_eq!(saved.probe.notifications.group, "AgentHub");
        assert_eq!(
            state
                .db
                .get_secret_setting_bytes(settings_service::PROBE_BARK_DEVICE_KEY_SETTING)
                .unwrap()
                .as_deref(),
            Some(b"top-key".as_slice())
        );
    }

    #[cfg(unix)]
    #[test]
    fn repeated_full_form_and_key_only_save_preserve_config_inode() {
        use std::os::unix::fs::MetadataExt;
        let (_temp, state, platform) = command_test_state(PlatformKind::Macos);
        let full_form: ProbeSettingsSaveRequest = serde_json::from_value(serde_json::json!({
            "codex": { "home": state.config().codex.home },
            "probe": state.config().probe,
        }))
        .unwrap();
        test_probe_save_settings_with_state(&state, full_form.clone()).unwrap();
        let before = std::fs::metadata(&platform.config_file).unwrap();
        for request in [
            full_form,
            ProbeSettingsSaveRequest::default(),
            ProbeSettingsSaveRequest {
                notifications: Some(ProbeNotificationsSavePatch {
                    device_key: Some("fixture-key-only".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ] {
            test_probe_save_settings_with_state(&state, request).unwrap();
            let after = std::fs::metadata(&platform.config_file).unwrap();
            assert_eq!(before.ino(), after.ino());
            assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        }
        assert_eq!(
            state
                .db
                .get_secret_setting_bytes(settings_service::PROBE_BARK_DEVICE_KEY_SETTING)
                .unwrap()
                .as_deref(),
            Some(b"fixture-key-only".as_slice())
        );
    }

    #[test]
    fn probe_settings_save_command_honors_core_capability_gate() {
        let (_temp, state, _platform) = command_test_state(PlatformKind::Windows);

        let err = test_probe_save_settings_with_state(&state, ProbeSettingsSaveRequest::default())
            .unwrap_err()
            .to_string();

        assert!(err.contains("settings is unavailable on windows"), "{err}");
    }

    #[test]
    fn probe_fixed_process_job_executes_the_core_command_with_literal_arguments() {
        let (_temp, state, platform) = command_test_state(PlatformKind::Macos);
        std::fs::create_dir_all(platform.daemon_binary().parent().unwrap()).unwrap();
        std::fs::write(
            platform.daemon_binary(),
            "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(platform.daemon_binary())
                .unwrap()
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(platform.daemon_binary(), permissions).unwrap();
        }
        assert!(platform.config_file.to_string_lossy().contains(' '));
        let core_plan = probe_service::plan_probe_action(
            state.platform(),
            probe_service::ProbeAction::BarkTest,
            &state.platform().config_file,
        )
        .unwrap();
        let expected_process = probe_service::probe_process_command(
            state.platform(),
            &platform.config_file,
            probe_service::ProbeAction::BarkTest,
        );
        assert_eq!(expected_process.program, platform.daemon_binary());
        assert_eq!(
            expected_process.args,
            vec![
                "--config".into(),
                platform.config_file.as_os_str().to_owned(),
                "probe".into(),
                "bark-test".into(),
            ]
        );

        let response = probe_action_with_state(&state, probe_service::ProbeAction::BarkTest)
            .expect("Probe action should start a fixed job");
        let job_id = response.job_id.expect("job id");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let job = loop {
            let job = state.db.job(&job_id).unwrap().expect("job record");
            if job.status == "succeeded" || job.status == "failed" {
                break job;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Probe process job did not finish in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };

        assert!(response.available);
        assert_eq!(job.status, "succeeded");
        assert_eq!(job.kind, core_plan.job.kind);
        assert_eq!(job.title, core_plan.job.title);
        let expected_output = format!(
            "<--config>\n<{}>\n<probe>\n<bark-test>\n",
            platform.config_file.display()
        );
        assert_eq!(job.output, expected_output);
    }

    #[test]
    fn probe_process_command_preserves_config_path_as_one_argument() {
        let (_temp, _state, platform) = command_test_state(PlatformKind::Macos);
        let config_path = platform.config_file.clone();
        assert!(config_path.to_string_lossy().contains(' '));

        for (action, subcommand) in [
            (probe_service::ProbeAction::BarkTest, "bark-test"),
            (probe_service::ProbeAction::InstallHooks, "hooks-install"),
            (probe_service::ProbeAction::RemoveHooks, "hooks-remove"),
        ] {
            let process = probe_service::probe_process_command(&platform, &config_path, action);
            assert_eq!(process.program, platform.daemon_binary());
            assert_eq!(
                process.args,
                vec![
                    "--config".into(),
                    config_path.as_os_str().to_owned(),
                    "probe".into(),
                    subcommand.into(),
                ]
            );
        }
    }
}
