use crate::shell::{shell_quote, QuoteStyle};
use crate::{
    crypto::SecretBox,
    platform::{PlatformKind, PlatformPaths},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

pub const DEFAULT_HOST_LABEL: &str = "AgentHub";
const DEFAULT_LOOPBACK_PORT: u16 = 15742;
const LEGACY_LOOPBACK_PORT: u16 = 15732;
const LEGACY_PRECHECK_COMMAND_SIMPLE: &str = "codex --version && sudo -n codex --version && /usr/local/bin/codex-raw --version && sqlite3 /root/.codex/state_5.sqlite 'pragma integrity_check;' && /home/ubuntu/codex-admin/bin/codex-cloud-doctor";
const LEGACY_PRECHECK_COMMAND_WITH_AUDIT: &str = "codex --version && sudo -n codex --version && /usr/local/bin/codex-raw --version && readlink -f /usr/local/bin/codex && readlink -f /usr/local/bin/codex-raw && sqlite3 /root/.codex/state_5.sqlite 'pragma integrity_check;' && sqlite3 /root/.codex/state_5.sqlite \"select count(*) total, sum(archived_at is null) active, sum(archived_at is not null) archived from threads;\" && /home/ubuntu/codex-admin/bin/codex-cloud-doctor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub codex: CodexConfig,
    #[serde(default)]
    pub probe: ProbeConfig,
    pub security: SecurityConfig,
    pub paths: PathConfig,
    pub update: UpdateConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub listen: SocketAddr,
}

