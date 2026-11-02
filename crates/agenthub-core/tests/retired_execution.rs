#[test]
fn retired_task_execution_has_no_core_entrypoints() {
    for (source, forbidden) in [
        (include_str!("../src/jobs.rs"), "pub fn start_codex_job("),
        (
            include_str!("../src/services/use_cases.rs"),
            "pub fn create_job(",
        ),
        (
            include_str!("../src/services/use_cases.rs"),
            "pub fn autosubmit_followup_job(",
        ),
        (
            include_str!("../src/services/jobs.rs"),
            "pub fn enqueue_planned_followup(",
        ),
        (include_str!("../src/lib.rs"), "pub mod claude_code;"),
        (include_str!("../src/services/mod.rs"), "pub mod uploads;"),
    ] {
        assert!(
            !source.contains(forbidden),
            "retired entrypoint remains: {forbidden}"
        );
    }
}
