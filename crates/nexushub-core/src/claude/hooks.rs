//! Explicit local settings management. Never runs as part of App installation.
use super::*;
use serde_json::json;
use std::io::Write;
use std::sync::Mutex;

static SETTINGS: Mutex<()> = Mutex::new(());
pub const HANDLER: &str = "claude-hook";
pub const GROK_HANDLER: &str = "grok-hook";
pub const ANTIGRAVITY_HANDLER: &str = "antigravity-hook";
const EVENTS: [(&str, &str); 3] = [
    ("Stop", ""),
    (
        "Notification",
        "permission_prompt|elicitation_dialog|elicitation_url_dialog|agent_needs_input|idle_prompt",
    ),
    ("StopFailure", ""),
];

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum HookProvider {
    #[default]
    Claude,
    Grok,
    Antigravity,
}

impl HookProvider {
    pub fn handler(self) -> &'static str {
        match self {
            Self::Claude => HANDLER,
            Self::Grok => GROK_HANDLER,
            Self::Antigravity => ANTIGRAVITY_HANDLER,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Grok => "Grok",
            Self::Antigravity => "Antigravity",
        }
    }

    pub fn default_settings_path(self) -> PathBuf {
        match self {
            Self::Claude => ClaudePaths::default_for_user().config.join("settings.json"),
            Self::Grok => crate::grok::GrokPaths::default_for_user()
                .home
                .join("hooks/nexushub.json"),
            Self::Antigravity => {
                let home = crate::antigravity::sessions::AntigravityPaths::default_for_user().home;
                home.parent()
                    .unwrap_or(home.as_path())
                    .join("config/hooks.json")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeHooksAction {
    Status,
    Install,
    Remove,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaudeHooksRequest {
    pub action: ClaudeHooksAction,
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeHooksStatus {
    pub installed: bool,
    pub managed_count: usize,
    pub changed: bool,
    #[serde(default)]
    pub config_path: String,
}

fn is_any_nexushub_handler_name(name: &str) -> bool {
    matches!(name, HANDLER | GROK_HANDLER | ANTIGRAVITY_HANDLER)
}

fn managed_for_handler(value: &Value, binary: &Path, handler: &str) -> bool {
    value["type"] == "command"
        && value["command"].as_str() == binary.to_str()
        && value["args"].as_array().is_some_and(|args| {
            args.len() == 3 && args[0] == "--config" && args[1].is_string() && args[2] == handler
        })
}

fn managed_any_nexushub_handler(value: &Value, binary: &Path) -> bool {
    (value["type"] == "command"
        && value["command"].as_str() == binary.to_str()
        && value["args"].as_array().is_some_and(|args| {
            args.len() == 3
                && args[0] == "--config"
                && args[1].is_string()
                && args[2].as_str().is_some_and(is_any_nexushub_handler_name)
        }))
        || is_managed_antigravity_stop_entry(value, binary)
        || value["name"]
            .as_str()
            .is_some_and(|n| n.starts_with("nexushub-"))
}

fn expected_for_handler(binary: &Path, config: &Path, handler: &str) -> Value {
    if handler == GROK_HANDLER {
        json!({"type": "command", "command": binary, "args": ["--config", config, handler]})
    } else {
        json!({"type": "command", "command": binary, "args": ["--config", config, handler], "async": true})
    }
}

fn expected_group_for_handler(binary: &Path, config: &Path, handler: &str, matcher: &str) -> Value {
    let hook = expected_for_handler(binary, config, handler);
    if handler == GROK_HANDLER && matcher.is_empty() {
        json!({"hooks": [hook]})
    } else {
        json!({"matcher": matcher, "hooks": [hook]})
    }
}

fn group_matcher_matches(group: &Value, handler: &str, expected_matcher: &str) -> bool {
    if handler == GROK_HANDLER && expected_matcher.is_empty() {
        group.get("matcher").is_none()
    } else {
        group["matcher"].as_str() == Some(expected_matcher)
    }
}

fn antigravity_command_string(binary: &Path, config: &Path) -> String {
    format!(
        "{} --config {} {}",
        crate::shell::shell_quote(
            &binary.display().to_string(),
            crate::shell::QuoteStyle::Minimal
        ),
        crate::shell::shell_quote(
            &config.display().to_string(),
            crate::shell::QuoteStyle::Minimal
        ),
        ANTIGRAVITY_HANDLER
    )
}

fn expected_for_antigravity(binary: &Path, config: &Path) -> Value {
    json!({
        "type": "command",
        "command": antigravity_command_string(binary, config),
        "timeout": 10
    })
}

fn is_managed_antigravity_stop_entry(value: &Value, binary: &Path) -> bool {
    let prefix = format!(
        "{} --config ",
        crate::shell::shell_quote(
            &binary.display().to_string(),
            crate::shell::QuoteStyle::Minimal
        )
    );
    let suffix = format!(" {ANTIGRAVITY_HANDLER}");
    value["type"] == "command"
        && value["command"]
            .as_str()
            .is_some_and(|cmd| cmd.starts_with(&prefix) && cmd.ends_with(&suffix))
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(metadata) => {
            ensure!(
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= 2 * 1024 * 1024,
                "Claude 设置文件无法安全核验"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                ensure!(metadata.nlink() == 1, "Claude 设置文件存在硬链接");
            }
            Ok(Some(fs::read(path)?))
        }
    }
}

fn checked(bytes: Option<&[u8]>) -> Result<Value> {
    let root: Value = bytes
        .map(serde_json::from_slice)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    ensure!(root.is_object(), "Claude 设置必须为 JSON 对象，未作修改");
    if let Some(hooks) = root.get("hooks") {
        let hooks = hooks
            .as_object()
            .context("Claude hooks 格式错误，未作修改")?;
        for groups in hooks.values() {
            for group in groups
                .as_array()
                .context("Claude hook 分组格式错误，未作修改")?
            {
                ensure!(
                    group.is_object()
                        && group["hooks"]
                            .as_array()
                            .is_some_and(|hs| hs.iter().all(Value::is_object)),
                    "Claude hook 条目格式错误，未作修改"
                );
            }
        }
    }
    if let Some(nexushub) = root.get("nexushub") {
        let obj = nexushub
            .as_object()
            .context("Antigravity nexushub hook 配置格式错误，未作修改")?;
        if let Some(stop) = obj.get("Stop") {
            ensure!(
                stop.as_array()
                    .is_some_and(|items| items.iter().all(Value::is_object)),
                "Antigravity Stop hook 条目格式错误，未作修改"
            );
        }
    }
    Ok(root)
}

fn count_legacy_hooks_entries(root: &Value, binary: &Path, handler: &str) -> usize {
    let in_hooks = root["hooks"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .flat_map(|v| v.as_array().into_iter().flatten())
        .flat_map(|g| g["hooks"].as_array().into_iter().flatten())
        .filter(|h| managed_any_nexushub_handler(h, binary))
        .count();
    let misplaced_top = if handler == GROK_HANDLER {
        EVENTS
            .iter()
            .filter(|(ev, _)| root.get(*ev).is_some())
            .count()
    } else {
        0
    };
    in_hooks + misplaced_top
}

fn status_for_handler(
    path: &Path,
    root: &Value,
    binary: &Path,
    config: &Path,
    handler: &str,
) -> ClaudeHooksStatus {
    let legacy_count = count_legacy_hooks_entries(root, binary, handler);

    if handler == ANTIGRAVITY_HANDLER {
        let stop_entries = root["nexushub"]["Stop"].as_array();
        let agy_count = stop_entries
            .into_iter()
            .flatten()
            .filter(|h| is_managed_antigravity_stop_entry(h, binary))
            .count()
            + usize::from(
                root.get("nexushub").is_some()
                    && stop_entries.is_none_or(|items| {
                        !items
                            .iter()
                            .any(|h| is_managed_antigravity_stop_entry(h, binary))
                    }),
            );
        let expected = expected_for_antigravity(binary, config);
        let installed = legacy_count == 0
            && root["nexushub"]["enabled"] == true
            && stop_entries.is_some_and(|items| items.len() == 1 && items[0] == expected);
        return ClaudeHooksStatus {
            installed,
            managed_count: agy_count + legacy_count,
            changed: false,
            config_path: path.display().to_string(),
        };
    }

    let expected_hook = expected_for_handler(binary, config, handler);
    let installed = legacy_count == EVENTS.len()
        && EVENTS.iter().all(|(event, matcher)| {
            root["hooks"][event].as_array().is_some_and(|groups| {
                groups.iter().any(|g| {
                    group_matcher_matches(g, handler, matcher)
                        && g["hooks"]
                            .as_array()
                            .is_some_and(|hs| hs.contains(&expected_hook))
                })
            })
        });
    ClaudeHooksStatus {
        installed,
        managed_count: legacy_count,
        changed: false,
        config_path: path.display().to_string(),
    }
}

pub fn installed(paths: &ClaudePaths, binary: &Path, config: &Path) -> bool {
    let settings_path = paths.config.join("settings.json");
    read(&settings_path)
        .and_then(|bytes| checked(bytes.as_deref()))
        .is_ok_and(|root| {
            status_for_handler(&settings_path, &root, binary, config, HANDLER).installed
        })
}

/// The monitor runs the same fixed executable as the hook, including when a
/// user selected a non-default AgentHub config file.
pub fn configured(paths: &ClaudePaths, binary: &Path) -> bool {
    configured_path(&paths.config.join("settings.json"), binary, HANDLER)
}

pub fn provider_configured(provider: HookProvider, binary: &Path) -> bool {
    configured_path(
        &provider.default_settings_path(),
        binary,
        provider.handler(),
    )
}

pub fn configured_path(settings_path: &Path, binary: &Path, handler: &str) -> bool {
    read(settings_path)
        .and_then(|bytes| checked(bytes.as_deref()))
        .is_ok_and(|root| {
            if handler == ANTIGRAVITY_HANDLER {
                return count_legacy_hooks_entries(&root, binary, handler) == 0
                    && root["nexushub"]["enabled"] == true
                    && root["nexushub"]["Stop"].as_array().is_some_and(|items| {
                        items.len() == 1
                            && is_managed_antigravity_stop_entry(&items[0], binary)
                            && items[0]["timeout"] == 10
                    });
            }
            let config = root["hooks"]["Stop"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|g| g["hooks"].as_array().into_iter().flatten())
                .find(|h| managed_for_handler(h, binary, handler))
                .and_then(|h| h["args"][1].as_str());
            config.is_some_and(|config| {
                status_for_handler(settings_path, &root, binary, Path::new(config), handler)
                    .installed
            })
        })
}

pub fn manage(
    paths: &ClaudePaths,
    binary: &Path,
    config: &Path,
    request: ClaudeHooksRequest,
) -> Result<ClaudeHooksStatus> {
    manage_file(
        &paths.config.join("settings.json"),
        HANDLER,
        "Claude",
        binary,
        config,
        request,
    )
}

pub fn manage_provider(
    provider: HookProvider,
    binary: &Path,
    config: &Path,
    request: ClaudeHooksRequest,
) -> Result<ClaudeHooksStatus> {
    manage_file(
        &provider.default_settings_path(),
        provider.handler(),
        provider.label(),
        binary,
        config,
        request,
    )
}

pub fn manage_file(
    path: &Path,
    handler: &str,
    label: &str,
    binary: &Path,
    config: &Path,
    request: ClaudeHooksRequest,
) -> Result<ClaudeHooksStatus> {
    let _lock = SETTINGS
        .lock()
        .map_err(|_| anyhow::anyhow!("{label} 设置管理锁不可用"))?;
    let before = read(path)?;
    let before_stamp = before
        .as_ref()
        .map(|_| super::reader::stamp(path))
        .transpose()?;
    let mut root = checked(before.as_deref())?;
    let prior = status_for_handler(path, &root, binary, config, handler);
    if request.action == ClaudeHooksAction::Status {
        return Ok(prior);
    }
    ensure!(request.confirmed, "{label} hooks 安装或移除需要明确确认");
    if (request.action == ClaudeHooksAction::Install && prior.installed)
        || (request.action == ClaudeHooksAction::Remove && prior.managed_count == 0)
    {
        return Ok(prior);
    }
    if request.action == ClaudeHooksAction::Install {
        ensure!(
            binary.is_absolute() && binary.is_file(),
            "本机通知程序不可用，请修复 App 安装"
        );
    }
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut() {
            let groups = groups.as_array_mut().unwrap();
            groups.retain_mut(|group| {
                let handlers = group["hooks"].as_array_mut().unwrap();
                let before_len = handlers.len();
                handlers.retain(|h| !managed_any_nexushub_handler(h, binary));
                before_len == handlers.len() || !handlers.is_empty()
            });
        }
        if handler == ANTIGRAVITY_HANDLER
            && hooks
                .values()
                .all(|v| v.as_array().is_some_and(Vec::is_empty))
        {
            root.as_object_mut().unwrap().remove("hooks");
        }
    }
    if handler == GROK_HANDLER {
        if let Some(obj) = root.as_object_mut() {
            for (ev, _) in EVENTS {
                obj.remove(ev);
            }
        }
    }
    if handler == ANTIGRAVITY_HANDLER {
        let root_obj = root.as_object_mut().unwrap();
        if request.action == ClaudeHooksAction::Install {
            root_obj.insert(
                "nexushub".into(),
                json!({
                    "enabled": true,
                    "Stop": [expected_for_antigravity(binary, config)]
                }),
            );
        } else {
            root_obj.remove("nexushub");
        }
    } else if request.action == ClaudeHooksAction::Install {
        let hooks = root
            .as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap();
        for (event, matcher) in EVENTS {
            hooks
                .entry(event)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(expected_group_for_handler(binary, config, handler, matcher));
        }
    }
    let bytes = serde_json::to_vec_pretty(&root)?;
    let parent_dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent_dir)?;
    let temporary = parent_dir.join(format!(".nexushub-hooks-{}.tmp", uuid::Uuid::new_v4()));
    let write = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        #[cfg(test)]
        SETTINGS_CHECKPOINT.with(|slot| {
            if let Some(hook) = slot.borrow_mut().as_mut() {
                hook(path);
            }
        });
        ensure!(
            read(path)? == before
                && before_stamp
                    == if path.exists() {
                        Some(super::reader::stamp(path)?)
                    } else {
                        None
                    },
            "{label} 设置已被其他程序修改，请刷新后重试"
        );
        fs::rename(&temporary, path)?;
        ensure!(
            read(path)?.as_deref() == Some(bytes.as_slice()),
            "{label} 设置读回发生变化，请刷新后检查"
        );
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    write?;
    Ok(ClaudeHooksStatus {
        changed: true,
        ..status_for_handler(path, &root, binary, config, handler)
    })
}

#[cfg(test)]
type SettingsHook = Box<dyn FnMut(&Path)>;
#[cfg(test)]
thread_local! { static SETTINGS_CHECKPOINT: std::cell::RefCell<Option<SettingsHook>> = const { std::cell::RefCell::new(None) }; }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_repair_remove_preserve_user_hooks_and_reject_conflicts() {
        let dir = std::env::temp_dir().join(format!("claude-hooks-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let paths = ClaudePaths {
            config: dir.clone(),
            projects: dir.join("projects"),
        };
        let binary = dir.join("nexushub-webd");
        fs::write(&binary, "fixture").unwrap();
        let config = dir.join("fixture.toml");
        let path = dir.join("settings.json");
        let original = json!({"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"user-fixture","args":[]}]}]}});
        fs::write(&path, original.to_string()).unwrap();
        let run = |action| {
            manage(
                &paths,
                &binary,
                &config,
                ClaudeHooksRequest {
                    action,
                    confirmed: true,
                },
            )
        };
        let status = run(ClaudeHooksAction::Install).unwrap();
        assert!(status.installed && status.config_path == path.display().to_string());
        assert!(!run(ClaudeHooksAction::Install).unwrap().changed);
        let installed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(installed["theme"], "dark");
        assert_eq!(installed["hooks"]["Stop"][0], original["hooks"]["Stop"][0]);
        assert_eq!(
            installed["hooks"]["Stop"][1]["hooks"][0]["args"][2],
            HANDLER
        );
        assert_eq!(installed["hooks"]["Stop"][1]["hooks"][0]["async"], true);
        assert!(!run(ClaudeHooksAction::Remove).unwrap().installed);
        assert!(!run(ClaudeHooksAction::Remove).unwrap().changed);

        SETTINGS_CHECKPOINT.with(|s| {
            *s.borrow_mut() = Some(Box::new(|p| fs::write(p, "{\"concurrent\":true}").unwrap()))
        });
        assert!(run(ClaudeHooksAction::Install).is_err());
        SETTINGS_CHECKPOINT.with(|s| *s.borrow_mut() = None);
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"concurrent\":true}");
        for broken in ["{broken", "[]", "{\"hooks\":{\"Stop\":false}}"] {
            fs::write(&path, broken).unwrap();
            assert!(run(ClaudeHooksAction::Install).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        }

        // Grok hook cleans legacy claude-hook / misplaced top-level keys, omits async and empty matcher.
        let grok_path = dir.join("grok/hooks/nexushub.json");
        fs::create_dir_all(grok_path.parent().unwrap()).unwrap();
        fs::write(
            &grok_path,
            serde_json::to_vec(&json!({
                "Stop": [{"matcher": "", "hooks": []}],
                "hooks": {
                    "Stop": [{"matcher": "", "hooks": [expected_for_handler(&binary, &config, HANDLER)]}]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let run_grok = |action| {
            manage_file(
                &grok_path,
                GROK_HANDLER,
                "Grok",
                &binary,
                &config,
                ClaudeHooksRequest {
                    action,
                    confirmed: true,
                },
            )
        };
        let grok_st = run_grok(ClaudeHooksAction::Install).unwrap();
        assert!(grok_st.installed && grok_st.managed_count == 3);
        assert!(configured_path(&grok_path, &binary, GROK_HANDLER));
        let grok_json: Value = serde_json::from_slice(&fs::read(&grok_path).unwrap()).unwrap();
        assert!(grok_json.get("Stop").is_none());
        assert!(grok_json["hooks"]["Stop"][0].get("matcher").is_none());
        assert!(grok_json["hooks"]["Stop"][0]["hooks"][0]
            .get("async")
            .is_none());
        let grok_rem = run_grok(ClaudeHooksAction::Remove).unwrap();
        assert!(!grok_rem.installed && grok_rem.managed_count == 0);

        // Antigravity hook registers Stop under top-level "nexushub" and cleans command-style or args-style "hooks".
        let agy_path = dir.join("gemini/config/hooks.json");
        fs::create_dir_all(agy_path.parent().unwrap()).unwrap();
        fs::write(
            &agy_path,
            serde_json::to_vec(&json!({
                "cargo-target-gc": {
                    "enabled": true,
                    "Stop": [{"type": "command", "command": "/bin/true", "timeout": 10}]
                },
                "hooks": {
                    "AfterAgent": [{"matcher": "*", "hooks": [{"name": "nexushub-antigravity-hook", "type": "command", "command": antigravity_command_string(&binary, &config), "timeout": 10000}]}]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let run_agy = |action| {
            manage_file(
                &agy_path,
                ANTIGRAVITY_HANDLER,
                "Antigravity",
                &binary,
                &config,
                ClaudeHooksRequest {
                    action,
                    confirmed: true,
                },
            )
        };
        assert_eq!(run_agy(ClaudeHooksAction::Status).unwrap().managed_count, 1);
        let agy_st = run_agy(ClaudeHooksAction::Install).unwrap();
        assert!(agy_st.installed && agy_st.managed_count == 1);
        assert!(configured_path(&agy_path, &binary, ANTIGRAVITY_HANDLER));
        let agy_json: Value = serde_json::from_slice(&fs::read(&agy_path).unwrap()).unwrap();
        assert!(agy_json.get("hooks").is_none());
        assert_eq!(agy_json["cargo-target-gc"]["enabled"], true);
        assert_eq!(agy_json["nexushub"]["enabled"], true);
        let agy_rem = run_agy(ClaudeHooksAction::Remove).unwrap();
        assert!(!agy_rem.installed && agy_rem.managed_count == 0);
        let agy_after: Value = serde_json::from_slice(&fs::read(&agy_path).unwrap()).unwrap();
        assert!(
            agy_after.get("nexushub").is_none() && agy_after["cargo-target-gc"]["enabled"] == true
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
