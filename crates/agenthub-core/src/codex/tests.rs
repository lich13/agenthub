use super::{
    archived_thread_ids, hidden_thread_ids, is_request_user_input, list_threads,
    parse_message_event, resolve_codex_paths_with_options, rollout_request_user_input_state,
    scan_rollout, set_thread_title, thread_detail, thread_source_counts, window_thread_detail,
    CodexPathDiscoveryOptions, CodexPaths, RolloutRequestUserInputState, ThreadStatus,
};
use rusqlite::Connection;
use serde_json::json;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn resolved_codex_paths_prefers_valid_configured_home_before_auto_candidates() {
    let root = unique_temp_dir("resolved-codex-configured");
    let configured = root.join("configured/.codex");
    let env_home = root.join("env/.codex");
    let socket_home = root.join("socket/.codex");
    mark_codex_home(&configured);
    mark_codex_home(&env_home);
    mark_codex_home(&socket_home);
    let options = CodexPathDiscoveryOptions {
        env_codex_home: Some(env_home.clone()),
        current_user_home: None,
        root_codex_home: fallback_codex_home(&root),
        ubuntu_codex_home: root.join("ubuntu/.codex"),
        home_scan_root: root.join("home"),
        fallback_codex_home: fallback_codex_home(&root),
        fallback_codex_home_source: "fallback_root",
    };

    let resolved = resolve_codex_paths_with_options(&configured, &options);

    assert_eq!(resolved.home, configured);
    assert_eq!(resolved.codex_home_source, "configured");
    assert_eq!(resolved.logs_db, resolved.home.join("logs_2.sqlite"));
    assert_eq!(resolved.state_db, resolved.home.join("state_5.sqlite"));
    assert_eq!(
        resolved.session_index,
        resolved.home.join("session_index.jsonl")
    );
    assert_eq!(resolved.sessions_dir, resolved.home.join("sessions"));
    assert_eq!(resolved.logs_db_source, "configured");
    assert!(resolved.discovery_warnings.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolved_codex_paths_treats_auto_and_empty_config_as_discovery() {
    let root = unique_temp_dir("resolved-codex-auto");
    let env_home = root.join("env/.codex");
    mark_codex_home(&env_home);
    let options = CodexPathDiscoveryOptions {
        env_codex_home: Some(env_home.clone()),
        current_user_home: None,
        root_codex_home: fallback_codex_home(&root),
        ubuntu_codex_home: root.join("ubuntu/.codex"),
        home_scan_root: root.join("home"),
        fallback_codex_home: fallback_codex_home(&root),
        fallback_codex_home_source: "fallback_root",
    };

    let auto = resolve_codex_paths_with_options(Path::new("auto"), &options);
    let empty = resolve_codex_paths_with_options(Path::new(""), &options);

    assert_eq!(auto.home, env_home);
    assert_eq!(auto.configured_codex_home, None);
    assert_eq!(auto.codex_home_source, "env:CODEX_HOME");
    assert_eq!(empty.home, env_home);
    assert_eq!(empty.configured_codex_home, None);
    assert_eq!(empty.codex_home_source, "env:CODEX_HOME");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolved_codex_paths_ignores_app_server_socket_for_home_discovery() {
    let root = unique_temp_dir("resolved-codex-no-socket-discovery");
    let socket_home = root.join("socket-owner/.codex");
    let current_home = root.join("current-user");
    mark_codex_home(&socket_home);
    mark_codex_home(&current_home.join(".codex"));
    let resolved = resolve_codex_paths_with_options(
        Path::new("auto"),
        &CodexPathDiscoveryOptions {
            env_codex_home: None,
            current_user_home: Some(current_home.clone()),
            root_codex_home: fallback_codex_home(&root),
            ubuntu_codex_home: root.join("ubuntu/.codex"),
            home_scan_root: root.join("home"),
            fallback_codex_home: fallback_codex_home(&root),
            fallback_codex_home_source: "fallback_root",
        },
    );

    assert_eq!(resolved.home, current_home.join(".codex"));
    assert_eq!(resolved.codex_home_source, "current_user");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolved_codex_paths_does_not_synthesize_app_server_socket() {
    let root = unique_temp_dir("resolved-codex-no-synthetic-socket");
    let env_home = root.join("env/.codex");
    mark_codex_home(&env_home);
    let options = CodexPathDiscoveryOptions {
        env_codex_home: Some(env_home.clone()),
        current_user_home: None,
        root_codex_home: fallback_codex_home(&root),
        ubuntu_codex_home: root.join("ubuntu/.codex"),
        home_scan_root: root.join("home"),
        fallback_codex_home: fallback_codex_home(&root),
        fallback_codex_home_source: "fallback_root",
    };

    let resolved = resolve_codex_paths_with_options(Path::new("auto"), &options);

    assert_eq!(resolved.home, env_home);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolved_codex_paths_keeps_app_server_socket_empty_without_config() {
    let root = unique_temp_dir("resolved-codex-no-synth-socket");
    let env_home = root.join("env/.codex");
    mark_codex_home(&env_home);
    let options = CodexPathDiscoveryOptions {
        env_codex_home: Some(env_home.clone()),
        current_user_home: None,
        root_codex_home: fallback_codex_home(&root),
        ubuntu_codex_home: root.join("ubuntu/.codex"),
        home_scan_root: root.join("home"),
        fallback_codex_home: fallback_codex_home(&root),
        fallback_codex_home_source: "fallback_root",
    };

    let resolved = resolve_codex_paths_with_options(Path::new("auto"), &options);

    assert_eq!(resolved.home, env_home);
    let _ = fs::remove_dir_all(root);
}

#[cfg(target_os = "macos")]
#[test]
fn default_discovery_options_do_not_probe_linux_autofs_homes_on_macos() {
    let options = CodexPathDiscoveryOptions::default();

    assert!(!options.root_codex_home.starts_with("/root"));
    assert!(!options.ubuntu_codex_home.starts_with("/home"));
    assert!(!options.home_scan_root.starts_with("/home"));
}

#[test]
fn resolved_codex_paths_uses_current_root_ubuntu_and_home_scan_without_socket() {
    let root = unique_temp_dir("resolved-codex-order");
    let invalid_config = root.join("missing/.codex");
    let current_home = root.join("current-user");
    let root_home = root.join("root/.codex");
    let ubuntu_home = root.join("ubuntu/.codex");
    let scanned_home = root.join("home/alice/.codex");
    mark_codex_home(&current_home.join(".codex"));
    mark_codex_home(&root_home);
    mark_codex_home(&ubuntu_home);
    mark_codex_home(&scanned_home);

    let current_resolved = resolve_codex_paths_with_options(
        &invalid_config,
        &CodexPathDiscoveryOptions {
            env_codex_home: None,
            current_user_home: Some(current_home.clone()),
            root_codex_home: root_home.clone(),
            ubuntu_codex_home: ubuntu_home.clone(),
            home_scan_root: root.join("home"),
            fallback_codex_home: fallback_codex_home(&root),
            fallback_codex_home_source: "fallback_root",
        },
    );
    assert_eq!(current_resolved.home, current_home.join(".codex"));
    assert_eq!(current_resolved.codex_home_source, "current_user");
    assert!(current_resolved
        .discovery_warnings
        .iter()
        .any(|warning| warning.contains("configured Codex home is not valid")));

    let root_resolved = resolve_codex_paths_with_options(
        Path::new("auto"),
        &CodexPathDiscoveryOptions {
            env_codex_home: None,
            current_user_home: None,
            root_codex_home: root_home.clone(),
            ubuntu_codex_home: ubuntu_home.clone(),
            home_scan_root: root.join("home"),
            fallback_codex_home: root_home.clone(),
            fallback_codex_home_source: "fallback_root",
        },
    );
    assert_eq!(root_resolved.home, root_home);
    assert_eq!(root_resolved.codex_home_source, "root");

    let ubuntu_resolved = resolve_codex_paths_with_options(
        Path::new("auto"),
        &CodexPathDiscoveryOptions {
            env_codex_home: None,
            current_user_home: None,
            root_codex_home: root.join("missing-root/.codex"),
            ubuntu_codex_home: ubuntu_home,
            home_scan_root: root.join("home"),
            fallback_codex_home: fallback_codex_home(&root),
            fallback_codex_home_source: "fallback_root",
        },
    );
    assert_eq!(ubuntu_resolved.codex_home_source, "home_ubuntu");

    let scan_resolved = resolve_codex_paths_with_options(
        Path::new("auto"),
        &CodexPathDiscoveryOptions {
            env_codex_home: None,
            current_user_home: None,
            root_codex_home: root.join("missing-root/.codex"),
            ubuntu_codex_home: root.join("missing-ubuntu/.codex"),
            home_scan_root: root.join("home"),
            fallback_codex_home: fallback_codex_home(&root),
            fallback_codex_home_source: "fallback_root",
        },
    );
    assert_eq!(scan_resolved.home, scanned_home);
    assert_eq!(scan_resolved.codex_home_source, "home_scan");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolved_codex_paths_keeps_linux_root_fallback_source_name() {
    let root = unique_temp_dir("resolved-codex-fallback-root");
    let root_home = fallback_codex_home(&root);
    let resolved = resolve_codex_paths_with_options(
        Path::new("auto"),
        &CodexPathDiscoveryOptions {
            env_codex_home: None,
            current_user_home: None,
            root_codex_home: root_home.clone(),
            ubuntu_codex_home: root.join("missing-ubuntu/.codex"),
            home_scan_root: root.join("home"),
            fallback_codex_home: root_home.clone(),
            fallback_codex_home_source: "fallback_root",
        },
    );

    assert_eq!(resolved.home, root_home);
    assert_eq!(resolved.codex_home_source, "fallback_root");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolved_codex_paths_ignores_configured_socket_outside_resolved_home() {
    let root = unique_temp_dir("resolved-codex-custom-socket");
    let env_home = root.join("env/.codex");
    mark_codex_home(&env_home);
    let options = CodexPathDiscoveryOptions {
        env_codex_home: Some(env_home.clone()),
        current_user_home: None,
        root_codex_home: fallback_codex_home(&root),
        ubuntu_codex_home: root.join("ubuntu/.codex"),
        home_scan_root: root.join("home"),
        fallback_codex_home: fallback_codex_home(&root),
        fallback_codex_home_source: "fallback_root",
    };

    let resolved = resolve_codex_paths_with_options(Path::new("auto"), &options);

    assert_eq!(resolved.home, env_home);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn detects_request_user_input_function_call() {
    let value = json!({"payload":{"type":"function_call","name":"request_user_input"}});
    assert!(is_request_user_input(&value));
    assert!(is_request_user_input(&json!({
        "method":"item/tool/requestUserInput",
        "params":{"questions":[]}
    })));
    assert!(is_request_user_input(&json!({
        "payload":{"type":"function_call","toolName":"requestUserInput"}
    })));
}

#[test]
fn parses_message_text() {
    let value =
        json!({"payload":{"type":"message","role":"assistant","content":[{"text":"hello"}]}});
    let msg = parse_message_event(&value).unwrap();
    assert_eq!(msg.text, "hello");
}

#[test]
fn parse_message_event_ignores_turn_context_summary() {
    let value = json!({
        "type": "turn_context",
        "payload": {
            "turn_id": "turn-live",
            "summary": "auto",
            "cwd": "/tmp",
        }
    });

    assert!(parse_message_event(&value).is_none());
}

#[test]
fn clears_old_plan_pending_after_later_user_and_assistant_progress() {
    let scan = scan_fixture(&[
        json!({"type":"item_completed","turn_id":"turn-1","item":{"type":"Plan"}}),
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>先做 A，再做 B。</proposed_plan>"}]}}),
        json!({"type":"response_item","turn_id":"turn-2","payload":{"type":"message","role":"user","content":[{"text":"执行"}]}}),
        json!({"type":"response_item","turn_id":"turn-2","payload":{"type":"message","role":"assistant","content":[{"text":"开始执行计划。"}]}}),
        json!({"type":"task_complete","turn_id":"turn-2","status":"completed","last_agent_message":"开始执行计划。"}),
    ]);

    assert!(!scan.reply_needed);
    assert!(scan.pending_elicitation.is_none());
}

#[test]
fn clears_request_user_input_after_matching_function_call_output() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"function_call","name":"request_user_input","call_id":"call-1","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"选项 1"},{"label":"选项 2"}]}]}}}),
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"function_call_output","call_id":"call-1","output":"{\"choice\":[\"选项 1\"]}"}}),
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"message","role":"assistant","content":[{"text":"已收到选择，继续执行。"}]}}),
        json!({"type":"task_complete","turn_id":"turn-1","status":"completed","last_agent_message":"已收到选择，继续执行。"}),
    ]);

    assert!(!scan.reply_needed);
    assert!(scan.pending_elicitation.is_none());
}

#[test]
fn clears_request_user_input_after_user_input_answer_item() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"function_call","name":"request_user_input","call_id":"call-1","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"选项 1"},{"label":"选项 2"}]}]}}}),
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"UserInputAnswer","call_id":"call-1","answers":{"choice":["选项 1"]}}}),
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"message","role":"assistant","content":[{"text":"已收到选择。"}]}}),
    ]);

    assert!(!scan.reply_needed);
    assert!(scan.pending_elicitation.is_none());
}

