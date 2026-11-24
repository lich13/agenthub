import Foundation

// MARK: - Probe Status, Settings & Draft Validation

public struct ProbeLiveSession: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var sessionKey: String?
    public var provider: String?
    public var bucket: String?
    public var title: String?
    public var cwd: String?

    enum CodingKeys: String, CodingKey {
        case id
        case sessionKey = "session_key"
        case provider, bucket, title, cwd
    }

    public var uniqueKey: String {
        "\(provider ?? "codex"):\(sessionKey ?? id):\(bucket ?? "live")"
    }
}

public struct ProviderNotificationRuntimeStats: Codable, Equatable, Sendable {
    public var provider: String
    public var streams: Int
}

public struct ProbeStatus: Codable, Equatable, Sendable {
    public var available: Bool?
    public var error: String?
    public var hostLabel: String?
    public var hookStatus: String?
    public var errorMonitorStatus: String?
    public var errorMonitorIncidentCount: Int?
    public var codexHome: String?
    public var codexHooksPath: String?
    public var dshSessionsPath: String?
    public var actualCommands: [String]?
    public var staleCommandCount: Int?
    public var emptyGroupCount: Int?
    public var discoveryWarnings: [String]?
    public var runningCount: Int?
    public var replyNeededCount: Int?
    public var recoverableCount: Int?
    public var runningThreads: [ProbeLiveSession]?
    public var replyNeededThreads: [ProbeLiveSession]?
    public var recoverableThreads: [ProbeLiveSession]?
    public var activeSessions: [ProbeLiveSession]?
    public var providerNotifications: [ProviderNotificationRuntimeStats]?

    enum CodingKeys: String, CodingKey {
        case available
        case error
        case hostLabel = "host_label"
        case hookStatus = "hook_status"
        case errorMonitorStatus = "error_monitor_status"
        case errorMonitorIncidentCount = "error_monitor_incident_count"
        case codexHome = "codex_home"
        case codexHooksPath = "codex_hooks_path"
        case dshSessionsPath = "dsh_sessions_path"
        case actualCommands = "actual_commands"
        case staleCommandCount = "stale_command_count"
        case emptyGroupCount = "empty_group_count"
        case discoveryWarnings = "discovery_warnings"
        case runningCount = "running_count"
        case replyNeededCount = "reply_needed_count"
        case recoverableCount = "recoverable_count"
        case runningThreads = "running_threads"
        case replyNeededThreads = "reply_needed_threads"
        case recoverableThreads = "recoverable_threads"
        case activeSessions = "active_sessions"
        case providerNotifications = "provider_notifications"
    }
}

public struct ProbeSettingsView: Codable, Equatable, Sendable {
    public struct Codex: Codable, Equatable, Sendable {
        public var configuredCodexHome: String?
        public var workspace: String
        public var hostLabel: String

        enum CodingKeys: String, CodingKey {
            case configuredCodexHome = "configured_codex_home"
            case workspace
            case hostLabel = "host_label"
        }
    }

    public struct Probe: Codable, Equatable, Sendable {
        public var enabled: Bool
        public var pollSeconds: Int
        public var recentLimit: Int
        public var hooks: Hooks
        public var observability: Observability
        public var errorMonitor: ErrorMonitor

        enum CodingKeys: String, CodingKey {
            case enabled
            case pollSeconds = "poll_seconds"
            case recentLimit = "recent_limit"
            case hooks
            case observability
            case errorMonitor = "error_monitor"
        }
    }

    public struct Hooks: Codable, Equatable, Sendable {
        public var manageStopHook: Bool
        public var clearAccountScope: Bool

        enum CodingKeys: String, CodingKey {
            case manageStopHook = "manage_stop_hook"
            case clearAccountScope = "clear_account_scope"
        }
    }

    public struct Observability: Codable, Equatable, Sendable {
        public var eventRetentionDays: Int
        public var hookEventMaxLines: Int
        public var hookCooldownMaxLines: Int
        public var logMaxBytes: Int

        enum CodingKeys: String, CodingKey {
            case eventRetentionDays = "event_retention_days"
            case hookEventMaxLines = "hook_event_max_lines"
            case hookCooldownMaxLines = "hook_cooldown_max_lines"
            case logMaxBytes = "log_max_bytes"
        }
    }

