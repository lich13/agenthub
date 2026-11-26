import Foundation

// MARK: - Shared Timeline, Attachment & Storage Models

public struct TimelineEntry: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var title: String
    public var preview: String?

    public init(id: String, title: String, preview: String? = nil) {
        self.id = id
        self.title = title
        self.preview = preview
    }
}

public struct HistoryChangeEntry: Codable, Equatable, Sendable {
    public var revision: String
    public var changedIds: [String]
    public var reset: Bool?

    enum CodingKeys: String, CodingKey {
        case revision
        case changedIds
        case reset
    }

    public init(revision: String, changedIds: [String] = [], reset: Bool? = nil) {
        self.revision = revision
        self.changedIds = changedIds
        self.reset = reset
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        revision = try c.decode(String.self, forKey: .revision)
        changedIds = try c.decodeIfPresent([String].self, forKey: .changedIds) ?? []
        reset = try c.decodeIfPresent(Bool.self, forKey: .reset)
    }
}

public struct HistoryMetadata: Codable, Equatable, Sendable {
    public var epoch: String
    public var revision: String
    public var changedIds: [String]?
    public var reset: Bool?
    public var changes: [HistoryChangeEntry]?

    public init(
        epoch: String,
        revision: String,
        changedIds: [String]? = nil,
        reset: Bool? = nil,
        changes: [HistoryChangeEntry]? = nil
    ) {
        self.epoch = epoch
        self.revision = revision
        self.changedIds = changedIds
        self.reset = reset
        self.changes = changes
    }
}

public struct UserAttachment: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var kind: String // "image" | "file"
    public var path: String?
    public var reason: String?

    public init(id: String, name: String, kind: String, path: String? = nil, reason: String? = nil) {
        self.id = id
        self.name = name
        self.kind = kind
        self.path = path
        self.reason = reason
    }
}

public struct UserContextBlock: Codable, Hashable, Sendable {
    public var kind: String // "instructions" | "environment" | "runtime" | "files"
    public var text: String

    public init(kind: String, text: String) {
        self.kind = kind
        self.text = text
    }
}

public struct UserMessageContent: Codable, Equatable, Sendable {
    public var id: String
    public var text: String
    public var attachments: [UserAttachment]
    public var contextBlocks: [UserContextBlock]?

    enum CodingKeys: String, CodingKey {
        case id
        case text
        case attachments
        case contextBlocks
    }

    public init(
        id: String,
        text: String,
        attachments: [UserAttachment] = [],
        contextBlocks: [UserContextBlock]? = nil
    ) {
        self.id = id
        self.text = text
        self.attachments = attachments
        self.contextBlocks = contextBlocks
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
        attachments = try c.decodeIfPresent([UserAttachment].self, forKey: .attachments) ?? []
        contextBlocks = try c.decodeIfPresent([UserContextBlock].self, forKey: .contextBlocks)
    }
}

public struct SessionStorageSize: Codable, Equatable, Sendable {
    public var bytes: Int64?
    public var scope: String // "file" | "directory"
    public var status: String // "complete" | "partial" | "pending" | "unavailable"

    public init(bytes: Int64?, scope: String, status: String) {
        self.bytes = bytes
        self.scope = scope
        self.status = status
    }
}

// MARK: - Subagents

public struct SubagentActivity: Codable, Equatable, Sendable, Identifiable {
    public var eventKind: String?
    public var eventId: String?
    public var agentId: String?
    public var name: String
    public var role: String?
    public var status: String // "creating" | "running" | "completed" | "failed" | "interrupted" | "unknown"
    public var available: Bool
    public var unavailableReason: String?
    public var delegation: String?

    public var id: String {
        agentId ?? eventId ?? name
    }

    public init(
        eventKind: String? = nil,
        eventId: String? = nil,
        agentId: String? = nil,
        name: String,
        role: String? = nil,
        status: String = "unknown",
        available: Bool = true,
        unavailableReason: String? = nil,
        delegation: String? = nil
    ) {
        self.eventKind = eventKind
        self.eventId = eventId
        self.agentId = agentId
        self.name = name
        self.role = role
        self.status = status
        self.available = available
        self.unavailableReason = unavailableReason
        self.delegation = delegation
    }

