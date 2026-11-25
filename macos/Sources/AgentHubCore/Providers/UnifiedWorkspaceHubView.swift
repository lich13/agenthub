import AppKit
import Combine
import SwiftUI

// MARK: - Cross-Provider Workspace Hub Item & Aggregated Store

public struct CrossProviderSessionItem: Equatable, Sendable, Identifiable {
    public var provider: SessionProvider
    public var item: UnifiedSessionSummaryItem

    public var compositeKey: String {
        "\(provider.rawValue)::\(item.sessionKey)"
    }

    public var id: String { compositeKey }

    public init(provider: SessionProvider, item: UnifiedSessionSummaryItem) {
        self.provider = provider
        self.item = item
    }
}

extension CrossProviderSessionItem: ProjectWorkspaceGroupableItem {
    public var workspaceGroupItemId: String { compositeKey }
    public var workspaceSummaryItem: UnifiedSessionSummaryItem { item }
}

public struct CrossProviderBatchPlan: Equatable, Sendable {
    public var operation: SessionOperation
    public var previewsByProvider: [(provider: SessionProvider, preview: SessionBatchPreview)]
    public var mergedPreview: SessionBatchPreview

    public static func == (lhs: CrossProviderBatchPlan, rhs: CrossProviderBatchPlan) -> Bool {
        lhs.operation == rhs.operation
            && lhs.mergedPreview == rhs.mergedPreview
            && lhs.previewsByProvider.count == rhs.previewsByProvider.count
            && zip(lhs.previewsByProvider, rhs.previewsByProvider).allSatisfy {
                $0.provider == $1.provider && $0.preview == $1.preview
            }
    }
}

@MainActor
public final class UnifiedWorkspaceHubStore: ObservableObject {
    public static let codingProviders: [SessionProvider] = [
        .codex, .claudeCode, .grok, .antigravity, .dsh,
    ]

    public let store: RuntimeConnectionStore
    public let sourceRevision: UInt64
    public let codexThreadStore = ThreadMessageStore()
    private var observations = Set<AnyCancellable>()

    @Published public var query: String = ""
    @Published public var workspaceFilter: ProjectWorkspaceFilter = .all {
        didSet {
            if oldValue != workspaceFilter { syncPartitionedSelection() }
        }
    }
    @Published public var collapsedWorkspaceKeys: Set<ProjectWorkspaceKey> = [] {
        didSet {
            if oldValue != collapsedWorkspaceKeys { syncPartitionedSelection() }
        }
    }
    @Published public var collapsedArchivedWorkspaceKeys: Set<ProjectWorkspaceKey> = [] {
        didSet {
            if oldValue != collapsedArchivedWorkspaceKeys { syncPartitionedSelection() }
        }
    }
    @Published public var activeItems: [CrossProviderSessionItem] = []
    @Published public var archivedItems: [CrossProviderSessionItem] = []
    @Published public var archivedOpen: Bool = false
    @Published public var selectedCompositeKey: String?
    @Published public var renameRequestSeqByKey: [String: Int] = [:]
    @Published public var providerErrors: [String] = []
    @Published public var feedbackMessage: String?

    @Published public var partitionedSelection = PartitionedSessionSelectionState(activeScope: "hub-init")
    @Published public var batchTargetPartition: SessionSelectionPartition = .active
    @Published public var batchBusy: Bool = false
    @Published public var batchPrepareError: String?
    @Published public var batchExecuteError: String?
    @Published public var batchPlan: CrossProviderBatchPlan?
    @Published public var batchResult: SessionBatchResult?

    public init(store: RuntimeConnectionStore) {
        self.store = store
        self.sourceRevision = store.revision
        codexThreadStore.objectWillChange
            .sink { [weak self] in self?.objectWillChange.send() }
            .store(in: &observations)
    }

    public var filteredActiveItems: [CrossProviderSessionItem] {
        ProjectWorkspaceEngine.filterItems(activeItems, by: workspaceFilter)
    }

    public var filteredArchivedItems: [CrossProviderSessionItem] {
        ProjectWorkspaceEngine.filterItems(archivedItems, by: workspaceFilter)
    }

    public var activeWorkspaceGroups: [ProjectWorkspaceGroup<CrossProviderSessionItem>] {
        ProjectWorkspaceEngine.buildGroups(items: filteredActiveItems)
    }

    public var archivedWorkspaceGroups: [ProjectWorkspaceGroup<CrossProviderSessionItem>] {
        ProjectWorkspaceEngine.buildGroups(items: filteredArchivedItems)
    }

