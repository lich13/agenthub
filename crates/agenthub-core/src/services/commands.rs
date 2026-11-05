pub const SESSIONS_ATTACHMENT_READ: &str = "sessions.attachmentRead";
pub const SESSIONS_SEARCH: &str = "sessions.search";
pub const GROK_LIST: &str = "grok.list";
pub const SESSIONS_BULK_PREVIEW: &str = "sessions.bulkPreview";
pub const SESSIONS_BULK_EXECUTE: &str = "sessions.bulkExecute";
pub const GROK_DETAIL: &str = "grok.detail";
pub const GROK_RENAME: &str = "grok.rename";
pub const GROK_DELETE_PREVIEW: &str = "grok.deletePreview";
pub const GROK_DELETE_EXECUTE: &str = "grok.deleteExecute";
pub const CLAUDE_LIST: &str = "claude.list";
pub const CLAUDE_DETAIL: &str = "claude.detail";
pub const CLAUDE_RENAME: &str = "claude.rename";
pub const CLAUDE_DELETE_PREVIEW: &str = "claude.deletePreview";
pub const CLAUDE_DELETE_EXECUTE: &str = "claude.deleteExecute";
pub const ANTIGRAVITY_LIST: &str = "antigravity.list";
pub const ANTIGRAVITY_DETAIL: &str = "antigravity.detail";
pub const ANTIGRAVITY_RENAME: &str = "antigravity.rename";
pub const ANTIGRAVITY_ARCHIVE: &str = "antigravity.archive";
pub const ANTIGRAVITY_RESTORE: &str = "antigravity.restore";
pub const DSH_LIST: &str = "dsh.list";
pub const DSH_DETAIL: &str = "dsh.detail";
pub const DSH_RENAME: &str = "dsh.rename";
pub const DSH_ARCHIVE: &str = "dsh.archive";
pub const DSH_RESTORE: &str = "dsh.restore";
pub const SYSTEM_CAPABILITIES: &str = "system.capabilities";
pub const SYSTEM_VERSION: &str = "system.version";
pub const SYSTEM_PLATFORM: &str = "system.platform";
pub const SYSTEM_PROVIDERS: &str = "system.providers";
pub const THREADS_LIST: &str = "threads.list";
pub const THREADS_SUBAGENT_DETAIL: &str = "threads.subagentDetail";
pub const THREADS_DETAIL: &str = "threads.detail";
pub const THREADS_BLOCKS: &str = "threads.blocks";
pub const THREADS_ARCHIVE: &str = "threads.archive";
pub const THREADS_RESTORE: &str = "threads.restore";
pub const THREADS_RENAME: &str = "threads.rename";
pub const JOBS_LIST: &str = "jobs.list";
pub const JOBS_DETAIL: &str = "jobs.detail";
pub const PROBE_STATUS: &str = "probe.status";
pub const PROBE_SETTINGS_GET: &str = "probe.settings.get";
pub const PROBE_SETTINGS_SAVE: &str = "probe.settings.save";
pub const PROBE_EVENTS: &str = "probe.events";
pub const PROBE_BARK_TEST: &str = "probe.barkTest";
pub const PROBE_INSTALL_HOOKS: &str = "probe.installHooks";
pub const PROBE_REMOVE_HOOKS: &str = "probe.removeHooks";
pub const UPDATES_STATUS: &str = "updates.status";
pub const UPDATES_CHECK: &str = "updates.check";
pub const UPDATES_INSTALL: &str = "updates.install";
pub const UPDATES_PRUNE: &str = "updates.prune";
pub const CLEANUP_UNIFIED_DRY_RUN: &str = "cleanup.unifiedDryRun";
pub const CLEANUP_UNIFIED_EXECUTE: &str = "cleanup.unifiedExecute";
pub const CLEANUP_HIDDEN_DRY_RUN: &str = "cleanup.hiddenDryRun";

