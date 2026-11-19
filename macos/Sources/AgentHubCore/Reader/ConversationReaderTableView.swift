import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

@MainActor
public final class ConversationTableCoordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    public weak var scrollView: NSScrollView?
    public weak var tableView: NSTableView?
    public weak var column: NSTableColumn?

    public var store: RuntimeConnectionStore
    public var disclosureStore: ActivityDisclosureStore
    public let attachmentViewport = AttachmentViewportManager()

    public private(set) var rows: [ConversationRowItem] = []
    public var scheme: ColorSchemeKind = .light
    public var workspace: WorkspaceId = .codex
    public var sessionScopeKey: String = "local::codex"
    public var sessionProvider: SessionProvider = .codex
    public var sessionKey: String = ""
    public var rootThreadId: String?
    public var cwd: String?
    public var hasTimelineRail: Bool = true
    public var isSubagent: Bool = false

    public var onLoadOlderRequested: (() -> Void)?
    public var onShowAllHistory: (() -> Void)?
    public var onOpenSubagent: ((SubagentActivity) -> Void)?
    public var onPreviewImage: ((String, NSImage) -> Void)?
    public var onCopyFeedback: ((String) -> Void)?
    public var onActiveTimelineChanged: ((String?) -> Void)?

    private var highlightedTimelineId: String?
    private var highlightClearTask: Task<Void, Never>?
    private var isRestoringScroll = false
    private var lastLaidOutWidth: CGFloat = 0

    public struct ScrollAnchorSnapshot: Equatable, Sendable {
        public let timelineId: String
        public let deltaY: CGFloat
    }

    public init(
        store: RuntimeConnectionStore,
        disclosureStore: ActivityDisclosureStore = .shared
    ) {
        self.store = store
        self.disclosureStore = disclosureStore
        super.init()
        attachmentViewport.onImageUpdated = { [weak self] _ in
            self?.tableView?.reloadData()
        }
    }

    public var palette: DesignPalette {
        DesignPalette.resolve(scheme: scheme, workspace: workspace)
    }

    public func contentColumnWidth(for totalWidth: CGFloat) -> CGFloat {
        let insets = LayoutMetrics.readingHorizontalInsets(
            availableWidth: totalWidth,
            hasTimelineRail: hasTimelineRail,
            isSubagent: isSubagent
        )
        return max(160, totalWidth - insets.leading - insets.trailing)
    }

    // MARK: - 0.0pt Same-Frame Scroll Anchor Capture & Restore

    public func captureScrollAnchor() -> ScrollAnchorSnapshot? {
        guard let tableView, let clipView = scrollView?.contentView, !rows.isEmpty else {
            return nil
        }
        let viewportTop = clipView.bounds.origin.y
        let visibleRange = tableView.rows(in: clipView.bounds)
        guard visibleRange.length > 0 else { return nil }
        for idx in visibleRange.location ..< min(rows.count, visibleRange.location + visibleRange.length) {
            let rect = tableView.rect(ofRow: idx)
            if rect.maxY > viewportTop {
                return ScrollAnchorSnapshot(
                    timelineId: rows[idx].timelineId,
                    deltaY: rect.minY - viewportTop
                )
            }
        }
        return nil
    }

    public func isNearBottom(threshold: CGFloat = 96) -> Bool {
        guard let tableView, let clipView = scrollView?.contentView else { return true }
        return ConversationViewModel.shouldAutoFollow(
            scrollTop: clipView.bounds.origin.y,
            clientHeight: clipView.bounds.height,
            scrollHeight: tableView.frame.height,
            threshold: threshold
        )
    }

    public func updateRowsPreservingAnchor(
        _ nextRows: [ConversationRowItem],
        preferAutoFollowBottom: Bool = false,
        forceScrollToBottom: Bool = false
    ) {
        guard let tableView, let clipView = scrollView?.contentView else {
            rows = nextRows
            return
        }
        let wasEmpty = rows.isEmpty
        let shouldFollow = forceScrollToBottom || (preferAutoFollowBottom && (wasEmpty || isNearBottom()))
        let anchor = shouldFollow ? nil : captureScrollAnchor()

        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0
            context.allowsImplicitAnimation = false
            self.rows = nextRows
            tableView.reloadData()
            tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0 ..< nextRows.count))
            tableView.tile()
            tableView.layoutSubtreeIfNeeded()

            let minY = -(self.scrollView?.contentInsets.top ?? 0)
            if shouldFollow, !nextRows.isEmpty {
                let targetY = max(minY, tableView.frame.height - clipView.bounds.height)
                clipView.setBoundsOrigin(NSPoint(x: 0, y: targetY))
                self.scrollView?.reflectScrolledClipView(clipView)
            } else if let anchor,
                      let newIdx = nextRows.firstIndex(where: { $0.timelineId == anchor.timelineId })
            {
                let newRect = tableView.rect(ofRow: newIdx)
                let restoredY = max(minY, newRect.minY - anchor.deltaY)
                clipView.setBoundsOrigin(NSPoint(x: 0, y: restoredY))
                self.scrollView?.reflectScrolledClipView(clipView)
            }
        }
        isRestoringScroll = false
        refreshViewportAttachmentsAndTimeline()
    }

    public func restoreExplicitAnchor(_ anchor: ScrollAnchorSnapshot) {
        guard let tableView, let clipView = scrollView?.contentView,
              let idx = rows.firstIndex(where: { $0.timelineId == anchor.timelineId })
        else { return }
        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0
            ctx.allowsImplicitAnimation = false
            tableView.tile()
            tableView.layoutSubtreeIfNeeded()
            let minY = -(self.scrollView?.contentInsets.top ?? 0)
            let newRect = tableView.rect(ofRow: idx)
            clipView.setBoundsOrigin(NSPoint(x: 0, y: max(minY, newRect.minY - anchor.deltaY)))
            self.scrollView?.reflectScrolledClipView(clipView)
        }
        isRestoringScroll = false
        refreshViewportAttachmentsAndTimeline()
    }

    public func toggleDisclosurePreservingAnchor(stateKey: String, currentlyOpen: Bool) {
        let anchor = captureScrollAnchor()
        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0
            context.allowsImplicitAnimation = false
            self.disclosureStore.setOpen(scope: self.sessionScopeKey, stateKey: stateKey, isOpen: !currentlyOpen)
            if let tableView = self.tableView {
                tableView.reloadData()
                tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0 ..< self.rows.count))
                tableView.tile()
                tableView.layoutSubtreeIfNeeded()
                let minY = -(self.scrollView?.contentInsets.top ?? 0)
                if let anchor,
                   let clipView = self.scrollView?.contentView,
                   let idx = self.rows.firstIndex(where: { $0.timelineId == anchor.timelineId })
                {
                    let newRect = tableView.rect(ofRow: idx)
                    let restoredY = max(minY, newRect.minY - anchor.deltaY)
                    clipView.setBoundsOrigin(NSPoint(x: 0, y: restoredY))
                    self.scrollView?.reflectScrolledClipView(clipView)
                }
            }
        }
        isRestoringScroll = false
    }

    // MARK: - "auto" Instant Scroll Jump + 1.8s --ab Highlight

    @discardableResult
    public func scrollToPositionKey(_ positionKey: String, reduceMotion: Bool = false) -> Bool {
        guard let tableView, let clipView = scrollView?.contentView else { return false }
        guard let rowIdx = rows.firstIndex(where: { $0.matches(positionKey: positionKey) }) else {
            return false
        }
        // Auto-open disclosure if jumping to a folded group/command
        let targetRow = rows[rowIdx]
        if case let .executionGroup(group) = targetRow.kind {
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: group.id, isOpen: true)
            for cmd in group.commands where TimelineViewModel.matchesTimelinePosition(positionKey, id: cmd.id, aliases: cmd.sourceIds) {
                disclosureStore.setOpen(scope: sessionScopeKey, stateKey: cmd.id, isOpen: true)
            }
            tableView.reloadData()
            tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integer: rowIdx))
            tableView.layoutSubtreeIfNeeded()
        }

        let rect = tableView.rect(ofRow: rowIdx)
        let maxScroll = max(0, tableView.frame.height - clipView.bounds.height)
        let targetY = max(0, min(maxScroll, rect.minY - 16))
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0
            ctx.allowsImplicitAnimation = false
            clipView.setBoundsOrigin(NSPoint(x: 0, y: targetY))
            self.scrollView?.reflectScrolledClipView(clipView)
        }

        if !reduceMotion {
            highlightedTimelineId = targetRow.timelineId
            tableView.reloadData(forRowIndexes: IndexSet(integer: rowIdx), columnIndexes: IndexSet(integer: 0))
            highlightClearTask?.cancel()
            highlightClearTask = Task { @MainActor [weak self] in
                try? await Task.sleep(nanoseconds: 1_800_000_000)
                guard let self, !Task.isCancelled else { return }
                self.highlightedTimelineId = nil
                self.tableView?.reloadData()
            }
        }
        refreshViewportAttachmentsAndTimeline()
        return true
    }

    // MARK: - Scroll Bounds Observation (±240pt Thumbnail Eviction, <=200pt Pagination, Timeline Active Tick)

    @objc public func scrollViewFrameDidChange(_ notification: Notification) {
        guard !isRestoringScroll, let scrollView, let tableView else { return }
        let w = scrollView.bounds.width
        guard w > 1, abs(w - lastLaidOutWidth) > 0.5 else { return }
        let wasZero = (lastLaidOutWidth == 0)
        let anchor = wasZero ? nil : captureScrollAnchor()
        lastLaidOutWidth = w
        column?.width = w
        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0
            ctx.allowsImplicitAnimation = false
            tableView.reloadData()
            tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0 ..< rows.count))
            tableView.tile()
            tableView.layoutSubtreeIfNeeded()
            let clipView = scrollView.contentView
            let minY = -scrollView.contentInsets.top
            if wasZero, !rows.isEmpty {
                let targetY = max(minY, tableView.frame.height - clipView.bounds.height)
                clipView.setBoundsOrigin(NSPoint(x: 0, y: targetY))
                scrollView.reflectScrolledClipView(clipView)
            } else if let anchor,
                      let idx = rows.firstIndex(where: { $0.timelineId == anchor.timelineId })
            {
                let rect = tableView.rect(ofRow: idx)
                clipView.setBoundsOrigin(NSPoint(x: 0, y: max(minY, rect.minY - anchor.deltaY)))
                scrollView.reflectScrolledClipView(clipView)
            }
        }
        isRestoringScroll = false
        refreshViewportAttachmentsAndTimeline()
    }

    @objc public func clipViewBoundsDidChange(_ notification: Notification) {
        guard !isRestoringScroll, let clipView = scrollView?.contentView else { return }
        if clipView.bounds.origin.y <= 200 {
            onLoadOlderRequested?()
        }
        refreshViewportAttachmentsAndTimeline()
    }

    public func refreshViewportAttachmentsAndTimeline() {
        guard let tableView, let clipView = scrollView?.contentView else { return }
        let expandedBounds = clipView.bounds.insetBy(dx: 0, dy: -AttachmentViewportManager.viewportMargin)
        let expandedRange = tableView.rows(in: expandedBounds)
        var inViewportAttachmentKeys = Set<String>()

        if expandedRange.length > 0 {
            for idx in expandedRange.location ..< min(rows.count, expandedRange.location + expandedRange.length) {
                let row = rows[idx]
                if case let .user(msg, _, namesOnly) = row.kind, !namesOnly, let attachments = msg?.attachments {
                    for att in attachments where att.kind == "image" && att.reason == nil {
                        let key = "\(sessionScopeKey):\(msg?.id ?? row.id):\(att.id)"
                        inViewportAttachmentKeys.insert(key)
                        if !sessionKey.isEmpty {
                            attachmentViewport.ensureLoaded(
                                key: key,
                                request: SessionAttachmentRequest(
                                    provider: sessionProvider,
                                    sessionKey: sessionKey,
                                    messageId: msg?.id ?? row.id,
                                    attachmentId: att.id,
                                    rootThreadId: rootThreadId
                                ),
                                store: store
                            )
                        }
                    }
                }
            }
        }
        attachmentViewport.evictOutside(visibleKeys: inViewportAttachmentKeys)

        // Compute active user timeline entry at viewport top
        var userAnchors: [(id: String, top: CGFloat)] = []
        for (idx, row) in rows.enumerated() {
            if case .user = row.kind {
                userAnchors.append((id: row.timelineId, top: tableView.rect(ofRow: idx).minY))
            }
        }
        let activeId = TimelineViewModel.activeUserTimelineId(
            anchors: userAnchors,
            viewportTop: clipView.bounds.origin.y + 28
        )
        onActiveTimelineChanged?(activeId)
    }

    // MARK: - NSTableViewDataSource & Delegate

    public func numberOfRows(in tableView: NSTableView) -> Int {
        rows.count
    }

    public func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        guard row >= 0, row < rows.count else { return 44 }
        let colWidth = contentColumnWidth(for: max(320, scrollView?.bounds.width ?? tableView.bounds.width))
        return max(32, ceil(measureRowHeight(rows[row], width: colWidth)))
    }

    public func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        false
    }

    public func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row >= 0, row < rows.count else { return nil }
        let identifier = NSUserInterfaceItemIdentifier("ConversationRowCell")
        let cell = (tableView.makeView(withIdentifier: identifier, owner: nil) as? ConversationRowCellView)
            ?? ConversationRowCellView(identifier: identifier)
        let totalWidth = max(320, scrollView?.bounds.width ?? tableView.bounds.width)
        let insets = LayoutMetrics.readingHorizontalInsets(
            availableWidth: totalWidth,
            hasTimelineRail: hasTimelineRail,
            isSubagent: isSubagent
        )
        let item = rows[row]
        cell.configure(
            item: item,
            leadingInset: insets.leading,
            trailingInset: insets.trailing,
            contentWidth: contentColumnWidth(for: totalWidth),
            highlighted: highlightedTimelineId == item.timelineId,
            coordinator: self
        )
        return cell
    }

    // MARK: - Deterministic Row Height Calculation

    public func measureRowHeight(_ row: ConversationRowItem, width: CGFloat) -> CGFloat {
        switch row.kind {
        case let .user(msg, rawText, namesOnly):
            let body = msg?.text ?? rawText
            let proj = UserContextEngine.projectUserMessage(body, contextBlocks: msg?.contextBlocks)
            let segs = UserContextEngine.userMessageSegments(proj.text, contextBlocks: proj.contextBlocks)
            var h: CGFloat = 8
            if let atts = msg?.attachments, !atts.isEmpty {
                h += namesOnly ? 34 : 104
            }
            if !proj.contextBlocks.isEmpty {
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: "\(row.timelineId):context", initiallyOpen: false)
                if open {
                    let ctxText = proj.contextBlocks.map { UserContextEngine.visibleContextText($0.text) }.joined(separator: "\n\n")
                    h += 34 + 8 + Self.measureMonospaceBoxHeight(text: ctxText, width: min(width, 620))
                } else {
                    h += 34
                }
            }
            let bubbleWidth = max(120, min(width * 0.82, width - 32))
            let para = NSMutableParagraphStyle()
            para.minimumLineHeight = 24.75 // 15pt * 1.65
            para.maximumLineHeight = 24.75
            let attrs: [NSAttributedString.Key: Any] = [
                .font: DesignFonts.nsBodyFont(size: 15),
                .paragraphStyle: para,
            ]
            for seg in segs {
                let text: String = switch seg {
                case let .text(_, t): t
                case let .reply(_, q, a): q.isEmpty ? a : "\(q)\n\(a)"
                }
                let attr = NSAttributedString(string: text, attributes: attrs)
                h += ReaderSelectableTextView.measureHeight(attributed: attr, width: bubbleWidth - 28) + 24
            }
            return max(44, h)

        case let .assistant(markdown, foldInstructions):
            return measureMarkdownSegmentsHeight(ownerId: row.id, markdown: markdown, foldInstructions: foldInstructions, width: width) + 12

        case let .executionGroup(group):
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: group.id,
                aliases: group.mergedIds,
                initiallyOpen: group.initiallyOpen
            )
            if !isOpen { return 40 }
            var h: CGFloat = 48
            for cmd in group.commands {
                let cmdOpen = disclosureStore.isOpen(
                    scope: sessionScopeKey,
                    stateKey: cmd.id,
                    aliases: cmd.sourceIds,
                    initiallyOpen: !cmd.instructionFile && (cmd.status == .running || cmd.status == .failed)
                )
                if cmdOpen {
                    let detail = cmd.sections.map { "\($0.label)\n\($0.text)" }.joined(separator: "\n\n")
                    h += 36 + 8 + Self.measureMonospaceBoxHeight(text: detail.isEmpty ? "暂无输出" : detail, width: width - 16)
                } else {
                    h += 36
                }
            }
            return h

        case let .thinking(_, text, running):
            let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: row.id, initiallyOpen: running)
            if !open { return 38 }
            return 48 + measureMarkdownSegmentsHeight(ownerId: row.id, markdown: text, foldInstructions: false, width: max(120, width - 28))

        case let .plan(markdown, _, _):
            return 56 + measureMarkdownSegmentsHeight(ownerId: row.id, markdown: markdown, foldInstructions: false, width: max(120, width - 32))

        case let .question(_, questions):
            return CGFloat(56 + questions.count * 76)

        case let .questionResult(_, questions, answers):
            return CGFloat(64 + questions.count * 52 + answers.count * 32)

        case .subagent:
            return 54

        case .historyCollapse:
            return 52

        case .systemNotice:
            return 42
        }
    }

    public static func measureMonospaceBoxHeight(text: String, width: CGFloat) -> CGFloat {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
        let attr = NSAttributedString(
            string: cleaned.isEmpty ? " " : cleaned,
            attributes: [.font: DesignFonts.nsMonoFont(size: 12)]
        )
        let measured = ReaderSelectableTextView.measureHeight(attributed: attr, width: max(80, width - 24))
        return min(220, max(48, measured + 18))
    }

    private func measureMarkdownSegmentsHeight(ownerId: String, markdown: String, foldInstructions: Bool, width: CGFloat) -> CGFloat {
        let segs = GfmLayoutCache.shared.getOrRender(
            markdown: markdown,
            foldInstructions: foldInstructions,
            scheme: scheme,
            workspace: workspace,
            transport: store.transport
        )
        var total: CGFloat = 0
        for seg in segs {
            switch seg {
            case let .prose(attr, isQuote):
                let w = isQuote ? max(100, width - 32) : width
                total += ReaderSelectableTextView.measureHeight(attributed: attr, width: w) + (isQuote ? 24 : 8)
            case let .codeBlock(_, code):
                let lines = max(1, code.components(separatedBy: "\n").count)
                total += CGFloat(min(lines, 18)) * 20.8 + 54 + 24
            case let .table(_, _, rows):
                total += CGFloat(rows.count + 1) * 34 + 28
            case .thematicBreak:
                total += 21
            case let .instructionsFold(id, text, _, _):
                let scopedKey = "\(ownerId):\(id)"
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: scopedKey, initiallyOpen: false)
                if open {
                    total += 42 + measureMarkdownSegmentsHeight(ownerId: scopedKey, markdown: text, foldInstructions: false, width: width)
                } else {
                    total += 38
                }
            }
        }
        return max(28, total)
    }
}

