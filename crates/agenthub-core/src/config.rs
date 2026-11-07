use crate::{
    crypto::SecretBox,
    platform::{PlatformKind, PlatformPaths},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    net::IpAddr,
    path::{Path, PathBuf},
};

pub const DEFAULT_HOST_LABEL: &str = "AgentHub";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub codex: CodexConfig,
    #[serde(default)]
    pub probe: ProbeConfig,
    pub security: SecurityConfig,
    pub paths: PathConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexConfig {
    #[serde(default = "default_codex_home")]
    pub home: PathBuf,
    pub workspace: PathBuf,

    pub host_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_probe_poll_seconds")]
    pub poll_seconds: u64,
    #[serde(default = "default_probe_recent_limit")]
    pub recent_limit: usize,
    #[serde(default)]
    pub hooks: ProbeHooksConfig,
    #[serde(default)]
    pub notifications: ProbeNotificationsConfig,
    #[serde(default)]
    pub observability: ProbeObservabilityConfig,
    #[serde(default)]
    pub error_monitor: ProbeErrorMonitorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeErrorMonitorConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeHooksConfig {
    #[serde(default = "default_true")]
    pub manage_stop_hook: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeNotificationsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_bark_server_url")]
    pub server_url: String,
    #[serde(default)]
    pub sound: Option<String>,
    #[serde(default = "default_probe_notification_group")]
    pub group: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_true")]
    pub notify_completion: bool,
    #[serde(default = "default_true")]
    pub notify_reply_needed: bool,
    #[serde(default = "default_true")]
    pub notify_recoverable: bool,
    #[serde(default = "default_true")]
    pub notify_codex: bool,
    #[serde(default = "default_true")]
    pub notify_grok: bool,
    #[serde(default)]
    pub notify_grokbot: bool,
    #[serde(default)]
    pub notify_github_ci: bool,
    #[serde(default = "default_true")]
    pub notify_grok_completion: bool,
    #[serde(default = "default_true")]
    pub notify_grok_failure: bool,
    #[serde(default = "default_true")]
    pub notify_grok_reply_needed: bool,
    #[serde(default = "default_true")]
    pub notify_claude: bool,
    #[serde(default = "default_true")]
    pub notify_claude_completion: bool,
    #[serde(default = "default_true")]
    pub notify_claude_failure: bool,
    #[serde(default = "default_true")]
    pub notify_claude_reply_needed: bool,
    #[serde(default)]
    pub notify_antigravity: bool,
    #[serde(default)]
    pub notify_antigravity_completion: bool,
    #[serde(default)]
    pub notify_antigravity_failure: bool,
    #[serde(default)]
    pub notify_antigravity_reply_needed: bool,
    #[serde(default)]
    pub notify_antigravity_authorization: bool,
    #[serde(default)]
    pub notify_dsh: bool,
    #[serde(default)]
    pub notify_dsh_completion: bool,
    #[serde(default)]
    pub notify_dsh_failure: bool,
    #[serde(default)]
    pub notify_dsh_reply_needed: bool,
    #[serde(default)]
    pub notify_dsh_authorization: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeObservabilityConfig {
    #[serde(default = "default_event_retention_days")]
    pub event_retention_days: u32,
    #[serde(default = "default_hook_event_max_lines")]
    pub hook_event_max_lines: usize,
    #[serde(default = "default_hook_cooldown_max_lines")]
    pub hook_cooldown_max_lines: usize,
    #[serde(default = "default_log_max_bytes")]
    pub log_max_bytes: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeConfigFilePatch {
    pub codex: Option<CodexProbeConfigPatch>,
    pub probe: Option<ProbeSettingsPatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexProbeConfigPatch {
    #[serde(default, deserialize_with = "deserialize_optional_string_field")]
    pub home: Option<Option<String>>,
    pub workspace: Option<String>,
    pub host_label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeSettingsPatch {
    pub enabled: Option<bool>,
    pub poll_seconds: Option<u64>,
    pub recent_limit: Option<usize>,
    pub hooks: Option<ProbeHooksConfigPatch>,
    pub notifications: Option<ProbeNotificationsConfigPatch>,
    pub observability: Option<ProbeObservabilityConfigPatch>,
    pub error_monitor: Option<ProbeErrorMonitorConfigPatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeErrorMonitorConfigPatch {
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeHooksConfigPatch {
    pub manage_stop_hook: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeNotificationsConfigPatch {
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
    pub notify_github_ci: Option<bool>,
    pub notify_grok_completion: Option<bool>,
    pub notify_grok_failure: Option<bool>,
    pub notify_grok_reply_needed: Option<bool>,
    pub notify_claude: Option<bool>,
    pub notify_claude_completion: Option<bool>,
    pub notify_claude_failure: Option<bool>,
    pub notify_claude_reply_needed: Option<bool>,
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeObservabilityConfigPatch {
    pub event_retention_days: Option<u32>,
    pub hook_event_max_lines: Option<usize>,
    pub hook_cooldown_max_lines: Option<usize>,
    pub log_max_bytes: Option<usize>,
}

fn default_codex_home() -> PathBuf {
    PathBuf::from("auto")
}

fn default_true() -> bool {
    true
}

fn default_probe_poll_seconds() -> u64 {
    15
}

fn default_probe_recent_limit() -> usize {
    50
}

fn default_bark_server_url() -> String {
    "https://api.day.app".to_string()
}

fn default_probe_notification_group() -> String {
    "AgentHub".to_string()
}

fn default_hook_event_max_lines() -> usize {
    500
}

fn default_hook_cooldown_max_lines() -> usize {
    1_000
}

fn default_log_max_bytes() -> usize {
    5 * 1024 * 1024
}

fn default_event_retention_days() -> u32 {
    2
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            poll_seconds: default_probe_poll_seconds(),
            recent_limit: default_probe_recent_limit(),
            hooks: ProbeHooksConfig::default(),
            notifications: ProbeNotificationsConfig::default(),
            observability: ProbeObservabilityConfig::default(),
            error_monitor: ProbeErrorMonitorConfig::default(),
        }
    }
}

impl Default for ProbeHooksConfig {
    fn default() -> Self {
        Self {
            manage_stop_hook: true,
        }
    }
}

impl Default for ProbeNotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_url: default_bark_server_url(),
            sound: None,
            group: default_probe_notification_group(),
            url: None,
            notify_completion: true,
            notify_reply_needed: true,
            notify_recoverable: true,
            notify_codex: true,
            notify_grok: true,
            notify_grokbot: false,
            notify_github_ci: false,
            notify_grok_completion: true,
            notify_grok_failure: true,
            notify_grok_reply_needed: true,
            notify_claude: true,
            notify_claude_completion: true,
            notify_claude_failure: true,
            notify_claude_reply_needed: true,
            notify_antigravity: false,
            notify_antigravity_completion: false,
            notify_antigravity_failure: false,
            notify_antigravity_reply_needed: false,
            notify_antigravity_authorization: false,
            notify_dsh: false,
            notify_dsh_completion: false,
            notify_dsh_failure: false,
            notify_dsh_reply_needed: false,
            notify_dsh_authorization: false,
        }
    }
}

impl Default for ProbeObservabilityConfig {
    fn default() -> Self {
        Self {
            event_retention_days: default_event_retention_days(),
            hook_event_max_lines: default_hook_event_max_lines(),
            hook_cooldown_max_lines: default_hook_cooldown_max_lines(),
            log_max_bytes: default_log_max_bytes(),
        }
    }
}

impl Default for ProbeErrorMonitorConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,

    pub log_dir: PathBuf,
}

fn default_workspace_for_platform(platform: &PlatformPaths, home: &Path) -> PathBuf {
    match platform.kind {
        PlatformKind::Linux => PathBuf::from("/home/ubuntu/codex-workspace"),
        PlatformKind::Macos => home.join("agenthub-workspace"),
        PlatformKind::Windows => PathBuf::from(r"%USERPROFILE%\AgentHub\workspace"),
    }
}

fn default_desktop_workspace_for_platform(platform: &PlatformPaths, home: &Path) -> PathBuf {
    match platform.kind {
        PlatformKind::Linux | PlatformKind::Macos => home.join("agenthub-workspace"),
        PlatformKind::Windows => home.join("AgentHub/workspace"),
    }
}

fn current_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"))
}

fn current_platform_kind() -> PlatformKind {
    #[cfg(target_os = "macos")]
    {
        PlatformKind::Macos
    }
    #[cfg(target_os = "windows")]
    {
        PlatformKind::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        PlatformKind::Linux
    }
}

impl Config {
    pub fn for_platform_kind(kind: PlatformKind) -> Self {
        Self::for_platform_kind_with_home(kind, current_home_dir())
    }

    pub fn for_platform_kind_with_home(kind: PlatformKind, home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        let platform = PlatformPaths::for_kind_with_home(kind, &home);
        Self::for_platform_paths_with_home(platform, &home, false)
    }

    pub fn for_desktop_platform_kind(kind: PlatformKind) -> Self {
        Self::for_desktop_platform_kind_with_home(kind, current_home_dir())
    }

    pub fn for_desktop_platform_kind_with_home(
        kind: PlatformKind,
        home: impl Into<PathBuf>,
    ) -> Self {
        let home = home.into();
        let platform = PlatformPaths::for_desktop_kind_with_home(kind, &home);
        Self::for_platform_paths_with_home(platform, &home, true)
    }

    fn for_platform_paths_with_home(platform: PlatformPaths, home: &Path, desktop: bool) -> Self {
        Self {
            codex: CodexConfig {
                home: default_codex_home(),
                workspace: if desktop {
                    default_desktop_workspace_for_platform(&platform, home)
                } else {
                    default_workspace_for_platform(&platform, home)
                },

                host_label: DEFAULT_HOST_LABEL.to_string(),
            },
            probe: ProbeConfig::default(),
            security: SecurityConfig {
                secret_key: String::new(),
            },
            paths: PathConfig {
                data_dir: platform.data_dir.clone(),
                db_path: platform.data_dir.join("agenthub.sqlite"),

                log_dir: platform.log_dir.clone(),
            },
        }
    }

    pub fn current_default_config_path() -> PathBuf {
        PlatformPaths::current().config_file
    }

    pub fn current_desktop_config_path() -> PathBuf {
        PlatformPaths::desktop_current().config_file
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::for_platform_kind(current_platform_kind())
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            let mut config = Self::default();
            config.load_sibling_env(path)?;
            config.normalize()?;
            return Ok(config);
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
        Self::from_text(&text, path)
    }

    fn from_text(text: &str, path: &Path) -> Result<Self> {
        let mut config: Self =
            toml::from_str(text).with_context(|| format!("parse config {}", path.display()))?;
        config.load_sibling_env(path)?;
        config.normalize()?;
        Ok(config)
    }

    pub fn save_probe_patch(path: &Path, patch: &ProbeConfigFilePatch) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let updated = patch_probe_config_toml(&text, patch)?;
        let config = Self::from_text(&updated, path)?;
        if updated != text {
            write_config_atomic(path, &updated)?;
        }
        Ok(config)
    }

    /// The App and its monitor share the existing sibling env file. Server
    /// deployments continue to require their explicitly provisioned key.
    pub fn load_runtime(path: impl AsRef<Path>) -> Result<Self> {
        let config = Self::load(path.as_ref())?;
        #[cfg(target_os = "macos")]
        {
            let mut config = config;
            config.ensure_runtime_key(path.as_ref())?;
            Ok(config)
        }
        #[cfg(not(target_os = "macos"))]
        Ok(config)
    }

    #[cfg(any(target_os = "macos", test))]
    fn ensure_runtime_key(&mut self, path: &Path) -> Result<()> {
        if !self.security.secret_key.trim().is_empty() {
            self.secret_box()?;
            return Ok(());
        }
        let parent = path.parent().context("运行配置缺少目录")?;
        fs::create_dir_all(parent)?;
        let env_path = parent.join("env");
        let original = match fs::read_to_string(&env_path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let key = SecretBox::random_key_material();
        let mut updated = original
            .as_deref()
            .unwrap_or("")
            .lines()
            .filter(|line| {
                line.split_once('=')
                    .is_none_or(|(name, _)| name.trim() != "AGENTHUB_SECRET_KEY")
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !updated.is_empty() {
            updated.push('\n');
        }
        updated.push_str(&format!("AGENTHUB_SECRET_KEY={key}\n"));
        write_runtime_env(&env_path, &updated)?;
        self.security.secret_key = key;
        Ok(())
    }

    pub fn write_default(path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create config dir {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(&Self::default()).context("serialize default config")?;
        fs::write(path, text).with_context(|| format!("write config {}", path.display()))
    }

    pub fn write_runtime_if_missing(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        if path.is_file() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    pub fn normalize(&mut self) -> Result<()> {
        if let Some(value) = env::var("AGENTHUB_SECRET_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
        {
            self.security.secret_key = value;
        }
        if self.codex.host_label.trim().is_empty() {
            self.codex.host_label = DEFAULT_HOST_LABEL.to_string();
        }
        self.probe.normalize();
        Ok(())
    }

    pub fn secret_box(&self) -> Result<SecretBox> {
        let value = self.security.secret_key.trim();
        if value.is_empty() {
            anyhow::bail!("AGENTHUB_SECRET_KEY is required");
        }
        SecretBox::from_key_material(value)
    }

    fn load_sibling_env(&mut self, config_path: &Path) -> Result<()> {
        if !self.security.secret_key.trim().is_empty() {
            return Ok(());
        }
        let Some(parent) = config_path.parent() else {
            return Ok(());
        };
        let env_path = parent.join("env");
        if !env_path.exists() {
            return Ok(());
        }
        let text = fs::read_to_string(&env_path)
            .with_context(|| format!("read env {}", env_path.display()))?;
        if let Some(value) = env_file_value(&text, "AGENTHUB_SECRET_KEY") {
            self.security.secret_key = value;
        }
        Ok(())
    }
}

impl ProbeConfig {
    pub fn monitor_enabled(&self) -> bool {
        self.enabled
            && (self.error_monitor.enabled
                || self.notifications.enabled
                || self.notifications.notify_github_ci)
    }

    pub fn normalize(&mut self) {
        self.poll_seconds = self.poll_seconds.clamp(5, 3_600);
        self.recent_limit = self.recent_limit.clamp(1, 500);
        // Preserve invalid destinations so send-time validation fails closed. Replacing
        // one with the default service could disclose a key belonging to another server.
        let group = self.notifications.group.trim();
        if group.is_empty() {
            self.notifications.group = default_probe_notification_group();
        }
        self.observability.event_retention_days =
            self.observability.event_retention_days.clamp(1, 3_650);
        self.observability.hook_event_max_lines =
            self.observability.hook_event_max_lines.clamp(10, 10_000);
        self.observability.hook_cooldown_max_lines =
            self.observability.hook_cooldown_max_lines.clamp(10, 10_000);
        self.observability.log_max_bytes = self.observability.log_max_bytes.clamp(4_096, 8_388_608);
    }
}

fn env_file_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (line_key, value) = line.split_once('=')?;
        if line_key.trim() != key {
            return None;
        }
        Some(
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        )
    })
}

pub fn valid_probe_notification_server_url(value: &str) -> bool {
    use url::Host;

    // Reject parser repairs (backslashes, control characters and numeric-IP aliases)
    // before checking the canonical URL. No DNS resolution is performed here.
    let value = value.trim();
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\\')
    {
        return false;
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "https" | "http") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains(['@', '%']) {
        return false;
    }
    let Ok(parsed) = url::Url::parse(value) else {
        return false;
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    let Some(host) = parsed.host() else {
        return false;
    };
    let raw_host = if authority.starts_with('[') {
        let Some(end) = authority.find(']') else {
            return false;
        };
        &authority[1..end]
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    let ip = match host {
        Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        Host::Domain(_) => None,
    };
    if let Some(ip) = ip {
        // WHATWG accepts 127.1, octal, hexadecimal and integer IPv4 forms. Only
        // literal, unambiguous IP spellings are valid notification destinations.
        if raw_host.parse::<IpAddr>().ok() != Some(ip) {
            return false;
        }
        let ip = match ip {
            IpAddr::V6(ip) => ip
                .to_ipv4_mapped()
                .map(IpAddr::V4)
                .unwrap_or(IpAddr::V6(ip)),
            ip => ip,
        };
        if scheme == "http" {
            return ip.is_loopback();
        }
        return match ip {
            IpAddr::V4(ip) => {
                !(ip.is_loopback() || ip.is_unspecified() || ip.is_private() || ip.is_link_local())
            }
            IpAddr::V6(ip) => {
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local())
            }
        };
    }
    let Host::Domain(host) = host else {
        return false;
    };
    let host = host.trim_end_matches('.');
    if scheme == "http" {
        return host == "localhost";
    }
    !host.is_empty() && host != "localhost" && !host.ends_with(".localhost")
}

fn deserialize_optional_string_field<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

fn non_auto_string(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto")).then_some(value)
    })
}

pub fn patch_probe_config_toml(text: &str, patch: &ProbeConfigFilePatch) -> Result<String> {
    let mut editor = TomlPatchEditor::new(text);
    if let Some(codex) = patch.codex.as_ref() {
        if let Some(home) = codex.home.as_ref() {
            match non_auto_string(home.as_deref()) {
                Some(value) => editor.set_string("codex", "home", Some(value)),
                None => editor.remove_key("codex", "home"),
            }
        }
        editor.set_string("codex", "workspace", codex.workspace.as_deref());
        editor.set_string("codex", "host_label", codex.host_label.as_deref());
    }
    if let Some(probe) = patch.probe.as_ref() {
        editor.set_bool("probe", "enabled", probe.enabled);
        editor.set_u64("probe", "poll_seconds", probe.poll_seconds);
        editor.set_usize("probe", "recent_limit", probe.recent_limit);
        if let Some(hooks) = probe.hooks.as_ref() {
            editor.set_bool("probe.hooks", "manage_stop_hook", hooks.manage_stop_hook);
        }
        if let Some(error_monitor) = probe.error_monitor.as_ref() {
            editor.set_bool("probe.error_monitor", "enabled", error_monitor.enabled);
        }
        if let Some(notifications) = probe.notifications.as_ref() {
            editor.set_bool(
                "probe.notifications",
                "notify_codex",
                notifications.notify_codex,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_grok",
                notifications.notify_grok,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_grokbot",
                notifications.notify_grokbot,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_github_ci",
                notifications.notify_github_ci,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_grok_completion",
                notifications.notify_grok_completion,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_grok_failure",
                notifications.notify_grok_failure,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_grok_reply_needed",
                notifications.notify_grok_reply_needed,
            );

            editor.set_bool(
                "probe.notifications",
                "notify_claude",
                notifications.notify_claude,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_claude_completion",
                notifications.notify_claude_completion,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_claude_failure",
                notifications.notify_claude_failure,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_claude_reply_needed",
                notifications.notify_claude_reply_needed,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_antigravity",
                notifications.notify_antigravity,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_antigravity_completion",
                notifications.notify_antigravity_completion,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_antigravity_failure",
                notifications.notify_antigravity_failure,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_antigravity_reply_needed",
                notifications.notify_antigravity_reply_needed,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_antigravity_authorization",
                notifications.notify_antigravity_authorization,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_dsh",
                notifications.notify_dsh,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_dsh_completion",
                notifications.notify_dsh_completion,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_dsh_failure",
                notifications.notify_dsh_failure,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_dsh_reply_needed",
                notifications.notify_dsh_reply_needed,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_dsh_authorization",
                notifications.notify_dsh_authorization,
            );
            editor.set_bool("probe.notifications", "enabled", notifications.enabled);
            editor.set_string(
                "probe.notifications",
                "server_url",
                notifications.server_url.as_deref(),
            );
            if let Some(value) = notifications.sound.as_ref() {
                editor.set_string("probe.notifications", "sound", value.as_deref());
            }
            editor.set_string(
                "probe.notifications",
                "group",
                notifications.group.as_deref(),
            );
            if let Some(value) = notifications.url.as_ref() {
                editor.set_string("probe.notifications", "url", value.as_deref());
            }
            editor.set_bool(
                "probe.notifications",
                "notify_completion",
                notifications.notify_completion,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_reply_needed",
                notifications.notify_reply_needed,
            );
            editor.set_bool(
                "probe.notifications",
                "notify_recoverable",
                notifications.notify_recoverable,
            );
        }
        if let Some(observability) = probe.observability.as_ref() {
            editor.set_u32(
                "probe.observability",
                "event_retention_days",
                observability.event_retention_days,
            );
            editor.set_usize(
                "probe.observability",
                "hook_event_max_lines",
                observability.hook_event_max_lines,
            );
            editor.set_usize(
                "probe.observability",
                "hook_cooldown_max_lines",
                observability.hook_cooldown_max_lines,
            );
            editor.set_usize(
                "probe.observability",
                "log_max_bytes",
                observability.log_max_bytes,
            );
        }
    }
    Ok(editor.finish())
}

/// Replace the actual configuration file without exposing a truncated TOML document.
pub fn write_config_atomic(path: &Path, updated: &str) -> Result<()> {
    let permissions = fs::metadata(path)?.permissions();
    write_atomic(path, updated, Some(permissions))
}

#[cfg(any(target_os = "macos", test))]
fn write_runtime_env(path: &Path, updated: &str) -> Result<()> {
    write_atomic(path, updated, None)
}

fn write_atomic(path: &Path, updated: &str, permissions: Option<fs::Permissions>) -> Result<()> {
    use std::io::Write;
    let candidate = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&candidate)?;
    let result = (|| -> Result<()> {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        fs::rename(&candidate, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&candidate);
    }
    result
}

struct TomlPatchEditor {
    lines: Vec<String>,
}

impl TomlPatchEditor {
    fn new(text: &str) -> Self {
        Self {
            lines: text.lines().map(ToString::to_string).collect(),
        }
    }

    fn finish(self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    fn set_string(&mut self, section: &str, key: &str, value: Option<&str>) {
        self.set_value(section, key, value.map(toml_string));
    }

    fn set_bool(&mut self, section: &str, key: &str, value: Option<bool>) {
        self.set_value(section, key, value.map(|value| value.to_string()));
    }

    fn set_u32(&mut self, section: &str, key: &str, value: Option<u32>) {
        self.set_value(section, key, value.map(|value| value.to_string()));
    }

    fn set_u64(&mut self, section: &str, key: &str, value: Option<u64>) {
        self.set_value(section, key, value.map(|value| value.to_string()));
    }

    fn set_usize(&mut self, section: &str, key: &str, value: Option<usize>) {
        self.set_value(section, key, value.map(|value| value.to_string()));
    }

    fn set_value(&mut self, section: &str, key: &str, value: Option<String>) {
        let Some(value) = value else {
            return;
        };
        let (start, end) = self.ensure_section(section);
        for index in start + 1..end {
            let stripped = self.lines[index].trim();
            if stripped.starts_with('#') || !stripped.contains('=') {
                continue;
            }
            let Some((line_key, _)) = stripped.split_once('=') else {
                continue;
            };
            if line_key.trim() == key {
                self.lines[index] = format!("{key} = {value}");
                return;
            }
        }
        self.lines.insert(end, format!("{key} = {value}"));
    }

    fn remove_key(&mut self, section: &str, key: &str) {
        let Some((start, end)) = self.section_range(section) else {
            return;
        };
        if let Some(index) = (start + 1..end).find(|index| {
            let stripped = self.lines[*index].trim();
            if stripped.starts_with('#') || !stripped.contains('=') {
                return false;
            }
            stripped
                .split_once('=')
                .is_some_and(|(line_key, _)| line_key.trim() == key)
        }) {
            self.lines.remove(index);
        }
    }

    fn ensure_section(&mut self, section: &str) -> (usize, usize) {
        if let Some(range) = self.section_range(section) {
            return range;
        }
        if self
            .lines
            .last()
            .is_some_and(|line| !line.trim().is_empty())
        {
            self.lines.push(String::new());
        }
        let start = self.lines.len();
        self.lines.push(format!("[{section}]"));
        (start, self.lines.len())
    }

    fn section_range(&self, section: &str) -> Option<(usize, usize)> {
        let header = format!("[{section}]");
        let start = self.lines.iter().position(|line| line.trim() == header)?;
        let end = self
            .lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find_map(|(index, line)| {
                let trimmed = line.trim();
                (trimmed.starts_with('[') && trimmed.ends_with(']')).then_some(index)
            })
            .unwrap_or(self.lines.len());
        Some((start, end))
    }
}

fn toml_string(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(|ch| match ch {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            _ => vec![ch],
        })
        .collect::<String>();
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::{Config, ProbeConfigFilePatch, ProbeSettingsPatch};
    use crate::platform::{PlatformKind, PlatformPaths};
    use std::{fs, time::SystemTime};

    #[test]
    fn empty_runtime_key_is_replaced_without_duplicate_env_entries() {
        let root =
            std::env::temp_dir().join(format!("agenthub-empty-key-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(
            root.join("env"),
            "# fixture\nAGENTHUB_SECRET_KEY=\nUNCHANGED=fixture\n",
        )
        .unwrap();
        let mut config = Config::for_platform_kind_with_home(PlatformKind::Macos, &root);
        config.ensure_runtime_key(&path).unwrap();
        let text = fs::read_to_string(root.join("env")).unwrap();
        assert_eq!(text.matches("AGENTHUB_SECRET_KEY=").count(), 1);
        assert!(text.contains("UNCHANGED=fixture"));
        assert!(config.secret_box().is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_key_initialization_preserves_valid_keys_and_reports_read_errors() {
        use crate::{crypto::SecretBox, db::PanelDb};
        let root =
            std::env::temp_dir().join(format!("agenthub-runtime-key-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut keys = Vec::new();
        for index in 0..2 {
            let home = root.join(index.to_string());
            let mut config = Config::for_platform_kind_with_home(PlatformKind::Macos, &home);
            let path = home.join("config.toml");
            config.ensure_runtime_key(&path).unwrap();
            config.secret_box().unwrap();
            keys.push(config.security.secret_key.clone());
            config.write_runtime_if_missing(&path).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            let env = home.join("env");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&env).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            fs::write(
                &path,
                toml::to_string(&Config::for_platform_kind_with_home(
                    PlatformKind::Macos,
                    &home,
                ))
                .unwrap(),
            )
            .unwrap();
            let loaded = Config::load(&path).unwrap();
            assert_eq!(loaded.security.secret_key, config.security.secret_key);
            let env_bytes = fs::read(&env).unwrap();
            config.ensure_runtime_key(&path).unwrap();
            assert_eq!(fs::read(&env).unwrap(), env_bytes);
            let db =
                PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box().unwrap())
                    .unwrap();
            db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
                .unwrap();
            drop(db);
            let wrong = SecretBox::from_key_material(&SecretBox::random_key_material()).unwrap();
            assert!(PanelDb::open_with_secret_box(&config.paths.db_path, wrong)
                .unwrap()
                .get_secret_setting_bytes("probe_bark_device_key")
                .is_err());
            fs::remove_file(&env).unwrap();
            config.security.secret_key.clear();
            config.ensure_runtime_key(&path).unwrap();
            assert!(env.exists());
            config.security.secret_key = keys[index].clone();
            let db =
                PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box().unwrap())
                    .unwrap();
            assert_eq!(
                db.get_secret_setting_bytes("probe_bark_device_key")
                    .unwrap()
                    .unwrap(),
                b"fixture-device"
            );
            config.security.secret_key = "invalid-fixture".into();
            assert!(config.ensure_runtime_key(&path).is_err());
            fs::write(&path, "invalid = [").unwrap();
            assert!(Config::load_runtime(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), "invalid = [");
        }
        assert_ne!(keys[0], keys[1]);
        assert!(SecretBox::from_key_material(&keys[0]).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linux_desktop_config_uses_user_scoped_tauri_paths_not_server_paths() {
        let config =
            Config::for_desktop_platform_kind_with_home(PlatformKind::Linux, "/home/alice");

        assert_eq!(
            config.paths.data_dir.to_string_lossy(),
            "/home/alice/.local/share/AgentHub"
        );
        assert_eq!(
            Config::current_default_config_path(),
            PlatformPaths::current().config_file
        );
        assert_eq!(
            config.codex.workspace.to_string_lossy(),
            "/home/alice/agenthub-workspace"
        );
        assert!(!config.paths.data_dir.starts_with("/var/lib/agenthub-webd"));
        assert!(!config.codex.workspace.starts_with("/home/ubuntu"));
    }

    #[test]
    fn default_config_includes_builtin_probe_settings() {
        let config = Config::default();

        assert!(config.probe.enabled);
        assert_eq!(config.probe.poll_seconds, 15);
        assert_eq!(config.probe.recent_limit, 50);
        assert!(config.probe.hooks.manage_stop_hook);
        assert!(!config.probe.notifications.enabled);
        assert_eq!(config.probe.notifications.server_url, "https://api.day.app");
        assert!(config.probe.notifications.notify_reply_needed);
        assert!(config.probe.notifications.notify_recoverable);
    }

    #[test]
    fn default_config_matches_mac_app_probe_runtime_defaults() {
        let config = Config::default();

        assert_eq!(config.probe.observability.hook_event_max_lines, 500);
        assert_eq!(config.probe.observability.hook_cooldown_max_lines, 1_000);
        assert_eq!(config.probe.observability.log_max_bytes, 5 * 1024 * 1024);
    }

    #[test]
    fn probe_notification_server_url_requires_https_except_loopback_http() {
        let mut config = Config::default();
        config.probe.notifications.server_url = "http://example.com".to_string();
        config.normalize().unwrap();

        assert_eq!(config.probe.notifications.server_url, "http://example.com");

        config.probe.notifications.server_url = "http://127.0.0.1:8080".to_string();
        config.normalize().unwrap();
        assert_eq!(
            config.probe.notifications.server_url,
            "http://127.0.0.1:8080"
        );

        config.probe.notifications.server_url = "https://bark.example.com".to_string();
        config.normalize().unwrap();
        assert_eq!(
            config.probe.notifications.server_url,
            "https://bark.example.com"
        );
    }

    #[test]
    fn probe_config_patch_preserves_unknown_sections_and_comments() {
        let input = r#"# hand kept
[server]
listen = "127.0.0.1:15742"

[custom]
keep = "yes"

[codex]
home = "/root/.codex"
host_label = "old-host"

[probe]
enabled = true
poll_seconds = 15

[probe.hooks]
manage_stop_hook = true

[probe.notifications]
enabled = false
group = "old"
"#;
        let patch = ProbeConfigFilePatch {
            codex: Some(super::CodexProbeConfigPatch {
                home: Some(Some("/srv/codex".into())),
                host_label: Some("192.0.2.10".into()),
                ..Default::default()
            }),
            probe: Some(ProbeSettingsPatch {
                enabled: Some(false),
                poll_seconds: Some(30),
                recent_limit: Some(80),
                notifications: Some(super::ProbeNotificationsConfigPatch {
                    enabled: Some(true),
                    group: Some("AgentHub".into()),
                    notify_completion: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        };

        let output = super::patch_probe_config_toml(input, &patch).unwrap();

        assert!(output.contains("# hand kept"));
        assert!(output.contains("[custom]\nkeep = \"yes\""));
        assert!(output.contains("home = \"/srv/codex\""));
        assert!(output.contains("host_label = \"192.0.2.10\""));
        assert!(output.contains("[probe]\nenabled = false\npoll_seconds = 30"));
        assert!(output.contains("recent_limit = 80"));
        assert!(output.contains("[probe.notifications]"));
        assert!(output.contains("enabled = true"));
        assert!(output.contains("group = \"AgentHub\""));
    }

    #[test]
    fn config_accepts_missing_codex_home_as_auto() {
        let input = r#"
[server]
listen = "127.0.0.1:15742"
trust_forwarded_headers = true

[codex]
workspace = "/home/ubuntu/codex-workspace"
host_label = "192.0.2.10"

[security]
secret_key = "test-secret"
cookie_secure = true
session_ttl_seconds = 31536000
login_rate_limit_per_minute = 8

[paths]
data_dir = "/var/lib/agenthub-webd"
db_path = "/var/lib/agenthub-webd/agenthub.sqlite"
webui_dir = "/usr/share/agenthub-webd/webui"
log_dir = "/var/log/agenthub-webd"

"#;
        let config: Config = toml::from_str(input).unwrap();

        assert_eq!(config.codex.home.to_string_lossy(), "auto");
    }

    #[test]
    fn probe_config_patch_removes_codex_home_for_auto_discovery() {
        let input = r#"
[codex]
home = "/root/.codex"
workspace = "/home/ubuntu/codex-workspace"
"#;
        let patch: ProbeConfigFilePatch =
            serde_json::from_str(r#"{"codex":{"home":null}}"#).unwrap();
        let output = super::patch_probe_config_toml(input, &patch).unwrap();

        assert!(!output.contains("home = "));
        assert!(output.contains("workspace = \"/home/ubuntu/codex-workspace\""));
    }

    #[test]
    fn load_reads_secret_key_from_sibling_env_file() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agenthub-config-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        let env_path = dir.join("env");
        let mut config = Config::default();
        config.security.secret_key.clear();
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        fs::write(
            &env_path,
            "AGENTHUB_SECRET_KEY=7q9DCmCPyxnTrH3FhrV1sUJol1yqPgscQsBnR-mXA2E\n",
        )
        .unwrap();

        let loaded = Config::load(&config_path).unwrap();

        assert_eq!(
            loaded.security.secret_key,
            "7q9DCmCPyxnTrH3FhrV1sUJol1yqPgscQsBnR-mXA2E"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_config_uses_api_auth_and_neutral_host() {
        let config = Config::default();

        assert_eq!(config.codex.host_label, "AgentHub");
        let serialized = toml::to_string(&config).unwrap();
        for retired in [
            "cookie_secure",
            "session_ttl_seconds",
            "turnstile",
            "webui_dir",
            "public_base_url",
            "trust_forwarded_headers",
        ] {
            assert!(!serialized.contains(retired), "retired field {retired}");
        }
    }
}

#[cfg(all(test, unix))]
mod atomic_write_tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn saved_patch_loads_env_and_normalizes_before_replacing_config() {
        let root =
            std::env::temp_dir().join(format!("agenthub-config-save-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let target = root.join("config.toml");
        let config = Config::for_platform_kind_with_home(PlatformKind::Macos, &root);
        fs::write(&target, toml::to_string(&config).unwrap()).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let key = SecretBox::random_key_material();
        write_runtime_env(&root.join("env"), &format!("AGENTHUB_SECRET_KEY={key}\n")).unwrap();
        let saved = Config::save_probe_patch(
            &target,
            &ProbeConfigFilePatch {
                probe: Some(ProbeSettingsPatch {
                    enabled: Some(true),
                    poll_seconds: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(saved.probe.enabled);
        assert_eq!(saved.probe.poll_seconds, 5);
        assert_eq!(saved.security.secret_key, key);
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            fs::metadata(root.join("env")).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let before = fs::metadata(&target).unwrap();
        Config::save_probe_patch(
            &target,
            &ProbeConfigFilePatch {
                probe: Some(ProbeSettingsPatch {
                    enabled: Some(true),
                    poll_seconds: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let after = fs::metadata(&target).unwrap();
        assert_eq!(
            before.ino(),
            after.ino(),
            "identical patch does not replace the file"
        );
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        assert_eq!(Config::load(&target).unwrap().probe, saved.probe);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replaces_complete_content_preserving_permissions() {
        let root = std::env::temp_dir().join(format!("agenthub-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let target = root.join("config.toml");
        fs::write(&target, "enabled = false\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        write_config_atomic(&target, "enabled = true\n").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "enabled = true\n");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert!(write_config_atomic(&root.join("missing.toml"), "unused").is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
