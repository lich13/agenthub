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
    public var isLoadOlderBlockedByError: Bool = false
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
        if clipView.bounds.origin.y <= 200 && !isLoadOlderBlockedByError {
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
                let hasImage = !namesOnly && atts.contains(where: { $0.kind == "image" && $0.reason == nil })
                h += hasImage ? 104 : 44
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
            let bubbleHPad: CGFloat = (width <= 500) ? 16 : 18
            let bubbleWidth = max(140, min(width * 0.85, 640))
            let para = NSMutableParagraphStyle()
            para.minimumLineHeight = 24.75 // 15pt * 1.65
            para.maximumLineHeight = 24.75
            let attrs: [NSAttributedString.Key: Any] = [
                .font: DesignFonts.nsBodyFont(size: 15),
                .paragraphStyle: para,
            ]
            for seg in segs {
                switch seg {
                case let .text(_, t):
                    let attr = NSAttributedString(string: t, attributes: attrs)
                    h += ReaderSelectableTextView.measureHeight(attributed: attr, width: bubbleWidth - bubbleHPad * 2) + 28
                case let .reply(segId, q, a):
                    let trimmedQ = q.trimmingCharacters(in: .whitespacesAndNewlines)
                    var qExtra: CGFloat = 0
                    if !trimmedQ.isEmpty {
                        let qKey = "\(row.timelineId):\(segId):question"
                        let qOpen = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: qKey, initiallyOpen: false)
                        if qOpen {
                            let qAttr = NSAttributedString(
                                string: trimmedQ,
                                attributes: [.font: DesignFonts.nsBodyFont(size: 13)]
                            )
                            qExtra = 32 + ReaderSelectableTextView.measureHeight(
                                attributed: qAttr,
                                width: max(80, bubbleWidth - bubbleHPad * 2 - 44)
                            )
                        } else {
                            qExtra = 32
                        }
                    }
                    let aAttr = NSAttributedString(string: a, attributes: attrs)
                    h += qExtra + ReaderSelectableTextView.measureHeight(
                        attributed: aAttr,
                        width: max(80, bubbleWidth - bubbleHPad * 2 - 38)
                    ) + 28
                }
            }
            return max(48, h)

        case let .assistant(markdown, foldInstructions):
            // Side-hover copy button is floating in the right margin and NEVER adds vertical height (0.0pt impact)!
            return measureMarkdownSegmentsHeight(ownerId: row.id, markdown: markdown, foldInstructions: foldInstructions, width: width) + 8

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
            return 74 + measureMarkdownSegmentsHeight(ownerId: row.id, markdown: markdown, foldInstructions: false, width: max(120, width - 35))

        case let .question(_, questions):
            return CGFloat(56 + questions.count * 76)

        case let .questionResult(_, questions, answers):
            return CGFloat(64 + questions.count * 52 + answers.count * 32)

        case let .subagent(sub, supported):
            let available = supported && sub.available && (sub.agentId?.isEmpty == false)
            let reason = supported ? sub.unavailableReason : "请更新当前机器服务以查看子智能体"
            return (!available && (reason?.isEmpty == false)) ? 66 : 48

        case .historyCollapse:
            return 52

        case .systemNotice:
            return 42
        }
    }

    public static func measureCodeBlockHeight(code: String) -> CGFloat {
        let lines = max(1, code.components(separatedBy: "\n").count)
        return ceil(CGFloat(lines) * 20.8) + 16
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
                // Full vertical height of fenced code blocks — NEVER clip at 18 lines!
                total += Self.measureCodeBlockHeight(code: code) + 52
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
    public private(set) var floatingSideCopyButton: IconFeedbackActionButton?
    private var hoverTrackingArea: NSTrackingArea?
    public private(set) var isRowHovered: Bool = false {
        didSet { updateFloatingCopyVisibility() }
    }
    private var configuredLeadingInset: CGFloat = 0
    private var configuredContentWidth: CGFloat = 0

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

    /// Computes X origin for the 30x30pt floating reply copy button:
    /// `right: -36pt` relative to the `768pt` content column (`leadingInset + contentWidth + 6`),
    /// clamped inside viewport (`cellWidth - 34`) on narrow widths.
    public static func floatingSideCopyButtonX(
        leadingInset: CGFloat,
        contentWidth: CGFloat,
        cellWidth: CGFloat
    ) -> CGFloat {
        let desired = leadingInset + contentWidth + 6
        return max(8, min(max(8, cellWidth - 34), desired))
    }

    override public func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let hoverTrackingArea {
            removeTrackingArea(hoverTrackingArea)
        }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        hoverTrackingArea = area
    }

    override public func mouseEntered(with event: NSEvent) {
        isRowHovered = true
    }

    override public func mouseExited(with event: NSEvent) {
        isRowHovered = false
    }

    override public func layout() {
        super.layout()
        positionFloatingCopyButton()
    }

    private func positionFloatingCopyButton() {
        guard let btn = floatingSideCopyButton else { return }
        let totalW = bounds.width > 0 ? bounds.width : (configuredLeadingInset + configuredContentWidth + 46)
        let x = Self.floatingSideCopyButtonX(
            leadingInset: configuredLeadingInset,
            contentWidth: configuredContentWidth,
            cellWidth: totalW
        )
        // NSView is non-flipped (y=0 at bottom); top: -2pt relative to containerStack (topAnchor + 2) => top offset 0pt!
        let y = max(0, bounds.height - 30)
        btn.frame = NSRect(x: x, y: y, width: 30, height: 30)
        updateFloatingCopyVisibility()
    }

    public func setSimulatedHoverForTesting(_ hovered: Bool) {
        isRowHovered = hovered
    }

    private func updateFloatingCopyVisibility() {
        guard let btn = floatingSideCopyButton else { return }
        let narrowMobile = bounds.width > 0 && bounds.width <= LayoutMetrics.mobileBreakpoint
        let focusedInside = (window?.firstResponder as? NSView).map { $0 === btn || $0.isDescendant(of: self) } ?? false
        let visible = isRowHovered || btn.isShowingCopied || focusedInside || narrowMobile
        btn.alphaValue = visible ? 1.0 : 0.0
    }

    public func configure(
        item: ConversationRowItem,
        leadingInset: CGFloat,
        trailingInset: CGFloat,
        contentWidth: CGFloat,
        highlighted: Bool,
        coordinator: ConversationTableCoordinator
    ) {
        configuredLeadingInset = leadingInset
        configuredContentWidth = contentWidth
        floatingSideCopyButton?.removeFromSuperview()
        floatingSideCopyButton = nil
        isRowHovered = false

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
            buildMarkdownViews(ownerId: item.id, markdown: markdown, foldInstructions: foldInstructions, width: contentWidth, coordinator: coordinator)
            let cleanedReply = VisibleMarkdownEngine.visibleMarkdown(markdown)
            if !cleanedReply.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                let floatBtn = IconFeedbackActionButton(
                    dimension: 30,
                    iconSize: 17,
                    idleIcon: .copy,
                    palette: palette,
                    toolTipText: "复制答复"
                ) { [weak self, weak coordinator] in
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(cleanedReply, forType: .string)
                    coordinator?.onCopyFeedback?("已复制答复")
                    self?.updateFloatingCopyVisibility()
                }
                floatBtn.onFeedbackStateChanged = { [weak self] in
                    self?.updateFloatingCopyVisibility()
                }
                addSubview(floatBtn)
                floatingSideCopyButton = floatBtn
                positionFloatingCopyButton()
            }
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
                if !namesOnly && att.kind == "image" {
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
                    if att.reason == nil, let img = coordinator.attachmentViewport.image(for: key) {
                        thumbBtn.image = img
                        thumbBtn.imageScaling = .scaleProportionallyUpOrDown
                        thumbBtn.title = ""
                        thumbBtn.toolTip = att.name
                        thumbBtn.onTap = { [weak coordinator] in
                            coordinator?.onPreviewImage?(att.name, img)
                        }
                    } else {
                        thumbBtn.image = LucideVectorCatalog.nsImage(
                            for: .imageOff,
                            size: 22,
                            color: palette.textMuted.nsColor
                        )
                        thumbBtn.imagePosition = .imageAbove
                        thumbBtn.font = DesignFonts.nsBodyFont(size: 11)
                        thumbBtn.contentTintColor = palette.textMuted.nsColor
                        thumbBtn.title = (att.reason != nil) ? "图片不可用" : "图片"
                        thumbBtn.toolTip = att.reason ?? att.name
                        thumbBtn.isEnabled = false
                    }
                    attRow.addArrangedSubview(thumbBtn)
                } else {
                    attRow.addArrangedSubview(makeFileAttachmentCard(att: att, namesOnly: namesOnly, palette: palette, coordinator: coordinator))
                }
            }
            rightAlignRow.addArrangedSubview(attRow)
        }

        if !proj.contextBlocks.isEmpty {
            let ctxKey = "\(item.timelineId):context"
            let open = coordinator.disclosureStore.isOpen(scope: coordinator.sessionScopeKey, stateKey: ctxKey, initiallyOpen: false)
            let toggle = makeDisclosureHeaderButton(
                title: "上下文",
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
        let hPad: CGFloat = (width <= 500) ? 16 : 18
        let vPad: CGFloat = 12
        let maxBubbleW = max(140, min(width * 0.85, 640))

        for seg in segs {
            switch seg {
            case let .text(_, t):
                let bubble = makeUserBubbleContainer(palette: palette)
                let tv = ReaderSelectableTextView()
                let attr = NSAttributedString(string: t, attributes: attrs)
                tv.textStorage?.setAttributedString(attr)
                let textW = min(maxBubbleW - hPad * 2, max(64, ceil(attr.size().width) + 8))
                let textH = ReaderSelectableTextView.measureHeight(attributed: attr, width: textW)
                tv.translatesAutoresizingMaskIntoConstraints = false
                bubble.addSubview(tv)
                NSLayoutConstraint.activate([
                    tv.leadingAnchor.constraint(equalTo: bubble.leadingAnchor, constant: hPad),
                    tv.trailingAnchor.constraint(equalTo: bubble.trailingAnchor, constant: -hPad),
                    tv.topAnchor.constraint(equalTo: bubble.topAnchor, constant: vPad),
                    tv.bottomAnchor.constraint(equalTo: bubble.bottomAnchor, constant: -vPad),
                    tv.widthAnchor.constraint(equalToConstant: textW),
                    tv.heightAnchor.constraint(equalToConstant: max(24, textH)),
                ])
                rightAlignRow.addArrangedSubview(bubble)

            case let .reply(segId, q, a):
                let replyRow = NSStackView()
                replyRow.orientation = .horizontal
                replyRow.alignment = .top
                replyRow.spacing = 8

                let copyAnsBtn = IconFeedbackActionButton(
                    dimension: 30,
                    iconSize: 17,
                    idleIcon: .copy,
                    palette: palette,
                    toolTipText: "复制回答"
                ) { [weak coordinator] in
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(a, forType: .string)
                    coordinator?.onCopyFeedback?("已复制回答")
                }

                let bubble = makeUserBubbleContainer(palette: palette)
                let innerStack = NSStackView()
                innerStack.orientation = .vertical
                innerStack.alignment = .leading
                innerStack.spacing = 6
                innerStack.translatesAutoresizingMaskIntoConstraints = false
                bubble.addSubview(innerStack)

                let trimmedQ = q.trimmingCharacters(in: .whitespacesAndNewlines)
                let availBubbleTextW = max(80, maxBubbleW - hPad * 2 - 38)
                if !trimmedQ.isEmpty {
                    let qKey = "\(item.timelineId):\(segId):question"
                    let qOpen = coordinator.disclosureStore.isOpen(
                        scope: coordinator.sessionScopeKey,
                        stateKey: qKey,
                        initiallyOpen: false
                    )
                    let qHeaderBtn = ActionButton()
                    qHeaderBtn.title = "\(qOpen ? "▾" : "▸") \(trimmedQ)"
                    qHeaderBtn.alignment = .left
                    qHeaderBtn.lineBreakMode = .byTruncatingTail
                    qHeaderBtn.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
                    qHeaderBtn.contentTintColor = palette.textSecondary.nsColor
                    qHeaderBtn.translatesAutoresizingMaskIntoConstraints = false
                    NSLayoutConstraint.activate([
                        qHeaderBtn.widthAnchor.constraint(lessThanOrEqualToConstant: availBubbleTextW),
                        qHeaderBtn.heightAnchor.constraint(equalToConstant: 22),
                    ])
                    qHeaderBtn.onTap = { [weak coordinator] in
                        coordinator?.toggleDisclosurePreservingAnchor(stateKey: qKey, currentlyOpen: qOpen)
                    }
                    innerStack.addArrangedSubview(qHeaderBtn)

                    if qOpen {
                        let qFull = ReaderSelectableTextView()
                        let qAttr = NSAttributedString(
                            string: trimmedQ,
                            attributes: [
                                .font: DesignFonts.nsBodyFont(size: 13),
                                .foregroundColor: palette.textSecondary.nsColor,
                            ]
                        )
                        qFull.textStorage?.setAttributedString(qAttr)
                        let qW = min(availBubbleTextW, max(80, ceil(qAttr.size().width) + 8))
                        let qH = max(20, ReaderSelectableTextView.measureHeight(attributed: qAttr, width: qW))
                        qFull.translatesAutoresizingMaskIntoConstraints = false
                        NSLayoutConstraint.activate([
                            qFull.widthAnchor.constraint(equalToConstant: qW),
                            qFull.heightAnchor.constraint(equalToConstant: qH),
                        ])
                        innerStack.addArrangedSubview(qFull)
                    }
                }

                let ansTv = ReaderSelectableTextView()
                let ansAttr = NSAttributedString(string: a, attributes: attrs)
                ansTv.textStorage?.setAttributedString(ansAttr)
                let ansW = min(availBubbleTextW, max(64, ceil(ansAttr.size().width) + 8))
                let ansH = max(24, ReaderSelectableTextView.measureHeight(attributed: ansAttr, width: ansW))
                ansTv.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    ansTv.widthAnchor.constraint(equalToConstant: ansW),
                    ansTv.heightAnchor.constraint(equalToConstant: ansH),
                ])
                innerStack.addArrangedSubview(ansTv)

                NSLayoutConstraint.activate([
                    innerStack.leadingAnchor.constraint(equalTo: bubble.leadingAnchor, constant: hPad),
                    innerStack.trailingAnchor.constraint(equalTo: bubble.trailingAnchor, constant: -hPad),
                    innerStack.topAnchor.constraint(equalTo: bubble.topAnchor, constant: vPad),
                    innerStack.bottomAnchor.constraint(equalTo: bubble.bottomAnchor, constant: -vPad),
                ])

                replyRow.addArrangedSubview(copyAnsBtn)
                replyRow.addArrangedSubview(bubble)
                rightAlignRow.addArrangedSubview(replyRow)
            }
        }
        containerStack.addArrangedSubview(rightAlignRow)
    }

    private func makeUserBubbleContainer(palette: DesignPalette) -> NSView {
        let bubble = NSView()
        bubble.wantsLayer = true
        bubble.layer?.cornerRadius = 20
        bubble.layer?.backgroundColor = palette.surface.nsColor.cgColor
        bubble.layer?.borderWidth = 1
        bubble.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        return bubble
    }

    private func makeFileAttachmentCard(
        att: UserAttachment,
        namesOnly: Bool,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let card = NSStackView()
        card.orientation = .horizontal
        card.alignment = .centerY
        card.spacing = 8
        card.wantsLayer = true
        card.layer?.cornerRadius = 10
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        card.edgeInsets = NSEdgeInsets(top: 6, left: 10, bottom: 6, right: 8)

        let iconView = NSImageView(
            image: LucideVectorCatalog.nsImage(for: .file, size: 18, color: palette.textSecondary.nsColor)
        )
        iconView.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            iconView.widthAnchor.constraint(equalToConstant: 18),
            iconView.heightAnchor.constraint(equalToConstant: 18),
        ])
        card.addArrangedSubview(iconView)

        let textCol = NSStackView()
        textCol.orientation = .vertical
        textCol.alignment = .leading
        textCol.spacing = 2

        if !namesOnly, let rawPath = att.path, !rawPath.isEmpty {
            let linkBtn = ActionButton()
            let attrTitle = NSAttributedString(
                string: att.name,
                attributes: [
                    .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                    .foregroundColor: palette.accent.nsColor,
                    .underlineStyle: NSUnderlineStyle.single.rawValue,
                ]
            )
            linkBtn.attributedTitle = attrTitle
            linkBtn.toolTip = rawPath
            linkBtn.onTap = { [weak self, weak coordinator] in
                guard let self, let coordinator else { return }
                _ = self.handleActivatedHref(rawPath, coordinator: coordinator)
            }
            textCol.addArrangedSubview(linkBtn)
        } else {
            let nameLbl = NSTextField(labelWithString: att.name)
            nameLbl.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
            nameLbl.textColor = palette.textPrimary.nsColor
            textCol.addArrangedSubview(nameLbl)
        }

        if let reason = att.reason, !reason.isEmpty {
            let reasonLbl = NSTextField(labelWithString: reason)
            reasonLbl.font = DesignFonts.nsBodyFont(size: 11)
            reasonLbl.textColor = palette.textMuted.nsColor
            textCol.addArrangedSubview(reasonLbl)
        }
        card.addArrangedSubview(textCol)

        if !namesOnly, let rawPath = att.path, !rawPath.isEmpty {
            let copyPathBtn = IconFeedbackActionButton(
                dimension: 28,
                iconSize: 15,
                idleIcon: .copy,
                palette: palette,
                toolTipText: "复制路径"
            ) { [weak coordinator] in
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(rawPath, forType: .string)
                coordinator?.onCopyFeedback?("已复制路径")
            }
            card.addArrangedSubview(copyPathBtn)
        }

        return card
    }

    private func buildMarkdownViews(
        ownerId: String,
        markdown: String,
        foldInstructions: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
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
                        coordinator: coordinator
                    )
                }
            }
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
        let palette = coordinator.palette
        let tv = ReaderSelectableTextView()
        tv.quoteBackgroundColor = palette.surface.nsColor
        tv.quoteAccentColor = palette.accent.nsColor
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

        let headerTitle = (lang?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false) ? lang! : "code"
        let langLabel = NSTextField(labelWithString: headerTitle)
        langLabel.font = DesignFonts.nsMonoFont(size: 11, weight: .medium)
        langLabel.textColor = palette.textMuted.nsColor
        langLabel.translatesAutoresizingMaskIntoConstraints = false

        let copyBtn = IconFeedbackActionButton(
            dimension: 32,
            iconSize: 16,
            idleIcon: .copy,
            palette: palette,
            toolTipText: "复制代码"
        ) { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(code, forType: .string)
            coordinator?.onCopyFeedback?("已复制代码")
        }
        copyBtn.wantsLayer = true
        copyBtn.layer?.cornerRadius = 8
        copyBtn.layer?.backgroundColor = palette.elevated.nsColor.cgColor
        copyBtn.translatesAutoresizingMaskIntoConstraints = false

        let scroll = NSScrollView()
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let codeHeight = ConversationTableCoordinator.measureCodeBlockHeight(code: code)
        let maxLineChars = code.components(separatedBy: "\n").map(\.count).max() ?? 1
        let docWidth = max(width - 32, CGFloat(maxLineChars) * 8.2 + 24)

        let pStyle = NSMutableParagraphStyle()
        pStyle.minimumLineHeight = 20.8 // 13pt * 1.6
        pStyle.maximumLineHeight = 20.8
        let codeAttr = NSAttributedString(
            string: code,
            attributes: [
                .font: DesignFonts.nsMonoFont(size: 13),
                .foregroundColor: palette.textPrimary.nsColor,
                .paragraphStyle: pStyle,
            ]
        )

        let codeTv = NSTextView(frame: NSRect(x: 0, y: 0, width: docWidth, height: codeHeight))
        codeTv.isEditable = false
        codeTv.isSelectable = true
        codeTv.drawsBackground = false
        codeTv.isRichText = false
        codeTv.isHorizontallyResizable = true
        codeTv.isVerticallyResizable = false
        codeTv.textContainerInset = NSSize(width: 0, height: 6)
        codeTv.textContainer?.lineFragmentPadding = 0
        codeTv.textContainer?.widthTracksTextView = false
        codeTv.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: codeHeight)
        codeTv.textStorage?.setAttributedString(codeAttr)
        scroll.documentView = codeTv

        box.addSubview(langLabel)
        box.addSubview(scroll)
        box.addSubview(copyBtn)

        NSLayoutConstraint.activate([
            box.widthAnchor.constraint(equalToConstant: width),
            langLabel.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            langLabel.topAnchor.constraint(equalTo: box.topAnchor, constant: 12),
            copyBtn.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            copyBtn.topAnchor.constraint(equalTo: box.topAnchor, constant: 6),
            copyBtn.widthAnchor.constraint(equalToConstant: 32),
            copyBtn.heightAnchor.constraint(equalToConstant: 32),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16),
            scroll.topAnchor.constraint(equalTo: box.topAnchor, constant: 40),
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
            buildMarkdownViews(ownerId: id, markdown: text, foldInstructions: false, width: max(120, width - 20), coordinator: coordinator)
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

        let planCard = NSView()
        planCard.wantsLayer = true
        planCard.layer?.cornerRadius = 12
        planCard.layer?.masksToBounds = true
        planCard.layer?.backgroundColor = palette.surface.nsColor.cgColor
        planCard.layer?.borderWidth = 1
        planCard.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let leftBar = NSView()
        leftBar.wantsLayer = true
        leftBar.layer?.backgroundColor = palette.accent.nsColor.cgColor
        leftBar.translatesAutoresizingMaskIntoConstraints = false
        planCard.addSubview(leftBar)

        let innerStack = NSStackView()
        innerStack.orientation = .vertical
        innerStack.alignment = .leading
        innerStack.spacing = 10
        innerStack.translatesAutoresizingMaskIntoConstraints = false
        planCard.addSubview(innerStack)

        let innerWidth = max(120, width - 35)
        let headerRow = NSStackView()
        headerRow.orientation = .horizontal
        headerRow.alignment = .centerY
        headerRow.spacing = 6
        headerRow.widthAnchor.constraint(equalToConstant: innerWidth).isActive = true

        let badge = NSTextField(labelWithString: "Proposed Plan · \(statusText)")
        badge.font = DesignFonts.nsBodyFont(size: 12, weight: .semibold)
        badge.textColor = palette.accent.nsColor

        let copyBtn = IconFeedbackActionButton(
            dimension: 30,
            iconSize: 17,
            idleIcon: .copy,
            palette: palette,
            toolTipText: "复制计划"
        ) { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(cleanedPlan, forType: .string)
            coordinator?.onCopyFeedback?("已复制")
        }

        let saveBtn = ActionButton(frame: NSRect(x: 0, y: 0, width: 30, height: 30))
        saveBtn.image = LucideVectorCatalog.nsImage(for: .download, size: 17, color: palette.textMuted.nsColor)
        saveBtn.imagePosition = .imageOnly
        saveBtn.toolTip = "下载计划 Markdown"
        saveBtn.wantsLayer = true
        saveBtn.layer?.cornerRadius = 8
        saveBtn.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            saveBtn.widthAnchor.constraint(equalToConstant: 30),
            saveBtn.heightAnchor.constraint(equalToConstant: 30),
        ])
        saveBtn.onTap = { [weak coordinator, weak saveBtn] in
            guard let coordinator else { return }
            saveBtn?.isEnabled = false
            let filename = FileLinksAndPlanExport.planFilename(markdown: cleanedPlan, fallbackTitle: fallbackTitle)
            Task { @MainActor in
                defer { saveBtn?.isEnabled = true }
                struct Wrap: Encodable { let request: PlanSaveRequest }
                do {
                    let _: PlanSaveResult = try await coordinator.store.rpc(
                        "plans.save",
                        args: Wrap(request: PlanSaveRequest(filename: filename, markdown: cleanedPlan)),
                        localOnly: true
                    )
                    coordinator.onCopyFeedback?("已保存到下载文件夹")
                } catch {
                    coordinator.onCopyFeedback?("保存失败，请重试。")
                }
            }
        }

        headerRow.addArrangedSubview(badge)
        headerRow.addArrangedSubview(NSView())
        headerRow.addArrangedSubview(copyBtn)
        headerRow.addArrangedSubview(saveBtn)
        innerStack.addArrangedSubview(headerRow)

        let segs = GfmLayoutCache.shared.getOrRender(
            markdown: cleanedPlan,
            foldInstructions: false,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            transport: coordinator.store.transport
        )
        for seg in segs {
            switch seg {
            case let .prose(attr, _):
                innerStack.addArrangedSubview(makeProseTextView(attr: attr, width: innerWidth, coordinator: coordinator))
            case let .codeBlock(lang, code):
                innerStack.addArrangedSubview(makeCodeBlockView(lang: lang, code: code, width: innerWidth, coordinator: coordinator))
            case let .table(_, headers, rows):
                innerStack.addArrangedSubview(makeTableView(headers: headers, rows: rows, width: innerWidth, palette: palette, coordinator: coordinator))
            case .thematicBreak:
                let rule = NSView()
                rule.wantsLayer = true
                rule.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                rule.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    rule.widthAnchor.constraint(equalToConstant: innerWidth),
                    rule.heightAnchor.constraint(equalToConstant: 1),
                ])
                innerStack.addArrangedSubview(rule)
            case .instructionsFold:
                break
            }
        }

        planCard.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            planCard.widthAnchor.constraint(equalToConstant: width),
            leftBar.leadingAnchor.constraint(equalTo: planCard.leadingAnchor),
            leftBar.topAnchor.constraint(equalTo: planCard.topAnchor),
            leftBar.bottomAnchor.constraint(equalTo: planCard.bottomAnchor),
            leftBar.widthAnchor.constraint(equalToConstant: 3),
            innerStack.leadingAnchor.constraint(equalTo: planCard.leadingAnchor, constant: 19),
            innerStack.trailingAnchor.constraint(equalTo: planCard.trailingAnchor, constant: -16),
            innerStack.topAnchor.constraint(equalTo: planCard.topAnchor, constant: 14),
            innerStack.bottomAnchor.constraint(equalTo: planCard.bottomAnchor, constant: -14),
        ])
        containerStack.addArrangedSubview(planCard)
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

    public static func subagentActivityVerb(_ sub: SubagentActivity) -> String {
        if let ek = sub.eventKind {
            switch ek {
            case "started": return "开始工作"
            case "completed": return "已完成"
            case "interrupted": return "已中断"
            case "interacted": return "已交互"
            default: break
            }
        }
        switch sub.status {
        case "creating": return "正在创建"
        case "running": return "运行中"
        case "completed": return "已完成"
        case "failed": return "失败"
        case "interrupted": return "已中断"
        default: return "状态未知"
        }
    }

    private func buildSubagentRow(
        sub: SubagentActivity,
        supported: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let available = supported && sub.available && (sub.agentId?.isEmpty == false)
        let reason = supported ? sub.unavailableReason : "请更新当前机器服务以查看子智能体"

        let wrapper = NSStackView()
        wrapper.orientation = .vertical
        wrapper.alignment = .leading
        wrapper.spacing = 4
        wrapper.widthAnchor.constraint(equalToConstant: width).isActive = true

        let rowBox = ActionRowContainerView()
        rowBox.wantsLayer = true
        rowBox.layer?.cornerRadius = 10
        rowBox.layer?.backgroundColor = palette.surface.nsColor.cgColor
        rowBox.layer?.borderWidth = 1
        rowBox.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        rowBox.isEnabled = available
        rowBox.toolTip = reason
        rowBox.onTap = { [weak coordinator] in
            if available {
                coordinator?.onOpenSubagent?(sub)
            }
        }

        let rowStack = NSStackView()
        rowStack.orientation = .horizontal
        rowStack.alignment = .centerY
        rowStack.spacing = 8
        rowStack.translatesAutoresizingMaskIntoConstraints = false
        rowBox.addSubview(rowStack)

        let botIcon = NSImageView(
            image: LucideVectorCatalog.nsImage(for: .bot, size: 17, color: palette.textSecondary.nsColor)
        )
        botIcon.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            botIcon.widthAnchor.constraint(equalToConstant: 17),
            botIcon.heightAnchor.constraint(equalToConstant: 17),
        ])
        rowStack.addArrangedSubview(botIcon)

        let nameLbl = NSTextField(labelWithString: VisibleMarkdownEngine.visibleMarkdown(sub.name))
        nameLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        nameLbl.textColor = (available ? palette.textPrimary : palette.textSecondary).nsColor
        nameLbl.lineBreakMode = .byTruncatingTail
        rowStack.addArrangedSubview(nameLbl)

        rowStack.addArrangedSubview(NSView())

        if sub.status == "running" && (sub.eventKind == nil || sub.eventKind == "started") {
            let ring = AppKitRunningRingView(
                size: 13,
                trackColor: palette.borderStrong.nsColor,
                accentColor: palette.accent.nsColor
            )
            rowStack.addArrangedSubview(ring)
        }

        let verbLbl = NSTextField(labelWithString: Self.subagentActivityVerb(sub))
        verbLbl.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
        verbLbl.textColor = (sub.status == "failed" ? palette.errFg : palette.textMuted).nsColor
        rowStack.addArrangedSubview(verbLbl)

        if available {
            let chev = NSImageView(
                image: LucideVectorCatalog.nsImage(for: .chevronRight, size: 15, color: palette.textMuted.nsColor)
            )
            chev.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                chev.widthAnchor.constraint(equalToConstant: 15),
                chev.heightAnchor.constraint(equalToConstant: 15),
            ])
            rowStack.addArrangedSubview(chev)
        }

        rowBox.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            rowBox.widthAnchor.constraint(equalToConstant: width),
            rowBox.heightAnchor.constraint(equalToConstant: 38),
            rowStack.leadingAnchor.constraint(equalTo: rowBox.leadingAnchor, constant: 12),
            rowStack.trailingAnchor.constraint(equalTo: rowBox.trailingAnchor, constant: -12),
            rowStack.centerYAnchor.constraint(equalTo: rowBox.centerYAnchor),
        ])
        wrapper.addArrangedSubview(rowBox)

        if !available, let reason, !reason.isEmpty {
            let reasonLbl = NSTextField(labelWithString: reason)
            reasonLbl.font = DesignFonts.nsBodyFont(size: 11)
            reasonLbl.textColor = palette.textMuted.nsColor
            reasonLbl.lineBreakMode = .byTruncatingTail
            reasonLbl.widthAnchor.constraint(equalToConstant: width).isActive = true
            wrapper.addArrangedSubview(reasonLbl)
        }

        containerStack.addArrangedSubview(wrapper)
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

