import AppKit
import SwiftUI

// MARK: - Normalized Session Row Adapter Across All 6 Providers

public struct UnifiedSessionSummaryItem: Equatable, Sendable, Identifiable {
    public var sessionKey: String
    public var nativeId: String
    public var title: String
    public var cwd: String
    public var path: String
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
    public let isCompactMobile: Bool
    public let scheme: ColorSchemeKind
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
        isCompactMobile: Bool,
        scheme: ColorSchemeKind,
        codexNewThreadSlot: (@MainActor (@escaping (String) -> Void, @escaping (String?) -> Void) -> AnyView)? = nil,
        codexCollaborationFooterSlot: (@MainActor (String, String?, CGFloat, CGFloat, @escaping @MainActor () -> Void) -> AnyView)? = nil
    ) {
        _model = StateObject(wrappedValue: ProviderWorkspaceStore(store: store, workspace: workspace, isCompactMobile: isCompactMobile))
        self.store = store
        self.workspace = workspace
        self.isCompactMobile = isCompactMobile
        self.scheme = scheme
        self.codexNewThreadSlot = codexNewThreadSlot
        self.codexCollaborationFooterSlot = codexCollaborationFooterSlot
    }

    public var body: some View {
        ZStack {
            GeometryReader { geo in
                HStack(spacing: 0) {
                    if !isCompactMobile || !model.mobileDetailOpen || model.selectedItem == nil {
                        sidebarColumn
                            .frame(width: isCompactMobile ? geo.size.width : LayoutMetrics.sessionListWidth)
                            .frame(maxHeight: .infinity)
                            .background(palette.surface.color)
                            .overlay(alignment: .trailing) {
                                if !isCompactMobile {
                                    Rectangle().fill(palette.borderSubtle.color).frame(width: 1)
                                }
                            }
                    }

                    if !isCompactMobile || (model.mobileDetailOpen && model.selectedItem != nil) {
                        let stageWidth = isCompactMobile
                            ? geo.size.width
                            : max(320, geo.size.width - LayoutMetrics.sessionListWidth)
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
                ModalConfirmDialogView(busy: model.batchBusy, onCancel: { model.batchPreview = nil }) {
                    SessionBatchPreviewDialogContentView(
                        preview: preview,
                        busy: model.batchBusy,
                        onCancel: { model.batchPreview = nil },
                        onConfirm: { Task { await model.executePreparedBatch() } }
                    )
                }
            }

            if let claudePreview = model.claudeDeletePreview {
                ModalConfirmDialogView(busy: model.batchBusy, onCancel: {
                    model.claudeDeletePreview = nil
                    model.claudeDeleteError = nil
                    model.claudeDeleteResiduals = []
                }) {
                    SingleSessionDeleteConfirmContentView(
                        title: claudePreview.title,
                        paths: claudePreview.resolvedPaths,
                        summaryText: "共 \(claudePreview.fileCount) 个文件 · \(SessionStorageFormatter.formatThousands(claudePreview.bytes)) 字节。仅删除上述会话文件，工作目录和配置保留。",
                        busy: model.batchBusy,
                        errorText: model.claudeDeleteError,
                        warnings: claudePreview.warnings ?? [],
                        residuals: model.claudeDeleteResiduals,
                        onCancel: {
                            model.claudeDeletePreview = nil
                            model.claudeDeleteError = nil
                            model.claudeDeleteResiduals = []
                        },
                        onConfirm: { Task { await model.confirmClaudeDelete(claudePreview) } }
                    )
                }
            }

            if let grokPreview = model.grokDeletePreview {
                ModalConfirmDialogView(busy: model.batchBusy, onCancel: {
                    model.grokDeletePreview = nil
                    model.grokDeleteError = nil
                    model.grokDeleteResiduals = []
                }) {
                    SingleSessionDeleteConfirmContentView(
                        title: grokPreview.title,
                        paths: grokPreview.path.isEmpty ? [] : [grokPreview.path],
                        summaryText: "共 \(grokPreview.fileCount) 个文件 · \(SessionStorageFormatter.formatThousands(grokPreview.bytes)) 字节。仅删除上述任务文件，工作目录和配置保留。",
                        busy: model.batchBusy,
                        errorText: model.grokDeleteError,
                        residuals: model.grokDeleteResiduals,
                        onCancel: {
                            model.grokDeletePreview = nil
                            model.grokDeleteError = nil
                            model.grokDeleteResiduals = []
                        },
                        onConfirm: { Task { await model.confirmGrokDelete(grokPreview) } }
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
            WorkspaceKeyboardShortcutMonitorView(
                isModalOpen: store.hasAnyActiveModal || ModalPresentationRegistry.shared.hasActiveModal || searchModalOpen || model.batchPreview != nil || model.claudeDeletePreview != nil || model.grokDeletePreview != nil || previewImageItem != nil,
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
            .frame(width: 0, height: 0)
        )
        .onAppear { ensureCoordinator() }
        .onChange(of: isCompactMobile) { model.isCompactMobile = $0 }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSearchRequested)) { _ in
            searchModalOpen = true
        }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubOpenMobileThreadsRequested)) { _ in
            if workspace == .codex {
                model.mobileDetailOpen = false
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSelectAllRequested)) { _ in
            guard model.provider != .grokbot,
                  !model.batchBusy,
                  !store.hasAnyActiveModal,
                  !ModalPresentationRegistry.shared.hasActiveModal,
                  !searchModalOpen,
                  model.batchPreview == nil,
                  model.claudeDeletePreview == nil,
                  model.grokDeletePreview == nil,
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
        .onChange(of: store.revision) { _ in
            subagentDrawerOpen = false
            selectedSubagent = nil
            model.revealedPositionKey = nil
            model.pendingSearch = nil
            model.loadOlderError = nil
            model.detailLoadedForKey = nil
            readWarningExpanded = false
            model.claudeOwnershipKey = nil
            model.claudeOwnershipData = nil
            model.claudeOwnershipError = nil
            model.batchPreview = nil
            model.batchResult = nil
            model.batchPrepareError = nil
            model.batchExecuteError = nil
            model.claudeDeletePreview = nil
            model.claudeDeleteError = nil
            model.claudeDeleteResiduals = []
            model.grokDeletePreview = nil
            model.grokDeleteError = nil
            model.grokDeleteResiduals = []
            model.codexCreateError = nil
            model.timelineWindow.returnToLatest()
            model.claudeHistoryStore.clearAll()
            model.grokHistoryStore.clearAll()
            model.providerHistoryStore.clearAll()
        }
        .onDisappear { model.codexTailRefreshTask?.cancel() }
        .onChange(of: model.selectedItem?.sessionKey) { _ in
            model.codexTailRefreshTask?.cancel()
            subagentDrawerOpen = false
            selectedSubagent = nil
            if model.searchSelection?.sessionKey != model.selectedKey { model.searchSelection = nil }
            if model.pendingSearch?.sessionKey != model.selectedKey { model.pendingSearch = nil }
            previewImageItem = nil
            model.errorMessage = nil
            model.feedbackMessage = nil
            model.currentDetailHistory = nil
            model.headerInlineRename = false
            model.loadingOlder = false
            model.hasMoreOlder = false
            model.beforeCursor = nil
            model.showAllHistory = false
            model.detailLoadedForKey = nil
            model.latestReplyText = nil
            model.rows = []
            model.timelineEntries = []
            model.subagents = nil
            model.revealedPositionKey = nil
            model.loadOlderError = nil
            readWarningExpanded = false
            model.timelineWindow.returnToLatest()
        }
        // 5s Session List Polling Clock (paused when window hidden)
        .task(id: "\(store.revision):\(workspace.rawValue):\(model.query):\(model.archivedOpen)") {
            model.syncPartitionedSelection()
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await model.fetchSessionList()
                }
                try? await Task.sleep(nanoseconds: 5_000_000_000)
            }
        }
        // Exact Detail Polling Clock (Claude/Grok 1s/2s; Codex/Antigravity/DSH/GrokBot 2s/5s)
        .task(id: "\(store.revision):\(workspace.rawValue):\(model.selectedItem?.sessionKey ?? "none"):\(model.showAllHistory)") {
            guard let item = model.selectedItem else {
                model.rows = []
                model.timelineEntries = []
                return
            }
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await model.fetchSessionDetail(for: item)
                }
                let isFastProvider = (model.provider == .claudeCode || model.provider == .grok)
                let running = item.running
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
        case .codex: "搜索标题或 ID"
        case .claudeCode: "搜索标题、ID 或工作目录"
        case .grok: "搜索任务"
        default: "搜索标题、ID 或工作目录"
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

                Text("\(model.activeItems.count)")
                    .font(DesignFonts.tabularBody(11, weight: .medium))
                    .foregroundStyle(palette.accent.color)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 1)
                    .frame(minWidth: 22)
                    .background(Capsule().fill(palette.accentSoft.color))
                    .overlay(Capsule().strokeBorder(palette.accentBorder.color, lineWidth: 1))

                Spacer(minLength: 4)

                HStack(spacing: 4) {
                    if workspace == .codex, store.target == .local, store.capabilities.codexDesktop,
                       let slot = codexNewThreadSlot
                    {
                        slot({ newId in
                            model.query = ""
                            model.selectedKey = newId
                            model.mobileDetailOpen = true
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

            HStack(spacing: 8) {
                LucideIcon(.search, size: 15)
                    .foregroundStyle(palette.textMuted.color)
                TextField(searchPlaceholderText, text: $model.query)
                    .textFieldStyle(.plain)
                    .font(DesignFonts.body(13))
                if !model.query.isEmpty {
                    Button { model.query = "" } label: { LucideIcon(.x, size: 13) }
                        .buttonStyle(IconButtonStyle(dimension: 22))
                }
            }
            .padding(.horizontal, 10)
            .frame(height: 34)
            .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))

            if let err = model.errorMessage {
                Text(err)
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.errFg.color)
                    .lineLimit(2)
            }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(model.activeItems) { item in
                        sessionRowView(item, partition: .active)
                    }

                    if model.supportsArchive && model.archivedOpen {
                        VStack(alignment: .leading, spacing: 4) {
                            archivedToggleButton
                            ForEach(model.archivedItems) { item in
                                sessionRowView(item, partition: .archived)
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
                    archivedToggleButton
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

    private var archivedToggleButton: some View {
        Button {
            model.archivedOpen.toggle()
        } label: {
            HStack(spacing: 8) {
                LucideIcon(model.archivedOpen ? .chevronDown : .chevronRight, size: 14)
                    .foregroundStyle((model.archivedOpen ? palette.accent : palette.textMuted).color)
                Text("已归档")
                    .font(DesignFonts.body(12, weight: .medium))
                    .foregroundStyle(palette.textMuted.color)
                Spacer()
                if model.archivedOpen {
                    Text("\(model.archivedItems.count)")
                        .font(DesignFonts.tabularBody(11, weight: .medium))
                        .foregroundStyle(palette.textMuted.color)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(palette.elevated.color))
                }
            }
            .padding(.vertical, 6)
            .padding(.horizontal, 10)
            .frame(minHeight: 32)
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
    }

    private func sessionRowView(_ item: UnifiedSessionSummaryItem, partition: SessionSelectionPartition) -> some View {
        let partState = model.partitionedSelection.state(for: partition)
        let isSelected = (model.selectedItem?.sessionKey == item.sessionKey)
        let isMulti = partState.keys.contains(item.sessionKey)

        return RenameableSessionRowView(
            sessionKey: item.sessionKey,
            title: item.title,
            idLine: (model.provider == .claudeCode && item.title != item.nativeId) ? item.nativeId : nil,
            subtitle: model.provider == .codex ? "" : item.cwd,
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
                        model.mobileDetailOpen = true
                    }
                    return
                }
                guard !model.batchBusy else { return }
                let partVisibleKeys = (partition == .archived ? model.archivedItems : model.activeItems).map(\.sessionKey)
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
                    model.mobileDetailOpen = true
                    model.headerInlineRename = false
                }
            },
            onToggleCheckbox: {
                guard !model.batchBusy else { return }
                let partVisibleKeys = (partition == .archived ? model.archivedItems : model.activeItems).map(\.sessionKey)
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
            let subagentsSupported = store.capabilities.isReady && (model.provider == .codex || model.provider == .claudeCode)
            let mainColumnWidth = (subagentDrawerOpen && canDockSubagents)
                ? max(320, availableWidth - LayoutMetrics.subagentDrawerWidth)
                : availableWidth
            let currentRows = model.displayedRows
            let showSubagentSummaryBar = (model.provider == .codex || model.provider == .claudeCode)
                && SubagentSummaryBarView.shouldDisplay(
                    provider: model.provider,
                    collection: model.subagents,
                    isSupported: subagentsSupported
                )

            ZStack(alignment: .trailing) {
                HStack(spacing: 0) {
                    VStack(spacing: 0) {
                        WorkspaceStageHeaderView(
                            title: item.title,
                            subtitle: (model.provider == .codex || item.cwd.isEmpty) ? nil : item.cwd,
                            statusBadge: item.statusChipBadge,
                            statusTone: item.statusChipTone,
                            availableWidth: mainColumnWidth,
                            isCompactMobile: isCompactMobile,
                            onBack: { model.mobileDetailOpen = false },
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
                                isSupported: subagentsSupported
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
                                            isLoadOlderBlockedByError: model.loadOlderError != nil
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
                                    hasNewer: model.timelineWindow.hasNewer(latestTailId: model.rows.last?.timelineId),
                                    onLoadNewer: {
                                        Task { await model.loadNewerTimelineWindowPage() }
                                    },
                                    onReturnLatest: {
                                        model.returnFromTimelineWindowToLatest()
                                    }
                                )

                                FloatingSearchPillButton(isCompactMobile: isCompactMobile) {
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
            guard model.loadOlderError == nil else { return }
            Task { await model.loadOlderPageIfNeeded() }
        }
        c.onShowAllHistory = {
            model.showAllHistory = true
            if let item = model.selectedItem, model.provider == .codex {
                model.threadStore.setHistoryExpanded(threadId: item.sessionKey, expanded: true)
            }
        }
        c.onOpenSubagent = { sub in
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
