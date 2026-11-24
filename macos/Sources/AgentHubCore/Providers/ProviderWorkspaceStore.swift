import AppKit
import Combine
import SwiftUI

@MainActor
final class ProviderWorkspaceStore: ObservableObject {
    let store: RuntimeConnectionStore
    let workspace: WorkspaceId
    var isCompactMobile: Bool
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
    @Published var activeItems: [UnifiedSessionSummaryItem] = []
    @Published var archivedItems: [UnifiedSessionSummaryItem] = []
    @Published var archivedOpen: Bool = false
    @Published var selectedKey: String?
    @Published var mobileDetailOpen: Bool = false

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
    @Published var claudeDeletePreview: ClaudeDeletePreview?
    @Published var claudeDeleteError: String?
    @Published var claudeDeleteResiduals: [String] = []
    @Published var grokDeletePreview: GrokDeletePreview?
    @Published var grokDeleteError: String?
    @Published var grokDeleteResiduals: [String] = []

    @Published var headerInlineRename: Bool = false
    @Published var claudeOwnershipKey: String?
    @Published var claudeOwnershipData: ClaudeOwnershipDiagnostics?
    @Published var claudeOwnershipError: String?
    @Published var claudeOwnershipLoading: Bool = false
    @Published var feedbackMessage: String?
    @Published var errorMessage: String?

    init(store: RuntimeConnectionStore, workspace: WorkspaceId, isCompactMobile: Bool) {
        self.store = store
        self.workspace = workspace
        self.isCompactMobile = isCompactMobile
        self.sourceRevision = store.revision
        for changes in [threadStore.objectWillChange, claudeHistoryStore.objectWillChange,
                        grokHistoryStore.objectWillChange, providerHistoryStore.objectWillChange,
                        timelineWindow.objectWillChange, motionObserver.objectWillChange] {
            changes.sink { [weak self] in self?.objectWillChange.send() }.store(in: &observations)
        }
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

    var allVisibleItems: [UnifiedSessionSummaryItem] {
        activeItems + (archivedOpen ? archivedItems : [])
    }

    var selectedItem: UnifiedSessionSummaryItem? {
        if let key = selectedKey, let match = allVisibleItems.first(where: { $0.sessionKey == key }) {
            return match
        }
        if let searchSelection, searchSelection.sessionKey == selectedKey { return searchSelection }
        return isCompactMobile ? nil : activeItems.first
    }

    var displayedRows: [ConversationRowItem] {
        timelineWindow.items ?? rows
    }

    func syncPartitionedSelection() {
        let baseScope = "\(store.revision):\(workspace.rawValue):\(query)"
        partitionedSelection.sync(
            activeScope: "\(baseScope)#active",
            activeVisible: activeItems.map(\.sessionKey),
            archivedScope: "\(baseScope)#archived",
            archivedVisible: archivedItems.map(\.sessionKey),
            archivedOpen: supportsArchive && archivedOpen
        )
    }

    func selectAllVisible() {
        _ = partitionedSelection.selectAll(
            activeVisible: activeItems.map(\.sessionKey),
            archivedVisible: archivedItems.map(\.sessionKey),
            supportsArchive: supportsArchive,
            archivedOpen: archivedOpen
        )
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
            case .codex:
                struct ListArgs: Encodable {
                    let archived: Bool
                    let q: String?
                    let limit: Int
                }
                let active: [ThreadSummary] = try await store.rpc(
                    "threads.list",
                    args: ListArgs(archived: false, q: query.isEmpty ? nil : query, limit: 120)
                )
                guard isCurrent() else { return }
                activeItems = active.map { mapCodexSummary($0, archived: false) }
                if archivedOpen {
                    let arch: [ThreadSummary] = try await store.rpc(
                        "threads.list",
                        args: ListArgs(archived: true, q: query.isEmpty ? nil : query, limit: 120)
                    )
                    guard isCurrent() else { return }
                    archivedItems = arch.map { mapCodexSummary($0, archived: true) }
                }

            case .claudeCode:
                struct ClaudeListArgs: Encodable { let q: String?; let limit: Int }
                let list: [ClaudeSessionSummary] = try await store.rpc(
                    "claude.list",
                    args: ClaudeListArgs(q: query.isEmpty ? nil : query, limit: 100)
                )
                guard isCurrent() else { return }
                activeItems = list.map(mapClaudeSummary)

            case .grok:
                struct GrokListArgs: Encodable { let q: String?; let limit: Int }
                let list: [GrokSessionSummary] = try await store.rpc(
                    "grok.list",
                    args: GrokListArgs(q: query.isEmpty ? nil : query, limit: 100)
                )
                guard isCurrent() else { return }
                activeItems = list.map(mapGrokSummary)

            case .antigravity, .dsh:
                let prefix = (provider == .antigravity) ? "antigravity" : "dsh"
                struct ProvListWrap: Encodable {
                    struct Inner: Encodable { let q: String?; let archived: Bool; let limit: Int }
                    let request: Inner
                }
                let active: [ProviderSessionSummary] = try await store.rpc(
                    "\(prefix).list",
                    args: ProvListWrap(request: .init(q: query.isEmpty ? nil : query, archived: false, limit: 100))
                )
                guard isCurrent() else { return }
                activeItems = active.map(mapProviderSummary)
                if archivedOpen {
                    let arch: [ProviderSessionSummary] = try await store.rpc(
                        "\(prefix).list",
                        args: ProvListWrap(request: .init(q: query.isEmpty ? nil : query, archived: true, limit: 100))
                    )
                    guard isCurrent() else { return }
                    archivedItems = arch.map(mapProviderSummary)
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
                activeItems = list.map(mapGrokBotSummary)
            }
            syncPartitionedSelection()
            errorMessage = nil
        } catch {
            guard isCurrent() else { return }
            errorMessage = error.localizedDescription
        }
    }

