use super::*;
use serde_json::json;
const ID: &str = "11111111-1111-4111-8111-111111111111";
fn fixture() -> (Value, Value) {
    let summary = json!({"trajectoryId":"fixture-trajectory","createdTime":"2026-01-01T00:00:00Z","summary":"Fixture","trajectoryMetadata":{"rootConversationId":ID}});
    let response = json!({"status":"CASCADE_RUN_STATUS_IDLE","numTotalSteps":2,"trajectory":{"cascadeId":ID,"trajectoryId":"fixture-trajectory","steps":[
        {"type":"CORTEX_STEP_TYPE_USER_INPUT","status":"CORTEX_STEP_STATUS_DONE","metadata":{"executionId":"execution-1","createdAt":"2026-01-01T00:00:00Z"}},
        {"type":"CORTEX_STEP_TYPE_PLANNER_RESPONSE","status":"CORTEX_STEP_STATUS_DONE","metadata":{"executionId":"execution-1","createdAt":"2026-01-01T00:00:01Z","completedAt":"2026-01-01T00:00:02Z"},"plannerResponse":{"response":"Fixture completed","thinking":"private reasoning"}}
    ],"executorMetadatas":[{"executionId":"execution-1","lastStepIdx":1,"terminationReason":"EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"}]}});
    (summary, response)
}
#[test]
fn main_completion_uses_explicit_execution_terminal_and_visible_response() {
    let (s, mut r) = fixture();
    let scan = snapshot(ID, &s, &r).unwrap();
    assert_eq!(scan.events.len(), 1);
    assert_eq!(scan.events[0].body, "Fixture completed");
    assert_eq!(scan.settled_count, 2);

    // Active cascade status suppresses premature completion and holds settled_count
    r["status"] = json!("CASCADE_RUN_STATUS_BUSY");
    let busy = snapshot(ID, &s, &r).unwrap();
    assert!(busy.events.is_empty());
    assert_eq!(busy.settled_count, 0);
    r["status"] = json!("CASCADE_RUN_STATUS_IDLE");

    // Active child subtrajectory suppresses premature completion
    let sub_busy = snapshot_with_subagents(ID, &s, &r, true).unwrap();
    assert!(sub_busy.events.is_empty());
    assert_eq!(sub_busy.settled_count, 0);

    for reason in [
        "EXECUTOR_TERMINATION_REASON_USER_CANCELED",
        "EXECUTOR_TERMINATION_REASON_EARLY_CONTINUE",
        "EXECUTOR_TERMINATION_REASON_UNSPECIFIED",
        "unknown",
    ] {
        r["trajectory"]["executorMetadatas"][0]["terminationReason"] = json!(reason);
        assert!(snapshot(ID, &s, &r).unwrap().events.is_empty());
    }
}