    public struct ErrorMonitor: Codable, Equatable, Sendable {
        public var enabled: Bool
    }

    public struct Notifications: Codable, Equatable, Sendable {
        public var deviceKeyConfigured: Bool
        public var serverUrl: String
        public var enabled: Bool
        public var sound: String?
        public var group: String
        public var url: String?
        public var notifyCompletion: Bool
        public var notifyReplyNeeded: Bool
        public var notifyRecoverable: Bool
        public var notifyCodex: Bool
        public var notifyGrok: Bool
        public var notifyGrokbot: Bool
        public var notifyGithubCi: Bool
        public var githubCiRepositories: [String]
        public var githubCiFilterBots: Bool
        public var notifyGrokCompletion: Bool
        public var notifyGrokFailure: Bool
        public var notifyGrokReplyNeeded: Bool
        public var notifyClaudeReplyNeeded: Bool
        public var notifyClaudeFailure: Bool
        public var notifyClaudeCompletion: Bool
        public var notifyClaude: Bool
        public var notifyAntigravity: Bool
        public var notifyAntigravityCompletion: Bool
        public var notifyAntigravityFailure: Bool
        public var notifyAntigravityReplyNeeded: Bool
        public var notifyAntigravityAuthorization: Bool
        public var notifyDsh: Bool
        public var notifyDshCompletion: Bool
        public var notifyDshFailure: Bool
        public var notifyDshReplyNeeded: Bool
        public var notifyDshAuthorization: Bool

        enum CodingKeys: String, CodingKey {
            case deviceKeyConfigured = "device_key_configured"
            case serverUrl = "server_url"
            case enabled
            case sound
            case group
            case url
            case notifyCompletion = "notify_completion"
            case notifyReplyNeeded = "notify_reply_needed"
            case notifyRecoverable = "notify_recoverable"
            case notifyCodex = "notify_codex"
            case notifyGrok = "notify_grok"
            case notifyGrokbot = "notify_grokbot"
            case notifyGithubCi = "notify_github_ci"
            case githubCiRepositories = "github_ci_repositories"
            case githubCiFilterBots = "github_ci_filter_bots"
            case notifyGrokCompletion = "notify_grok_completion"
            case notifyGrokFailure = "notify_grok_failure"
            case notifyGrokReplyNeeded = "notify_grok_reply_needed"
            case notifyClaudeReplyNeeded = "notify_claude_reply_needed"
            case notifyClaudeFailure = "notify_claude_failure"
            case notifyClaudeCompletion = "notify_claude_completion"
            case notifyClaude = "notify_claude"
            case notifyAntigravity = "notify_antigravity"
            case notifyAntigravityCompletion = "notify_antigravity_completion"
            case notifyAntigravityFailure = "notify_antigravity_failure"
            case notifyAntigravityReplyNeeded = "notify_antigravity_reply_needed"
            case notifyAntigravityAuthorization = "notify_antigravity_authorization"
            case notifyDsh = "notify_dsh"
            case notifyDshCompletion = "notify_dsh_completion"
            case notifyDshFailure = "notify_dsh_failure"
            case notifyDshReplyNeeded = "notify_dsh_reply_needed"
            case notifyDshAuthorization = "notify_dsh_authorization"
        }
    }

    public var codex: Codex
    public var probe: Probe
    public var notifications: Notifications
}

public struct ProbeSettingsDraft: Equatable, Sendable {
    public var codexHome: String
    public var codexWorkspace: String
    public var hostLabel: String

    public var probeEnabled: Bool
    public var errorMonitorEnabled: Bool
    public var manageStopHook: Bool
    public var clearAccountScope: Bool

    // Raw string drafts preserve intermediate typing & IME composition.
    public var pollSecondsText: String
    public var recentLimitText: String
    public var eventRetentionDaysText: String
    public var hookEventMaxLinesText: String
    public var hookCooldownMaxLinesText: String
    public var logMaxMiBText: String

    public var barkEnabled: Bool
    public var barkServerUrl: String
    public var barkDeviceKey: String
    public var barkDeviceKeyConfigured: Bool
    public var barkSound: String
    public var barkGroup: String
    public var barkUrl: String

