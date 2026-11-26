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

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ParsedVersion {
    core: [u64; 3],
    pre: Vec<VersionIdentifier>,
}

impl ParsedVersion {
    pub(crate) const fn stable(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            core: [major, minor, patch],
            pre: Vec::new(),
        }
    }

    pub(crate) fn extract_and_parse(value: &str) -> Option<Self> {
        extract_semver(value).and_then(|semver| Self::parse(&semver))
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
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

#[derive(Debug, Clone, Eq, PartialEq)]
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

#[cfg(test)]
mod tests {
    use super::{compare_semver, extract_semver};

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
}
