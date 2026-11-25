import AppKit
import SwiftUI

// MARK: - Normalized Session Row Adapter Across All 6 Providers

public struct UnifiedSessionSummaryItem: Equatable, Sendable, Identifiable {
    public var sessionKey: String
    public var nativeId: String
    public var title: String
    public var cwd: String
    public var workspaceRoot: String? = nil
    public var path: String
    public var updatedAt: String? = nil
    public var previewText: String = ""
    public var statusText: String
    public var statusChipBadge: String? = nil
    public var statusChipTone: StatusChipTone = .neutral
    public var running: Bool
    public var archived: Bool
    public var canRename: Bool
    public var renameBlockReason: String? = nil
    public var canArchive: Bool
    public var archiveBlockReason: String? = nil
    public var canDelete: Bool
    public var deleteBlockReason: String? = nil
    public var storageSize: SessionStorageSize? = nil
    public var readError: String? = nil
    public var readWarning: String? = nil

    public var id: String { sessionKey }
}

// MARK: - Unified 6-Provider Workspace View (Codex, Claude, Grok, Antigravity, DSH, GrokBot)

public struct UnifiedProviderWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public let workspace: WorkspaceId
    public let scheme: ColorSchemeKind
    public var hideSidebar: Bool
    public var externalSeedItem: UnifiedSessionSummaryItem?
    public var onExternalMutation: (() -> Void)?
    public var onExternalSelectSession: ((String?) -> Void)?
    public var codexNewThreadSlot: (@MainActor (@escaping (String) -> Void, @escaping (String?) -> Void) -> AnyView)?
    public var codexCollaborationFooterSlot: (@MainActor (String, String?, CGFloat, CGFloat, @escaping @MainActor () -> Void) -> AnyView)?

    @StateObject private var model: ProviderWorkspaceStore
    @StateObject private var timelineTracker = TimelineActiveTracker()
    @State private var readWarningExpanded: Bool = false
    @State private var searchModalOpen: Bool = false
    @State private var subagentDrawerOpen: Bool = false
    @State private var subagentTriggerResponder: NSResponder?
    @State private var selectedSubagent: SubagentActivity?
    @State private var previewImageItem: (title: String, image: NSImage)?
    @State private var coordinatorRef: ConversationTableCoordinator?

    public init(
        store: RuntimeConnectionStore,
        workspace: WorkspaceId,
        scheme: ColorSchemeKind,
        hideSidebar: Bool = false,
        externalSeedItem: UnifiedSessionSummaryItem? = nil,
        onExternalMutation: (() -> Void)? = nil,
        onExternalSelectSession: ((String?) -> Void)? = nil,
        codexNewThreadSlot: (@MainActor (@escaping (String) -> Void, @escaping (String?) -> Void) -> AnyView)? = nil,
        codexCollaborationFooterSlot: (@MainActor (String, String?, CGFloat, CGFloat, @escaping @MainActor () -> Void) -> AnyView)? = nil
    ) {
        let initialStore = ProviderWorkspaceStore(store: store, workspace: workspace)
        initialStore.skipSidebarListFetch = hideSidebar
        if let externalSeedItem {
            initialStore.externalSeedItem = externalSeedItem
            initialStore.selectedKey = externalSeedItem.sessionKey
        }
        initialStore.onExternalMutation = onExternalMutation
        initialStore.onExternalSelectSession = onExternalSelectSession
        _model = StateObject(wrappedValue: initialStore)
        self.store = store
        self.workspace = workspace
        self.scheme = scheme
        self.hideSidebar = hideSidebar
        self.externalSeedItem = externalSeedItem
        self.onExternalMutation = onExternalMutation
        self.onExternalSelectSession = onExternalSelectSession
        self.codexNewThreadSlot = codexNewThreadSlot
        self.codexCollaborationFooterSlot = codexCollaborationFooterSlot
    }

    init(
        model: ProviderWorkspaceStore,
        scheme: ColorSchemeKind,
        hideSidebar: Bool = false
    ) {
        model.skipSidebarListFetch = hideSidebar
        _model = StateObject(wrappedValue: model)
        self.store = model.store
        self.workspace = model.workspace
        self.scheme = scheme
        self.hideSidebar = hideSidebar
        self.externalSeedItem = nil
        self.onExternalMutation = nil
        self.onExternalSelectSession = nil
        self.codexNewThreadSlot = nil
        self.codexCollaborationFooterSlot = nil
    }

    public var body: some View {
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

            if let imgItem = previewImageItem {
                ImagePreviewModalView(title: imgItem.title, image: imgItem.image) {
                    previewImageItem = nil
                }
            }
        }
        .background(
            Group {
                if !hideSidebar {
                    WorkspaceKeyboardShortcutMonitorView(
                        isModalOpen: store.hasAnyActiveModal || ModalPresentationRegistry.shared.hasActiveModal || searchModalOpen || model.batchPreview != nil || previewImageItem != nil,
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
        .onAppear { ensureCoordinator() }
        .onChange(of: externalSeedItem) { nextSeed in
            guard let nextSeed else { return }
            model.externalSeedItem = nextSeed
            if model.selectedKey != nextSeed.sessionKey {
                model.selectedKey = nextSeed.sessionKey
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSearchRequested)) { _ in
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
                  previewImageItem == nil
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
        .onDisappear { model.codexTailRefreshTask?.cancel() }
        .onChange(of: model.selectedItem?.sessionKey) { _ in
            model.codexTailRefreshTask?.cancel()
            subagentDrawerOpen = false
            selectedSubagent = nil
            if model.searchSelection?.sessionKey != model.selectedKey { model.searchSelection = nil }
            if model.pendingSearch?.sessionKey != model.selectedKey { model.pendingSearch = nil }
            previewImageItem = nil
            readWarningExpanded = false
            model.resetDetailStateForSelectionChange()
        }
        // 5s Session List Polling Clock (paused when window hidden or sidebar hidden)
        .task(id: "\(store.revision):\(store.capabilities.isReady):\(workspace.rawValue):\(model.query):\(model.archivedOpen):\(hideSidebar)") {
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
                    || latestItem.statusChipBadge == "等待处理"
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

    private var sidebarColumn: some View {
        let activePart = model.activeSelectionPartition
        let activeSel = model.partitionedSelection.state(for: activePart)

        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 9) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(palette.accentSoft.color)
                        .frame(width: 24, height: 24)
                    AgentBrandIcon(brandId, size: 16)
                }

                Text(workspace.label)
                    .font(DesignFonts.body(15, weight: .bold))
                    .foregroundStyle(palette.textPrimary.color)
                    .lineLimit(1)

                Text("\(model.filteredActiveItems.count)")
                    .font(DesignFonts.tabularBody(11, weight: .medium))
                    .foregroundStyle(palette.accent.color)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 1)
                    .frame(minWidth: 22)
                    .background(Capsule().fill(palette.accentSoft.color))
                    .overlay(Capsule().strokeBorder(palette.accentBorder.color, lineWidth: 1))

                Spacer(minLength: 4)

                HStack(spacing: 4) {
                    if model.supportsWorkspaceGrouping {
                        Button {
                            model.groupingMode = (model.groupingMode == .byWorkspace) ? .flat : .byWorkspace
                        } label: {
                            LucideIcon(.folderGit2, size: 15)
                                .foregroundStyle((model.groupingMode == .byWorkspace ? palette.accent : palette.textMuted).color)
                                .frame(width: 28, height: 28)
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

                    if workspace == .codex, store.target == .local, store.capabilities.codexDesktop,
                       let slot = codexNewThreadSlot
                    {
                        slot({ newId in
                            model.query = ""
                            model.workspaceFilter = .all
                            model.selectedKey = newId
                            Task { await model.fetchSessionList() }
                        }, { createErr in
                            model.codexCreateError = createErr
                        })
                    }

                    Button {
                        Task { await model.fetchSessionList() }
                    } label: {
                        LucideIcon(.refreshCw, size: 15)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 28))
                    .accessibilityLabel("刷新列表")
                }
            }
            .padding(.horizontal, 4)

            if let createErr = model.codexCreateError, !createErr.isEmpty {
                Text(createErr)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
                    .padding(.vertical, 8)
                    .padding(.horizontal, 10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 10).fill(palette.codeBg.color))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
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
                    if model.filteredActiveItems.isEmpty
                        && model.errorMessage == nil
                        && (!model.query.isEmpty || model.workspaceFilter != .all)
                    {
                        Text("无匹配会话")
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.textMuted.color)
                            .padding(.vertical, 12)
                            .padding(.horizontal, 10)
                    } else if model.supportsWorkspaceGrouping && model.groupingMode == .byWorkspace {
                        ForEach(model.activeWorkspaceGroups) { group in
                            let expanded = !model.collapsedWorkspaceKeys.contains(group.key)
                            ProjectWorkspaceGroupHeaderView(
                                displayName: group.displayName,
                                fullPath: group.fullPath,
                                count: group.items.count,
                                status: group.aggregateStatus,
                                expanded: expanded,
                                isFilteredToThisGroup: model.workspaceFilter.selectedKey == group.key,
                                onToggleExpanded: {
                                    if expanded {
                                        model.collapsedWorkspaceKeys.insert(group.key)
                                    } else {
                                        model.collapsedWorkspaceKeys.remove(group.key)
                                    }
                                },
                                onFilterOnlyThisGroup: {
                                    if model.workspaceFilter.selectedKey == group.key {
                                        model.workspaceFilter = .all
                                    } else {
                                        model.workspaceFilter = .workspace(group.key, includeSubdirectories: false)
                                    }
                                },
                                onSelectAllInGroup: model.provider == .grokbot ? nil : {
                                    model.selectAllInWorkspaceGroup(keys: group.items.map(\.sessionKey), partition: .active)
                                },
                                onCopyPath: { path in
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(path, forType: .string)
                                    model.feedbackMessage = "已复制工作区路径"
                                }
                            )
                            if expanded {
                                ForEach(group.items) { item in
                                    sessionRowView(
                                        item,
                                        partition: .active,
                                        groupedSubtitleOverride: group.subtitlesByItemId[item.sessionKey] ?? ""
                                    )
                                }
                            }
                        }
                    } else {
                        ForEach(model.filteredActiveItems) { item in
                            sessionRowView(item, partition: .active, groupedSubtitleOverride: nil)
                        }
                    }

                    if model.supportsArchive && model.archivedOpen {
                        VStack(alignment: .leading, spacing: 4) {
                            WorkspaceArchivedToggleRow(isOpen: $model.archivedOpen, archivedCount: model.filteredArchivedItems.count)
                            if model.supportsWorkspaceGrouping && model.groupingMode == .byWorkspace {
                                ForEach(model.archivedWorkspaceGroups) { group in
                                    let expanded = !model.collapsedArchivedWorkspaceKeys.contains(group.key)
                                    ProjectWorkspaceGroupHeaderView(
                                        displayName: group.displayName,
                                        fullPath: group.fullPath,
                                        count: group.items.count,
                                        status: .idle,
                                        expanded: expanded,
                                        isFilteredToThisGroup: model.workspaceFilter.selectedKey == group.key,
                                        onToggleExpanded: {
                                            if expanded {
                                                model.collapsedArchivedWorkspaceKeys.insert(group.key)
                                            } else {
                                                model.collapsedArchivedWorkspaceKeys.remove(group.key)
                                            }
                                        },
                                        onFilterOnlyThisGroup: {
                                            if model.workspaceFilter.selectedKey == group.key {
                                                model.workspaceFilter = .all
                                            } else {
                                                model.workspaceFilter = .workspace(group.key, includeSubdirectories: false)
                                            }
                                        },
                                        onSelectAllInGroup: {
                                            model.selectAllInWorkspaceGroup(keys: group.items.map(\.sessionKey), partition: .archived)
                                        },
                                        onCopyPath: { path in
                                            NSPasteboard.general.clearContents()
                                            NSPasteboard.general.setString(path, forType: .string)
                                            model.feedbackMessage = "已复制工作区路径"
                                        }
                                    )
                                    if expanded {
                                        ForEach(group.items) { item in
                                            sessionRowView(
                                                item,
                                                partition: .archived,
                                                groupedSubtitleOverride: group.subtitlesByItemId[item.sessionKey] ?? ""
                                            )
                                        }
                                    }
                                }
                            } else {
                                ForEach(model.filteredArchivedItems) { item in
                                    sessionRowView(item, partition: .archived, groupedSubtitleOverride: nil)
                                }
                            }
                        }
                        .padding(.top, 8)
                        .overlay(alignment: .top) {
                            Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                        }
                    }
                }
            }
            .frame(maxHeight: .infinity, alignment: .top)

            if model.supportsArchive && !model.archivedOpen {
                VStack(alignment: .leading, spacing: 0) {
                    WorkspaceArchivedToggleRow(isOpen: $model.archivedOpen, archivedCount: model.filteredArchivedItems.count)
                }
                .padding(.top, 8)
                .overlay(alignment: .top) {
                    Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                }
            }

            if model.provider != .grokbot {
                SessionBatchBarAndDialogView(
                    selectedCount: activeSel.keys.count,
                    operations: model.batchOperations(for: activePart),
                    busy: model.batchBusy,
                    notice: activeSel.notice,
                    prepareError: model.batchPrepareError,
                    executeError: model.batchExecuteError,
                    preview: model.batchPreview,
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
                    },
                    onConfirmExecute: { Task { await model.executePreparedBatch() } },
                    onDismissPreview: { model.batchPreview = nil }
                )
            }
        }
        .padding(.top, 14)
        .padding(.horizontal, 10)
        .padding(.bottom, 10)
    }

    private func sessionRowView(
        _ item: UnifiedSessionSummaryItem,
        partition: SessionSelectionPartition,
        groupedSubtitleOverride: String?
    ) -> some View {
        let partState = model.partitionedSelection.state(for: partition)
        let isSelected = (model.selectedItem?.sessionKey == item.sessionKey)
        let isMulti = partState.keys.contains(item.sessionKey)
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
            onToggleCheckbox: {
                guard !model.batchBusy else { return }
                let partVisibleKeys = (partition == .archived)
                    ? model.visualOrderedArchivedKeys
                    : model.visualOrderedActiveKeys
                _ = model.partitionedSelection.click(
                    partition: partition,
                    visible: partVisibleKeys,
                    key: item.sessionKey,
                    click: SessionClick(detail: 1, metaKey: true),
                    activeKey: model.selectedItem?.sessionKey
                )
                model.batchPreview = nil
                model.batchResult = nil
                model.batchPrepareError = nil
                model.batchExecuteError = nil
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
            let mainColumnWidth = (subagentDrawerOpen && canDockSubagents)
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

                        // .subagent-summary bar directly below glass header (DESIGN.md:113, styles.css:790-794)
                        if showSubagentSummaryBar {
                            SubagentSummaryBarView(
                                provider: model.provider,
                                collection: model.subagents,
                                isSupported: subagentsSupported,
                                capabilitiesReady: store.capabilities.isReady
                            ) { triggerResponder in
                                subagentTriggerResponder = triggerResponder
                                selectedSubagent = nil
                                subagentDrawerOpen.toggle()
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
                            VStack(alignment: .leading, spacing: 6) {
                                HStack(spacing: 8) {
                                    Text(blockReason)
                                        .font(DesignFonts.body(12))
                                        .foregroundStyle(palette.textSecondary.color)
                                    Spacer()
                                    if store.target == .local {
                                        Button("查看占用详情") {
                                            Task { await model.fetchClaudeOwnershipDiagnostics(sessionKey: item.sessionKey) }
                                        }
                                        .buttonStyle(SecondaryButtonStyle())
                                        .disabled(model.claudeOwnershipLoading)
                                    }
                                }

                                if store.target == .local, model.claudeOwnershipKey == item.sessionKey {
                                    if let diagErr = model.claudeOwnershipError {
                                        Text(diagErr)
                                            .font(DesignFonts.body(12))
                                            .foregroundStyle(palette.errFg.color)
                                    }
                                    if let diag = model.claudeOwnershipData {
                                        let summaryText = switch diag.ownership {
                                        case "inactive": "未发现关联的活跃会话进程"
                                        case "open": "存在活跃会话或文件占用"
                                        default: "归属尚未确认"
                                        }
                                        Text(summaryText)
                                            .font(DesignFonts.body(12, weight: .medium))
                                            .foregroundStyle(palette.textPrimary.color)
                                        ForEach(Array(diag.processes.enumerated()), id: \.offset) { _, proc in
                                            let roleLabel = switch proc.role {
                                            case "session": "会话"
                                            case "daemon": "后台服务"
                                            case "pty_host": "终端宿主"
                                            case "unknown": "未核验进程"
                                            default: proc.role
                                            }
                                            let mismatch = proc.mismatchedFields.isEmpty
                                                ? ""
                                                : " · 不匹配字段：\(proc.mismatchedFields.joined(separator: "、"))"
                                            Text("PID \(proc.pid) · \(roleLabel) · \(proc.registrationVerified ? "登记已核验" : "登记未核验")\(mismatch)")
                                                .font(DesignFonts.mono(11))
                                                .foregroundStyle(palette.textSecondary.color)
                                        }
                                    }
                                }
                            }
                            .padding(.horizontal, 20)
                            .padding(.vertical, 8)
                            .background(palette.surface.color)
                            .overlay(alignment: .bottom) {
                                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                            }
                        }

                        ZStack(alignment: .bottomTrailing) {
                            ZStack(alignment: .top) {
                                ZStack(alignment: .leading) {
                                    if let coord = coordinatorRef {
                                        ConversationReaderTableView(
                                            coordinator: coord,
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
                                    }

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
                                    searchModalOpen = true
                                }
                            }
                            .padding(.trailing, 16)
                            .padding(.bottom, 16)
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)

                        if workspace == .codex,
                           !item.archived,
                           store.target == .local,
                           store.capabilities.codexDesktop,
                           let footerSlot = codexCollaborationFooterSlot
                        {
                            footerSlot(
                                item.sessionKey,
                                item.cwd.isEmpty ? nil : item.cwd,
                                mainColumnWidth,
                                availableHeight
                            ) {
                                model.scheduleDebouncedCodexTailRefresh()
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)

                    if subagentDrawerOpen && canDockSubagents {
                        SubagentPanelView(
                            store: store,
                            provider: model.provider,
                            rootSessionKey: item.sessionKey,
                            collection: model.subagents,
                            initialAgent: selectedSubagent,
                            scheme: scheme,
                            workspace: workspace,
                            isModalOverlay: false,
                            onClose: { subagentDrawerOpen = false }
                        )
                    }
                }

                // <960pt Overlay Subagent Drawer with .34 Backdrop, Isolated Focus Root & Trigger Focus Restore
                if subagentDrawerOpen && !canDockSubagents {
                    Color.black.opacity(0.34)
                        .ignoresSafeArea()
                        .onTapGesture { subagentDrawerOpen = false }

                    SubagentOverlayFocusContainerView(palette: palette) {
                        SubagentPanelView(
                            store: store,
                            provider: model.provider,
                            rootSessionKey: item.sessionKey,
                            collection: model.subagents,
                            initialAgent: selectedSubagent,
                            scheme: scheme,
                            workspace: workspace,
                            isModalOverlay: true,
                            restoreFocusResponder: subagentTriggerResponder,
                            onClose: { subagentDrawerOpen = false }
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

    private func ensureCoordinator() {
        guard coordinatorRef == nil else { return }
        let c = ConversationTableCoordinator(store: store)
        c.onLoadOlderRequested = {
            guard model.loadOlderError == nil,
                  !model.timelineWindow.isActive || model.timelineWindow.error == nil
            else { return }
            Task { await model.loadOlderPageIfNeeded() }
        }
        c.onShowAllHistory = {
            model.showAllHistory = true
            if let item = model.selectedItem, model.provider == .codex {
                model.threadStore.setHistoryExpanded(threadId: item.sessionKey, expanded: true)
            }
        }
        c.onOpenSubagent = { sub in
            guard model.subagentsSupportedForCurrentProvider else { return }
            subagentTriggerResponder = NSApp.keyWindow?.firstResponder
            selectedSubagent = sub
            subagentDrawerOpen = true
        }
        c.onPreviewImage = { title, img in
            previewImageItem = (title, img)
        }
        c.onCopyFeedback = { text in
            model.feedbackMessage = text
        }
        c.onActiveTimelineChanged = { [weak timelineTracker] id in
            DispatchQueue.main.async {
                if timelineTracker?.activeId != id {
                    timelineTracker?.activeId = id
                }
            }
        }
        model.coordinatorRef = c
        coordinatorRef = c
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
                    // Codex sidebar row menu uses prepareSingle preview confirmation (ChatWorkspace.tsx:318)
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
