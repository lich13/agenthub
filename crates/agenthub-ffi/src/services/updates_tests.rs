use super::*;
use agenthub_core::{db::JobRecord, platform::PlatformPaths};

#[test]
fn native_update_commands_are_validated_against_core_plan() {
    let platform = PlatformPaths::macos("/fixture/home");

    let check = native_update_job_plan_for_action(&platform, UpdateAction::Check).unwrap();
    assert_eq!(check.kind, "agenthub_update_check");
    assert!(update_job_id(&check).starts_with("desktop-update-check-"));

    let install = native_update_job_plan_for_action(&platform, UpdateAction::Install).unwrap();
    assert_eq!(install.kind, "agenthub_update_install");
    assert!(update_job_id(&install).starts_with("desktop-update-install-"));
}

#[test]
fn recent_check_job_output_keeps_no_update_status_explicit() {
    let platform = PlatformPaths::macos("/fixture/home");
    let job = job_record(
        "succeeded",
        "checking signed app updater feed\nno signed app update available\n",
    );

    let status = updates::update_status_with_recent_check_job(&platform, None, None, Some(&job));

    assert_eq!(status.state, UpdateState::Idle);
    assert_eq!(
        status.latest_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(status.update_available, Some(false));
}

#[test]
fn minisign_verifier_parses_embedded_pubkey_and_rejects_tampered_signature() {
    let pubkey = minisign_verify::PublicKey::decode(UPDATER_PUBKEY_BOX).unwrap();
    assert!(UPDATER_PUBKEY_BOX.contains("C3A11D2DD9BD734A"));
    let _ = pubkey;
    assert!(verify_minisign_archive(b"payload", "bm90LWEtc2ln", "2.1.28").is_err());
}

fn job_record(status: &str, output: &str) -> JobRecord {
    JobRecord {
        id: "job-1".to_string(),
        kind: "agenthub_update_check".to_string(),
        status: status.to_string(),
        title: "AgentHub app update check".to_string(),
        started_at: 1,
        finished_at: Some(2),
        exit_code: Some(0),
        output: output.to_string(),
        error: None,
    }
}

#[test]
fn double_rename_failure_retains_previous_bundle_and_reports_location() {
    let root = tempfile::tempdir().unwrap();
    let backup = root.path().join("saved.app");
    std::fs::create_dir(&backup).unwrap();
    std::fs::write(backup.join("previous"), b"old").unwrap();
    let target = root.path().join("missing-parent/AgentHub.app");
    let error = super::replace_backed_up_bundle(&root.path().join("absent.app"), &target, &backup)
        .unwrap_err()
        .to_string();
    assert!(error.contains("替换应用失败") && error.contains("恢复失败"));
    assert!(error.contains(backup.to_str().unwrap()));
    assert_eq!(std::fs::read(backup.join("previous")).unwrap(), b"old");
}

#[test]
fn desktop_update_status_finds_recent_check_job_past_twenty_five_unrelated_jobs() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let platform = PlatformPaths::macos(&home);
    let db = agenthub_core::db::PanelDb::open_with_secret_box(
        root.path().join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"22".repeat(32)).unwrap(),
    )
    .unwrap();
    let config = agenthub_core::config::Config::for_platform_kind_with_home(
        agenthub_core::platform::PlatformKind::Macos,
        &home,
    );
    let state = DesktopState::new(config, db, platform);

    state
        .db
        .create_job(
            "check-1",
            "agenthub_update_check",
            "AgentHub app update check",
        )
        .unwrap();
    state
        .db
        .append_job_output(
            "check-1",
            "checking signed app updater feed\nno signed app update available\n",
        )
        .unwrap();
    state
        .db
        .finish_job("check-1", "succeeded", Some(0), None)
        .unwrap();

    for idx in 0..30 {
        let jid = format!("unrelated-{idx:02}");
        state
            .db
            .create_job(&jid, "probe_bark_test", "Bark test")
            .unwrap();
        state
            .db
            .finish_job(&jid, "succeeded", Some(0), None)
            .unwrap();
    }

    let status = desktop_update_status_with_state(&state, None, None).unwrap();
    assert_eq!(status.state, UpdateState::Idle);
    assert_eq!(status.update_available, Some(false));
    assert_eq!(
        status.latest_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}