    enum CodingKeys: String, CodingKey {
        case eventKind
        case eventId
        case agentId
        case name
        case role
        case status
        case available
        case unavailableReason
        case delegation
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        eventKind = try c.decodeIfPresent(String.self, forKey: .eventKind)
        eventId = try c.decodeIfPresent(String.self, forKey: .eventId)
        agentId = try c.decodeIfPresent(String.self, forKey: .agentId)
        name = try c.decode(String.self, forKey: .name)
        role = try c.decodeIfPresent(String.self, forKey: .role)
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? "unknown"
        available = try c.decodeIfPresent(Bool.self, forKey: .available) ?? true
        unavailableReason = try c.decodeIfPresent(String.self, forKey: .unavailableReason)
        delegation = try c.decodeIfPresent(String.self, forKey: .delegation)
    }
}

public struct SubagentCollection: Codable, Equatable, Sendable {
    public var agents: [SubagentActivity]
    public var counts: [String: Int]
    public var complete: Bool
    public var warning: String?

    enum CodingKeys: String, CodingKey {
        case agents
        case counts
        case complete
        case warning
    }

    public init(
        agents: [SubagentActivity] = [],
        counts: [String: Int] = [:],
        complete: Bool = true,
        warning: String? = nil
    ) {
        self.agents = agents
        self.counts = counts
        self.complete = complete
        self.warning = warning
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        agents = try c.decodeIfPresent([SubagentActivity].self, forKey: .agents) ?? []
        counts = try c.decodeIfPresent([String: Int].self, forKey: .counts) ?? [:]
        complete = try c.decodeIfPresent(Bool.self, forKey: .complete) ?? true
        warning = try c.decodeIfPresent(String.self, forKey: .warning)
    }
}

public struct SubagentDetailRequest: Codable, Equatable, Sendable {
    public var rootThreadId: String
    public var agentId: String
    public var limit: Int?
    public var before: String?

    public init(rootThreadId: String, agentId: String, limit: Int? = 120, before: String? = nil) {
        self.rootThreadId = rootThreadId
        self.agentId = agentId
        self.limit = limit
        self.before = before
    }
}

public struct SubagentDetailResponse: Codable, Equatable, Sendable {
    public var rootThreadId: String
    public var parentThreadId: String
    public var agent: SubagentActivity
    public var detail: ThreadDetail
}

// MARK: - Codex Thread & MessageBlock Models

public struct UserInputOption: Codable, Equatable, Sendable {
    public var label: String
    public var description: String?

    public init(label: String, description: String? = nil) {
        self.label = label
        self.description = description
    }
}

public struct UserInputQuestion: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var header: String?
    public var question: String
    public var options: [UserInputOption]

    enum CodingKeys: String, CodingKey {
        case id
        case header
        case question
        case options
    }

    public init(id: String, header: String? = nil, question: String, options: [UserInputOption] = []) {
        self.id = id
        self.header = header
        self.question = question
        self.options = options
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        header = try c.decodeIfPresent(String.self, forKey: .header)
        question = try c.decodeIfPresent(String.self, forKey: .question) ?? ""
        options = try c.decodeIfPresent([UserInputOption].self, forKey: .options) ?? []
    }
}

public struct UserInputAnswer: Codable, Equatable, Sendable {
    public var questionId: String
    public var answers: [String]
    public var note: String?

    enum CodingKeys: String, CodingKey {
        case questionId = "question_id"
        case answers
        case note
    }

    public init(questionId: String, answers: [String], note: String? = nil) {
        self.questionId = questionId
        self.answers = answers
        self.note = note
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        questionId = try c.decode(String.self, forKey: .questionId)
        answers = try c.decodeIfPresent([String].self, forKey: .answers) ?? []
        note = try c.decodeIfPresent(String.self, forKey: .note)
    }
}

public struct PendingElicitation: Codable, Equatable, Sendable {
    public var turnId: String?
    public var itemId: String?
    public var questions: [UserInputQuestion]

    enum CodingKeys: String, CodingKey {
        case turnId = "turn_id"
        case itemId = "item_id"
        case questions
    }

