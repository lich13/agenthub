import AppKit
import SwiftUI

// MARK: - Normalized Session Row Adapter Across All 6 Providers

struct UnifiedSessionSummaryItem: Equatable, Sendable, Identifiable {
    var sessionKey: String
    var nativeId: String
    var title: String
    var cwd: String
    var workspaceRoot: String?
    var path: String
    var updatedAt: String? {
        didSet {
            if oldValue != updatedAt {
                updatedAtMs = ProjectWorkspaceEngine.parseTimestampMs(updatedAt)
            }
        }
    }
    private(set) var updatedAtMs: Int64
    var previewText: String
    var statusText: String
    var statusChipBadge: String?
    var statusChipTone: StatusChipTone
    var running: Bool
    var archived: Bool
    var canRename: Bool
    var renameBlockReason: String?
    var canArchive: Bool
    var archiveBlockReason: String?
    var canDelete: Bool
    var deleteBlockReason: String?
    var storageSize: SessionStorageSize?
    var readError: String?
    var readWarning: String?

    init(
        sessionKey: String,
        nativeId: String,
        title: String,
        cwd: String,
        workspaceRoot: String? = nil,
        path: String,
        updatedAt: String? = nil,
        previewText: String = "",
        statusText: String,
        statusChipBadge: String? = nil,
        statusChipTone: StatusChipTone = .neutral,
        running: Bool,
        archived: Bool,
        canRename: Bool,
        renameBlockReason: String? = nil,
        canArchive: Bool,
        archiveBlockReason: String? = nil,
        canDelete: Bool,
        deleteBlockReason: String? = nil,
        storageSize: SessionStorageSize? = nil,
        readError: String? = nil,
        readWarning: String? = nil
    ) {
        self.sessionKey = sessionKey
        self.nativeId = nativeId
        self.title = title
        self.cwd = cwd
        self.workspaceRoot = workspaceRoot
        self.path = path
        self.updatedAt = updatedAt
        self.updatedAtMs = ProjectWorkspaceEngine.parseTimestampMs(updatedAt)
        self.previewText = previewText
        self.statusText = statusText
        self.statusChipBadge = statusChipBadge
        self.statusChipTone = statusChipTone
        self.running = running
        self.archived = archived
        self.canRename = canRename
        self.renameBlockReason = renameBlockReason
        self.canArchive = canArchive
        self.archiveBlockReason = archiveBlockReason
        self.canDelete = canDelete
        self.deleteBlockReason = deleteBlockReason
        self.storageSize = storageSize
        self.readError = readError
        self.readWarning = readWarning
    }

    var id: String { sessionKey }
}

// MARK: - Unified 6-Provider Workspace View (Codex, Claude, Grok, Antigravity, DSH, GrokBot)

