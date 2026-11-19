import AppKit
import Foundation

// MARK: - Unified Reader Row Item Across All 6 Providers

public enum ConversationRowKind: Equatable, Sendable {
    case user(message: UserMessageContent?, rawText: String, attachmentsAsNamesOnly: Bool)
    case assistant(markdown: String, foldInstructions: Bool)
    case executionGroup(ExecutionGroup)
    case thinking(title: String, text: String, running: Bool)
    case plan(markdown: String, statusText: String, fallbackTitle: String)
    case question(header: String, questions: [UserInputQuestion])
    case questionResult(statusText: String, questions: [UserInputQuestion], answers: [UserInputAnswer])
    case subagent(SubagentActivity, supported: Bool)
    case historyCollapse(eyebrow: String, label: String, expanded: Bool)
    case systemNotice(label: String, detail: String)
}

public struct ConversationRowItem: Equatable, Sendable, Identifiable {
    public var id: String
    public var timelineId: String
    public var timelineAliases: [String]
    public var kind: ConversationRowKind

    public init(
        id: String,
        timelineId: String? = nil,
        timelineAliases: [String] = [],
        kind: ConversationRowKind
    ) {
        self.id = id
        self.timelineId = timelineId ?? id
        self.timelineAliases = timelineAliases
        self.kind = kind
    }

    public func matches(positionKey: String?) -> Bool {
        TimelineViewModel.matchesTimelinePosition(positionKey, id: timelineId, aliases: timelineAliases)
    }
}

