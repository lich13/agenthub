use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RpcPayloadError {
    message: String,
}

impl RpcPayloadError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

pub(crate) fn rpc_payload<T: DeserializeOwned>(value: &Value) -> Result<T, RpcPayloadError> {
    serde_json::from_value(value.clone()).map_err(|err| RpcPayloadError::new(err.to_string()))
}

#[cfg(test)]
pub(crate) fn rpc_payload_or_empty<T: DeserializeOwned>(
    value: &Value,
) -> Result<T, RpcPayloadError> {
    if value.is_null() {
        serde_json::from_value(json!({}))
    } else {
        serde_json::from_value(value.clone())
    }
    .map_err(|err| RpcPayloadError::new(err.to_string()))
}

pub(crate) fn rpc_wrapped_payload<T: DeserializeOwned>(
    value: &Value,
    keys: &[&str],
) -> Result<T, RpcPayloadError> {
    let payload = keys.iter().find_map(|key| value.get(*key)).unwrap_or(value);
    rpc_payload(&rpc_compat_value(payload))
}

#[cfg(test)]
pub(crate) fn rpc_wrapped_payload_or_empty<T: DeserializeOwned>(
    value: &Value,
    keys: &[&str],
) -> Result<T, RpcPayloadError> {
    let payload = keys.iter().find_map(|key| value.get(*key)).unwrap_or(value);
    rpc_payload_or_empty(&rpc_compat_value(payload))
}

pub(crate) fn rpc_nested_payload_or_empty<T: DeserializeOwned>(
    value: &Value,
    key: &str,
) -> Result<T, RpcPayloadError> {
    let payload = value.get(key).cloned().unwrap_or_else(|| json!({}));
    serde_json::from_value(payload).map_err(|err| RpcPayloadError::new(err.to_string()))
}

pub(crate) fn rpc_required_string(value: &Value, key: &str) -> Result<String, RpcPayloadError> {
    rpc_string(value, key).ok_or_else(|| RpcPayloadError::new(format!("{key} is required")))
}

pub(crate) fn rpc_string(value: &Value, key: &str) -> Option<String> {
    rpc_value(value, key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn rpc_query_strings(value: &Value, keys: &[&str]) -> HashMap<String, String> {
    keys.iter()
        .filter_map(|key| {
            value.get(*key).and_then(|value| match value {
                Value::String(text) if !text.trim().is_empty() => {
                    Some(((*key).to_string(), text.trim().to_string()))
                }
                Value::Number(number) => Some(((*key).to_string(), number.to_string())),
                Value::Bool(boolean) => Some(((*key).to_string(), boolean.to_string())),
                _ => None,
            })
        })
        .collect()
}

fn rpc_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    rpc_direct_value(value, key).or_else(|| {
        ["payload", "request", "settings"]
            .iter()
            .filter_map(|wrapper| value.get(*wrapper))
            .find_map(|nested| rpc_direct_value(nested, key))
    })
}

fn rpc_direct_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).or_else(|| {
        key.strip_suffix("Id")
            .and_then(|prefix| value.get(format!("{prefix}_id")))
    })
}

fn rpc_compat_value(value: &Value) -> Value {
    let mut value = value.clone();
    rpc_normalize_value(&mut value);
    value
}

fn rpc_normalize_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for value in object.values_mut() {
                rpc_normalize_value(value);
            }
            for (from, to) in RPC_COMPAT_FIELD_ALIASES {
                if object.contains_key(*to) {
                    continue;
                }
                if let Some(value) = object.remove(*from) {
                    object.insert((*to).to_string(), value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                rpc_normalize_value(item);
            }
        }
        _ => {}
    }
}

const RPC_COMPAT_FIELD_ALIASES: &[(&str, &str)] = &[
    ("threadId", "thread_id"),
    ("followUpId", "followup_id"),
    ("tokenBudget", "token_budget"),
    ("serviceTier", "service_tier"),
    ("reasoningEffort", "reasoning_effort"),
    ("permissionProfile", "permission_profile"),
    ("approvalPolicy", "approval_policy"),
    ("sandboxMode", "sandbox_mode"),
    ("networkAccess", "network_access"),
    ("collaborationMode", "collaboration_mode"),
    ("preparedAttachments", "prepared_attachments"),
    ("turnId", "turn_id"),
    ("itemId", "item_id"),
    ("jobId", "job_id"),
    ("requestId", "request_id"),
    ("pollSeconds", "poll_seconds"),
    ("recentLimit", "recent_limit"),
    ("serverUrl", "server_url"),
    ("deviceKey", "device_key"),
    ("notifyCompletion", "notify_completion"),
    ("notifyReplyNeeded", "notify_reply_needed"),
    ("notifyRecoverable", "notify_recoverable"),
];
