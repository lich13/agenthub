import Foundation
@testable import AgentHubCore

enum ProbeSettingsFixture {
    static let data = Data("""
        {
          "codex": {
            "configured_codex_home": null,
            "workspace": "/fixture/workspace",
            "host_label": "fixture"
          },
          "probe": {
            "enabled": true,
            "poll_seconds": 15,
            "recent_limit": 50,
            "hooks": {
              "manage_stop_hook": true,
              "clear_account_scope": false
            },
            "error_monitor": {
              "enabled": true
            },
            "observability": {
              "event_retention_days": 2,
              "hook_event_max_lines": 500,
              "hook_cooldown_max_lines": 1000,
              "log_max_bytes": 5242880
            }
          },
          "notifications": {
            "device_key_configured": false,
            "server_url": "https://example.invalid",
            "enabled": false,
            "sound": null,
            "group": "AgentHub",
            "url": null,
            "notify_completion": true,
            "notify_reply_needed": true,
            "notify_recoverable": true,
            "notify_codex": true,
            "notify_grok": true,
            "notify_grokbot": false,
            "notify_github_ci": false,
            "github_ci_repositories": [],
            "github_ci_filter_bots": true,
            "notify_grok_completion": true,
            "notify_grok_failure": true,
            "notify_grok_reply_needed": true,
            "notify_claude_reply_needed": true,
            "notify_claude_failure": true,
            "notify_claude_completion": true,
            "notify_claude": true,
            "notify_antigravity": false,
            "notify_antigravity_completion": false,
            "notify_antigravity_failure": false,
            "notify_antigravity_reply_needed": false,
            "notify_antigravity_authorization": false,
            "notify_dsh": false,
            "notify_dsh_completion": false,
            "notify_dsh_failure": false,
            "notify_dsh_reply_needed": false,
            "notify_dsh_authorization": false
          }
        }
        """.utf8)

    static func settings() throws -> ProbeSettingsView {
        try JSONDecoder().decode(ProbeSettingsView.self, from: data)
    }
}
