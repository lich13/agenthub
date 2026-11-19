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
        case available
        case data
    }

    public init(available: Bool, data: T?) {
        self.available = available
        self.data = data
    }

    public init(from decoder: Decoder) throws {
        if let keyed = try? decoder.container(keyedBy: CodingKeys.self),
           keyed.contains(.available),
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
    public var threads: Bool = false
    public var threadSubagents: Bool?
    public var claudeSubagents: Bool?
    public var codexDesktop: Bool?
    public var jobs: Bool = false
    public var probe: Bool = false
    public var settings: Bool = false
    public var jobHistory: Bool = false
    public var appUpdater: Bool = false
    public var threadCleanup: Bool = false
    public var hiddenCleanupScan: Bool?
    public var codexAccountScope: Bool?
    public var threadArchiveActions: Bool = false
    public var systemd: Bool = false
    public var linuxUpdateJob: Bool = false
    public var pruneBackups: Bool = false
    public var antigravitySessions: Bool?
    public var antigravityNotifications: Bool?
    public var dshSessions: Bool?
    public var dshNotifications: Bool?
    public var grokbotSessions: Bool?
    public var grokbotNotifications: Bool?
    public var githubCi: Bool?

    enum CodingKeys: String, CodingKey {
        case threads
        case threadSubagents = "thread_subagents"
        case claudeSubagents = "claude_subagents"
        case codexDesktop = "codex_desktop"
        case jobs, probe, settings
        case jobHistory = "job_history"
        case appUpdater = "app_updater"
        case threadCleanup = "thread_cleanup"
        case hiddenCleanupScan = "hidden_cleanup_scan"
        case codexAccountScope = "codex_account_scope"
        case threadArchiveActions = "thread_archive_actions"
        case systemd
        case linuxUpdateJob = "linux_update_job"
        case pruneBackups = "prune_backups"
        case antigravitySessions = "antigravity_sessions"
        case antigravityNotifications = "antigravity_notifications"
        case dshSessions = "dsh_sessions"
        case dshNotifications = "dsh_notifications"
        case grokbotSessions = "grokbot_sessions"
        case grokbotNotifications = "grokbot_notifications"
        case githubCi = "github_ci"
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

    public init(apiVersion: Int = 2, hostSurface: String = "desktop_embedded_tauri", capabilities: SystemCapabilitiesCore = SystemCapabilitiesCore()) {
        self.apiVersion = apiVersion
        self.hostSurface = hostSurface
        self.capabilities = capabilities
    }
}

public struct RuntimeCapabilityMatrix: Equatable, Sendable {
    public var hostSurface: String
    public var codexStatePaths: Bool
    public var threadSubagents: Bool
    public var claudeSubagents: Bool
    public var hiddenCleanupScan: Bool
    public var codexAccountScope: Bool
    public var antigravityNotifications: Bool
    public var dshNotifications: Bool
    public var antigravitySessions: Bool
    public var dshSessions: Bool
    public var grokbotSessions: Bool
    public var githubCi: Bool
    public var grokbotNotifications: Bool
    public var codexDesktop: Bool
    public var updatePrune: Bool
    public var threadCleanup: Bool
    public var threadArchiveActions: Bool
    public var updateServiceLabels: Bool

    public static let bootstrap = RuntimeCapabilityMatrix(
        hostSurface: "desktop_embedded_tauri",
        codexStatePaths: false,
        threadSubagents: false,
        claudeSubagents: false,
        hiddenCleanupScan: false,
        codexAccountScope: false,
        antigravityNotifications: false,
        dshNotifications: false,
        antigravitySessions: true,
        dshSessions: true,
        grokbotSessions: true,
        githubCi: false,
        grokbotNotifications: false,
        codexDesktop: false,
        updatePrune: false,
        threadCleanup: false,
        threadArchiveActions: false,
        updateServiceLabels: false
    )

    public static func from(_ response: SystemCapabilitiesResponse?) -> RuntimeCapabilityMatrix {
        guard let response else { return .bootstrap }
        let c = response.capabilities
        let remote = response.hostSurface == "linux_server_api"
        return RuntimeCapabilityMatrix(
            hostSurface: response.hostSurface,
            codexStatePaths: remote && c.systemd,
            threadSubagents: c.threadSubagents == true,
            claudeSubagents: c.claudeSubagents == true,
            hiddenCleanupScan: c.hiddenCleanupScan == true,
            codexAccountScope: c.codexAccountScope == true,
            antigravityNotifications: !remote && c.antigravityNotifications == true,
            dshNotifications: !remote && c.dshNotifications == true,
            antigravitySessions: c.antigravitySessions ?? true,
            dshSessions: c.dshSessions ?? true,
            grokbotSessions: !remote && (c.grokbotSessions == true),
            githubCi: c.githubCi == true,
            grokbotNotifications: !remote && c.grokbotNotifications == true,
            codexDesktop: !remote && c.codexDesktop == true,
            updatePrune: remote && c.pruneBackups,
            threadCleanup: c.threadCleanup,
            threadArchiveActions: c.threadArchiveActions,
            updateServiceLabels: remote && c.linuxUpdateJob
        )
    }

    public var visibleWorkspaces: [WorkspaceId] {
        WorkspaceId.allCases.filter { id in
            switch id {
            case .grokbot: return grokbotSessions
            case .antigravity: return antigravitySessions
            case .dsh: return dshSessions
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
    public var summary: String?
    public var hint: String?
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

public struct UpdateActionViewItem: Equatable, Sendable, Identifiable {
    public var id: String { action.rawValue }
    public let action: UnifiedUpdateActionKind
    public let label: String
    public let tone: String
    public let disabled: Bool

    public static func actions(status: UpdateStatus?, capabilities: RuntimeCapabilityMatrix) -> [UpdateActionViewItem] {
        var out: [UpdateActionViewItem] = [
            UpdateActionViewItem(
                action: .check,
                label: capabilities.updateServiceLabels ? "检查服务更新" : "检查更新",
                tone: "secondary",
                disabled: false
            ),
        ]
        if status?.updateAvailable == true {
            out.append(
                UpdateActionViewItem(
                    action: .install,
                    label: capabilities.updateServiceLabels ? "执行服务更新" : "下载并更新",
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
    public var id: String { "\(scope ?? ""):\(targetId)" }
    public var scope: String?
    public var targetId: String
    public var title: String?
    public var estimatedBytes: Int64
    public var deletable: Bool
    public var skipReason: String?
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
}

public struct UnifiedCleanupPlan: Codable, Equatable, Sendable {
    public var totalCandidates: Int
    public var totalDeletable: Int
    public var totalEstimatedBytes: Int64
    public var scopes: [UnifiedCleanupScopePlan]
}

public struct UnifiedCleanupItemOutcome: Codable, Equatable, Identifiable, Sendable {
    public var id: String { "\(scope):\(targetId)" }
    public var scope: String
    public var targetId: String
    public var status: String
    public var error: String?
    public var freedBytes: Int64?
}

public struct UnifiedCleanupResult: Codable, Equatable, Sendable {
    public var deletedCount: Int
    public var failedCount: Int
    public var skippedCount: Int
    public var freedBytes: Int64
    public var items: [UnifiedCleanupItemOutcome]
}

public struct HiddenScanProgress: Codable, Equatable, Sendable {
    public var scanId: String
    public var status: String
    public var scannedFiles: Int?
    public var nextCursor: String?
}

public struct HiddenThreadDeletePlan: Codable, Equatable, Sendable {
    public var candidateCount: Int
    public var deletableCount: Int
    public var estimatedBytes: Int64
    public var candidates: [UnifiedCleanupCandidate]
    public var scan: HiddenScanProgress?
}

public struct CodexAccountScopeStatus: Codable, Equatable, Sendable {
    public var scopedThreads: Int
    public var checkedFiles: Int?
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