#[test]
fn background_tasks_and_subagents_delay_completion_until_reactive_wakeup_finishes() {
    let sub_id = "22222222-2222-4222-8222-222222222222";
    let task_id = format!("{ID}/task-101");
    let summary = json!({
        "trajectoryId": "fixture-trajectory",
        "createdTime": "2026-01-01T00:00:00Z",
        "summary": "Long background workflow",
        "status": "CASCADE_RUN_STATUS_IDLE",
        "trajectoryMetadata": { "rootConversationId": ID }
    });

    // Phase 1: Executor 1 launched a background task + subagent and ended with NO_TOOL_CALL while waiting
    let mut response = json!({
        "status": "CASCADE_RUN_STATUS_IDLE",
        "numTotalSteps": 4,
        "trajectory": {
            "cascadeId": ID,
            "trajectoryId": "fixture-trajectory",
            "steps": [
                {
                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:00Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_GENERIC",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "taskDetails": { "id": task_id, "movedToBackgroundAt": "2026-01-01T00:00:01Z" },
                    "generic": { "name": "run_command", "arguments": "{\"IsDaemon\":false}" },
                    "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:01Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_GENERIC",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "generic": {
                        "name": "invoke_subagent",
                        "result": { "payload": { "results": [{ "conversationId": sub_id }] } }
                    },
                    "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:01Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": "exec-1",
                        "createdAt": "2026-01-01T00:00:02Z",
                        "completedAt": "2026-01-01T00:00:03Z"
                    },
                    "plannerResponse": { "response": "Waiting for background command and subagent..." }
                }
            ],
            "executorMetadatas": [
                {
                    "executionId": "exec-1",
                    "lastStepIdx": 3,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                }
            ]
        }
    });
    let mid = snapshot(ID, &summary, &response).unwrap();
    assert!(
        mid.events.is_empty(),
        "must not notify while waiting on background task/subagent"
    );
    assert_eq!(mid.settled_count, 0);

    // Phase 2: Background task woke up via SYSTEM_MESSAGE, but subagent is still unsettled
    response["trajectory"]["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type": "CORTEX_STEP_TYPE_SYSTEM_MESSAGE",
            "status": "CORTEX_STEP_STATUS_DONE",
            "systemMessage": { "agentMessage": { "sender": task_id } },
            "metadata": { "executionId": "exec-2", "createdAt": "2026-01-01T00:00:10Z" }
        }));
    response["trajectory"]["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
            "status": "CORTEX_STEP_STATUS_DONE",
            "metadata": {
                "executionId": "exec-2",
                "createdAt": "2026-01-01T00:00:11Z",
                "completedAt": "2026-01-01T00:00:12Z"
            },
            "plannerResponse": { "response": "Command done, still waiting for subagent..." }
        }));
    response["trajectory"]["executorMetadatas"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "executionId": "exec-2",
            "lastStepIdx": 5,
            "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
        }));
    response["numTotalSteps"] = json!(6);
    let mid2 = snapshot(ID, &summary, &response).unwrap();
    assert!(
        mid2.events.is_empty(),
        "must not notify while subagent wakeup is still pending"
    );
    assert_eq!(mid2.settled_count, 4);

    // Phase 3: Subagent delivers SYSTEM_MESSAGE and final executor completes cleanly
    response["trajectory"]["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type": "CORTEX_STEP_TYPE_SYSTEM_MESSAGE",
            "status": "CORTEX_STEP_STATUS_DONE",
            "systemMessage": { "agentMessage": { "sender": sub_id } },
            "metadata": { "executionId": "exec-3", "createdAt": "2026-01-01T00:00:20Z" }
        }));
    response["trajectory"]["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
            "status": "CORTEX_STEP_STATUS_DONE",
            "metadata": {
                "executionId": "exec-3",
                "createdAt": "2026-01-01T00:00:21Z",
                "completedAt": "2026-01-01T00:00:22Z"
            },
            "plannerResponse": { "response": "All tasks and subagents finished!" }
        }));
    response["trajectory"]["executorMetadatas"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "executionId": "exec-3",
            "lastStepIdx": 7,
            "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
        }));
    response["numTotalSteps"] = json!(8);
    let done = snapshot(ID, &summary, &response).unwrap();
    assert_eq!(
        done.events.len(),
        1,
        "must emit exactly one final completion event"
    );
    assert_eq!(done.events[0].turn_id, "exec-3");
    assert_eq!(done.events[0].position, 8);
    assert_eq!(done.events[0].body, "All tasks and subagents finished!");
    assert_eq!(done.settled_count, 8);
}
#[test]
fn failure_is_obsolete_after_resume_and_tool_errors_are_not_terminal() {
    let (s, mut r) = fixture();
    r["trajectory"]["executorMetadatas"][0]["terminationReason"] =
        json!("EXECUTOR_TERMINATION_REASON_ERROR");
    assert_eq!(snapshot(ID, &s, &r).unwrap().events[0].kind, "failure");
    r["trajectory"]["executorMetadatas"]
        .as_array_mut()
        .unwrap()
        .push(json!({"executionId":"execution-2"}));
    assert!(snapshot(ID, &s, &r).unwrap().events.is_empty());
    r["trajectory"]["executorMetadatas"][0]["terminationReason"] = Value::Null;
    r["trajectory"]["steps"][1]["status"] = json!("CORTEX_STEP_STATUS_ERROR");
    assert!(snapshot(ID, &s, &r).unwrap().events.is_empty());
}
#[test]
fn waiting_requires_native_interaction_and_clears_after_answer() {
    let (s, mut r) = fixture();
    r["trajectory"]["executorMetadatas"][0]["terminationReason"] = Value::Null;
    let step = &mut r["trajectory"]["steps"][1];
    step["status"] = json!("CORTEX_STEP_STATUS_WAITING");
    assert!(snapshot(ID, &s, &r).unwrap().events.is_empty());
    r["trajectory"]["steps"][1]["requestedInteraction"] =
        json!({"askQuestion":{"questions":[{"question":"Continue?"}]}});
    let before = snapshot(ID, &s, &r).unwrap();
    assert_eq!(before.events[0].kind, "reply_needed");
    assert_eq!(before.settled_count, 0);
    r["trajectory"]["steps"][1]["status"] = json!("CORTEX_STEP_STATUS_DONE");
    assert!(snapshot(ID, &s, &r).unwrap().events.is_empty());
}
#[test]
fn permission_request_identity_changes_and_generating_tool_does_not_notify() {
    let (s, mut r) = fixture();
    r["trajectory"]["executorMetadatas"][0]["terminationReason"] = Value::Null;
    r["trajectory"]["steps"][1]["requestedInteraction"] =
        json!({"permission":{"reason":"fixture approval"}});
    r["trajectory"]["steps"][1]["status"] = json!("CORTEX_STEP_STATUS_RUNNING");
    assert!(snapshot(ID, &s, &r).unwrap().events.is_empty());
    r["trajectory"]["steps"][1]["status"] = json!("CORTEX_STEP_STATUS_WAITING");
    let a = snapshot(ID, &s, &r).unwrap();
    assert_eq!(a.events[0].kind, "authorization");
    r["trajectory"]["steps"][1]["requestedInteraction"]["permission"]["reason"] =
        json!("different fixture request");
    let b = snapshot(ID, &s, &r).unwrap();
    assert_ne!(a.event_key(&a.events[0]), b.event_key(&b.events[0]));
}
#[test]
fn mismatched_identity_subagents_and_incomplete_snapshots_are_rejected() {
    let (mut s, mut r) = fixture();
    r["numTotalSteps"] = json!(3);
    assert!(snapshot(ID, &s, &r).is_err());
    r["numTotalSteps"] = json!(2);
    s["trajectoryMetadata"]["rootConversationId"] = json!("parent");
    assert!(snapshot(ID, &s, &r).is_err());
    s["trajectoryMetadata"]["rootConversationId"] = json!(ID);
    r["trajectory"]["cascadeId"] = json!("other");
    assert!(snapshot(ID, &s, &r).is_err());
}

