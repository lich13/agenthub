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
    private var heightCacheScopeKey: String = ""

    private static let maxCachedCells = 128
    private var cellCacheByRowId: [String: ConversationRowCellView] = [:]
    private var cellCacheOrder: [String] = []
    private var cellCacheScopeKey: String = ""

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
            && sessionScopeKey == heightCacheScopeKey
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
        heightCacheScopeKey = sessionScopeKey
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
        if forceScrollToBottom || sessionScopeKey != cellCacheScopeKey {
            if forceScrollToBottom {
                upwardScrollIntent = false
            }
            cellCacheByRowId.removeAll()
            cellCacheOrder.removeAll()
            cellCacheScopeKey = sessionScopeKey
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

    public func toggleDisclosurePreservingAnchor(
        stateKey: String,
        aliases: [String] = [],
        currentlyOpen: Bool
    ) {
        let anchor = captureScrollAnchor()
        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0
            context.allowsImplicitAnimation = false
            self.disclosureStore.setOpen(
                scope: self.sessionScopeKey,
                stateKey: stateKey,
                aliases: aliases,
                isOpen: !currentlyOpen
            )
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

    private func openClaudeMetadataGroup(_ meta: ClaudeMetadataGroup, matching positionKey: String) {
        let metaAliases = meta.sourceIds.map { "claude-metadata:\($0)" }
        disclosureStore.setOpen(scope: sessionScopeKey, stateKey: meta.id, aliases: metaAliases, isOpen: true)
        for sub in meta.entries {
            switch sub {
            case let .group(group):
                if group.commands.contains(where: { TimelineViewModel.matchesTimelinePosition(positionKey, id: $0.id, aliases: $0.sourceIds) }) {
                    disclosureStore.setOpen(scope: sessionScopeKey, stateKey: group.id, aliases: group.stateAliases, isOpen: true)
                    for cmd in group.commands where TimelineViewModel.matchesTimelinePosition(positionKey, id: cmd.id, aliases: cmd.sourceIds) {
                        disclosureStore.setOpen(scope: sessionScopeKey, stateKey: cmd.id, aliases: cmd.sourceIds, isOpen: true)
                    }
                }
            case let .item(ev, key, sids):
                if let detail = ev.detail, !detail.isEmpty,
                   TimelineViewModel.matchesTimelinePosition(positionKey, id: key, aliases: sids)
                {
                    disclosureStore.setOpen(scope: sessionScopeKey, stateKey: key, aliases: sids, isOpen: true)
                }
            }
        }
    }

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
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: group.id, aliases: group.stateAliases, isOpen: true)
            for cmd in group.commands where TimelineViewModel.matchesTimelinePosition(positionKey, id: cmd.id, aliases: cmd.sourceIds) {
                disclosureStore.setOpen(scope: sessionScopeKey, stateKey: cmd.id, aliases: cmd.sourceIds, isOpen: true)
            }
            didMutateDisclosure = true
        case let .processGroup(proc):
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: proc.stateKey, aliases: proc.stateAliases, isOpen: true)
            let wrapThinking = proc.toolCount > 0 || proc.thoughtCount > 1
            for step in proc.steps {
                switch step {
                case let .execution(group, _):
                    for cmd in group.commands where TimelineViewModel.matchesTimelinePosition(positionKey, id: cmd.id, aliases: cmd.sourceIds) {
                        disclosureStore.setOpen(scope: sessionScopeKey, stateKey: cmd.id, aliases: cmd.sourceIds, isOpen: true)
                    }
                case let .detail(id, sids, _, _, _, _, _):
                    if TimelineViewModel.matchesTimelinePosition(positionKey, id: id, aliases: sids) {
                        disclosureStore.setOpen(scope: sessionScopeKey, stateKey: id, aliases: sids, isOpen: true)
                    }
                case let .thinking(id, sids, _, _, _):
                    if wrapThinking, TimelineViewModel.matchesTimelinePosition(positionKey, id: id, aliases: sids) {
                        disclosureStore.setOpen(
                            scope: sessionScopeKey,
                            stateKey: "\(id):thinking",
                            aliases: sids.map { "\($0):thinking" },
                            isOpen: true
                        )
                    }
                }
            }
            didMutateDisclosure = true
        case let .claudeProcessGroup(proc, _):
            let procAliases = proc.sourceIds.map { "claude-process:\(proc.turnId):\($0)" }
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: proc.stateKey, aliases: procAliases, isOpen: true)
            let wrapThinking = proc.wrapsThinkingDisclosures
            for entry in proc.entries {
                switch entry {
                case let .group(group):
                    if group.commands.contains(where: { TimelineViewModel.matchesTimelinePosition(positionKey, id: $0.id, aliases: $0.sourceIds) }) {
                        disclosureStore.setOpen(scope: sessionScopeKey, stateKey: group.id, aliases: group.stateAliases, isOpen: true)
                        for cmd in group.commands where TimelineViewModel.matchesTimelinePosition(positionKey, id: cmd.id, aliases: cmd.sourceIds) {
                            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: cmd.id, aliases: cmd.sourceIds, isOpen: true)
                        }
                    }
                case let .metadata(meta):
                    let metaIdAliases = meta.sourceIds + meta.sourceIds.map { "claude-metadata:\($0)" }
                    if TimelineViewModel.matchesTimelinePosition(positionKey, id: meta.id, aliases: metaIdAliases) {
                        openClaudeMetadataGroup(meta, matching: positionKey)
                    }
                case let .item(ev, key, sids):
                    if TimelineViewModel.matchesTimelinePosition(positionKey, id: key, aliases: sids) {
                        if ClaudeProcessGroupingEngine.isThoughtOrCommentary(ev) {
                            if wrapThinking {
                                disclosureStore.setOpen(
                                    scope: sessionScopeKey,
                                    stateKey: "\(key):thinking",
                                    aliases: sids.map { "\($0):thinking" },
                                    isOpen: true
                                )
                            }
                        } else if let detail = ev.detail, !detail.isEmpty {
                            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: key, aliases: sids, isOpen: true)
                        }
                    }
                }
            }
            didMutateDisclosure = true
        case let .claudeMetadataGroup(meta, _):
            openClaudeMetadataGroup(meta, matching: positionKey)
            didMutateDisclosure = true
        case .thinking:
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: targetRow.id, aliases: targetRow.timelineAliases, isOpen: true)
            didMutateDisclosure = true
        case let .user(msg, rawText, _):
            let proj = UserContextEngine.projectUserMessage(msg?.text ?? rawText, contextBlocks: msg?.contextBlocks)
            let segs = UserContextEngine.userMessageSegments(proj.text, contextBlocks: proj.contextBlocks)
            if !proj.contextBlocks.isEmpty && (segs.isEmpty || positionKey.contains("context") || positionKey.contains("instruction")) {
                let ctxKey = "\(msg?.id ?? targetRow.timelineId):context"
                disclosureStore.setOpen(
                    scope: sessionScopeKey,
                    stateKey: ctxKey,
                    aliases: ["\(targetRow.timelineId):context"],
                    isOpen: true
                )
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
            cachedRowHeights = Array<CGFloat?>(repeating: nil, count: rows.count)
            heightCacheDisclosureRev = disclosureStore.revision
            tableView.reloadData()
            tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integer: rowIdx))
            tableView.tile()
            tableView.layoutSubtreeIfNeeded()
        }

        let rect = tableView.rect(ofRow: rowIdx)
        let minY = -(scrollView?.contentInsets.top ?? 0)
        let maxScroll = max(minY, tableView.frame.height - clipView.bounds.height)
        let targetY = max(minY, min(maxScroll, rect.minY - 16))
        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0
            ctx.allowsImplicitAnimation = false
            clipView.setBoundsOrigin(NSPoint(x: 0, y: targetY))
            self.scrollView?.reflectScrolledClipView(clipView)
            self.lastClipOriginY = clipView.bounds.origin.y
        }
        isRestoringScroll = false

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
                      let idx = findAnchorRowIndex(for: anchor.timelineId, in: rows)
            {
                let rect = tableView.rect(ofRow: idx)
                clipView.setBoundsOrigin(NSPoint(x: 0, y: max(minY, rect.minY - anchor.deltaY)))
                scrollView.reflectScrolledClipView(clipView)
            }
            self.lastClipOriginY = clipView.bounds.origin.y
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
            || sessionScopeKey != heightCacheScopeKey
            || cachedRowHeights.count != rows.count
        {
            cachedRowHeights = Array<CGFloat?>(repeating: nil, count: rows.count)
            heightCacheWidthKey = widthKey
            heightCacheDisclosureRev = discRev
            heightCacheScheme = scheme
            heightCacheWorkspace = workspace
            heightCacheScopeKey = sessionScopeKey
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
        if sessionScopeKey != cellCacheScopeKey {
            cellCacheByRowId.removeAll()
            cellCacheOrder.removeAll()
            cellCacheScopeKey = sessionScopeKey
        }
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
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: group.id,
                aliases: group.stateAliases,
                initiallyOpen: group.initiallyOpen
            )
            if !isOpen { return 34 }
            return 34 + 16 + measureExecutionCommandsHeight(commands: group.commands, width: max(120, width - 20))

        case let .processGroup(proc):
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: proc.stateKey,
                aliases: proc.stateAliases,
                initiallyOpen: proc.initiallyOpen
            )
            if !isOpen { return 34 }
            let bodyW = max(120, width - 22)
            let wrapThinking = proc.toolCount > 0 || proc.thoughtCount > 1
            var bodyH: CGFloat = 14
            for (idx, step) in proc.steps.enumerated() {
                if idx > 0 { bodyH += 8 }
                switch step {
                case let .thinking(id, sids, text, _, _):
                    let thinkKey = "\(id):thinking"
                    if wrapThinking {
                        let thinkOpen = disclosureStore.isOpen(
                            scope: sessionScopeKey,
                            stateKey: thinkKey,
                            aliases: sids.map { "\($0):thinking" },
                            initiallyOpen: false
                        )
                        bodyH += thinkOpen
                            ? (32 + 6 + measureThoughtMarkdownHeight(ownerId: thinkKey, markdown: text, width: bodyW))
                            : 32
                    } else {
                        bodyH += measureThoughtMarkdownHeight(ownerId: thinkKey, markdown: text, width: bodyW)
                    }
                case let .execution(group, _):
                    bodyH += 4 + measureExecutionCommandsHeight(commands: group.commands, width: max(100, bodyW - 20))
                case let .detail(id, sids, _, detail, running, failed, _):
                    let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: id, aliases: sids, initiallyOpen: running || failed)
                    bodyH += open ? (30 + 6 + Self.measureMonospaceBoxHeight(text: detail, width: bodyW)) : 30
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
            return 34 + 14 + measureClaudeEntriesHeight(
                entries: proc.entries,
                width: bodyW,
                wrapThinkingDisclosures: proc.wrapsThinkingDisclosures
            )

        case let .claudeMetadataGroup(meta, _):
            let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
            let isOpen = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: meta.id, aliases: aliases, initiallyOpen: false)
            if !isOpen { return 34 }
            let bodyW = max(100, width - 22)
            let items = meta.entries.map { entry -> ClaudeEventRenderItem in
                switch entry {
                case let .item(it, key, sids): .item(item: it, key: key, sourceIds: sids)
                case let .group(g): .group(g)
                }
            }
            return 34 + 14 + measureClaudeEntriesHeight(entries: items, width: bodyW, wrapThinkingDisclosures: false)

        case let .thinking(_, text, _):
            let open = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: row.id,
                aliases: row.timelineAliases,
                initiallyOpen: false
            )
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
            h += 30
            if cmdOpen {
                let detail = cmd.sections.map { sec in
                    sec.label.isEmpty ? sec.text : "\(sec.label)\n\(sec.text)"
                }.joined(separator: "\n\n")
                h += 6 + Self.measureMonospaceBoxHeight(text: detail.isEmpty ? "暂无输出" : detail, width: width)
            }
            if idx < commands.count - 1 {
                h += 7
            }
        }
        return h
    }

    private func measureClaudeEntriesHeight(
        entries: [ClaudeEventRenderItem],
        width: CGFloat,
        wrapThinkingDisclosures: Bool
    ) -> CGFloat {
        var total: CGFloat = 0
        var renderedCount = 0
        func appendRenderedHeight(_ itemHeight: CGFloat) {
            if renderedCount > 0 {
                total += 8
            }
            total += itemHeight
            renderedCount += 1
        }
        for entry in entries {
            switch entry {
            case let .group(group):
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: group.id, aliases: group.stateAliases, initiallyOpen: group.initiallyOpen)
                appendRenderedHeight(
                    open ? (46 + measureExecutionCommandsHeight(commands: group.commands, width: max(120, width - 20))) : 30
                )
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
                    appendRenderedHeight(
                        46 + measureClaudeEntriesHeight(
                            entries: subItems,
                            width: max(100, width - 22),
                            wrapThinkingDisclosures: false
                        )
                    )
                } else {
                    appendRenderedHeight(30)
                }
            case let .item(ev, key, sids):
                if ClaudeProcessGroupingEngine.isThoughtOrCommentary(ev) {
                    let markdown = ev.text ?? ""
                    guard !VisibleMarkdownEngine.visibleMarkdown(markdown).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }
                    let thinkKey = "\(key):thinking"
                    if wrapThinkingDisclosures {
                        let thinkOpen = disclosureStore.isOpen(
                            scope: sessionScopeKey,
                            stateKey: thinkKey,
                            aliases: sids.map { "\($0):thinking" },
                            initiallyOpen: false
                        )
                        appendRenderedHeight(
                            thinkOpen
                                ? (32 + 6 + measureThoughtMarkdownHeight(ownerId: thinkKey, markdown: markdown, width: width))
                                : 32
                        )
                    } else {
                        appendRenderedHeight(measureThoughtMarkdownHeight(ownerId: key, markdown: markdown, width: width))
                    }
                } else if let detail = ev.detail, !detail.isEmpty {
                    let status = ExecutionGroupingEngine.executionStatus(ev.status)
                    let open = disclosureStore.isOpen(
                        scope: sessionScopeKey,
                        stateKey: key,
                        aliases: sids,
                        initiallyOpen: status == .running || status == .failed
                    )
                    appendRenderedHeight(open ? (30 + 6 + Self.measureMonospaceBoxHeight(text: detail, width: width)) : 30)
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    appendRenderedHeight(measureThoughtMarkdownHeight(ownerId: key, markdown: text, width: width))
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
        let pStyle = NSMutableParagraphStyle()
        pStyle.minimumLineHeight = 19.2 // 12pt * 1.6 (matches makeMonospaceDetailBox)
        pStyle.maximumLineHeight = 19.2
        let attr = NSAttributedString(
            string: cleaned.isEmpty ? " " : cleaned,
            attributes: [
                .font: DesignFonts.nsMonoFont(size: 12),
                .paragraphStyle: pStyle,
            ]
        )
        let measured = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: max(80, width - 26)))
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
