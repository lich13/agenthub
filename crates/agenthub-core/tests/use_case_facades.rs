use agenthub_core::{
    config::Config,
    db::JobRecord,
    platform::{PlatformKind, PlatformPaths},
    services::{
        probe::{ProbeAction, ProbeExecutionKind},
        settings::{
            ProbeNotificationsSavePatch, ProbeSecretState, ProbeSettingsSavePatch,
            ProbeSettingsSaveRequest,
        },
        system::Capability,
        use_cases::AgentHubUseCases,
    },
};

#[test]
fn read_and_history_use_cases_expose_complete_adapter_transaction_plans() {
    let linux_platform = PlatformPaths::for_kind(PlatformKind::Linux);
    let use_cases = AgentHubUseCases::new(&linux_platform);

    let list = use_cases
        .threads()
        .list_read(agenthub_core::services::threads::ThreadsQuery {
            archived: Some(true),
            q: Some(" build ".to_string()),
            limit: Some(25),
        })
        .unwrap();
    assert_eq!(list.list.required_capability, Capability::Threads);
    assert_eq!(list.list.query.archived, Some(true));
    assert_eq!(list.list.query.q.as_deref(), Some("build"));

    let detail = use_cases
        .threads()
        .detail_read(agenthub_core::services::threads::ThreadDetailRequest {
            id: " thread-a ".to_string(),
            limit: Some(10),
            full: None,
            before: None,
        })
        .unwrap();
    assert_eq!(detail.detail.thread_id, "thread-a");

    let job_list = use_cases.jobs().list(Some(500)).unwrap();
    assert_eq!(job_list.required_capability, Capability::JobHistory);
    assert_eq!(job_list.limit, 200);

    let job_detail = use_cases.jobs().detail(" job-a ").unwrap();
    assert_eq!(job_detail.required_capability, Capability::JobHistory);
    assert_eq!(job_detail.job_id, "job-a");

    let failed = use_cases.jobs().response(JobRecord {
        id: "job-a".to_string(),
        kind: "codex".to_string(),
        status: "failed".to_string(),
        title: "Codex".to_string(),
        thread_id: Some("thread-a".to_string()),
        turn_id: None,
        started_at: 1,
        finished_at: Some(2),
        exit_code: Some(1),
        output: "auth failed".to_string(),
        error: Some("401 Unauthorized".to_string()),
    });
    assert_eq!(failed.job.id, "job-a");
    assert!(failed.failure_analysis.is_some());
    assert!(failed
        .analysis
        .as_deref()
        .is_some_and(|value| !value.is_empty()));
    assert!(failed
        .explanation
        .as_deref()
        .is_some_and(|value| !value.is_empty()));

    let listed = vec![
        JobRecord {
            id: "job-a".to_string(),
            kind: "codex".to_string(),
            status: "failed".to_string(),
            title: "Codex".to_string(),
            thread_id: Some("thread-a".to_string()),
            turn_id: None,
            started_at: 1,
            finished_at: Some(2),
            exit_code: Some(1),
            output: "auth failed".to_string(),
            error: Some("401 Unauthorized".to_string()),
        },
        JobRecord {
            id: "job-b".to_string(),
            kind: "agenthub_update_check".to_string(),
            status: "succeeded".to_string(),
            title: "Update".to_string(),
            thread_id: None,
            turn_id: None,
            started_at: 3,
            finished_at: Some(4),
            exit_code: Some(0),
            output: "ok".to_string(),
            error: None,
        },
    ]
    .into_iter()
    .map(|job| use_cases.jobs().response(job))
    .collect::<Vec<_>>();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].job.id, "job-a");
    assert!(listed[0].failure_analysis.is_some());
    assert_eq!(listed[1].job.id, "job-b");
    assert!(listed[1].failure_analysis.is_none());
    assert!(listed[1].analysis.is_none());
    assert!(listed[1].explanation.is_none());

    let detail_response = use_cases.jobs().response(JobRecord {
        id: "job-c".to_string(),
        kind: "codex".to_string(),
        status: "succeeded".to_string(),
        title: "Codex".to_string(),
        thread_id: Some("thread-c".to_string()),
        turn_id: None,
        started_at: 5,
        finished_at: Some(6),
        exit_code: Some(0),
        output: "done".to_string(),
        error: None,
    });
    assert_eq!(detail_response.job.id, "job-c");
}

#[test]
fn operational_use_cases_expose_probe_settings_updates_and_system_plans() {
    let linux_config = Config::for_platform_kind(PlatformKind::Linux);
    let linux_platform = PlatformPaths::for_kind(PlatformKind::Linux);
    let linux = AgentHubUseCases::with_config(&linux_config, &linux_platform);

    let settings_view = linux
        .settings()
        .unwrap()
        .probe_settings_view(ProbeSecretState::Configured)
        .unwrap();
    assert_eq!(settings_view.required_capability, Capability::Settings);
    assert!(settings_view.settings.notifications.device_key_configured);

    let settings_save = linux
        .settings()
        .unwrap()
        .save_probe_settings(ProbeSettingsSaveRequest {
            probe: Some(ProbeSettingsSavePatch {
                notifications: Some(ProbeNotificationsSavePatch {
                    device_key: Some(" top-level-bark-key ".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(settings_save.required_capability, Capability::Settings);
    assert_eq!(
        settings_save.bark_device_key.as_deref(),
        Some("top-level-bark-key")
    );
    let serialized_save = serde_json::to_string(&settings_save).unwrap();
    assert!(serialized_save.contains("[configured]"));
    assert!(!serialized_save.contains("top-level-bark-key"));

    let bark = linux
        .probe()
        .unwrap()
        .action_with_device_key(ProbeAction::BarkTest, true)
        .unwrap();
    assert_eq!(bark.required_capability, Capability::Probe);
    assert_eq!(bark.execution, ProbeExecutionKind::FixedShellJob);
    assert_eq!(bark.diagnostic_plan.unwrap().kind, "bark-test".to_string());

    let capabilities = linux.system().unwrap().capabilities();
    assert!(capabilities.probe);
    assert!(capabilities.prune_backups);
}

#[test]
fn config_backed_use_cases_report_missing_config_in_core() {
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);
    let use_cases = AgentHubUseCases::new(&platform);

    for result in [
        use_cases.settings().map(|_| ()),
        use_cases.probe().map(|_| ()),
        use_cases.system().map(|_| ()),
    ] {
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("config is required"));
    }
}