    public var notifyCompletion: Bool
    public var notifyReplyNeeded: Bool
    public var notifyRecoverable: Bool
    public var notifyCodex: Bool
    public var notifyGrok: Bool
    public var notifyGrokCompletion: Bool
    public var notifyGrokFailure: Bool
    public var notifyGrokReplyNeeded: Bool
    public var notifyGrokbot: Bool
    public var notifyClaude: Bool
    public var notifyClaudeCompletion: Bool
    public var notifyClaudeFailure: Bool
    public var notifyClaudeReplyNeeded: Bool
    public var notifyAntigravity: Bool
    public var notifyAntigravityCompletion: Bool
    public var notifyAntigravityFailure: Bool
    public var notifyAntigravityReplyNeeded: Bool
    public var notifyAntigravityAuthorization: Bool
    public var notifyDsh: Bool
    public var notifyDshCompletion: Bool
    public var notifyDshFailure: Bool
    public var notifyDshReplyNeeded: Bool
    public var notifyDshAuthorization: Bool

    public var notifyGithubCi: Bool
    public var githubCiRepositories: [String]
    public var githubCiFilterBots: Bool

    public static func from(_ view: ProbeSettingsView) -> ProbeSettingsDraft {
        let codex = view.codex
        let probe = view.probe
        let hooks = probe.hooks
        let obs = probe.observability
        let notif = view.notifications
        let logBytes = obs.logMaxBytes
        let logMiB = Double(logBytes) / (1024.0 * 1024.0)
        let formattedMiB = logMiB.truncatingRemainder(dividingBy: 1) == 0
            ? String(Int(logMiB))
            : String(format: "%.6g", logMiB)

        return ProbeSettingsDraft(
            codexHome: codex.configuredCodexHome ?? "",
            codexWorkspace: codex.workspace,
            hostLabel: codex.hostLabel,
            probeEnabled: probe.enabled,
            errorMonitorEnabled: probe.errorMonitor.enabled,
            manageStopHook: hooks.manageStopHook,
            clearAccountScope: hooks.clearAccountScope,
            pollSecondsText: String(probe.pollSeconds),
            recentLimitText: String(probe.recentLimit),
            eventRetentionDaysText: String(obs.eventRetentionDays),
            hookEventMaxLinesText: String(obs.hookEventMaxLines),
            hookCooldownMaxLinesText: String(obs.hookCooldownMaxLines),
            logMaxMiBText: formattedMiB,
            barkEnabled: notif.enabled,
            barkServerUrl: notif.serverUrl,
            barkDeviceKey: "",
            barkDeviceKeyConfigured: notif.deviceKeyConfigured,
            barkSound: notif.sound ?? "",
            barkGroup: notif.group,
            barkUrl: notif.url ?? "",
            notifyCompletion: notif.notifyCompletion,
            notifyReplyNeeded: notif.notifyReplyNeeded,
            notifyRecoverable: notif.notifyRecoverable,
            notifyCodex: notif.notifyCodex,
            notifyGrok: notif.notifyGrok,
            notifyGrokCompletion: notif.notifyGrokCompletion,
            notifyGrokFailure: notif.notifyGrokFailure,
            notifyGrokReplyNeeded: notif.notifyGrokReplyNeeded,
            notifyGrokbot: notif.notifyGrokbot,
            notifyClaude: notif.notifyClaude,
            notifyClaudeCompletion: notif.notifyClaudeCompletion,
            notifyClaudeFailure: notif.notifyClaudeFailure,
            notifyClaudeReplyNeeded: notif.notifyClaudeReplyNeeded,
            notifyAntigravity: notif.notifyAntigravity,
            notifyAntigravityCompletion: notif.notifyAntigravityCompletion,
            notifyAntigravityFailure: notif.notifyAntigravityFailure,
            notifyAntigravityReplyNeeded: notif.notifyAntigravityReplyNeeded,
            notifyAntigravityAuthorization: notif.notifyAntigravityAuthorization,
            notifyDsh: notif.notifyDsh,
            notifyDshCompletion: notif.notifyDshCompletion,
            notifyDshFailure: notif.notifyDshFailure,
            notifyDshReplyNeeded: notif.notifyDshReplyNeeded,
            notifyDshAuthorization: notif.notifyDshAuthorization,
            notifyGithubCi: notif.notifyGithubCi,
            githubCiRepositories: notif.githubCiRepositories,
            githubCiFilterBots: notif.githubCiFilterBots
        )
    }

