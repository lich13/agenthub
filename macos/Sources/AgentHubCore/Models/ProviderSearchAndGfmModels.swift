import Foundation

// MARK: - Claude, Grok & Standard Provider Session Models

public struct ClaudeSessionSummary: Codable, Equatable, Sendable, Identifiable {
    public var storageSize: SessionStorageSize?
    public var id: String
    public var sessionKey: String
    public var title: String
    public var cwd: String
    public var workspaceRoot: String?
    public var path: String
    public var updatedAt: String?
    public var messageCount: Int
    public var lastMessage: String?
    public var status: String
    public var formatVersion: String?
    public var readWarning: String?
    public var canRename: Bool
    public var renameBlockReason: String?
    public var canDelete: Bool
    public var deleteBlockReason: String?
    public var readError: String?

    enum CodingKeys: String, CodingKey {
        case storageSize, id, sessionKey, title, cwd, workspaceRoot, path
        case updatedAt, messageCount, lastMessage, status, formatVersion
        case readWarning, canRename, renameBlockReason, canDelete, deleteBlockReason, readError
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        storageSize = try c.decodeIfPresent(SessionStorageSize.self, forKey: .storageSize)
        id = try c.decode(String.self, forKey: .id)
        sessionKey = try c.decode(String.self, forKey: .sessionKey)
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        cwd = try c.decodeIfPresent(String.self, forKey: .cwd) ?? ""
        workspaceRoot = try c.decodeIfPresent(String.self, forKey: .workspaceRoot)
        path = try c.decodeIfPresent(String.self, forKey: .path) ?? ""
        updatedAt = try c.decodeIfPresent(String.self, forKey: .updatedAt)
        messageCount = try c.decodeIfPresent(Int.self, forKey: .messageCount) ?? 0
        lastMessage = try c.decodeIfPresent(String.self, forKey: .lastMessage)
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? "recent"
        formatVersion = try c.decodeIfPresent(String.self, forKey: .formatVersion)
        readWarning = try c.decodeIfPresent(String.self, forKey: .readWarning)
        canRename = try c.decodeIfPresent(Bool.self, forKey: .canRename) ?? false
        renameBlockReason = try c.decodeIfPresent(String.self, forKey: .renameBlockReason)
        canDelete = try c.decodeIfPresent(Bool.self, forKey: .canDelete) ?? true
        deleteBlockReason = try c.decodeIfPresent(String.self, forKey: .deleteBlockReason)
        readError = try c.decodeIfPresent(String.self, forKey: .readError)
    }
}

public struct ClaudeHistoryEvent: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var turnId: String?
    public var result: String?
    public var userMessage: UserMessageContent?
    public var timestamp: String?
    public var kind: String
    public var role: String?
    public var text: String?
    public var callId: String?
    public var status: String?
    public var detail: String?
    public var assistantPhase: String?

    public init(
        id: String,
        kind: String,
        role: String? = nil,
        text: String? = nil,
        turnId: String? = nil,
        callId: String? = nil,
        status: String? = nil,
        detail: String? = nil,
        result: String? = nil,
        assistantPhase: String? = nil,
        userMessage: UserMessageContent? = nil
    ) {
        self.id = id
        self.kind = kind
        self.role = role
        self.text = text
        self.turnId = turnId
        self.callId = callId
        self.status = status
        self.detail = detail
        self.result = result
        self.assistantPhase = assistantPhase
        self.userMessage = userMessage
    }
}

public struct ClaudeSessionDetail: Codable, Equatable, Sendable {
    public var agent: SubagentActivity?
    public var subagents: SubagentCollection?
    public var timeline: [TimelineEntry]?
    public var afterCursor: String?
    public var history: HistoryMetadata?
    public var summary: ClaudeSessionSummary
    public var events: [ClaudeHistoryEvent]
    public var totalEvents: Int
    public var hasMore: Bool
    public var beforeCursor: String?

