use crate::{
    codex::resolve_codex_paths,
    config::{
        valid_probe_notification_server_url, CodexProbeConfigPatch, Config, ProbeConfig,
        ProbeConfigFilePatch, ProbeErrorMonitorConfigPatch, ProbeHooksConfigPatch,
        ProbeNotificationsConfig, ProbeNotificationsConfigPatch, ProbeObservabilityConfigPatch,
        ProbeSettingsPatch,
    },
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const PROBE_BARK_DEVICE_KEY_SETTING: &str = "probe_bark_device_key";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeSecretState {
    Configured,
    Missing,
}

impl ProbeSecretState {
    pub fn from_secret_bytes(value: Option<&[u8]>) -> Self {
        match value {
            Some(value) if !value.is_empty() => Self::Configured,
            _ => Self::Missing,
        }
    }

    pub fn is_configured(self) -> bool {
        matches!(self, Self::Configured)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsView {
    pub codex: CodexSettingsView,
    pub probe: ProbeConfig,
    pub notifications: ProbeNotificationsSettingsView,
}

#[derive(Debug, Clone, Copy)]
pub struct SettingsUseCases<'a> {
    config: &'a Config,
}

impl<'a> SettingsUseCases<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub fn probe_settings_view(self, bark_device_key: ProbeSecretState) -> Result<SettingsView> {
        Ok(build_settings_view(self.config, bark_device_key))
    }

    pub fn save_probe_settings(
        self,
        request: ProbeSettingsSaveRequest,
    ) -> Result<ProbeSettingsSavePlan> {
        plan_probe_settings_save(request)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexSettingsView {
    pub home: Option<String>,
    pub configured_codex_home: Option<String>,
    pub resolved_codex_home: String,
    pub codex_home_source: String,
    pub logs_db_source: String,
    pub discovery_warnings: Vec<String>,
    pub workspace: String,
    pub host_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeNotificationsSettingsView {
    pub device_key_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_key: Option<String>,
    pub server_url: String,
    pub enabled: bool,
    pub sound: Option<String>,
    pub group: String,
    pub url: Option<String>,
    pub notify_completion: bool,
    pub notify_reply_needed: bool,
    pub notify_recoverable: bool,
    pub notify_codex: bool,
    pub notify_grok: bool,
    pub notify_grokbot: bool,
    #[serde(default)]
    pub notify_github_ci: bool,
    #[serde(default)]
    pub github_ci_repositories: Vec<String>,
    #[serde(default = "default_true")]
    pub github_ci_filter_bots: bool,
    pub notify_grok_completion: bool,
    pub notify_grok_failure: bool,
    pub notify_grok_reply_needed: bool,
    pub notify_claude_reply_needed: bool,
    pub notify_claude_failure: bool,
    pub notify_claude_completion: bool,
    pub notify_claude: bool,
    pub notify_antigravity: bool,
    pub notify_antigravity_completion: bool,
    pub notify_antigravity_failure: bool,
    pub notify_antigravity_reply_needed: bool,
    pub notify_antigravity_authorization: bool,
    pub notify_dsh: bool,
    pub notify_dsh_completion: bool,
    pub notify_dsh_failure: bool,
    pub notify_dsh_reply_needed: bool,
    pub notify_dsh_authorization: bool,
}

fn default_true() -> bool {
    true
}

pub fn build_settings_view(config: &Config, bark_device_key: ProbeSecretState) -> SettingsView {
    let resolved = resolve_codex_paths(&config.codex.home);
    SettingsView {
        codex: CodexSettingsView {
            home: resolved.configured_codex_home.clone(),
            configured_codex_home: resolved.configured_codex_home,
            resolved_codex_home: resolved.home.display().to_string(),
            codex_home_source: resolved.codex_home_source,
            logs_db_source: resolved.logs_db_source,
            discovery_warnings: resolved.discovery_warnings,
            workspace: config.codex.workspace.display().to_string(),
            host_label: config.codex.host_label.clone(),
        },
        probe: config.probe.clone(),
        notifications: probe_notifications_settings_view(
            &config.probe.notifications,
            bark_device_key,
        ),
    }
}

pub fn probe_notifications_settings_view(
    notifications: &ProbeNotificationsConfig,
    bark_device_key: ProbeSecretState,
) -> ProbeNotificationsSettingsView {
    ProbeNotificationsSettingsView {
        device_key_configured: bark_device_key.is_configured(),
        device_key: None,
        server_url: notifications.server_url.clone(),
        enabled: notifications.enabled,
        sound: notifications.sound.clone(),
        group: notifications.group.clone(),
        url: notifications.url.clone(),
        notify_completion: notifications.notify_completion,
        notify_reply_needed: notifications.notify_reply_needed,
        notify_recoverable: notifications.notify_recoverable,
        notify_codex: notifications.notify_codex,
        notify_grok: notifications.notify_grok,
        notify_grokbot: notifications.notify_grokbot,
        notify_github_ci: notifications.notify_github_ci,
        github_ci_repositories: notifications.github_ci_repositories.clone(),
        github_ci_filter_bots: notifications.github_ci_filter_bots,
        notify_grok_completion: notifications.notify_grok_completion,
        notify_grok_failure: notifications.notify_grok_failure,
        notify_grok_reply_needed: notifications.notify_grok_reply_needed,
        notify_claude_reply_needed: notifications.notify_claude_reply_needed,
        notify_claude_failure: notifications.notify_claude_failure,
        notify_claude_completion: notifications.notify_claude_completion,
        notify_claude: notifications.notify_claude,
        notify_antigravity: notifications.notify_antigravity,
        notify_antigravity_completion: notifications.notify_antigravity_completion,
        notify_antigravity_failure: notifications.notify_antigravity_failure,
        notify_antigravity_reply_needed: notifications.notify_antigravity_reply_needed,
        notify_antigravity_authorization: notifications.notify_antigravity_authorization,
        notify_dsh: notifications.notify_dsh,
        notify_dsh_completion: notifications.notify_dsh_completion,
        notify_dsh_failure: notifications.notify_dsh_failure,
        notify_dsh_reply_needed: notifications.notify_dsh_reply_needed,
        notify_dsh_authorization: notifications.notify_dsh_authorization,
    }
}

pub fn normalize_bark_device_key(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

macro_rules! merge_some_fields {
    ($target:expr, $source:expr, [$($field:ident),+ $(,)?]) => {
        $(
            if $source.$field.is_some() {
                $target.$field = $source.$field;
            }
        )+
    };
}

pub fn merge_probe_notification_patch(
    target: &mut ProbeNotificationsConfigPatch,
    source: ProbeNotificationsConfigPatch,
) {
    merge_some_fields!(
        target,
        source,
        [
            enabled,
            server_url,
            sound,
            group,
            url,
            notify_completion,
            notify_reply_needed,
            notify_recoverable,
            notify_codex,
            notify_github_ci,
            github_ci_repositories,
            github_ci_filter_bots,
            notify_grokbot,
            notify_grok,
            notify_grok_completion,
            notify_grok_failure,
            notify_grok_reply_needed,
            notify_claude,
            notify_claude_completion,
            notify_claude_failure,
            notify_claude_reply_needed,
            notify_antigravity,
            notify_antigravity_completion,
            notify_antigravity_failure,
            notify_antigravity_reply_needed,
            notify_antigravity_authorization,
            notify_dsh,
            notify_dsh_completion,
            notify_dsh_failure,
            notify_dsh_reply_needed,
            notify_dsh_authorization,
        ]
    );
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeSettingsSaveRequest {
    pub codex: Option<CodexProbeConfigPatch>,
    pub probe: Option<ProbeSettingsSavePatch>,
    pub notifications: Option<ProbeNotificationsSavePatch>,
}

impl ProbeSettingsSaveRequest {
    pub fn normalize(self) -> Result<NormalizedProbeSettingsPatch> {
        normalize_probe_settings_save_request(self)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeSettingsSavePatch {
    pub enabled: Option<bool>,
    pub poll_seconds: Option<u64>,
    pub recent_limit: Option<usize>,
    pub hooks: Option<ProbeHooksConfigPatch>,
    pub notifications: Option<ProbeNotificationsSavePatch>,
    pub observability: Option<ProbeObservabilityConfigPatch>,
    pub error_monitor: Option<ProbeErrorMonitorConfigPatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeNotificationsSavePatch {
    #[serde(flatten)]
    pub config: ProbeNotificationsConfigPatch,
    pub device_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedProbeSettingsPatch {
    pub config_patch: ProbeConfigFilePatch,
    #[serde(default, skip_serializing)]
    pub bark_device_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretSettingWritePlan {
    pub setting_key: String,
    #[serde(default, skip_serializing)]
    pub secret_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeSettingsSavePlan {
    pub config_patch: ProbeConfigFilePatch,
    pub secret_write: Option<SecretSettingWritePlan>,
}

pub fn plan_probe_settings_save(
    request: ProbeSettingsSaveRequest,
) -> Result<ProbeSettingsSavePlan> {
    let normalized = normalize_probe_settings_save_request(request)?;
    let secret_write = normalized
        .bark_device_key
        .as_deref()
        .map(bark_device_key_write_plan);
    Ok(ProbeSettingsSavePlan {
        config_patch: normalized.config_patch,
        secret_write,
    })
}

pub fn apply_probe_settings_save_plan(
    config_path: &std::path::Path,
    db: &crate::db::PanelDb,
    plan: ProbeSettingsSavePlan,
) -> Result<Config> {
    db.with_transaction(|tx| {
        let (before, config) = if probe_config_file_patch_has_changes(&plan.config_patch) {
            Config::save_probe_patch_with_before(config_path, &plan.config_patch)?
        } else {
            let config = Config::load(config_path)?;
            (config.clone(), config)
        };
        if let Some(write) = plan.secret_write {
            db.set_secret_setting_bytes_in_transaction(
                tx,
                &write.setting_key,
                write.secret_value.as_bytes(),
            )?;
        }
        db.native_notification_settings_changed(
            tx,
            &before,
            &config,
            jiff::Timestamp::now().as_millisecond(),
        )?;
        crate::github_ci::settings_changed(tx, &before, &config)?;
        Ok(config)
    })
}

pub fn normalize_probe_settings_save_request(
    request: ProbeSettingsSaveRequest,
) -> Result<NormalizedProbeSettingsPatch> {
    let (mut probe_patch, mut bark_device_key) = match request.probe {
        Some(probe) => probe.into_config_patch_and_bark_key(),
        None => (None, None),
    };

    if let Some(notifications) = request.notifications {
        let top_level_bark_device_key = normalize_bark_device_key(notifications.device_key);
        if notifications.config != ProbeNotificationsConfigPatch::default() {
            let probe = probe_patch.get_or_insert_with(ProbeSettingsPatch::default);
            let target = probe
                .notifications
                .get_or_insert_with(ProbeNotificationsConfigPatch::default);
            merge_probe_notification_patch(target, notifications.config);
        }
        if top_level_bark_device_key.is_some() {
            bark_device_key = top_level_bark_device_key;
        }
    }

    if probe_patch
        .as_ref()
        .is_some_and(|p| *p == ProbeSettingsPatch::default())
    {
        probe_patch = None;
    }

    let config_patch = normalize_probe_config_file_patch(ProbeConfigFilePatch {
        codex: request.codex,
        probe: probe_patch,
    })?;

    Ok(NormalizedProbeSettingsPatch {
        config_patch,
        bark_device_key,
    })
}

impl ProbeSettingsSavePatch {
    fn into_config_patch_and_bark_key(self) -> (Option<ProbeSettingsPatch>, Option<String>) {
        let (notifications, bark_device_key) = match self.notifications {
            Some(notifications) => {
                let key = normalize_bark_device_key(notifications.device_key);
                let patch = (notifications.config != ProbeNotificationsConfigPatch::default())
                    .then_some(notifications.config);
                (patch, key)
            }
            None => (None, None),
        };

        let patch = ProbeSettingsPatch {
            enabled: self.enabled,
            poll_seconds: self.poll_seconds,
            recent_limit: self.recent_limit,
            hooks: self.hooks,
            notifications,
            observability: self.observability,
            error_monitor: self.error_monitor,
        };

        (
            (patch != ProbeSettingsPatch::default()).then_some(patch),
            bark_device_key,
        )
    }
}

fn bark_device_key_write_plan(secret_value: &str) -> SecretSettingWritePlan {
    SecretSettingWritePlan {
        setting_key: PROBE_BARK_DEVICE_KEY_SETTING.to_string(),
        secret_value: secret_value.to_string(),
    }
}

fn probe_config_file_patch_has_changes(patch: &ProbeConfigFilePatch) -> bool {
    patch.codex.is_some() || patch.probe.is_some()
}

pub fn normalize_probe_config_file_patch(
    patch: ProbeConfigFilePatch,
) -> Result<ProbeConfigFilePatch> {
    Ok(ProbeConfigFilePatch {
        codex: patch.codex.map(normalize_codex_patch),
        probe: patch
            .probe
            .map(normalize_probe_settings_patch)
            .transpose()?,
    })
}

pub fn normalize_probe_settings_patch(mut patch: ProbeSettingsPatch) -> Result<ProbeSettingsPatch> {
    patch.poll_seconds = patch.poll_seconds.map(|value| value.clamp(5, 3_600));
    patch.recent_limit = patch.recent_limit.map(|value| value.clamp(1, 500));
    patch.hooks = patch.hooks.map(normalize_probe_hooks_patch);
    patch.notifications = patch
        .notifications
        .map(normalize_probe_notifications_patch)
        .transpose()?;
    patch.observability = patch.observability.map(normalize_probe_observability_patch);
    patch.error_monitor = patch
        .error_monitor
        .map(|patch| ProbeErrorMonitorConfigPatch {
            enabled: patch.enabled,
        });
    Ok(patch)
}

fn normalize_codex_patch(mut patch: CodexProbeConfigPatch) -> CodexProbeConfigPatch {
    patch.workspace = normalize_optional_string(patch.workspace);
    patch.host_label = normalize_optional_string(patch.host_label);
    patch
}

fn normalize_probe_hooks_patch(patch: ProbeHooksConfigPatch) -> ProbeHooksConfigPatch {
    ProbeHooksConfigPatch {
        manage_stop_hook: patch.manage_stop_hook,
        clear_account_scope: patch.clear_account_scope,
    }
}

fn normalize_probe_notifications_patch(
    mut patch: ProbeNotificationsConfigPatch,
) -> Result<ProbeNotificationsConfigPatch> {
    if let Some(server_url) = patch.server_url.take() {
        let server_url = server_url.trim().to_string();
        if !valid_probe_notification_server_url(&server_url) {
            bail!("probe notifications server_url must use HTTPS except localhost HTTP");
        }
        patch.server_url = Some(server_url);
    }
    if let Some(group) = patch.group.take() {
        let group = group.trim();
        patch.group = Some(if group.is_empty() {
            "AgentHub".to_string()
        } else {
            group.to_string()
        });
    }
    if let Some(repositories) = patch.github_ci_repositories.take() {
        patch.github_ci_repositories = Some(crate::config::normalize_github_ci_repositories(
            repositories,
        ));
    }
    patch.sound = normalize_optional_nullable_string(patch.sound);
    patch.url = normalize_optional_nullable_string(patch.url);
    Ok(patch)
}

fn normalize_probe_observability_patch(
    mut patch: ProbeObservabilityConfigPatch,
) -> ProbeObservabilityConfigPatch {
    patch.event_retention_days = patch
        .event_retention_days
        .map(|value| value.clamp(1, 3_650));
    patch.hook_event_max_lines = patch
        .hook_event_max_lines
        .map(|value| value.clamp(10, 10_000));
    patch.hook_cooldown_max_lines = patch
        .hook_cooldown_max_lines
        .map(|value| value.clamp(10, 10_000));
    patch.log_max_bytes = patch
        .log_max_bytes
        .map(|value| value.clamp(4_096, 8_388_608));
    patch
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalize_optional_nullable_string(value: Option<Option<String>>) -> Option<Option<String>> {
    value.map(|inner| {
        inner.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{
        plan_probe_settings_save, ProbeNotificationsSavePatch, ProbeSettingsSavePatch,
        ProbeSettingsSaveRequest,
    };

    #[test]
    fn probe_settings_save_plan_normalizes_nested_bark_key_and_patch() {
        let plan = plan_probe_settings_save(ProbeSettingsSaveRequest {
            probe: Some(ProbeSettingsSavePatch {
                poll_seconds: Some(1),
                recent_limit: Some(999),
                notifications: Some(ProbeNotificationsSavePatch {
                    device_key: Some("  bark-key  ".to_string()),
                    config: crate::config::ProbeNotificationsConfigPatch {
                        server_url: Some(" https://api.day.app ".to_string()),
                        group: Some("  ".to_string()),
                        ..Default::default()
                    },
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .expect("settings save should be allowed");

        assert_eq!(
            Some(plan.secret_write.as_ref().unwrap().secret_value.as_str()),
            Some("bark-key")
        );
        let probe = plan.config_patch.probe.as_ref().expect("probe patch");
        assert_eq!(probe.poll_seconds, Some(5));
        assert_eq!(probe.recent_limit, Some(500));
        let notifications = probe.notifications.as_ref().expect("notifications patch");
        assert_eq!(
            notifications.server_url.as_deref(),
            Some("https://api.day.app")
        );
        assert_eq!(notifications.group.as_deref(), Some("AgentHub"));
        assert!(super::probe_config_file_patch_has_changes(
            &plan.config_patch
        ));
    }

    #[test]
    fn probe_settings_save_plan_separates_config_write_from_secret_write_audit() {
        let plan = plan_probe_settings_save(ProbeSettingsSaveRequest {
            notifications: Some(ProbeNotificationsSavePatch {
                device_key: Some("  bark-secret  ".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .expect("shared settings planning");

        assert!(plan.config_patch.probe.is_none());
        assert!(!super::probe_config_file_patch_has_changes(
            &plan.config_patch
        ));
        assert_eq!(
            plan.secret_write.as_ref().map(|w| w.secret_value.as_str()),
            Some("bark-secret")
        );
        let serialized = serde_json::to_string(&plan).unwrap();
        assert!(!serialized.contains("bark-secret"));
    }
    #[test]
    fn nullable_notification_fields_round_trip_and_remove_only_the_requested_keys() {
        use serde_json::json;
        let original = "[probe.notifications] # kept\nsound = \"bell\"\nurl = \"https://example.invalid/\"\ngroup = \"Fixture\"\n[custom]\nkeep = true\n";
        for nested in [false, true] {
            for input in [
                json!({}),
                json!({"sound":null,"url":null}),
                json!({"sound":" ","url":"\t"}),
                json!({"sound":"chime","url":"https://example.invalid/new"}),
            ] {
                let request = if nested {
                    json!({"probe":{"notifications":input}})
                } else {
                    json!({"notifications":input})
                };
                let request: ProbeSettingsSaveRequest = serde_json::from_value(request).unwrap();
                let request =
                    serde_json::from_value(serde_json::to_value(request).unwrap()).unwrap();
                let plan = plan_probe_settings_save(request).unwrap();
                let patch: crate::config::ProbeConfigFilePatch =
                    serde_json::from_value(serde_json::to_value(plan.config_patch).unwrap())
                        .unwrap();
                let output = crate::config::patch_probe_config_toml(original, &patch).unwrap();
                let parsed: toml::Value = toml::from_str(&output).unwrap();
                let notifications = parsed["probe"]["notifications"].as_table().unwrap();
                if input.as_object().unwrap().is_empty() {
                    assert_eq!(output, original);
                } else if input["sound"] == "chime" {
                    assert_eq!(notifications["sound"].as_str(), Some("chime"));
                    assert_eq!(
                        notifications["url"].as_str(),
                        Some("https://example.invalid/new")
                    );
                } else {
                    assert!(!notifications.contains_key("sound"));
                    assert!(!notifications.contains_key("url"));
                }
                assert!(output.contains("[probe.notifications] # kept"));
                assert_eq!(notifications["group"].as_str(), Some("Fixture"));
                assert_eq!(parsed["custom"]["keep"].as_bool(), Some(true));
            }
        }
    }
}