public enum ConversationRowBuilder {
    public static func buildCodexRows(
        blocks: [MessageBlock],
        showAllHistory: Bool,
        planFallbackTitle: String,
        subagentsSupported: Bool,
        revealedBlockId: String? = nil
    ) -> [ConversationRowItem] {
        let visible = ConversationViewModel.compactConversationBlocks(
            blocks,
            showAllHistory: showAllHistory,
            revealedBlockId: revealedBlockId
        )
        let grouped = ExecutionGroupingEngine.groupCodexBlocks(visible)
        var rows: [ConversationRowItem] = []

        for entry in grouped {
            switch entry {
            case let .group(group):
                let aliases = group.mergedIds + group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(
                    id: group.id,
                    timelineId: group.id,
                    timelineAliases: aliases,
                    kind: .executionGroup(group)
                ))
            case let .item(block, key, sourceIds):
                if let sub = block.subagent {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: block.id,
                        timelineAliases: sourceIds,
                        kind: .subagent(sub, supported: subagentsSupported)
                    ))
                } else if ConversationViewModel.isHistoryCollapsedBlock(block) {
                    let cKind = ConversationViewModel.historyCollapseKind(block)
                    let eyebrow = cKind == "tool" ? "Tool activity" : (cKind == "action" ? "Plan & questions" : "Earlier messages")
                    let label = block.summary ?? block.text ?? "较早消息已折叠"
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: block.id,
                        timelineAliases: block.collapsedIds ?? [],
                        kind: .historyCollapse(eyebrow: eyebrow, label: label, expanded: showAllHistory)
                    ))
                } else if ConversationViewModel.isPlanBlock(block) {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(
                        (block.text ?? "").replacingOccurrences(of: "</?proposed_plan>", with: "", options: .regularExpression)
                    )
                    let md = cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "暂无计划内容" : cleaned
                    let statusText = block.planStatus ?? block.status ?? block.turnId ?? block.kind
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: block.id,
                        timelineAliases: sourceIds,
                        kind: .plan(markdown: md, statusText: statusText, fallbackTitle: planFallbackTitle)
                    ))
                } else if ConversationViewModel.isQuestionResultBlock(block) {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: block.id,
                        timelineAliases: sourceIds,
                        kind: .questionResult(
                            statusText: block.status ?? "completed",
                            questions: block.questions,
                            answers: block.answers ?? []
                        )
                    ))
                } else if ConversationViewModel.isQuestionBlock(block) {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: block.id,
                        timelineAliases: sourceIds,
                        kind: .question(
                            header: block.turnId ?? block.itemId ?? block.callId ?? "request_user_input",
                            questions: block.questions
                        )
                    ))
                } else if block.role == "user" {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: block.id,
                        timelineAliases: sourceIds,
                        kind: .user(message: block.userMessage, rawText: block.text ?? "", attachmentsAsNamesOnly: false)
                    ))
                } else if ConversationViewModel.shouldRenderConversationMessage(block) {
                    let md = ConversationViewModel.messageBlockText(block)
                    if !md.isEmpty {
                        rows.append(ConversationRowItem(
                            id: key,
                            timelineId: block.id,
                            timelineAliases: sourceIds,
                            kind: .assistant(markdown: md, foldInstructions: true)
                        ))
                    }
                }
            }
        }
        return rows
    }

    public static func buildClaudeRows(
        events: [ClaudeHistoryEvent],
        recordedOnly: Bool = false,
        attachmentsAsNamesOnly: Bool = false
    ) -> [ConversationRowItem] {
        let grouped = ExecutionGroupingEngine.groupClaudeEvents(events, recordedOnly: recordedOnly)
        var rows: [ConversationRowItem] = []
        for entry in grouped {
            switch entry {
            case let .group(group):
                let aliases = group.mergedIds + group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(id: group.id, timelineId: group.id, timelineAliases: aliases, kind: .executionGroup(group)))
            case let .item(ev, key, sourceIds):
                if ev.kind == "user_message" {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .user(message: ev.userMessage, rawText: ev.text ?? "", attachmentsAsNamesOnly: attachmentsAsNamesOnly)
                    ))
                } else if ev.kind == "thinking" {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
                    if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        rows.append(ConversationRowItem(
                            id: key,
                            timelineId: ev.id,
                            timelineAliases: sourceIds,
                            kind: .thinking(title: recordedOnly ? "思考记录" : "已思考", text: cleaned, running: !recordedOnly && ev.status == "running")
                        ))
                    }
                } else if ev.kind == "assistant_message" || ev.role == "assistant" {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
                    if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        rows.append(ConversationRowItem(
                            id: key,
                            timelineId: ev.id,
                            timelineAliases: sourceIds,
                            kind: .assistant(markdown: cleaned, foldInstructions: true)
                        ))
                    }
                } else if let text = ev.text ?? ev.detail, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .systemNotice(label: ev.role ?? ev.kind, detail: text)
                    ))
                }
            }
        }
        return rows
    }

    public static func buildGrokRows(events: [GrokHistoryEvent], fallbackTitle: String) -> [ConversationRowItem] {
        let grouped = ExecutionGroupingEngine.groupGrokEvents(events)
        var rows: [ConversationRowItem] = []
        for entry in grouped {
            switch entry {
            case let .group(group):
                let aliases = group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(id: group.id, timelineId: group.id, timelineAliases: aliases, kind: .executionGroup(group)))
            case let .item(ev, key, sourceIds):
                let tid = ev.id ?? ev.userMessage?.id ?? key
                if ev.kind == "user_message_chunk" || ev.kind == "user_message" {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: tid,
                        timelineAliases: sourceIds,
                        kind: .user(message: ev.userMessage, rawText: ev.text ?? "", attachmentsAsNamesOnly: false)
                    ))
                } else if ev.kind == "thought_chunk" || ev.kind == "thinking" {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
                    if !cleaned.isEmpty {
                        rows.append(ConversationRowItem(
                            id: key,
                            timelineId: tid,
                            timelineAliases: sourceIds,
                            kind: .thinking(title: "已思考", text: cleaned, running: ev.status == "running")
                        ))
                    }
                } else if ev.kind == "plan" {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? ev.detail ?? "")
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: tid,
                        timelineAliases: sourceIds,
                        kind: .plan(markdown: cleaned.isEmpty ? "暂无计划内容" : cleaned, statusText: ev.status ?? "plan", fallbackTitle: fallbackTitle)
                    ))
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: tid,
                        timelineAliases: sourceIds,
                        kind: .assistant(markdown: VisibleMarkdownEngine.visibleMarkdown(text), foldInstructions: true)
                    ))
                }
            }
        }
        return rows
    }

    public static func buildProviderRows(events: [ProviderHistoryEvent], providerLabel: String) -> [ConversationRowItem] {
        let grouped = ExecutionGroupingEngine.groupProviderEvents(events, provider: providerLabel)
        var rows: [ConversationRowItem] = []
        for entry in grouped {
            switch entry {
            case let .group(group):
                let aliases = group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(id: group.id, timelineId: group.id, timelineAliases: aliases, kind: .executionGroup(group)))
            case let .item(ev, key, sourceIds):
                if ev.kind == "user_message" {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .user(message: ev.userMessage, rawText: ev.text ?? "", attachmentsAsNamesOnly: false)
                    ))
                } else if ev.kind == "thinking" {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
                    if !cleaned.isEmpty {
                        rows.append(ConversationRowItem(
                            id: key,
                            timelineId: ev.id,
                            timelineAliases: sourceIds,
                            kind: .thinking(title: "已思考", text: cleaned, running: ev.status == "running")
                        ))
                    }
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .assistant(markdown: VisibleMarkdownEngine.visibleMarkdown(text), foldInstructions: true)
                    ))
                }
            }
        }
        return rows
    }
}