    public var availableWorkspaceGroups: [ProjectWorkspaceGroup<CrossProviderSessionItem>] {
        ProjectWorkspaceEngine.buildGroups(items: activeItems + (archivedOpen ? archivedItems : []))
    }

    public var visualOrderedActiveKeys: [String] {
        ProjectWorkspaceEngine.visualOrderedKeys(
            groups: activeWorkspaceGroups,
            collapsedKeys: collapsedWorkspaceKeys
        )
    }

    public var visualOrderedArchivedKeys: [String] {
        ProjectWorkspaceEngine.visualOrderedKeys(
            groups: archivedWorkspaceGroups,
            collapsedKeys: collapsedArchivedWorkspaceKeys
        )
    }

    public var selectedEntry: CrossProviderSessionItem? {
        if let key = selectedCompositeKey {
            if let found = (activeItems + archivedItems).first(where: { $0.compositeKey == key }) {
                return found
            }
            if let searchFallbackEntry, searchFallbackEntry.compositeKey == key {
                return searchFallbackEntry
            }
        }
        if workspaceFilter != .all || !query.isEmpty {
            return activeWorkspaceGroups.first?.items.first ?? filteredActiveItems.first
        }
        return activeWorkspaceGroups.first?.items.first ?? filteredActiveItems.first ?? activeItems.first
    }

    @Published public var searchFallbackEntry: CrossProviderSessionItem?

    public func handleStageSessionSelected(provider: SessionProvider, sessionKey: String?) {
        guard let sessionKey, !sessionKey.isEmpty else {
            selectedCompositeKey = nil
            searchFallbackEntry = nil
            return
        }
        let nextComposite = "\(provider.rawValue)::\(sessionKey)"
        selectedCompositeKey = nextComposite
        if let activeHit = activeItems.first(where: { $0.compositeKey == nextComposite }) {
            searchFallbackEntry = nil
            if workspaceFilter != .all,
               !ProjectWorkspaceEngine.matchesFilter(
                   workspaceFilter,
                   cwd: activeHit.item.cwd,
                   workspaceRoot: activeHit.item.workspaceRoot
               )
            {
                workspaceFilter = .all
            }
        } else if let archHit = archivedItems.first(where: { $0.compositeKey == nextComposite }) {
            searchFallbackEntry = nil
            if !archivedOpen { archivedOpen = true }
            if workspaceFilter != .all,
               !ProjectWorkspaceEngine.matchesFilter(
                   workspaceFilter,
                   cwd: archHit.item.cwd,
                   workspaceRoot: archHit.item.workspaceRoot
               )
            {
                workspaceFilter = .all
            }
        } else {
            let placeholder = UnifiedSessionSummaryItem(
                sessionKey: sessionKey,
                nativeId: sessionKey,
                title: sessionKey,
                cwd: "",
                path: "",
                statusText: "最近",
                running: false,
                archived: false,
                canRename: false,
                canArchive: false,
                canDelete: false
            )
            searchFallbackEntry = CrossProviderSessionItem(provider: provider, item: placeholder)
            if !archivedOpen && (provider == .codex || provider == .antigravity || provider == .dsh) {
                archivedOpen = true
            }
        }
    }

    public var activeSelectionPartition: SessionSelectionPartition {
        if archivedOpen && (partitionedSelection.archived.selecting || !partitionedSelection.archived.keys.isEmpty) {
            return .archived
        }
        return .active
    }

    public func batchOperations(for partition: SessionSelectionPartition) -> [SessionOperation] {
        let selKeys = Set(partitionedSelection.state(for: partition).keys)
        let pool = (partition == .archived) ? filteredArchivedItems : filteredActiveItems
        let selectedProviders = Set(pool.filter { selKeys.contains($0.compositeKey) }.map(\.provider))
        let allSupportArchive = !selectedProviders.isEmpty && selectedProviders.allSatisfy {
            $0 == .codex || $0 == .antigravity || $0 == .dsh
        }
        if partition == .archived {
            return [.restore, .delete]
        }
        return allSupportArchive ? [.archive, .delete] : [.delete]
    }

    public func syncPartitionedSelection() {
        let baseScope = "\(store.revision):workspaces:\(query):\(workspaceFilter.scopeToken)"
        partitionedSelection.sync(
            activeScope: "\(baseScope)#active",
            activeVisible: visualOrderedActiveKeys,
            archivedScope: "\(baseScope)#archived",
            archivedVisible: visualOrderedArchivedKeys,
            archivedOpen: archivedOpen
        )
    }

