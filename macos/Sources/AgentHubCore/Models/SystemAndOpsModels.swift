import Foundation

// MARK: - Flexible JSON Value

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

// MARK: - System Capabilities

public struct SystemCapabilitiesCore: Codable, Equatable, Sendable {
    public var threadSubagents = false
    public var claudeSubagents = false
    public var antigravitySubagents = false
    public var hiddenCleanupScan = false
    public var codexAccountScope = false
    public var githubCi = false
    public var antigravitySessions = false
    public var dshSessions = false
    public var codexDesktop = false
    public var systemd = false
    public var linuxUpdateJob = false
    public var pruneBackups = false
    public var antigravityNotifications = false
    public var dshNotifications = false
    public var grokbotSessions = false
    public var grokbotNotifications = false

    enum CodingKeys: String, CodingKey {
        case threadSubagents = "thread_subagents"
        case claudeSubagents = "claude_subagents"
        case antigravitySubagents = "antigravity_subagents"
        case hiddenCleanupScan = "hidden_cleanup_scan"
        case codexAccountScope = "codex_account_scope"
        case githubCi = "github_ci"
        case antigravitySessions = "antigravity_sessions"
        case dshSessions = "dsh_sessions"
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

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        threadSubagents = try c.decodeIfPresent(Bool.self, forKey: .threadSubagents) ?? false
        claudeSubagents = try c.decodeIfPresent(Bool.self, forKey: .claudeSubagents) ?? false
        antigravitySubagents = try c.decodeIfPresent(Bool.self, forKey: .antigravitySubagents) ?? false
        hiddenCleanupScan = try c.decodeIfPresent(Bool.self, forKey: .hiddenCleanupScan) ?? false
        codexAccountScope = try c.decodeIfPresent(Bool.self, forKey: .codexAccountScope) ?? false
        githubCi = try c.decodeIfPresent(Bool.self, forKey: .githubCi) ?? false
        antigravitySessions = try c.decodeIfPresent(Bool.self, forKey: .antigravitySessions) ?? false
        dshSessions = try c.decodeIfPresent(Bool.self, forKey: .dshSessions) ?? false
        codexDesktop = try c.decodeIfPresent(Bool.self, forKey: .codexDesktop) ?? false
        systemd = try c.decodeIfPresent(Bool.self, forKey: .systemd) ?? false
        linuxUpdateJob = try c.decodeIfPresent(Bool.self, forKey: .linuxUpdateJob) ?? false
        pruneBackups = try c.decodeIfPresent(Bool.self, forKey: .pruneBackups) ?? false
        antigravityNotifications = try c.decodeIfPresent(Bool.self, forKey: .antigravityNotifications) ?? false
        dshNotifications = try c.decodeIfPresent(Bool.self, forKey: .dshNotifications) ?? false
        grokbotSessions = try c.decodeIfPresent(Bool.self, forKey: .grokbotSessions) ?? false
        grokbotNotifications = try c.decodeIfPresent(Bool.self, forKey: .grokbotNotifications) ?? false
    }
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
