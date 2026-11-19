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
    public var codex: [String: JSONValue]?
    public var probe: [String: JSONValue]?
    public var notifications: [String: JSONValue]?
}

public enum ProbeNotificationPlatform: String, CaseIterable, Sendable, Identifiable {
    case codex
    case claudeCode = "claude_code"
    case grok
    case grokbot
    case antigravity
    case dsh

    public var id: String { rawValue }

    public var brand: AgentBrandId {
        switch self {
        case .codex: .codex
        case .claudeCode: .claude
        case .grok: .grok
        case .grokbot: .grokbot
        case .antigravity: .antigravity
        case .dsh: .dsh
        }
    }

    public var label: String {
        switch self {
        case .codex: "Codex"
        case .claudeCode: "Claude Code"
        case .grok: "Grok Build"
        case .grokbot: "Grok Bot"
        case .antigravity: "Antigravity"
        case .dsh: "DeepSeek Harness"
        }
    }
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
        func str(_ dict: [String: JSONValue]?, _ key: String, fallback: String = "") -> String {
            if case let .string(s)? = dict?[key] { return s }
            return fallback
        }
        func bool(_ dict: [String: JSONValue]?, _ key: String, fallback: Bool) -> Bool {
            if case let .bool(b)? = dict?[key] { return b }
            return fallback
        }
        func int(_ dict: [String: JSONValue]?, _ key: String, fallback: Int) -> Int {
            if case let .number(d)? = dict?[key] { return Int(d) }
            return fallback
        }
        func obj(_ dict: [String: JSONValue]?, _ key: String) -> [String: JSONValue]? {
            if case let .object(o)? = dict?[key] { return o }
            return nil
        }

        let codex = view.codex
        let probe = view.probe
        let hooks = obj(probe, "hooks")
        let errorMon = obj(probe, "error_monitor")
        let obs = obj(probe, "observability")
        let notif = view.notifications ?? obj(probe, "notifications")

        let logBytes = int(obs, "log_max_bytes", fallback: 5 * 1024 * 1024)
        let logMiB = Double(logBytes) / (1024.0 * 1024.0)
        let formattedMiB = logMiB.truncatingRemainder(dividingBy: 1) == 0
            ? String(Int(logMiB))
            : String(format: "%.6g", logMiB)

        let repos: [String] = {
            if case let .array(arr)? = notif?["github_ci_repositories"] {
                return arr.compactMap {
                    if case let .string(s) = $0 {
                        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
                        return t.isEmpty ? nil : t
                    }
                    return nil
                }
            }
            return []
        }()

        let configuredHome: String = {
            if case let .string(explicit)? = codex?["configured_home"] {
                return explicit
            }
            return str(codex, "home")
        }()

