import AppKit
import Foundation

// MARK: - Unified Reader Row Item Across All 6 Providers

public enum ConversationRowKind: Equatable, Sendable {
    case user(message: UserMessageContent?, rawText: String, attachmentsAsNamesOnly: Bool)
    case assistant(markdown: String, foldInstructions: Bool)
    case executionGroup(ExecutionGroup)
    case processGroup(UnifiedProcessGroup)
    case claudeProcessGroup(ClaudeProcessGroup, title: String)
    case claudeMetadataGroup(ClaudeMetadataGroup, title: String)
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
    private static func executionTimelineAliases(_ group: ExecutionGroup) -> [String] {
        ProcessGroupingEngine.uniqueStrings(group.stateAliases + group.commands.flatMap { [$0.id] + $0.sourceIds })
    }

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
        let folded = ProcessGroupingEngine.foldProcessSequence(prefix: "codex", items: grouped) { entry in
            switch entry {
            case let .group(group):
                return .step(.execution(group: group, turnId: group.turnId))
            case let .item(block, key, sourceIds):
                if block.subagent != nil
                    || ConversationViewModel.isHistoryCollapsedBlock(block)
                    || ConversationViewModel.isPlanBlock(block)
                    || ConversationViewModel.isQuestionBlock(block)
                    || ConversationViewModel.isQuestionResultBlock(block)
                {
                    return .passthrough
                }
                if block.role == "assistant" && block.phase == "commentary" {
                    return ProcessGroupingEngine.thinkingStepFromText(
                        id: key,
                        sourceIds: sourceIds,
                        rawText: block.text,
                        running: false,
                        turnId: block.turnId
                    )
                }
                if ConversationViewModel.isToolBlock(block) {
                    let st = (block.status ?? "").lowercased()
                    let failed = st == "failed" || st == "error"
                    let running = ["pending", "running", "in_progress", "inprogress", "active"].contains(st)
                    let sids = [block.id, block.callId].compactMap { $0 }.filter { !$0.isEmpty }
                    return .step(.detail(
                        id: block.id,
                        sourceIds: sids,
                        title: block.toolName ?? block.kind,
                        detail: ConversationViewModel.toolBlockDetailText(block),
                        running: running,
                        failed: failed,
                        turnId: block.turnId
                    ))
                }
                return .passthrough
            }
        }