    enum CodingKeys: String, CodingKey {
        case agent, subagents, timeline, afterCursor, history
        case summary, events, totalEvents, hasMore, beforeCursor
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        agent = try c.decodeIfPresent(SubagentActivity.self, forKey: .agent)
        subagents = try c.decodeIfPresent(SubagentCollection.self, forKey: .subagents)
        timeline = try c.decodeIfPresent([TimelineEntry].self, forKey: .timeline)
        afterCursor = try c.decodeIfPresent(String.self, forKey: .afterCursor)
        history = try c.decodeIfPresent(HistoryMetadata.self, forKey: .history)
        summary = try c.decode(ClaudeSessionSummary.self, forKey: .summary)
        let decodedEvents = try c.decodeIfPresent([ClaudeHistoryEvent].self, forKey: .events) ?? []
        events = decodedEvents
        totalEvents = try c.decodeIfPresent(Int.self, forKey: .totalEvents) ?? decodedEvents.count
        hasMore = try c.decodeIfPresent(Bool.self, forKey: .hasMore) ?? false
        beforeCursor = try c.decodeIfPresent(String.self, forKey: .beforeCursor)
    }
}

public struct ClaudeDeletePreview: Codable, Equatable, Sendable {
    public var sessionKey: String
    public var id: String
    public var title: String
    public var path: String
    public var paths: [String]?
    public var fingerprint: String
    public var fileCount: Int
    public var bytes: Int64
    public var warnings: [String]?

    public init(
        sessionKey: String,
        id: String,
        title: String,
        path: String,
        paths: [String]? = nil,
        fingerprint: String,
        fileCount: Int,
        bytes: Int64,
        warnings: [String]? = nil
    ) {
        self.sessionKey = sessionKey
        self.id = id
        self.title = title
        self.path = path
        self.paths = paths
        self.fingerprint = fingerprint
        self.fileCount = fileCount
        self.bytes = bytes
        self.warnings = warnings
    }
}

public struct ClaudeDeleteResult: Codable, Equatable, Sendable {
    public var sessionKey: String
    public var deleted: Bool
    public var bytes: Int64
    public var residuals: [String]?
}

public struct ClaudeOwnershipProcess: Codable, Equatable, Sendable {
    public var pid: Int
    public var role: String
    public var registrationVerified: Bool
    public var mismatchedFields: [String]
}

public struct ClaudeOwnershipDiagnostics: Codable, Equatable, Sendable {
    public var ownership: String
    public var reasonCodes: [String]
    public var processes: [ClaudeOwnershipProcess]
}

public struct GrokSessionSummary: Codable, Equatable, Sendable, Identifiable {
    public var storageSize: SessionStorageSize?
    public var handoffPath: String?
    public var id: String
    public var title: String
    public var cwd: String
    public var workspaceRoot: String?
    public var path: String
    public var updatedAt: String?
    public var messageCount: Int
    public var lastMessage: String?
    public var status: String

    enum CodingKeys: String, CodingKey {
        case storageSize, handoffPath
        case id, title, cwd, workspaceRoot, path, updatedAt, messageCount, lastMessage, status
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        storageSize = try c.decodeIfPresent(SessionStorageSize.self, forKey: .storageSize)
        handoffPath = try c.decodeIfPresent(String.self, forKey: .handoffPath)
        id = try c.decode(String.self, forKey: .id)
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        cwd = try c.decodeIfPresent(String.self, forKey: .cwd) ?? ""
        workspaceRoot = try c.decodeIfPresent(String.self, forKey: .workspaceRoot)
        path = try c.decodeIfPresent(String.self, forKey: .path) ?? ""
        updatedAt = try c.decodeIfPresent(String.self, forKey: .updatedAt)
        messageCount = try c.decodeIfPresent(Int.self, forKey: .messageCount) ?? 0
        lastMessage = try c.decodeIfPresent(String.self, forKey: .lastMessage)
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? "recent"
    }
}

