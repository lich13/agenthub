import Foundation

// MARK: - Runtime Capability Matrix

public struct RuntimeCapabilityMatrix: Equatable, Sendable {
    var isReady: Bool
    var hostSurface: String
    var threadSubagents: Bool
    var claudeSubagents: Bool
    var antigravitySubagents: Bool
    var hiddenCleanupScan: Bool
    var codexAccountScope: Bool
    var githubCi: Bool
    var antigravitySessions: Bool
    var dshSessions: Bool
    var antigravityNotifications: Bool
    var dshNotifications: Bool
    var grokbotSessions: Bool
    var grokbotNotifications: Bool
    var updatePrune: Bool
    var updateServiceLabels: Bool

    static let bootstrap = RuntimeCapabilityMatrix(
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
        updatePrune: false,
        updateServiceLabels: false
    )

    static func from(_ response: SystemCapabilitiesResponse?) -> RuntimeCapabilityMatrix {
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
            updatePrune: remote && c.pruneBackups,
            updateServiceLabels: remote && c.linuxUpdateJob
        )
    }

    var visibleWorkspaces: [WorkspaceId] {
        WorkspaceId.allCases.filter { id in
            switch id {
            case .grokbot: return grokbotSessions
            default: return true
            }
        }
    }
}

// MARK: - Update & Cleanup Presentation Helpers

enum UnifiedUpdateActionKind: String, Sendable {
    case check
    case install
    case prune
}

