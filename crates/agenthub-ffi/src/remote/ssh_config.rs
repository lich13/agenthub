use agenthub_core::services::remote::SshHost;
use std::{
    collections::BTreeSet,
    io::Write,
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
    token_spans(line)
        .into_iter()
        .map(|(value, _)| value)
        .collect()
}

fn token_spans(line: &str) -> Vec<(String, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut token = String::new();
    let mut start = None;
    let mut quote = false;
    let mut escape = false;
    for (offset, c) in line.char_indices() {
        if escape {
            token.push(c);
            escape = false;
            continue;
        }
        if c == '\\' {
            start.get_or_insert(offset);
            escape = true;
            continue;
        }
        if c == '"' {
            start.get_or_insert(offset);
            quote = !quote;
            continue;
        }
        if c == '#' && !quote {
            if let Some(start) = start.take() {
                out.push((std::mem::take(&mut token), start..offset));
            }
            break;
        }
        if !quote
            && (c.is_whitespace()
                || (c == '=' && (out.is_empty() || (out.len() == 1 && token.is_empty()))))
        {
            if let Some(start) = start.take() {
                out.push((std::mem::take(&mut token), start..offset));
            }
        } else {
            start.get_or_insert(offset);
            token.push(c);
        }
    }
    if let Some(start) = start {
        out.push((token, start..line.len()));
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
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("无法读取 SSH 配置".into()),
    };
    if !seen.insert(path.clone()) {
        return Ok(());
    }
    files.push(path.clone());
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
                        files,
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
        &mut Vec::new(),
    )?;
    scan(
        &system.join("ssh_config"),
        system,
        home,
        &mut seen,
        &mut hosts,
        &mut Vec::new(),
    )?;
    Ok(hosts.into_iter().map(|alias| SshHost { alias }).collect())
}

struct Edit {
    path: PathBuf,
    original: String,
    updated: String,
    permissions: std::fs::Permissions,
}

fn without_alias(text: &str, alias: &str) -> String {
    let mut output = String::new();
    let mut removing = false;
    for line in text.split_inclusive('\n') {
        let parts = token_spans(line);
        let key = parts.first().map(|(value, _)| value.as_str()).unwrap_or("");
        if key.eq_ignore_ascii_case("host") || key.eq_ignore_ascii_case("match") {
            removing = false;
        }
        if key.eq_ignore_ascii_case("host") && parts[1..].iter().any(|(value, _)| value == alias) {
            let remaining = parts[1..].iter().any(|(value, _)| value != alias);
            if remaining {
                let mut header = line.to_string();
                for (_, span) in parts[1..].iter().rev().filter(|(value, _)| value == alias) {
                    let mut start = span.start;
                    while start > 0 && matches!(header.as_bytes()[start - 1], b' ' | b'\t') {
                        start -= 1;
                    }
                    header.replace_range(start..span.end, "");
                }
                output.push_str(&header);
            } else {
                removing = true;
            }
        } else if !removing || key.is_empty() {
            output.push_str(line);
        }
    }
    output
}

fn replace(edit: &Edit, text: &str) -> Result<(), String> {
    let mut file = tempfile::Builder::new()
        .prefix(&format!(
            "{}.",
            edit.path.file_name().unwrap_or_default().to_string_lossy()
        ))
        .tempfile_in(edit.path.parent().ok_or("无法写入 SSH 配置")?)
        .map_err(|_| "无法写入 SSH 配置")?;
    (|| -> anyhow::Result<()> {
        file.as_file().set_permissions(edit.permissions.clone())?;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        anyhow::ensure!(
            std::fs::read_to_string(&edit.path)? == edit.original,
            "SSH 配置已被修改，请刷新后重试"
        );
        file.persist(&edit.path)?;
        Ok(())
    })()
    .map_err(|error| format!("无法保存 SSH 配置：{error}"))
}

pub fn delete_with<T>(
    alias: &str,
    commit: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let home = dirs::home_dir().ok_or("无法定位 SSH 配置")?;
    delete_at(&home, Path::new("/etc/ssh"), alias, commit)
}