public struct GrokHistoryEvent: Codable, Equatable, Sendable {
    public var id: String?
    public var userMessage: UserMessageContent?
    public var callId: String?
    public var status: String?
    public var detail: String?
    public var timestamp: String?
    public var kind: String
    public var text: String?
    public var method: String?

    public init(
        id: String? = nil,
        kind: String,
        text: String? = nil,
        method: String? = nil,
        callId: String? = nil,
        status: String? = nil,
        detail: String? = nil,
        userMessage: UserMessageContent? = nil
    ) {
        self.id = id
        self.kind = kind
        self.text = text
        self.method = method
        self.callId = callId
        self.status = status
        self.detail = detail
        self.userMessage = userMessage
    }
}

public struct GrokSessionDetail: Codable, Equatable, Sendable {
    public var timeline: [TimelineEntry]?
    public var afterCursor: String?
    public var history: HistoryMetadata?
    public var totalEvents: Int?
    public var hasMore: Bool?
    public var beforeCursor: String?
    public var summary: GrokSessionSummary
    public var events: [GrokHistoryEvent]

    enum CodingKeys: String, CodingKey {
        case timeline, afterCursor, history, totalEvents, hasMore, beforeCursor, summary, events
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        timeline = try c.decodeIfPresent([TimelineEntry].self, forKey: .timeline)
        afterCursor = try c.decodeIfPresent(String.self, forKey: .afterCursor)
        history = try c.decodeIfPresent(HistoryMetadata.self, forKey: .history)
        totalEvents = try c.decodeIfPresent(Int.self, forKey: .totalEvents)
        hasMore = try c.decodeIfPresent(Bool.self, forKey: .hasMore)
        beforeCursor = try c.decodeIfPresent(String.self, forKey: .beforeCursor)
        summary = try c.decode(GrokSessionSummary.self, forKey: .summary)
        events = try c.decodeIfPresent([GrokHistoryEvent].self, forKey: .events) ?? []
    }
}

public struct GrokDeletePreview: Codable, Equatable, Sendable {
    public var id: String
    public var title: String
    public var path: String
    public var fingerprint: String
    public var fileCount: Int
    public var bytes: Int64

    public init(
        id: String,
        title: String,
        path: String,
        fingerprint: String,
        fileCount: Int,
        bytes: Int64
    ) {
        self.id = id
        self.title = title
        self.path = path
        self.fingerprint = fingerprint
        self.fileCount = fileCount
        self.bytes = bytes
    }
}

public struct GrokDeleteResult: Codable, Equatable, Sendable {
    public var id: String
    public var deleted: Bool
    public var bytes: Int64
    public var residuals: [String]?
}

public struct ProviderSessionSummary: Codable, Equatable, Sendable, Identifiable {
    public var storageSize: SessionStorageSize?
    public var id: String
    public var sessionKey: String
    public var title: String
    public var cwd: String
    public var workspaceRoot: String?
    public var path: String
    public var archived: Bool
    public var updatedAt: String?
    public var messageCount: Int
    public var lastMessage: String?
    public var status: String
    public var formatVersion: String?
    public var canRename: Bool
    public var renameBlockReason: String?
    public var canArchive: Bool
    public var archiveBlockReason: String?
    public var canDelete: Bool
    public var deleteBlockReason: String?
    public var readError: String?
    public var readWarning: String?

    enum CodingKeys: String, CodingKey {
        case storageSize, id, sessionKey, title, cwd, workspaceRoot, path
        case archived, updatedAt, messageCount, lastMessage, status, formatVersion
        case canRename, renameBlockReason, canArchive, archiveBlockReason
        case canDelete, deleteBlockReason, readError, readWarning
    }

