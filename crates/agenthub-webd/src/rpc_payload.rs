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
    rpc_payload(payload)
}

#[cfg(test)]
pub(crate) fn rpc_wrapped_payload_or_empty<T: DeserializeOwned>(
    value: &Value,
    keys: &[&str],
) -> Result<T, RpcPayloadError> {
    let payload = keys.iter().find_map(|key| value.get(*key)).unwrap_or(value);
    rpc_payload_or_empty(payload)
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
    value
        .get(key)
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