@MainActor
public final class IconFeedbackActionButton: NSButton {
    public private(set) var isShowingCopied: Bool = false
    public var onFeedbackStateChanged: (() -> Void)?
    private let dimension: CGFloat
    private let iconSize: CGFloat
    private let idleIcon: LucideIconName
    private let idleColor: NSColor
    private let okColor: NSColor
    private let hoverBg: NSColor
    private let onActivate: () -> Void
    private var resetTask: Task<Void, Never>?
    private var tracking: NSTrackingArea?

    public init(
        dimension: CGFloat = 30,
        iconSize: CGFloat = 17,
        idleIcon: LucideIconName = .copy,
        palette: DesignPalette,
        toolTipText: String,
        onActivate: @escaping () -> Void
    ) {
        self.dimension = dimension
        self.iconSize = iconSize
        self.idleIcon = idleIcon
        self.idleColor = palette.textMuted.nsColor
        self.okColor = palette.okFg.nsColor
        self.hoverBg = palette.hover.nsColor
        self.onActivate = onActivate
        super.init(frame: NSRect(x: 0, y: 0, width: dimension, height: dimension))
        isBordered = false
        bezelStyle = .regularSquare
        imagePosition = .imageOnly
        wantsLayer = true
        layer?.cornerRadius = 8
        toolTip = toolTipText
        setAccessibilityLabel(toolTipText)
        target = self
        action = #selector(handleClick)
        translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: dimension),
            heightAnchor.constraint(equalToConstant: dimension),
        ])
        refreshIcon()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override public func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        tracking = area
    }

    override public func mouseEntered(with event: NSEvent) {
        layer?.backgroundColor = hoverBg.cgColor
    }

    override public func mouseExited(with event: NSEvent) {
        layer?.backgroundColor = NSColor.clear.cgColor
    }

    @objc public func handleClick() {
        onActivate()
        resetTask?.cancel()
        isShowingCopied = true
        refreshIcon()
        onFeedbackStateChanged?()
        resetTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            guard !Task.isCancelled, let self else { return }
            self.isShowingCopied = false
            self.refreshIcon()
            self.onFeedbackStateChanged?()
        }
    }

    private func refreshIcon() {
        let icon = isShowingCopied ? LucideIconName.check : idleIcon
        let color = isShowingCopied ? okColor : idleColor
        image = LucideVectorCatalog.nsImage(for: icon, size: iconSize, color: color)
    }
}