    public init(
        storageSize: SessionStorageSize? = nil,
        id: String,
        sessionKey: String,
        title: String,
        cwd: String = "",
        workspaceRoot: String? = nil,
        path: String = "",
        archived: Bool = false,
        updatedAt: String? = nil,
        messageCount: Int = 0,
        lastMessage: String? = nil,
        status: String = "idle",
        formatVersion: String? = nil,
        canRename: Bool = false,
        renameBlockReason: String? = nil,
        canArchive: Bool = false,
        archiveBlockReason: String? = nil,
        canDelete: Bool = false,
        deleteBlockReason: String? = nil,
        readError: String? = nil,
        readWarning: String? = nil
    ) {
        self.storageSize = storageSize
        self.id = id
        self.sessionKey = sessionKey
        self.title = title
        self.cwd = cwd
        self.workspaceRoot = workspaceRoot
        self.path = path
        self.archived = archived
        self.updatedAt = updatedAt
        self.messageCount = messageCount
        self.lastMessage = lastMessage
        self.status = status
        self.formatVersion = formatVersion
        self.canRename = canRename
        self.renameBlockReason = renameBlockReason
        self.canArchive = canArchive
        self.archiveBlockReason = archiveBlockReason
        self.canDelete = canDelete
        self.deleteBlockReason = deleteBlockReason
        self.readError = readError
        self.readWarning = readWarning
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        storageSize = try c.decodeIfPresent(SessionStorageSize.self, forKey: .storageSize)
        id = try c.decode(String.self, forKey: .id)
        sessionKey = try c.decode(String.self, forKey: .sessionKey)
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        cwd = try c.decodeIfPresent(String.self, forKey: .cwd) ?? ""
        workspaceRoot = try c.decodeIfPresent(String.self, forKey: .workspaceRoot)
        path = try c.decodeIfPresent(String.self, forKey: .path) ?? ""
        archived = try c.decodeIfPresent(Bool.self, forKey: .archived) ?? false
        updatedAt = try c.decodeIfPresent(String.self, forKey: .updatedAt)
        messageCount = try c.decodeIfPresent(Int.self, forKey: .messageCount) ?? 0
        lastMessage = try c.decodeIfPresent(String.self, forKey: .lastMessage)
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? "idle"
        formatVersion = try c.decodeIfPresent(String.self, forKey: .formatVersion)
        canRename = try c.decodeIfPresent(Bool.self, forKey: .canRename) ?? false
        renameBlockReason = try c.decodeIfPresent(String.self, forKey: .renameBlockReason)
        canArchive = try c.decodeIfPresent(Bool.self, forKey: .canArchive) ?? false
        archiveBlockReason = try c.decodeIfPresent(String.self, forKey: .archiveBlockReason)
        canDelete = try c.decodeIfPresent(Bool.self, forKey: .canDelete) ?? false
        deleteBlockReason = try c.decodeIfPresent(String.self, forKey: .deleteBlockReason)
        readError = try c.decodeIfPresent(String.self, forKey: .readError)
        readWarning = try c.decodeIfPresent(String.self, forKey: .readWarning)
    }
}

public struct ProviderDetailRequest: Codable, Equatable, Sendable {
    public var sessionKey: String
    public var agentId: String?
    public var limit: Int?
    public var before: String?

    public init(
        sessionKey: String,
        agentId: String? = nil,
        limit: Int? = nil,
        before: String? = nil
    ) {
        self.sessionKey = sessionKey
        self.agentId = agentId
        self.limit = limit
        self.before = before
    }
}

public struct ProviderHistoryEvent: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var timestamp: String?
    public var kind: String
    public var role: String?
    public var text: String?
    public var callId: String?
    public var turnId: String?
    public var status: String?
    public var detail: String?
    public var result: String?
    public var userMessage: UserMessageContent?
    public var subagent: SubagentActivity?

    public init(
        id: String,
        kind: String,
        role: String? = nil,
        text: String? = nil,
        callId: String? = nil,
        turnId: String? = nil,
        status: String? = nil,
        detail: String? = nil,
        result: String? = nil,
        userMessage: UserMessageContent? = nil,
        subagent: SubagentActivity? = nil
    ) {
        self.id = id
        self.kind = kind
        self.role = role
        self.text = text
        self.callId = callId
        self.turnId = turnId
        self.status = status
        self.detail = detail
        self.result = result
        self.userMessage = userMessage
        self.subagent = subagent
    }
}