        return ProbeSettingsDraft(
            codexHome: configuredHome,
            codexWorkspace: str(codex, "workspace"),
            hostLabel: str(codex, "host_label", fallback: "AgentHub"),
            probeEnabled: bool(probe, "enabled", fallback: true),
            errorMonitorEnabled: bool(errorMon, "enabled", fallback: true),
            manageStopHook: bool(hooks, "manage_stop_hook", fallback: true),
            clearAccountScope: bool(hooks, "clear_account_scope", fallback: false),
            pollSecondsText: String(int(probe, "poll_seconds", fallback: 15)),
            recentLimitText: String(int(probe, "recent_limit", fallback: 50)),
            eventRetentionDaysText: String(int(obs, "event_retention_days", fallback: 2)),
            hookEventMaxLinesText: String(int(obs, "hook_event_max_lines", fallback: 500)),
            hookCooldownMaxLinesText: String(int(obs, "hook_cooldown_max_lines", fallback: 1000)),
            logMaxMiBText: formattedMiB,
            barkEnabled: bool(notif, "enabled", fallback: false),
            barkServerUrl: str(notif, "server_url", fallback: "https://api.day.app"),
            barkDeviceKey: "",
            barkDeviceKeyConfigured: bool(notif, "device_key_configured", fallback: false),
            barkSound: str(notif, "sound"),
            barkGroup: str(notif, "group", fallback: "AgentHub"),
            barkUrl: str(notif, "url"),
            notifyCompletion: bool(notif, "notify_completion", fallback: true),
            notifyReplyNeeded: bool(notif, "notify_reply_needed", fallback: true),
            notifyRecoverable: bool(notif, "notify_recoverable", fallback: true),
            notifyCodex: bool(notif, "notify_codex", fallback: true),
            notifyGrok: bool(notif, "notify_grok", fallback: true),
            notifyGrokCompletion: bool(notif, "notify_grok_completion", fallback: true),
            notifyGrokFailure: bool(notif, "notify_grok_failure", fallback: true),
            notifyGrokReplyNeeded: bool(notif, "notify_grok_reply_needed", fallback: true),
            notifyGrokbot: bool(notif, "notify_grokbot", fallback: false),
            notifyClaude: bool(notif, "notify_claude", fallback: true),
            notifyClaudeCompletion: bool(notif, "notify_claude_completion", fallback: true),
            notifyClaudeFailure: bool(notif, "notify_claude_failure", fallback: true),
            notifyClaudeReplyNeeded: bool(notif, "notify_claude_reply_needed", fallback: true),
            notifyAntigravity: bool(notif, "notify_antigravity", fallback: false),
            notifyAntigravityCompletion: bool(notif, "notify_antigravity_completion", fallback: false),
            notifyAntigravityFailure: bool(notif, "notify_antigravity_failure", fallback: false),
            notifyAntigravityReplyNeeded: bool(notif, "notify_antigravity_reply_needed", fallback: false),
            notifyAntigravityAuthorization: bool(notif, "notify_antigravity_authorization", fallback: false),
            notifyDsh: bool(notif, "notify_dsh", fallback: false),
            notifyDshCompletion: bool(notif, "notify_dsh_completion", fallback: false),
            notifyDshFailure: bool(notif, "notify_dsh_failure", fallback: false),
            notifyDshReplyNeeded: bool(notif, "notify_dsh_reply_needed", fallback: false),
            notifyDshAuthorization: bool(notif, "notify_dsh_authorization", fallback: false),
            notifyGithubCi: bool(notif, "notify_github_ci", fallback: false),
            githubCiRepositories: repos,
            githubCiFilterBots: bool(notif, "github_ci_filter_bots", fallback: true)
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

public enum ProbeFormatting {
    private static let safeBarkCategories: [String: String] = [
        "dns": "域名解析失败",
        "connection": "连接失败",
        "tls": "TLS 连接失败",
        "certificate_validation": "证书验证失败",
        "timeout": "请求超时",
        "response_decode": "响应无法确认",
        "response_read": "响应读取失败",
        "redirect_rejected": "重定向已拒绝",
        "http_status": "HTTP 拒绝",
        "bark_response_code": "服务拒绝",
        "apns_permanent_rejection": "推送服务永久拒绝",
        "invalid_server_url": "地址无效",
        "network_unknown": "网络错误",
        "request_error": "网络错误",
    ]

    private static let unknownReasons: Set<String> = [
        "request_error", "network_unknown", "timeout",
        "response_decode", "response_read", "delivery_interrupted_outcome_unknown",
    ]

    public static func eventKindLabel(_ raw: String?) -> String {
        let trimmed = raw?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !trimmed.isEmpty else { return "Probe 事件" }
        return switch trimmed {
        case "hook-stop", "hook_stop": "Stop Hook"
        case "completion": "完成"
        case "failure": "失败"
        case "reply-needed", "reply_needed": "需回复"
        case "recoverable": "异常/可恢复"
        case "running": "运行中"
        case "bark-test": "Bark 测试"
        default: trimmed
        }
    }

    public static func barkBadge(_ event: ProbeEventItem) -> (label: String, tone: StatusChipTone) {
        let barkObj: [String: JSONValue]? = {
            if case let .object(o)? = event.payload?["bark"] { return o }
            return nil
        }()
        func str(_ key: String) -> String? {
            if case let .string(s)? = barkObj?[key] { return s }
            return nil
        }
        func bool(_ key: String) -> Bool? {
            if case let .bool(b)? = barkObj?[key] { return b }
            return nil
        }
        func int(_ key: String) -> Int? {
            if case let .number(n)? = barkObj?[key], n.isFinite { return Int(n) }
            return nil
        }

        let status = (str("status") ?? event.barkStatus)?.trimmingCharacters(in: .whitespacesAndNewlines)
        if barkObj == nil && (status == nil || status?.isEmpty == true) {
            return ("Bark 未记录", .muted)
        }
        if bool("sent") == true || status == "sent" {
            return ("Bark 已受理", .success)
        }
        if bool("skipped") == true || status == "skipped" {
            return ("已跳过", .muted)
        }
        if status == "waiting_retry" {
            if let nextMs = int("next_retry_ms"), nextMs > 0 {
                let date = Date(timeIntervalSince1970: Double(nextMs) / 1000.0)
                let df = DateFormatter()
                df.dateFormat = "HH:mm:ss"
                return ("等待重试 · \(df.string(from: date))", .warning)
            }
            return ("等待重试", .warning)
        }
        if status == "retrying" {
            if let attempts = int("attempts"), attempts > 0 {
                return ("重试中 · 第 \(attempts) 次", .warning)
            }
            return ("重试中", .warning)
        }

        let reason = str("reason") ?? event.reason ?? ""
        let isUnknown = status == "unknown" || ((status == nil || status?.isEmpty == true) && unknownReasons.contains(reason))
        let base = isUnknown ? "结果未确认" : "发送失败"
        if let cat = safeBarkCategories[reason] {
            return ("\(base) · \(cat)", .warning)
        }
        return (base, .warning)
    }

    public static func dedupeBadge(_ event: ProbeEventItem) -> (label: String, tone: StatusChipTone)? {
        if let payload = event.payload {
            if case let .object(dedupe)? = payload["dedupe"] {
                if case .bool(true)? = dedupe["duplicate"] { return ("重复事件", .warning) }
                if case .bool(true)? = dedupe["claimed"] { return ("已认领", .success) }
                if case let .string(st)? = dedupe["status"], !st.isEmpty {
                    return (st, st.lowercased().contains("duplicate") ? .warning : .muted)
                }
            }
            if case .bool(true)? = payload["duplicate"] { return ("重复事件", .warning) }
        }
        if let ds = event.dedupeStatus, !ds.isEmpty {
            if ds == "duplicate" { return ("重复事件", .warning) }
            if ds == "claimed" { return ("已认领", .success) }
            return (ds, ds.lowercased().contains("duplicate") ? .warning : .muted)
        }
        if let dk = event.dedupeKey, !dk.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return ("去重键已记录", .muted)
        }
        return nil
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
        repositoryId = (try? c.decode(UInt64.self, forKey: .repositoryId)) ?? 0
        repositoryName = (try? c.decode(String.self, forKey: .repositoryName)) ?? ""
        name = try c.decodeIfPresent(String.self, forKey: .name)
        workflowName = (try? c.decode(String.self, forKey: .workflowName)) ?? (name ?? "工作流")
        displayTitle = try c.decodeIfPresent(String.self, forKey: .displayTitle)
        headBranch = try c.decodeIfPresent(String.self, forKey: .headBranch)
        headSha = (try? c.decode(String.self, forKey: .headSha)) ?? ""
        status = (try? c.decode(String.self, forKey: .status)) ?? "completed"
        conclusion = try c.decodeIfPresent(String.self, forKey: .conclusion)
        htmlUrl = (try? c.decode(String.self, forKey: .htmlUrl)) ?? ""
        updatedAt = (try? c.decode(String.self, forKey: .updatedAt)) ?? ""
        runAttempt = try c.decodeIfPresent(UInt64.self, forKey: .runAttempt)
        isBot = (try? c.decode(Bool.self, forKey: .isBot)) ?? false
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
}

public struct GithubCiRepoOption: Equatable, Identifiable, Sendable {
    public var id: String { key }
    public let key: String
    public let repoId: String
    public let fullName: String
}

public enum GithubCiDomain {
    public static func runStatus(_ run: GithubCiRun) -> (label: String, tone: StatusChipTone) {
        let state = run.status == "completed" ? (run.conclusion ?? "completed") : run.status
        let label: String = switch state {
        case "success": "成功"
        case "failure": "失败"
        case "timed_out": "超时"
        case "cancelled": "已取消"
        case "skipped": "已跳过"
        case "in_progress": "运行中"
        case "queued": "排队中"
        case "waiting", "requested", "pending": "等待中"
        case "action_required": "待处理"
        case "startup_failure": "启动失败"
        case "neutral", "completed": "已完成"
        case "stale": "已过期"
        default: state
        }
        let tone: StatusChipTone = if state == "success" {
            .success
        } else if ["failure", "timed_out", "startup_failure"].contains(state) {
            .danger
        } else if run.status != "completed" {
            .warning
        } else {
            .neutral
        }
        return (label, tone)
    }

    public static func visibleRuns(
        runs: [GithubCiRun],
        repositories: [String],
        filterBots: Bool = false
    ) -> [GithubCiRun] {
        let normalized = repositories
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
            .filter { !$0.isEmpty }
        let filtered = runs.filter { run in
            (!filterBots || !run.isBot) && (
                normalized.isEmpty || normalized.contains { repo in
                    String(run.repositoryId).lowercased() == repo ||
                    run.repositoryName.lowercased() == repo
                }
            )
        }
        let sorted = filtered.sorted { lhs, rhs in
            if lhs.updatedAt != rhs.updatedAt {
                return lhs.updatedAt > rhs.updatedAt
            }
            return false
        }
        return Array(sorted.prefix(20))
    }

    public static func formatRunUpdatedAt(_ raw: String) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "—" }
        let isoFrac = ISO8601DateFormatter()
        isoFrac.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let isoPlain = ISO8601DateFormatter()
        isoPlain.formatOptions = [.withInternetDateTime]
        guard let date = isoFrac.date(from: trimmed) ?? isoPlain.date(from: trimmed) else {
            return trimmed
        }
        let df = DateFormatter()
        df.dateFormat = "MM-dd HH:mm"
        return df.string(from: date)
    }