fn delete_at<T>(
    home: &Path,
    system: &Path,
    alias: &str,
    commit: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if !valid_alias(alias) {
        return Err("请选择有效的 SSH Host".into());
    }
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    let mut system_hosts = BTreeSet::new();
    scan(
        &system.join("ssh_config"),
        system,
        home,
        &mut seen,
        &mut system_hosts,
        &mut Vec::new(),
    )?;
    if system_hosts.contains(alias) {
        return Err("系统 SSH 配置不能在这里删除".into());
    }
    scan(
        &home.join(".ssh/config"),
        &home.join(".ssh"),
        home,
        &mut seen,
        &mut BTreeSet::new(),
        &mut files,
    )?;
    let mut edits = Vec::new();
    for path in files {
        let original = std::fs::read_to_string(&path).map_err(|_| "无法读取 SSH 配置")?;
        let updated = without_alias(&original, alias);
        if original == updated {
            continue;
        }
        let metadata = std::fs::metadata(&path).map_err(|_| "无法读取 SSH 配置")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid()
                != std::fs::metadata(home)
                    .map_err(|_| "无法读取 SSH 配置")?
                    .uid()
            {
                return Err("只能删除当前用户拥有的 SSH 配置".into());
            }
        }
        edits.push(Edit {
            path,
            original,
            updated,
            permissions: metadata.permissions(),
        });
    }
    if edits.is_empty() {
        return Err("所选 SSH 配置已不存在，请刷新".into());
    }
    let mut applied = 0;
    let result = (|| {
        for edit in &edits {
            replace(edit, &edit.updated)?;
            applied += 1;
        }
        commit()
    })();
    if let Err(error) = &result {
        return Err(if applied > 0 {
            format!("已更新 {applied} 个 SSH 配置文件；{error}。请刷新后检查剩余配置")
        } else {
            error.clone()
        });
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deletion_handles_same_stem_and_preserves_concurrent_edits() {
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        std::fs::write(ssh.join("config"), "Include node.conf node.extra\n").unwrap();
        for name in ["node.conf", "node.extra"] {
            std::fs::write(ssh.join(name), "Host fixture # removed host\n  HostName example.invalid # removed directive\n# independent\nHost keep\n  Port 22\n").unwrap();
        }
        std::fs::write(ssh.join("node.agenthub.pending"), "existing unrelated file").unwrap();
        delete_at(root.path(), &root.path().join("system"), "fixture", || {
            Ok(())
        })
        .unwrap();
        for name in ["node.conf", "node.extra"] {
            assert_eq!(
                std::fs::read_to_string(ssh.join(name)).unwrap(),
                "# independent\nHost keep\n  Port 22\n"
            );
        }
        let path = ssh.join("node.conf");
        let edit = Edit {
            original: std::fs::read_to_string(&path).unwrap(),
            updated: String::new(),
            permissions: std::fs::metadata(&path).unwrap().permissions(),
            path: path.clone(),
        };
        std::fs::write(&path, "Host changed\n").unwrap();
        assert!(replace(&edit, "").unwrap_err().contains("已被修改"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "Host changed\n");
        assert_eq!(std::fs::read_dir(ssh).unwrap().count(), 4);
    }
    #[test]
    fn delete_removes_all_explicit_aliases_preserving_other_hosts_and_comments() {
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        let config = ssh.join("config");
        let included = ssh.join("hosts");
        std::fs::write(&config, "# global\nInclude hosts\nHost fixture # selected\n  HostName example.invalid # address\n  IdentityFile fixture-key\n\nHost keep\n  User fixture-user\n").unwrap();
        std::fs::write(&included, "# shared\nHost fixture other fixture # aliases\n  HostName example.invalid\nHost fixture\n  Port 2222\nMatch all\n  ServerAliveInterval 15\n").unwrap();
        let key = ssh.join("fixture-key");
        std::fs::write(&key, "fixture-key-content").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        delete_at(root.path(), &root.path().join("system"), "fixture", || {
            Ok(())
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "# global\nInclude hosts\n\nHost keep\n  User fixture-user\n"
        );
        assert_eq!(std::fs::read_to_string(&included).unwrap(), "# shared\nHost other # aliases\n  HostName example.invalid\nMatch all\n  ServerAliveInterval 15\n");
        assert_eq!(std::fs::read_to_string(key).unwrap(), "fixture-key-content");
        assert_eq!(
            list_at(root.path(), &root.path().join("system"))
                .unwrap()
                .into_iter()
                .map(|h| h.alias)
                .collect::<Vec<_>>(),
            ["keep", "other"]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(config).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }

    #[test]
    fn stale_temporary_files_do_not_block_and_partial_failure_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        let config = ssh.join("config");
        let included = ssh.join("hosts.conf");
        std::fs::write(
            &config,
            "Include hosts.conf\nHost fixture\n  HostName example.invalid\n",
        )
        .unwrap();
        std::fs::write(&included, "Host fixture other\n  User fixture-user\n").unwrap();
        let pending = included.with_extension(format!("agenthub-{}.pending", std::process::id()));
        std::fs::write(&pending, "existing-file").unwrap();
        let error = delete_at(root.path(), &root.path().join("system"), "fixture", || {
            Err::<(), _>("preference write failed".into())
        })
        .unwrap_err();
        assert!(error.contains("已更新 2 个"));
        assert_eq!(
            std::fs::read_to_string(config).unwrap(),
            "Include hosts.conf\n"
        );
        assert_eq!(
            std::fs::read_to_string(included).unwrap(),
            "Host other\n  User fixture-user\n"
        );
        assert_eq!(std::fs::read_to_string(pending).unwrap(), "existing-file");
    }

    #[test]
    fn system_hosts_cannot_be_deleted_and_client_paths_are_not_accepted() {
        let root = tempfile::tempdir().unwrap();
        let system = root.path().join("system");
        std::fs::create_dir(&system).unwrap();
        let config = system.join("ssh_config");
        let original = "Host fixture\n  HostName example.invalid\n";
        std::fs::write(&config, original).unwrap();
        assert!(delete_at(root.path(), &system, "fixture", || Ok(()))
            .unwrap_err()
            .contains("系统"));
        assert_eq!(std::fs::read_to_string(config).unwrap(), original);
        assert!(
            serde_json::from_value::<agenthub_core::services::remote::SshDeleteRequest>(
                serde_json::json!({"revision": 0, "alias": "fixture", "path": "fixture-config"})
            )
            .is_err()
        );
    }
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
