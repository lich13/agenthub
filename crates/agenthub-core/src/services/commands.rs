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
pub const AUTH_PUBLIC_SETTINGS: &str = "auth.publicSettings";
pub const AUTH_LOGIN: &str = "auth.login";
pub const AUTH_LOGOUT: &str = "auth.logout";
pub const AUTH_ME: &str = "auth.me";
pub const SECURITY_GET: &str = "security.get";
pub const SECURITY_SAVE: &str = "security.save";
pub const SECURITY_CHANGE_PASSWORD: &str = "security.changePassword";
pub const SYSTEM_CAPABILITIES: &str = "system.capabilities";
pub const SYSTEM_VERSION: &str = "system.version";
pub const SYSTEM_PLATFORM: &str = "system.platform";
pub const SYSTEM_PROVIDERS: &str = "system.providers";
pub const SYSTEM_PLUGINS: &str = "system.plugins";
pub const SYSTEM_MODELS: &str = "system.models";
pub const SYSTEM_PERMISSION_PROFILES: &str = "system.permissionProfiles";
pub const SYSTEM_CODEX_CONFIG: &str = "system.codexConfig";
pub const SYSTEM_CLAUDE_CODE_OVERVIEW: &str = "system.claudeCodeOverview";
pub const THREADS_LIST: &str = "threads.list";
pub const THREADS_SUBAGENT_DETAIL: &str = "threads.subagentDetail";
pub const THREADS_DETAIL: &str = "threads.detail";
pub const THREADS_BLOCKS: &str = "threads.blocks";
pub const THREADS_CREATE: &str = "threads.create";
pub const THREADS_SEND: &str = "threads.send";
pub const THREADS_STEER: &str = "threads.steer";
pub const THREADS_STOP: &str = "threads.stop";
pub const THREADS_ARCHIVE: &str = "threads.archive";
pub const THREADS_RESTORE: &str = "threads.restore";
pub const THREADS_RENAME: &str = "threads.rename";
pub const THREADS_FORK: &str = "threads.fork";
pub const THREADS_FOLLOWUPS_LIST: &str = "threads.followups.list";
pub const THREADS_FOLLOWUPS_ENQUEUE: &str = "threads.followups.enqueue";
pub const THREADS_FOLLOWUPS_CLAIM: &str = "threads.followups.claim";
pub const THREADS_FOLLOWUPS_SUBMIT: &str = "threads.followups.submit";
pub const THREADS_FOLLOWUPS_ERROR: &str = "threads.followups.error";
pub const THREADS_FOLLOWUPS_CANCEL: &str = "threads.followups.cancel";
pub const THREADS_PLAN_ACCEPT: &str = "threads.plan.accept";
pub const THREADS_PLAN_REVISE: &str = "threads.plan.revise";
pub const THREADS_ELICITATION_ANSWER: &str = "threads.elicitation.answer";
pub const THREADS_APPROVAL_ANSWER: &str = "threads.approval.answer";
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
pub const CLEANUP_DSH_ARCHIVE_DRY_RUN: &str = "cleanup.dshArchiveDryRun";
pub const CLEANUP_DSH_ARCHIVE_EXECUTE: &str = "cleanup.dshArchiveExecute";
pub const CLEANUP_UNIFIED_DRY_RUN: &str = "cleanup.unifiedDryRun";
pub const CLEANUP_UNIFIED_EXECUTE: &str = "cleanup.unifiedExecute";
pub const CLEANUP_ARCHIVE_DRY_RUN: &str = "cleanup.archiveDryRun";
pub const CLEANUP_ARCHIVE_EXECUTE: &str = "cleanup.archiveExecute";
pub const CLEANUP_HIDDEN_DRY_RUN: &str = "cleanup.hiddenDryRun";
pub const CLEANUP_HIDDEN_EXECUTE: &str = "cleanup.hiddenExecute";
pub const UPLOADS_DELETE: &str = "uploads.delete";

