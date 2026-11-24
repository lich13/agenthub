import AppKit
import Foundation

// MARK: - Local Codex Collaboration Wire DTOs (webui/src/lib/api/desktop.ts)

public enum DesktopMode: String, Codable, Equatable, Sendable {
    case `default`
    case plan
}

public struct DesktopDiagnostic: Codable, Equatable, Sendable {
    public var timestamp: Int64
    public var source: String
    public var method: String
    public var version: Int?
    public var code: Int?
    public var category: String
    public var hint: String
}

public struct DesktopDiagnostics: Codable, Equatable, Sendable {
    public var records: [DesktopDiagnostic]
    public var desktopBuild: String?
    public var bundledCliVersion: String?
}

public struct DesktopAnswer: Codable, Equatable, Sendable {
    public var questionId: String
    public var answers: [String]

    public init(questionId: String, answers: [String]) {
        self.questionId = questionId
        self.answers = answers
    }
}

public enum DesktopAction: Encodable, Equatable, Sendable {
    case send(messageId: String, text: String, references: [String], referenceSpans: [DesktopReferenceSpan]?)
    case steer(messageId: String, text: String, references: [String], referenceSpans: [DesktopReferenceSpan]?, turnId: String)
    case stop(turnId: String)
    case cancelQueued(messageId: String)
    case dismissUnknown(messageId: String)
    case resumeQueue
    case settings(model: String, reasoningEffort: String?, mode: DesktopMode)
    case decide(requestKey: String, allow: Bool, decision: String?)
    case answer(requestKey: String, answers: [DesktopAnswer])
    case elicitation(requestKey: String, accept: Bool, content: [String: JSONValue]?)

    private enum CodingKeys: String, CodingKey {
        case type, messageId, text, references, referenceSpans, turnId
        case model, reasoningEffort, mode, requestKey, allow, decision, answers, accept, content
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .send(messageId, text, references, spans):
            try c.encode("send", forKey: .type)
            try c.encode(messageId, forKey: .messageId)
            try c.encode(text, forKey: .text)
            try c.encode(references, forKey: .references)
            if let spans, !spans.isEmpty { try c.encode(spans, forKey: .referenceSpans) }
        case let .steer(messageId, text, references, spans, turnId):
            try c.encode("steer", forKey: .type)
            try c.encode(messageId, forKey: .messageId)
            try c.encode(text, forKey: .text)
            try c.encode(references, forKey: .references)
            if let spans, !spans.isEmpty { try c.encode(spans, forKey: .referenceSpans) }
            try c.encode(turnId, forKey: .turnId)
        case let .stop(turnId):
            try c.encode("stop", forKey: .type)
            try c.encode(turnId, forKey: .turnId)
        case let .cancelQueued(messageId):
            try c.encode("cancelQueued", forKey: .type)
            try c.encode(messageId, forKey: .messageId)
        case let .dismissUnknown(messageId):
            try c.encode("dismissUnknown", forKey: .type)
            try c.encode(messageId, forKey: .messageId)
        case .resumeQueue:
            try c.encode("resumeQueue", forKey: .type)
        case let .settings(model, effort, mode):
            try c.encode("settings", forKey: .type)
            try c.encode(model, forKey: .model)
            try c.encode(effort, forKey: .reasoningEffort)
            try c.encode(mode, forKey: .mode)
        case let .decide(requestKey, allow, decision):
            try c.encode("decide", forKey: .type)
            try c.encode(requestKey, forKey: .requestKey)
            try c.encode(allow, forKey: .allow)
            try c.encodeIfPresent(decision, forKey: .decision)
        case let .answer(requestKey, answers):
            try c.encode("answer", forKey: .type)
            try c.encode(requestKey, forKey: .requestKey)
            try c.encode(answers, forKey: .answers)
        case let .elicitation(requestKey, accept, content):
            try c.encode("elicitation", forKey: .type)
            try c.encode(requestKey, forKey: .requestKey)
            try c.encode(accept, forKey: .accept)
            try c.encode(content, forKey: .content)
        }
    }
}

public struct DesktopQueuedMessage: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var status: String // "queued" | "dispatching" | "held" | "outcome_unknown"
    public var kind: String // "queue" | "steer"
    public var preview: String
    public var createdAt: Int64

    public init(
        id: String,
        status: String = "queued",
        kind: String = "queue",
        preview: String = "",
        createdAt: Int64 = 0
    ) {
        self.id = id
        self.status = status
        self.kind = kind
        self.preview = preview
        self.createdAt = createdAt
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? "queued"
        kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "queue"
        preview = try c.decodeIfPresent(String.self, forKey: .preview) ?? ""
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
    }
}

public struct DesktopQuestionOption: Codable, Equatable, Sendable {
    public var label: String
    public var description: String

