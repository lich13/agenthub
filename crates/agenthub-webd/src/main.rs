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
mod ssh_server;
mod state;

#[cfg(test)]
mod notification_accuracy_tests;
#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;

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
const QUESTION_CONFIRMATION_DELAY_MS: u64 = 1_000;
/// The foreground Stop hook spawns the detached child right before calling `exit(0)`.
/// Antigravity's hook runner waits for the foreground process to exit and close stdout
/// before writing the current turn's `ExecutorMetadata`. Waiting 120ms lets the host
/// persist `ExecutorMetadata` before the first `GetAllCascadeTrajectories` query.
const ANTIGRAVITY_DETACHED_SETTLE_DELAY_MS: u64 = 120;
const PROBE_ERROR_MONITOR_CURSOR_SETTING: &str = "probe_error_monitor_cursor";
const PROBE_ERROR_MONITOR_STATUS_SETTING: &str = "probe_error_monitor_status";
const PROBE_ERROR_MONITOR_BATCH_LIMIT: usize = 1_000;
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
    let mut runtime = if matches!(
        cli.command,
        Command::Serve
            | Command::Probe {
                command: ProbeCommand::MonitorErrors
            }
    ) {
        let mut builder = tokio::runtime::Builder::new_multi_thread();
        builder.worker_threads(2);
        builder.max_blocking_threads(32);
        builder
    } else {
        tokio::runtime::Builder::new_current_thread()
    };
    runtime.enable_all().build()?.block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::RpcStdio => return ssh_server::proxy().await,
        Command::Health => return ssh_server::health().await,
        _ => (),
    }
    let config_path = match cli.config {
        Some(path) => path,
        None => PlatformPaths::current()?.config_file,
    };
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
                    claude_hook::run(&config_path),
                )
                .await;
            }
            Command::GrokHook => {
                let _ = tokio::time::timeout(
                    Duration::from_millis(4800),
                    provider_monitor::run_provider_hook(
                        &config_path,
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
                    if spawn_detached_antigravity_hook(&config_path, conversation_id.as_deref()) {
                        std::process::exit(0);
                    }
                    let _ = tokio::time::timeout(
                        Duration::from_millis(4800),
                        provider_monitor::run_antigravity_hook(
                            &config_path,
                            conversation_id.as_deref(),
                        ),
                    )
                    .await;
                } else {
                    tokio::time::sleep(Duration::from_millis(ANTIGRAVITY_DETACHED_SETTLE_DELAY_MS))
                        .await;
                    let _ = tokio::time::timeout(
                        Duration::from_millis(4500),
                        provider_monitor::run_antigravity_hook(&config_path, cascade_id.as_deref()),
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
            Config::write_default(&config_path)?;
            println!("wrote {}", config_path.display());
        }
        Command::Doctor => {
            let config = Config::load_runtime(&config_path)?;
            let db = open_panel_db(&config)?;
            let resolved = resolve_codex_paths(&config.codex.home);
            println!("config={}", config_path.display());
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
            if let Err(err) = handle_hook_request_user_input_command(&config_path).await {
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
            if let Err(err) = handle_hook_request_user_input_confirm_command(&config_path).await {
                tracing::warn!("probe hook-request-user-input-confirm failed closed");
                tracing::debug!("probe hook-request-user-input-confirm diagnostic: {err:#}");
            }
        }
        Command::Probe {
            command: ProbeCommand::MonitorErrors,
        } => run_probe_error_monitor_daemon(config_path).await?,
        Command::Probe { command } => {
            let config = Config::load_runtime(&config_path)?;
            let db = open_panel_db(&config)?;
            run_probe_command(command, &config, db, &config_path).await?;
        }
        Command::Serve => serve(config_path).await?,
        Command::RpcStdio | Command::Health => unreachable!(),
    }
    Ok(())
}

async fn run_probe_command(
    command: ProbeCommand,
    config: &Config,
    db: PanelDb,
    config_path: &Path,
) -> Result<()> {
    let cache = monitor_start::DaemonMonitorCache::default();
    let load_config = || cache.load_config(config_path);
    let mut platform = PlatformPaths::current()?;
    platform.config_file = config_path.to_path_buf();
    match command {
        ProbeCommand::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &ProbeRuntime::new(config.clone(), platform.clone())
                        .status_with_db(&db, false)
                        .await?
                )?
            );
        }
        ProbeCommand::HookStatus => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &ProbeRuntime::new(config.clone(), platform.clone()).hook_status()
                )?
            );
        }
        ProbeCommand::Events { limit } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "events": db.list_probe_events(limit)?.into_iter().map(redact_probe_event_for_output).collect::<Vec<_>>()
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
            let event = build_probe_event(config, input);
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
        ProbeCommand::DiagnoseAntigravity { cascade_id } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&antigravity::diagnose(&cascade_id, &db).await?)?
            );
        }
    }
    Ok(())
}