#[test]
fn keeps_latest_request_user_input_pending_without_resolution() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","turn_id":"turn-1","payload":{"type":"function_call","name":"request_user_input","call_id":"call-1","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"选项 1"},{"label":"选项 2"}]}]}}}),
    ]);

    assert!(scan.reply_needed);
    assert_eq!(
        scan.pending_elicitation.unwrap().questions[0].question,
        "选择方案"
    );
}

#[test]
fn old_plan_marker_does_not_reclassify_later_silent_completion() {
    let scan = scan_fixture(&[
        json!({"type":"item_completed","turn_id":"turn-plan","item":{"type":"Plan"}}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>旧计划</proposed_plan>"}]}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"user","content":[{"text":"执行"}]}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"开始执行。"}]}}),
        json!({"type":"task_complete","turn_id":"turn-work","status":"completed","last_agent_message":"开始执行。"}),
        json!({"type":"task_complete","turn_id":"turn-later","status":"completed","last_agent_message":null}),
    ]);

    assert!(!scan.reply_needed);
    assert!(!scan.recoverable);
}

#[test]
fn same_turn_proposed_plan_survives_task_complete_last_agent_message() {
    let events = [
        json!({"type":"thread.started","thread_id":"thread-plan"}),
        json!({"type":"turn.started","turn_id":"turn-plan"}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# Plan\n- inspect\n- patch\n</proposed_plan>"}]}}),
        json!({"type":"task_complete","turn_id":"turn-plan","status":"completed","last_agent_message":"I will run the plan now."}),
    ];
    let scan = scan_fixture(&events);
    assert!(scan.reply_needed);
    assert!(scan.pending_elicitation.is_none());
    assert_eq!(scan.active_turn_id, None);

    let path = rollout_fixture_path("same-turn-proposed-plan", &events);
    let (message, source) = super::rollout_hook_stop_message_with_source(&path, Some("turn-plan"))
        .unwrap()
        .unwrap();
    assert_eq!(source, "proposed_plan");
    assert!(message.contains("- inspect"));
    assert!(!message.contains("I will run the plan now."));
    let _ = fs::remove_file(path);
}

#[test]
fn same_turn_unrelated_function_output_does_not_clear_current_proposed_plan() {
    let scan = scan_fixture(&[
        json!({"type":"turn.started","turn_id":"turn-plan"}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>先检查，再修复。</proposed_plan>"}]}}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"function_call_output","call_id":"unrelated-call","output":"{}"}}),
    ]);

    assert!(scan.reply_needed);
    assert!(scan.pending_elicitation.is_none());
}

#[test]
fn same_turn_unrelated_answer_and_function_output_do_not_clear_request_user_input() {
    let answered_elsewhere = scan_fixture(&[
        json!({"type":"turn.started","turn_id":"turn-choice"}),
        json!({"type":"RequestUserInput","turn_id":"turn-choice","item_id":"choice-live","questions":[{"id":"choice","question":"继续吗？","options":[{"label":"继续"}]}]}),
        json!({"type":"UserInputAnswer","turn_id":"turn-choice","item_id":"choice-other","answers":{"choice":["继续"]}}),
    ]);
    assert!(answered_elsewhere.reply_needed);
    assert!(answered_elsewhere.pending_elicitation.is_some());

    let output_elsewhere = scan_fixture(&[
        json!({"type":"turn.started","turn_id":"turn-choice"}),
        json!({"type":"RequestUserInput","turn_id":"turn-choice","item_id":"choice-live","questions":[{"id":"choice","question":"继续吗？","options":[{"label":"继续"}]}]}),
        json!({"type":"response_item","turn_id":"turn-choice","item_id":"choice-other","payload":{"type":"function_call_output","output":"{}"}}),
    ]);
    assert!(output_elsewhere.reply_needed);
    assert!(output_elsewhere.pending_elicitation.is_some());
}

#[test]
fn official_turn_started_alias_marks_thread_running() {
    let scan = scan_fixture(&[
        json!({"type":"thread.started","thread_id":"thread-running"}),
        json!({"type":"turn.started","turn_id":"turn-running"}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-running"));
}

#[test]
fn completed_plan_item_alias_supplies_full_hook_stop_plan_text() {
    let completed_plan =
        "# Full Plan\n- inspect current state\n- patch state machine\n- verify targeted tests";
    let events = [
        json!({"type":"turn.started","turn_id":"turn-plan"}),
        json!({"type":"item.plan.delta","turn_id":"turn-plan","item_id":"plan-1","delta":"# Full Plan\n- inspect current state\n"}),
        json!({"type":"item.completed","turn_id":"turn-plan","item":{"id":"plan-1","type":"Plan","text":completed_plan}}),
        json!({"type":"task_complete","turn_id":"turn-plan","status":"completed","last_agent_message":"short completion"}),
    ];

    let path = rollout_fixture_path("completed-plan-item-alias", &events);
    let (message, source) = super::rollout_hook_stop_message_with_source(&path, Some("turn-plan"))
        .unwrap()
        .unwrap();

    assert_eq!(source, "proposed_plan");
    assert_eq!(message, completed_plan);
    let _ = fs::remove_file(path);
}

#[test]
fn request_user_input_survives_task_complete_and_hook_stop_until_answer() {
    let waiting_events = [
        json!({"type":"turn.started","turn_id":"turn-choice"}),
        json!({"type":"RequestUserInput","turnId":"turn-choice","itemId":"choice-1","questions":[{"id":"choice","question":"Choose a path?","options":[{"label":"A"},{"label":"B"}]}]}),
        json!({"type":"task_complete","turn_id":"turn-choice","status":"completed","last_agent_message":"Waiting for your choice."}),
    ];
    let waiting = scan_fixture(&waiting_events);
    assert!(waiting.reply_needed);
    assert_eq!(
        waiting.pending_elicitation.unwrap().questions[0].question,
        "Choose a path?"
    );

    let path = rollout_fixture_path("request-user-input-pending", &waiting_events);
    let (message, source) =
        super::rollout_hook_stop_message_with_source(&path, Some("turn-choice"))
            .unwrap()
            .unwrap();
    assert_eq!(source, "request_user_input");
    assert!(message.contains("Choose a path?"));
    assert!(!message.contains("Waiting for your choice."));
    let _ = fs::remove_file(path);

    let answered = scan_fixture(&[
        json!({"type":"turn.started","turn_id":"turn-choice"}),
        json!({"type":"RequestUserInput","turnId":"turn-choice","itemId":"choice-1","questions":[{"id":"choice","question":"Choose a path?","options":[{"label":"A"},{"label":"B"}]}]}),
        json!({"type":"task_complete","turn_id":"turn-choice","status":"completed","last_agent_message":"Waiting for your choice."}),
        json!({"type":"UserInputAnswer","turnId":"turn-choice","itemId":"choice-1","answers":{"choice":["A"]}}),
    ]);
    assert!(!answered.reply_needed);
    assert!(answered.pending_elicitation.is_none());

    let output_cleared = scan_fixture(&[
        json!({"type":"turn.started","turn_id":"turn-choice"}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"choice-call","arguments":{"questions":[{"id":"choice","question":"Choose a path?","options":[{"label":"A"},{"label":"B"}]}]}}}),
        json!({"type":"task_complete","turn_id":"turn-choice","status":"completed","last_agent_message":"Waiting for your choice."}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call_output","call_id":"choice-call","output":"{\"choice\":[\"B\"]}"}}),
    ]);
    assert!(!output_cleared.reply_needed);
    assert!(output_cleared.pending_elicitation.is_none());
}

#[test]
fn rollout_nested_turn_metadata_tracks_sequential_request_user_input_without_reviving_old_plan() {
    let first_question = json!({
        "type": "response_item",
        "payload": {
            "type": "function_call",
            "id": "fc-first",
            "name": "request_user_input",
            "arguments": {
                "questions": [{
                    "id": "first",
                    "question": "First choice?",
                    "options": [{"label": "A"}, {"label": "B"}]
                }]
            },
            "call_id": "call-first",
            "internal_chat_message_metadata_passthrough": {"turn_id": "turn-live"}
        }
    });
    let first_answer = json!({
        "type": "response_item",
        "payload": {
            "type": "function_call_output",
            "call_id": "call-first",
            "output": "{\"answers\":{\"first\":{\"answers\":[\"A\"]}}}",
            "internal_chat_message_metadata_passthrough": {"turn_id": "turn-live"}
        }
    });
    let second_question = json!({
        "type": "response_item",
        "payload": {
            "type": "function_call",
            "id": "fc-second",
            "name": "request_user_input",
            "arguments": {
                "questions": [{
                    "id": "second",
                    "question": "Second choice?",
                    "options": [{"label": "X"}, {"label": "Y"}]
                }]
            },
            "call_id": "call-second",
            "internal_chat_message_metadata_passthrough": {"turn_id": "turn-live"}
        }
    });
    let waiting_events = [
        json!({"type":"response_item","turn_id":"turn-old-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>old plan</proposed_plan>"}]}}),
        first_question.clone(),
        first_answer,
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"First answer accepted."}],"internal_chat_message_metadata_passthrough":{"turn_id":"turn-live"}}}),
        second_question.clone(),
    ];

    let waiting = scan_fixture(&waiting_events);
    let pending = waiting
        .pending_elicitation
        .expect("second question pending");
    assert!(waiting.reply_needed);
    assert_eq!(pending.turn_id.as_deref(), Some("turn-live"));
    assert_eq!(pending.item_id.as_deref(), Some("call-second"));
    assert_eq!(pending.questions[0].question, "Second choice?");

    let path = rollout_fixture_path("nested-sequential-request-user-input", &waiting_events);
    let selection = super::rollout_hook_stop_message_selection(&path, Some("turn-live"))
        .unwrap()
        .expect("latest pending question selection");
    assert_eq!(selection.source, "request_user_input");
    assert_eq!(selection.selected_turn_id.as_deref(), Some("turn-live"));
    assert!(selection.message.contains("Second choice?"));
    assert!(!selection.message.contains("First choice?"));
    assert!(!selection.message.contains("old plan"));
    let _ = fs::remove_file(path);

    let answered_events = [
        waiting_events[0].clone(),
        first_question,
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-first","output":"{}","internal_chat_message_metadata_passthrough":{"turn_id":"turn-live"}}}),
        second_question,
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-second","output":"{}","internal_chat_message_metadata_passthrough":{"turn_id":"turn-live"}}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-live","last_agent_message":"Both answers accepted."}}),
    ];
    let answered = scan_fixture(&answered_events);
    assert!(!answered.reply_needed);
    assert!(answered.pending_elicitation.is_none());

    let path = rollout_fixture_path("answered-sequential-request-user-input", &answered_events);
    let selection = super::rollout_hook_stop_message_selection(&path, Some("turn-live"))
        .unwrap()
        .expect("completed answer selection");
    assert_eq!(selection.source, "task_complete.last_agent_message");
    assert_eq!(selection.message, "Both answers accepted.");
    assert!(!selection.message.contains("old plan"));
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_request_user_input_pending_requires_exact_turn_and_call() {
    let path = rollout_fixture_path(
        "request-user-input-confirm-pending",
        &[
            json!({"type":"response_item","payload":{"type":"function_call","name":"request_user_input","arguments":"{\"questions\":[{\"id\":\"mode\",\"question\":\"Choose?\",\"options\":[{\"label\":\"A\"}]}]}","call_id":"call-other","internal_chat_message_metadata_passthrough":{"turn_id":"turn-live"}}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"request_user_input","arguments":"{\"questions\":[{\"id\":\"mode\",\"question\":\"Choose?\",\"options\":[{\"label\":\"A\"}]}]}","call_id":"call-live","internal_chat_message_metadata_passthrough":{"turn_id":"turn-live"}}}),
        ],
    );

    assert_eq!(
        rollout_request_user_input_state(&path, "turn-live", "call-live").unwrap(),
        RolloutRequestUserInputState::Pending
    );
    assert_eq!(
        rollout_request_user_input_state(&path, "turn-other", "call-live").unwrap(),
        RolloutRequestUserInputState::Missing
    );
    assert_eq!(
        rollout_request_user_input_state(&path, "turn-live", "call-missing").unwrap(),
        RolloutRequestUserInputState::Missing
    );
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_request_user_input_screenshot_calls_are_resolved_by_function_outputs() {
    let turn_id = "00000000-0000-4000-8000-000000000100";
    let call_ids = [
        "call_ewbJTMQy5Y9jfFvGrL4cQRhd",
        "call_BlwC14PE47BjSEAbt54FOdPo",
        "call_J93MFun82lrB2G97ZWg5bMUu",
        "call_C9kAFgmLeNwVTFAZOMU9GecB",
    ];
    let mut events = Vec::new();
    for call_id in call_ids {
        events.push(json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "request_user_input",
                "arguments": "{\"questions\":[{\"id\":\"unused\",\"question\":\"占位\",\"options\":[{\"label\":\"继续（推荐）\"}]}]}",
                "call_id": call_id,
                "internal_chat_message_metadata_passthrough": {"turn_id": turn_id}
            }
        }));
        events.push(json!({
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": call_id,
                "output": "request_user_input is unavailable in Default mode",
                "internal_chat_message_metadata_passthrough": {"turn_id": turn_id}
            }
        }));
    }
    let path = rollout_fixture_path("request-user-input-screenshot-resolved", &events);

    for call_id in call_ids {
        assert_eq!(
            rollout_request_user_input_state(&path, turn_id, call_id).unwrap(),
            RolloutRequestUserInputState::Resolved,
            "call_id={call_id}"
        );
    }
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_request_user_input_answer_and_turn_terminal_resolve_pending_call() {
    for terminal in [
        json!({"type":"UserInputAnswer","turnId":"turn-live","callId":"call-live","answers":{"mode":["A"]}}),
        json!({"type":"turn_aborted","turn_id":"turn-live"}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-live","last_agent_message":"done"}}),
    ] {
        let path = rollout_fixture_path(
            "request-user-input-confirm-terminal",
            &[
                json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call","name":"request_user_input","call_id":"call-live","arguments":{"questions":[{"id":"mode","question":"Choose?","options":[{"label":"A"}]}]}}}),
                terminal,
            ],
        );
        assert_eq!(
            rollout_request_user_input_state(&path, "turn-live", "call-live").unwrap(),
            RolloutRequestUserInputState::Resolved
        );
        let _ = fs::remove_file(path);
    }
}

#[test]
fn rollout_request_user_input_missing_or_unreadable_is_not_confirmed() {
    let missing = unique_temp_dir("request-user-input-missing").join("rollout.jsonl");
    assert_eq!(
        rollout_request_user_input_state(&missing, "turn-live", "call-live").unwrap(),
        RolloutRequestUserInputState::Missing
    );

    let path = rollout_fixture_path(
        "request-user-input-invalid",
        &[
            json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"message","role":"assistant","content":[{"text":"not the call"}]}}),
        ],
    );
    fs::write(&path, "not-json\n").unwrap();
    assert_eq!(
        rollout_request_user_input_state(&path, "turn-live", "call-live").unwrap(),
        RolloutRequestUserInputState::Missing
    );
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_request_user_input_bounded_tail_does_not_confirm_calls_before_eight_mib() {
    let path = rollout_fixture_path(
        "request-user-input-before-bounded-tail",
        &[
            json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call","name":"request_user_input","call_id":"call-live","arguments":{"questions":[]}}}),
        ],
    );
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let padding = format!(
        "{{\"type\":\"padding\",\"text\":\"{}\"}}\n",
        "x".repeat(9 * 1024 * 1024)
    );
    std::io::Write::write_all(&mut file, padding.as_bytes()).unwrap();

    assert_eq!(
        rollout_request_user_input_state(&path, "turn-live", "call-live").unwrap(),
        RolloutRequestUserInputState::Missing
    );
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_request_user_input_ignores_resolution_from_another_turn() {
    let path = rollout_fixture_path(
        "request-user-input-other-turn-output",
        &[
            json!({"type":"response_item","payload":{"type":"function_call","name":"request_user_input","call_id":"call-live","internal_chat_message_metadata_passthrough":{"turn_id":"turn-live"}}}),
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-live","internal_chat_message_metadata_passthrough":{"turn_id":"turn-other"},"output":"request_user_input is unavailable"}}),
        ],
    );
    assert_eq!(
        rollout_request_user_input_state(&path, "turn-live", "call-live").unwrap(),
        RolloutRequestUserInputState::Pending
    );
    let _ = fs::remove_file(path);
}

#[test]
fn stale_plan_stays_resolved_after_user_tool_assistant_and_turn_completion() {
    let scan = scan_fixture(&[
        json!({"type":"turn.started","turn_id":"turn-plan"}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>old plan</proposed_plan>"}]}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"user","content":[{"text":"go ahead"}]}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":{"cmd":"pwd"}}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"function_call_output","call_id":"call-1","output":"/tmp"}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"execution progress"}]}}),
        json!({"type":"turn.completed","turn_id":"turn-plan"}),
    ]);

    assert!(!scan.reply_needed);
    assert!(scan.pending_elicitation.is_none());
}