    public init(turnId: String? = nil, itemId: String? = nil, questions: [UserInputQuestion] = []) {
        self.turnId = turnId
        self.itemId = itemId
        self.questions = questions
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        turnId = try c.decodeIfPresent(String.self, forKey: .turnId)
        itemId = try c.decodeIfPresent(String.self, forKey: .itemId)
        questions = try c.decodeIfPresent([UserInputQuestion].self, forKey: .questions) ?? []
    }
}

public struct CodexMessage: Codable, Equatable, Sendable {
    public var role: String
    public var kind: String
    public var text: String
    public var createdAt: String?

    enum CodingKeys: String, CodingKey {
        case role
        case kind
        case text
        case createdAt = "created_at"
    }
}

public struct MessageBlock: Codable, Equatable, Sendable, Identifiable {
    public var subagent: SubagentActivity?
    public var userMessage: UserMessageContent?
    public var id: String
    public var role: String
    public var kind: String
    public var displayKind: String?
    public var phase: String?
    public var status: String?
    public var text: String?
    public var summary: String?
    public var input: String?
    public var truncated: Bool?
    public var resolved: Bool?
    public var answers: [UserInputAnswer]
    public var planStatus: String?
    public var groupId: String?
    public var toolName: String?
    public var callId: String?
    public var turnId: String?
    public var itemId: String?
    public var createdAt: String?
    public var questions: [UserInputQuestion]
    public var payload: JSONValue?
    public var collapsedIds: [String]?

    enum CodingKeys: String, CodingKey {
        case subagent
        case userMessage = "user_message"
        case id, role, kind
        case displayKind = "display_kind"
        case phase
        case status, text, summary, input, truncated, resolved, answers
        case planStatus = "plan_status"
        case groupId = "group_id"
        case toolName = "tool_name"
        case callId = "call_id"
        case turnId = "turn_id"
        case itemId = "item_id"
        case createdAt = "created_at"
        case questions, payload, collapsedIds
    }

    public init(
        id: String,
        role: String,
        kind: String,
        displayKind: String? = nil,
        phase: String? = nil,
        status: String? = nil,
        text: String? = nil,
        summary: String? = nil,
        input: String? = nil,
        truncated: Bool? = nil,
        resolved: Bool? = nil,
        answers: [UserInputAnswer] = [],
        planStatus: String? = nil,
        toolName: String? = nil,
        callId: String? = nil,
        turnId: String? = nil,
        itemId: String? = nil,
        createdAt: String? = nil,
        questions: [UserInputQuestion] = [],
        userMessage: UserMessageContent? = nil,
        subagent: SubagentActivity? = nil,
        payload: JSONValue? = nil,
        collapsedIds: [String]? = nil
    ) {
        self.id = id
        self.role = role
        self.kind = kind
        self.displayKind = displayKind
        self.phase = phase
        self.status = status
        self.text = text
        self.summary = summary
        self.input = input
        self.truncated = truncated
        self.resolved = resolved
        self.answers = answers
        self.planStatus = planStatus
        self.toolName = toolName
        self.callId = callId
        self.turnId = turnId
        self.itemId = itemId
        self.createdAt = createdAt
        self.questions = questions
        self.userMessage = userMessage
        self.subagent = subagent
        self.payload = payload
        self.collapsedIds = collapsedIds
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        subagent = try c.decodeIfPresent(SubagentActivity.self, forKey: .subagent)
        userMessage = try c.decodeIfPresent(UserMessageContent.self, forKey: .userMessage)
        id = try c.decode(String.self, forKey: .id)
        role = try c.decode(String.self, forKey: .role)
        kind = try c.decode(String.self, forKey: .kind)
        displayKind = try c.decodeIfPresent(String.self, forKey: .displayKind)
        phase = try c.decodeIfPresent(String.self, forKey: .phase)
        status = try c.decodeIfPresent(String.self, forKey: .status)
        text = try c.decodeIfPresent(String.self, forKey: .text)
        summary = try c.decodeIfPresent(String.self, forKey: .summary)
        input = try c.decodeIfPresent(String.self, forKey: .input)
        truncated = try c.decodeIfPresent(Bool.self, forKey: .truncated)
        resolved = try c.decodeIfPresent(Bool.self, forKey: .resolved)
        answers = try c.decodeIfPresent([UserInputAnswer].self, forKey: .answers) ?? []
        planStatus = try c.decodeIfPresent(String.self, forKey: .planStatus)
        groupId = try c.decodeIfPresent(String.self, forKey: .groupId)
        toolName = try c.decodeIfPresent(String.self, forKey: .toolName)
        callId = try c.decodeIfPresent(String.self, forKey: .callId)
        turnId = try c.decodeIfPresent(String.self, forKey: .turnId)
        itemId = try c.decodeIfPresent(String.self, forKey: .itemId)
        createdAt = try c.decodeIfPresent(String.self, forKey: .createdAt)
        questions = try c.decodeIfPresent([UserInputQuestion].self, forKey: .questions) ?? []
        payload = try c.decodeIfPresent(JSONValue.self, forKey: .payload)
        collapsedIds = try c.decodeIfPresent([String].self, forKey: .collapsedIds)
    }
}

