use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub active: bool,
    pub active_state: Option<String>,
    pub sub_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub panel_current: String,
    pub panel_latest: Option<String>,
    pub panel_update_available: Option<bool>,
    pub codex_current: Option<String>,
    pub codex_latest: Option<String>,
    pub codex_update_available: Option<bool>,
    pub codex_user: Option<String>,
    pub codex_root: Option<String>,
    pub codex_raw: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct VersionInfoInputs {
    pub panel_latest: Option<String>,
    pub codex_latest: Option<String>,
}

pub async fn version_info() -> Result<VersionInfo> {
    version_info_with_inputs(VersionInfoInputs::default()).await
}

pub async fn version_info_with_inputs(inputs: VersionInfoInputs) -> Result<VersionInfo> {
    let latest = inputs.panel_latest.and_then(non_empty_string);
    let current = env!("CARGO_PKG_VERSION").to_string();
    let update_available = latest
        .as_ref()
        .map(|latest| latest.trim_start_matches('v') != current);
    let codex_raw = command_stdout_timeout(
        "/usr/local/bin/codex-raw",
        &["--version"],
        Duration::from_secs(3),
    )
    .await
    .ok();
    let codex_root = command_stdout_timeout(
        "sudo",
        &["-n", "codex", "--version"],
        Duration::from_secs(3),
    )
    .await
    .ok();
    let codex_user = command_stdout_timeout("codex", &["--version"], Duration::from_secs(3))
        .await
        .ok();
    let codex_current = current_codex_version(
        codex_raw.as_deref(),
        codex_root.as_deref(),
        codex_user.as_deref(),
    );
    let codex_latest = inputs.codex_latest.and_then(non_empty_string);
    let codex_update_available =
        codex_update_available(codex_current.as_deref(), codex_latest.as_deref());
    Ok(VersionInfo {
        panel_current: current,
        panel_latest: latest,
        panel_update_available: update_available,
        codex_current,
        codex_latest,
        codex_update_available,
        codex_user,
        codex_root,
        codex_raw,
    })
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn current_codex_version(
    raw: Option<&str>,
    root: Option<&str>,
    user: Option<&str>,
) -> Option<String> {
    [raw, root, user]
        .into_iter()
        .flatten()
        .find_map(extract_semver)
}

fn codex_update_available(current: Option<&str>, latest: Option<&str>) -> Option<bool> {
    Some(
        compare_semver(
            extract_semver(latest?)?.as_str(),
            extract_semver(current?)?.as_str(),
        )?
        .is_gt(),
    )
}

pub(crate) fn extract_semver(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    for start in 0..bytes.len() {
        let first = bytes[start] as char;
        if !first.is_ascii_digit() && first != 'v' {
            continue;
        }
        let mut index = if first == 'v' { start + 1 } else { start };
        if index >= bytes.len() || !(bytes[index] as char).is_ascii_digit() {
            continue;
        }
        let mut parts = Vec::new();
        for _ in 0..3 {
            let part_start = index;
            while index < bytes.len() && (bytes[index] as char).is_ascii_digit() {
                index += 1;
            }
            if part_start == index {
                parts.clear();
                break;
            }
            parts.push(&value[part_start..index]);
            if parts.len() < 3 {
                if index >= bytes.len() || bytes[index] != b'.' {
                    parts.clear();
                    break;
                }
                index += 1;
            }
        }
        if parts.len() != 3 {
            continue;
        }
        let mut end = index;
        if end < bytes.len() && bytes[end] == b'-' {
            end += 1;
            let pre_start = end;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                    end += 1;
                } else {
                    break;
                }
            }
            if pre_start == end {
                end = index;
            }
        }
        return Some(value[start..end].trim_start_matches('v').to_string());
    }
    None
}

pub(crate) fn compare_semver(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let left = ParsedVersion::parse(left)?;
    let right = ParsedVersion::parse(right)?;
    Some(left.cmp(&right))
}

#[derive(Debug, Eq, PartialEq)]
struct ParsedVersion {
    core: [u64; 3],
    pre: Vec<VersionIdentifier>,
}

