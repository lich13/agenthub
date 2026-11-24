use super::{api_error, ApiError};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

pub(super) fn rpc_payload<T: DeserializeOwned>(value: &Value) -> Result<T, ApiError> {
    T::deserialize(value).map_err(|err| api_error(err.to_string()))
}

pub(super) fn rpc_wrapped_payload<T: DeserializeOwned>(
    value: &Value,
    keys: &[&str],
) -> Result<T, ApiError> {
    let payload = keys.iter().find_map(|key| value.get(*key)).unwrap_or(value);
    rpc_payload(payload)
}

pub(super) fn rpc_nested_payload_or_empty<T: DeserializeOwned>(
    value: &Value,
    key: &str,
) -> Result<T, ApiError> {
    match value.get(key) {
        Some(payload) => rpc_payload(payload),
        None => rpc_payload(&json!({})),
    }
}

pub(super) fn rpc_required_string(value: &Value, key: &str) -> Result<String, ApiError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| api_error(format!("{key} is required")))
}

#[cfg(test)]
mod tests {
    use super::{
        rpc_nested_payload_or_empty, rpc_payload, rpc_required_string, rpc_wrapped_payload,
    };
    use crate::api::ApiError;
    use agenthub_core::services::jobs::ThreadRenameRequest;
    use agenthub_core::services::settings as settings_service;
    use serde_json::{json, Value};

    fn must<T>(result: Result<T, ApiError>) -> T {
        match result {
            Ok(value) => value,
            Err(_) => panic!("expected RPC payload parsing to succeed"),
        }
    }

    #[test]
    fn rpc_payload_accepts_current_rename_request_wrapper() {
        let payload: ThreadRenameRequest = must(rpc_wrapped_payload(
            &json!({
                "request": { "thread_id": "thread-a", "name": "Task name" }
            }),
            &["request"],
        ));
        assert_eq!(payload.thread_id, "thread-a");
        assert_eq!(payload.name, "Task name");
    }

    #[test]
    fn rpc_payload_accepts_current_probe_settings_wrapper() {
        let payload: settings_service::ProbeSettingsSaveRequest = must(rpc_wrapped_payload(
            &json!({
            "settings": {
                "probe": {
                    "poll_seconds": 20,
                    "recent_limit": 50
                    },
                    "notifications": {
                        "server_url": "https://example.invalid",
                        "device_key": "bark",
                        "notify_reply_needed": true
                    }
                }
            }),
            &["settings"],
        ));

        assert_eq!(payload.probe.unwrap().poll_seconds, Some(20));
        let notifications = payload.notifications.unwrap();
        assert_eq!(
            notifications.server_url.as_deref(),
            Some("https://example.invalid")
        );
        assert_eq!(notifications.device_key.as_deref(), Some("bark"));
        assert_eq!(notifications.notify_reply_needed, Some(true));
    }

    #[test]
    fn rpc_strings_and_nested_defaults_preserve_parse_errors() {
        assert_eq!(
            must(rpc_required_string(
                &json!({"threadId":"thread-a"}),
                "threadId"
            )),
            "thread-a"
        );
        assert!(
            rpc_required_string(&json!({"payload":{"threadId":"thread-a"}}), "threadId").is_err()
        );
        assert!(rpc_required_string(&json!({"thread_id":"thread-a"}), "threadId").is_err());
        assert_eq!(
            must(rpc_nested_payload_or_empty::<Value>(&json!({}), "request")),
            json!({})
        );
        assert!(rpc_payload::<ThreadRenameRequest>(&Value::Null).is_err());
    }
}