    public func isPlatformEnabled(_ platform: ProbeNotificationPlatform) -> Bool {
        switch platform {
        case .codex:
            notifyCodex && (notifyCompletion || notifyReplyNeeded || notifyRecoverable)
        case .claudeCode:
            notifyClaude && (notifyClaudeCompletion || notifyClaudeFailure || notifyClaudeReplyNeeded)
        case .grok:
            notifyGrok && (notifyGrokCompletion || notifyGrokFailure || notifyGrokReplyNeeded)
        case .grokbot:
            notifyGrokbot
        case .antigravity:
            notifyAntigravity && (notifyAntigravityCompletion || notifyAntigravityFailure || notifyAntigravityReplyNeeded || notifyAntigravityAuthorization)
        case .dsh:
            notifyDsh && (notifyDshCompletion || notifyDshFailure || notifyDshReplyNeeded || notifyDshAuthorization)
        }
    }

    public mutating func setPlatformEnabled(_ platform: ProbeNotificationPlatform, enabled: Bool) {
        switch platform {
        case .codex:
            notifyCodex = enabled
            if enabled && !(notifyCompletion || notifyReplyNeeded || notifyRecoverable) {
                notifyCompletion = true
                notifyReplyNeeded = true
                notifyRecoverable = true
            }
        case .claudeCode:
            notifyClaude = enabled
            if enabled && !(notifyClaudeCompletion || notifyClaudeFailure || notifyClaudeReplyNeeded) {
                notifyClaudeCompletion = true
                notifyClaudeFailure = true
                notifyClaudeReplyNeeded = true
            }
        case .grok:
            notifyGrok = enabled
            if enabled && !(notifyGrokCompletion || notifyGrokFailure || notifyGrokReplyNeeded) {
                notifyGrokCompletion = true
                notifyGrokFailure = true
                notifyGrokReplyNeeded = true
            }
        case .grokbot:
            notifyGrokbot = enabled
        case .antigravity:
            notifyAntigravity = enabled
            if enabled && !(notifyAntigravityCompletion || notifyAntigravityFailure || notifyAntigravityReplyNeeded || notifyAntigravityAuthorization) {
                notifyAntigravityCompletion = true
                notifyAntigravityFailure = true
                notifyAntigravityReplyNeeded = true
                notifyAntigravityAuthorization = true
            }
        case .dsh:
            notifyDsh = enabled
            if enabled && !(notifyDshCompletion || notifyDshFailure || notifyDshReplyNeeded || notifyDshAuthorization) {
                notifyDshCompletion = true
                notifyDshFailure = true
                notifyDshReplyNeeded = true
                notifyDshAuthorization = true
            }
        }
    }