#[test]
fn unknown_origin_and_waiting_interaction_are_reported_as_unreadable() {
    let (mut s, mut r) = fixture();
    s["trajectoryMetadata"] = Value::Null;
    assert!(snapshot(ID, &s, &r).is_err());
    s["trajectoryMetadata"] = json!({"rootConversationId":ID});
    r["trajectory"]["steps"][1]["status"] = json!("CORTEX_STEP_STATUS_WAITING");
    r["trajectory"]["steps"][1]["requestedInteraction"] = json!({"unknown":{}});
    assert!(snapshot(ID, &s, &r).is_err());
}

#[test]
fn fork_inherited_executions_never_become_new_completion_events() {
    let (s, mut r) = fixture();
    for step in r["trajectory"]["steps"].as_array_mut().unwrap() {
        step["metadata"]["sourceTrajectoryStepInfo"] =
            json!({"cascadeId":"original-cascade","trajectoryId":"original-trajectory"});
    }
    assert!(snapshot(ID, &s, &r).unwrap().events.is_empty());
}

#[test]
fn silent_subagents_and_background_tasks_without_system_message_eventually_deliver_completion() {
    let sub_id = "22222222-2222-4222-8222-222222222222";
    let task_id = format!("{ID}/task-202");
    let summary = json!({
        "trajectoryId": "fixture-trajectory",
        "createdTime": "2026-01-01T00:00:00Z",
        "summary": "Silent subagent and task",
        "status": "CASCADE_RUN_STATUS_IDLE",
        "trajectoryMetadata": { "rootConversationId": ID }
    });
    // Base timestamp: 2026-01-01T00:00:10Z = 1_767_225_610_000 ms
    let base_ms = native_probe::timestamp(&json!("2026-01-01T00:00:10Z")).unwrap();
    let response = json!({
        "status": "CASCADE_RUN_STATUS_IDLE",
        "stepOffset": 2300,
        "numTotalSteps": 2304,
        "trajectory": {
            "cascadeId": ID,
            "trajectoryId": "fixture-trajectory",
            "steps": [
                {
                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": { "executionId": "exec-tail", "createdAt": "2026-01-01T00:00:00Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_GENERIC",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "taskDetails": {
                        "id": task_id,
                        "movedToBackgroundAt": "2026-01-01T00:00:02Z",
                        "logUri": "file:///tmp/task-202.log"
                    },
                    "generic": { "name": "run_command", "arguments": "{\"IsDaemon\":false}" },
                    "metadata": { "executionId": "exec-tail", "createdAt": "2026-01-01T00:00:02Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_GENERIC",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "generic": {
                        "name": "send_message",
                        "arguments": format!("{{\"Recipient\":\"{sub_id}\",\"Message\":\"ping\"}}")
                    },
                    "metadata": { "executionId": "exec-tail", "createdAt": "2026-01-01T00:00:05Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": "exec-tail",
                        "createdAt": "2026-01-01T00:00:09Z",
                        "completedAt": "2026-01-01T00:00:10Z"
                    },
                    "plannerResponse": { "response": "Completed after subagent & task exit." }
                }
            ],
            "executorMetadatas": [
                {
                    "executionId": "exec-tail",
                    "lastStepIdx": 2303,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                }
            ]
        }
    });

    // 1. While subagent or task is still active (even 2 minutes later), never fire prematurely
    let mut ctx = TrajectoryLivenessContext {
        has_active_subagents: false,
        has_active_child_processes: false,
        now_ms: Some(base_ms + 120_000),
        subagents: BTreeMap::from([(
            sub_id.into(),
            SubagentLiveness {
                active: true,
                last_modified_ms: Some(base_ms + 5_000),
            },
        )]),
        tasks: BTreeMap::from([(
            task_id.clone(),
            TaskLiveness {
                active: false,
                last_activity_ms: Some(base_ms + 4_000),
            },
        )]),
    };
    assert!(snapshot_with_context(ID, &summary, &response, &ctx)
        .unwrap()
        .events
        .is_empty());

    // 2. Both subagent and task become idle at base_ms + 5_000, inside 15s grace window -> hold
    ctx.subagents.get_mut(sub_id).unwrap().active = false;
    ctx.now_ms = Some(base_ms + 5_000 + BACKGROUND_HANDOFF_GRACE_MS - 1);
    let within_grace = snapshot_with_context(ID, &summary, &response, &ctx).unwrap();
    assert!(within_grace.events.is_empty());
    assert_eq!(within_grace.settled_count, 2300);

    // 3. Past 15s grace window without SYSTEM_MESSAGE -> guaranteed completion delivery!
    ctx.now_ms = Some(base_ms + 5_000 + BACKGROUND_HANDOFF_GRACE_MS);
    let settled = snapshot_with_context(ID, &summary, &response, &ctx).unwrap();
    assert_eq!(settled.record_count, 2304);
    assert_eq!(settled.settled_count, 2304);
    assert_eq!(settled.events.len(), 1);
    assert_eq!(settled.events[0].kind, "completion");
    assert_eq!(settled.events[0].position, 2304);
    assert_eq!(settled.events[0].turn_id, "exec-tail");
    assert_eq!(
        settled.events[0].body,
        "Completed after subagent & task exit."
    );
}