    public init(label: String, description: String = "") {
        self.label = label
        self.description = description
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        label = try c.decode(String.self, forKey: .label)
        description = try c.decodeIfPresent(String.self, forKey: .description) ?? ""
    }
}

public struct DesktopQuestion: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var header: String
    public var question: String
    public var secret: Bool
    public var options: [DesktopQuestionOption]

    public init(
        id: String,
        header: String = "",
        question: String,
        secret: Bool = false,
        options: [DesktopQuestionOption] = []
    ) {
        self.id = id
        self.header = header
        self.question = question
        self.secret = secret
        self.options = options
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        header = try c.decodeIfPresent(String.self, forKey: .header) ?? ""
        question = try c.decodeIfPresent(String.self, forKey: .question) ?? ""
        secret = try c.decodeIfPresent(Bool.self, forKey: .secret) ?? false
        options = try c.decodeIfPresent([DesktopQuestionOption].self, forKey: .options) ?? []
    }
}

public struct DesktopFileReview: Codable, Equatable, Sendable {
    public var path: String
    public var kind: String
    public var diff: String
    public var truncated: Bool

    public init(path: String, kind: String = "update", diff: String = "", truncated: Bool = false) {
        self.path = path
        self.kind = kind
        self.diff = diff
        self.truncated = truncated
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        path = try c.decode(String.self, forKey: .path)
        kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "update"
        diff = try c.decodeIfPresent(String.self, forKey: .diff) ?? ""
        truncated = try c.decodeIfPresent(Bool.self, forKey: .truncated) ?? false
    }
}

public struct DesktopPermissionsReview: Codable, Equatable, Sendable {
    public var network: String?
    public var fileSystem: String?

    public init(network: String? = nil, fileSystem: String? = nil) {
        self.network = network
        self.fileSystem = fileSystem
    }
}

public struct DesktopReview: Codable, Equatable, Sendable {
    public var command: String?
    public var cwd: String?
    public var reason: String?
    public var changes: [DesktopFileReview]
    public var permissions: DesktopPermissionsReview?
    public var extra: String?
    public var truncated: Bool

    public init(
        command: String? = nil,
        cwd: String? = nil,
        reason: String? = nil,
        changes: [DesktopFileReview] = [],
        permissions: DesktopPermissionsReview? = nil,
        extra: String? = nil,
        truncated: Bool = false
    ) {
        self.command = command
        self.cwd = cwd
        self.reason = reason
        self.changes = changes
        self.permissions = permissions
        self.extra = extra
        self.truncated = truncated
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        command = try c.decodeIfPresent(String.self, forKey: .command)
        cwd = try c.decodeIfPresent(String.self, forKey: .cwd)
        reason = try c.decodeIfPresent(String.self, forKey: .reason)
        changes = try c.decodeIfPresent([DesktopFileReview].self, forKey: .changes) ?? []
        permissions = try c.decodeIfPresent(DesktopPermissionsReview.self, forKey: .permissions)
        extra = try c.decodeIfPresent(String.self, forKey: .extra)
        truncated = try c.decodeIfPresent(Bool.self, forKey: .truncated) ?? false
    }
}

public struct DesktopPendingRequest: Codable, Equatable, Sendable, Identifiable {
    public var key: String
    public var kind: String // "command" | "file" | "permissions" | "question" | "elicitation"
    public var title: String
    public var body: String
    public var questions: [DesktopQuestion]
    public var formSchema: JSONValue?
    public var canAccept: Bool
    public var supportedDecisions: [String]?
    public var review: DesktopReview?

    public var id: String { key }

    public init(
        key: String,
        kind: String,
        title: String,
        body: String = "",
        questions: [DesktopQuestion] = [],
        formSchema: JSONValue? = nil,
        canAccept: Bool = false,
        supportedDecisions: [String]? = nil,
        review: DesktopReview? = nil
    ) {
        self.key = key
        self.kind = kind
        self.title = title
        self.body = body
        self.questions = questions
        self.formSchema = formSchema
        self.canAccept = canAccept
        self.supportedDecisions = supportedDecisions
        self.review = review
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        key = try c.decode(String.self, forKey: .key)
        kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "command"
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        body = try c.decodeIfPresent(String.self, forKey: .body) ?? ""
        questions = try c.decodeIfPresent([DesktopQuestion].self, forKey: .questions) ?? []
        formSchema = try c.decodeIfPresent(JSONValue.self, forKey: .formSchema)
        canAccept = try c.decodeIfPresent(Bool.self, forKey: .canAccept) ?? false
        supportedDecisions = try c.decodeIfPresent([String].self, forKey: .supportedDecisions)
        review = try c.decodeIfPresent(DesktopReview.self, forKey: .review)
    }
}