    public func validationErrors() -> [String] {
        var errors: [String] = []
        func checkInt(_ raw: String, label: String, min: Int, max: Int) {
            guard let v = Int(raw.trimmingCharacters(in: .whitespaces)), v >= min, v <= max else {
                errors.append("\(label)须为 \(min) 到 \(max) 之间的整数")
                return
            }
        }
        checkInt(pollSecondsText, label: "轮询秒数", min: 5, max: 3600)
        checkInt(recentLimitText, label: "最近事件数", min: 1, max: 500)
        checkInt(eventRetentionDaysText, label: "通知事件保留天数", min: 1, max: 3650)
        checkInt(hookEventMaxLinesText, label: "Hook 事件行数", min: 10, max: 10000)
        checkInt(hookCooldownMaxLinesText, label: "冷却行数", min: 10, max: 10000)

        if let mib = Double(logMaxMiBText.trimmingCharacters(in: .whitespaces)), mib.isFinite {
            let bytes = Int((mib * 1024.0 * 1024.0).rounded())
            if bytes < 4096 || bytes > 8 * 1024 * 1024 {
                errors.append("日志上限 MiB 须在 0.00390625 到 8 之间")
            }
        } else {
            errors.append("日志上限 MiB 格式无效")
        }

        if hostLabel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            errors.append("主机标签不能为空")
        }
        return errors
    }

    public func buildSavePayload(submittedDeviceKey: String? = nil) throws -> [String: JSONValue] {
        let errs = validationErrors()
        if let first = errs.first {
            throw NSError(domain: "AgentHubProbe", code: 1, userInfo: [NSLocalizedDescriptionKey: first])
        }
        let pollSeconds = Int(pollSecondsText.trimmingCharacters(in: .whitespaces)) ?? 15
        let recentLimit = Int(recentLimitText.trimmingCharacters(in: .whitespaces)) ?? 50
        let retentionDays = Int(eventRetentionDaysText.trimmingCharacters(in: .whitespaces)) ?? 2
        let hookMaxLines = Int(hookEventMaxLinesText.trimmingCharacters(in: .whitespaces)) ?? 500
        let cooldownLines = Int(hookCooldownMaxLinesText.trimmingCharacters(in: .whitespaces)) ?? 1000
        let mib = Double(logMaxMiBText.trimmingCharacters(in: .whitespaces)) ?? 5.0
        let logBytes = Int((mib * 1024.0 * 1024.0).rounded())

        let key = (submittedDeviceKey ?? barkDeviceKey).trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedHome = codexHome.trimmingCharacters(in: .whitespacesAndNewlines)
        let homePatch: JSONValue = (trimmedHome.isEmpty || trimmedHome.lowercased() == "auto") ? .null : .string(trimmedHome)
        let trimmedWs = codexWorkspace.trimmingCharacters(in: .whitespacesAndNewlines)

        var notifObj: [String: JSONValue] = [
            "enabled": .bool(barkEnabled || !key.isEmpty),
            "server_url": .string(barkServerUrl.trimmingCharacters(in: .whitespacesAndNewlines)),
            "sound": barkSound.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .null : .string(barkSound.trimmingCharacters(in: .whitespacesAndNewlines)),
            "group": .string(barkGroup.trimmingCharacters(in: .whitespacesAndNewlines)),
            "url": barkUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .null : .string(barkUrl.trimmingCharacters(in: .whitespacesAndNewlines)),
            "notify_completion": .bool(notifyCompletion),
            "notify_reply_needed": .bool(notifyReplyNeeded),
            "notify_recoverable": .bool(notifyRecoverable),
            "notify_codex": .bool(notifyCodex),
            "notify_grok": .bool(notifyGrok),
            "notify_grokbot": .bool(notifyGrokbot),
            "notify_github_ci": .bool(notifyGithubCi),
            "github_ci_repositories": .array(githubCiRepositories.map { .string($0.trimmingCharacters(in: .whitespacesAndNewlines)) }),
            "github_ci_filter_bots": .bool(githubCiFilterBots),
            "notify_grok_completion": .bool(notifyGrokCompletion),
            "notify_grok_failure": .bool(notifyGrokFailure),
            "notify_grok_reply_needed": .bool(notifyGrokReplyNeeded),
            "notify_claude": .bool(notifyClaude),
            "notify_claude_completion": .bool(notifyClaudeCompletion),
            "notify_claude_failure": .bool(notifyClaudeFailure),
            "notify_claude_reply_needed": .bool(notifyClaudeReplyNeeded),
            "notify_antigravity": .bool(notifyAntigravity),
            "notify_antigravity_completion": .bool(notifyAntigravityCompletion),
            "notify_antigravity_failure": .bool(notifyAntigravityFailure),
            "notify_antigravity_reply_needed": .bool(notifyAntigravityReplyNeeded),
            "notify_antigravity_authorization": .bool(notifyAntigravityAuthorization),
            "notify_dsh": .bool(notifyDsh),
            "notify_dsh_completion": .bool(notifyDshCompletion),
            "notify_dsh_failure": .bool(notifyDshFailure),
            "notify_dsh_reply_needed": .bool(notifyDshReplyNeeded),
            "notify_dsh_authorization": .bool(notifyDshAuthorization),
        ]
        if !key.isEmpty {
            notifObj["device_key"] = .string(key)
        }

        var payload: [String: JSONValue] = [
            "codex": .object([
                "home": homePatch,
                "workspace": trimmedWs.isEmpty ? .null : .string(trimmedWs),
                "host_label": .string(hostLabel.trimmingCharacters(in: .whitespacesAndNewlines)),
            ]),
            "probe": .object([
                "enabled": .bool(probeEnabled),
                "poll_seconds": .number(Double(pollSeconds)),
                "recent_limit": .number(Double(recentLimit)),
                "error_monitor": .object(["enabled": .bool(errorMonitorEnabled)]),
                "hooks": .object([
                    "manage_stop_hook": .bool(manageStopHook),
                    "clear_account_scope": .bool(clearAccountScope),
                ]),
                "notifications": .object(notifObj),
                "observability": .object([
                    "event_retention_days": .number(Double(retentionDays)),
                    "hook_event_max_lines": .number(Double(hookMaxLines)),
                    "hook_cooldown_max_lines": .number(Double(cooldownLines)),
                    "log_max_bytes": .number(Double(logBytes)),
                ]),
            ]),
        ]
        if !key.isEmpty {
            payload["notifications"] = .object(["device_key": .string(key)])
        }
        return payload
    }
}

