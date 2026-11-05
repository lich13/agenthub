use agenthub_core::{
    config::Config,
    db::NotificationChannel,
    platform::PlatformKind,
    services::{
        commands::{is_retired_command, ALLOWED_RPC_COMMANDS},
        settings::{build_settings_view, ProbeSecretState},
    },
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-notification-retirement-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn old_nested_quoted_and_inline_toml_remove_gotify_preserving_other_values_and_comments() {
    for text in [
        "# fixture header\n[probe.notifications]\nenabled = true # fixture Bark comment\nserver_url = 'https://bark.example.invalid'\ngroup = 'fixture group'\n[probe.notifications.gotify]\nenabled = true\nserver_url = 'https://notify.example.invalid'\npriority = 7\n",
        "# fixture header\n['probe'.'notifications']\nenabled = true # fixture Bark comment\nserver_url = 'https://bark.example.invalid'\ngroup = 'fixture group'\n['probe'.'notifications'.'gotify']\nenabled = true\npriority = 7\n",
        "# fixture header\n[probe]\nnotifications = { enabled = true, server_url = 'https://bark.example.invalid', group = 'fixture group', gotify = { enabled = true, priority = 7 } } # fixture Bark comment\n",
        "# fixture header\nprobe = { enabled = true, notifications = { enabled = true, server_url = 'https://bark.example.invalid', group = 'fixture group', gotify = { enabled = true, priority = 7 } } } # fixture Bark comment\n",
    ] {
        let fixture = Fixture::new();
        let path = fixture.root.join("config.toml");
        let defaults = Config::for_platform_kind_with_home(PlatformKind::Linux, &fixture.root);
        let mut base = toml::Value::try_from(defaults).unwrap();
        base.as_table_mut().unwrap().remove("probe");
        let base = toml::to_string_pretty(&base).unwrap();
        let text = format!("{text}\n{base}\n# fixture other comment\n[fixture]\nvalue = 'gotify remains ordinary text'\n");
        let mut expected: toml::Value = toml::from_str(&text).unwrap();
        expected["probe"]["notifications"].as_table_mut().unwrap().remove("gotify");
        fs::write(&path, &text).unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.probe.notifications.enabled);
        assert_eq!(config.probe.notifications.server_url, "https://bark.example.invalid");
        assert_eq!(config.probe.notifications.group, "fixture group");
        let migrated = fs::read_to_string(&path).unwrap();
        assert_eq!(toml::from_str::<toml::Value>(&migrated).unwrap(), expected);
        for comment in ["# fixture header", "# fixture Bark comment", "# fixture other comment"] {
            assert!(migrated.contains(comment));
        }
        Config::load(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), migrated);
    }
}

#[test]
fn retired_channel_has_no_serialized_settings_or_executable_command() {
    let fixture = Fixture::new();
    let config = Config::for_platform_kind_with_home(PlatformKind::Linux, &fixture.root);
    let view =
        serde_json::to_value(build_settings_view(&config, ProbeSecretState::Configured)).unwrap();
    assert!(view.get("gotify").is_none());
    assert_eq!(view["notifications"]["device_key_configured"], true);
    assert!(view["notifications"]["device_key"].is_null());
    assert!(serde_json::from_value::<NotificationChannel>(json!("gotify")).is_err());
    assert!(is_retired_command("probe.gotifyTest"));
    assert!(!ALLOWED_RPC_COMMANDS.contains(&"probe.gotifyTest"));
    assert!(ALLOWED_RPC_COMMANDS.contains(&"probe.barkTest"));
    let contract: Value =
        serde_json::from_str(include_str!("../../../contracts/agenthub-contract.json")).unwrap();
    assert!(contract["retiredActions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "probe.gotifyTest"));
    assert!(!contract["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["id"] == "probe.gotifyTest"));
}
