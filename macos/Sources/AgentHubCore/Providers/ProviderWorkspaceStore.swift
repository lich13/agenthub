import AppKit
import Combine
import SwiftUI

@MainActor
final class ProviderWorkspaceStore: ObservableObject {
    let store: RuntimeConnectionStore
    let workspace: WorkspaceId
    weak var coordinatorRef: ConversationTableCoordinator?
    private var observations = Set<AnyCancellable>()

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
            if oldValue != groupingMode { syncPartitionedSelection() }
        }
    }
    @Published var workspaceFilter: ProjectWorkspaceFilter = .all {
        didSet {
            if oldValue != workspaceFilter { syncPartitionedSelection() }
        }
    }
    @Published var collapsedWorkspaceKeys: Set<ProjectWorkspaceKey> = [] {
        didSet {
            if oldValue != collapsedWorkspaceKeys { syncPartitionedSelection() }
        }
    }
    @Published var collapsedArchivedWorkspaceKeys: Set<ProjectWorkspaceKey> = [] {
        didSet {
            if oldValue != collapsedArchivedWorkspaceKeys { syncPartitionedSelection() }
        }
    }
    @Published var activeItems: [UnifiedSessionSummaryItem] = []
    @Published var archivedItems: [UnifiedSessionSummaryItem] = []
    @Published var archivedOpen: Bool = false
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
    @Published var codexTailRefreshTask: Task<Void, Never>?
    @Published var codexCreateError: String?

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
        self.store = store
        self.workspace = workspace
        self.sourceRevision = store.revision
        for changes in [threadStore.objectWillChange, claudeHistoryStore.objectWillChange,
                        grokHistoryStore.objectWillChange, providerHistoryStore.objectWillChange,
                        timelineWindow.objectWillChange, motionObserver.objectWillChange] {
            changes.sink { [weak self] in self?.objectWillChange.send() }.store(in: &observations)
        }
        NotificationCenter.default.publisher(for: .agentHubCodexDesktopViewChanged)
            .sink { [weak self] _ in
                Task { @MainActor [weak self] in
                    self?.applyLiveCodexDesktopOverlay()
                }
            }
            .store(in: &observations)
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
        ProjectWorkspaceEngine.filterItems(activeItems, by: workspaceFilter, enabled: supportsWorkspaceGrouping)
    }

    var filteredArchivedItems: [UnifiedSessionSummaryItem] {
        ProjectWorkspaceEngine.filterItems(archivedItems, by: workspaceFilter, enabled: supportsWorkspaceGrouping)
    }

    var activeWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] {
        ProjectWorkspaceEngine.buildGroups(items: filteredActiveItems)
    }

    var archivedWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] {
        ProjectWorkspaceEngine.buildGroups(items: filteredArchivedItems)
    }

    var availableWorkspaceGroups: [ProjectWorkspaceGroup<UnifiedSessionSummaryItem>] {
        ProjectWorkspaceEngine.buildGroups(items: activeItems + (archivedOpen ? archivedItems : []))
    }

    var visualOrderedActiveKeys: [String] {
        if supportsWorkspaceGrouping && groupingMode == .byWorkspace {
            return ProjectWorkspaceEngine.visualOrderedKeys(
                groups: activeWorkspaceGroups,
                collapsedKeys: collapsedWorkspaceKeys
            )
        }
        return filteredActiveItems.map(\.sessionKey)
    }

    var visualOrderedArchivedKeys: [String] {
        if supportsWorkspaceGrouping && groupingMode == .byWorkspace {
            return ProjectWorkspaceEngine.visualOrderedKeys(
                groups: archivedWorkspaceGroups,
                collapsedKeys: collapsedArchivedWorkspaceKeys
            )
        }
        return filteredArchivedItems.map(\.sessionKey)
    }

    var allVisibleItems: [UnifiedSessionSummaryItem] {
        filteredActiveItems + (archivedOpen ? filteredArchivedItems : [])
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
            if let match = (activeItems + archivedItems).first(where: { $0.sessionKey == key }) {
                return match
            }
            if let searchSelection, searchSelection.sessionKey == key { return searchSelection }
            if let externalSeedItem, externalSeedItem.sessionKey == key { return externalSeedItem }
        }
        if supportsWorkspaceGrouping && workspaceFilter != .all {
            return filteredActiveItems.first
        }
        if !query.isEmpty {
            return filteredActiveItems.first
        }
        return filteredActiveItems.first ?? activeItems.first
    }

    var displayedRows: [ConversationRowItem] {
        timelineWindow.items ?? rows
    }

    func syncPartitionedSelection() {
        let baseScope = "\(store.revision):\(workspace.rawValue):\(query):\(workspaceFilter.scopeToken)"
        partitionedSelection.sync(
            activeScope: "\(baseScope)#active",
            activeVisible: visualOrderedActiveKeys,
            archivedScope: "\(baseScope)#archived",
            archivedVisible: visualOrderedArchivedKeys,
            archivedOpen: supportsArchive && archivedOpen
        )
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
        let requestedQuery = query
        let requestedArchived = archivedOpen
        func isCurrent() -> Bool {
            !Task.isCancelled && store.revision == sourceRevision && query == requestedQuery && archivedOpen == requestedArchived
        }
        guard isCurrent() else { return }
        do {
            switch provider {
            case .codex, .claudeCode, .grok, .antigravity, .dsh:
                if isRemoteProviderUnsupported {
                    activeItems = []
                    archivedItems = []
                    syncPartitionedSelection()
                    errorMessage = nil
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
                activeItems = active
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
                    archivedItems = arch
                }

            case .grokbot:
                guard store.target == .local else { activeItems = []; return }
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
                activeItems = list.map(Self.mapGrokBotSummary)
            }
            syncPartitionedSelection()
            errorMessage = nil
        } catch {
            guard isCurrent() else { return }
            errorMessage = error.localizedDescription
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

    func scheduleDebouncedCodexTailRefresh() {
        codexTailRefreshTask?.cancel()
        codexTailRefreshTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 350_000_000)
            guard let self, !Task.isCancelled, provider == .codex, let item = selectedItem else { return }
            await fetchSessionDetail(for: item)
        }
    }

    func resetDetailStateForSelectionChange() {
        detailSelectionGeneration &+= 1
        errorMessage = nil
        feedbackMessage = nil
        currentDetailHistory = nil
        headerInlineRename = false
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
                let candidateSig: LastRenderedPollSignature? = detail.history.flatMap { h in
                    guard !h.epoch.isEmpty, !h.revision.isEmpty, h.reset != true else { return nil }
                    return LastRenderedPollSignature(
                        scopeKey: historyScope,
                        epoch: h.epoch,
                        revision: h.revision,
                        running: mappedCodex.running,
                        title: effectiveTitle,
                        showAllHistory: showAllHistory || threadStore.slot(for: item.sessionKey).showAllHistory,
                        revealedPositionKey: revealedPositionKey,
                        subagentsSupported: subagentsSupportedForCurrentProvider,
                        subagents: detail.subagents,
                        subagentUpdates: detail.subagentUpdates
                    )
                }
                if !isFirstLoadForKey, !rows.isEmpty, let candidateSig, candidateSig == lastRenderedPollSig {
                    break
                }
                let slot = threadStore.applyDetail(detail)
                latestNativeTailId = slot.blocks.last?.id
                let nextSubs = subagentsSupportedForCurrentProvider ? slot.subagents : nil
                if subagents != nextSubs { subagents = nextSubs }
                if hasMoreOlder != slot.hasMoreBlocks { hasMoreOlder = slot.hasMoreBlocks }
                if beforeCursor != slot.beforeCursor { beforeCursor = slot.beforeCursor }
                let nextRows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: effectiveTitle,
                    subagentsSupported: subagentsSupportedForCurrentProvider,
                    revealedBlockId: revealedPositionKey
                )
                if rows != nextRows { rows = nextRows }
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .codex,
                    sources: slot.blocks.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                if timelineEntries != nextTimeline { timelineEntries = nextTimeline }
                let nextReply = nextRows.reversed().compactMap { r -> String? in
                    if case let .assistant(md, _) = r.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }
                    return nil
                }.first
                if latestReplyText != nextReply { latestReplyText = nextReply }
                lastRenderedPollSig = candidateSig

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
                let candidateSig: LastRenderedPollSignature? = detail.history.flatMap { h in
                    guard !h.epoch.isEmpty, !h.revision.isEmpty, h.reset != true else { return nil }
                    return LastRenderedPollSignature(
                        scopeKey: historyScope,
                        epoch: h.epoch,
                        revision: h.revision,
                        running: mappedClaude.running,
                        title: mappedClaude.title,
                        showAllHistory: false,
                        revealedPositionKey: nil,
                        subagentsSupported: subagentsSupportedForCurrentProvider,
                        subagents: detail.subagents,
                        subagentUpdates: nil
                    )
                }
                if !isFirstLoadForKey, !rows.isEmpty, let candidateSig, candidateSig == lastRenderedPollSig {
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
                latestNativeTailId = slot.events.last?.id
                let nextSubs = subagentsSupportedForCurrentProvider ? slot.subagents : nil
                if subagents != nextSubs { subagents = nextSubs }
                if hasMoreOlder != slot.hasMore { hasMoreOlder = slot.hasMore }
                if beforeCursor != slot.beforeCursor { beforeCursor = slot.beforeCursor }
                let nextRows = ConversationRowBuilder.buildClaudeRows(events: slot.events)
                if rows != nextRows { rows = nextRows }
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .claudeCode,
                    sources: slot.events.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                if timelineEntries != nextTimeline { timelineEntries = nextTimeline }
                let nextReply = nextRows.reversed().compactMap { if case let .assistant(md, _) = $0.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }; return nil }.first
                if latestReplyText != nextReply { latestReplyText = nextReply }
                lastRenderedPollSig = candidateSig

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
                let candidateSig: LastRenderedPollSignature? = detail.history.flatMap { h in
                    guard !h.epoch.isEmpty, !h.revision.isEmpty, h.reset != true else { return nil }
                    return LastRenderedPollSignature(
                        scopeKey: historyScope,
                        epoch: h.epoch,
                        revision: h.revision,
                        running: mappedGrok.running,
                        title: mappedGrok.title,
                        showAllHistory: false,
                        revealedPositionKey: nil,
                        subagentsSupported: false,
                        subagents: nil,
                        subagentUpdates: nil
                    )
                }
                if !isFirstLoadForKey, !rows.isEmpty, let candidateSig, candidateSig == lastRenderedPollSig {
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
                latestNativeTailId = slot.events.last.flatMap { $0.id ?? $0.userMessage?.id }
                if subagents != nil { subagents = nil }
                if hasMoreOlder != slot.hasMore { hasMoreOlder = slot.hasMore }
                if beforeCursor != slot.beforeCursor { beforeCursor = slot.beforeCursor }
                let nextRows = ConversationRowBuilder.buildGrokRows(
                    events: slot.events,
                    fallbackTitle: mappedGrok.title,
                    sessionRunning: mappedGrok.running
                )
                if rows != nextRows { rows = nextRows }
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .grok,
                    sources: slot.events.enumerated().map { idx, ev in
                        TimelineSourceItem(id: ev.id ?? ev.userMessage?.id ?? "grok-\(idx)", role: nil, kind: ev.kind, text: ev.text, userMessage: ev.userMessage)
                    }
                )
                if timelineEntries != nextTimeline { timelineEntries = nextTimeline }
                let nextReply = nextRows.reversed().compactMap { if case let .assistant(md, _) = $0.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }; return nil }.first
                if latestReplyText != nextReply { latestReplyText = nextReply }
                lastRenderedPollSig = candidateSig

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
                let candidateSig: LastRenderedPollSignature? = detail.history.flatMap { h in
                    guard !h.epoch.isEmpty, !h.revision.isEmpty, h.reset != true else { return nil }
                    return LastRenderedPollSignature(
                        scopeKey: historyScope,
                        epoch: h.epoch,
                        revision: h.revision,
                        running: mappedSummary.running,
                        title: mappedSummary.title,
                        showAllHistory: false,
                        revealedPositionKey: nil,
                        subagentsSupported: subagentsSupportedForCurrentProvider,
                        subagents: detail.subagents,
                        subagentUpdates: detail.subagentUpdates
                    )
                }
                if !isFirstLoadForKey, !rows.isEmpty, let candidateSig, candidateSig == lastRenderedPollSig {
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
                latestNativeTailId = slot.events.last?.id
                let nextSubs = subagentsSupportedForCurrentProvider ? slot.subagents : nil
                if subagents != nextSubs { subagents = nextSubs }
                if hasMoreOlder != slot.hasMore { hasMoreOlder = slot.hasMore }
                if beforeCursor != slot.beforeCursor { beforeCursor = slot.beforeCursor }
                let nextRows = ConversationRowBuilder.buildProviderRows(
                    events: slot.events,
                    providerLabel: workspace.label,
                    sessionRunning: mappedSummary.running,
                    subagentsSupported: subagentsSupportedForCurrentProvider
                )
                if rows != nextRows { rows = nextRows }
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: provider,
                    sources: slot.events.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                if timelineEntries != nextTimeline { timelineEntries = nextTimeline }
                let nextReply = nextRows.reversed().compactMap { if case let .assistant(md, _) = $0.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }; return nil }.first
                if latestReplyText != nextReply { latestReplyText = nextReply }
                lastRenderedPollSig = candidateSig
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

    @discardableResult
    func loadOlderPageIfNeeded() async -> Bool {
        let generation = detailSelectionGeneration
        guard let item = selectedItem, isCurrentDetail(item, generation: generation) else { return false }
        coordinatorRef?.markUpwardNavigationIntent()
        if timelineWindow.isActive {
            var anyLoaded = false
            for step in 0 ..< 5 {
                guard isCurrentDetail(item, generation: generation), timelineWindow.hasMoreOlder, !timelineWindow.busy, timelineWindow.error == nil else { break }
                let prevHeight = coordinatorRef?.renderedContentHeight ?? 0
                let prevRowCount = coordinatorRef?.rows.count ?? 0
                let pageLimit: Int? = step > 0 ? 500 : nil
                await timelineWindow.loadOlder { cursor in
                    try await readTimelineWindowPage(for: item, cursor: cursor, limitOverride: pageLimit)
                }
                guard isCurrentDetail(item, generation: generation) else { break }
                if let winRows = timelineWindow.items {
                    coordinatorRef?.updateRowsPreservingAnchor(winRows, preferAutoFollowBottom: false)
                }
                guard timelineWindow.error == nil else { break }
                anyLoaded = true
                guard let coord = coordinatorRef else { break }
                let newHeight = coord.renderedContentHeight
                let newCount = coord.rows.count
                let needsMore = coord.isViewportUnderfilled
                    || ((newHeight - prevHeight) < 48 && coord.isNearTopForContinuation)
                    || (newCount <= prevRowCount && newCount <= 2)
                if !needsMore { break }
            }
            return anyLoaded
        }

        guard hasMoreOlder, !loadingOlder, loadOlderError == nil else { return false }
        var anyLoaded = false
        for step in 0 ..< 5 {
            guard isCurrentDetail(item, generation: generation), hasMoreOlder, !loadingOlder, loadOlderError == nil else { break }
            let prevHeight = coordinatorRef?.renderedContentHeight ?? 0
            let prevRowCount = rows.count
            let limitOverride: Int? = (step > 0 || prevRowCount <= 2) ? 500 : nil
            let loaded = await loadSingleOlderPage(for: item, generation: generation, limitOverride: limitOverride, preferBottom: false)
            if !loaded { break }
            anyLoaded = true
            guard let coord = coordinatorRef else { break }
            let newHeight = coord.renderedContentHeight
            let needsMore = coord.isViewportUnderfilled
                || ((newHeight - prevHeight) < 48 && coord.isNearTopForContinuation)
                || (rows.count <= prevRowCount && rows.count <= 2)
            if !needsMore { break }
        }
        return anyLoaded
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
                let nextRows = ConversationRowBuilder.buildGrokRows(events: slot.events, fallbackTitle: item.title)
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
            let ack: StartedJobResponse = try await store.rpc("threads.rename", args: RenameArgs(threadId: sessionKey, name: newTitle))
            store.trackStartedJob(ack.jobId)
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
            let ack: StartedJobResponse = try await store.rpc(cmd, args: Args(threadId: sessionKey))
            store.trackStartedJob(ack.jobId)
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
        claudeOwnershipError = nil
        claudeOwnershipLoading = true
        defer { claudeOwnershipLoading = false }
        struct Args: Encodable { let sessionKey: String }
        do {
            let diag: ClaudeOwnershipDiagnostics = try await store.rpc(
                "claude.ownershipDiagnostics",
                args: Args(sessionKey: sessionKey),
                localOnly: true
            )
            guard store.revision == capturedRevision else { return }
            claudeOwnershipData = diag
        } catch {
            guard store.revision == capturedRevision else { return }
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
        struct Wrap: Encodable { let request: SessionBatchRequest }

        let targetStore = store
        do {
            let preview = try await SessionBatchChunkExecutor.preview(
                provider: provider,
                operation: operation,
                sessionKeys: keys
            ) { req in
                try await targetStore.rpc("sessions.bulkPreview", args: Wrap(request: req), at: capturedRevision)
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
        struct Wrap: Encodable { let request: SessionBatchExecuteRequest }

        let targetStore = store
        let outcome = await SessionBatchChunkExecutor.execute(preview: preview) { req in
            try await targetStore.rpc("sessions.bulkExecute", args: Wrap(request: req), at: capturedRevision)
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
        guard let item = selectedItem, timelineWindow.isActive else { return }
        await timelineWindow.loadNewer(latestTailId: latestNativeTailId ?? rows.last?.timelineId) { cursor in
            try await readTimelineWindowPage(for: item, cursor: cursor)
        }
        if let winRows = timelineWindow.items {
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
                items: ConversationRowBuilder.buildGrokRows(events: rawPage.items, fallbackTitle: item.title),
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
        var item = UnifiedSessionSummaryItem(
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
        if !isArchived, store.target == .local,
           let live = CodexDesktopStore.liveView(revision: store.revision, threadId: s.id),
           live.connected
        {
            item = projectCodexDesktopLiveView(live, onto: item)
        }
        return item
    }

    static func projectCodexDesktopLiveView(
        _ live: DesktopView,
        onto base: UnifiedSessionSummaryItem
    ) -> UnifiedSessionSummaryItem {
        guard !base.archived, live.connected else { return base }
        var item = base
        if !live.requests.isEmpty {
            item.running = false
            item.statusText = "等待处理"
            item.statusChipBadge = "等待处理"
            item.statusChipTone = .warning
        } else if live.runtime == "active" || live.activeTurnId != nil {
            item.running = true
            item.statusText = "运行中"
            item.statusChipBadge = "运行中"
            item.statusChipTone = .info
        } else if item.running {
            item.running = false
            item.statusText = "最近"
            item.statusChipBadge = "最近"
            item.statusChipTone = .success
        }
        return item
    }

    func applyLiveCodexDesktopOverlay() {
        guard provider == .codex, store.target == .local, store.revision == sourceRevision else { return }
        var changed = false
        for idx in activeItems.indices {
            let key = activeItems[idx].sessionKey
            if let live = CodexDesktopStore.liveView(revision: store.revision, threadId: key), live.connected {
                let updated = Self.projectCodexDesktopLiveView(live, onto: activeItems[idx])
                if updated != activeItems[idx] {
                    activeItems[idx] = updated
                    changed = true
                }
            }
        }
        if changed {
            objectWillChange.send()
        }
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
