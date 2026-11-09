use agenthub_core::shell::{shell_quote, QuoteStyle};
mod antigravity;
mod claude_hook;
mod cli;
mod hooks;
mod monitor_start;
mod notification_format;
use cli::*;
use hooks::*;
use monitor_start::*;

mod api;
mod async_question_monitor;
mod audit;
mod bark;
mod github_ci_monitor;
mod linux_adapter;
mod notification_delivery;
mod provider_monitor;
mod question_monitor;
mod rpc_payload;
mod ssh_server;
mod state;

#[cfg(test)]
mod notification_accuracy_tests;

use agenthub_core::{
    codex::{
        resolve_codex_paths, rollout_completion_last_agent_message_selection,
        rollout_hook_stop_message_selection, rollout_request_user_input_state, PendingElicitation,
        RolloutMessageSelection, RolloutRequestUserInputState,
    },
    config::Config,
    db::{NewProbeErrorIncident, NewProbeEvent, PanelDb, ProbeErrorIncident},
    platform::PlatformPaths,
    probe::{redact_probe_event_for_output, ProbeEventInput, ProbeEventOutcome, ProbeRuntime},
    probe_error_monitor::{
        scan_codex_turn_errors, ProbeErrorCursor, ProbeErrorMonitorRuntimeStatus,
    },
    services::probe as probe_service,
    services::system::HostSurface,
};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use state::AppState;
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::Duration,
};
use tokio::time;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const PROBE_THREAD_SCAN_TICK_SECONDS: u64 = 120;
#[cfg(test)]
const PROBE_BARK_BODY_CHUNK_BYTES: usize = 2_400;
const QUESTION_CONFIRMATION_DELAY_MS: u64 = 1_000;
/// The foreground Stop hook spawns the detached child right before calling `exit(0)`.
/// Antigravity's hook runner waits for the foreground process to exit and close stdout
/// before writing the current turn's `ExecutorMetadata`. Waiting 120ms lets the host
/// persist `ExecutorMetadata` before the first `GetAllCascadeTrajectories` query.
const ANTIGRAVITY_DETACHED_SETTLE_DELAY_MS: u64 = 120;
const PROBE_ERROR_MONITOR_CURSOR_SETTING: &str = "probe_error_monitor_cursor";
const PROBE_ERROR_MONITOR_STATUS_SETTING: &str = "probe_error_monitor_status";
const PROBE_ERROR_MONITOR_BATCH_LIMIT: usize = 100;
static PROBE_THREAD_SCAN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static PROBE_ERROR_MONITOR_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agenthub_webd=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let cli = Cli::parse();
    let mut runtime = if matches!(cli.command, Command::RpcStdio | Command::Health) {
        tokio::runtime::Builder::new_current_thread()
    } else {
        tokio::runtime::Builder::new_multi_thread()
    };
    runtime.enable_all().build()?.block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    if matches!(
        cli.command,
        Command::ClaudeHook | Command::GrokHook | Command::AntigravityHook { .. }
    ) {
        let is_antigravity_foreground = matches!(
            cli.command,
            Command::AntigravityHook {
                detached: false,
                ..
            }
        );
        let allow_printed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            !is_antigravity_foreground,
        ));
        let watchdog_allow_printed = std::sync::Arc::clone(&allow_printed);
        // A hook never blocks the host Agent or leaks payloads to logs.
        // Force exit also bounds any filesystem/stdin work in blocking tasks.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            if is_antigravity_foreground {
                let mut stdout = io::stdout().lock();
                let _ = print_antigravity_stop_allow_once(&watchdog_allow_printed, &mut stdout);
            }
            std::process::exit(0);
        });
        match cli.command {
            Command::ClaudeHook => {
                let _ = tokio::time::timeout(
                    Duration::from_millis(4800),
                    claude_hook::run(&cli.config),
                )
                .await;
            }
            Command::GrokHook => {
                let _ = tokio::time::timeout(
                    Duration::from_millis(4800),
                    provider_monitor::run_provider_hook(
                        &cli.config,
                        agenthub_core::native_probe::NativeProvider::Grok,
                    ),
                )
                .await;
            }
            Command::AntigravityHook {
                detached,
                cascade_id,
            } => {
                if !detached {
                    let conversation_id = read_optional_stdin_json()
                        .ok()
                        .flatten()
                        .and_then(|payload| {
                            read_string_field(
                                &payload,
                                &[
                                    "conversationId",
                                    "conversation_id",
                                    "cascadeId",
                                    "cascade_id",
                                    "session_id",
                                    "sessionId",
                                ],
                            )
                        })
                        .or(cascade_id);
                    {
                        let mut stdout = io::stdout().lock();
                        let _ = print_antigravity_stop_allow_once(&allow_printed, &mut stdout);
                    }
                    if spawn_detached_antigravity_hook(&cli.config, conversation_id.as_deref()) {
                        std::process::exit(0);
                    }
                    let _ = tokio::time::timeout(
                        Duration::from_millis(4800),
                        provider_monitor::run_antigravity_hook(
                            &cli.config,
                            conversation_id.as_deref(),
                        ),
                    )
                    .await;
                } else {
                    tokio::time::sleep(Duration::from_millis(ANTIGRAVITY_DETACHED_SETTLE_DELAY_MS))
                        .await;
                    let _ = tokio::time::timeout(
                        Duration::from_millis(4500),
                        provider_monitor::run_antigravity_hook(&cli.config, cascade_id.as_deref()),
                    )
                    .await;
                }
            }
            _ => unreachable!(),
        }
        std::process::exit(0);
    }
    match cli.command {
        Command::ClaudeHook | Command::GrokHook | Command::AntigravityHook { .. } => unreachable!(),
        Command::InitConfig => {
            Config::write_default(&cli.config)?;
            println!("wrote {}", cli.config.display());
        }
        Command::Doctor => {
            let config = Config::load_runtime(&cli.config)?;
            let db = open_panel_db(&config)?;
            let resolved = resolve_codex_paths(&config.codex.home);
            println!("config={}", cli.config.display());
            println!("db={}", db.path().display());
            println!("codex_home={}", resolved.home.display());
            println!(
                "configured_codex_home={}",
                resolved.configured_codex_home.as_deref().unwrap_or("auto")
            );
            println!("codex_home_source={}", resolved.codex_home_source);
            println!("codex_read_model=local_state_rollout_logs");
        }
        Command::Probe {
            command: ProbeCommand::HookRequestUserInput,
        } => {
            if let Err(err) = handle_hook_request_user_input_command(&cli.config).await {
                eprintln!(
                    "{}",
                    serde_json::to_string(&json!({
                        "ok": false,
                        "error": "probe_hook_request_user_input_failed",
                    }))?
                );
                tracing::warn!("probe hook-request-user-input failed open");
                tracing::debug!("probe hook-request-user-input diagnostic: {err:#}");
            }
        }
        Command::Probe {
            command: ProbeCommand::HookRequestUserInputConfirm,
        } => {
            if let Err(err) = handle_hook_request_user_input_confirm_command(&cli.config).await {
                tracing::warn!("probe hook-request-user-input-confirm failed closed");
                tracing::debug!("probe hook-request-user-input-confirm diagnostic: {err:#}");
            }
        }
        Command::Probe {
            command: ProbeCommand::MonitorErrors,
        } => run_probe_error_monitor_daemon(cli.config).await?,
        Command::Probe { command } => {
            let config = Config::load_runtime(&cli.config)?;
            let db = open_panel_db(&config)?;
            run_probe_command(command, &config, db, &cli.config).await?;
        }
        Command::Serve { surface } => serve(cli.config, surface).await?,
        Command::RpcStdio => ssh_server::proxy().await?,
        Command::Health => ssh_server::health().await?,
    }
    Ok(())
}

async fn run_probe_command(
    command: ProbeCommand,
    config: &Config,
    db: PanelDb,
    config_path: &Path,
) -> Result<()> {
    let cache = monitor_start::DaemonMonitorCache::with_config(config_path, config);
    let load_config = || cache.load_config(config_path);
    match command {
        ProbeCommand::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&probe_runtime(config).status().await?)?
            );
        }
        ProbeCommand::HookStatus => {
            println!(
                "{}",
                serde_json::to_string_pretty(&probe_runtime(config).hook_status())?
            );
        }
        ProbeCommand::Events { limit } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "events": db.list_probe_events(limit)?.into_iter().map(redact_probe_event).collect::<Vec<_>>()
                }))?
            );
        }
        ProbeCommand::Running { limit } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &probe_thread_snapshot(config, &db, "running", limit).await?
                )?
            );
        }
        ProbeCommand::ReplyNeeded { limit } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &probe_thread_snapshot(config, &db, "reply-needed", limit).await?
                )?
            );
        }
        ProbeCommand::Recoverable { limit } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &probe_thread_snapshot(config, &db, "recoverable", limit).await?
                )?
            );
        }
        ProbeCommand::HookStop {
            thread_id,
            turn_id,
            kind,
        } => match handle_hook_stop_command(&load_config, &db, thread_id, turn_id, kind).await {
            Ok(result) => {
                let (stdout, stderr) = hook_stop_cli_output(&result)?;
                eprint!("{stderr}");
                print!("{stdout}");
            }
            Err(err) => {
                eprintln!(
                    "{}",
                    serde_json::to_string(&json!({
                        "ok": false,
                        "error": "probe_hook_stop_failed",
                    }))?
                );
                tracing::warn!("probe hook-stop failed before event recording");
                tracing::debug!("probe hook-stop diagnostic: {err:#}");
                println!("{}", serde_json::to_string(&codex_stop_continue_output())?);
            }
        },
        ProbeCommand::HookRequestUserInput
        | ProbeCommand::HookRequestUserInputConfirm
        | ProbeCommand::MonitorErrors => {
            unreachable!("handled before config loading")
        }
        ProbeCommand::HooksInstall { dry_run } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &install_probe_hooks(config, Some(config_path), dry_run).await?
                )?
            );
        }
        ProbeCommand::HooksRemove { dry_run } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &remove_probe_hooks(config, Some(config_path), dry_run).await?
                )?
            );
        }
        ProbeCommand::NotifyCompletion { thread_id, turn_id } => {
            let stdin_payload = read_optional_stdin_json()?;
            let input = notify_completion_context(
                config,
                stdin_payload.as_ref(),
                thread_id.as_deref(),
                turn_id.as_deref(),
            )?;
            let event = probe_runtime(config).build_event(input);
            let (outcome, bark) = record_probe_event_with_bark(&load_config, &db, event).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "probe_event": outcome,
                    "bark": bark,
                }))?
            );
        }
        ProbeCommand::BarkTest => {
            let bark = notification_delivery::test(config_path, &db).await?;
            println!("{}", serde_json::to_string_pretty(&bark)?);
            anyhow::ensure!(bark.sent, "Bark test was not accepted");
        }
        ProbeCommand::LifecycleRepair => {
            anyhow::bail!(
                "unsupported_probe_action: lifecycle_repair has no fixed AgentHub implementation"
            );
        }
        ProbeCommand::ServiceRestart => {
            anyhow::bail!(
                "unsupported_probe_action: service_restart has no fixed AgentHub implementation"
            );
        }
        ProbeCommand::LegacyImport => {
            anyhow::bail!("unavailable: legacy configuration import is retired");
        }
        ProbeCommand::DiagnoseAntigravity { cascade_id } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&antigravity::diagnose(&cascade_id, &db).await?)?
            );
        }
    }
    Ok(())
}

fn probe_runtime(config: &Config) -> ProbeRuntime {
    ProbeRuntime::new(config.clone(), PlatformPaths::current())
}

fn redact_probe_event(event: agenthub_core::db::ProbeEvent) -> agenthub_core::db::ProbeEvent {
    redact_probe_event_for_output(event)
}

async fn probe_thread_snapshot(
    config: &Config,
    db: &PanelDb,
    status: &'static str,
    limit: usize,
) -> Result<Value> {
    let status = status.trim();
    let state = AppState::new(config.clone(), db.clone());
    let threads = api::load_probe_threads(&state, status, limit).await?;
    Ok(json!({
        "status": status,
        "count": threads.len(),
        "threads": threads,
    }))
}

async fn install_probe_hooks(
    config: &Config,
    config_path: Option<&Path>,
    dry_run: bool,
) -> Result<Value> {
    let mut out = install_probe_hooks_with_repair(config, dry_run, |platform, enabled| {
        Ok(serde_json::to_value(
            agenthub_core::probe_error_monitor::ensure_probe_error_monitor_launch_agent(
                platform, enabled,
            )?,
        )?)
    })
    .await?;
    if !dry_run && !config.probe.hooks.manage_stop_hook {
        if let Some(path) = config_path.filter(|p| p.exists()) {
            let before = read_optional_text(path)?;
            let patched = agenthub_core::config::patch_probe_config_toml(
                &before,
                &agenthub_core::config::ProbeConfigFilePatch {
                    codex: None,
                    probe: Some(agenthub_core::config::ProbeSettingsPatch {
                        hooks: Some(agenthub_core::config::ProbeHooksConfigPatch {
                            manage_stop_hook: Some(true),
                        }),
                        ..Default::default()
                    }),
                },
            )?;
            if patched != before {
                fs::write(path, patched)
                    .with_context(|| format!("write probe config {}", path.display()))?;
                out["changed"] = json!(true);
                out["probe_config_changed"] = json!(true);
            }
        }
    }
    Ok(out)
}

async fn remove_probe_hooks(
    config: &Config,
    config_path: Option<&Path>,
    dry_run: bool,
) -> Result<Value> {
    let resolved = resolve_codex_paths(&config.codex.home);
    let hooks_path = resolved.home.join("hooks.json");
    let codex_config_path = resolved.home.join("config.toml");
    let had_hooks_file = hooks_path.exists();
    let mut root = read_hooks_json(&hooks_path)?;
    let hooks_json_changed =
        had_hooks_file && agenthub_core::probe::remove_agenthub_managed_hooks(&mut root);
    let config_before = read_optional_text(&codex_config_path)?;
    let codex_config_after = prune_codex_hook_state_toml(&config_before)?;
    let codex_config_changed = codex_config_after != config_before;
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let backup_path = hooks_path.with_extension(format!("json.agenthub-probe-bak-{stamp}"));
    let codex_config_backup_path =
        codex_config_path.with_extension(format!("toml.agenthub-probe-bak-{stamp}"));
    if hooks_json_changed && !dry_run {
        fs::copy(&hooks_path, &backup_path).with_context(|| {
            format!(
                "backup hooks {} to {}",
                hooks_path.display(),
                backup_path.display()
            )
        })?;
        fs::write(&hooks_path, serde_json::to_vec_pretty(&root)?)
            .with_context(|| format!("write hooks {}", hooks_path.display()))?;
    }
    if codex_config_changed && !dry_run {
        if codex_config_path.exists() {
            fs::copy(&codex_config_path, &codex_config_backup_path).with_context(|| {
                format!(
                    "backup codex config {} to {}",
                    codex_config_path.display(),
                    codex_config_backup_path.display()
                )
            })?;
        }
        fs::write(&codex_config_path, codex_config_after)
            .with_context(|| format!("write codex config {}", codex_config_path.display()))?;
    }
    let mut probe_config_changed = false;
    if !dry_run {
        if let Some(path) = config_path.filter(|p| p.exists()) {
            let before = read_optional_text(path)?;
            let patched = agenthub_core::config::patch_probe_config_toml(
                &before,
                &agenthub_core::config::ProbeConfigFilePatch {
                    codex: None,
                    probe: Some(agenthub_core::config::ProbeSettingsPatch {
                        hooks: Some(agenthub_core::config::ProbeHooksConfigPatch {
                            manage_stop_hook: Some(false),
                        }),
                        ..Default::default()
                    }),
                },
            )?;
            if patched != before {
                fs::write(path, patched)
                    .with_context(|| format!("write probe config {}", path.display()))?;
                probe_config_changed = true;
            }
        }
    }
    let changed = hooks_json_changed || codex_config_changed || probe_config_changed;
    Ok(json!({
        "ok": true,
        "dry_run": dry_run,
        "changed": changed,
        "hooks_json_changed": hooks_json_changed,
        "codex_config_changed": codex_config_changed,
        "probe_config_changed": probe_config_changed,
        "hooks_json": hooks_path,
        "codex_config": codex_config_path,
        "resolved_codex_home": resolved.home,
        "backup_path": if hooks_json_changed && had_hooks_file { Some(backup_path) } else { None },
        "codex_config_backup_path": if codex_config_changed && codex_config_path.exists() { Some(codex_config_backup_path) } else { None },
    }))
}