struct UnifiedProviderWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject var store: RuntimeConnectionStore
    let workspace: WorkspaceId
    let scheme: ColorSchemeKind
    var hideSidebar: Bool
    var externalSeedItem: UnifiedSessionSummaryItem?
    var onExternalMutation: (() -> Void)?
    var onExternalSelectSession: ((String?) -> Void)?
    var onExternalSummaryUpdated: ((UnifiedSessionSummaryItem, UInt64) -> Void)?

    @StateObject private var model: ProviderWorkspaceStore
    @StateObject private var timelineTracker = TimelineActiveTracker()
    @StateObject private var stageController = ProviderStageController()
    @State private var readWarningExpanded: Bool = false
    @State private var searchModalOpen: Bool = false

    init(
        store: RuntimeConnectionStore,
        workspace: WorkspaceId,
        scheme: ColorSchemeKind,
        hideSidebar: Bool = false,
        externalSeedItem: UnifiedSessionSummaryItem? = nil,
        onExternalMutation: (() -> Void)? = nil,
        onExternalSelectSession: ((String?) -> Void)? = nil,
        onExternalSummaryUpdated: ((UnifiedSessionSummaryItem, UInt64) -> Void)? = nil
    ) {
        _model = StateObject(wrappedValue: {
            let initialStore = ProviderWorkspaceStore(store: store, workspace: workspace)
            initialStore.skipSidebarListFetch = hideSidebar
            if let externalSeedItem {
                initialStore.externalSeedItem = externalSeedItem
                initialStore.selectedKey = externalSeedItem.sessionKey
            }
            initialStore.onExternalMutation = onExternalMutation
            initialStore.onExternalSelectSession = onExternalSelectSession
            initialStore.onExternalSummaryUpdated = onExternalSummaryUpdated
            return initialStore
        }())
        self.store = store
        self.workspace = workspace
        self.scheme = scheme
        self.hideSidebar = hideSidebar
        self.externalSeedItem = externalSeedItem
        self.onExternalMutation = onExternalMutation
        self.onExternalSelectSession = onExternalSelectSession
        self.onExternalSummaryUpdated = onExternalSummaryUpdated
    }

    init(
        model: ProviderWorkspaceStore,
        scheme: ColorSchemeKind,
        hideSidebar: Bool = false,
        externalSeedItem: UnifiedSessionSummaryItem? = nil,
        onExternalMutation: (() -> Void)? = nil,
        onExternalSelectSession: ((String?) -> Void)? = nil,
        onExternalSummaryUpdated: ((UnifiedSessionSummaryItem, UInt64) -> Void)? = nil
    ) {
        _model = StateObject(wrappedValue: {
            model.skipSidebarListFetch = hideSidebar
            if let externalSeedItem {
                model.externalSeedItem = externalSeedItem
                model.selectedKey = externalSeedItem.sessionKey
            }
            if let onExternalMutation {
                model.onExternalMutation = onExternalMutation
            }
            if let onExternalSelectSession {
                model.onExternalSelectSession = onExternalSelectSession
            }
            if let onExternalSummaryUpdated {
                model.onExternalSummaryUpdated = onExternalSummaryUpdated
            }
            return model
        }())
        self.store = model.store
        self.workspace = model.workspace
        self.scheme = scheme
        self.hideSidebar = hideSidebar
        self.externalSeedItem = externalSeedItem ?? model.externalSeedItem
        self.onExternalMutation = onExternalMutation ?? model.onExternalMutation
        self.onExternalSelectSession = onExternalSelectSession ?? model.onExternalSelectSession
        self.onExternalSummaryUpdated = onExternalSummaryUpdated ?? model.onExternalSummaryUpdated
    }

    private func syncExternalBindings() {
        model.skipSidebarListFetch = hideSidebar
        model.onExternalMutation = onExternalMutation
        model.onExternalSelectSession = onExternalSelectSession
        model.onExternalSummaryUpdated = onExternalSummaryUpdated
    }

    private struct ExternalBindingsSyncView: NSViewRepresentable {
        let model: ProviderWorkspaceStore
        let hideSidebar: Bool
        let onExternalMutation: (() -> Void)?
        let onExternalSelectSession: ((String?) -> Void)?
        let onExternalSummaryUpdated: ((UnifiedSessionSummaryItem, UInt64) -> Void)?

        func makeNSView(context: Context) -> NSView {
            sync()
            return NSView(frame: .zero)
        }

        func updateNSView(_ nsView: NSView, context: Context) {
            sync()
        }

        @MainActor
        private func sync() {
            model.skipSidebarListFetch = hideSidebar
            model.onExternalMutation = onExternalMutation
            model.onExternalSelectSession = onExternalSelectSession
            model.onExternalSummaryUpdated = onExternalSummaryUpdated
        }
    }

    var body: some View {
        ZStack {
            GeometryReader { geo in
                if hideSidebar {
                    let stageWidth = max(320, geo.size.width)
                    stageArea(availableWidth: stageWidth, availableHeight: geo.size.height)
                        .frame(width: stageWidth, height: geo.size.height)
                } else {
                    HStack(spacing: 0) {
                        sidebarColumn
                            .frame(width: LayoutMetrics.sessionListWidth)
                            .frame(maxHeight: .infinity)
                            .background(palette.surface.color)
                            .overlay(alignment: .trailing) {
                                Rectangle().fill(palette.borderSubtle.color).frame(width: 1)
                            }

                        let stageWidth = max(320, geo.size.width - LayoutMetrics.sessionListWidth)
                        stageArea(availableWidth: stageWidth, availableHeight: geo.size.height)
                            .frame(width: stageWidth, height: geo.size.height)
                    }
                }
            }

            if searchModalOpen {
                SessionSearchModalView(
                    store: store,
                    provider: model.provider,
                    currentSessionKey: model.selectedItem?.sessionKey,
                    onSelectResult: { res in
                        model.handleSearchResultSelected(res)
                    },
                    onClose: { searchModalOpen = false }
                )
            }

            if let preview = model.batchPreview {
                ConfirmDialogOverlay(busy: model.batchBusy, onCancel: { model.batchPreview = nil }) {
                    SessionBatchPreviewDialogContentView(
                        preview: preview,
                        busy: model.batchBusy,
                        onCancel: { model.batchPreview = nil },
                        onConfirm: { Task { await model.executePreparedBatch() } }
                    )
                }
            }

            if let imgItem = stageController.previewImageItem {
                ImagePreviewModalView(title: imgItem.title, image: imgItem.image) {
                    stageController.previewImageItem = nil
                }
            }
        }
        .background(
            Group {
                if !hideSidebar {
                    WorkspaceKeyboardShortcutMonitorView(
                        isModalOpen: store.hasAnyActiveModal || ModalPresentationRegistry.shared.hasActiveModal || searchModalOpen || model.batchPreview != nil || stageController.previewImageItem != nil,
                        onRenameSelected: {
                            guard model.provider != .grokbot, !model.currentSelectionState.selecting, let item = model.selectedItem else { return false }
                            model.renameRequestSeqByKey[item.sessionKey, default: 0] += 1
                            return true
                        },
                        onEscapeClearSelection: {
                            let part = model.activeSelectionPartition
                            guard model.partitionedSelection.state(for: part).selecting else { return false }
                            model.partitionedSelection.clear(partition: part)
                            return true
                        },
                        onBatchDelete: {
                            let part = model.activeSelectionPartition
                            let sel = model.partitionedSelection.state(for: part)
                            guard model.provider != .grokbot, !model.batchBusy, sel.selecting, !sel.keys.isEmpty else { return false }
                            Task { await model.prepareBatch(operation: .delete, keys: sel.keys, partition: part) }
                            return true
                        }
                    )
                }
            }
            .frame(width: 0, height: 0)
        )
        .background(
            ExternalBindingsSyncView(
                model: model,
                hideSidebar: hideSidebar,
                onExternalMutation: onExternalMutation,
                onExternalSelectSession: onExternalSelectSession,
                onExternalSummaryUpdated: onExternalSummaryUpdated
            )
            .frame(width: 0, height: 0)
        )
        .onAppear {
            syncExternalBindings()
            if let externalSeedItem {
                model.externalSeedItem = externalSeedItem
                if model.selectedKey == nil {
                    model.selectedKey = externalSeedItem.sessionKey
                }
            }
            wireCoordinatorCallbacks()
        }
        // Note: 1-parameter .onChange fires async and may capture pre-update `self`;
        // use `nextHideSidebar`/`nextSeed` parameters here while ExternalBindingsSyncView
        // updates `onExternalMutation`/`onExternalSelectSession` from fresh `body` values.
        .onChange(of: hideSidebar) { nextHideSidebar in
            model.skipSidebarListFetch = nextHideSidebar
        }
        .onChange(of: externalSeedItem) { nextSeed in
            guard let nextSeed else { return }
            model.externalSeedItem = nextSeed
            if model.selectedKey != nextSeed.sessionKey {
                model.selectedKey = nextSeed.sessionKey
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSearchRequested)) { _ in
            guard searchModalOpen || canPresentSearchModal else { return }
            searchModalOpen = true
        }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSelectAllRequested)) { _ in
            guard !hideSidebar,
                  model.provider != .grokbot,
                  !model.batchBusy,
                  !store.hasAnyActiveModal,
                  !ModalPresentationRegistry.shared.hasActiveModal,
                  !searchModalOpen,
                  model.batchPreview == nil,
                  stageController.previewImageItem == nil
            else { return }
            model.selectAllVisible()
        }
        .onChange(of: model.archivedOpen) { isOpen in
            model.partitionedSelection.setArchivedOpen(isOpen)
            if !isOpen && model.batchTargetPartition == .archived {
                model.batchPreview = nil
                model.batchResult = nil
                model.batchPrepareError = nil
                model.batchExecuteError = nil
            }
        }
        .onChange(of: model.selectedItem?.sessionKey) { _ in
            stageController.subagentDrawerOpen = false
            stageController.selectedSubagent = nil
            if model.searchSelection?.sessionKey != model.selectedKey { model.searchSelection = nil }
            if model.pendingSearch?.sessionKey != model.selectedKey { model.pendingSearch = nil }
            stageController.previewImageItem = nil
            readWarningExpanded = false
            model.resetDetailStateForSelectionChange()
        }
        // 5s Session List Polling Clock (paused when window hidden or sidebar hidden)
        .task(id: "\(store.revision):\(store.capabilities.isReady):\(workspace.rawValue):\(model.query):\(model.archivedOpen):\(hideSidebar)") {
            syncExternalBindings()
            guard !hideSidebar else { return }
            model.syncPartitionedSelection()
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await model.fetchSessionList()
                }
                try? await Task.sleep(nanoseconds: 5_000_000_000)
            }
        }
        // Exact Detail Polling Clock (Claude/Grok 1s/2s; Codex/Antigravity/DSH/GrokBot 2s/5s)
        .task(id: "\(store.revision):\(store.capabilities.isReady):\(workspace.rawValue):\(model.selectedItem?.sessionKey ?? "none"):\(model.showAllHistory)") {
            guard let initialItem = model.selectedItem else {
                model.rows = []
                model.timelineEntries = []
                return
            }
            while !Task.isCancelled {
                let liveItem = model.selectedItem ?? initialItem
                guard liveItem.sessionKey == initialItem.sessionKey else { break }
                if store.isWindowVisible {
                    await model.fetchSessionDetail(for: liveItem)
                }
                let latestItem = model.selectedItem ?? liveItem
                let hasActiveSubagents = model.subagents?.agents.contains {
                    $0.status == "running" || $0.status == "creating"
                } == true
                let isFastProvider = (model.provider == .claudeCode || model.provider == .grok)
                let running = latestItem.running
                    || latestItem.statusChipBadge == SessionStatusLabels.waiting
                    || hasActiveSubagents
                let intervalNs: UInt64 = if isFastProvider {
                    running ? 1_000_000_000 : 2_000_000_000
                } else {
                    running ? 2_000_000_000 : 5_000_000_000
                }
                try? await Task.sleep(nanoseconds: intervalNs)
            }
        }
    }

    // MARK: - Sidebar List Column

    private var searchPlaceholderText: String {
        switch model.provider {
        case .grokbot: "搜索标题或 ID"
        case .codex, .claudeCode, .grok, .antigravity, .dsh: "搜索标题、ID 或工作目录"
        }
    }

    private var canPresentSearchModal: Bool {
        !model.batchBusy
            && !store.hasAnyActiveModal
            && !ModalPresentationRegistry.shared.hasActiveModal
            && model.batchPreview == nil
            && stageController.previewImageItem == nil
    }

    private var sidebarColumn: some View {
        let activePart = model.activeSelectionPartition
        let activeSel = model.partitionedSelection.state(for: activePart)
        let resolvedSelectedKey = model.selectedItem?.sessionKey
        let activePartState = model.partitionedSelection.active
        let activeSelectedSet = Set(activePartState.keys)
        let archivedPartState = model.partitionedSelection.archived
        let archivedSelectedSet = Set(archivedPartState.keys)
        let filtActive = model.filteredActiveItems
        let filtArchived = model.filteredArchivedItems

        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 4) {
                HStack(spacing: 5) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 7, style: .continuous)
                            .fill(palette.accentSoft.color)
                            .frame(width: 22, height: 22)
                        AgentBrandIcon(brandId, size: 14)
                            .foregroundStyle(palette.accent.color)
                    }

                    Text(workspace.label)
                        .font(DesignFonts.body(14, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                        .lineLimit(1)
                        .minimumScaleFactor(0.9)
                        .layoutPriority(2)

                    Text("\(filtActive.count)")
                        .font(DesignFonts.body(11, weight: .medium).monospacedDigit())
                        .foregroundStyle(palette.accent.color)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .frame(minWidth: 20)
                        .fixedSize(horizontal: true, vertical: false)
                        .background(Capsule().fill(palette.accentSoft.color))
                        .overlay(Capsule().strokeBorder(palette.accentBorder.color, lineWidth: 1))
                }

                Spacer(minLength: 2)

                HStack(spacing: 2) {
                    if model.supportsWorkspaceGrouping {
                        Button {
                            model.groupingMode = (model.groupingMode == .byWorkspace) ? .flat : .byWorkspace
                        } label: {
                            LucideIcon(.folderGit2, size: 14)
                                .foregroundStyle((model.groupingMode == .byWorkspace ? palette.accent : palette.textMuted).color)
                                .frame(width: 25, height: 25)
                                .background(
                                    RoundedRectangle(cornerRadius: 7, style: .continuous)
                                        .fill(model.groupingMode == .byWorkspace ? palette.accentSoft.color : Color.clear)
                                )
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(PlainHitButtonStyle())
                        .help(model.groupingMode == .byWorkspace ? "切换为平铺时间流" : "按工作区分组")
                        .accessibilityLabel(model.groupingMode == .byWorkspace ? "切换为平铺时间流" : "按工作区分组")

                        WorkspaceSidebarFilterMenuButton(
                            isFiltered: model.workspaceFilter != .all,
                            makeActions: {
                                WorkspaceFilterMenuBuilder.buildActions(
                                    currentFilter: model.workspaceFilter,
                                    groups: model.availableWorkspaceGroups,
                                    idPrefix: "ws-filter"
                                ) { nextFilter in
                                    model.workspaceFilter = nextFilter
                                }
                            }
                        )
                    }

                    Button {
                        Task { await model.fetchSessionList() }
                    } label: {
                        LucideIcon(.refreshCw, size: 14)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 25))
                    .accessibilityLabel("刷新列表")
                }
            }

            WorkspaceSidebarSearchField(placeholder: searchPlaceholderText, query: $model.query)

            WorkspaceActiveFilterPillBanner(filter: model.workspaceFilter) {
                model.workspaceFilter = .all
            }

            if let err = model.errorMessage {
                Text(err)
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.errFg.color)
                    .lineLimit(2)
            }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    if filtActive.isEmpty
                        && model.errorMessage == nil
                        && (!model.query.isEmpty || model.workspaceFilter != .all)
                    {
                        WorkspaceEmptyMatchPlaceholder()
                    } else if model.supportsWorkspaceGrouping && model.groupingMode == .byWorkspace {
                        WorkspaceGroupedSessionRows(
                            groups: model.activeWorkspaceGroups,
                            collapsedKeys: $model.collapsedWorkspaceKeys,
                            workspaceFilter: $model.workspaceFilter,
                            isArchived: false,
                            onSelectAllInGroup: model.provider == .grokbot ? nil : { group in
                                model.selectAllInWorkspaceGroup(keys: group.items.map(\.sessionKey), partition: .active)
                            },
                            onCopyPath: copyWorkspacePath
                        ) { item, group in
                            sessionRowView(
                                item,
                                partition: .active,
                                partState: activePartState,
                                selectedSet: activeSelectedSet,
                                resolvedSelectedKey: resolvedSelectedKey,
                                groupedSubtitleOverride: group.subtitlesByItemId[item.sessionKey] ?? ""
                            )
                        }
                    } else {
                        ForEach(filtActive) { item in
                            sessionRowView(
                                item,
                                partition: .active,
                                partState: activePartState,
                                selectedSet: activeSelectedSet,
                                resolvedSelectedKey: resolvedSelectedKey,
                                groupedSubtitleOverride: nil
                            )
                        }
                    }

                    if model.supportsArchive && model.archivedOpen {
                        WorkspaceArchivedExpandedSection(
                            isOpen: $model.archivedOpen,
                            archivedCount: filtArchived.count
                        ) {
                            if model.supportsWorkspaceGrouping && model.groupingMode == .byWorkspace {
                                WorkspaceGroupedSessionRows(
                                    groups: model.archivedWorkspaceGroups,
                                    collapsedKeys: $model.collapsedArchivedWorkspaceKeys,
                                    workspaceFilter: $model.workspaceFilter,
                                    isArchived: true,
                                    onSelectAllInGroup: { group in
                                        model.selectAllInWorkspaceGroup(keys: group.items.map(\.sessionKey), partition: .archived)
                                    },
                                    onCopyPath: copyWorkspacePath
                                ) { item, group in
                                    sessionRowView(
                                        item,
                                        partition: .archived,
                                        partState: archivedPartState,
                                        selectedSet: archivedSelectedSet,
                                        resolvedSelectedKey: resolvedSelectedKey,
                                        groupedSubtitleOverride: group.subtitlesByItemId[item.sessionKey] ?? ""
                                    )
                                }
                            } else {
                                ForEach(filtArchived) { item in
                                    sessionRowView(
                                        item,
                                        partition: .archived,
                                        partState: archivedPartState,
                                        selectedSet: archivedSelectedSet,
                                        resolvedSelectedKey: resolvedSelectedKey,
                                        groupedSubtitleOverride: nil
                                    )
                                }
                            }
                        }
                    }
                }
            }
            .frame(maxHeight: .infinity, alignment: .top)

            if model.supportsArchive && !model.archivedOpen {
                WorkspaceArchivedClosedFooter(
                    isOpen: $model.archivedOpen,
                    archivedCount: filtArchived.count
                )
            }

            if model.provider != .grokbot {
                SessionBatchBarAndDialogView(
                    selectedCount: activeSel.keys.count,
                    operations: model.batchOperations(for: activePart),
                    busy: model.batchBusy,
                    notice: activeSel.notice,
                    prepareError: model.batchPrepareError,
                    executeError: model.batchExecuteError,
                    result: model.batchResult,
                    onSelectAll: model.selectAllVisible,
                    onClear: {
                        model.partitionedSelection.clear(partition: activePart)
                        model.batchPreview = nil
                        model.batchResult = nil
                        model.batchPrepareError = nil
                        model.batchExecuteError = nil
                    },
                    onPrepareOperation: { op in
                        Task { await model.prepareBatch(operation: op, keys: activeSel.keys, partition: activePart) }
                    }
                )
            }
        }
        .padding(.top, 14)
        .padding(.horizontal, 10)
        .padding(.bottom, 10)
    }

    private func copyWorkspacePath(_ path: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(path, forType: .string)
        model.feedbackMessage = "已复制工作区路径"
    }

    private func sessionRowView(
        _ item: UnifiedSessionSummaryItem,
        partition: SessionSelectionPartition,
        partState: SessionSelectionState,
        selectedSet: Set<String>,
        resolvedSelectedKey: String?,
        groupedSubtitleOverride: String?
    ) -> some View {
        let isSelected = (resolvedSelectedKey == item.sessionKey)
        let isMulti = selectedSet.contains(item.sessionKey)
        let resolvedSubtitle = groupedSubtitleOverride ?? (model.provider == .grokbot ? "" : item.cwd)

        return RenameableSessionRowView(
            sessionKey: item.sessionKey,
            title: item.title,
            idLine: (model.provider == .claudeCode && item.title != item.nativeId) ? item.nativeId : nil,
            subtitle: resolvedSubtitle,
            inlinePreview: item.previewText,
            isCodexCompactRow: model.provider == .codex,
            statusText: item.statusText,
            running: item.running,
            storageSize: item.storageSize,
            selected: isSelected,
            selecting: partState.selecting,
            multiSelected: isMulti,
            disabled: model.batchBusy,
            canRename: item.canRename,
            renameBlockReason: item.renameBlockReason,
            externalRenameRequestId: model.renameRequestSeqByKey[item.sessionKey, default: 0],
            hasRowMenu: true,
            batchSelectedCount: partState.keys.count,
            makeRowMenuActions: {
                buildMenuActionsForItem(item, isHeaderMenu: false)
            },
            makeBatchMenuActions: {
                let currentState = model.partitionedSelection.state(for: partition)
                return ProviderMenuMatrixBuilder.buildBatchContextMenuActions(
                    selectedCount: currentState.keys.count,
                    operations: model.batchOperations(for: partition),
                    busy: model.batchBusy,
                    onPrepare: { op in
                        Task { await model.prepareBatch(operation: op, keys: currentState.keys, partition: partition) }
                    },
                    onClear: {
                        model.partitionedSelection.clear(partition: partition)
                        model.batchPreview = nil
                        model.batchResult = nil
                        model.batchPrepareError = nil
                        model.batchExecuteError = nil
                    }
                )
            },
            onRowClick: { click in
                if model.provider == .grokbot {
                    if !(click.ctrlKey && click.detail != 0) {
                        model.selectedKey = item.sessionKey
                    }
                    return
                }
                guard !model.batchBusy else { return }
                let partVisibleKeys = (partition == .archived)
                    ? model.visualOrderedArchivedKeys
                    : model.visualOrderedActiveKeys
                let shouldOpen = model.partitionedSelection.click(
                    partition: partition,
                    visible: partVisibleKeys,
                    key: item.sessionKey,
                    click: click,
                    activeKey: model.selectedItem?.sessionKey
                )
                if !(click.ctrlKey && click.detail != 0) {
                    model.batchPreview = nil
                    model.batchResult = nil
                    model.batchPrepareError = nil
                    model.batchExecuteError = nil
                }
                if shouldOpen {
                    model.selectedKey = item.sessionKey
                    model.headerInlineRename = false
                }
            },
            onRename: model.provider == .grokbot ? nil : { nextTitle in
                try await model.performRename(item: item, newTitle: nextTitle)
            }
        )
    }

    // MARK: - Stage Conversation Reading Column + Subagents + Floating Search

    @ViewBuilder
    private func stageArea(availableWidth: CGFloat, availableHeight: CGFloat) -> some View {
        if let item = model.selectedItem {
            let canDockSubagents = availableWidth >= LayoutMetrics.subagentDockThreshold
            let subagentsSupported = model.subagentsSupportedForCurrentProvider
            let mainColumnWidth = (stageController.subagentDrawerOpen && canDockSubagents)
                ? max(320, availableWidth - LayoutMetrics.subagentDrawerWidth)
                : availableWidth
            let currentRows = model.displayedRows
            let showSubagentSummaryBar = (model.provider == .codex || model.provider == .claudeCode || model.provider == .antigravity)
                && SubagentSummaryBarView.shouldDisplay(
                    provider: model.provider,
                    collection: model.subagents,
                    isSupported: subagentsSupported,
                    capabilitiesReady: store.capabilities.isReady
                )

            ZStack(alignment: .trailing) {
                HStack(spacing: 0) {
                    VStack(spacing: 0) {
                        WorkspaceStageHeaderView(
                            title: item.title,
                            subtitle: item.cwd.isEmpty ? nil : item.cwd,
                            statusBadge: item.statusChipBadge,
                            statusTone: item.statusChipTone,
                            availableWidth: mainColumnWidth,
                            inlineRenameActive: model.headerInlineRename,
                            initialRenameText: item.title,
                            onCommitInlineRename: { next in
                                try await model.performRename(item: item, newTitle: next)
                                model.headerInlineRename = false
                            },
                            onCancelInlineRename: { model.headerInlineRename = false }
                        ) {
                            TaskMenuTriggerButton(
                                rightAligned: true,
                                accessibilityLabel: "会话菜单",
                                makeActions: { buildMenuActionsForItem(item, isHeaderMenu: true) }
                            ) {
                                LucideIcon(.moreHorizontal, size: 18)
                                    .foregroundStyle(palette.textMuted.color)
                                    .frame(width: 32, height: 32)
                                    .contentShape(Rectangle())
                            }
                        }

                        // Subagent summary bar directly below glass header (DESIGN.md:113)
                        if showSubagentSummaryBar {
                            SubagentSummaryBarView(
                                provider: model.provider,
                                collection: model.subagents,
                                isSupported: subagentsSupported,
                                capabilitiesReady: store.capabilities.isReady
                            ) { triggerResponder in
                                stageController.subagentTriggerResponder = triggerResponder
                                stageController.selectedSubagent = nil
                                stageController.subagentDrawerOpen.toggle()
                            }
                        }

                        if let fb = model.feedbackMessage, !fb.isEmpty {
                            Text(fb)
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.okFg.color)
                                .padding(.vertical, 4)
                        }

                        if hideSidebar,
                           let stageBatchErr = model.batchPrepareError ?? model.batchExecuteError ?? model.errorMessage,
                           !stageBatchErr.isEmpty
                        {
                            Text(stageBatchErr)
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.errFg.color)
                                .padding(.horizontal, 20)
                                .padding(.vertical, 6)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(palette.errBg.color)
                        }

                        if let rErr = item.readError, !rErr.isEmpty {
                            Text(rErr)
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.errFg.color)
                                .padding(.horizontal, 20)
                                .padding(.vertical, 6)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(palette.errBg.color)
                        }

                        if let rWarn = item.readWarning, !rWarn.isEmpty {
                            VStack(alignment: .leading, spacing: 4) {
                                Button {
                                    readWarningExpanded.toggle()
                                } label: {
                                    HStack(spacing: 6) {
                                        LucideIcon(readWarningExpanded ? .chevronDown : .chevronRight, size: 14)
                                        Text("读取提示")
                                            .font(DesignFonts.body(12, weight: .medium))
                                        Spacer()
                                    }
                                    .foregroundStyle(palette.textSecondary.color)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(PlainHitButtonStyle())

                                if readWarningExpanded {
                                    Text(rWarn)
                                        .font(DesignFonts.body(12))
                                        .foregroundStyle(palette.textSecondary.color)
                                }
                            }
                            .padding(.horizontal, 20)
                            .padding(.vertical, 6)
                            .background(palette.surface.color)
                            .overlay(alignment: .bottom) {
                                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                            }
                        }

                        if model.provider == .claudeCode,
                           item.readError == nil,
                           let blockReason = item.renameBlockReason,
                           !blockReason.isEmpty
                        {
                            let hasExpandedDiag = store.target == .local
                                && model.claudeOwnershipKey == item.sessionKey
                                && (model.claudeOwnershipError != nil || model.claudeOwnershipData != nil)
                            VStack(alignment: .leading, spacing: 5) {
                                HStack(spacing: 8) {
                                    Text(blockReason)
                                        .font(DesignFonts.body(11.5))
                                        .foregroundStyle(palette.textMuted.color)
                                        .lineLimit(1)
                                    Spacer(minLength: 8)
                                    if store.target == .local {
                                        Button {
                                            Task { await model.fetchClaudeOwnershipDiagnostics(sessionKey: item.sessionKey) }
                                        } label: {
                                            Text("查看占用详情")
                                                .font(DesignFonts.body(11.5, weight: .medium))
                                                .foregroundStyle(palette.textSecondary.color)
                                                .padding(.horizontal, 8)
                                                .frame(height: 24)
                                                .background(
                                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                                        .fill(palette.elevated.color)
                                                )
                                                .overlay(
                                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                                        .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                                                )
                                                .contentShape(Rectangle())
                                        }
                                        .buttonStyle(PlainHitButtonStyle())
                                        .disabled(model.claudeOwnershipLoading)
                                    }
                                }

                                if hasExpandedDiag {
                                    if let diagErr = model.claudeOwnershipError {
                                        Text(diagErr)
                                            .font(DesignFonts.body(12))
                                            .foregroundStyle(palette.errFg.color)
                                    }
                                    if let diag = model.claudeOwnershipData {
                                        let summaryText: String = switch diag.ownership {
                                        case "inactive": "未发现关联的活跃会话进程"
                                        case "open": "存在活跃会话或文件占用"
                                        default: "归属尚未确认"
                                        }
                                        Text(summaryText)
                                            .font(DesignFonts.body(12, weight: .medium))
                                            .foregroundStyle(palette.textPrimary.color)
                                        ForEach(Array(diag.processes.enumerated()), id: \.offset) { _, proc in
                                            let roleLabel: String = switch proc.role {
                                            case "session": "会话"
                                            case "daemon": "后台服务"
                                            case "pty_host": "终端宿主"
                                            case "unknown": "未核验进程"
                                            default: proc.role
                                            }
                                            let mismatch: String = proc.mismatchedFields.isEmpty
                                                ? ""
                                                : " · 不匹配字段：\(proc.mismatchedFields.joined(separator: "、"))"
                                            let regLabel: String = proc.registrationVerified ? "登记已核验" : "登记未核验"
                                            let procSummary: String = "PID \(proc.pid) · \(roleLabel) · \(regLabel)\(mismatch)"
                                            Text(procSummary)
                                                .font(DesignFonts.mono(11))
                                                .foregroundStyle(palette.textSecondary.color)
                                        }
                                    }
                                }
                            }
                            .padding(.horizontal, 20)
                            .padding(.vertical, hasExpandedDiag ? 8 : 5)
                            .background(palette.surface.color)
                            .overlay(alignment: .bottom) {
                                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                            }
                        }

                        ZStack(alignment: .bottomTrailing) {
                            ZStack(alignment: .top) {
                                ZStack(alignment: .leading) {
                                    ConversationReaderTableView(
                                        coordinator: model.coordinatorRef ?? model.tableCoordinator,
                                        rows: currentRows,
                                        scheme: scheme,
                                        workspace: workspace,
                                        sessionScopeKey: "\(store.machineScope):\(model.provider.rawValue):\(item.sessionKey)",
                                        sessionProvider: model.provider,
                                        sessionKey: item.sessionKey,
                                        cwd: item.cwd,
                                        hasTimelineRail: !model.timelineEntries.isEmpty,
                                        isSubagent: false,
                                        canLoadOlder: model.timelineWindow.isActive ? model.timelineWindow.hasMoreOlder : (model.hasMoreOlder && model.beforeCursor != nil),
                                        isLoadingOlder: model.loadingOlder || model.timelineWindow.busy,
                                        isLoadOlderBlockedByError: model.loadOlderError != nil || (model.timelineWindow.isActive && model.timelineWindow.error != nil)
                                    )

                                    if !model.detailLoading && model.detailLoadedForKey == item.sessionKey && currentRows.isEmpty {
                                        VStack {
                                            Spacer()
                                            Text("暂无对话")
                                                .font(DesignFonts.body(13))
                                                .foregroundStyle(palette.textMuted.color)
                                            Spacer()
                                        }
                                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                                        .allowsHitTesting(false)
                                    }

                                    if !model.timelineEntries.isEmpty {
                                        TimelineRailObserverView(
                                            tracker: timelineTracker,
                                            entries: model.timelineEntries,
                                            availableWidth: mainColumnWidth,
                                            loadOlder: (model.timelineWindow.isActive ? model.timelineWindow.hasMoreOlder : model.hasMoreOlder) ? {
                                                Task {
                                                    model.loadOlderError = nil
                                                    model.timelineWindow.clearError()
                                                    await model.loadOlderPageIfNeeded()
                                                }
                                            } : nil,
                                            loadingOlder: model.loadingOlder || model.timelineWindow.busy
                                        ) { entry in
                                            Task { await model.handleTimelineTickSelected(entry) }
                                        }
                                    }
                                }

                                // Top History Loading / Error Retry Bar (halts <200pt auto-trigger on error, DESIGN.md:141)
                                HistoryLoadingBarView(
                                    loading: model.loadingOlder,
                                    error: model.loadOlderError,
                                    onRetry: {
                                        model.loadOlderError = nil
                                        Task { await model.loadOlderPageIfNeeded() }
                                    }
                                )
                            }

                            VStack(alignment: .trailing, spacing: 8) {
                                TimelineHistoryStatusView(
                                    active: model.timelineWindow.isActive,
                                    busy: model.timelineWindow.busy,
                                    error: model.timelineWindow.error,
                                    hasNewer: model.timelineWindow.hasNewer(latestTailId: model.latestNativeTailId ?? model.rows.last?.timelineId),
                                    onLoadNewer: {
                                        Task { await model.loadNewerTimelineWindowPage() }
                                    },
                                    onReturnLatest: {
                                        model.returnFromTimelineWindowToLatest()
                                    }
                                )

                                FloatingSearchPillButton {
                                    guard canPresentSearchModal else { return }
                                    searchModalOpen = true
                                }
                            }
                            .padding(.trailing, 16)
                            .padding(.bottom, 16)
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)

                    if stageController.subagentDrawerOpen && canDockSubagents {
                        SubagentPanelView(
                            store: store,
                            provider: model.provider,
                            rootSessionKey: item.sessionKey,
                            collection: model.subagents,
                            initialAgent: stageController.selectedSubagent,
                            scheme: scheme,
                            workspace: workspace,
                            isModalOverlay: false,
                            onClose: { stageController.subagentDrawerOpen = false }
                        )
                    }
                }

                // <960pt Overlay Subagent Drawer with .34 Backdrop, Isolated Focus Root & Trigger Focus Restore
                if stageController.subagentDrawerOpen && !canDockSubagents {
                    Color.black.opacity(0.34)
                        .ignoresSafeArea()
                        .onTapGesture { stageController.subagentDrawerOpen = false }

                    SubagentOverlayFocusContainerView(palette: palette) {
                        SubagentPanelView(
                            store: store,
                            provider: model.provider,
                            rootSessionKey: item.sessionKey,
                            collection: model.subagents,
                            initialAgent: stageController.selectedSubagent,
                            scheme: scheme,
                            workspace: workspace,
                            isModalOverlay: true,
                            restoreFocusResponder: stageController.subagentTriggerResponder,
                            onClose: { stageController.subagentDrawerOpen = false }
                        )
                    }
                    .frame(width: min(LayoutMetrics.subagentDrawerWidth, availableWidth))
                    .frame(maxHeight: .infinity)
                }
            }
        } else if model.isRemoteProviderUnsupported {
            VStack(spacing: 10) {
                Spacer()
                AgentBrandIcon(brandId, size: 32)
                    .foregroundStyle(palette.textMuted.color)
                Text("当前远端服务尚未支持 \(workspace.label)")
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                Text("请前往「设置 → 更新与维护」升级远端 agenthub-webd 服务。")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.textMuted.color)
                Button("前往更新远端服务") {
                    store.activeWorkspace = .ops
                    store.settingsSection = .maintenance
                }
                .buttonStyle(SecondaryButtonStyle())
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.background.color)
        } else {
            VStack(spacing: 10) {
                Spacer()
                AgentBrandIcon(brandId, size: 32)
                    .foregroundStyle(palette.textMuted.color)
                Text("选择一个 \(workspace.label) 会话")
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.background.color)
        }
    }

    // MARK: - Coordinator & RPC Actions

    private var brandId: AgentBrandId {
        switch workspace {
        case .codex: .codex
        case .claude: .claude
        case .grok: .grok
        case .grokbot: .grokbot
        case .antigravity: .antigravity
        case .dsh: .dsh
        default: .codex
        }
    }

    private func wireCoordinatorCallbacks() {
        let c = model.coordinatorRef ?? model.tableCoordinator
        c.onOpenSubagent = { [weak model, weak stageController] sub in
            guard let model, let stageController, model.subagentsSupportedForCurrentProvider else { return }
            stageController.subagentTriggerResponder = NSApp.keyWindow?.firstResponder
            stageController.selectedSubagent = sub
            stageController.subagentDrawerOpen = true
        }
        c.onPreviewImage = { [weak stageController] title, img in
            stageController?.previewImageItem = (title, img)
        }
        c.onActiveTimelineChanged = { [weak timelineTracker] id in
            DispatchQueue.main.async {
                if timelineTracker?.activeId != id {
                    timelineTracker?.activeId = id
                }
            }
        }
    }

    private func buildMenuActionsForItem(_ item: UnifiedSessionSummaryItem, isHeaderMenu: Bool) -> [SessionMenuAction] {
        ProviderMenuMatrixBuilder.buildActions(
            provider: model.provider,
            isHeaderMenu: isHeaderMenu,
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
            busy: model.batchBusy,
            latestReplyText: isHeaderMenu ? model.latestReplyText : nil,
            onRename: model.provider == .grokbot ? nil : {
                if isHeaderMenu {
                    model.headerInlineRename = true
                } else {
                    model.renameRequestSeqByKey[item.sessionKey, default: 0] += 1
                }
            },
            onToggleArchive: model.supportsArchive ? {
                if model.provider == .codex && !isHeaderMenu {
                    // Codex sidebar row menu uses prepareSingle preview confirmation
                    let targetPartition: SessionSelectionPartition = item.archived ? .archived : .active
                    Task {
                        await model.prepareBatch(
                            operation: item.archived ? .restore : .archive,
                            keys: [item.sessionKey],
                            partition: targetPartition
                        )
                    }
                } else {
                    Task { await model.toggleSingleArchive(item) }
                }
            } : nil,
            onCopy: { text, msg in
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
                model.feedbackMessage = msg
            },
            onRevealInFinder: { path in
                Task {
                    do {
                        try await FileOpenerService.revealPaths([path])
                    } catch {
                        model.feedbackMessage = "无法在 Finder 中显示，请检查文件是否仍存在"
                    }
                }
            },
            onDelete: model.provider == .grokbot ? nil : {
                Task { await model.triggerSingleDelete(item) }
            }
        )
    }


}

@MainActor
private final class ProviderStageController: ObservableObject {
    @Published var subagentDrawerOpen: Bool = false
    @Published var selectedSubagent: SubagentActivity?
    @Published var previewImageItem: (title: String, image: NSImage)?
    weak var subagentTriggerResponder: NSResponder?
}

@MainActor
private final class TimelineActiveTracker: ObservableObject {
    @Published var activeId: String? = nil
}

private struct TimelineRailObserverView: View {
    @ObservedObject var tracker: TimelineActiveTracker
    let entries: [TimelineEntry]
    let availableWidth: CGFloat
    let loadOlder: (() -> Void)?
    let loadingOlder: Bool
    let onSelectEntry: (TimelineEntry) -> Void

    var body: some View {
        TimelineRailView(
            entries: entries,
            activeId: tracker.activeId,
            availableWidth: availableWidth,
            loadOlder: loadOlder,
            loadingOlder: loadingOlder,
            onSelectEntry: onSelectEntry
        )
    }
}