    public func selectAllVisible() {
        _ = partitionedSelection.selectAll(
            activeVisible: visualOrderedActiveKeys,
            archivedVisible: visualOrderedArchivedKeys,
            supportsArchive: true,
            archivedOpen: archivedOpen
        )
        clearBatchFeedback()
    }

    public func selectAllInWorkspaceGroup(keys: [String], partition: SessionSelectionPartition) {
        guard !keys.isEmpty else { return }
        var state = partitionedSelection.state(for: partition)
        state = SessionSelectionEngine.add(state, keys: keys)
        if state.anchor == nil { state.anchor = keys.first }
        partitionedSelection.setState(state, for: partition)
        clearBatchFeedback()
    }

    public func clearBatchFeedback() {
        batchPlan = nil
        batchResult = nil
        batchPrepareError = nil
        batchExecuteError = nil
    }

    public static func sortMergedItems(_ items: [CrossProviderSessionItem]) -> [CrossProviderSessionItem] {
        items.enumerated().sorted { lhs, rhs in
            let lMs = lhs.element.item.updatedAtMs
            let rMs = rhs.element.item.updatedAtMs
            if lMs != rMs { return lMs > rMs }
            return lhs.offset < rhs.offset
        }.map(\.element)
    }

    public func fetchAllSessions() async {
        let requestedQuery = query
        let requestedArchived = archivedOpen
        func isCurrent() -> Bool {
            !Task.isCancelled && store.revision == sourceRevision && query == requestedQuery && archivedOpen == requestedArchived
        }
        guard isCurrent() else { return }

        let targetStore = store
        let tStore = codexThreadStore
        let supportedProviders = Self.codingProviders.filter {
            ProviderWorkspaceStore.isCodingProviderSupported(provider: $0, store: targetStore)
        }

        let (mergedActive, activeErrs) = await Self.fetchConcurrently(
            providers: supportedProviders,
            store: targetStore,
            query: requestedQuery,
            archived: false,
            threadStore: tStore
        )
        guard isCurrent() else { return }
        activeItems = Self.sortMergedItems(mergedActive)

        var allErrs = activeErrs
        if requestedArchived {
            let archiveProviders = supportedProviders.filter {
                $0 == .codex || $0 == .antigravity || $0 == .dsh
            }
            let (mergedArch, archErrs) = await Self.fetchConcurrently(
                providers: archiveProviders,
                store: targetStore,
                query: requestedQuery,
                archived: true,
                threadStore: tStore
            )
            guard isCurrent() else { return }
            archivedItems = Self.sortMergedItems(mergedArch)
            allErrs.append(contentsOf: archErrs)
        }
        providerErrors = allErrs
        syncPartitionedSelection()
    }

    private struct ProviderFetchOutcome: Sendable {
        let index: Int
        let provider: SessionProvider
        let items: [UnifiedSessionSummaryItem]
        let errorMessage: String?
    }

    private static func fetchConcurrently(
        providers: [SessionProvider],
        store: RuntimeConnectionStore,
        query: String,
        archived: Bool,
        threadStore: ThreadMessageStore
    ) async -> ([CrossProviderSessionItem], [String]) {
        let tasks: [Task<ProviderFetchOutcome, Never>] = providers.enumerated().map { idx, prov in
            Task { @MainActor in
                do {
                    let list = try await ProviderWorkspaceStore.fetchProviderSummaries(
                        store: store,
                        provider: prov,
                        query: query,
                        archived: archived,
                        limit: 500,
                        threadStore: prov == .codex ? threadStore : nil
                    )
                    return ProviderFetchOutcome(index: idx, provider: prov, items: list, errorMessage: nil)
                } catch {
                    return ProviderFetchOutcome(
                        index: idx,
                        provider: prov,
                        items: [],
                        errorMessage: "\(prov.label)：\(error.localizedDescription)"
                    )
                }
            }
        }
        var buckets: [(Int, [CrossProviderSessionItem])] = []
        var errors: [(Int, String)] = []
        for task in tasks {
            if Task.isCancelled {
                tasks.forEach { $0.cancel() }
                break
            }
            let out = await task.value
            if let err = out.errorMessage {
                errors.append((out.index, err))
            } else {
                buckets.append((out.index, out.items.map { CrossProviderSessionItem(provider: out.provider, item: $0) }))
            }
        }
        buckets.sort { $0.0 < $1.0 }
        errors.sort { $0.0 < $1.0 }
        return (buckets.flatMap(\.1), errors.map(\.1))
    }