fn prune_codex_hook_state_toml(text: &str) -> Result<String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let mut value: toml::Value = toml::from_str(text).context("parse Codex config.toml")?;
    let Some(root) = value.as_table_mut() else {
        return Ok(text.to_string());
    };
    if !prune_codex_hook_state(root) {
        return Ok(text.to_string());
    }
    toml::to_string_pretty(&value).context("serialize Codex config.toml")
}

async fn install_probe_hooks_with_repair(
    config: &Config,
    dry_run: bool,
    repair_error_monitor: impl FnOnce(&PlatformPaths, bool) -> Result<Value>,
) -> Result<Value> {
    let resolved = resolve_codex_paths(&config.codex.home);
    let hooks_path = resolved.home.join("hooks.json");
    let codex_config_path = resolved.home.join("config.toml");
    let platform = PlatformPaths::current();
    let hook_command = format!(
        "{} --config {} probe hook-stop",
        shell_quote(
            &platform.daemon_binary().display().to_string(),
            QuoteStyle::Minimal
        ),
        shell_quote(
            &platform.config_file.display().to_string(),
            QuoteStyle::Minimal
        )
    );
    let request_user_input_hook_command = format!(
        "{} --config {} probe hook-request-user-input",
        shell_quote(
            &platform.daemon_binary().display().to_string(),
            QuoteStyle::Minimal
        ),
        shell_quote(
            &platform.config_file.display().to_string(),
            QuoteStyle::Minimal
        )
    );
    let mut root = read_hooks_json(&hooks_path)?;
    let hooks_json_changed = ensure_probe_hook(&mut root, "Stop", "*", &hook_command, None)
        | ensure_probe_hook(
            &mut root,
            "PreToolUse",
            agenthub_core::probe::PROBE_QUESTION_HOOK_MATCHER,
            &request_user_input_hook_command,
            Some(5),
        );
    let config_before = read_optional_text(&codex_config_path)?;
    let codex_config_after = ensure_codex_hooks_feature(&config_before)?;
    let codex_config_changed = codex_config_after != config_before;
    let backup_path = hooks_path.with_extension(format!(
        "json.agenthub-probe-bak-{}",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    let codex_config_backup_path = codex_config_path.with_extension(format!(
        "toml.agenthub-probe-bak-{}",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    if hooks_json_changed && !dry_run {
        if let Some(parent) = hooks_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create hooks dir {}", parent.display()))?;
        }
        if hooks_path.exists() {
            fs::copy(&hooks_path, &backup_path).with_context(|| {
                format!(
                    "backup hooks {} to {}",
                    hooks_path.display(),
                    backup_path.display()
                )
            })?;
        }
        fs::write(&hooks_path, serde_json::to_vec_pretty(&root)?)
            .with_context(|| format!("write hooks {}", hooks_path.display()))?;
    }
    if codex_config_changed && !dry_run {
        if let Some(parent) = codex_config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create codex config dir {}", parent.display()))?;
        }
        if codex_config_path.exists() {
            fs::copy(&codex_config_path, &codex_config_backup_path).with_context(|| {
                format!(
                    "backup codex config {} to {}",
                    codex_config_path.display(),
                    codex_config_backup_path.display()
                )
            })?;
        }
        fs::write(&codex_config_path, codex_config_after)
            .with_context(|| format!("write codex config {}", codex_config_path.display()))?;
    }
    let error_monitor_launch_agent = if dry_run {
        Value::Null
    } else {
        repair_error_monitor(&platform, config.probe.monitor_enabled())?
    };
    let launch_agent_changed = error_monitor_launch_agent
        .get("changed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let changed = hooks_json_changed || codex_config_changed || launch_agent_changed;
    Ok(json!({
        "ok": true,
        "dry_run": dry_run,
        "changed": changed,
        "hooks_json_changed": hooks_json_changed,
        "codex_config_changed": codex_config_changed,
        "hooks_json": hooks_path,
        "codex_config": codex_config_path,
        "configured_codex_home": resolved.configured_codex_home,
        "resolved_codex_home": resolved.home,
        "codex_home_source": resolved.codex_home_source,
        "discovery_warnings": resolved.discovery_warnings,
        "backup_path": if hooks_path.exists() { Some(backup_path) } else { None },
        "codex_config_backup_path": if codex_config_path.exists() { Some(codex_config_backup_path) } else { None },
        "hook_command": hook_command,
        "request_user_input_hook_command": request_user_input_hook_command,
        "reload_result": Value::Null,
        "error_monitor_launch_agent": error_monitor_launch_agent,
    }))
}

fn read_optional_text(path: &Path) -> Result<String> {
    if path.exists() {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
    } else {
        Ok(String::new())
    }
}

fn ensure_codex_hooks_feature(text: &str) -> Result<String> {
    let mut value: toml::Value = if text.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(text).context("parse Codex config.toml")?
    };
    let root = value
        .as_table_mut()
        .context("Codex config.toml root must be a table")?;
    let features = root
        .entry("features")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if !features.is_table() {
        *features = toml::Value::Table(toml::map::Map::new());
    }
    let features = features
        .as_table_mut()
        .context("Codex config features must be a table")?;
    let mut changed = false;
    if !matches!(features.get("hooks"), Some(toml::Value::Boolean(true))) {
        features.insert("hooks".to_string(), toml::Value::Boolean(true));
        changed = true;
    }
    changed |= prune_codex_hook_state(root);
    if !changed {
        return Ok(text.to_string());
    }
    toml::to_string_pretty(&value).context("serialize Codex config.toml")
}

fn prune_codex_hook_state(root: &mut toml::map::Map<String, toml::Value>) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(toml::Value::as_table_mut) else {
        return false;
    };
    let Some(state) = hooks.get_mut("state").and_then(toml::Value::as_table_mut) else {
        return false;
    };
    let stale_keys = state
        .keys()
        .filter(|key| is_codex_hook_state_key(key))
        .cloned()
        .collect::<Vec<_>>();
    if stale_keys.is_empty() {
        return false;
    }
    for key in stale_keys {
        state.remove(&key);
    }
    if state.is_empty() {
        hooks.remove("state");
    }
    true
}

fn is_codex_hook_state_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    lowered.contains("hooks.json:stop:") || lowered.contains("hooks.json:pre_tool_use:")
}

fn read_hooks_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({"hooks": {}}));
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn ensure_probe_hook(
    root: &mut Value,
    event: &str,
    matcher: &str,
    hook_command: &str,
    timeout_seconds: Option<u64>,
) -> bool {
    let mut changed = false;
    if !root.is_object() {
        *root = json!({"hooks": {}});
        changed = true;
    }
    let object = root.as_object_mut().expect("object initialized");
    let hooks = object.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
        changed = true;
    }
    let hooks_object = hooks.as_object_mut().expect("hooks object initialized");
    let event_hooks = hooks_object
        .entry(event)
        .or_insert_with(|| Value::Array(Vec::new()));
    if !event_hooks.is_array() {
        *event_hooks = Value::Array(Vec::new());
        changed = true;
    }
    let groups = event_hooks.as_array_mut().expect("hook array initialized");
    let mut expected_installed = false;
    for group in groups.iter_mut() {
        if !group.is_object() {
            *group = json!({"matcher": "*", "hooks": []});
            changed = true;
        }
        let group_object = group.as_object_mut().expect("group object initialized");
        let group_matches = group_object.get("matcher").and_then(Value::as_str) == Some(matcher);
        let hooks = group_object
            .entry("hooks")
            .or_insert_with(|| Value::Array(Vec::new()));
        if !hooks.is_array() {
            *hooks = Value::Array(Vec::new());
            changed = true;
        }
        let items = hooks.as_array_mut().expect("group hooks array initialized");
        let before = items.len();
        items.retain(|item| {
            let Some(command) = item.get("command").and_then(Value::as_str) else {
                return true;
            };
            let timeout_matches = match timeout_seconds {
                Some(timeout) => item.get("timeout").and_then(Value::as_u64) == Some(timeout),
                None => item.get("timeout").is_none(),
            };
            let expected = command == hook_command
                && item.get("type").and_then(Value::as_str) == Some("command")
                && group_matches
                && timeout_matches
                && item.get("async").is_none();
            if expected && !expected_installed {
                expected_installed = true;
                return true;
            }
            !is_agenthub_managed_hook_command(command)
        });
        if items.len() != before {
            changed = true;
        }
    }
    let before_groups = groups.len();
    groups.retain(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
    });
    if groups.len() != before_groups {
        changed = true;
    }
    if expected_installed {
        return changed;
    }
    groups.push(json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": hook_command,
            "timeout": timeout_seconds,
        }]
    }));
    if timeout_seconds.is_none() {
        if let Some(hook) = groups
            .last_mut()
            .and_then(|group| group.get_mut("hooks"))
            .and_then(Value::as_array_mut)
            .and_then(|hooks| hooks.first_mut())
            .and_then(Value::as_object_mut)
        {
            hook.remove("timeout");
        }
    }
    true
}

fn is_agenthub_managed_hook_command(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    (lowered.contains("probe hook-stop") || lowered.contains("probe hook-request-user-input"))
        && (lowered.contains("agenthubd")
            || lowered.contains("agenthub-webd")
            || lowered.contains("/opt/agenthub")
            || lowered.contains("application support/agenthub")
            || lowered.contains("codex-sentinel"))
}

fn upgrade_managed_question_hook_groups(root: &mut Value) -> bool {
    let Some(groups) = root
        .pointer_mut("/hooks/PreToolUse")
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    let original = groups.clone();
    let mut upgraded = Vec::new();
    for group in &original {
        let Some(items) = group.get("hooks").and_then(Value::as_array) else {
            upgraded.push(group.clone());
            continue;
        };
        let (mut owned, others): (Vec<_>, Vec<_>) = items.iter().cloned().partition(|item| {
            item.get("command")
                .and_then(Value::as_str)
                .is_some_and(|command| {
                    is_agenthub_managed_hook_command(command)
                        && command.contains("probe hook-request-user-input")
                })
        });
        if owned.is_empty() {
            upgraded.push(group.clone());
            continue;
        }
        if !others.is_empty() {
            let mut remaining = group.clone();
            remaining["hooks"] = json!(others);
            upgraded.push(remaining);
        }
        for item in &mut owned {
            item["timeout"] = json!(5);
            if let Some(item) = item.as_object_mut() {
                item.remove("async");
            }
        }
        let mut managed = group.clone();
        managed["matcher"] = json!(agenthub_core::probe::PROBE_QUESTION_HOOK_MATCHER);
        managed["hooks"] = json!(owned);
        upgraded.push(managed);
    }
    if upgraded == original {
        return false;
    }
    *groups = upgraded;
    true
}