@MainActor
private final class ActionRowContainerView: NSView {
    var isEnabled: Bool = true
    var onTap: (() -> Void)?

    override var acceptsFirstResponder: Bool { isEnabled }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        onTap?()
    }

    override func keyDown(with event: NSEvent) {
        guard isEnabled else {
            super.keyDown(with: event)
            return
        }
        if event.keyCode == 36 || event.keyCode == 76 || event.keyCode == 49 {
            onTap?()
            return
        }
        super.keyDown(with: event)
    }
}

@MainActor
private final class AppKitRunningRingView: NSView {
    private let size: CGFloat
    private let trackColor: NSColor
    private let accentColor: NSColor

    init(size: CGFloat = 13, trackColor: NSColor, accentColor: NSColor) {
        self.size = size
        self.trackColor = trackColor
        self.accentColor = accentColor
        super.init(frame: NSRect(x: 0, y: 0, width: size, height: size))
        translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: size),
            heightAnchor.constraint(equalToConstant: size),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func draw(_ dirtyRect: NSRect) {
        let inset = bounds.insetBy(dx: 1.5, dy: 1.5)
        let ring = NSBezierPath(ovalIn: inset)
        ring.lineWidth = 2.0
        trackColor.setStroke()
        ring.stroke()

        let arc = NSBezierPath()
        let center = NSPoint(x: bounds.midX, y: bounds.midY)
        arc.appendArc(withCenter: center, radius: inset.width * 0.5, startAngle: 20, endAngle: 120)
        arc.lineWidth = 2.0
        arc.lineCapStyle = .round
        accentColor.setStroke()
        arc.stroke()
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
    public var isLoadOlderBlockedByError: Bool

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
        isSubagent: Bool = false,
        isLoadOlderBlockedByError: Bool = false
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
        self.isLoadOlderBlockedByError = isLoadOlderBlockedByError
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

        coordinator.isLoadOlderBlockedByError = isLoadOlderBlockedByError
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
        coordinator.isLoadOlderBlockedByError = isLoadOlderBlockedByError
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