    public func performRename(entry: CrossProviderSessionItem, newTitle: String) async throws {
        guard store.revision == sourceRevision else { throw CancellationError() }
        try await ProviderWorkspaceStore.performRenameStatic(
            store: store,
            provider: entry.provider,
            sessionKey: entry.item.sessionKey,
            newTitle: newTitle,
            threadStore: entry.provider == .codex ? codexThreadStore : nil
        )
        await fetchAllSessions()
    }

    public func toggleSingleArchive(_ entry: CrossProviderSessionItem) async {
        guard store.revision == sourceRevision else { return }
        do {
            try await ProviderWorkspaceStore.performToggleArchiveStatic(
                store: store,
                provider: entry.provider,
                sessionKey: entry.item.sessionKey,
                currentlyArchived: entry.item.archived
            )
            feedbackMessage = entry.item.archived ? "已恢复线程" : "已归档线程"
            if !entry.item.archived && selectedCompositeKey == entry.compositeKey && !archivedOpen {
                selectedCompositeKey = nil
            }
            await fetchAllSessions()
        } catch {
            batchPrepareError = error.localizedDescription
        }
    }

    public func prepareBatch(
        operation: SessionOperation,
        compositeKeys: [String],
        partition: SessionSelectionPartition
    ) async {
        guard store.revision == sourceRevision, !compositeKeys.isEmpty, !batchBusy else { return }
        let capturedRevision = store.revision
        batchTargetPartition = partition
        batchBusy = true
        batchPrepareError = nil
        batchExecuteError = nil
        batchResult = nil
        batchPlan = nil
        defer { batchBusy = false }

        var keysByProvider: [SessionProvider: [String]] = [:]
        var providerOrder: [SessionProvider] = []
        for composite in compositeKeys {
            guard let sep = composite.range(of: "::") else { continue }
            let rawProv = String(composite[..<sep.lowerBound])
            let rawKey = String(composite[sep.upperBound...])
            guard let prov = SessionProvider(rawValue: rawProv), !rawKey.isEmpty else { continue }
            if keysByProvider[prov] == nil {
                providerOrder.append(prov)
                keysByProvider[prov] = []
            }
            keysByProvider[prov]?.append(rawKey)
        }
        guard !providerOrder.isEmpty else { return }

        let targetStore = store
        do {
            let previewTasks: [Task<(Int, SessionProvider, SessionBatchPreview), Error>] = providerOrder.enumerated().map { idx, prov in
                let keys = keysByProvider[prov] ?? []
                return Task { @MainActor in
                    let preview = try await SessionBatchChunkExecutor.preview(
                        provider: prov,
                        operation: operation,
                        sessionKeys: keys
                    ) { req in
                        try await ProviderWorkspaceStore.sendBulkPreview(
                            store: targetStore,
                            request: req,
                            at: capturedRevision
                        )
                    }
                    return (idx, prov, preview)
                }
            }
            var collected: [(Int, SessionProvider, SessionBatchPreview)] = []
            do {
                for task in previewTasks {
                    collected.append(try await task.value)
                }
            } catch {
                previewTasks.forEach { $0.cancel() }
                throw error
            }
            collected.sort { $0.0 < $1.0 }
            let perProvider = collected.map { ($0.1, $0.2) }
            guard store.revision == capturedRevision else { return }
            let mergedItems = perProvider.flatMap { prov, prev in
                prev.items.map { item -> SessionBatchItem in
                    var tagged = item
                    tagged.title = "[\(prov.label)] \(item.title)"
                    return tagged
                }
            }
            let merged = SessionBatchPreview(
                provider: perProvider.first?.0 ?? .codex,
                operation: operation,
                items: mergedItems
            )
            batchPlan = CrossProviderBatchPlan(
                operation: operation,
                previewsByProvider: perProvider,
                mergedPreview: merged
            )
        } catch {
            guard store.revision == capturedRevision else { return }
            batchPrepareError = error.localizedDescription
        }
    }