public struct ProviderSessionDetail: Codable, Equatable, Sendable {
    public var timeline: [TimelineEntry]?
    public var afterCursor: String?
    public var history: HistoryMetadata?
    public var agent: SubagentActivity?
    public var subagents: SubagentCollection?
    public var subagentUpdates: [String: SubagentActivity]?
    public var summary: ProviderSessionSummary
    public var events: [ProviderHistoryEvent]
    public var totalEvents: Int
    public var hasMore: Bool
    public var beforeCursor: String?

    enum CodingKeys: String, CodingKey {
        case timeline, afterCursor, history, agent, subagents, subagentUpdates
        case summary, events, totalEvents, hasMore, beforeCursor
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        timeline = try c.decodeIfPresent([TimelineEntry].self, forKey: .timeline)
        afterCursor = try c.decodeIfPresent(String.self, forKey: .afterCursor)
        history = try c.decodeIfPresent(HistoryMetadata.self, forKey: .history)
        agent = try c.decodeIfPresent(SubagentActivity.self, forKey: .agent)
        subagents = try c.decodeIfPresent(SubagentCollection.self, forKey: .subagents)
        subagentUpdates = try c.decodeIfPresent([String: SubagentActivity].self, forKey: .subagentUpdates)
        summary = try c.decode(ProviderSessionSummary.self, forKey: .summary)
        let decodedEvents = try c.decodeIfPresent([ProviderHistoryEvent].self, forKey: .events) ?? []
        events = decodedEvents
        totalEvents = try c.decodeIfPresent(Int.self, forKey: .totalEvents) ?? decodedEvents.count
        hasMore = try c.decodeIfPresent(Bool.self, forKey: .hasMore) ?? false
        beforeCursor = try c.decodeIfPresent(String.self, forKey: .beforeCursor)
    }
}

// MARK: - Session Batch, Attachment, Search & Plan Export DTOs

public enum SessionProvider: String, Codable, CaseIterable, Sendable, Identifiable {
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

    public static func from(workspace: WorkspaceId) -> SessionProvider? {
        switch workspace {
        case .codex: .codex
        case .claude: .claudeCode
        case .grok: .grok
        case .grokbot: .grokbot
        case .antigravity: .antigravity
        case .dsh: .dsh
        case .workspaces, .probe, .ops: nil
        }
    }

    public var workspaceId: WorkspaceId {
        switch self {
        case .codex: .codex
        case .claudeCode: .claude
        case .grok: .grok
        case .grokbot: .grokbot
        case .antigravity: .antigravity
        case .dsh: .dsh
        }
    }
}

public enum SessionOperation: String, Codable, Sendable {
    case archive
    case restore
    case delete

    public var label: String {
        switch self {
        case .archive: "归档"
        case .restore: "恢复"
        case .delete: "删除"
        }
    }
}

public struct SessionBatchRequest: Codable, Equatable, Sendable {
    public var provider: SessionProvider
    public var operation: SessionOperation
    public var sessionKeys: [String]
    public var includeDescendants: Bool?

    public init(
        provider: SessionProvider,
        operation: SessionOperation,
        sessionKeys: [String],
        includeDescendants: Bool? = nil
    ) {
        self.provider = provider
        self.operation = operation
        self.sessionKeys = sessionKeys
        self.includeDescendants = includeDescendants
    }
}

public struct SessionBatchItem: Codable, Equatable, Sendable {
    public var sessionKey: String
    public var id: String
    public var title: String
    public var paths: [String]
    public var bytes: Int64
    public var allowed: Bool
    public var reason: String?
    public var fingerprint: String?
    public var descendants: [String]?
    public var warnings: [String]?