impl ServerConfig {
    /// The API is reached through the authenticated reverse proxy, never a LAN socket.
    pub fn validate_listen(&self) -> Result<()> {
        let address = match self.listen.ip() {
            IpAddr::V6(ip) => ip
                .to_ipv4_mapped()
                .map(IpAddr::V4)
                .unwrap_or(IpAddr::V6(ip)),
            address => address,
        };
        anyhow::ensure!(
            address.is_loopback(),
            "server.listen must be a loopback address"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexConfig {
    #[serde(default = "default_codex_home")]
    pub home: PathBuf,
    pub workspace: PathBuf,
    #[serde(default, skip_serializing)]
    pub app_server_service: String,
    #[serde(default, skip_serializing)]
    pub app_server_socket: Option<PathBuf>,
    #[serde(default, skip_serializing)]
    pub bridge_enabled: bool,
    #[serde(default, skip_serializing)]
    pub bridge_transport: Option<String>,
    #[serde(default, skip_serializing)]
    pub bridge_timeout_seconds: Option<u64>,
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
    #[serde(default = "default_true")]
    pub reload_app_server_after_install: bool,
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
    pub reload_app_server_after_install: Option<bool>,
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
            reload_app_server_after_install: false,
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
    #[serde(default = "default_auth_rate_limit")]
    pub auth_rate_limit_per_minute: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,

    pub log_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    pub precheck_command: String,
    pub update_command: String,
    pub prune_command: String,
    pub doctor_command: String,
    #[serde(default = "default_panel_update_command")]
    pub panel_update_command: String,
    #[serde(default = "default_panel_precheck_command")]
    pub panel_precheck_command: String,
}

fn default_auth_rate_limit() -> u32 {
    8
}

fn default_update_command() -> String {
    "/usr/local/bin/agenthub-codex-update".to_string()
}

fn default_precheck_command() -> String {
    "/usr/local/bin/agenthub-codex-precheck".to_string()
}

fn default_prune_command() -> String {
    "/usr/local/bin/agenthub-codex-prune".to_string()
}

fn default_panel_update_command() -> String {
    crate::update::default_panel_update_command()
}

fn default_panel_precheck_command() -> String {
    default_panel_precheck_command_for_platform(&PlatformPaths::current())
}

fn default_panel_precheck_command_for_platform(platform: &PlatformPaths) -> String {
    match platform.kind {
        PlatformKind::Linux => crate::update::PANEL_PRECHECK_DEFAULT.to_string(),
        PlatformKind::Macos => format!(
            "test -d {}",
            shell_quote(&platform.data_dir.display().to_string(), QuoteStyle::Always)
        ),
        PlatformKind::Windows => "agenthub-update --precheck".to_string(),
    }
}

fn default_doctor_command_for_platform(platform: &PlatformPaths) -> String {
    match platform.kind {
        PlatformKind::Linux => "/home/ubuntu/codex-admin/bin/codex-cloud-doctor".to_string(),
        _ => format!(
            "{} --config {} doctor",
            shell_quote(
                &platform.daemon_binary().display().to_string(),
                QuoteStyle::Always
            ),
            shell_quote(
                &platform.config_file.display().to_string(),
                QuoteStyle::Always
            )
        ),
    }
}

fn default_desktop_doctor_command_for_platform(platform: &PlatformPaths) -> String {
    format!(
        "{} --config {} doctor",
        shell_quote(
            &platform.daemon_binary().display().to_string(),
            QuoteStyle::Always
        ),
        shell_quote(
            &platform.config_file.display().to_string(),
            QuoteStyle::Always
        )
    )
}

fn default_desktop_panel_precheck_command_for_platform(platform: &PlatformPaths) -> String {
    format!(
        "test -d {}",
        shell_quote(&platform.data_dir.display().to_string(), QuoteStyle::Always)
    )
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
        let listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_LOOPBACK_PORT);
        Self {
            server: ServerConfig { listen },
            codex: CodexConfig {
                home: default_codex_home(),
                workspace: if desktop {
                    default_desktop_workspace_for_platform(&platform, home)
                } else {
                    default_workspace_for_platform(&platform, home)
                },
                app_server_service: String::new(),
                app_server_socket: None,
                bridge_enabled: false,
                bridge_transport: None,
                bridge_timeout_seconds: None,
                host_label: DEFAULT_HOST_LABEL.to_string(),
            },
            probe: ProbeConfig::default(),
            security: SecurityConfig {
                secret_key: String::new(),
                auth_rate_limit_per_minute: default_auth_rate_limit(),
            },
            paths: PathConfig {
                data_dir: platform.data_dir.clone(),
                db_path: platform.data_dir.join("agenthub.sqlite"),

                log_dir: platform.log_dir.clone(),
            },
            update: UpdateConfig {
                precheck_command: default_precheck_command(),
                update_command: default_update_command(),
                prune_command: default_prune_command(),
                doctor_command: if desktop {
                    default_desktop_doctor_command_for_platform(&platform)
                } else {
                    default_doctor_command_for_platform(&platform)
                },
                panel_update_command: default_panel_update_command(),
                panel_precheck_command: if desktop {
                    default_desktop_panel_precheck_command_for_platform(&platform)
                } else {
                    default_panel_precheck_command_for_platform(&platform)
                },
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
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
        let migrated = migrate_all_retired_config(&text)?;
        let mut config: Self = toml::from_str(migrated.as_deref().unwrap_or(&text))
            .with_context(|| format!("parse config {}", path.display()))?;
        config.server.validate_listen()?;
        config.load_sibling_env(path)?;
        config.normalize()?;
        if let Some(updated) = migrated {
            replace_config_atomically(path, &text, &updated)?;
        }
        Ok(config)
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

    pub fn normalize(&mut self) -> Result<()> {
        self.server.validate_listen()?;
        if self.server.listen
            == SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), LEGACY_LOOPBACK_PORT)
        {
            self.server.listen =
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_LOOPBACK_PORT);
        }
        if let Ok(value) = env::var("AGENTHUB_SECRET_KEY") {
            self.security.secret_key = value;
        }
        self.security.auth_rate_limit_per_minute =
            self.security.auth_rate_limit_per_minute.clamp(1, 120);
        if self.codex.host_label.trim().is_empty() {
            self.codex.host_label = DEFAULT_HOST_LABEL.to_string();
        }
        self.probe.normalize();
        if is_legacy_precheck_command(&self.update.precheck_command) {
            self.update.precheck_command = default_precheck_command();
        }
        if self.update.precheck_command == "/usr/local/bin/codex-cloud-panel-codex-precheck" {
            self.update.precheck_command = default_precheck_command();
        }
        if self.update.update_command == "/usr/local/bin/codex-cloud-panel-codex-update" {
            self.update.update_command = default_update_command();
        }
        if self.update.prune_command == "/usr/local/bin/codex-cloud-panel-codex-prune" {
            self.update.prune_command = default_prune_command();
        }
        if self.update.panel_update_command.trim().is_empty() {
            self.update.panel_update_command = default_panel_update_command();
        }
        if crate::update::is_legacy_panel_update(&self.update.panel_update_command) {
            self.update.panel_update_command = default_panel_update_command();
        }
        if self.update.panel_precheck_command.trim().is_empty() {
            self.update.panel_precheck_command = default_panel_precheck_command();
        }
        if crate::update::is_legacy_panel_precheck(&self.update.panel_precheck_command) {
            self.update.panel_precheck_command = default_panel_precheck_command();
        }
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

fn is_legacy_precheck_command(value: &str) -> bool {
    let normalized = value.trim();
    normalized == LEGACY_PRECHECK_COMMAND_SIMPLE || normalized == LEGACY_PRECHECK_COMMAND_WITH_AUDIT
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
    remove_legacy_app_server_patch_keys(&mut editor);
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

const RETIRED_CONFIG_KEYS: &[&str] = &[
    "logs_db",
    "auto_resume_goals",
    "cookie_secure",
    "session_ttl_seconds",
    "login_rate_limit_per_minute",
    "turnstile_expected_hostname",
    "turnstile_expected_action",
    "public_base_url",
    "trust_forwarded_headers",
    "webui_dir",
    "notify_pi",
    "gotify",
];

fn migrate_all_retired_config(text: &str) -> Result<Option<String>> {
    if !RETIRED_CONFIG_KEYS.iter().any(|key| text.contains(key)) {
        return Ok(None);
    }
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let mut changed = false;
    changed |= migrate_retired_logs_db_doc(&mut doc)?;
    changed |= migrate_retired_probe_goal_doc(&mut doc);
    changed |= migrate_retired_web_doc(&mut doc);
    changed |= migrate_retired_pi_doc(&mut doc);
    changed |= migrate_retired_push_doc(&mut doc);
    Ok(changed.then(|| doc.to_string()))
}

fn migrate_retired_push_doc(doc: &mut toml_edit::DocumentMut) -> bool {
    doc.get_mut("probe")
        .and_then(|item| item.get_mut("notifications"))
        .and_then(|item| item.as_table_like_mut())
        .is_some_and(|table| table.remove("gotify").is_some())
}

/// Remove retired push settings while preserving all other values and comments,
/// including credentials and inline or quoted table definitions.
#[cfg(test)]
fn migrate_retired_push_config(text: &str) -> Result<Option<String>> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    Ok(migrate_retired_push_doc(&mut doc).then(|| doc.to_string()))
}

fn migrate_retired_pi_doc(doc: &mut toml_edit::DocumentMut) -> bool {
    let mut changed = false;
    if let Some(notifications) = doc
        .get_mut("probe")
        .and_then(|item| item.get_mut("notifications"))
        .and_then(|item| item.as_table_like_mut())
    {
        for key in ["notify_pi", "notify_pi_completion", "notify_pi_failure"] {
            changed |= notifications.remove(key).is_some();
        }
    }
    changed
}

#[cfg(test)]
fn migrate_retired_pi_config(text: &str) -> Result<Option<String>> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    Ok(migrate_retired_pi_doc(&mut doc).then(|| doc.to_string()))
}

fn migrate_retired_web_doc(doc: &mut toml_edit::DocumentMut) -> bool {
    let mut changed = false;
    if let Some(security) = doc
        .get_mut("security")
        .and_then(|item| item.as_table_like_mut())
    {
        for key in [
            "cookie_secure",
            "session_ttl_seconds",
            "login_rate_limit_per_minute",
            "turnstile_expected_hostname",
            "turnstile_expected_action",
        ] {
            changed |= security.remove(key).is_some();
        }
    }
    if let Some(server) = doc
        .get_mut("server")
        .and_then(|item| item.as_table_like_mut())
    {
        for key in ["public_base_url", "trust_forwarded_headers"] {
            changed |= server.remove(key).is_some();
        }
    }
    if let Some(paths) = doc
        .get_mut("paths")
        .and_then(|item| item.as_table_like_mut())
    {
        changed |= paths.remove("webui_dir").is_some();
    }
    changed
}

#[cfg(test)]
fn migrate_retired_web_config(text: &str) -> Result<Option<String>> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    Ok(migrate_retired_web_doc(&mut doc).then(|| doc.to_string()))
}

fn migrate_retired_logs_db_doc(document: &mut toml_edit::DocumentMut) -> Result<bool> {
    let Some(probe) = document
        .get_mut("probe")
        .and_then(toml_edit::Item::as_table_like_mut)
    else {
        return Ok(false);
    };
    let Some(old) = probe.remove("logs_db") else {
        return Ok(false);
    };
    let retention = old
        .get("retention_days")
        .and_then(toml_edit::Item::as_integer)
        .unwrap_or(2)
        .clamp(1, 3_650);
    if !probe.contains_key("observability") {
        probe.insert(
            "observability",
            toml_edit::Item::Table(toml_edit::Table::new()),
        );
    }
    let observability = probe
        .get_mut("observability")
        .and_then(toml_edit::Item::as_table_like_mut)
        .context("probe.observability must be a table")?;
    if !observability.contains_key("event_retention_days") {
        observability.insert("event_retention_days", toml_edit::value(retention));
    }
    Ok(true)
}

#[cfg(test)]
fn migrate_retired_logs_db_section(text: &str) -> Result<Option<String>> {
    let mut document = text.parse::<toml_edit::DocumentMut>()?;
    Ok(migrate_retired_logs_db_doc(&mut document)?.then(|| document.to_string()))
}

/// Remove the retired AgentHub Goal recovery switch while preserving the rest
/// of the user's configuration and comments.
fn migrate_retired_probe_goal_doc(document: &mut toml_edit::DocumentMut) -> bool {
    document
        .get_mut("probe")
        .and_then(toml_edit::Item::as_table_like_mut)
        .and_then(|probe| probe.get_mut("error_monitor"))
        .and_then(toml_edit::Item::as_table_like_mut)
        .and_then(|error_monitor| error_monitor.remove("auto_resume_goals"))
        .is_some()
}

fn replace_config_atomically(path: &Path, original: &str, updated: &str) -> Result<()> {
    use std::io::Write;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        anyhow::bail!("configuration migration requires a regular file");
    }
    let candidate = path.with_extension(format!("migration-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&candidate)?;
    let result = (|| -> Result<()> {
        file.set_permissions(metadata.permissions())?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        if fs::read_to_string(path)? != original {
            anyhow::bail!("configuration changed during migration; retry");
        }
        fs::rename(&candidate, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&candidate);
    }
    result
}

fn remove_legacy_app_server_patch_keys(editor: &mut TomlPatchEditor) {
    for key in [
        "app_server_service",
        "app_server_socket",
        "bridge_enabled",
        "bridge_transport",
        "bridge_timeout_seconds",
    ] {
        editor.remove_key("codex", key);
    }
    editor.remove_key("probe.hooks", "reload_app_server_after_install");
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
    fn linux_config_uses_loopback_panel_port() {
        let config = Config::for_platform_kind(PlatformKind::Linux);
        assert_eq!(config.server.listen.to_string(), "127.0.0.1:15742");
        assert_eq!(
            config.update.panel_precheck_command,
            "/usr/local/bin/agenthub-webd-update --precheck"
        );
        assert_eq!(config.codex.home.to_string_lossy(), "auto");
    }

    #[test]
    fn macos_config_uses_tauri_app_precheck() {
        let config = Config::for_platform_kind_with_home(PlatformKind::Macos, "/Users/example");

        assert_eq!(config.server.listen.to_string(), "127.0.0.1:15742");
        assert_eq!(
            config.update.panel_precheck_command,
            "test -d '/Users/example/Library/Application Support/AgentHub'"
        );
        assert!(!config
            .update
            .panel_precheck_command
            .contains("http://127.0.0.1:15742/healthz"));
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
        assert_eq!(
            config.update.panel_precheck_command,
            "test -d '/home/alice/.local/share/AgentHub'"
        );
        assert!(config
            .update
            .doctor_command
            .contains("/home/alice/.local/share/AgentHub/bin/agenthub-webd"));
        assert!(!config.paths.data_dir.starts_with("/var/lib/agenthub-webd"));
        assert!(!config
            .update
            .panel_precheck_command
            .contains("/usr/local/bin/agenthub-webd-update"));
        assert!(!config.codex.workspace.starts_with("/home/ubuntu"));
    }

    #[test]
    fn default_config_uses_codex_update_transient_wrappers() {
        let config = Config::default();

        assert_eq!(
            config.update.precheck_command,
            "/usr/local/bin/agenthub-codex-precheck"
        );
        assert_eq!(
            config.update.update_command,
            "/usr/local/bin/agenthub-codex-update"
        );
        assert_eq!(
            config.update.prune_command,
            "/usr/local/bin/agenthub-codex-prune"
        );
    }

    #[test]
    fn default_config_has_no_app_server_runtime_dependency() {
        let config = Config::default();

        assert!(!config.codex.bridge_enabled);
        assert!(config.codex.app_server_service.trim().is_empty());
        assert!(config.codex.app_server_socket.is_none());
        assert!(!config.probe.hooks.reload_app_server_after_install);
    }

    #[test]
    fn default_config_includes_builtin_probe_settings() {
        let config = Config::default();

        assert!(config.probe.enabled);
        assert_eq!(config.probe.poll_seconds, 15);
        assert_eq!(config.probe.recent_limit, 50);
        assert!(config.probe.hooks.manage_stop_hook);
        assert!(!config.probe.hooks.reload_app_server_after_install);
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
app_server_service = "codex-app-server-root.service"
app_server_socket = "/root/.codex/app-server.sock"
bridge_enabled = true
bridge_transport = "socket"
bridge_timeout_seconds = 60
host_label = "old-host"

[probe]
enabled = true
poll_seconds = 15

[probe.hooks]
manage_stop_hook = true
reload_app_server_after_install = true

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
        assert!(!output.contains("app_server_service"));
        assert!(!output.contains("app_server_socket"));
        assert!(!output.contains("bridge_enabled"));
        assert!(!output.contains("bridge_transport"));
        assert!(!output.contains("bridge_timeout_seconds"));
        assert!(!output.contains("reload_app_server_after_install"));
        assert!(output.contains("[probe]\nenabled = false\npoll_seconds = 30"));
        assert!(output.contains("recent_limit = 80"));
        assert!(output.contains("[probe.notifications]"));
        assert!(output.contains("enabled = true"));
        assert!(output.contains("group = \"AgentHub\""));
        assert!(!output.contains("logs_db"));
    }

    #[test]
    fn retired_maintenance_migration_preserves_credentials_and_retention() {
        let input = r#"# retained comment
[server]
listen = "127.0.0.1:15742"
public_base_url = "https://panel.example.com/agenthub/"
[security]
turnstile_expected_hostname = "panel.example.com"
[probe.notifications]
enabled = true
group = "saved-group"
[probe.logs_db]
retention_days = 7
compact_enabled = true
[custom]
token = "fixture-not-a-real-secret"
"#;
        let updated = super::migrate_retired_logs_db_section(input)
            .unwrap()
            .unwrap();
        let old: toml::Value = toml::from_str(input).unwrap();
        let new: toml::Value = toml::from_str(&updated).unwrap();
        for key in ["server", "security", "custom"] {
            assert_eq!(old[key], new[key]);
        }
        assert_eq!(old["probe"]["notifications"], new["probe"]["notifications"]);
        assert_eq!(
            new["probe"]["observability"]["event_retention_days"].as_integer(),
            Some(7)
        );
        assert!(new["probe"].get("logs_db").is_none());
        assert!(updated.contains("# retained comment"));
        assert!(super::migrate_retired_logs_db_section(&updated)
            .unwrap()
            .is_none());
        let inline = r#"probe = { logs_db = { retention_days = 7 }, observability = { event_retention_days = 3 } }"#;
        let updated = super::migrate_retired_logs_db_section(inline)
            .unwrap()
            .unwrap();
        let parsed: toml::Value = toml::from_str(&updated).unwrap();
        assert_eq!(
            parsed["probe"]["observability"]["event_retention_days"].as_integer(),
            Some(3)
        );
        assert!(parsed["probe"].get("logs_db").is_none());
    }

    #[test]
    fn loading_old_config_migrates_once_and_preserves_file_mode() {
        let directory =
            std::env::temp_dir().join(format!("config-upgrade-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("config.toml");
        let text = format!(
            "{}\n[probe.logs_db]\nretention_days = 7\n[probe.notifications.gotify]\nenabled = true\n",
            toml::to_string_pretty(&Config::default()).unwrap()
        );
        // An explicit existing observability preference takes precedence.
        std::fs::write(&path, text).unwrap();
        #[cfg(unix)]
        let initial_ino = {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::metadata(&path).unwrap().ino()
        };
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.probe.observability.event_retention_days, 2);
        assert_eq!(loaded.probe.notifications.group, "AgentHub");
        let migrated = std::fs::read(&path).unwrap();
        let migrated_text = String::from_utf8_lossy(&migrated);
        assert!(!migrated_text.contains("[probe.logs_db]"));
        assert!(!migrated_text.contains("gotify"));
        assert!(migrated_text.contains("group = \"AgentHub\""));
        assert!(super::migrate_retired_push_config(&migrated_text)
            .unwrap()
            .is_none());
        let migrated_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        #[cfg(unix)]
        let migrated_ino = {
            use std::os::unix::fs::MetadataExt;
            let ino = std::fs::metadata(&path).unwrap().ino();
            assert_ne!(
                ino, initial_ino,
                "first load must atomically replace legacy config"
            );
            ino
        };
        Config::load(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), migrated);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            migrated_mtime,
            "second load of migrated config must not rewrite file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let meta = std::fs::metadata(&path).unwrap();
            assert_eq!(
                meta.ino(),
                migrated_ino,
                "second load must not replace config file inode"
            );
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
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

[update]
precheck_command = "/usr/local/bin/agenthub-codex-precheck"
update_command = "/usr/local/bin/agenthub-codex-update"
prune_command = "/usr/local/bin/agenthub-codex-prune"
doctor_command = "/home/ubuntu/codex-admin/bin/codex-cloud-doctor"
panel_update_command = "/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest"
panel_precheck_command = "/usr/local/bin/agenthub-webd-update --precheck"
"#;
        let config: Config = toml::from_str(input).unwrap();

        assert_eq!(config.codex.home.to_string_lossy(), "auto");
        assert!(config.codex.app_server_service.is_empty());
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
    fn probe_config_patch_cannot_reintroduce_legacy_app_server_bridge_keys() {
        let input = r#"
[codex]
home = "/root/.codex"
workspace = "/home/ubuntu/codex-workspace"
host_label = "old"
"#;
        let patch: ProbeConfigFilePatch = serde_json::from_str(
            r#"{
                "codex": {
                    "home": "/srv/codex",
                    "workspace": "/srv/workspace",
                    "host_label": "cloud",
                    "app_server_service": "codex-app-server-root.service",
                    "app_server_socket": "/root/.codex/app-server.sock",
                    "bridge_enabled": true,
                    "bridge_transport": "socket",
                    "bridge_timeout_seconds": 99
                }
            }"#,
        )
        .unwrap();

        let output = super::patch_probe_config_toml(input, &patch).unwrap();

        assert!(output.contains("home = \"/srv/codex\""));
        assert!(output.contains("workspace = \"/srv/workspace\""));
        assert!(output.contains("host_label = \"cloud\""));
        assert!(!output.contains("app_server_service"));
        assert!(!output.contains("app_server_socket"));
        assert!(!output.contains("bridge_enabled"));
        assert!(!output.contains("bridge_transport"));
        assert!(!output.contains("bridge_timeout_seconds"));
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
        assert!(config.security.auth_rate_limit_per_minute > 0);
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

    #[test]
    fn web_config_upgrade_removes_retired_fields_and_preserves_secret_and_business_values() {
        for input in [
            r#"# preserved configuration
[server]
listen = "127.0.0.1:15742"
public_base_url = "https://panel.example.com/agenthub/"
trust_forwarded_headers = true
[security]
secret_key = "fixture-secret-key"
cookie_secure = true
session_ttl_seconds = 31536000
login_rate_limit_per_minute = 10
turnstile_expected_hostname = "panel.example.com"
turnstile_expected_action = "login"
[paths]
data_dir = "/tmp/agenthub-fixture"
db_path = "/tmp/agenthub-fixture/agenthub.sqlite"
webui_dir = "/tmp/agenthub-fixture/webui"
[probe.notifications]
enabled = true
group = "fixture-group"
[custom]
token = "fixture-custom-value"
"#,
            r#"# preserved configuration
server = { listen = "127.0.0.1:15742", public_base_url = "https://panel.example.com/agenthub/", trust_forwarded_headers = true }
security = { secret_key = "fixture-secret-key", cookie_secure = true, session_ttl_seconds = 31536000, login_rate_limit_per_minute = 10, turnstile_expected_hostname = "panel.example.com", turnstile_expected_action = "login" }
paths = { data_dir = "/tmp/agenthub-fixture", db_path = "/tmp/agenthub-fixture/agenthub.sqlite", webui_dir = "/tmp/agenthub-fixture/webui" }
probe = { notifications = { enabled = true, group = "fixture-group" } }
custom = { token = "fixture-custom-value" }
"#,
        ] {
            let upgraded = super::migrate_retired_web_config(input).unwrap().unwrap();
            let previous: toml::Value = toml::from_str(input).unwrap();
            let current: toml::Value = toml::from_str(&upgraded).unwrap();
            assert_eq!(current["server"]["listen"], previous["server"]["listen"]);
            assert_eq!(
                current["security"]["secret_key"],
                previous["security"]["secret_key"]
            );
            assert_eq!(current["paths"]["db_path"], previous["paths"]["db_path"]);
            assert_eq!(current["paths"]["data_dir"], previous["paths"]["data_dir"]);
            assert_eq!(current["probe"], previous["probe"]);
            assert_eq!(current["custom"], previous["custom"]);
            for retired in [
                "public_base_url",
                "trust_forwarded_headers",
                "cookie_secure",
                "session_ttl_seconds",
                "login_rate_limit_per_minute",
                "turnstile_expected_hostname",
                "turnstile_expected_action",
                "webui_dir",
            ] {
                assert!(!upgraded.contains(retired), "retired field {retired}");
            }
            assert!(upgraded.contains("# preserved configuration"));
            assert!(super::migrate_retired_web_config(&upgraded)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn normalize_migrates_legacy_runtime_values_and_preserves_host_label() {
        let mut config = Config::default();
        config.codex.host_label = "configured-host".into();
        config.server.listen = "127.0.0.1:15732".parse().unwrap();
        config.update.panel_precheck_command =
            "test -x /usr/local/bin/agenthub-update && systemctl is-active agenthub && curl -fsS http://127.0.0.1:15732/healthz".to_string();

        config.normalize().unwrap();

        assert_eq!(config.codex.host_label, "configured-host");
        assert_eq!(config.server.listen.to_string(), "127.0.0.1:15742");
        match super::current_platform_kind() {
            PlatformKind::Linux | PlatformKind::Windows => {
                assert!(config.update.panel_precheck_command.contains("--precheck"))
            }
            PlatformKind::Macos => {
                assert!(config.update.panel_precheck_command.starts_with("test -d "));
                assert!(!config
                    .update
                    .panel_precheck_command
                    .contains("http://127.0.0.1:15742/healthz"));
            }
        }
    }

    #[test]
    fn normalize_migrates_legacy_codex_precheck_to_transient_wrapper() {
        for legacy in [
            super::LEGACY_PRECHECK_COMMAND_SIMPLE,
            super::LEGACY_PRECHECK_COMMAND_WITH_AUDIT,
        ] {
            let mut config = Config::default();
            config.update.precheck_command = legacy.to_string();

            config.normalize().unwrap();

            assert_eq!(
                config.update.precheck_command,
                "/usr/local/bin/agenthub-codex-precheck"
            );
        }
    }

    #[test]
    fn normalize_keeps_custom_codex_precheck_command() {
        let mut config = Config::default();
        config.update.precheck_command = "/opt/custom/precheck --strict".to_string();

        config.normalize().unwrap();

        assert_eq!(
            config.update.precheck_command,
            "/opt/custom/precheck --strict"
        );
    }
}

#[cfg(test)]
mod retired_provider_tests {
    #[test]
    fn pi_configuration_upgrade_is_precise_and_idempotent() {
        let before = r#"[probe.notifications]
enabled = true
notify_pi = true
notify_pi_completion = false
notify_pi_failure = true
notify_claude = true
server_url = "https://example.invalid"
[security]
secret_key = "fixture-only-key"
"#;
        let after = super::migrate_retired_pi_config(before).unwrap().unwrap();
        let doc: toml::Value = toml::from_str(&after).unwrap();
        for key in ["notify_pi", "notify_pi_completion", "notify_pi_failure"] {
            assert!(doc["probe"]["notifications"].get(key).is_none());
        }
        assert_eq!(
            doc["probe"]["notifications"]["notify_claude"].as_bool(),
            Some(true)
        );
        assert_eq!(
            doc["security"]["secret_key"].as_str(),
            Some("fixture-only-key")
        );
        assert!(super::migrate_retired_pi_config(&after).unwrap().is_none());
    }

    #[test]
    fn every_migrated_retired_key_matches_fast_path_filter_and_triggers_migration() {
        for (key, snippet) in [
            ("logs_db", "[probe.logs_db]\nretention_days = 7\n"),
            (
                "auto_resume_goals",
                "[probe.error_monitor]\nauto_resume_goals = true\n",
            ),
            ("cookie_secure", "[security]\ncookie_secure = true\n"),
            (
                "session_ttl_seconds",
                "[security]\nsession_ttl_seconds = 60\n",
            ),
            (
                "login_rate_limit_per_minute",
                "[security]\nlogin_rate_limit_per_minute = 5\n",
            ),
            (
                "turnstile_expected_hostname",
                "[security]\nturnstile_expected_hostname = \"example.invalid\"\n",
            ),
            (
                "turnstile_expected_action",
                "[security]\nturnstile_expected_action = \"login\"\n",
            ),
            (
                "public_base_url",
                "[server]\npublic_base_url = \"https://example.invalid\"\n",
            ),
            (
                "trust_forwarded_headers",
                "[server]\ntrust_forwarded_headers = true\n",
            ),
            ("webui_dir", "[paths]\nwebui_dir = \"/tmp/webui\"\n"),
            ("notify_pi", "[probe.notifications]\nnotify_pi = true\n"),
            (
                "notify_pi_completion",
                "[probe.notifications]\nnotify_pi_completion = true\n",
            ),
            (
                "notify_pi_failure",
                "[probe.notifications]\nnotify_pi_failure = true\n",
            ),
            ("gotify", "[probe.notifications.gotify]\nenabled = true\n"),
        ] {
            assert!(
                super::RETIRED_CONFIG_KEYS
                    .iter()
                    .any(|pattern| key.contains(pattern)),
                "retired key {key} is not covered by RETIRED_CONFIG_KEYS"
            );
            let migrated = super::migrate_all_retired_config(snippet)
                .unwrap()
                .unwrap_or_else(|| {
                    panic!("retired key {key} was skipped by migrate_all_retired_config")
                });
            assert!(
                !migrated.contains(key),
                "migrated config still contains {key}: {migrated}"
            );
        }
    }
}
