use agenthub_core::services::remote::SshHost;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
pub fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && !alias.starts_with(['-', '!'])
        && !alias
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '*' | '?'))
}

fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut token = String::new();
    let mut quote = false;
    let mut escape = false;
    for c in line.chars() {
        if escape {
            token.push(c);
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if c == '"' {
            quote = !quote;
            continue;
        }
        if c == '#' && !quote {
            break;
        }
        if !quote
            && (c.is_whitespace()
                || (c == '=' && (out.is_empty() || (out.len() == 1 && token.is_empty()))))
        {
            if !token.is_empty() {
                out.push(std::mem::take(&mut token));
            }
        } else {
            token.push(c);
        }
    }
    if !token.is_empty() {
        out.push(token);
    }
    out
}
fn expand(value: &str, base: &Path, home: &Path) -> PathBuf {
    let mut value = value.replace("%d", &home.to_string_lossy());
    while let Some(start) = value.find("${") {
        let Some(end) = value[start + 2..].find('}') else {
            break;
        };
        let end = start + 2 + end;
        let name = &value[start + 2..end];
        let replacement = std::env::var(name).unwrap_or_default();
        value.replace_range(start..=end, &replacement);
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home.join(rest);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}
fn scan(
    path: &Path,
    base: &Path,
    home: &Path,
    seen: &mut BTreeSet<PathBuf>,
    hosts: &mut BTreeSet<String>,
) -> Result<(), String> {
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("无法读取 SSH 配置".into()),
    };
    if !seen.insert(path.clone()) {
        return Ok(());
    }
    let text = std::fs::read_to_string(path).map_err(|_| "无法读取 SSH 配置")?;
    for line in text.lines() {
        let parts = tokens(line);
        let Some(key) = parts.first() else {
            continue;
        };
        if key.eq_ignore_ascii_case("host") {
            for alias in &parts[1..] {
                if valid_alias(alias) {
                    hosts.insert(alias.clone());
                }
            }
        }
        if key.eq_ignore_ascii_case("include") {
            for item in &parts[1..] {
                let pattern = expand(item, base, home);
                let entries =
                    glob::glob(&pattern.to_string_lossy()).map_err(|_| "SSH Include 格式无效")?;
                for entry in entries {
                    scan(
                        &entry.map_err(|_| "无法读取 SSH Include")?,
                        base,
                        home,
                        seen,
                        hosts,
                    )?;
                }
            }
        }
    }
    Ok(())
}
pub fn list() -> Result<Vec<SshHost>, String> {
    let home = dirs::home_dir().ok_or("无法定位 SSH 配置")?;
    list_at(&home, &PathBuf::from("/etc/ssh"))
}
fn list_at(home: &Path, system: &Path) -> Result<Vec<SshHost>, String> {
    let mut seen = BTreeSet::new();
    let mut hosts = BTreeSet::new();
    scan(
        &home.join(".ssh/config"),
        &home.join(".ssh"),
        home,
        &mut seen,
        &mut hosts,
    )?;
    scan(
        &system.join("ssh_config"),
        system,
        home,
        &mut seen,
        &mut hosts,
    )?;
    Ok(hosts.into_iter().map(|alias| SshHost { alias }).collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn included_aliases_are_sorted_unique_and_not_patterns() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".ssh")).unwrap();
        std::fs::write(
            root.path().join(".ssh/config"),
            "Include \"hosts-*\"\nHost main other 工作机 !blocked *.invalid\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join(".ssh/hosts-a"),
            "Host = jump main\nInclude config\nProxyJump proxy\n",
        )
        .unwrap();
        assert_eq!(
            list_at(root.path(), &root.path().join("system"))
                .unwrap()
                .into_iter()
                .map(|h| h.alias)
                .collect::<Vec<_>>(),
            vec!["jump", "main", "other", "工作机"]
        );
        assert!(!valid_alias("-oProxyCommand=bad"));
    }
}
