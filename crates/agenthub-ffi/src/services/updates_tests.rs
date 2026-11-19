use super::*;
use agenthub_core::{
    db::JobRecord,
    platform::{PlatformKind, PlatformPaths},
};

#[test]
fn native_update_commands_are_validated_against_core_plan() {
    let platform = PlatformPaths::for_kind(PlatformKind::Macos);

    let check = native_update_job_plan_for_action(&platform, UpdateAction::Check).unwrap();
    assert_eq!(check.kind, "agenthub_update_check");
    assert!(update_job_id(&check).starts_with("desktop-update-check-"));

    let install = native_update_job_plan_for_action(&platform, UpdateAction::Install).unwrap();
    assert_eq!(install.kind, "agenthub_update_install");
    assert!(update_job_id(&install).starts_with("desktop-update-install-"));
}

#[test]
fn recent_check_job_output_keeps_no_update_status_explicit() {
    let config = agenthub_core::config::Config::for_platform_kind(PlatformKind::Macos);
    let platform = PlatformPaths::for_kind(PlatformKind::Macos);
    let job = job_record(
        "succeeded",
        "checking signed Tauri updater feed\nno signed app update available\n",
    );

    let status = updates::update_status_with_recent_check_job_for_surface(
        &config,
        &platform,
        agenthub_core::services::system::HostSurface::default_for_platform(&platform),
        None,
        None,
        Some(&job),
    );

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