#[test]
fn unrelated_function_output_without_ids_does_not_clear_pending_choice() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","payload":{"type":"function_call","name":"request_user_input","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"选项 1"},{"label":"选项 2"}]}]}}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","output":"{}"}}),
    ]);

    assert!(scan.reply_needed);
    assert_eq!(
        scan.pending_elicitation.unwrap().questions[0].question,
        "选择方案"
    );
}

#[test]
fn thread_detail_blocks_hide_internal_context_and_keep_chat_messages() {
    let detail = detail_fixture(&[
        json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"text":"internal instructions"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"system","content":[{"text":"system context"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"<environment_context>\n  <cwd>/tmp</cwd>\n</environment_context>"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"hello"}]}}),
        json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"text":"hidden reasoning"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"world"}]}}),
    ]);

    assert_eq!(detail.blocks.len(), 3);
    assert_eq!(detail.blocks[0].role, "user");
    assert_eq!(detail.blocks[0].text.as_deref(), Some(""));
    let context_only = detail.blocks[0]
        .user_message
        .as_ref()
        .expect("context-only user block keeps its structured message");
    assert!(context_only.text.is_empty());
    assert_eq!(context_only.context_blocks.len(), 1);
    assert_eq!(
        context_only.context_blocks[0].kind,
        crate::user_message::UserContextKind::Environment
    );
    assert!(context_only.context_blocks[0]
        .text
        .contains("<environment_context>"));
    assert_eq!(detail.blocks[1].role, "user");
    assert_eq!(detail.blocks[1].text.as_deref(), Some("hello"));
    assert_eq!(detail.blocks[2].role, "assistant");
    assert_eq!(detail.blocks[2].text.as_deref(), Some("world"));
    let visible_text = detail
        .blocks
        .iter()
        .filter_map(|block| block.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    for hidden in [
        "internal instructions",
        "system context",
        "hidden reasoning",
    ] {
        assert!(!visible_text.contains(hidden));
    }
}

#[test]
fn thread_detail_blocks_hide_event_msg_progress_rows() {
    let detail = detail_fixture(&[
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"progress update"}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"duplicate user text"}}),
        json!({"type":"turn_context","payload":{"cwd":"/tmp"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"final answer"}]}}),
    ]);

    assert_eq!(detail.blocks.len(), 1);
    assert_eq!(detail.blocks[0].role, "assistant");
    assert_eq!(detail.blocks[0].text.as_deref(), Some("final answer"));
}

#[test]
fn thread_detail_blocks_hide_subagent_context_fragments() {
    let detail = detail_fixture(&[
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"<subagent_notification>{\"agent_path\":\"/tmp/child\",\"status\":{\"completed\":\"done\"}}</subagent_notification>"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"<subagent_context>\n- /tmp/child: worker\n</subagent_context>"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"真实用户消息"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"可见回复"}]}}),
    ]);

    assert_eq!(detail.blocks.len(), 4);
    for (index, expected) in ["<subagent_notification>", "<subagent_context>"]
        .into_iter()
        .enumerate()
    {
        let block = &detail.blocks[index];
        assert_eq!(block.role, "user");
        assert_eq!(block.text.as_deref(), Some(""));
        let message = block
            .user_message
            .as_ref()
            .expect("context-only subagent block keeps its structured message");
        assert!(message.text.is_empty());
        assert_eq!(message.context_blocks.len(), 1);
        assert_eq!(
            message.context_blocks[0].kind,
            crate::user_message::UserContextKind::Runtime
        );
        assert!(message.context_blocks[0].text.contains(expected));
    }
    assert_eq!(detail.blocks[2].role, "user");
    assert_eq!(detail.blocks[2].text.as_deref(), Some("真实用户消息"));
    assert_eq!(detail.blocks[3].role, "assistant");
    assert_eq!(detail.blocks[3].text.as_deref(), Some("可见回复"));
    assert!(detail.blocks.iter().all(|block| {
        let text = block.text.as_deref().unwrap_or_default();
        !text.contains("subagent_notification")
            && !text.contains("subagent_context")
            && !text.contains("/tmp/child")
    }));
}

#[test]
fn proposed_plan_message_becomes_action_block_only() {
    let detail = detail_fixture(&[
        json!({"type":"item_completed","turn_id":"turn-plan","item":{"id":"plan-item","type":"Plan"}}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# Summary\n- Fix it\n</proposed_plan>"}]}}),
    ]);

    assert_eq!(detail.blocks.len(), 1);
    let block = &detail.blocks[0];
    assert_eq!(block.role, "assistant");
    assert_eq!(block.kind, "plan");
    assert_eq!(block.turn_id.as_deref(), Some("turn-plan"));
    assert_eq!(block.item_id.as_deref(), Some("plan-item"));
    assert!(block
        .text
        .as_deref()
        .is_some_and(|text| text.contains("<proposed_plan>")));
}

#[test]
fn same_turn_task_complete_last_agent_message_keeps_current_plan_pending_block() {
    let detail = detail_fixture(&[
        json!({"type":"item_completed","turn_id":"turn-plan","item":{"id":"plan-item","type":"Plan"}}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>先检查，再修复。</proposed_plan>"}]}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":"我会按这个计划继续。"}}),
    ]);

    let plan = detail
        .blocks
        .iter()
        .find(|block| block.kind == "plan")
        .unwrap();
    assert_eq!(plan.status.as_deref(), Some("pending"));
    assert_eq!(plan.plan_status.as_deref(), Some("pending"));
    assert_eq!(plan.resolved, Some(false));
}

#[test]
fn same_turn_task_complete_last_agent_message_keeps_unidentified_current_plan_pending_block() {
    let detail = detail_fixture(&[
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>先检查，再修复。</proposed_plan>"}]}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":"我会按这个计划继续。"}}),
    ]);

    let plan = detail
        .blocks
        .iter()
        .find(|block| block.kind == "plan")
        .unwrap();
    assert_eq!(plan.status.as_deref(), Some("pending"));
    assert_eq!(plan.plan_status.as_deref(), Some("pending"));
    assert_eq!(plan.resolved, Some(false));
}

#[test]
fn old_proposed_plan_block_is_resolved_after_user_reply_and_assistant_progress() {
    let detail = detail_fixture(&[
        json!({"type":"item_completed","turn_id":"turn-plan","item":{"id":"plan-item","type":"Plan"}}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>先做 A，再做 B。</proposed_plan>"}]}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"user","content":[{"text":"执行"}]}}),
        json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"开始执行。"}]}}),
    ]);

    let plan = detail
        .blocks
        .iter()
        .find(|block| block.kind == "plan")
        .unwrap();
    assert_eq!(plan.status.as_deref(), Some("completed"));
    assert_eq!(plan.plan_status.as_deref(), Some("completed"));
    assert_eq!(plan.resolved, Some(true));
}

#[test]
fn old_proposed_plan_block_is_resolved_after_successful_task_complete() {
    let detail = detail_fixture(&[
        json!({"type":"item_completed","turn_id":"turn-plan","item":{"id":"plan-item","type":"Plan"}}),
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>先做 A，再做 B。</proposed_plan>"}]}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-work","last_agent_message":"计划已处理。"}}),
    ]);

    let plan = detail
        .blocks
        .iter()
        .find(|block| block.kind == "plan")
        .unwrap();
    assert_eq!(plan.status.as_deref(), Some("completed"));
    assert_eq!(plan.plan_status.as_deref(), Some("completed"));
    assert_eq!(plan.resolved, Some(true));
}

#[test]
fn plan_protocol_variants_merge_into_stable_plan_block() {
    let blocks = super::message_blocks_from_events(
        [
            json!({"type":"PlanDelta","turn_id":"turn-plan","delta":"- inspect\n"}),
            json!({"type":"item/plan/delta","turn_id":"turn-plan","item_id":"plan-1","delta":"- patch\n"}),
            json!({"type":"turn/plan/updated","turn_id":"turn-plan","plan":{"text":"- inspect\n- patch\n- test"}}),
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"Plan","id":"plan-1","text":"- inspect\n- patch\n- test","status":"completed"}}),
        ]
        .iter(),
    );

    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert_eq!(block.kind, "plan");
    assert_eq!(block.display_kind.as_deref(), Some("plan"));
    assert_eq!(block.group_id.as_deref(), Some("plan-turn-turn-plan"));
    assert_eq!(block.item_id.as_deref(), Some("plan-1"));
    assert_eq!(block.status.as_deref(), Some("completed"));
    assert_eq!(block.resolved, Some(true));
    assert_eq!(block.text.as_deref(), Some("- inspect\n- patch\n- test"));
}

#[test]
fn scan_rollout_accepts_official_pascal_case_tui_protocol_aliases() {
    let plan = scan_fixture(&[
        json!({"type":"TurnStarted","turn_id":"turn-plan"}),
        json!({"type":"PlanDelta","turn_id":"turn-plan","item_id":"plan-1","delta":"- 检查\n"}),
        json!({"type":"response_item","turn_id":"turn-plan","item_id":"plan-1","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 计划\n- 检查\n</proposed_plan>"}]}}),
        json!({"type":"TurnComplete","turn_id":"turn-plan","last_agent_message":null}),
    ]);
    assert!(plan.reply_needed);
    assert!(!plan.running);
    assert_eq!(plan.active_turn_id, None);

    let question = scan_fixture(&[
        json!({"type":"TurnStarted","turnId":"turn-choice"}),
        json!({"type":"RequestUserInput","turnId":"turn-choice","itemId":"choice-1","questions":[{"id":"choice","question":"是否继续？","options":[{"label":"继续"}]}]}),
    ]);
    assert!(question.reply_needed);
    assert!(question.pending_elicitation.is_some());
    assert_eq!(question.active_turn_id.as_deref(), Some("turn-choice"));

    let answered = scan_fixture(&[
        json!({"type":"TurnStarted","turnId":"turn-choice"}),
        json!({"type":"RequestUserInput","turnId":"turn-choice","itemId":"choice-1","questions":[{"id":"choice","question":"是否继续？","options":[{"label":"继续"}]}]}),
        json!({"type":"UserInputAnswer","turnId":"turn-choice","itemId":"choice-1","answers":{"choice":["继续"]}}),
    ]);
    assert!(!answered.reply_needed);
    assert!(answered.pending_elicitation.is_none());
}