#[test]
fn multi_executor_gap_and_non_terminal_tool_steps_never_fire_early() {
    let summary = json!({
        "trajectoryId": "fixture-trajectory",
        "createdTime": "2026-01-01T00:00:00Z",
        "summary": "Multi-executor turn",
        "status": "CASCADE_RUN_STATUS_IDLE",
        "trajectoryMetadata": { "rootConversationId": ID }
    });
    // Real inter-executor snapshot: exec-1 ended on a tool-calling PLANNER_RESPONSE + GENERIC step,
    // and exec-2 just began or lastStepIdx in metadata has not yet caught up to step 2503.
    let mut response = json!({
        "status": "CASCADE_RUN_STATUS_IDLE",
        "stepOffset": 2500,
        "numTotalSteps": 2504,
        "trajectory": {
            "cascadeId": ID,
            "trajectoryId": "fixture-trajectory",
            "steps": [
                {
                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:00Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": "exec-1",
                        "createdAt": "2026-01-01T00:00:01Z",
                        "completedAt": "2026-01-01T00:00:02Z"
                    },
                    "plannerResponse": {
                        "response": "Checking workspace...",
                        "toolCalls": [{ "id": "call-1", "name": "read_file" }]
                    }
                },
                {
                    "type": "CORTEX_STEP_TYPE_GENERIC",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "generic": { "name": "read_file" },
                    "metadata": {
                        "executionId": "exec-1",
                        "createdAt": "2026-01-01T00:00:02Z",
                        "completedAt": "2026-01-01T00:00:03Z"
                    }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": "exec-2",
                        "createdAt": "2026-01-01T00:00:04Z",
                        "completedAt": "2026-01-01T00:00:05Z"
                    },
                    "plannerResponse": {
                        "response": "Running next tool...",
                        "toolCalls": [{ "id": "call-2", "name": "run_command" }]
                    }
                }
            ],
            "executorMetadatas": [
                {
                    "executionId": "exec-1",
                    "lastStepIdx": 2502,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                },
                {
                    "executionId": "exec-2",
                    "lastStepIdx": 2503,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                }
            ]
        }
    });

    // Even if summary momentarily looks IDLE, last step has toolCalls -> not a terminal NO_TOOL_CALL!
    let gap = snapshot(ID, &summary, &response).unwrap();
    assert!(gap.events.is_empty());
    assert_eq!(gap.settled_count, 2503);

    // Once exec-3 finishes cleanly with a tool-free PLANNER_RESPONSE and matching lastStepIdx, it fires.
    response["trajectory"]["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
            "status": "CORTEX_STEP_STATUS_DONE",
            "metadata": {
                "executionId": "exec-3",
                "createdAt": "2026-01-01T00:00:06Z",
                "completedAt": "2026-01-01T00:00:07Z"
            },
            "plannerResponse": { "response": "All multi-executor steps done." }
        }));
    response["numTotalSteps"] = json!(2505);
    // Mismatched lastStepIdx (2503 vs actual 2504) holds notification
    response["trajectory"]["executorMetadatas"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "executionId": "exec-3",
            "lastStepIdx": 2503,
            "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
        }));
    let stale_meta = snapshot(ID, &summary, &response).unwrap();
    assert!(stale_meta.events.is_empty());

    // Accurate native lastStepIdx == 2504 delivers completion
    response["trajectory"]["executorMetadatas"][2]["lastStepIdx"] = json!(2504);
    let done = snapshot(ID, &summary, &response).unwrap();
    assert_eq!(done.events.len(), 1);
    assert_eq!(done.events[0].kind, "completion");
    assert_eq!(done.events[0].turn_id, "exec-3");
    assert_eq!(done.settled_count, 2505);
}