        var rows: [ConversationRowItem] = []
        for foldedItem in folded {
            switch foldedItem {
            case let .process(proc):
                rows.append(ConversationRowItem(
                    id: proc.id,
                    timelineId: proc.id,
                    timelineAliases: proc.timelineAliases,
                    kind: .processGroup(proc)
                ))
            case let .execution(group):
                rows.append(ConversationRowItem(
                    id: group.id,
                    timelineId: group.id,
                    timelineAliases: executionTimelineAliases(group),
                    kind: .executionGroup(group)
                ))
            case let .passthrough(entry):
                guard case let .item(block, key, sourceIds) = entry else { continue }
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
                            answers: block.answers
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
                } else if block.role == "user", ConversationViewModel.shouldRenderConversationMessage(block) {
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
        attachmentsAsNamesOnly: Bool = false,
        fallbackTitle: String = ""
    ) -> [ConversationRowItem] {
        let entries = ClaudeProcessGroupingEngine.groupClaudeProcesses(events, recordedOnly: recordedOnly)
        let folded = ProcessGroupingEngine.foldProcessSequence(prefix: "claude", items: entries) { entry in
            switch entry {
            case let .group(group):
                return .step(.execution(group: group, turnId: group.turnId))
            case let .item(ev, key, sourceIds):
                if ClaudeProcessGroupingEngine.isThoughtOrCommentary(ev) {
                    return ProcessGroupingEngine.thinkingStepFromText(
                        id: key,
                        sourceIds: sourceIds,
                        rawText: ev.text,
                        running: !recordedOnly && ev.status == "running",
                        turnId: ev.turnId
                    )
                }
                if let detail = ev.detail, !detail.isEmpty, ClaudeProcessGroupingEngine.isProcess(ev) {
                    let status = ExecutionGroupingEngine.executionStatus(ev.status)
                    return .step(.detail(
                        id: key,
                        sourceIds: sourceIds,
                        title: ev.text ?? ev.role ?? "活动记录",
                        detail: detail,
                        running: !recordedOnly && status == .running,
                        failed: ev.kind == "failure" || status == .failed,
                        turnId: ev.turnId
                    ))
                }
                return .passthrough
            case .process, .metadata:
                return .passthrough
            }
        }

        var rows: [ConversationRowItem] = []
        for foldedItem in folded {
            switch foldedItem {
            case let .process(proc):
                rows.append(ConversationRowItem(
                    id: proc.id,
                    timelineId: proc.id,
                    timelineAliases: proc.timelineAliases,
                    kind: .processGroup(proc)
                ))
            case let .execution(group):
                rows.append(ConversationRowItem(
                    id: group.id,
                    timelineId: group.id,
                    timelineAliases: executionTimelineAliases(group),
                    kind: .executionGroup(group)
                ))
            case let .passthrough(entry):
                switch entry {
                case let .process(proc):
                    let procIdAliases = proc.sourceIds.map { "claude-process[\(proc.turnId)][\($0)]" }
                    let innerCmdAliases = proc.entries.flatMap { sub -> [String] in
                        switch sub {
                        case let .item(_, key, sids): [key] + sids
                        case let .metadata(m): [m.id] + m.sourceIds + m.sourceIds.map { "claude-metadata:\($0)" }
                        case let .group(g): [g.id] + executionTimelineAliases(g)
                        }
                    }
                    rows.append(ConversationRowItem(
                        id: proc.id,
                        timelineId: proc.id,
                        timelineAliases: ProcessGroupingEngine.uniqueStrings(proc.sourceIds + procIdAliases + innerCmdAliases),
                        kind: .claudeProcessGroup(proc, title: fallbackTitle)
                    ))
                case let .metadata(meta):
                    rows.append(ConversationRowItem(
                        id: meta.id,
                        timelineId: meta.id,
                        timelineAliases: ProcessGroupingEngine.uniqueStrings(
                            meta.sourceIds + meta.sourceIds.map { "claude-metadata:\($0)" }
                        ),
                        kind: .claudeMetadataGroup(meta, title: fallbackTitle)
                    ))
                case let .group(group):
                    rows.append(ConversationRowItem(
                        id: group.id,
                        timelineId: group.id,
                        timelineAliases: executionTimelineAliases(group),
                        kind: .executionGroup(group)
                    ))
                case let .item(ev, key, sourceIds):
                    appendSingleClaudeEventRow(
                        ev: ev,
                        key: key,
                        sourceIds: sourceIds,
                        recordedOnly: recordedOnly,
                        attachmentsAsNamesOnly: attachmentsAsNamesOnly,
                        fallbackTitle: fallbackTitle,
                        into: &rows
                    )
                }
            }
        }
        return rows
    }

    private static func appendSingleClaudeEventRow(
        ev: ClaudeHistoryEvent,
        key: String,
        sourceIds: [String],
        recordedOnly: Bool,
        attachmentsAsNamesOnly: Bool,
        fallbackTitle: String,
        into rows: inout [ConversationRowItem]
    ) {
        if ev.kind == "user_message" {
            let aliases = [key] + sourceIds + (ev.userMessage.map { [$0.id] } ?? [])
            rows.append(ConversationRowItem(
                id: key,
                timelineId: ev.id.isEmpty ? key : ev.id,
                timelineAliases: aliases,
                kind: .user(message: ev.userMessage, rawText: ev.text ?? "", attachmentsAsNamesOnly: attachmentsAsNamesOnly)
            ))
        } else if ev.kind == "plan" {
            let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
            rows.append(ConversationRowItem(
                id: key,
                timelineId: key,
                timelineAliases: sourceIds,
                kind: .plan(markdown: cleaned.isEmpty ? "暂无计划内容" : cleaned, statusText: ev.status ?? "计划", fallbackTitle: fallbackTitle)
            ))
        } else if ev.kind == "thinking" {
            let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
            if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                rows.append(ConversationRowItem(
                    id: key,
                    timelineId: key,
                    timelineAliases: sourceIds,
                    kind: .thinking(title: recordedOnly ? "思考记录" : "已思考", text: cleaned, running: !recordedOnly && ev.status == "running")
                ))
            }
        } else if ev.kind.hasPrefix("assistant_message") || ev.role == "assistant" {
            let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
            if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                rows.append(ConversationRowItem(
                    id: key,
                    timelineId: key,
                    timelineAliases: sourceIds,
                    kind: .assistant(markdown: cleaned, foldInstructions: true)
                ))
            }
        } else if let text = ev.text ?? ev.detail, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            rows.append(ConversationRowItem(
                id: key,
                timelineId: key,
                timelineAliases: sourceIds,
                kind: .systemNotice(label: ev.role ?? ev.kind, detail: text)
            ))
        }
    }

    public static func buildGrokRows(events: [GrokHistoryEvent], fallbackTitle: String, sessionRunning: Bool = false) -> [ConversationRowItem] {
        let grouped = ExecutionGroupingEngine.groupGrokEvents(events)
        let lastIndex = grouped.count - 1
        let indexed = Array(grouped.enumerated())
        let folded = ProcessGroupingEngine.foldProcessSequence(prefix: "grok", items: indexed.map(\.element)) { entry in
            switch entry {
            case let .group(group):
                return .step(.execution(group: group, turnId: group.turnId))
            case let .item(ev, key, sourceIds):
                if ev.kind == "agent_thought_chunk" || ev.kind == "thought_chunk" || ev.kind == "thinking" {
                    let isTrailing = (grouped.last == entry) && lastIndex >= 0
                    return ProcessGroupingEngine.thinkingStepFromText(
                        id: key,
                        sourceIds: sourceIds,
                        rawText: ev.text,
                        running: sessionRunning && isTrailing
                    )
                }
                return .passthrough
            }
        }

        var rows: [ConversationRowItem] = []
        for foldedItem in folded {
            switch foldedItem {
            case let .process(proc):
                rows.append(ConversationRowItem(id: proc.id, timelineId: proc.id, timelineAliases: proc.timelineAliases, kind: .processGroup(proc)))
            case let .execution(group):
                rows.append(ConversationRowItem(
                    id: group.id,
                    timelineId: group.id,
                    timelineAliases: executionTimelineAliases(group),
                    kind: .executionGroup(group)
                ))
            case let .passthrough(entry):
                guard case let .item(ev, key, sourceIds) = entry else { continue }
                let tid = ev.id ?? ev.userMessage?.id ?? key
                if ev.kind == "user_message_chunk" || ev.kind == "user_message" {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: tid,
                        timelineAliases: sourceIds,
                        kind: .user(message: ev.userMessage, rawText: ev.text ?? "", attachmentsAsNamesOnly: false)
                    ))
                } else if ev.kind == "plan" {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? ev.detail ?? "")
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: tid,
                        timelineAliases: sourceIds,
                        kind: .plan(markdown: cleaned.isEmpty ? "暂无计划内容" : cleaned, statusText: ev.status ?? "plan", fallbackTitle: fallbackTitle)
                    ))
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
                    if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        rows.append(ConversationRowItem(
                            id: key,
                            timelineId: tid,
                            timelineAliases: sourceIds,
                            kind: .assistant(markdown: cleaned, foldInstructions: true)
                        ))
                    }
                }
            }
        }
        return rows
    }

    public static func buildProviderRows(
        events: [ProviderHistoryEvent],
        providerLabel: String,
        sessionRunning: Bool = false,
        subagentsSupported: Bool = false
    ) -> [ConversationRowItem] {
        let prefix = providerLabel == "Antigravity" ? "antigravity" : (providerLabel == "Grok Bot" ? "grokbot" : "dsh")
        let grouped = ExecutionGroupingEngine.groupProviderEvents(
            events,
            provider: providerLabel,
            recordedOnly: !sessionRunning
        )
        let folded = ProcessGroupingEngine.foldProcessSequence(prefix: prefix, items: grouped) { entry in
            switch entry {
            case let .group(group):
                return .step(.execution(group: group, turnId: group.turnId))
            case let .item(ev, key, sourceIds):
                if ev.subagent != nil {
                    return .passthrough
                }
                if ev.kind == "thinking" {
                    let isTrailing = (grouped.last == entry)
                    return ProcessGroupingEngine.thinkingStepFromText(
                        id: key,
                        sourceIds: sourceIds,
                        rawText: ev.text,
                        running: sessionRunning && isTrailing,
                        turnId: ev.turnId
                    )
                }
                if let detail = ev.detail, !detail.isEmpty,
                   ev.kind != "interaction",
                   ev.kind != "user_message",
                   ev.kind != "plan",
                   !ev.kind.hasPrefix("assistant_message")
                {
                    let st = (ev.status ?? "").lowercased()
                    return .step(.detail(
                        id: key,
                        sourceIds: sourceIds,
                        title: ev.text ?? "活动记录",
                        detail: detail,
                        running: false,
                        failed: ev.kind == "failure" || st == "failed" || st == "error",
                        turnId: ev.turnId
                    ))
                }
                return .passthrough
            }
        }

        var rows: [ConversationRowItem] = []
        for foldedItem in folded {
            switch foldedItem {
            case let .process(proc):
                rows.append(ConversationRowItem(id: proc.id, timelineId: proc.id, timelineAliases: proc.timelineAliases, kind: .processGroup(proc)))
            case let .execution(group):
                rows.append(ConversationRowItem(
                    id: group.id,
                    timelineId: group.id,
                    timelineAliases: executionTimelineAliases(group),
                    kind: .executionGroup(group)
                ))
            case let .passthrough(entry):
                guard case let .item(ev, key, sourceIds) = entry else { continue }
                if let sub = ev.subagent {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .subagent(sub, supported: subagentsSupported)
                    ))
                } else if ev.kind == "user_message" {
                    let userMsg: UserMessageContent?
                    if providerLabel == "Antigravity" {
                        var literal = ev.userMessage ?? UserMessageContent(id: ev.id, text: ev.text ?? "")
                        literal.contextBlocks = []
                        userMsg = literal
                    } else {
                        userMsg = ev.userMessage
                    }
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .user(message: userMsg, rawText: ev.text ?? "", attachmentsAsNamesOnly: false)
                    ))
                } else if ev.kind == "plan" {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(ev.text ?? "")
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .plan(markdown: cleaned.isEmpty ? "暂无计划内容" : cleaned, statusText: ev.status ?? "计划", fallbackTitle: providerLabel)
                    ))
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
                    if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        rows.append(ConversationRowItem(
                            id: key,
                            timelineId: ev.id,
                            timelineAliases: sourceIds,
                            kind: .assistant(markdown: cleaned, foldInstructions: ev.kind != "compaction" && ev.kind != "branch_summary")
                        ))
                    }
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
        let sourceRevision = store.revision
        Task { @MainActor [weak self] in
            guard let self else { return }
            defer { self.inFlightKeys.remove(key) }
            struct ReqWrap: Encodable { let request: SessionAttachmentRequest }
            guard let resp: SessionAttachmentResponse = try? await store.rpc(
                "sessions.attachmentRead",
                args: ReqWrap(request: request), at: sourceRevision
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
    public var onFocusStateChanged: (() -> Void)?
    public var quoteBackgroundColor: NSColor?
    public var quoteAccentColor: NSColor?
    private var ownedTextStorage: NSTextStorage?

    override public init(frame frameRect: NSRect, textContainer container: NSTextContainer?) {
        if let container {
            self.ownedTextStorage = container.layoutManager?.textStorage
            super.init(frame: frameRect, textContainer: container)
        } else {
            // Explicit TextKit 1 stack so accessing `layoutManager` inside `drawBackground`
            // never triggers a mid-draw TextKit 2 -> TextKit 1 downgrade that blanks glyphs.
            let storage = NSTextStorage()
            let manager = NSLayoutManager()
            let builtContainer = NSTextContainer(
                size: NSSize(width: max(1, frameRect.width), height: CGFloat.greatestFiniteMagnitude)
            )
            builtContainer.lineFragmentPadding = 0
            manager.addTextContainer(builtContainer)
            storage.addLayoutManager(manager)
            self.ownedTextStorage = storage
            super.init(frame: frameRect, textContainer: builtContainer)
        }
        configureReaderTextView()
    }

    override public convenience init(frame frameRect: NSRect) {
        self.init(frame: frameRect, textContainer: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    public convenience init() {
        self.init(frame: NSRect(x: 0, y: 0, width: 200, height: 24), textContainer: nil)
    }

    public func applyAttributedText(_ attributed: NSAttributedString, width: CGFloat? = nil) {
        if let width, width > 1 {
            textContainer?.containerSize = NSSize(
                width: max(1, width - textContainerInset.width * 2),
                height: CGFloat.greatestFiniteMagnitude
            )
        }
        textStorage?.setAttributedString(attributed)
        if let manager = layoutManager, let container = textContainer {
            manager.ensureLayout(for: container)
        }
        needsDisplay = true
    }

    public func measuredUsedHeight() -> CGFloat {
        guard let manager = layoutManager, let container = textContainer else { return 22 }
        manager.ensureLayout(for: container)
        return ceil(manager.usedRect(for: container).height)
    }

    private func configureReaderTextView() {
        minSize = .zero
        maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        isEditable = false
        isSelectable = true
        drawsBackground = false
        isRichText = true
        importsGraphics = true
        linkTextAttributes = [.cursor: NSCursor.pointingHand]
        textContainerInset = .zero
        textContainer?.lineFragmentPadding = 0
        textContainer?.widthTracksTextView = true
        textContainer?.heightTracksTextView = false
        textContainer?.containerSize = NSSize(
            width: max(1, bounds.width - textContainerInset.width * 2),
            height: CGFloat.greatestFiniteMagnitude
        )
        isVerticallyResizable = false
        isHorizontallyResizable = false
    }

    override public func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        if newSize.width > 1, let container = textContainer {
            container.containerSize = NSSize(
                width: max(1, newSize.width - textContainerInset.width * 2),
                height: CGFloat.greatestFiniteMagnitude
            )
            layoutManager?.ensureLayout(for: container)
        }
    }

    override public func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFocusStateChanged?() }
        return ok
    }

    override public func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { onFocusStateChanged?() }
        return ok
    }

    override public func writeSelection(to pboard: NSPasteboard, types: [NSPasteboard.PasteboardType]) -> Bool {
        let range = selectedRange()
        guard range.length > 0, let storage = textStorage, NSMaxRange(range) <= storage.length else {
            return super.writeSelection(to: pboard, types: types)
        }
        let raw = (storage.string as NSString).substring(with: range)
        let cleaned = raw
            .replacingOccurrences(of: "\u{FFFC}", with: "")
            .replacingOccurrences(of: "\u{202F}", with: "")
        pboard.clearContents()
        return pboard.setString(cleaned, forType: .string)
    }

    private static func horizontalRoundedRectPath(
        _ rect: NSRect,
        leftRadius lRaw: CGFloat,
        rightRadius rRaw: CGFloat
    ) -> NSBezierPath {
        let maxR = min(rect.width * 0.5, rect.height * 0.5)
        let lR = max(0, min(lRaw, maxR))
        let rR = max(0, min(rRaw, maxR))
        if abs(lR - rR) < 0.01 {
            return lR > 0.01
                ? NSBezierPath(roundedRect: rect, xRadius: lR, yRadius: lR)
                : NSBezierPath(rect: rect)
        }
        let path = NSBezierPath()
        path.move(to: NSPoint(x: rect.minX + lR, y: rect.minY))
        path.line(to: NSPoint(x: rect.maxX - rR, y: rect.minY))
        if rR > 0.01 {
            path.appendArc(
                withCenter: NSPoint(x: rect.maxX - rR, y: rect.minY + rR),
                radius: rR,
                startAngle: 270,
                endAngle: 360
            )
        }
        path.line(to: NSPoint(x: rect.maxX, y: rect.maxY - rR))
        if rR > 0.01 {
            path.appendArc(
                withCenter: NSPoint(x: rect.maxX - rR, y: rect.maxY - rR),
                radius: rR,
                startAngle: 0,
                endAngle: 90
            )
        }
        path.line(to: NSPoint(x: rect.minX + lR, y: rect.maxY))
        if lR > 0.01 {
            path.appendArc(
                withCenter: NSPoint(x: rect.minX + lR, y: rect.maxY - lR),
                radius: lR,
                startAngle: 90,
                endAngle: 180
            )
        }
        path.line(to: NSPoint(x: rect.minX, y: rect.minY + lR))
        if lR > 0.01 {
            path.appendArc(
                withCenter: NSPoint(x: rect.minX + lR, y: rect.minY + lR),
                radius: lR,
                startAngle: 180,
                endAngle: 270
            )
        }
        path.close()
        return path
    }

    override public func drawBackground(in rect: NSRect) {
        if let storage = textStorage,
           storage.length > 0,
           let manager = layoutManager,
           let container = textContainer
        {
            let origin = textContainerOrigin
            let fullRange = NSRange(location: 0, length: storage.length)

            // 1) Blockquote card (0 10px 10px 0 radius + 3pt solid --a left bar)
            storage.enumerateAttribute(.agentHubBlockQuote, in: fullRange, options: []) { value, charRange, _ in
                guard (value as? Bool) == true, charRange.length > 0 else { return }
                let glyphRange = manager.glyphRange(forCharacterRange: charRange, actualCharacterRange: nil)
                let bbox = manager.boundingRect(forGlyphRange: glyphRange, in: container)
                guard bbox.height > 0 else { return }
                let bgRect = NSRect(
                    x: origin.x,
                    y: origin.y + bbox.minY - 3,
                    width: max(20, bounds.width - origin.x),
                    height: bbox.height + 6
                )
                if let fill = quoteBackgroundColor {
                    fill.setFill()
                    Self.horizontalRoundedRectPath(bgRect, leftRadius: 0, rightRadius: 10).fill()
                }
                if let accent = quoteAccentColor {
                    accent.setFill()
                    NSRect(x: bgRect.minX, y: bgRect.minY, width: 3, height: bgRect.height).fill()
                }
            }

            // 2) Inline `code` pills (6pt end-cap / 2.5pt soft-wrap seam --h background, per-line-fragment)
            let unselected = NSRange(location: NSNotFound, length: 0)
            let nsText = storage.string as NSString
            let viewBounds = bounds
            storage.enumerateAttribute(.agentHubInlineCode, in: fullRange, options: []) { value, charRange, _ in
                guard let pillColor = value as? NSColor, charRange.length > 0 else { return }
                let glyphRange = manager.glyphRange(forCharacterRange: charRange, actualCharacterRange: nil)
                guard glyphRange.length > 0 else { return }

                // Resolve canonical monospace metrics once per inline-code attribute span so NSTextStorage
                // font fallback (Menlo on U+202F, SF Mono on ASCII, PingFang on CJK) never causes pill height
                // or vertical center jumps across wrapped line fragments of the same `code` span.
                let spanStartIdx = min(nsText.length - 1, charRange.location)
                let rawSpanFont = storage.attribute(.font, at: spanStartIdx, effectiveRange: nil) as? NSFont
                let font = DesignFonts.nsMonoFont(size: rawSpanFont?.pointSize ?? 12, weight: .regular)
                let para = storage.attribute(.paragraphStyle, at: spanStartIdx, effectiveRange: nil) as? NSParagraphStyle
                let desiredPillH = max(16, ceil(font.ascender - font.descender) + 5)

                var spanHasNonBlankCodeChar = false
                for chIdx in charRange.location ..< min(nsText.length, NSMaxRange(charRange)) {
                    let cu = nsText.character(at: chIdx)
                    if cu != 0x20 && cu != 0x09 && cu != 0x0A && cu != 0x0D && cu != 0x202F && cu != 0x00A0 {
                        spanHasNonBlankCodeChar = true
                        break
                    }
                }

                pillColor.setFill()
                manager.enumerateLineFragments(forGlyphRange: glyphRange) { lineRect, _, lineContainer, lineGlyphRange, _ in
                    guard lineContainer === container else { return }
                    var subGlyphRange = NSIntersectionRange(glyphRange, lineGlyphRange)
                    guard subGlyphRange.length > 0 else { return }

                    let wrappedFromPrev = subGlyphRange.location > glyphRange.location
                    let wrapsToNext = NSMaxRange(subGlyphRange) < NSMaxRange(glyphRange)

                    if spanHasNonBlankCodeChar {
                        while wrappedFromPrev, subGlyphRange.length > 0 {
                            let chIdx = manager.characterIndexForGlyph(at: subGlyphRange.location)
                            guard chIdx < nsText.length else { break }
                            let cu = nsText.character(at: chIdx)
                            if cu == 0x20 || cu == 0x09 {
                                subGlyphRange.location += 1
                                subGlyphRange.length -= 1
                            } else {
                                break
                            }
                        }
                        while subGlyphRange.length > 0 {
                            let lastGlyph = NSMaxRange(subGlyphRange) - 1
                            let chIdx = manager.characterIndexForGlyph(at: lastGlyph)
                            guard chIdx < nsText.length else { break }
                            let cu = nsText.character(at: chIdx)
                            if (wrapsToNext && (cu == 0x20 || cu == 0x09)) || cu == 0x0A || cu == 0x0D {
                                subGlyphRange.length -= 1
                            } else {
                                break
                            }
                        }
                        guard subGlyphRange.length > 0 else { return }

                        if wrappedFromPrev || wrapsToNext {
                            var hasVisibleGlyph = false
                            for g in subGlyphRange.location ..< NSMaxRange(subGlyphRange) {
                                let chIdx = manager.characterIndexForGlyph(at: g)
                                if chIdx < nsText.length {
                                    let cu = nsText.character(at: chIdx)
                                    if cu != 0x20 && cu != 0x09 && cu != 0x0A && cu != 0x0D && cu != 0x202F && cu != 0x00A0 {
                                        hasVisibleGlyph = true
                                        break
                                    }
                                }
                            }
                            guard hasVisibleGlyph else { return }
                        }
                    } else if wrappedFromPrev {
                        return
                    }

                    let nominalLineH: CGFloat = if let para, para.maximumLineHeight > 0 {
                        min(lineRect.height, para.maximumLineHeight)
                    } else if let para, para.minimumLineHeight > 0 {
                        min(lineRect.height, max(para.minimumLineHeight, ceil(font.ascender - font.descender + font.leading)))
                    } else {
                        min(lineRect.height, ceil(font.ascender - font.descender + font.leading + 6))
                    }
                    let pillH = max(12, min(nominalLineH - 2.2, desiredPillH))
                    let baselineY = lineRect.minY + manager.location(forGlyphAt: subGlyphRange.location).y
                    let glyphMidY = baselineY - (font.ascender + font.descender) * 0.5
                    var pillMinY = origin.y + glyphMidY - pillH * 0.5
                    if viewBounds.height >= pillH {
                        pillMinY = max(0, min(viewBounds.maxY - pillH, pillMinY))
                    }

                    let leftPad: CGFloat = wrappedFromPrev ? 2.5 : 0
                    let rightPad: CGFloat = wrapsToNext ? 2.5 : 0
                    let leftRadius: CGFloat = wrappedFromPrev ? 2.5 : 6
                    let rightRadius: CGFloat = wrapsToNext ? 2.5 : 6

                    manager.enumerateEnclosingRects(
                        forGlyphRange: subGlyphRange,
                        withinSelectedGlyphRange: unselected,
                        in: container
                    ) { sliceRect, _ in
                        guard sliceRect.width > 1, sliceRect.height > 1 else { return }
                        let minX = max(0, origin.x + sliceRect.minX - leftPad)
                        let maxX = min(viewBounds.maxX, origin.x + sliceRect.maxX + rightPad)
                        guard maxX - minX > 1 else { return }
                        let pillRect = NSRect(
                            x: minX,
                            y: pillMinY,
                            width: maxX - minX,
                            height: pillH
                        )
                        Self.horizontalRoundedRectPath(
                            pillRect,
                            leftRadius: leftRadius,
                            rightRadius: rightRadius
                        ).fill()
                    }
                }
            }
        }
        // Draw selection highlight AFTER blockquote & inline-code backgrounds so selected text stays highlighted!
        super.drawBackground(in: rect)
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
