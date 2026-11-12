//! Local Claude hook ingestion. Persist first; the existing monitor owns retries.
use agenthub_core::claude::{self, hooks, ClaudePaths};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct HookEvent {
    pub session_id: String,
    pub prompt_id: Option<String>,
    pub path: PathBuf,
    pub kind: String,
    pub body: String,
    pub idle: bool,
    pub received_ms: i64,
    pub identity: Option<String>,
}

pub(super) fn installed() -> bool {
    std::env::current_exe()
        .is_ok_and(|binary| hooks::configured(&ClaudePaths::default_for_user(), &binary))
}

/// The Stop payload lists in-flight background work so a hook can tell a
/// finished session from one paused until background work wakes it. Delegated
/// agents and workflows continue this thread; shells and monitors do not.
fn stop_awaits_background(value: &Value) -> bool {
    value["background_tasks"].as_array().is_some_and(|tasks| {
        tasks
            .iter()
            .any(|task| matches!(task["type"].as_str().unwrap_or(""), "subagent" | "workflow"))
    })
}

pub(super) fn parse(value: &Value) -> Result<Option<HookEvent>> {
    let get = |key| value[key].as_str().unwrap_or("");
    let notification = get("notification_type");
    // A hook fired inside a subagent describes that subagent's transcript, not
    // this session's turn.
    if !get("agent_id").is_empty() && matches!(get("hook_event_name"), "Stop" | "StopFailure") {
        return Ok(None);
    }
    let (kind, body, idle) = match (get("hook_event_name"), notification) {
        ("Stop", _) => (
            "completion",
            get("last_assistant_message").to_string(),
            false,
        ),
        ("Notification", "permission_prompt") => {
            ("authorization", get("message").to_string(), false)
        }
        ("Notification", "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input") => {
            ("reply_needed", get("message").to_string(), false)
        }
        ("Notification", "idle_prompt") => ("completion", String::new(), true),
        ("StopFailure", _) => (
            "failure",
            format!(
                "{}，请打开线程查看详情。",
                match get("error") {
                    "rate_limit" => "请求达到速率限制",
                    "overloaded" => "服务暂时繁忙",
                    "authentication_failed"
                    | "oauth_org_not_allowed"
                    | "cloud_credential_error" => "身份验证失败",
                    "billing_error" | "account_on_hold" => "账户暂时不可用",
                    "invalid_request" => "请求未被接受",
                    "model_not_found" => "模型不可用",
                    "server_error" => "服务出现错误",
                    "max_output_tokens" => "输出达到上限",
                    _ => "回合因 API 错误结束",
                }
            ),
            false,
        ),
        _ => return Ok(None),
    };
    if get("hook_event_name") == "Stop" && stop_awaits_background(value) {
        return Ok(None);
    }
    ensure!(
        !get("session_id").is_empty() && get("session_id").len() <= 256,
        "invalid hook identity"
    );
    let path = PathBuf::from(get("transcript_path"));
    let root = ClaudePaths::default_for_user().projects;
    let canonical_root = root.canonicalize().ok();
    ensure!(
        path.is_absolute()
            && std::iter::once(root.as_path())
                .chain(canonical_root.as_deref())
                .any(|root| path
                    .strip_prefix(root)
                    .is_ok_and(|p| p.components().count() == 2
                        && p.components()
                            .all(|c| matches!(c, std::path::Component::Normal(_)))))
            && path.extension().is_some_and(|e| e == "jsonl"),
        "invalid hook source"
    );
    let prompt = get("prompt_id");
    ensure!(prompt.len() <= 256, "invalid prompt identity");
    Ok(Some(HookEvent {
        session_id: get("session_id").into(),
        prompt_id: (!prompt.is_empty()).then(|| prompt.into()),
        identity: agenthub_core::native_probe::file_identity(&path).ok(),
        path,
        kind: kind.into(),
        body: super::notification_format::summary(&body),
        idle,
        received_ms: chrono::Utc::now().timestamp_millis(),
    }))
}

