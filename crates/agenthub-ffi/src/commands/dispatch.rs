use crate::{
    codex_desktop::{self, CodexDesktopBridge},
    host::HostBridge,
    overview::DesktopState,
    remote::{self, RemoteConnections},
    services::{
        jobs as job_service, probe as probe_service, settings as settings_service,
        threads as thread_service, updates as update_service,
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

use agenthub_core::services::commands as ids;
mod collaboration;
mod connections;
mod conversations;
mod maintenance;
mod providers;

macro_rules! desktop_handlers {
    ($( $id:path => $domain:ident::$handler:ident, )*) => {
        pub const REGISTERED_COMMAND_ENTRIES: &[(&str, &str)] = &[
            $(($id, concat!("dispatch::", stringify!($domain), "::", stringify!($handler))),)*
        ];
        pub async fn dispatch(ctx: DispatchContext<'_>, command: &str, args_json: &str) -> Result<String, String> {
            match command {
                $($id => $domain::$handler(ctx, args_json).await,)*
                _ => Err(format!("unsupported desktop command: {command}")),
            }
        }
    };
}

desktop_handlers! {
    ids::SYSTEM_CAPABILITIES => maintenance::system_capabilities,
    ids::THREADS_LIST => conversations::threads_list,
    ids::THREADS_DETAIL => conversations::threads_detail,
    ids::THREADS_SUBAGENT_DETAIL => conversations::threads_subagent_detail,
    ids::THREADS_BLOCKS => conversations::threads_blocks,
    ids::THREADS_ARCHIVE => conversations::threads_archive,
    ids::THREADS_RESTORE => conversations::threads_restore,
    ids::THREADS_RENAME => conversations::threads_rename,
    ids::THREADS_ACCOUNT_SCOPE_STATUS => conversations::threads_account_scope_status,
    ids::THREADS_ACCOUNT_SCOPE_CLEAR => conversations::threads_account_scope_clear,
    ids::JOBS_LIST => maintenance::jobs_list,
    ids::JOBS_DETAIL => maintenance::jobs_detail,
    ids::PROBE_STATUS => maintenance::probe_status,
    ids::GITHUB_CI_STATUS => maintenance::github_ci_status,
    ids::PROBE_SETTINGS_GET => maintenance::probe_settings_get,
    ids::PROBE_SETTINGS_SAVE => maintenance::probe_settings_save,
    ids::PROBE_EVENTS => maintenance::probe_events,
    ids::PROBE_BARK_TEST => maintenance::probe_bark_test,
    ids::PROBE_INSTALL_HOOKS => maintenance::probe_install_hooks,
    ids::PROBE_REMOVE_HOOKS => maintenance::probe_remove_hooks,
    ids::CLAUDE_HOOKS => providers::claude_hooks,
    ids::UPDATES_STATUS => maintenance::updates_status,
    ids::UPDATES_CHECK => maintenance::updates_check,
    ids::UPDATES_INSTALL => maintenance::updates_install,
    ids::CLEANUP_UNIFIED_DRY_RUN => maintenance::cleanup_unified_dry_run,
    ids::CLEANUP_UNIFIED_EXECUTE => maintenance::cleanup_unified_execute,
    ids::CLEANUP_HIDDEN_DRY_RUN => maintenance::cleanup_hidden_dry_run,
    ids::GROK_LIST => providers::grok_list,
    ids::GROK_DETAIL => providers::grok_detail,
    ids::GROK_RENAME => providers::grok_rename,
    ids::GROK_DELETE_PREVIEW => providers::grok_delete_preview,
    ids::GROK_DELETE_EXECUTE => providers::grok_delete_execute,
    ids::CLAUDE_LIST => providers::claude_list,
    ids::CLAUDE_DETAIL => providers::claude_detail,
    ids::CLAUDE_OWNERSHIP_DIAGNOSTICS => providers::claude_ownership_diagnostics,
    ids::CLAUDE_RENAME => providers::claude_rename,
    ids::CLAUDE_DELETE_PREVIEW => providers::claude_delete_preview,
    ids::CLAUDE_DELETE_EXECUTE => providers::claude_delete_execute,
    ids::ANTIGRAVITY_LIST => providers::antigravity_list,
    ids::ANTIGRAVITY_DETAIL => providers::antigravity_detail,
    ids::ANTIGRAVITY_RENAME => providers::antigravity_rename,
    ids::ANTIGRAVITY_ARCHIVE => providers::antigravity_archive,
    ids::ANTIGRAVITY_RESTORE => providers::antigravity_restore,
    ids::DSH_LIST => providers::dsh_list,
    ids::DSH_DETAIL => providers::dsh_detail,
    ids::DSH_RENAME => providers::dsh_rename,
    ids::DSH_ARCHIVE => providers::dsh_archive,
    ids::DSH_RESTORE => providers::dsh_restore,
    ids::GROKBOT_LIST => providers::grokbot_list,
    ids::GROKBOT_DETAIL => providers::grokbot_detail,
    ids::SESSIONS_SEARCH => conversations::sessions_search,
    ids::SESSIONS_ATTACHMENT_READ => conversations::sessions_attachment_read,
    ids::SESSIONS_BULK_PREVIEW => conversations::sessions_bulk_preview,
    ids::SESSIONS_BULK_EXECUTE => conversations::sessions_bulk_execute,
    ids::PLANS_SAVE => conversations::plans_save,
    ids::REMOTE_GET => connections::remote_get,
    ids::REMOTE_SSH_HOSTS => connections::remote_ssh_hosts,
    ids::REMOTE_SSH_DELETE => connections::remote_ssh_delete,
    ids::REMOTE_VERIFY => connections::remote_verify,
    ids::REMOTE_SAVE => connections::remote_save,
    ids::REMOTE_INSTALL => connections::remote_install,
    ids::REMOTE_REMOVE => connections::remote_remove,
    ids::REMOTE_SELECT => connections::remote_select,
    ids::REMOTE_INVOKE => connections::remote_invoke,
    ids::CODEX_DESKTOP_ATTACH => collaboration::codex_desktop_attach,
    ids::CODEX_DESKTOP_ACTION => collaboration::codex_desktop_action,
    ids::CODEX_DESKTOP_DETACH => collaboration::codex_desktop_detach,
    ids::CODEX_DESKTOP_OPEN => collaboration::codex_desktop_open,
    ids::CODEX_DESKTOP_CATALOG => collaboration::codex_desktop_catalog,
    ids::CODEX_DESKTOP_CHOOSE => collaboration::codex_desktop_choose,
    ids::CODEX_DESKTOP_CREATE => collaboration::codex_desktop_create,
    ids::CODEX_DESKTOP_ATTACHMENTS => collaboration::codex_desktop_attachments,
    ids::CODEX_DESKTOP_DIAGNOSTICS => collaboration::codex_desktop_diagnostics,
}