    public func executePreparedBatch() async {
        guard store.revision == sourceRevision, let plan = batchPlan, !batchBusy else { return }
        let capturedRevision = store.revision
        let targetPartition = batchTargetPartition
        batchPlan = nil
        batchBusy = true
        batchExecuteError = nil
        defer { batchBusy = false }

        let targetStore = store
        var mergedResultItems: [SessionBatchItemResult] = []
        var succeededComposites = Set<String>()
        var errorMessages: [String] = []

        var interrupted = false
        for (prov, provPreview) in plan.previewsByProvider {
            let allowedItems = provPreview.items.filter(\.allowed)
            if allowedItems.isEmpty {
                continue
            }
            if interrupted {
                for item in allowedItems {
                    mergedResultItems.append(
                        SessionBatchItemResult(
                            sessionKey: "\(prov.label)·\(item.sessionKey)",
                            status: "failed",
                            message: "前序请求中断，未执行"
                        )
                    )
                }
                continue
            }

            let outcome = await SessionBatchChunkExecutor.execute(preview: provPreview) { req in
                try await ProviderWorkspaceStore.sendBulkExecute(
                    store: targetStore,
                    request: req,
                    at: capturedRevision
                )
            }
            if let res = outcome.result {
                for var item in res.items {
                    if item.status == "succeeded" {
                        succeededComposites.insert("\(prov.rawValue)::\(item.sessionKey)")
                    }
                    item.sessionKey = "\(prov.label)·\(item.sessionKey)"
                    mergedResultItems.append(item)
                }
                for okKey in outcome.confirmedSucceededKeys {
                    succeededComposites.insert("\(prov.rawValue)::\(okKey)")
                }
            }
            if let err = outcome.errorMessage, !err.isEmpty {
                errorMessages.append("\(prov.label)：\(err)")
                interrupted = true
                if outcome.result == nil {
                    if mergedResultItems.isEmpty {
                        // DESIGN.md:80 — first request failure shows only error, no empty success stats
                        break
                    }
                    let size = (plan.operation == .delete)
                        ? max(1, allowedItems.count)
                        : SessionBatchChunkExecutor.archiveRestoreChunkSize
                    for (idx, item) in allowedItems.enumerated() {
                        mergedResultItems.append(
                            SessionBatchItemResult(
                                sessionKey: "\(prov.label)·\(item.sessionKey)",
                                status: "failed",
                                message: idx < size ? "请求中断，结果未确认" : "前序请求中断，未执行"
                            )
                        )
                    }
                }
            }
        }
        guard store.revision == capturedRevision else { return }

        if !mergedResultItems.isEmpty {
            batchResult = SessionBatchResult(items: mergedResultItems)
        }
        if !succeededComposites.isEmpty {
            var updatedState = partitionedSelection.state(for: targetPartition)
            updatedState = SessionSelectionEngine.applyBatchSucceeded(updatedState, succeededKeys: succeededComposites)
            partitionedSelection.setState(updatedState, for: targetPartition)
            if let sel = selectedCompositeKey, succeededComposites.contains(sel) {
                selectedCompositeKey = nil
            }
        }
        batchExecuteError = errorMessages.isEmpty ? nil : errorMessages.joined(separator: "；")
        await fetchAllSessions()
    }
}

// MARK: - Unified Cross-Provider Workspace Hub View

