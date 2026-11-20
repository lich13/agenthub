#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemoteAccess {
    Read,
    Write,
}

macro_rules! remote_commands {
    ($( $name:ident = $wire:literal, $access:ident; )*) => {
        $(pub const $name: &str = $wire;)*
        pub const ALLOWED_RPC_COMMANDS: &[&str] = &[$($name),*];
        pub fn remote_access(command: &str) -> Option<RemoteAccess> {
            match command { $($name => Some(RemoteAccess::$access),)* _ => None }
        }
    }
}

remote_commands! {
    GITHUB_CI_STATUS = "github.ci.status", Read;
    SESSIONS_ATTACHMENT_READ = "sessions.attachmentRead", Read;
    SESSIONS_SEARCH = "sessions.search", Read;
    GROK_LIST = "grok.list", Read;
    SESSIONS_BULK_PREVIEW = "sessions.bulkPreview", Read;
    SESSIONS_BULK_EXECUTE = "sessions.bulkExecute", Write;
    GROK_DETAIL = "grok.detail", Read;
    GROK_RENAME = "grok.rename", Write;
    GROK_DELETE_PREVIEW = "grok.deletePreview", Read;
    GROK_DELETE_EXECUTE = "grok.deleteExecute", Write;
    CLAUDE_LIST = "claude.list", Read;
    CLAUDE_DETAIL = "claude.detail", Read;
    CLAUDE_RENAME = "claude.rename", Write;
    CLAUDE_DELETE_PREVIEW = "claude.deletePreview", Read;
    CLAUDE_DELETE_EXECUTE = "claude.deleteExecute", Write;
    ANTIGRAVITY_LIST = "antigravity.list", Read;
    ANTIGRAVITY_DETAIL = "antigravity.detail", Read;
    ANTIGRAVITY_RENAME = "antigravity.rename", Write;
    ANTIGRAVITY_ARCHIVE = "antigravity.archive", Write;
    ANTIGRAVITY_RESTORE = "antigravity.restore", Write;
    DSH_LIST = "dsh.list", Read;
    DSH_DETAIL = "dsh.detail", Read;
    DSH_RENAME = "dsh.rename", Write;
    DSH_ARCHIVE = "dsh.archive", Write;
    DSH_RESTORE = "dsh.restore", Write;
    SYSTEM_CAPABILITIES = "system.capabilities", Read;
    THREADS_LIST = "threads.list", Read;
    THREADS_SUBAGENT_DETAIL = "threads.subagentDetail", Read;
    THREADS_DETAIL = "threads.detail", Read;
    THREADS_BLOCKS = "threads.blocks", Read;
    THREADS_ARCHIVE = "threads.archive", Write;
    THREADS_RESTORE = "threads.restore", Write;
    THREADS_RENAME = "threads.rename", Write;
    THREADS_ACCOUNT_SCOPE_STATUS = "threads.accountScopeStatus", Read;
    THREADS_ACCOUNT_SCOPE_CLEAR = "threads.accountScopeClear", Write;
    JOBS_LIST = "jobs.list", Read;
    JOBS_DETAIL = "jobs.detail", Read;
    PROBE_STATUS = "probe.status", Read;
    PROBE_SETTINGS_GET = "probe.settings.get", Read;
    PROBE_SETTINGS_SAVE = "probe.settings.save", Write;
    PROBE_EVENTS = "probe.events", Read;
    PROBE_BARK_TEST = "probe.barkTest", Write;
    PROBE_INSTALL_HOOKS = "probe.installHooks", Write;
    PROBE_REMOVE_HOOKS = "probe.removeHooks", Write;
    UPDATES_STATUS = "updates.status", Read;
    UPDATES_CHECK = "updates.check", Write;
    UPDATES_INSTALL = "updates.install", Write;
    UPDATES_PRUNE = "updates.prune", Write;
    CLEANUP_UNIFIED_DRY_RUN = "cleanup.unifiedDryRun", Read;
    CLEANUP_UNIFIED_EXECUTE = "cleanup.unifiedExecute", Write;
    CLEANUP_HIDDEN_DRY_RUN = "cleanup.hiddenDryRun", Read;
}

pub const CLAUDE_HOOKS: &str = "claude.hooks";
pub const CLAUDE_OWNERSHIP_DIAGNOSTICS: &str = "claude.ownershipDiagnostics";
pub const GROKBOT_LIST: &str = "grokbot.list";
pub const GROKBOT_DETAIL: &str = "grokbot.detail";
pub const PLANS_SAVE: &str = "plans.save";
pub const REMOTE_GET: &str = "remote.get";
pub const REMOTE_SSH_HOSTS: &str = "remote.sshHosts";
pub const REMOTE_SSH_DELETE: &str = "remote.sshDelete";
pub const REMOTE_VERIFY: &str = "remote.verify";
pub const REMOTE_SAVE: &str = "remote.save";
pub const REMOTE_INSTALL: &str = "remote.install";
pub const REMOTE_REMOVE: &str = "remote.remove";
pub const REMOTE_SELECT: &str = "remote.select";
pub const REMOTE_INVOKE: &str = "remote.invoke";
pub const CODEX_DESKTOP_ATTACH: &str = "codex.desktop.attach";
pub const CODEX_DESKTOP_ACTION: &str = "codex.desktop.action";
pub const CODEX_DESKTOP_DETACH: &str = "codex.desktop.detach";
pub const CODEX_DESKTOP_OPEN: &str = "codex.desktop.open";
pub const CODEX_DESKTOP_CATALOG: &str = "codex.desktop.catalog";
pub const CODEX_DESKTOP_CHOOSE: &str = "codex.desktop.choose";
pub const CODEX_DESKTOP_CREATE: &str = "codex.desktop.create";
pub const CODEX_DESKTOP_ATTACHMENTS: &str = "codex.desktop.attachments";
pub const CODEX_DESKTOP_DIAGNOSTICS: &str = "codex.desktop.diagnostics";

pub fn desktop_command_is_read(command: &str, hook_status: bool) -> bool {
    match remote_access(command) {
        Some(access) => access == RemoteAccess::Read,
        None => match command {
            CLAUDE_HOOKS => hook_status,
            CLAUDE_OWNERSHIP_DIAGNOSTICS
            | GROKBOT_LIST
            | GROKBOT_DETAIL
            | CODEX_DESKTOP_ATTACH
            | CODEX_DESKTOP_CATALOG
            | CODEX_DESKTOP_DIAGNOSTICS => true,
            _ => false,
        },
    }
}

pub fn is_mutating_rpc_command(command: &str) -> bool {
    !matches!(remote_access(command), Some(RemoteAccess::Read))
}

pub fn is_allowed_rpc_command(command: &str) -> bool {
    remote_access(command).is_some()
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
        assert!(!is_mutating_rpc_command(GITHUB_CI_STATUS));
    }
}
