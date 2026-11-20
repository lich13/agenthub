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

public struct UserContextBlock: Codable, Equatable, Sendable {
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
}

public struct SubagentCollection: Codable, Equatable, Sendable {
    public var agents: [SubagentActivity]
    public var counts: [String: Int]
    public var complete: Bool
    public var warning: String?

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

    public init(id: String, header: String? = nil, question: String, options: [UserInputOption] = []) {
        self.id = id
        self.header = header
        self.question = question
        self.options = options
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
        status = try c.decodeIfPresent(String.self, forKey: .status)
        text = try c.decodeIfPresent(String.self, forKey: .text)
        summary = try c.decodeIfPresent(String.self, forKey: .summary)
        input = try c.decodeIfPresent(String.self, forKey: .input)
        truncated = try c.decodeIfPresent(Bool.self, forKey: .truncated)
        resolved = try c.decodeIfPresent(Bool.self, forKey: .resolved)
        answers = try c.decode([UserInputAnswer].self, forKey: .answers)
        planStatus = try c.decodeIfPresent(String.self, forKey: .planStatus)
        groupId = try c.decodeIfPresent(String.self, forKey: .groupId)
        toolName = try c.decodeIfPresent(String.self, forKey: .toolName)
        callId = try c.decodeIfPresent(String.self, forKey: .callId)
        turnId = try c.decodeIfPresent(String.self, forKey: .turnId)
        itemId = try c.decodeIfPresent(String.self, forKey: .itemId)
        createdAt = try c.decodeIfPresent(String.self, forKey: .createdAt)
        questions = try c.decode([UserInputQuestion].self, forKey: .questions)
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
    public var model: String?
    public var rolloutPath: String?
    public var activeTurnId: String?
    public var activeJobId: String?
    public var pendingElicitation: PendingElicitation?
    public var lastEventKind: String?

    enum CodingKeys: String, CodingKey {
        case storageSize
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
        self.rolloutPath = rolloutPath
        self.storageSize = storageSize
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
        messages = try c.decode([CodexMessage].self, forKey: .messages)
        blocks = try c.decode([MessageBlock].self, forKey: .blocks)
        rawEventCount = try c.decode(Int.self, forKey: .rawEventCount)
        totalBlocks = try c.decodeIfPresent(Int.self, forKey: .totalBlocks)
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
}
