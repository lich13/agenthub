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
    public var canLoadOlder: Bool = true
    public var isLoadingOlder: Bool = false
    private var highlightClearTask: Task<Void, Never>?
    private var isRestoringScroll = false
    private var lastLaidOutWidth: CGFloat = 0
    private var lastClipOriginY: CGFloat = .greatestFiniteMagnitude
    private var upwardScrollIntent: Bool = false
    private var lastReportedActiveTimelineId: String? = "__uninitialized__"
    private var userAnchorRowIndices: [Int] = []
    private var hasAnyUserImageAttachments: Bool = false

    private var cachedRowHeights: [CGFloat?] = []
    private var heightCacheWidthKey: Int = 0
    private var heightCacheDisclosureRev: UInt64 = 0
    private var heightCacheScheme: ColorSchemeKind = .light
    private var heightCacheWorkspace: WorkspaceId = .codex

    private static let maxCachedCells = 128
    private var cellCacheByRowId: [String: ConversationRowCellView] = [:]
    private var cellCacheOrder: [String] = []

    public struct ScrollAnchorSnapshot: Equatable, Sendable {
        public let timelineId: String
        public let deltaY: CGFloat
        public var savedScrollTop: CGFloat
        public var savedContentHeight: CGFloat

        public init(
            timelineId: String,
            deltaY: CGFloat,
            savedScrollTop: CGFloat = 0,
            savedContentHeight: CGFloat = 0
        ) {
            self.timelineId = timelineId
            self.deltaY = deltaY
            self.savedScrollTop = savedScrollTop
            self.savedContentHeight = savedContentHeight
        }
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
                    deltaY: rect.minY - viewportTop,
                    savedScrollTop: viewportTop,
                    savedContentHeight: tableView.frame.height
                )
            }
        }
        return nil
    }

    private func rebuildRowIndexesAndCarryHeightCache(nextRows: [ConversationRowItem], currentWidthKey: Int) {
        let canCarry = (currentWidthKey > 0
            && currentWidthKey == heightCacheWidthKey
            && disclosureStore.revision == heightCacheDisclosureRev
            && scheme == heightCacheScheme
            && workspace == heightCacheWorkspace
            && cachedRowHeights.count == rows.count)

        var previousById: [String: (item: ConversationRowItem, height: CGFloat)] = [:]
        if canCarry {
            previousById.reserveCapacity(rows.count)
            for (idx, oldRow) in rows.enumerated() {
                if let h = cachedRowHeights[idx] {
                    previousById[oldRow.id] = (oldRow, h)
                }
            }
        }

        var nextHeights = Array<CGFloat?>(repeating: nil, count: nextRows.count)
        var nextUserIndices: [Int] = []
        var anyImages = false

        for (idx, r) in nextRows.enumerated() {
            if canCarry, let prev = previousById[r.id], prev.item == r {
                nextHeights[idx] = prev.height
            }
            if case let .user(msg, _, namesOnly) = r.kind {
                nextUserIndices.append(idx)
                if !namesOnly, let atts = msg?.attachments, atts.contains(where: { $0.kind == "image" && $0.reason == nil }) {
                    anyImages = true
                }
            }
        }

        cachedRowHeights = nextHeights
        heightCacheWidthKey = currentWidthKey
        heightCacheDisclosureRev = disclosureStore.revision
        heightCacheScheme = scheme
        heightCacheWorkspace = workspace
        userAnchorRowIndices = nextUserIndices
        hasAnyUserImageAttachments = anyImages
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

    private func findAnchorRowIndex(for timelineId: String, in list: [ConversationRowItem]) -> Int? {
        if let exact = list.firstIndex(where: { $0.timelineId == timelineId }) {
            return exact
        }
        return list.firstIndex(where: { $0.timelineAliases.contains(timelineId) })
    }

    public func updateRowsPreservingAnchor(
        _ nextRows: [ConversationRowItem],
        preferAutoFollowBottom: Bool = false,
        forceScrollToBottom: Bool = false
    ) {
        let totalW = max(320, scrollView?.bounds.width ?? tableView?.bounds.width ?? 0)
        let widthKey = totalW > 0 ? Int((contentColumnWidth(for: totalW) * 2).rounded()) : 0
        rebuildRowIndexesAndCarryHeightCache(nextRows: nextRows, currentWidthKey: widthKey)

        guard let tableView, let clipView = scrollView?.contentView else {
            rows = nextRows
            return
        }
        let wasEmpty = rows.isEmpty
        if forceScrollToBottom {
            upwardScrollIntent = false
            cellCacheByRowId.removeAll()
            cellCacheOrder.removeAll()
        }
        let shouldFollow = forceScrollToBottom || (preferAutoFollowBottom && !upwardScrollIntent && (wasEmpty || isNearBottom()))
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
            } else if let anchor {
                if let newIdx = self.findAnchorRowIndex(for: anchor.timelineId, in: nextRows) {
                    let newRect = tableView.rect(ofRow: newIdx)
                    let restoredY = max(minY, newRect.minY - anchor.deltaY)
                    clipView.setBoundsOrigin(NSPoint(x: 0, y: restoredY))
                    self.scrollView?.reflectScrolledClipView(clipView)
                } else if anchor.savedContentHeight > 0 && abs(tableView.frame.height - anchor.savedContentHeight) > 0.5 {
                    let restoredY = max(minY, anchor.savedScrollTop + (tableView.frame.height - anchor.savedContentHeight))
                    clipView.setBoundsOrigin(NSPoint(x: 0, y: restoredY))
                    self.scrollView?.reflectScrolledClipView(clipView)
                }
            }
            self.lastClipOriginY = clipView.bounds.origin.y
        }
        isRestoringScroll = false
        refreshViewportAttachmentsAndTimeline()
    }

    public func restoreExplicitAnchor(_ anchor: ScrollAnchorSnapshot) {
        guard let tableView, let clipView = scrollView?.contentView,
              let idx = findAnchorRowIndex(for: anchor.timelineId, in: rows)
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
            self.lastClipOriginY = clipView.bounds.origin.y
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
            self.cachedRowHeights = Array<CGFloat?>(repeating: nil, count: self.rows.count)
            self.heightCacheDisclosureRev = self.disclosureStore.revision
            if let tableView = self.tableView {
                tableView.reloadData()
                tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0 ..< self.rows.count))
                tableView.tile()
                tableView.layoutSubtreeIfNeeded()
                let minY = -(self.scrollView?.contentInsets.top ?? 0)
                if let anchor,
                   let clipView = self.scrollView?.contentView,
                   let idx = self.findAnchorRowIndex(for: anchor.timelineId, in: self.rows)
                {
                    let newRect = tableView.rect(ofRow: idx)
                    let restoredY = max(minY, newRect.minY - anchor.deltaY)
                    clipView.setBoundsOrigin(NSPoint(x: 0, y: restoredY))
                    self.scrollView?.reflectScrolledClipView(clipView)
                    self.lastClipOriginY = restoredY
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
        // Auto-open disclosure if jumping to a folded group, thinking row, or instruction/context fold (DESIGN.md:43, 140)
        let targetRow = rows[rowIdx]
        var didMutateDisclosure = false
        switch targetRow.kind {
        case let .executionGroup(group):
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: group.id, isOpen: true)
            for cmd in group.commands where TimelineViewModel.matchesTimelinePosition(positionKey, id: cmd.id, aliases: cmd.sourceIds) {
                disclosureStore.setOpen(scope: sessionScopeKey, stateKey: cmd.id, isOpen: true)
            }
            didMutateDisclosure = true
        case let .processGroup(proc):
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: proc.stateKey, isOpen: true)
            for step in proc.steps {
                switch step {
                case let .execution(group, _):
                    for cmd in group.commands where TimelineViewModel.matchesTimelinePosition(positionKey, id: cmd.id, aliases: cmd.sourceIds) {
                        disclosureStore.setOpen(scope: sessionScopeKey, stateKey: cmd.id, isOpen: true)
                    }
                case let .detail(id, sids, _, _, _, _, _):
                    if TimelineViewModel.matchesTimelinePosition(positionKey, id: id, aliases: sids) {
                        disclosureStore.setOpen(scope: sessionScopeKey, stateKey: id, isOpen: true)
                    }
                case .thinking:
                    break
                }
            }
            didMutateDisclosure = true
        case let .claudeProcessGroup(proc, _):
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: proc.stateKey, isOpen: true)
            didMutateDisclosure = true
        case let .claudeMetadataGroup(meta, _):
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: meta.id, isOpen: true)
            didMutateDisclosure = true
        case .thinking:
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: targetRow.id, isOpen: true)
            didMutateDisclosure = true
        case let .user(msg, rawText, _):
            let proj = UserContextEngine.projectUserMessage(msg?.text ?? rawText, contextBlocks: msg?.contextBlocks)
            let segs = UserContextEngine.userMessageSegments(proj.text, contextBlocks: proj.contextBlocks)
            if !proj.contextBlocks.isEmpty && (segs.isEmpty || positionKey.contains("context") || positionKey.contains("instruction")) {
                let ctxKey = "\(msg?.id ?? targetRow.timelineId):context"
                disclosureStore.setOpen(scope: sessionScopeKey, stateKey: ctxKey, isOpen: true)
                didMutateDisclosure = true
            }
        case let .assistant(markdown, foldInstructions) where foldInstructions:
            let slices = VisibleMarkdownEngine.instructionSegments(VisibleMarkdownEngine.visibleMarkdown(markdown))
            for s in slices where s.kind == "instructions" {
                disclosureStore.setOpen(scope: sessionScopeKey, stateKey: "\(targetRow.id):\(s.id)", isOpen: true)
                didMutateDisclosure = true
            }
        default:
            break
        }
        if didMutateDisclosure {
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
        let originY = clipView.bounds.origin.y
        let movedUp = (lastClipOriginY == .greatestFiniteMagnitude) || (originY < lastClipOriginY - 0.5)
        if movedUp {
            upwardScrollIntent = true
        } else if originY > lastClipOriginY + 0.5 {
            upwardScrollIntent = false
        }
        lastClipOriginY = originY

        if upwardScrollIntent
            && originY <= 200
            && canLoadOlder
            && !isLoadingOlder
            && !isLoadOlderBlockedByError
        {
            onLoadOlderRequested?()
        }
        refreshViewportAttachmentsAndTimeline()
    }

    public func refreshViewportAttachmentsAndTimeline() {
        guard let tableView, let clipView = scrollView?.contentView else { return }

        if hasAnyUserImageAttachments || attachmentViewport.loadedCount > 0 {
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
        }

        // Compute active user timeline entry using precomputed user row indices
        if !userAnchorRowIndices.isEmpty {
            var userAnchors: [(id: String, top: CGFloat)] = []
            userAnchors.reserveCapacity(userAnchorRowIndices.count)
            for idx in userAnchorRowIndices where idx < rows.count {
                userAnchors.append((id: rows[idx].timelineId, top: tableView.rect(ofRow: idx).minY))
            }
            let activeId = TimelineViewModel.activeUserTimelineId(
                anchors: userAnchors,
                viewportTop: clipView.bounds.origin.y + 28
            )
            if activeId != lastReportedActiveTimelineId {
                lastReportedActiveTimelineId = activeId
                onActiveTimelineChanged?(activeId)
            }
        } else if lastReportedActiveTimelineId != nil {
            lastReportedActiveTimelineId = nil
            onActiveTimelineChanged?(nil)
        }
    }

    // MARK: - NSTableViewDataSource & Delegate

    public func numberOfRows(in tableView: NSTableView) -> Int {
        rows.count
    }

    public func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        guard row >= 0, row < rows.count else { return 44 }
        let colWidth = contentColumnWidth(for: max(320, scrollView?.bounds.width ?? tableView.bounds.width))
        let widthKey = Int((colWidth * 2).rounded())
        let discRev = disclosureStore.revision

        if widthKey != heightCacheWidthKey
            || discRev != heightCacheDisclosureRev
            || scheme != heightCacheScheme
            || workspace != heightCacheWorkspace
            || cachedRowHeights.count != rows.count
        {
            cachedRowHeights = Array<CGFloat?>(repeating: nil, count: rows.count)
            heightCacheWidthKey = widthKey
            heightCacheDisclosureRev = discRev
            heightCacheScheme = scheme
            heightCacheWorkspace = workspace
        }

        if let cached = cachedRowHeights[row] {
            return cached
        }
        let computed = max(32, ceil(measureRowHeight(rows[row], width: colWidth)))
        cachedRowHeights[row] = computed
        return computed
    }

    public func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        false
    }

    public func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row >= 0, row < rows.count else { return nil }
        let item = rows[row]
        let identifier = NSUserInterfaceItemIdentifier("ConversationRowCell")
        let cell: ConversationRowCellView
        if let cached = cellCacheByRowId[item.id], cached.superview == nil {
            cell = cached
        } else {
            let created = (tableView.makeView(withIdentifier: identifier, owner: nil) as? ConversationRowCellView)
                ?? ConversationRowCellView(identifier: identifier)
            if cellCacheByRowId[item.id] == nil {
                if cellCacheOrder.count >= Self.maxCachedCells, let oldest = cellCacheOrder.first {
                    cellCacheOrder.removeFirst()
                    cellCacheByRowId.removeValue(forKey: oldest)
                }
                cellCacheOrder.append(item.id)
            }
            cellCacheByRowId[item.id] = created
            cell = created
        }
        let totalWidth = max(320, scrollView?.bounds.width ?? tableView.bounds.width)
        let insets = LayoutMetrics.readingHorizontalInsets(
            availableWidth: totalWidth,
            hasTimelineRail: hasTimelineRail,
            isSubagent: isSubagent
        )
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
            var blockCount = 0
            if let atts = msg?.attachments, !atts.isEmpty {
                let hasImage = !namesOnly && atts.contains(where: { $0.kind == "image" && $0.reason == nil })
                h += hasImage ? 100 : 42
                blockCount += 1
            }
            let maxCardW = max(140, min(width * 0.85, 640))
            if !proj.contextBlocks.isEmpty {
                let ctxKey = "\(msg?.id ?? row.timelineId):context"
                let open = disclosureStore.isOpen(
                    scope: sessionScopeKey,
                    stateKey: ctxKey,
                    aliases: ["\(row.timelineId):context"],
                    initiallyOpen: false
                )
                if open {
                    let ctxText = proj.contextBlocks.map { UserContextEngine.visibleContextText($0.text) }.joined(separator: "\n\n")
                    h += 38 + Self.measureUserContextBodyHeight(text: ctxText, width: maxCardW - 24)
                } else {
                    h += 38
                }
                blockCount += 1
            }
            let bubbleHPad: CGFloat = (width <= 500) ? 16 : 18
            let para = NSMutableParagraphStyle()
            para.minimumLineHeight = 24.75 // 15pt * 1.65
            para.maximumLineHeight = 24.75
            let attrs: [NSAttributedString.Key: Any] = [
                .font: DesignFonts.nsBodyFont(size: 15),
                .paragraphStyle: para,
            ]
            for seg in segs {
                blockCount += 1
                switch seg {
                case let .text(_, t):
                    let attr = NSAttributedString(string: t, attributes: attrs)
                    let textW = min(maxCardW - bubbleHPad * 2, max(64, ceil(attr.size().width) + 8))
                    let textH = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: textW))
                    h += textH + 24
                case let .reply(segId, q, a):
                    let trimmedQ = q.trimmingCharacters(in: .whitespacesAndNewlines)
                    let availW = max(80, maxCardW - bubbleHPad * 2 - 38)
                    var qExtra: CGFloat = 0
                    if !trimmedQ.isEmpty {
                        let qKey = "\(msg?.id ?? row.timelineId):\(segId):question"
                        let qOpen = disclosureStore.isOpen(
                            scope: sessionScopeKey,
                            stateKey: qKey,
                            aliases: ["\(row.timelineId):\(segId):question"],
                            initiallyOpen: false
                        )
                        if qOpen {
                            let qAttr = NSAttributedString(
                                string: trimmedQ,
                                attributes: [.font: DesignFonts.nsBodyFont(size: 13)]
                            )
                            let qW = min(availW, max(80, ceil(qAttr.size().width) + 8))
                            qExtra = 28 + max(20, ReaderSelectableTextView.measureHeight(attributed: qAttr, width: qW)) + 6
                        } else {
                            qExtra = 28
                        }
                    }
                    let aAttr = NSAttributedString(string: a, attributes: attrs)
                    let aW = min(availW, max(64, ceil(aAttr.size().width) + 8))
                    let aH = max(24, ReaderSelectableTextView.measureHeight(attributed: aAttr, width: aW))
                    h += qExtra + aH + 24
                }
            }
            if blockCount > 1 {
                h += CGFloat(blockCount - 1) * 10
            }
            return max(44, h + 4)

        case let .assistant(markdown, foldInstructions):
            // Side-hover copy button is floating in the right margin and NEVER adds vertical height (0.0pt impact)!
            return measureMarkdownSegmentsHeight(ownerId: row.id, markdown: markdown, foldInstructions: foldInstructions, width: width) + 8

        case let .executionGroup(group):
            let aliases = group.mergedIds + group.commands.map { "group:\($0.id)" }
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: group.id,
                aliases: aliases,
                initiallyOpen: group.initiallyOpen
            )
            if !isOpen { return 34 }
            return 34 + 20 + measureExecutionCommandsHeight(commands: group.commands, width: max(120, width - 20))

        case let .processGroup(proc):
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: proc.stateKey,
                aliases: proc.stateAliases,
                initiallyOpen: proc.failedCount > 0
            )
            if !isOpen { return 34 }
            let bodyW = max(120, width - 22)
            var bodyH: CGFloat = 14
            for (idx, step) in proc.steps.enumerated() {
                if idx > 0 { bodyH += 8 }
                switch step {
                case let .thinking(id, _, text, _, _):
                    bodyH += measureThoughtMarkdownHeight(ownerId: "\(id):thinking", markdown: text, width: bodyW)
                case let .execution(group, _):
                    bodyH += 8 + measureExecutionCommandsHeight(commands: group.commands, width: max(100, bodyW - 20))
                case let .detail(id, sids, _, detail, running, failed, _):
                    let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: id, aliases: sids, initiallyOpen: running || failed)
                    bodyH += open ? (32 + 8 + Self.measureMonospaceBoxHeight(text: detail, width: bodyW)) : 32
                }
            }
            return 34 + bodyH

        case let .claudeProcessGroup(proc, _):
            let aliases = proc.sourceIds.map { "claude-process:\(proc.turnId):\($0)" }
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: proc.stateKey,
                aliases: aliases,
                initiallyOpen: false
            )
            if !isOpen { return 34 }
            let bodyW = max(120, width - 22)
            return 34 + 14 + measureClaudeEntriesHeight(entries: proc.entries, width: bodyW)

        case let .claudeMetadataGroup(meta, _):
            let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
            let isOpen = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: meta.id, aliases: aliases, initiallyOpen: false)
            if !isOpen { return 34 }
            let bodyW = max(120, width - 22)
            let items = meta.entries.map { entry -> ClaudeEventRenderItem in
                switch entry {
                case let .item(it, key, sids): .item(item: it, key: key, sourceIds: sids)
                case let .group(g): .group(g)
                }
            }
            return 34 + 12 + measureClaudeEntriesHeight(entries: items, width: bodyW)

        case let .thinking(_, text, running):
            let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: row.id, initiallyOpen: running)
            if !open { return 34 }
            return 34 + 14 + measureThoughtMarkdownHeight(ownerId: row.id, markdown: text, width: max(120, width - 22))

        case let .plan(markdown, _, _):
            return 72 + measureMarkdownSegmentsHeight(ownerId: row.id, markdown: markdown, foldInstructions: false, width: max(120, width - 35))

        case let .question(_, questions):
            return Self.measureQuestionsHeight(questions: questions, answers: nil, width: width)

        case let .questionResult(_, questions, answers):
            return Self.measureQuestionsHeight(questions: questions, answers: answers, width: width)

        case let .subagent(sub, supported):
            let available = supported && sub.available && (sub.agentId?.isEmpty == false)
            let reason = supported ? sub.unavailableReason : "请更新当前机器服务以查看子智能体"
            return (!available && (reason?.isEmpty == false)) ? 54 : 34

        case .historyCollapse:
            return 64

        case .systemNotice:
            return 36
        }
    }

    private func measureExecutionCommandsHeight(commands: [ExecutionCommand], width: CGFloat) -> CGFloat {
        var h: CGFloat = 0
        for (idx, cmd) in commands.enumerated() {
            let cmdOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: cmd.id,
                aliases: cmd.sourceIds,
                initiallyOpen: !cmd.instructionFile && (cmd.status == .running || cmd.status == .failed)
            )
            h += 32
            if cmdOpen {
                let detail = cmd.sections.map { sec in
                    sec.label.isEmpty ? sec.text : "\(sec.label)\n\(sec.text)"
                }.joined(separator: "\n\n")
                h += 8 + Self.measureMonospaceBoxHeight(text: detail.isEmpty ? "暂无输出" : detail, width: width) + 6
            }
            if idx < commands.count - 1 {
                h += 7
            }
        }
        return h
    }

    private func measureClaudeEntriesHeight(entries: [ClaudeEventRenderItem], width: CGFloat) -> CGFloat {
        var total: CGFloat = 0
        for (idx, entry) in entries.enumerated() {
            if idx > 0 { total += 8 }
            switch entry {
            case let .group(group):
                let aliases = group.mergedIds + group.commands.map { "group:\($0.id)" }
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: group.id, aliases: aliases, initiallyOpen: group.initiallyOpen)
                total += open ? (34 + 10 + measureExecutionCommandsHeight(commands: group.commands, width: max(100, width - 16))) : 34
            case let .metadata(meta):
                let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: meta.id, aliases: aliases, initiallyOpen: false)
                if open {
                    let subItems = meta.entries.map { e -> ClaudeEventRenderItem in
                        switch e {
                        case let .item(it, k, sids): .item(item: it, key: k, sourceIds: sids)
                        case let .group(g): .group(g)
                        }
                    }
                    total += 34 + 8 + measureClaudeEntriesHeight(entries: subItems, width: max(100, width - 16))
                } else {
                    total += 34
                }
            case let .item(ev, key, _):
                if ev.kind == "thinking" {
                    total += measureThoughtMarkdownHeight(ownerId: key, markdown: ev.text ?? "", width: width)
                } else if let detail = ev.detail, !detail.isEmpty {
                    let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: key, initiallyOpen: false)
                    total += open ? (32 + 8 + Self.measureMonospaceBoxHeight(text: detail, width: width)) : 32
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    total += measureThoughtMarkdownHeight(ownerId: key, markdown: text, width: width)
                }
            }
        }
        return total
    }

    public static func renderThoughtAttributedString(markdown: String, palette: DesignPalette) -> NSAttributedString {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(markdown)
        let para = NSMutableParagraphStyle()
        para.minimumLineHeight = 20.8
        para.maximumLineHeight = 20.8
        let baseAttrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsBodyFont(size: 13),
            .foregroundColor: palette.textMuted.nsColor,
            .paragraphStyle: para,
        ]
        let codeAttrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsMonoFont(size: 12),
            .foregroundColor: palette.textSecondary.nsColor,
            .paragraphStyle: para,
            .agentHubInlineCode: palette.hover.nsColor,
        ]
        let out = NSMutableAttributedString()
        let parts = cleaned.components(separatedBy: "`")
        for (idx, part) in parts.enumerated() {
            if idx % 2 == 1 && idx < parts.count - 1 && !part.isEmpty && !part.contains("\n") {
                out.append(NSAttributedString(string: "\u{202F}\(part)\u{202F}", attributes: codeAttrs))
            } else {
                let prefix = (idx % 2 == 1 && idx == parts.count - 1) ? "`" : ""
                out.append(NSAttributedString(string: prefix + part, attributes: baseAttrs))
            }
        }
        return out
    }

    private func measureThoughtMarkdownHeight(ownerId _: String, markdown: String, width: CGFloat) -> CGFloat {
        let attr = Self.renderThoughtAttributedString(markdown: markdown, palette: palette)
        return max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: width)) + 4
    }

    public static func measureUserContextBodyHeight(text: String, width: CGFloat) -> CGFloat {
        let para = NSMutableParagraphStyle()
        para.minimumLineHeight = 23.8 // 14pt * 1.7
        para.maximumLineHeight = 23.8
        let attr = NSAttributedString(
            string: text.isEmpty ? " " : text,
            attributes: [
                .font: DesignFonts.nsBodyFont(size: 14),
                .paragraphStyle: para,
            ]
        )
        let measured = ReaderSelectableTextView.measureHeight(attributed: attr, width: max(80, width))
        return min(300, max(32, measured + 20))
    }

    public static func normalizeCodeBlockText(_ code: String) -> String {
        if code.hasSuffix("\r\n") {
            return String(code.dropLast(2))
        }
        if code.hasSuffix("\n") {
            return String(code.dropLast())
        }
        return code
    }

    public static func measureCodeBlockHeight(code: String) -> CGFloat {
        let normalized = normalizeCodeBlockText(code)
        let lines = max(1, normalized.components(separatedBy: "\n").count)
        return ceil(CGFloat(lines) * 20.8) + 16
    }

    public static func measureCodeDocumentWidth(code: String, minWidth: CGFloat) -> CGFloat {
        let normalized = normalizeCodeBlockText(code)
        let attr = NSAttributedString(
            string: normalized.isEmpty ? " " : normalized,
            attributes: [.font: DesignFonts.nsMonoFont(size: 13)]
        )
        return max(minWidth, ceil(attr.size().width) + 24)
    }

    public static func measureTableLayout(
        headers: [NSAttributedString],
        rows: [[NSAttributedString]],
        maxWidth: CGFloat
    ) -> (colWidths: [CGFloat], rowHeights: [CGFloat], totalWidth: CGFloat, totalHeight: CGFloat) {
        let colCount = max(1, headers.count)
        let allRows = [headers] + rows
        var colWidths = Array(repeating: CGFloat(64), count: colCount)
        for cols in allRows {
            for cIdx in 0 ..< min(colCount, cols.count) {
                let natural = ceil(cols[cIdx].size().width) + 28
                colWidths[cIdx] = max(colWidths[cIdx], min(360, natural))
            }
        }
        var rowHeights: [CGFloat] = []
        for cols in allRows {
            var maxCellH: CGFloat = 19
            for cIdx in 0 ..< min(colCount, cols.count) {
                let innerW = max(36, colWidths[cIdx] - 24)
                let h = ReaderSelectableTextView.measureHeight(attributed: cols[cIdx], width: innerW)
                maxCellH = max(maxCellH, h)
            }
            rowHeights.append(max(35, maxCellH + 16))
        }
        let totalW = colWidths.reduce(0, +)
        let totalH = rowHeights.reduce(0, +)
        return (colWidths, rowHeights, max(120, totalW), totalH)
    }

    private static func measureQuestionsHeight(
        questions: [UserInputQuestion],
        answers: [UserInputAnswer]?,
        width: CGFloat
    ) -> CGFloat {
        let innerW = max(120, width - 32)
        var total: CGFloat = 28 + 20 + 10 // padding 14*2 + meta + gap
        let qFont = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
        for q in questions {
            let qAttr = NSAttributedString(string: q.question, attributes: [.font: qFont])
            let qH = max(20, ReaderSelectableTextView.measureHeight(attributed: qAttr, width: innerW))
            total += qH + 10
            for opt in q.options {
                let hasDesc = (opt.description?.isEmpty == false)
                total += hasDesc ? 56 : 38
            }
            total += 6
        }
        if let answers {
            if !answers.isEmpty {
                total += CGFloat(answers.count) * 44 + CGFloat(max(0, answers.count - 1)) * 8 + 6
            } else {
                total += 26
            }
        }
        return max(84, total)
    }

    public static func measureMonospaceBoxHeight(text: String, width: CGFloat) -> CGFloat {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
        let attr = NSAttributedString(
            string: cleaned.isEmpty ? " " : cleaned,
            attributes: [.font: DesignFonts.nsMonoFont(size: 12)]
        )
        let measured = ReaderSelectableTextView.measureHeight(attributed: attr, width: max(80, width - 26))
        return min(240, max(48, measured + 22))
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
                total += Self.measureCodeBlockHeight(code: code) + 54
            case let .table(_, headers, rows):
                let layout = Self.measureTableLayout(headers: headers, rows: rows, maxWidth: width)
                total += layout.totalHeight + 16
            case .thematicBreak:
                total += 21
            case let .instructionsFold(id, text, _, _):
                let scopedKey = "\(ownerId):\(id)"
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: scopedKey, initiallyOpen: false)
                if open {
                    total += 46 + measureMarkdownSegmentsHeight(ownerId: scopedKey, markdown: text, foldInstructions: false, width: max(120, width - 24))
                } else {
                    total += 42
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
    public private(set) var questionReplyCopyButtons: [IconFeedbackActionButton] = []
    private var hoverTrackingArea: NSTrackingArea?
    public private(set) var isRowHovered: Bool = false {
        didSet { updateFloatingCopyVisibility() }
    }
    private var configuredLeadingInset: CGFloat = 0
    private var configuredContentWidth: CGFloat = 0
    private var stackConstraints: [NSLayoutConstraint] = []

    private struct CellConfigSignature: Equatable {
        let item: ConversationRowItem
        let leadingInset: CGFloat
        let contentWidth: CGFloat
        let highlighted: Bool
        let scheme: ColorSchemeKind
        let workspace: WorkspaceId
        let disclosureRev: UInt64
    }

    private var lastConfiguredSignature: CellConfigSignature?

    public init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier
        clipsToBounds = false
        wantsLayer = true
        containerStack.clipsToBounds = false
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
        let narrowMobile = bounds.width > 0 && bounds.width <= LayoutMetrics.mobileBreakpoint
        let firstRespView = window?.firstResponder as? NSView
        let focusedInside = firstRespView.map { $0 === self || $0.isDescendant(of: self) } ?? false

        if let btn = floatingSideCopyButton {
            let visible = isRowHovered || btn.isShowingCopied || focusedInside || narrowMobile
            btn.alphaValue = visible ? 1.0 : 0.0
        }
        for qBtn in questionReplyCopyButtons {
            let visible = isRowHovered || qBtn.isShowingCopied || focusedInside || narrowMobile
            qBtn.alphaValue = visible ? 1.0 : 0.0
        }
    }

    public func configure(
        item: ConversationRowItem,
        leadingInset: CGFloat,
        trailingInset: CGFloat,
        contentWidth: CGFloat,
        highlighted: Bool,
        coordinator: ConversationTableCoordinator
    ) {
        let nextSig = CellConfigSignature(
            item: item,
            leadingInset: leadingInset,
            contentWidth: contentWidth,
            highlighted: highlighted,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            disclosureRev: coordinator.disclosureStore.revision
        )
        if lastConfiguredSignature == nextSig, !containerStack.arrangedSubviews.isEmpty {
            positionFloatingCopyButton()
            return
        }
        lastConfiguredSignature = nextSig

        configuredLeadingInset = leadingInset
        configuredContentWidth = contentWidth
        floatingSideCopyButton?.removeFromSuperview()
        floatingSideCopyButton = nil
        questionReplyCopyButtons.removeAll()
        isRowHovered = false

        containerStack.arrangedSubviews.forEach { v in
            containerStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        NSLayoutConstraint.deactivate(stackConstraints)
        stackConstraints = [
            containerStack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: leadingInset),
            containerStack.topAnchor.constraint(equalTo: topAnchor, constant: 2),
        ]
        NSLayoutConstraint.activate(stackConstraints)

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
        case let .processGroup(proc):
            buildProcessGroupView(proc: proc, width: contentWidth, coordinator: coordinator)
        case let .claudeProcessGroup(proc, _):
            buildClaudeProcessGroupView(proc: proc, width: contentWidth, coordinator: coordinator)
        case let .claudeMetadataGroup(meta, _):
            buildClaudeMetadataGroupView(meta: meta, width: contentWidth, coordinator: coordinator)
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
        let identity = msg?.id ?? item.timelineId

        let rightAlignRow = NSStackView()
        rightAlignRow.orientation = .vertical
        rightAlignRow.alignment = .trailing
        rightAlignRow.spacing = 10
        rightAlignRow.translatesAutoresizingMaskIntoConstraints = false
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
                    thumbBtn.layer?.cornerRadius = 12
                    thumbBtn.layer?.masksToBounds = true
                    thumbBtn.layer?.backgroundColor = palette.surface.nsColor.cgColor
                    thumbBtn.layer?.borderWidth = 1
                    thumbBtn.layer?.borderColor = palette.borderStrong.nsColor.cgColor
                    thumbBtn.translatesAutoresizingMaskIntoConstraints = false
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

        let maxBubbleW = max(140, min(width * 0.85, 640))
        if !proj.contextBlocks.isEmpty {
            let ctxKey = "\(identity):context"
            let open = coordinator.disclosureStore.isOpen(
                scope: coordinator.sessionScopeKey,
                stateKey: ctxKey,
                aliases: ["\(item.timelineId):context"],
                initiallyOpen: false
            )
            let ctxText = proj.contextBlocks.map { UserContextEngine.visibleContextText($0.text) }.joined(separator: "\n\n")
            rightAlignRow.addArrangedSubview(
                makeUserInstructionsCard(
                    stateKey: ctxKey,
                    open: open,
                    text: ctxText,
                    maxCardWidth: maxBubbleW,
                    palette: palette,
                    coordinator: coordinator
                )
            )
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

        for seg in segs {
            switch seg {
            case let .text(_, t):
                let bubble = makeUserBubbleContainer(palette: palette)
                let attr = NSAttributedString(string: t, attributes: attrs)
                let textW = min(maxBubbleW - hPad * 2, max(36, ceil(attr.size().width) + 4))
                let textH = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: textW))
                let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: textW, height: textH))
                tv.applyAttributedText(attr, width: textW)
                tv.translatesAutoresizingMaskIntoConstraints = false
                bubble.addSubview(tv)
                NSLayoutConstraint.activate([
                    tv.leadingAnchor.constraint(equalTo: bubble.leadingAnchor, constant: hPad),
                    tv.trailingAnchor.constraint(equalTo: bubble.trailingAnchor, constant: -hPad),
                    tv.topAnchor.constraint(equalTo: bubble.topAnchor, constant: vPad),
                    tv.bottomAnchor.constraint(equalTo: bubble.bottomAnchor, constant: -vPad),
                    tv.widthAnchor.constraint(equalToConstant: textW),
                    tv.heightAnchor.constraint(equalToConstant: textH),
                ])
                rightAlignRow.addArrangedSubview(bubble)

            case let .reply(segId, q, a):
                let replyRow = NSStackView()
                replyRow.orientation = .horizontal
                replyRow.alignment = .centerY
                replyRow.spacing = 8

                let copyAnsBtn = IconFeedbackActionButton(
                    dimension: 30,
                    iconSize: 17,
                    idleIcon: .copy,
                    palette: palette,
                    toolTipText: "复制回答"
                ) { [weak self, weak coordinator] in
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(a, forType: .string)
                    coordinator?.onCopyFeedback?("已复制回答")
                    self?.updateFloatingCopyVisibility()
                }
                copyAnsBtn.onFeedbackStateChanged = { [weak self] in
                    self?.updateFloatingCopyVisibility()
                }
                questionReplyCopyButtons.append(copyAnsBtn)

                let bubble = makeUserBubbleContainer(palette: palette)
                let innerStack = NSStackView()
                innerStack.orientation = .vertical
                innerStack.alignment = .leading
                innerStack.spacing = 6
                innerStack.translatesAutoresizingMaskIntoConstraints = false
                bubble.addSubview(innerStack)

                let trimmedQ = q.trimmingCharacters(in: .whitespacesAndNewlines)
                let availBubbleTextW = max(80, maxBubbleW - hPad * 2 - 38)
                var computedAnsW: CGFloat = 64
                let ansAttr = NSAttributedString(string: a, attributes: attrs)
                computedAnsW = min(availBubbleTextW, max(64, ceil(ansAttr.size().width) + 4))

                if !trimmedQ.isEmpty {
                    let qKey = "\(identity):\(segId):question"
                    let qOpen = coordinator.disclosureStore.isOpen(
                        scope: coordinator.sessionScopeKey,
                        stateKey: qKey,
                        aliases: ["\(item.timelineId):\(segId):question"],
                        initiallyOpen: false
                    )
                    let qSingleWidth = ceil(NSAttributedString(
                        string: trimmedQ,
                        attributes: [.font: DesignFonts.nsBodyFont(size: 13)]
                    ).size().width) + 26
                    let targetInnerW = min(availBubbleTextW, max(computedAnsW, qSingleWidth))

                    let qHeaderBox = ActionRowContainerView()
                    qHeaderBox.translatesAutoresizingMaskIntoConstraints = false
                    qHeaderBox.onTap = { [weak coordinator] in
                        coordinator?.toggleDisclosurePreservingAnchor(stateKey: qKey, currentlyOpen: qOpen)
                    }
                    let qHeaderStack = NSStackView()
                    qHeaderStack.orientation = .horizontal
                    qHeaderStack.alignment = .centerY
                    qHeaderStack.spacing = 6
                    qHeaderStack.translatesAutoresizingMaskIntoConstraints = false
                    qHeaderBox.addSubview(qHeaderStack)

                    let qSummaryLbl = NSTextField(labelWithString: trimmedQ)
                    qSummaryLbl.font = DesignFonts.nsBodyFont(size: 13)
                    qSummaryLbl.textColor = palette.textSecondary.nsColor.withAlphaComponent(0.84)
                    qSummaryLbl.lineBreakMode = .byTruncatingTail
                    qSummaryLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

                    let qChev = NSImageView(
                        image: LucideVectorCatalog.nsImage(
                            for: qOpen ? .chevronDown : .chevronRight,
                            size: 15,
                            color: (qOpen ? palette.accent : palette.textMuted).nsColor
                        )
                    )
                    qChev.translatesAutoresizingMaskIntoConstraints = false
                    NSLayoutConstraint.activate([
                        qChev.widthAnchor.constraint(equalToConstant: 15),
                        qChev.heightAnchor.constraint(equalToConstant: 15),
                        qHeaderBox.widthAnchor.constraint(equalToConstant: targetInnerW),
                        qHeaderBox.heightAnchor.constraint(equalToConstant: 22),
                        qHeaderStack.leadingAnchor.constraint(equalTo: qHeaderBox.leadingAnchor),
                        qHeaderStack.trailingAnchor.constraint(equalTo: qHeaderBox.trailingAnchor),
                        qHeaderStack.centerYAnchor.constraint(equalTo: qHeaderBox.centerYAnchor),
                    ])
                    qHeaderStack.addArrangedSubview(qSummaryLbl)
                    qHeaderStack.addArrangedSubview(NSView())
                    qHeaderStack.addArrangedSubview(qChev)
                    innerStack.addArrangedSubview(qHeaderBox)

                    if qOpen {
                        let qAttr = NSAttributedString(
                            string: trimmedQ,
                            attributes: [
                                .font: DesignFonts.nsBodyFont(size: 13),
                                .foregroundColor: palette.textSecondary.nsColor,
                            ]
                        )
                        let qH = max(20, ReaderSelectableTextView.measureHeight(attributed: qAttr, width: targetInnerW))
                        let qFull = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: targetInnerW, height: qH))
                        qFull.applyAttributedText(qAttr, width: targetInnerW)
                        qFull.translatesAutoresizingMaskIntoConstraints = false
                        NSLayoutConstraint.activate([
                            qFull.widthAnchor.constraint(equalToConstant: targetInnerW),
                            qFull.heightAnchor.constraint(equalToConstant: qH),
                        ])
                        innerStack.addArrangedSubview(qFull)
                    }
                    computedAnsW = max(computedAnsW, targetInnerW)
                }

                let ansH = max(24, ReaderSelectableTextView.measureHeight(attributed: ansAttr, width: computedAnsW))
                let ansTv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: computedAnsW, height: ansH))
                ansTv.applyAttributedText(ansAttr, width: computedAnsW)
                ansTv.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    ansTv.widthAnchor.constraint(equalToConstant: computedAnsW),
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
        bubble.translatesAutoresizingMaskIntoConstraints = false
        bubble.wantsLayer = true
        bubble.layer?.cornerRadius = 20
        bubble.layer?.backgroundColor = palette.surface.nsColor.cgColor
        bubble.layer?.borderWidth = 1
        bubble.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        return bubble
    }

    private func makeUserInstructionsCard(
        stateKey: String,
        open: Bool,
        text: String,
        maxCardWidth: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 10
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let header = ActionRowContainerView()
        header.translatesAutoresizingMaskIntoConstraints = false
        header.onTap = { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: stateKey, currentlyOpen: open)
        }
        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 10
        hStack.translatesAutoresizingMaskIntoConstraints = false
        header.addSubview(hStack)

        let titleLbl = NSTextField(labelWithString: "上下文")
        titleLbl.font = DesignFonts.nsBodyFont(size: 14)
        titleLbl.textColor = palette.textPrimary.nsColor

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
            header.heightAnchor.constraint(equalToConstant: 30),
            hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
        ])
        hStack.addArrangedSubview(titleLbl)
        if open {
            hStack.addArrangedSubview(NSView())
        }
        hStack.addArrangedSubview(chev)
        card.addSubview(header)

        if !open {
            NSLayoutConstraint.activate([
                header.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
                header.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
                header.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
                header.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -4),
            ])
            return card
        }

        let innerW = max(120, maxCardWidth - 24)
        let para = NSMutableParagraphStyle()
        para.minimumLineHeight = 23.8 // 14pt * 1.7
        para.maximumLineHeight = 23.8
        let attr = NSAttributedString(
            string: text,
            attributes: [
                .font: DesignFonts.nsBodyFont(size: 14),
                .foregroundColor: palette.textPrimary.nsColor,
                .paragraphStyle: para,
            ]
        )
        let measuredH = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: innerW))
        let bodyBoxH = min(300, max(32, measuredH + 20))

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: true)
        scroll.hasVerticalScroller = measuredH + 20 > bodyBoxH
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: innerW, height: max(bodyBoxH, measuredH + 16)))
        tv.textContainerInset = NSSize(width: 0, height: 8)
        tv.applyAttributedText(attr, width: innerW)
        scroll.documentView = tv
        card.addSubview(scroll)

        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: maxCardWidth),
            header.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            header.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            header.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
            scroll.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            scroll.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 2),
            scroll.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -2),
            scroll.heightAnchor.constraint(equalToConstant: bodyBoxH),
        ])
        return card
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
        card.layer?.cornerRadius = 12
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        card.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 10)

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
                    .font: DesignFonts.nsBodyFont(size: 14),
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
            nameLbl.font = DesignFonts.nsBodyFont(size: 14)
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
                iconSize: 14,
                idleIcon: .copy,
                palette: palette,
                toolTipText: "复制文件路径"
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
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil
    ) {
        let destStack = targetStack ?? containerStack
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
                    quoteWrap.translatesAutoresizingMaskIntoConstraints = false
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
                    destStack.addArrangedSubview(quoteWrap)
                } else {
                    let tv = makeProseTextView(attr: attr, width: width, coordinator: coordinator)
                    destStack.addArrangedSubview(tv)
                }

            case let .codeBlock(lang, code):
                destStack.addArrangedSubview(makeCodeBlockView(lang: lang, code: code, width: width, coordinator: coordinator))

            case let .table(_, headers, rows):
                destStack.addArrangedSubview(makeTableView(headers: headers, rows: rows, width: width, palette: palette, coordinator: coordinator))

            case .thematicBreak:
                let rule = NSView()
                rule.wantsLayer = true
                rule.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                rule.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    rule.widthAnchor.constraint(equalToConstant: width),
                    rule.heightAnchor.constraint(equalToConstant: 1),
                ])
                destStack.addArrangedSubview(rule)

            case let .instructionsFold(id, text, lines, bytes):
                let scopedKey = "\(ownerId):\(id)"
                let open = coordinator.disclosureStore.isOpen(scope: coordinator.sessionScopeKey, stateKey: scopedKey, initiallyOpen: false)
                destStack.addArrangedSubview(
                    makeInstructionFileCard(
                        scopedKey: scopedKey,
                        title: "AGENTS.md",
                        meta: "\(lines) 行 · \(bytes) 字节",
                        markdown: text,
                        open: open,
                        width: width,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            }
        }
    }

    private func makeInstructionFileCard(
        scopedKey: String,
        title: String,
        meta: String,
        markdown: String,
        open: Bool,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 10
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let header = ActionRowContainerView()
        header.translatesAutoresizingMaskIntoConstraints = false
        header.onTap = { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: scopedKey, currentlyOpen: open)
        }
        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 8
        hStack.translatesAutoresizingMaskIntoConstraints = false
        header.addSubview(hStack)

        let titleLbl = NSTextField(labelWithString: title)
        titleLbl.font = DesignFonts.nsBodyFont(size: 14)
        titleLbl.textColor = palette.textPrimary.nsColor

        let metaLbl = NSTextField(labelWithString: meta)
        metaLbl.font = DesignFonts.nsBodyFont(size: 12)
        metaLbl.textColor = palette.textMuted.nsColor

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
            header.heightAnchor.constraint(equalToConstant: 30),
            hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
        ])
        hStack.addArrangedSubview(titleLbl)
        hStack.addArrangedSubview(NSView())
        hStack.addArrangedSubview(metaLbl)
        hStack.addArrangedSubview(chev)
        card.addSubview(header)

        if !open {
            NSLayoutConstraint.activate([
                header.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
                header.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
                header.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
                header.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -4),
            ])
            return card
        }

        let innerStack = NSStackView()
        innerStack.orientation = .vertical
        innerStack.alignment = .leading
        innerStack.spacing = 8
        innerStack.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(innerStack)
        buildMarkdownViews(
            ownerId: scopedKey,
            markdown: markdown,
            foldInstructions: false,
            width: max(120, width - 24),
            coordinator: coordinator,
            into: innerStack
        )
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            header.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            header.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
            innerStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            innerStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            innerStack.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 10),
            innerStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -10),
        ])
        return card
    }

    private func handleActivatedHref(_ href: String, coordinator: ConversationTableCoordinator) -> Bool {
        if href.hasPrefix("agenthub-copy-file:") {
            let rawFileHref = String(href.dropFirst("agenthub-copy-file:".count))
            let target = FileLinksAndPlanExport.fileLinkTarget(rawFileHref, cwd: coordinator.cwd)
            let copyPath = target?.path ?? rawFileHref
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(copyPath, forType: .string)
            coordinator.onCopyFeedback?(
                (target?.resolved == false) ? "已复制原路径；缺少工作目录，无法定位。" : "已复制路径"
            )
            return true
        }
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
        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: width, height: 24))
        tv.quoteBackgroundColor = palette.surface.nsColor
        tv.quoteAccentColor = palette.accent.nsColor
        tv.applyAttributedText(attr, width: width)
        let h = max(22, tv.measuredUsedHeight())
        tv.setFrameSize(NSSize(width: width, height: h))
        tv.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            tv.widthAnchor.constraint(equalToConstant: width),
            tv.heightAnchor.constraint(equalToConstant: h),
        ])
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        tv.onFocusStateChanged = { [weak self] in
            self?.updateFloatingCopyVisibility()
        }
        return tv
    }

    private func makeCodeBlockView(
        lang _: String?,
        code: String,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let palette = coordinator.palette
        let normalizedCode = ConversationTableCoordinator.normalizeCodeBlockText(code)
        let box = NSView()
        box.translatesAutoresizingMaskIntoConstraints = false
        box.wantsLayer = true
        box.layer?.cornerRadius = 12
        box.layer?.backgroundColor = palette.codeBg.nsColor.cgColor
        box.layer?.borderWidth = 1
        box.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let copyBtn = IconFeedbackActionButton(
            dimension: 32,
            iconSize: 15,
            idleIcon: .copy,
            palette: palette,
            borderedSurface: palette.elevated.nsColor,
            borderColor: palette.borderStrong.nsColor,
            toolTipText: "复制代码"
        ) { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(normalizedCode, forType: .string)
            coordinator?.onCopyFeedback?("已复制代码")
        }

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: false)
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let codeHeight = ConversationTableCoordinator.measureCodeBlockHeight(code: normalizedCode)
        let docWidth = ConversationTableCoordinator.measureCodeDocumentWidth(code: normalizedCode, minWidth: width - 32)

        let pStyle = NSMutableParagraphStyle()
        pStyle.minimumLineHeight = 20.8 // 13pt * 1.6
        pStyle.maximumLineHeight = 20.8
        let codeAttr = NSAttributedString(
            string: normalizedCode,
            attributes: [
                .font: DesignFonts.nsMonoFont(size: 13),
                .foregroundColor: palette.textPrimary.nsColor,
                .paragraphStyle: pStyle,
            ]
        )

        let codeTv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: docWidth, height: codeHeight))
        codeTv.isRichText = false
        codeTv.isHorizontallyResizable = true
        codeTv.isVerticallyResizable = false
        codeTv.textContainerInset = NSSize(width: 0, height: 6)
        codeTv.textContainer?.lineFragmentPadding = 0
        codeTv.textContainer?.widthTracksTextView = false
        codeTv.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: codeHeight)
        codeTv.textStorage?.setAttributedString(codeAttr)
        if let manager = codeTv.layoutManager, let container = codeTv.textContainer {
            manager.ensureLayout(for: container)
        }
        scroll.documentView = codeTv

        box.addSubview(scroll)
        box.addSubview(copyBtn)

        NSLayoutConstraint.activate([
            box.widthAnchor.constraint(equalToConstant: width),
            copyBtn.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            copyBtn.topAnchor.constraint(equalTo: box.topAnchor, constant: 8),
            copyBtn.widthAnchor.constraint(equalToConstant: 32),
            copyBtn.heightAnchor.constraint(equalToConstant: 32),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16),
            scroll.topAnchor.constraint(equalTo: box.topAnchor, constant: 38),
            scroll.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -16),
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
        let layout = ConversationTableCoordinator.measureTableLayout(headers: headers, rows: rows, maxWidth: width)
        let visibleW = min(width, layout.totalWidth)

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: false)
        scroll.hasHorizontalScroller = layout.totalWidth > visibleW
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let colCount = max(1, headers.count)
        let allRows = [headers] + rows
        let docView = NSView(frame: NSRect(x: 0, y: 0, width: layout.totalWidth, height: layout.totalHeight))
        var currentTopY = layout.totalHeight

        for (rIdx, cols) in allRows.enumerated() {
            let rowH = layout.rowHeights[rIdx]
            currentTopY -= rowH
            var currentX: CGFloat = 0
            for cIdx in 0 ..< colCount {
                let colW = layout.colWidths[cIdx]
                let cellRect = NSRect(x: currentX, y: currentTopY, width: colW, height: rowH)
                let cellBox = NSView(frame: cellRect)
                cellBox.wantsLayer = true
                cellBox.layer?.backgroundColor = (rIdx == 0 ? palette.surface : palette.background).nsColor.cgColor
                cellBox.layer?.borderWidth = 0.5
                cellBox.layer?.borderColor = palette.borderStrong.nsColor.cgColor

                let cellAttr = cIdx < cols.count ? cols[cIdx] : NSAttributedString(string: "")
                let innerW = max(36, colW - 24)
                let textH = max(18, ReaderSelectableTextView.measureHeight(attributed: cellAttr, width: innerW))
                let tv = ReaderSelectableTextView(
                    frame: NSRect(x: 12, y: max(4, (rowH - textH) * 0.5), width: innerW, height: textH)
                )
                tv.quoteBackgroundColor = palette.surface.nsColor
                tv.quoteAccentColor = palette.accent.nsColor
                tv.applyAttributedText(cellAttr, width: innerW)
                tv.onLinkActivated = { [weak self, weak coordinator] href in
                    guard let self, let coordinator else { return false }
                    return self.handleActivatedHref(href, coordinator: coordinator)
                }
                cellBox.addSubview(tv)
                docView.addSubview(cellBox)
                currentX += colW
            }
        }
        docView.wantsLayer = true
        docView.layer?.borderWidth = 1
        docView.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        scroll.documentView = docView
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: visibleW),
            scroll.heightAnchor.constraint(equalToConstant: layout.totalHeight),
        ])
        return scroll
    }

    private func buildExecutionGroupView(
        group: ExecutionGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil,
        nestedInRail: Bool = false
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let aliases = group.mergedIds + group.commands.map { "group:\($0.id)" }
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: group.id,
            aliases: aliases,
            initiallyOpen: group.initiallyOpen
        )
        let header = group.summaryHeader
        let countMeta = "\(group.commands.count) \(group.kind == "tool" ? "项工具" : "条命令")"
            + (group.failedCount > 0 ? " · \(group.failedCount) 条失败" : "")

        let summaryRow = makeProcessSummaryRow(
            gutterIcon: header.icon,
            nestedInRail: nestedInRail,
            title: header.label,
            metaPill: countMeta,
            failureCount: 0,
            extraPills: [],
            running: group.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: group.id, currentlyOpen: isOpen)
        }
        destStack.addArrangedSubview(summaryRow)

        // Lazy child creation: do NOT build inner views when collapsed!
        guard isOpen else { return }
        let bodyWidth = max(120, width - 20)
        let cmdStack = makeExecutionCommandsList(
            commands: group.commands,
            width: bodyWidth,
            wrappedInSurfaceCard: false,
            palette: palette,
            coordinator: coordinator
        )
        destStack.addArrangedSubview(makeLeftRailContainer(content: cmdStack, width: width, palette: palette))
    }

    private func buildProcessGroupView(
        proc: UnifiedProcessGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: proc.stateKey,
            aliases: proc.stateAliases,
            initiallyOpen: proc.failedCount > 0
        )
        let summary = proc.summaryText
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: proc.iconName,
            title: summary.primary,
            metaPill: summary.meta,
            failureCount: proc.failedCount,
            extraPills: [],
            running: proc.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: proc.stateKey, currentlyOpen: isOpen)
        }
        containerStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let bodyWidth = max(120, width - 22)
        let stepsStack = NSStackView()
        stepsStack.orientation = .vertical
        stepsStack.alignment = .leading
        stepsStack.spacing = 8
        stepsStack.translatesAutoresizingMaskIntoConstraints = false

        for step in proc.steps {
            switch step {
            case let .thinking(_, _, text, _, _):
                stepsStack.addArrangedSubview(makeThoughtProseView(markdown: text, width: bodyWidth, palette: palette, coordinator: coordinator))
            case let .execution(group, _):
                stepsStack.addArrangedSubview(
                    makeExecutionCommandsList(
                        commands: group.commands,
                        width: bodyWidth,
                        wrappedInSurfaceCard: true,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            case let .detail(id, sids, title, detail, running, failed, _):
                let detailCmd = ExecutionCommand(
                    id: id,
                    sourceIds: sids,
                    title: title,
                    preview: nil,
                    status: failed ? .failed : (running ? .running : .completed),
                    instructionFile: false,
                    sections: [ExecutionSection(label: "", text: detail)]
                )
                stepsStack.addArrangedSubview(
                    makeExecutionCommandsList(
                        commands: [detailCmd],
                        width: bodyWidth,
                        wrappedInSurfaceCard: false,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            }
        }
        containerStack.addArrangedSubview(makeLeftRailContainer(content: stepsStack, width: width, palette: palette))
    }

    private func buildClaudeProcessGroupView(
        proc: ClaudeProcessGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let aliases = proc.sourceIds.map { "claude-process:\(proc.turnId):\($0)" }
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: proc.stateKey,
            aliases: aliases,
            initiallyOpen: false
        )
        var extra: [String] = []
        if proc.needsReply { extra.append("待回复") }
        if proc.hasPermission { extra.append(proc.needsPermission ? "待授权" : "授权记录") }
        if !proc.running && (proc.needsReply || proc.needsPermission) { extra.append("展开查看") }

        let summaryRow = makeProcessSummaryRow(
            gutterIcon: proc.iconName,
            title: proc.labelText,
            metaPill: proc.metaText,
            failureCount: proc.failedCount,
            extraPills: extra,
            running: proc.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: proc.stateKey, currentlyOpen: isOpen)
        }
        containerStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let bodyWidth = max(120, width - 22)
        let innerStack = buildClaudeEntriesStack(entries: proc.entries, width: bodyWidth, coordinator: coordinator)
        containerStack.addArrangedSubview(makeLeftRailContainer(content: innerStack, width: width, palette: palette))
    }

    private func buildClaudeMetadataGroupView(
        meta: ClaudeMetadataGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: meta.id,
            aliases: aliases,
            initiallyOpen: false
        )
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: nil,
            title: meta.label,
            metaPill: nil,
            failureCount: 0,
            extraPills: [],
            running: false,
            open: isOpen,
            expandsChevronToTrailing: true,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: meta.id, currentlyOpen: isOpen)
        }
        destStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let subItems = meta.entries.map { entry -> ClaudeEventRenderItem in
            switch entry {
            case let .item(it, key, sids): .item(item: it, key: key, sourceIds: sids)
            case let .group(g): .group(g)
            }
        }
        let bodyWidth = max(100, width - 22)
        let innerStack = buildClaudeEntriesStack(entries: subItems, width: bodyWidth, coordinator: coordinator)
        destStack.addArrangedSubview(makeLeftRailContainer(content: innerStack, width: width, palette: palette))
    }

    private func buildClaudeEntriesStack(
        entries: [ClaudeEventRenderItem],
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) -> NSStackView {
        let palette = coordinator.palette
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false

        for entry in entries {
            switch entry {
            case let .group(group):
                buildExecutionGroupView(group: group, width: width, coordinator: coordinator, into: stack, nestedInRail: true)
            case let .metadata(meta):
                buildClaudeMetadataGroupView(meta: meta, width: width, coordinator: coordinator, into: stack)
            case let .item(ev, key, sids):
                if ev.kind == "thinking" {
                    stack.addArrangedSubview(makeThoughtProseView(markdown: ev.text ?? "", width: width, palette: palette, coordinator: coordinator))
                } else if let detail = ev.detail, !detail.isEmpty {
                    let cmd = ExecutionCommand(
                        id: key,
                        sourceIds: sids,
                        title: ev.text ?? ev.role ?? "活动记录",
                        preview: nil,
                        status: ExecutionGroupingEngine.executionStatus(ev.status),
                        instructionFile: false,
                        sections: [ExecutionSection(label: "", text: detail)]
                    )
                    stack.addArrangedSubview(
                        makeExecutionCommandsList(
                            commands: [cmd],
                            width: width,
                            wrappedInSurfaceCard: false,
                            palette: palette,
                            coordinator: coordinator
                        )
                    )
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    stack.addArrangedSubview(makeThoughtProseView(markdown: text, width: width, palette: palette, coordinator: coordinator))
                }
            }
        }
        return stack
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
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: .brain,
            title: running ? "正在思考…" : title,
            metaPill: nil,
            failureCount: 0,
            extraPills: [],
            running: running,
            open: open,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: id, currentlyOpen: open)
        }
        containerStack.addArrangedSubview(summaryRow)
        guard open else { return }
        let thoughtView = makeThoughtProseView(markdown: text, width: max(120, width - 22), palette: palette, coordinator: coordinator)
        containerStack.addArrangedSubview(makeLeftRailContainer(content: thoughtView, width: width, palette: palette))
    }

    private func makeThoughtProseView(
        markdown: String,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let attr = ConversationTableCoordinator.renderThoughtAttributedString(markdown: markdown, palette: palette)
        let h = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: width))
        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: width, height: h))
        tv.applyAttributedText(attr, width: width)
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

        let badgeTitle = NSTextField(labelWithString: "Proposed Plan")
        badgeTitle.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
        badgeTitle.textColor = palette.textMuted.nsColor

        let badgeStatus = NSTextField(labelWithString: statusText)
        badgeStatus.font = DesignFonts.nsBodyFont(size: 12)
        badgeStatus.textColor = palette.textMuted.nsColor

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

        let saveBtn = IconFeedbackActionButton(
            dimension: 30,
            iconSize: 17,
            idleIcon: .download,
            palette: palette,
            toolTipText: "下载计划 Markdown"
        ) { [weak coordinator] in
            guard let coordinator else { return }
            let filename = FileLinksAndPlanExport.planFilename(markdown: cleanedPlan, fallbackTitle: fallbackTitle)
            Task { @MainActor in
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

        headerRow.addArrangedSubview(badgeTitle)
        headerRow.addArrangedSubview(badgeStatus)
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
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let innerW = max(120, width - 32)
        let innerStack = NSStackView()
        innerStack.orientation = .vertical
        innerStack.alignment = .leading
        innerStack.spacing = 10
        innerStack.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(innerStack)

        let metaRow = NSStackView()
        metaRow.orientation = .horizontal
        metaRow.alignment = .centerY
        metaRow.spacing = 8
        let metaPrimary = NSTextField(labelWithString: "Questions")
        metaPrimary.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
        metaPrimary.textColor = palette.textMuted.nsColor
        let metaSub = NSTextField(labelWithString: header)
        metaSub.font = DesignFonts.nsBodyFont(size: 12)
        metaSub.textColor = palette.textMuted.nsColor
        metaRow.addArrangedSubview(metaPrimary)
        metaRow.addArrangedSubview(metaSub)
        innerStack.addArrangedSubview(metaRow)

        for q in questions {
            let qLbl = NSTextField(wrappingLabelWithString: q.question)
            qLbl.font = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
            qLbl.textColor = palette.textPrimary.nsColor
            qLbl.preferredMaxLayoutWidth = innerW
            innerStack.addArrangedSubview(qLbl)

            let optsStack = NSStackView()
            optsStack.orientation = .vertical
            optsStack.alignment = .leading
            optsStack.spacing = 0
            optsStack.translatesAutoresizingMaskIntoConstraints = false
            optsStack.widthAnchor.constraint(equalToConstant: innerW).isActive = true

            for (idx, opt) in q.options.enumerated() {
                let optBox = NSView()
                optBox.translatesAutoresizingMaskIntoConstraints = false
                let numLbl = NSTextField(labelWithString: "\(idx + 1)")
                numLbl.font = DesignFonts.nsBodyFont(size: 13)
                numLbl.textColor = palette.textMuted.nsColor
                numLbl.translatesAutoresizingMaskIntoConstraints = false

                let textCol = NSStackView()
                textCol.orientation = .vertical
                textCol.alignment = .leading
                textCol.spacing = 4
                textCol.translatesAutoresizingMaskIntoConstraints = false

                let optTitle = NSTextField(wrappingLabelWithString: opt.label)
                optTitle.font = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
                optTitle.textColor = palette.textPrimary.nsColor
                optTitle.preferredMaxLayoutWidth = max(80, innerW - 32)
                textCol.addArrangedSubview(optTitle)

                if let desc = opt.description, !desc.isEmpty {
                    let descLbl = NSTextField(wrappingLabelWithString: desc)
                    descLbl.font = DesignFonts.nsBodyFont(size: 12)
                    descLbl.textColor = palette.textMuted.nsColor
                    descLbl.preferredMaxLayoutWidth = max(80, innerW - 32)
                    textCol.addArrangedSubview(descLbl)
                }

                optBox.addSubview(numLbl)
                optBox.addSubview(textCol)
                NSLayoutConstraint.activate([
                    optBox.widthAnchor.constraint(equalToConstant: innerW),
                    numLbl.leadingAnchor.constraint(equalTo: optBox.leadingAnchor),
                    numLbl.topAnchor.constraint(equalTo: optBox.topAnchor, constant: 10),
                    numLbl.widthAnchor.constraint(equalToConstant: 24),
                    textCol.leadingAnchor.constraint(equalTo: optBox.leadingAnchor, constant: 32),
                    textCol.trailingAnchor.constraint(equalTo: optBox.trailingAnchor),
                    textCol.topAnchor.constraint(equalTo: optBox.topAnchor, constant: 10),
                    textCol.bottomAnchor.constraint(equalTo: optBox.bottomAnchor, constant: -10),
                ])

                if idx < q.options.count - 1 {
                    let sep = NSView()
                    sep.wantsLayer = true
                    sep.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                    sep.translatesAutoresizingMaskIntoConstraints = false
                    optBox.addSubview(sep)
                    NSLayoutConstraint.activate([
                        sep.leadingAnchor.constraint(equalTo: optBox.leadingAnchor),
                        sep.trailingAnchor.constraint(equalTo: optBox.trailingAnchor),
                        sep.bottomAnchor.constraint(equalTo: optBox.bottomAnchor),
                        sep.heightAnchor.constraint(equalToConstant: 1),
                    ])
                }
                optsStack.addArrangedSubview(optBox)
            }
            innerStack.addArrangedSubview(optsStack)
        }

        if let answers {
            if !answers.isEmpty {
                let ansList = NSStackView()
                ansList.orientation = .vertical
                ansList.alignment = .leading
                ansList.spacing = 8
                ansList.translatesAutoresizingMaskIntoConstraints = false
                for ans in answers {
                    let ansBox = NSView()
                    ansBox.translatesAutoresizingMaskIntoConstraints = false
                    ansBox.wantsLayer = true
                    ansBox.layer?.cornerRadius = 10
                    ansBox.layer?.backgroundColor = palette.elevated.nsColor.cgColor
                    ansBox.layer?.borderWidth = 1
                    ansBox.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

                    let qIdLbl = NSTextField(labelWithString: ans.questionId)
                    qIdLbl.font = DesignFonts.nsBodyFont(size: 12)
                    qIdLbl.textColor = palette.textMuted.nsColor
                    qIdLbl.lineBreakMode = .byTruncatingTail
                    qIdLbl.translatesAutoresizingMaskIntoConstraints = false

                    let ansValText = ans.answers.isEmpty ? "未回答" : ans.answers.joined(separator: ", ")
                    let ansValLbl = NSTextField(wrappingLabelWithString: ansValText)
                    ansValLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .semibold)
                    ansValLbl.textColor = palette.textPrimary.nsColor
                    let rightColW = max(80, innerW - 150)
                    ansValLbl.preferredMaxLayoutWidth = rightColW
                    ansValLbl.translatesAutoresizingMaskIntoConstraints = false

                    ansBox.addSubview(qIdLbl)
                    ansBox.addSubview(ansValLbl)
                    NSLayoutConstraint.activate([
                        ansBox.widthAnchor.constraint(equalToConstant: innerW),
                        qIdLbl.leadingAnchor.constraint(equalTo: ansBox.leadingAnchor, constant: 12),
                        qIdLbl.widthAnchor.constraint(equalToConstant: min(124, innerW * 0.34)),
                        qIdLbl.topAnchor.constraint(equalTo: ansBox.topAnchor, constant: 10),
                        ansValLbl.leadingAnchor.constraint(equalTo: qIdLbl.trailingAnchor, constant: 10),
                        ansValLbl.trailingAnchor.constraint(equalTo: ansBox.trailingAnchor, constant: -12),
                        ansValLbl.topAnchor.constraint(equalTo: ansBox.topAnchor, constant: 10),
                        ansValLbl.bottomAnchor.constraint(equalTo: ansBox.bottomAnchor, constant: -10),
                    ])
                    ansList.addArrangedSubview(ansBox)
                }
                innerStack.addArrangedSubview(ansList)
            } else {
                let pLbl = NSTextField(labelWithString: "Questions answered")
                pLbl.font = DesignFonts.nsBodyFont(size: 13)
                pLbl.textColor = palette.textMuted.nsColor
                innerStack.addArrangedSubview(pLbl)
            }
        }

        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            innerStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 16),
            innerStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -16),
            innerStack.topAnchor.constraint(equalTo: card.topAnchor, constant: 14),
            innerStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -14),
        ])
        containerStack.addArrangedSubview(card)
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
        wrapper.clipsToBounds = false
        wrapper.translatesAutoresizingMaskIntoConstraints = false
        wrapper.widthAnchor.constraint(equalToConstant: width).isActive = true

        let rowBox = HoverSummaryRowView(
            open: false,
            isEnabled: available,
            hoverFill: palette.hover.nsColor.withAlphaComponent(0.5),
            openFill: .clear
        )
        rowBox.clipsToBounds = false
        rowBox.toolTip = reason
        rowBox.onTap = { [weak coordinator] in
            if available {
                coordinator?.onOpenSubagent?(sub)
            }
        }

        let botIcon = NSImageView(
            image: LucideVectorCatalog.nsImage(for: .bot, size: 16, color: palette.textSecondary.nsColor)
        )
        botIcon.translatesAutoresizingMaskIntoConstraints = false
        rowBox.addSubview(botIcon)

        let rowStack = NSStackView()
        rowStack.orientation = .horizontal
        rowStack.alignment = .centerY
        rowStack.spacing = 8
        rowStack.translatesAutoresizingMaskIntoConstraints = false
        rowBox.addSubview(rowStack)

        let nameLbl = NSTextField(labelWithString: VisibleMarkdownEngine.visibleMarkdown(sub.name))
        nameLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        nameLbl.textColor = (available ? palette.textMuted : palette.textMuted).nsColor
        nameLbl.lineBreakMode = .byTruncatingTail
        nameLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        rowStack.addArrangedSubview(nameLbl)

        if sub.status == "running" && (sub.eventKind == nil || sub.eventKind == "started") {
            let ring = AppKitRunningRingView(
                size: 13,
                trackColor: palette.borderStrong.nsColor,
                accentColor: palette.accent.nsColor
            )
            rowStack.addArrangedSubview(ring)
        }

        let verbLbl = NSTextField(labelWithString: Self.subagentActivityVerb(sub))
        verbLbl.font = DesignFonts.nsBodyFont(size: 12)
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
        rowStack.addArrangedSubview(NSView())

        NSLayoutConstraint.activate([
            rowBox.widthAnchor.constraint(equalToConstant: width),
            rowBox.heightAnchor.constraint(equalToConstant: 30),
            botIcon.leadingAnchor.constraint(equalTo: rowBox.leadingAnchor, constant: -24),
            botIcon.centerYAnchor.constraint(equalTo: rowBox.centerYAnchor),
            botIcon.widthAnchor.constraint(equalToConstant: 16),
            botIcon.heightAnchor.constraint(equalToConstant: 16),
            rowStack.leadingAnchor.constraint(equalTo: rowBox.leadingAnchor),
            rowStack.trailingAnchor.constraint(equalTo: rowBox.trailingAnchor),
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
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let textCol = NSStackView()
        textCol.orientation = .vertical
        textCol.alignment = .leading
        textCol.spacing = 4
        textCol.translatesAutoresizingMaskIntoConstraints = false

        let eyeLbl = NSTextField(labelWithString: eyebrow)
        eyeLbl.font = DesignFonts.nsBodyFont(size: 12)
        eyeLbl.textColor = palette.textMuted.nsColor

        let mainLbl = NSTextField(labelWithString: label)
        mainLbl.font = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
        mainLbl.textColor = palette.textPrimary.nsColor
        mainLbl.lineBreakMode = .byTruncatingTail
        textCol.addArrangedSubview(eyeLbl)
        textCol.addArrangedSubview(mainLbl)

        let btn = ActionButton()
        btn.title = expanded ? "已显示全部" : "显示全部历史"
        btn.isEnabled = !expanded
        btn.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        btn.contentTintColor = palette.textPrimary.nsColor
        btn.wantsLayer = true
        btn.layer?.cornerRadius = 8
        btn.layer?.backgroundColor = palette.elevated.nsColor.cgColor
        btn.layer?.borderWidth = 1
        btn.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        btn.translatesAutoresizingMaskIntoConstraints = false
        let btnW = ceil(NSAttributedString(string: btn.title, attributes: [.font: btn.font!]).size().width) + 28
        NSLayoutConstraint.activate([
            btn.widthAnchor.constraint(equalToConstant: max(108, btnW)),
            btn.heightAnchor.constraint(equalToConstant: 35),
        ])
        btn.onTap = { [weak coordinator] in
            coordinator?.onShowAllHistory?()
        }

        card.addSubview(textCol)
        card.addSubview(btn)
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            card.heightAnchor.constraint(equalToConstant: 64),
            textCol.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 16),
            textCol.centerYAnchor.constraint(equalTo: card.centerYAnchor),
            textCol.trailingAnchor.constraint(lessThanOrEqualTo: btn.leadingAnchor, constant: -12),
            btn.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -16),
            btn.centerYAnchor.constraint(equalTo: card.centerYAnchor),
        ])
        containerStack.addArrangedSubview(card)
    }

    private func buildSystemNoticeRow(label: String, detail: String, width: CGFloat, palette: DesignPalette) {
        let lbl = NSTextField(labelWithString: "\(label): \(detail)")
        lbl.font = DesignFonts.nsBodyFont(size: 12)
        lbl.textColor = palette.textMuted.nsColor
        lbl.lineBreakMode = .byTruncatingTail
        lbl.widthAnchor.constraint(equalToConstant: width).isActive = true
        containerStack.addArrangedSubview(lbl)
    }

    private func makeProcessSummaryRow(
        gutterIcon: LucideIconName?,
        nestedInRail: Bool = false,
        title: String,
        metaPill: String?,
        failureCount: Int,
        extraPills: [String],
        running: Bool,
        open: Bool,
        expandsChevronToTrailing: Bool,
        width: CGFloat,
        palette: DesignPalette,
        action: @escaping () -> Void
    ) -> NSView {
        let row = HoverSummaryRowView(
            open: open,
            isEnabled: true,
            hoverFill: palette.surface.nsColor,
            openFill: palette.surface.nsColor
        )
        row.clipsToBounds = false
        row.onTap = action

        if let gutterIcon, !nestedInRail {
            let iconView = NSImageView(
                image: LucideVectorCatalog.nsImage(for: gutterIcon, size: 15, color: palette.accent.nsColor)
            )
            iconView.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(iconView)
            NSLayoutConstraint.activate([
                iconView.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: -20),
                iconView.centerYAnchor.constraint(equalTo: row.centerYAnchor),
                iconView.widthAnchor.constraint(equalToConstant: 15),
                iconView.heightAnchor.constraint(equalToConstant: 15),
            ])
        }

        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 8
        hStack.translatesAutoresizingMaskIntoConstraints = false
        row.addSubview(hStack)

        if let gutterIcon, nestedInRail {
            let inlineIcon = NSImageView(
                image: LucideVectorCatalog.nsImage(for: gutterIcon, size: 14, color: palette.accent.nsColor)
            )
            inlineIcon.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                inlineIcon.widthAnchor.constraint(equalToConstant: 14),
                inlineIcon.heightAnchor.constraint(equalToConstant: 14),
            ])
            hStack.addArrangedSubview(inlineIcon)
        }

        let titleLbl = NSTextField(labelWithString: title)
        titleLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        titleLbl.textColor = (open ? palette.textPrimary : palette.textSecondary).nsColor
        titleLbl.lineBreakMode = .byTruncatingTail
        titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        hStack.addArrangedSubview(titleLbl)

        if let metaPill, !metaPill.isEmpty {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: metaPill,
                    textColor: palette.textMuted.nsColor,
                    fill: palette.elevated.nsColor,
                    border: palette.borderStrong.nsColor,
                    semibold: false
                )
            )
        }

        if failureCount > 0 {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: "\(failureCount) 项失败",
                    textColor: palette.errFg.nsColor,
                    fill: palette.errBg.nsColor,
                    border: palette.errBorder.nsColor,
                    semibold: true
                )
            )
        }

        for extra in extraPills {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: extra,
                    textColor: palette.textMuted.nsColor,
                    fill: palette.elevated.nsColor,
                    border: palette.borderStrong.nsColor,
                    semibold: false
                )
            )
        }

        if running || expandsChevronToTrailing {
            hStack.addArrangedSubview(NSView())
        }
        if running {
            hStack.addArrangedSubview(
                AppKitRunningRingView(
                    size: 13,
                    trackColor: palette.borderStrong.nsColor,
                    accentColor: palette.accent.nsColor
                )
            )
        }

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
        ])
        hStack.addArrangedSubview(chev)
        if !running && !expandsChevronToTrailing {
            hStack.addArrangedSubview(NSView())
        }

        NSLayoutConstraint.activate([
            row.widthAnchor.constraint(equalToConstant: width),
            row.heightAnchor.constraint(equalToConstant: 30),
            hStack.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: open ? 6 : 0),
            hStack.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -8),
            hStack.centerYAnchor.constraint(equalTo: row.centerYAnchor),
        ])
        return row
    }

    private func makeCapsulePill(
        text: String,
        textColor: NSColor,
        fill: NSColor,
        border: NSColor,
        semibold: Bool
    ) -> NSView {
        let box = NSView()
        box.translatesAutoresizingMaskIntoConstraints = false
        box.wantsLayer = true
        box.layer?.cornerRadius = 10
        box.layer?.backgroundColor = fill.cgColor
        box.layer?.borderWidth = 1
        box.layer?.borderColor = border.cgColor

        let lbl = NSTextField(labelWithString: text)
        lbl.font = DesignFonts.nsBodyFont(size: 11, weight: semibold ? .semibold : .regular)
        lbl.textColor = textColor
        lbl.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(lbl)
        NSLayoutConstraint.activate([
            box.heightAnchor.constraint(equalToConstant: 20),
            lbl.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 8),
            lbl.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            lbl.centerYAnchor.constraint(equalTo: box.centerYAnchor),
        ])
        return box
    }

    private func makeLeftRailContainer(
        content: NSView,
        width: CGFloat,
        palette: DesignPalette
    ) -> NSView {
        let wrap = NSView()
        wrap.translatesAutoresizingMaskIntoConstraints = false

        let rail = NSView()
        rail.translatesAutoresizingMaskIntoConstraints = false
        rail.wantsLayer = true
        rail.layer?.backgroundColor = palette.borderStrong.nsColor.cgColor

        content.translatesAutoresizingMaskIntoConstraints = false
        wrap.addSubview(rail)
        wrap.addSubview(content)
        NSLayoutConstraint.activate([
            wrap.widthAnchor.constraint(equalToConstant: width),
            rail.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 2),
            rail.topAnchor.constraint(equalTo: wrap.topAnchor, constant: 4),
            rail.bottomAnchor.constraint(equalTo: wrap.bottomAnchor, constant: -2),
            rail.widthAnchor.constraint(equalToConstant: 2),
            content.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 16),
            content.trailingAnchor.constraint(equalTo: wrap.trailingAnchor, constant: -4),
            content.topAnchor.constraint(equalTo: wrap.topAnchor, constant: 4),
            content.bottomAnchor.constraint(equalTo: wrap.bottomAnchor, constant: -4),
        ])
        return wrap
    }

    private func makeExecutionCommandsList(
        commands: [ExecutionCommand],
        width: CGFloat,
        wrappedInSurfaceCard: Bool,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let listStack = NSStackView()
        listStack.orientation = .vertical
        listStack.alignment = .leading
        listStack.spacing = 0
        listStack.translatesAutoresizingMaskIntoConstraints = false
        let innerW = wrappedInSurfaceCard ? max(100, width - 20) : width

        for (idx, cmd) in commands.enumerated() {
            let cmdOpen = coordinator.disclosureStore.isOpen(
                scope: coordinator.sessionScopeKey,
                stateKey: cmd.id,
                aliases: cmd.sourceIds,
                initiallyOpen: !cmd.instructionFile && (cmd.status == .running || cmd.status == .failed)
            )
            let itemCol = NSStackView()
            itemCol.orientation = .vertical
            itemCol.alignment = .leading
            itemCol.spacing = 6
            itemCol.translatesAutoresizingMaskIntoConstraints = false
            itemCol.widthAnchor.constraint(equalToConstant: innerW).isActive = true

            let header = HoverSummaryRowView(
                open: false,
                isEnabled: true,
                hoverFill: palette.hover.nsColor.withAlphaComponent(0.55),
                openFill: .clear
            )
            header.onTap = { [weak coordinator] in
                coordinator?.toggleDisclosurePreservingAnchor(stateKey: cmd.id, currentlyOpen: cmdOpen)
            }
            let hStack = NSStackView()
            hStack.orientation = .horizontal
            hStack.alignment = .centerY
            hStack.spacing = 8
            hStack.translatesAutoresizingMaskIntoConstraints = false
            header.addSubview(hStack)

            let termIcon = NSImageView(
                image: LucideVectorCatalog.nsImage(for: .terminal, size: 15, color: palette.accent.nsColor)
            )
            termIcon.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                termIcon.widthAnchor.constraint(equalToConstant: 15),
                termIcon.heightAnchor.constraint(equalToConstant: 15),
            ])
            hStack.addArrangedSubview(termIcon)

            let titleAttr = NSMutableAttributedString()
            if cmd.instructionFile {
                titleAttr.append(NSAttributedString(string: "AGENTS.md", attributes: [
                    .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                    .foregroundColor: palette.textPrimary.nsColor,
                ]))
            } else {
                titleAttr.append(NSAttributedString(string: VisibleMarkdownEngine.visibleMarkdown(cmd.title), attributes: [
                    .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                    .foregroundColor: palette.textPrimary.nsColor,
                ]))
                if let preview = cmd.preview, !preview.isEmpty {
                    titleAttr.append(NSAttributedString(string: " \(VisibleMarkdownEngine.visibleMarkdown(preview))", attributes: [
                        .font: DesignFonts.nsBodyFont(size: 12),
                        .foregroundColor: palette.textMuted.nsColor,
                    ]))
                }
            }
            let titleLbl = NSTextField(labelWithAttributedString: titleAttr)
            titleLbl.lineBreakMode = .byTruncatingTail
            titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            hStack.addArrangedSubview(titleLbl)

            hStack.addArrangedSubview(NSView())
            if cmd.status == .running {
                hStack.addArrangedSubview(
                    AppKitRunningRingView(
                        size: 13,
                        trackColor: palette.borderStrong.nsColor,
                        accentColor: palette.accent.nsColor
                    )
                )
            }

            let stLbl = NSTextField(labelWithString: cmd.status.label)
            stLbl.font = DesignFonts.nsBodyFont(size: 12)
            stLbl.textColor = (cmd.status == .failed ? palette.errFg : palette.textMuted).nsColor
            hStack.addArrangedSubview(stLbl)

            let chev = NSImageView(
                image: LucideVectorCatalog.nsImage(
                    for: cmdOpen ? .chevronDown : .chevronRight,
                    size: 16,
                    color: (cmdOpen ? palette.accent : palette.textMuted).nsColor
                )
            )
            chev.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                chev.widthAnchor.constraint(equalToConstant: 16),
                chev.heightAnchor.constraint(equalToConstant: 16),
                header.widthAnchor.constraint(equalToConstant: innerW),
                header.heightAnchor.constraint(equalToConstant: 30),
                hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor, constant: 4),
                hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -4),
                hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            ])
            hStack.addArrangedSubview(chev)
            itemCol.addArrangedSubview(header)

            if cmdOpen {
                let detail = cmd.sections.map { sec in
                    sec.label.isEmpty ? sec.text : "\(sec.label)\n\(sec.text)"
                }.joined(separator: "\n\n")
                itemCol.addArrangedSubview(
                    makeMonospaceDetailBox(
                        text: detail.isEmpty ? "暂无输出" : detail,
                        width: innerW,
                        palette: palette,
                        coordinator: coordinator,
                        linkifyFiles: true
                    )
                )
            }

            if idx < commands.count - 1 {
                let sep = NSView()
                sep.translatesAutoresizingMaskIntoConstraints = false
                sep.wantsLayer = true
                sep.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                NSLayoutConstraint.activate([
                    sep.widthAnchor.constraint(equalToConstant: innerW),
                    sep.heightAnchor.constraint(equalToConstant: 1),
                ])
                itemCol.addArrangedSubview(sep)
            }
            listStack.addArrangedSubview(itemCol)
        }

        if !wrappedInSurfaceCard {
            return listStack
        }
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 10
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        card.addSubview(listStack)
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            listStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 10),
            listStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -10),
            listStack.topAnchor.constraint(equalTo: card.topAnchor, constant: 2),
            listStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -2),
        ])
        return card
    }

    private func makeMonospaceDetailBox(
        text: String,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator,
        linkifyFiles: Bool
    ) -> NSView {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
        let pStyle = NSMutableParagraphStyle()
        pStyle.minimumLineHeight = 19.2 // 12pt * 1.6
        pStyle.maximumLineHeight = 19.2
        let baseAttrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsMonoFont(size: 12),
            .foregroundColor: palette.textSecondary.nsColor,
            .paragraphStyle: pStyle,
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

        let innerW = max(80, width - 26)
        let contentH = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: innerW))
        let boxH = min(240, max(46, contentH + 22))

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: true)
        scroll.hasVerticalScroller = contentH + 22 > boxH
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = palette.codeBg.nsColor
        scroll.wantsLayer = true
        scroll.layer?.cornerRadius = 10
        scroll.layer?.borderWidth = 1
        scroll.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let tv = ReaderSelectableTextView(
            frame: NSRect(x: 0, y: 0, width: max(100, width), height: max(boxH, contentH + 22))
        )
        tv.textContainerInset = NSSize(width: 13, height: 11)
        tv.applyAttributedText(attr, width: max(100, width))
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