pub(super) fn context(hook: &HookEvent) -> Result<claude::ClaudeNotificationContext> {
    let context = claude::notification_context(&ClaudePaths::default_for_user(), &hook.path)?;
    ensure!(
        context.stream.id == hook.session_id
            && hook
                .identity
                .as_ref()
                .is_none_or(|id| id == &context.stream.identity),
        "hook source identity changed"
    );
    let prompt = hook
        .prompt_id
        .as_ref()
        .and_then(|id| context.prompts.get(id));
    ensure!(
        prompt.is_some_and(|p| !p.ambiguous),
        "hook prompt identity is not yet verified"
    );
    Ok(context)
}

pub(super) async fn run(config_path: &Path) -> Result<()> {
    let cache = super::monitor_start::DaemonMonitorCache::default();
    let load_config = || cache.load_config(config_path);
    let config = load_config()?;
    let paths = ClaudePaths::default_for_user();
    ensure!(
        hooks::installed(&paths, &std::env::current_exe()?, config_path),
        "managed hook is not installed"
    );
    let db = super::open_panel_db(&config)?;
    let bytes = tokio::task::spawn_blocking(|| {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    })
    .await??;
    ensure!(bytes.len() <= 1024 * 1024, "hook input exceeds limit");
    let Some(hook) = parse(&serde_json::from_slice(&bytes)?)? else {
        return Ok(());
    };
    super::notification_delivery::claude_hook(&load_config, &db, hook, Duration::from_secs(3)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_categories_never_include_raw_api_errors() {
        let _env_guard = crate::api::test_support::CONFIG_ENV_LOCK.blocking_lock();
        let root = ClaudePaths::default_for_user().projects;
        let value = serde_json::json!({"hook_event_name":"StopFailure", "error":"rate_limit", "error_details":"private fixture detail", "last_assistant_message":"private fixture error", "session_id":"fixture", "prompt_id":"prompt-fixture", "transcript_path":root.join("fixture/session.jsonl")});
        let event = parse(&value).unwrap().unwrap();
        assert_eq!(event.kind, "failure");
        assert_eq!(event.body, "请求达到速率限制，请打开线程查看详情。");
    }

    #[test]
    fn subagent_stops_and_pauses_for_delegated_work_are_not_session_terminals() {
        let _env_guard = crate::api::test_support::CONFIG_ENV_LOCK.blocking_lock();
        let root = ClaudePaths::default_for_user().projects;
        let base = |event: &str| serde_json::json!({"hook_event_name":event, "error":"rate_limit", "last_assistant_message":"Fixture answer", "session_id":"fixture", "prompt_id":"prompt-fixture", "transcript_path":root.join("fixture/session.jsonl")});
        for event in ["Stop", "StopFailure"] {
            assert!(parse(&base(event)).unwrap().is_some(), "{event}");
            let mut subagent = base(event);
            subagent["agent_id"] = "agent-fixture".into();
            assert!(parse(&subagent).unwrap().is_none(), "{event}");
        }
        for (tasks, terminal) in [
            (serde_json::json!([]), true),
            (
                serde_json::json!([{"id":"b1","type":"shell","status":"running","description":"Fixture wait"}]),
                true,
            ),
            (
                serde_json::json!([{"id":"m1","type":"monitor","status":"running","description":"Fixture monitor"}]),
                true,
            ),
            (
                serde_json::json!([{"id":"a1","type":"subagent","status":"running","description":"Fixture agent","agent_type":"general-purpose"}]),
                false,
            ),
            (
                serde_json::json!([{"id":"b1","type":"shell","status":"running","description":"Fixture wait"},{"id":"w1","type":"workflow","status":"running","description":"Fixture workflow"}]),
                false,
            ),
        ] {
            let mut stop = base("Stop");
            stop["background_tasks"] = tasks.clone();
            assert_eq!(parse(&stop).unwrap().is_some(), terminal, "{tasks}");
        }
        // Only Stop carries the in-flight list; a failure stays a failure.
        let mut failure = base("StopFailure");
        failure["background_tasks"] = serde_json::json!([{"id":"a1","type":"subagent","status":"running","description":"Fixture agent"}]);
        assert_eq!(parse(&failure).unwrap().unwrap().kind, "failure");
    }
}