// MARK: - Probe Events & Claude Hooks

public struct ProbeEventItem: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var kind: String?
    public var provider: String?
    public var title: String?
    public var message: String?
    public var summary: String?
    public var reason: String?
    public var source: String?
    public var threadId: String?
    public var barkStatus: String?
    public var dedupeStatus: String?
    public var dedupeKey: String?
    public var createdAt: Int64?
    public var payload: [String: JSONValue]?

    enum CodingKeys: String, CodingKey {
        case id, kind, provider, title, message, summary, reason, source, payload
        case threadId = "thread_id"
        case barkStatus = "bark_status"
        case dedupeStatus = "dedupe_status"
        case dedupeKey = "dedupe_key"
        case createdAt = "created_at"
    }

    public init(
        id: String,
        kind: String? = nil,
        provider: String? = nil,
        title: String? = nil,
        message: String? = nil,
        summary: String? = nil,
        reason: String? = nil,
        source: String? = nil,
        threadId: String? = nil,
        barkStatus: String? = nil,
        dedupeStatus: String? = nil,
        dedupeKey: String? = nil,
        createdAt: Int64? = nil,
        payload: [String: JSONValue]? = nil
    ) {
        self.id = id
        self.kind = kind
        self.provider = provider
        self.title = title
        self.message = message
        self.summary = summary
        self.reason = reason
        self.source = source
        self.threadId = threadId
        self.barkStatus = barkStatus
        self.dedupeStatus = dedupeStatus
        self.dedupeKey = dedupeKey
        self.createdAt = createdAt
        self.payload = payload
    }
}

public struct ProbeEventsResponse: Codable, Equatable, Sendable {
    public var events: [ProbeEventItem]
    public var limit: Int?
}

public struct ClaudeHooksStatus: Codable, Equatable, Sendable {
    public var installed: Bool
    public var managedCount: Int
    public var configPath: String?
}

// MARK: - GitHub CI Snapshot & Pure Domain Functions

public struct GithubCiAccount: Codable, Equatable, Sendable {
    public var id: UInt64
    public var login: String
}

public struct GithubCiRepository: Codable, Equatable, Identifiable, Sendable {
    public var id: UInt64
    public var fullName: String
    public var archived: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case fullName = "full_name"
        case archived
    }

    public init(id: UInt64, fullName: String, archived: Bool = false) {
        self.id = id
        self.fullName = fullName
        self.archived = archived
    }
}

public struct GithubCiRun: Codable, Equatable, Identifiable, Sendable {
    public var id: UInt64
    public var repositoryId: UInt64
    public var repositoryName: String
    public var name: String?
    public var workflowName: String
    public var displayTitle: String?
    public var headBranch: String?
    public var headSha: String
    public var status: String
    public var conclusion: String?
    public var htmlUrl: String
    public var updatedAt: String
    public var runAttempt: UInt64?
    public var isBot: Bool

    enum CodingKeys: String, CodingKey {
        case id, name
        case repositoryId = "repository_id"
        case repositoryName = "repository_name"
        case workflowName = "workflow_name"
        case displayTitle = "display_title"
        case headBranch = "head_branch"
        case headSha = "head_sha"
        case status, conclusion
        case htmlUrl = "html_url"
        case updatedAt = "updated_at"
        case runAttempt = "run_attempt"
        case isBot = "is_bot"
    }

