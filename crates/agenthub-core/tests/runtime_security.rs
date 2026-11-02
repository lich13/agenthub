use agenthub_core::{
    config::{valid_probe_notification_server_url, Config, ServerConfig},
    platform::PlatformKind,
};
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Once,
};

const FIXTURE_SECRET_KEY: &str = "runtime-security-fixture-key-material";
static FIXTURE_SECRET_KEY_INIT: Once = Once::new();

struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-runtime-security-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("create isolated runtime-security fixture");
        Self { root }
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn config_value(listen: &str) -> toml::Value {
    let config = Config::for_platform_kind_with_home(
        PlatformKind::Macos,
        Path::new("/synthetic/runtime-security/home"),
    );
    let mut value: toml::Value =
        toml::from_str(&toml::to_string_pretty(&config).expect("serialize fixture config"))
            .expect("parse fixture config");
    value["server"]["listen"] = toml::Value::String(listen.to_string());
    value
}

fn socket_addr(value: &str) -> SocketAddr {
    value.parse().expect("parse fixture listener address")
}

fn fixture_url(scheme: &str, authority: &str, suffix: &str) -> String {
    format!("{scheme}://{authority}{suffix}")
}

fn use_fixture_secret_key() {
    // Config::normalize reads this environment variable; keep the test isolated
    // from any key supplied by the host process.
    FIXTURE_SECRET_KEY_INIT.call_once(|| {
        std::env::set_var("AGENTHUB_SECRET_KEY", FIXTURE_SECRET_KEY);
    });
}

#[test]
fn listener_validation_accepts_loopback_ranges_and_preserves_port_rules() {
    use_fixture_secret_key();

    let cases = [
        ("127.0.0.1:15742", "127.0.0.1:15742"),
        ("127.23.45.67:8080", "127.23.45.67:8080"),
        ("127.255.255.255:0", "127.255.255.255:0"),
        ("127.0.0.1:65535", "127.0.0.1:65535"),
        ("[::1]:15742", "[::1]:15742"),
        ("[::ffff:127.0.0.1]:15742", "[::ffff:127.0.0.1]:15742"),
        ("[::ffff:127.23.45.67]:15742", "[::ffff:127.23.45.67]:15742"),
        ("127.0.0.1:15732", "127.0.0.1:15742"),
        ("127.0.0.2:15732", "127.0.0.2:15732"),
        ("[::ffff:127.0.0.1]:15732", "[::ffff:127.0.0.1]:15732"),
    ];

    for (input, expected) in cases {
        let mut config = Config::for_platform_kind_with_home(
            PlatformKind::Macos,
            Path::new("/synthetic/runtime-security/home"),
        );
        config.server.listen = socket_addr(input);

        config
            .server
            .validate_listen()
            .unwrap_or_else(|_| panic!("expected loopback listener case to pass: {input}"));
        config
            .normalize()
            .unwrap_or_else(|_| panic!("expected listener normalization to pass: {input}"));

        assert_eq!(config.server.listen.to_string(), expected, "input {input}");
    }
}

#[test]
fn listener_validation_rejects_non_loopback_addresses_without_echoing_them() {
    let cases = [
        "0.0.0.0:15742",
        "192.0.2.9:15742",
        "[::]:15742",
        "[2001:db8::1]:15742",
        "[fd00::1]:15742",
        "[::ffff:192.0.2.9]:15742",
    ];

    for input in cases {
        let error = ServerConfig {
            listen: socket_addr(input),
        }
        .validate_listen()
        .expect_err("non-loopback listener must be rejected");
        let rendered = format!("{error:#}");

        assert!(
            !rendered.contains(input),
            "listener validation error must not echo its rejected input"
        );
        assert!(
            rendered.contains("loopback"),
            "listener validation should return the fixed safe error"
        );
    }
}

#[test]
fn config_load_rejects_non_loopback_before_migration_rewrites_the_file() {
    let fixture = TempFixture::new();
    let path = fixture.config_path();
    let rejected_listener = "203.0.113.77:15742";
    let mut value = config_value(rejected_listener);
    let probe = value
        .get_mut("probe")
        .and_then(toml::Value::as_table_mut)
        .expect("fixture probe table");
    let error_monitor = probe
        .entry("error_monitor")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .expect("fixture error-monitor table");
    error_monitor.insert("auto_resume_goals".to_string(), toml::Value::Boolean(true));
    let original = toml::to_string_pretty(&value).expect("serialize isolated migration fixture");
    fs::write(&path, &original).expect("write isolated config fixture");

    let error = Config::load(&path).expect_err("public listener must fail config loading");
    let rendered = format!("{error:#}");

    assert!(
        !rendered.contains(rejected_listener),
        "config load error must not echo its rejected listener"
    );
    assert!(
        !rendered.contains("203.0.113.77"),
        "config load error must not expose the rejected host"
    );
    assert_eq!(
        fs::read(&path).expect("read original config fixture"),
        original.as_bytes(),
        "a rejected listener must prevent migration writeback"
    );
    assert_eq!(
        fs::read_dir(&fixture.root)
            .expect("read fixture directory")
            .count(),
        1,
        "a rejected listener must not leave a migration temporary file"
    );
}

#[test]
fn config_load_preserves_an_invalid_bark_destination_for_send_time_validation() {
    use_fixture_secret_key();

    let fixture = TempFixture::new();
    let path = fixture.config_path();
    let dangerous_url = fixture_url("https", "[::ffff:10.1.2.3]", "");
    let mut value = config_value("127.0.0.1:15742");
    value["probe"]["notifications"]["enabled"] = toml::Value::Boolean(true);
    value["probe"]["notifications"]["server_url"] = toml::Value::String(dangerous_url.clone());
    fs::write(
        &path,
        toml::to_string_pretty(&value).expect("serialize isolated notification fixture"),
    )
    .expect("write isolated notification fixture");

    let loaded = Config::load(&path).expect("loopback config should load");

    assert!(loaded.probe.notifications.enabled);
    assert_eq!(loaded.probe.notifications.server_url, dangerous_url);
    assert!(!valid_probe_notification_server_url(&dangerous_url));
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