public struct ThreadSummary: Codable, Equatable, Sendable, Identifiable {
    public var storageSize: SessionStorageSize?
    public var id: String
    public var title: String
    public var status: String // "Recent" | "Running" | "ReplyNeeded" | "Recoverable" | "Archived"
    public var updatedAt: String?
    public var archivedAt: String?
    public var messageCount: Int
    public var latestMessage: String?
    public var cwd: String?
    public var workspaceRoot: String?
    public var model: String?
    public var rolloutPath: String?
    public var activeTurnId: String?
    public var activeJobId: String?
    public var pendingElicitation: PendingElicitation?
    public var lastEventKind: String?

    enum CodingKeys: String, CodingKey {
        case storageSize
        case workspaceRoot
        case id, title, status, cwd, model
        case updatedAt = "updated_at"
        case archivedAt = "archived_at"
        case messageCount = "message_count"
        case latestMessage = "latest_message"
        case rolloutPath = "rollout_path"
        case activeTurnId = "active_turn_id"
        case activeJobId = "active_job_id"
        case pendingElicitation = "pending_elicitation"
        case lastEventKind = "last_event_kind"
    }

    public init(
        id: String,
        title: String,
        status: String = "Recent",
        updatedAt: String? = nil,
        archivedAt: String? = nil,
        messageCount: Int = 0,
        latestMessage: String? = nil,
        cwd: String? = nil,
        workspaceRoot: String? = nil,
        rolloutPath: String? = nil,
        storageSize: SessionStorageSize? = nil
    ) {
        self.id = id
        self.title = title
        self.status = status
        self.updatedAt = updatedAt
        self.archivedAt = archivedAt
        self.messageCount = messageCount
        self.latestMessage = latestMessage
        self.cwd = cwd
        self.workspaceRoot = workspaceRoot
        self.rolloutPath = rolloutPath
        self.storageSize = storageSize
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        storageSize = try c.decodeIfPresent(SessionStorageSize.self, forKey: .storageSize)
        id = try c.decode(String.self, forKey: .id)
        title = try c.decode(String.self, forKey: .title)
        status = try c.decode(String.self, forKey: .status)
        updatedAt = try c.decodeIfPresent(String.self, forKey: .updatedAt)
        archivedAt = try c.decodeIfPresent(String.self, forKey: .archivedAt)
        messageCount = try c.decode(Int.self, forKey: .messageCount)
        latestMessage = try c.decodeIfPresent(String.self, forKey: .latestMessage)
        cwd = try c.decodeIfPresent(String.self, forKey: .cwd)
        workspaceRoot = try c.decodeIfPresent(String.self, forKey: .workspaceRoot)
        model = try c.decodeIfPresent(String.self, forKey: .model)
        rolloutPath = try c.decodeIfPresent(String.self, forKey: .rolloutPath)
        activeTurnId = try c.decodeIfPresent(String.self, forKey: .activeTurnId)
        activeJobId = try c.decodeIfPresent(String.self, forKey: .activeJobId)
        pendingElicitation = try c.decodeIfPresent(PendingElicitation.self, forKey: .pendingElicitation)
        lastEventKind = try c.decodeIfPresent(String.self, forKey: .lastEventKind)
    }
}

public struct ThreadDetail: Codable, Equatable, Sendable {
    public var timeline: [TimelineEntry]?
    public var afterCursor: String?
    public var history: HistoryMetadata?
    public var subagents: SubagentCollection?
    public var subagentUpdates: [String: SubagentActivity]?
    public var summary: ThreadSummary
    public var messages: [CodexMessage]
    public var blocks: [MessageBlock]
    public var rawEventCount: Int
    public var totalBlocks: Int?
    public var hasMoreBlocks: Bool?
    public var beforeCursor: String?