    func scheduleDebouncedCodexTailRefresh() {
        codexTailRefreshTask?.cancel()
        codexTailRefreshTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 350_000_000)
            guard let self, !Task.isCancelled, provider == .codex, let item = selectedItem else { return }
            await fetchSessionDetail(for: item)
        }
    }

    func isCurrentDetail(_ item: UnifiedSessionSummaryItem) -> Bool {
        !Task.isCancelled && store.revision == sourceRevision && selectedItem?.sessionKey == item.sessionKey
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
    }

    func fetchSessionDetail(for item: UnifiedSessionSummaryItem) async {
        guard isCurrentDetail(item) else { return }
        let historyScope = "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)"
        if detailLoadedForKey != item.sessionKey {
            detailLoading = true
        }
        defer { if isCurrentDetail(item) { detailLoading = false } }
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
                guard isCurrentDetail(item) else { return }
                syncSidebarItem(mapCodexSummary(detail.summary, archived: detail.summary.status == "Archived"))
                let slot = threadStore.applyDetail(detail)
                incomingHistory = detail.history
                if subagents != slot.subagents { subagents = slot.subagents }
                if hasMoreOlder != slot.hasMoreBlocks { hasMoreOlder = slot.hasMoreBlocks }
                if beforeCursor != slot.beforeCursor { beforeCursor = slot.beforeCursor }
                let nextRows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: item.title,
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

            case .claudeCode:
                struct Req: Encodable { let sessionKey: String; let limit: Int }
                let detail: ClaudeSessionDetail = try await store.rpc(
                    "claude.detail",
                    args: Req(sessionKey: item.sessionKey, limit: 160)
                )
                guard isCurrentDetail(item) else { return }
                syncSidebarItem(mapClaudeSummary(detail.summary))
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
                incomingHistory = detail.history
                if subagents != slot.subagents { subagents = slot.subagents }
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

            case .grok:
                struct Req: Encodable { let id: String; let limit: Int }
                let detail: GrokSessionDetail = try await store.rpc(
                    "grok.detail",
                    args: Req(id: item.sessionKey, limit: 160)
                )
                guard isCurrentDetail(item) else { return }
                let mappedGrok = mapGrokSummary(detail.summary)
                syncSidebarItem(mappedGrok)
                let slot = grokHistoryStore.applyHeadPoll(
                    key: historyScope,
                    headEvents: detail.events,
                    history: detail.history,
                    totalEvents: detail.totalEvents,
                    beforeCursor: detail.beforeCursor,
                    hasMore: detail.hasMore ?? false,
                    timeline: detail.timeline
                )
                incomingHistory = detail.history
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

            case .antigravity, .dsh, .grokbot:
                let cmd = provider == .antigravity ? "antigravity.detail" : (provider == .dsh ? "dsh.detail" : "grokbot.detail")
                struct Wrap: Encodable { let request: ProviderDetailRequest }
                let detail: ProviderSessionDetail = try await store.rpc(
                    cmd,
                    args: Wrap(request: ProviderDetailRequest(sessionKey: item.sessionKey, limit: 160)),
                    localOnly: provider == .grokbot
                )
                guard isCurrentDetail(item) else { return }
                let mappedSummary = provider == .grokbot ? mapGrokBotSummary(detail.summary) : mapProviderSummary(detail.summary)
                syncSidebarItem(mappedSummary)
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
                incomingHistory = detail.history
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
            }

            currentDetailHistory = incomingHistory
            detailLoadedForKey = item.sessionKey

            if timelineWindow.isActive {
                await timelineWindow.reconcilePollHistory(incoming: incomingHistory) { cursor in
                    try await readTimelineWindowPage(for: item, cursor: cursor)
                }
            }

            if let pending = pendingSearch, pending.sessionKey == item.sessionKey, isCurrentDetail(item) {
                pendingSearch = nil
                await jumpAndRevealPositionKey(pending.positionKey)
            }
        } catch {
            guard isCurrentDetail(item) else { return }
            errorMessage = error.localizedDescription
        }
    }

    @discardableResult
    func loadOlderPageIfNeeded() async -> Bool {
        guard let item = selectedItem else { return false }
        if timelineWindow.isActive {
            guard timelineWindow.hasMoreOlder, !timelineWindow.busy else { return false }
            await timelineWindow.loadOlder { cursor in
                try await readTimelineWindowPage(for: item, cursor: cursor)
            }
            if let winRows = timelineWindow.items {
                coordinatorRef?.updateRowsPreservingAnchor(winRows, preferAutoFollowBottom: false)
            }
            return timelineWindow.error == nil
        }

        guard hasMoreOlder, !loadingOlder, loadOlderError == nil, let cursor = beforeCursor else { return false }
        loadingOlder = true
        defer { if isCurrentDetail(item) { loadingOlder = false } }
        let historyScope = "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)"

        do {
            switch provider {
            case .codex:
                struct BlocksArgs: Encodable {
                    struct Opts: Encodable { let limit: Int; let before: String }
                    let id: String
                    let options: Opts
                }
                let page: ThreadBlockPage = try await store.rpc(
                    "threads.blocks",
                    args: BlocksArgs(id: item.sessionKey, options: .init(limit: 120, before: cursor))
                )
                guard isCurrentDetail(item), beforeCursor == cursor else { return false }
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
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: false)
                rows = nextRows
                return advanced || !page.blocks.isEmpty

            case .claudeCode:
                struct Req: Encodable { let sessionKey: String; let limit: Int; let before: String }
                let page: ClaudeSessionDetail = try await store.rpc(
                    "claude.detail",
                    args: Req(sessionKey: item.sessionKey, limit: 160, before: cursor)
                )
                guard isCurrentDetail(item), beforeCursor == cursor else { return false }
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
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: false)
                rows = nextRows
                return !page.events.isEmpty

            case .grok:
                struct Req: Encodable { let id: String; let limit: Int; let before: String }
                let page: GrokSessionDetail = try await store.rpc(
                    "grok.detail",
                    args: Req(id: item.sessionKey, limit: 160, before: cursor)
                )
                guard isCurrentDetail(item), beforeCursor == cursor else { return false }
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
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: false)
                rows = nextRows
                return !page.events.isEmpty

            case .antigravity, .dsh, .grokbot:
                let cmd = provider == .antigravity ? "antigravity.detail" : (provider == .dsh ? "dsh.detail" : "grokbot.detail")
                struct Wrap: Encodable { let request: ProviderDetailRequest }
                let page: ProviderSessionDetail = try await store.rpc(
                    cmd,
                    args: Wrap(request: ProviderDetailRequest(sessionKey: item.sessionKey, limit: 160, before: cursor)),
                    localOnly: provider == .grokbot
                )
                guard isCurrentDetail(item), beforeCursor == cursor else { return false }
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
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: false)
                rows = nextRows
                return !page.events.isEmpty
            }
        } catch {
            guard isCurrentDetail(item) else { return false }
            loadOlderError = error.localizedDescription
            return false
        }
    }

    func performRename(item: UnifiedSessionSummaryItem, newTitle: String) async throws {
        guard store.revision == sourceRevision else { throw CancellationError() }
        switch provider {
        case .codex:
            struct RenameArgs: Encodable { let threadId: String; let name: String }
            let ack: StartedJobResponse = try await store.rpc("threads.rename", args: RenameArgs(threadId: item.sessionKey, name: newTitle))
            store.trackStartedJob(ack.jobId)
            threadStore.recordOptimisticTitle(threadId: item.sessionKey, title: newTitle)
        case .claudeCode:
            struct Args: Encodable { let sessionKey: String; let title: String }
            let _: ClaudeSessionSummary = try await store.rpc("claude.rename", args: Args(sessionKey: item.sessionKey, title: newTitle))
        case .grok:
            struct Args: Encodable { let id: String; let title: String }
            let _: GrokSessionSummary = try await store.rpc("grok.rename", args: Args(id: item.sessionKey, title: newTitle))
        case .antigravity, .dsh:
            let cmd = provider == .antigravity ? "antigravity.rename" : "dsh.rename"
            struct Wrap: Encodable {
                struct Inner: Encodable { let sessionKey: String; let title: String }
                let request: Inner
            }
            let _: ProviderSessionSummary = try await store.rpc(cmd, args: Wrap(request: .init(sessionKey: item.sessionKey, title: newTitle)))
        case .grokbot:
            break
        }
        await fetchSessionList()
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
            if provider == .codex {
                struct Args: Encodable { let threadId: String }
                let cmd = item.archived ? "threads.restore" : "threads.archive"
                let ack: StartedJobResponse = try await store.rpc(cmd, args: Args(threadId: item.sessionKey))
                store.trackStartedJob(ack.jobId)
            } else {
                let prefix = provider == .antigravity ? "antigravity" : "dsh"
                let action = item.archived ? "restore" : "archive"
                struct Wrap: Encodable {
                    struct Inner: Encodable { let sessionKey: String }
                    let request: Inner
                }
                let _: ProviderSessionSummary = try await store.rpc("\(prefix).\(action)", args: Wrap(request: .init(sessionKey: item.sessionKey)))
            }
            feedbackMessage = item.archived ? "已恢复线程" : "已归档线程"
            if !item.archived && selectedKey == item.sessionKey && !archivedOpen {
                selectedKey = nextFallbackKey
                headerInlineRename = false
            }
            await fetchSessionList()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func triggerSingleDelete(_ item: UnifiedSessionSummaryItem) async {
        guard store.revision == sourceRevision else { return }
        if provider == .claudeCode {
            claudeDeleteError = nil
            claudeDeleteResiduals = []
            struct Args: Encodable { let sessionKey: String }
            do {
                claudeDeletePreview = try await store.rpc("claude.deletePreview", args: Args(sessionKey: item.sessionKey))
            } catch {
                errorMessage = error.localizedDescription
            }
        } else if provider == .grok {
            grokDeleteError = nil
            grokDeleteResiduals = []
            struct Args: Encodable { let id: String }
            do {
                grokDeletePreview = try await store.rpc("grok.deletePreview", args: Args(id: item.sessionKey))
            } catch {
                errorMessage = error.localizedDescription
            }
        } else {
            let targetPartition: SessionSelectionPartition = item.archived ? .archived : .active
            await prepareBatch(operation: .delete, keys: [item.sessionKey], partition: targetPartition)
        }
    }

    func confirmClaudeDelete(_ preview: ClaudeDeletePreview) async {
        guard store.revision == sourceRevision else { return }
        batchBusy = true
        claudeDeleteError = nil
        defer { batchBusy = false }
        struct Wrap: Encodable {
            struct Inner: Encodable { let sessionKey: String; let confirmed: Bool; let fingerprint: String }
            let request: Inner
        }
        do {
            let result: ClaudeDeleteResult = try await store.rpc(
                "claude.deleteExecute",
                args: Wrap(request: .init(sessionKey: preview.sessionKey, confirmed: true, fingerprint: preview.fingerprint))
            )
            if let residuals = result.residuals, !residuals.isEmpty {
                claudeDeleteResiduals = residuals
                claudeDeleteError = "部分任务文件未能清理"
            } else {
                claudeDeletePreview = nil
                claudeDeleteResiduals = []
                activeItems.removeAll { $0.sessionKey == preview.sessionKey }
                if selectedKey == preview.sessionKey { selectedKey = nil }
                await fetchSessionList()
            }
        } catch {
            claudeDeleteError = error.localizedDescription
        }
    }

    func confirmGrokDelete(_ preview: GrokDeletePreview) async {
        guard store.revision == sourceRevision else { return }
        batchBusy = true
        grokDeleteError = nil
        defer { batchBusy = false }
        struct Wrap: Encodable {
            struct Inner: Encodable { let id: String; let confirmed: Bool; let fingerprint: String }
            let request: Inner
        }
        do {
            let result: GrokDeleteResult = try await store.rpc(
                "grok.deleteExecute",
                args: Wrap(request: .init(id: preview.id, confirmed: true, fingerprint: preview.fingerprint))
            )
            if let residuals = result.residuals, !residuals.isEmpty {
                grokDeleteResiduals = residuals
                grokDeleteError = "部分任务文件未能清理"
            } else {
                grokDeletePreview = nil
                grokDeleteResiduals = []
                activeItems.removeAll { $0.sessionKey == preview.id }
                if selectedKey == preview.id { selectedKey = nil }
                await fetchSessionList()
            }
        } catch {
            grokDeleteError = error.localizedDescription
        }
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
            }
        }
        batchExecuteError = outcome.errorMessage
        await fetchSessionList()
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
        await timelineWindow.loadNewer(latestTailId: rows.last?.timelineId) { cursor in
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
        cursor: String
    ) async throws -> TimelineWindowPage<ConversationRowItem> {
        switch provider {
        case .codex:
            struct DetailArgs: Encodable {
                struct Opts: Encodable { let limit: Int; let before: String }
                let id: String
                let options: Opts
            }
            guard let page: ThreadDetail = try await store.rpc(
                "threads.detail",
                args: DetailArgs(id: item.sessionKey, options: .init(limit: 120, before: cursor))
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
                history: rawPage.history
            )

        case .claudeCode:
            struct Req: Encodable { let sessionKey: String; let limit: Int; let before: String }
            let page: ClaudeSessionDetail = try await store.rpc(
                "claude.detail",
                args: Req(sessionKey: item.sessionKey, limit: 160, before: cursor)
            )
            let rawPage = TimelineWindowPage<ClaudeHistoryEvent>.from(claudeDetail: page)
            return TimelineWindowPage<ConversationRowItem>(
                items: ConversationRowBuilder.buildClaudeRows(events: rawPage.items),
                before: rawPage.before,
                after: rawPage.after,
                hasMore: rawPage.hasMore,
                history: rawPage.history
            )

        case .grok:
            struct Req: Encodable { let id: String; let limit: Int; let before: String }
            let page: GrokSessionDetail = try await store.rpc(
                "grok.detail",
                args: Req(id: item.sessionKey, limit: 160, before: cursor)
            )
            let rawPage = TimelineWindowPage<GrokHistoryEvent>.from(grokDetail: page)
            return TimelineWindowPage<ConversationRowItem>(
                items: ConversationRowBuilder.buildGrokRows(events: rawPage.items, fallbackTitle: item.title),
                before: rawPage.before,
                after: rawPage.after,
                hasMore: rawPage.hasMore,
                history: rawPage.history
            )

        case .antigravity, .dsh, .grokbot:
            let cmd = provider == .antigravity ? "antigravity.detail" : (provider == .dsh ? "dsh.detail" : "grokbot.detail")
            struct Wrap: Encodable { let request: ProviderDetailRequest }
            let page: ProviderSessionDetail = try await store.rpc(
                cmd,
                args: Wrap(request: ProviderDetailRequest(sessionKey: item.sessionKey, limit: 160, before: cursor)),
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
                history: rawPage.history
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
            mobileDetailOpen = true
            if supportsArchive && !activeItems.contains(where: { $0.sessionKey == result.sessionKey }) {
                archivedOpen = true
            }
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
        if let updatedAt, !updatedAt.isEmpty {
            let iso = ISO8601DateFormatter()
            iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            let fallbackIso = ISO8601DateFormatter()
            if let date = iso.date(from: updatedAt) ?? fallbackIso.date(from: updatedAt) {
                let df = DateFormatter()
                df.dateFormat = "yyyy/M/d HH:mm:ss"
                return df.string(from: date)
            }
        }
        return "最近"
    }

    func mapClaudeSummary(_ item: ClaudeSessionSummary) -> UnifiedSessionSummaryItem {
        let stLabel = Self.providerSessionStatusText(status: item.status, updatedAt: item.updatedAt, archived: false)
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
            path: item.path,
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
        UnifiedSessionSummaryItem(
            sessionKey: item.id,
            nativeId: item.id,
            title: item.title.isEmpty ? item.id : item.title,
            cwd: item.cwd,
            path: HistoryAndResumeHelpers.grokHandoffPath(item),
            statusText: Self.providerSessionStatusText(status: item.status, updatedAt: item.updatedAt, archived: false),
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
        let st = item.status == "authorization" ? "需要授权"
            : (item.status == "reply_needed" ? "需要回复" : (!item.path.isEmpty ? "近期缓存" : "未缓存"))
        return UnifiedSessionSummaryItem(
            sessionKey: item.sessionKey,
            nativeId: item.id,
            title: item.title.isEmpty ? item.id : item.title,
            cwd: item.cwd,
            path: item.path,
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
        let displayTitle = threadStore.effectiveTitle(
            threadId: s.id,
            serverTitle: s.title.isEmpty ? s.id : s.title
        )
        let running = (s.status == "Running" || s.activeTurnId != nil)
        let isArchived = archived || s.status == "Archived"
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
            path: s.rolloutPath ?? "",
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
            path: s.path,
            statusText: Self.providerSessionStatusText(status: s.status, updatedAt: s.updatedAt, archived: s.archived),
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