    public static func buildCatalog(
        repositories: [GithubCiRepository] = [],
        runs: [GithubCiRun] = [],
        configured: [String] = []
    ) -> [GithubCiRepoOption] {
        var catalog: [GithubCiRepoOption] = []
        var seenNames = Set<String>()
        var seenIds = Set<String>()

        func push(id: String, fullName: String) {
            let tName = fullName.trimmingCharacters(in: .whitespacesAndNewlines)
            let tId = id.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !tName.isEmpty else { return }
            let lowerName = tName.lowercased()
            let lowerId = tId.lowercased()
            if seenNames.contains(lowerName) || (!lowerId.isEmpty && seenIds.contains(lowerId)) {
                return
            }
            seenNames.insert(lowerName)
            if !lowerId.isEmpty { seenIds.insert(lowerId) }
            catalog.append(GithubCiRepoOption(key: tName, repoId: tId.isEmpty ? tName : tId, fullName: tName))
        }

        for repo in repositories where !repo.archived {
            push(id: String(repo.id), fullName: repo.fullName)
        }
        for run in runs {
            push(id: String(run.repositoryId), fullName: run.repositoryName)
        }
        for entry in configured {
            let t = entry.trimmingCharacters(in: .whitespacesAndNewlines)
            if !t.isEmpty { push(id: t, fullName: t) }
        }
        return catalog
    }