fn build_probe_event(
    config: &Config,
    input: ProbeEventInput,
) -> agenthub_core::probe::ProbeBuiltEvent {
    ProbeRuntime::build_event_for(
        config,
        agenthub_core::platform::PlatformKind::current(),
        input,
    )
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
    let mut out =
        install_probe_hooks_with_repair(config, config_path, dry_run, |platform, enabled| {
            Ok(serde_json::to_value(
                agenthub_core::probe_error_monitor::ensure_probe_error_monitor_launch_agent(
                    platform, enabled, false,
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
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            )?;
            if patched != before {
                agenthub_core::config::write_config_atomic(path, &patched)
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
    let stamp = jiff::Timestamp::now().strftime("%Y%m%d%H%M%S");
    let backup_path = hooks_path.with_extension(format!("json.agenthub-probe-bak-{stamp}"));
    let codex_config_backup_path =
        codex_config_path.with_extension(format!("toml.agenthub-probe-bak-{stamp}"));
    let mut hooks_backed_up = false;
    let mut codex_config_backed_up = false;
    if hooks_json_changed && !dry_run {
        fs::copy(&hooks_path, &backup_path).with_context(|| {
            format!(
                "backup hooks {} to {}",
                hooks_path.display(),
                backup_path.display()
            )
        })?;
        hooks_backed_up = true;
        agenthub_core::config::write_bytes_atomic_preserving_permissions(
            &hooks_path,
            &serde_json::to_vec_pretty(&root)?,
        )
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
            codex_config_backed_up = true;
        }
        agenthub_core::config::write_bytes_atomic_preserving_permissions(
            &codex_config_path,
            codex_config_after.as_bytes(),
        )
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
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            )?;
            if patched != before {
                agenthub_core::config::write_config_atomic(path, &patched)
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
        "backup_path": hooks_backed_up.then_some(backup_path),
        "codex_config_backup_path": codex_config_backed_up.then_some(codex_config_backup_path),
    }))
}

fn prune_codex_hook_state_toml(text: &str) -> Result<String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let mut document: toml_edit::DocumentMut = text.parse().context("parse Codex config.toml")?;
    if !prune_codex_hook_state(document.as_table_mut()) {
        return Ok(text.to_string());
    }
    Ok(document.to_string())
}

async fn install_probe_hooks_with_repair(
    config: &Config,
    config_path: Option<&Path>,
    dry_run: bool,
    repair_error_monitor: impl FnOnce(&PlatformPaths, bool) -> Result<Value>,
) -> Result<Value> {
    let resolved = resolve_codex_paths(&config.codex.home);
    let hooks_path = resolved.home.join("hooks.json");
    let codex_config_path = resolved.home.join("config.toml");
    let mut platform = PlatformPaths::current()?;
    if let Some(path) = config_path {
        platform.config_file = path.to_path_buf();
    }
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
        jiff::Timestamp::now().strftime("%Y%m%d%H%M%S")
    ));
    let codex_config_backup_path = codex_config_path.with_extension(format!(
        "toml.agenthub-probe-bak-{}",
        jiff::Timestamp::now().strftime("%Y%m%d%H%M%S")
    ));
    let mut hooks_backed_up = false;
    let mut codex_config_backed_up = false;
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
            hooks_backed_up = true;
        }
        agenthub_core::config::write_bytes_atomic_preserving_permissions(
            &hooks_path,
            &serde_json::to_vec_pretty(&root)?,
        )
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
            codex_config_backed_up = true;
        }
        agenthub_core::config::write_bytes_atomic_preserving_permissions(
            &codex_config_path,
            codex_config_after.as_bytes(),
        )
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
        "backup_path": hooks_backed_up.then_some(backup_path),
        "codex_config_backup_path": codex_config_backed_up.then_some(codex_config_backup_path),
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