pub const ALLOWED_RPC_COMMANDS: &[&str] = &[
    SYSTEM_CAPABILITIES,
    SYSTEM_VERSION,
    SYSTEM_PLATFORM,
    SYSTEM_PROVIDERS,
    THREADS_LIST,
    THREADS_DETAIL,
    THREADS_SUBAGENT_DETAIL,
    THREADS_BLOCKS,
    THREADS_ARCHIVE,
    THREADS_RESTORE,
    THREADS_RENAME,
    JOBS_LIST,
    JOBS_DETAIL,
    PROBE_STATUS,
    PROBE_SETTINGS_GET,
    PROBE_SETTINGS_SAVE,
    PROBE_EVENTS,
    PROBE_BARK_TEST,
    PROBE_INSTALL_HOOKS,
    PROBE_REMOVE_HOOKS,
    UPDATES_STATUS,
    UPDATES_CHECK,
    UPDATES_INSTALL,
    UPDATES_PRUNE,
    CLEANUP_UNIFIED_DRY_RUN,
    CLEANUP_UNIFIED_EXECUTE,
    CLEANUP_HIDDEN_DRY_RUN,
    GROK_LIST,
    GROK_DETAIL,
    GROK_RENAME,
    GROK_DELETE_PREVIEW,
    GROK_DELETE_EXECUTE,
    CLAUDE_LIST,
    CLAUDE_DETAIL,
    CLAUDE_RENAME,
    CLAUDE_DELETE_PREVIEW,
    CLAUDE_DELETE_EXECUTE,
    ANTIGRAVITY_LIST,
    ANTIGRAVITY_DETAIL,
    ANTIGRAVITY_RENAME,
    ANTIGRAVITY_ARCHIVE,
    ANTIGRAVITY_RESTORE,
    DSH_LIST,
    DSH_DETAIL,
    DSH_RENAME,
    DSH_ARCHIVE,
    DSH_RESTORE,
    SESSIONS_ATTACHMENT_READ,
    SESSIONS_SEARCH,
    SESSIONS_BULK_PREVIEW,
    SESSIONS_BULK_EXECUTE,
];

/// Unknown commands fail closed as writes. Remote mutations are never retried.
pub fn is_mutating_rpc_command(command: &str) -> bool {
    !matches!(
        command,
        SYSTEM_CAPABILITIES
            | SYSTEM_VERSION
            | SYSTEM_PLATFORM
            | SYSTEM_PROVIDERS
            | THREADS_LIST
            | THREADS_DETAIL
            | THREADS_SUBAGENT_DETAIL
            | THREADS_BLOCKS
            | JOBS_LIST
            | JOBS_DETAIL
            | PROBE_STATUS
            | PROBE_SETTINGS_GET
            | PROBE_EVENTS
            | UPDATES_STATUS
            | CLEANUP_UNIFIED_DRY_RUN
            | CLEANUP_HIDDEN_DRY_RUN
            | GROK_LIST
            | GROK_DETAIL
            | GROK_DELETE_PREVIEW
            | CLAUDE_LIST
            | CLAUDE_DETAIL
            | CLAUDE_DELETE_PREVIEW
            | ANTIGRAVITY_LIST
            | ANTIGRAVITY_DETAIL
            | DSH_LIST
            | DSH_DETAIL
            | SESSIONS_ATTACHMENT_READ
            | SESSIONS_SEARCH
            | SESSIONS_BULK_PREVIEW
    )
}

pub fn is_allowed_rpc_command(command: &str) -> bool {
    ALLOWED_RPC_COMMANDS.contains(&command)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rpc_commands_are_dot_named_unique_and_unknown_commands_fail_closed() {
        let mut seen = std::collections::HashSet::new();
        for command in ALLOWED_RPC_COMMANDS {
            assert!(command.contains('.'));
            assert!(seen.insert(command));
        }
        assert!(!is_allowed_rpc_command("unknown.command"));
        assert!(is_mutating_rpc_command("unknown.command"));
    }
}