#[test]
fn untracked_background_task_respects_active_child_processes_past_60s_quiescence() {
    let task_id = format!("{ID}/task-long-build");
    let summary = json!({
        "trajectoryId": "fixture-trajectory",
        "createdTime": "2026-01-01T00:00:00Z",
        "summary": "Silent cargo build",
        "status": "CASCADE_RUN_STATUS_IDLE",
        "trajectoryMetadata": { "rootConversationId": ID }
    });
    let base_ms = native_probe::timestamp(&json!("2026-01-01T00:00:10Z")).unwrap();
    let response = json!({
        "status": "CASCADE_RUN_STATUS_IDLE",
        "numTotalSteps": 3,
        "trajectory": {
            "cascadeId": ID,
            "trajectoryId": "fixture-trajectory",
            "steps": [
                {
                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:00Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_GENERIC",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "taskDetails": { "id": task_id, "movedToBackgroundAt": "2026-01-01T00:00:05Z" },
                    "generic": { "name": "run_command", "arguments": "{\"CommandLine\":\"cargo build --release\",\"IsDaemon\":false}" },
                    "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:05Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": "exec-1",
                        "createdAt": "2026-01-01T00:00:09Z",
                        "completedAt": "2026-01-01T00:00:10Z"
                    },
                    "plannerResponse": { "response": "Build started in background." }
                }
            ],
            "executorMetadatas": [
                {
                    "executionId": "exec-1",
                    "lastStepIdx": 2,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                }
            ]
        }
    });
    let steps = response["trajectory"]["steps"].as_array().unwrap();
    let specs = collect_untracked_task_commands(ID, "fixture-trajectory", steps, &BTreeMap::new());
    assert_eq!(
        specs,
        vec![UntrackedCommandSpec {
            spawn_ms: base_ms - 5_000,
            command_line: "cargo build --release".into(),
        }]
    );

    // Tasks without CommandLine, missing timestamps, or IsDaemon=true never emit a zero-timestamp fallback.
    let untracked_edge_steps = vec![
        json!({
            "type": "CORTEX_STEP_TYPE_USER_INPUT",
            "status": "CORTEX_STEP_STATUS_DONE"
        }),
        json!({
            "type": "CORTEX_STEP_TYPE_GENERIC",
            "status": "CORTEX_STEP_STATUS_DONE",
            "taskDetails": { "id": "daemon-1", "movedToBackgroundAt": "2026-01-01T00:00:05Z" },
            "generic": { "name": "run_command", "arguments": "{\"CommandLine\":\"pnpm dev\",\"IsDaemon\":true}" }
        }),
        json!({
            "type": "CORTEX_STEP_TYPE_GENERIC",
            "status": "CORTEX_STEP_STATUS_DONE",
            "taskDetails": { "id": "missing-ts" },
            "generic": { "name": "run_command", "arguments": "{\"CommandLine\":\"cargo check\",\"IsDaemon\":false}" }
        }),
    ];
    assert!(collect_untracked_task_commands(
        ID,
        "fixture-trajectory",
        &untracked_edge_steps,
        &BTreeMap::new(),
    )
    .is_empty());

    // Within 60s cap (30s later), ps matching holds completion
    let mut ctx = TrajectoryLivenessContext {
        has_active_child_processes: true,
        now_ms: Some(base_ms + 30_000),
        ..Default::default()
    };
    let held = snapshot_with_context(ID, &summary, &response, &ctx).unwrap();
    assert!(held.events.is_empty());
    assert_eq!(held.settled_count, 0);

    // At 60s cap, ps matching can no longer suppress NO_TOOL_CALL completion indefinitely
    ctx.now_ms = Some(base_ms + UNTRACKED_TASK_QUIESCENCE_MS);
    let released = snapshot_with_context(ID, &summary, &response, &ctx).unwrap();
    assert_eq!(released.events.len(), 1);
    assert_eq!(released.events[0].kind, "completion");
}