    public init(
        sessionKey: String,
        id: String,
        title: String,
        paths: [String],
        bytes: Int64,
        allowed: Bool,
        reason: String? = nil,
        fingerprint: String? = nil,
        descendants: [String]? = nil,
        warnings: [String]? = nil
    ) {
        self.sessionKey = sessionKey
        self.id = id
        self.title = title
        self.paths = paths
        self.bytes = bytes
        self.allowed = allowed
        self.reason = reason
        self.fingerprint = fingerprint
        self.descendants = descendants
        self.warnings = warnings
    }
}

public struct SessionBatchPreview: Codable, Equatable, Sendable {
    public var provider: SessionProvider
    public var operation: SessionOperation
    public var items: [SessionBatchItem]

    public init(provider: SessionProvider, operation: SessionOperation, items: [SessionBatchItem]) {
        self.provider = provider
        self.operation = operation
        self.items = items
    }

    public var allowedCount: Int {
        items.filter(\.allowed).count
    }

    public var blockedCount: Int {
        items.count - allowedCount
    }

    public var descendantCount: Int {
        items.reduce(0) { $0 + ($1.descendants?.count ?? 0) }
    }

    public var fileCount: Int {
        items.reduce(0) { $0 + $1.paths.count }
    }

    public var totalBytes: Int64 {
        items.reduce(0) { $0 + $1.bytes }
    }
}

public struct SessionBatchExecuteItem: Codable, Equatable, Sendable {
    public var sessionKey: String
    public var fingerprint: String?
    public var descendants: [String]?

    public init(sessionKey: String, fingerprint: String? = nil, descendants: [String]? = nil) {
        self.sessionKey = sessionKey
        self.fingerprint = fingerprint
        self.descendants = descendants
    }
}

public struct SessionBatchExecuteRequest: Codable, Equatable, Sendable {
    public var provider: SessionProvider
    public var operation: SessionOperation
    public var items: [SessionBatchExecuteItem]
    public var confirmed: Bool
    public var includeDescendants: Bool?

    public init(
        provider: SessionProvider,
        operation: SessionOperation,
        items: [SessionBatchExecuteItem],
        confirmed: Bool = true,
        includeDescendants: Bool? = nil
    ) {
        self.provider = provider
        self.operation = operation
        self.items = items
        self.confirmed = confirmed
        self.includeDescendants = includeDescendants
    }
}

public struct SessionBatchItemResult: Codable, Equatable, Sendable {
    public var sessionKey: String
    public var status: String // "succeeded" | "blocked" | "failed"
    public var message: String?
    public var deletedIds: [String]?
    public var residuals: [String]?

    public init(
        sessionKey: String,
        status: String,
        message: String? = nil,
        deletedIds: [String]? = nil,
        residuals: [String]? = nil
    ) {
        self.sessionKey = sessionKey
        self.status = status
        self.message = message
        self.deletedIds = deletedIds
        self.residuals = residuals
    }
}

public struct SessionBatchResult: Codable, Equatable, Sendable {
    public var items: [SessionBatchItemResult]

    public init(items: [SessionBatchItemResult] = []) {
        self.items = items
    }
}

public struct SessionAttachmentRequest: Codable, Equatable, Sendable {
    public var rootThreadId: String?
    public var provider: SessionProvider
    public var sessionKey: String
    public var messageId: String
    public var attachmentId: String

    public init(
        provider: SessionProvider,
        sessionKey: String,
        messageId: String,
        attachmentId: String,
        rootThreadId: String? = nil
    ) {
        self.provider = provider
        self.sessionKey = sessionKey
        self.messageId = messageId
        self.attachmentId = attachmentId
        self.rootThreadId = rootThreadId
    }
}