    public init(
        id: UInt64,
        repositoryId: UInt64,
        repositoryName: String,
        name: String? = nil,
        workflowName: String,
        displayTitle: String? = nil,
        headBranch: String? = nil,
        headSha: String = "",
        status: String,
        conclusion: String? = nil,
        htmlUrl: String,
        updatedAt: String,
        runAttempt: UInt64? = 1,
        isBot: Bool = false
    ) {
        self.id = id
        self.repositoryId = repositoryId
        self.repositoryName = repositoryName
        self.name = name
        self.workflowName = workflowName
        self.displayTitle = displayTitle
        self.headBranch = headBranch
        self.headSha = headSha
        self.status = status
        self.conclusion = conclusion
        self.htmlUrl = htmlUrl
        self.updatedAt = updatedAt
        self.runAttempt = runAttempt
        self.isBot = isBot
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UInt64.self, forKey: .id)
        repositoryId = try c.decode(UInt64.self, forKey: .repositoryId)
        repositoryName = try c.decode(String.self, forKey: .repositoryName)
        name = try c.decodeIfPresent(String.self, forKey: .name)
        workflowName = try c.decodeIfPresent(String.self, forKey: .workflowName) ?? name ?? "工作流"
        displayTitle = try c.decodeIfPresent(String.self, forKey: .displayTitle)
        headBranch = try c.decodeIfPresent(String.self, forKey: .headBranch)
        headSha = try c.decode(String.self, forKey: .headSha)
        status = try c.decode(String.self, forKey: .status)
        conclusion = try c.decodeIfPresent(String.self, forKey: .conclusion)
        htmlUrl = try c.decode(String.self, forKey: .htmlUrl)
        updatedAt = try c.decode(String.self, forKey: .updatedAt)
        runAttempt = try c.decodeIfPresent(UInt64.self, forKey: .runAttempt)
        isBot = try c.decode(Bool.self, forKey: .isBot)
    }
}

public struct GithubCiSnapshot: Codable, Equatable, Sendable {
    public var enabled: Bool
    public var refreshing: Bool
    public var filterBots: Bool
    public var monitoredRepositories: [String]
    public var account: GithubCiAccount?
    public var repositories: [GithubCiRepository]
    public var runs: [GithubCiRun]
    public var error: String?

    enum CodingKeys: String, CodingKey {
        case enabled, refreshing
        case filterBots = "filter_bots"
        case monitoredRepositories = "monitored_repositories"
        case selectedRepositories = "selected_repositories"
        case account, repositories, runs, error
    }

    public init(
        enabled: Bool = false,
        refreshing: Bool = false,
        filterBots: Bool = true,
        monitoredRepositories: [String] = [],
        account: GithubCiAccount? = nil,
        repositories: [GithubCiRepository] = [],
        runs: [GithubCiRun] = [],
        error: String? = nil
    ) {
        self.enabled = enabled
        self.refreshing = refreshing
        self.filterBots = filterBots
        self.monitoredRepositories = monitoredRepositories
        self.account = account
        self.repositories = repositories
        self.runs = runs
        self.error = error
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? false
        refreshing = try c.decodeIfPresent(Bool.self, forKey: .refreshing) ?? false
        filterBots = try c.decodeIfPresent(Bool.self, forKey: .filterBots) ?? true
        monitoredRepositories = try c.decodeIfPresent([String].self, forKey: .monitoredRepositories)
            ?? (try c.decodeIfPresent([String].self, forKey: .selectedRepositories))
            ?? []
        account = try c.decodeIfPresent(GithubCiAccount.self, forKey: .account)
        repositories = try c.decodeIfPresent([GithubCiRepository].self, forKey: .repositories) ?? []
        runs = try c.decodeIfPresent([GithubCiRun].self, forKey: .runs) ?? []
        error = try c.decodeIfPresent(String.self, forKey: .error)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(enabled, forKey: .enabled)
        try c.encode(refreshing, forKey: .refreshing)
        try c.encode(filterBots, forKey: .filterBots)
        try c.encode(monitoredRepositories, forKey: .monitoredRepositories)
        try c.encodeIfPresent(account, forKey: .account)
        try c.encode(repositories, forKey: .repositories)
        try c.encode(runs, forKey: .runs)
        try c.encodeIfPresent(error, forKey: .error)
    }
}