#[test]
fn user_input_answer_clears_pending_but_turn_completion_and_progress_do_not() {
    let answered = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-choice"}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"call-choice","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"UserInputAnswer","call_id":"call-choice","answers":{"choice":["A"]}}}),
    ]);
    assert!(!answered.reply_needed);
    assert!(answered.pending_elicitation.is_none());

    let completed = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-choice"}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"call-choice","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
        json!({"type":"turn_completed","turn_id":"turn-choice"}),
    ]);
    assert!(completed.reply_needed);
    assert!(completed.pending_elicitation.is_some());

    let progressed = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-choice"}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"call-choice","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
        json!({"type":"event_msg","payload":{"type":"progress","turn_id":"turn-choice","message":"continuing"}}),
    ]);
    assert!(progressed.reply_needed);
    assert!(progressed.pending_elicitation.is_some());
}

#[test]
fn request_user_input_output_becomes_resolved_history_block() {
    let blocks = super::message_blocks_from_events(
        [
            json!({"type":"turn_started","turn_id":"turn-choice"}),
            json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"call-choice","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
            json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call_output","call_id":"call-choice","output":"{\"choice\":[\"A\"]}"}}),
        ]
        .iter(),
    );

    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert_eq!(block.kind, "request_user_input_result");
    assert_eq!(block.display_kind.as_deref(), Some("question_result"));
    assert_eq!(block.status.as_deref(), Some("completed"));
    assert_eq!(block.resolved, Some(true));
    assert_eq!(block.questions[0].question, "选择方案");
    assert_eq!(block.answers[0].question_id, "choice");
    assert_eq!(block.answers[0].answers, vec!["A".to_string()]);
}

#[test]
fn internal_roles_and_reasoning_protocol_blocks_are_filtered() {
    let detail = detail_fixture(&[
        json!({"type":"response_item","payload":{"type":"message","role":"system","content":[{"text":"system context"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"text":"developer context"}]}}),
        json!({"type":"response_item","payload":{"type":"reasoning_delta","summary":[{"text":"hidden reasoning"}]}}),
        json!({"type":"response_item","payload":{"type":"internal","text":"hidden internal"}}),
        json!({"type":"response_item","payload":{"type":"subagent","text":"hidden subagent"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"visible user"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"visible assistant"}]}}),
    ]);

    assert_eq!(detail.blocks.len(), 2);
    assert_eq!(detail.messages.len(), 2);
    assert_eq!(detail.blocks[0].text.as_deref(), Some("visible user"));
    assert_eq!(detail.blocks[1].text.as_deref(), Some("visible assistant"));
}

#[test]
fn message_blocks_parse_request_user_input_protocol_shapes() {
    let events = [
        json!({
            "method":"item/tool/requestUserInput",
            "params":{
                "turnId":"turn-choice",
                "itemId":"item-choice",
                "questions":[{
                    "id":"q1",
                    "header":"选择",
                    "question":"选择方案",
                    "options":[{"label":"A","description":"执行 A"}]
                }]
            }
        }),
        json!({
            "type":"response_item",
            "payload":{
                "type":"function_call",
                "toolName":"requestUserInput",
                "callId":"call-choice",
                "input":{
                    "arguments":{
                        "questions":[{
                            "id":"q2",
                            "question":"继续吗",
                            "options":[{"value":"继续"}]
                        }]
                    }
                }
            }
        }),
    ];

    let blocks = super::message_blocks_from_events(events.iter());

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].kind, "request_user_input");
    assert_eq!(blocks[0].turn_id.as_deref(), Some("turn-choice"));
    assert_eq!(blocks[0].item_id.as_deref(), Some("item-choice"));
    assert_eq!(blocks[0].questions[0].question, "选择方案");
    assert_eq!(blocks[0].questions[0].options[0].label, "A");
    assert_eq!(blocks[1].call_id.as_deref(), Some("call-choice"));
    assert_eq!(blocks[1].questions[0].options[0].label, "继续");
}

#[test]
fn thread_detail_blocks_merge_function_call_with_output() {
    let detail = detail_fixture(&[
        json!({"type":"response_item","timestamp":"2026-06-07T10:00:00Z","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","status":"completed","arguments":{"cmd":"pwd"}}}),
        json!({"type":"response_item","timestamp":"2026-06-07T10:00:01Z","payload":{"type":"function_call_output","call_id":"call-1","output":"Output:\n/home/ubuntu"}}),
    ]);

    assert_eq!(detail.blocks.len(), 1);
    let block = &detail.blocks[0];
    assert_eq!(block.role, "tool");
    assert_eq!(block.kind, "function_call_output");
    assert_eq!(block.tool_name.as_deref(), Some("exec_command"));
    assert_eq!(block.call_id.as_deref(), Some("call-1"));
    assert_eq!(block.input.as_deref(), Some("{\n  \"cmd\": \"pwd\"\n}"));
    assert_eq!(block.text.as_deref(), Some("Output:\n/home/ubuntu"));
}

#[test]
fn thread_detail_blocks_do_not_emit_empty_function_call_shells() {
    let detail = detail_fixture(&[
        json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":{"cmd":"pwd"}}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"done"}]}}),
    ]);

    assert_eq!(detail.blocks.len(), 2);
    assert_eq!(detail.blocks[1].role, "assistant");
    assert_eq!(detail.blocks[1].text.as_deref(), Some("done"));
    assert_eq!(detail.blocks[0].role, "tool");
    assert_eq!(detail.blocks[0].status.as_deref(), Some("running"));
    assert_eq!(detail.blocks[0].tool_name.as_deref(), Some("exec_command"));
    assert!(detail.blocks[0]
        .text
        .as_deref()
        .unwrap_or_default()
        .is_empty());
}

#[test]
fn thread_detail_blocks_clear_wait_agent_after_task_complete_same_turn() {
    let detail = detail_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-main"}}),
        json!({"type":"response_item","turn_id":"turn-main","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-main","last_agent_message":"主线程完成。"}}),
    ]);

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert!(detail.summary.active_turn_id.is_none());
    assert!(!detail.blocks.iter().any(|block| {
        block.call_id.as_deref() == Some("wait-agent-1")
            && block.status.as_deref().is_some_and(|status| {
                matches!(
                    status,
                    "pending" | "running" | "in_progress" | "inProgress" | "active"
                )
            })
    }));
}

#[test]
fn thread_detail_blocks_clear_anonymous_tool_after_task_complete_same_turn() {
    let detail = detail_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-main"}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-main","last_agent_message":"主线程完成。"}}),
    ]);

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert!(detail.summary.active_turn_id.is_none());
    assert!(
        !detail
            .blocks
            .iter()
            .any(|block| block.call_id.as_deref() == Some("wait-agent-1")),
        "anonymous same-turn wait_agent should not survive as a residual pending block"
    );
}

#[test]
fn thread_detail_blocks_clear_custom_tool_after_turn_completed_same_turn() {
    let detail = detail_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-main"}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"custom-1","input":"*** Begin Patch"}}),
        json!({"type":"turn_completed","turn_id":"turn-main"}),
    ]);

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert!(detail.summary.active_turn_id.is_none());
    assert!(
        !detail
            .blocks
            .iter()
            .any(|block| block.call_id.as_deref() == Some("custom-1")),
        "anonymous same-turn custom_tool_call should not survive as a residual pending block"
    );
}

#[test]
fn thread_detail_blocks_merge_custom_tool_call_with_output() {
    let detail = detail_fixture(&[
        json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"call-2","status":"completed","input":"*** Begin Patch"}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call-2","output":"Success. Updated files."}}),
    ]);

    assert_eq!(detail.blocks.len(), 1);
    let block = &detail.blocks[0];
    assert_eq!(block.role, "tool");
    assert_eq!(block.kind, "custom_tool_call_output");
    assert_eq!(block.tool_name.as_deref(), Some("apply_patch"));
    assert_eq!(block.input.as_deref(), Some("*** Begin Patch"));
    assert_eq!(block.text.as_deref(), Some("Success. Updated files."));
}

#[test]
fn thread_detail_blocks_keep_orphan_tool_outputs() {
    let detail = detail_fixture(&[json!({
        "type":"response_item",
        "payload":{"type":"function_call_output","call_id":"missing-call","output":"orphan output"}
    })]);

    assert_eq!(detail.blocks.len(), 1);
    let block = &detail.blocks[0];
    assert_eq!(block.role, "tool");
    assert_eq!(block.kind, "function_call_output");
    assert_eq!(block.call_id.as_deref(), Some("missing-call"));
    assert_eq!(block.text.as_deref(), Some("orphan output"));
}

#[test]
fn scan_rollout_ignores_subagent_notifications_for_latest_message() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"<subagent_notification>{\"agent_path\":\"/tmp/child\",\"status\":{\"completed\":\"done\"}}</subagent_notification>"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"真实回复"}]}}),
    ]);

    assert_eq!(scan.message_count, 1);
    assert_eq!(scan.latest_message.as_deref(), Some("真实回复"));
}

#[test]
fn rollout_latest_assistant_message_reads_last_visible_assistant_message() {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-latest-assistant-{}-{counter}.jsonl",
        std::process::id()
    ));
    let events = [
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"first answer"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"next"}]}}),
        json!({"type":"subagent_notification","message":"worker done"}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"final answer"}]}}),
    ];
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

    assert_eq!(
        super::rollout_latest_assistant_message(&path)
            .unwrap()
            .as_deref(),
        Some("final answer")
    );
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_hook_stop_message_prefers_full_plan_over_null_completion_and_short_summary() {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-hook-stop-full-plan-{}-{counter}.jsonl",
        std::process::id()
    ));
    let full_plan = format!("# 完整计划\n{}\n末尾唯一完整计划", "计划正文".repeat(1200));
    let events = [
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n短摘要\n</proposed_plan>"}]}}),
        json!({"type":"item_completed","thread_id":"thread-a","turn_id":"turn-plan","item":{"type":"Plan","id":"plan-1","text":full_plan}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":null}}),
    ];
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

    let message = super::rollout_hook_stop_message(&path, Some("turn-plan"))
        .unwrap()
        .unwrap();

    assert!(message.contains("# 完整计划"));
    assert!(message.contains("末尾唯一完整计划"));
    assert!(!message.contains("[truncated]"));
    assert!(message.len() > 4000);
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_hook_stop_message_keeps_final_answer_plan_over_later_progress() {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-hook-stop-final-answer-plan-{}-{counter}.jsonl",
        std::process::id()
    ));
    let plan = "# Remove Local WARP Cleanly\n- stop service\n- verify cleanup";
    let events = [
        json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":format!("<proposed_plan>\n{plan}\n</proposed_plan>")}],"phase":"final_answer"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":null}}),
        json!({"type":"response_item","turn_id":"turn-next","payload":{"type":"message","role":"assistant","content":[{"text":"我会按刚才的计划执行系统清理。"}],"phase":"commentary"}}),
    ];
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

    let (message, source) = super::rollout_hook_stop_message_with_source(&path, Some("turn-plan"))
        .unwrap()
        .unwrap();

    assert_eq!(source, "proposed_plan");
    assert!(message.contains(plan));
    assert!(!message.contains("我会按刚才的计划执行系统清理"));
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_hook_stop_message_uses_final_response_when_turn_context_has_auto_summary() {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-hook-stop-final-response-{}-{counter}.jsonl",
        std::process::id()
    ));
    let final_answer = "已按计划执行完。\n\n验证结果：全部通过。\n末尾唯一完整反馈";
    let events = [
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"turn_context","payload":{"turn_id":"turn-live","summary":"auto","cwd":"/tmp"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":final_answer}],"phase":"final_answer"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-live","last_agent_message":final_answer}}),
    ];
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

    let (message, source) = super::rollout_hook_stop_message_with_source(&path, Some("turn-live"))
        .unwrap()
        .unwrap();

    assert_eq!(source, "task_complete.last_agent_message");
    assert_eq!(message, final_answer);
    assert!(!message.contains("auto"));
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_hook_stop_message_falls_back_to_unscoped_final_response_before_task_complete_flush() {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-hook-stop-final-response-race-{}-{counter}.jsonl",
        std::process::id()
    ));
    let final_answer = "最终回复开头\n\n完整正文完整正文\n末尾唯一完整反馈";
    let events = [
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"turn_context","payload":{"turn_id":"turn-live","summary":"auto","cwd":"/tmp"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":final_answer}],"phase":"final_answer"}}),
    ];
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

    let (message, source) = super::rollout_hook_stop_message_with_source(&path, Some("turn-live"))
        .unwrap()
        .unwrap();

    assert_eq!(source, "last_assistant_message");
    assert_eq!(message, final_answer);
    assert!(!message.contains("auto"));
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_completion_does_not_fallback_to_old_turn_when_current_turn_has_no_body_yet() {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-completion-no-old-turn-fallback-{}-{counter}.jsonl",
        std::process::id()
    ));
    let events = [
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"上一轮完成正文"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"turn_context","payload":{"turn_id":"turn-live","summary":"auto","cwd":"/tmp"}}),
    ];
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

    let message =
        super::rollout_completion_last_agent_message_with_source(&path, Some("turn-live")).unwrap();

    assert!(
        message.is_none(),
        "current turn has started, so completion selection must wait instead of using an old turn"
    );
    let _ = fs::remove_file(path);
}