public struct SessionAttachmentResponse: Codable, Equatable, Sendable {
    public var mimeType: String
    public var base64: String
}

public enum SearchScope: String, Codable, Sendable {
    case thread
    case provider
}

public struct SessionSearchRequest: Codable, Equatable, Sendable {
    public var provider: SessionProvider
    public var scope: SearchScope
    public var sessionKey: String?
    public var query: String
    public var cursor: String?
    public var limit: Int?

    public init(
        provider: SessionProvider,
        scope: SearchScope,
        sessionKey: String? = nil,
        query: String,
        cursor: String? = nil,
        limit: Int? = 50
    ) {
        self.provider = provider
        self.scope = scope
        self.sessionKey = sessionKey
        self.query = query
        self.cursor = cursor
        self.limit = limit
    }
}

public struct SessionSearchResult: Codable, Equatable, Sendable, Identifiable {
    public var resultId: String
    public var sessionKey: String
    public var nativeId: String
    public var title: String
    public var cwd: String?
    public var matchKind: String
    public var positionKey: String
    public var snippet: String
    public var timestamp: String?

    public var id: String { resultId }
}

public struct SessionSearchResponse: Codable, Equatable, Sendable {
    public var provider: SessionProvider
    public var scope: SearchScope
    public var query: String
    public var results: [SessionSearchResult]
    public var nextCursor: String?
    public var truncated: Bool
    public var warnings: [String]?
}

public struct PlanSaveRequest: Codable, Equatable, Sendable {
    public var filename: String
    public var markdown: String

    public init(filename: String, markdown: String) {
        self.filename = filename
        self.markdown = markdown
    }
}

public struct PlanSaveResult: Codable, Equatable, Sendable {
    public var filename: String
}

// MARK: - Rust pulldown-cmark GFM AST Models

public struct GfmDocument: Codable, Equatable, Sendable {
    public var blocks: [GfmBlock]

    public init(blocks: [GfmBlock] = []) {
        self.blocks = blocks
    }
}

public enum GfmAlignment: String, Codable, Equatable, Sendable {
    case none
    case left
    case center
    case right
}

public struct GfmListItem: Codable, Equatable, Sendable {
    public var checked: Bool?
    public var blocks: [GfmBlock]
}

public enum GfmBlock: Codable, Equatable, Sendable {
    case paragraph(inlines: [GfmInline])
    case heading(level: UInt8, inlines: [GfmInline])
    case codeBlock(language: String?, code: String)
    case blockQuote(blocks: [GfmBlock])
    case list(ordered: Bool, start: UInt64?, items: [GfmListItem])
    case table(alignments: [GfmAlignment], headers: [[GfmInline]], rows: [[[GfmInline]]])
    case thematicBreak