public struct DesktopGoal: Codable, Equatable, Sendable {
    public var objective: String
    public var status: String
    public var truncated: Bool

    public init(objective: String, status: String = "active", truncated: Bool = false) {
        self.objective = objective
        self.status = status
        self.truncated = truncated
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        objective = try c.decodeIfPresent(String.self, forKey: .objective) ?? ""
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? "active"
        truncated = try c.decodeIfPresent(Bool.self, forKey: .truncated) ?? false
    }
}

public struct DesktopView: Codable, Equatable, Sendable {
    public var connectionPhase: String?
    public var desktopBuild: String?
    public var goal: DesktopGoal?
    public var sessionId: String
    public var threadId: String
    public var revision: UInt64
    public var connected: Bool
    public var writable: Bool
    public var runtime: String
    public var activeTurnId: String?
    public var model: String?
    public var reasoningEffort: String?
    public var mode: DesktopMode?
    public var requests: [DesktopPendingRequest]
    public var queue: [DesktopQueuedMessage]
    public var error: String?

    public init(
        connectionPhase: String? = nil,
        desktopBuild: String? = nil,
        goal: DesktopGoal? = nil,
        sessionId: String,
        threadId: String,
        revision: UInt64 = 0,
        connected: Bool = false,
        writable: Bool = false,
        runtime: String = "idle",
        activeTurnId: String? = nil,
        model: String? = nil,
        reasoningEffort: String? = nil,
        mode: DesktopMode? = nil,
        requests: [DesktopPendingRequest] = [],
        queue: [DesktopQueuedMessage] = [],
        error: String? = nil
    ) {
        self.connectionPhase = connectionPhase
        self.desktopBuild = desktopBuild
        self.goal = goal
        self.sessionId = sessionId
        self.threadId = threadId
        self.revision = revision
        self.connected = connected
        self.writable = writable
        self.runtime = runtime
        self.activeTurnId = activeTurnId
        self.model = model
        self.reasoningEffort = reasoningEffort
        self.mode = mode
        self.requests = requests
        self.queue = queue
        self.error = error
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        connectionPhase = try c.decodeIfPresent(String.self, forKey: .connectionPhase)
        desktopBuild = try c.decodeIfPresent(String.self, forKey: .desktopBuild)
        goal = try c.decodeIfPresent(DesktopGoal.self, forKey: .goal)
        sessionId = try c.decodeIfPresent(String.self, forKey: .sessionId) ?? ""
        threadId = try c.decodeIfPresent(String.self, forKey: .threadId) ?? ""
        revision = try c.decodeIfPresent(UInt64.self, forKey: .revision) ?? 0
        connected = try c.decodeIfPresent(Bool.self, forKey: .connected) ?? false
        writable = try c.decodeIfPresent(Bool.self, forKey: .writable) ?? false
        runtime = try c.decodeIfPresent(String.self, forKey: .runtime) ?? "idle"
        activeTurnId = try c.decodeIfPresent(String.self, forKey: .activeTurnId)
        model = try c.decodeIfPresent(String.self, forKey: .model)
        reasoningEffort = try c.decodeIfPresent(String.self, forKey: .reasoningEffort)
        mode = try c.decodeIfPresent(DesktopMode.self, forKey: .mode)
        requests = try c.decodeIfPresent([DesktopPendingRequest].self, forKey: .requests) ?? []
        queue = try c.decodeIfPresent([DesktopQueuedMessage].self, forKey: .queue) ?? []
        error = try c.decodeIfPresent(String.self, forKey: .error)
    }
}

public struct DesktopCatalogItem: Codable, Equatable, Sendable, Identifiable {
    public var handle: String
    public var kind: String
    public var name: String
    public var description: String
    public var reasoningEfforts: [String]
    public var defaultReasoningEffort: String?

    public var id: String { handle }

    public var asChosenItem: DesktopChosenItem {
        DesktopChosenItem(handle: handle, name: name, kind: kind, description: description)
    }
}

public struct DesktopCatalog: Codable, Equatable, Sendable {
    public var items: [DesktopCatalogItem]
    public var nextCursor: String?
}

public struct DesktopChoice: Codable, Equatable, Sendable {
    public var item: DesktopChosenItem?
}

public struct DesktopCreatedThread: Codable, Equatable, Sendable {
    public var threadId: String
}

public struct DesktopAck: Codable, Equatable, Sendable {
    public var ok: Bool
}

public struct DesktopAttachmentBounds: Codable, Equatable, Sendable {
    public var x: Double
    public var y: Double
    public var width: Double
    public var height: Double

    public init(x: Double, y: Double, width: Double, height: Double) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }
}

public struct DesktopAttachmentEvent: Codable, Equatable, Sendable {
    public var bindingId: String
    public var items: [DesktopChosenItem]
    public var error: String?

