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
    public var errorMonitorIncidentCount: Int?
    public var codexHome: String?
    public var codexHooksPath: String?
    public var dshSessionsPath: String?
    public var actualCommands: [String]?
    public var staleCommandCount: Int?
    public var discoveryWarnings: [String]?
    public var activeSessions: [ProbeLiveSession]?
    public var providerNotifications: [ProviderNotificationRuntimeStats]?

    enum CodingKeys: String, CodingKey {
        case available
        case error
        case hostLabel = "host_label"
        case hookStatus = "hook_status"
        case errorMonitorIncidentCount = "error_monitor_incident_count"
        case codexHome = "codex_home"
        case codexHooksPath = "codex_hooks_path"
        case dshSessionsPath = "dsh_sessions_path"
        case actualCommands = "actual_commands"
        case staleCommandCount = "stale_command_count"
        case discoveryWarnings = "discovery_warnings"
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

public enum ProbeSettingsSaveScope: String, CaseIterable, Sendable {
    case ci
    case settings
    case bark
}

public enum ProbeEventCategory: String, CaseIterable, Hashable, Sendable {
    case completion
    case replyNeeded
    case authorization
    case failure

    public var label: String {
        switch self {
        case .completion: "完成"
        case .replyNeeded: "需回复"
        case .authorization: "需授权"
        case .failure: "失败"
        }
    }
}

public struct ProbeSettingsDraft: Equatable, Sendable {
    public var codexHome: String
    public var hostLabel: String

    public var probeEnabled: Bool
    public var errorMonitorEnabled: Bool

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
        let obs = probe.observability
        let notif = view.notifications
        let logBytes = obs.logMaxBytes
        let logMiB: Double = Double(logBytes) / (1024.0 * 1024.0)
        let isWholeMiB: Bool = logMiB.truncatingRemainder(dividingBy: 1) == 0
        let formattedMiB: String = isWholeMiB
            ? String(Int(logMiB))
            : String(format: "%.6g", logMiB)

        return ProbeSettingsDraft(
            codexHome: codex.configuredCodexHome ?? "",
            hostLabel: codex.hostLabel,
            probeEnabled: probe.enabled,
            errorMonitorEnabled: probe.errorMonitor.enabled,
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

    public static func supportedCategories(for platform: SessionProvider) -> [ProbeEventCategory] {
        switch platform {
        case .codex, .claudeCode, .grok:
            [.completion, .replyNeeded, .failure]
        case .antigravity, .dsh:
            [.completion, .replyNeeded, .authorization, .failure]
        case .grokbot:
            []
        }
    }

    /// Returns the raw per-platform master switch so turning off the last sub-category pill
    /// never hides the inline category bar or prevents turning a single category back on.
    public func isPlatformEnabled(_ platform: SessionProvider) -> Bool {
        switch platform {
        case .codex: notifyCodex
        case .claudeCode: notifyClaude
        case .grok: notifyGrok
        case .grokbot: notifyGrokbot
        case .antigravity: notifyAntigravity
        case .dsh: notifyDsh
        }
    }

    public mutating func setPlatformEnabled(_ platform: SessionProvider, enabled: Bool) {
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

    public func isCategoryEnabled(_ platform: SessionProvider, _ category: ProbeEventCategory) -> Bool {
        switch (platform, category) {
        case (.codex, .completion): notifyCompletion
        case (.codex, .replyNeeded): notifyReplyNeeded
        case (.codex, .failure): notifyRecoverable
        case (.claudeCode, .completion): notifyClaudeCompletion
        case (.claudeCode, .replyNeeded): notifyClaudeReplyNeeded
        case (.claudeCode, .failure): notifyClaudeFailure
        case (.grok, .completion): notifyGrokCompletion
        case (.grok, .replyNeeded): notifyGrokReplyNeeded
        case (.grok, .failure): notifyGrokFailure
        case (.antigravity, .completion): notifyAntigravityCompletion
        case (.antigravity, .replyNeeded): notifyAntigravityReplyNeeded
        case (.antigravity, .authorization): notifyAntigravityAuthorization
        case (.antigravity, .failure): notifyAntigravityFailure
        case (.dsh, .completion): notifyDshCompletion
        case (.dsh, .replyNeeded): notifyDshReplyNeeded
        case (.dsh, .authorization): notifyDshAuthorization
        case (.dsh, .failure): notifyDshFailure
        default: false
        }
    }

    public mutating func setCategoryEnabled(
        _ platform: SessionProvider,
        _ category: ProbeEventCategory,
        enabled: Bool
    ) {
        switch (platform, category) {
        case (.codex, .completion): notifyCompletion = enabled
        case (.codex, .replyNeeded): notifyReplyNeeded = enabled
        case (.codex, .failure): notifyRecoverable = enabled
        case (.claudeCode, .completion): notifyClaudeCompletion = enabled
        case (.claudeCode, .replyNeeded): notifyClaudeReplyNeeded = enabled
        case (.claudeCode, .failure): notifyClaudeFailure = enabled
        case (.grok, .completion): notifyGrokCompletion = enabled
        case (.grok, .replyNeeded): notifyGrokReplyNeeded = enabled
        case (.grok, .failure): notifyGrokFailure = enabled
        case (.antigravity, .completion): notifyAntigravityCompletion = enabled
        case (.antigravity, .replyNeeded): notifyAntigravityReplyNeeded = enabled
        case (.antigravity, .authorization): notifyAntigravityAuthorization = enabled
        case (.antigravity, .failure): notifyAntigravityFailure = enabled
        case (.dsh, .completion): notifyDshCompletion = enabled
        case (.dsh, .replyNeeded): notifyDshReplyNeeded = enabled
        case (.dsh, .authorization): notifyDshAuthorization = enabled
        case (.dsh, .failure): notifyDshFailure = enabled
        default: break
        }
    }

    public func validationErrors(for scope: ProbeSettingsSaveScope = .settings) -> [String] {
        guard scope == .settings else { return [] }
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

        if Self.validatedLogMaxBytes(from: logMaxMiBText) == nil {
            let trimmed = logMaxMiBText.trimmingCharacters(in: .whitespaces)
            if let mib = Double(trimmed), mib.isFinite {
                errors.append("日志上限 MiB 须在 0.00390625 到 8 之间")
            } else {
                errors.append("日志上限 MiB 格式无效")
            }
        }

        if hostLabel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            errors.append("主机标签不能为空")
        }
        return errors
    }

    private static func validatedLogMaxBytes(from rawMiBText: String) -> Int? {
        guard let mib = Double(rawMiBText.trimmingCharacters(in: .whitespaces)),
              mib.isFinite,
              mib >= 0
        else {
            return nil
        }
        let minBytes = 4096.0
        let maxBytes = Double(8 * 1024 * 1024)
        let roundedBytes = (mib * 1024.0 * 1024.0).rounded()
        guard roundedBytes >= minBytes, roundedBytes <= maxBytes else {
            return nil
        }
        return Int(roundedBytes)
    }

    public func buildSavePayload(
        for scope: ProbeSettingsSaveScope = .settings,
        submittedDeviceKey: String? = nil
    ) throws -> [String: JSONValue] {
        let errs = validationErrors(for: scope)
        if let first = errs.first {
            throw NSError(domain: "AgentHubProbe", code: 1, userInfo: [NSLocalizedDescriptionKey: first])
        }
        switch scope {
        case .ci:
            return [
                "probe": .object([
                    "notifications": .object([
                        "notify_github_ci": .bool(notifyGithubCi),
                        "github_ci_repositories": .array(
                            githubCiRepositories.map { .string($0.trimmingCharacters(in: .whitespacesAndNewlines)) }
                        ),
                        "github_ci_filter_bots": .bool(githubCiFilterBots),
                    ]),
                ]),
            ]
        case .bark:
            let key = (submittedDeviceKey ?? barkDeviceKey).trimmingCharacters(in: .whitespacesAndNewlines)
            var notifObj: [String: JSONValue] = [
                "enabled": .bool(barkEnabled),
                "server_url": .string(barkServerUrl.trimmingCharacters(in: .whitespacesAndNewlines)),
            ]
            if !key.isEmpty {
                notifObj["device_key"] = .string(key)
            }
            return [
                "probe": .object([
                    "notifications": .object(notifObj),
                ]),
            ]
        case .settings:
            let pollSeconds = Int(pollSecondsText.trimmingCharacters(in: .whitespaces)) ?? 15
            let recentLimit = Int(recentLimitText.trimmingCharacters(in: .whitespaces)) ?? 50
            let retentionDays = Int(eventRetentionDaysText.trimmingCharacters(in: .whitespaces)) ?? 2
            let hookMaxLines = Int(hookEventMaxLinesText.trimmingCharacters(in: .whitespaces)) ?? 500
            let cooldownLines = Int(hookCooldownMaxLinesText.trimmingCharacters(in: .whitespaces)) ?? 1000
            let logBytes = Self.validatedLogMaxBytes(from: logMaxMiBText) ?? (5 * 1024 * 1024)

            let trimmedHome = codexHome.trimmingCharacters(in: .whitespacesAndNewlines)
            let homePatch: JSONValue = (trimmedHome.isEmpty || trimmedHome.lowercased() == "auto") ? .null : .string(trimmedHome)

            let notifObj: [String: JSONValue] = [
                "notify_completion": .bool(notifyCompletion),
                "notify_reply_needed": .bool(notifyReplyNeeded),
                "notify_recoverable": .bool(notifyRecoverable),
                "notify_codex": .bool(notifyCodex),
                "notify_grok": .bool(notifyGrok),
                "notify_grokbot": .bool(notifyGrokbot),
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

            return [
                "codex": .object([
                    "home": homePatch,
                    "host_label": .string(hostLabel.trimmingCharacters(in: .whitespacesAndNewlines)),
                ]),
                "probe": .object([
                    "enabled": .bool(probeEnabled),
                    "poll_seconds": .number(Double(pollSeconds)),
                    "recent_limit": .number(Double(recentLimit)),
                    "error_monitor": .object(["enabled": .bool(errorMonitorEnabled)]),
                    "notifications": .object(notifObj),
                    "observability": .object([
                        "event_retention_days": .number(Double(retentionDays)),
                        "hook_event_max_lines": .number(Double(hookMaxLines)),
                        "hook_cooldown_max_lines": .number(Double(cooldownLines)),
                        "log_max_bytes": .number(Double(logBytes)),
                    ]),
                ]),
            ]
        }
    }

    public func subsetEquals(_ other: ProbeSettingsDraft, scope: ProbeSettingsSaveScope) -> Bool {
        switch scope {
        case .ci:
            return notifyGithubCi == other.notifyGithubCi &&
                githubCiRepositories == other.githubCiRepositories &&
                githubCiFilterBots == other.githubCiFilterBots
        case .bark:
            return barkEnabled == other.barkEnabled &&
                barkServerUrl == other.barkServerUrl &&
                barkDeviceKey == other.barkDeviceKey &&
                barkDeviceKeyConfigured == other.barkDeviceKeyConfigured
        case .settings:
            let runtimeEqual: Bool =
                codexHome == other.codexHome &&
                hostLabel == other.hostLabel &&
                probeEnabled == other.probeEnabled &&
                errorMonitorEnabled == other.errorMonitorEnabled
            let limitsEqual: Bool =
                pollSecondsText == other.pollSecondsText &&
                recentLimitText == other.recentLimitText &&
                eventRetentionDaysText == other.eventRetentionDaysText &&
                hookEventMaxLinesText == other.hookEventMaxLinesText &&
                hookCooldownMaxLinesText == other.hookCooldownMaxLinesText &&
                logMaxMiBText == other.logMaxMiBText
            let codexEqual: Bool =
                notifyCompletion == other.notifyCompletion &&
                notifyReplyNeeded == other.notifyReplyNeeded &&
                notifyRecoverable == other.notifyRecoverable &&
                notifyCodex == other.notifyCodex
            let grokEqual: Bool =
                notifyGrok == other.notifyGrok &&
                notifyGrokCompletion == other.notifyGrokCompletion &&
                notifyGrokFailure == other.notifyGrokFailure &&
                notifyGrokReplyNeeded == other.notifyGrokReplyNeeded &&
                notifyGrokbot == other.notifyGrokbot
            let claudeEqual: Bool =
                notifyClaude == other.notifyClaude &&
                notifyClaudeCompletion == other.notifyClaudeCompletion &&
                notifyClaudeFailure == other.notifyClaudeFailure &&
                notifyClaudeReplyNeeded == other.notifyClaudeReplyNeeded
            let antigravityEqual: Bool =
                notifyAntigravity == other.notifyAntigravity &&
                notifyAntigravityCompletion == other.notifyAntigravityCompletion &&
                notifyAntigravityFailure == other.notifyAntigravityFailure &&
                notifyAntigravityReplyNeeded == other.notifyAntigravityReplyNeeded &&
                notifyAntigravityAuthorization == other.notifyAntigravityAuthorization
            let dshEqual: Bool =
                notifyDsh == other.notifyDsh &&
                notifyDshCompletion == other.notifyDshCompletion &&
                notifyDshFailure == other.notifyDshFailure &&
                notifyDshReplyNeeded == other.notifyDshReplyNeeded &&
                notifyDshAuthorization == other.notifyDshAuthorization
            let providersEqual: Bool = codexEqual && grokEqual && claudeEqual && antigravityEqual && dshEqual
            return runtimeEqual && limitsEqual && providersEqual
        }
    }

    public mutating func applySubset(from refreshed: ProbeSettingsDraft, scope: ProbeSettingsSaveScope) {
        switch scope {
        case .ci:
            notifyGithubCi = refreshed.notifyGithubCi
            githubCiRepositories = refreshed.githubCiRepositories
            githubCiFilterBots = refreshed.githubCiFilterBots
        case .bark:
            barkEnabled = refreshed.barkEnabled
            barkServerUrl = refreshed.barkServerUrl
            barkDeviceKey = refreshed.barkDeviceKey
            barkDeviceKeyConfigured = refreshed.barkDeviceKeyConfigured
        case .settings:
            codexHome = refreshed.codexHome
            hostLabel = refreshed.hostLabel
            probeEnabled = refreshed.probeEnabled
            errorMonitorEnabled = refreshed.errorMonitorEnabled
            pollSecondsText = refreshed.pollSecondsText
            recentLimitText = refreshed.recentLimitText
            eventRetentionDaysText = refreshed.eventRetentionDaysText
            hookEventMaxLinesText = refreshed.hookEventMaxLinesText
            hookCooldownMaxLinesText = refreshed.hookCooldownMaxLinesText
            logMaxMiBText = refreshed.logMaxMiBText
            notifyCompletion = refreshed.notifyCompletion
            notifyReplyNeeded = refreshed.notifyReplyNeeded
            notifyRecoverable = refreshed.notifyRecoverable
            notifyCodex = refreshed.notifyCodex
            notifyGrok = refreshed.notifyGrok
            notifyGrokCompletion = refreshed.notifyGrokCompletion
            notifyGrokFailure = refreshed.notifyGrokFailure
            notifyGrokReplyNeeded = refreshed.notifyGrokReplyNeeded
            notifyGrokbot = refreshed.notifyGrokbot
            notifyClaude = refreshed.notifyClaude
            notifyClaudeCompletion = refreshed.notifyClaudeCompletion
            notifyClaudeFailure = refreshed.notifyClaudeFailure
            notifyClaudeReplyNeeded = refreshed.notifyClaudeReplyNeeded
            notifyAntigravity = refreshed.notifyAntigravity
            notifyAntigravityCompletion = refreshed.notifyAntigravityCompletion
            notifyAntigravityFailure = refreshed.notifyAntigravityFailure
            notifyAntigravityReplyNeeded = refreshed.notifyAntigravityReplyNeeded
            notifyAntigravityAuthorization = refreshed.notifyAntigravityAuthorization
            notifyDsh = refreshed.notifyDsh
            notifyDshCompletion = refreshed.notifyDshCompletion
            notifyDshFailure = refreshed.notifyDshFailure
            notifyDshReplyNeeded = refreshed.notifyDshReplyNeeded
            notifyDshAuthorization = refreshed.notifyDshAuthorization
        }
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
        monitoredRepositories = try c.decodeIfPresent([String].self, forKey: .monitoredRepositories) ?? []
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