@MainActor
private final class HoverSummaryRowView: NSView {
    var isEnabled: Bool
    var onTap: (() -> Void)?
    private let open: Bool
    private let hoverFill: NSColor
    private let openFill: NSColor
    private var tracking: NSTrackingArea?

    init(open: Bool, isEnabled: Bool, hoverFill: NSColor, openFill: NSColor) {
        self.open = open
        self.isEnabled = isEnabled
        self.hoverFill = hoverFill
        self.openFill = openFill
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = (open ? openFill : NSColor.clear).cgColor
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var acceptsFirstResponder: Bool { isEnabled }

    override func updateTrackingAreas() {
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

    override func mouseEntered(with event: NSEvent) {
        guard isEnabled else { return }
        layer?.backgroundColor = hoverFill.cgColor
    }

    override func mouseExited(with event: NSEvent) {
        layer?.backgroundColor = (open ? openFill : NSColor.clear).cgColor
    }

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
    private let baseBg: NSColor
    private let hoverBg: NSColor
    private let onActivate: () -> Void
    private var resetTask: Task<Void, Never>?
    private var tracking: NSTrackingArea?

    public init(
        dimension: CGFloat = 30,
        iconSize: CGFloat = 17,
        idleIcon: LucideIconName = .copy,
        palette: DesignPalette,
        borderedSurface: NSColor? = nil,
        borderColor: NSColor? = nil,
        toolTipText: String,
        onActivate: @escaping () -> Void
    ) {
        self.dimension = dimension
        self.iconSize = iconSize
        self.idleIcon = idleIcon
        self.idleColor = palette.textMuted.nsColor
        self.okColor = palette.okFg.nsColor
        self.baseBg = borderedSurface ?? .clear
        self.hoverBg = palette.hover.nsColor
        self.onActivate = onActivate
        super.init(frame: NSRect(x: 0, y: 0, width: dimension, height: dimension))
        isBordered = false
        bezelStyle = .regularSquare
        imagePosition = .imageOnly
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = baseBg.cgColor
        if let borderColor {
            layer?.borderWidth = 1
            layer?.borderColor = borderColor.cgColor
        }
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

    override public var acceptsFirstResponder: Bool { isEnabled }
    override public var canBecomeKeyView: Bool { isEnabled }

    override public func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFeedbackStateChanged?() }
        return ok
    }

    override public func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok {
            DispatchQueue.main.async { [weak self] in
                self?.onFeedbackStateChanged?()
            }
        }
        return ok
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
        layer?.backgroundColor = baseBg.cgColor
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

@MainActor
private final class PassthroughInnerScrollView: NSScrollView {
    let allowsVerticalScroll: Bool

    init(allowsVerticalScroll: Bool) {
        self.allowsVerticalScroll = allowsVerticalScroll
        super.init(frame: .zero)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func scrollWheel(with event: NSEvent) {
        if !allowsVerticalScroll {
            // Horizontal-only code blocks & tables must NEVER trap vertical conversation scrolling!
            if !hasHorizontalScroller || abs(event.scrollingDeltaY) >= abs(event.scrollingDeltaX) {
                nextResponder?.scrollWheel(with: event)
                return
            }
            super.scrollWheel(with: event)
            return
        }

        let docH = documentView?.frame.height ?? 0
        let clipH = contentView.bounds.height
        let maxScroll = max(0, docH - clipH)
        // Forward momentum scrolls or at-boundary vertical scrolls to the outer conversation NSScrollView
        if !hasVerticalScroller || maxScroll <= 0.5 || event.momentumPhase != [] {
            nextResponder?.scrollWheel(with: event)
            return
        }
        let originY = contentView.bounds.origin.y
        if (event.scrollingDeltaY > 0 && originY <= 0.5)
            || (event.scrollingDeltaY < 0 && originY >= maxScroll - 0.5)
        {
            nextResponder?.scrollWheel(with: event)
            return
        }
        super.scrollWheel(with: event)
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
    public var canLoadOlder: Bool
    public var isLoadingOlder: Bool
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
        canLoadOlder: Bool = true,
        isLoadingOlder: Bool = false,
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
        self.canLoadOlder = canLoadOlder
        self.isLoadingOlder = isLoadingOlder
        self.isLoadOlderBlockedByError = isLoadOlderBlockedByError
    }

    public func makeCoordinator() -> ConversationTableCoordinator {
        coordinator
    }

    public func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.wantsLayer = true
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

        coordinator.canLoadOlder = canLoadOlder
        coordinator.isLoadingOlder = isLoadingOlder
        coordinator.isLoadOlderBlockedByError = isLoadOlderBlockedByError
        coordinator.scrollView = scroll
        coordinator.tableView = table
        coordinator.column = col
        coordinator.updateRowsPreservingAnchor(rows, preferAutoFollowBottom: true)
        return scroll
    }

    public func updateNSView(_ nsView: NSScrollView, context: Context) {
        let sessionChanged = (coordinator.sessionScopeKey != sessionScopeKey)
        let themeOrWorkspaceChanged = (coordinator.scheme != scheme || coordinator.workspace != workspace)
        coordinator.scheme = scheme
        coordinator.workspace = workspace
        coordinator.sessionScopeKey = sessionScopeKey
        coordinator.sessionProvider = sessionProvider
        coordinator.sessionKey = sessionKey
        coordinator.rootThreadId = rootThreadId
        coordinator.cwd = cwd
        coordinator.hasTimelineRail = hasTimelineRail
        coordinator.isSubagent = isSubagent
        coordinator.canLoadOlder = canLoadOlder
        coordinator.isLoadingOlder = isLoadingOlder
        coordinator.isLoadOlderBlockedByError = isLoadOlderBlockedByError
        nsView.backgroundColor = coordinator.palette.background.nsColor

        if sessionChanged || themeOrWorkspaceChanged || coordinator.rows != rows {
            coordinator.updateRowsPreservingAnchor(
                rows,
                preferAutoFollowBottom: !themeOrWorkspaceChanged,
                forceScrollToBottom: sessionChanged
            )
        }
    }
}