    enum CodingKeys: String, CodingKey {
        case bindingId, items, error
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        bindingId = try c.decode(String.self, forKey: .bindingId)
        items = try c.decodeIfPresent([DesktopChosenItem].self, forKey: .items) ?? []
        error = try c.decodeIfPresent(String.self, forKey: .error)
    }
}

public struct DesktopAttachmentResult: Codable, Equatable, Sendable {
    public var bindingId: String
    public var pasteDisposition: String? // "attachments" | "text" | "unsupported"
}

// MARK: - Codex Desktop Session & Attachment Coordinator

public extension Notification.Name {
    static let agentHubCodexDesktopViewChanged = Notification.Name("AgentHubCodexDesktopViewChanged")
}

@MainActor
public final class CodexDesktopStore: ObservableObject {
    public static let maxMessageUTF8Bytes = 256 * 1024
    public static let maxMessageUTF16Units = 131_072
    public static let maxReferencesCount = 32
    private static var liveViewsStorage: [String: DesktopView] = [:]

    public static func liveView(revision: UInt64, threadId: String) -> DesktopView? {
        liveViewsStorage["\(revision):\(threadId)"]
    }

    public let connection: RuntimeConnectionStore
    public let threadId: String

    @Published public private(set) var view: DesktopView? {
        didSet {
            let key = "\(connection.revision):\(threadId)"
            if let view {
                Self.liveViewsStorage[key] = view
            } else {
                Self.liveViewsStorage.removeValue(forKey: key)
            }
            if oldValue != view {
                NotificationCenter.default.post(name: .agentHubCodexDesktopViewChanged, object: nil)
            }
        }
    }
    @Published public private(set) var busy: Bool = false
    @Published public var actionError: String?
    @Published public private(set) var attachmentBindingId: String?
    public private(set) var lastAttachmentBounds = DesktopAttachmentBounds(x: 0, y: 0, width: 640, height: 160)
    private var attachGeneration: UInt64 = 0
    private var lastStreamFrameGeneration: UInt64 = 0

    public var onItemsAttached: (([DesktopChosenItem]) -> Void)?
    public var onAttachmentError: ((String) -> Void)?
    public var onStreamViewUpdated: (() -> Void)?

    public init(connection: RuntimeConnectionStore, threadId: String) {
        self.connection = connection
        self.threadId = threadId
    }

    public static func validateOutgoingPayload(text: String, referencesCount: Int) throws {
        if (text as NSString).length > maxMessageUTF16Units {
            throw AgentHubFFIError("消息长度超出上限（最多 131,072 个字符）")
        }
        if text.utf8.count > maxMessageUTF8Bytes {
            throw AgentHubFFIError("消息字节大小超出上限（最多 256 KiB UTF-8）")
        }
        if referencesCount > maxReferencesCount {
            throw AgentHubFFIError("引用数量超出限制（最多 32 项）")
        }
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && referencesCount == 0 {
            throw AgentHubFFIError("消息内容不能为空")
        }
    }

    public func connect(confirmOwner: Bool = true) async {
        guard connection.target == .local, !connection.changing else {
            actionError = "桌面协作仅支持本机 Codex"
            return
        }
        let rev = connection.revision
        let tid = threadId
        busy = true
        actionError = nil
        attachGeneration &+= 1
        let currentAttachGen = attachGeneration
        defer { if attachGeneration == currentAttachGen { busy = false } }

        struct AttachReq: Encodable {
            struct Inner: Encodable {
                let revision: UInt64
                let threadId: String
                let confirmOwner: Bool
            }
            let request: Inner
        }
        guard let reqData = try? JSONEncoder().encode(AttachReq(request: .init(revision: rev, threadId: tid, confirmOwner: confirmOwner))),
              let reqJSON = String(data: reqData, encoding: .utf8)
        else { return }

        do {
            let initialData = try await connection.transport.desktopAttach(
                requestJSON: reqJSON,
                onStreamFrame: { [weak self] frameData in
                    let decoded: DesktopView
                    do {
                        decoded = try JSONDecoder().decode(DesktopView.self, from: frameData)
                    } catch {
                        Task { @MainActor [weak self] in
                            guard let self, self.connection.revision == rev, self.attachGeneration == currentAttachGen else { return }
                            self.actionError = "桌面状态流解析失败，请重新连接"
                        }
                        return
                    }
                    Task { @MainActor [weak self] in
                        guard let self,
                              self.connection.revision == rev,
                              decoded.threadId == tid,
                              self.attachGeneration == currentAttachGen
                        else { return }
                        self.lastStreamFrameGeneration = currentAttachGen
                        self.view = decoded
                        self.onStreamViewUpdated?()
                    }
                },
                onStreamClosed: nil
            )
            let initialView = try JSONDecoder().decode(DesktopView.self, from: initialData)
            guard connection.revision == rev, initialView.threadId == tid, attachGeneration == currentAttachGen else {
                await detach(sessionId: initialView.sessionId, revision: rev)
                return
            }
            if lastStreamFrameGeneration != currentAttachGen {
                view = initialView
            }
        } catch {
            guard attachGeneration == currentAttachGen, connection.revision == rev else { return }
            actionError = (error is DecodingError) ? "桌面连接响应解析失败，请重新连接" : error.localizedDescription
        }
    }

