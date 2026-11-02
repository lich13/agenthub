use super::*;
use axum::{http::StatusCode, response::IntoResponse, routing::post, Router};
#[tokio::test]
async fn transport_only_reads_allowlisted_methods_and_refuses_redirects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new()
        .route(
            &format!("{SERVICE}GetAllCascadeTrajectories"),
            post(|| async { axum::Json(json!({"trajectorySummaries":{}})) }),
        )
        .route(
            &format!("{SERVICE}GetCascadeTrajectory"),
            post(|| async {
                (
                    StatusCode::FOUND,
                    [("location", "http://example.invalid/redirect")],
                )
                    .into_response()
            }),
        );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = client().unwrap();
    assert!(query(
        &client,
        port,
        "fixture-token",
        "GetAllCascadeTrajectories",
        json!({})
    )
    .await
    .is_ok());
    assert!(query(
        &client,
        port,
        "fixture-token",
        "GetCascadeTrajectory",
        json!({})
    )
    .await
    .is_err());
    assert!(
        query(&client, port, "fixture-token", "SendUserMessage", json!({}))
            .await
            .is_err()
    );
    task.abort();
}
#[tokio::test]
async fn unavailable_native_endpoint_returns_a_safe_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let error = query(
        &client().unwrap(),
        port,
        "fixture-token",
        "GetAllCascadeTrajectories",
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Antigravity read unavailable");
}

#[tokio::test]
async fn scan_paginates_steps_and_suppresses_completion_while_subtrajectory_runs() {
    let root_id = "11111111-1111-4111-8111-111111111111";
    let sub_id = "22222222-2222-4222-8222-222222222222";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new()
        .route(
            &format!("{SERVICE}GetCascadeTrajectory"),
            post(move || async move {
                axum::Json(json!({
                    "status": "CASCADE_RUN_STATUS_IDLE",
                    "numTotalSteps": 2,
                    "trajectory": {
                        "cascadeId": root_id,
                        "trajectoryId": "traj-root",
                        "steps": [
                            {
                                "type": "CORTEX_STEP_TYPE_USER_INPUT",
                                "status": "CORTEX_STEP_STATUS_DONE",
                                "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:00Z" }
                            }
                        ],
                        "executorMetadatas": [
                            {
                                "executionId": "exec-1",
                                "lastStepIdx": 1,
                                "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                            }
                        ]
                    }
                }))
            }),
        )
        .route(
            &format!("{SERVICE}GetCascadeTrajectorySteps"),
            post(|axum::Json(body): axum::Json<Value>| async move {
                assert_eq!(body["stepOffset"], json!(1));
                axum::Json(json!({
                    "steps": [
                        {
                            "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                            "status": "CORTEX_STEP_STATUS_DONE",
                            "metadata": {
                                "executionId": "exec-1",
                                "createdAt": "2026-01-01T00:00:01Z",
                                "completedAt": "2026-01-01T00:00:02Z"
                            },
                            "plannerResponse": { "response": "Final paginated reply" }
                        }
                    ]
                }))
            }),
        );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let http = client().unwrap();

    // 1. Subagent is RUNNING -> root scan skips subagent as stream AND suppresses root completion
    let summaries_busy = json!({
        "trajectorySummaries": {
            root_id: {
                "trajectoryId": "traj-root",
                "createdTime": "2026-01-01T00:00:00Z",
                "summary": "Root task",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "trajectoryMetadata": { "rootConversationId": root_id }
            },
            sub_id: {
                "trajectoryId": "traj-sub",
                "createdTime": "2026-01-01T00:00:00Z",
                "summary": "Child research",
                "status": "CASCADE_RUN_STATUS_RUNNING",
                "trajectoryMetadata": {
                    "rootConversationId": root_id,
                    "parentConversationId": root_id,
                    "nestingDepth": 1
                }
            }
        }
    });
    let scan_busy = scan_at(&http, port, "fixture-token", 42, &summaries_busy, None)
        .await
        .unwrap();
    assert_eq!(scan_busy.errors, 0);
    assert_eq!(scan_busy.streams.len(), 1);
    assert!(scan_busy.streams[0].events.is_empty());

    // 2. Subagent finishes -> root scan paginates step 1 and emits completion
    let mut summaries_idle = summaries_busy.clone();
    summaries_idle["trajectorySummaries"][sub_id]["status"] = json!("CASCADE_RUN_STATUS_IDLE");
    let scan_idle = scan_at(&http, port, "fixture-token", 42, &summaries_idle, None)
        .await
        .unwrap();
    assert_eq!(scan_idle.errors, 0);
    assert_eq!(scan_idle.streams.len(), 1);
    assert_eq!(scan_idle.streams[0].events.len(), 1);
    assert_eq!(scan_idle.streams[0].events[0].body, "Final paginated reply");

    task.abort();
}

