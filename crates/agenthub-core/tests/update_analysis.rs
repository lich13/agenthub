use agenthub_core::update::{analyze_job_failure, JobFailureCategory};

#[test]
fn categorizes_release_missing_failures() {
    let analysis = analyze_job_failure(
        "panel_update_start",
        "GET /repos/lich13/agenthub/releases/tags/v9.9.9 returned 404 Not Found",
        None,
        Some(1),
    );

    assert_eq!(
        analysis.map(|analysis| analysis.category),
        Some(JobFailureCategory::ReleaseMissing)
    );
}

#[test]
fn categorizes_sha256_mismatch_failures() {
    let analysis = analyze_job_failure(
        "panel_update_start",
        "expected sha256 abc but got def; checksum mismatch",
        None,
        Some(1),
    );

    assert_eq!(
        analysis.map(|analysis| analysis.category),
        Some(JobFailureCategory::DownloadSha256Mismatch)
    );
}

#[test]
fn categorizes_permission_denied_sudo_failures_before_unknown() {
    let analysis = analyze_job_failure(
        "update_start",
        "sudo: a password is required\npermission denied",
        None,
        Some(1),
    );

    assert_eq!(
        analysis.map(|analysis| analysis.category),
        Some(JobFailureCategory::PermissionDeniedSudo)
    );
}

#[test]
fn categorizes_read_only_file_system_before_network_tls_eof() {
    let analysis = analyze_job_failure(
        "update_start",
        "curl: (56) unexpected EOF while writing /root/.codex/install.lock: Read-only file system (os error 30)",
        Some("failed to create /root/.codex/install.lock: EROFS"),
        Some(1),
    )
    .expect("failed jobs should be analyzed");

    assert_eq!(analysis.category, JobFailureCategory::ReadOnlyFileSystem);
    assert!(analysis.explanation.contains("read-only"));
}

#[test]
fn returns_none_for_successful_jobs() {
    assert!(analyze_job_failure("panel_update_start", "update complete", None, Some(0),).is_none());
}
