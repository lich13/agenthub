import Foundation

// MARK: - Flexible JSON Value & OptionalResult Wrapper

public enum JSONValue: Codable, Equatable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let b = try? container.decode(Bool.self) {
            self = .bool(b)
        } else if let d = try? container.decode(Double.self) {
            self = .number(d)
        } else if let s = try? container.decode(String.self) {
            self = .string(s)
        } else if let a = try? container.decode([JSONValue].self) {
            self = .array(a)
        } else if let o = try? container.decode([String: JSONValue].self) {
            self = .object(o)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unsupported JSONValue")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case let .bool(b): try container.encode(b)
        case let .number(n):
            if n.rounded(.towardZero) == n, let i = Int64(exactly: n) {
                try container.encode(i)
            } else {
                try container.encode(n)
            }
        case let .string(s): try container.encode(s)
        case let .array(a): try container.encode(a)
        case let .object(o): try container.encode(o)
        }
    }
}

public struct OptionalResultEnvelope<T: Decodable & Sendable>: Decodable, Sendable {
    public let available: Bool
    public let data: T?

    private enum CodingKeys: String, CodingKey {
        case available, data, reason, error
    }

    public init(available: Bool, data: T?) {
        self.available = available
        self.data = data
    }

    public init(from decoder: Decoder) throws {
        if let keyed = try? decoder.container(keyedBy: CodingKeys.self),
           keyed.contains(.data) || keyed.contains(.error) || keyed.contains(.reason),
           let avail = try? keyed.decode(Bool.self, forKey: .available)
        {
            self.available = avail
            self.data = try keyed.decodeIfPresent(T.self, forKey: .data)
            return
        }
        let direct = try T(from: decoder)
        self.available = true
        self.data = direct
    }
}

// MARK: - System Capabilities

public struct SystemCapabilitiesCore: Codable, Equatable, Sendable {
    public var codexDesktop = false
    public var systemd = false
    public var linuxUpdateJob = false
    public var pruneBackups = false
    public var antigravityNotifications = false
    public var dshNotifications = false
    public var grokbotSessions = false
    public var grokbotNotifications = false

    enum CodingKeys: String, CodingKey {
        case codexDesktop = "codex_desktop"
        case systemd
        case linuxUpdateJob = "linux_update_job"
        case pruneBackups = "prune_backups"
        case antigravityNotifications = "antigravity_notifications"
        case dshNotifications = "dsh_notifications"
        case grokbotSessions = "grokbot_sessions"
        case grokbotNotifications = "grokbot_notifications"
    }

    public init() {}
}

public struct SystemCapabilitiesResponse: Codable, Equatable, Sendable {
    public var apiVersion: Int
    public var hostSurface: String
    public var capabilities: SystemCapabilitiesCore

    enum CodingKeys: String, CodingKey {
        case apiVersion = "api_version"
        case hostSurface = "host_surface"
        case capabilities
    }

    // Protocol 2 keeps this wire identifier for the native macOS host.
    public static let desktopSurface = "desktop_embedded_tauri"

    public init(apiVersion: Int = 2, hostSurface: String = SystemCapabilitiesResponse.desktopSurface, capabilities: SystemCapabilitiesCore = SystemCapabilitiesCore()) {
        self.apiVersion = apiVersion
        self.hostSurface = hostSurface
        self.capabilities = capabilities
    }

}

public struct RuntimeCapabilityMatrix: Equatable, Sendable {
    public var isReady: Bool
    public var hostSurface: String
    public var codexStatePaths: Bool
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
        codexStatePaths: false,
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
            codexStatePaths: remote && c.systemd,
            antigravityNotifications: !remote && c.antigravityNotifications,
            dshNotifications: !remote && c.dshNotifications,
            grokbotSessions: !remote && (c.grokbotSessions),
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

// MARK: - Remote Connection DTOs

public enum MachineTarget: String, Codable, Equatable, Sendable {
    case local
    case remote
}

public struct RemoteMachineSummary: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var sshHost: String
    public var useSudo: Bool