    public func detachIfNeeded() async {
        attachGeneration &+= 1
        busy = false
        let revision = connection.revision
        guard let sessionId = view?.sessionId else { return }
        view = nil
        if let bindingId = attachmentBindingId {
            attachmentBindingId = nil
            await unbindAttachments(sessionId: sessionId, bindingId: bindingId)
        }
        await detach(sessionId: sessionId, revision: revision)
    }

    private func detach(sessionId: String, revision: UInt64) async {
        struct DetachWrap: Encodable {
            struct Inner: Encodable { let revision: UInt64; let sessionId: String }
            let request: Inner
        }
        let _: DesktopAck? = try? await connection.invokeLocal(
            "codex.desktop.detach",
            args: DetachWrap(request: .init(revision: revision, sessionId: sessionId))
        )
    }

    public func act(_ action: DesktopAction) async throws {
        guard let sessionId = view?.sessionId else {
            throw AgentHubFFIError("尚未连接本机 Codex 会话")
        }
        switch action {
        case let .send(_, text, refs, _), let .steer(_, text, refs, _, _):
            try Self.validateOutgoingPayload(text: text, referencesCount: refs.count)
        default:
            break
        }
        busy = true
        actionError = nil
        defer { busy = false }

        struct ActWrap: Encodable {
            struct Inner: Encodable {
                let revision: UInt64
                let sessionId: String
                let action: DesktopAction
            }
            let request: Inner
        }
        do {
            let updated: DesktopView = try await connection.rpc(
                "codex.desktop.action",
                args: ActWrap(request: .init(revision: connection.revision, sessionId: sessionId, action: action)),
                localOnly: true
            )
            guard view?.sessionId == sessionId else { return }
            view = updated
            onStreamViewUpdated?()
        } catch {
            actionError = Self.safeCollaborationErrorMessage(error)
            throw error
        }
    }

    public func openInCodexApp() async {
        struct OpenWrap: Encodable {
            struct Inner: Encodable { let revision: UInt64; let threadId: String }
            let request: Inner
        }
        do {
            let _: DesktopAck = try await connection.rpc(
                "codex.desktop.open",
                args: OpenWrap(request: .init(revision: connection.revision, threadId: threadId)),
                localOnly: true
            )
        } catch {
            actionError = Self.safeCollaborationErrorMessage(error)
        }
    }

    public func fetchCatalog(kind: String, query: String, cursor: String? = nil, forceRefetch: Bool = false) async throws -> DesktopCatalog {
        struct CatWrap: Encodable {
            struct Inner: Encodable {
                let forceRefetch: Bool?
                let revision: UInt64
                let sessionId: String?
                let projectHandle: String?
                let kind: String
                let query: String
                let cursor: String?
            }
            let request: Inner
        }
        return try await connection.rpc(
            "codex.desktop.catalog",
            args: CatWrap(request: .init(
                forceRefetch: forceRefetch ? true : nil,
                revision: connection.revision,
                sessionId: view?.sessionId,
                projectHandle: nil,
                kind: kind,
                query: query,
                cursor: cursor
            )),
            localOnly: true
        )
    }

    public struct ReferenceCatalogBatch: Equatable, Sendable {
        public var items: [DesktopCatalogItem]
        public var nextCursors: [String: String]
        public var errors: [String: String]
        public var sourceKinds: [String]
    }