    public static func isSelected(_ option: GithubCiRepoOption, configured: [String]) -> Bool {
        let lowerName = option.fullName.lowercased()
        let lowerId = option.repoId.lowercased()
        return configured.contains { item in
            let t = item.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            return !t.isEmpty && (t == lowerName || t == lowerId)
        }
    }

    public static func toggleSelection(_ option: GithubCiRepoOption, configured: [String]) -> [String] {
        let lowerName = option.fullName.lowercased()
        let lowerId = option.repoId.lowercased()
        if isSelected(option, configured: configured) {
            return configured.filter { item in
                let t = item.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                return !t.isEmpty && t != lowerName && t != lowerId
            }
        }
        let cleaned = configured
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        return cleaned + [option.fullName]
    }

    public static func addSelection(
        query: String,
        catalog: [GithubCiRepoOption],
        configured: [String]
    ) -> [String] {
        let cleaned = configured
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return cleaned }
        let lower = trimmed.lowercased()
        let exact = catalog.first {
            $0.fullName.lowercased() == lower || $0.repoId.lowercased() == lower
        }
        let partials = exact != nil ? [exact!] : catalog.filter { $0.fullName.lowercased().contains(lower) }
        let matched = exact ?? (partials.count == 1 ? partials[0] : nil)
        let targetName = matched?.fullName ?? trimmed
        let already = cleaned.contains { item in
            let il = item.lowercased()
            return il == targetName.lowercased() || (matched != nil && il == matched!.repoId.lowercased())
        }
        return already ? cleaned : (cleaned + [targetName])
    }
}