#[test]
fn rollout_completion_associates_unscoped_final_answer_with_current_turn_context() {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-completion-unscoped-final-{}-{counter}.jsonl",
        std::process::id()
    ));
    let final_answer = "已接着完成。\n最终答复唯一标记";
    let events = [
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"上一轮完成正文"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"turn_context","payload":{"turn_id":"turn-live","summary":"auto","cwd":"/tmp"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":final_answer}],"phase":"final_answer"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-live","last_agent_message":final_answer}}),
    ];
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

    let (message, source) =
        super::rollout_completion_last_agent_message_with_source(&path, Some("turn-live"))
            .unwrap()
            .unwrap();

    assert_eq!(source, "task_complete.last_agent_message");
    assert_eq!(message, final_answer);
    assert!(!message.contains("上一轮完成正文"));
    let _ = fs::remove_file(path);
}

#[test]
fn list_threads_preserves_db_rollout_path_when_session_index_misses_thread() {
    let root = unique_temp_dir("db-rollout-path");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("db-only-rollout.jsonl");
    fs::write(
        &rollout,
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"from db rollout"}]}})
            .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        json!({"id":"other-thread","path":root.join("other.jsonl")}).to_string(),
    )
    .unwrap();
    write_thread_db(&root, "test-thread", &rollout, 1, 0);

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();

    let row = rows
        .iter()
        .find(|thread| thread.id == "test-thread")
        .unwrap();
    assert_eq!(row.rollout_path.as_deref(), Some(rollout.as_path()));
    assert_eq!(row.latest_message.as_deref(), Some("from db rollout"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn list_threads_hides_db_rows_with_missing_rollout_path() {
    let root = unique_temp_dir("missing-rollout-path");
    fs::create_dir_all(&root).unwrap();
    let missing_rollout = root.join("sessions/2026/06/25/missing-rollout.jsonl");
    write_thread_db(&root, "dangling-thread", &missing_rollout, 1, 0);

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let detail = thread_detail(&CodexPaths::new(&root), "dangling-thread").unwrap();

    assert!(!rows.iter().any(|thread| thread.id == "dangling-thread"));
    assert!(detail.is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn list_threads_keeps_rows_after_native_compression() {
    let root = unique_temp_dir("compressed-rollout-path");
    let rollout = root
        .join("sessions/rollout-2026-01-01T00-00-00-11111111-1111-4111-8111-111111111111.jsonl");
    fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    fs::write(
        crate::codex::rollout_file::compressed_path(&rollout),
        zstd::stream::encode_all(b"{\"type\":\"session_meta\"}\n".as_slice(), 0).unwrap(),
    )
    .unwrap();
    write_thread_db(&root, "compressed-thread", &rollout, 1, 0);
    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let compressed_row = rows
        .iter()
        .find(|thread| thread.id == "compressed-thread")
        .expect("compressed thread in active list");
    assert_eq!(
        compressed_row.rollout_path,
        Some(crate::codex::rollout_file::compressed_path(&rollout))
    );
    assert!(thread_detail(&CodexPaths::new(&root), "compressed-thread")
        .unwrap()
        .is_some());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn list_threads_does_not_read_rollout_outside_codex_home() {
    let root = unique_temp_dir("external-rollout-path");
    let external = unique_temp_dir("external-rollout-content");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&external).unwrap();
    let external_rollout = external.join("external-rollout.jsonl");
    fs::write(
        &external_rollout,
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"external message must not be read"}]}})
            .to_string(),
    )
    .unwrap();
    write_thread_db(&root, "external-thread", &external_rollout, 1, 0);

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();

    let row = rows
        .iter()
        .find(|thread| thread.id == "external-thread")
        .unwrap();
    assert!(row.rollout_path.is_none());
    assert!(row.latest_message.is_none());
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(external);
}

#[test]
fn list_threads_does_not_discover_rollout_path_from_sessions_tree() {
    let root = unique_temp_dir("list-no-session-tree-scan");
    let sessions = root.join("sessions/2026/06/20");
    fs::create_dir_all(&sessions).unwrap();
    let rollout = sessions.join("rollout-special-thread.jsonl");
    fs::write(
        &rollout,
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}})
            .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        json!({"id":"special-thread"}).to_string(),
    )
    .unwrap();
    write_thread_db(&root, "special-thread", Path::new(""), 1, 0);

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let row = rows
        .iter()
        .find(|thread| thread.id == "special-thread")
        .unwrap();
    assert!(row.rollout_path.is_none());
    assert_ne!(row.status, ThreadStatus::Running);

    let detail = thread_detail(&CodexPaths::new(&root), "special-thread")
        .unwrap()
        .unwrap();
    assert!(detail.summary.rollout_path.is_none());
    assert!(detail.summary.active_turn_id.is_none());
    let _ = fs::remove_dir_all(root);
}

#[cfg(target_os = "macos")]
#[test]
fn codex_paths_rejects_network_volume_paths() {
    let paths = CodexPaths::new("/Volumes/share/.codex");
    assert!(!paths.contains_path(Path::new("/Volumes/share/.codex/sessions/a.jsonl")));
    assert!(!super::paths::is_valid_codex_home(Path::new(
        "/Volumes/share/.codex"
    )));
    assert!(super::is_macos_network_volume_path(Path::new(
        "/Volumes/share/.codex"
    )));
}