    public init(id: String, name: String, sshHost: String, useSudo: Bool) {
        self.id = id
        self.name = name
        self.sshHost = sshHost
        self.useSudo = useSudo
    }
}

public struct RemoteConnectionView: Codable, Equatable, Sendable {
    public var target: MachineTarget
    public var revision: UInt64
    public var sshHost: String?
    public var configured: Bool
    public var activeMachineId: String?
    public var activeMachineName: String?
    public var machines: [RemoteMachineSummary]?

    public init(
        target: MachineTarget = .local,
        revision: UInt64 = 0,
        sshHost: String? = nil,
        configured: Bool = false,
        activeMachineId: String? = nil,
        activeMachineName: String? = nil,
        machines: [RemoteMachineSummary]? = []
    ) {
        self.target = target
        self.revision = revision
        self.sshHost = sshHost
        self.configured = configured
        self.activeMachineId = activeMachineId
        self.activeMachineName = activeMachineName
        self.machines = machines
    }
}

public struct RemoteConnectionCredentials: Codable, Equatable, Sendable {
    public var revision: UInt64
    public var sshHost: String
    public var useSudo: Bool
    public var id: String?
    public var name: String?

    public init(revision: UInt64, sshHost: String, useSudo: Bool, id: String? = nil, name: String? = nil) {
        self.revision = revision
        self.sshHost = sshHost
        self.useSudo = useSudo
        self.id = id
        self.name = name
    }
}

public struct SshHostItem: Codable, Equatable, Sendable {
    public var alias: String
}

// MARK: - Jobs & Action Responses (Dual-end camelCase & snake_case compatible)

public struct StartedJobResponse: Decodable, Equatable, Sendable {
    public let jobId: String?
    public let threadId: String?

    enum CodingKeys: String, CodingKey {
        case jobId
        case jobIdSnake = "job_id"
        case threadId
        case threadIdSnake = "thread_id"
    }

    public init(jobId: String?, threadId: String? = nil) {
        self.jobId = jobId
        self.threadId = threadId
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.jobId = try c.decodeIfPresent(String.self, forKey: .jobId)
            ?? c.decodeIfPresent(String.self, forKey: .jobIdSnake)
        self.threadId = try c.decodeIfPresent(String.self, forKey: .threadId)
            ?? c.decodeIfPresent(String.self, forKey: .threadIdSnake)
    }
}

public struct JobFailureAnalysis: Codable, Equatable, Sendable {
    public var explanation: String
    public var suggestions: [String]
}

public struct JobRecord: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var kind: String
    public var status: String
    public var title: String
    public var startedAt: Int64
    public var finishedAt: Int64?
    public var exitCode: Int32?
    public var output: String?
    public var error: String?
    public var failureAnalysis: JobFailureAnalysis?

    enum CodingKeys: String, CodingKey {
        case id, kind, status, title, output, error
        case startedAt = "started_at"
        case finishedAt = "finished_at"
        case exitCode = "exit_code"
        case failureAnalysis
    }

    public var isTerminal: Bool {
        status == "succeeded" || status == "failed" || status == "cancelled"
    }
}

// MARK: - Updates, Unified Cleanup & Codex Account Scope

public struct UpdateStatus: Codable, Equatable, Sendable {
    public var currentVersion: String
    public var latestVersion: String?
    public var updateAvailable: Bool?
    public var state: String
    public var method: String?
    public var recommendedAction: String?

    enum CodingKeys: String, CodingKey {
        case currentVersion = "current_version"
        case latestVersion = "latest_version"
        case updateAvailable = "update_available"
        case state, method
        case recommendedAction = "recommended_action"
    }
}

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

    public static func displayWithVPrefix(_ raw: String?) -> String {
        let bare = bareVersion(raw)
        guard !bare.isEmpty else { return "未知" }
        return bare.first?.isNumber == true ? "v\(bare)" : bare
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

public struct UnifiedCleanupCandidate: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var title: String
    public var estimatedBytes: Int64
    public var blockedReason: String?
    public var fingerprint: String?
}