public struct UnifiedWorkspaceHubView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public let scheme: ColorSchemeKind

    @StateObject private var hubModel: UnifiedWorkspaceHubStore

    public init(
        store: RuntimeConnectionStore,
        scheme: ColorSchemeKind
    ) {
        _hubModel = StateObject(wrappedValue: UnifiedWorkspaceHubStore(store: store))
        self.store = store
        self.scheme = scheme
    }

    public init(
        hubModel: UnifiedWorkspaceHubStore,
        scheme: ColorSchemeKind
    ) {
        _hubModel = StateObject(wrappedValue: hubModel)
        self.store = hubModel.store
        self.scheme = scheme
    }

    public var body: some View {
        ZStack {
            GeometryReader { geo in
                HStack(spacing: 0) {
                    sidebarColumn
                        .frame(width: LayoutMetrics.sessionListWidth)
                        .frame(maxHeight: .infinity)
                        .background(palette.surface.color)
                        .overlay(alignment: .trailing) {
                            Rectangle().fill(palette.borderSubtle.color).frame(width: 1)
                        }

                    let stageWidth = max(320, geo.size.width - LayoutMetrics.sessionListWidth)
                    stageArea
                        .frame(width: stageWidth, height: geo.size.height)
                }
            }

            if let plan = hubModel.batchPlan {
                ConfirmDialogOverlay(busy: hubModel.batchBusy, onCancel: { hubModel.batchPlan = nil }) {
                    SessionBatchPreviewDialogContentView(
                        preview: plan.mergedPreview,
                        busy: hubModel.batchBusy,
                        onCancel: { hubModel.batchPlan = nil },
                        onConfirm: { Task { await hubModel.executePreparedBatch() } }
                    )
                }
            }
        }
        .background(
            WorkspaceKeyboardShortcutMonitorView(
                isModalOpen: store.hasAnyActiveModal || ModalPresentationRegistry.shared.hasActiveModal || hubModel.batchPlan != nil,
                onRenameSelected: {
                    let activePart = hubModel.activeSelectionPartition
                    guard !hubModel.partitionedSelection.state(for: activePart).selecting,
                          let entry = hubModel.selectedEntry
                    else { return false }
                    hubModel.renameRequestSeqByKey[entry.compositeKey, default: 0] += 1
                    return true
                },
                onEscapeClearSelection: {
                    let part = hubModel.activeSelectionPartition
                    guard hubModel.partitionedSelection.state(for: part).selecting else { return false }
                    hubModel.partitionedSelection.clear(partition: part)
                    return true
                },
                onBatchDelete: {
                    let part = hubModel.activeSelectionPartition
                    let sel = hubModel.partitionedSelection.state(for: part)
                    guard !hubModel.batchBusy, sel.selecting, !sel.keys.isEmpty else { return false }
                    Task { await hubModel.prepareBatch(operation: .delete, compositeKeys: sel.keys, partition: part) }
                    return true
                }
            )
            .frame(width: 0, height: 0)
        )
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSelectAllRequested)) { _ in
            guard !hubModel.batchBusy,
                  !store.hasAnyActiveModal,
                  !ModalPresentationRegistry.shared.hasActiveModal,
                  hubModel.batchPlan == nil
            else { return }
            hubModel.selectAllVisible()
        }
        .onChange(of: hubModel.archivedOpen) { isOpen in
            hubModel.partitionedSelection.setArchivedOpen(isOpen)
            if !isOpen && hubModel.batchTargetPartition == .archived {
                hubModel.clearBatchFeedback()
            }
        }
        .task(id: "\(store.revision):workspaces:\(hubModel.query):\(hubModel.archivedOpen)") {
            hubModel.syncPartitionedSelection()
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await hubModel.fetchAllSessions()
                }
                try? await Task.sleep(nanoseconds: 5_000_000_000)
            }
        }
    }

    private var sidebarColumn: some View {
        let activePart = hubModel.activeSelectionPartition
        let activeSel = hubModel.partitionedSelection.state(for: activePart)

        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 9) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(palette.accentSoft.color)
                        .frame(width: 24, height: 24)
                    LucideIcon(.folderGit2, size: 16)
                        .foregroundStyle(palette.accent.color)
                }

                Text("工作区")
                    .font(DesignFonts.body(15, weight: .bold))
                    .foregroundStyle(palette.textPrimary.color)
                    .lineLimit(1)

                Text("\(hubModel.filteredActiveItems.count)")
                    .font(DesignFonts.tabularBody(11, weight: .medium))
                    .foregroundStyle(palette.accent.color)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 1)
                    .frame(minWidth: 22)
                    .background(Capsule().fill(palette.accentSoft.color))
                    .overlay(Capsule().strokeBorder(palette.accentBorder.color, lineWidth: 1))

                Spacer(minLength: 4)

                HStack(spacing: 4) {
                    WorkspaceSidebarFilterMenuButton(
                        isFiltered: hubModel.workspaceFilter != .all,
                        makeActions: {
                            WorkspaceFilterMenuBuilder.buildActions(
                                currentFilter: hubModel.workspaceFilter,
                                groups: hubModel.availableWorkspaceGroups,
                                idPrefix: "hub-ws"
                            ) { nextFilter in
                                hubModel.workspaceFilter = nextFilter
                            }
                        }
                    )

                    Button {
                        Task { await hubModel.fetchAllSessions() }
                    } label: {
                        LucideIcon(.refreshCw, size: 15)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 28))
                    .accessibilityLabel("刷新工作区列表")
                }
            }
            .padding(.horizontal, 4)

            WorkspaceSidebarSearchField(placeholder: "搜索标题、ID 或工作目录", query: $hubModel.query)

            WorkspaceActiveFilterPillBanner(filter: hubModel.workspaceFilter) {
                hubModel.workspaceFilter = .all
            }

            if !hubModel.providerErrors.isEmpty {
                Text(hubModel.providerErrors.joined(separator: "；"))
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.errFg.color)
                    .lineLimit(2)
            }

            if let fb = hubModel.feedbackMessage, !fb.isEmpty {
                Text(fb)
                    .font(DesignFonts.body(11))
                    .foregroundStyle((fb.contains("无法") || fb.contains("失败") ? palette.errFg : palette.okFg).color)
                    .lineLimit(2)
            }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    if hubModel.activeWorkspaceGroups.isEmpty
                        && hubModel.providerErrors.isEmpty
                        && (!hubModel.query.isEmpty || hubModel.workspaceFilter != .all)
                    {
                        WorkspaceEmptyMatchPlaceholder()
                    }
                    WorkspaceGroupedSessionRows(
                        groups: hubModel.activeWorkspaceGroups,
                        collapsedKeys: $hubModel.collapsedWorkspaceKeys,
                        workspaceFilter: $hubModel.workspaceFilter,
                        isArchived: false,
                        onSelectAllInGroup: { group in
                            hubModel.selectAllInWorkspaceGroup(keys: group.items.map(\.compositeKey), partition: .active)
                        },
                        onCopyPath: copyWorkspacePath
                    ) { entry, group in
                        hubRowView(
                            entry,
                            partition: .active,
                            subtitle: group.subtitlesByItemId[entry.compositeKey] ?? ""
                        )
                    }

                    if hubModel.archivedOpen {
                        WorkspaceArchivedExpandedSection(
                            isOpen: $hubModel.archivedOpen,
                            archivedCount: hubModel.filteredArchivedItems.count
                        ) {
                            WorkspaceGroupedSessionRows(
                                groups: hubModel.archivedWorkspaceGroups,
                                collapsedKeys: $hubModel.collapsedArchivedWorkspaceKeys,
                                workspaceFilter: $hubModel.workspaceFilter,
                                isArchived: true,
                                onSelectAllInGroup: { group in
                                    hubModel.selectAllInWorkspaceGroup(keys: group.items.map(\.compositeKey), partition: .archived)
                                },
                                onCopyPath: copyWorkspacePath
                            ) { entry, group in
                                hubRowView(
                                    entry,
                                    partition: .archived,
                                    subtitle: group.subtitlesByItemId[entry.compositeKey] ?? ""
                                )
                            }
                        }
                    }
                }
            }
            .frame(maxHeight: .infinity, alignment: .top)

            if !hubModel.archivedOpen {
                WorkspaceArchivedClosedFooter(
                    isOpen: $hubModel.archivedOpen,
                    archivedCount: hubModel.filteredArchivedItems.count
                )
            }

            SessionBatchBarAndDialogView(
                selectedCount: activeSel.keys.count,
                operations: hubModel.batchOperations(for: activePart),
                busy: hubModel.batchBusy,
                notice: activeSel.notice,
                prepareError: hubModel.batchPrepareError,
                executeError: hubModel.batchExecuteError,
                result: hubModel.batchResult,
                onSelectAll: hubModel.selectAllVisible,
                onClear: {
                    hubModel.partitionedSelection.clear(partition: activePart)
                    hubModel.clearBatchFeedback()
                },
                onPrepareOperation: { op in
                    Task { await hubModel.prepareBatch(operation: op, compositeKeys: activeSel.keys, partition: activePart) }
                }
            )
        }
        .padding(.top, 14)
        .padding(.horizontal, 10)
        .padding(.bottom, 10)
    }

    private func copyWorkspacePath(_ path: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(path, forType: .string)
        hubModel.feedbackMessage = "已复制工作区路径"
    }

    private func hubRowView(
        _ entry: CrossProviderSessionItem,
        partition: SessionSelectionPartition,
        subtitle: String
    ) -> some View {
        let item = entry.item
        let partState = hubModel.partitionedSelection.state(for: partition)
        let isSelected = (hubModel.selectedEntry?.compositeKey == entry.compositeKey)
        let isMulti = partState.keys.contains(entry.compositeKey)

        return RenameableSessionRowView(
            sessionKey: entry.compositeKey,
            leadingBrandIcon: entry.provider.brand,
            title: item.title,
            idLine: (entry.provider == .claudeCode && item.title != item.nativeId) ? item.nativeId : nil,
            subtitle: subtitle,
            inlinePreview: item.previewText,
            isCodexCompactRow: entry.provider == .codex,
            statusText: item.statusText,
            running: item.running,
            storageSize: item.storageSize,
            selected: isSelected,
            selecting: partState.selecting,
            multiSelected: isMulti,
            disabled: hubModel.batchBusy,
            canRename: item.canRename,
            renameBlockReason: item.renameBlockReason,
            externalRenameRequestId: hubModel.renameRequestSeqByKey[entry.compositeKey, default: 0],
            hasRowMenu: true,
            batchSelectedCount: partState.keys.count,
            makeRowMenuActions: {
                buildRowMenuActions(entry, partition: partition)
            },
            makeBatchMenuActions: {
                let currentState = hubModel.partitionedSelection.state(for: partition)
                return ProviderMenuMatrixBuilder.buildBatchContextMenuActions(
                    selectedCount: currentState.keys.count,
                    operations: hubModel.batchOperations(for: partition),
                    busy: hubModel.batchBusy,
                    onPrepare: { op in
                        Task { await hubModel.prepareBatch(operation: op, compositeKeys: currentState.keys, partition: partition) }
                    },
                    onClear: {
                        hubModel.partitionedSelection.clear(partition: partition)
                        hubModel.clearBatchFeedback()
                    }
                )
            },
            onRowClick: { click in
                guard !hubModel.batchBusy else { return }
                let visible = (partition == .archived)
                    ? hubModel.visualOrderedArchivedKeys
                    : hubModel.visualOrderedActiveKeys
                let shouldOpen = hubModel.partitionedSelection.click(
                    partition: partition,
                    visible: visible,
                    key: entry.compositeKey,
                    click: click,
                    activeKey: hubModel.selectedEntry?.compositeKey
                )
                if !(click.ctrlKey && click.detail != 0) {
                    hubModel.clearBatchFeedback()
                }
                if shouldOpen {
                    hubModel.selectedCompositeKey = entry.compositeKey
                }
            },
            onToggleCheckbox: {
                guard !hubModel.batchBusy else { return }
                let visible = (partition == .archived)
                    ? hubModel.visualOrderedArchivedKeys
                    : hubModel.visualOrderedActiveKeys
                _ = hubModel.partitionedSelection.click(
                    partition: partition,
                    visible: visible,
                    key: entry.compositeKey,
                    click: SessionClick(detail: 1, metaKey: true),
                    activeKey: hubModel.selectedEntry?.compositeKey
                )
                hubModel.clearBatchFeedback()
            },
            onRename: { nextTitle in
                try await hubModel.performRename(entry: entry, newTitle: nextTitle)
            }
        )
    }

    private func buildRowMenuActions(
        _ entry: CrossProviderSessionItem,
        partition: SessionSelectionPartition
    ) -> [SessionMenuAction] {
        let item = entry.item
        let supportsArch = (entry.provider == .codex || entry.provider == .antigravity || entry.provider == .dsh)
        return ProviderMenuMatrixBuilder.buildActions(
            provider: entry.provider,
            isHeaderMenu: false,
            isLocalMachine: store.target == .local,
            id: item.nativeId,
            path: item.path,
            cwd: item.cwd,
            archived: item.archived,
            canRename: item.canRename,
            renameBlockReason: item.renameBlockReason,
            canArchive: item.canArchive,
            archiveBlockReason: item.archiveBlockReason,
            canDelete: item.canDelete,
            deleteBlockReason: item.deleteBlockReason,
            busy: hubModel.batchBusy,
            latestReplyText: nil,
            onRename: {
                hubModel.renameRequestSeqByKey[entry.compositeKey, default: 0] += 1
            },
            onToggleArchive: supportsArch ? {
                if entry.provider == .codex {
                    Task {
                        await hubModel.prepareBatch(
                            operation: item.archived ? .restore : .archive,
                            compositeKeys: [entry.compositeKey],
                            partition: partition
                        )
                    }
                } else {
                    Task { await hubModel.toggleSingleArchive(entry) }
                }
            } : nil,
            onCopy: { text, msg in
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
                hubModel.feedbackMessage = msg
            },
            onRevealInFinder: { path in
                Task {
                    do {
                        try await FileOpenerService.revealPaths([path])
                    } catch {
                        hubModel.feedbackMessage = "无法在 Finder 中显示，请检查文件是否仍存在"
                    }
                }
            },
            onDelete: {
                Task {
                    await hubModel.prepareBatch(
                        operation: .delete,
                        compositeKeys: [entry.compositeKey],
                        partition: partition
                    )
                }
            }
        )
    }

    @ViewBuilder
    private var stageArea: some View {
        if let entry = hubModel.selectedEntry {
            let targetWs = entry.provider.workspaceId
            let stagePalette = DesignPalette.resolve(scheme: scheme, workspace: targetWs)
            UnifiedProviderWorkspaceView(
                store: store,
                workspace: targetWs,
                scheme: scheme,
                hideSidebar: true,
                externalSeedItem: entry.item,
                onExternalMutation: {
                    Task { await hubModel.fetchAllSessions() }
                },
                onExternalSelectSession: { nextSessionKey in
                    hubModel.handleStageSessionSelected(provider: entry.provider, sessionKey: nextSessionKey)
                }
            )
            .environment(\.agentHubPalette, stagePalette)
            .id("\(store.revision):hub-stage:\(entry.provider.rawValue)")
        } else {
            VStack(spacing: 10) {
                Spacer()
                LucideIcon(.folderGit2, size: 32)
                    .foregroundStyle(palette.textMuted.color)
                Text("当前机器暂无工作区会话")
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.background.color)
        }
    }
}