// MARK: - Reusable NSTableCellView Rendering Prose NSTextViews, Code Blocks, Tables & Disclosures

@MainActor
public final class ConversationRowCellView: NSTableCellView {
    private let containerStack = NSStackView()

    public init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier
        wantsLayer = true
        containerStack.orientation = .vertical
        containerStack.alignment = .leading
        containerStack.spacing = 8
        containerStack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(containerStack)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    public func configure(
        item: ConversationRowItem,
        leadingInset: CGFloat,
        trailingInset: CGFloat,
        contentWidth: CGFloat,
        highlighted: Bool,
        coordinator: ConversationTableCoordinator
    ) {
        containerStack.arrangedSubviews.forEach { v in
            containerStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        NSLayoutConstraint.deactivate(constraints)
        NSLayoutConstraint.activate([
            containerStack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: leadingInset),
            containerStack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -trailingInset),
            containerStack.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            containerStack.bottomAnchor.constraint(lessThanOrEqualTo: bottomAnchor, constant: -2),
        ])

        let palette = coordinator.palette
        layer?.cornerRadius = 10
        layer?.backgroundColor = highlighted
            ? palette.accentBorder.nsColor.withAlphaComponent(0.28).cgColor
            : NSColor.clear.cgColor

        switch item.kind {
        case let .user(msg, rawText, namesOnly):
            buildUserView(item: item, msg: msg, rawText: rawText, namesOnly: namesOnly, width: contentWidth, coordinator: coordinator)
        case let .assistant(markdown, foldInstructions):
            buildMarkdownViews(ownerId: item.id, markdown: markdown, foldInstructions: foldInstructions, width: contentWidth, showCopyReply: true, coordinator: coordinator)
        case let .executionGroup(group):
            buildExecutionGroupView(group: group, width: contentWidth, coordinator: coordinator)
        case let .thinking(title, text, running):
            buildThinkingView(id: item.id, title: title, text: text, running: running, width: contentWidth, coordinator: coordinator)
        case let .plan(markdown, statusText, fallbackTitle):
            buildPlanView(id: item.id, markdown: markdown, statusText: statusText, fallbackTitle: fallbackTitle, width: contentWidth, coordinator: coordinator)
        case let .question(header, questions):
            buildQuestionView(header: header, questions: questions, answers: nil, width: contentWidth, coordinator: coordinator)
        case let .questionResult(statusText, questions, answers):
            buildQuestionView(header: statusText, questions: questions, answers: answers, width: contentWidth, coordinator: coordinator)
        case let .subagent(sub, supported):
            buildSubagentRow(sub: sub, supported: supported, width: contentWidth, coordinator: coordinator)
        case let .historyCollapse(eyebrow, label, expanded):
            buildHistoryCollapseRow(eyebrow: eyebrow, label: label, expanded: expanded, width: contentWidth, coordinator: coordinator)
        case let .systemNotice(label, detail):
            buildSystemNoticeRow(label: label, detail: detail, width: contentWidth, palette: palette)
        }
    }

    private func buildUserView(
        item: ConversationRowItem,
        msg: UserMessageContent?,
        rawText: String,
        namesOnly: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let body = msg?.text ?? rawText
        let proj = UserContextEngine.projectUserMessage(body, contextBlocks: msg?.contextBlocks)
        let segs = UserContextEngine.userMessageSegments(proj.text, contextBlocks: proj.contextBlocks)

        let rightAlignRow = NSStackView()
        rightAlignRow.orientation = .vertical
        rightAlignRow.alignment = .trailing
        rightAlignRow.spacing = 6
        rightAlignRow.widthAnchor.constraint(equalToConstant: width).isActive = true

        if let attachments = msg?.attachments, !attachments.isEmpty {
            let attRow = NSStackView()
            attRow.orientation = .horizontal
            attRow.spacing = 8
            for att in attachments {
                if !namesOnly && att.kind == "image" && att.reason == nil {
                    let key = "\(coordinator.sessionScopeKey):\(msg?.id ?? item.id):\(att.id)"
                    let thumbBtn = ActionButton(frame: NSRect(x: 0, y: 0, width: 96, height: 96))
                    thumbBtn.wantsLayer = true
                    thumbBtn.layer?.cornerRadius = 10
                    thumbBtn.layer?.masksToBounds = true
                    thumbBtn.layer?.backgroundColor = palette.surface.nsColor.cgColor
                    thumbBtn.layer?.borderWidth = 1
                    thumbBtn.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
                    thumbBtn.widthAnchor.constraint(equalToConstant: 96).isActive = true
                    thumbBtn.heightAnchor.constraint(equalToConstant: 96).isActive = true
                    if let img = coordinator.attachmentViewport.image(for: key) {
                        thumbBtn.image = img
                        thumbBtn.imageScaling = .scaleProportionallyUpOrDown
                        thumbBtn.title = ""
                        thumbBtn.onTap = { [weak coordinator] in
                            coordinator?.onPreviewImage?(att.name, img)
                        }
                    } else {
                        thumbBtn.title = "图片"
                    }
                    attRow.addArrangedSubview(thumbBtn)
                } else {
                    let chip = NSTextField(labelWithString: "📎 \(att.name)")
                    chip.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
                    chip.textColor = palette.textSecondary.nsColor
                    attRow.addArrangedSubview(chip)
                }
            }
            rightAlignRow.addArrangedSubview(attRow)
        }

        if !proj.contextBlocks.isEmpty {
            let ctxKey = "\(item.timelineId):context"
            let open = coordinator.disclosureStore.isOpen(scope: coordinator.sessionScopeKey, stateKey: ctxKey, initiallyOpen: false)
            let toggle = makeDisclosureHeaderButton(
                title: "上下文 (\(proj.contextBlocks.count))",
                meta: open ? "收起" : "展开",
                open: open,
                width: min(width, 420),
                palette: palette
            ) { [weak coordinator] in
                coordinator?.toggleDisclosurePreservingAnchor(stateKey: ctxKey, currentlyOpen: open)
            }
            rightAlignRow.addArrangedSubview(toggle)
            if open {
                let text = proj.contextBlocks.map { UserContextEngine.visibleContextText($0.text) }.joined(separator: "\n\n")
                let box = makeMonospaceDetailBox(text: text, width: min(width, 620), palette: palette, coordinator: coordinator, linkifyFiles: false)
                rightAlignRow.addArrangedSubview(box)
            }
        }

        let para = NSMutableParagraphStyle()
        para.minimumLineHeight = 24.75 // 15pt * 1.65
        para.maximumLineHeight = 24.75
        let attrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsBodyFont(size: 15),
            .foregroundColor: palette.textPrimary.nsColor,
            .paragraphStyle: para,
        ]

        for seg in segs {
            let displayString: String = switch seg {
            case let .text(_, t): t
            case let .reply(_, q, a): q.isEmpty ? a : "▸ \(q)\n\(a)"
            }
            let bubble = NSView()
            bubble.wantsLayer = true
            bubble.layer?.cornerRadius = 16
            bubble.layer?.backgroundColor = palette.surface.nsColor.cgColor
            bubble.layer?.borderWidth = 1
            bubble.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

            let tv = ReaderSelectableTextView()
            let maxBubbleW = max(140, min(width * 0.82, width - 32))
            let attr = NSAttributedString(string: displayString, attributes: attrs)
            tv.textStorage?.setAttributedString(attr)
            let textW = min(maxBubbleW - 28, max(80, ceil(attr.size().width) + 8))
            let textH = ReaderSelectableTextView.measureHeight(attributed: attr, width: textW)
            tv.translatesAutoresizingMaskIntoConstraints = false
            bubble.addSubview(tv)
            NSLayoutConstraint.activate([
                tv.leadingAnchor.constraint(equalTo: bubble.leadingAnchor, constant: 14),
                tv.trailingAnchor.constraint(equalTo: bubble.trailingAnchor, constant: -14),
                tv.topAnchor.constraint(equalTo: bubble.topAnchor, constant: 10),
                tv.bottomAnchor.constraint(equalTo: bubble.bottomAnchor, constant: -10),
                tv.widthAnchor.constraint(equalToConstant: textW),
                tv.heightAnchor.constraint(equalToConstant: max(24, textH)),
            ])
            rightAlignRow.addArrangedSubview(bubble)
        }
        containerStack.addArrangedSubview(rightAlignRow)
    }

    private func buildMarkdownViews(
        ownerId: String,
        markdown: String,
        foldInstructions: Bool,
        width: CGFloat,
        showCopyReply: Bool,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let cleanedSource = VisibleMarkdownEngine.visibleMarkdown(markdown)
        let segs = GfmLayoutCache.shared.getOrRender(
            markdown: markdown,
            foldInstructions: foldInstructions,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            transport: coordinator.store.transport
        )

        for seg in segs {
            switch seg {
            case let .prose(attr, isQuote):
                if isQuote {
                    let quoteWrap = NSView()
                    quoteWrap.wantsLayer = true
                    quoteWrap.layer?.backgroundColor = palette.surface.nsColor.cgColor
                    quoteWrap.layer?.cornerRadius = 10
                    let bar = NSView()
                    bar.wantsLayer = true
                    bar.layer?.backgroundColor = palette.accent.nsColor.cgColor
                    bar.translatesAutoresizingMaskIntoConstraints = false

                    let innerW = max(100, width - 32)
                    let tv = makeProseTextView(attr: attr, width: innerW, coordinator: coordinator)
                    quoteWrap.addSubview(bar)
                    quoteWrap.addSubview(tv)
                    NSLayoutConstraint.activate([
                        quoteWrap.widthAnchor.constraint(equalToConstant: width),
                        bar.leadingAnchor.constraint(equalTo: quoteWrap.leadingAnchor),
                        bar.topAnchor.constraint(equalTo: quoteWrap.topAnchor),
                        bar.bottomAnchor.constraint(equalTo: quoteWrap.bottomAnchor),
                        bar.widthAnchor.constraint(equalToConstant: 3),
                        tv.leadingAnchor.constraint(equalTo: quoteWrap.leadingAnchor, constant: 14),
                        tv.trailingAnchor.constraint(equalTo: quoteWrap.trailingAnchor, constant: -14),
                        tv.topAnchor.constraint(equalTo: quoteWrap.topAnchor, constant: 8),
                        tv.bottomAnchor.constraint(equalTo: quoteWrap.bottomAnchor, constant: -8),
                    ])
                    containerStack.addArrangedSubview(quoteWrap)
                } else {
                    let tv = makeProseTextView(attr: attr, width: width, coordinator: coordinator)
                    containerStack.addArrangedSubview(tv)
                }

            case let .codeBlock(lang, code):
                containerStack.addArrangedSubview(makeCodeBlockView(lang: lang, code: code, width: width, coordinator: coordinator))

            case let .table(_, headers, rows):
                containerStack.addArrangedSubview(makeTableView(headers: headers, rows: rows, width: width, palette: palette, coordinator: coordinator))

            case .thematicBreak:
                let rule = NSView()
                rule.wantsLayer = true
                rule.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                rule.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    rule.widthAnchor.constraint(equalToConstant: width),
                    rule.heightAnchor.constraint(equalToConstant: 1),
                ])
                containerStack.addArrangedSubview(rule)

            case let .instructionsFold(id, text, lines, bytes):
                let scopedKey = "\(ownerId):\(id)"
                let open = coordinator.disclosureStore.isOpen(scope: coordinator.sessionScopeKey, stateKey: scopedKey, initiallyOpen: false)
                let btn = makeDisclosureHeaderButton(
                    title: "AGENTS.md",
                    meta: "\(lines) 行 · \(bytes) 字节",
                    open: open,
                    width: width,
                    palette: palette
                ) { [weak coordinator] in
                    coordinator?.toggleDisclosurePreservingAnchor(stateKey: scopedKey, currentlyOpen: open)
                }
                containerStack.addArrangedSubview(btn)
                if open {
                    buildMarkdownViews(
                        ownerId: scopedKey,
                        markdown: text,
                        foldInstructions: false,
                        width: width,
                        showCopyReply: false,
                        coordinator: coordinator
                    )
                }
            }
        }

        if showCopyReply {
            let copyBtn = ActionButton()
            copyBtn.title = "复制答复"
            copyBtn.font = DesignFonts.nsBodyFont(size: 11, weight: .medium)
            copyBtn.contentTintColor = palette.textMuted.nsColor
            copyBtn.onTap = { [weak coordinator] in
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(cleanedSource, forType: .string)
                coordinator?.onCopyFeedback?("已复制答复")
            }
            containerStack.addArrangedSubview(copyBtn)
        }
    }

    private func handleActivatedHref(_ href: String, coordinator: ConversationTableCoordinator) -> Bool {
        if let fileTarget = FileLinksAndPlanExport.fileLinkTarget(href, cwd: coordinator.cwd) {
            if coordinator.store.target == .local && fileTarget.resolved {
                let targetPath = fileTarget.path
                Task {
                    try? await FileOpenerService.revealPaths([targetPath])
                }
            } else {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(fileTarget.path, forType: .string)
                coordinator.onCopyFeedback?(
                    fileTarget.resolved ? "已复制路径" : "已复制原路径；缺少工作目录，无法定位。"
                )
            }
            return true
        }
        if let url = URL(string: href),
           let scheme = url.scheme?.lowercased(),
           scheme == "https" || scheme == "http" || scheme == "mailto"
        {
            NSWorkspace.shared.open(url)
            return true
        }
        return false
    }

    private func makeProseTextView(
        attr: NSAttributedString,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) -> ReaderSelectableTextView {
        let tv = ReaderSelectableTextView()
        tv.textStorage?.setAttributedString(attr)
        let h = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: width))
        tv.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            tv.widthAnchor.constraint(equalToConstant: width),
            tv.heightAnchor.constraint(equalToConstant: h),
        ])
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        return tv
    }

    private func makeCodeBlockView(
        lang: String?,
        code: String,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let palette = coordinator.palette
        let box = NSView()
        box.wantsLayer = true
        box.layer?.cornerRadius = 12
        box.layer?.backgroundColor = palette.codeBg.nsColor.cgColor
        box.layer?.borderWidth = 1
        box.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let langLabel = NSTextField(labelWithString: lang ?? "code")
        langLabel.font = DesignFonts.nsMonoFont(size: 11, weight: .medium)
        langLabel.textColor = palette.textMuted.nsColor
        langLabel.translatesAutoresizingMaskIntoConstraints = false

        let copyBtn = ActionButton()
        copyBtn.title = "复制"
        copyBtn.font = DesignFonts.nsBodyFont(size: 11, weight: .medium)
        copyBtn.wantsLayer = true
        copyBtn.layer?.cornerRadius = 8
        copyBtn.layer?.backgroundColor = palette.elevated.nsColor.cgColor
        copyBtn.translatesAutoresizingMaskIntoConstraints = false
        copyBtn.onTap = { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(code, forType: .string)
            coordinator?.onCopyFeedback?("已复制代码")
        }

        let scroll = NSScrollView()
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = false
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let lines = max(1, code.components(separatedBy: "\n").count)
        let codeHeight = CGFloat(min(lines, 18)) * 20.8 + 16
        let codeLabel = NSTextField(labelWithString: code)
        codeLabel.font = DesignFonts.nsMonoFont(size: 13)
        codeLabel.textColor = palette.textPrimary.nsColor
        codeLabel.isSelectable = true
        codeLabel.frame = NSRect(x: 0, y: 0, width: max(width - 32, CGFloat(code.count) * 4), height: codeHeight)
        scroll.documentView = codeLabel

        box.addSubview(langLabel)
        box.addSubview(scroll)
        box.addSubview(copyBtn)

        NSLayoutConstraint.activate([
            box.widthAnchor.constraint(equalToConstant: width),
            langLabel.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            langLabel.topAnchor.constraint(equalTo: box.topAnchor, constant: 10),
            copyBtn.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            copyBtn.topAnchor.constraint(equalTo: box.topAnchor, constant: 8),
            copyBtn.widthAnchor.constraint(greaterThanOrEqualToConstant: 32),
            copyBtn.heightAnchor.constraint(equalToConstant: 32),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16),
            scroll.topAnchor.constraint(equalTo: box.topAnchor, constant: 38),
            scroll.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -12),
            scroll.heightAnchor.constraint(equalToConstant: codeHeight),
        ])
        return box
    }

    private func makeTableView(
        headers: [NSAttributedString],
        rows: [[NSAttributedString]],
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let scroll = NSScrollView()
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = false
        scroll.drawsBackground = false
        scroll.wantsLayer = true
        scroll.layer?.cornerRadius = 8
        scroll.layer?.borderWidth = 1
        scroll.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let colCount = max(1, headers.count)
        let colWidth = max(120, floor(width / CGFloat(colCount)))
        let totalTableW = colWidth * CGFloat(colCount)
        let rowH: CGFloat = 34
        let allRows = [headers] + rows
        let docView = NSView(frame: NSRect(x: 0, y: 0, width: totalTableW, height: CGFloat(allRows.count) * rowH))

        for (rIdx, cols) in allRows.enumerated() {
            let y = CGFloat(allRows.count - 1 - rIdx) * rowH
            for cIdx in 0 ..< colCount {
                let cellRect = NSRect(x: CGFloat(cIdx) * colWidth, y: y, width: colWidth, height: rowH)
                let cellBox = NSView(frame: cellRect)
                cellBox.wantsLayer = true
                cellBox.layer?.backgroundColor = (rIdx == 0 ? palette.surface : palette.background).nsColor.cgColor
                cellBox.layer?.borderWidth = 0.5
                cellBox.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

                let cellAttr = cIdx < cols.count ? cols[cIdx] : NSAttributedString(string: "")
                let tv = ReaderSelectableTextView()
                tv.textStorage?.setAttributedString(cellAttr)
                tv.frame = NSRect(x: 12, y: 7, width: max(40, colWidth - 24), height: 20)
                tv.onLinkActivated = { [weak self, weak coordinator] href in
                    guard let self, let coordinator else { return false }
                    return self.handleActivatedHref(href, coordinator: coordinator)
                }
                cellBox.addSubview(tv)
                docView.addSubview(cellBox)
            }
        }
        scroll.documentView = docView
        scroll.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: width),
            scroll.heightAnchor.constraint(equalToConstant: CGFloat(allRows.count) * rowH),
        ])
        return scroll
    }

    private func buildExecutionGroupView(
        group: ExecutionGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: group.id,
            aliases: group.mergedIds,
            initiallyOpen: group.initiallyOpen
        )
        let header = group.summaryHeader
        let countMeta = "\(group.commands.count) \(group.kind == "tool" ? "项工具" : "条命令")"
            + (group.failedCount > 0 ? " · \(group.failedCount) 条失败" : "")
        let groupBtn = makeDisclosureHeaderButton(
            title: header.label,
            meta: countMeta,
            open: isOpen,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: group.id, currentlyOpen: isOpen)
        }
        containerStack.addArrangedSubview(groupBtn)

        // Lazy child creation: do NOT build inner views when collapsed!
        guard isOpen else { return }
        for cmd in group.commands {
            let cmdOpen = coordinator.disclosureStore.isOpen(
                scope: coordinator.sessionScopeKey,
                stateKey: cmd.id,
                aliases: cmd.sourceIds,
                initiallyOpen: !cmd.instructionFile && (cmd.status == .running || cmd.status == .failed)
            )
            let cmdTitle = cmd.instructionFile
                ? "AGENTS.md"
                : "\(VisibleMarkdownEngine.visibleMarkdown(cmd.title))\(cmd.preview.map { " \($0)" } ?? "")"
            let cmdBtn = makeDisclosureHeaderButton(
                title: cmdTitle,
                meta: cmd.status.label,
                open: cmdOpen,
                width: width - 16,
                palette: palette
            ) { [weak coordinator] in
                coordinator?.toggleDisclosurePreservingAnchor(stateKey: cmd.id, currentlyOpen: cmdOpen)
            }
            containerStack.addArrangedSubview(cmdBtn)
            if cmdOpen {
                let detail = cmd.sections.map { "\($0.label)\n\($0.text)" }.joined(separator: "\n\n")
                containerStack.addArrangedSubview(
                    makeMonospaceDetailBox(
                        text: detail.isEmpty ? "暂无输出" : detail,
                        width: width - 16,
                        palette: palette,
                        coordinator: coordinator,
                        linkifyFiles: true
                    )
                )
            }
        }
    }

    private func buildThinkingView(
        id: String,
        title: String,
        text: String,
        running: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let open = coordinator.disclosureStore.isOpen(scope: coordinator.sessionScopeKey, stateKey: id, initiallyOpen: running)
        let btn = makeDisclosureHeaderButton(
            title: running ? "正在思考…" : title,
            meta: open ? "收起" : "展开",
            open: open,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: id, currentlyOpen: open)
        }
        containerStack.addArrangedSubview(btn)
        if open {
            buildMarkdownViews(ownerId: id, markdown: text, foldInstructions: false, width: max(120, width - 20), showCopyReply: false, coordinator: coordinator)
        }
    }

    private func buildPlanView(
        id: String,
        markdown: String,
        statusText: String,
        fallbackTitle: String,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let cleanedPlan = VisibleMarkdownEngine.visibleMarkdown(markdown)
        let headerRow = NSStackView()
        headerRow.orientation = .horizontal
        headerRow.spacing = 8
        headerRow.widthAnchor.constraint(equalToConstant: width).isActive = true

        let badge = NSTextField(labelWithString: "Proposed Plan · \(statusText)")
        badge.font = DesignFonts.nsBodyFont(size: 12, weight: .semibold)
        badge.textColor = palette.accent.nsColor

        let copyBtn = ActionButton()
        copyBtn.title = "复制计划"
        copyBtn.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
        copyBtn.onTap = { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(cleanedPlan, forType: .string)
            coordinator?.onCopyFeedback?("已复制计划")
        }

        let saveBtn = ActionButton()
        saveBtn.title = "导出 .md"
        saveBtn.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
        saveBtn.onTap = { [weak coordinator] in
            guard let coordinator else { return }
            let filename = FileLinksAndPlanExport.planFilename(markdown: cleanedPlan, fallbackTitle: fallbackTitle)
            Task { @MainActor in
                struct Wrap: Encodable { let request: PlanSaveRequest }
                if let saved: PlanSaveResult = try? await coordinator.store.rpc(
                    "plans.save",
                    args: Wrap(request: PlanSaveRequest(filename: filename, markdown: cleanedPlan)),
                    localOnly: true
                ) {
                    coordinator.onCopyFeedback?("已保存至 \(saved.path)")
                }
            }
        }

        headerRow.addArrangedSubview(badge)
        headerRow.addArrangedSubview(NSView())
        headerRow.addArrangedSubview(copyBtn)
        headerRow.addArrangedSubview(saveBtn)
        containerStack.addArrangedSubview(headerRow)
        buildMarkdownViews(ownerId: id, markdown: cleanedPlan, foldInstructions: false, width: width, showCopyReply: false, coordinator: coordinator)
    }

    private func buildQuestionView(
        header: String,
        questions: [UserInputQuestion],
        answers: [UserInputAnswer]?,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let titleLbl = NSTextField(labelWithString: "Questions · \(header)")
        titleLbl.font = DesignFonts.nsBodyFont(size: 12, weight: .semibold)
        titleLbl.textColor = palette.textMuted.nsColor
        containerStack.addArrangedSubview(titleLbl)

        for q in questions {
            let qLbl = NSTextField(wrappingLabelWithString: q.question)
            qLbl.font = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
            qLbl.textColor = palette.textPrimary.nsColor
            qLbl.preferredMaxLayoutWidth = width
            containerStack.addArrangedSubview(qLbl)
            for (idx, opt) in q.options.enumerated() {
                let optLbl = NSTextField(labelWithString: "\(idx + 1). \(opt.label)\(opt.description.map { " — \($0)" } ?? "")")
                optLbl.font = DesignFonts.nsBodyFont(size: 13)
                optLbl.textColor = palette.textSecondary.nsColor
                containerStack.addArrangedSubview(optLbl)
            }
        }
        if let answers, !answers.isEmpty {
            for ans in answers {
                let ansText = ans.answers.isEmpty ? "未回答" : ans.answers.joined(separator: ", ")
                let ansLbl = NSTextField(labelWithString: "✓ \(ans.questionId): \(ansText)")
                ansLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
                ansLbl.textColor = palette.okFg.nsColor
                containerStack.addArrangedSubview(ansLbl)
            }
        }
    }

    private func buildSubagentRow(
        sub: SubagentActivity,
        supported: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let btn = ActionButton()
        btn.title = "🤖 子智能体：\(sub.name)\(sub.role.map { " (\($0))" } ?? "") · \(sub.status)"
        btn.alignment = .left
        btn.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        btn.isEnabled = supported && sub.available
        btn.wantsLayer = true
        btn.layer?.cornerRadius = 10
        btn.layer?.backgroundColor = palette.surface.nsColor.cgColor
        btn.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            btn.widthAnchor.constraint(equalToConstant: width),
            btn.heightAnchor.constraint(equalToConstant: 42),
        ])
        btn.onTap = { [weak coordinator] in
            coordinator?.onOpenSubagent?(sub)
        }
        containerStack.addArrangedSubview(btn)
    }

    private func buildHistoryCollapseRow(
        eyebrow: String,
        label: String,
        expanded: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let row = NSStackView()
        row.orientation = .horizontal
        row.spacing = 10
        row.wantsLayer = true
        row.layer?.cornerRadius = 10
        row.layer?.backgroundColor = palette.surface.nsColor.cgColor
        row.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        row.widthAnchor.constraint(equalToConstant: width).isActive = true

        let lbl = NSTextField(labelWithString: "\(eyebrow) · \(label)")
        lbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        lbl.textColor = palette.textSecondary.nsColor

        let btn = ActionButton()
        btn.title = expanded ? "已显示全部" : "显示全部历史"
        btn.isEnabled = !expanded
        btn.font = DesignFonts.nsBodyFont(size: 12, weight: .semibold)
        btn.onTap = { [weak coordinator] in
            coordinator?.onShowAllHistory?()
        }
        row.addArrangedSubview(lbl)
        row.addArrangedSubview(NSView())
        row.addArrangedSubview(btn)
        containerStack.addArrangedSubview(row)
    }

    private func buildSystemNoticeRow(label: String, detail: String, width: CGFloat, palette: DesignPalette) {
        let lbl = NSTextField(labelWithString: "\(label): \(detail)")
        lbl.font = DesignFonts.nsBodyFont(size: 12)
        lbl.textColor = palette.textMuted.nsColor
        lbl.lineBreakMode = .byTruncatingTail
        lbl.widthAnchor.constraint(equalToConstant: width).isActive = true
        containerStack.addArrangedSubview(lbl)
    }

    private func makeDisclosureHeaderButton(
        title: String,
        meta: String,
        open: Bool,
        width: CGFloat,
        palette: DesignPalette,
        action: @escaping () -> Void
    ) -> NSView {
        let btn = ActionButton()
        btn.title = "\(open ? "▾" : "▸") \(title)   \(meta)"
        btn.alignment = .left
        btn.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        btn.contentTintColor = palette.textSecondary.nsColor
        btn.wantsLayer = true
        btn.layer?.cornerRadius = 8
        btn.layer?.backgroundColor = palette.surface.nsColor.cgColor
        btn.layer?.borderWidth = 1
        btn.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        btn.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            btn.widthAnchor.constraint(equalToConstant: max(100, width)),
            btn.heightAnchor.constraint(equalToConstant: 34),
        ])
        btn.onTap = action
        return btn
    }

    private func makeMonospaceDetailBox(
        text: String,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator,
        linkifyFiles: Bool
    ) -> NSView {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
        let baseAttrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsMonoFont(size: 12),
            .foregroundColor: palette.textSecondary.nsColor,
        ]
        let attr = NSMutableAttributedString()
        if linkifyFiles {
            let spans = FileLinksAndPlanExport.fileLinkSpans(cleaned)
            let ns = cleaned as NSString
            var cursor = 0
            for span in spans where span.nsRange.location >= cursor {
                if span.nsRange.location > cursor {
                    let prefix = ns.substring(with: NSRange(location: cursor, length: span.nsRange.location - cursor))
                    attr.append(NSAttributedString(string: prefix, attributes: baseAttrs))
                }
                var linkAttrs = baseAttrs
                linkAttrs[.foregroundColor] = palette.accent.nsColor
                linkAttrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
                linkAttrs[.link] = span.href
                attr.append(NSAttributedString(string: span.label, attributes: linkAttrs))
                cursor = span.nsRange.location + span.nsRange.length
            }
            if cursor < ns.length {
                attr.append(NSAttributedString(string: ns.substring(from: cursor), attributes: baseAttrs))
            }
        } else {
            attr.append(NSAttributedString(string: cleaned, attributes: baseAttrs))
        }

        let innerW = max(80, width - 24)
        let contentH = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: innerW))
        let boxH = min(220, max(48, contentH + 18))

        let scroll = NSScrollView()
        scroll.hasVerticalScroller = contentH + 18 > boxH
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = palette.codeBg.nsColor
        scroll.wantsLayer = true
        scroll.layer?.cornerRadius = 8
        scroll.layer?.borderWidth = 1
        scroll.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let tv = ReaderSelectableTextView()
        tv.textStorage?.setAttributedString(attr)
        tv.textContainerInset = NSSize(width: 10, height: 8)
        tv.frame = NSRect(x: 0, y: 0, width: max(100, width), height: max(boxH, contentH + 16))
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        scroll.documentView = tv
        scroll.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: max(100, width)),
            scroll.heightAnchor.constraint(equalToConstant: boxH),
        ])
        return scroll
    }
}

