use agenthub_core::config::valid_probe_notification_server_url;
fn fixture_url(scheme: &str, authority: &str, suffix: &str) -> String {
    format!("{scheme}://{authority}{suffix}")
}

#[test]
fn bark_url_validation_keeps_https_domains_and_real_http_loopback() {
    let accepted = vec![
        "https://bark.example.invalid".to_string(),
        "https://bark.example.invalid:8443/push?fixture=1#section".to_string(),
        fixture_url("https", "[::ffff:203.0.113.8]", ""),
        "http://localhost:8080/push".to_string(),
        "http://127.0.0.1:15742".to_string(),
        "http://127.23.45.67:8080".to_string(),
        "http://127.255.255.255:8080".to_string(),
        "http://[::1]:8080".to_string(),
        "http://[::ffff:127.23.45.67]:8080".to_string(),
    ];

    for input in accepted {
        assert!(
            valid_probe_notification_server_url(&input),
            "expected safe Bark URL case to pass: {input}"
        );
    }
}

#[test]
fn bark_url_validation_rejects_private_mapped_credential_and_numeric_authorities() {
    let rejected = [
        (fixture_url("https", "127.0.0.1", ""), "IPv4 loopback"),
        (
            fixture_url("https", "127.23.45.67", ""),
            "IPv4 loopback range",
        ),
        (fixture_url("https", "10.1.2.3", ""), "private IPv4"),
        (fixture_url("https", "172.16.0.1", ""), "private IPv4"),
        (fixture_url("https", "192.168.1.1", ""), "private IPv4"),
        (fixture_url("https", "0.0.0.0", ""), "unspecified IPv4"),
        (fixture_url("https", "169.254.1.1", ""), "link-local IPv4"),
        (fixture_url("https", "[::1]", ""), "IPv6 loopback"),
        (fixture_url("https", "[::]", ""), "unspecified IPv6"),
        (fixture_url("https", "[fe80::1]", ""), "link-local IPv6"),
        (fixture_url("https", "[fc00::1]", ""), "unique-local IPv6"),
        (fixture_url("https", "[fd00::1]", ""), "unique-local IPv6"),
        (
            fixture_url("https", "[::ffff:127.0.0.1]", ""),
            "mapped loopback IPv4",
        ),
        (
            fixture_url("https", "[::ffff:10.1.2.3]", ""),
            "mapped private IPv4",
        ),
        (
            fixture_url("https", "[::ffff:0.0.0.0]", ""),
            "mapped unspecified IPv4",
        ),
        (
            fixture_url("https", "[::ffff:169.254.1.1]", ""),
            "mapped link-local IPv4",
        ),
        (
            fixture_url("https", "fixture@bark.example.invalid", ""),
            "URL credentials",
        ),
        (
            fixture_url("https", "fixture:example@bark.example.invalid", ""),
            "URL credentials",
        ),
        (
            fixture_url("http", "fixture@localhost", ""),
            "HTTP URL credentials",
        ),
        (
            fixture_url("https", "127.1", ""),
            "abbreviated numeric IPv4",
        ),
        (fixture_url("https", "0177.0.0.1", ""), "octal numeric IPv4"),
        (
            fixture_url("https", "0x7f000001", ""),
            "hexadecimal numeric IPv4",
        ),
        (
            fixture_url("https", "2130706433", ""),
            "integer numeric IPv4",
        ),
        (fixture_url("https", "0x7f.1", ""), "mixed numeric IPv4"),
        (
            fixture_url("https", "127.0.0.1.", ""),
            "trailing-dot numeric IPv4",
        ),
        (fixture_url("https", "", ""), "empty authority"),
        (fixture_url("https", "", "/push"), "empty authority"),
        (fixture_url("https", ":443", "/push"), "empty host"),
        (
            fixture_url("https", "bark.example.invalid:invalid", ""),
            "malformed port",
        ),
        (
            fixture_url("https", "bark.example.invalid:65536", ""),
            "out-of-range port",
        ),
        (
            fixture_url("https", "[::1", ""),
            "unterminated IPv6 authority",
        ),
        (fixture_url("https", "[]", ""), "empty IPv6 authority"),
        (
            fixture_url("https", "[::1]suffix", ""),
            "malformed bracketed authority",
        ),
        (
            fixture_url("https", "bark.example.invalid:443:1", ""),
            "multiple authority ports",
        ),
        (
            fixture_url("https", "bark.example.invalid\\@127.0.0.1", ""),
            "backslash authority repair",
        ),
    ];

    for (input, category) in rejected {
        assert!(
            !valid_probe_notification_server_url(&input),
            "Bark URL validator accepted a rejected category: {category}"
        );
    }
}