/// Edits the user's Codex configuration in place: everything outside
/// `features.hooks` and the stale hook state keeps its bytes.
fn ensure_codex_hooks_feature(text: &str) -> Result<String> {
    let mut document = if text.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        text.parse::<toml_edit::DocumentMut>()
            .context("parse Codex config.toml")?
    };
    let root = document.as_table_mut();
    if !root
        .get("features")
        .is_some_and(toml_edit::Item::is_table_like)
    {
        let mut features = toml_edit::Table::new();
        // A table without a position is printed next to the table visited before it,
        // which can be the middle of the file. Append it instead.
        features.set_position(usize::MAX);
        root.insert("features", toml_edit::Item::Table(features));
    }
    let features = root
        .get_mut("features")
        .and_then(toml_edit::Item::as_table_like_mut)
        .context("Codex config features must be a table")?;
    let mut changed = false;
    if features.get("hooks").and_then(toml_edit::Item::as_bool) != Some(true) {
        let mut enabled = toml_edit::Value::from(true);
        match features.get_mut("hooks") {
            // Replacing only the value keeps the key spelling and its trailing comment.
            Some(toml_edit::Item::Value(previous)) => {
                *enabled.decor_mut() = previous.decor().clone();
                *previous = enabled;
            }
            _ => {
                features.insert("hooks", toml_edit::Item::Value(enabled));
            }
        }
        changed = true;
    }
    changed |= prune_codex_hook_state(root);
    if !changed {
        return Ok(text.to_string());
    }
    Ok(document.to_string())
}