    enum CodingKeys: String, CodingKey {
        case timeline, history, subagents, summary, messages, blocks
        case afterCursor = "after_cursor"
        case subagentUpdates = "subagent_updates"
        case rawEventCount = "raw_event_count"
        case totalBlocks = "total_blocks"
        case hasMoreBlocks = "has_more_blocks"
        case beforeCursor = "before_cursor"
    }

    public init(
        summary: ThreadSummary,
        blocks: [MessageBlock] = [],
        timeline: [TimelineEntry]? = nil,
        history: HistoryMetadata? = nil,
        subagents: SubagentCollection? = nil,
        subagentUpdates: [String: SubagentActivity]? = nil,
        totalBlocks: Int? = nil,
        hasMoreBlocks: Bool? = nil,
        beforeCursor: String? = nil
    ) {
        self.summary = summary
        self.messages = []
        self.blocks = blocks
        self.timeline = timeline
        self.history = history
        self.subagents = subagents
        self.subagentUpdates = subagentUpdates
        self.rawEventCount = totalBlocks ?? blocks.count
        self.totalBlocks = totalBlocks
        self.hasMoreBlocks = hasMoreBlocks
        self.beforeCursor = beforeCursor
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        timeline = try c.decodeIfPresent([TimelineEntry].self, forKey: .timeline)
        afterCursor = try c.decodeIfPresent(String.self, forKey: .afterCursor)
        history = try c.decodeIfPresent(HistoryMetadata.self, forKey: .history)
        subagents = try c.decodeIfPresent(SubagentCollection.self, forKey: .subagents)
        subagentUpdates = try c.decodeIfPresent([String: SubagentActivity].self, forKey: .subagentUpdates)
        summary = try c.decode(ThreadSummary.self, forKey: .summary)
        messages = try c.decodeIfPresent([CodexMessage].self, forKey: .messages) ?? []
        let decodedBlocks = try c.decode([MessageBlock].self, forKey: .blocks)
        blocks = decodedBlocks
        let decodedTotalBlocks = try c.decodeIfPresent(Int.self, forKey: .totalBlocks)
        totalBlocks = decodedTotalBlocks
        rawEventCount = try c.decodeIfPresent(Int.self, forKey: .rawEventCount) ?? decodedTotalBlocks ?? decodedBlocks.count
        hasMoreBlocks = try c.decodeIfPresent(Bool.self, forKey: .hasMoreBlocks)
        beforeCursor = try c.decodeIfPresent(String.self, forKey: .beforeCursor)
    }
}

public struct ThreadBlockPage: Codable, Equatable, Sendable {
    public var history: HistoryMetadata?
    public var threadId: String
    public var blocks: [MessageBlock]
    public var totalBlocks: Int
    public var hasMoreBlocks: Bool
    public var beforeCursor: String?

    enum CodingKeys: String, CodingKey {
        case history, blocks
        case threadId
        case totalBlocks
        case hasMoreBlocks
        case beforeCursor
    }

    public init(
        threadId: String,
        blocks: [MessageBlock],
        totalBlocks: Int,
        hasMoreBlocks: Bool,
        beforeCursor: String? = nil,
        history: HistoryMetadata? = nil
    ) {
        self.threadId = threadId
        self.blocks = blocks
        self.totalBlocks = totalBlocks
        self.hasMoreBlocks = hasMoreBlocks
        self.beforeCursor = beforeCursor
        self.history = history
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        history = try c.decodeIfPresent(HistoryMetadata.self, forKey: .history)
        threadId = try c.decode(String.self, forKey: .threadId)
        let decodedBlocks = try c.decodeIfPresent([MessageBlock].self, forKey: .blocks) ?? []
        blocks = decodedBlocks
        totalBlocks = try c.decodeIfPresent(Int.self, forKey: .totalBlocks) ?? decodedBlocks.count
        hasMoreBlocks = try c.decodeIfPresent(Bool.self, forKey: .hasMoreBlocks) ?? false
        beforeCursor = try c.decodeIfPresent(String.self, forKey: .beforeCursor)
    }
}
