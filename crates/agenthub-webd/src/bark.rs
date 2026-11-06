//! HTTP transport only. Diagnostics never contain response text, URLs or credentials.
use super::{BarkPushResponse, ProbeBarkOutcome, ProbeBarkRequest};
use agenthub_core::config::{valid_probe_notification_server_url, Config};
use chrono::Utc;
use reqwest::{header::RETRY_AFTER, Client};
use serde_json::json;
use std::{
    error::Error,
    sync::OnceLock,
    time::{Duration, Instant},
};

fn client() -> Option<&'static Client> {
    static CLIENT: OnceLock<Option<Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(8))
                .pool_idle_timeout(Duration::from_secs(90))
                .pool_max_idle_per_host(4)
                .build()
                .ok()
        })
        .as_ref()
}

fn failure(category: &str, reason: &str, retryable: bool, uncertain: bool) -> ProbeBarkOutcome {
    ProbeBarkOutcome {
        error_category: Some(category.into()),
        retryable,
        uncertain,
        ..ProbeBarkOutcome::failed_request(reason, true, true, true, None)
    }
}

pub(crate) fn classify(error: &reqwest::Error, reading: bool) -> ProbeBarkOutcome {
    // The outer reqwest message includes the request URL, which must not affect classification.
    let mut current: Option<&(dyn Error + 'static)> = error.source();
    let mut messages = String::new();
    while let Some(e) = current {
        messages.push_str(&e.to_string().to_ascii_lowercase());
        messages.push(' ');
        current = e.source();
    }
    // Only this fixed classification leaves the transport; the source chain is never logged.
    if [
        "certificate",
        "unknownissuer",
        "invalidpeer",
        "certnotvalid",
        "certificateverify",
    ]
    .iter()
    .any(|s| messages.contains(s))
    {
        failure("tls", "certificate_validation", false, false)
    } else if error.is_timeout() {
        failure("timeout", "timeout", true, true)
    } else if [
        "dns",
        "name or service",
        "failed to lookup",
        "nodename nor servname",
    ]
    .iter()
    .any(|s| messages.contains(s))
    {
        failure("dns", "dns", true, false)
    } else if messages.contains("tls") || messages.contains("ssl") {
        failure("tls", "tls", true, false)
    } else if reading {
        failure("response_read", "response_read", true, true)
    } else if error.is_connect() {
        failure("connection", "connection", true, false)
    } else {
        failure("network", "network_unknown", true, true)
    }
}

pub(crate) fn retry_after(value: Option<&str>, now_ms: i64) -> Option<i64> {
    let value = value?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1000).min(i64::MAX as u64) as i64);
    }
    chrono::DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|date| date.timestamp_millis().saturating_sub(now_ms).max(0))
}

fn permanent_apns(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    [
        "baddevicetoken",
        "devicetokennotfortopic",
        "unregistered",
        "badcollapseid",
        "badexpirationdate",
        "badpriority",
        "badtopic",
        "missingdevicetoken",
        "missingtopic",
        "payloadempty",
        "payloadtoolarge",
        "topicdisallowed",
        "badcertificate",
        "invalidprovidertoken",
        "expiredprovidertoken",
        "missingprovidertoken",
        "forbidden",
        "badpath",
        "methodnotallowed",
        "duplicatedheaders",
        "badmessageid",
        "badpushtype",
    ]
    .iter()
    .any(|reason| normalized.contains(reason))
}

#[allow(clippy::too_many_arguments)]
pub async fn send_chunk(
    config: &Config,
    device_key: &[u8],
    request: &ProbeBarkRequest,
    body: &str,
    index: usize,
    count: usize,
    id: &str,
    attempt: usize,
    timeout: Duration,
) -> ProbeBarkOutcome {
    let started = Instant::now();
    let mut result = send(config, device_key, request, body, index, count, id, timeout).await;
    result.notifications_enabled = config.probe.notifications.enabled;
    result.chunk_count = count;
    result.request_count = usize::from(result.request_count > 0);
    result.attempts = attempt;
    tracing::info!(
        category = result.error_category.as_deref().unwrap_or("accepted"),
        http_status = result.http_status,
        elapsed_ms = started.elapsed().as_millis() as u64,
        attempt,
        correlation = id,
        "Bark delivery attempt"
    );
    result
}

