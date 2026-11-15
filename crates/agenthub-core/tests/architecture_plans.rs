use agenthub_core::services::probe::{probe_status_snapshot_view, ProbeSnapshotStatus};
use serde_json::json;

#[test]
fn core_probe_views_own_snapshot_metadata() {
    let snapshot = probe_status_snapshot_view(
        json!({"label": "Probe"}),
        42,
        true,
        ProbeSnapshotStatus::Cached,
    );
    assert_eq!(snapshot["snapshot_age_seconds"], 42);
    assert_eq!(snapshot["is_refreshing"], true);
    assert_eq!(snapshot["snapshot_status"], "cached");
}