#[cfg(target_os = "macos")]
#[test]
fn resolve_codex_paths_does_not_fall_back_to_network_volume_configured_home() {
    let root = unique_temp_dir("network-volume-configured-home");
    let options = CodexPathDiscoveryOptions {
        env_codex_home: None,
        current_user_home: Some(root.clone()),
        root_codex_home: root.join("root/.codex"),
        ubuntu_codex_home: root.join("home/ubuntu/.codex"),
        home_scan_root: root.join("home"),
        fallback_codex_home: root.join("fallback/.codex"),
        fallback_codex_home_source: "fallback_current_user",
    };
    let resolved = resolve_codex_paths_with_options(Path::new("/Volumes/share/.codex"), &options);
    assert!(!resolved.home.starts_with("/Volumes"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rollout_session_meta_subagent_thread_is_hidden_from_list() {
    let root = unique_temp_dir("rollout-subagent-hidden");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-subagent.jsonl");
    fs::write(
        &rollout,
        [
            json!({"session_meta":{"payload":{"thread_source":"subagent","parent_thread_id":"parent","agent_nickname":"worker","agent_role":"explorer"}}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"worker result"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    write_thread_db(&root, "child-thread", &rollout, 1, 0);

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();

    assert!(rows.iter().all(|row| row.id != "child-thread"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn list_threads_title_priority_filters_placeholders_and_preview_titles() {
    let root = unique_temp_dir("thread-title-priority");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();

    write_thread_fixture(
        &conn,
        &root,
        "db-title",
        "真实 DB 标题",
        "",
        &[
            json!({"session_meta":{"payload":{"title":"rollout metadata title"}}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"首条用户消息不应覆盖 DB 标题"}]}}),
        ],
        4,
    );
    write_thread_fixture(
        &conn,
        &root,
        "metadata-title",
        "读取中...",
        "",
        &[json!({"session_meta":{"payload":{"thread_title":"来自元数据的标题"}}})],
        3,
    );
    write_thread_fixture(
        &conn,
        &root,
        "user-title",
        "<proposed_plan>计划内容</proposed_plan>",
        "Assistant preview: This is a very long assistant response body that should not become the local thread title because it is only preview text.",
        &[
            json!({"session_meta":{"payload":{"name":"Untitled"}}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"Assistant preview: This is a very long assistant response body that should not become the title."}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"请修复本地线程标题显示，并验证结果。"}]}}),
        ],
        2,
    );
    write_thread_fixture(
        &conn,
        &root,
        "fallback-title",
        "Untitled",
        "Assistant preview: This long assistant body must stay out of the title even when no user message exists.",
        &[json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"Assistant preview: This long assistant body must stay out of the title even when no user message exists."}]}})],
        1,
    );

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let by_id = rows
        .iter()
        .map(|row| (row.id.as_str(), row.title.as_str()))
        .collect::<HashMap<_, _>>();

    assert_eq!(by_id.get("db-title"), Some(&"真实 DB 标题"));
    assert_eq!(by_id.get("metadata-title"), Some(&"来自元数据的标题"));
    assert_eq!(
        by_id.get("user-title"),
        Some(&"请修复本地线程标题显示，并验证结果。")
    );
    assert_eq!(by_id.get("fallback-title"), Some(&"未命名线程"));
    assert!(by_id.values().all(|title| {
        !title.contains("<proposed_plan>") && !title.contains("Assistant preview")
    }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn db_first_user_message_titles_list_and_detail_when_db_title_is_unusable() {
    let root = unique_temp_dir("db-first-user-title");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-first-user.jsonl");
    fs::write(&rollout, "").unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            first_user_message TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, first_user_message, preview)
         VALUES('first-user-thread', ?1, 1, 1, 'codex', '/tmp', ?2, '请把本地线程标题恢复为首条用户消息摘要，并验证详情页。', '')",
        (
            rollout.display().to_string(),
            "这是一个超过一百二十个字符的临时标题，不能直接作为线程标题使用，但也不能因此丢弃 first_user_message 的兜底能力。这个字段可能来自旧版本数据库里的助手正文或其他不可用内容，需要继续尝试更可靠的用户消息。这里继续补足长度，确保它一定超过一百二十个字符。",
        ),
    )
    .unwrap();

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let row = rows
        .iter()
        .find(|thread| thread.id == "first-user-thread")
        .unwrap();
    assert_eq!(
        row.title,
        "请把本地线程标题恢复为首条用户消息摘要，并验证详情页。"
    );
    let detail = thread_detail(&CodexPaths::new(&root), "first-user-thread")
        .unwrap()
        .unwrap();
    assert_eq!(
        detail.summary.title,
        "请把本地线程标题恢复为首条用户消息摘要，并验证详情页。"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn session_index_metadata_title_or_first_user_message_titles_db_row_without_rollout_path() {
    let root = unique_temp_dir("session-index-metadata-title");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES('index-title-thread', '', 1, 2, 'codex', '/tmp', 'Untitled', '')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES('index-user-thread', '', 1, 1, 'codex', '/tmp', 'Untitled', '')",
        [],
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        [
            json!({"id":"index-title-thread","title":"session index 标题"}).to_string(),
            json!({"id":"index-user-thread","first_user_message":"用 session index 首条用户消息作为标题"}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let by_id = rows
        .iter()
        .map(|row| (row.id.as_str(), row.title.as_str()))
        .collect::<HashMap<_, _>>();

    assert_eq!(by_id.get("index-title-thread"), Some(&"session index 标题"));
    assert_eq!(
        by_id.get("index-user-thread"),
        Some(&"用 session index 首条用户消息作为标题")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn session_index_thread_name_titles_db_row_without_rollout_path() {
    let root = unique_temp_dir("session-index-thread-name-title");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES('thread-name-thread', '', 1, 1, 'codex', '/tmp', 'Untitled', '')",
        [],
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        json!({"id":"thread-name-thread","thread_name":"更新"}).to_string(),
    )
    .unwrap();

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let row = rows
        .iter()
        .find(|thread| thread.id == "thread-name-thread")
        .unwrap();

    assert_eq!(row.title, "更新");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn session_index_uses_latest_thread_name_for_repeated_thread_records() {
    let root = unique_temp_dir("session-index-latest-thread-name");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            first_user_message TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    let copied_title = "接手这个线程的工作 00000000-0000-4000-8000-000000000101，检查示例工作区中各模块的职责边界与调用流程，我打算继续处理线上问题";
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, first_user_message, preview)
         VALUES('thread-name-thread', '', 1, 1, 'codex', '/tmp', ?1, ?1, '')",
        [copied_title],
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        [
            json!({"id":"thread-name-thread","thread_name":"梳理脚本"}).to_string(),
            json!({"id":"thread-name-thread","thread_name":"example_task"}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let row = list_threads(&CodexPaths::new(&root), false, None, 10)
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "thread-name-thread")
        .unwrap();

    assert_eq!(row.title, "example_task");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn session_index_thread_name_repairs_first_user_message_title_from_old_db() {
    let root = unique_temp_dir("session-index-thread-name-repairs-db-title");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            first_user_message TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, first_user_message, preview)
         VALUES('thread-name-thread', '', 1, 1, 'codex', '/tmp', 'Untitled', '请根据上面的计划修复所有线上问题，并完整验收。', '')",
        [],
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        json!({"id":"thread-name-thread","thread_name":"更新"}).to_string(),
    )
    .unwrap();

    let row = list_threads(&CodexPaths::new(&root), false, None, 10)
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "thread-name-thread")
        .unwrap();

    assert_eq!(row.title, "更新");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn session_index_thread_name_repairs_db_title_copied_from_first_user_message() {
    let root = unique_temp_dir("session-index-thread-name-repairs-copied-title");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            first_user_message TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    let copied_title = "接手这个线程的工作 00000000-0000-4000-8000-000000000102，检查示例工作区中各模块的职责边界与调用流程，有没有模块间的标准调用约定部分";
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, first_user_message, preview)
         VALUES('thread-name-thread', '', 1, 1, 'codex', '/tmp', ?1, ?1, '')",
        [copied_title],
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        json!({"id":"thread-name-thread","thread_name":"更新"}).to_string(),
    )
    .unwrap();

    let row = list_threads(&CodexPaths::new(&root), false, None, 10)
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "thread-name-thread")
        .unwrap();

    assert_eq!(row.title, "更新");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn session_index_path_finds_rollout_title_when_filename_omits_thread_id() {
    let root = unique_temp_dir("session-index-rollout-path");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("unrelated-file-name.jsonl");
    fs::write(
        &rollout,
        [
            json!({"session_meta":{"payload":{"title":"Untitled"}}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"标题来自 session index 指向的 rollout"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        json!({"id":"thread-id-not-in-filename","path":rollout}).to_string(),
    )
    .unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES('thread-id-not-in-filename', '', 1, 1, 'codex', '/tmp', 'Untitled', '')",
        [],
    )
    .unwrap();

    let row = list_threads(&CodexPaths::new(&root), false, None, 10)
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "thread-id-not-in-filename")
        .unwrap();

    assert_eq!(row.rollout_path.as_deref(), Some(rollout.as_path()));
    assert_eq!(row.title, "标题来自 session index 指向的 rollout");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn plan_and_assistant_preview_do_not_override_real_user_title() {
    let root = unique_temp_dir("plan-preview-title");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-plan-preview.jsonl");
    fs::write(
        &rollout,
        [
            json!({"session_meta":{"payload":{"title":"Assistant preview: This is a very long assistant response body that should not become a thread title because it is only preview text."}}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>实现计划不应成为标题</proposed_plan>"}]}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"Assistant preview: This is a long assistant body and must not override the user request."}]}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"真实用户标题应该保留"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES('plan-preview-thread', ?1, 1, 1, 'codex', '/tmp', 'Untitled', 'Assistant preview: This is a long assistant preview and must not become the title.')",
        [rollout.display().to_string()],
    )
    .unwrap();

    let row = list_threads(&CodexPaths::new(&root), false, None, 10)
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "plan-preview-thread")
        .unwrap();

    assert_eq!(row.title, "真实用户标题应该保留");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn archived_thread_is_hidden_from_default_and_all_lists_but_visible_in_archived_list() {
    let root = unique_temp_dir("archived-hidden-default");
    fs::create_dir_all(&root).unwrap();
    let active_rollout = root.join("active-rollout.jsonl");
    fs::write(&active_rollout, "").unwrap();
    let archived_rollout = root.join("archived-rollout.jsonl");
    fs::write(
        &archived_rollout,
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-active"}})
            .to_string(),
    )
    .unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            archived INTEGER NOT NULL DEFAULT 0,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, preview)
         VALUES('active-thread', ?1, 1, 2, 'codex', '', '/tmp', 'active', '', '', 0, '')",
        [active_rollout.display().to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, preview)
         VALUES('archived-thread', ?1, 1, 1, 'codex', '', '/tmp', 'archived', '', '', 1, '')",
        [archived_rollout.display().to_string()],
    )
    .unwrap();

    let default_rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let archived_rows = list_threads(&CodexPaths::new(&root), true, None, 10).unwrap();

    assert!(default_rows.iter().all(|row| row.id != "archived-thread"));
    assert!(archived_rows.iter().any(|row| row.id == "archived-thread"));
    assert!(thread_detail(&CodexPaths::new(&root), "archived-thread")
        .unwrap()
        .is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn list_threads_sanitizes_proposed_plan_latest_message_but_keeps_reply_needed() {
    let root = unique_temp_dir("thread-plan-latest-message");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    write_thread_fixture(
        &conn,
        &root,
        "plan-thread",
        "真实标题",
        "",
        &[
            json!({"type":"item_completed","turn_id":"turn-plan","item":{"id":"plan-item","type":"Plan"}}),
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 修复计划\n- 处理标题\n</proposed_plan>"}]}}),
        ],
        1,
    );

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, ThreadStatus::ReplyNeeded);
    let latest = rows[0].latest_message.as_deref().unwrap();
    assert!(latest.contains("# 修复计划"));
    assert!(!latest.contains("<proposed_plan>"));
    assert!(!latest.contains("</proposed_plan>"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn request_user_input_status_takes_priority_over_running_turn() {
    let root = unique_temp_dir("request-user-input-priority");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    write_thread_fixture(
        &conn,
        &root,
        "choice-thread",
        "选择线程",
        "",
        &[
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-choice"}}),
            json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"choice-1","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
            json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
        ],
        1,
    );

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, ThreadStatus::ReplyNeeded);
    assert_eq!(rows[0].active_turn_id.as_deref(), Some("turn-choice"));
    assert!(rows[0].pending_elicitation.is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn db_metadata_subagent_thread_is_hidden_from_list() {
    let root = unique_temp_dir("db-subagent-hidden");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-subagent.jsonl");
    fs::write(&rollout, "").unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            thread_source TEXT NOT NULL,
            parent_thread_id TEXT,
            agent_nickname TEXT,
            agent_role TEXT,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, thread_source, parent_thread_id, agent_nickname, agent_role, cwd, title)
         VALUES('child-thread', ?1, 1, 1, 'subagent', 'parent-thread', 'worker', 'explorer', '/tmp', 'worker')",
        [rollout.display().to_string()],
    )
    .unwrap();

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();

    assert!(rows.iter().all(|row| row.id != "child-thread"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn hidden_thread_ids_exports_state_db_subagent_metadata_for_app_server_pruning() {
    let root = unique_temp_dir("db-hidden-thread-ids");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            thread_source TEXT NOT NULL,
            source TEXT,
            parent_thread_id TEXT,
            agent_nickname TEXT,
            agent_role TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, thread_source, source, parent_thread_id, agent_nickname, agent_role)
         VALUES('main-thread', 'user', 'vscode', NULL, NULL, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, thread_source, source, parent_thread_id, agent_nickname, agent_role)
         VALUES('child-thread', 'subagent', '{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"main-thread\"}}}', 'main-thread', 'worker', 'explorer')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, thread_source, source, parent_thread_id, agent_nickname, agent_role)
         VALUES('child-source-json', 'user', '{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"main-thread\"}}}', NULL, NULL, NULL)",
        [],
    )
    .unwrap();

    let hidden = hidden_thread_ids(&CodexPaths::new(&root)).unwrap();

    assert!(hidden.contains("child-thread"));
    assert!(hidden.contains("child-source-json"));
    assert!(!hidden.contains("main-thread"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn archived_thread_ids_exports_state_db_archived_status_for_injection_guards() {
    let root = unique_temp_dir("db-archived-thread-ids");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            archived INTEGER,
            archived_at INTEGER
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, archived, archived_at)
         VALUES('recent-thread', 0, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, archived, archived_at)
         VALUES('flag-archived', 1, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, archived, archived_at)
         VALUES('time-archived', 0, 1710000000)",
        [],
    )
    .unwrap();

    let archived = archived_thread_ids(&CodexPaths::new(&root)).unwrap();

    assert!(archived.contains("flag-archived"));
    assert!(archived.contains("time-archived"));
    assert!(!archived.contains("recent-thread"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn main_thread_with_interactive_source_metadata_remains_visible() {
    let root = unique_temp_dir("db-main-source-visible");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-main.jsonl");
    fs::write(
        &rollout,
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"main answer"}]}})
            .to_string(),
    )
    .unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source_kind TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source_kind, cwd, title)
         VALUES('main-thread', ?1, 1, 1, 'cli', '/tmp', 'main')",
        [rollout.display().to_string()],
    )
    .unwrap();

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "main-thread");
    assert_eq!(rows[0].latest_message.as_deref(), Some("main answer"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn exec_readonly_verification_threads_are_hidden_from_main_list() {
    let root = unique_temp_dir("db-internal-exec-hidden");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-internal.jsonl");
    fs::write(&rollout, "").unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            thread_source TEXT NOT NULL,
            has_user_event INTEGER NOT NULL,
            first_user_message TEXT NOT NULL,
            preview TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, thread_source, has_user_event, first_user_message, preview, cwd, title)
         VALUES('internal-exec', ?1, 1, 2, 'exec', 'user', 0, '只读验证任务。不要修改文件。使用 tool_search 查询 spawn_agent。', '只读验证任务。', '/tmp', '只读验证任务。不要修改文件。')",
        [rollout.display().to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, thread_source, has_user_event, first_user_message, preview, cwd, title)
         VALUES('internal-subagent-prompt', ?1, 1, 2, 'exec', 'user', 0, '', '', '/tmp', '你是子代理 A，必须使用 gpt-5.5 和 xhigh。')",
        [rollout.display().to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, thread_source, has_user_event, first_user_message, preview, cwd, title)
         VALUES('main-thread', ?1, 1, 1, 'vscode', 'user', 0, '接手这个线程的工作，修复项目。', '接手这个线程的工作，修复项目。', '/tmp', 'example-user')",
        [rollout.display().to_string()],
    )
    .unwrap();

    let rows = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    let hidden = hidden_thread_ids(&CodexPaths::new(&root)).unwrap();
    let counts = thread_source_counts(&CodexPaths::new(&root)).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "main-thread");
    assert!(hidden.contains("internal-exec"));
    assert!(hidden.contains("internal-subagent-prompt"));
    assert_eq!(counts.get("internal").copied(), Some(2));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn thread_detail_keeps_completed_tool_history_in_order() {
    let mut events = Vec::new();
    events.push(json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"starting"}]}}));
    for index in 0..90 {
        events.push(json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":format!("call-{index}"),"arguments":{"cmd":"pwd"}}}));
        events.push(json!({"type":"response_item","payload":{"type":"function_call_output","call_id":format!("call-{index}"),"output":format!("out-{index}")}}));
    }

    let detail = detail_fixture(&events);

    let tools = detail
        .blocks
        .iter()
        .filter(|block| block.role == "tool")
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 90);
    assert!(detail
        .blocks
        .iter()
        .all(|block| block.id != "completed-tool-history-collapsed"));
    assert_eq!(
        tools.first().and_then(|block| block.call_id.as_deref()),
        Some("call-0")
    );
    assert_eq!(
        tools.last().and_then(|block| block.call_id.as_deref()),
        Some("call-89")
    );
    assert!(detail
        .blocks
        .iter()
        .any(|block| block.text.as_deref() == Some("out-89")));
}

#[test]
fn thread_detail_preserves_call_position_with_parallel_and_pending_tools() {
    let events = vec![
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"before"}]}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"first","arguments":{"cmd":"pwd"}}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"read_file","call_id":"second","arguments":{"path":"README.md"}}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"between"}]}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"second","output":"second output"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"first","output":"first output"}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"pending","arguments":{"cmd":"tail example.log"}}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"after"}]}}),
    ];
    let detail = detail_fixture(&events);
    let order = detail
        .blocks
        .iter()
        .map(|block| {
            block
                .call_id
                .as_deref()
                .or(block.text.as_deref())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        vec!["before", "first", "second", "between", "pending", "after"]
    );
    let latest = window_thread_detail(detail.clone(), Some(3), None);
    assert_eq!(latest.blocks.len(), 3);
    let earlier = window_thread_detail(detail, Some(3), latest.before_cursor.as_deref());
    assert_eq!(earlier.blocks[1].call_id.as_deref(), Some("first"));
    assert!(!earlier.has_more_blocks);
}

#[test]
fn thread_detail_collapses_old_chat_history_but_keeps_recent_and_running_blocks() {
    let mut events = Vec::new();
    events.push(json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"live-call","arguments":{"cmd":"tail -f log"}}}));
    for index in 0..180 {
        events.push(json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":format!("assistant message {index}")}]}}));
    }

    let detail = detail_fixture(&events);

    assert!(detail.blocks.len() < 100);
    assert!(detail
        .blocks
        .iter()
        .any(|block| block.id == "chat-history-collapsed"));
    assert!(!detail
        .blocks
        .iter()
        .any(|block| block.text.as_deref() == Some("assistant message 0")));
    assert!(detail
        .blocks
        .iter()
        .any(|block| block.text.as_deref() == Some("assistant message 179")));
    assert!(detail.blocks.iter().any(|block| {
        block.role == "tool"
            && block.status.as_deref() == Some("running")
            && block.call_id.as_deref() == Some("live-call")
    }));
}