#[allow(clippy::too_many_arguments)]
async fn send(
    config: &Config,
    device_key: &[u8],
    request: &ProbeBarkRequest,
    body: &str,
    index: usize,
    count: usize,
    id: &str,
    timeout: Duration,
) -> ProbeBarkOutcome {
    let Ok(key) = std::str::from_utf8(device_key) else {
        return failure("configuration", "invalid_device_key_encoding", false, false);
    };
    let server = config.probe.notifications.server_url.trim();
    if !valid_probe_notification_server_url(server) {
        return failure("configuration", "invalid_server_url", false, false);
    }
    let Ok(url) = reqwest::Url::parse(&format!("{}/", server.trim_end_matches('/')))
        .and_then(|base| base.join("push"))
    else {
        return failure("configuration", "invalid_server_url", false, false);
    };
    let Some(client) = client() else {
        return failure("configuration", "client_build_error", false, false);
    };
    let title = if count > 1 {
        format!("{} ({}/{count})", request.title, index + 1)
    } else {
        request.title.clone()
    };
    let mut payload = json!({"device_key":key.trim(),"title":title,"body":body,"id":id});
    if let Some(presentation) = &request.presentation {
        let settings = &config.probe.notifications;
        payload["group"] = json!(format!(
            "{}｜{}",
            settings.group.trim(),
            presentation.provider
        ));
        payload["level"] = json!(if presentation.urgent {
            "timeSensitive"
        } else {
            "active"
        });
        if let Some(sound) = settings.sound.as_deref().filter(|s| !s.is_empty()) {
            payload["sound"] = json!(sound);
        }
        if let Some(url) = request
            .url
            .as_deref()
            .or(settings.url.as_deref())
            .filter(|s| {
                reqwest::Url::parse(s).is_ok_and(|u| {
                    !matches!(
                        u.scheme(),
                        "javascript" | "data" | "file" | "about" | "blob"
                    ) && u.username().is_empty()
                        && u.password().is_none()
                })
            })
        {
            payload["url"] = json!(url);
        }
    }
    let response = client
        .post(url)
        .timeout(timeout)
        .json(&payload)
        .send()
        .await;
    let mut response = match response {
        Ok(r) => r,
        Err(e) => {
            let mut result = classify(&e, false);
            result.request_count = 1;
            return result;
        }
    };
    let status = response.status().as_u16();
    let delay = retry_after(
        response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|h| h.to_str().ok()),
        Utc::now().timestamp_millis(),
    );
    let temporary = matches!(status, 408 | 429 | 500 | 502 | 503 | 504);
    let mut bytes = Vec::new();
    let mut read_error = None;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= 64 * 1024 => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(Some(_)) => {
                read_error = Some(failure("response_read", "response_too_large", true, true));
                break;
            }
            Ok(None) => break,
            Err(e) => {
                read_error = Some(classify(&e, true));
                break;
            }
        }
    }
    let parsed = serde_json::from_slice::<BarkPushResponse>(&bytes).ok();
    let mut result = if parsed
        .as_ref()
        .and_then(|v| v.message.as_deref())
        .is_some_and(permanent_apns)
    {
        failure("bark_rejection", "apns_permanent_rejection", false, false)
    } else if !(200..300).contains(&status) {
        failure(
            "http_rejection",
            if (300..400).contains(&status) {
                "redirect_rejected"
            } else {
                "http_status"
            },
            temporary,
            false,
        )
    } else if let Some(error) = read_error {
        error
    } else if let Some(code) = parsed.and_then(|v| v.code) {
        if code == 200 {
            ProbeBarkOutcome::sent(status, true, true, true, None)
        } else {
            failure(
                "bark_rejection",
                "bark_response_code",
                matches!(code, 408 | 429 | 500 | 502 | 503 | 504),
                false,
            )
        }
    } else {
        failure("response_read", "response_decode", true, true)
    };
    result.http_status = Some(status);
    result.retry_after_ms = delay;
    result.request_count = 1;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_url(scheme: &str, authority: &str) -> String {
        format!("{scheme}://{authority}")
    }

    #[tokio::test]
    async fn invalid_server_urls_fail_without_network_delivery_or_retry() {
        let request = ProbeBarkRequest {
            url: None,
            presentation: None,
            title: "fixture title".into(),
            body: "fixture body".into(),
            dedupe_key: "fixture-event".into(),
        };

        for (case, server_url) in [
            ("empty", String::new()),
            ("whitespace", " \t ".into()),
            ("https private ipv4", fixture_url("https", "192.168.1.12")),
            (
                "https mapped private ipv6",
                fixture_url("https", "[::ffff:192.168.1.12]"),
            ),
            ("https numeric alias short", fixture_url("https", "127.1")),
            (
                "https numeric alias integer",
                fixture_url("https", "2130706433"),
            ),
            (
                "https credential authority",
                fixture_url("https", "fixturekey@example.invalid"),
            ),
        ] {
            let mut config = Config::default();
            config.probe.notifications.enabled = true;
            config.probe.notifications.server_url = server_url.clone();

            let outcome = send(
                &config,
                b"fixturekey",
                &request,
                "fixture body",
                0,
                1,
                "fixture-id",
                Duration::from_millis(10),
            )
            .await;

            assert_eq!(
                outcome.reason.as_deref(),
                Some("invalid_server_url"),
                "{case}"
            );
            assert_eq!(
                outcome.error_category.as_deref(),
                Some("configuration"),
                "{case}"
            );
            assert_eq!(outcome.request_count, 0, "{case}");
            assert_eq!(outcome.attempts, 0, "{case}");
            assert!(!outcome.retryable, "{case}");
            assert!(!outcome.uncertain, "{case}");
            assert!(!outcome.sent, "{case}");
            assert!(!outcome.skipped, "{case}");
            assert!(outcome.http_status.is_none(), "{case}");
            assert!(outcome.request_url.is_none(), "{case}");
            assert_eq!(config.probe.notifications.server_url, server_url, "{case}");

            let outcome = send_chunk(
                &config,
                b"fixturekey",
                &request,
                "fixture body",
                0,
                1,
                "fixture-id",
                1,
                Duration::from_millis(10),
            )
            .await;

            assert_eq!(
                outcome.reason.as_deref(),
                Some("invalid_server_url"),
                "{case}"
            );
            assert_eq!(
                outcome.error_category.as_deref(),
                Some("configuration"),
                "{case}"
            );
            assert_eq!(outcome.request_count, 0, "{case}");
            assert_eq!(outcome.attempts, 1, "{case}");
            assert!(!outcome.retryable, "{case}");
            assert!(!outcome.uncertain, "{case}");
            assert!(!outcome.sent, "{case}");
            assert!(!outcome.skipped, "{case}");
            assert!(outcome.http_status.is_none(), "{case}");
            assert!(outcome.request_url.is_none(), "{case}");
            assert!(outcome.notifications_enabled, "{case}");
            assert_eq!(config.probe.notifications.server_url, server_url, "{case}");
        }
    }
}
