//! Volatile, allowlisted diagnostics. Native free text never enters the ring.
use super::{protocol::Method, DesktopError};
use crate::services::codex_desktop::DesktopDiagnostic;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
};

const CAPACITY: usize = 50;
static RECORDS: OnceLock<Mutex<VecDeque<DesktopDiagnostic>>> = OnceLock::new();

#[derive(Clone, Copy)]
pub enum Source {
    Ipc,
    Helper,
    Local,
}

/// Both method and hint are selected from closed vocabularies. In particular,
/// truncating a native error is NOT redaction: it may contain credentials.
pub fn record(
    source: Source,
    method: &str,
    version: Option<u64>,
    code: Option<i64>,
    native: Option<&str>,
    error: DesktopError,
) {
    let method = safe_method(method);
    let hint = match native {
        Some("thread not found") => "线程不属于目录 helper",
        Some("request-version-mismatch" | "unsupported-version") => "原生方法版本不匹配",
        Some("no-handler-for-request" | "method-not-found") => "原生方法没有处理器",
        Some("no-client-found") => "未发现线程 owner",
        Some("ipc-connection-reset" | "client-disconnected" | "server-closed") => "原生连接已断开",
        None => error.user_message(),
        _ => "原生错误详情已省略",
    };
    let entry = DesktopDiagnostic {
        timestamp: chrono::Utc::now().timestamp_millis(),
        source: match source {
            Source::Ipc => "ipc",
            Source::Helper => "helper",
            Source::Local => "local",
        }
        .into(),
        method: method.into(),
        version: version.filter(|value| *value <= 1000),
        code,
        category: format!("{error:?}"),
        hint: hint.chars().take(200).collect(),
    };
    if let Ok(mut records) = RECORDS.get_or_init(Default::default).lock() {
        if records.len() == CAPACITY {
            records.pop_front();
        }
        records.push_back(entry);
    }
}

pub fn snapshot() -> Vec<DesktopDiagnostic> {
    RECORDS
        .get_or_init(Default::default)
        .lock()
        .map(|records| records.iter().cloned().collect())
        .unwrap_or_default()
}

fn safe_method(value: &str) -> &str {
    if Method::ALL.iter().any(|method| method.wire().0 == value)
        || matches!(
            value,
            "model/list"
                | "collaborationMode/list"
                | "skills/list"
                | "plugin/list"
                | "app/list"
                | "thread/start"
                | "thread-stream-state-changed"
                | "thread-queued-followups-changed"
                | "connect"
                | "attachments"
        )
    {
        value
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_are_bounded_and_never_retain_native_free_text() {
        for _ in 0..60 {
            record(
                Source::Helper,
                "app/list",
                None,
                Some(-32600),
                Some("secret /private/fixture user@example.invalid https://example.invalid"),
                DesktopError::Rejected,
            );
        }
        record(
            Source::Ipc,
            "secret-method",
            Some(u64::MAX),
            None,
            None,
            DesktopError::Timeout,
        );
        let records = snapshot();
        assert!(records.len() <= CAPACITY);
        let json = serde_json::to_string(&records).unwrap();
        for forbidden in ["secret", "/private", "example.invalid"] {
            assert!(!json.contains(forbidden));
        }
        assert!(records
            .iter()
            .all(|entry| entry.hint.chars().count() <= 200));
    }
}
