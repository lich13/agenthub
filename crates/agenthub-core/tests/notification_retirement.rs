use agenthub_core::{
    config::Config,
    db::NotificationChannel,
    platform::PlatformKind,
    services::{
        commands::ALLOWED_RPC_COMMANDS,
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
fn retired_channel_has_no_serialized_settings_or_executable_command() {
    let fixture = Fixture::new();
    let config = Config::for_platform_kind_with_home(PlatformKind::Linux, &fixture.root);
    let view =
        serde_json::to_value(build_settings_view(&config, ProbeSecretState::Configured)).unwrap();
    assert!(view.get("gotify").is_none());
    assert_eq!(view["notifications"]["device_key_configured"], true);
    assert!(view["notifications"]["device_key"].is_null());
    assert!(serde_json::from_value::<NotificationChannel>(json!("gotify")).is_err());
    assert!(!ALLOWED_RPC_COMMANDS.contains(&"probe.gotifyTest"));
    assert!(ALLOWED_RPC_COMMANDS.contains(&"probe.barkTest"));
    let contract: Value =
        serde_json::from_str(include_str!("../../../contracts/agenthub-contract.json")).unwrap();
    assert!(!contract["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["id"] == "probe.gotifyTest"));
}