#[test]
fn mid_turn_plan_feedback_plaintext_questions_and_first_turn_waiting_emit_reply_needed() {
    let summary = json!({
        "trajectoryId": "fixture-trajectory",
        "createdTime": "2026-01-01T00:00:00Z",
        "lastModifiedTime": "2026-01-01T00:00:15Z",
        "summary": "Interactive planning thread",
        "status": "CASCADE_RUN_STATUS_IDLE",
        "trajectoryMetadata": { "rootConversationId": ID }
    });

    // Case 1: First-turn WAITING before any executor terminates (empty executorMetadatas),
    // top-level CORTEX_STEP_TYPE_ASK_QUESTION without requestedInteraction and without createdAt.
    let first_turn_waiting = json!({
        "status": "CASCADE_RUN_STATUS_WAITING",
        "numTotalSteps": 2,
        "trajectory": {
            "cascadeId": ID,
            "trajectoryId": "fixture-trajectory",
            "steps": [
                {
                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": { "executionId": "exec-0", "createdAt": "2026-01-01T00:00:00Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_ASK_QUESTION",
                    "status": "CORTEX_STEP_STATUS_WAITING",
                    "askQuestion": {
                        "questions": [{ "question": "要包含哪些平台？" }]
                    },
                    "metadata": { "executionId": "exec-0", "startedAt": "2026-01-01T00:00:04Z" }
                }
            ],
            "executorMetadatas": []
        }
    });
    let snap_waiting = snapshot(ID, &summary, &first_turn_waiting).unwrap();
    assert_eq!(snap_waiting.events.len(), 1);
    assert_eq!(snap_waiting.events[0].kind, "reply_needed");
    assert_eq!(snap_waiting.events[0].body, "要包含哪些平台？");

    // Case 2: Browser setup / elicitation interaction variants + summary.lastModifiedTime fallback
    let mut browser_waiting = first_turn_waiting.clone();
    browser_waiting["trajectory"]["steps"][1] = json!({
        "type": "CORTEX_STEP_TYPE_GENERIC",
        "status": "CORTEX_STEP_STATUS_WAITING",
        "requestedInteraction": { "confirmBrowserSetup": {} },
        "metadata": { "executionId": "exec-0" }
    });
    let snap_auth = snapshot(ID, &summary, &browser_waiting).unwrap();
    assert_eq!(snap_auth.events.len(), 1);
    assert_eq!(snap_auth.events[0].kind, "authorization");

    // Case 3: Multi-executor /plan turn where exec-1 wrote RequestFeedback=true artifact
    // and exec-2 finished with a declarative summary (no question mark) -> reply_needed!
    let plan_turn = json!({
        "status": "CASCADE_RUN_STATUS_IDLE",
        "numTotalSteps": 3,
        "trajectory": {
            "cascadeId": ID,
            "trajectoryId": "fixture-trajectory",
            "steps": [
                {
                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": { "executionId": "exec-1", "createdAt": "2026-01-01T00:00:00Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_CODE_ACTION",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "codeAction": {
                        "artifactMetadata": {
                            "requestFeedback": true,
                            "summary": "Implementation plan ready for approval."
                        }
                    },
                    "metadata": {
                        "executionId": "exec-1",
                        "createdAt": "2026-01-01T00:00:05Z",
                        "completedAt": "2026-01-01T00:00:06Z"
                    }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": "exec-2",
                        "createdAt": "2026-01-01T00:00:07Z",
                        "completedAt": "2026-01-01T00:00:08Z"
                    },
                    "plannerResponse": {
                        "response": "计划已写入 implementation_plan.md，重点确认清理范围。"
                    }
                }
            ],
            "executorMetadatas": [
                {
                    "executionId": "exec-1",
                    "lastStepIdx": 1,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                },
                {
                    "executionId": "exec-2",
                    "lastStepIdx": 2,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                }
            ]
        }
    });
    let snap_plan = snapshot(ID, &summary, &plan_turn).unwrap();
    assert_eq!(snap_plan.events.len(), 1);
    assert_eq!(snap_plan.events[0].kind, "reply_needed");
    assert!(snap_plan.events[0].body.contains("implementation_plan.md"));

    // Case 4: Plain-text mid-turn question without ask_question tool -> reply_needed!
    let (s, mut plain_q) = fixture();
    plain_q["trajectory"]["steps"][1]["plannerResponse"]["response"] =
        json!("我已经定位到两个分支差异。你希望保留旧接口兼容还是直接移除？");
    let snap_plain = snapshot(ID, &s, &plain_q).unwrap();
    assert_eq!(snap_plain.events.len(), 1);
    assert_eq!(snap_plain.events[0].kind, "reply_needed");
}