pub const TRANSPORT_UPLOAD_FILES: &str = "uploadFiles";
pub const TRANSPORT_THREAD_EVENTS: &str = "threadEvents";

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
    CLEANUP_DSH_ARCHIVE_DRY_RUN,
    CLEANUP_DSH_ARCHIVE_EXECUTE,
    CLEANUP_UNIFIED_DRY_RUN,
    CLEANUP_UNIFIED_EXECUTE,
    CLEANUP_ARCHIVE_DRY_RUN,
    CLEANUP_ARCHIVE_EXECUTE,
    CLEANUP_HIDDEN_DRY_RUN,
    CLEANUP_HIDDEN_EXECUTE,
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
            | CLEANUP_DSH_ARCHIVE_DRY_RUN
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

pub const ALLOWED_TRANSPORT_COMMANDS: &[&str] = &[];

pub const INTERNAL_COMMANDS: &[&str] = &[];

pub const DECLARED_COMMANDS: &[&str] = &[
    SESSIONS_ATTACHMENT_READ,
    SESSIONS_SEARCH,
    SESSIONS_BULK_PREVIEW,
    SESSIONS_BULK_EXECUTE,
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
    AUTH_PUBLIC_SETTINGS,
    AUTH_LOGIN,
    AUTH_LOGOUT,
    AUTH_ME,
    SECURITY_GET,
    SECURITY_SAVE,
    SECURITY_CHANGE_PASSWORD,
    SYSTEM_CAPABILITIES,
    SYSTEM_VERSION,
    SYSTEM_PLATFORM,
    SYSTEM_PROVIDERS,
    SYSTEM_PLUGINS,
    SYSTEM_MODELS,
    SYSTEM_PERMISSION_PROFILES,
    SYSTEM_CODEX_CONFIG,
    SYSTEM_CLAUDE_CODE_OVERVIEW,
    THREADS_LIST,
    THREADS_DETAIL,
    THREADS_SUBAGENT_DETAIL,
    THREADS_BLOCKS,
    THREADS_CREATE,
    THREADS_SEND,
    THREADS_STEER,
    THREADS_STOP,
    THREADS_ARCHIVE,
    THREADS_RESTORE,
    THREADS_RENAME,
    THREADS_FORK,
    THREADS_FOLLOWUPS_LIST,
    THREADS_FOLLOWUPS_ENQUEUE,
    THREADS_FOLLOWUPS_CLAIM,
    THREADS_FOLLOWUPS_SUBMIT,
    THREADS_FOLLOWUPS_ERROR,
    THREADS_FOLLOWUPS_CANCEL,
    THREADS_PLAN_ACCEPT,
    THREADS_PLAN_REVISE,
    THREADS_ELICITATION_ANSWER,
    THREADS_APPROVAL_ANSWER,
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
    CLEANUP_DSH_ARCHIVE_DRY_RUN,
    CLEANUP_DSH_ARCHIVE_EXECUTE,
    CLEANUP_UNIFIED_DRY_RUN,
    CLEANUP_UNIFIED_EXECUTE,
    CLEANUP_ARCHIVE_DRY_RUN,
    CLEANUP_ARCHIVE_EXECUTE,
    CLEANUP_HIDDEN_DRY_RUN,
    CLEANUP_HIDDEN_EXECUTE,
    UPLOADS_DELETE,
    TRANSPORT_UPLOAD_FILES,
    TRANSPORT_THREAD_EVENTS,
];

pub use super::retired_commands::{
    is_retired_command, is_retired_tauri_command, RetiredSurface, RETIRED_COMMANDS,
    RETIRED_COMMAND_REGISTRY,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Rpc,
    Transport,
    Internal,
    Retired,
    Unknown,
}

pub fn classify_command(command: &str) -> CommandKind {
    if ALLOWED_RPC_COMMANDS.contains(&command) {
        CommandKind::Rpc
    } else if ALLOWED_TRANSPORT_COMMANDS.contains(&command) {
        CommandKind::Transport
    } else if INTERNAL_COMMANDS.contains(&command) {
        CommandKind::Internal
    } else if RETIRED_COMMANDS.contains(&command) {
        CommandKind::Retired
    } else {
        CommandKind::Unknown
    }
}

pub fn is_allowed_rpc_command(command: &str) -> bool {
    ALLOWED_RPC_COMMANDS.contains(&command)
}

