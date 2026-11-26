import AppKit
import Combine
import SwiftUI

@MainActor
final class ProviderWorkspaceStore: ObservableObject {
    let store: RuntimeConnectionStore
    let workspace: WorkspaceId
    let tableCoordinator: ConversationTableCoordinator
    weak var coordinatorRef: ConversationTableCoordinator?
    private var observations = Set<AnyCancellable>()
    private var listFetchSeq: UInt64 = 0
    private var sidebarCacheRevision: UInt64 = 0

    private struct SidebarDerivedCache {
        var revision: UInt64 = .max
        var filteredActiveItems: [UnifiedSessionSummaryItem] = []
        var filteredArchivedItems: [UnifiedSessionSummaryItem] = []
        var activeWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] = []
        var archivedWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] = []
        var availableWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] = []
        var visualOrderedActiveKeys: [String] = []
        var visualOrderedArchivedKeys: [String] = []
    }
    private var sidebarCache = SidebarDerivedCache()

    let sourceRevision: UInt64
    let threadStore = ThreadMessageStore()
    let claudeHistoryStore = PagedEventHistoryStore<ClaudeHistoryEvent> { ev, _ in ev.id }
    let grokHistoryStore = PagedEventHistoryStore<GrokHistoryEvent> { ev, idx in
        ev.id ?? ev.userMessage?.id ?? "\(ev.kind):\(ev.callId ?? ""):\(ev.timestamp ?? ""):\(idx)"
    }
    let providerHistoryStore = PagedEventHistoryStore<ProviderHistoryEvent>.forProviderEvents()
    let timelineWindow = TimelineWindowController<ConversationRowItem>(idOf: \.timelineId)
    let motionObserver = AccessibilityMotionObserver()

    @Published var query: String = ""
    @Published var groupingMode: ProjectWorkspaceGroupingMode = .flat {
        didSet {
            if oldValue != groupingMode {
                sidebarCacheRevision &+= 1
                syncPartitionedSelection()
            }
        }
    }
    @Published var workspaceFilter: ProjectWorkspaceFilter = .all {
        didSet {
            if oldValue != workspaceFilter {
                sidebarCacheRevision &+= 1
                syncPartitionedSelection()
            }
        }
    }
    @Published var collapsedWorkspaceKeys: Set<ProjectWorkspaceKey> = [] {
        didSet {
            if oldValue != collapsedWorkspaceKeys {
                sidebarCacheRevision &+= 1
                syncPartitionedSelection()
            }
        }
    }
    @Published var collapsedArchivedWorkspaceKeys: Set<ProjectWorkspaceKey> = [] {
        didSet {
            if oldValue != collapsedArchivedWorkspaceKeys {
                sidebarCacheRevision &+= 1
                syncPartitionedSelection()
            }
        }
    }
    @Published var activeItems: [UnifiedSessionSummaryItem] = [] {
        didSet { sidebarCacheRevision &+= 1 }
    }
    @Published var archivedItems: [UnifiedSessionSummaryItem] = [] {
        didSet { sidebarCacheRevision &+= 1 }
    }
    @Published var archivedOpen: Bool = false {
        didSet { sidebarCacheRevision &+= 1 }
    }
    @Published var selectedKey: String?
    @Published var externalSeedItem: UnifiedSessionSummaryItem?
    var skipSidebarListFetch: Bool = false
    var onExternalMutation: (() -> Void)?
    var onExternalSelectSession: ((String?) -> Void)?

    @Published var rows: [ConversationRowItem] = []
    @Published var currentDetailHistory: HistoryMetadata?
    @Published var detailLoading: Bool = false
    @Published var detailLoadedForKey: String?
    @Published var timelineEntries: [TimelineEntry] = []
    @Published var subagents: SubagentCollection?
    @Published var beforeCursor: String?
    @Published var hasMoreOlder: Bool = false
    @Published var loadingOlder: Bool = false
    @Published var loadOlderError: String?
    @Published var showAllHistory: Bool = false
    @Published var revealedPositionKey: String?
    @Published var pendingSearch: SessionSearchResult?
    @Published var searchSelection: UnifiedSessionSummaryItem?
    @Published var latestReplyText: String?
    private(set) var latestNativeTailId: String?
    private(set) var detailSelectionGeneration: UInt64 = 0
    private var lastRenderedPollSig: LastRenderedPollSignature?

    private struct LastRenderedPollSignature: Equatable {
        let scopeKey: String
        let epoch: String
        let revision: String
        let running: Bool
        let title: String
        let showAllHistory: Bool
        let revealedPositionKey: String?
        let subagentsSupported: Bool
        let subagents: SubagentCollection?
        let subagentUpdates: [String: SubagentActivity]?
    }
    @Published var renameRequestSeqByKey: [String: Int] = [:]

    @Published var partitionedSelection = PartitionedSessionSelectionState(activeScope: "init")
    @Published var batchTargetPartition: SessionSelectionPartition = .active
    @Published var batchBusy: Bool = false
    @Published var batchPrepareError: String?
    @Published var batchExecuteError: String?
    @Published var batchPreview: SessionBatchPreview?
    @Published var batchResult: SessionBatchResult?

    @Published var headerInlineRename: Bool = false
    @Published var claudeOwnershipKey: String?
    @Published var claudeOwnershipData: ClaudeOwnershipDiagnostics?
    @Published var claudeOwnershipError: String?
    @Published var claudeOwnershipLoading: Bool = false
    @Published var feedbackMessage: String?
    @Published var errorMessage: String?

    init(store: RuntimeConnectionStore, workspace: WorkspaceId) {
        let coord = ConversationTableCoordinator(store: store)
        self.store = store
        self.workspace = workspace
        self.tableCoordinator = coord
        self.coordinatorRef = coord
        self.sourceRevision = store.revision
        timelineWindow.objectWillChange
            .sink { [weak self] in self?.objectWillChange.send() }
            .store(in: &observations)

        coord.onLoadOlderRequested = { [weak self] in
            guard let self,
                  self.loadOlderError == nil,
                  !self.timelineWindow.isActive || self.timelineWindow.error == nil
            else { return }
            Task { await self.loadOlderPageIfNeeded() }
        }
        coord.onShowAllHistory = { [weak self] in
            guard let self else { return }
            self.showAllHistory = true
            if let item = self.selectedItem, self.provider == .codex {
                self.threadStore.setHistoryExpanded(threadId: item.sessionKey, expanded: true)
            }
        }
        coord.onCopyFeedback = { [weak self] text in
            self?.feedbackMessage = text
        }
    }

    private func refreshedSidebarCache() -> SidebarDerivedCache {
        if sidebarCache.revision == sidebarCacheRevision {
            return sidebarCache
        }
        let supportsGrp = supportsWorkspaceGrouping
        let filtActive = ProjectWorkspaceEngine.filterItems(activeItems, by: workspaceFilter, enabled: supportsGrp)
        let filtArch = ProjectWorkspaceEngine.filterItems(archivedItems, by: workspaceFilter, enabled: supportsGrp)
        let activeGrps = ProjectWorkspaceEngine.buildGroups(items: filtActive)
        let archGrps = ProjectWorkspaceEngine.buildGroups(items: filtArch)
        let allScopeItems: [UnifiedSessionSummaryItem] = archivedOpen ? (activeItems + archivedItems) : activeItems
        let availGrps = ProjectWorkspaceEngine.buildGroups(items: allScopeItems)
        let orderedActive: [String] = if supportsGrp && groupingMode == .byWorkspace {
            ProjectWorkspaceEngine.visualOrderedKeys(groups: activeGrps, collapsedKeys: collapsedWorkspaceKeys)
        } else {
            filtActive.map(\.sessionKey)
        }
        let orderedArch: [String] = if supportsGrp && groupingMode == .byWorkspace {
            ProjectWorkspaceEngine.visualOrderedKeys(groups: archGrps, collapsedKeys: collapsedArchivedWorkspaceKeys)
        } else {
            filtArch.map(\.sessionKey)
        }
        let built = SidebarDerivedCache(
            revision: sidebarCacheRevision,
            filteredActiveItems: filtActive,
            filteredArchivedItems: filtArch,
            activeWorkspaceGroups: activeGrps,
            archivedWorkspaceGroups: archGrps,
            availableWorkspaceGroups: availGrps,
            visualOrderedActiveKeys: orderedActive,
            visualOrderedArchivedKeys: orderedArch
        )
        sidebarCache = built
        return built
    }

    var supportsWorkspaceGrouping: Bool {
        provider != .grokbot
    }

    var provider: SessionProvider {
        SessionProvider.from(workspace: workspace) ?? .codex
    }

    var supportsArchive: Bool {
        provider == .codex || provider == .antigravity || provider == .dsh
    }

    var activeSelectionPartition: SessionSelectionPartition {
        if supportsArchive && archivedOpen && (partitionedSelection.archived.selecting || !partitionedSelection.archived.keys.isEmpty) {
            return .archived
        }
        return .active
    }

    var currentSelectionState: SessionSelectionState {
        partitionedSelection.state(for: activeSelectionPartition)
    }

    func batchOperations(for partition: SessionSelectionPartition) -> [SessionOperation] {
        if provider == .grokbot { return [] }
        if supportsArchive {
            return partition == .archived ? [.restore, .delete] : [.archive, .delete]
        }
        return [.delete]
    }

    var batchOperations: [SessionOperation] {
        batchOperations(for: activeSelectionPartition)
    }

    var filteredActiveItems: [UnifiedSessionSummaryItem] {
        refreshedSidebarCache().filteredActiveItems
    }

    var filteredArchivedItems: [UnifiedSessionSummaryItem] {
        refreshedSidebarCache().filteredArchivedItems
    }

    var activeWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] {
        refreshedSidebarCache().activeWorkspaceGroups
    }

    var archivedWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] {
        refreshedSidebarCache().archivedWorkspaceGroups
    }

    var availableWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] {
        refreshedSidebarCache().availableWorkspaceGroups
    }

    var visualOrderedActiveKeys: [String] {
        refreshedSidebarCache().visualOrderedActiveKeys
    }

    var visualOrderedArchivedKeys: [String] {
        refreshedSidebarCache().visualOrderedArchivedKeys
    }

    var allVisibleItems: [UnifiedSessionSummaryItem] {
        let cached = refreshedSidebarCache()
        return archivedOpen ? (cached.filteredActiveItems + cached.filteredArchivedItems) : cached.filteredActiveItems
    }

    var isRemoteProviderUnsupported: Bool {
        guard store.target == .remote, store.capabilities.isReady else { return false }
        switch provider {
        case .antigravity: return !store.capabilities.antigravitySessions
        case .dsh: return !store.capabilities.dshSessions
        default: return false
        }
    }

    var selectedItem: UnifiedSessionSummaryItem? {
        if let key = selectedKey {
            if let match = activeItems.first(where: { $0.sessionKey == key })
                ?? archivedItems.first(where: { $0.sessionKey == key })
            {
                return match
            }
            if let searchSelection, searchSelection.sessionKey == key { return searchSelection }
            if let externalSeedItem, externalSeedItem.sessionKey == key { return externalSeedItem }
        }
        let filtActive = filteredActiveItems
        if supportsWorkspaceGrouping && workspaceFilter != .all {
            return filtActive.first
        }
        if !query.isEmpty {
            return filtActive.first
        }
        return filtActive.first ?? activeItems.first
    }

    var displayedRows: [ConversationRowItem] {
        timelineWindow.items ?? rows
    }

    func syncPartitionedSelection() {
        let baseScope = "\(store.revision):\(workspace.rawValue):\(query):\(workspaceFilter.scopeToken)"
        let cached = refreshedSidebarCache()
        var next = partitionedSelection
        next.sync(
            activeScope: "\(baseScope)#active",
            activeVisible: cached.visualOrderedActiveKeys,
            archivedScope: "\(baseScope)#archived",
            archivedVisible: cached.visualOrderedArchivedKeys,
            archivedOpen: supportsArchive && archivedOpen
        )
        if next != partitionedSelection {
            partitionedSelection = next
        }
    }

    func selectAllVisible() {
        _ = partitionedSelection.selectAll(
            activeVisible: visualOrderedActiveKeys,
            archivedVisible: visualOrderedArchivedKeys,
            supportsArchive: supportsArchive,
            archivedOpen: archivedOpen
        )
        batchPreview = nil
        batchResult = nil
        batchPrepareError = nil
        batchExecuteError = nil
    }

    func selectAllInWorkspaceGroup(keys: [String], partition: SessionSelectionPartition) {
        guard !keys.isEmpty else { return }
        var state = partitionedSelection.state(for: partition)
        state = SessionSelectionEngine.add(state, keys: keys)
        if state.anchor == nil { state.anchor = keys.first }
        partitionedSelection.setState(state, for: partition)
        batchPreview = nil
        batchResult = nil
        batchPrepareError = nil
        batchExecuteError = nil
    }

    func fetchSessionList() async {
        listFetchSeq &+= 1
        let fetchSeq = listFetchSeq
        let requestedQuery = query
        let requestedArchived = archivedOpen
        func isCurrent() -> Bool {
            !Task.isCancelled
                && listFetchSeq == fetchSeq
                && store.revision == sourceRevision
                && query == requestedQuery
                && archivedOpen == requestedArchived
        }
        guard isCurrent() else { return }
        do {
            switch provider {
            case .codex, .claudeCode, .grok, .antigravity, .dsh:
                if isRemoteProviderUnsupported {
                    if !activeItems.isEmpty { activeItems = [] }
                    if !archivedItems.isEmpty { archivedItems = [] }
                    syncPartitionedSelection()
                    if errorMessage != nil { errorMessage = nil }
                    return
                }
                let active = try await Self.fetchProviderSummaries(
                    store: store,
                    provider: provider,
                    query: query,
                    archived: false,
                    limit: 500,
                    threadStore: threadStore
                )
                guard isCurrent() else { return }
                if activeItems != active { activeItems = active }
                if supportsArchive && archivedOpen {
                    let arch = try await Self.fetchProviderSummaries(
                        store: store,
                        provider: provider,
                        query: query,
                        archived: true,
                        limit: 500,
                        threadStore: threadStore
                    )
                    guard isCurrent() else { return }
                    if archivedItems != arch { archivedItems = arch }
                }

            case .grokbot:
                guard store.target == .local else {
                    if !activeItems.isEmpty { activeItems = [] }
                    return
                }
                struct GBWrap: Encodable {
                    struct Inner: Encodable { let q: String?; let limit: Int }
                    let request: Inner
                }
                let list: [ProviderSessionSummary] = try await store.rpc(
                    "grokbot.list",
                    args: GBWrap(request: .init(q: query.isEmpty ? nil : query, limit: 100)),
                    localOnly: true
                )
                guard isCurrent() else { return }
                let mapped = list.map(Self.mapGrokBotSummary)
                if activeItems != mapped { activeItems = mapped }
            }
            syncPartitionedSelection()
            if errorMessage != nil { errorMessage = nil }
        } catch {
            guard isCurrent() else { return }
            let nextErr = error.localizedDescription
            if errorMessage != nextErr { errorMessage = nextErr }
        }
    }

    static func isCodingProviderSupported(provider: SessionProvider, store: RuntimeConnectionStore) -> Bool {
        if provider == .grokbot { return false }
        if store.target == .remote, store.capabilities.isReady {
            switch provider {
            case .antigravity: return store.capabilities.antigravitySessions
            case .dsh: return store.capabilities.dshSessions
            default: return true
            }
        }
        return true
    }

    static func fetchProviderSummaries(
        store: RuntimeConnectionStore,
        provider: SessionProvider,
        query: String,
        archived: Bool,
        limit: Int = 500,
        threadStore: ThreadMessageStore? = nil
    ) async throws -> [UnifiedSessionSummaryItem] {
        guard isCodingProviderSupported(provider: provider, store: store) else { return [] }
        let qArg = query.isEmpty ? nil : query
        switch provider {
        case .codex:
            struct ListArgs: Encodable {
                let archived: Bool
                let q: String?
                let limit: Int
            }
            let rows: [ThreadSummary] = try await store.rpc(
                "threads.list",
                args: ListArgs(archived: archived, q: qArg, limit: limit)
            )
            return rows.map {
                mapCodexSummaryStatic($0, archived: archived, store: store, threadStore: threadStore)
            }
        case .claudeCode:
            if archived { return [] }
            struct ClaudeListArgs: Encodable { let q: String?; let limit: Int }
            let rows: [ClaudeSessionSummary] = try await store.rpc(
                "claude.list",
                args: ClaudeListArgs(q: qArg, limit: limit)
            )
            return rows.map(mapClaudeSummaryStatic)
        case .grok:
            if archived { return [] }
            struct GrokListArgs: Encodable { let q: String?; let limit: Int }
            let rows: [GrokSessionSummary] = try await store.rpc(
                "grok.list",
                args: GrokListArgs(q: qArg, limit: limit)
            )
            return rows.map(mapGrokSummaryStatic)
        case .antigravity, .dsh:
            let prefix = (provider == .antigravity) ? "antigravity" : "dsh"
            struct ProvListWrap: Encodable {
                struct Inner: Encodable { let q: String?; let archived: Bool; let limit: Int }
                let request: Inner
            }
            let rows: [ProviderSessionSummary] = try await store.rpc(
                "\(prefix).list",
                args: ProvListWrap(request: .init(q: qArg, archived: archived, limit: limit))
            )
            return rows.map(mapProviderSummaryStatic)
        case .grokbot:
            return []
        }
    }

    static func sendBulkPreview(
        store: RuntimeConnectionStore,
        request: SessionBatchRequest,
        at revision: UInt64
    ) async throws -> SessionBatchPreview {
        struct Wrap: Encodable { let request: SessionBatchRequest }
        return try await store.rpc("sessions.bulkPreview", args: Wrap(request: request), at: revision)
    }

    static func sendBulkExecute(
        store: RuntimeConnectionStore,
        request: SessionBatchExecuteRequest,
        at revision: UInt64
    ) async throws -> SessionBatchResult {
        struct Wrap: Encodable { let request: SessionBatchExecuteRequest }
        return try await store.rpc("sessions.bulkExecute", args: Wrap(request: request), at: revision)
    }

    func resetDetailStateForSelectionChange() {
        detailSelectionGeneration &+= 1
        errorMessage = nil
        feedbackMessage = nil
        currentDetailHistory = nil
        headerInlineRename = false
        claudeOwnershipKey = nil
        claudeOwnershipData = nil
        claudeOwnershipError = nil
        claudeOwnershipLoading = false
        detailLoading = false
        loadingOlder = false
        hasMoreOlder = false
        beforeCursor = nil
        showAllHistory = false
        detailLoadedForKey = nil
        latestReplyText = nil
        latestNativeTailId = nil
        lastRenderedPollSig = nil
        rows = []
        timelineEntries = []
        subagents = nil
        revealedPositionKey = nil
        loadOlderError = nil
        timelineWindow.returnToLatest()
    }

    func isCurrentDetail(_ item: UnifiedSessionSummaryItem, generation: UInt64? = nil) -> Bool {
        !Task.isCancelled
            && store.revision == sourceRevision
            && selectedItem?.sessionKey == item.sessionKey
            && (generation == nil || detailSelectionGeneration == generation)
    }

    var subagentsSupportedForCurrentProvider: Bool {
        guard store.capabilities.isReady else { return false }
        return switch provider {
        case .codex: store.capabilities.threadSubagents
        case .claudeCode: store.capabilities.claudeSubagents
        case .antigravity: store.capabilities.antigravitySubagents
        default: false
        }
    }

    private func syncSidebarItem(_ updated: UnifiedSessionSummaryItem) {
        if let idx = activeItems.firstIndex(where: { $0.sessionKey == updated.sessionKey }) {
            var merged = updated
            if merged.storageSize == nil {
                merged.storageSize = activeItems[idx].storageSize
            }
            if activeItems[idx] != merged {
                activeItems[idx] = merged
            }
        }
        if let idx = archivedItems.firstIndex(where: { $0.sessionKey == updated.sessionKey }) {
            var merged = updated
            if merged.storageSize == nil {
                merged.storageSize = archivedItems[idx].storageSize
            }
            if archivedItems[idx] != merged {
                archivedItems[idx] = merged
            }
        }
        if searchSelection?.sessionKey == updated.sessionKey, searchSelection != updated {
            searchSelection = updated
        }
        if let seed = externalSeedItem, seed.sessionKey == updated.sessionKey {
            var merged = updated
            if merged.storageSize == nil {
                merged.storageSize = seed.storageSize
            }
            if externalSeedItem != merged {
                externalSeedItem = merged
            }
        }
    }

    func fetchSessionDetail(for item: UnifiedSessionSummaryItem) async {
        let generation = detailSelectionGeneration
        guard isCurrentDetail(item, generation: generation) else { return }
        let historyScope = "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)"
        let isFirstLoadForKey = (detailLoadedForKey != item.sessionKey)
        if isFirstLoadForKey {
            detailLoading = true
            latestNativeTailId = nil
            lastRenderedPollSig = nil
        }
        defer { if isCurrentDetail(item, generation: generation) { detailLoading = false } }
        do {
            var incomingHistory: HistoryMetadata?
            switch provider {
            case .codex:
                struct DetailArgs: Encodable {
                    struct Opts: Encodable { let limit: Int }
                    let id: String
                    let options: Opts
                }
                guard let detail: ThreadDetail = try await store.rpc(
                    "threads.detail",
                    args: DetailArgs(id: item.sessionKey, options: .init(limit: 120))
                ) else { return }
                guard isCurrentDetail(item, generation: generation) else { return }
                let mappedCodex = mapCodexSummary(detail.summary, archived: detail.summary.status == "Archived")
                syncSidebarItem(mappedCodex)
                incomingHistory = detail.history
                let effectiveTitle = threadStore.effectiveTitle(threadId: detail.summary.id, serverTitle: detail.summary.title)
                let candidateSig = makePollSignature(
                    scopeKey: historyScope,
                    history: detail.history,
                    running: mappedCodex.running,
                    title: effectiveTitle,
                    showAllHistory: showAllHistory || threadStore.slot(for: item.sessionKey).showAllHistory,
                    revealedPositionKey: revealedPositionKey,
                    subagentsSupported: subagentsSupportedForCurrentProvider,
                    subagents: detail.subagents,
                    subagentUpdates: detail.subagentUpdates
                )
                if shouldSkipUnchangedDetailPoll(isFirstLoadForKey: isFirstLoadForKey, candidateSig: candidateSig) {
                    break
                }
                let slot = threadStore.applyDetail(detail)
                let nextRows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: effectiveTitle,
                    subagentsSupported: subagentsSupportedForCurrentProvider,
                    revealedBlockId: revealedPositionKey
                )
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .codex,
                    sources: slot.blocks.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                applyRenderedDetailSnapshot(
                    tailId: slot.blocks.last?.id,
                    nextSubagents: subagentsSupportedForCurrentProvider ? slot.subagents : nil,
                    hasMore: slot.hasMoreBlocks,
                    before: slot.beforeCursor,
                    nextRows: nextRows,
                    nextTimeline: nextTimeline,
                    signature: candidateSig
                )

            case .claudeCode:
                struct Req: Encodable { let sessionKey: String; let limit: Int }
                let detail: ClaudeSessionDetail = try await store.rpc(
                    "claude.detail",
                    args: Req(sessionKey: item.sessionKey, limit: 160)
                )
                guard isCurrentDetail(item, generation: generation) else { return }
                let mappedClaude = mapClaudeSummary(detail.summary)
                syncSidebarItem(mappedClaude)
                incomingHistory = detail.history
                let candidateSig = makePollSignature(
                    scopeKey: historyScope,
                    history: detail.history,
                    running: mappedClaude.running,
                    title: mappedClaude.title,
                    subagentsSupported: subagentsSupportedForCurrentProvider,
                    subagents: detail.subagents
                )
                if shouldSkipUnchangedDetailPoll(isFirstLoadForKey: isFirstLoadForKey, candidateSig: candidateSig) {
                    break
                }
                let slot = claudeHistoryStore.applyHeadPoll(
                    key: historyScope,
                    headEvents: detail.events,
                    history: detail.history,
                    totalEvents: detail.totalEvents,
                    beforeCursor: detail.beforeCursor,
                    hasMore: detail.hasMore,
                    timeline: detail.timeline,
                    subagents: detail.subagents
                )
                let nextRows = ConversationRowBuilder.buildClaudeRows(events: slot.events)
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .claudeCode,
                    sources: slot.events.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                applyRenderedDetailSnapshot(
                    tailId: slot.events.last?.id,
                    nextSubagents: subagentsSupportedForCurrentProvider ? slot.subagents : nil,
                    hasMore: slot.hasMore,
                    before: slot.beforeCursor,
                    nextRows: nextRows,
                    nextTimeline: nextTimeline,
                    signature: candidateSig
                )

            case .grok:
                struct Req: Encodable { let id: String; let limit: Int }
                let detail: GrokSessionDetail = try await store.rpc(
                    "grok.detail",
                    args: Req(id: item.sessionKey, limit: 160)
                )
                guard isCurrentDetail(item, generation: generation) else { return }
                let mappedGrok = mapGrokSummary(detail.summary)
                syncSidebarItem(mappedGrok)
                incomingHistory = detail.history
                let candidateSig = makePollSignature(
                    scopeKey: historyScope,
                    history: detail.history,
                    running: mappedGrok.running,
                    title: mappedGrok.title,
                    subagentsSupported: false
                )
                if shouldSkipUnchangedDetailPoll(isFirstLoadForKey: isFirstLoadForKey, candidateSig: candidateSig) {
                    break
                }
                let slot = grokHistoryStore.applyHeadPoll(
                    key: historyScope,
                    headEvents: detail.events,
                    history: detail.history,
                    totalEvents: detail.totalEvents,
                    beforeCursor: detail.beforeCursor,
                    hasMore: detail.hasMore ?? false,
                    timeline: detail.timeline
                )
                let nextRows = ConversationRowBuilder.buildGrokRows(
                    events: slot.events,
                    fallbackTitle: mappedGrok.title,
                    sessionRunning: mappedGrok.running
                )
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .grok,
                    sources: slot.events.enumerated().map { idx, ev in
                        TimelineSourceItem(id: ev.id ?? ev.userMessage?.id ?? "grok-\(idx)", role: nil, kind: ev.kind, text: ev.text, userMessage: ev.userMessage)
                    }
                )
                applyRenderedDetailSnapshot(
                    tailId: slot.events.last.flatMap { $0.id ?? $0.userMessage?.id },
                    nextSubagents: nil,
                    hasMore: slot.hasMore,
                    before: slot.beforeCursor,
                    nextRows: nextRows,
                    nextTimeline: nextTimeline,
                    signature: candidateSig
                )

            case .antigravity, .dsh, .grokbot:
                let cmd = provider == .antigravity ? "antigravity.detail" : (provider == .dsh ? "dsh.detail" : "grokbot.detail")
                struct Wrap: Encodable { let request: ProviderDetailRequest }
                let detail: ProviderSessionDetail = try await store.rpc(
                    cmd,
                    args: Wrap(request: ProviderDetailRequest(sessionKey: item.sessionKey, limit: 160)),
                    localOnly: provider == .grokbot
                )
                guard isCurrentDetail(item, generation: generation) else { return }
                let mappedSummary = provider == .grokbot ? mapGrokBotSummary(detail.summary) : mapProviderSummary(detail.summary)
                syncSidebarItem(mappedSummary)
                incomingHistory = detail.history
                let candidateSig = makePollSignature(
                    scopeKey: historyScope,
                    history: detail.history,
                    running: mappedSummary.running,
                    title: mappedSummary.title,
                    subagentsSupported: subagentsSupportedForCurrentProvider,
                    subagents: detail.subagents,
                    subagentUpdates: detail.subagentUpdates
                )
                if shouldSkipUnchangedDetailPoll(isFirstLoadForKey: isFirstLoadForKey, candidateSig: candidateSig) {
                    break
                }
                let slot = providerHistoryStore.applyHeadPoll(
                    key: historyScope,
                    headEvents: detail.events,
                    history: detail.history,
                    totalEvents: detail.totalEvents,
                    beforeCursor: detail.beforeCursor,
                    hasMore: detail.hasMore,
                    timeline: detail.timeline,
                    subagents: detail.subagents,
                    subagentUpdates: detail.subagentUpdates
                )
                let nextRows = ConversationRowBuilder.buildProviderRows(
                    events: slot.events,
                    providerLabel: workspace.label,
                    sessionRunning: mappedSummary.running,
                    subagentsSupported: subagentsSupportedForCurrentProvider
                )
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: provider,
                    sources: slot.events.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                applyRenderedDetailSnapshot(
                    tailId: slot.events.last?.id,
                    nextSubagents: subagentsSupportedForCurrentProvider ? slot.subagents : nil,
                    hasMore: slot.hasMore,
                    before: slot.beforeCursor,
                    nextRows: nextRows,
                    nextTimeline: nextTimeline,
                    signature: candidateSig
                )
            }

            currentDetailHistory = incomingHistory
            detailLoadedForKey = item.sessionKey

            // Bounded initial backfill when an entire tail page collapses into <= 2 rows (e.g. massive active tool turn)
            if isFirstLoadForKey,
               !timelineWindow.isActive,
               hasMoreOlder,
               loadOlderError == nil,
               rows.count <= 2
            {
                for _ in 0 ..< 4 {
                    guard isCurrentDetail(item, generation: generation), hasMoreOlder, loadOlderError == nil, rows.count <= 3 else { break }
                    let advanced = await loadSingleOlderPage(for: item, generation: generation, limitOverride: 500, preferBottom: true)
                    if !advanced { break }
                }
            }

            if timelineWindow.isActive, isCurrentDetail(item, generation: generation) {
                await timelineWindow.reconcilePollHistory(incoming: incomingHistory) { cursor in
                    try await readTimelineWindowPage(for: item, cursor: cursor)
                }
            }

            if let pending = pendingSearch, pending.sessionKey == item.sessionKey, isCurrentDetail(item, generation: generation) {
                pendingSearch = nil
                await jumpAndRevealPositionKey(pending.positionKey)
            }
        } catch {
            guard isCurrentDetail(item, generation: generation) else { return }
            errorMessage = error.localizedDescription
        }
    }

    private func makePollSignature(
        scopeKey: String,
        history: HistoryMetadata?,
        running: Bool,
        title: String,
        showAllHistory: Bool = false,
        revealedPositionKey: String? = nil,
        subagentsSupported: Bool,
        subagents: SubagentCollection? = nil,
        subagentUpdates: [String: SubagentActivity]? = nil
    ) -> LastRenderedPollSignature? {
        guard let h = history, !h.epoch.isEmpty, !h.revision.isEmpty, h.reset != true else { return nil }
        return LastRenderedPollSignature(
            scopeKey: scopeKey,
            epoch: h.epoch,
            revision: h.revision,
            running: running,
            title: title,
            showAllHistory: showAllHistory,
            revealedPositionKey: revealedPositionKey,
            subagentsSupported: subagentsSupported,
            subagents: subagents,
            subagentUpdates: subagentUpdates
        )
    }

    private func shouldSkipUnchangedDetailPoll(
        isFirstLoadForKey: Bool,
        candidateSig: LastRenderedPollSignature?
    ) -> Bool {
        !isFirstLoadForKey && !rows.isEmpty && candidateSig != nil && candidateSig == lastRenderedPollSig
    }

    private func applyRenderedDetailSnapshot(
        tailId: String?,
        nextSubagents: SubagentCollection?,
        hasMore: Bool,
        before: String?,
        nextRows: [ConversationRowItem],
        nextTimeline: [TimelineEntry],
        signature: LastRenderedPollSignature?
    ) {
        latestNativeTailId = tailId
        if subagents != nextSubagents { subagents = nextSubagents }
        if hasMoreOlder != hasMore { hasMoreOlder = hasMore }
        if beforeCursor != before { beforeCursor = before }
        if rows != nextRows { rows = nextRows }
        if timelineEntries != nextTimeline { timelineEntries = nextTimeline }
        var nextReply: String?
        for row in nextRows.reversed() {
            if case let .assistant(md, _) = row.kind {
                nextReply = VisibleMarkdownEngine.visibleMarkdown(md)
                break
            }
        }
        if latestReplyText != nextReply { latestReplyText = nextReply }
        lastRenderedPollSig = signature
    }

    @discardableResult
    func loadOlderPageIfNeeded() async -> Bool {
        let generation = detailSelectionGeneration
        guard let item = selectedItem, isCurrentDetail(item, generation: generation) else { return false }
        coordinatorRef?.markUpwardNavigationIntent()
        if timelineWindow.isActive {
            return await ConversationTableCoordinator.runBoundedOlderContinuationLoop(
                coordinator: { [weak self] in self?.coordinatorRef },
                currentRowCount: { [weak self] in self?.coordinatorRef?.rows.count ?? 0 },
                upgradeLimitOnSparseRows: false
            ) { [weak self] limitOverride in
                guard let self, self.isCurrentDetail(item, generation: generation),
                      self.timelineWindow.hasMoreOlder, !self.timelineWindow.busy, self.timelineWindow.error == nil
                else { return false }
                await self.timelineWindow.loadOlder { cursor in
                    try await self.readTimelineWindowPage(for: item, cursor: cursor, limitOverride: limitOverride)
                }
                guard self.isCurrentDetail(item, generation: generation) else { return false }
                if let winRows = self.timelineWindow.items {
                    self.coordinatorRef?.updateRowsPreservingAnchor(winRows, preferAutoFollowBottom: false)
                }
                return self.timelineWindow.error == nil
            }
        }

        guard hasMoreOlder, !loadingOlder, loadOlderError == nil else { return false }
        return await ConversationTableCoordinator.runBoundedOlderContinuationLoop(
            coordinator: { [weak self] in self?.coordinatorRef },
            currentRowCount: { [weak self] in self?.rows.count ?? 0 },
            upgradeLimitOnSparseRows: true
        ) { [weak self] limitOverride in
            guard let self, self.isCurrentDetail(item, generation: generation),
                  self.hasMoreOlder, !self.loadingOlder, self.loadOlderError == nil
            else { return false }
            return await self.loadSingleOlderPage(
                for: item,
                generation: generation,
                limitOverride: limitOverride,
                preferBottom: false
            )
        }
    }

    private func loadSingleOlderPage(
        for item: UnifiedSessionSummaryItem,
        generation: UInt64,
        limitOverride: Int?,
        preferBottom: Bool
    ) async -> Bool {
        guard isCurrentDetail(item, generation: generation), hasMoreOlder, !loadingOlder, loadOlderError == nil, let cursor = beforeCursor else { return false }
        loadingOlder = true
        defer { if isCurrentDetail(item, generation: generation) { loadingOlder = false } }
        let historyScope = "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)"

        do {
            switch provider {
            case .codex:
                let reqLimit = limitOverride ?? 120
                struct BlocksArgs: Encodable {
                    struct Opts: Encodable { let limit: Int; let before: String }
                    let id: String
                    let options: Opts
                }
                let page: ThreadBlockPage = try await store.rpc(
                    "threads.blocks",
                    args: BlocksArgs(id: item.sessionKey, options: .init(limit: reqLimit, before: cursor))
                )
                guard isCurrentDetail(item, generation: generation), beforeCursor == cursor else { return false }
                let slot = threadStore.applyBlockPage(page, expectedCursor: cursor)
                let advanced = (!page.blocks.isEmpty && slot.beforeCursor != cursor)
                hasMoreOlder = advanced ? slot.hasMoreBlocks : false
                beforeCursor = advanced ? slot.beforeCursor : nil
                let nextRows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: item.title,
                    subagentsSupported: subagentsSupportedForCurrentProvider,
                    revealedBlockId: revealedPositionKey
                )
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: preferBottom)
                rows = nextRows
                return advanced || !page.blocks.isEmpty

            case .claudeCode:
                let reqLimit = limitOverride ?? 160
                struct Req: Encodable { let sessionKey: String; let limit: Int; let before: String }
                let page: ClaudeSessionDetail = try await store.rpc(
                    "claude.detail",
                    args: Req(sessionKey: item.sessionKey, limit: reqLimit, before: cursor)
                )
                guard isCurrentDetail(item, generation: generation), beforeCursor == cursor else { return false }
                let slot = claudeHistoryStore.applyOlderPage(
                    key: historyScope,
                    expectedCursor: cursor,
                    olderEvents: page.events,
                    history: page.history,
                    totalEvents: page.totalEvents,
                    beforeCursor: page.beforeCursor,
                    hasMore: page.hasMore
                )
                hasMoreOlder = slot.hasMore
                beforeCursor = slot.beforeCursor
                let nextRows = ConversationRowBuilder.buildClaudeRows(events: slot.events)
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: preferBottom)
                rows = nextRows
                return !page.events.isEmpty && slot.beforeCursor != cursor

            case .grok:
                let reqLimit = limitOverride ?? 160
                struct Req: Encodable { let id: String; let limit: Int; let before: String }
                let page: GrokSessionDetail = try await store.rpc(
                    "grok.detail",
                    args: Req(id: item.sessionKey, limit: reqLimit, before: cursor)
                )
                guard isCurrentDetail(item, generation: generation), beforeCursor == cursor else { return false }
                let slot = grokHistoryStore.applyOlderPage(
                    key: historyScope,
                    expectedCursor: cursor,
                    olderEvents: page.events,
                    history: page.history,
                    totalEvents: page.totalEvents,
                    beforeCursor: page.beforeCursor,
                    hasMore: page.hasMore ?? false
                )
                hasMoreOlder = slot.hasMore
                beforeCursor = slot.beforeCursor
                let nextRows = ConversationRowBuilder.buildGrokRows(
                    events: slot.events,
                    fallbackTitle: item.title,
                    sessionRunning: selectedItem?.running ?? item.running
                )
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: preferBottom)
                rows = nextRows
                return !page.events.isEmpty && slot.beforeCursor != cursor

            case .antigravity, .dsh, .grokbot:
                let reqLimit = limitOverride ?? 160
                let cmd = provider == .antigravity ? "antigravity.detail" : (provider == .dsh ? "dsh.detail" : "grokbot.detail")
                struct Wrap: Encodable { let request: ProviderDetailRequest }
                let page: ProviderSessionDetail = try await store.rpc(
                    cmd,
                    args: Wrap(request: ProviderDetailRequest(sessionKey: item.sessionKey, limit: reqLimit, before: cursor)),
                    localOnly: provider == .grokbot
                )
                guard isCurrentDetail(item, generation: generation), beforeCursor == cursor else { return false }
                let slot = providerHistoryStore.applyOlderPage(
                    key: historyScope,
                    expectedCursor: cursor,
                    olderEvents: page.events,
                    history: page.history,
                    totalEvents: page.totalEvents,
                    beforeCursor: page.beforeCursor,
                    hasMore: page.hasMore,
                    subagentUpdates: page.subagentUpdates
                )
                hasMoreOlder = slot.hasMore
                beforeCursor = slot.beforeCursor
                let nextRows = ConversationRowBuilder.buildProviderRows(
                    events: slot.events,
                    providerLabel: workspace.label,
                    sessionRunning: selectedItem?.running ?? item.running,
                    subagentsSupported: subagentsSupportedForCurrentProvider
                )
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: preferBottom)
                rows = nextRows
                return !page.events.isEmpty && slot.beforeCursor != cursor
            }
        } catch {
            guard isCurrentDetail(item, generation: generation) else { return false }
            loadOlderError = error.localizedDescription
            return false
        }
    }

    func performRename(item: UnifiedSessionSummaryItem, newTitle: String) async throws {
        guard store.revision == sourceRevision else { throw CancellationError() }
        try await Self.performRenameStatic(
            store: store,
            provider: provider,
            sessionKey: item.sessionKey,
            newTitle: newTitle,
            threadStore: threadStore
        )
        let trimmed = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            var optimistic = selectedItem ?? item
            optimistic.title = trimmed
            syncSidebarItem(optimistic)
        }
        if !skipSidebarListFetch {
            await fetchSessionList()
        }
        onExternalMutation?()
    }

    static func performRenameStatic(
        store: RuntimeConnectionStore,
        provider: SessionProvider,
        sessionKey: String,
        newTitle: String,
        threadStore: ThreadMessageStore? = nil
    ) async throws {
        switch provider {
        case .codex:
            struct RenameArgs: Encodable { let threadId: String; let name: String }
            let _: StartedJobResponse = try await store.rpc("threads.rename", args: RenameArgs(threadId: sessionKey, name: newTitle))
            threadStore?.recordOptimisticTitle(threadId: sessionKey, title: newTitle)
        case .claudeCode:
            struct Args: Encodable { let sessionKey: String; let title: String }
            let _: ClaudeSessionSummary = try await store.rpc("claude.rename", args: Args(sessionKey: sessionKey, title: newTitle))
        case .grok:
            struct Args: Encodable { let id: String; let title: String }
            let _: GrokSessionSummary = try await store.rpc("grok.rename", args: Args(id: sessionKey, title: newTitle))
        case .antigravity, .dsh:
            let cmd = provider == .antigravity ? "antigravity.rename" : "dsh.rename"
            struct Wrap: Encodable {
                struct Inner: Encodable { let sessionKey: String; let title: String }
                let request: Inner
            }
            let _: ProviderSessionSummary = try await store.rpc(cmd, args: Wrap(request: .init(sessionKey: sessionKey, title: newTitle)))
        case .grokbot:
            break
        }
    }

    static func performToggleArchiveStatic(
        store: RuntimeConnectionStore,
        provider: SessionProvider,
        sessionKey: String,
        currentlyArchived: Bool
    ) async throws {
        if provider == .codex {
            struct Args: Encodable { let threadId: String }
            let cmd = currentlyArchived ? "threads.restore" : "threads.archive"
            let _: StartedJobResponse = try await store.rpc(cmd, args: Args(threadId: sessionKey))
        } else {
            let prefix = provider == .antigravity ? "antigravity" : "dsh"
            let action = currentlyArchived ? "restore" : "archive"
            struct Wrap: Encodable {
                struct Inner: Encodable { let sessionKey: String }
                let request: Inner
            }
            let _: ProviderSessionSummary = try await store.rpc("\(prefix).\(action)", args: Wrap(request: .init(sessionKey: sessionKey)))
        }
    }

    func toggleSingleArchive(_ item: UnifiedSessionSummaryItem) async {
        guard store.revision == sourceRevision else { return }
        do {
            let nextFallbackKey: String? = {
                guard !item.archived, selectedKey == item.sessionKey, !archivedOpen else { return nil }
                guard let idx = activeItems.firstIndex(where: { $0.sessionKey == item.sessionKey }) else { return nil }
                if idx + 1 < activeItems.count { return activeItems[idx + 1].sessionKey }
                if idx > 0 { return activeItems[idx - 1].sessionKey }
                return nil
            }()
            try await Self.performToggleArchiveStatic(
                store: store,
                provider: provider,
                sessionKey: item.sessionKey,
                currentlyArchived: item.archived
            )
            feedbackMessage = item.archived ? "已恢复线程" : "已归档线程"
            if !item.archived && selectedKey == item.sessionKey && !archivedOpen {
                selectedKey = nextFallbackKey
                headerInlineRename = false
                onExternalSelectSession?(nextFallbackKey)
            }
            if !skipSidebarListFetch {
                await fetchSessionList()
            }
            onExternalMutation?()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func triggerSingleDelete(_ item: UnifiedSessionSummaryItem) async {
        guard store.revision == sourceRevision else { return }
        let targetPartition: SessionSelectionPartition = item.archived ? .archived : .active
        await prepareBatch(operation: .delete, keys: [item.sessionKey], partition: targetPartition)
    }

    func fetchClaudeOwnershipDiagnostics(sessionKey: String) async {
        guard store.target == .local, !claudeOwnershipLoading else { return }
        let capturedRevision = store.revision
        claudeOwnershipKey = sessionKey
        claudeOwnershipData = nil
        claudeOwnershipError = nil
        claudeOwnershipLoading = true
        defer {
            if claudeOwnershipKey == sessionKey {
                claudeOwnershipLoading = false
            }
        }
        struct Args: Encodable { let sessionKey: String }
        do {
            let diag: ClaudeOwnershipDiagnostics = try await store.rpc(
                "claude.ownershipDiagnostics",
                args: Args(sessionKey: sessionKey),
                localOnly: true
            )
            guard store.revision == capturedRevision,
                  claudeOwnershipKey == sessionKey,
                  selectedItem?.sessionKey == sessionKey
            else { return }
            claudeOwnershipData = diag
        } catch {
            guard store.revision == capturedRevision,
                  claudeOwnershipKey == sessionKey,
                  selectedItem?.sessionKey == sessionKey
            else { return }
            claudeOwnershipError = error.localizedDescription
        }
    }

    // Chunked 100-item archive/restore & single-batch delete with partial-failure preservation
    func prepareBatch(
        operation: SessionOperation,
        keys: [String],
        partition: SessionSelectionPartition
    ) async {
        guard store.revision == sourceRevision, !keys.isEmpty, !batchBusy else { return }
        let capturedRevision = store.revision
        batchTargetPartition = partition
        batchBusy = true
        batchPrepareError = nil
        batchExecuteError = nil
        batchResult = nil
        batchPreview = nil
        defer { batchBusy = false }
        let targetStore = store
        do {
            let preview = try await SessionBatchChunkExecutor.preview(
                provider: provider,
                operation: operation,
                sessionKeys: keys
            ) { req in
                try await Self.sendBulkPreview(store: targetStore, request: req, at: capturedRevision)
            }
            guard store.revision == capturedRevision else { return }
            batchPreview = preview
        } catch {
            guard store.revision == capturedRevision else { return }
            batchPrepareError = error.localizedDescription
        }
    }

    func executePreparedBatch() async {
        guard store.revision == sourceRevision, let preview = batchPreview, !batchBusy else { return }
        let capturedRevision = store.revision
        let targetPartition = batchTargetPartition
        batchPreview = nil
        batchBusy = true
        batchExecuteError = nil
        defer { batchBusy = false }

        let targetStore = store
        let outcome = await SessionBatchChunkExecutor.execute(preview: preview) { req in
            try await Self.sendBulkExecute(store: targetStore, request: req, at: capturedRevision)
        }
        guard store.revision == capturedRevision else { return }

        if let res = outcome.result {
            batchResult = res
            // Remove ONLY confirmed succeeded keys from selection; keep failed / unconfirmed items selected!
            partitionedSelection.applyBatchResult(res, partition: targetPartition)
            let okSet = Set(outcome.confirmedSucceededKeys)
            if preview.operation == .delete && provider != .codex && !okSet.isEmpty {
                activeItems.removeAll { okSet.contains($0.sessionKey) }
                archivedItems.removeAll { okSet.contains($0.sessionKey) }
            }
            if let sel = selectedKey, okSet.contains(sel) {
                selectedKey = nil
                onExternalSelectSession?(nil)
            }
        }
        batchExecuteError = outcome.errorMessage
        if !skipSidebarListFetch {
            await fetchSessionList()
        }
        onExternalMutation?()
    }

    // MARK: - Bounded Timeline Window Jump (j:/w:/n: Cursors) & Search Reveal

    func handleTimelineTickSelected(_ entry: TimelineEntry) async {
        await jumpAndRevealPositionKey(entry.id)
    }

    func jumpAndRevealPositionKey(_ targetId: String) async {
        guard targetId != "session", !targetId.isEmpty, let item = selectedItem, isCurrentDetail(item) else { return }

        // If target is already loaded in the current displayed rows, jump directly without fetching.
        if coordinatorRef?.scrollToPositionKey(targetId, reduceMotion: motionObserver.reduceMotion) == true {
            return
        }

        // Unfold single target inside Codex 60/3 compact history if already in tail slot.
        if provider == .codex, !timelineWindow.isActive {
            let slot = threadStore.slot(for: item.sessionKey)
            if slot.blocks.contains(where: { TimelineViewModel.matchesTimelinePosition(targetId, id: $0.id) }) {
                revealedPositionKey = targetId
                let rebuilt = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: item.title,
                    subagentsSupported: subagentsSupportedForCurrentProvider,
                    revealedBlockId: targetId
                )
                coordinatorRef?.updateRowsPreservingAnchor(rebuilt, preferAutoFollowBottom: false)
                rows = rebuilt
                _ = coordinatorRef?.scrollToPositionKey(targetId, reduceMotion: motionObserver.reduceMotion)
                return
            }
        }

        let scopeKey = "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)"
        await timelineWindow.jump(
            to: targetId,
            in: scopeKey,
            currentHistory: currentDetailHistory,
            matchesTarget: { row, id in row.matches(positionKey: id) }
        ) { cursor in
            try await readTimelineWindowPage(for: item, cursor: cursor)
        }

        if isCurrentDetail(item), timelineWindow.pendingTargetId == targetId, let winRows = timelineWindow.items {
            coordinatorRef?.updateRowsPreservingAnchor(winRows, preferAutoFollowBottom: false)
            _ = coordinatorRef?.scrollToPositionKey(targetId, reduceMotion: motionObserver.reduceMotion)
            timelineWindow.clearPendingTarget()
        }
    }

    func loadNewerTimelineWindowPage() async {
        let generation = detailSelectionGeneration
        guard let item = selectedItem, timelineWindow.isActive, isCurrentDetail(item, generation: generation) else { return }
        await timelineWindow.loadNewer(latestTailId: latestNativeTailId ?? rows.last?.timelineId) { cursor in
            try await readTimelineWindowPage(for: item, cursor: cursor)
        }
        guard isCurrentDetail(item, generation: generation) else { return }
        if let winRows = timelineWindow.items, coordinatorRef?.rows != winRows {
            coordinatorRef?.updateRowsPreservingAnchor(winRows, preferAutoFollowBottom: false)
        }
    }

    func returnFromTimelineWindowToLatest() {
        timelineWindow.returnToLatest()
        coordinatorRef?.updateRowsPreservingAnchor(rows, preferAutoFollowBottom: true, forceScrollToBottom: true)
    }

    func readTimelineWindowPage(
        for item: UnifiedSessionSummaryItem,
        cursor: String,
        limitOverride: Int? = nil
    ) async throws -> TimelineWindowPage<ConversationRowItem> {
        switch provider {
        case .codex:
            let reqLimit = limitOverride ?? 120
            struct DetailArgs: Encodable {
                struct Opts: Encodable { let limit: Int; let before: String }
                let id: String
                let options: Opts
            }
            guard let page: ThreadDetail = try await store.rpc(
                "threads.detail",
                args: DetailArgs(id: item.sessionKey, options: .init(limit: reqLimit, before: cursor))
            ) else {
                throw NSError(
                    domain: "AgentHub.TimelineWindow",
                    code: 404,
                    userInfo: [NSLocalizedDescriptionKey: "历史读取失败，请重试"]
                )
            }
            let rawPage = TimelineWindowPage<MessageBlock>.from(threadDetail: page)
            let mappedRows = ConversationRowBuilder.buildCodexRows(
                blocks: rawPage.items,
                showAllHistory: true,
                planFallbackTitle: item.title,
                subagentsSupported: subagentsSupportedForCurrentProvider,
                revealedBlockId: cursor.hasPrefix("j:") ? String(cursor.dropFirst(2)) : nil
            )
            return TimelineWindowPage<ConversationRowItem>(
                items: mappedRows,
                before: rawPage.before,
                after: rawPage.after,
                hasMore: rawPage.hasMore,
                history: rawPage.history,
                firstNativeId: rawPage.firstNativeId,
                lastNativeId: rawPage.lastNativeId,
                nativeIds: rawPage.nativeIds
            )

        case .claudeCode:
            let reqLimit = limitOverride ?? 160
            struct Req: Encodable { let sessionKey: String; let limit: Int; let before: String }
            let page: ClaudeSessionDetail = try await store.rpc(
                "claude.detail",
                args: Req(sessionKey: item.sessionKey, limit: reqLimit, before: cursor)
            )
            let rawPage = TimelineWindowPage<ClaudeHistoryEvent>.from(claudeDetail: page)
            return TimelineWindowPage<ConversationRowItem>(
                items: ConversationRowBuilder.buildClaudeRows(events: rawPage.items),
                before: rawPage.before,
                after: rawPage.after,
                hasMore: rawPage.hasMore,
                history: rawPage.history,
                firstNativeId: rawPage.firstNativeId,
                lastNativeId: rawPage.lastNativeId,
                nativeIds: rawPage.nativeIds
            )

        case .grok:
            let reqLimit = limitOverride ?? 160
            struct Req: Encodable { let id: String; let limit: Int; let before: String }
            let page: GrokSessionDetail = try await store.rpc(
                "grok.detail",
                args: Req(id: item.sessionKey, limit: reqLimit, before: cursor)
            )
            let rawPage = TimelineWindowPage<GrokHistoryEvent>.from(grokDetail: page)
            return TimelineWindowPage<ConversationRowItem>(
                items: ConversationRowBuilder.buildGrokRows(
                    events: rawPage.items,
                    fallbackTitle: item.title,
                    sessionRunning: selectedItem?.running ?? item.running
                ),
                before: rawPage.before,
                after: rawPage.after,
                hasMore: rawPage.hasMore,
                history: rawPage.history,
                firstNativeId: rawPage.firstNativeId,
                lastNativeId: rawPage.lastNativeId,
                nativeIds: rawPage.nativeIds
            )

        case .antigravity, .dsh, .grokbot:
            let reqLimit = limitOverride ?? 160
            let cmd = provider == .antigravity ? "antigravity.detail" : (provider == .dsh ? "dsh.detail" : "grokbot.detail")
            struct Wrap: Encodable { let request: ProviderDetailRequest }
            let page: ProviderSessionDetail = try await store.rpc(
                cmd,
                args: Wrap(request: ProviderDetailRequest(sessionKey: item.sessionKey, limit: reqLimit, before: cursor)),
                localOnly: provider == .grokbot
            )
            let rawPage = TimelineWindowPage<ProviderHistoryEvent>.from(providerDetail: page)
            return TimelineWindowPage<ConversationRowItem>(
                items: ConversationRowBuilder.buildProviderRows(
                    events: rawPage.items,
                    providerLabel: workspace.label,
                    sessionRunning: selectedItem?.running ?? item.running,
                    subagentsSupported: subagentsSupportedForCurrentProvider
                ),
                before: rawPage.before,
                after: rawPage.after,
                hasMore: rawPage.hasMore,
                history: rawPage.history,
                firstNativeId: rawPage.firstNativeId,
                lastNativeId: rawPage.lastNativeId,
                nativeIds: rawPage.nativeIds
            )
        }
    }

    func handleSearchResultSelected(_ result: SessionSearchResult) {
        if selectedItem?.sessionKey != result.sessionKey {
            pendingSearch = result
            if !allVisibleItems.contains(where: { $0.sessionKey == result.sessionKey }) {
                searchSelection = UnifiedSessionSummaryItem(
                    sessionKey: result.sessionKey, nativeId: result.nativeId, title: result.title,
                    cwd: result.cwd ?? "", path: "", statusText: "", running: false, archived: false,
                    canRename: false, canArchive: false, canDelete: false
                )
            }
            selectedKey = result.sessionKey
            if supportsArchive && !activeItems.contains(where: { $0.sessionKey == result.sessionKey }) {
                archivedOpen = true
            }
            onExternalSelectSession?(result.sessionKey)
        } else {
            Task { await self.jumpAndRevealPositionKey(result.positionKey) }
        }
    }

    static func providerSessionStatusText(status: String, updatedAt: String?, archived: Bool) -> String {
        if status == "running" { return "运行中" }
        if status == "waiting" { return "等待处理" }
        if status == "reply_needed" { return "待回复" }
        if status == "failed" { return "异常" }
        if archived { return "已归档" }
        if status == "unknown" { return "状态未知" }
        if let date = SharedTimestampFormatters.parseISO8601(updatedAt) {
            return SharedTimestampFormatters.formatSessionStatusTimestamp(date)
        }
        return "最近"
    }

    func mapClaudeSummary(_ item: ClaudeSessionSummary) -> UnifiedSessionSummaryItem {
        Self.mapClaudeSummaryStatic(item)
    }

    static func mapClaudeSummaryStatic(_ item: ClaudeSessionSummary) -> UnifiedSessionSummaryItem {
        let stLabel = providerSessionStatusText(status: item.status, updatedAt: item.updatedAt, archived: false)
        let chip: (String?, StatusChipTone) = if item.status == "running" {
            ("运行中", .info)
        } else if item.status == "waiting" {
            ("等待处理", .warning)
        } else if item.readError != nil {
            ("异常", .danger)
        } else {
            (nil, .neutral)
        }
        return UnifiedSessionSummaryItem(
            sessionKey: item.sessionKey,
            nativeId: item.id,
            title: item.title.isEmpty ? item.id : item.title,
            cwd: item.cwd,
            workspaceRoot: item.workspaceRoot,
            path: item.path,
            updatedAt: item.updatedAt,
            statusText: stLabel,
            statusChipBadge: chip.0,
            statusChipTone: chip.1,
            running: item.status == "running",
            archived: false,
            canRename: item.canRename,
            renameBlockReason: item.renameBlockReason,
            canArchive: false,
            canDelete: true,
            deleteBlockReason: item.deleteBlockReason,
            storageSize: item.storageSize,
            readError: item.readError,
            readWarning: item.readWarning
        )
    }

    func mapGrokSummary(_ item: GrokSessionSummary) -> UnifiedSessionSummaryItem {
        Self.mapGrokSummaryStatic(item)
    }

    static func mapGrokSummaryStatic(_ item: GrokSessionSummary) -> UnifiedSessionSummaryItem {
        UnifiedSessionSummaryItem(
            sessionKey: item.id,
            nativeId: item.id,
            title: item.title.isEmpty ? item.id : item.title,
            cwd: item.cwd,
            workspaceRoot: item.workspaceRoot,
            path: HistoryAndResumeHelpers.grokHandoffPath(item),
            updatedAt: item.updatedAt,
            statusText: providerSessionStatusText(status: item.status, updatedAt: item.updatedAt, archived: false),
            statusChipBadge: item.status == "running" ? "运行中" : nil,
            statusChipTone: .info,
            running: item.status == "running",
            archived: false,
            canRename: true,
            renameBlockReason: nil,
            canArchive: false,
            canDelete: true,
            storageSize: item.storageSize
        )
    }

    func mapGrokBotSummary(_ item: ProviderSessionSummary) -> UnifiedSessionSummaryItem {
        Self.mapGrokBotSummary(item)
    }

    static func mapGrokBotSummary(_ item: ProviderSessionSummary) -> UnifiedSessionSummaryItem {
        let st = item.status == "authorization" ? "需要授权"
            : (item.status == "reply_needed" ? "需要回复" : (!item.path.isEmpty ? "近期缓存" : "未缓存"))
        return UnifiedSessionSummaryItem(
            sessionKey: item.sessionKey,
            nativeId: item.id,
            title: item.title.isEmpty ? item.id : item.title,
            cwd: item.cwd,
            workspaceRoot: item.workspaceRoot,
            path: item.path,
            updatedAt: item.updatedAt,
            statusText: st,
            statusChipBadge: "只读",
            statusChipTone: .neutral,
            running: false,
            archived: false,
            canRename: false,
            renameBlockReason: nil,
            canArchive: false,
            canDelete: false,
            storageSize: item.storageSize
        )
    }

    func mapCodexSummary(_ s: ThreadSummary, archived: Bool) -> UnifiedSessionSummaryItem {
        Self.mapCodexSummaryStatic(s, archived: archived, store: store, threadStore: threadStore)
    }

    static func mapCodexSummaryStatic(
        _ s: ThreadSummary,
        archived: Bool,
        store: RuntimeConnectionStore,
        threadStore: ThreadMessageStore?
    ) -> UnifiedSessionSummaryItem {
        let serverTitle = s.title.isEmpty ? s.id : s.title
        let displayTitle = threadStore?.effectiveTitle(threadId: s.id, serverTitle: serverTitle) ?? serverTitle
        let isArchived = archived || s.status == "Archived"
        let running = !isArchived && (s.status == "Running")
        let stText: String = switch s.status {
        case "Running": "运行中"
        case "ReplyNeeded": "待回复"
        case "Recoverable": "可恢复"
        case "Archived": "已归档"
        default: isArchived ? "已归档" : "最近"
        }
        let chipTone: StatusChipTone = if isArchived {
            .neutral
        } else if running {
            .info
        } else {
            switch s.status {
            case "ReplyNeeded": .warning
            case "Recoverable": .danger
            default: .success
            }
        }
        let rawPreview = VisibleMarkdownEngine.visibleMarkdown(s.latestMessage ?? "")
            .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let preview = (rawPreview == displayTitle) ? "" : rawPreview
        return UnifiedSessionSummaryItem(
            sessionKey: s.id,
            nativeId: s.id,
            title: displayTitle,
            cwd: s.cwd ?? "",
            workspaceRoot: s.workspaceRoot,
            path: s.rolloutPath ?? "",
            updatedAt: s.updatedAt,
            previewText: preview,
            statusText: stText,
            statusChipBadge: stText,
            statusChipTone: chipTone,
            running: running,
            archived: isArchived,
            canRename: true,
            renameBlockReason: nil,
            canArchive: store.capabilities.isReady,
            canDelete: true,
            storageSize: s.storageSize
        )
    }

    func mapProviderSummary(_ s: ProviderSessionSummary) -> UnifiedSessionSummaryItem {
        Self.mapProviderSummaryStatic(s)
    }

    static func mapProviderSummaryStatic(_ s: ProviderSessionSummary) -> UnifiedSessionSummaryItem {
        let chip: (String?, StatusChipTone) = if s.archived {
            ("已归档", .neutral)
        } else if s.status == "running" {
            ("运行中", .info)
        } else if s.status == "waiting" {
            ("等待处理", .warning)
        } else if s.status == "reply_needed" {
            ("待回复", .warning)
        } else if s.status == "failed" || s.readError != nil {
            ("异常", .danger)
        } else {
            (nil, .neutral)
        }
        return UnifiedSessionSummaryItem(
            sessionKey: s.sessionKey,
            nativeId: s.id,
            title: s.title.isEmpty ? s.id : s.title,
            cwd: s.cwd,
            workspaceRoot: s.workspaceRoot,
            path: s.path,
            updatedAt: s.updatedAt,
            statusText: providerSessionStatusText(status: s.status, updatedAt: s.updatedAt, archived: s.archived),
            statusChipBadge: chip.0,
            statusChipTone: chip.1,
            running: s.status == "running",
            archived: s.archived,
            canRename: s.canRename,
            renameBlockReason: s.renameBlockReason,
            canArchive: s.canArchive,
            archiveBlockReason: s.archiveBlockReason,
            canDelete: s.canDelete,
            deleteBlockReason: s.deleteBlockReason,
            storageSize: s.storageSize,
            readError: s.readError,
            readWarning: s.readWarning
        )
    }
}