    private enum CodingKeys: String, CodingKey {
        case type, inlines, level, language, code, blocks, ordered, start, items, alignments, headers, rows
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "paragraph":
            self = .paragraph(inlines: try c.decode([GfmInline].self, forKey: .inlines))
        case "heading":
            self = .heading(
                level: try c.decode(UInt8.self, forKey: .level),
                inlines: try c.decode([GfmInline].self, forKey: .inlines)
            )
        case "codeBlock":
            self = .codeBlock(
                language: try c.decodeIfPresent(String.self, forKey: .language),
                code: try c.decode(String.self, forKey: .code)
            )
        case "blockQuote":
            self = .blockQuote(blocks: try c.decode([GfmBlock].self, forKey: .blocks))
        case "list":
            self = .list(
                ordered: try c.decode(Bool.self, forKey: .ordered),
                start: try c.decodeIfPresent(UInt64.self, forKey: .start),
                items: try c.decode([GfmListItem].self, forKey: .items)
            )
        case "table":
            self = .table(
                alignments: try c.decode([GfmAlignment].self, forKey: .alignments),
                headers: try c.decode([[GfmInline]].self, forKey: .headers),
                rows: try c.decode([[[GfmInline]]].self, forKey: .rows)
            )
        default:
            self = .thematicBreak
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .paragraph(inlines):
            try c.encode("paragraph", forKey: .type)
            try c.encode(inlines, forKey: .inlines)
        case let .heading(level, inlines):
            try c.encode("heading", forKey: .type)
            try c.encode(level, forKey: .level)
            try c.encode(inlines, forKey: .inlines)
        case let .codeBlock(language, code):
            try c.encode("codeBlock", forKey: .type)
            try c.encodeIfPresent(language, forKey: .language)
            try c.encode(code, forKey: .code)
        case let .blockQuote(blocks):
            try c.encode("blockQuote", forKey: .type)
            try c.encode(blocks, forKey: .blocks)
        case let .list(ordered, start, items):
            try c.encode("list", forKey: .type)
            try c.encode(ordered, forKey: .ordered)
            try c.encodeIfPresent(start, forKey: .start)
            try c.encode(items, forKey: .items)
        case let .table(alignments, headers, rows):
            try c.encode("table", forKey: .type)
            try c.encode(alignments, forKey: .alignments)
            try c.encode(headers, forKey: .headers)
            try c.encode(rows, forKey: .rows)
        case .thematicBreak:
            try c.encode("thematicBreak", forKey: .type)
        }
    }
}

public enum GfmInline: Codable, Equatable, Sendable {
    case text(String)
    case code(String)
    case strong([GfmInline])
    case emphasis([GfmInline])
    case strikethrough([GfmInline])
    case link(url: String, title: String?, children: [GfmInline])
    case image(url: String, alt: String, title: String?)
    case softBreak
    case hardBreak

    private enum CodingKeys: String, CodingKey {
        case type, text, code, children, url, title, alt
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "text":
            self = .text(try c.decode(String.self, forKey: .text))
        case "code":
            self = .code(try c.decode(String.self, forKey: .code))
        case "strong":
            self = .strong(try c.decode([GfmInline].self, forKey: .children))
        case "emphasis":
            self = .emphasis(try c.decode([GfmInline].self, forKey: .children))
        case "strikethrough":
            self = .strikethrough(try c.decode([GfmInline].self, forKey: .children))
        case "link":
            self = .link(
                url: try c.decode(String.self, forKey: .url),
                title: try c.decodeIfPresent(String.self, forKey: .title),
                children: try c.decode([GfmInline].self, forKey: .children)
            )
        case "image":
            self = .image(
                url: try c.decode(String.self, forKey: .url),
                alt: try c.decode(String.self, forKey: .alt),
                title: try c.decodeIfPresent(String.self, forKey: .title)
            )
        case "softBreak":
            self = .softBreak
        default:
            self = .hardBreak
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .text(text):
            try c.encode("text", forKey: .type)
            try c.encode(text, forKey: .text)
        case let .code(code):
            try c.encode("code", forKey: .type)
            try c.encode(code, forKey: .code)
        case let .strong(children):
            try c.encode("strong", forKey: .type)
            try c.encode(children, forKey: .children)
        case let .emphasis(children):
            try c.encode("emphasis", forKey: .type)
            try c.encode(children, forKey: .children)
        case let .strikethrough(children):
            try c.encode("strikethrough", forKey: .type)
            try c.encode(children, forKey: .children)
        case let .link(url, title, children):
            try c.encode("link", forKey: .type)
            try c.encode(url, forKey: .url)
            try c.encodeIfPresent(title, forKey: .title)
            try c.encode(children, forKey: .children)
        case let .image(url, alt, title):
            try c.encode("image", forKey: .type)
            try c.encode(url, forKey: .url)
            try c.encode(alt, forKey: .alt)
            try c.encodeIfPresent(title, forKey: .title)
        case .softBreak:
            try c.encode("softBreak", forKey: .type)
        case .hardBreak:
            try c.encode("hardBreak", forKey: .type)
        }
    }
}
