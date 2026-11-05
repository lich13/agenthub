use crate::{
    codex::resolve_codex_paths,
    config::{
        valid_probe_notification_server_url, CodexProbeConfigPatch, Config, ProbeConfig,
        ProbeConfigFilePatch, ProbeErrorMonitorConfigPatch, ProbeHooksConfigPatch,
        ProbeNotificationsConfig, ProbeNotificationsConfigPatch, ProbeObservabilityConfigPatch,
        ProbeSettingsPatch,
    },
    platform::PlatformPaths,
    services::system::{require_capability, Capability},
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeSettingsViewPlan {
    pub required_capability: Capability,
    pub settings: SettingsView,
}

#[derive(Debug, Clone, Copy)]
pub struct SettingsUseCases<'a> {
    config: &'a Config,
    platform: &'a PlatformPaths,
}

impl<'a> SettingsUseCases<'a> {
    pub fn new(config: &'a Config, platform: &'a PlatformPaths) -> Self {
        Self { config, platform }
    }

    pub fn probe_settings_view(
        self,
        bark_device_key: ProbeSecretState,
    ) -> Result<ProbeSettingsViewPlan> {
        probe_settings_view_with_capability(self.config, self.platform, bark_device_key)
    }

    pub fn save_probe_settings(
        self,
        request: ProbeSettingsSaveRequest,
    ) -> Result<ProbeSettingsSavePlan> {
        plan_probe_settings_save(self.platform, request)
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

pub fn probe_settings_view_with_capability(
    config: &Config,
    platform: &PlatformPaths,
    bark_device_key: ProbeSecretState,
) -> Result<ProbeSettingsViewPlan> {
    require_capability(platform, Capability::Settings)?;
    Ok(ProbeSettingsViewPlan {
        required_capability: Capability::Settings,
        settings: build_settings_view(config, bark_device_key),
    })
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

pub fn merge_probe_notification_patch(
    target: &mut ProbeNotificationsConfigPatch,
    source: ProbeNotificationsConfigPatch,
) {
    if source.enabled.is_some() {
        target.enabled = source.enabled;
    }
    if source.server_url.is_some() {
        target.server_url = source.server_url;
    }
    if source.sound.is_some() {
        target.sound = source.sound;
    }
    if source.group.is_some() {
        target.group = source.group;
    }
    if source.url.is_some() {
        target.url = source.url;
    }
    if source.notify_completion.is_some() {
        target.notify_completion = source.notify_completion;
    }
    if source.notify_reply_needed.is_some() {
        target.notify_reply_needed = source.notify_reply_needed;
    }
    if source.notify_codex.is_some() {
        target.notify_codex = source.notify_codex;
    }
    if source.notify_grok.is_some() {
        target.notify_grok = source.notify_grok;
    }
    if source.notify_grok_completion.is_some() {
        target.notify_grok_completion = source.notify_grok_completion;
    }
    if source.notify_grok_failure.is_some() {
        target.notify_grok_failure = source.notify_grok_failure;
    }
    if source.notify_grok_reply_needed.is_some() {
        target.notify_grok_reply_needed = source.notify_grok_reply_needed;
    }
    if source.notify_claude.is_some() {
        target.notify_claude = source.notify_claude;
    }
    if source.notify_claude_completion.is_some() {
        target.notify_claude_completion = source.notify_claude_completion;
    }
    if source.notify_claude_failure.is_some() {
        target.notify_claude_failure = source.notify_claude_failure;
    }
    if source.notify_claude_reply_needed.is_some() {
        target.notify_claude_reply_needed = source.notify_claude_reply_needed;
    }
    if source.notify_antigravity.is_some() {
        target.notify_antigravity = source.notify_antigravity;
    }
    if source.notify_antigravity_completion.is_some() {
        target.notify_antigravity_completion = source.notify_antigravity_completion;
    }
    if source.notify_antigravity_failure.is_some() {
        target.notify_antigravity_failure = source.notify_antigravity_failure;
    }
    if source.notify_antigravity_reply_needed.is_some() {
        target.notify_antigravity_reply_needed = source.notify_antigravity_reply_needed;
    }
    if source.notify_antigravity_authorization.is_some() {
        target.notify_antigravity_authorization = source.notify_antigravity_authorization;
    }
    if source.notify_dsh.is_some() {
        target.notify_dsh = source.notify_dsh;
    }
    if source.notify_dsh_completion.is_some() {
        target.notify_dsh_completion = source.notify_dsh_completion;
    }
    if source.notify_dsh_failure.is_some() {
        target.notify_dsh_failure = source.notify_dsh_failure;
    }
    if source.notify_dsh_reply_needed.is_some() {
        target.notify_dsh_reply_needed = source.notify_dsh_reply_needed;
    }
    if source.notify_dsh_authorization.is_some() {
        target.notify_dsh_authorization = source.notify_dsh_authorization;
    }
    if source.notify_recoverable.is_some() {
        target.notify_recoverable = source.notify_recoverable;
    }
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
    pub enabled: Option<bool>,
    pub server_url: Option<String>,
    pub sound: Option<Option<String>>,
    pub group: Option<String>,
    pub url: Option<Option<String>>,
    pub notify_completion: Option<bool>,
    pub notify_reply_needed: Option<bool>,
    pub notify_recoverable: Option<bool>,
    pub notify_codex: Option<bool>,
    pub notify_grok: Option<bool>,
    pub notify_grokbot: Option<bool>,
    pub notify_grok_completion: Option<bool>,
    pub notify_grok_failure: Option<bool>,
    pub notify_grok_reply_needed: Option<bool>,
    pub notify_claude_reply_needed: Option<bool>,
    pub notify_claude_failure: Option<bool>,
    pub notify_claude_completion: Option<bool>,
    pub notify_claude: Option<bool>,
    pub notify_antigravity: Option<bool>,
    pub notify_antigravity_completion: Option<bool>,
    pub notify_antigravity_failure: Option<bool>,
    pub notify_antigravity_reply_needed: Option<bool>,
    pub notify_antigravity_authorization: Option<bool>,
    pub notify_dsh: Option<bool>,
    pub notify_dsh_completion: Option<bool>,
    pub notify_dsh_failure: Option<bool>,
    pub notify_dsh_reply_needed: Option<bool>,
    pub notify_dsh_authorization: Option<bool>,

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
    pub audit_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeSettingsSavePlan {
    pub required_capability: Capability,
    pub config_patch: ProbeConfigFilePatch,
    pub config_write: Option<ProbeConfigWritePlan>,
    #[serde(default, skip_serializing)]
    pub bark_device_key: Option<String>,
    pub secret_writes: Vec<SecretSettingWritePlan>,
    pub audit_detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeConfigWritePlan {
    pub patch: ProbeConfigFilePatch,
}

pub fn plan_probe_settings_save(
    platform: &PlatformPaths,
    request: ProbeSettingsSaveRequest,
) -> Result<ProbeSettingsSavePlan> {
    require_capability(platform, Capability::Settings)?;
    let normalized = normalize_probe_settings_save_request(request)?;
    let secret_writes: Vec<SecretSettingWritePlan> = normalized
        .bark_device_key
        .as_deref()
        .map(bark_device_key_write_plan)
        .into_iter()
        .collect();
    let config_write = probe_config_file_patch_has_changes(&normalized.config_patch).then(|| {
        ProbeConfigWritePlan {
            patch: normalized.config_patch.clone(),
        }
    });
    let audit_detail = probe_settings_save_audit_detail(&config_write, &secret_writes);
    Ok(ProbeSettingsSavePlan {
        required_capability: Capability::Settings,
        config_patch: normalized.config_patch,
        config_write,
        bark_device_key: normalized.bark_device_key,
        secret_writes,
        audit_detail,
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
        let (notifications_patch, top_level_bark_device_key) =
            notifications.into_config_patch_and_bark_key();
        if !is_probe_notifications_patch_empty(&notifications_patch) {
            let probe = probe_patch.get_or_insert_with(ProbeSettingsPatch::default);
            let target = probe
                .notifications
                .get_or_insert_with(ProbeNotificationsConfigPatch::default);
            merge_probe_notification_patch(target, notifications_patch);
        }
        if top_level_bark_device_key.is_some() {
            bark_device_key = top_level_bark_device_key;
        }
    }

    if probe_patch
        .as_ref()
        .is_some_and(is_probe_settings_patch_empty)
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
                let (patch, bark_device_key) = notifications.into_config_patch_and_bark_key();
                let patch = (!is_probe_notifications_patch_empty(&patch)).then_some(patch);
                (patch, bark_device_key)
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
            (!is_probe_settings_patch_empty(&patch)).then_some(patch),
            bark_device_key,
        )
    }
}

impl ProbeNotificationsSavePatch {
    fn into_config_patch_and_bark_key(self) -> (ProbeNotificationsConfigPatch, Option<String>) {
        (
            ProbeNotificationsConfigPatch {
                enabled: self.enabled,
                server_url: self.server_url,
                sound: self.sound,
                group: self.group,
                url: self.url,
                notify_completion: self.notify_completion,
                notify_reply_needed: self.notify_reply_needed,
                notify_recoverable: self.notify_recoverable,
                notify_codex: self.notify_codex,
                notify_grok: self.notify_grok,
                notify_grokbot: self.notify_grokbot,
                notify_grok_completion: self.notify_grok_completion,
                notify_grok_failure: self.notify_grok_failure,
                notify_grok_reply_needed: self.notify_grok_reply_needed,
                notify_claude_reply_needed: self.notify_claude_reply_needed,
                notify_claude_failure: self.notify_claude_failure,
                notify_claude_completion: self.notify_claude_completion,
                notify_claude: self.notify_claude,
                notify_antigravity: self.notify_antigravity,
                notify_antigravity_completion: self.notify_antigravity_completion,
                notify_antigravity_failure: self.notify_antigravity_failure,
                notify_antigravity_reply_needed: self.notify_antigravity_reply_needed,
                notify_antigravity_authorization: self.notify_antigravity_authorization,
                notify_dsh: self.notify_dsh,
                notify_dsh_completion: self.notify_dsh_completion,
                notify_dsh_failure: self.notify_dsh_failure,
                notify_dsh_reply_needed: self.notify_dsh_reply_needed,
                notify_dsh_authorization: self.notify_dsh_authorization,
            },
            normalize_bark_device_key(self.device_key),
        )
    }
}

fn bark_device_key_write_plan(secret_value: &str) -> SecretSettingWritePlan {
    SecretSettingWritePlan {
        setting_key: PROBE_BARK_DEVICE_KEY_SETTING.to_string(),
        secret_value: secret_value.to_string(),
        audit_value: "[configured]".to_string(),
    }
}

fn probe_settings_save_audit_detail(
    config_write: &Option<ProbeConfigWritePlan>,
    secret_writes: &[SecretSettingWritePlan],
) -> Value {
    let mut detail = serde_json::Map::new();
    detail.insert("config_write".to_string(), json!(config_write.is_some()));
    for write in secret_writes {
        detail.insert(write.setting_key.clone(), json!(write.audit_value));
    }
    Value::Object(detail)
}

fn probe_config_file_patch_has_changes(patch: &ProbeConfigFilePatch) -> bool {
    patch.codex.is_some() || patch.probe.is_some()
}

fn is_probe_settings_patch_empty(patch: &ProbeSettingsPatch) -> bool {
    patch.enabled.is_none()
        && patch.poll_seconds.is_none()
        && patch.recent_limit.is_none()
        && patch.hooks.is_none()
        && patch.notifications.is_none()
        && patch.observability.is_none()
        && patch.error_monitor.is_none()
}

fn is_probe_notifications_patch_empty(patch: &ProbeNotificationsConfigPatch) -> bool {
    patch.enabled.is_none()
        && patch.server_url.is_none()
        && patch.sound.is_none()
        && patch.group.is_none()
        && patch.url.is_none()
        && patch.notify_completion.is_none()
        && patch.notify_reply_needed.is_none()
        && patch.notify_recoverable.is_none()
        && patch.notify_codex.is_none()
        && patch.notify_grok.is_none()
        && patch.notify_grok_completion.is_none()
        && patch.notify_grok_failure.is_none()
        && patch.notify_grok_reply_needed.is_none()
        && patch.notify_claude.is_none()
        && patch.notify_claude_completion.is_none()
        && patch.notify_claude_failure.is_none()
        && patch.notify_claude_reply_needed.is_none()
        && patch.notify_antigravity.is_none()
        && patch.notify_antigravity_completion.is_none()
        && patch.notify_antigravity_failure.is_none()
        && patch.notify_antigravity_reply_needed.is_none()
        && patch.notify_antigravity_authorization.is_none()
        && patch.notify_dsh.is_none()
        && patch.notify_dsh_completion.is_none()
        && patch.notify_dsh_failure.is_none()
        && patch.notify_dsh_reply_needed.is_none()
        && patch.notify_dsh_authorization.is_none()
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
    use crate::{
        platform::{PlatformKind, PlatformPaths},
        services::system::Capability,
    };

    #[test]
    fn probe_settings_save_plan_normalizes_nested_bark_key_and_patch() {
        let platform = PlatformPaths::for_kind(PlatformKind::Linux);
        let plan = plan_probe_settings_save(
            &platform,
            ProbeSettingsSaveRequest {
                probe: Some(ProbeSettingsSavePatch {
                    poll_seconds: Some(1),
                    recent_limit: Some(999),
                    notifications: Some(ProbeNotificationsSavePatch {
                        device_key: Some("  bark-key  ".to_string()),
                        server_url: Some(" https://api.day.app ".to_string()),
                        group: Some("  ".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .expect("settings save should be allowed on Linux");

        assert_eq!(plan.required_capability, Capability::Settings);
        assert_eq!(plan.bark_device_key.as_deref(), Some("bark-key"));
        let probe = plan.config_patch.probe.expect("probe patch");
        assert_eq!(probe.poll_seconds, Some(5));
        assert_eq!(probe.recent_limit, Some(500));
        let notifications = probe.notifications.expect("notifications patch");
        assert_eq!(
            notifications.server_url.as_deref(),
            Some("https://api.day.app")
        );
        assert_eq!(notifications.group.as_deref(), Some("AgentHub"));
        assert!(plan.config_write.is_some());
    }

    #[test]
    fn probe_settings_save_plan_requires_shared_settings_capability() {
        let platform = PlatformPaths::for_kind(PlatformKind::Windows);
        let err = plan_probe_settings_save(&platform, ProbeSettingsSaveRequest::default())
            .expect_err("Windows should not allow settings facade");

        assert!(err
            .to_string()
            .contains("settings is unavailable on windows"));
    }

    #[test]
    fn probe_settings_save_plan_separates_config_write_from_secret_write_audit() {
        let platform = PlatformPaths::for_kind(PlatformKind::Macos);
        let plan = plan_probe_settings_save(
            &platform,
            ProbeSettingsSaveRequest {
                notifications: Some(ProbeNotificationsSavePatch {
                    device_key: Some("  bark-secret  ".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .expect("macOS should allow shared settings planning");

        assert!(plan.config_patch.probe.is_none());
        assert!(plan.config_write.is_none());
        assert_eq!(plan.secret_writes.len(), 1);
        assert_eq!(plan.secret_writes[0].secret_value, "bark-secret");
        assert_eq!(
            plan.audit_detail["probe_bark_device_key"],
            serde_json::json!("[configured]")
        );
        let serialized = serde_json::to_string(&plan).unwrap();
        assert!(serialized.contains("[configured]"));
        assert!(!serialized.contains("bark-secret"));
    }
}