    /// Fans out `"mentions"` into `["plugins", "apps"]` (matching `useDesktopReferenceCatalog.ts`),
    /// since Rust `codex.desktop.catalog` only accepts `models | modes | skills | plugins | apps`.
    public func fetchReferenceCatalog(
        kind: String,
        query: String,
        cursors: [String: String] = [:],
        nextPageOnly: Bool = false,
        forceRefetch: Bool = false,
        retryKind: String? = nil
    ) async -> ReferenceCatalogBatch {
        let kinds = (kind == "mentions") ? ["plugins", "apps"] : [kind]
        var mergedItems: [DesktopCatalogItem] = []
        var nextCursors: [String: String] = [:]
        var errors: [String: String] = [:]

        for sourceKind in kinds {
            if let retryKind, retryKind != sourceKind { continue }
            let cursor = nextPageOnly ? cursors[sourceKind] : nil
            if nextPageOnly && cursor == nil { continue }
            do {
                let page = try await fetchCatalog(
                    kind: sourceKind,
                    query: query,
                    cursor: cursor,
                    forceRefetch: forceRefetch
                )
                mergedItems.append(contentsOf: page.items)
                if let next = page.nextCursor, !next.isEmpty {
                    nextCursors[sourceKind] = next
                }
            } catch {
                errors[sourceKind] = Self.safeCollaborationErrorMessage(error, fallback: "目录响应无效")
            }
        }
        return ReferenceCatalogBatch(items: mergedItems, nextCursors: nextCursors, errors: errors, sourceKinds: kinds)
    }

    public static func chooseItem(
        connection: RuntimeConnectionStore,
        sessionId: String?,
        kind: String
    ) async throws -> DesktopChosenItem? {
        struct ChooseWrap: Encodable {
            struct Inner: Encodable {
                let revision: UInt64
                let sessionId: String?
                let kind: String
            }
            let request: Inner
        }
        let choice: DesktopChoice = try await connection.rpc(
            "codex.desktop.choose",
            args: ChooseWrap(request: .init(revision: connection.revision, sessionId: sessionId, kind: kind)),
            localOnly: true
        )
        return choice.item
    }

    public func chooseItem(kind: String) async throws -> DesktopChosenItem? {
        try await Self.chooseItem(connection: connection, sessionId: view?.sessionId, kind: kind)
    }

    public func copyDiagnostics() async throws {
        struct DiagWrap: Encodable {
            struct Inner: Encodable { let revision: UInt64; let copy: Bool }
            let request: Inner
        }
        let _: DesktopDiagnostics = try await connection.rpc(
            "codex.desktop.diagnostics",
            args: DiagWrap(request: .init(revision: connection.revision, copy: true)),
            localOnly: true
        )
    }

    // MARK: - Attachment Binding, Live Bounds/Handles Sync, Paste & Drop

    private enum AttachmentActionPayload: Encodable {
        case bind(bounds: DesktopAttachmentBounds, handles: [String])
        case bounds(bindingId: String, bounds: DesktopAttachmentBounds, handles: [String])
        case paste(bindingId: String)
        case release(bindingId: String, handles: [String])
        case unbind(bindingId: String)

        private enum CodingKeys: String, CodingKey {
            case type, bindingId, bounds, handles
        }

        func encode(to encoder: Encoder) throws {
            var c = encoder.container(keyedBy: CodingKeys.self)
            switch self {
            case let .bind(bounds, handles):
                try c.encode("bind", forKey: .type)
                try c.encode(bounds, forKey: .bounds)
                try c.encode(handles, forKey: .handles)
            case let .bounds(bindingId, bounds, handles):
                try c.encode("bounds", forKey: .type)
                try c.encode(bindingId, forKey: .bindingId)
                try c.encode(bounds, forKey: .bounds)
                try c.encode(handles, forKey: .handles)
            case let .paste(bindingId):
                try c.encode("paste", forKey: .type)
                try c.encode(bindingId, forKey: .bindingId)
            case let .release(bindingId, handles):
                try c.encode("release", forKey: .type)
                try c.encode(bindingId, forKey: .bindingId)
                try c.encode(handles, forKey: .handles)
            case let .unbind(bindingId):
                try c.encode("unbind", forKey: .type)
                try c.encode(bindingId, forKey: .bindingId)
            }
        }
    }

    private struct AttachmentRequestEnvelope: Encodable {
        struct Inner: Encodable {
            let revision: UInt64
            let sessionId: String
            let action: AttachmentActionPayload
        }
        let request: Inner
    }

