use agenthub_core::services::system::HostSurface;
use agenthub_core::{
    config::Config,
    jobs::ProcessCommand,
    platform::{PlatformKind, PlatformPaths},
    services::updates::{
        linux_update_job_spec, macos_updater_job_spec, macos_updater_no_update_output,
        macos_updater_update_available_output, update_job_plan_for_surface,
        update_status_for_surface, update_status_with_recent_check_job_for_surface, UpdateAction,
        UpdateExecutionMethod, UpdateFailureCategory, UpdateState, MACOS_UPDATER_CHECKING_OUTPUT,
    },
};

#[test]
fn linux_update_status_uses_fixed_panel_job_executor() {
    let config = Config::for_platform_kind(PlatformKind::Linux);
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);

    let status = update_status_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        Some("v999.0.0"),
        None,
    );

    assert_eq!(status.current_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(status.latest_version.as_deref(), Some("v999.0.0"));
    assert_eq!(status.update_available, Some(true));
    assert_eq!(status.channel, "stable");
    assert_eq!(status.method, UpdateExecutionMethod::LinuxSystemdJob);
    assert_eq!(status.state, UpdateState::Idle);
    assert_eq!(status.failure_category, None);
    assert!(status
        .recommended_action
        .contains("Linux server update job"));
}

#[test]
fn update_status_does_not_downgrade_or_reinstall_same_version() {
    let config = Config::for_platform_kind(PlatformKind::Macos);
    let platform = PlatformPaths::for_kind(PlatformKind::Macos);

    let older = update_status_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        Some("v0.1.105"),
        None,
    );
    let same = update_status_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        Some(env!("CARGO_PKG_VERSION")),
        None,
    );

    assert_eq!(older.update_available, Some(false));
    assert_eq!(same.update_available, Some(false));
    assert!(older.recommended_action.contains("Tauri updater"));
}

#[test]
fn macos_update_status_uses_tauri_updater_without_linux_leakage() {
    let home = temp_dir("agenthub-updates-macos-status");
    std::fs::create_dir_all(&home).unwrap();
    let config = Config::for_platform_kind_with_home(PlatformKind::Macos, &home);
    let platform = PlatformPaths::for_kind_with_home(PlatformKind::Macos, &home);

    let status = update_status_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        Some("v0.1.103"),
        Some("network timeout"),
    );
    let serialized = serde_json::to_string(&status).unwrap();

    assert_eq!(status.method, UpdateExecutionMethod::MacosTauriUpdater);
    assert_eq!(status.state, UpdateState::Failed);
    assert_eq!(
        status.failure_category,
        Some(UpdateFailureCategory::NetworkTlsEof)
    );
    assert!(status.recommended_action.contains("Tauri updater"));
    assert!(!serialized.contains("systemctl"));
    assert!(!serialized.contains("nginx"));
    assert!(!serialized.contains("/opt/agenthub"));
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn update_action_plans_are_shared_and_platform_scoped_without_shell_commands() {
    let linux_platform = PlatformPaths::for_kind(PlatformKind::Linux);

    let start = update_job_plan_for_surface(
        &linux_platform,
        HostSurface::default_for_platform(&linux_platform),
        UpdateAction::Install,
    );
    assert_eq!(start.action, UpdateAction::Install);
    assert_eq!(start.method, UpdateExecutionMethod::LinuxSystemdJob);
    assert!(start.exclusive);

    let mac_home = temp_dir("agenthub-updates-macos-job");
    std::fs::create_dir_all(&mac_home).unwrap();
    let mac_platform = PlatformPaths::for_kind_with_home(PlatformKind::Macos, &mac_home);

    let mac_plan = update_job_plan_for_surface(
        &mac_platform,
        HostSurface::default_for_platform(&mac_platform),
        UpdateAction::Install,
    );
    assert_eq!(mac_plan.method, UpdateExecutionMethod::MacosTauriUpdater);
    assert!(!mac_plan.exclusive);
    std::fs::remove_dir_all(mac_home).unwrap();
}

#[test]
fn linux_update_job_specs_are_planned_in_core_service() {
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);

    let precheck = linux_update_job_spec(update_job_plan_for_surface(
        &platform,
        HostSurface::default_for_platform(&platform),
        UpdateAction::Check,
    ));
    assert_eq!(precheck.kind, "agenthub_update_check");
    assert_eq!(precheck.title, "AgentHub update precheck");
    assert_eq!(
        precheck.command,
        "/usr/local/bin/agenthub-webd-update --precheck"
    );
    assert_eq!(
        precheck.exclusive_group.as_deref(),
        Some("agenthub-webd-update")
    );
    assert_eq!(
        precheck.process_command().unwrap(),
        ProcessCommand::new("/usr/local/bin/agenthub-webd-update", ["--precheck"])
    );

    let install = linux_update_job_spec(update_job_plan_for_surface(
        &platform,
        HostSurface::default_for_platform(&platform),
        UpdateAction::Install,
    ));
    assert_eq!(install.kind, "agenthub_update_install");
    assert_eq!(
        install.command,
        "/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest"
    );
    assert_eq!(
        install.process_command().unwrap(),
        ProcessCommand::new(
            "/usr/local/bin/agenthub-webd-update",
            ["--repo", "lich13/agenthub", "--version", "latest"]
        )
    );

    let prune = linux_update_job_spec(update_job_plan_for_surface(
        &platform,
        HostSurface::default_for_platform(&platform),
        UpdateAction::Prune,
    ));
    assert_eq!(prune.kind, "agenthub_update_prune");
    assert!(prune.command.contains("release update backups"));
    let prune_process = prune.process_command().unwrap();
    assert_eq!(prune_process.program, std::path::PathBuf::from("python3"));
    assert_eq!(prune_process.args.len(), 2);
    assert_eq!(prune_process.args[0].to_string_lossy(), "-c");
    assert_eq!(
        prune_process.args[1].to_string_lossy(),
        agenthub_core::update::PANEL_PRUNE_SCRIPT
    );

    let mac = PlatformPaths::for_kind(PlatformKind::Macos);
    let plan =
        agenthub_core::services::updates::plan_update_action(&mac, UpdateAction::Check).unwrap();
    assert!(plan.linux_job.is_none());
    assert!(plan.macos_job.is_some());
}