#[tokio::test]
async fn long_session_scan_fetches_only_current_turn_tail_steps_without_full_trajectory() {
    let root_id = "33333333-3333-4333-8333-333333333333";
    let waiting_id = "44444444-4444-4444-8444-444444444444";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new()
        .route(
            &format!("{SERVICE}GetCascadeTrajectory"),
            post(|| async { StatusCode::PAYLOAD_TOO_LARGE.into_response() }),
        )
        .route(
            &format!("{SERVICE}GetCascadeTrajectorySteps"),
            post(move |axum::Json(body): axum::Json<Value>| async move {
                if body["cascadeId"] == json!(waiting_id) {
                    assert_eq!(body["stepOffset"], json!(1800));
                    return axum::Json(json!({
                        "steps": [
                            {
                                "type": "CORTEX_STEP_TYPE_USER_INPUT",
                                "status": "CORTEX_STEP_STATUS_DONE",
                                "metadata": { "executionId": "exec-w1", "createdAt": "2026-10-29T04:10:00Z" }
                            },
                            {
                                "type": "CORTEX_STEP_TYPE_ASK_QUESTION",
                                "status": "CORTEX_STEP_STATUS_WAITING",
                                "askQuestion": {
                                    "questions": [{ "question": "确认清理范围？" }]
                                },
                                "metadata": { "executionId": "exec-w1", "startedAt": "2026-10-29T04:10:03Z" }
                            }
                        ]
                    }));
                }
                assert_eq!(body["stepOffset"], json!(2305));
                axum::Json(json!({
                    "steps": [
                        {
                            "type": "CORTEX_STEP_TYPE_USER_INPUT",
                            "status": "CORTEX_STEP_STATUS_DONE",
                            "metadata": { "executionId": "exec-98", "createdAt": "2026-10-29T04:00:00Z" }
                        },
                        {
                            "type": "CORTEX_STEP_TYPE_GENERIC",
                            "status": "CORTEX_STEP_STATUS_DONE",
                            "generic": { "name": "read_file" },
                            "metadata": {
                                "executionId": "exec-98",
                                "createdAt": "2026-10-29T04:00:01Z",
                                "completedAt": "2026-10-29T04:00:02Z"
                            }
                        },
                        {
                            "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                            "status": "CORTEX_STEP_STATUS_DONE",
                            "metadata": {
                                "executionId": "exec-99",
                                "createdAt": "2026-10-29T04:00:03Z",
                                "completedAt": "2026-10-29T04:00:05Z"
                            },
                            "plannerResponse": { "response": "Long session completed via incremental steps." }
                        }
                    ]
                }))
            }),
        )
        .route(
            &format!("{SERVICE}GetCascadeTrajectoryExecutorMetadatas"),
            post(move |axum::Json(body): axum::Json<Value>| async move {
                if body["cascadeId"] == json!(waiting_id) {
                    return axum::Json(json!({}));
                }
                let off = body["executorMetadataOffset"].as_u64().unwrap_or(0);
                if off == 0 {
                    axum::Json(json!({
                        "executorMetadata": [
                            {
                                "executionId": "exec-98",
                                "lastStepIdx": 2306,
                                "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                            },
                            {
                                "executionId": "exec-99",
                                "lastStepIdx": 2307,
                                "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                            }
                        ]
                    }))
                } else {
                    axum::Json(json!({}))
                }
            }),
        );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let summaries = json!({
        "trajectorySummaries": {
            root_id: {
                "trajectoryId": "traj-long",
                "createdTime": "2026-10-28T00:00:00Z",
                "lastModifiedTime": "2026-10-29T04:00:05Z",
                "summary": "2300-step long session",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "stepCount": 2308,
                "lastUserInputStepIndex": 2305,
                "latestExecutorTerminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL",
                "trajectoryMetadata": { "rootConversationId": root_id }
            },
            waiting_id: {
                "trajectoryId": "traj-waiting",
                "createdTime": "2026-10-28T00:00:00Z",
                "lastModifiedTime": "2026-10-29T04:10:03Z",
                "summary": "Forked session waiting on first turn without termination reason",
                "status": "CASCADE_RUN_STATUS_WAITING",
                "stepCount": 1802,
                "lastUserInputStepIndex": 1800,
                "trajectoryMetadata": { "rootConversationId": waiting_id }
            }
        }
    });
    let scan = scan_at(
        &client().unwrap(),
        port,
        "fixture-token",
        0,
        &summaries,
        None,
    )
    .await
    .unwrap();
    assert_eq!(scan.errors, 0);
    assert_eq!(scan.streams.len(), 2);

    let completed = scan.streams.iter().find(|s| s.id == root_id).unwrap();
    assert_eq!(completed.record_count, 2308);
    assert_eq!(completed.settled_count, 2308);
    assert_eq!(completed.events.len(), 1);
    assert_eq!(completed.events[0].position, 2308);
    assert_eq!(completed.events[0].turn_id, "exec-99");
    assert_eq!(
        completed.events[0].body,
        "Long session completed via incremental steps."
    );

    let waiting = scan.streams.iter().find(|s| s.id == waiting_id).unwrap();
    assert_eq!(waiting.record_count, 1802);
    assert_eq!(waiting.settled_count, 1801);
    assert_eq!(waiting.events.len(), 1);
    assert_eq!(waiting.events[0].kind, "reply_needed");
    assert_eq!(waiting.events[0].body, "确认清理范围？");
    task.abort();
}

#[test]
fn ps_etime_parser_handles_mm_ss_hh_mm_ss_and_days() {
    assert_eq!(parse_ps_etime_ms("00:45"), Some(45_000));
    assert_eq!(parse_ps_etime_ms(" 02:03:04 "), Some(7_384_000));
    assert_eq!(parse_ps_etime_ms("1-00:00:05"), Some(86_405_000));
    assert_eq!(parse_ps_etime_ms(""), None);
    assert_eq!(parse_ps_etime_ms("00:99"), None);
}

#[test]
fn untracked_child_process_matcher_isolates_current_turn_command_from_other_sessions_and_daemons() {
    let now_ms = 1_800_000_000_000_i64;
    let spawn_ms = now_ms - 30_000; // spawned 30s ago
    let specs = vec![antigravity::UntrackedCommandSpec {
        spawn_ms,
        command_line: "cd /workspace && cargo build --release -p app".into(),
    }];

    // Process table containing:
    // - 100: language_server root
    // - 200: another session's Vite dev server started 10m ago
    // - 201: another session's different command started at the exact same 30s timestamp
    // - 202: daemonized background process started 30s ago
    // - 300: non-descendant process with identical command started 30s ago
    let unrelated_table = "\
  100     1    02:00:00 /Applications/Antigravity.app/Contents/Resources/bin/language_server
  200   100       10:00 pnpm --dir webui run dev
  201   100       00:30 /bin/zsh -c pytest tests/other_suite.py
  202   100       00:30 nohup cargo build --release -p app
  300   999       00:30 cargo build --release -p app
";
    assert!(!ps_table_matches_untracked_commands(
        unrelated_table,
        100,
        &specs,
        now_ms,
    ));

    // Grandchild (shell -> cargo build) spawned inside [spawn_ms - 5s, spawn_ms + 15s] matches!
    let matching_table = format!(
        "{unrelated_table}  205   100       00:29 /bin/zsh -c cd /workspace && cargo build --release -p app\n  206   205       00:28 cargo build --release -p app\n"
    );
    assert!(ps_table_matches_untracked_commands(
        &matching_table,
        100,
        &specs,
        now_ms,
    ));
}

#[tokio::test]
async fn idle_reply_needed_stream_hits_cache_and_executor_metadatas_fetch_incrementally() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let cascade_id = "77777777-7777-4777-8777-777777777777";
    let step_calls = Arc::new(AtomicUsize::new(0));
    let meta_offsets = Arc::new(std::sync::Mutex::new(Vec::<u64>::new()));

    let step_calls_handler = step_calls.clone();
    let meta_offsets_handler = meta_offsets.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new()
        .route(
            &format!("{SERVICE}GetCascadeTrajectorySteps"),
            post(move |axum::Json(body): axum::Json<Value>| {
                let step_calls_handler = step_calls_handler.clone();
                async move {
                    step_calls_handler.fetch_add(1, Ordering::SeqCst);
                    let off = body["stepOffset"].as_u64().unwrap_or(0);
                    if off == 10 {
                        axum::Json(json!({
                            "steps": [
                                {
                                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                                    "status": "CORTEX_STEP_STATUS_DONE",
                                    "metadata": { "executionId": "exec-3", "createdAt": "2026-10-30T01:00:00Z" }
                                },
                                {
                                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                                    "status": "CORTEX_STEP_STATUS_DONE",
                                    "metadata": {
                                        "executionId": "exec-3",
                                        "createdAt": "2026-10-30T01:00:02Z",
                                        "completedAt": "2026-10-30T01:00:04Z"
                                    },
                                    "plannerResponse": { "response": "补丁已准备好。你希望直接部署还是先保留本地构建？" }
                                }
                            ]
                        }))
                    } else if off == 12 {
                        axum::Json(json!({
                            "steps": [
                                {
                                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                                    "status": "CORTEX_STEP_STATUS_DONE",
                                    "metadata": { "executionId": "exec-4", "createdAt": "2026-10-30T01:05:00Z" }
                                },
                                {
                                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                                    "status": "CORTEX_STEP_STATUS_DONE",
                                    "metadata": {
                                        "executionId": "exec-4",
                                        "createdAt": "2026-10-30T01:05:01Z",
                                        "completedAt": "2026-10-30T01:05:03Z"
                                    },
                                    "plannerResponse": { "response": "已完成部署。" }
                                }
                            ]
                        }))
                    } else {
                        axum::Json(json!({}))
                    }
                }
            }),
        )
        .route(
            &format!("{SERVICE}GetCascadeTrajectoryExecutorMetadatas"),
            post(move |axum::Json(body): axum::Json<Value>| {
                let meta_offsets_handler = meta_offsets_handler.clone();
                async move {
                    let off = body["executorMetadataOffset"].as_u64().unwrap_or(0);
                    meta_offsets_handler.lock().unwrap().push(off);
                    let all_four = meta_offsets_handler.lock().unwrap().len() >= 3;
                    let mut all = vec![
                        json!({"executionId": "exec-1", "lastStepIdx": 4, "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"}),
                        json!({"executionId": "exec-2", "lastStepIdx": 9, "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"}),
                        json!({"executionId": "exec-3", "lastStepIdx": 11, "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"}),
                    ];
                    if all_four {
                        all.push(json!({"executionId": "exec-4", "lastStepIdx": 13, "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"}));
                    }
                    let slice = if (off as usize) < all.len() {
                        all[off as usize..].to_vec()
                    } else {
                        Vec::new()
                    };
                    axum::Json(json!({ "executorMetadata": slice }))
                }
            }),
        );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let http = client().unwrap();
    let summaries_turn_1 = json!({
        "trajectorySummaries": {
            cascade_id: {
                "trajectoryId": "traj-inc-1",
                "createdTime": "2026-10-30T00:00:00Z",
                "lastModifiedTime": "2026-10-30T01:00:04Z",
                "summary": "Incremental metadata & reply_needed cache test",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "stepCount": 12,
                "lastUserInputStepIndex": 10,
                "trajectoryMetadata": { "rootConversationId": cascade_id }
            }
        }
    });

    // 1st scan: fetches steps once + executor metadatas from offset 0 -> emits reply_needed.
    let s1 = scan_at(&http, port, "fixture-token", 0, &summaries_turn_1, None)
        .await
        .unwrap();
    assert_eq!(s1.streams[0].events.len(), 1);
    assert_eq!(s1.streams[0].events[0].kind, "reply_needed");
    assert_eq!(step_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*meta_offsets.lock().unwrap(), vec![0, 3]);

    // 2nd scan with unchanged summary: settled idle `reply_needed` MUST hit CACHE (0 RPC calls!).
    let s2 = scan_at(&http, port, "fixture-token", 0, &summaries_turn_1, None)
        .await
        .unwrap();
    assert_eq!(s2.streams[0].events[0].kind, "reply_needed");
    assert_eq!(step_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*meta_offsets.lock().unwrap(), vec![0, 3]);

    // 3rd scan after user answers & turn 2 completes: signature changes, META_CACHE fetches from cached_len - 1 (offset 2, NOT 0!).
    let summaries_turn_2 = json!({
        "trajectorySummaries": {
            cascade_id: {
                "trajectoryId": "traj-inc-1",
                "createdTime": "2026-10-30T00:00:00Z",
                "lastModifiedTime": "2026-10-30T01:05:03Z",
                "summary": "Incremental metadata & reply_needed cache test",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "stepCount": 14,
                "lastUserInputStepIndex": 12,
                "trajectoryMetadata": { "rootConversationId": cascade_id }
            }
        }
    });
    let s3 = scan_at(&http, port, "fixture-token", 0, &summaries_turn_2, None)
        .await
        .unwrap();
    assert_eq!(s3.streams[0].events.len(), 1);
    assert_eq!(s3.streams[0].events[0].kind, "completion");
    assert_eq!(s3.streams[0].events[0].turn_id, "exec-4");
    assert_eq!(*meta_offsets.lock().unwrap(), vec![0, 3, 2, 4]);

    task.abort();
}