    private func encodeAttachmentRequest(
        revision: UInt64,
        sessionId: String,
        action: AttachmentActionPayload
    ) -> String? {
        let envelope = AttachmentRequestEnvelope(
            request: .init(revision: revision, sessionId: sessionId, action: action)
        )
        guard let data = try? JSONEncoder().encode(envelope) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    public static func safeAttachmentErrorMessage(_ error: Error) -> String {
        if error is DecodingError || error is EncodingError {
            return "附件通道响应无效"
        }
        return error.localizedDescription
    }

    public static func safeCollaborationErrorMessage(
        _ error: Error,
        fallback: String = "桌面协作响应无效"
    ) -> String {
        if error is DecodingError || error is EncodingError {
            return fallback
        }
        return error.localizedDescription
    }

    public func bindAttachmentsIfNeeded(bounds: DesktopAttachmentBounds, handles: [String]) async {
        lastAttachmentBounds = bounds
        guard let sessionId = view?.sessionId, view?.writable == true, attachmentBindingId == nil else { return }
        let rev = connection.revision
        guard let jsonStr = encodeAttachmentRequest(
            revision: rev,
            sessionId: sessionId,
            action: .bind(bounds: bounds, handles: handles)
        ) else { return }

        do {
            let data = try await connection.transport.desktopAttachments(
                requestJSON: jsonStr,
                onEventFrame: { [weak self] frameData in
                    let ev: DesktopAttachmentEvent
                    do {
                        ev = try JSONDecoder().decode(DesktopAttachmentEvent.self, from: frameData)
                    } catch {
                        Task { @MainActor [weak self] in
                            guard let self, self.connection.revision == rev, self.view?.sessionId == sessionId else { return }
                            self.onAttachmentError?("附件事件解析失败")
                        }
                        return
                    }
                    Task { @MainActor [weak self] in
                        guard let self, self.connection.revision == rev, self.view?.sessionId == sessionId else { return }
                        if let err = ev.error, !err.isEmpty {
                            self.onAttachmentError?(err)
                        } else if !ev.items.isEmpty {
                            self.onItemsAttached?(ev.items)
                        }
                    }
                },
                onEventClosed: nil
            )
            let res = try JSONDecoder().decode(DesktopAttachmentResult.self, from: data)
            if connection.revision == rev, view?.sessionId == sessionId {
                attachmentBindingId = res.bindingId
            } else {
                await unbindAttachments(sessionId: sessionId, bindingId: res.bindingId)
            }
        } catch {
            if connection.revision == rev, view?.sessionId == sessionId {
                onAttachmentError?(Self.safeAttachmentErrorMessage(error))
            }
        }
    }

    public func updateAttachmentBoundsAndHandles(bounds: DesktopAttachmentBounds? = nil, handles: [String]) async {
        let effectiveBounds = bounds ?? lastAttachmentBounds
        lastAttachmentBounds = effectiveBounds
        guard let sessionId = view?.sessionId, view?.writable == true else { return }
        guard let bindingId = attachmentBindingId else {
            await bindAttachmentsIfNeeded(bounds: effectiveBounds, handles: handles)
            return
        }
        guard let jsonStr = encodeAttachmentRequest(
            revision: connection.revision,
            sessionId: sessionId,
            action: .bounds(bindingId: bindingId, bounds: effectiveBounds, handles: handles)
        ) else { return }
        do {
            _ = try await connection.transport.desktopAttachments(requestJSON: jsonStr, onEventFrame: nil, onEventClosed: nil)
        } catch {
            attachmentBindingId = nil
            await bindAttachmentsIfNeeded(bounds: effectiveBounds, handles: handles)
        }
    }

    /// Returns `true` when Rust disposition falls back to plain-text paste (`super.paste` preserving Undo).
    public func handlePasteboardIngress(currentHandles: [String]) async -> Bool {
        if attachmentBindingId == nil {
            await bindAttachmentsIfNeeded(
                bounds: lastAttachmentBounds,
                handles: currentHandles
            )
        }
        guard let sessionId = view?.sessionId, let bindingId = attachmentBindingId else {
            return true
        }
        guard let jsonStr = encodeAttachmentRequest(
            revision: connection.revision,
            sessionId: sessionId,
            action: .paste(bindingId: bindingId)
        ) else { return true }

        do {
            let resData = try await connection.transport.desktopPasteCurrentGeneralPasteboard(requestJSON: jsonStr)
            let res = try JSONDecoder().decode(DesktopAttachmentResult.self, from: resData)
            if res.pasteDisposition == "text" {
                return true
            }
            if res.pasteDisposition == "unsupported" {
                let hasPlainText = !(NSPasteboard.general.string(forType: .string) ?? "").isEmpty
                if hasPlainText { return true }
                onAttachmentError?("剪贴板没有可添加的文件或图片")
                return false
            }
            return false
        } catch {
            onAttachmentError?(Self.safeAttachmentErrorMessage(error))
            return false
        }
    }

    public func releaseAttachments(handles: [String]) async {
        guard let sessionId = view?.sessionId, let bindingId = attachmentBindingId, !handles.isEmpty else { return }
        if let jsonStr = encodeAttachmentRequest(
            revision: connection.revision,
            sessionId: sessionId,
            action: .release(bindingId: bindingId, handles: handles)
        ) {
            _ = try? await connection.transport.desktopAttachments(requestJSON: jsonStr, onEventFrame: nil, onEventClosed: nil)
        }
    }

    private func unbindAttachments(sessionId: String, bindingId: String) async {
        if let jsonStr = encodeAttachmentRequest(
            revision: connection.revision,
            sessionId: sessionId,
            action: .unbind(bindingId: bindingId)
        ) {
            _ = try? await connection.transport.desktopAttachments(requestJSON: jsonStr, onEventFrame: nil, onEventClosed: nil)
        }
    }
}

// MARK: - Primitive JSON-Schema Elicitation Form Engine (DesktopRequests.tsx:81-108)

public enum ElicitationSchemaFormEngine {
    public struct ChoiceOption: Equatable, Sendable {
        public var encodedJSON: String
        public var label: String
    }