fn prune_codex_hook_state(root: &mut toml_edit::Table) -> bool {
    let Some(hooks) = root
        .get_mut("hooks")
        .and_then(toml_edit::Item::as_table_like_mut)
    else {
        return false;
    };
    let Some(state) = hooks
        .get_mut("state")
        .and_then(toml_edit::Item::as_table_like_mut)
    else {
        return false;
    };
    let stale_keys = state
        .iter()
        .map(|(key, _)| key)
        .filter(|key| is_codex_hook_state_key(key))
        .map(str::to_string)
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
            !agenthub_core::probe::is_agenthub_managed_hook_command(command)
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

fn refresh_managed_provider_hooks(_config: &Config) -> Result<()> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let platform = PlatformPaths::current()?;
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
    Ok(())
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
    let config = &db.sync_notification_channels_from_config(load_config)?;
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
    let record_plan = probe_service::probe_event_record_plan(event);
    let event = record_plan.event;
    let passive_sent =
        passive_unresolved_action_sent(db, record_plan.passive_marker_key.as_deref())?;
    let (feedback_claimed, claimed, mut bark) = notification_delivery::codex_with_claims(
        load_config,
        db,
        &event,
        feedback_key.as_deref(),
        passive_sent,
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

async fn serve(config_path: PathBuf) -> Result<()> {
    anyhow::ensure!(
        cfg!(target_os = "linux"),
        "Management API is available only on Linux"
    );
    let config = Config::load_runtime(&config_path)?;
    refresh_managed_provider_hooks(&config)?;
    let db = open_panel_db(&config)?;
    db.bind_legacy_panel_records()?;
    let state = AppState::new(config.clone(), db).with_config_path(config_path.clone());
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
    if let Err(error) = notification_delivery::retry_pending_with_config(db, load_config).await {
        tracing::warn!(
            "pending notification retry failed: {}",
            safe_probe_monitor_error(&error.to_string())
        );
    }
    let config =
        load_config().map_err(|_| anyhow::anyhow!("notification configuration unavailable"))?;
    let config = &config;
    let shared_codex_threads =
        if async_question_monitor::is_enabled(config) || question_monitor::is_enabled(config) {
            let paths = resolve_codex_paths(&config.codex.home).codex_paths();
            agenthub_core::read_tasks::spawn_blocking(move || {
                agenthub_core::codex::notification_thread_headers_with_identities(&paths)
                    .ok()
                    .map(|mut rows| {
                        rows.sort_by(|a, b| b.0.updated_at.cmp(&a.0.updated_at));
                        rows
                    })
            })
            .await
            .ok()
            .flatten()
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
    let logs_db = resolved.logs_db.clone();
    struct CachedProbeErrorScan {
        db_path: PathBuf,
        logs_db_path: PathBuf,
        source_stamp: agenthub_core::probe_error_monitor::ProbeErrorSourceStamp,
        cursor: ProbeErrorCursor,
    }
    static LAST_CAUGHT_UP_SCAN: std::sync::Mutex<Option<CachedProbeErrorScan>> =
        std::sync::Mutex::new(None);
    let stamp_before = agenthub_core::probe_error_monitor::ProbeErrorSourceStamp::read(&logs_db);
    let unchanged_cursor = stamp_before
        .exists()
        .then(|| {
            LAST_CAUGHT_UP_SCAN.lock().ok().and_then(|guard| {
                guard.as_ref().and_then(|cached| {
                    (cached.db_path == db.path()
                        && cached.logs_db_path == logs_db
                        && cached.source_stamp == stamp_before
                        && previous.as_ref() == Some(&cached.cursor))
                    .then(|| cached.cursor.clone())
                })
            })
        })
        .flatten();
    let scan = if let Some(cursor) = unchanged_cursor {
        agenthub_core::probe_error_monitor::ProbeErrorScan {
            cursor,
            baseline_only: false,
            rows_seen: 0,
            incidents: Vec::new(),
        }
    } else {
        let logs_db_for_scan = logs_db.clone();
        let previous_for_scan = previous.clone();
        let scan_result = agenthub_core::read_tasks::spawn_blocking(move || {
            scan_codex_turn_errors(
                &logs_db_for_scan,
                previous_for_scan.as_ref(),
                PROBE_ERROR_MONITOR_BATCH_LIMIT,
            )
        })
        .await
        .map_err(|err| anyhow::anyhow!("scan task join failed: {err}"))?;
        let scan = match scan_result {
            Ok(scan) => scan,
            Err(err) => {
                if let Ok(mut guard) = LAST_CAUGHT_UP_SCAN.lock() {
                    *guard = None;
                }
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
        if let Ok(mut guard) = LAST_CAUGHT_UP_SCAN.lock() {
            if !scan.baseline_only
                && scan.rows_seen < PROBE_ERROR_MONITOR_BATCH_LIMIT
                && stamp_before.exists()
                && agenthub_core::probe_error_monitor::ProbeErrorSourceStamp::read(&logs_db)
                    == stamp_before
            {
                *guard = Some(CachedProbeErrorScan {
                    db_path: db.path().to_path_buf(),
                    logs_db_path: logs_db,
                    source_stamp: stamp_before,
                    cursor: scan.cursor.clone(),
                });
            } else {
                *guard = None;
            }
        }
        scan
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
        let event = build_probe_event(
            config,
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
    let config = state.refresh_config()?;
    if !config.probe.enabled
        || !config.probe.notifications.enabled
        || !config.probe.notifications.notify_codex
        || (!config.probe.notifications.notify_reply_needed
            && !config.probe.notifications.notify_recoverable)
    {
        return Ok(0);
    }
    let mut recorded = 0usize;
    let mut active_markers = Vec::new();
    let codex_paths = state.codex_paths();
    let recent_limit = config.probe.recent_limit;
    let buckets = agenthub_core::read_tasks::spawn_blocking(move || {
        probe_service::probe_passive_scan_threads_with_paths(&codex_paths, recent_limit)
    })
    .await
    .map_err(|err| anyhow::anyhow!("passive thread scan join failed: {err}"))??;
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
            let mut event = build_probe_event(
                &config,
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
                .and_then(|time| (time).parse::<jiff::Timestamp>().ok())
                .map(|time| time.as_millisecond())
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
                record_probe_event_with_bark(&|| state.refresh_config(), &state.db, event).await?;
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
