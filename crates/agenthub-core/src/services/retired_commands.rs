//! Protocol-2 tombstones. Keep transport ownership and existing error semantics.
//! Remove only after an explicitly approved compatibility-boundary change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetiredSurface {
    Shared,
    Tauri,
}
#[derive(Debug, Clone, Copy)]
pub struct RetiredCommand {
    pub name: &'static str,
    pub surface: RetiredSurface,
}
macro_rules! retired_commands {
    (shared: [$($shared:literal),* $(,)?], tauri: [$($tauri:literal),* $(,)?] $(,)?) => {
        pub const RETIRED_COMMANDS: &[&str] = &[$($shared),*];
        pub const RETIRED_COMMAND_REGISTRY: &[RetiredCommand] = &[
            $(RetiredCommand { name: $shared, surface: RetiredSurface::Shared },)*
            $(RetiredCommand { name: $tauri, surface: RetiredSurface::Tauri },)*
        ];
    };
}
retired_commands! {
    shared: [
    "probe.gotifyTest",
    "pi.list",
    "pi.detail",
    "pi.rename",
    "pi.deletePreview",
    "pi.deleteExecute",
    "auth.login",
    "auth.logout",
    "auth.me",
    "auth.publicSettings",
    "security.changePassword",
    "security.get",
    "security.save",
    "threadEvents",
    "system.status",
    "system.plugins",
    "system.models",
    "system.permissionProfiles",
    "system.codexConfig",
    "system.claudeCodeOverview",
    "threads.create",
    "threads.send",
    "threads.steer",
    "threads.stop",
    "threads.fork",
    "threads.followups.list",
    "threads.followups.enqueue",
    "threads.followups.claim",
    "threads.followups.submit",
    "threads.followups.error",
    "threads.followups.cancel",
    "threads.plan.accept",
    "threads.plan.revise",
    "threads.elicitation.answer",
    "threads.approval.answer",
    "threads.goal.get",
    "threads.goal.save",
    "threads.goal.clear",
    "threads.goal.pause",
    "threads.goal.resume",
    "uploads.delete",
    "uploadFiles",
    "probe.logsDb.status",
    "probe.logsDbDryRun",
    "probe.logsDbExecute",
    "desktopWebUi.settings.get",
    "desktopWebUi.settings.save",
    "desktopWebUi.status",
    "desktopWebUi.start",
    "desktopWebUi.stop",
    "desktopWebUi.password.reset",
    "getPublicSettings",
    "login",
    "logout",
    "me",
    "getSecurity",
    "saveSecurity",
    "changePassword",
    "listProviders",
    "getClaudeCodeOverview",
    "getPlatformOverview",
    "listPlugins",
    "getProbeStatus",
    "getProbeSettings",
    "saveProbeSettings",
    "getProbeLogsDbStatus",
    "getProbeEvents",
    "startProbeBarkTest",
    "startProbeHooksInstall",
    "startProbeLogsDbDryRun",
    "startProbeLogsDbExecute",
    "startProbeJob",
    "dryRunArchiveDelete",
    "startArchiveDelete",
    "dryRunHiddenThreadDelete",
    "startHiddenThreadDelete",
    "getUpdateStatus",
    "checkUpdate",
    "installUpdateAndRestart",
    "runUpdateAction",
    "backupPrune",
    "listThreads",
    "getThread",
    "getThreadBlocks",
    "createThread",
    "sendMessage",
    "steerThread",
    "listFollowUps",
    "enqueueFollowUp",
    "cancelFollowUp",
    "stopThread",
    "archiveThread",
    "restoreThread",
    "renameThread",
    "forkThread",
    "acceptPlan",
    "revisePlan",
    "answerElicitation",
    "answerApproval",
    "deleteUpload",
    "getSystemCapabilities",
    "getSystemVersion",
    "listModels",
    "listPermissionProfiles",
    "getCodexConfig",
    "getCodexGoal",
    "saveCodexGoal",
    "clearCodexGoal",
    "pauseCodexGoal",
    "resumeCodexGoal",
    "listJobs",
    "getJob",
    "getDesktopOverview",
    "getDesktopHome",
    "getDesktopPlatformStatus",
    "getDesktopClaudeCodeOverview",
    ],
    tauri: ["listPiSessions", "getPiSession", "renamePiSession", "previewPiSessionDelete", "deletePiSession"],
}
pub fn is_retired_command(command: &str) -> bool {
    RETIRED_COMMANDS.contains(&command)
}
pub fn is_retired_tauri_command(command: &str) -> bool {
    RETIRED_COMMAND_REGISTRY
        .iter()
        .any(|entry| entry.name == command)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_tombstone_keeps_its_transport_scope_and_cannot_execute() {
        let mut names = std::collections::HashSet::new();
        for entry in RETIRED_COMMAND_REGISTRY {
            assert!(
                names.insert(entry.name),
                "duplicate tombstone {}",
                entry.name
            );
            assert!(is_retired_tauri_command(entry.name));
            assert_eq!(
                is_retired_command(entry.name),
                entry.surface == RetiredSurface::Shared
            );
            assert!(!super::super::commands::is_allowed_rpc_command(entry.name));
            assert!(!super::super::commands::is_allowed_transport_command(
                entry.name
            ));
        }
        assert!(!is_retired_tauri_command("fixture.unknown"));
    }
}