#[test]
fn turn_ending_with_planner_response_no_tool_call_emits_completion_and_survives_hook_race_and_turn_switch(
) {
    let cascade_id = "632d506e-5152-49ac-a5b0-a1ecf2db684c";
    let trajectory_id = "bc4ff77e-6e9e-4bc2-a1bd-e0c0d4da8391";
    let exec_prev = "8992e0dd-047d-4cc0-8fd8-e64694fb3c88";
    let exec_done = "f1a85bcb-9e9b-4bf5-9d88-ef4a7d52d275";
    let summary = json!({
        "trajectoryId": trajectory_id,
        "createdTime": "2026-11-01T10:25:00Z",
        "lastModifiedTime": "2026-11-01T10:50:37.090521Z",
        "summary": "Sanitized regression fixture 632d506e",
        "status": "CASCADE_RUN_STATUS_IDLE",
        "stepCount": 137,
        "lastUserInputStepIndex": 132,
        "latestExecutorTerminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL",
        "trajectoryMetadata": { "rootConversationId": cascade_id }
    });
    // Phase 1: During Stop hook or final step transition, steps 132..=136 are DONE,
    // but f1a85bcb has not yet been committed to executorMetadatas.
    let mut response = json!({
        "status": "CASCADE_RUN_STATUS_IDLE",
        "stepOffset": 132,
        "numTotalSteps": 137,
        "trajectory": {
            "cascadeId": cascade_id,
            "trajectoryId": trajectory_id,
            "steps": [
                {
                    "type": "CORTEX_STEP_TYPE_USER_INPUT",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": { "createdAt": "2026-11-01T10:49:10Z" }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": exec_done,
                        "createdAt": "2026-11-01T10:49:15Z",
                        "completedAt": "2026-11-01T10:49:20Z"
                    },
                    "plannerResponse": {
                        "response": "Checking status...",
                        "toolCalls": [{ "id": "call-1", "name": "read_file" }]
                    }
                },
                {
                    "type": "CORTEX_STEP_TYPE_READ_FILE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": exec_done,
                        "createdAt": "2026-11-01T10:49:20Z",
                        "completedAt": "2026-11-01T10:49:22Z"
                    }
                },
                {
                    "type": "CORTEX_STEP_TYPE_RUN_COMMAND",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": exec_done,
                        "createdAt": "2026-11-01T10:50:10Z",
                        "completedAt": "2026-11-01T10:50:30Z"
                    }
                },
                {
                    "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                    "status": "CORTEX_STEP_STATUS_DONE",
                    "metadata": {
                        "executionId": exec_done,
                        "createdAt": "2026-11-01T10:50:35.095443Z",
                        "completedAt": "2026-11-01T10:50:37.090521Z"
                    },
                    "plannerResponse": {
                        "response": "审计已完成，全部结论如下。"
                    }
                }
            ],
            "executorMetadatas": [
                {
                    "executionId": "4b6a7b1f-9614-4d86-b62c-1cb4d2bc48df",
                    "lastStepIdx": 51,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                },
                {
                    "executionId": exec_prev,
                    "lastStepIdx": 130,
                    "terminationReason": "EXECUTOR_TERMINATION_REASON_USER_CANCELED"
                }
            ]
        }
    });

    let pre_meta = snapshot(cascade_id, &summary, &response).unwrap();
    assert!(pre_meta.events.is_empty());
    assert_eq!(
        pre_meta.settled_count, 133,
        "unrecorded execution must clamp settled_count so cursor does not advance to 137"
    );

    // Phase 2: ExecutorMetadata for f1a85bcb is committed with NO_TOOL_CALL on PLANNER_RESPONSE without tool calls.
    response["trajectory"]["executorMetadatas"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "executionId": exec_done,
            "lastStepIdx": 136,
            "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL",
            "proceededWithAutoContinue": false
        }));
    let post_meta = snapshot(cascade_id, &summary, &response).unwrap();
    assert_eq!(post_meta.events.len(), 1);
    assert_eq!(post_meta.events[0].kind, "completion");
    assert_eq!(post_meta.events[0].turn_id, exec_done);
    assert_eq!(post_meta.events[0].position, 137);
    assert_eq!(post_meta.settled_count, 137);

    let diag = diagnose_trajectory(
        cascade_id,
        &summary,
        &response,
        &TrajectoryLivenessContext::default(),
        Some(133),
    )
    .unwrap();
    assert_eq!(diag["recordCount"], json!(137));
    assert_eq!(diag["settledCount"], json!(137));
    assert_eq!(diag["cursor"], json!(133));
    assert_eq!(diag["cascadeActive"]["active"], json!(false));
    assert_eq!(diag["unsettledBackground"]["unsettled"], json!(false));
    assert_eq!(
        diag["candidates"].as_array().unwrap().last().unwrap()["status"],
        json!("emitted")
    );

    // Phase 3: If a second USER_INPUT arrives at step 137 before f1a85bcb was staged,
    // f1a85bcb's completion is still emitted for the previous turn while the new turn is running.
    let mut turn2_response = response.clone();
    turn2_response["status"] = json!("CASCADE_RUN_STATUS_RUNNING");
    turn2_response["numTotalSteps"] = json!(139);
    let steps_mut = turn2_response["trajectory"]["steps"]
        .as_array_mut()
        .unwrap();
    steps_mut.push(json!({
        "type": "CORTEX_STEP_TYPE_USER_INPUT",
        "status": "CORTEX_STEP_STATUS_DONE",
        "metadata": { "createdAt": "2026-11-01T10:55:00Z" }
    }));
    steps_mut.push(json!({
        "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
        "status": "CORTEX_STEP_STATUS_RUNNING",
        "metadata": {
            "executionId": "exec-turn-2",
            "createdAt": "2026-11-01T10:55:01Z"
        }
    }));
    let turn2_snap = snapshot(cascade_id, &summary, &turn2_response).unwrap();
    assert_eq!(turn2_snap.events.len(), 1);
    assert_eq!(turn2_snap.events[0].turn_id, exec_done);
    assert_eq!(turn2_snap.events[0].kind, "completion");
    assert_eq!(turn2_snap.settled_count, 137);
}