fn upgrade_managed_question_hooks(config: &Config) -> Result<bool> {
    if !config.probe.hooks.manage_stop_hook {
        return Ok(false);
    }
    let path = resolve_codex_paths(&config.codex.home)
        .home
        .join("hooks.json");
    if !path.exists() {
        return Ok(false);
    }
    let metadata = fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "managed hooks source is not a regular file"
    );
    let before = fs::read(&path)?;
    let mut root: Value = serde_json::from_slice(&before)?;
    if !upgrade_managed_question_hook_groups(&mut root) {
        return Ok(false);
    }
    let staged = path.with_file_name(format!(".agenthub-hooks-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.set_permissions(metadata.permissions())?;
        file.write_all(&serde_json::to_vec_pretty(&root)?)?;
        file.sync_all()?;
        anyhow::ensure!(
            fs::read(&path)? == before,
            "managed hooks changed during upgrade"
        );
        fs::rename(&staged, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result?;
    Ok(true)
}

fn upgrade_managed_question_hooks_on_start(config: &Config) {
    if let Err(error) = upgrade_managed_question_hooks(config) {
        tracing::warn!(
            "Managed question hook upgrade failed: {}",
            safe_probe_monitor_error(&error.to_string())
        );
    }
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let platform = PlatformPaths::current();
        let binary = platform.daemon_binary();
        if binary.is_file() {
            for provider in [
                agenthub_core::claude::hooks::HookProvider::Grok,
                agenthub_core::claude::hooks::HookProvider::Antigravity,
            ] {
                if let Ok(status) = agenthub_core::claude::hooks::manage_provider(
                    provider,
                    &binary,
                    &platform.config_file,
                    agenthub_core::claude::hooks::ClaudeHooksRequest {
                        action: agenthub_core::claude::hooks::ClaudeHooksAction::Status,
                        confirmed: false,
                    },
                ) {
                    if status.managed_count > 0 && !status.installed {
                        let _ = agenthub_core::claude::hooks::manage_provider(
                            provider,
                            &binary,
                            &platform.config_file,
                            agenthub_core::claude::hooks::ClaudeHooksRequest {
                                action: agenthub_core::claude::hooks::ClaudeHooksAction::Install,
                                confirmed: true,
                            },
                        );
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct HookStopResult {
    stdout: Value,
    outcome: ProbeEventOutcome,
    bark: ProbeBarkOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeBarkRequest {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    presentation: Option<notification_format::Presentation>,
    title: String,
    body: String,
    dedupe_key: String,
}

#[derive(Debug, Deserialize)]
struct BarkPushResponse {
    code: Option<i64>,
    message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct ProbeBarkOutcome {
    status: Option<String>,
    error_category: Option<String>,
    attempts: usize,
    confirmed_chunks: usize,
    next_retry_ms: Option<i64>,
    #[serde(skip_serializing)]
    retryable: bool,
    #[serde(skip_serializing)]
    retry_after_ms: Option<i64>,
    #[serde(skip_serializing)]
    uncertain: bool,
    sent: bool,
    skipped: bool,
    reason: Option<String>,
    http_status: Option<u16>,
    server_url: Option<String>,
    request_url: Option<String>,
    request_count: usize,
    chunk_count: usize,
    notifications_enabled: bool,
    relevant_switch_enabled: bool,
    device_key_configured: bool,
    dedupe_hit: bool,
    dedupe_key: Option<String>,
}

impl ProbeBarkOutcome {
    fn sent(
        status: u16,
        notifications_enabled: bool,
        relevant_switch_enabled: bool,
        device_key_configured: bool,
        dedupe_key: Option<String>,
    ) -> Self {
        Self {
            sent: true,
            skipped: false,
            reason: None,
            http_status: Some(status),
            server_url: None,
            request_url: None,
            request_count: 0,
            chunk_count: 0,
            notifications_enabled,
            relevant_switch_enabled,
            device_key_configured,
            dedupe_hit: false,
            dedupe_key,
            ..Self::default()
        }
    }

    fn skipped(
        reason: &str,
        notifications_enabled: bool,
        relevant_switch_enabled: bool,
        device_key_configured: bool,
    ) -> Self {
        Self {
            sent: false,
            skipped: true,
            reason: Some(reason.to_string()),
            http_status: None,
            server_url: None,
            request_url: None,
            request_count: 0,
            chunk_count: 0,
            notifications_enabled,
            relevant_switch_enabled,
            device_key_configured,
            dedupe_hit: reason == "dedupe",
            dedupe_key: None,
            ..Self::default()
        }
    }

    fn failed_request(
        reason: &str,
        notifications_enabled: bool,
        relevant_switch_enabled: bool,
        device_key_configured: bool,
        dedupe_key: Option<String>,
    ) -> Self {
        Self {
            sent: false,
            skipped: false,
            reason: Some(reason.to_string()),
            http_status: None,
            server_url: None,
            request_url: None,
            request_count: 0,
            chunk_count: 0,
            notifications_enabled,
            relevant_switch_enabled,
            device_key_configured,
            dedupe_hit: false,
            dedupe_key,
            ..Self::default()
        }
    }
}

async fn handle_built_probe_event(
    load_config: &notification_delivery::ConfigLoader<'_>,
    db: &PanelDb,
    event: agenthub_core::probe::ProbeBuiltEvent,
) -> Result<HookStopResult> {
    let (outcome, bark) = record_probe_event_with_bark(load_config, db, event).await?;
    Ok(HookStopResult {
        stdout: codex_stop_continue_output(),
        outcome,
        bark,
    })
}

fn hook_stop_cli_output(result: &HookStopResult) -> Result<(String, String)> {
    let stdout = format!("{}\n", serde_json::to_string(&result.stdout)?);
    let stderr = format!(
        "{}\n",
        serde_json::to_string(&json!({
            "probe_event": result.outcome,
            "bark": result.bark,
        }))?
    );
    Ok((stdout, stderr))
}

async fn record_probe_event_with_bark(
    load_config: &notification_delivery::ConfigLoader<'_>,
    db: &PanelDb,
    event: agenthub_core::probe::ProbeBuiltEvent,
) -> Result<(ProbeEventOutcome, ProbeBarkOutcome)> {
    record_probe_event_with_bark_timeout(load_config, db, event, std::time::Duration::from_secs(8))
        .await
}

async fn record_probe_event_with_bark_timeout(
    load_config: &notification_delivery::ConfigLoader<'_>,
    db: &PanelDb,
    event: agenthub_core::probe::ProbeBuiltEvent,
    bark_timeout: std::time::Duration,
) -> Result<(ProbeEventOutcome, ProbeBarkOutcome)> {
    let config = &load_config()?;
    if let Some(reason) = event
        .suppression_reason
        .as_deref()
        .or_else(|| task_notification_suppression_reason(config, event.thread_id.as_deref()))
    {
        let mut outcome = ProbeEventOutcome::from_claim(&event, false);
        outcome.duplicate = false;
        return Ok((
            outcome,
            ProbeBarkOutcome::skipped(
                reason,
                config.probe.notifications.enabled,
                probe_service::probe_event_bark_switch_enabled(config, &event.kind),
                false,
            ),
        ));
    }
    let event = async_question_monitor::prepare_event(config, db, event)?;
    let event = question_monitor::prepare_event(config, db, event)?;
    if let Some(reason) = event.suppression_reason.as_deref() {
        return Ok((
            ProbeEventOutcome::from_claim(&event, false),
            ProbeBarkOutcome::skipped(
                reason,
                config.probe.notifications.enabled,
                probe_service::probe_event_bark_switch_enabled(config, &event.kind),
                false,
            ),
        ));
    }
    let feedback_key = async_question_monitor::delivery_key(&event)
        .or_else(|| question_monitor::delivery_key(&event));
    let feedback_claimed = match feedback_key.as_deref() {
        Some(key) => db.claim_feedback_delivery(key)?,
        None => true,
    };
    let record_plan = probe_service::probe_event_record_plan(event);
    let event = record_plan.event;
    let passive_sent =
        passive_unresolved_action_sent(db, record_plan.passive_marker_key.as_deref())?;
    let claimed = db.claim_probe_dedupe(
        &event.dedupe_namespace,
        &event.dedupe_key,
        event.ttl_seconds,
    )?;
    let mut bark = notification_delivery::codex(
        load_config,
        db,
        &event,
        claimed && feedback_claimed && !passive_sent,
        bark_timeout,
    )
    .await?;
    if passive_sent {
        bark.reason = Some("sent_marker".into());
        bark.dedupe_hit = false;
    }
    if feedback_claimed {
        if let Some(key) = feedback_key.as_deref() {
            db.set_setting(key, &serde_json::to_string(&bark)?)?;
        }
    }
    let write_plan = probe_service::probe_event_record_write_plan(
        &event,
        claimed && feedback_claimed && !passive_sent,
        &bark,
        bark.status.as_deref().unwrap_or_else(|| {
            probe_service::probe_bark_status_label(bark.sent, bark.skipped, bark.reason.as_deref())
        }),
    )?;
    if let Some(mut record) = write_plan.record {
        record.payload["notification_event_key"] = json!(notification_delivery::codex_key(&event));
        db.record_probe_event(NewProbeEvent {
            kind: &record.kind,
            thread_id: record.thread_id.as_deref(),
            title: Some(&record.title),
            message: Some(&record.message),
            dedupe_key: Some(&record.dedupe_key),
            source: &record.source,
            payload: record.payload,
        })?;
    }
    if let Some(marker) = write_plan.passive_marker {
        mark_passive_unresolved_action_sent(db, marker)?;
    }

    Ok((write_plan.outcome, bark))
}

fn passive_unresolved_action_sent(db: &PanelDb, key: Option<&str>) -> Result<bool> {
    let Some(key) = key else {
        return Ok(false);
    };
    let exists = db.get_setting(key)?.is_some();
    if exists {
        let _ = db.touch_setting_if_older_than(key, 86_400);
    }
    Ok(exists)
}

fn mark_passive_unresolved_action_sent(
    db: &PanelDb,
    marker: probe_service::ProbePassiveMarkerWrite,
) -> Result<()> {
    db.set_setting(&marker.key, &serde_json::to_string(&marker.value)?)
}

fn codex_stop_continue_output() -> Value {
    json!({
        "continue": true,
        "suppressOutput": false,
    })
}

fn antigravity_stop_allow_output() -> Value {
    json!({
        "decision": "allow",
    })
}

fn print_antigravity_stop_allow_once(
    printed: &std::sync::atomic::AtomicBool,
    writer: &mut impl Write,
) -> bool {
    if printed.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    let _ = writeln!(writer, "{}", antigravity_stop_allow_output());
    let _ = writer.flush();
    true
}

fn spawn_detached_antigravity_hook(config_path: &Path, cascade_id: Option<&str>) -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    let mut cmd = ProcessCommand::new(exe);
    cmd.arg("--config")
        .arg(config_path)
        .arg("antigravity-hook")
        .arg("--detached")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cascade_id) = cascade_id.map(str::trim).filter(|value| !value.is_empty()) {
        cmd.arg("--cascade-id").arg(cascade_id);
    }
    cmd.spawn().is_ok()
}

#[cfg(test)]
async fn send_bark_notification(
    config: &Config,
    device_key: &[u8],
    request: &ProbeBarkRequest,
    timeout: Duration,
) -> Result<ProbeBarkOutcome> {
    let chunks = bark_body_chunks(&request.body, PROBE_BARK_BODY_CHUNK_BYTES);
    let identity = uuid::Uuid::new_v4().to_string();
    let mut outcome = ProbeBarkOutcome::default();
    for (index, chunk) in chunks.iter().enumerate() {
        outcome = bark::send_chunk(
            config,
            device_key,
            request,
            chunk,
            index,
            chunks.len(),
            &format!("{identity}-{index}"),
            1,
            timeout,
        )
        .await;
        outcome.request_count = index + 1;
        if !outcome.sent {
            break;
        }
    }
    Ok(outcome)
}

#[cfg(test)]
fn utf8_chunks(value: &str, max_bytes: usize) -> Vec<String> {
    if value.is_empty() || max_bytes == 0 {
        return vec![String::new()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < value.len() {
        let mut end = (start + max_bytes).min(value.len());
        while end > start && !value.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = value[start..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| start + offset)
                .unwrap_or(value.len());
        }
        chunks.push(value[start..end].to_string());
        start = end;
    }
    chunks
}

#[cfg(test)]
fn bark_body_chunks(value: &str, max_bytes: usize) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || max_bytes == 0 {
        return vec![String::new()];
    }
    let raw_chunks = utf8_chunks(trimmed, max_bytes);
    let chunk_count = raw_chunks.len();
    if chunk_count <= 1 {
        return raw_chunks;
    }
    raw_chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| format!("{}{chunk}", bark_chunk_prefix(index + 1, chunk_count)))
        .collect()
}

#[cfg(test)]
fn bark_chunk_prefix(index: usize, chunk_count: usize) -> String {
    format!("第 {index}/{chunk_count} 段\n\n")
}

async fn serve(config_path: PathBuf, host_surface: HostSurface) -> Result<()> {
    anyhow::ensure!(
        host_surface == HostSurface::LinuxServerApi && cfg!(target_os = "linux"),
        "Management API is available only on Linux"
    );
    let config = Config::load_runtime(&config_path)?;
    upgrade_managed_question_hooks_on_start(&config);
    let db = open_panel_db(&config)?;
    let state = AppState::new_for_surface(config.clone(), db, host_surface);
    spawn_probe_thread_scan(state.clone());
    spawn_probe_error_monitor(state.clone());
    github_ci_monitor::spawn(config_path.clone(), Some(state.clone()));
    api::spawn_probe_status_refresh(state.clone());
    ssh_server::serve(state).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ProbeErrorMonitorRunOutcome {
    baseline_only: bool,
    rows_seen: usize,
    new_incidents: usize,
    deliveries_completed: usize,
}

async fn run_probe_error_monitor_once(
    db: &PanelDb,
    load_config: &(dyn Fn() -> Result<Config> + Sync),
) -> Result<ProbeErrorMonitorRunOutcome> {
    let _guard = PROBE_ERROR_MONITOR_LOCK.lock().await;
    notification_delivery::retry_pending_with_config(db, load_config).await?;
    let config =
        load_config().map_err(|_| anyhow::anyhow!("notification configuration unavailable"))?;
    let config = &config;
    let shared_codex_threads =
        if async_question_monitor::is_enabled(config) || question_monitor::is_enabled(config) {
            let paths = resolve_codex_paths(&config.codex.home).codex_paths();
            agenthub_core::codex::notification_thread_headers(&paths)
                .ok()
                .map(|mut rows| {
                    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                    rows
                })
        } else {
            None
        };
    if let Err(error) =
        async_question_monitor::run_with_threads(load_config, db, shared_codex_threads.as_deref())
            .await
    {
        tracing::warn!(
            "Codex structured question monitor failed: {}",
            safe_probe_monitor_error(&error.to_string())
        );
    }
    if let Err(error) =
        question_monitor::run_with_threads(load_config, db, shared_codex_threads.as_deref()).await
    {
        tracing::warn!(
            "Codex feedback monitor failed: {}",
            safe_probe_monitor_error(&error.to_string())
        );
    }
    if let Err(error) = provider_monitor::run(load_config, db).await {
        tracing::warn!(
            "native provider notification monitor failed: {}",
            safe_probe_monitor_error(&error.to_string())
        );
    }
    if !config.probe.enabled || !config.probe.error_monitor.enabled {
        store_probe_error_monitor_status(
            db,
            ProbeErrorMonitorRuntimeStatus {
                status: "disabled".to_string(),
                last_scan_at: PanelDb::now(),
                last_error: None,
                rows_seen: 0,
                new_incidents: 0,
                incident_count: db.probe_error_incident_count()?,
                cursor: load_probe_error_cursor(db)?,
            },
        )?;
        return Ok(ProbeErrorMonitorRunOutcome {
            baseline_only: false,
            rows_seen: 0,
            new_incidents: 0,
            deliveries_completed: 0,
        });
    }

    let resolved = resolve_codex_paths(&config.codex.home);
    let previous = load_probe_error_cursor(db)?;
    let scan = match scan_codex_turn_errors(
        &resolved.logs_db,
        previous.as_ref(),
        PROBE_ERROR_MONITOR_BATCH_LIMIT,
    ) {
        Ok(scan) => scan,
        Err(err) => {
            let safe_error = safe_probe_monitor_error(&err.to_string());
            store_probe_error_monitor_status(
                db,
                ProbeErrorMonitorRuntimeStatus {
                    status: "error".to_string(),
                    last_scan_at: PanelDb::now(),
                    last_error: Some(safe_error),
                    rows_seen: 0,
                    new_incidents: 0,
                    incident_count: db.probe_error_incident_count()?,
                    cursor: previous,
                },
            )?;
            return Err(err);
        }
    };

    let mut claimed = Vec::new();
    for incident in &scan.incidents {
        if task_notification_suppression_reason(config, Some(&incident.thread_id)).is_some() {
            continue;
        }
        let record = NewProbeErrorIncident {
            incident_key: incident.incident_key.clone(),
            source_ts: incident.source_ts,
            source_ts_nanos: incident.source_ts_nanos,
            source_row_id: incident.source_row_id,
            thread_id: incident.thread_id.clone(),
            turn_id: incident.turn_id.clone(),
            classification: incident.classification.as_str().to_string(),
            error_sha256: incident.error_sha256.clone(),
            error_summary: incident.summary.clone(),
        };
        if db.claim_probe_error_incident(&record)? {
            claimed.push(incident.clone());
        }
    }

    let pending_deliveries = db.list_pending_probe_error_deliveries(100)?;
    let deliveries_completed =
        deliver_probe_error_incidents(load_config, db, &pending_deliveries).await?;
    if previous.as_ref() != Some(&scan.cursor) {
        db.set_setting(
            PROBE_ERROR_MONITOR_CURSOR_SETTING,
            &serde_json::to_string(&scan.cursor)?,
        )?;
    }
    store_probe_error_monitor_status(
        db,
        ProbeErrorMonitorRuntimeStatus {
            status: if scan.baseline_only { "baseline" } else { "ok" }.to_string(),
            last_scan_at: PanelDb::now(),
            last_error: None,
            rows_seen: scan.rows_seen,
            new_incidents: claimed.len(),
            incident_count: db.probe_error_incident_count()?,
            cursor: Some(scan.cursor.clone()),
        },
    )?;
    Ok(ProbeErrorMonitorRunOutcome {
        baseline_only: scan.baseline_only,
        rows_seen: scan.rows_seen,
        new_incidents: claimed.len(),
        deliveries_completed,
    })
}

async fn deliver_probe_error_incidents(
    load_config: &notification_delivery::ConfigLoader<'_>,
    db: &PanelDb,
    incidents: &[ProbeErrorIncident],
) -> Result<usize> {
    let config = &load_config()?;
    let mut delivered = 0usize;
    for incident in incidents {
        let title = local_thread_title(config, &incident.thread_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| "未命名线程".to_string());
        let mut diagnostics = BTreeMap::new();
        diagnostics.insert(
            "error_classification".to_string(),
            json!(incident.classification),
        );
        diagnostics.insert("error_sha256".to_string(), json!(incident.error_sha256));
        diagnostics.insert("source_log_ts".to_string(), json!(incident.source_ts));
        diagnostics.insert(
            "source_log_ts_nanos".to_string(),
            json!(incident.source_ts_nanos),
        );
        diagnostics.insert(
            "source_log_row_id".to_string(),
            json!(incident.source_row_id),
        );
        let event = probe_runtime(config).build_event(
            ProbeEventInput::hook_stop_with_context(
                Some(&incident.thread_id),
                Some(&incident.turn_id),
                Some(&incident.thread_id),
                None,
                Some(&incident.error_summary),
                "turn-error",
            )
            .with_thread_title(Some(&title))
            .with_body_source(Some("turn_error"))
            .with_body_selection_diagnostics(diagnostics)
            .with_error_monitor_source(),
        );
        let (outcome, bark) =
            record_probe_event_with_bark_timeout(load_config, db, event, Duration::from_secs(3))
                .await?;
        let event_id = db.probe_event_id_by_dedupe_key(&outcome.dedupe_key)?;
        let bark_status = if let Some(status) = bark.status.as_ref() {
            status.clone()
        } else if bark.sent {
            "sent".to_string()
        } else if bark.skipped {
            format!("skipped:{}", bark.reason.as_deref().unwrap_or("unknown"))
        } else {
            format!("failed:{}", bark.reason.as_deref().unwrap_or("unknown"))
        };
        db.update_probe_error_incident_delivery(
            &incident.incident_key,
            event_id.as_deref(),
            &bark_status,
        )?;
        delivered += 1;
    }
    Ok(delivered)
}

fn load_probe_error_cursor(db: &PanelDb) -> Result<Option<ProbeErrorCursor>> {
    db.get_setting(PROBE_ERROR_MONITOR_CURSOR_SETTING)?
        .map(|value| serde_json::from_str(&value).context("decode Probe error monitor cursor"))
        .transpose()
}

fn store_probe_error_monitor_status(
    db: &PanelDb,
    status: ProbeErrorMonitorRuntimeStatus,
) -> Result<()> {
    if status.status == "disabled" {
        if let Some(raw) = db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)? {
            if let Ok(mut previous) = serde_json::from_str::<ProbeErrorMonitorRuntimeStatus>(&raw) {
                previous.last_scan_at = status.last_scan_at;
                if previous == status {
                    return Ok(());
                }
            }
        }
    }
    db.set_setting(
        PROBE_ERROR_MONITOR_STATUS_SETTING,
        &serde_json::to_string(&status)?,
    )
}

fn safe_probe_monitor_error(value: &str) -> String {
    let redacted = agenthub_core::security::redact_output(value.trim());
    let mut end = redacted.len().min(512);
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    if end < redacted.len() {
        format!("{} [truncated]", &redacted[..end])
    } else {
        redacted
    }
}

fn open_panel_db(config: &Config) -> Result<PanelDb> {
    PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box()?)
}

fn spawn_probe_thread_scan(state: AppState) {
    tokio::spawn(async move {
        loop {
            match run_probe_thread_scan_if_due(state.clone()).await {
                Ok(count) if count > 0 => {
                    tracing::info!(count, "probe thread scan notifications recorded");
                }
                Ok(_) => {
                    tracing::debug!("probe thread scan skipped");
                }
                Err(err) => {
                    tracing::warn!("probe thread scan failed: {err}");
                }
            }
            time::sleep(std::time::Duration::from_secs(
                PROBE_THREAD_SCAN_TICK_SECONDS,
            ))
            .await;
        }
    });
}

async fn run_probe_thread_scan_if_due(state: AppState) -> Result<usize> {
    let _guard = PROBE_THREAD_SCAN_LOCK.lock().await;
    let config = state.config();
    if !config.probe.enabled || !config.probe.notifications.enabled {
        return Ok(0);
    }
    let mut recorded = 0usize;
    let mut active_markers = Vec::new();
    let codex_paths = state.codex_paths();
    let buckets = probe_service::probe_passive_scan_threads_with_paths(
        &codex_paths,
        state.db.path(),
        config.probe.recent_limit,
    )?;
    for (status, threads) in buckets {
        for thread in threads {
            if task_notification_suppression_reason(&config, Some(&thread.id)).is_some() {
                continue;
            }
            let plan = probe_service::probe_passive_thread_notification_plan(&thread, status);
            if !plan.fresh {
                continue;
            }
            let transcript_path = thread
                .rollout_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string());
            let mut event = probe_runtime(&config).build_event(
                ProbeEventInput::hook_stop_with_context(
                    Some(thread.id.as_str()),
                    thread.active_turn_id.as_deref(),
                    Some(thread.id.as_str()),
                    transcript_path.as_deref(),
                    plan.body.as_deref(),
                    status,
                )
                .with_thread_title(Some(thread.title.as_str()))
                .with_body_source(plan.body_source.as_deref())
                .with_passive_scan_source(),
            );
            event.payload["notification_source_ms"] = json!(thread
                .updated_at
                .as_deref()
                .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.timestamp_millis())
                .unwrap_or(0));
            event.payload["thread_title"] = json!(thread.title.clone());
            event.payload["thread_id"] = json!(thread.id.clone());
            if let Some(active_turn_id) = thread.active_turn_id.as_deref() {
                event.payload["turn_id"] = json!(active_turn_id);
            }
            if let Some(elicitation) = &thread.pending_elicitation {
                if let Some(item_id) = elicitation.item_id.as_deref() {
                    event.payload["item_id"] = json!(item_id);
                    event.payload["call_id"] = json!(item_id);
                }
            }
            if let Some(reason_label) = plan.reason_label.as_deref() {
                event.payload["reason_label"] = json!(reason_label);
            }
            let pre_plan = probe_service::probe_event_record_plan(event.clone());
            if let Some(key) = pre_plan.passive_marker_key {
                active_markers.push(key);
            }
            let (outcome, bark) =
                record_probe_event_with_bark(&|| Ok(state.config()), &state.db, event).await?;
            if outcome.recorded || bark.sent {
                recorded += 1;
            }
        }
    }
    let _ = state
        .db
        .retain_active_unresolved_markers("thread_scan", &active_markers);
    Ok(recorded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::time::SystemTime;

    #[test]
    fn disabled_error_monitor_retains_timestamp_until_status_changes() {
        let db = PanelDb::open(":memory:").unwrap();
        let mut status = ProbeErrorMonitorRuntimeStatus {
            status: "disabled".into(),
            last_scan_at: 1,
            last_error: None,
            rows_seen: 0,
            new_incidents: 0,
            incident_count: 0,
            cursor: None,
        };
        store_probe_error_monitor_status(&db, status.clone()).unwrap();
        status.last_scan_at = 2;
        store_probe_error_monitor_status(&db, status.clone()).unwrap();
        let saved: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
            &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.last_scan_at, 1);
        status.incident_count = 1;
        store_probe_error_monitor_status(&db, status.clone()).unwrap();
        let saved: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
            &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved, status);
        status.status = "watching".into();
        for timestamp in [3, 4] {
            status.last_scan_at = timestamp;
            store_probe_error_monitor_status(&db, status.clone()).unwrap();
            let saved: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
                &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(saved, status);
        }
    }

    #[tokio::test]
    async fn unsupported_probe_cli_actions_do_not_report_fake_success() {
        let (config, sandbox_root) = crate::api::test_support::sandboxed_test_config();

        for command in [ProbeCommand::LifecycleRepair, ProbeCommand::ServiceRestart] {
            let db = PanelDb::open(":memory:").unwrap();
            let err = run_probe_command(command, &config, db, Path::new("fixture-unused.toml"))
                .await
                .unwrap_err();
            let message = format!("{err:#}");
            assert!(message.contains("unsupported"));
            assert!(!message.contains("\"ok\": true"));
        }
        let _ = fs::remove_dir_all(sandbox_root);
    }

    #[test]
    fn hook_request_user_input_cli_subcommand_is_internal_and_argument_free() {
        assert!(Cli::try_parse_from(["agenthub-webd", "probe", "hook-request-user-input"]).is_ok());
        assert!(
            Cli::try_parse_from(["agenthub-webd", "probe", "hook-request-user-input-confirm"])
                .is_ok()
        );
        assert!(Cli::try_parse_from([
            "agenthub-webd",
            "probe",
            "hook-request-user-input",
            "--thread-id",
            "thread-a"
        ])
        .is_err());
    }

    #[test]
    fn legacy_import_is_hidden_and_retired() {
        use clap::CommandFactory;
        let mut command = Cli::command();
        let mut help = Vec::new();
        command
            .find_subcommand_mut("probe")
            .unwrap()
            .write_long_help(&mut help)
            .unwrap();
        assert!(!String::from_utf8(help).unwrap().contains("legacy-import"));
    }

    #[test]
    fn codex_config_patch_enables_features_hooks_and_preserves_existing_values() {
        let updated = ensure_codex_hooks_feature(
            r#"
model = "gpt-5"

[features]
foo = true
hooks = false
"#,
        )
        .unwrap();

        assert!(updated.contains("hooks = true"));
        assert!(updated.contains("foo = true"));
        assert!(updated.contains("model = \"gpt-5\""));
    }

    #[test]
    fn codex_config_patch_creates_features_table_when_missing() {
        let updated = ensure_codex_hooks_feature("model = \"gpt-5\"\n").unwrap();

        assert!(updated.contains("[features]"));
        assert!(updated.contains("hooks = true"));
    }

    async fn install_probe_hooks_for_test(config: &Config, dry_run: bool) -> Result<Value> {
        install_probe_hooks_with_repair(config, dry_run, |platform, enabled| {
            Ok(json!({
                "supported": true,
                "enabled": enabled,
                "loaded": enabled,
                "changed": false,
                "label": agenthub_core::probe_error_monitor::PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL,
                "plist_path": Value::Null,
                "helper_path": platform.daemon_binary(),
                "config_path": &platform.config_file,
            }))
        })
        .await
    }

    #[tokio::test]
    async fn install_probe_hooks_writes_hooks_json_and_codex_features_hooks() {
        let dir = temp_test_dir("agenthub-hooks-install");
        let codex_home = dir.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::write(codex_home.join("config.toml"), "model = \"gpt-5\"\n").unwrap();
        let config = test_config(codex_home.clone());

        let result = install_probe_hooks_for_test(&config, false).await.unwrap();

        assert_eq!(result["ok"], true);
        assert_eq!(result["hooks_json_changed"], true);
        assert_eq!(result["codex_config_changed"], true);
        assert!(result["reload_result"].is_null());
        let hooks_json: Value =
            serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
        assert!(hooks_json.to_string().contains("probe hook-stop"));
        let pre_tool_use = hooks_json["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool_use.len(), 1);
        assert_eq!(
            pre_tool_use[0]["matcher"],
            "^(functions\\.)?request_user_input(_async)?$"
        );
        let request_hook = &pre_tool_use[0]["hooks"][0];
        assert!(request_hook["command"]
            .as_str()
            .unwrap()
            .contains("probe hook-request-user-input"));
        assert_eq!(request_hook["timeout"], 5);
        assert!(request_hook.get("async").is_none());
        let codex_config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(codex_config.contains("[features]"));
        assert!(codex_config.contains("hooks = true"));

        let second = install_probe_hooks_for_test(&config, false).await.unwrap();
        assert_eq!(second["changed"], false);
        assert_eq!(
            fs::read(codex_home.join("hooks.json")).unwrap(),
            serde_json::to_vec_pretty(&hooks_json).unwrap()
        );

        let third_party = "/usr/local/bin/custom-codex-hook --event stop";
        let mut with_custom = hooks_json.clone();
        with_custom["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "matcher": "*",
                "hooks": [{"type": "command", "command": third_party}]
            }));
        fs::write(
            codex_home.join("hooks.json"),
            serde_json::to_vec_pretty(&with_custom).unwrap(),
        )
        .unwrap();
        let probe_cfg_path = dir.join("config.toml");
        fs::write(&probe_cfg_path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let removed = remove_probe_hooks(&config, Some(&probe_cfg_path), false)
            .await
            .unwrap();
        assert_eq!(removed["ok"], true);
        assert_eq!(removed["hooks_json_changed"], true);
        assert_eq!(removed["probe_config_changed"], true);
        let after_remove: Value =
            serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
        assert!(!after_remove.to_string().contains("probe hook-stop"));
        assert!(!after_remove
            .to_string()
            .contains("probe hook-request-user-input"));
        assert!(after_remove.to_string().contains(third_party));
        let reloaded = Config::load_runtime(&probe_cfg_path).unwrap();
        assert!(!reloaded.probe.hooks.manage_stop_hook);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn install_probe_hooks_replaces_stale_agenthub_hooks_and_hook_state() {
        let dir = temp_test_dir("agenthub-hooks-replace-stale");
        let codex_home = dir.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        fs::write(
            codex_home.join("hooks.json"),
            serde_json::to_vec_pretty(&json!({
                "hooks": {
                    "Stop": [
                        {"hooks": []},
                        {
                            "matcher": "*",
                            "hooks": [{
                                "type": "command",
                                "command": "/opt/agenthub/bin/agenthubd --config /opt/agenthub/config.toml probe hook-stop"
                            }]
                        },
                        {
                            "matcher": "*",
                            "hooks": [{
                                "type": "command",
                                "command": "/usr/local/bin/third-party-hook"
                            }]
                        }
                    ],
                    "PreToolUse": [
                        {
                            "matcher": "^request_user_input$",
                            "hooks": [{
                                "type": "command",
                                "command": "/opt/agenthub/bin/agenthubd --config /opt/agenthub/config.toml probe hook-request-user-input",
                                "timeout": 9,
                                "async": true
                            }]
                        },
                        {
                            "matcher": "^request_user_input$",
                            "hooks": [{
                                "type": "command",
                                "command": "/usr/local/bin/third-party-request-hook"
                            }]
                        }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            codex_home.join("config.toml"),
            r#"
model = "gpt-5"

[features]
hooks = true

[hooks.state."/tmp/.codex/hooks.json:stop:0:0"]
last_error = "old empty group"

[hooks.state."/tmp/.codex/hooks.json:stop:1:0"]
last_error = "old agenthub hook"

[hooks.state."/tmp/.codex/hooks.json:pre_tool_use:0:0"]
last_error = "old agenthub request hook"
"#,
        )
        .unwrap();
        let config = test_config(codex_home.clone());

        let result = install_probe_hooks_for_test(&config, false).await.unwrap();

        assert_eq!(result["hooks_json_changed"], true);
        assert_eq!(result["codex_config_changed"], true);
        let hooks_json: Value =
            serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
        let stop = hooks_json["hooks"]["Stop"].as_array().unwrap();
        let commands = stop
            .iter()
            .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
            .filter_map(|hook| hook["command"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.contains("probe hook-stop"))
                .count(),
            1
        );
        assert!(commands
            .iter()
            .any(|command| command.contains("agenthub-webd")));
        assert!(!commands.iter().any(|command| command.contains("agenthubd")));
        assert!(commands.contains(&"/usr/local/bin/third-party-hook"));
        assert!(stop.iter().all(|group| group["hooks"]
            .as_array()
            .is_some_and(|hooks| !hooks.is_empty())));
        let pre_tool_use = hooks_json["hooks"]["PreToolUse"].as_array().unwrap();
        let request_commands = pre_tool_use
            .iter()
            .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
            .filter_map(|hook| hook["command"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            request_commands
                .iter()
                .filter(|command| command.contains("probe hook-request-user-input"))
                .count(),
            1
        );
        assert!(request_commands
            .iter()
            .any(|command| command.contains("agenthub-webd")));
        assert!(request_commands.contains(&"/usr/local/bin/third-party-request-hook"));
        let managed_request_hook = pre_tool_use
            .iter()
            .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
            .find(|hook| {
                hook["command"]
                    .as_str()
                    .is_some_and(|command| command.contains("agenthub-webd"))
            })
            .unwrap();
        assert_eq!(managed_request_hook["timeout"], 5);
        assert!(managed_request_hook.get("async").is_none());
        let codex_config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(!codex_config.contains("[hooks.state."));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn hook_stop_records_probe_event_but_returns_codex_stop_json_and_redacted_bark_state() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-a"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_completion = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let db = PanelDb::open(":memory:").unwrap();

        let event = probe_runtime(&config).build_event(ProbeEventInput::hook_stop(
            Some("thread-a"),
            Some("turn-1"),
            "hook-stop",
        ));
        let result = handle_built_probe_event(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();

        assert_eq!(
            result.stdout,
            json!({"continue": true, "suppressOutput": false})
        );
        assert!(result.outcome.recorded);
        assert!(!result.bark.sent);
        assert!(result.bark.skipped);
        assert_eq!(result.bark.reason.as_deref(), Some("device_key_missing"));
        assert!(!result.bark.device_key_configured);
        let output = serde_json::to_string(&result).unwrap();
        assert!(!output.contains("device-key"));
        assert!(!output.contains("secret"));

        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "hook-stop");
        assert_eq!(events[0].payload["bark"]["reason"], "device_key_missing");
        assert!(events[0].payload["bark"].get("device_key").is_none());
    }

    #[tokio::test]
    async fn probe_control_payload_suppression_skips_dedupe_db_and_bark() {
        let dir = temp_test_dir("agenthub-control-payload-suppression");
        fs::create_dir_all(&dir).unwrap();
        let db = PanelDb::open(dir.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-extra-key"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_completion = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();

        for (index, body) in [r#"{"suggestions":[]}"#, r#"{"exclude":[]}"#]
            .into_iter()
            .enumerate()
        {
            let event =
                probe_runtime(&config).build_event(ProbeEventInput::hook_stop_with_context(
                    Some(&format!("thread-control-{index}")),
                    Some(&format!("turn-control-{index}")),
                    Some(&format!("thread-control-{index}")),
                    None,
                    Some(body),
                    "hook-stop",
                ));
            let (outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
                .await
                .unwrap();

            assert!(!outcome.recorded);
            assert!(!outcome.duplicate);
            assert!(bark.skipped);
            assert_eq!(bark.reason.as_deref(), Some("internal_control_payload"));
            assert_eq!(bark.request_count, 0);
        }

        let conn = Connection::open(db.path()).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM probe_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );

        let normal_event =
            probe_runtime(&config).build_event(ProbeEventInput::hook_stop_with_context(
                Some("thread-extra-key"),
                Some("turn-extra-key"),
                Some("thread-extra-key"),
                None,
                Some(r#"{"suggestions":[],"message":"normal JSON"}"#),
                "hook-stop",
            ));
        let (normal_outcome, normal_bark) =
            record_probe_event_with_bark(&|| Ok(config.clone()), &db, normal_event)
                .await
                .unwrap();
        assert!(normal_outcome.recorded);
        assert!(!normal_outcome.duplicate);
        assert_eq!(normal_bark.request_count, 1);
        assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn hook_stop_suppresses_internal_memory_consolidation_before_body_dedupe_db_and_bark() {
        let dir = temp_test_dir("agenthub-memory-hook-stop-suppression");
        let codex_home = dir.join("custom-codex-home");
        let memory_root = codex_home.join("memories");
        let normalized_cwd = memory_root.join("nested").join("..");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::create_dir_all(memory_root.join("nested")).unwrap();
        let db = PanelDb::open(dir.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let mut config = test_config(codex_home);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_completion = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let memory_body = "MEMORY_CONSOLIDATION_BODY_MUST_NOT_BE_READ_OR_STORED";

        let event_input = hook_stop_event_input(
            &config,
            Some(&json!({
                "cwd": normalized_cwd,
                "hook_event_name": "Stop",
                "ephemeral": true,
                "session_id": "00000000-0000-4000-8000-000000000106",
                "turn_id": "turn-memory",
                "transcript_path": null,
                "last_assistant_message": memory_body,
            })),
            None,
            None,
            "hook-stop",
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(event_input);

        assert_eq!(
            event.suppression_reason.as_deref(),
            Some("internal_memory_consolidation")
        );
        assert!(event.payload["last_assistant_message"].is_null());
        assert!(!event.bark_body.contains(memory_body));

        let result = handle_built_probe_event(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();
        assert_eq!(
            result.stdout,
            json!({"continue": true, "suppressOutput": false})
        );
        assert!(!result.outcome.recorded);
        assert!(!result.outcome.duplicate);
        assert_eq!(
            result.bark.reason.as_deref(),
            Some("internal_memory_consolidation")
        );
        assert_eq!(result.bark.request_count, 0);

        let conn = Connection::open(db.path()).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM probe_events", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hook_stop_memory_identity_is_exact_and_fails_open_for_counterexamples() {
        let dir = temp_test_dir("agenthub-memory-hook-stop-counterexamples");
        let codex_home = dir.join(".codex");
        let memory_root = codex_home.join("memories");
        let ordinary_cwd = dir.join("workspace");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::create_dir_all(&memory_root).unwrap();
        fs::create_dir_all(&ordinary_cwd).unwrap();
        let transcript = dir.join("rollout.jsonl");
        fs::write(
            &transcript,
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "task_complete",
                    "turn_id": "turn-user",
                    "last_agent_message": "用户线程完成"
                }
            })
            .to_string(),
        )
        .unwrap();
        let config = test_config(codex_home);

        let payloads = [
            json!({
                "cwd": ordinary_cwd,
                "session_id": "ordinary-empty-transcript",
                "turn_id": "turn-user",
                "transcript_path": null,
                "last_assistant_message": "正文提及 MEMORY.md、memory_summary.md 和记忆整理"
            }),
            json!({
                "cwd": memory_root,
                "session_id": "memory-with-transcript",
                "turn_id": "turn-user",
                "transcript_path": transcript,
                "last_assistant_message": "显式用户线程"
            }),
            json!({
                "session_id": "missing-cwd",
                "turn_id": "turn-user",
                "transcript_path": null,
                "last_assistant_message": "普通用户线程"
            }),
            json!({
                "cwd": dir.join("does-not-exist"),
                "session_id": "invalid-cwd",
                "turn_id": "turn-user",
                "transcript_path": null,
                "last_assistant_message": "普通用户线程"
            }),
            json!({
                "cwd": memory_root,
                "session_id": "invalid-transcript-field",
                "turn_id": "turn-user",
                "transcript_path": 42,
                "last_assistant_message": "普通用户线程"
            }),
        ];

        for payload in payloads {
            let event_input =
                hook_stop_event_input(&config, Some(&payload), None, None, "hook-stop").unwrap();
            let event = probe_runtime(&config).build_event(event_input);
            assert_ne!(
                event.suppression_reason.as_deref(),
                Some("internal_memory_consolidation"),
                "payload should fail open: {payload}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hook_request_user_input_memory_context_with_transcript_or_invalid_field_fails_open() {
        let dir = temp_test_dir("agenthub-memory-pre-tool-use-counterexamples");
        let codex_home = dir.join(".codex");
        let memory_root = codex_home.join("memories");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::create_dir_all(&memory_root).unwrap();
        let config = test_config(codex_home);
        notification_accuracy_tests::seed_main_task_identities(
            &config.codex.home,
            &["thread-user-question"],
        );

        for transcript_path in [json!("/tmp/user-rollout.jsonl"), json!(42)] {
            let event_input = hook_request_user_input_event_input(
                &config,
                json!({
                    "cwd": memory_root,
                    "hook_event_name": "PreToolUse",
                    "session_id": "thread-user-question",
                    "turn_id": "turn-user-question",
                    "tool_name": "request_user_input",
                    "tool_use_id": "call-user-question",
                    "tool_input": {
                        "questions": [{
                            "id": "mode",
                            "question": "Choose a mode?",
                            "options": [{"label": "Continue"}]
                        }]
                    },
                    "transcript_path": transcript_path
                }),
            )
            .unwrap();
            let event = probe_runtime(&config).build_event(event_input);
            assert_ne!(
                event.suppression_reason.as_deref(),
                Some("internal_memory_consolidation")
            );
            assert!(event.bark_body.contains("Choose a mode?"));
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn hook_request_user_input_suppresses_memory_context_without_db_dedupe_or_bark() {
        let dir = temp_test_dir("agenthub-memory-pre-tool-use-suppression");
        let codex_home = dir.join(".codex");
        let memory_root = codex_home.join("memories");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::create_dir_all(&memory_root).unwrap();
        let db = PanelDb::open(dir.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let mut config = test_config(codex_home);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let event_input = hook_request_user_input_event_input(
            &config,
            json!({
                "cwd": memory_root,
                "hook_event_name": "PreToolUse",
                "session_id": "thread-memory-question",
                "turn_id": "turn-memory-question",
                "tool_name": "request_user_input",
                "tool_use_id": "call-memory-question",
                "tool_input": {
                    "questions": [{
                        "id": "memory",
                        "question": "Internal memory question?",
                        "options": [{"label": "Continue"}]
                    }]
                },
                "transcript_path": null
            }),
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(event_input);

        assert_eq!(
            event.suppression_reason.as_deref(),
            Some("internal_memory_consolidation")
        );
        let (outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();
        assert!(!outcome.recorded);
        assert!(!outcome.duplicate);
        assert_eq!(
            bark.reason.as_deref(),
            Some("internal_memory_consolidation")
        );
        assert_eq!(bark.request_count, 0);
        assert!(db.list_probe_events(10).unwrap().is_empty());
        let conn = Connection::open(db.path()).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn hook_stop_records_stdin_compatible_context_fields() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["session-stdin"]);
        config.probe.notifications.enabled = false;
        let db = PanelDb::open(":memory:").unwrap();
        let event = probe_runtime(&config).build_event(ProbeEventInput::hook_stop_with_context(
            None,
            Some("turn-stdin"),
            Some("session-stdin"),
            Some("/tmp/transcript.jsonl"),
            Some("assistant body"),
            "hook-stop",
        ));

        let result = handle_built_probe_event(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();

        assert_eq!(
            result.stdout,
            json!({"continue": true, "suppressOutput": false})
        );
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].thread_id.as_deref(), Some("session-stdin"));
        assert_eq!(events[0].payload["session_id"], "session-stdin");
        assert_eq!(events[0].payload["turn_id"], "turn-stdin");
        assert_eq!(
            events[0].payload["transcript_path"],
            "/tmp/transcript.jsonl"
        );
        assert_eq!(
            events[0].payload["last_assistant_message"]["summary"],
            "assistant body"
        );
        assert_eq!(
            events[0].payload["last_assistant_message"]["classification"],
            "completion"
        );
        assert_eq!(events[0].payload["event_type"], "completion");
        assert_eq!(events[0].payload["raw_kind"], "hook-stop");
        assert_eq!(events[0].payload["dedupe"]["namespace"], "probe_event");
        assert_eq!(events[0].payload["dedupe"]["claimed"], true);
        assert_eq!(events[0].payload["dedupe"]["duplicate"], false);
        assert_eq!(events[0].kind, "completion");
    }

    #[tokio::test]
    async fn hook_stop_uses_transcript_latest_assistant_when_stdin_omits_body() {
        let (mut config, _identity) = notification_accuracy_tests::test_notification_config(&[
            "thread-transcript",
            "session-transcript",
        ]);
        config.probe.notifications.enabled = false;
        let db = PanelDb::open(":memory:").unwrap();
        let dir = temp_test_dir("agenthub-hook-transcript-summary");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        fs::write(
            &transcript,
            [
                json!({"type":"response_item","turn_id":"turn-transcript","payload":{"type":"message","role":"assistant","content":[{"text":"first answer"}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-transcript","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"text":"final answer"}]}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let selection = hook_stop_last_assistant_message(
            Some(transcript.to_string_lossy().as_ref()),
            Some("turn-transcript"),
            None,
        )
        .unwrap()
        .unwrap();
        let event = probe_runtime(&config).build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-transcript"),
                Some("turn-transcript"),
                Some("session-transcript"),
                Some(transcript.to_string_lossy().as_ref()),
                Some(&selection.message),
                "hook-stop",
            )
            .with_body_source(Some(&selection.source)),
        );

        handle_built_probe_event(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();

        let events = db.list_probe_events(10).unwrap();
        assert_eq!(
            events[0].payload["last_assistant_message"]["summary"],
            "final answer"
        );
        assert_eq!(
            events[0].payload["last_assistant_message"]["classification"],
            "completion"
        );
        assert_eq!(events[0].payload["body_summary"], "final answer");
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn hook_stop_uses_local_thread_title_for_bark_title_when_payload_omits_title() {
        let db = PanelDb::open(":memory:").unwrap();
        let dir = temp_test_dir("agenthub-hook-local-title");
        let codex_home = dir.join(".codex");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        let transcript = codex_home.join("sessions").join("rollout-title.jsonl");
        fs::write(
            &transcript,
            json!({"type":"response_item","turn_id":"turn-title","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"text":"完整最终回复"}]}})
                .to_string(),
        )
        .unwrap();
        let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT,
                source TEXT,
                thread_source TEXT,
                updated_at INTEGER,
                rollout_path TEXT
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
            params![
                "thread-title",
                "真实 HookStop 标题",
                chrono::Utc::now().timestamp_millis(),
                transcript.to_string_lossy().as_ref()
            ],
        )
        .unwrap();
        fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
        let mut config = test_config(codex_home);
        config.probe.notifications.enabled = false;

        let selection = hook_stop_last_assistant_message(
            Some(transcript.to_string_lossy().as_ref()),
            Some("turn-title"),
            None,
        )
        .unwrap()
        .unwrap();
        let event_input = ProbeEventInput::hook_stop_with_context(
            Some("thread-title"),
            Some("turn-title"),
            Some("thread-title"),
            Some(transcript.to_string_lossy().as_ref()),
            Some(&selection.message),
            "hook-stop",
        )
        .with_body_source(Some(&selection.source))
        .with_thread_title(
            local_thread_title(&config, "thread-title")
                .unwrap()
                .as_deref(),
        );
        let event = probe_runtime(&config).build_event(event_input);

        handle_built_probe_event(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();

        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events[0].title.as_deref(), Some("真实 HookStop 标题"));
        assert_eq!(events[0].payload["thread_title"], "真实 HookStop 标题");
        assert_eq!(
            events[0].payload["bark"]["title"],
            "线程正常完成：真实 HookStop 标题"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn hook_stop_dedupe_skips_duplicate_bark_without_leaking_device_key() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-a"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();

        let first = handle_built_probe_event(
            &|| Ok(config.clone()),
            &db,
            probe_runtime(&config).build_event(ProbeEventInput::hook_stop(
                Some("thread-a"),
                Some("turn-1"),
                "hook-stop",
            )),
        )
        .await
        .unwrap();
        let duplicate = handle_built_probe_event(
            &|| Ok(config.clone()),
            &db,
            probe_runtime(&config).build_event(ProbeEventInput::hook_stop(
                Some("thread-a"),
                Some("turn-1"),
                "hook-stop",
            )),
        )
        .await
        .unwrap();

        assert_eq!(first.bark.reason.as_deref(), Some("connection"));
        assert_eq!(
            duplicate.stdout,
            json!({"continue": true, "suppressOutput": false})
        );
        assert!(!duplicate.outcome.recorded);
        assert!(!duplicate.bark.sent);
        assert!(duplicate.bark.skipped);
        assert_eq!(duplicate.bark.reason.as_deref(), Some("dedupe"));
        assert!(duplicate.bark.device_key_configured);
        assert!(duplicate.bark.dedupe_hit);
        assert!(!serde_json::to_string(&duplicate)
            .unwrap()
            .contains("super-secret-device"));

        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["bark"]["device_key_configured"], true);
        assert!(events[0].payload["bark"].get("device_key").is_none());
    }

    #[tokio::test]
    async fn passive_reply_needed_scan_uses_sent_marker_while_hook_window_stays_short() {
        let (mut config, _identity) = notification_accuracy_tests::test_notification_config(&[
            "thread-passive",
            "thread-hook",
        ]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();

        let passive_input = || {
            ProbeEventInput::hook_stop_with_context(
                Some("thread-passive"),
                Some("turn-passive"),
                Some("thread-passive"),
                None,
                Some("等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-passive\nTurn ID：turn-passive\n时间：2026-06-16 12:00:00 北京时间\n状态说明：这一轮正在等待用户选择，不是异常停止。\n\n待选择内容：\n问题 1：继续吗？\n选项 1：继续"),
                "reply-needed",
            )
            .with_body_source(Some("request_user_input"))
            .with_passive_scan_source()
        };

        let first = record_probe_event_with_bark(
            &|| Ok(config.clone()),
            &db,
            probe_runtime(&config).build_event(passive_input()),
        )
        .await
        .unwrap();
        let duplicate = record_probe_event_with_bark(
            &|| Ok(config.clone()),
            &db,
            probe_runtime(&config).build_event(passive_input()),
        )
        .await
        .unwrap();

        assert!(first.0.recorded);
        assert!(!duplicate.0.recorded);
        assert_eq!(duplicate.1.reason.as_deref(), Some("sent_marker"));
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["dedupe_ttl_seconds"], 6 * 60 * 60);
        assert_eq!(events[0].payload["scan_source"], "passive-scan");

        let hook = probe_runtime(&config).build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-hook"),
            Some("turn-hook"),
            Some("thread-hook"),
            None,
            Some("等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-hook\nTurn ID：turn-hook"),
            "reply-needed",
        ));

        assert_eq!(
            hook.ttl_seconds,
            agenthub_core::probe::PROBE_EVENT_TTL_SECONDS
        );
        assert_eq!(
            hook.payload["dedupe_ttl_seconds"],
            agenthub_core::probe::PROBE_EVENT_TTL_SECONDS
        );
        assert_eq!(hook.payload["scan_source"], "hook-stop");
    }

    #[tokio::test]
    async fn notify_completion_uses_completion_bark_switch() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-a"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_completion = false;
        let db = PanelDb::open(":memory:").unwrap();
        let event = probe_runtime(&config).build_event(ProbeEventInput::notify_completion(
            Some("thread-a"),
            Some("turn-a"),
        ));

        let (_outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();

        assert!(bark.skipped);
        assert_eq!(bark.reason.as_deref(), Some("event_switch_disabled"));
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events[0].kind, "completion");
        assert_eq!(events[0].payload["bark"]["relevant_switch_enabled"], false);
    }

    #[tokio::test]
    async fn notify_completion_uses_complete_last_agent_message_from_rollout_without_storing_body()
    {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-complete"]);
        config.probe.notifications.enabled = false;
        let db = PanelDb::open(":memory:").unwrap();
        let dir = temp_test_dir("agenthub-notify-completion-rollout");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        let final_message = format!(
            "最终反馈第一行\n{}\nAuthorization: Bearer secret-token\n末尾唯一完整反馈",
            "完整正文".repeat(900)
        );
        fs::write(
            &transcript,
            [
                json!({"type":"response_item","turn_id":"turn-complete","payload":{"type":"message","role":"assistant","content":[{"text":"short assistant fallback"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-complete","last_agent_message":final_message}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let context = notify_completion_context(
            &config,
            Some(&json!({
                "thread_id": "thread-complete",
                "turn_id": "turn-complete",
                "transcript_path": transcript,
            })),
            None,
            None,
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(context);
        let (_outcome, _bark) =
            record_probe_event_with_bark(&|| Ok(config.clone()), &db, event.clone())
                .await
                .unwrap();

        assert_eq!(event.kind, "completion");
        assert!(event.bark_body.contains("最终反馈第一行"));
        assert!(event.bark_body.contains("完整正文完整正文"));
        assert!(event.bark_body.contains("末尾唯一完整反馈"));
        assert!(!event.bark_body.contains("secret-token"));
        assert!(!event.bark_body.contains("[truncated]"));
        assert!(!event.bark_body.contains("最后反馈："));
        assert!(event.payload["bark"].get("body").is_none());
        assert_eq!(
            event.payload["body_source"],
            "task_complete.last_agent_message"
        );
        let stored = db.list_probe_events(10).unwrap();
        let stored_json = serde_json::to_string(&stored[0]).unwrap();
        assert!(!stored_json.contains("末尾唯一完整反馈"));
        assert!(!stored_json.contains("secret-token"));
        assert_eq!(
            stored[0].payload["body_summary"],
            event.payload["body_summary"]
        );
        assert_eq!(
            stored[0].payload["body_sha256"],
            event.payload["body_sha256"]
        );
        assert!(stored[0].payload["bark"].get("body").is_none());

        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn notify_completion_prefers_transcript_final_message_over_stale_stdin_message() {
        let (config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-complete"]);
        let dir = temp_test_dir("agenthub-notify-completion-stale-stdin");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        fs::write(
            &transcript,
            [
                json!({"type":"response_item","turn_id":"turn-complete","payload":{"type":"message","role":"assistant","content":[{"text":"上一条 assistant fallback"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-complete","last_agent_message":"最后一条 final message"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let context = notify_completion_context(
            &config,
            Some(&json!({
                "thread_id": "thread-complete",
                "turn_id": "turn-complete",
                "transcript_path": transcript,
                "last_assistant_message": "上一条旧消息"
            })),
            None,
            None,
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(context);

        assert!(event.bark_body.contains("最后一条 final message"));
        assert!(!event.bark_body.contains("上一条旧消息"));
        assert_eq!(
            event.payload["body_source"],
            "task_complete.last_agent_message"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn notify_completion_waits_for_later_final_message_and_records_selection_diagnostics() {
        let (config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-live"]);
        let dir = temp_test_dir("agenthub-notify-completion-waits-final");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        fs::write(
            &transcript,
            [
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"上一轮完成正文"}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}).to_string(),
                json!({"type":"turn_context","payload":{"turn_id":"turn-live","summary":"auto","cwd":"/tmp"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let transcript_for_writer = transcript.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            let final_message = "已接着完成。\n最终完成正文唯一标记";
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&transcript_for_writer)
                .unwrap();
            writeln!(
                file,
                "{}",
                json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":final_message}],"phase":"final_answer"}})
            )
            .unwrap();
            writeln!(
                file,
                "{}",
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-live","last_agent_message":final_message}})
            )
            .unwrap();
        });

        let context = notify_completion_context(
            &config,
            Some(&json!({
                "thread_id": "thread-live",
                "turn_id": "turn-live",
                "transcript_path": transcript,
                "last_assistant_message": "过早 stdin 正文"
            })),
            None,
            None,
        )
        .unwrap();
        writer.join().unwrap();
        let event = probe_runtime(&config).build_event(context);

        assert!(event.bark_body.contains("最终完成正文唯一标记"));
        assert!(!event.bark_body.contains("过早 stdin 正文"));
        assert!(!event.bark_body.contains("上一轮完成正文"));
        assert_eq!(
            event.payload["body_source"],
            "task_complete.last_agent_message"
        );
        assert_eq!(event.payload["body_selected_turn_id"], "turn-live");
        assert!(event.payload["body_selected_line"].as_u64().unwrap() >= 4);
        assert_eq!(event.payload["transcript_stabilized"], true);
        assert!(
            event.payload["transcript_size_after"].as_u64().unwrap()
                > event.payload["transcript_size_before"].as_u64().unwrap()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn notify_completion_uses_full_raw_assistant_message_when_task_complete_body_is_missing()
    {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-complete"]);
        config.probe.notifications.enabled = false;
        let db = PanelDb::open(":memory:").unwrap();
        let dir = temp_test_dir("agenthub-notify-completion-raw-assistant");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        let final_message = format!(
            "助手完整回复开头\n{}\n助手完整回复末尾唯一标记",
            "没有完成字段也不能截断".repeat(900)
        );
        fs::write(
            &transcript,
            [
                json!({"type":"response_item","turn_id":"turn-complete","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"text":final_message}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-complete","last_agent_message":null}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let context = notify_completion_context(
            &config,
            Some(&json!({
                "thread_id": "thread-complete",
                "turn_id": "turn-complete",
                "transcript_path": transcript,
            })),
            None,
            None,
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(context);
        let (_outcome, _bark) =
            record_probe_event_with_bark(&|| Ok(config.clone()), &db, event.clone())
                .await
                .unwrap();

        assert_eq!(event.kind, "completion");
        assert_eq!(event.payload["body_source"], "last_assistant_message");
        assert!(event.bark_body.contains("助手完整回复开头"));
        assert!(event.bark_body.contains("助手完整回复末尾唯一标记"));
        assert!(!event.bark_body.contains("[truncated]"));
        assert!(event.payload["body_length"].as_u64().unwrap() > 4000);
        let stored = db.list_probe_events(10).unwrap();
        assert!(!serde_json::to_string(&stored[0])
            .unwrap()
            .contains("助手完整回复末尾唯一标记"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn hook_stop_uses_cli_turn_id_when_payload_turn_id_is_missing() {
        let (config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-cli"]);
        let dir = temp_test_dir("agenthub-hook-cli-turn-id");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        fs::write(
            &transcript,
            [
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"上一轮完成正文"}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-cli"}}).to_string(),
                json!({"type":"turn_context","payload":{"turn_id":"turn-cli","summary":"auto","cwd":"/tmp"}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-cli","last_agent_message":"CLI turn 最终正文"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let event_input = hook_stop_event_input(
            &config,
            Some(&json!({
                "thread_id": "thread-cli",
                "transcript_path": transcript,
                "last_assistant_message": "payload 早期正文"
            })),
            Some("thread-cli"),
            Some("turn-cli"),
            "hook-stop",
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(event_input);

        assert!(event.bark_body.contains("CLI turn 最终正文"));
        assert!(!event.bark_body.contains("上一轮完成正文"));
        assert!(!event.bark_body.contains("payload 早期正文"));
        assert_eq!(event.payload["turn_id"], "turn-cli");
        assert_eq!(event.payload["body_selected_turn_id"], "turn-cli");
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn notify_completion_context_keeps_unresolved_plan_as_reply_needed() {
        let (config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-plan"]);
        let dir = temp_test_dir("agenthub-notify-completion-plan-pending");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        fs::write(
            &transcript,
            [
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 待确认计划\n- 检查\n</proposed_plan>"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":"计划等待确认。"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let context = notify_completion_context(
            &config,
            Some(&json!({
                "thread_id": "thread-plan",
                "turn_id": "turn-plan",
                "transcript_path": transcript,
            })),
            None,
            None,
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(context);

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.event_type, "reply_needed");
        assert_eq!(event.payload["body_source"], "proposed_plan");
        assert_eq!(event.bark_body, "# 待确认计划\n- 检查");

        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn notify_completion_context_keeps_unresolved_question_as_reply_needed() {
        let (config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-choice"]);
        let dir = temp_test_dir("agenthub-notify-completion-question-pending");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("rollout.jsonl");
        fs::write(
            &transcript,
            [
                json!({"type":"RequestUserInput","turn_id":"turn-choice","item_id":"choice-1","questions":[{"id":"choice","question":"继续吗？","options":[{"label":"继续"},{"label":"停止"}]}]}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-choice","last_agent_message":"问题等待选择。"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let context = notify_completion_context(
            &config,
            Some(&json!({
                "thread_id": "thread-choice",
                "turn_id": "turn-choice",
                "transcript_path": transcript,
            })),
            None,
            None,
        )
        .unwrap();
        let event = probe_runtime(&config).build_event(context);

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.event_type, "reply_needed");
        assert_eq!(event.payload["body_source"], "request_user_input");
        assert!(event.bark_body.contains("问题 1：继续吗？"));
        assert!(event.bark_body.contains("选项 1：继续"));
        assert!(event.bark_body.contains("选项 2：停止"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn hook_stop_prefers_full_rollout_plan_over_short_stdin_summary() {
        let dir = temp_test_dir("agenthub-hook-stop-full-plan");
        fs::create_dir_all(&dir).unwrap();
        let mut config = test_config(dir.join(".codex"));
        config.probe.notifications.enabled = false;
        let transcript = dir.join("rollout.jsonl");
        let full_plan = format!("# 完整计划\n{}\n末尾唯一完整计划", "计划正文".repeat(1200));
        fs::write(
            &transcript,
            [
                json!({"type":"item_completed","thread_id":"thread-plan","turn_id":"turn-plan","item":{"type":"Plan","id":"plan-1","text":full_plan}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n短摘要\n</proposed_plan>"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":null}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let selection = hook_stop_last_assistant_message(
            transcript.to_str(),
            Some("turn-plan"),
            Some("<proposed_plan>\n短摘要\n</proposed_plan>"),
        )
        .unwrap()
        .unwrap();
        let event = probe_runtime(&config).build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-plan"),
                Some("turn-plan"),
                Some("thread-plan"),
                transcript.to_str(),
                Some(&selection.message),
                "hook-stop",
            )
            .with_body_source(Some(&selection.source)),
        );

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.event_type, "reply_needed");
        assert!(event.bark_body.contains("# 完整计划"));
        assert!(event.bark_body.contains("末尾唯一完整计划"));
        assert!(!event.bark_body.contains("短摘要"));
        assert!(!event.bark_body.contains("[truncated]"));
        assert!(event.payload["body_length"].as_u64().unwrap() > 4000);
        assert_eq!(event.payload["body_source"], "proposed_plan");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn passive_reply_needed_body_formats_questions_and_extracts_plan_text() {
        let thread = agenthub_core::codex::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "thread-question".to_string(),
            title: "问题线程".to_string(),
            status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
            updated_at: None,
            archived_at: None,
            message_count: 1,
            latest_message: Some("fallback should not be used".to_string()),
            cwd: None,
            model: None,
            rollout_path: None,
            active_turn_id: Some("turn-question".to_string()),
            active_job_id: None,
            pending_elicitation: Some(agenthub_core::codex::PendingElicitation {
                turn_id: Some("turn-question".to_string()),
                item_id: Some("call-question".to_string()),
                questions: vec![agenthub_core::codex::UserInputQuestion {
                    id: "q1".to_string(),
                    header: Some("Mode".to_string()),
                    question: "怎么继续？".to_string(),
                    options: vec![
                        agenthub_core::codex::UserInputOption {
                            label: "直接执行".to_string(),
                            description: Some("按当前计划继续".to_string()),
                        },
                        agenthub_core::codex::UserInputOption {
                            label: "先调整".to_string(),
                            description: Some("补充约束后再执行".to_string()),
                        },
                    ],
                }],
            }),
            last_event_kind: None,
        };

        let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
        let body = plan.body;
        let source = plan.body_source;

        let body = body.expect("request_user_input body");
        assert_eq!(source.as_deref(), Some("request_user_input"));
        assert!(!body.contains("等待用户选择"));
        assert!(!body.contains("Call ID："));
        assert!(!body.contains("Turn ID："));
        assert!(!body.contains("待选择内容"));
        assert!(body.contains("问题 1：怎么继续？"));
        assert!(body.contains("选项 1：直接执行"));
        assert!(body.contains("说明：按当前计划继续"));
        assert!(body.contains("选项 2：先调整"));
        assert!(!body.contains("fallback should not be used"));

        let (sandbox_cfg, sandbox_root) = crate::api::test_support::sandboxed_test_config();
        let event = probe_runtime(&sandbox_cfg).build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-question"),
                Some("turn-question"),
                Some("thread-question"),
                None,
                Some(&body),
                "reply-needed",
            )
            .with_thread_title(Some("问题线程"))
            .with_body_source(source.as_deref()),
        );
        assert_eq!(event.bark_title, "等待回复：问题线程");
        let _ = fs::remove_dir_all(sandbox_root);
        assert_eq!(event.bark_body, body);
        assert!(!event.bark_body.contains("thread-question"));

        let plan_thread = agenthub_core::codex::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            pending_elicitation: None,
            latest_message: Some(
                "<proposed_plan>\n# 修复计划\n- 等待确认\n</proposed_plan>".to_string(),
            ),
            ..thread
        };

        let plan =
            probe_service::probe_passive_thread_notification_plan(&plan_thread, "reply-needed");
        let body = plan.body;
        let source = plan.body_source;

        let body = body.expect("plan body");
        assert_eq!(source.as_deref(), Some("proposed_plan"));
        assert_eq!(body, "# 修复计划\n- 等待确认");
        assert!(!body.contains("<proposed_plan>"));
        assert!(!body.contains("</proposed_plan>"));
    }

    #[test]
    fn passive_reply_needed_body_suppresses_plan_after_later_reply_or_completion() {
        let dir = temp_test_dir("agenthub-plan-suppressed-after-reply");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-plan-replied.jsonl");
        fs::write(
            &rollout,
            [
                json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"批准，继续。"}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"正在执行计划。"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-work","last_agent_message":"已完成。"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let thread = agenthub_core::codex::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "thread-plan-replied".to_string(),
            title: "计划已回复线程".to_string(),
            status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
            updated_at: None,
            archived_at: None,
            message_count: 4,
            latest_message: Some(
                "<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>".to_string(),
            ),
            cwd: None,
            model: None,
            rollout_path: Some(rollout),
            active_turn_id: Some("turn-plan".to_string()),
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: Some("task_complete".to_string()),
        };

        let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
        let body = plan.body;
        let source = plan.body_source;

        assert_eq!(body, None);
        assert_eq!(source, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn passive_reply_needed_scan_rejects_old_reply_needed_threads() {
        let thread = agenthub_core::codex::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "thread-old-plan".to_string(),
            title: "旧计划线程".to_string(),
            status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
            updated_at: Some(
                (chrono::Utc::now()
                    - chrono::Duration::seconds(
                        probe_service::PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS + 60,
                    ))
                .to_rfc3339(),
            ),
            archived_at: None,
            message_count: 1,
            latest_message: Some(
                "<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>".to_string(),
            ),
            cwd: None,
            model: None,
            rollout_path: None,
            active_turn_id: Some("turn-old".to_string()),
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: Some("task_complete".to_string()),
        };

        let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
        assert!(!plan.fresh);
    }

    #[tokio::test]
    async fn passive_reply_needed_plan_dedupe_key_changes_with_plan_hash_and_no_ttl_resend() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-plan"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let dir = temp_test_dir("agenthub-plan-ttl-marker");
        fs::create_dir_all(&dir).unwrap();
        let db = PanelDb::open(dir.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();

        let body_one = agenthub_core::codex::extract_proposed_plan_text(
            "<proposed_plan>\n# 第一版\n- A\n</proposed_plan>",
        )
        .unwrap();
        let body_two = agenthub_core::codex::extract_proposed_plan_text(
            "<proposed_plan>\n# 第二版\n- B\n</proposed_plan>",
        )
        .unwrap();
        let make_event = |body: &str| {
            probe_runtime(&config).build_event(
                ProbeEventInput::hook_stop_with_context(
                    Some("thread-plan"),
                    Some("turn-plan"),
                    Some("thread-plan"),
                    None,
                    Some(body),
                    "reply-needed",
                )
                .with_thread_title(Some("真实计划线程"))
                .with_body_source(Some("proposed_plan"))
                .with_passive_scan_source(),
            )
        };

        let first_event = make_event(&body_one);
        let first_key = first_event.dedupe_key.clone();
        let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, first_event)
            .await
            .unwrap();
        let duplicate =
            record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event(&body_one))
                .await
                .unwrap();
        let changed_event = make_event(&body_two);
        let changed_key = changed_event.dedupe_key.clone();
        let changed = record_probe_event_with_bark(&|| Ok(config.clone()), &db, changed_event)
            .await
            .unwrap();

        assert!(first.0.recorded);
        assert_eq!(duplicate.1.reason.as_deref(), Some("sent_marker"));
        assert!(!duplicate.0.recorded);
        assert!(changed.0.recorded);
        assert_ne!(first_key, changed_key);
        assert!(first_key.contains("thread-plan"));
        assert!(first_key.contains("turn-plan"));
        assert_ne!(
            first_key,
            "reply-needed:thread-plan:turn-plan:reply_needed:turn:turn-plan"
        );
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn passive_reply_needed_plan_does_not_resend_after_ttl_expires() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-plan-ttl"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let dir = temp_test_dir("agenthub-plan-ttl-marker-single");
        fs::create_dir_all(&dir).unwrap();
        let db = PanelDb::open(dir.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let body = agenthub_core::codex::extract_proposed_plan_text(
            "<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>",
        )
        .unwrap();
        let make_event = || {
            probe_runtime(&config).build_event(
                ProbeEventInput::hook_stop_with_context(
                    Some("thread-plan-ttl"),
                    Some("turn-plan-ttl"),
                    Some("thread-plan-ttl"),
                    None,
                    Some(&body),
                    "reply-needed",
                )
                .with_thread_title(Some("旧计划 TTL 线程"))
                .with_body_source(Some("proposed_plan"))
                .with_passive_scan_source(),
            )
        };

        let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
            .await
            .unwrap();
        Connection::open(db.path())
            .unwrap()
            .execute("DELETE FROM probe_dedupe", [])
            .unwrap();
        let after_ttl = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
            .await
            .unwrap();

        assert!(first.0.recorded);
        assert_eq!(after_ttl.1.reason.as_deref(), Some("sent_marker"));
        assert!(!after_ttl.0.recorded);
        assert!(!after_ttl.1.sent);
        assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn passive_request_user_input_event_does_not_resend_after_ttl_expires() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-question-ttl"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let dir = temp_test_dir("agenthub-input-ttl-marker");
        fs::create_dir_all(&dir).unwrap();
        let db = PanelDb::open(dir.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let body = "等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-question\nTurn ID：turn-question\n时间：2026-06-16 12:00:00 北京时间\n状态说明：这一轮正在等待用户选择，不是异常停止。\n\n待选择内容：\n问题 1：Continue?\n选项 1：继续";
        let make_event = || {
            probe_runtime(&config).build_event(
                ProbeEventInput::hook_stop_with_context(
                    Some("thread-question-ttl"),
                    Some("turn-question"),
                    Some("thread-question-ttl"),
                    None,
                    Some(body),
                    "reply-needed",
                )
                .with_thread_title(Some("问题 TTL 线程"))
                .with_body_source(Some("request_user_input"))
                .with_passive_scan_source(),
            )
        };

        let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
            .await
            .unwrap();
        Connection::open(db.path())
            .unwrap()
            .execute("DELETE FROM probe_dedupe", [])
            .unwrap();
        let after_ttl = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
            .await
            .unwrap();

        assert!(first.0.recorded);
        assert_eq!(after_ttl.1.reason.as_deref(), Some("sent_marker"));
        assert!(!after_ttl.0.recorded);
        assert!(!after_ttl.1.sent);
        assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn pre_tool_use_request_user_input_event_does_not_resend_after_ttl_expires() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-pre-tool-question"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let dir = temp_test_dir("agenthub-pre-tool-use-input-marker");
        fs::create_dir_all(&dir).unwrap();
        let db = PanelDb::open(dir.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let body = "问题 1：Continue?\n选项 1：继续\n选项 2：停止";
        let make_event = || {
            probe_runtime(&config).build_event(
                ProbeEventInput::hook_stop_with_context(
                    Some("thread-pre-tool-question"),
                    Some("turn-pre-tool-question"),
                    Some("thread-pre-tool-question"),
                    None,
                    Some(body),
                    "reply-needed",
                )
                .with_thread_title(Some("PreToolUse 问题"))
                .with_body_source(Some("request_user_input"))
                .with_pre_tool_use_scan_source()
                .with_call_id(Some("call-pre-tool-question")),
            )
        };

        let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
            .await
            .unwrap();
        Connection::open(db.path())
            .unwrap()
            .execute("DELETE FROM probe_dedupe", [])
            .unwrap();
        let after_ttl = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
            .await
            .unwrap();

        assert!(first.0.recorded);
        assert_eq!(after_ttl.1.reason.as_deref(), Some("sent_marker"));
        assert!(!after_ttl.0.recorded);
        assert!(!after_ttl.1.sent);
        assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn passive_request_user_input_marker_ignores_scan_time_changes() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-question-time"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let make_body = |time: &str| {
            format!(
                "等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-question\nTurn ID：turn-question\n时间：{time}\n状态说明：这一轮正在等待用户选择，不是异常停止。\n\n待选择内容：\n问题 1：Continue?\n选项 1：继续"
            )
        };
        let make_event = |body: String| {
            probe_runtime(&config).build_event(
                ProbeEventInput::hook_stop_with_context(
                    Some("thread-question-time"),
                    Some("turn-question"),
                    Some("thread-question-time"),
                    None,
                    Some(&body),
                    "reply-needed",
                )
                .with_thread_title(Some("问题时间线程"))
                .with_body_source(Some("request_user_input"))
                .with_passive_scan_source(),
            )
        };

        let first = record_probe_event_with_bark(
            &|| Ok(config.clone()),
            &db,
            make_event(make_body("2026-06-16 12:00:00 北京时间")),
        )
        .await
        .unwrap();
        let second = record_probe_event_with_bark(
            &|| Ok(config.clone()),
            &db,
            make_event(make_body("2026-06-16 12:05:00 北京时间")),
        )
        .await
        .unwrap();

        assert!(first.0.recorded);
        assert_eq!(second.1.reason.as_deref(), Some("sent_marker"));
        assert!(!second.0.recorded);
        assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn send_bark_posts_lite_payload_and_reports_non_success_response_code() {
        let server = TestHttpServer::start_n(
            1,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 39\r\n\r\n{\"code\":400,\"message\":\"bad bark token\"}",
        );
        let (mut config, sandbox_root) = crate::api::test_support::sandboxed_test_config();
        config.probe.notifications.server_url = server.url();
        config.probe.notifications.group = "Probe Group".to_string();
        config.probe.notifications.sound = Some("bell".to_string());
        config.probe.notifications.url = Some("https://panel.example.com/agenthub/".to_string());

        let request = ProbeBarkRequest {
            url: None,
            presentation: None,
            title: "AgentHub 推送测试".to_string(),
            body: "来自本机的测试通知。".to_string(),
            dedupe_key: "hook-stop:thread-a:turn-1".to_string(),
        };
        let result = send_bark_notification(
            &config,
            b"device key/with spaces",
            &request,
            std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();

        assert!(!result.sent);
        assert!(!result.skipped);
        assert_eq!(result.reason.as_deref(), Some("bark_response_code"));
        assert_eq!(result.http_status, Some(200));
        assert!(result.device_key_configured);
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("device key"));
        let raw = server.request();
        assert!(raw.starts_with("POST /push "));
        let body = raw.split("\r\n\r\n").nth(1).unwrap();
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["device_key"], "device key/with spaces");
        assert_eq!(payload["title"], "AgentHub 推送测试");
        assert_eq!(payload["body"], "来自本机的测试通知。");
        assert_eq!(payload.as_object().unwrap().len(), 4);
        assert!(payload["id"].is_string());
        assert!(payload.get("group").is_none());
        assert!(payload.get("sound").is_none());
        assert!(payload.get("url").is_none());
        let _ = fs::remove_dir_all(sandbox_root);
    }

    #[tokio::test]
    async fn send_bark_splits_long_body_on_utf8_boundaries_with_segment_prefix() {
        let server = TestHttpServer::start_n(
            20,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let (mut config, sandbox_root) = crate::api::test_support::sandboxed_test_config();
        config.probe.notifications.server_url = server.url();
        config.probe.notifications.group = "Probe Group".to_string();

        let request = ProbeBarkRequest {
            url: None,
            presentation: None,
            title: "AgentHub Probe long body".to_string(),
            body: "完成".repeat(4_000),
            dedupe_key: "hook-stop:thread-a:turn-long".to_string(),
        };
        let result = send_bark_notification(
            &config,
            b"device-key",
            &request,
            std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();

        assert!(result.sent);
        assert_eq!(result.http_status, Some(200));
        let requests = server.requests(result.chunk_count);
        assert_eq!(requests.len(), result.chunk_count);
        for (index, raw) in requests.iter().enumerate() {
            assert!(raw.starts_with("POST /push "));
            let body = raw.split("\r\n\r\n").nth(1).unwrap();
            let payload: Value = serde_json::from_str(body).unwrap();
            assert_eq!(payload["device_key"], "device-key");
            assert_eq!(
                payload["title"],
                format!(
                    "AgentHub Probe long body ({}/{})",
                    index + 1,
                    result.chunk_count
                )
            );
            let chunk = payload["body"].as_str().unwrap();
            let prefix = format!("第 {}/{} 段\n\n", index + 1, result.chunk_count);
            assert!(chunk.starts_with(&prefix));
            let body_part = chunk.strip_prefix(&prefix).unwrap();
            assert!(body_part.len() <= PROBE_BARK_BODY_CHUNK_BYTES);
            assert!(chunk.is_char_boundary(chunk.len()));
        }
        let combined = requests
            .iter()
            .enumerate()
            .map(|(index, raw)| {
                let body = raw.split("\r\n\r\n").nth(1).unwrap();
                let chunk = serde_json::from_str::<Value>(body).unwrap()["body"]
                    .as_str()
                    .unwrap()
                    .to_string();
                let prefix = format!("第 {}/{} 段\n\n", index + 1, result.chunk_count);
                chunk.strip_prefix(&prefix).unwrap().to_string()
            })
            .collect::<String>();
        assert_eq!(combined, request.body);
        let _ = fs::remove_dir_all(sandbox_root);
    }

    #[tokio::test]
    async fn bark_delivery_sends_one_bounded_summary_without_ids_or_chunks() {
        let server = TestHttpServer::start_n(
            20,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-long"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_completion = true;
        config.probe.notifications.server_url = server.url();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"device-key")
            .unwrap();
        let full_body = format!("开头\n{}\n末尾唯一完整反馈", "完整正文".repeat(900));
        let event = probe_runtime(&config).build_event(
            ProbeEventInput::notify_completion_with_context(
                Some("thread-long"),
                Some("turn-long"),
                Some("thread-long"),
                None,
                Some(&full_body),
                Some("task_complete.last_agent_message"),
            )
            .with_thread_title(Some("真实长正文线程")),
        );
        assert_eq!(event.bark_title, "线程正常完成：真实长正文线程");
        assert!(event.payload["body_truncated"].as_bool().unwrap());
        assert!(!event.payload["body_summary"]
            .as_str()
            .unwrap()
            .contains("末尾唯一完整反馈"));

        let (_outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();

        assert!(bark.sent);
        let requests = server.requests(bark.request_count);
        assert_eq!(requests.len(), bark.request_count);
        assert_eq!(bark.request_count, 1);
        assert_eq!(bark.chunk_count, 1);
        let payload: Value =
            serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["title"], "Codex｜完成｜真实长正文线程");
        let summary = payload["body"].as_str().unwrap();
        assert!(summary.starts_with("开头\n"));
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), 300);
        assert!(!summary.contains("末尾唯一完整反馈"));
        assert_eq!(payload["group"], "AgentHub｜Codex");
        assert_eq!(payload["level"], "active");
        let stored = db.list_probe_events(10).unwrap();
        let stored_json = serde_json::to_string(&stored[0]).unwrap();
        assert!(!stored_json.contains("末尾唯一完整反馈"));
        assert!(stored[0].payload["bark"].get("body").is_none());
    }

    #[test]
    fn bark_body_chunks_match_lite_split_and_trim_body() {
        let body = format!("  {}  \n", "完成".repeat(4_000));
        let chunks = bark_body_chunks(&body, PROBE_BARK_BODY_CHUNK_BYTES);

        assert!(!chunks[0].contains("  完成"));
        for (index, chunk) in chunks.iter().enumerate() {
            let prefix = format!("第 {}/{} 段\n\n", index + 1, chunks.len());
            assert!(chunk.starts_with(&prefix));
            let body_part = chunk.strip_prefix(&prefix).unwrap();
            assert!(body_part.len() <= PROBE_BARK_BODY_CHUNK_BYTES);
            assert!(chunk.is_char_boundary(chunk.len()));
        }
        let joined = chunks
            .iter()
            .enumerate()
            .map(|(index, chunk)| {
                let prefix = format!("第 {}/{} 段\n\n", index + 1, chunks.len());
                chunk.strip_prefix(&prefix).unwrap()
            })
            .collect::<String>();
        assert_eq!(joined, body.trim());
    }

    #[test]
    fn bark_body_chunks_keeps_body_budget_when_prefix_digit_count_grows() {
        let body = "a".repeat((PROBE_BARK_BODY_CHUNK_BYTES * 9) - 100);
        let chunks = bark_body_chunks(&body, PROBE_BARK_BODY_CHUNK_BYTES);

        assert_eq!(chunks.len(), 9);
        let joined = chunks
            .iter()
            .enumerate()
            .map(|(index, chunk)| {
                let prefix = format!("第 {}/{} 段\n\n", index + 1, chunks.len());
                let body_part = chunk.strip_prefix(&prefix).unwrap();
                assert!(body_part.len() <= PROBE_BARK_BODY_CHUNK_BYTES);
                body_part
            })
            .collect::<String>();
        assert_eq!(joined, body);
    }

    #[test]
    fn bark_body_chunks_match_lite_exact_2400_body_bytes_before_prefix() {
        let body = "a".repeat(PROBE_BARK_BODY_CHUNK_BYTES * 2);
        let chunks = bark_body_chunks(&body, PROBE_BARK_BODY_CHUNK_BYTES);

        assert_eq!(chunks.len(), 2);
        for (index, chunk) in chunks.iter().enumerate() {
            let prefix = format!("第 {}/{} 段\n\n", index + 1, chunks.len());
            let body_part = chunk.strip_prefix(&prefix).unwrap();
            assert_eq!(body_part.len(), PROBE_BARK_BODY_CHUNK_BYTES);
            assert!(body_part.is_char_boundary(body_part.len()));
        }
        let joined = chunks
            .iter()
            .enumerate()
            .map(|(index, chunk)| {
                let prefix = format!("第 {}/{} 段\n\n", index + 1, chunks.len());
                chunk.strip_prefix(&prefix).unwrap()
            })
            .collect::<String>();
        assert_eq!(joined, body);
    }

    #[tokio::test]
    async fn record_probe_event_stores_body_metadata_but_not_full_bark_body_or_tokens() {
        let server = TestHttpServer::start_n(
            20,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-safe-store"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_completion = true;
        config.probe.notifications.server_url = server.url();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"secret-device-token")
            .unwrap();
        let full_body = format!(
            "开头\nAuthorization: Bearer secret-token\n{}\n<proposed_plan>不要存这个标签</proposed_plan>\n末尾唯一完整反馈",
            "完整正文".repeat(900)
        );
        let event = probe_runtime(&config).build_event(
            ProbeEventInput::notify_completion_with_context(
                Some("thread-safe-store"),
                Some("turn-safe-store"),
                Some("thread-safe-store"),
                None,
                Some(&full_body),
                Some("task_complete.last_agent_message"),
            )
            .with_thread_title(Some("安全存储线程")),
        );

        let (_outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();
        assert!(bark.sent);
        let _requests = server.requests(bark.request_count);

        let stored = db.list_probe_events(10).unwrap();
        assert_eq!(stored.len(), 1);
        let payload = &stored[0].payload;
        assert!(payload["body_summary"].as_str().unwrap().contains("开头"));
        assert!(payload["body_sha256"]
            .as_str()
            .is_some_and(|value| value.len() == 64));
        assert_eq!(payload["body_length"], full_body.len() as u64);
        assert_eq!(payload["bark"]["body_length"], full_body.len() as u64);
        assert_eq!(payload["bark"]["body_sha256"], payload["body_sha256"]);
        assert!(payload["bark"]["chunk_count"].as_u64().is_some());
        assert!(payload["bark"]["request_count"].as_u64().is_some());
        assert!(payload["bark"].get("body").is_none());
        assert!(payload.get("bark_body").is_none() || payload["bark_body"].is_null());
        let stored_json = serde_json::to_string(&stored[0]).unwrap();
        assert!(!stored_json.contains("末尾唯一完整反馈"));
        assert!(!stored_json.contains("secret-token"));
        assert!(!stored_json.contains("secret-device-token"));
        assert!(!stored_json.contains("<proposed_plan>"));
        assert!(!stored_json.contains("</proposed_plan>"));
        assert!(!stored_json.contains("/push"));
    }

    #[tokio::test]
    async fn passive_reply_needed_plan_dedupe_key_includes_thread_turn_item_and_plan_hash() {
        let server = TestHttpServer::start_n(
            1,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-plan-stable"]);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = server.url();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();
        let raw_plan = "<proposed_plan>\n# 稳定计划\n- A\n</proposed_plan>";
        let body = agenthub_core::codex::extract_proposed_plan_text(raw_plan).unwrap();
        let event = probe_runtime(&config).build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-plan-stable"),
                Some("turn-plan-stable"),
                Some("thread-plan-stable"),
                None,
                Some(&body),
                "reply-needed",
            )
            .with_thread_title(Some("稳定计划线程"))
            .with_body_source(Some("proposed_plan"))
            .with_passive_scan_source(),
        );
        let mut event = event;
        event.payload["item_id"] = json!("item-plan-stable");

        let (outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();
        assert!(bark.sent);
        let _raw = server.request();
        let dedupe_key = outcome.dedupe_key;

        assert!(dedupe_key.contains("thread-plan-stable"));
        assert!(dedupe_key.contains("turn-plan-stable"));
        assert!(dedupe_key.contains("item-plan-stable"));
        assert!(dedupe_key.contains("plan_hash"));
        assert_ne!(
            dedupe_key,
            "reply-needed:thread-plan-stable:turn-plan-stable:reply_needed:turn:turn-plan-stable"
        );
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn passive_reply_needed_body_suppresses_plan_after_tool_output_assistant_or_turn_completed() {
        for (name, tail) in [
            (
                "tool-output",
                vec![
                    json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"function_call_output","call_id":"call-plan","output":"完成选择"}}),
                ],
            ),
            (
                "assistant-progress",
                vec![
                    json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"继续执行计划。"}]}}),
                ],
            ),
            (
                "turn-completed",
                vec![json!({"type":"turn_completed","turn_id":"turn-plan"})],
            ),
        ] {
            let dir = temp_test_dir(&format!("agenthub-plan-suppressed-{name}"));
            fs::create_dir_all(&dir).unwrap();
            let rollout = dir.join("rollout.jsonl");
            let mut lines = vec![
                json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>"}]}}),
            ];
            lines.extend(tail);
            fs::write(
                &rollout,
                lines
                    .into_iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            let thread = agenthub_core::codex::ThreadSummary {
                activity_updated_at: None,
                storage_size: None,
                id: format!("thread-{name}"),
                title: "计划应被抑制线程".to_string(),
                status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
                updated_at: None,
                archived_at: None,
                message_count: 2,
                latest_message: Some(
                    "<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>".to_string(),
                ),
                cwd: None,
                model: None,
                rollout_path: Some(rollout),
                active_turn_id: Some("turn-plan".to_string()),
                active_job_id: None,
                pending_elicitation: None,
                last_event_kind: None,
            };

            let plan =
                probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
            let body = plan.body;
            let source = plan.body_source;

            assert_eq!(body, None, "{name}");
            assert_eq!(source, None, "{name}");
            fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn passive_reply_needed_fallback_suppresses_old_plan_after_later_completion() {
        let dir = temp_test_dir("agenthub-fallback-old-plan-completed");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout.jsonl");
        fs::write(
            &rollout,
            [
                json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧 fallback 计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"继续"}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"执行完成。"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-work","last_agent_message":"执行完成。"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let thread = agenthub_core::codex::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "thread-fallback-old-plan".to_string(),
            title: "fallback 旧计划线程".to_string(),
            status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
            updated_at: None,
            archived_at: None,
            message_count: 4,
            latest_message: None,
            cwd: None,
            model: None,
            rollout_path: Some(rollout),
            active_turn_id: None,
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: Some("task_complete".to_string()),
        };

        let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
        let body = plan.body;
        let source = plan.body_source;

        assert_eq!(body, None);
        assert_eq!(source, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn passive_reply_needed_fallback_suppresses_old_plan_when_completion_has_no_body() {
        let dir = temp_test_dir("agenthub-fallback-old-plan-empty-complete");
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout.jsonl");
        fs::write(
            &rollout,
            [
                json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧 fallback 计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"继续"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":null}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        assert_eq!(
            agenthub_core::codex::rollout_completion_last_agent_message(
                &rollout,
                Some("turn-plan"),
            )
            .unwrap(),
            None
        );
        let thread = agenthub_core::codex::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "thread-fallback-empty-complete".to_string(),
            title: "fallback 空完成线程".to_string(),
            status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
            updated_at: None,
            archived_at: None,
            message_count: 3,
            latest_message: None,
            cwd: None,
            model: None,
            rollout_path: Some(rollout),
            active_turn_id: Some("turn-plan".to_string()),
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: Some("task_complete".to_string()),
        };

        let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
        let body = plan.body;
        let source = plan.body_source;

        assert_eq!(body, None);
        assert_eq!(source, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn probe_thread_scan_does_not_resend_completed_old_plan_when_app_server_is_offline() {
        let dir = temp_test_dir("agenthub-thread-scan-old-plan-offline");
        let codex_home = dir.join(".codex");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        let rollout = codex_home.join("sessions").join("rollout-old-plan.jsonl");
        fs::write(
            &rollout,
            [
                json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"同意，继续。"}]}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-work","last_agent_message":"已完成。"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT,
                source TEXT,
                thread_source TEXT,
                updated_at INTEGER,
                archived_at INTEGER,
                rollout_path TEXT
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
            params![
                "thread-old-plan",
                "真实旧计划线程",
                chrono::Utc::now().timestamp_millis(),
                rollout.to_string_lossy().as_ref()
            ],
        )
        .unwrap();
        fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
        let mut config = test_config(codex_home.clone());

        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let db = PanelDb::open(":memory:").unwrap();
        let state = AppState::new(config, db.clone());

        let count = run_probe_thread_scan_if_due(state).await.unwrap();

        assert_eq!(count, 0);
        assert!(db.list_probe_events(10).unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn passive_request_user_input_does_not_resend_after_ttl_or_revive_after_answer() {
        let dir = temp_test_dir("agenthub-request-user-input-no-revive");
        let codex_home = dir.join(".codex");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        let rollout = codex_home.join("sessions").join("rollout-question.jsonl");
        fs::write(
            &rollout,
            json!({
                "type": "response_item",
                "turn_id": "turn-question",
                "payload": {
                    "type": "function_call",
                    "name": "request_user_input",
                    "status": "pending",
                    "call_id": "call-question",
                    "questions": [{
                        "id": "choice",
                        "header": "Mode",
                        "question": "Continue?",
                        "options": [
                            {"label": "继续", "description": "按计划执行"},
                            {"label": "停止"}
                        ]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT,
                source TEXT,
                thread_source TEXT,
                updated_at INTEGER,
                archived_at INTEGER,
                rollout_path TEXT
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
            params![
                "thread-question-no-repeat",
                "待选择线程",
                chrono::Utc::now().timestamp_millis(),
                rollout.to_string_lossy().as_ref()
            ],
        )
        .unwrap();
        fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
        let mut config = test_config(codex_home.clone());

        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
            .unwrap();

        let first_count = run_probe_thread_scan_if_due(AppState::new(config.clone(), db.clone()))
            .await
            .unwrap();
        db.maintain_probe_events(1, 100).unwrap();
        let second_count = run_probe_thread_scan_if_due(AppState::new(config.clone(), db.clone()))
            .await
            .unwrap();
        fs::write(
            &rollout,
            [
                json!({"type":"response_item","turn_id":"turn-question","payload":{"type":"function_call","name":"request_user_input","status":"pending","call_id":"call-question","questions":[{"id":"choice","header":"Mode","question":"Continue?","options":[{"label":"继续","description":"按计划执行"},{"label":"停止"}]}]}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-question","payload":{"type":"UserInputAnswer","call_id":"call-question","answers":{"choice":["继续"]}}}).to_string(),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-question","last_agent_message":"已继续。"}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let answered_count = run_probe_thread_scan_if_due(AppState::new(config, db.clone()))
            .await
            .unwrap();

        assert_eq!(first_count, 1);
        assert_eq!(second_count, 0);
        assert_eq!(answered_count, 0);
        assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn send_bark_success_reports_redacted_request_metadata() {
        let server = TestHttpServer::start_n(
            1,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let (mut config, sandbox_root) = crate::api::test_support::sandboxed_test_config();
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = server.url();
        config.probe.notifications.group = "Probe Group".to_string();

        let result = send_bark_notification(
            &config,
            b"device-key-secret",
            &ProbeBarkRequest {
                url: None,
                presentation: None,
                title: "AgentHub 推送测试".to_string(),
                body: "ok".to_string(),
                dedupe_key: "hook-stop:thread-a:turn-1".to_string(),
            },
            std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();

        assert!(result.sent);
        assert_eq!(result.reason, None);
        assert_eq!(result.http_status, Some(200));
        assert_eq!(result.request_count, 1);
        assert_eq!(result.chunk_count, 1);
        assert!(result.server_url.is_none());
        assert!(result.request_url.is_none());
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("device-key-secret"));
        let _ = server.request();
        let _ = fs::remove_dir_all(sandbox_root);
    }

    #[tokio::test]
    async fn bark_test_uses_agenthub_title_and_machine_body() {
        let server = TestHttpServer::start_n(
            1,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let dir = temp_test_dir("agenthub-bark-cli");
        fs::create_dir_all(&dir).unwrap();
        let mut config = test_config(dir.join(".codex"));
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = server.url();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"device-key")
            .unwrap();

        let path = dir.join("config.toml");
        fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        run_probe_command(ProbeCommand::BarkTest, &config, db, &path)
            .await
            .unwrap();
        fs::remove_dir_all(dir).unwrap();

        let raw = server.request();
        assert!(raw.starts_with("POST /push "));
        let body = raw.split("\r\n\r\n").nth(1).unwrap();
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["device_key"], "device-key");
        assert_eq!(payload["title"], "AgentHub 推送测试");
        assert_eq!(payload["body"], agenthub_core::probe::bark_test_body());
    }

    #[tokio::test]
    async fn hook_stop_cli_payload_keeps_stdout_codex_only_and_stderr_diagnostics() {
        let (mut config, _identity) =
            notification_accuracy_tests::test_notification_config(&["thread-a"]);
        config.probe.notifications.enabled = false;
        let db = PanelDb::open(":memory:").unwrap();
        let result = handle_built_probe_event(
            &|| Ok(config.clone()),
            &db,
            probe_runtime(&config).build_event(ProbeEventInput::hook_stop(
                Some("thread-a"),
                Some("turn-a"),
                "hook-stop",
            )),
        )
        .await
        .unwrap();

        let (stdout, stderr) = hook_stop_cli_output(&result).unwrap();
        assert_eq!(stdout.trim(), r#"{"continue":true,"suppressOutput":false}"#);
        let diagnostics: Value = serde_json::from_str(stderr.trim()).unwrap();
        assert_eq!(diagnostics["probe_event"]["recorded"], true);
        assert_eq!(diagnostics["bark"]["skipped"], true);
        assert_eq!(diagnostics["bark"]["reason"], "notifications_disabled");
        assert!(!stdout.contains("bark"));
        assert!(!stdout.contains("probe_event"));
    }

    #[test]
    fn utf8_chunks_preserve_multibyte_boundaries_and_content() {
        let value = "完成a".repeat(10);
        let chunks = utf8_chunks(&value, 7);

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 7));
        assert!(chunks
            .iter()
            .all(|chunk| chunk.is_char_boundary(chunk.len())));
        assert_eq!(chunks.concat(), value);
    }

    #[test]
    fn antigravity_stop_allow_output_returns_decision_allow_json() {
        let out = antigravity_stop_allow_output();
        assert_eq!(out, json!({"decision": "allow"}));

        let printed = std::sync::atomic::AtomicBool::new(false);
        let mut buffer = Vec::new();
        assert!(print_antigravity_stop_allow_once(&printed, &mut buffer));
        assert!(!print_antigravity_stop_allow_once(&printed, &mut buffer));
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "{\"decision\":\"allow\"}\n"
        );
    }

    pub(super) struct TestHttpServer {
        address: std::net::SocketAddr,
        request: std::sync::mpsc::Receiver<String>,
    }

    impl TestHttpServer {
        pub(super) fn start_n(expected_requests: usize, response: &'static str) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for _ in 0..expected_requests {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request = String::new();
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        request.push_str(&line);
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                    }
                    let content_length = request
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    let mut body = vec![0_u8; content_length];
                    reader.read_exact(&mut body).unwrap();
                    request.push_str(&String::from_utf8_lossy(&body));
                    tx.send(request).unwrap();
                    stream.write_all(response.as_bytes()).unwrap();
                }
            });
            Self {
                address,
                request: rx,
            }
        }

        pub(super) fn url(&self) -> String {
            format!("http://{}", self.address)
        }

        pub(super) fn request(self) -> String {
            self.request
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
        }

        fn requests(self, count: usize) -> Vec<String> {
            (0..count)
                .map(|_| {
                    self.request
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .unwrap()
                })
                .collect()
        }
    }

    #[tokio::test]
    async fn probe_thread_scan_records_request_user_input_body_for_reply_needed() {
        let dir = temp_test_dir("agenthub-thread-scan-request-user-input");
        let codex_home = dir.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::write(
            codex_home
                .join("sessions")
                .join("rollout-stale-reply.jsonl"),
            json!({
                "type": "response_item",
                "turn_id": "turn-stale",
                "payload": {
                    "type": "function_call",
                    "name": "request_user_input",
                    "status": "pending",
                    "call_id": "call-choice",
                    "questions": [{
                        "id": "q1",
                        "header": "确认",
                        "question": "Continue?",
                        "options": [
                            {"label": "继续", "description": "按计划执行"},
                            {"label": "停止"}
                        ]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT,
                source TEXT,
                thread_source TEXT,
                updated_at INTEGER,
                archived_at INTEGER,
                rollout_path TEXT
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
            params![
                "stale-reply",
                "stale reply",
                chrono::Utc::now().timestamp_millis(),
                codex_home
                    .join("sessions")
                    .join("rollout-stale-reply.jsonl")
                    .to_string_lossy()
                    .as_ref()
            ],
        )
        .unwrap();
        fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
        let mut config = test_config(codex_home.clone());
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.notify_recoverable = true;
        let db = PanelDb::open(":memory:").unwrap();
        let state = AppState::new(config, db.clone());

        let count = run_probe_thread_scan_if_due(state).await.unwrap();

        assert_eq!(count, 1);
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "reply-needed");
        assert_eq!(events[0].payload["thread_title"], "stale reply");
        assert_eq!(events[0].payload["body_source"], "request_user_input");
        let body_summary = events[0].payload["body_summary"].as_str().unwrap();
        assert!(!body_summary.contains("等待用户选择"));
        assert!(!body_summary.contains("Turn ID："));
        assert!(!body_summary.contains("Call ID："));
        assert!(!body_summary.contains("状态说明："));
        assert!(body_summary.contains("问题 1：Continue?"));
        assert!(body_summary.contains("选项 1：继续"));
        assert!(body_summary.contains("说明：按计划执行"));
        assert!(body_summary.contains("选项 2：停止"));
        assert!(events[0].payload["bark"].get("body").is_none());
        assert!(events[0].payload["bark"]["title"]
            .as_str()
            .unwrap()
            .contains("等待回复"));
        fs::remove_dir_all(&dir).unwrap();
    }

    fn test_config(codex_home: PathBuf) -> Config {
        let root = codex_home.parent().unwrap_or(&codex_home).to_path_buf();
        let mut config = Config::default();
        config.codex.home = codex_home;
        config.codex.workspace = root.join("workspace");
        config.paths.data_dir = root.join("data");
        config.paths.db_path = root.join("data/panel.sqlite");
        config.paths.log_dir = root.join("logs");

        config
    }

    fn temp_test_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }
}
