use crate::{
    codex_desktop::{self, CodexDesktopBridge},
    host::HostBridge,
    overview::DesktopState,
    remote::{self, RemoteConnections},
    services::{
        jobs::{self as job_service, DesktopJobDetailRequest, DesktopJobsRequest},
        probe as probe_service,
        settings::{self as settings_service, DesktopProbeEventsRequest},
        system as system_service,
        threads::{
            self as thread_service, DesktopRenameThreadRequest, DesktopThreadIdRequest,
            ThreadBlocksRequest, ThreadDetailRequest, ThreadListRequest,
        },
        updates as update_service,
    },
};
use agenthub_core::{
    archive::hidden_scan::HiddenScanRequest,
    claude::{
        hooks::{ClaudeHooksRequest, HookProvider},
        ClaudeDeleteRequest, ClaudeDetailRequest,
    },
    codex::subagents::SubagentDetailRequest,
    grok::GrokDeleteRequest,
    platform::PlatformPaths,
    provider_sessions::{
        ProviderDetailRequest, ProviderListRequest, ProviderRenameRequest, ProviderSessionRequest,
    },
    services::{
        cleanup::UnifiedCleanupExecuteRequest,
        codex_desktop::*,
        plans::PlanSaveRequest,
        probe::ProbeAction,
        remote::*,
        search::SessionSearchRequest,
        sessions::{SessionBatchExecuteRequest, SessionBatchRequest},
        settings::ProbeSettingsSaveRequest,
        use_cases::AgentHubUseCases,
    },
    user_message::SessionAttachmentRequest,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

pub const REGISTERED_COMMAND_ENTRIES: &[(&str, &str)] = &[
    (
        "system.capabilities",
        "commands::system::getSystemCapabilities",
    ),
    ("threads.list", "commands::threads::listThreads"),
    ("threads.detail", "commands::threads::getThread"),
    (
        "threads.subagentDetail",
        "commands::threads::getSubagentDetail",
    ),
    ("threads.blocks", "commands::threads::getThreadBlocks"),
    ("threads.archive", "commands::threads::archiveThread"),
    ("threads.restore", "commands::threads::restoreThread"),
    ("threads.rename", "commands::threads::renameThread"),
    (
        "threads.accountScopeStatus",
        "commands::threads::accountScopeStatus",
    ),
    (
        "threads.accountScopeClear",
        "commands::threads::accountScopeClear",
    ),
    ("jobs.list", "commands::jobs::listJobs"),
    ("jobs.detail", "commands::jobs::getJob"),
    ("probe.status", "commands::probe::getProbeStatus"),
    ("github.ci.status", "commands::probe::getGithubCiStatus"),
    ("probe.settings.get", "commands::settings::getProbeSettings"),
    (
        "probe.settings.save",
        "commands::settings::saveProbeSettings",
    ),
    ("probe.events", "commands::settings::getProbeEvents"),
    ("probe.barkTest", "commands::settings::probeBarkTest"),
    (
        "probe.installHooks",
        "commands::settings::probeInstallHooks",
    ),
    ("probe.removeHooks", "commands::settings::probeRemoveHooks"),
    ("claude.hooks", "commands::settings::claudeHooks"),
    ("updates.status", "commands::updates::getUpdateStatus"),
    ("updates.check", "commands::updates::updatesCheck"),
    ("updates.install", "commands::updates::updatesInstall"),
    (
        "cleanup.unifiedDryRun",
        "commands::settings::dryRunUnifiedCleanup",
    ),
    (
        "cleanup.unifiedExecute",
        "commands::settings::startUnifiedCleanup",
    ),
    (
        "cleanup.hiddenDryRun",
        "commands::settings::dryRunHiddenThreadDelete",
    ),
    ("grok.list", "commands::grok::listGrokSessions"),
    ("grok.detail", "commands::grok::getGrokSession"),
    ("grok.rename", "commands::grok::renameGrokSession"),
    (
        "grok.deletePreview",
        "commands::grok::previewGrokSessionDelete",
    ),
    ("grok.deleteExecute", "commands::grok::deleteGrokSession"),
    ("claude.list", "commands::claude::listClaudeSessions"),
    ("claude.detail", "commands::claude::getClaudeSession"),
    (
        "claude.ownershipDiagnostics",
        "commands::claude::claudeOwnershipDiagnostics",
    ),
    ("claude.rename", "commands::claude::renameClaudeSession"),
    (
        "claude.deletePreview",
        "commands::claude::previewClaudeSessionDelete",
    ),
    (
        "claude.deleteExecute",
        "commands::claude::deleteClaudeSession",
    ),
    (
        "antigravity.list",
        "commands::provider_sessions::listAntigravitySessions",
    ),
    (
        "antigravity.detail",
        "commands::provider_sessions::getAntigravitySession",
    ),
    (
        "antigravity.rename",
        "commands::provider_sessions::renameAntigravitySession",
    ),
    (
        "antigravity.archive",
        "commands::provider_sessions::archiveAntigravitySession",
    ),
    (
        "antigravity.restore",
        "commands::provider_sessions::restoreAntigravitySession",
    ),
    ("dsh.list", "commands::provider_sessions::listDshSessions"),
    ("dsh.detail", "commands::provider_sessions::getDshSession"),
    (
        "dsh.rename",
        "commands::provider_sessions::renameDshSession",
    ),
    (
        "dsh.archive",
        "commands::provider_sessions::archiveDshSession",
    ),
    (
        "dsh.restore",
        "commands::provider_sessions::restoreDshSession",
    ),
    (
        "grokbot.list",
        "commands::provider_sessions::listGrokBotSessions",
    ),
    (
        "grokbot.detail",
        "commands::provider_sessions::getGrokBotSession",
    ),
    ("sessions.search", "commands::sessions::searchSessions"),
    (
        "sessions.attachmentRead",
        "commands::sessions::readSessionAttachment",
    ),
    (
        "sessions.bulkPreview",
        "commands::sessions::previewSessionBatch",
    ),
    (
        "sessions.bulkExecute",
        "commands::sessions::executeSessionBatch",
    ),
    ("plans.save", "commands::plans::savePlanMarkdown"),
    ("remote.get", "remote::remoteGet"),
    ("remote.sshHosts", "remote::remoteSshHosts"),
    ("remote.sshDelete", "remote::remoteSshDelete"),
    ("remote.verify", "remote::remoteVerify"),
    ("remote.save", "remote::remoteSave"),
    ("remote.install", "remote::remoteInstall"),
    ("remote.remove", "remote::remoteRemove"),
    ("remote.select", "remote::remoteSelect"),
    ("remote.invoke", "remote::remoteInvoke"),
    ("codex.desktop.attach", "codex_desktop::attach"),
    ("codex.desktop.action", "codex_desktop::action"),
    ("codex.desktop.detach", "codex_desktop::detach"),
    ("codex.desktop.open", "codex_desktop::open"),
    ("codex.desktop.catalog", "codex_desktop::catalog"),
    ("codex.desktop.choose", "codex_desktop::choose"),
    ("codex.desktop.create", "codex_desktop::create"),
    ("codex.desktop.attachments", "codex_desktop::attachments"),
    ("codex.desktop.diagnostics", "codex_desktop::diagnostics"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WrappedRequest<T> {
    pub(crate) request: T,
}

#[derive(Default, Deserialize)]
struct OptionalWrappedRequest<T> {
    request: Option<T>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadDetailOptions {
    limit: Option<usize>,
    before: Option<String>,
    full: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsListArgs {
    archived: Option<bool>,
    q: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadDetailArgs {
    id: String,
    options: Option<ThreadDetailOptions>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadIdArgs {
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadRenameArgs {
    thread_id: String,
    name: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LimitU32Args {
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobDetailArgs {
    id: String,
    include_output: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbeStatusArgs {
    refresh: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubCiArgs {
    #[serde(default)]
    refresh: Option<bool>,
    #[serde(default)]
    cached_only: Option<bool>,
}

#[derive(Deserialize)]
struct ProbeSettingsSaveArgs {
    settings: ProbeSettingsSaveRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeHooksArgs {
    request: ClaudeHooksRequest,
    provider: Option<HookProvider>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryLimitArgs {
    q: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrokDetailArgs {
    id: String,
    limit: Option<usize>,
    before: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrokRenameArgs {
    id: String,
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdOnlyArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionKeyOnlyArgs {
    session_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeDetailArgs {
    session_key: String,
    agent_id: Option<String>,
    limit: Option<usize>,
    before: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeRenameArgs {
    session_key: String,
    title: String,
}

fn parse_args<T: DeserializeOwned>(args_json: &str) -> Result<T, String> {
    let trimmed = args_json.trim();
    let effective = if trimmed.is_empty() { "{}" } else { trimmed };
    serde_json::from_str(effective).map_err(|err| format!("invalid command arguments: {err}"))
}

fn encode<T: Serialize>(result: Result<T, String>) -> Result<String, String> {
    result.and_then(|val| serde_json::to_string(&val).map_err(|err| err.to_string()))
}

pub struct DispatchContext<'a> {
    pub state: &'a DesktopState,
    pub remote: &'a RemoteConnections,
    pub bridge: &'a CodexDesktopBridge,
    pub host: &'a HostBridge,
    pub isolated: bool,
}

pub async fn dispatch(
    ctx: DispatchContext<'_>,
    command: &str,
    args_json: &str,
) -> Result<String, String> {
    match command {
        "system.capabilities" => {
            encode(system_service::system_capabilities(ctx.state).map_err(|e| e.to_string()))
        }
        "threads.list" => {
            let args: ThreadsListArgs = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    thread_service::threads_with_state(
                        &state,
                        ThreadListRequest {
                            archived: args.archived,
                            query: args.q,
                            limit: args.limit,
                        },
                    )
                })
                .await,
            )
        }
        "threads.detail" => {
            let args: ThreadDetailArgs = parse_args(args_json)?;
            let opts = args.options.unwrap_or_default();
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    thread_service::thread_detail_with_state(
                        &state,
                        ThreadDetailRequest {
                            id: args.id,
                            limit: opts.limit,
                            before: opts.before,
                            full: opts.full,
                        },
                    )
                })
                .await,
            )
        }
        "threads.subagentDetail" => {
            let args: WrappedRequest<SubagentDetailRequest> = parse_args(args_json)?;
            let paths = ctx.state.codex_paths();
            let platform = ctx.state.platform().clone();
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&platform)
                        .threads()
                        .subagent_detail(&paths, &args.request)
                })
                .await,
            )
        }
        "threads.blocks" => {
            let args: ThreadDetailArgs = parse_args(args_json)?;
            let opts = args.options.unwrap_or_default();
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    thread_service::thread_blocks_with_state(
                        &state,
                        ThreadBlocksRequest {
                            id: args.id,
                            limit: opts.limit,
                            before: opts.before,
                        },
                    )
                })
                .await,
            )
        }
        "threads.archive" => {
            let args: ThreadIdArgs = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    thread_service::archive_thread_with_state(
                        &state,
                        DesktopThreadIdRequest {
                            thread_id: args.thread_id,
                        },
                    )
                })
                .await,
            )
        }
        "threads.restore" => {
            let args: ThreadIdArgs = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    thread_service::restore_thread_with_state(
                        &state,
                        DesktopThreadIdRequest {
                            thread_id: args.thread_id,
                        },
                    )
                })
                .await,
            )
        }
        "threads.rename" => {
            let args: ThreadRenameArgs = parse_args(args_json)?;
            encode(
                thread_service::rename_thread_with_state(
                    ctx.state,
                    DesktopRenameThreadRequest {
                        thread_id: args.thread_id,
                        name: args.name,
                    },
                )
                .await
                .map_err(|e| e.to_string()),
            )
        }
        "threads.accountScopeStatus" => {
            let paths = ctx.state.codex_paths();
            let platform = ctx.state.platform().clone();
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&platform)
                        .threads()
                        .account_scope_status(&paths)
                })
                .await,
            )
        }
        "threads.accountScopeClear" => {
            let paths = ctx.state.codex_paths();
            let platform = ctx.state.platform().clone();
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&platform)
                        .threads()
                        .account_scope_clear(&paths)
                })
                .await,
            )
        }
        "jobs.list" => {
            let args: LimitU32Args = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    job_service::jobs_with_state(&state, DesktopJobsRequest { limit: args.limit })
                })
                .await,
            )
        }
        "jobs.detail" => {
            let args: JobDetailArgs = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    job_service::job_detail_with_state(
                        &state,
                        DesktopJobDetailRequest {
                            id: args.id,
                            include_output: args.include_output,
                        },
                    )
                })
                .await,
            )
        }
        "probe.status" => {
            let args: ProbeStatusArgs = parse_args(args_json)?;
            let _ = args.refresh;
            encode(
                probe_service::desktop_probe_status_with_state(ctx.state)
                    .await
                    .map_err(|e| e.to_string()),
            )
        }
        "github.ci.status" => {
            let args: GithubCiArgs = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                tokio::task::spawn_blocking(move || {
                    probe_service::desktop_github_ci_status(
                        &state,
                        agenthub_core::github_ci::Request {
                            refresh: args.refresh.unwrap_or(false),
                            cached_only: args.cached_only.unwrap_or(false),
                        },
                    )
                })
                .await
                .map_err(|e| e.to_string())?,
            )
        }
        "probe.settings.get" => {
            let state = ctx.state.clone();
            encode(
                super::blocking(move || settings_service::probe_settings_with_state(&state)).await,
            )
        }
        "probe.settings.save" => {
            let args: ProbeSettingsSaveArgs = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    settings_service::probe_save_settings_with_state(&state, args.settings)
                })
                .await,
            )
        }
        "probe.events" => {
            let args: LimitU32Args = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    settings_service::probe_events_with_state(
                        &state,
                        DesktopProbeEventsRequest { limit: args.limit },
                    )
                })
                .await,
            )
        }
        "probe.barkTest" => {
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    settings_service::probe_action_with_state(&state, ProbeAction::BarkTest)
                })
                .await,
            )
        }
        "probe.installHooks" => {
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    settings_service::probe_action_with_state(&state, ProbeAction::InstallHooks)
                })
                .await,
            )
        }
        "probe.removeHooks" => {
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    settings_service::probe_action_with_state(&state, ProbeAction::RemoveHooks)
                })
                .await,
            )
        }
        "claude.hooks" => {
            let args: ClaudeHooksArgs = parse_args(args_json)?;
            let state = ctx.state.clone();
            let provider = args.provider.unwrap_or_default();
            encode(
                agenthub_core::read_tasks::spawn_blocking(move || {
                    AgentHubUseCases::new(state.platform())
                        .probe()
                        .provider_hooks(provider, args.request)
                })
                .await
                .map_err(|_| "Hooks 设置任务未完成".to_string())?
                .map_err(|e| e.to_string()),
            )
        }
        "updates.status" => {
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    update_service::desktop_update_status_with_state(&state, None, None)
                })
                .await,
            )
        }
        "updates.check" => encode(update_service::check_update_status(ctx.state).await),
        "updates.install" => encode(
            update_service::install_update_and_restart(ctx.state, ctx.host, ctx.isolated).await,
        ),
        "cleanup.unifiedDryRun" => {
            let state = ctx.state.clone();
            encode(
                agenthub_core::read_tasks::spawn_blocking(move || {
                    let paths =
                        agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(
                            state.codex_paths(),
                        );
                    AgentHubUseCases::new(state.platform())
                        .cleanup()
                        .unified_dry_run(&paths)
                        .map_err(|e| e.to_string())
                })
                .await
                .map_err(|_| "全平台清理预览任务未完成".to_string())?,
            )
        }
        "cleanup.unifiedExecute" => {
            let args: WrappedRequest<UnifiedCleanupExecuteRequest> = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                agenthub_core::read_tasks::spawn_blocking(move || {
                    let paths =
                        agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(
                            state.codex_paths(),
                        );
                    AgentHubUseCases::new(state.platform())
                        .cleanup()
                        .unified_execute(&paths, args.request)
                        .map_err(|e| e.to_string())
                })
                .await
                .map_err(|_| "全平台清理任务未完成".to_string())?,
            )
        }
        "cleanup.hiddenDryRun" => {
            let args: WrappedRequest<HiddenScanRequest> = parse_args(args_json)?;
            encode(
                AgentHubUseCases::new(ctx.state.platform())
                    .cleanup()
                    .scan_hidden(ctx.state.codex_paths(), "desktop".into(), args.request)
                    .await
                    .map_err(|e| e.to_string()),
            )
        }
        "grok.list" => {
            let args: QueryLimitArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .grok()
                        .list(args.limit.unwrap_or(100), args.q.as_deref())
                })
                .await,
            )
        }
        "grok.detail" => {
            let args: GrokDetailArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .grok()
                        .page(&args.id, args.limit, args.before.as_deref())
                })
                .await,
            )
        }
        "grok.rename" => {
            let args: GrokRenameArgs = parse_args(args_json)?;
            encode(
                AgentHubUseCases::new(&PlatformPaths::desktop_current())
                    .grok()
                    .rename(&args.id, &args.title)
                    .await
                    .map_err(|e| e.to_string()),
            )
        }
        "grok.deletePreview" => {
            let args: IdOnlyArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .grok()
                        .delete_preview(&args.id)
                })
                .await,
            )
        }
        "grok.deleteExecute" => {
            let args: WrappedRequest<GrokDeleteRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .grok()
                        .delete_execute(args.request)
                })
                .await,
            )
        }
        "claude.list" => {
            let args: QueryLimitArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .claude()
                        .list(args.limit.unwrap_or(100), args.q.as_deref())
                })
                .await,
            )
        }
        "claude.detail" => {
            let args: ClaudeDetailArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .claude()
                        .detail(ClaudeDetailRequest {
                            agent_id: args.agent_id,
                            session_key: args.session_key,
                            limit: args.limit,
                            before: args.before,
                        })
                })
                .await,
            )
        }
        "claude.ownershipDiagnostics" => {
            let args: SessionKeyOnlyArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .claude()
                        .ownership_diagnostics(&args.session_key)
                })
                .await,
            )
        }
        "claude.rename" => {
            let args: ClaudeRenameArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .claude()
                        .rename(&args.session_key, &args.title)
                })
                .await,
            )
        }
        "claude.deletePreview" => {
            let args: SessionKeyOnlyArgs = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .claude()
                        .delete_preview(&args.session_key)
                })
                .await,
            )
        }
        "claude.deleteExecute" => {
            let args: WrappedRequest<ClaudeDeleteRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .claude()
                        .delete_execute(args.request)
                })
                .await,
            )
        }
        "antigravity.list" => {
            let args: OptionalWrappedRequest<ProviderListRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .antigravity()
                        .list(args.request.unwrap_or_default())
                })
                .await,
            )
        }
        "antigravity.detail" => {
            let args: WrappedRequest<ProviderDetailRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .antigravity()
                        .detail(args.request)
                })
                .await,
            )
        }
        "antigravity.rename" => {
            let args: WrappedRequest<ProviderRenameRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .antigravity()
                        .rename(&args.request.session_key, &args.request.title)
                })
                .await,
            )
        }
        "antigravity.archive" => {
            let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .antigravity()
                        .archive(&args.request.session_key)
                })
                .await,
            )
        }
        "antigravity.restore" => {
            let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .antigravity()
                        .restore(&args.request.session_key)
                })
                .await,
            )
        }
        "dsh.list" => {
            let args: OptionalWrappedRequest<ProviderListRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .dsh()
                        .list(args.request.unwrap_or_default())
                })
                .await,
            )
        }
        "dsh.detail" => {
            let args: WrappedRequest<ProviderDetailRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .dsh()
                        .detail(args.request)
                })
                .await,
            )
        }
        "dsh.rename" => {
            let args: WrappedRequest<ProviderRenameRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .dsh()
                        .rename(&args.request.session_key, &args.request.title)
                })
                .await,
            )
        }
        "dsh.archive" => {
            let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .dsh()
                        .archive(&args.request.session_key)
                })
                .await,
            )
        }
        "dsh.restore" => {
            let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .dsh()
                        .restore(&args.request.session_key)
                })
                .await,
            )
        }
        "grokbot.list" => {
            let args: OptionalWrappedRequest<ProviderListRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .grokbot()
                        .list(args.request.unwrap_or_default())
                })
                .await,
            )
        }
        "grokbot.detail" => {
            let args: WrappedRequest<ProviderDetailRequest> = parse_args(args_json)?;
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(&PlatformPaths::desktop_current())
                        .grokbot()
                        .detail(args.request)
                })
                .await,
            )
        }
        "sessions.search" => {
            let args: WrappedRequest<SessionSearchRequest> = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(state.platform())
                        .search(state.codex_paths())
                        .search(args.request)
                })
                .await,
            )
        }
        "sessions.attachmentRead" => {
            let args: WrappedRequest<SessionAttachmentRequest> = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(state.platform())
                        .sessions(state.codex_paths())
                        .attachment_read(args.request)
                })
                .await,
            )
        }
        "sessions.bulkPreview" => {
            let args: WrappedRequest<SessionBatchRequest> = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                super::blocking(move || {
                    AgentHubUseCases::new(state.platform())
                        .sessions(state.codex_paths())
                        .bulk_preview(args.request)
                })
                .await,
            )
        }
        "sessions.bulkExecute" => {
            let args: WrappedRequest<SessionBatchExecuteRequest> = parse_args(args_json)?;
            let state = ctx.state.clone();
            encode(
                tokio::task::spawn_blocking(move || {
                    AgentHubUseCases::new(state.platform())
                        .sessions(state.codex_paths())
                        .bulk_execute(args.request)
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string()),
            )
        }
        "plans.save" => {
            let args: WrappedRequest<PlanSaveRequest> = parse_args(args_json)?;
            encode(super::plans::savePlanMarkdown(args.request).await)
        }
        "remote.get" => encode(remote::remoteGet(ctx.remote).await),
        "remote.sshHosts" => encode(remote::remoteSshHosts().await),
        "remote.sshDelete" => {
            let args: WrappedRequest<SshDeleteRequest> = parse_args(args_json)?;
            encode(remote::remoteSshDelete(ctx.remote, args.request).await)
        }
        "remote.verify" => {
            let args: WrappedRequest<RemoteConnectionCredentials> = parse_args(args_json)?;
            encode(remote::remoteVerify(ctx.remote, args.request).await)
        }
        "remote.save" => {
            let args: WrappedRequest<RemoteConnectionCredentials> = parse_args(args_json)?;
            encode(remote::remoteSave(ctx.remote, args.request).await)
        }
        "remote.install" => {
            let args: WrappedRequest<RemoteConnectionCredentials> = parse_args(args_json)?;
            encode(remote::remoteInstall(ctx.remote, args.request).await)
        }
        "remote.remove" => {
            let args: WrappedRequest<RemoteRevisionRequest> = parse_args(args_json)?;
            encode(remote::remoteRemove(ctx.remote, args.request))
        }
        "remote.select" => {
            let args: WrappedRequest<RemoteSelectionRequest> = parse_args(args_json)?;
            encode(remote::remoteSelect(ctx.remote, args.request).await)
        }
        "remote.invoke" => {
            let args: WrappedRequest<RemoteInvokeRequest> = parse_args(args_json)?;
            let value: Result<Value, String> = remote::remoteInvoke(ctx.remote, args.request).await;
            encode(value)
        }
        "codex.desktop.attach" => Err(
            "codex.desktop.attach requires agenthub_ffi_desktop_attach stream entrypoint".into(),
        ),
        "codex.desktop.action" => {
            let args: WrappedRequest<DesktopActionRequest> = parse_args(args_json)?;
            encode(codex_desktop::action(ctx.state, ctx.remote, ctx.bridge, args.request).await)
        }
        "codex.desktop.detach" => {
            let args: WrappedRequest<DesktopSessionRequest> = parse_args(args_json)?;
            encode(codex_desktop::detach(ctx.bridge, args.request).await)
        }
        "codex.desktop.open" => {
            let args: WrappedRequest<DesktopThreadRequest> = parse_args(args_json)?;
            encode(codex_desktop::open(ctx.host, ctx.state, ctx.remote, args.request).await)
        }
        "codex.desktop.catalog" => {
            let args: WrappedRequest<DesktopCatalogRequest> = parse_args(args_json)?;
            encode(codex_desktop::catalog(ctx.state, ctx.remote, ctx.bridge, args.request).await)
        }
        "codex.desktop.choose" => {
            let args: WrappedRequest<DesktopChooseRequest> = parse_args(args_json)?;
            encode(codex_desktop::choose(ctx.host, ctx.remote, ctx.bridge, args.request).await)
        }
        "codex.desktop.create" => {
            let args: WrappedRequest<DesktopCreateRequest> = parse_args(args_json)?;
            encode(
                codex_desktop::create(ctx.host, ctx.state, ctx.remote, ctx.bridge, args.request)
                    .await,
            )
        }
        "codex.desktop.attachments" => {
            let args: WrappedRequest<DesktopAttachmentRequest> = parse_args(args_json)?;
            if matches!(&args.request.action, DesktopAttachmentAction::Bind { .. }) {
                return Err(
                    "codex.desktop.attachments bind requires agenthub_ffi_desktop_attachments"
                        .into(),
                );
            }
            encode(
                codex_desktop::attachments::handle(
                    ctx.host,
                    ctx.state,
                    ctx.remote,
                    ctx.bridge,
                    args.request,
                    None,
                )
                .await,
            )
        }
        "codex.desktop.diagnostics" => {
            let args: WrappedRequest<DesktopDiagnosticsRequest> = parse_args(args_json)?;
            encode(codex_desktop::diagnostics(ctx.host, ctx.remote, args.request).await)
        }
        _ => Err(format!("unsupported desktop command: {command}")),
    }
}
