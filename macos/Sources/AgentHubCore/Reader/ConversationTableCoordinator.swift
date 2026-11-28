import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

@MainActor
final class ConversationTableCoordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    weak var scrollView: NSScrollView?
    weak var tableView: NSTableView?
    weak var column: NSTableColumn?

    var store: RuntimeConnectionStore
    var disclosureStore: ActivityDisclosureStore
    let attachmentViewport = AttachmentViewportManager()

    private(set) var rows: [ConversationRowItem] = []
    var scheme: ColorSchemeKind = .light
    var workspace: WorkspaceId = .codex
    var sessionScopeKey: String = "local::codex"
    var sessionProvider: SessionProvider = .codex
    var sessionKey: String = ""
    var rootThreadId: String?
    var cwd: String?
    var hasTimelineRail: Bool = true
    var isSubagent: Bool = false

    var onLoadOlderRequested: (() -> Void)?
    var onShowAllHistory: (() -> Void)?
    var onOpenSubagent: ((SubagentActivity) -> Void)?
    var onPreviewImage: ((String, NSImage) -> Void)?
    var onCopyFeedback: ((String) -> Void)?
    var onActiveTimelineChanged: ((String?) -> Void)?

    private var highlightedTimelineId: String?
    var isLoadOlderBlockedByError: Bool = false
    var canLoadOlder: Bool = true
    var isLoadingOlder: Bool = false
    nonisolated(unsafe) private var highlightClearTask: Task<Void, Never>?
    private var isRestoringScroll = false
    private var lastLaidOutWidth: CGFloat = 0
    private var lastClipOriginY: CGFloat = .greatestFiniteMagnitude
    private var upwardScrollIntent: Bool = false
    private var lastReportedActiveTimelineId: String? = "__uninitialized__"
    private var userAnchorRowIndices: [Int] = []
    private var hasAnyUserImageAttachments: Bool = false

    static var scrollerStyleOverrideForTesting: NSScroller.Style?

    static var effectiveScrollerStyle: NSScroller.Style {
        scrollerStyleOverrideForTesting ?? NSScroller.preferredScrollerStyle
    }

    private var cachedRowHeights: [CGFloat?] = []
    private var heightCacheWidthKey: Int = 0
    private var heightCacheDisclosureRev: UInt64 = 0
    private var heightCacheScheme: ColorSchemeKind = .light
    private var heightCacheWorkspace: WorkspaceId = .codex
    private var heightCacheScopeKey: String = ""
    private var heightCacheScrollerStyle: NSScroller.Style = ConversationTableCoordinator.effectiveScrollerStyle

    static let maxCachedCells = 48
    private var cellCache = BoundedLRUCache<String, ConversationRowCellView>(maxEntries: maxCachedCells)
    private var cellCacheScopeKey: String = ""

    private struct SegmentWidthKey: Hashable {
        let contentHash: Int
        let quantizedWidth: Int
        let scheme: ColorSchemeKind
        let workspace: WorkspaceId
    }

    private var proseHeightCache = BoundedLRUCache<SegmentWidthKey, CGFloat>(maxEntries: 512)
    private var thoughtLayoutCache = BoundedLRUCache<SegmentWidthKey, (attributed: NSAttributedString, height: CGFloat)>(maxEntries: 256)
    private var codeLayoutCache = BoundedLRUCache<SegmentWidthKey, (normalized: String, codeHeight: CGFloat, docWidth: CGFloat)>(maxEntries: 256)
    private var tableLayoutCache = BoundedLRUCache<SegmentWidthKey, (colWidths: [CGFloat], rowHeights: [CGFloat], totalWidth: CGFloat, totalHeight: CGFloat)>(maxEntries: 128)

    struct ScrollAnchorSnapshot: Equatable, Sendable {
        let timelineId: String
        let deltaY: CGFloat
        var deltaMaxY: CGFloat
        var savedRowHeight: CGFloat
        var wasFirstRow: Bool
        var savedScrollTop: CGFloat
        var savedContentHeight: CGFloat

        init(
            timelineId: String,
            deltaY: CGFloat,
            deltaMaxY: CGFloat = 0,
            savedRowHeight: CGFloat = 0,
            wasFirstRow: Bool = false,
            savedScrollTop: CGFloat = 0,
            savedContentHeight: CGFloat = 0
        ) {
            self.timelineId = timelineId
            self.deltaY = deltaY
            self.deltaMaxY = deltaMaxY
            self.savedRowHeight = savedRowHeight
            self.wasFirstRow = wasFirstRow
            self.savedScrollTop = savedScrollTop
            self.savedContentHeight = savedContentHeight
        }
    }

    init(
        store: RuntimeConnectionStore,
        disclosureStore: ActivityDisclosureStore = .shared
    ) {
        self.store = store
        self.disclosureStore = disclosureStore
        super.init()
        attachmentViewport.onImageUpdated = { [weak self] key in
            self?.reloadRowForAttachmentKey(key)
        }
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(ConversationTableCoordinator.preferredScrollerStyleDidChange(_:)),
            name: NSScroller.preferredScrollerStyleDidChangeNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(ConversationTableCoordinator.handleWindowDidHide(_:)),
            name: .agentHubWindowDidHide,
            object: nil
        )
    }

    @objc private func handleWindowDidHide(_ notification: Notification) {
        evictOffscreenCaches()
    }

    func evictOffscreenCaches() {
        cellCache.removeAll()
        proseHeightCache.removeAll()
        thoughtLayoutCache.removeAll()
        codeLayoutCache.removeAll()
        tableLayoutCache.removeAll()
        GfmLayoutCache.shared.trim()
    }

    struct LoadedAttachmentState: Equatable, Sendable {
        let lowMask: UInt64
        let overflowIndices: [Int]
        let failedLowMask: UInt64
        let failedOverflowIndices: [Int]

        init(
            lowMask: UInt64 = 0,
            overflowIndices: [Int] = [],
            failedLowMask: UInt64 = 0,
            failedOverflowIndices: [Int] = []
        ) {
            self.lowMask = lowMask
            self.overflowIndices = overflowIndices
            self.failedLowMask = failedLowMask
            self.failedOverflowIndices = failedOverflowIndices
        }
    }

    func loadedAttachmentState(for item: ConversationRowItem) -> LoadedAttachmentState {
        guard case let .user(msg, _, namesOnly) = item.kind,
              !namesOnly,
              let attachments = msg?.attachments
        else { return LoadedAttachmentState() }
        var mask: UInt64 = 0
        var overflow: [Int] = []
        var failedMask: UInt64 = 0
        var failedOverflow: [Int] = []
        let msgId = msg?.id ?? item.id
        for (idx, att) in attachments.enumerated() where att.kind == "image" && att.reason == nil {
            let key = "\(sessionScopeKey):\(msgId):\(att.id)"
            if attachmentViewport.image(for: key) != nil {
                if idx < 64 {
                    mask |= (1 << UInt64(idx))
                } else {
                    overflow.append(idx)
                }
            } else if attachmentViewport.hasFailed(for: key) {
                if idx < 64 {
                    failedMask |= (1 << UInt64(idx))
                } else {
                    failedOverflow.append(idx)
                }
            }
        }
        return LoadedAttachmentState(
            lowMask: mask,
            overflowIndices: overflow,
            failedLowMask: failedMask,
            failedOverflowIndices: failedOverflow
        )
    }

    private func reloadRowForAttachmentKey(_ targetKey: String) {
        guard let tableView else { return }
        var matchedIndexes = IndexSet()
        for idx in userAnchorRowIndices where idx < rows.count {
            let row = rows[idx]
            if case let .user(msg, _, namesOnly) = row.kind, !namesOnly, let attachments = msg?.attachments {
                let prefix = "\(sessionScopeKey):\(msg?.id ?? row.id):"
                if targetKey.hasPrefix(prefix), attachments.contains(where: { "\(prefix)\($0.id)" == targetKey }) {
                    matchedIndexes.insert(idx)
                }
            }
        }
        if !matchedIndexes.isEmpty {
            tableView.reloadData(forRowIndexes: matchedIndexes, columnIndexes: IndexSet(integer: 0))
        }
    }

    deinit {
        highlightClearTask?.cancel()
        NotificationCenter.default.removeObserver(self)
    }

    var palette: DesignPalette {
        DesignPalette.resolve(scheme: scheme, workspace: workspace)
    }

    func contentColumnWidth(for totalWidth: CGFloat) -> CGFloat {
        let insets = LayoutMetrics.readingHorizontalInsets(
            availableWidth: totalWidth,
            hasTimelineRail: hasTimelineRail,
            isSubagent: isSubagent
        )
        return max(160, totalWidth - insets.leading - insets.trailing)
    }

    // MARK: - 0.0pt Same-Frame Scroll Anchor Capture & Restore

    func captureScrollAnchor() -> ScrollAnchorSnapshot? {
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
                    deltaMaxY: rect.maxY - viewportTop,
                    savedRowHeight: rect.height,
                    wasFirstRow: idx == 0,
                    savedScrollTop: viewportTop,
                    savedContentHeight: renderedContentHeight
                )
            }
        }
        return nil
    }

    private func isHeightCacheValid(forWidthKey widthKey: Int, disclosureRevision: UInt64) -> Bool {
        let dimensionsMatch: Bool = widthKey > 0
            && widthKey == heightCacheWidthKey
            && disclosureRevision == heightCacheDisclosureRev
            && cachedRowHeights.count == rows.count
        let contextMatch: Bool = scheme == heightCacheScheme
            && workspace == heightCacheWorkspace
            && sessionScopeKey == heightCacheScopeKey
            && Self.effectiveScrollerStyle == heightCacheScrollerStyle
        return dimensionsMatch && contextMatch
    }

    private struct HeightCacheUpdatePlan {
        let changedHeightIndexes: IndexSet
        let isPureTailAppend: Bool
        let modifiedPrefixIndexes: IndexSet
    }

    private func rebuildRowIndexesAndCarryHeightCache(nextRows: [ConversationRowItem], currentWidthKey: Int) -> HeightCacheUpdatePlan {
        let currentScrollerStyle = Self.effectiveScrollerStyle
        let canCarry = isHeightCacheValid(
            forWidthKey: currentWidthKey,
            disclosureRevision: disclosureStore.revision
        )

        var previousById: [String: (item: ConversationRowItem, height: CGFloat)] = [:]
        if canCarry {
            previousById.reserveCapacity(rows.count)
            for (idx, oldRow) in rows.enumerated() {
                if let h = cachedRowHeights[idx] {
                    previousById[oldRow.id] = (oldRow, h)
                }
            }
        }

        let oldCount = rows.count
        var commonPrefix = 0
        if canCarry && oldCount > 0 && nextRows.count >= oldCount {
            while commonPrefix < oldCount && nextRows[commonPrefix].id == rows[commonPrefix].id {
                commonPrefix += 1
            }
        }
        let prefixIdsMatch = canCarry
            && oldCount > 0
            && nextRows.count >= oldCount
            && (commonPrefix == oldCount || (oldCount >= 2 && commonPrefix == oldCount - 1))

        var nextHeights = Array<CGFloat?>(repeating: nil, count: nextRows.count)
        var nextUserIndices: [Int] = []
        var anyImages = false
        var modifiedPrefix = IndexSet()

        for (idx, r) in nextRows.enumerated() {
            if prefixIdsMatch, idx >= commonPrefix, idx < oldCount {
                modifiedPrefix.insert(idx)
            } else if canCarry, let prev = previousById[r.id], prev.item == r {
                nextHeights[idx] = prev.height
            } else if prefixIdsMatch, idx < oldCount {
                modifiedPrefix.insert(idx)
            }
            if case let .user(msg, _, namesOnly) = r.kind {
                nextUserIndices.append(idx)
                if !namesOnly, let atts = msg?.attachments, atts.contains(where: { $0.kind == "image" && $0.reason == nil }) {
                    anyImages = true
                }
            }
        }

        let changedHeightIndexes: IndexSet
        if prefixIdsMatch {
            var changed = modifiedPrefix
            if nextRows.count > oldCount {
                changed.insert(integersIn: oldCount ..< nextRows.count)
            }
            changedHeightIndexes = changed
        } else if !canCarry || nextRows.count != oldCount {
            changedHeightIndexes = IndexSet(integersIn: 0 ..< nextRows.count)
        } else {
            var changed = IndexSet()
            for idx in 0 ..< nextRows.count {
                if nextHeights[idx] == nil || nextHeights[idx] != cachedRowHeights[idx] {
                    changed.insert(idx)
                }
            }
            changedHeightIndexes = changed
        }

        cachedRowHeights = nextHeights
        heightCacheWidthKey = currentWidthKey
        heightCacheDisclosureRev = disclosureStore.revision
        heightCacheScheme = scheme
        heightCacheWorkspace = workspace
        heightCacheScopeKey = sessionScopeKey
        heightCacheScrollerStyle = currentScrollerStyle
        userAnchorRowIndices = nextUserIndices
        hasAnyUserImageAttachments = anyImages
        return HeightCacheUpdatePlan(
            changedHeightIndexes: changedHeightIndexes,
            isPureTailAppend: prefixIdsMatch,
            modifiedPrefixIndexes: modifiedPrefix
        )
    }

    func isNearBottom(threshold: CGFloat = 96) -> Bool {
        guard let tableView, let clipView = scrollView?.contentView else { return true }
        return ConversationViewModel.shouldAutoFollow(
            scrollTop: clipView.bounds.origin.y,
            clientHeight: clipView.bounds.height,
            scrollHeight: tableView.frame.height,
            threshold: threshold
        )
    }

    @discardableResult
    static func runBoundedOlderContinuationLoop(
        coordinator: @MainActor () -> ConversationTableCoordinator?,
        currentRowCount: @MainActor () -> Int,
        upgradeLimitOnSparseRows: Bool,
        loadStep: @MainActor (_ limitOverride: Int?) async -> Bool
    ) async -> Bool {
        var anyLoaded = false
        for step in 0 ..< 5 {
            let prevHeight = coordinator()?.renderedContentHeight ?? 0
            let prevCount = currentRowCount()
            let limitOverride: Int? = (step > 0 || (upgradeLimitOnSparseRows && prevCount <= 2)) ? 500 : nil
            let loaded = await loadStep(limitOverride)
            if !loaded { break }
            anyLoaded = true
            guard let coord = coordinator() else { break }
            let newHeight = coord.renderedContentHeight
            let newCount = currentRowCount()
            let needsMore = coord.isViewportUnderfilled
                || ((newHeight - prevHeight) < 48 && coord.isNearTopForContinuation)
                || (newCount <= prevCount && newCount <= 2)
            if !needsMore { break }
        }
        return anyLoaded
    }

    private func findAnchorRowIndex(for timelineId: String, in list: [ConversationRowItem]) -> Int? {
        if let exact = list.firstIndex(where: { $0.timelineId == timelineId }) {
            return exact
        }
        return list.firstIndex(where: { $0.timelineAliases.contains(timelineId) })
    }

    func updateRowsPreservingAnchor(
        _ nextRows: [ConversationRowItem],
        preferAutoFollowBottom: Bool = false,
        forceScrollToBottom: Bool = false
    ) {
        let totalW = max(320, scrollView?.bounds.width ?? tableView?.bounds.width ?? 0)
        let widthKey = totalW > 0 ? Int((contentColumnWidth(for: totalW) * 2).rounded()) : 0
        let oldCount = rows.count
        let scopeReset = forceScrollToBottom || sessionScopeKey != cellCacheScopeKey
        let updatePlan = rebuildRowIndexesAndCarryHeightCache(nextRows: nextRows, currentWidthKey: widthKey)
        let changedHeightIndexes = updatePlan.changedHeightIndexes

        guard let tableView, let clipView = scrollView?.contentView else {
            rows = nextRows
            return
        }
        let wasEmpty = rows.isEmpty
        if scopeReset {
            if sessionScopeKey != cellCacheScopeKey {
                highlightClearTask?.cancel()
                highlightClearTask = nil
                highlightedTimelineId = nil
                attachmentViewport.resetScope(sessionScopeKey)
            }
            if forceScrollToBottom {
                upwardScrollIntent = false
            }
            evictOffscreenCaches()
            cellCacheScopeKey = sessionScopeKey
        }
        let shouldFollow = forceScrollToBottom || (preferAutoFollowBottom && !upwardScrollIntent && (wasEmpty || isNearBottom()))
        let anchor = shouldFollow ? nil : captureScrollAnchor()
        let canUseIncrementalAppend = !scopeReset
            && updatePlan.isPureTailAppend
            && tableView.numberOfRows == oldCount

        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0
            context.allowsImplicitAnimation = false
            self.rows = nextRows
            if canUseIncrementalAppend {
                if nextRows.count > oldCount {
                    tableView.insertRows(
                        at: IndexSet(integersIn: oldCount ..< nextRows.count),
                        withAnimation: []
                    )
                }
                if !updatePlan.modifiedPrefixIndexes.isEmpty {
                    tableView.reloadData(
                        forRowIndexes: updatePlan.modifiedPrefixIndexes,
                        columnIndexes: IndexSet(integer: 0)
                    )
                }
                if !changedHeightIndexes.isEmpty {
                    tableView.noteHeightOfRows(withIndexesChanged: changedHeightIndexes)
                }
            } else {
                tableView.reloadData()
                if !changedHeightIndexes.isEmpty {
                    tableView.noteHeightOfRows(withIndexesChanged: changedHeightIndexes)
                }
            }
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
                    let restoredY: CGFloat
                    if !preferAutoFollowBottom
                        && anchor.wasFirstRow
                        && anchor.savedRowHeight > 0
                        && newRect.height > anchor.savedRowHeight + 0.5
                    {
                        restoredY = max(minY, newRect.maxY - anchor.deltaMaxY)
                    } else {
                        restoredY = max(minY, newRect.minY - anchor.deltaY)
                    }
                    clipView.setBoundsOrigin(NSPoint(x: 0, y: restoredY))
                    self.scrollView?.reflectScrolledClipView(clipView)
                } else if anchor.savedContentHeight > 0 && abs(self.renderedContentHeight - anchor.savedContentHeight) > 0.5 {
                    let restoredY = max(minY, anchor.savedScrollTop + (self.renderedContentHeight - anchor.savedContentHeight))
                    clipView.setBoundsOrigin(NSPoint(x: 0, y: restoredY))
                    self.scrollView?.reflectScrolledClipView(clipView)
                }
            }
            self.lastClipOriginY = clipView.bounds.origin.y
        }
        isRestoringScroll = false
        refreshViewportAttachmentsAndTimeline()
    }

    func restoreExplicitAnchor(_ anchor: ScrollAnchorSnapshot) {
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

    private static func executionGroupReferencesDisclosure(_ group: ExecutionGroup, targetKeys: Set<String>) -> Bool {
        if targetKeys.contains(group.id) { return true }
        for alias in group.stateAliases where targetKeys.contains(alias) {
            return true
        }
        for cmd in group.commands {
            if targetKeys.contains(cmd.id) { return true }
            for sid in cmd.sourceIds where targetKeys.contains(sid) {
                return true
            }
        }
        return false
    }

    private static func claudeEntriesReferenceDisclosure(_ entries: [ClaudeEventRenderItem], targetKeys: Set<String>) -> Bool {
        for entry in entries {
            switch entry {
            case let .group(group):
                if executionGroupReferencesDisclosure(group, targetKeys: targetKeys) { return true }
            case let .metadata(meta):
                if targetKeys.contains(meta.id) { return true }
                for sid in meta.sourceIds where targetKeys.contains("claude-metadata:\(sid)") {
                    return true
                }
                if claudeEntriesReferenceDisclosure(meta.renderItems, targetKeys: targetKeys) { return true }
            case let .item(_, key, sids):
                if targetKeys.contains(key) || targetKeys.contains("\(key):thinking") { return true }
                for sid in sids {
                    if targetKeys.contains(sid) || targetKeys.contains("\(sid):thinking") {
                        return true
                    }
                }
            }
        }
        return false
    }

    private static func rowMayReferenceDisclosure(_ row: ConversationRowItem, targetKeys: Set<String>) -> Bool {
        guard !targetKeys.isEmpty else { return false }
        switch row.kind {
        case let .user(msg, _, _):
            let p1 = "\(msg?.id ?? row.timelineId):"
            let p2 = "\(row.timelineId):"
            return targetKeys.contains { $0.hasPrefix(p1) || $0.hasPrefix(p2) }
        case let .assistant(_, foldInstructions):
            guard foldInstructions else { return false }
            let prefix = "\(row.id):"
            return targetKeys.contains { $0.hasPrefix(prefix) }
        case let .executionGroup(group):
            return executionGroupReferencesDisclosure(group, targetKeys: targetKeys)
        case let .processGroup(proc):
            if targetKeys.contains(proc.stateKey) { return true }
            for alias in proc.stateAliases where targetKeys.contains(alias) {
                return true
            }
            for step in proc.steps {
                switch step {
                case let .thinking(id, sids, _, _, _):
                    if targetKeys.contains("\(id):thinking") { return true }
                    for sid in sids where targetKeys.contains("\(sid):thinking") {
                        return true
                    }
                case let .execution(group, _):
                    if executionGroupReferencesDisclosure(group, targetKeys: targetKeys) { return true }
                case let .detail(id, sids, _, _, _, _, _):
                    if targetKeys.contains(id) { return true }
                    for sid in sids where targetKeys.contains(sid) {
                        return true
                    }
                }
            }
            return false
        case let .claudeProcessGroup(proc):
            if targetKeys.contains(proc.stateKey) { return true }
            for sid in proc.sourceIds where targetKeys.contains("claude-process:\(proc.turnId):\(sid)") {
                return true
            }
            return claudeEntriesReferenceDisclosure(proc.entries, targetKeys: targetKeys)
        case let .claudeMetadataGroup(meta):
            if targetKeys.contains(meta.id) { return true }
            for sid in meta.sourceIds where targetKeys.contains("claude-metadata:\(sid)") {
                return true
            }
            return claudeEntriesReferenceDisclosure(meta.renderItems, targetKeys: targetKeys)
        case .thinking:
            if targetKeys.contains(row.id) { return true }
            for alias in row.timelineAliases where targetKeys.contains(alias) {
                return true
            }
            return false
        case .plan, .question, .questionResult, .subagent, .historyCollapse, .systemNotice:
            return false
        }
    }

    func toggleDisclosurePreservingAnchor(
        stateKey: String,
        aliases: [String] = [],
        currentlyOpen: Bool
    ) {
        let anchor = captureScrollAnchor()
        let targetKeys = Set(([stateKey] + aliases).filter { !$0.isEmpty })
        var affected = IndexSet()
        for (idx, row) in rows.enumerated() where Self.rowMayReferenceDisclosure(row, targetKeys: targetKeys) {
            affected.insert(idx)
        }
        if affected.isEmpty && !rows.isEmpty {
            affected = IndexSet(integersIn: 0 ..< rows.count)
        }

        let totalW = max(320, scrollView?.bounds.width ?? tableView?.bounds.width ?? 0)
        let widthKey = totalW > 0 ? Int((contentColumnWidth(for: totalW) * 2).rounded()) : 0
        let canPartiallyInvalidate = isHeightCacheValid(
            forWidthKey: widthKey,
            disclosureRevision: disclosureStore.revision
        )

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
            let reloadIndexes: IndexSet
            if canPartiallyInvalidate {
                for idx in affected {
                    self.cachedRowHeights[idx] = nil
                }
                self.heightCacheDisclosureRev = self.disclosureStore.revision
                reloadIndexes = affected
            } else {
                self.cachedRowHeights = Array<CGFloat?>(repeating: nil, count: self.rows.count)
                self.heightCacheWidthKey = widthKey
                self.heightCacheDisclosureRev = self.disclosureStore.revision
                self.heightCacheScheme = self.scheme
                self.heightCacheWorkspace = self.workspace
                self.heightCacheScopeKey = self.sessionScopeKey
                self.heightCacheScrollerStyle = Self.effectiveScrollerStyle
                reloadIndexes = IndexSet(integersIn: 0 ..< self.rows.count)
            }
            if let tableView = self.tableView {
                if !reloadIndexes.isEmpty {
                    tableView.reloadData(forRowIndexes: reloadIndexes, columnIndexes: IndexSet(integer: 0))
                    tableView.noteHeightOfRows(withIndexesChanged: reloadIndexes)
                }
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
    func scrollToPositionKey(_ positionKey: String, reduceMotion: Bool = false) -> Bool {
        guard let tableView, let clipView = scrollView?.contentView else { return false }
        guard let rowIdx = rows.firstIndex(where: { $0.matches(positionKey: positionKey) }) else {
            return false
        }
        // Auto-open disclosure if jumping to a folded group, thinking row, or instruction/context fold (DESIGN.md:43, 140)
        let totalW = max(320, scrollView?.bounds.width ?? tableView.bounds.width)
        let widthKey = Int((contentColumnWidth(for: totalW) * 2).rounded())
        let canPartiallyInvalidate = isHeightCacheValid(
            forWidthKey: widthKey,
            disclosureRevision: disclosureStore.revision
        )
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
        case let .claudeProcessGroup(proc):
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
        case let .claudeMetadataGroup(meta):
            openClaudeMetadataGroup(meta, matching: positionKey)
            didMutateDisclosure = true
        case .thinking:
            disclosureStore.setOpen(scope: sessionScopeKey, stateKey: targetRow.id, aliases: targetRow.timelineAliases, isOpen: true)
            didMutateDisclosure = true
        case let .user(msg, rawText, _):
            let (proj, segs) = UserContextEngine.projectAndSegmentUserMessage(
                msg?.text ?? rawText,
                contextBlocks: msg?.contextBlocks
            )
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
            let reloadIndexes: IndexSet
            if canPartiallyInvalidate {
                cachedRowHeights[rowIdx] = nil
                heightCacheDisclosureRev = disclosureStore.revision
                reloadIndexes = IndexSet(integer: rowIdx)
            } else {
                cachedRowHeights = Array<CGFloat?>(repeating: nil, count: rows.count)
                heightCacheWidthKey = widthKey
                heightCacheDisclosureRev = disclosureStore.revision
                heightCacheScheme = scheme
                heightCacheWorkspace = workspace
                heightCacheScopeKey = sessionScopeKey
                heightCacheScrollerStyle = Self.effectiveScrollerStyle
                reloadIndexes = IndexSet(integersIn: 0 ..< rows.count)
            }
            tableView.reloadData(forRowIndexes: reloadIndexes, columnIndexes: IndexSet(integer: 0))
            tableView.noteHeightOfRows(withIndexesChanged: reloadIndexes)
            tableView.tile()
            tableView.layoutSubtreeIfNeeded()
        }

        let rect = tableView.rect(ofRow: rowIdx)
        let minY = -(scrollView?.contentInsets.top ?? 0)
        let maxScroll = max(minY, tableView.frame.height - clipView.bounds.height)
        let targetY = max(minY, min(maxScroll, rect.minY - 16))
        if targetY < maxScroll - 1.0 {
            upwardScrollIntent = true
        }
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
                let expiredId = self.highlightedTimelineId
                self.highlightedTimelineId = nil
                if let expiredId, let idx = self.rows.firstIndex(where: { $0.timelineId == expiredId }) {
                    self.tableView?.reloadData(forRowIndexes: IndexSet(integer: idx), columnIndexes: IndexSet(integer: 0))
                }
            }
        }
        refreshViewportAttachmentsAndTimeline()
        return true
    }

    // MARK: - Scroll Bounds Observation (±240pt Thumbnail Eviction, <=200pt Pagination, Timeline Active Tick)

    @objc func scrollViewFrameDidChange(_ notification: Notification) {
        guard !isRestoringScroll, let scrollView, let tableView else { return }
        let w = scrollView.bounds.width
        guard w > 1, abs(w - lastLaidOutWidth) > 0.5 else { return }
        let wasZero = (lastLaidOutWidth == 0)
        let prevContentW = wasZero ? 0 : contentColumnWidth(for: max(320, lastLaidOutWidth))
        let nextContentW = contentColumnWidth(for: max(320, w))
        if !wasZero, abs(nextContentW - prevContentW) < 0.5 {
            lastLaidOutWidth = w
            column?.width = w
            let insets = LayoutMetrics.readingHorizontalInsets(
                availableWidth: max(320, w),
                hasTimelineRail: hasTimelineRail,
                isSubagent: isSubagent
            )
            tableView.tile()
            tableView.enumerateAvailableRowViews { rowView, _ in
                if let cell = rowView.view(atColumn: 0) as? ConversationRowCellView {
                    cell.updateHorizontalInset(insets.leading)
                }
            }
            tableView.layoutSubtreeIfNeeded()
            refreshViewportAttachmentsAndTimeline()
            return
        }
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

    var renderedContentHeight: CGFloat {
        guard let tableView, !rows.isEmpty else { return 0 }
        return tableView.rect(ofRow: rows.count - 1).maxY
    }

    var isViewportUnderfilled: Bool {
        guard let scrollView, scrollView.bounds.height > 1 else { return false }
        let visibleBudget = scrollView.contentView.bounds.height - scrollView.contentInsets.top - scrollView.contentInsets.bottom
        return visibleBudget > 64 && renderedContentHeight + 1 < visibleBudget
    }

    var isNearTopForContinuation: Bool {
        guard let scrollView, scrollView.bounds.height > 1 else { return false }
        return scrollView.contentView.bounds.origin.y <= 200
    }

    private func requestLoadOlderIfEligible() {
        guard canLoadOlder, !isLoadingOlder, !isLoadOlderBlockedByError else { return }
        onLoadOlderRequested?()
    }

    func handleOuterScrollWheel(
        deltaY: CGFloat,
        phase _: NSEvent.Phase = [],
        momentumPhase: NSEvent.Phase = []
    ) {
        if deltaY > 0.1 {
            upwardScrollIntent = true
            let originY = scrollView?.contentView.bounds.origin.y ?? 0
            if originY <= 200 {
                requestLoadOlderIfEligible()
            }
        } else if deltaY < -0.5, momentumPhase.isEmpty {
            let minY = -(scrollView?.contentInsets.top ?? 0)
            let originY = scrollView?.contentView.bounds.origin.y ?? 0
            if originY > minY + 1.0 {
                upwardScrollIntent = false
            }
        }
    }

    func handleOuterUpwardKeyNavigation() {
        upwardScrollIntent = true
        let originY = scrollView?.contentView.bounds.origin.y ?? 0
        if originY <= 200 {
            requestLoadOlderIfEligible()
        }
    }

    func markUpwardNavigationIntent() {
        upwardScrollIntent = true
    }

    @objc func preferredScrollerStyleDidChange(_ notification: Notification) {
        guard !isRestoringScroll, let tableView else { return }
        cachedRowHeights = Array<CGFloat?>(repeating: nil, count: rows.count)
        heightCacheScrollerStyle = Self.effectiveScrollerStyle
        evictOffscreenCaches()
        let anchor = captureScrollAnchor()
        isRestoringScroll = true
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0
            ctx.allowsImplicitAnimation = false
            tableView.reloadData()
            tableView.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0 ..< rows.count))
            tableView.tile()
            tableView.layoutSubtreeIfNeeded()
            if let anchor,
               let clipView = scrollView?.contentView,
               let idx = findAnchorRowIndex(for: anchor.timelineId, in: rows)
            {
                let minY = -(scrollView?.contentInsets.top ?? 0)
                let rect = tableView.rect(ofRow: idx)
                clipView.setBoundsOrigin(NSPoint(x: 0, y: max(minY, rect.minY - anchor.deltaY)))
                scrollView?.reflectScrolledClipView(clipView)
                lastClipOriginY = clipView.bounds.origin.y
            }
        }
        isRestoringScroll = false
    }

    @objc func clipViewBoundsDidChange(_ notification: Notification) {
        guard !isRestoringScroll, let scrollView else { return }
        let clipView = scrollView.contentView
        let originY = clipView.bounds.origin.y
        let minY = -scrollView.contentInsets.top
        if originY <= minY + 1.0 {
            if lastClipOriginY == .greatestFiniteMagnitude || originY < lastClipOriginY - 0.5 {
                upwardScrollIntent = true
            }
        } else {
            let movedUp = (lastClipOriginY == .greatestFiniteMagnitude) || (originY < lastClipOriginY - 0.5)
            if movedUp {
                upwardScrollIntent = true
            } else if originY > lastClipOriginY + 0.5 {
                upwardScrollIntent = false
            }
        }
        lastClipOriginY = originY

        if upwardScrollIntent && originY <= 200 {
            requestLoadOlderIfEligible()
        }
        refreshViewportAttachmentsAndTimeline()
    }

    func refreshViewportAttachmentsAndTimeline() {
        guard let tableView, let clipView = scrollView?.contentView else { return }

        if hasAnyUserImageAttachments
            || attachmentViewport.loadedCount > 0
            || attachmentViewport.failedCount > 0
            || attachmentViewport.inFlightCount > 0
        {
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

        // Compute active user timeline entry via O(log U) binary search on precomputed user row indices
        if !userAnchorRowIndices.isEmpty {
            let viewportTop = clipView.bounds.origin.y + 28
            var low = 0
            var high = userAnchorRowIndices.count
            while low < high {
                let mid = (low + high) / 2
                let rowIdx = userAnchorRowIndices[mid]
                if rowIdx < rows.count, tableView.rect(ofRow: rowIdx).minY <= viewportTop {
                    low = mid + 1
                } else {
                    high = mid
                }
            }
            let chosenRowIdx = userAnchorRowIndices[max(0, low - 1)]
            let activeId: String? = chosenRowIdx < rows.count ? rows[chosenRowIdx].timelineId : nil
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

    func numberOfRows(in tableView: NSTableView) -> Int {
        rows.count
    }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        guard row >= 0, row < rows.count else { return 44 }
        let colWidth = contentColumnWidth(for: max(320, scrollView?.bounds.width ?? tableView.bounds.width))
        let widthKey = Int((colWidth * 2).rounded())
        let discRev = disclosureStore.revision

        let currentScrollerStyle = Self.effectiveScrollerStyle
        if widthKey != heightCacheWidthKey
            || discRev != heightCacheDisclosureRev
            || scheme != heightCacheScheme
            || workspace != heightCacheWorkspace
            || sessionScopeKey != heightCacheScopeKey
            || currentScrollerStyle != heightCacheScrollerStyle
            || cachedRowHeights.count != rows.count
        {
            cachedRowHeights = Array<CGFloat?>(repeating: nil, count: rows.count)
            heightCacheWidthKey = widthKey
            heightCacheDisclosureRev = discRev
            heightCacheScheme = scheme
            heightCacheWorkspace = workspace
            heightCacheScopeKey = sessionScopeKey
            heightCacheScrollerStyle = currentScrollerStyle
        }

        if let cached = cachedRowHeights[row] {
            return cached
        }
        let computed = max(32, ceil(measureRowHeight(rows[row], width: colWidth)))
        cachedRowHeights[row] = computed
        return computed
    }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        false
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row >= 0, row < rows.count else { return nil }
        if sessionScopeKey != cellCacheScopeKey {
            cellCache.removeAll()
            attachmentViewport.resetScope(sessionScopeKey)
            cellCacheScopeKey = sessionScopeKey
        }
        let item = rows[row]
        let identifier = NSUserInterfaceItemIdentifier("ConversationRowCell")
        let cell: ConversationRowCellView
        if let cached = cellCache.value(forKey: item.id), cached.superview == nil, cached.cachedRowId == item.id {
            cell = cached
        } else {
            let created = (tableView.makeView(withIdentifier: identifier, owner: nil) as? ConversationRowCellView)
                ?? ConversationRowCellView(identifier: identifier)
            if let prevId = created.cachedRowId, prevId != item.id, cellCache.peek(prevId) === created {
                _ = cellCache.removeValue(forKey: prevId)
            }
            created.cachedRowId = item.id
            cellCache.setValue(created, forKey: item.id)
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

    struct PositionedUserAttachment {
        let attachment: UserAttachment
        let size: NSSize
    }

    static func measureFileAttachmentCardSize(
        att: UserAttachment,
        namesOnly: Bool,
        maxWidth: CGFloat
    ) -> NSSize {
        let nameW: CGFloat = ceil(NSAttributedString(
            string: att.name,
            attributes: [.font: DesignFonts.nsBodyFont(size: 14)]
        ).size().width)
        let reasonW: CGFloat = if let reason = att.reason, !reason.isEmpty {
            ceil(NSAttributedString(
                string: reason,
                attributes: [.font: DesignFonts.nsBodyFont(size: 11)]
            ).size().width)
        } else {
            0
        }
        let hasCopy = !namesOnly && (att.path?.isEmpty == false)
        let leadingChromeW: CGFloat = 12 + 18 + 8
        let trailingChromeW: CGFloat = 10 + 6
        let copyButtonWithSpacingW: CGFloat = 8 + 28
        let copyActionW: CGFloat = hasCopy ? copyButtonWithSpacingW : 0
        let textW: CGFloat = max(nameW, reasonW)
        let naturalW: CGFloat = leadingChromeW + textW + copyActionW + trailingChromeW
        let clampedMax: CGFloat = max(120, min(maxWidth, 340))
        let minAllowedW: CGFloat = min(120, maxWidth)
        let clampedNaturalW: CGFloat = min(clampedMax, naturalW)
        let w: CGFloat = min(maxWidth, max(minAllowedW, clampedNaturalW))
        let h: CGFloat = (att.reason?.isEmpty == false) ? 52 : 40
        return NSSize(width: w, height: h)
    }

    static func layoutUserAttachmentRows(
        attachments: [UserAttachment],
        namesOnly: Bool,
        maxWidth: CGFloat
    ) -> [[PositionedUserAttachment]] {
        guard !attachments.isEmpty else { return [] }
        let usableWidth = max(120, maxWidth)
        var rows: [[PositionedUserAttachment]] = []
        var currentRow: [PositionedUserAttachment] = []
        var currentWidth: CGFloat = 0

        for att in attachments {
            let itemSize: NSSize = (!namesOnly && att.kind == "image")
                ? NSSize(width: 96, height: 96)
                : measureFileAttachmentCardSize(att: att, namesOnly: namesOnly, maxWidth: usableWidth)
            let needed = currentRow.isEmpty ? itemSize.width : (currentWidth + 8 + itemSize.width)
            if !currentRow.isEmpty && needed > usableWidth + 0.5 {
                rows.append(currentRow)
                currentRow = [PositionedUserAttachment(attachment: att, size: itemSize)]
                currentWidth = itemSize.width
            } else {
                currentRow.append(PositionedUserAttachment(attachment: att, size: itemSize))
                currentWidth = needed
            }
        }
        if !currentRow.isEmpty {
            rows.append(currentRow)
        }
        return rows
    }

    static func measureUserAttachmentsHeight(
        attachments: [UserAttachment],
        namesOnly: Bool,
        maxWidth: CGFloat
    ) -> CGFloat {
        let rows = layoutUserAttachmentRows(attachments: attachments, namesOnly: namesOnly, maxWidth: maxWidth)
        guard !rows.isEmpty else { return 0 }
        var total: CGFloat = 0
        for (idx, row) in rows.enumerated() {
            if idx > 0 { total += 8 }
            total += row.map(\.size.height).max() ?? 40
        }
        return total
    }

    func measureRowHeight(_ row: ConversationRowItem, width: CGFloat) -> CGFloat {
        switch row.kind {
        case let .user(msg, rawText, namesOnly):
            let body = msg?.text ?? rawText
            let (proj, segs) = UserContextEngine.projectAndSegmentUserMessage(body, contextBlocks: msg?.contextBlocks)
            var h: CGFloat = 8
            var blockCount = 0
            if let atts = msg?.attachments, !atts.isEmpty {
                h += Self.measureUserAttachmentsHeight(attachments: atts, namesOnly: namesOnly, maxWidth: width) + 4
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
                    let singleLineW: CGFloat = max(36, ceil(attr.size().width) + 4)
                    let maxTextW: CGFloat = maxCardW - (bubbleHPad * 2)
                    let textW: CGFloat = min(maxTextW, singleLineW)
                    let textH: CGFloat = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: textW))
                    h += textH + 24
                case let .reply(segId, q, a):
                    let trimmedQ = q.trimmingCharacters(in: .whitespacesAndNewlines)
                    let availW: CGFloat = max(80, maxCardW - (bubbleHPad * 2) - 38)
                    let aAttr = NSAttributedString(string: a, attributes: attrs)
                    let singleAnsW: CGFloat = max(64, ceil(aAttr.size().width) + 4)
                    var computedAnsW: CGFloat = min(availW, singleAnsW)
                    var qExtra: CGFloat = 0
                    if !trimmedQ.isEmpty {
                        let qKey = "\(msg?.id ?? row.timelineId):\(segId):question"
                        let qOpen = disclosureStore.isOpen(
                            scope: sessionScopeKey,
                            stateKey: qKey,
                            aliases: ["\(row.timelineId):\(segId):question"],
                            initiallyOpen: false
                        )
                        let qSingleWidth: CGFloat = ceil(NSAttributedString(
                            string: trimmedQ,
                            attributes: [.font: DesignFonts.nsBodyFont(size: 13)]
                        ).size().width) + 26
                        let targetInnerW: CGFloat = min(availW, max(computedAnsW, qSingleWidth))
                        computedAnsW = max(computedAnsW, targetInnerW)
                        if qOpen {
                            let qAttr = NSAttributedString(
                                string: trimmedQ,
                                attributes: [.font: DesignFonts.nsBodyFont(size: 13)]
                            )
                            let measuredQH: CGFloat = max(20, ReaderSelectableTextView.measureHeight(attributed: qAttr, width: targetInnerW))
                            qExtra = 28 + measuredQH + 6
                        } else {
                            qExtra = 28
                        }
                    }
                    let aH: CGFloat = max(24, ReaderSelectableTextView.measureHeight(attributed: aAttr, width: computedAnsW))
                    h += max(30, qExtra + aH + 24)
                }
            }
            if blockCount > 1 {
                h += CGFloat(blockCount - 1) * 10
            }
            return max(44, h + 4)

        case let .assistant(markdown, foldInstructions):
            // Side-hover copy button is floating in the right margin and NEVER adds vertical height (0.0pt impact)!
            return max(
                28,
                measureMarkdownSegmentsHeight(
                    ownerId: row.id,
                    markdown: markdown,
                    foldInstructions: foldInstructions,
                    width: width,
                    interItemSpacing: 8,
                    alreadyCleaned: true
                ) + 16
            )

        case let .executionGroup(group):
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: group.id,
                aliases: group.stateAliases,
                initiallyOpen: group.initiallyOpen
            )
            if !isOpen { return ProcessRowLayoutMetrics.collapsedGroupRowHeight }
            return ProcessRowLayoutMetrics.collapsedGroupRowHeight + 16 + measureExecutionCommandsHeight(commands: group.commands, width: max(120, width - 20))

        case let .processGroup(proc):
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: proc.stateKey,
                aliases: proc.stateAliases,
                initiallyOpen: proc.initiallyOpen
            )
            if !isOpen { return ProcessRowLayoutMetrics.collapsedGroupRowHeight }
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
                            ? (ProcessRowLayoutMetrics.thoughtDisclosureHeaderHeight + 6 + measureThoughtMarkdownHeight(markdown: text, width: bodyW))
                            : ProcessRowLayoutMetrics.thoughtDisclosureHeaderHeight
                    } else {
                        bodyH += measureThoughtMarkdownHeight(markdown: text, width: bodyW)
                    }
                case let .execution(group, _):
                    bodyH += 4 + measureExecutionCommandsHeight(commands: group.commands, width: max(100, bodyW - 20))
                case let .detail(id, sids, _, detail, running, failed, _):
                    let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: id, aliases: sids, initiallyOpen: running || failed)
                    bodyH += open
                        ? (ProcessRowLayoutMetrics.compactRowHeight + 6 + Self.measureMonospaceBoxHeight(text: detail, width: bodyW))
                        : ProcessRowLayoutMetrics.compactRowHeight
                }
            }
            return ProcessRowLayoutMetrics.collapsedGroupRowHeight + bodyH

        case let .claudeProcessGroup(proc):
            let aliases = proc.sourceIds.map { "claude-process:\(proc.turnId):\($0)" }
            let isOpen = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: proc.stateKey,
                aliases: aliases,
                initiallyOpen: false
            )
            if !isOpen { return ProcessRowLayoutMetrics.collapsedGroupRowHeight }
            let bodyW = max(120, width - 22)
            return ProcessRowLayoutMetrics.collapsedGroupRowHeight + 14 + measureClaudeEntriesHeight(
                entries: proc.entries,
                width: bodyW,
                wrapThinkingDisclosures: proc.wrapsThinkingDisclosures
            )

        case let .claudeMetadataGroup(meta):
            let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
            let isOpen = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: meta.id, aliases: aliases, initiallyOpen: false)
            if !isOpen { return ProcessRowLayoutMetrics.collapsedGroupRowHeight }
            let bodyW = max(100, width - 22)
            return ProcessRowLayoutMetrics.collapsedGroupRowHeight + 14 + measureClaudeEntriesHeight(entries: meta.renderItems, width: bodyW, wrapThinkingDisclosures: false)

        case let .thinking(_, text, _):
            let open = disclosureStore.isOpen(
                scope: sessionScopeKey,
                stateKey: row.id,
                aliases: row.timelineAliases,
                initiallyOpen: false
            )
            if !open { return ProcessRowLayoutMetrics.collapsedGroupRowHeight }
            return ProcessRowLayoutMetrics.collapsedGroupRowHeight + 14 + measureThoughtMarkdownHeight(markdown: text, width: max(120, width - 22))

        case let .plan(markdown, _, _):
            let innerWidth = max(120, width - 35)
            let segsH = measureMarkdownSegmentsHeight(
                ownerId: row.id,
                markdown: markdown,
                foldInstructions: false,
                width: innerWidth,
                interItemSpacing: 10,
                allowInstructionsFold: false,
                alreadyCleaned: true
            )
            return segsH > 0 ? (72 + segsH) : 62

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
            h += ProcessRowLayoutMetrics.compactRowHeight
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
                    open
                        ? (ProcessRowLayoutMetrics.compactRowHeight + 16 + measureExecutionCommandsHeight(commands: group.commands, width: max(120, width - 20)))
                        : ProcessRowLayoutMetrics.compactRowHeight
                )
            case let .metadata(meta):
                let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: meta.id, aliases: aliases, initiallyOpen: false)
                if open {
                    appendRenderedHeight(
                        ProcessRowLayoutMetrics.compactRowHeight + 16 + measureClaudeEntriesHeight(
                            entries: meta.renderItems,
                            width: max(100, width - 22),
                            wrapThinkingDisclosures: false
                        )
                    )
                } else {
                    appendRenderedHeight(ProcessRowLayoutMetrics.compactRowHeight)
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
                                ? (ProcessRowLayoutMetrics.thoughtDisclosureHeaderHeight + 6 + measureThoughtMarkdownHeight(markdown: markdown, width: width))
                                : ProcessRowLayoutMetrics.thoughtDisclosureHeaderHeight
                        )
                    } else {
                        appendRenderedHeight(measureThoughtMarkdownHeight(markdown: markdown, width: width))
                    }
                } else if let detail = ev.detail, !detail.isEmpty {
                    let status = ExecutionGroupingEngine.executionStatus(ev.status)
                    let open = disclosureStore.isOpen(
                        scope: sessionScopeKey,
                        stateKey: key,
                        aliases: sids,
                        initiallyOpen: status == .running || status == .failed
                    )
                    appendRenderedHeight(
                        open
                            ? (ProcessRowLayoutMetrics.compactRowHeight + 6 + Self.measureMonospaceBoxHeight(text: detail, width: width))
                            : ProcessRowLayoutMetrics.compactRowHeight
                    )
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    appendRenderedHeight(measureThoughtMarkdownHeight(markdown: text, width: width))
                }
            }
        }
        return total
    }

    static func renderThoughtAttributedString(markdown: String, palette: DesignPalette) -> NSAttributedString {
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

    private func measureThoughtMarkdownHeight(markdown: String, width: CGFloat) -> CGFloat {
        cachedThoughtLayout(markdown: markdown, width: width).height + 4
    }

    func cachedProseHeight(attributed: NSAttributedString, width: CGFloat) -> CGFloat {
        var hasher = Hasher()
        hasher.combine(ObjectIdentifier(attributed))
        hasher.combine(attributed.length)
        let key = SegmentWidthKey(
            contentHash: hasher.finalize(),
            quantizedWidth: Int((width * 2).rounded()),
            scheme: scheme,
            workspace: workspace
        )
        if let hit = proseHeightCache.value(forKey: key) {
            return hit
        }
        let measured = max(22, ReaderSelectableTextView.measureHeight(attributed: attributed, width: width))
        proseHeightCache.setValue(measured, forKey: key)
        return measured
    }

    func cachedThoughtLayout(markdown: String, width: CGFloat) -> (attributed: NSAttributedString, height: CGFloat) {
        var hasher = Hasher()
        hasher.combine(markdown)
        let key = SegmentWidthKey(
            contentHash: hasher.finalize(),
            quantizedWidth: Int((width * 2).rounded()),
            scheme: scheme,
            workspace: workspace
        )
        if let hit = thoughtLayoutCache.value(forKey: key) {
            return hit
        }
        let attr = Self.renderThoughtAttributedString(markdown: markdown, palette: palette)
        let h = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: width))
        let pair = (attributed: attr, height: h)
        thoughtLayoutCache.setValue(pair, forKey: key)
        return pair
    }

    func cachedCodeLayout(code: String, innerWidth: CGFloat) -> (normalized: String, codeHeight: CGFloat, docWidth: CGFloat) {
        var hasher = Hasher()
        hasher.combine(code)
        let key = SegmentWidthKey(
            contentHash: hasher.finalize(),
            quantizedWidth: Int((innerWidth * 2).rounded()),
            scheme: scheme,
            workspace: workspace
        )
        if let hit = codeLayoutCache.value(forKey: key) {
            return hit
        }
        let normalized = Self.normalizeCodeBlockText(code)
        let codeH = Self.measureCodeBlockHeight(code: normalized)
        let docW = Self.measureCodeDocumentWidth(code: normalized, minWidth: innerWidth)
        let result = (normalized: normalized, codeHeight: codeH, docWidth: docW)
        codeLayoutCache.setValue(result, forKey: key)
        return result
    }

    func cachedTableLayout(
        headers: [NSAttributedString],
        rows: [[NSAttributedString]],
        maxWidth: CGFloat
    ) -> (colWidths: [CGFloat], rowHeights: [CGFloat], totalWidth: CGFloat, totalHeight: CGFloat) {
        var hasher = Hasher()
        hasher.combine(headers.count)
        for h in headers {
            hasher.combine(ObjectIdentifier(h))
            hasher.combine(h.length)
        }
        hasher.combine(rows.count)
        for r in rows {
            for c in r {
                hasher.combine(ObjectIdentifier(c))
                hasher.combine(c.length)
            }
        }
        let key = SegmentWidthKey(
            contentHash: hasher.finalize(),
            quantizedWidth: Int((maxWidth * 2).rounded()),
            scheme: scheme,
            workspace: workspace
        )
        if let hit = tableLayoutCache.value(forKey: key) {
            return hit
        }
        let computed = Self.measureTableLayout(headers: headers, rows: rows, maxWidth: maxWidth)
        tableLayoutCache.setValue(computed, forKey: key)
        return computed
    }

    static func measureUserContextBodyHeight(text: String, width: CGFloat) -> CGFloat {
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

    static func normalizeCodeBlockText(_ code: String) -> String {
        if code.hasSuffix("\r\n") {
            return String(code.dropLast(2))
        }
        if code.hasSuffix("\n") {
            return String(code.dropLast())
        }
        return code
    }

    static func measureCodeBlockHeight(code: String) -> CGFloat {
        let normalized = normalizeCodeBlockText(code)
        let lines = max(1, normalized.components(separatedBy: "\n").count)
        return ceil(CGFloat(lines) * 20.8) + 16
    }

    private static let cachedMono13AsciiAdvance: CGFloat = {
        let font = DesignFonts.nsMonoFont(size: 13)
        let sample = NSAttributedString(
            string: String(repeating: "M", count: 100),
            attributes: [.font: font]
        )
        return max(6.0, sample.size().width / 100.0)
    }()

    static func measureCodeDocumentWidth(code: String, minWidth: CGFloat) -> CGFloat {
        let normalized = normalizeCodeBlockText(code)
        guard !normalized.isEmpty else {
            return max(minWidth, ceil(cachedMono13AsciiAdvance) + 24)
        }
        let font = DesignFonts.nsMonoFont(size: 13)
        var maxAsciiCols = 0
        var nonAsciiCandidates: [(estCols: Int, line: Substring)] = []

        for line in normalized.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline) {
            if line.isEmpty { continue }
            var asciiOnly = true
            var cols = 0
            for byte in line.utf8 {
                if byte >= 0x20 && byte < 0x7F {
                    cols += 1
                } else if byte == 0x09 {
                    cols += 4
                } else {
                    asciiOnly = false
                    break
                }
            }
            if asciiOnly {
                if cols > maxAsciiCols {
                    maxAsciiCols = cols
                }
            } else {
                var estCols = 0
                for scalar in line.unicodeScalars {
                    if scalar.value == 0x09 {
                        estCols += 4
                    } else if scalar.value < 0x80 {
                        estCols += 1
                    } else {
                        estCols += 2
                    }
                }
                if nonAsciiCandidates.count < 4 {
                    nonAsciiCandidates.append((estCols, line))
                } else if let minIdx = nonAsciiCandidates.indices.min(by: {
                    nonAsciiCandidates[$0].estCols < nonAsciiCandidates[$1].estCols
                }), estCols > nonAsciiCandidates[minIdx].estCols {
                    nonAsciiCandidates[minIdx] = (estCols, line)
                }
            }
        }

        var maxLineW = CGFloat(maxAsciiCols) * cachedMono13AsciiAdvance
        for (_, candidate) in nonAsciiCandidates {
            let attr = NSAttributedString(string: String(candidate), attributes: [.font: font])
            let w = attr.size().width
            if w > maxLineW {
                maxLineW = w
            }
        }
        return max(minWidth, ceil(maxLineW) + 24)
    }

    static func horizontalScrollerExtraHeight(needsHorizontalScroll: Bool) -> CGFloat {
        guard needsHorizontalScroll, effectiveScrollerStyle == .legacy else {
            return 0
        }
        return ceil(NSScroller.scrollerWidth(for: .regular, scrollerStyle: .legacy))
    }

    private enum TableCharBreakRole {
        case whitespace
        case cjkIdeograph
        case cjkOpening
        case cjkClosing
        case nonBreakable
    }

    private static func classifyTableScalar(_ scalar: Unicode.Scalar) -> TableCharBreakRole {
        if scalar.value == 0x00A0 || scalar.value == 0x202F || scalar.value == 0x2060 || scalar.value == 0xFEFF {
            return .nonBreakable
        }
        if scalar.properties.isWhitespace {
            return .whitespace
        }
        switch scalar.value {
        // UAX #14 OP (Kinsoku 避尾点): prohibited at line end, glues forward to following token
        case 0x3008, 0x300A, 0x300C, 0x300E, 0x3010, 0x3014, 0x3016, 0x3018, 0x301A, 0x301D,
             0xFE35, 0xFE37, 0xFE39, 0xFE3B, 0xFE3D, 0xFE3F, 0xFE41, 0xFE43, 0xFE47,
             0xFE59, 0xFE5B, 0xFE5D,
             0xFF08, 0xFF3B, 0xFF5B, 0xFF5F, 0xFF62:
            return .cjkOpening

        // UAX #14 CL/CP/EX/IS/NS (Kinsoku 避头点): prohibited at line start, glues backward to preceding token
        case 0x3001, 0x3002, 0x3005, 0x3009, 0x300B, 0x300D, 0x300F, 0x3011, 0x3015, 0x3017,
             0x3019, 0x301B, 0x301E, 0x301F, 0x309B, 0x309C, 0x309D, 0x309E, 0x30FB, 0x30FD, 0x30FE,
             0xFE30, 0xFE36, 0xFE38, 0xFE3A, 0xFE3C, 0xFE3E, 0xFE40, 0xFE42, 0xFE44, 0xFE48,
             0xFE50 ... 0xFE57, 0xFE5A, 0xFE5C, 0xFE5E,
             0xFF01, 0xFF09, 0xFF0C, 0xFF0E, 0xFF1A, 0xFF1B, 0xFF1F, 0xFF3D, 0xFF5D, 0xFF60,
             0xFF61, 0xFF63, 0xFF64, 0xFF65, 0xFF70, 0xFF9E, 0xFF9F:
            return .cjkClosing

        case 0x2E80 ... 0x9FFF,
             0xAC00 ... 0xD7AF,
             0xF900 ... 0xFAFF,
             0xFE30 ... 0xFE6F,
             0xFF00 ... 0xFFEF,
             0x20000 ... 0x2FA1F,
             0x30000 ... 0x323AF:
            return .cjkIdeograph

        default:
            return .nonBreakable
        }
    }

    private static func startsWithGlueScalar(_ ch: Character) -> Bool {
        guard let scalar = ch.unicodeScalars.first else { return false }
        return scalar.value == 0x202F || scalar.value == 0x00A0 || scalar.value == 0x2060 || scalar.value == 0xFEFF
    }

    private static func maxUnbreakableTokenColumnWidth(
        in attributed: NSAttributedString,
        cap: CGFloat = 168
    ) -> CGFloat {
        let str = attributed.string
        guard !str.isEmpty else { return 0 }
        var maxW: CGFloat = 0
        var tokenStart: String.Index?
        var tokenCharCount = 0

        func flushToken(upTo endIdx: String.Index) {
            guard let startIdx = tokenStart else { return }
            tokenStart = nil
            tokenCharCount = 0
            guard startIdx < endIdx else { return }
            let slice = str[startIdx ..< endIdx]
            guard slice.unicodeScalars.contains(where: { $0.value != 0x202F && $0.value != 0x00A0 }) else {
                return
            }
            let nsRange = NSRange(startIdx ..< endIdx, in: str)
            guard nsRange.length > 0 else { return }
            let subAttr = attributed.attributedSubstring(from: nsRange)
            let w = ceil(subAttr.size().width) + 28
            if w > maxW {
                maxW = min(cap, w)
            }
        }

        var idx = str.startIndex
        while idx < str.endIndex {
            let ch = str[idx]
            let nextIdx = str.index(after: idx)
            let role = ch.unicodeScalars.first.map(classifyTableScalar) ?? .nonBreakable

            switch role {
            case .whitespace:
                flushToken(upTo: idx)
                if maxW >= cap { return cap }

            case .cjkIdeograph:
                let nextGluesToThis = nextIdx < str.endIndex && (
                    startsWithGlueScalar(str[nextIdx])
                    || (str[nextIdx].unicodeScalars.first.map(classifyTableScalar) == .cjkClosing)
                )
                if tokenStart != nil {
                    let prevIdx = str.index(before: idx)
                    let prevRole = str[prevIdx].unicodeScalars.first.map(classifyTableScalar) ?? .nonBreakable
                    if prevRole != .cjkOpening && !startsWithGlueScalar(str[prevIdx]) {
                        flushToken(upTo: idx)
                        if maxW >= cap { return cap }
                    }
                }
                if tokenStart != nil || nextGluesToThis {
                    if tokenStart == nil {
                        tokenStart = idx
                        tokenCharCount = 0
                    }
                    tokenCharCount += 1
                    if !nextGluesToThis || tokenCharCount >= 64 {
                        flushToken(upTo: nextIdx)
                        if maxW >= cap { return cap }
                    }
                }

            case .cjkOpening:
                if tokenStart != nil {
                    let prevIdx = str.index(before: idx)
                    let prevRole = str[prevIdx].unicodeScalars.first.map(classifyTableScalar) ?? .nonBreakable
                    if prevRole != .cjkOpening && !startsWithGlueScalar(str[prevIdx]) {
                        flushToken(upTo: idx)
                        if maxW >= cap { return cap }
                    }
                }
                if tokenStart == nil {
                    tokenStart = idx
                    tokenCharCount = 0
                }
                tokenCharCount += 1
                if tokenCharCount >= 64 {
                    flushToken(upTo: nextIdx)
                    if maxW >= cap { return cap }
                }

            case .cjkClosing:
                if tokenStart == nil {
                    tokenStart = idx
                    tokenCharCount = 0
                }
                tokenCharCount += 1
                let nextContinuesCloseOrGlue = nextIdx < str.endIndex && (
                    startsWithGlueScalar(str[nextIdx])
                    || (str[nextIdx].unicodeScalars.first.map(classifyTableScalar) == .cjkClosing)
                )
                if !nextContinuesCloseOrGlue || tokenCharCount >= 64 {
                    flushToken(upTo: nextIdx)
                    if maxW >= cap { return cap }
                }

            case .nonBreakable:
                if tokenStart == nil {
                    tokenStart = idx
                    tokenCharCount = 0
                }
                tokenCharCount += 1
                if tokenCharCount >= 64 {
                    flushToken(upTo: nextIdx)
                    if maxW >= cap { return cap }
                    var skipIdx = nextIdx
                    while skipIdx < str.endIndex {
                        let sr = str[skipIdx].unicodeScalars.first.map(classifyTableScalar) ?? .nonBreakable
                        if sr != .nonBreakable && sr != .cjkClosing { break }
                        skipIdx = str.index(after: skipIdx)
                    }
                    idx = skipIdx
                    continue
                }
            }
            idx = nextIdx
        }
        flushToken(upTo: str.endIndex)
        return maxW
    }

    static func measureTableLayout(
        headers: [NSAttributedString],
        rows: [[NSAttributedString]],
        maxWidth: CGFloat
    ) -> (colWidths: [CGFloat], rowHeights: [CGFloat], totalWidth: CGFloat, totalHeight: CGFloat) {
        let colCount = max(1, max(headers.count, rows.map(\.count).max() ?? 0))
        let allRows = [headers] + rows
        let usableMaxW = max(160, floor(maxWidth))

        var naturalWidths = Array(repeating: CGFloat(64), count: colCount)
        var headerNaturalWidths = Array(repeating: CGFloat(64), count: colCount)
        var tokenFloorWidths = Array(repeating: CGFloat(0), count: colCount)
        for (rIdx, cols) in allRows.enumerated() {
            for cIdx in 0 ..< min(colCount, cols.count) {
                let cellAttr = cols[cIdx]
                let natural = ceil(cellAttr.size().width) + 28
                if rIdx == 0 {
                    headerNaturalWidths[cIdx] = max(64, natural)
                }
                naturalWidths[cIdx] = max(naturalWidths[cIdx], min(360, natural))
                if tokenFloorWidths[cIdx] < 168 {
                    let tokenW = maxUnbreakableTokenColumnWidth(in: cellAttr, cap: 168)
                    if tokenW > tokenFloorWidths[cIdx] {
                        tokenFloorWidths[cIdx] = tokenW
                    }
                }
            }
        }

        let colFloors: [CGFloat] = (0 ..< colCount).map { cIdx in
            let headerFloor = max(84, min(132, headerNaturalWidths[cIdx]))
            return min(naturalWidths[cIdx], max(headerFloor, tokenFloorWidths[cIdx]))
        }
        let naturalTotal = naturalWidths.reduce(0, +)
        let floorTotal = colFloors.reduce(0, +)

        var colWidths: [CGFloat]
        if naturalTotal <= usableMaxW {
            colWidths = naturalWidths
        } else if floorTotal <= usableMaxW {
            let targetShrink = naturalTotal - usableMaxW
            let shrinkable = zip(naturalWidths, colFloors).map { max(0, $0 - $1) }
            let totalShrinkable = shrinkable.reduce(0, +)
            if totalShrinkable > 0.5 {
                var allocated: CGFloat = 0
                colWidths = (0 ..< colCount).map { cIdx in
                    let cut = floor(targetShrink * (shrinkable[cIdx] / totalShrinkable))
                    let w = max(colFloors[cIdx], naturalWidths[cIdx] - cut)
                    allocated += w
                    return w
                }
                var excess = ceil(allocated - usableMaxW)
                if excess > 0 {
                    for cIdx in (0 ..< colCount).sorted(by: { (colWidths[$0] - colFloors[$0]) > (colWidths[$1] - colFloors[$1]) }) {
                        guard excess > 0 else { break }
                        let avail = floor(colWidths[cIdx] - colFloors[cIdx])
                        if avail > 0 {
                            let delta = min(excess, avail)
                            colWidths[cIdx] -= delta
                            excess -= delta
                        }
                    }
                }
            } else {
                colWidths = colFloors
            }
        } else {
            colWidths = (0 ..< colCount).map { cIdx in
                max(colFloors[cIdx], min(260, naturalWidths[cIdx]))
            }
        }

        var rowHeights: [CGFloat] = []
        rowHeights.reserveCapacity(allRows.count)
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
        let optColW = max(80, innerW - 32)
        let rightColW = max(80, innerW - 150)
        var total: CGFloat = 28 + 18 // padding 14*2 + metaRow
        let qFont = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
        let descFont = DesignFonts.nsBodyFont(size: 12)
        let ansFont = DesignFonts.nsBodyFont(size: 13, weight: .semibold)
        for q in questions {
            let qAttr = NSAttributedString(string: q.question, attributes: [.font: qFont])
            let qH = max(20, ReaderSelectableTextView.measureHeight(attributed: qAttr, width: innerW))
            total += 10 + qH + 10 // innerStack spacing before qLbl + qH + spacing before optsStack
            for opt in q.options {
                let optTitleAttr = NSAttributedString(string: opt.label, attributes: [.font: qFont])
                var optH: CGFloat = 20 + max(18, ReaderSelectableTextView.measureHeight(attributed: optTitleAttr, width: optColW))
                if let desc = opt.description, !desc.isEmpty {
                    let descAttr = NSAttributedString(string: desc, attributes: [.font: descFont])
                    optH += 4 + max(16, ReaderSelectableTextView.measureHeight(attributed: descAttr, width: optColW))
                }
                total += optH
            }
        }
        if let answers {
            total += 10 // innerStack spacing before answers block
            if !answers.isEmpty {
                for (idx, ans) in answers.enumerated() {
                    let ansValText = ans.answers.isEmpty ? "未回答" : ans.answers.joined(separator: ", ")
                    let ansAttr = NSAttributedString(string: ansValText, attributes: [.font: ansFont])
                    let ansTextH = max(20, ReaderSelectableTextView.measureHeight(attributed: ansAttr, width: rightColW))
                    total += 20 + ansTextH + (idx > 0 ? 8 : 0)
                }
            } else {
                total += 20
            }
        }
        return max(84, total)
    }

    static func measureMonospaceBoxHeight(text: String, width: CGFloat) -> CGFloat {
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
        let boxWidth = max(106, width)
        let innerW = boxWidth - 26
        let measured = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: innerW))
        return min(240, max(48, measured + 22))
    }

    private func measureMarkdownSegmentsHeight(
        ownerId: String,
        markdown: String,
        foldInstructions: Bool,
        width: CGFloat,
        interItemSpacing: CGFloat = 8,
        allowInstructionsFold: Bool = true,
        alreadyCleaned: Bool = false
    ) -> CGFloat {
        let segs = GfmLayoutCache.shared.getOrRender(
            markdown: markdown,
            foldInstructions: foldInstructions,
            scheme: scheme,
            workspace: workspace,
            transport: store.transport,
            alreadyCleaned: alreadyCleaned
        )
        var total: CGFloat = 0
        var renderedCount = 0
        for seg in segs {
            let itemHeight: CGFloat
            switch seg {
            case let .prose(attr):
                itemHeight = cachedProseHeight(attributed: attr, width: width)
            case let .codeBlock(_, code):
                // Full vertical height of fenced code blocks — NEVER clip at 18 lines!
                // Symmetric 12pt top/bottom/leading/trailing padding (+24pt vertical chrome, saving 30pt vs legacy 38+16pt).
                let innerCodeW = max(32, width - 24)
                let codeLayout = cachedCodeLayout(code: code, innerWidth: innerCodeW)
                let scrollerExtra = Self.horizontalScrollerExtraHeight(needsHorizontalScroll: codeLayout.docWidth > innerCodeW + 0.5)
                itemHeight = codeLayout.codeHeight + scrollerExtra + 24
            case let .table(_, headers, rows):
                let layout = cachedTableLayout(headers: headers, rows: rows, maxWidth: width)
                let visibleW = min(width, layout.totalWidth)
                let scrollerExtra = Self.horizontalScrollerExtraHeight(needsHorizontalScroll: layout.totalWidth > visibleW + 0.5)
                itemHeight = layout.totalHeight + scrollerExtra
            case .thematicBreak:
                itemHeight = 1
            case let .instructionsFold(id, text, _, _):
                guard allowInstructionsFold else { continue }
                let scopedKey = "\(ownerId):\(id)"
                let open = disclosureStore.isOpen(scope: sessionScopeKey, stateKey: scopedKey, initiallyOpen: false)
                if open {
                    itemHeight = 54 + measureMarkdownSegmentsHeight(
                        ownerId: scopedKey,
                        markdown: text,
                        foldInstructions: false,
                        width: max(120, width - 24),
                        interItemSpacing: 8,
                        alreadyCleaned: true
                    )
                } else {
                    itemHeight = 38
                }
            }
            if renderedCount > 0 {
                total += interItemSpacing
            }
            total += itemHeight
            renderedCount += 1
        }
        return total
    }
}

// MARK: - Reusable NSTableCellView Rendering Prose NSTextViews, Code Blocks, Tables & Disclosures