#[test]
fn macos_updater_job_specs_and_output_markers_are_core_contracts() {
    let check = macos_updater_job_spec(UpdateAction::Check).unwrap();
    assert_eq!(check.kind, "agenthub_update_check");
    assert_eq!(check.title, "AgentHub app update check");
    assert_eq!(check.initial_output, MACOS_UPDATER_CHECKING_OUTPUT);

    let install = macos_updater_job_spec(UpdateAction::Install).unwrap();
    assert_eq!(install.kind, "agenthub_update_install");
    assert_eq!(install.title, "AgentHub app update install");
    assert_eq!(install.initial_output, MACOS_UPDATER_CHECKING_OUTPUT);

    assert_eq!(
        macos_updater_update_available_output("999.0.0"),
        "signed app update available 999.0.0\n"
    );
    assert_eq!(
        macos_updater_no_update_output(),
        "no signed app update available\n"
    );

    let prune = macos_updater_job_spec(UpdateAction::Prune)
        .unwrap_err()
        .to_string();
    assert!(prune.contains("native updater does not support backup prune"));
}

#[test]
fn recent_macos_updater_check_job_derives_status_in_core() {
    let config = Config::for_platform_kind(PlatformKind::Macos);
    let platform = PlatformPaths::for_kind(PlatformKind::Macos);
    let available_job = job_record(
        "succeeded",
        &format!(
            "{}{}",
            MACOS_UPDATER_CHECKING_OUTPUT,
            macos_updater_update_available_output("999.0.0")
        ),
    );

    let available = update_status_with_recent_check_job_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        None,
        None,
        Some(&available_job),
    );
    assert_eq!(available.latest_version.as_deref(), Some("999.0.0"));
    assert_eq!(available.update_available, Some(true));
    assert_eq!(available.state, UpdateState::Ready);

    let no_update_job = job_record(
        "succeeded",
        &format!(
            "{}{}",
            MACOS_UPDATER_CHECKING_OUTPUT,
            macos_updater_no_update_output()
        ),
    );
    let no_update = update_status_with_recent_check_job_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        None,
        None,
        Some(&no_update_job),
    );
    assert_eq!(
        no_update.latest_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(no_update.update_available, Some(false));
    assert_eq!(no_update.state, UpdateState::Idle);

    let running_job = job_record("running", MACOS_UPDATER_CHECKING_OUTPUT);
    let running = update_status_with_recent_check_job_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        None,
        None,
        Some(&running_job),
    );
    assert_eq!(running.state, UpdateState::Checking);

    let failed_job = job_record("failed", "error: network timeout\n");
    let failed = update_status_with_recent_check_job_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        None,
        None,
        Some(&failed_job),
    );
    assert_eq!(failed.state, UpdateState::Failed);
}

#[test]
fn explicit_update_status_inputs_take_precedence_over_recent_check_job() {
    let config = Config::for_platform_kind(PlatformKind::Macos);
    let platform = PlatformPaths::for_kind(PlatformKind::Macos);
    let job = job_record(
        "succeeded",
        &format!(
            "{}{}",
            MACOS_UPDATER_CHECKING_OUTPUT,
            macos_updater_update_available_output("999.0.0")
        ),
    );

    let status = update_status_with_recent_check_job_for_surface(
        &config,
        &platform,
        HostSurface::default_for_platform(&platform),
        Some("0.1.0"),
        None,
        Some(&job),
    );

    assert_eq!(status.latest_version.as_deref(), Some("0.1.0"));
    assert_ne!(status.latest_version.as_deref(), Some("999.0.0"));
}

fn job_record(status: &str, output: &str) -> agenthub_core::db::JobRecord {
    agenthub_core::db::JobRecord {
        id: "job-a".to_string(),
        kind: "agenthub_update_check".to_string(),
        status: status.to_string(),
        title: "AgentHub app update check".to_string(),
        thread_id: None,
        turn_id: None,
        started_at: 10,
        finished_at: None,
        exit_code: None,
        output: output.to_string(),
        error: None,
    }
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(format!("{label}-{unique}"))
}