#[test]
fn window_thread_detail_returns_latest_window_with_cursor() {
    let events = (0..6)
        .map(|index| json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":format!("message-{index}")}]}}))
        .collect::<Vec<_>>();
    let window = window_thread_detail(detail_fixture(&events), Some(2), None);

    assert_eq!(window.total_blocks, 6);
    assert!(window.has_more_blocks);
    assert_eq!(window.before_cursor.as_deref(), Some("b:4"));
    assert!(window.messages.is_empty());
    assert_eq!(window.blocks.len(), 2);
    assert_eq!(window.blocks[0].text.as_deref(), Some("message-4"));
    assert_eq!(window.blocks[1].text.as_deref(), Some("message-5"));
}

#[test]
fn window_thread_detail_uses_before_cursor_for_older_window() {
    let events = (0..6)
        .map(|index| json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":format!("message-{index}")}]}}))
        .collect::<Vec<_>>();
    let window = window_thread_detail(detail_fixture(&events), Some(2), Some("b:4"));

    assert_eq!(window.total_blocks, 6);
    assert!(window.has_more_blocks);
    assert_eq!(window.before_cursor.as_deref(), Some("b:2"));
    assert_eq!(window.blocks.len(), 2);
    assert_eq!(window.blocks[0].text.as_deref(), Some("message-2"));
    assert_eq!(window.blocks[1].text.as_deref(), Some("message-3"));
}

#[test]
fn window_thread_detail_returns_empty_at_start_cursor() {
    let events = (0..3)
        .map(|index| json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":format!("message-{index}")}]}}))
        .collect::<Vec<_>>();
    let window = window_thread_detail(detail_fixture(&events), Some(2), Some("b:0"));

    assert_eq!(window.total_blocks, 3);
    assert!(!window.has_more_blocks);
    assert_eq!(window.before_cursor, None);
    assert!(window.messages.is_empty());
    assert!(window.blocks.is_empty());
}

#[test]
fn window_thread_detail_ignores_invalid_before_cursor() {
    let events = (0..5)
        .map(|index| json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":format!("message-{index}")}]}}))
        .collect::<Vec<_>>();
    let window = window_thread_detail(detail_fixture(&events), Some(2), Some("not-a-cursor"));

    assert_eq!(window.total_blocks, 5);
    assert!(window.has_more_blocks);
    assert_eq!(window.before_cursor.as_deref(), Some("b:3"));
    assert_eq!(window.blocks.len(), 2);
    assert_eq!(window.blocks[0].text.as_deref(), Some("message-3"));
    assert_eq!(window.blocks[1].text.as_deref(), Some("message-4"));
}

#[test]
fn thread_detail_page_resolves_stable_and_legacy_block_cursors() {
    let fixture = ThreadDetailPageFixture::new(&(0..6).map(page_message).collect::<Vec<_>>());

    for compact in [false, true] {
        let latest = fixture.read(compact, Some(2), None);
        assert_eq!(page_block_ids(&latest), vec!["message:4", "message:5"]);
        assert_eq!(latest.before_cursor.as_deref(), Some("b:4"));

        let stable_before = fixture.read(compact, Some(2), Some("b:999:message:3"));
        assert_eq!(
            page_block_ids(&stable_before),
            vec!["message:1", "message:2"]
        );
        assert_eq!(stable_before.before_cursor.as_deref(), Some("b:1"));

        let stable_after = fixture.read(compact, Some(2), Some("a:message:3"));
        assert_eq!(
            page_block_ids(&stable_after),
            vec!["message:2", "message:3"]
        );
        assert_eq!(stable_after.before_cursor.as_deref(), Some("b:2"));

        let legacy_before = fixture.read(compact, Some(2), Some("b:4"));
        assert_eq!(
            page_block_ids(&legacy_before),
            vec!["message:2", "message:3"]
        );

        let missing = fixture.read(compact, Some(2), Some("b:999:missing"));
        assert!(missing.blocks.is_empty());
        assert!(!missing.has_more_blocks);
        assert_eq!(missing.before_cursor, None);
    }
}

#[test]
fn stable_block_cursors_survive_appended_history_compaction_and_repeated_reads() {
    let fixture = ThreadDetailPageFixture::new(&(0..80).map(page_message).collect::<Vec<_>>());
    let initial = fixture.read(true, Some(3), None);
    let before_cursor = format!(
        "{}:{}",
        initial.before_cursor.as_deref().unwrap(),
        initial.blocks.first().unwrap().id
    );
    let after_cursor = format!("a:{}", initial.blocks.last().unwrap().id);

    let initial_before = fixture.read(true, Some(3), Some(&before_cursor));
    let initial_after = fixture.read(true, Some(3), Some(&after_cursor));
    assert_eq!(
        page_block_ids(&initial_before),
        vec!["message:74", "message:75", "message:76"]
    );
    assert_eq!(
        page_block_ids(&initial_after),
        vec!["message:77", "message:78", "message:79"]
    );

    fixture.append(&(80..85).map(page_message).collect::<Vec<_>>());

    let complete = fixture.read(true, None, None);
    assert_eq!(complete.total_blocks, 81);
    assert!(complete
        .blocks
        .iter()
        .any(|block| block.kind == "chat_history_collapsed"));

    let stable_before = fixture.read(true, Some(3), Some(&before_cursor));
    assert_eq!(
        page_block_ids(&stable_before),
        vec!["message:74", "message:75", "message:76"]
    );
    assert_eq!(stable_before.before_cursor.as_deref(), Some("b:70"));

    let stable_after = fixture.read(true, Some(3), Some(&after_cursor));
    assert_eq!(
        page_block_ids(&stable_after),
        vec!["message:77", "message:78", "message:79"]
    );
    assert_eq!(stable_after.before_cursor.as_deref(), Some("b:73"));

    let legacy = fixture.read(true, Some(3), Some("b:77"));
    assert_eq!(
        page_block_ids(&legacy),
        vec!["message:78", "message:79", "message:80"]
    );
    assert_eq!(legacy.before_cursor.as_deref(), Some("b:74"));

    for _ in 0..2 {
        assert_eq!(
            page_block_ids(&fixture.read(true, Some(3), Some(&before_cursor))),
            page_block_ids(&stable_before)
        );
        assert_eq!(
            page_block_ids(&fixture.read(true, Some(3), Some(&after_cursor))),
            page_block_ids(&stable_after)
        );
    }
}

#[test]
fn archived_thread_keeps_archived_status_despite_running_rollout() {
    let root = unique_temp_dir("archived-priority");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("archived-rollout.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"turn_started","turn_id":"turn-active"}).to_string(),
            json!({"type":"response_item","turn_id":"turn-active","payload":{"type":"function_call","name":"request_user_input","call_id":"call-1","arguments":{"questions":[{"id":"q","question":"continue?","options":[{"label":"yes"}]}]}}}).to_string(),
        ].join("\n"),
    )
    .unwrap();
    write_thread_db(&root, "test-thread", &rollout, 1, 1);

    let row = list_threads(&CodexPaths::new(&root), true, None, 10)
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "test-thread")
        .unwrap();

    assert_eq!(row.status, ThreadStatus::Archived);
    assert_eq!(row.active_turn_id.as_deref(), Some("turn-active"));
    assert!(row.pending_elicitation.is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lists_sort_by_last_conversation_message_before_limiting() {
    use std::io::Write;
    let root = unique_temp_dir("message-time-order");
    fs::create_dir_all(&root).unwrap();
    let first = root.join("first.jsonl");
    let second = root.join("second.jsonl");
    let message = |role: &str, time: &str| json!({"type":"response_item","timestamp":time,"payload":{"type":"message","role":role,"content":[{"text":"Fixture message"}]}});
    fs::write(
        &first,
        format!(
            "{}\n{}\n",
            message("assistant", "2026-10-01T11:00:00+08:00"),
            json!({"type":"turn_context","timestamp":"2026-11-01T00:00:00Z","payload":{}})
        ),
    )
    .unwrap();
    fs::write(
        &second,
        format!("{}\n", message("user", "2026-10-01T04:00:00Z")),
    )
    .unwrap();
    write_thread_db(&root, "first", &first, 9999, 0);
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute("INSERT INTO threads SELECT 'second', ?1, created_at, 1, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, preview FROM threads WHERE id = 'first'", [second.to_string_lossy().as_ref()]).unwrap();
    let paths = CodexPaths::new(&root);
    assert_eq!(
        list_threads(&paths, false, None, 1).unwrap()[0].id,
        "second"
    );
    let mut file = fs::OpenOptions::new().append(true).open(&first).unwrap();
    writeln!(file, "{}", json!({"type":"response_item","timestamp":"2026-12-01T00:00:00Z","payload":{"type":"function_call","name":"fixture_tool","call_id":"tool","arguments":"{}"}})).unwrap();
    assert_eq!(
        list_threads(&paths, false, None, 1).unwrap()[0].id,
        "second"
    );
    writeln!(file, "{}", message("user", "2026-10-01T05:00:00Z")).unwrap();
    let list = list_threads(&paths, false, None, 1).unwrap();
    assert_eq!(list[0].id, "first");
    assert_eq!(list[0].updated_at.as_deref(), Some("2026-10-01T05:00:00Z"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scan_rollout_latest_message_uses_last_message_beyond_window() {
    let mut events = Vec::new();
    for index in 0..100 {
        events.push(json!({
            "type":"response_item",
            "payload":{"type":"message","role":"assistant","content":[{"text":format!("message-{index}")}]}
        }));
    }

    let scan = scan_fixture(&events);

    assert_eq!(scan.message_count, 100);
    assert_eq!(scan.latest_message.as_deref(), Some("message-99"));
}

#[test]
fn list_threads_full_fetch_keeps_running_thread_beyond_small_page() {
    let root = unique_temp_dir("full-fetch-running");
    fs::create_dir_all(&root).unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            archived INTEGER NOT NULL DEFAULT 0,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    for index in 0..20 {
        let rollout = root.join(format!("rollout-recent-{index}.jsonl"));
        fs::write(&rollout, "").unwrap();
        conn.execute(
            "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, preview)
             VALUES(?1, ?2, 1, ?3, 'codex', '', '/tmp', ?1, '', '', 0, '')",
            (
                format!("recent-{index}"),
                rollout.display().to_string(),
                10_000 + index,
            ),
        )
        .unwrap();
    }
    let running_rollout = root.join("rollout-running-old.jsonl");
    fs::write(
        &running_rollout,
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}})
            .to_string(),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, preview)
         VALUES('running-old', ?1, 1, 1, 'codex', '', '/tmp', 'running-old', '', '', 0, '')",
        [running_rollout.display().to_string()],
    )
    .unwrap();

    let first_page = list_threads(&CodexPaths::new(&root), false, None, 10).unwrap();
    assert!(!first_page.iter().any(|thread| thread.id == "running-old"));
    let running = list_threads(&CodexPaths::new(&root), false, None, usize::MAX)
        .unwrap()
        .into_iter()
        .filter(|thread| matches!(thread.status, ThreadStatus::Running))
        .collect::<Vec<_>>();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].id, "running-old");
    assert_eq!(running[0].active_turn_id.as_deref(), Some("turn-live"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scan_rollout_running_when_turn_started_without_completion() {
    let scan = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-running"}),
        json!({"type":"response_item","turn_id":"turn-running","payload":{"type":"message","role":"assistant","content":[{"text":"working"}]}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-running"));
}

#[test]
fn scan_rollout_running_when_event_msg_task_started_without_turn_started() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_complete"}}),
        json!({"type":"event_msg","payload":{"type":"task_started"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"continue"}]}}),
    ]);

    assert!(scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_event_msg_task_started_uses_payload_turn_id_as_active_turn() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"done"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"working"}]}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_task_complete_for_prior_turn_does_not_clear_newer_task() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"done"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"still working"}]}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_latest_task_complete_clears_stale_older_task() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-stale"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-latest"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-latest","last_agent_message":"done"}}),
    ]);

    assert!(!scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_named_task_complete_clears_prior_anonymous_task_started() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-live","last_agent_message":"done"}}),
    ]);

    assert!(!scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_prior_named_task_complete_preserves_newer_anonymous_task_started() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"event_msg","payload":{"type":"task_started"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"done"}}),
    ]);

    assert!(scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_turn_completed_for_prior_turn_does_not_clear_active_turn() {
    let scan = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-live"}),
        json!({"type":"turn_completed","turn_id":"turn-old"}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"message","role":"assistant","content":[{"text":"still working"}]}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_example_task_style_live_task_stays_running_after_completed_tools() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":null}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live","model_context_window":258400}}),
        json!({"type":"turn_context","payload":{"turn_id":"turn-live","cwd":"/home/ubuntu/codex-workspace","model":"gpt-5.5"}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"custom_tool_call","status":"completed","call_id":"call-edit","name":"apply_patch","input":"*** Begin Patch"}}),
        json!({"type":"event_msg","payload":{"type":"patch_apply_end","turn_id":"turn-live","call_id":"call-edit","success":true}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"custom_tool_call_output","call_id":"call-edit","output":"Success"}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call","name":"exec_command","call_id":"call-test","arguments":{"cmd":"python3 -m unittest"}}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call_output","call_id":"call-test","output":"OK"}}),
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"继续处理中","phase":"commentary"}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
    assert_eq!(scan.latest_message.as_deref(), Some("继续处理中"));
}