// MARK: - 96x96pt User Image Thumbnail Viewport Manager (±240pt Load & Eviction)

@MainActor
public final class AttachmentViewportManager {
    public static let viewportMargin: CGFloat = 240
    private var loadedImages: [String: NSImage] = [:]
    private var inFlightKeys: Set<String> = []
    public var onImageUpdated: ((String) -> Void)?

    public init() {}

    public func image(for key: String) -> NSImage? {
        loadedImages[key]
    }

    public var loadedCount: Int { loadedImages.count }

    public func evictOutside(visibleKeys: Set<String>) {
        let keysToDrop = loadedImages.keys.filter { !visibleKeys.contains($0) }
        for k in keysToDrop {
            loadedImages.removeValue(forKey: k)
        }
    }

    public func ensureLoaded(
        key: String,
        request: SessionAttachmentRequest,
        store: RuntimeConnectionStore
    ) {
        guard loadedImages[key] == nil, !inFlightKeys.contains(key) else { return }
        inFlightKeys.insert(key)
        Task { @MainActor [weak self] in
            guard let self else { return }
            defer { self.inFlightKeys.remove(key) }
            struct ReqWrap: Encodable { let request: SessionAttachmentRequest }
            guard let resp: SessionAttachmentResponse = try? await store.rpc(
                "sessions.attachmentRead",
                args: ReqWrap(request: request)
            ),
                let data = Data(base64Encoded: resp.base64),
                let img = NSImage(data: data)
            else { return }
            self.loadedImages[key] = img
            self.onImageUpdated?(key)
        }
    }

    public func injectForTesting(key: String, image: NSImage) {
        loadedImages[key] = image
    }
}

// MARK: - Selectable Read-Only NSTextView for Natural Prose Selection

public final class ReaderSelectableTextView: NSTextView {
    public var onLinkActivated: ((String) -> Bool)?

    public convenience init() {
        self.init(frame: .zero)
        isEditable = false
        isSelectable = true
        drawsBackground = false
        isRichText = true
        importsGraphics = false
        textContainerInset = .zero
        textContainer?.lineFragmentPadding = 0
        textContainer?.widthTracksTextView = true
        isVerticallyResizable = true
        isHorizontallyResizable = false
    }

    override public func clicked(onLink link: Any, at charIndex: Int) {
        let href: String = if let url = link as? URL {
            url.absoluteString
        } else if let s = link as? String {
            s
        } else {
            ""
        }
        if !href.isEmpty, onLinkActivated?(href) == true {
            return
        }
        super.clicked(onLink: link, at: charIndex)
    }

    public static func measureHeight(attributed: NSAttributedString, width: CGFloat) -> CGFloat {
        guard attributed.length > 0, width > 1 else { return 0 }
        let storage = NSTextStorage(attributedString: attributed)
        let container = NSTextContainer(size: NSSize(width: width, height: .greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        let manager = NSLayoutManager()
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        return ceil(manager.usedRect(for: container).height)
    }
}