enum UpdateVersionFormatter {
    static func bareVersion(_ raw: String?) -> String {
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

    static func displayVersion(_ raw: String?) -> String {
        let bare = bareVersion(raw)
        return bare.isEmpty ? "未知" : bare
    }

    static func isValidSemver(_ raw: String?) -> Bool {
        let bare = bareVersion(raw)
        guard !bare.isEmpty else { return false }
        return bare.range(of: #"^\d+\.\d+\.\d+(?:[-+].*)?$"#, options: .regularExpression) != nil
    }

    static func canStartUpdateInstall(_ status: UpdateStatus?) -> Bool {
        guard let status,
              status.updateAvailable == true,
              status.state != "failed",
              status.state != "unsupported",
              isValidSemver(status.latestVersion)
        else { return false }
        return bareVersion(status.latestVersion) != bareVersion(status.currentVersion)
    }
}

struct UpdateActionViewItem: Equatable, Sendable, Identifiable {
    var id: String { action.rawValue }
    let action: UnifiedUpdateActionKind
    let label: String
    let tone: String
    let disabled: Bool

    static func panelTitle(capabilities: RuntimeCapabilityMatrix) -> String {
        capabilities.updateServiceLabels ? "远程服务更新" : "本机 App 更新"
    }

    static func actions(status: UpdateStatus?, capabilities: RuntimeCapabilityMatrix) -> [UpdateActionViewItem] {
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

enum CleanupFormatting {
    static func formatBytes(_ bytes: Int64) -> String {
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

enum ProbeFormatting {
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

    static func eventKindLabel(_ raw: String?) -> String {
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

    static func barkBadge(_ event: ProbeEventItem) -> (label: String, tone: StatusChipTone) {
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
                return ("等待重试 · \(SharedTimestampFormatters.formatTimeOfDay(date))", .warning)
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

    static func dedupeBadge(_ event: ProbeEventItem) -> (label: String, tone: StatusChipTone)? {
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

struct GithubCiRepoOption: Equatable, Identifiable, Sendable {
    var id: String { key }
    let key: String
    let repoId: String
    let fullName: String
}

enum GithubCiDomain {
    static func runStatus(_ run: GithubCiRun) -> (label: String, tone: StatusChipTone) {
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

    static func visibleRuns(
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

    static func formatRunUpdatedAt(_ raw: String) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "—" }
        guard let date = SharedTimestampFormatters.parseISO8601(trimmed) else {
            return trimmed
        }
        return SharedTimestampFormatters.formatMonthDayTime(date)
    }

    static func buildCatalog(
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

    static func isSelected(_ option: GithubCiRepoOption, configured: [String]) -> Bool {
        let lowerName = option.fullName.lowercased()
        let lowerId = option.repoId.lowercased()
        return configured.contains { item in
            let t = item.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            return !t.isEmpty && (t == lowerName || t == lowerId)
        }
    }

    static func toggleSelection(_ option: GithubCiRepoOption, configured: [String]) -> [String] {
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

    static func addSelection(
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

enum ProbeDisclosureSection: String, CaseIterable, Hashable, Sendable {
    case ci
    case ciRepos = "ci.repos"
    case advanced
    case bark
    case hosts
    case live
    case events
    case jobs
}

struct ProbeDisclosureRules: Equatable, Sendable {
    private static let tokenSeparator = "\u{001F}"
    static let defaultExpandedSections: Set<ProbeDisclosureSection> = [.ci, .bark, .hosts]

    private(set) var machineScope: String
    private(set) var openSections: Set<ProbeDisclosureSection>
    private(set) var activeIssueTokens: [ProbeDisclosureSection: Set<String>]

    init(
        machineScope: String = "",
        openSections: Set<ProbeDisclosureSection> = ProbeDisclosureRules.defaultExpandedSections
    ) {
        self.machineScope = machineScope
        self.openSections = openSections
        self.activeIssueTokens = [:]
    }

    func isExpanded(_ section: ProbeDisclosureSection) -> Bool {
        openSections.contains(section)
    }

    mutating func reset(machineScope: String) {
        self.machineScope = machineScope
        self.openSections = Self.defaultExpandedSections
        self.activeIssueTokens = [:]
    }

    mutating func resetIfMachineChanged(_ nextScope: String) {
        guard machineScope != nextScope else { return }
        reset(machineScope: nextScope)
    }

    mutating func setExpanded(_ section: ProbeDisclosureSection, _ expanded: Bool) {
        if expanded {
            openSections.insert(section)
        } else {
            openSections.remove(section)
        }
    }

    @discardableResult
    mutating func observeIssues(
        machineScope nextScope: String,
        section: ProbeDisclosureSection,
        tokens: Set<String>
    ) -> Bool {
        resetIfMachineChanged(nextScope)
        let cleaned = Set(
            tokens
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        )
        guard !cleaned.isEmpty else {
            activeIssueTokens.removeValue(forKey: section)
            return false
        }
        let previous = activeIssueTokens[section] ?? []
        let hasNewIssue = !cleaned.subtracting(previous).isEmpty
        activeIssueTokens[section] = cleaned
        guard hasNewIssue else {
            return false
        }
        openSections.insert(section)
        return true
    }

    @discardableResult
    mutating func observeIssue(
        machineScope nextScope: String,
        section: ProbeDisclosureSection,
        issueSignature: String?
    ) -> Bool {
        let tokens: Set<String> = if let issueSignature {
            Set(issueSignature.components(separatedBy: Self.tokenSeparator))
        } else {
            []
        }
        return observeIssues(machineScope: nextScope, section: section, tokens: tokens)
    }

    private static func joinTokens(_ tokens: [String]) -> String? {
        let uniqueSorted = Array(Set(tokens)).sorted()
        return uniqueSorted.isEmpty ? nil : uniqueSorted.joined(separator: tokenSeparator)
    }

    static func ciIssueSignature(
        errorMessage: String?,
        snapshotError: String?,
        visibleRuns: [GithubCiRun]
    ) -> String? {
        var tokens: [String] = []
        if let err = (errorMessage ?? snapshotError)?.trimmingCharacters(in: .whitespacesAndNewlines), !err.isEmpty {
            tokens.append("err:\(err)")
        }
        for run in visibleRuns {
            let state = run.status == "completed" ? (run.conclusion ?? "completed") : run.status
            if state == "failure" || state == "timed_out" || state == "startup_failure" {
                tokens.append("run:\(run.id):\(run.runAttempt ?? 1):\(state)")
            }
        }
        return joinTokens(tokens)
    }

    static func hostsIssueSignature(
        codexHookStatus: String?,
        statusError: String?,
        hookErrors: [String: String]
    ) -> String? {
        var tokens: [String] = []
        if codexHookStatus == "stale" {
            tokens.append("codex:stale")
        } else if codexHookStatus == nil, let err = statusError, !err.isEmpty {
            tokens.append("codex:unavailable")
        }
        for key in hookErrors.keys.sorted() {
            if let err = hookErrors[key]?.trimmingCharacters(in: .whitespacesAndNewlines), !err.isEmpty {
                tokens.append("hook:\(key):\(err)")
            }
        }
        return joinTokens(tokens)
    }

    static func liveIssueSignature(sessions: [ProbeLiveSession]) -> String? {
        let tokens = sessions
            .filter { $0.bucket == "reply_needed" || $0.bucket == "recoverable" }
            .map { "live:\($0.uniqueKey):\($0.bucket ?? "")" }
        return joinTokens(tokens)
    }

    static func eventsIssueSignature(eventsError: String?) -> String? {
        guard let err = eventsError?.trimmingCharacters(in: .whitespacesAndNewlines), !err.isEmpty else {
            return nil
        }
        return "err:\(err)"
    }
}

// MARK: - Probe Settings Out-of-Order Read/Save Fencing

struct ProbeSettingsSyncState: Equatable, Sendable {
    var settingsView: ProbeSettingsView?
    var settingsError: String?
    var loadedDraft: ProbeSettingsDraft?
    var draft: ProbeSettingsDraft?
    var draftRevision: UInt64
    private(set) var settingsFence: UInt64 = 0

    init(revision: UInt64 = 0) {
        self.draftRevision = revision
    }

    mutating func reset(revision: UInt64) {
        settingsFence &+= 1
        settingsView = nil
        settingsError = nil
        loadedDraft = nil
        draft = nil
        draftRevision = revision
    }

    func beginSettingsRead() -> UInt64 {
        settingsFence
    }

    @discardableResult
    mutating func applyLoadedSettings(
        _ loaded: ProbeSettingsView,
        readFence: UInt64,
        revision: UInt64
    ) -> Bool {
        guard readFence == settingsFence else { return false }
        settingsView = loaded
        settingsError = nil
        let parsed = ProbeSettingsDraft.from(loaded)
        let previousBaseline = loadedDraft
        loadedDraft = parsed
        if var currentDraft = draft, draftRevision == revision {
            for subScope in [ProbeSettingsSaveScope.ci, .settings, .bark] {
                let isClean = previousBaseline.map { currentDraft.subsetEquals($0, scope: subScope) } ?? true
                if isClean {
                    currentDraft.applySubset(from: parsed, scope: subScope)
                }
            }
            draft = currentDraft
        } else {
            draft = parsed
            draftRevision = revision
        }
        return true
    }

    @discardableResult
    mutating func applySettingsReadError(
        _ message: String,
        readFence: UInt64
    ) -> Bool {
        guard readFence == settingsFence else { return false }
        settingsError = message
        return true
    }

    mutating func applySavedSettings(
        _ saved: ProbeSettingsView,
        submittedDraft: ProbeSettingsDraft,
        scope: ProbeSettingsSaveScope,
        revision: UInt64
    ) {
        settingsFence &+= 1
        settingsView = saved
        settingsError = nil
        draftRevision = revision
        var refreshed = ProbeSettingsDraft.from(saved)
        if scope == .bark && !submittedDraft.barkDeviceKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            refreshed.barkDeviceKeyConfigured = true
        }
        if var baseline = loadedDraft {
            baseline.applySubset(from: refreshed, scope: scope)
            loadedDraft = baseline
        } else {
            loadedDraft = refreshed
        }
        if var live = draft, live.subsetEquals(submittedDraft, scope: scope) {
            live.applySubset(from: refreshed, scope: scope)
            draft = live
        }
    }
}