impl ParsedVersion {
    fn parse(value: &str) -> Option<Self> {
        let (core_text, pre_text) = value
            .trim()
            .trim_start_matches('v')
            .split_once('-')
            .unwrap_or((value.trim().trim_start_matches('v'), ""));
        let mut core = [0_u64; 3];
        let parts = core_text.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return None;
        }
        for (index, part) in parts.iter().enumerate() {
            core[index] = part.parse().ok()?;
        }
        let pre = if pre_text.is_empty() {
            Vec::new()
        } else {
            pre_text
                .split('.')
                .map(VersionIdentifier::parse)
                .collect::<Option<Vec<_>>>()?
        };
        Some(Self { core, pre })
    }
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.core.cmp(&other.core) {
            std::cmp::Ordering::Equal => compare_pre(&self.pre, &other.pre),
            ordering => ordering,
        }
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Eq, PartialEq)]
enum VersionIdentifier {
    Numeric(u64),
    Text(String),
}

impl VersionIdentifier {
    fn parse(value: &str) -> Option<Self> {
        if value.is_empty() {
            return None;
        }
        if value.chars().all(|ch| ch.is_ascii_digit()) {
            Some(Self::Numeric(value.parse().ok()?))
        } else if value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            Some(Self::Text(value.to_string()))
        } else {
            None
        }
    }
}

fn compare_pre(left: &[VersionIdentifier], right: &[VersionIdentifier]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if left.is_empty() && right.is_empty() {
        return Ordering::Equal;
    }
    if left.is_empty() {
        return Ordering::Greater;
    }
    if right.is_empty() {
        return Ordering::Less;
    }
    for (left, right) in left.iter().zip(right) {
        let ordering = match (left, right) {
            (VersionIdentifier::Numeric(left), VersionIdentifier::Numeric(right)) => {
                left.cmp(right)
            }
            (VersionIdentifier::Numeric(_), VersionIdentifier::Text(_)) => Ordering::Less,
            (VersionIdentifier::Text(_), VersionIdentifier::Numeric(_)) => Ordering::Greater,
            (VersionIdentifier::Text(left), VersionIdentifier::Text(right)) => left.cmp(right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

async fn command_stdout(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output().await?;
    if !output.status.success() {
        anyhow::bail!("{program} exited with {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn command_stdout_timeout(
    program: &str,
    args: &[&str],
    duration: Duration,
) -> Result<String> {
    tokio::time::timeout(duration, command_stdout(program, args))
        .await
        .with_context(|| format!("{program} timed out"))?
}

#[cfg(test)]
mod tests {
    use super::{codex_update_available, compare_semver, current_codex_version, extract_semver};

    #[test]
    fn extracts_codex_cli_semver() {
        assert_eq!(
            extract_semver("codex-cli 0.137.0").as_deref(),
            Some("0.137.0")
        );
        assert_eq!(
            extract_semver("v0.137.0-beta.1").as_deref(),
            Some("0.137.0-beta.1")
        );
        assert_eq!(extract_semver("unknown"), None);
    }

    #[test]
    fn current_codex_version_prefers_raw_root_user_order() {
        assert_eq!(
            current_codex_version(
                Some("codex-cli 0.137.0"),
                Some("codex-cli 0.136.0"),
                Some("codex-cli 0.135.0")
            )
            .as_deref(),
            Some("0.137.0")
        );
        assert_eq!(
            current_codex_version(None, Some("codex-cli 0.136.0"), None).as_deref(),
            Some("0.136.0")
        );
    }

    #[test]
    fn compares_semver_with_prerelease_rules() {
        assert!(compare_semver("0.138.0", "0.137.0").is_some_and(|ordering| ordering.is_gt()));
        assert!(compare_semver("0.137.0", "0.137.0").is_some_and(|ordering| ordering.is_eq()));
        assert!(compare_semver("0.137.0-beta.2", "0.137.0-beta.11")
            .is_some_and(|ordering| ordering.is_lt()));
        assert!(
            compare_semver("0.137.0-beta.1", "0.137.0").is_some_and(|ordering| ordering.is_lt())
        );
        assert_eq!(compare_semver("unknown", "0.137.0"), None);
    }

    #[test]
    fn codex_update_available_is_three_state() {
        assert_eq!(
            codex_update_available(Some("0.137.0"), Some("0.138.0")),
            Some(true)
        );
        assert_eq!(
            codex_update_available(Some("0.137.0"), Some("0.137.0")),
            Some(false)
        );
        assert_eq!(
            codex_update_available(Some("0.138.0"), Some("0.137.0")),
            Some(false)
        );
        assert_eq!(
            codex_update_available(Some("unknown"), Some("0.137.0")),
            None
        );
        assert_eq!(codex_update_available(Some("0.137.0"), None), None);
    }
}
