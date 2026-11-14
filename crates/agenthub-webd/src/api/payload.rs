use super::ApiErrorKind;
use super::{api_error, ApiError};
use crate::rpc_payload::{
    rpc_nested_payload_or_empty as parse_rpc_nested_payload_or_empty,
    rpc_payload as parse_rpc_payload, rpc_query_strings as parse_rpc_query_strings,
    rpc_required_string as parse_rpc_required_string,
    rpc_wrapped_payload as parse_rpc_wrapped_payload, RpcPayloadError,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn rpc_payload<T: DeserializeOwned>(value: &Value) -> Result<T, ApiError> {
    api_rpc_payload(parse_rpc_payload(value))
}

pub(super) fn rpc_wrapped_payload<T: DeserializeOwned>(
    value: &Value,
    keys: &[&str],
) -> Result<T, ApiError> {
    api_rpc_payload(parse_rpc_wrapped_payload(value, keys))
}

#[cfg(test)]
pub(super) fn rpc_wrapped_payload_or_empty<T: DeserializeOwned>(
    value: &Value,
    keys: &[&str],
) -> Result<T, ApiError> {
    api_rpc_payload(crate::rpc_payload::rpc_wrapped_payload_or_empty(
        value, keys,
    ))
}

pub(super) fn rpc_nested_payload_or_empty<T: DeserializeOwned>(
    value: &Value,
    key: &str,
) -> Result<T, ApiError> {
    api_rpc_payload(parse_rpc_nested_payload_or_empty(value, key))
}

pub(super) fn rpc_required_string(value: &Value, key: &str) -> Result<String, ApiError> {
    api_rpc_payload(parse_rpc_required_string(value, key))
}

pub(super) fn rpc_query_strings(value: &Value, keys: &[&str]) -> HashMap<String, String> {
    parse_rpc_query_strings(value, keys)
}

fn api_rpc_payload<T>(result: Result<T, RpcPayloadError>) -> Result<T, ApiError> {
    result.map_err(|err| api_error(ApiErrorKind::InvalidRequest, err.message()))
}

#[cfg(test)]
mod tests {
    use super::{rpc_required_string, rpc_wrapped_payload, rpc_wrapped_payload_or_empty};
    use crate::api::ApiError;
    use agenthub_core::services::jobs::ThreadRenameRequest;
    use agenthub_core::services::settings as settings_service;
    use serde_json::json;

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
    fn rpc_strings_use_the_current_top_level_field() {
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
        let empty: serde_json::Value = must(rpc_wrapped_payload_or_empty(
            &serde_json::Value::Null,
            &["request"],
        ));
        assert_eq!(empty, json!({}));
    }
}
