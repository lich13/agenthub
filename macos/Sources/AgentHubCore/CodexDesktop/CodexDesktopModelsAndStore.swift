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
}

public struct DesktopQuestionOption: Codable, Equatable, Sendable {
    public var label: String
    public var description: String
}

public struct DesktopQuestion: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var header: String
    public var question: String
    public var secret: Bool
    public var options: [DesktopQuestionOption]
}

public struct DesktopFileReview: Codable, Equatable, Sendable {
    public var path: String
    public var kind: String
    public var diff: String
    public var truncated: Bool
}

public struct DesktopPermissionsReview: Codable, Equatable, Sendable {
    public var network: String?
    public var fileSystem: String?
}

public struct DesktopReview: Codable, Equatable, Sendable {
    public var command: String?
    public var cwd: String?
    public var reason: String?
    public var changes: [DesktopFileReview]
    public var permissions: DesktopPermissionsReview?
    public var extra: String?
    public var truncated: Bool
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
}

public struct DesktopGoal: Codable, Equatable, Sendable {
    public var objective: String
    public var status: String
    public var truncated: Bool
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
}

public struct DesktopAttachmentResult: Codable, Equatable, Sendable {
    public var bindingId: String
    public var pasteDisposition: String? // "attachments" | "text" | "unsupported"
}

// MARK: - Codex Desktop Session & Attachment Coordinator

@MainActor
public final class CodexDesktopStore: ObservableObject {
    public static let maxMessageUTF8Bytes = 256 * 1024
    public static let maxMessageUTF16Units = 131_072
    public static let maxReferencesCount = 32

    public let connection: RuntimeConnectionStore
    public let threadId: String

    @Published public private(set) var view: DesktopView?
    @Published public private(set) var busy: Bool = false
    @Published public var actionError: String?
    @Published public private(set) var attachmentBindingId: String?

