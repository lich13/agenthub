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
            case let .item(block, _, _):
                if block.subagent != nil
                    || ConversationViewModel.isHistoryCollapsedBlock(block)
                    || ConversationViewModel.isPlanBlock(block)
                    || ConversationViewModel.isQuestionBlock(block)
                    || ConversationViewModel.isQuestionResultBlock(block)
                {
                    return .passthrough
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
                let aliases = group.mergedIds + group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(
                    id: group.id,
                    timelineId: group.id,
                    timelineAliases: aliases,
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
        attachmentsAsNamesOnly: Bool = false,
        fallbackTitle: String = ""
    ) -> [ConversationRowItem] {
        let entries = ClaudeProcessGroupingEngine.groupClaudeProcesses(events, recordedOnly: recordedOnly)
        var rows: [ConversationRowItem] = []

        for entry in entries {
            switch entry {
            case let .process(proc):
                let innerCmdAliases = proc.entries.flatMap { sub -> [String] in
                    switch sub {
                    case let .item(_, key, sids): [key] + sids
                    case let .metadata(m): [m.id] + m.sourceIds
                    case let .group(g): [g.id] + g.commands.flatMap { [$0.id] + $0.sourceIds }
                    }
                }
                rows.append(ConversationRowItem(
                    id: proc.id,
                    timelineId: proc.id,
                    timelineAliases: ProcessGroupingEngine.uniqueStrings(proc.sourceIds + innerCmdAliases),
                    kind: .claudeProcessGroup(proc, title: fallbackTitle)
                ))
            case let .metadata(meta):
                rows.append(ConversationRowItem(
                    id: meta.id,
                    timelineId: meta.id,
                    timelineAliases: meta.sourceIds,
                    kind: .claudeMetadataGroup(meta, title: fallbackTitle)
                ))
            case let .group(group):
                let aliases = group.mergedIds + group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(id: group.id, timelineId: group.id, timelineAliases: aliases, kind: .executionGroup(group)))
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
                let aliases = group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(id: group.id, timelineId: group.id, timelineAliases: aliases, kind: .executionGroup(group)))
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

    public static func buildProviderRows(events: [ProviderHistoryEvent], providerLabel: String, sessionRunning: Bool = false) -> [ConversationRowItem] {
        let prefix = providerLabel == "Antigravity" ? "antigravity" : (providerLabel == "Grok Bot" ? "grokbot" : "dsh")
        let grouped = ExecutionGroupingEngine.groupProviderEvents(events, provider: providerLabel)
        let folded = ProcessGroupingEngine.foldProcessSequence(prefix: prefix, items: grouped) { entry in
            switch entry {
            case let .group(group):
                return .step(.execution(group: group, turnId: group.turnId))
            case let .item(ev, key, sourceIds):
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
                let aliases = group.commands.flatMap { ["group:\($0.id)", $0.id] + $0.sourceIds }
                rows.append(ConversationRowItem(id: group.id, timelineId: group.id, timelineAliases: aliases, kind: .executionGroup(group)))
            case let .passthrough(entry):
                guard case let .item(ev, key, sourceIds) = entry else { continue }
                if ev.kind == "user_message" {
                    rows.append(ConversationRowItem(
                        id: key,
                        timelineId: ev.id,
                        timelineAliases: sourceIds,
                        kind: .user(message: ev.userMessage, rawText: ev.text ?? "", attachmentsAsNamesOnly: false)
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

    private static func rightRoundedRectPath(_ rect: NSRect, rightRadius r: CGFloat) -> NSBezierPath {
        let radius = min(r, min(rect.width * 0.5, rect.height * 0.5))
        let path = NSBezierPath()
        path.move(to: NSPoint(x: rect.minX, y: rect.minY))
        path.line(to: NSPoint(x: rect.maxX - radius, y: rect.minY))
        path.appendArc(
            withCenter: NSPoint(x: rect.maxX - radius, y: rect.minY + radius),
            radius: radius,
            startAngle: 270,
            endAngle: 360
        )
        path.line(to: NSPoint(x: rect.maxX, y: rect.maxY - radius))
        path.appendArc(
            withCenter: NSPoint(x: rect.maxX - radius, y: rect.maxY - radius),
            radius: radius,
            startAngle: 0,
            endAngle: 90
        )
        path.line(to: NSPoint(x: rect.minX, y: rect.maxY))
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
                    Self.rightRoundedRectPath(bgRect, rightRadius: 10).fill()
                }
                if let accent = quoteAccentColor {
                    accent.setFill()
                    NSRect(x: bgRect.minX, y: bgRect.minY, width: 3, height: bgRect.height).fill()
                }
            }

            // 2) Inline `code` pills (6pt rounded --h background, 20pt vertical height so adjacent lines never collide)
            let unselected = NSRange(location: NSNotFound, length: 0)
            storage.enumerateAttribute(.agentHubInlineCode, in: fullRange, options: []) { value, charRange, _ in
                guard let pillColor = value as? NSColor, charRange.length > 0 else { return }
                let glyphRange = manager.glyphRange(forCharacterRange: charRange, actualCharacterRange: nil)
                pillColor.setFill()
                manager.enumerateEnclosingRects(
                    forGlyphRange: glyphRange,
                    withinSelectedGlyphRange: unselected,
                    in: container
                ) { sliceRect, _ in
                    guard sliceRect.width > 1, sliceRect.height > 1 else { return }
                    let pillH = min(sliceRect.height - 3, 21)
                    let pillRect = NSRect(
                        x: origin.x + sliceRect.minX,
                        y: origin.y + sliceRect.midY - pillH * 0.5,
                        width: sliceRect.width,
                        height: pillH
                    )
                    NSBezierPath(roundedRect: pillRect, xRadius: 6, yRadius: 6).fill()
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