public final class ActionButton: NSButton {
    public var onTap: (() -> Void)?

    override public init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        isBordered = false
        bezelStyle = .regularSquare
        target = self
        action = #selector(handlePress)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    @objc private func handlePress() {
        onTap?()
    }
}

// MARK: - SwiftUI Representable Wrapping NSScrollView + Headerless View-Based NSTableView

public struct ConversationReaderTableView: NSViewRepresentable {
    public let coordinator: ConversationTableCoordinator
    public let rows: [ConversationRowItem]
    public let scheme: ColorSchemeKind
    public let workspace: WorkspaceId
    public let sessionScopeKey: String
    public let sessionProvider: SessionProvider
    public let sessionKey: String
    public let rootThreadId: String?
    public let cwd: String?
    public let hasTimelineRail: Bool
    public let isSubagent: Bool

    public init(
        coordinator: ConversationTableCoordinator,
        rows: [ConversationRowItem],
        scheme: ColorSchemeKind,
        workspace: WorkspaceId,
        sessionScopeKey: String,
        sessionProvider: SessionProvider,
        sessionKey: String,
        rootThreadId: String? = nil,
        cwd: String? = nil,
        hasTimelineRail: Bool = true,
        isSubagent: Bool = false
    ) {
        self.coordinator = coordinator
        self.rows = rows
        self.scheme = scheme
        self.workspace = workspace
        self.sessionScopeKey = sessionScopeKey
        self.sessionProvider = sessionProvider
        self.sessionKey = sessionKey
        self.rootThreadId = rootThreadId
        self.cwd = cwd
        self.hasTimelineRail = hasTimelineRail
        self.isSubagent = isSubagent
    }