    public var onItemsAttached: (([DesktopChosenItem]) -> Void)?
    public var onAttachmentError: ((String) -> Void)?

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
        defer { busy = false }

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
                    guard let decoded = try? JSONDecoder().decode(DesktopView.self, from: frameData) else { return }
                    Task { @MainActor [weak self] in
                        guard let self, self.connection.revision == rev, decoded.threadId == tid else { return }
                        self.view = decoded
                    }
                },
                onStreamClosed: nil
            )
            let initialView = try JSONDecoder().decode(DesktopView.self, from: initialData)
            if connection.revision == rev && initialView.threadId == tid {
                view = view ?? initialView
            }
        } catch {
            actionError = error.localizedDescription
        }
    }

    public func detachIfNeeded() async {
        if let bindingId = attachmentBindingId, let sessionId = view?.sessionId {
            attachmentBindingId = nil
            await unbindAttachments(sessionId: sessionId, bindingId: bindingId)
        }
        guard let sessionId = view?.sessionId else { return }
        struct DetachWrap: Encodable {
            struct Inner: Encodable { let revision: UInt64; let sessionId: String }
            let request: Inner
        }
        let _: DesktopAck? = try? await connection.invokeLocal(
            "codex.desktop.detach",
            args: DetachWrap(request: .init(revision: connection.revision, sessionId: sessionId))
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
            view = updated
        } catch {
            actionError = error.localizedDescription
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
            actionError = error.localizedDescription
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

    public func chooseItem(kind: String) async throws -> DesktopChosenItem? {
        struct ChooseWrap: Encodable {
            struct Inner: Encodable { let revision: UInt64; let sessionId: String?; let kind: String }
            let request: Inner
        }
        let choice: DesktopChoice = try await connection.rpc(
            "codex.desktop.choose",
            args: ChooseWrap(request: .init(revision: connection.revision, sessionId: view?.sessionId, kind: kind)),
            localOnly: true
        )
        return choice.item
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

    // MARK: - Attachment Binding, Paste & Drop

    public func bindAttachmentsIfNeeded(bounds: DesktopAttachmentBounds, handles: [String]) async {
        guard let sessionId = view?.sessionId, view?.writable == true, attachmentBindingId == nil else { return }
        let rev = connection.revision
        let payload: [String: JSONValue] = [
            "request": .object([
                "revision": .number(Double(rev)),
                "sessionId": .string(sessionId),
                "action": .object([
                    "type": .string("bind"),
                    "bounds": .object([
                        "x": .number(bounds.x), "y": .number(bounds.y),
                        "width": .number(bounds.width), "height": .number(bounds.height),
                    ]),
                    "handles": .array(handles.map { .string($0) }),
                ]),
            ]),
        ]
        guard let jsonData = try? JSONEncoder().encode(payload),
              let jsonStr = String(data: jsonData, encoding: .utf8)
        else { return }

        if let data = try? await connection.transport.desktopAttachments(
            requestJSON: jsonStr,
            onEventFrame: { [weak self] frameData in
                guard let ev = try? JSONDecoder().decode(DesktopAttachmentEvent.self, from: frameData) else { return }
                Task { @MainActor [weak self] in
                    guard let self, self.connection.revision == rev else { return }
                    if let err = ev.error, !err.isEmpty {
                        self.onAttachmentError?(err)
                    } else if !ev.items.isEmpty {
                        self.onItemsAttached?(ev.items)
                    }
                }
            },
            onEventClosed: nil
        ),
            let res = try? JSONDecoder().decode(DesktopAttachmentResult.self, from: data)
        {
            attachmentBindingId = res.bindingId
        }
    }

    /// Returns `true` when Rust disposition is `"text"`, instructing `ComposerNSTextView` to invoke `super.paste` preserving Undo!
    public func handlePasteboardIngress(currentHandles: [String]) async -> Bool {
        if attachmentBindingId == nil {
            await bindAttachmentsIfNeeded(
                bounds: DesktopAttachmentBounds(x: 0, y: 0, width: 640, height: 160),
                handles: currentHandles
            )
        }
        guard let sessionId = view?.sessionId, let bindingId = attachmentBindingId else {
            return true
        }
        let payload: [String: JSONValue] = [
            "request": .object([
                "revision": .number(Double(connection.revision)),
                "sessionId": .string(sessionId),
                "action": .object([
                    "type": .string("paste"),
                    "bindingId": .string(bindingId),
                ]),
            ]),
        ]
        guard let jsonData = try? JSONEncoder().encode(payload),
              let jsonStr = String(data: jsonData, encoding: .utf8)
        else { return true }

        do {
            let resData = try await connection.transport.desktopPasteCurrentGeneralPasteboard(requestJSON: jsonStr)
            let res = try JSONDecoder().decode(DesktopAttachmentResult.self, from: resData)
            return res.pasteDisposition == "text"
        } catch {
            actionError = error.localizedDescription
            return false
        }
    }

    public func releaseAttachments(handles: [String]) async {
        guard let sessionId = view?.sessionId, let bindingId = attachmentBindingId, !handles.isEmpty else { return }
        let payload: [String: JSONValue] = [
            "request": .object([
                "revision": .number(Double(connection.revision)),
                "sessionId": .string(sessionId),
                "action": .object([
                    "type": .string("release"),
                    "bindingId": .string(bindingId),
                    "handles": .array(handles.map { .string($0) }),
                ]),
            ]),
        ]
        if let jsonData = try? JSONEncoder().encode(payload),
           let jsonStr = String(data: jsonData, encoding: .utf8)
        {
            _ = try? await connection.transport.desktopAttachments(requestJSON: jsonStr, onEventFrame: nil, onEventClosed: nil)
        }
    }

    private func unbindAttachments(sessionId: String, bindingId: String) async {
        let payload: [String: JSONValue] = [
            "request": .object([
                "revision": .number(Double(connection.revision)),
                "sessionId": .string(sessionId),
                "action": .object([
                    "type": .string("unbind"),
                    "bindingId": .string(bindingId),
                ]),
            ]),
        ]
        if let jsonData = try? JSONEncoder().encode(payload),
           let jsonStr = String(data: jsonData, encoding: .utf8)
        {
            _ = try? await connection.transport.desktopAttachments(requestJSON: jsonStr, onEventFrame: nil, onEventClosed: nil)
        }
    }
}