public struct UnifiedCleanupScopePlan: Codable, Equatable, Identifiable, Sendable {
    public var id: String { scope }
    public var scope: String
    public var supported: Bool
    public var scanned: Bool
    public var unavailableReason: String?
    public var candidateCount: Int
    public var deletableCount: Int
    public var estimatedBytes: Int64
    public var candidates: [UnifiedCleanupCandidate]
    public var issues: [String]
}

public struct UnifiedCleanupPlan: Codable, Equatable, Sendable {
    public var totalCandidates: Int
    public var totalDeletable: Int
    public var totalEstimatedBytes: Int64
    public var scopes: [UnifiedCleanupScopePlan]
}

public struct UnifiedCleanupTargetSelection: Codable, Equatable, Sendable {
    public let scope: String
    public let id: String
    public let fingerprint: String?
}

public struct UnifiedCleanupExecuteRequest: Codable, Equatable, Sendable {
    public let confirmed: Bool
    public let scopes: [String]
    public let targets: [UnifiedCleanupTargetSelection]
}

public struct UnifiedCleanupScopeResult: Codable, Equatable, Sendable {
    public var scope: String
    public var label: String
    public var deletedCount: Int
    public var failedCount: Int
    public var skippedCount: Int
    public var deletedBytes: Int64
    public var issues: [String]
    public var residuals: [String]
}

public struct UnifiedCleanupResult: Codable, Equatable, Sendable {
    public var deletedCount: Int
    public var failedCount: Int
    public var skippedCount: Int
    public var deletedBytes: Int64
    public var scopes: [UnifiedCleanupScopeResult]

    var unsuccessfulDetails: [String] {
        scopes.flatMap { scope in
            (scope.issues + scope.residuals).map { "\(scope.label) · \($0)" }
        }
    }
}

public struct HiddenScanProgress: Codable, Equatable, Sendable {
    public enum Phase: String, Codable, Sendable {
        case preparing, scanning, complete, stopping, stopped, failed
    }

    public var id: String
    public var phase: Phase
    public var processed: Int
    public var total: Int
    public var cursor: Int
    public var nextCursor: Int
    public var hasMore: Bool
    public var error: String?
}

public struct HiddenThreadCandidate: Codable, Equatable, Sendable {
    public var id: String
    public var title: String
    public var fingerprint: String?
    public var allowed: Bool
    public var reason: String?

    var unified: UnifiedCleanupCandidate {
        UnifiedCleanupCandidate(id: id, title: title.isEmpty ? id : title, estimatedBytes: 0,
                                blockedReason: allowed ? nil : (reason ?? "当前线程暂不可清理"), fingerprint: fingerprint)
    }
}

public struct HiddenThreadDeletePlan: Codable, Equatable, Sendable {
    public var candidates: [HiddenThreadCandidate]
    public var scan: HiddenScanProgress
}

public struct CodexAccountScopeStatus: Codable, Equatable, Sendable {
    public var scopedThreads: Int
    public var scopedFiles: Int
}

public struct CodexAccountScopeItem: Codable, Equatable, Identifiable, Sendable {
    public var id: String { threadId }
    public var threadId: String
    public var reason: String
    public var detail: String?

    public var reasonLabel: String {
        switch reason {
        case "format": "格式不支持"
        case "busy": "Codex 使用中"
        case "changed": "文件已变化"
        case "original_missing": "缺少原始会话"
        default: "失败"
        }
    }
}

public struct CodexAccountScopeResult: Codable, Equatable, Sendable {
    public var clearedThreads: Int
    public var clearedFiles: Int
    public var remainingFiles: Int
    public var items: [CodexAccountScopeItem]
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