    public func makeCoordinator() -> ConversationTableCoordinator {
        coordinator
    }

    public func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = coordinator.palette.background.nsColor
        scroll.contentInsets = NSEdgeInsets(top: isSubagent ? 16 : 22, left: 0, bottom: isSubagent ? 16 : 32, right: 0)

        let table = NSTableView()
        table.headerView = nil
        table.style = .plain
        table.intercellSpacing = NSSize(width: 0, height: 12)
        table.selectionHighlightStyle = .none
        table.backgroundColor = .clear
        table.usesAutomaticRowHeights = false

        let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("MainConversationColumn"))
        col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.dataSource = coordinator
        table.delegate = coordinator

        scroll.documentView = table
        scroll.postsFrameChangedNotifications = true
        scroll.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            coordinator,
            selector: #selector(ConversationTableCoordinator.scrollViewFrameDidChange(_:)),
            name: NSView.frameDidChangeNotification,
            object: scroll
        )
        NotificationCenter.default.addObserver(
            coordinator,
            selector: #selector(ConversationTableCoordinator.clipViewBoundsDidChange(_:)),
            name: NSView.boundsDidChangeNotification,
            object: scroll.contentView
        )

        coordinator.scrollView = scroll
        coordinator.tableView = table
        coordinator.column = col
        coordinator.updateRowsPreservingAnchor(rows, preferAutoFollowBottom: true)
        return scroll
    }

    public func updateNSView(_ nsView: NSScrollView, context: Context) {
        let sessionChanged = (coordinator.sessionScopeKey != sessionScopeKey)
        coordinator.scheme = scheme
        coordinator.workspace = workspace
        coordinator.sessionScopeKey = sessionScopeKey
        coordinator.sessionProvider = sessionProvider
        coordinator.sessionKey = sessionKey
        coordinator.rootThreadId = rootThreadId
        coordinator.cwd = cwd
        coordinator.hasTimelineRail = hasTimelineRail
        coordinator.isSubagent = isSubagent
        nsView.backgroundColor = coordinator.palette.background.nsColor

        if coordinator.rows != rows || sessionChanged {
            coordinator.updateRowsPreservingAnchor(
                rows,
                preferAutoFollowBottom: true,
                forceScrollToBottom: sessionChanged
            )
        }
    }
}
