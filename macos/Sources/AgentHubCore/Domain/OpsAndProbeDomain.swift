import Foundation

// MARK: - Runtime Capability Matrix

public struct RuntimeCapabilityMatrix: Equatable, Sendable {
    public var isReady: Bool
    public var hostSurface: String
    public var threadSubagents: Bool
    public var claudeSubagents: Bool
    public var antigravitySubagents: Bool
    public var hiddenCleanupScan: Bool
    public var codexAccountScope: Bool
    public var githubCi: Bool
    public var antigravitySessions: Bool
    public var dshSessions: Bool
    public var antigravityNotifications: Bool
    public var dshNotifications: Bool
    public var grokbotSessions: Bool
    public var grokbotNotifications: Bool
    public var codexDesktop: Bool
    public var updatePrune: Bool
    public var updateServiceLabels: Bool

    public static let bootstrap = RuntimeCapabilityMatrix(
        isReady: false,
        hostSurface: SystemCapabilitiesResponse.desktopSurface,
        threadSubagents: false,
        claudeSubagents: false,
        antigravitySubagents: false,
        hiddenCleanupScan: false,
        codexAccountScope: false,
        githubCi: false,
        antigravitySessions: true,
        dshSessions: true,
        antigravityNotifications: false,
        dshNotifications: false,
        grokbotSessions: true,
        grokbotNotifications: false,
        codexDesktop: false,
        updatePrune: false,
        updateServiceLabels: false
    )

    public static func from(_ response: SystemCapabilitiesResponse?) -> RuntimeCapabilityMatrix {
        guard let response else { return .bootstrap }
        let c = response.capabilities
        let remote = response.hostSurface == "linux_server_api"
        return RuntimeCapabilityMatrix(
            isReady: true,
            hostSurface: response.hostSurface,
            threadSubagents: c.threadSubagents,
            claudeSubagents: c.claudeSubagents,
            antigravitySubagents: c.antigravitySubagents,
            hiddenCleanupScan: c.hiddenCleanupScan,
            codexAccountScope: c.codexAccountScope,
            githubCi: c.githubCi,
            antigravitySessions: c.antigravitySessions,
            dshSessions: c.dshSessions,
            antigravityNotifications: c.antigravityNotifications,
            dshNotifications: !remote && c.dshNotifications,
            grokbotSessions: !remote && c.grokbotSessions,
            grokbotNotifications: !remote && c.grokbotNotifications,
            codexDesktop: !remote && c.codexDesktop,
            updatePrune: remote && c.pruneBackups,
            updateServiceLabels: remote && c.linuxUpdateJob
        )
    }

    public var visibleWorkspaces: [WorkspaceId] {
        WorkspaceId.allCases.filter { id in
            switch id {
            case .grokbot: return grokbotSessions
            default: return true
            }
        }
    }
}

// MARK: - Update & Cleanup Presentation Helpers

public enum UnifiedUpdateActionKind: String, Sendable {
    case check
    case install
    case prune
}

public enum UpdateVersionFormatter {
    public static func bareVersion(_ raw: String?) -> String {
        guard let raw else { return "" }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "" }
        if (trimmed.hasPrefix("v") || trimmed.hasPrefix("V")),
           let second = trimmed.dropFirst().first,
           second.isNumber
        {
            return String(trimmed.dropFirst())
        }
        return trimmed
    }

    public static func displayVersion(_ raw: String?) -> String {
        let bare = bareVersion(raw)
        return bare.isEmpty ? "未知" : bare
    }

    public static func isValidSemver(_ raw: String?) -> Bool {
        let bare = bareVersion(raw)
        guard !bare.isEmpty else { return false }
        return bare.range(of: #"^\d+\.\d+\.\d+(?:[-+].*)?$"#, options: .regularExpression) != nil
    }

    public static func canStartUpdateInstall(_ status: UpdateStatus?) -> Bool {
        guard let status,
              status.updateAvailable == true,
              status.state != "failed",
              status.state != "unsupported",
              isValidSemver(status.latestVersion)
        else { return false }
        return bareVersion(status.latestVersion) != bareVersion(status.currentVersion)
    }
}

public struct UpdateActionViewItem: Equatable, Sendable, Identifiable {
    public var id: String { action.rawValue }
    public let action: UnifiedUpdateActionKind
    public let label: String
    public let tone: String
    public let disabled: Bool

    public static func panelTitle(capabilities: RuntimeCapabilityMatrix) -> String {
        capabilities.updateServiceLabels ? "远程服务更新" : "本机 App 更新"
    }

    public static func actions(status: UpdateStatus?, capabilities: RuntimeCapabilityMatrix) -> [UpdateActionViewItem] {
        var out: [UpdateActionViewItem] = [
            UpdateActionViewItem(
                action: .check,
                label: "检查更新",
                tone: "secondary",
                disabled: false
            ),
        ]
        if UpdateVersionFormatter.canStartUpdateInstall(status) {
            out.append(
                UpdateActionViewItem(
                    action: .install,
                    label: "更新至 \(UpdateVersionFormatter.displayVersion(status?.latestVersion))",
                    tone: "primary",
                    disabled: false
                )
            )
        }
        if capabilities.updatePrune {
            out.append(
                UpdateActionViewItem(
                    action: .prune,
                    label: "清理更新备份",
                    tone: "danger",
                    disabled: false
                )
            )
        }
        return out
    }
}

public enum CleanupFormatting {
    public static func formatBytes(_ bytes: Int64) -> String {
        if bytes <= 0 { return "0 B" }
        let units = ["B", "KB", "MB", "GB"]
        var value = Double(bytes)
        var idx = 0
        while value >= 1024.0 && idx < units.count - 1 {
            value /= 1024.0
            idx += 1
        }
        return idx == 0 ? "\(bytes) B" : String(format: "%.1f %@", value, units[idx])
    }
}

// MARK: - Probe & GitHub CI Presentation Helpers

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