    public struct FieldSpec: Equatable, Sendable, Identifiable {
        public var key: String
        public var type: String
        public var title: String
        public var description: String?
        public var required: Bool
        public var choices: [ChoiceOption]?
        public var maxLength: Int

        public var id: String { key }
    }

    private static func jsonString(_ value: JSONValue?) -> String? {
        guard case let .string(s)? = value else { return nil }
        return s
    }

    private static func jsonDouble(_ value: JSONValue?) -> Double? {
        guard case let .number(n)? = value else { return nil }
        return n
    }

    public static func parseFields(from schema: JSONValue?) -> [FieldSpec] {
        guard case let .object(root)? = schema,
              case let .object(props)? = root["properties"]
        else { return [] }
        let requiredKeys: Set<String> = {
            guard case let .array(arr)? = root["required"] else { return [] }
            return Set(arr.compactMap { jsonString($0) })
        }()
        return props.keys.sorted().compactMap { key -> FieldSpec? in
            guard case let .object(fieldObj)? = props[key] else { return nil }
            let type = jsonString(fieldObj["type"]) ?? "string"
            let title = jsonString(fieldObj["title"]).flatMap { $0.isEmpty ? nil : $0 } ?? key
            let description = jsonString(fieldObj["description"]).flatMap { $0.isEmpty ? nil : $0 }
            let maxLen = min(4096, max(1, Int(jsonDouble(fieldObj["maxLength"]) ?? 4096)))
            let enumNames: [String] = {
                guard case let .array(arr)? = fieldObj["enumNames"] else { return [] }
                return arr.compactMap { jsonString($0) }
            }()
            var choices: [ChoiceOption]? = nil
            if case let .array(enumVals)? = fieldObj["enum"] {
                choices = enumVals.enumerated().compactMap { idx, val in
                    guard let data = try? JSONEncoder().encode(val),
                          let encoded = String(data: data, encoding: .utf8)
                    else { return nil }
                    let fallbackLabel: String
                    switch val {
                    case let .bool(b): fallbackLabel = b ? "是" : "否"
                    case let .string(s): fallbackLabel = s
                    case let .number(n): fallbackLabel = n.truncatingRemainder(dividingBy: 1) == 0 ? String(Int64(n)) : String(n)
                    default: fallbackLabel = encoded
                    }
                    let label = idx < enumNames.count ? enumNames[idx] : fallbackLabel
                    return ChoiceOption(encodedJSON: encoded, label: label)
                }
            } else if type == "boolean" {
                choices = [
                    ChoiceOption(encodedJSON: "true", label: enumNames.count > 0 ? enumNames[0] : "是"),
                    ChoiceOption(encodedJSON: "false", label: enumNames.count > 1 ? enumNames[1] : "否"),
                ]
            }
            return FieldSpec(
                key: key,
                type: type,
                title: title,
                description: description,
                required: requiredKeys.contains(key),
                choices: choices,
                maxLength: maxLen
            )
        }
    }

    public static func buildSubmissionContent(
        fields: [FieldSpec],
        rawValues: [String: String]
    ) throws -> [String: JSONValue] {
        var out: [String: JSONValue] = [:]
        for field in fields {
            let raw = rawValues[field.key] ?? ""
            if raw.isEmpty {
                if field.required {
                    throw AgentHubFFIError("请检查表单内容")
                }
                continue
            }
            if field.choices != nil && field.type != "boolean" {
                guard let data = raw.data(using: .utf8),
                      let parsed = try? JSONDecoder().decode(JSONValue.self, from: data)
                else {
                    throw AgentHubFFIError("请检查表单内容")
                }
                out[field.key] = parsed
            } else if field.type == "boolean" {
                out[field.key] = .bool(raw == "true")
            } else if field.type == "integer" {
                guard let intVal = Int64(raw.trimmingCharacters(in: .whitespaces)) else {
                    throw AgentHubFFIError("请检查表单内容")
                }
                out[field.key] = .number(Double(intVal))
            } else if field.type == "number" {
                guard let num = Double(raw.trimmingCharacters(in: .whitespaces)), num.isFinite else {
                    throw AgentHubFFIError("请检查表单内容")
                }
                out[field.key] = .number(num)
            } else {
                out[field.key] = .string(String(raw.prefix(field.maxLength)))
            }
        }
        return out
    }
}