pub fn is_allowed_transport_command(command: &str) -> bool {
    ALLOWED_TRANSPORT_COMMANDS.contains(&command)
}

pub fn is_internal_command(command: &str) -> bool {
    INTERNAL_COMMANDS.contains(&command)
}

#[cfg(test)]
mod tests {
    use super::{
        classify_command, CommandKind, ALLOWED_RPC_COMMANDS, ALLOWED_TRANSPORT_COMMANDS,
        DECLARED_COMMANDS, INTERNAL_COMMANDS, RETIRED_COMMANDS,
    };
    use std::collections::HashSet;

    #[test]
    fn unified_rpc_commands_are_dot_named_and_unique() {
        let mut seen = HashSet::new();
        for command in ALLOWED_RPC_COMMANDS {
            assert!(
                command.contains('.'),
                "business RPC command must use dot naming: {command}"
            );
            assert!(
                seen.insert(*command),
                "business RPC command must be unique: {command}"
            );
        }
    }

    #[test]
    fn retired_authentication_and_event_transports_are_not_executable() {
        assert!(ALLOWED_TRANSPORT_COMMANDS.is_empty());
        for command in [
            "threadEvents",
            "auth.login",
            "auth.logout",
            "auth.me",
            "auth.publicSettings",
            "security.get",
            "security.save",
            "security.changePassword",
        ] {
            assert_eq!(classify_command(command), CommandKind::Retired, "{command}");
        }
    }

    #[test]
    fn command_allowlists_transport_allowlist_and_retired_denylist_are_disjoint() {
        for command in ALLOWED_RPC_COMMANDS {
            assert!(
                !ALLOWED_TRANSPORT_COMMANDS.contains(command),
                "business RPC command must not also be a transport command: {command}"
            );
            assert!(
                !INTERNAL_COMMANDS.contains(command),
                "business RPC command must not also be an internal command: {command}"
            );
            assert!(
                !RETIRED_COMMANDS.contains(command),
                "business RPC command must not also be retired: {command}"
            );
            assert_eq!(classify_command(command), CommandKind::Rpc);
        }

        for command in ALLOWED_TRANSPORT_COMMANDS {
            assert!(
                !ALLOWED_RPC_COMMANDS.contains(command),
                "transport command must not also be an RPC command: {command}"
            );
            assert!(
                !RETIRED_COMMANDS.contains(command),
                "transport command must not also be retired: {command}"
            );
            assert_eq!(classify_command(command), CommandKind::Transport);
        }

        for command in INTERNAL_COMMANDS {
            assert!(
                !ALLOWED_RPC_COMMANDS.contains(command),
                "internal command must not also be an allowed RPC command: {command}"
            );
            assert!(
                !ALLOWED_TRANSPORT_COMMANDS.contains(command),
                "internal command must not also be a transport command: {command}"
            );
            assert!(
                !RETIRED_COMMANDS.contains(command),
                "internal command must not also be retired: {command}"
            );
            assert_eq!(classify_command(command), CommandKind::Internal);
        }

        for command in RETIRED_COMMANDS {
            assert!(
                !ALLOWED_RPC_COMMANDS.contains(command),
                "retired command must not remain in allowed RPC set: {command}"
            );
            assert!(
                !ALLOWED_TRANSPORT_COMMANDS.contains(command),
                "retired command must not remain in transport set: {command}"
            );
            assert!(
                !INTERNAL_COMMANDS.contains(command),
                "retired command must not remain in internal set: {command}"
            );
            assert_eq!(classify_command(command), CommandKind::Retired);
        }
    }

    #[test]
    fn declared_command_constants_are_fully_classified() {
        let mut seen = HashSet::new();
        for command in DECLARED_COMMANDS {
            assert!(
                seen.insert(*command),
                "declared command constant must be unique: {command}"
            );
            assert_ne!(
                classify_command(command),
                CommandKind::Unknown,
                "declared command constant must be covered by an allowlist: {command}"
            );
        }

        assert_eq!(
            classify_command("not.a.realCommand"),
            CommandKind::Unknown,
            "unknown commands must stay denied by default"
        );
    }
}
