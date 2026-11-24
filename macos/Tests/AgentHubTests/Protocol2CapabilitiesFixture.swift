import Foundation

// v2.1.28 and v2.1.29 return the same capability fields on both host surfaces.
func protocol2CapabilitiesFixture(remote: Bool, pruneBackups: Bool? = nil) -> Data {
    let capabilities: [String: Bool] = [
        "threads": true,
        "thread_subagents": true,
        "claude_subagents": true,
        "antigravity_subagents": true,
        "jobs": true,
        "probe": true,
        "settings": true,
        "job_history": true,
        "app_updater": true,
        "thread_cleanup": true,
        "hidden_cleanup_scan": true,
        "codex_account_scope": true,
        "github_ci": true,
        "antigravity_sessions": true,
        "dsh_sessions": true,
        "thread_archive_actions": true,
        "codex_desktop": !remote,
        "systemd": remote,
        "linux_update_job": remote,
        "prune_backups": pruneBackups ?? remote,
        "antigravity_notifications": !remote,
        "dsh_notifications": !remote,
        "grokbot_sessions": !remote,
        "grokbot_notifications": !remote,
    ]
    let payload: [String: Any] = [
        "ok": true,
        "api_version": 2,
        "host_surface": remote ? "linux_server_api" : "desktop_embedded_tauri",
        "capabilities": capabilities,
    ]
    return try! JSONSerialization.data(withJSONObject: payload)
}