#[test]
fn scan_rollout_later_active_turn_suppresses_old_null_completion_recoverable() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","status":"interrupted","last_agent_message":null}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call","name":"exec_command","call_id":"call-live","arguments":{"cmd":"cargo test"}}}),
    ]);

    assert!(scan.running);
    assert!(!scan.recoverable);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_anonymous_task_complete_does_not_clear_explicit_active_turn() {
    let scan = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-live"}),
        json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"old done"}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"message","role":"assistant","content":[{"text":"still working"}]}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_nested_event_msg_payload_paths_provide_task_turn_id() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"event":{"type":"task_started","turnId":"turn-live"}}}),
        json!({"type":"event_msg","payload":{"event_type":"token_count","payload":{"turn_id":"turn-live"}}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_external_example_task_fixture_when_provided() {
    let Ok(path) = env::var("XIANBAO_ROLLOUT_FIXTURE") else {
        return;
    };
    let scan = scan_rollout(Path::new(&path), 80).unwrap();

    assert!(
        scan.running,
        "expected external example_task fixture to be running"
    );
    assert_eq!(
        scan.active_turn_id.as_deref(),
        Some("00000000-0000-4000-8000-000000000103")
    );
}

#[test]
fn scan_rollout_external_ld_fixture_when_provided() {
    let Ok(path) = env::var("LD_ROLLOUT_FIXTURE") else {
        return;
    };
    let scan = scan_rollout(Path::new(&path), 80).unwrap();

    assert!(
        !scan.running,
        "expected external LD fixture to be completed"
    );
    assert!(
        !scan.recoverable,
        "expected external LD fixture to end with a successful task_complete"
    );
    assert_eq!(scan.active_turn_id, None);
    assert!(scan.pending_elicitation.is_none());
}

#[test]
fn scan_rollout_later_successful_task_complete_clears_stale_recoverable() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":null}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-latest"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-latest","last_agent_message":"done"}}),
    ]);

    assert!(!scan.running);
    assert!(!scan.recoverable);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_event_msg_task_complete_clears_task_started_running() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"working"}]}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done"}}),
    ]);

    assert!(!scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_event_msg_turn_aborted_clears_task_started_running() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-abort"}}),
        json!({"type":"turn_started","turn_id":"turn-abort"}),
        json!({"type":"response_item","turn_id":"turn-abort","payload":{"type":"function_call","name":"exec_command","call_id":"call-abort","arguments":{"cmd":"sleep 10"}}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<turn_aborted>"}]}}),
        json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-abort","reason":"interrupted"}}),
    ]);

    assert!(!scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_slash_turn_aborted_clears_running() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-abort"}}),
        json!({"type":"turn_started","turn_id":"turn-abort"}),
        json!({"type":"turn/aborted","turn_id":"turn-abort","reason":"interrupted"}),
    ]);

    assert!(!scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[test]
fn scan_rollout_running_when_tool_call_has_no_output_without_turn_started() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call","name":"exec_command","call_id":"call-live","arguments":{"cmd":"sleep 10"}}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_task_complete_clears_wait_agent_pending_tool_for_same_turn() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-main"}}),
        json!({"type":"response_item","turn_id":"turn-main","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-main","last_agent_message":"主线程完成。"}}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
    assert!(!scan.recoverable);
}

#[test]
fn scan_rollout_latest_task_complete_retires_prior_pending_tools() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old-a"}}),
        json!({"type":"response_item","turn_id":"turn-old-a","payload":{"type":"function_call","name":"wait","call_id":"wait-old-a","arguments":{"cell_id":"cell-old-a"}}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old-b"}}),
        json!({"type":"response_item","turn_id":"turn-old-b","payload":{"type":"function_call","name":"exec_command","call_id":"exec-old-b","arguments":{"cmd":"sleep 10"}}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-latest"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-latest","last_agent_message":"最新 turn 已完成。"}}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
    assert!(!scan.recoverable);
}

#[test]
fn scan_rollout_latest_turn_completed_retires_prior_pending_tools() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"response_item","turn_id":"turn-old","payload":{"type":"function_call","name":"wait","call_id":"wait-old","arguments":{"cell_id":"cell-old"}}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-latest"}}),
        json!({"type":"turn_completed","turn_id":"turn-latest"}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
}

#[test]
fn scan_rollout_latest_turn_aborted_retires_prior_pending_tools() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"response_item","turn_id":"turn-old","payload":{"type":"function_call","name":"wait","call_id":"wait-old","arguments":{"cell_id":"cell-old"}}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-latest"}}),
        json!({"type":"turn_aborted","turn_id":"turn-latest","reason":"interrupted"}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
}

#[test]
fn scan_rollout_unmatched_named_terminal_preserves_anonymous_pending_tool() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","payload":{"type":"function_call","name":"wait","call_id":"wait-anonymous","arguments":{"cell_id":"cell-anonymous"}}}),
        json!({"type":"turn_completed","turn_id":"turn-not-active"}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id, None);
}

#[test]
fn scan_rollout_anonymous_terminal_conservatively_clears_pending_tools() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","payload":{"type":"function_call","name":"wait","call_id":"wait-anonymous","arguments":{"cell_id":"cell-anonymous"}}}),
        json!({"type":"turn_completed"}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
}

#[test]
fn scan_rollout_task_complete_clears_anonymous_wait_agent_pending_tool_for_same_turn() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-main"}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-main","last_agent_message":"主线程完成。"}}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
    assert!(!scan.recoverable);
}

#[test]
fn scan_rollout_turn_completed_clears_wait_agent_pending_tool_for_same_turn() {
    let scan = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-main"}),
        json!({"type":"response_item","turn_id":"turn-main","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
        json!({"type":"turn_completed","turn_id":"turn-main"}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
}

#[test]
fn scan_rollout_turn_completed_clears_anonymous_custom_tool_pending_for_same_turn() {
    let scan = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-main"}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"custom-1","input":"*** Begin Patch"}}),
        json!({"type":"turn_completed","turn_id":"turn-main"}),
    ]);

    assert!(!scan.running);
    assert_eq!(scan.active_turn_id, None);
}

#[test]
fn scan_rollout_task_complete_does_not_clear_newer_running_turn() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-live","arguments":{"targets":["agent-live"]}}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"旧 turn 完成。"}}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_old_turn_completed_preserves_newer_pending_tool() {
    let scan = scan_fixture(&[
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}),
        json!({"type":"response_item","turn_id":"turn-old","payload":{"type":"function_call","name":"wait","call_id":"wait-old","arguments":{"cell_id":"cell-old"}}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}),
        json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"function_call","name":"wait","call_id":"wait-live","arguments":{"cell_id":"cell-live"}}}),
        json!({"type":"turn_completed","turn_id":"turn-old"}),
    ]);

    assert!(scan.running);
    assert_eq!(scan.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn scan_rollout_request_user_input_still_reply_needed_until_cleared() {
    let waiting = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-choice"}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"choice-1","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
    ]);
    assert!(waiting.reply_needed);
    assert!(waiting.pending_elicitation.is_some());

    let cleared = scan_fixture(&[
        json!({"type":"turn_started","turn_id":"turn-choice"}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"choice-1","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}),
        json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call_output","call_id":"choice-1","output":"{\"choice\":[\"A\"]}"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-choice","last_agent_message":"选择已处理。"}}),
    ]);
    assert!(!cleared.reply_needed);
    assert!(cleared.pending_elicitation.is_none());
    assert!(!cleared.running);
}

#[test]
fn scan_rollout_completed_tool_call_without_turn_started_is_recent() {
    let scan = scan_fixture(&[
        json!({"type":"response_item","turn_id":"turn-done","payload":{"type":"function_call","name":"exec_command","call_id":"call-done","arguments":{"cmd":"pwd"}}}),
        json!({"type":"response_item","turn_id":"turn-done","payload":{"type":"function_call_output","call_id":"call-done","output":"/tmp"}}),
    ]);

    assert!(!scan.running);
    assert!(scan.active_turn_id.is_none());
}

#[tokio::test]
async fn set_thread_title_rejects_blank_without_touching_native_state() {
    let root = unique_temp_dir("set-title");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(&rollout, "").unwrap();
    write_thread_db(&root, "thread-a", &rollout, 1, 0);

    let before = fs::read(root.join("state_5.sqlite")).unwrap();
    assert!(set_thread_title(&CodexPaths::new(&root), "thread-a", "   ")
        .await
        .is_err());
    assert_eq!(fs::read(root.join("state_5.sqlite")).unwrap(), before);
    assert!(!root.join("session_index.jsonl").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn thread_names_follow_native_history_mode() {
    for (mode, expected) in [
        ("legacy", "Native legacy name"),
        ("paginated", "Native paginated name"),
    ] {
        let root = unique_temp_dir("history-mode-title");
        fs::create_dir_all(&root).unwrap();
        let rollout = root.join("rollout-thread-a.jsonl");
        fs::write(&rollout, "").unwrap();
        write_thread_db(&root, "thread-a", &rollout, 1, 0);
        let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
        conn.execute_batch("ALTER TABLE threads ADD COLUMN name TEXT; ALTER TABLE threads ADD COLUMN history_mode TEXT;").unwrap();
        conn.execute("UPDATE threads SET title='Native legacy name', name='Native paginated name', history_mode=?1", [mode]).unwrap();
        drop(conn);
        let rows = list_threads(&CodexPaths::new(&root), false, Some("thread-a"), 10).unwrap();
        assert_eq!(rows[0].title, expected, "{mode}");
        if mode == "legacy" {
            let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
            conn.execute("UPDATE threads SET title=''", []).unwrap();
            drop(conn);
            fs::write(
                root.join("session_index.jsonl"),
                json!({"id":"thread-a","thread_name":"Canonical index name"}).to_string(),
            )
            .unwrap();
            let rows = list_threads(&CodexPaths::new(&root), false, Some("thread-a"), 10).unwrap();
            assert_eq!(rows[0].title, "Canonical index name");
        }
        fs::remove_dir_all(root).unwrap();
    }
}

fn scan_fixture(events: &[serde_json::Value]) -> super::RolloutScan {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-test-{}-{}-{}.jsonl",
        std::process::id(),
        counter,
        events.len()
    ));
    let text = events
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, text).unwrap();
    let scan = scan_rollout(&path, 80).unwrap();
    let _ = fs::remove_file(path);
    scan
}

fn rollout_fixture_path(label: &str, events: &[serde_json::Value]) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!(
        "agenthub-rollout-{label}-{}-{counter}.jsonl",
        std::process::id()
    ));
    fs::write(
        &path,
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    path
}

fn detail_fixture(events: &[serde_json::Value]) -> super::ThreadDetail {
    let root = unique_temp_dir("detail");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-test-thread.jsonl");
    let text = events
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&rollout, text).unwrap();
    fs::write(
        root.join("session_index.jsonl"),
        json!({"id":"test-thread","path":rollout}).to_string(),
    )
    .unwrap();
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, preview)
         VALUES('test-thread', '', 1, 1, 'codex', '', '/tmp', 'test', '', '', '')",
        [],
    )
    .unwrap();

    let detail = thread_detail(&CodexPaths::new(&root), "test-thread")
        .unwrap()
        .unwrap();
    let _ = fs::remove_dir_all(root);
    detail
}

struct ThreadDetailPageFixture {
    root: PathBuf,
    summary: super::ThreadSummary,
}

impl ThreadDetailPageFixture {
    fn new(events: &[serde_json::Value]) -> Self {
        let root = unique_temp_dir("detail-page");
        fs::create_dir_all(&root).unwrap();
        let rollout = root.join("rollout-test-thread.jsonl");
        let text = events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&rollout, format!("{text}\n")).unwrap();
        let summary = super::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "fixture-thread".to_string(),
            title: "fixture history".to_string(),
            status: super::ThreadStatus::Recent,
            updated_at: None,
            archived_at: None,
            message_count: events.len(),
            latest_message: None,
            cwd: Some("/tmp".to_string()),
            model: None,
            rollout_path: Some(rollout),
            active_turn_id: None,
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: None,
        };
        Self { root, summary }
    }

    fn read(
        &self,
        compact: bool,
        limit: Option<usize>,
        before: Option<&str>,
    ) -> super::ThreadDetail {
        super::thread_detail_page_from_summary(self.summary.clone(), compact, limit, before, None)
            .unwrap()
    }

    fn append(&self, events: &[serde_json::Value]) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(self.summary.rollout_path.as_ref().unwrap())
            .unwrap();
        let text = events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        writeln!(file, "{text}").unwrap();
    }
}

impl Drop for ThreadDetailPageFixture {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.root).is_err() {
            eprintln!("CODEX_DETAIL_PAGE_FIXTURE_CLEANUP_FAILED");
        }
    }
}

fn page_message(index: usize) -> serde_json::Value {
    json!({
        "type":"response_item",
        "payload":{"type":"message","id":format!("message:{index}"),"role":"assistant","content":[{"text":format!("message-{index}")}]}
    })
}

fn page_block_ids(detail: &super::ThreadDetail) -> Vec<&str> {
    detail
        .blocks
        .iter()
        .map(|block| block.id.as_str())
        .collect()
}

fn write_thread_db(
    root: &Path,
    thread_id: &str,
    rollout: &std::path::Path,
    updated_at: i64,
    archived: i64,
) {
    let conn = Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            archived INTEGER NOT NULL DEFAULT 0,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, preview)
         VALUES(?1, ?2, 1, ?3, 'codex', '', '/tmp', 'test', '', '', ?4, '')",
        (thread_id, rollout.display().to_string(), updated_at, archived),
    )
    .unwrap();
}

fn write_thread_fixture(
    conn: &Connection,
    root: &Path,
    thread_id: &str,
    title: &str,
    preview: &str,
    events: &[serde_json::Value],
    updated_at: i64,
) {
    let rollout = root.join(format!("{thread_id}.jsonl"));
    let text = events
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&rollout, text).unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES(?1, ?2, 1, ?3, 'codex', '/tmp', ?4, ?5)",
        (
            thread_id,
            rollout.display().to_string(),
            updated_at,
            title,
            preview,
        ),
    )
    .unwrap();
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "agenthub-{label}-{}-{}-{}",
        std::process::id(),
        counter,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}

fn mark_codex_home(home: &Path) {
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::write(home.join("state_5.sqlite"), b"").unwrap();
    fs::write(home.join("session_index.jsonl"), b"").unwrap();
    fs::create_dir_all(home.join("app-server-control")).unwrap();
}

fn fallback_codex_home(root: &Path) -> PathBuf {
    root.join("root/.codex")
}
