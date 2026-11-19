import AppKit
import SwiftUI

// MARK: - Normalized Session Row Adapter Across All 6 Providers

public struct UnifiedSessionSummaryItem: Equatable, Sendable, Identifiable {
    public var sessionKey: String
    public var nativeId: String
    public var title: String
    public var cwd: String
    public var path: String
    public var statusText: String
    public var running: Bool
    public var archived: Bool
    public var canRename: Bool
    public var renameBlockReason: String?
    public var canArchive: Bool
    public var canDelete: Bool
    public var storageSize: SessionStorageSize?
    public var readError: String?
    public var readWarning: String?

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

    @StateObject private var threadStore = ThreadMessageStore()
    @StateObject private var claudeHistoryStore = PagedEventHistoryStore<ClaudeHistoryEvent> { ev, _ in ev.id }
    @StateObject private var grokHistoryStore = PagedEventHistoryStore<GrokHistoryEvent> { ev, idx in
        ev.id ?? ev.userMessage?.id ?? "\(ev.kind):\(ev.callId ?? ""):\(ev.timestamp ?? ""):\(idx)"
    }
    @StateObject private var providerHistoryStore = PagedEventHistoryStore<ProviderHistoryEvent> { ev, _ in ev.id }
    @StateObject private var timelineWindow = TimelineWindowController<ConversationRowItem>(idOf: \.timelineId)
    @StateObject private var motionObserver = AccessibilityMotionObserver()

    @State private var query: String = ""
    @State private var activeItems: [UnifiedSessionSummaryItem] = []
    @State private var archivedItems: [UnifiedSessionSummaryItem] = []
    @State private var archivedOpen: Bool = false
    @State private var selectedKey: String?
    @State private var mobileDetailOpen: Bool = false

    @State private var rows: [ConversationRowItem] = []
    @State private var currentDetailHistory: HistoryMetadata?
    @State private var detailLoading: Bool = false
    @State private var detailLoadedForKey: String?
    @State private var timelineEntries: [TimelineEntry] = []
    @State private var activeTimelineId: String?
    @State private var subagents: SubagentCollection?
    @State private var beforeCursor: String?
    @State private var hasMoreOlder: Bool = false
    @State private var loadingOlder: Bool = false
    @State private var loadOlderError: String?
    @State private var showAllHistory: Bool = false
    @State private var revealedPositionKey: String?
    @State private var pendingJumpPositionKey: String?
    @State private var latestReplyText: String?
    @State private var renameRequestSeqByKey: [String: Int] = [:]
    @State private var codexTailRefreshTask: Task<Void, Never>?
    @State private var codexCreateError: String?

    @State private var partitionedSelection = PartitionedSessionSelectionState(activeScope: "init")
    @State private var batchTargetPartition: SessionSelectionPartition = .active
    @State private var batchBusy: Bool = false
    @State private var batchPrepareError: String?
    @State private var batchExecuteError: String?
    @State private var batchPreview: SessionBatchPreview?
    @State private var batchResult: SessionBatchResult?
    @State private var claudeDeletePreview: ClaudeDeletePreview?
    @State private var claudeDeleteError: String?
    @State private var claudeDeleteResiduals: [String] = []
    @State private var grokDeletePreview: GrokDeletePreview?
    @State private var grokDeleteError: String?
    @State private var grokDeleteResiduals: [String] = []

    @State private var headerInlineRename: Bool = false
    @State private var readWarningExpanded: Bool = false
    @State private var claudeOwnershipKey: String?
    @State private var claudeOwnershipData: ClaudeOwnershipDiagnostics?
    @State private var claudeOwnershipError: String?
    @State private var claudeOwnershipLoading: Bool = false
    @State private var searchModalOpen: Bool = false
    @State private var subagentDrawerOpen: Bool = false
    @State private var subagentTriggerResponder: NSResponder?
    @State private var selectedSubagent: SubagentActivity?
    @State private var previewImageItem: (title: String, image: NSImage)?
    @State private var feedbackMessage: String?
    @State private var errorMessage: String?
    @State private var coordinatorRef: ConversationTableCoordinator?

    public init(
        store: RuntimeConnectionStore,
        workspace: WorkspaceId,
        isCompactMobile: Bool,
        scheme: ColorSchemeKind,
        codexNewThreadSlot: (@MainActor (@escaping (String) -> Void, @escaping (String?) -> Void) -> AnyView)? = nil,
        codexCollaborationFooterSlot: (@MainActor (String, String?, CGFloat, CGFloat, @escaping @MainActor () -> Void) -> AnyView)? = nil
    ) {
        self.store = store
        self.workspace = workspace
        self.isCompactMobile = isCompactMobile
        self.scheme = scheme
        self.codexNewThreadSlot = codexNewThreadSlot
        self.codexCollaborationFooterSlot = codexCollaborationFooterSlot
    }

    public var provider: SessionProvider {
        SessionProvider.from(workspace: workspace) ?? .codex
    }

    public var supportsArchive: Bool {
        provider == .codex || provider == .antigravity || provider == .dsh
    }

    private var activeSelectionPartition: SessionSelectionPartition {
        if supportsArchive && archivedOpen && (partitionedSelection.archived.selecting || !partitionedSelection.archived.keys.isEmpty) {
            return .archived
        }
        return .active
    }

    private var currentSelectionState: SessionSelectionState {
        partitionedSelection.state(for: activeSelectionPartition)
    }

    public func batchOperations(for partition: SessionSelectionPartition) -> [SessionOperation] {
        if provider == .grokbot { return [] }
        if supportsArchive {
            return partition == .archived ? [.restore, .delete] : [.archive, .delete]
        }
        return [.delete]
    }

    public var batchOperations: [SessionOperation] {
        batchOperations(for: activeSelectionPartition)
    }

    public var allVisibleItems: [UnifiedSessionSummaryItem] {
        activeItems + (archivedOpen ? archivedItems : [])
    }

    public var selectedItem: UnifiedSessionSummaryItem? {
        if let key = selectedKey, let match = allVisibleItems.first(where: { $0.sessionKey == key }) {
            return match
        }
        return isCompactMobile ? nil : activeItems.first
    }

    private var displayedRows: [ConversationRowItem] {
        timelineWindow.items ?? rows
    }

    public var body: some View {
        ZStack {
            GeometryReader { geo in
                HStack(spacing: 0) {
                    if !isCompactMobile || !mobileDetailOpen || selectedItem == nil {
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

                    if !isCompactMobile || (mobileDetailOpen && selectedItem != nil) {
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
                    provider: provider,
                    currentSessionKey: selectedItem?.sessionKey,
                    onSelectResult: { res in
                        handleSearchResultSelected(res)
                    },
                    onClose: { searchModalOpen = false }
                )
            }

            if let preview = batchPreview {
                ModalConfirmDialogView(busy: batchBusy, onCancel: { batchPreview = nil }) {
                    SessionBatchPreviewDialogContentView(
                        preview: preview,
                        busy: batchBusy,
                        onCancel: { batchPreview = nil },
                        onConfirm: { Task { await executePreparedBatch() } }
                    )
                }
            }

            if let claudePreview = claudeDeletePreview {
                ModalConfirmDialogView(busy: batchBusy, onCancel: {
                    claudeDeletePreview = nil
                    claudeDeleteError = nil
                    claudeDeleteResiduals = []
                }) {
                    SingleSessionDeleteConfirmContentView(
                        title: claudePreview.title,
                        paths: claudePreview.resolvedPaths,
                        summaryText: "共 \(claudePreview.fileCount) 个文件 · \(SessionStorageFormatter.formatThousands(claudePreview.bytes)) 字节。仅删除上述会话文件，工作目录和配置保留。",
                        busy: batchBusy,
                        errorText: claudeDeleteError,
                        warnings: claudePreview.warnings ?? [],
                        residuals: claudeDeleteResiduals,
                        onCancel: {
                            claudeDeletePreview = nil
                            claudeDeleteError = nil
                            claudeDeleteResiduals = []
                        },
                        onConfirm: { Task { await confirmClaudeDelete(claudePreview) } }
                    )
                }
            }

            if let grokPreview = grokDeletePreview {
                ModalConfirmDialogView(busy: batchBusy, onCancel: {
                    grokDeletePreview = nil
                    grokDeleteError = nil
                    grokDeleteResiduals = []
                }) {
                    SingleSessionDeleteConfirmContentView(
                        title: grokPreview.title,
                        paths: grokPreview.path.isEmpty ? [] : [grokPreview.path],
                        summaryText: "共 \(grokPreview.fileCount) 个文件 · \(SessionStorageFormatter.formatThousands(grokPreview.bytes)) 字节。仅删除上述任务文件，工作目录和配置保留。",
                        busy: batchBusy,
                        errorText: grokDeleteError,
                        residuals: grokDeleteResiduals,
                        onCancel: {
                            grokDeletePreview = nil
                            grokDeleteError = nil
                            grokDeleteResiduals = []
                        },
                        onConfirm: { Task { await confirmGrokDelete(grokPreview) } }
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
                isModalOpen: store.hasAnyActiveModal || ModalPresentationRegistry.shared.hasActiveModal || searchModalOpen || batchPreview != nil || claudeDeletePreview != nil || grokDeletePreview != nil || previewImageItem != nil,
                onRenameSelected: {
                    guard provider != .grokbot, !currentSelectionState.selecting, let item = selectedItem else { return false }
                    renameRequestSeqByKey[item.sessionKey, default: 0] += 1
                    return true
                },
                onEscapeClearSelection: {
                    let part = activeSelectionPartition
                    guard partitionedSelection.state(for: part).selecting else { return false }
                    partitionedSelection.clear(partition: part)
                    return true
                },
                onBatchDelete: {
                    let part = activeSelectionPartition
                    let sel = partitionedSelection.state(for: part)
                    guard provider != .grokbot, !batchBusy, sel.selecting, !sel.keys.isEmpty else { return false }
                    Task { await prepareBatch(operation: .delete, keys: sel.keys, partition: part) }
                    return true
                }
            )
            .frame(width: 0, height: 0)
        )
        .onAppear { ensureCoordinator() }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSearchRequested)) { _ in
            searchModalOpen = true
        }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubOpenMobileThreadsRequested)) { _ in
            if workspace == .codex {
                mobileDetailOpen = false
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .agentHubSelectAllRequested)) { _ in
            guard provider != .grokbot,
                  !batchBusy,
                  !store.hasAnyActiveModal,
                  !ModalPresentationRegistry.shared.hasActiveModal,
                  !searchModalOpen,
                  batchPreview == nil,
                  claudeDeletePreview == nil,
                  grokDeletePreview == nil,
                  previewImageItem == nil
            else { return }
            selectAllVisible()
        }
        .onChange(of: archivedOpen) { isOpen in
            partitionedSelection.setArchivedOpen(isOpen)
            if !isOpen && batchTargetPartition == .archived {
                batchPreview = nil
                batchResult = nil
                batchPrepareError = nil
                batchExecuteError = nil
            }
        }
        .onChange(of: store.revision) { _ in
            subagentDrawerOpen = false
            selectedSubagent = nil
            revealedPositionKey = nil
            pendingJumpPositionKey = nil
            loadOlderError = nil
            detailLoadedForKey = nil
            readWarningExpanded = false
            claudeOwnershipKey = nil
            claudeOwnershipData = nil
            claudeOwnershipError = nil
            batchPreview = nil
            batchResult = nil
            batchPrepareError = nil
            batchExecuteError = nil
            claudeDeletePreview = nil
            claudeDeleteError = nil
            claudeDeleteResiduals = []
            grokDeletePreview = nil
            grokDeleteError = nil
            grokDeleteResiduals = []
            codexCreateError = nil
            timelineWindow.returnToLatest()
            claudeHistoryStore.clearAll()
            grokHistoryStore.clearAll()
            providerHistoryStore.clearAll()
        }
        .onChange(of: selectedItem?.sessionKey) { _ in
            revealedPositionKey = nil
            loadOlderError = nil
            readWarningExpanded = false
            timelineWindow.returnToLatest()
        }
        // 5s Session List Polling Clock (paused when window hidden)
        .task(id: "\(store.revision):\(workspace.rawValue):\(query):\(archivedOpen)") {
            syncPartitionedSelection()
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await fetchSessionList()
                }
                try? await Task.sleep(nanoseconds: 5_000_000_000)
            }
        }
        // Exact Detail Polling Clock (Claude/Grok 1s/2s; Codex/Antigravity/DSH/GrokBot 2s/5s)
        .task(id: "\(store.revision):\(workspace.rawValue):\(selectedItem?.sessionKey ?? "none"):\(showAllHistory)") {
            guard let item = selectedItem else {
                rows = []
                timelineEntries = []
                return
            }
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await fetchSessionDetail(for: item)
                }
                let isFastProvider = (provider == .claudeCode || provider == .grok)
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

    private var sidebarColumn: some View {
        let activePart = activeSelectionPartition
        let activeSel = partitionedSelection.state(for: activePart)

        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                AgentBrandIcon(brandId, size: 17)
                Text(workspace.label)
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                Text("\(activeItems.count)")
                    .font(DesignFonts.body(11, weight: .medium))
                    .foregroundStyle(palette.textMuted.color)
                Spacer()

                if workspace == .codex, store.target == .local, store.capabilities.codexDesktop,
                   let slot = codexNewThreadSlot
                {
                    slot({ newId in
                        query = ""
                        selectedKey = newId
                        mobileDetailOpen = true
                        Task { await fetchSessionList() }
                    }, { createErr in
                        codexCreateError = createErr
                    })
                }

                Button {
                    Task { await fetchSessionList() }
                } label: {
                    LucideIcon(.refreshCw, size: 15)
                }
                .buttonStyle(IconButtonStyle(dimension: 28))
                .accessibilityLabel("刷新列表")
            }

            if let createErr = codexCreateError, !createErr.isEmpty {
                Text(createErr)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
                    .padding(.vertical, 8)
                    .padding(.horizontal, 10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 10).fill(palette.codeBg.color))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
            }

            HStack(spacing: 6) {
                LucideIcon(.search, size: 14)
                    .foregroundStyle(palette.textMuted.color)
                TextField("搜索标题、ID 或目录", text: $query)
                    .textFieldStyle(.plain)
                    .font(DesignFonts.body(13))
                if !query.isEmpty {
                    Button { query = "" } label: { LucideIcon(.x, size: 13) }
                        .buttonStyle(IconButtonStyle(dimension: 22))
                }
            }
            .padding(.horizontal, 10)
            .frame(height: 34)
            .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))

            if let err = errorMessage {
                Text(err)
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.errFg.color)
                    .lineLimit(2)
            }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 3) {
                    ForEach(activeItems) { item in
                        sessionRowView(item, partition: .active)
                    }

                    if supportsArchive {
                        Button {
                            archivedOpen.toggle()
                        } label: {
                            HStack(spacing: 6) {
                                LucideIcon(archivedOpen ? .chevronDown : .chevronRight, size: 14)
                                    .foregroundStyle((archivedOpen ? palette.accent : palette.textMuted).color)
                                Text("已归档")
                                    .font(DesignFonts.body(12, weight: .medium))
                                    .foregroundStyle(palette.textMuted.color)
                                Spacer()
                                if archivedOpen {
                                    Text("\(archivedItems.count)")
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

                        if archivedOpen {
                            ForEach(archivedItems) { item in
                                sessionRowView(item, partition: .archived)
                            }
                        }
                    }
                }
            }

            if provider != .grokbot {
                SessionBatchBarAndDialogView(
                    selectedCount: activeSel.keys.count,
                    operations: batchOperations(for: activePart),
                    busy: batchBusy,
                    notice: activeSel.notice,
                    prepareError: batchPrepareError,
                    executeError: batchExecuteError,
                    preview: batchPreview,
                    result: batchResult,
                    onSelectAll: selectAllVisible,
                    onClear: {
                        partitionedSelection.clear(partition: activePart)
                        batchPreview = nil
                        batchResult = nil
                        batchPrepareError = nil
                        batchExecuteError = nil
                    },
                    onPrepareOperation: { op in
                        Task { await prepareBatch(operation: op, keys: activeSel.keys, partition: activePart) }
                    },
                    onConfirmExecute: { Task { await executePreparedBatch() } },
                    onDismissPreview: { batchPreview = nil }
                )
            }
        }
        .padding(12)
    }

    private func sessionRowView(_ item: UnifiedSessionSummaryItem, partition: SessionSelectionPartition) -> some View {
        let partState = partitionedSelection.state(for: partition)
        let partVisibleKeys = (partition == .archived ? archivedItems : activeItems).map(\.sessionKey)
        let partOps = batchOperations(for: partition)
        let isSelected = (selectedItem?.sessionKey == item.sessionKey)
        let isMulti = partState.keys.contains(item.sessionKey)
        let batchMenu = ProviderMenuMatrixBuilder.buildBatchContextMenuActions(
            selectedCount: partState.keys.count,
            operations: partOps,
            busy: batchBusy,
            onPrepare: { op in Task { await prepareBatch(operation: op, keys: partState.keys, partition: partition) } },
            onClear: {
                partitionedSelection.clear(partition: partition)
                batchPreview = nil
                batchResult = nil
                batchPrepareError = nil
                batchExecuteError = nil
            }
        )
        let rowActions = buildMenuActionsForItem(item, isHeaderMenu: false)

        return RenameableSessionRowView(
            sessionKey: item.sessionKey,
            title: item.title,
            subtitle: item.cwd,
            statusText: item.statusText,
            running: item.running,
            storageSize: item.storageSize,
            selected: isSelected,
            selecting: partState.selecting,
            multiSelected: isMulti,
            disabled: batchBusy,
            canRename: item.canRename,
            renameBlockReason: item.renameBlockReason,
            externalRenameRequestId: renameRequestSeqByKey[item.sessionKey, default: 0],
            rowMenuActions: rowActions,
            batchMenuActions: batchMenu,
            onRowClick: { click in
                if provider == .grokbot {
                    if !(click.ctrlKey && click.detail != 0) {
                        selectedKey = item.sessionKey
                        mobileDetailOpen = true
                    }
                    return
                }
                guard !batchBusy else { return }
                let shouldOpen = partitionedSelection.click(
                    partition: partition,
                    visible: partVisibleKeys,
                    key: item.sessionKey,
                    click: click,
                    activeKey: selectedItem?.sessionKey
                )
                if !(click.ctrlKey && click.detail != 0) {
                    batchPreview = nil
                    batchResult = nil
                    batchPrepareError = nil
                    batchExecuteError = nil
                }
                if shouldOpen {
                    selectedKey = item.sessionKey
                    mobileDetailOpen = true
                    headerInlineRename = false
                }
            },
            onToggleCheckbox: {
                guard !batchBusy else { return }
                _ = partitionedSelection.click(
                    partition: partition,
                    visible: partVisibleKeys,
                    key: item.sessionKey,
                    click: SessionClick(detail: 1, metaKey: true),
                    activeKey: selectedItem?.sessionKey
                )
                batchPreview = nil
                batchResult = nil
                batchPrepareError = nil
                batchExecuteError = nil
            },
            onRename: provider == .grokbot ? nil : { nextTitle in
                try await performRename(item: item, newTitle: nextTitle)
            }
        )
    }

    // MARK: - Stage Conversation Reading Column + Subagents + Floating Search

    @ViewBuilder
    private func stageArea(availableWidth: CGFloat, availableHeight: CGFloat) -> some View {
        if let item = selectedItem {
            let canDockSubagents = availableWidth >= LayoutMetrics.subagentDockThreshold
            let subagentsSupported = (provider == .codex && store.capabilities.threadSubagents)
                || (provider == .claudeCode && store.capabilities.claudeSubagents)
            let mainColumnWidth = (subagentDrawerOpen && canDockSubagents)
                ? max(320, availableWidth - LayoutMetrics.subagentDrawerWidth)
                : availableWidth
            let currentRows = displayedRows
            let showSubagentSummaryBar = (provider == .codex || provider == .claudeCode)
                && SubagentSummaryBarView.shouldDisplay(
                    provider: provider,
                    collection: subagents,
                    isSupported: subagentsSupported
                )

            ZStack(alignment: .trailing) {
                HStack(spacing: 0) {
                    VStack(spacing: 0) {
                        WorkspaceStageHeaderView(
                            title: item.title,
                            statusBadge: item.statusText,
                            statusTone: item.running ? .info : .neutral,
                            isCompactMobile: isCompactMobile,
                            onBack: { mobileDetailOpen = false },
                            inlineRenameActive: headerInlineRename,
                            initialRenameText: item.title,
                            onCommitInlineRename: { next in
                                try await performRename(item: item, newTitle: next)
                                headerInlineRename = false
                            },
                            onCancelInlineRename: { headerInlineRename = false }
                        ) {
                            let headerActions = buildMenuActionsForItem(item, isHeaderMenu: true)
                            Menu {
                                ForEach(headerActions) { act in
                                    Button(act.label, action: act.run)
                                        .disabled(act.disabled)
                                }
                            } label: {
                                LucideIcon(.moreHorizontal, size: 17)
                                    .frame(width: 32, height: 32)
                                    .contentShape(Rectangle())
                            }
                            .menuStyle(.borderlessButton)
                            .frame(width: 32, height: 32)
                            .accessibilityLabel("会话菜单")
                        }

                        // .subagent-summary bar directly below glass header (DESIGN.md:113, styles.css:790-794)
                        if showSubagentSummaryBar {
                            SubagentSummaryBarView(
                                provider: provider,
                                collection: subagents,
                                isSupported: subagentsSupported
                            ) { triggerResponder in
                                subagentTriggerResponder = triggerResponder
                                selectedSubagent = nil
                                subagentDrawerOpen.toggle()
                            }
                        }

                        if let fb = feedbackMessage, !fb.isEmpty {
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

                        if provider == .claudeCode,
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
                                            Task { await fetchClaudeOwnershipDiagnostics(sessionKey: item.sessionKey) }
                                        }
                                        .buttonStyle(SecondaryButtonStyle())
                                        .disabled(claudeOwnershipLoading)
                                    }
                                }

                                if store.target == .local, claudeOwnershipKey == item.sessionKey {
                                    if let diagErr = claudeOwnershipError {
                                        Text(diagErr)
                                            .font(DesignFonts.body(12))
                                            .foregroundStyle(palette.errFg.color)
                                    }
                                    if let diag = claudeOwnershipData {
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
                                            sessionScopeKey: "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)",
                                            sessionProvider: provider,
                                            sessionKey: item.sessionKey,
                                            cwd: item.cwd,
                                            hasTimelineRail: !timelineEntries.isEmpty,
                                            isSubagent: false,
                                            isLoadOlderBlockedByError: loadOlderError != nil
                                        )
                                    }

                                    if !detailLoading && detailLoadedForKey == item.sessionKey && currentRows.isEmpty {
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

                                    if !timelineEntries.isEmpty {
                                        TimelineRailView(
                                            entries: timelineEntries,
                                            activeId: activeTimelineId,
                                            availableWidth: mainColumnWidth,
                                            loadOlder: (timelineWindow.isActive ? timelineWindow.hasMoreOlder : hasMoreOlder) ? {
                                                Task {
                                                    loadOlderError = nil
                                                    await loadOlderPageIfNeeded()
                                                }
                                            } : nil,
                                            loadingOlder: loadingOlder || timelineWindow.busy
                                        ) { entry in
                                            Task { await handleTimelineTickSelected(entry) }
                                        }
                                    }
                                }

                                // Top History Loading / Error Retry Bar (halts <200pt auto-trigger on error, DESIGN.md:141)
                                HistoryLoadingBarView(
                                    loading: loadingOlder,
                                    error: loadOlderError,
                                    onRetry: {
                                        loadOlderError = nil
                                        Task { await loadOlderPageIfNeeded() }
                                    }
                                )
                            }

                            VStack(alignment: .trailing, spacing: 8) {
                                TimelineHistoryStatusView(
                                    active: timelineWindow.isActive,
                                    busy: timelineWindow.busy,
                                    error: timelineWindow.error,
                                    hasNewer: timelineWindow.hasNewer(latestTailId: rows.last?.timelineId),
                                    onLoadNewer: {
                                        Task { await loadNewerTimelineWindowPage() }
                                    },
                                    onReturnLatest: {
                                        returnFromTimelineWindowToLatest()
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
                                scheduleDebouncedCodexTailRefresh()
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)

                    if subagentDrawerOpen && canDockSubagents {
                        SubagentPanelView(
                            store: store,
                            provider: provider,
                            rootSessionKey: item.sessionKey,
                            collection: subagents,
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
                            provider: provider,
                            rootSessionKey: item.sessionKey,
                            collection: subagents,
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
            guard loadOlderError == nil else { return }
            Task { await loadOlderPageIfNeeded() }
        }
        c.onShowAllHistory = {
            showAllHistory = true
            if let item = selectedItem, provider == .codex {
                threadStore.setHistoryExpanded(threadId: item.sessionKey, expanded: true)
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
            feedbackMessage = text
        }
        c.onActiveTimelineChanged = { id in
            if activeTimelineId != id { activeTimelineId = id }
        }
        coordinatorRef = c
    }

    private func syncPartitionedSelection() {
        let baseScope = "\(store.revision):\(workspace.rawValue):\(query)"
        partitionedSelection.sync(
            activeScope: "\(baseScope)#active",
            activeVisible: activeItems.map(\.sessionKey),
            archivedScope: "\(baseScope)#archived",
            archivedVisible: archivedItems.map(\.sessionKey),
            archivedOpen: supportsArchive && archivedOpen
        )
    }

    private func selectAllVisible() {
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

    private func buildMenuActionsForItem(_ item: UnifiedSessionSummaryItem, isHeaderMenu: Bool) -> [SessionMenuAction] {
        ProviderMenuMatrixBuilder.buildActions(
            provider: provider,
            isHeaderMenu: isHeaderMenu,
            isLocalMachine: store.target == .local,
            id: item.nativeId,
            path: item.path,
            cwd: item.cwd,
            archived: item.archived,
            canRename: item.canRename,
            canArchive: item.canArchive,
            canDelete: item.canDelete,
            busy: batchBusy,
            latestReplyText: isHeaderMenu ? latestReplyText : nil,
            onRename: provider == .grokbot ? nil : {
                if isHeaderMenu {
                    headerInlineRename = true
                } else {
                    renameRequestSeqByKey[item.sessionKey, default: 0] += 1
                }
            },
            onToggleArchive: supportsArchive ? {
                Task { await toggleSingleArchive(item) }
            } : nil,
            onCopy: { text, msg in
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
                feedbackMessage = msg
            },
            onRevealInFinder: { path in
                Task {
                    try? await FileOpenerService.revealPaths([path])
                }
            },
            onDelete: provider == .grokbot ? nil : {
                Task { await triggerSingleDelete(item) }
            }
        )
    }

    private func fetchSessionList() async {
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
                activeItems = active.map { mapCodexSummary($0, archived: false) }
                if archivedOpen {
                    let arch: [ThreadSummary] = try await store.rpc(
                        "threads.list",
                        args: ListArgs(archived: true, q: query.isEmpty ? nil : query, limit: 120)
                    )
                    archivedItems = arch.map { mapCodexSummary($0, archived: true) }
                }

            case .claudeCode:
                struct ClaudeListArgs: Encodable { let q: String?; let limit: Int }
                let list: [ClaudeSessionSummary] = try await store.rpc(
                    "claude.list",
                    args: ClaudeListArgs(q: query.isEmpty ? nil : query, limit: 100)
                )
                activeItems = list.map { item in
                    let stLabel = if item.status == "running" {
                        "运行中"
                    } else if item.status == "waiting" {
                        "等待处理"
                    } else if item.readError != nil {
                        "异常"
                    } else {
                        "近期"
                    }
                    return UnifiedSessionSummaryItem(
                        sessionKey: item.sessionKey,
                        nativeId: item.id,
                        title: item.title.isEmpty ? item.id : item.title,
                        cwd: item.cwd,
                        path: item.path,
                        statusText: stLabel,
                        running: item.status == "running",
                        archived: false,
                        canRename: item.canRename,
                        renameBlockReason: item.renameBlockReason,
                        canArchive: false,
                        // Per DESIGN.md:75, Claude delete is never disabled by read/activity state.
                        canDelete: true,
                        storageSize: item.storageSize,
                        readError: item.readError,
                        readWarning: item.readWarning
                    )
                }

            case .grok:
                struct GrokListArgs: Encodable { let q: String?; let limit: Int }
                let list: [GrokSessionSummary] = try await store.rpc(
                    "grok.list",
                    args: GrokListArgs(q: query.isEmpty ? nil : query, limit: 100)
                )
                activeItems = list.map { item in
                    UnifiedSessionSummaryItem(
                        sessionKey: item.id,
                        nativeId: item.id,
                        title: item.title.isEmpty ? item.id : item.title,
                        cwd: item.cwd,
                        path: HistoryAndResumeHelpers.grokHandoffPath(item),
                        statusText: item.status == "running" ? "运行中" : "近期",
                        running: item.status == "running",
                        archived: false,
                        canRename: true,
                        renameBlockReason: nil,
                        canArchive: false,
                        // Per DESIGN.md:85, Grok delete is unconditionally enabled regardless of legacy eligibility.
                        canDelete: true,
                        storageSize: item.storageSize
                    )
                }

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
                activeItems = active.map(mapProviderSummary)
                if archivedOpen {
                    let arch: [ProviderSessionSummary] = try await store.rpc(
                        "\(prefix).list",
                        args: ProvListWrap(request: .init(q: query.isEmpty ? nil : query, archived: true, limit: 100))
                    )
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
                activeItems = list.map { item in
                    let st = item.status == "authorization" ? "需要授权"
                        : (item.status == "reply_needed" ? "需要回复" : (!item.path.isEmpty ? "近期缓存" : "未缓存"))
                    return UnifiedSessionSummaryItem(
                        sessionKey: item.sessionKey,
                        nativeId: item.id,
                        title: item.title.isEmpty ? item.id : item.title,
                        cwd: item.cwd,
                        path: item.path,
                        statusText: st,
                        running: false,
                        archived: false,
                        canRename: false,
                        renameBlockReason: nil,
                        canArchive: false,
                        canDelete: false,
                        storageSize: item.storageSize
                    )
                }
            }
            syncPartitionedSelection()
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func scheduleDebouncedCodexTailRefresh() {
        codexTailRefreshTask?.cancel()
        codexTailRefreshTask = Task { @MainActor in
            try? await Task.sleep(nanoseconds: 350_000_000)
            guard !Task.isCancelled, provider == .codex, let item = selectedItem else { return }
            await fetchSessionDetail(for: item)
        }
    }

    private func fetchSessionDetail(for item: UnifiedSessionSummaryItem) async {
        let historyScope = "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)"
        if detailLoadedForKey != item.sessionKey {
            detailLoading = true
        }
        defer { detailLoading = false }
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
                let slot = threadStore.applyDetail(detail)
                incomingHistory = detail.history
                subagents = slot.subagents
                hasMoreOlder = slot.hasMoreBlocks
                beforeCursor = slot.beforeCursor
                rows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: item.title,
                    subagentsSupported: store.capabilities.threadSubagents,
                    revealedBlockId: revealedPositionKey
                )
                timelineEntries = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .codex,
                    sources: slot.blocks.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                latestReplyText = rows.reversed().compactMap { r -> String? in
                    if case let .assistant(md, _) = r.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }
                    return nil
                }.first

            case .claudeCode:
                struct Req: Encodable { let sessionKey: String; let limit: Int }
                let detail: ClaudeSessionDetail = try await store.rpc(
                    "claude.detail",
                    args: Req(sessionKey: item.sessionKey, limit: 160)
                )
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
                subagents = slot.subagents
                hasMoreOlder = slot.hasMore
                beforeCursor = slot.beforeCursor
                rows = ConversationRowBuilder.buildClaudeRows(events: slot.events)
                timelineEntries = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .claudeCode,
                    sources: slot.events.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                latestReplyText = rows.reversed().compactMap { if case let .assistant(md, _) = $0.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }; return nil }.first

            case .grok:
                struct Req: Encodable { let id: String; let limit: Int }
                let detail: GrokSessionDetail = try await store.rpc(
                    "grok.detail",
                    args: Req(id: item.sessionKey, limit: 160)
                )
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
                subagents = nil
                hasMoreOlder = slot.hasMore
                beforeCursor = slot.beforeCursor
                rows = ConversationRowBuilder.buildGrokRows(events: slot.events, fallbackTitle: item.title)
                timelineEntries = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .grok,
                    sources: slot.events.enumerated().map { idx, ev in
                        TimelineSourceItem(id: ev.id ?? ev.userMessage?.id ?? "grok-\(idx)", role: nil, kind: ev.kind, text: ev.text, userMessage: ev.userMessage)
                    }
                )
                latestReplyText = rows.reversed().compactMap { if case let .assistant(md, _) = $0.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }; return nil }.first

            case .antigravity, .dsh, .grokbot:
                let cmd = provider == .antigravity ? "antigravity.detail" : (provider == .dsh ? "dsh.detail" : "grokbot.detail")
                struct Wrap: Encodable {
                    struct Inner: Encodable { let sessionKey: String; let limit: Int }
                    let request: Inner
                }
                let detail: ProviderSessionDetail = try await store.rpc(
                    cmd,
                    args: Wrap(request: .init(sessionKey: item.sessionKey, limit: 160)),
                    localOnly: provider == .grokbot
                )
                let slot = providerHistoryStore.applyHeadPoll(
                    key: historyScope,
                    headEvents: detail.events,
                    history: detail.history,
                    totalEvents: detail.totalEvents,
                    beforeCursor: detail.beforeCursor,
                    hasMore: detail.hasMore,
                    timeline: detail.timeline
                )
                incomingHistory = detail.history
                subagents = nil
                hasMoreOlder = slot.hasMore
                beforeCursor = slot.beforeCursor
                rows = ConversationRowBuilder.buildProviderRows(events: slot.events, providerLabel: workspace.label)
                timelineEntries = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: provider,
                    sources: slot.events.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                latestReplyText = rows.reversed().compactMap { if case let .assistant(md, _) = $0.kind { return VisibleMarkdownEngine.visibleMarkdown(md) }; return nil }.first
            }

            currentDetailHistory = incomingHistory
            detailLoadedForKey = item.sessionKey

            if timelineWindow.isActive {
                await timelineWindow.reconcilePollHistory(incoming: incomingHistory) { cursor in
                    try await readTimelineWindowPage(for: item, cursor: cursor)
                }
            }

            if let pending = pendingJumpPositionKey, selectedItem?.sessionKey == item.sessionKey {
                pendingJumpPositionKey = nil
                await jumpAndRevealPositionKey(pending)
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    @discardableResult
    private func loadOlderPageIfNeeded() async -> Bool {
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
        defer { loadingOlder = false }
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
                let slot = threadStore.applyBlockPage(page, expectedCursor: cursor)
                let advanced = (!page.blocks.isEmpty && slot.beforeCursor != cursor)
                hasMoreOlder = advanced ? slot.hasMoreBlocks : false
                beforeCursor = advanced ? slot.beforeCursor : nil
                let nextRows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: item.title,
                    subagentsSupported: store.capabilities.threadSubagents,
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
                struct Wrap: Encodable {
                    struct Inner: Encodable { let sessionKey: String; let limit: Int; let before: String }
                    let request: Inner
                }
                let page: ProviderSessionDetail = try await store.rpc(
                    cmd,
                    args: Wrap(request: .init(sessionKey: item.sessionKey, limit: 160, before: cursor)),
                    localOnly: provider == .grokbot
                )
                let slot = providerHistoryStore.applyOlderPage(
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
                let nextRows = ConversationRowBuilder.buildProviderRows(events: slot.events, providerLabel: workspace.label)
                coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: false)
                rows = nextRows
                return !page.events.isEmpty
            }
        } catch {
            loadOlderError = error.localizedDescription
            return false
        }
    }

    private func performRename(item: UnifiedSessionSummaryItem, newTitle: String) async throws {
        switch provider {
        case .codex:
            struct RenameArgs: Encodable { let threadId: String; let name: String }
            struct RenameAck: Decodable { let jobId: String?; let threadId: String? }
            let ack: RenameAck = try await store.rpc("threads.rename", args: RenameArgs(threadId: item.sessionKey, name: newTitle))
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

    private func toggleSingleArchive(_ item: UnifiedSessionSummaryItem) async {
        do {
            if provider == .codex {
                struct Args: Encodable { let threadId: String }
                struct Ack: Decodable { let jobId: String? }
                let cmd = item.archived ? "threads.restore" : "threads.archive"
                let ack: Ack = try await store.rpc(cmd, args: Args(threadId: item.sessionKey))
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
            await fetchSessionList()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func triggerSingleDelete(_ item: UnifiedSessionSummaryItem) async {
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

    private func confirmClaudeDelete(_ preview: ClaudeDeletePreview) async {
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
            if let residuals = result.residuals, !residuals.isEmpty, !result.deleted {
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

    private func confirmGrokDelete(_ preview: GrokDeletePreview) async {
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
            if let residuals = result.residuals, !residuals.isEmpty, !result.deleted {
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

    private func fetchClaudeOwnershipDiagnostics(sessionKey: String) async {
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
    private func prepareBatch(
        operation: SessionOperation,
        keys: [String],
        partition: SessionSelectionPartition
    ) async {
        guard !keys.isEmpty, !batchBusy else { return }
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
                try await targetStore.rpc("sessions.bulkPreview", args: Wrap(request: req))
            }
            guard store.revision == capturedRevision else { return }
            batchPreview = preview
        } catch {
            guard store.revision == capturedRevision else { return }
            batchPrepareError = error.localizedDescription
        }
    }

    private func executePreparedBatch() async {
        guard let preview = batchPreview, !batchBusy else { return }
        let capturedRevision = store.revision
        let targetPartition = batchTargetPartition
        batchPreview = nil
        batchBusy = true
        batchExecuteError = nil
        defer { batchBusy = false }
        struct Wrap: Encodable { let request: SessionBatchExecuteRequest }

        let targetStore = store
        let outcome = await SessionBatchChunkExecutor.execute(preview: preview) { req in
            try await targetStore.rpc("sessions.bulkExecute", args: Wrap(request: req))
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

    private func handleTimelineTickSelected(_ entry: TimelineEntry) async {
        guard let item = selectedItem else { return }
        let targetId = entry.id

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
                    subagentsSupported: store.capabilities.threadSubagents,
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

        if let winRows = timelineWindow.items {
            coordinatorRef?.updateRowsPreservingAnchor(winRows, preferAutoFollowBottom: false)
            _ = coordinatorRef?.scrollToPositionKey(targetId, reduceMotion: motionObserver.reduceMotion)
            timelineWindow.clearPendingTarget()
        }
    }

    private func loadNewerTimelineWindowPage() async {
        guard let item = selectedItem, timelineWindow.isActive else { return }
        await timelineWindow.loadNewer(latestTailId: rows.last?.timelineId) { cursor in
            try await readTimelineWindowPage(for: item, cursor: cursor)
        }
        if let winRows = timelineWindow.items {
            coordinatorRef?.updateRowsPreservingAnchor(winRows, preferAutoFollowBottom: false)
        }
    }

    private func returnFromTimelineWindowToLatest() {
        timelineWindow.returnToLatest()
        coordinatorRef?.updateRowsPreservingAnchor(rows, preferAutoFollowBottom: true, forceScrollToBottom: true)
    }

    private func readTimelineWindowPage(
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
                subagentsSupported: store.capabilities.threadSubagents,
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
            struct Wrap: Encodable {
                struct Inner: Encodable { let sessionKey: String; let limit: Int; let before: String }
                let request: Inner
            }
            let page: ProviderSessionDetail = try await store.rpc(
                cmd,
                args: Wrap(request: .init(sessionKey: item.sessionKey, limit: 160, before: cursor)),
                localOnly: provider == .grokbot
            )
            let rawPage = TimelineWindowPage<ProviderHistoryEvent>.from(providerDetail: page)
            return TimelineWindowPage<ConversationRowItem>(
                items: ConversationRowBuilder.buildProviderRows(events: rawPage.items, providerLabel: workspace.label),
                before: rawPage.before,
                after: rawPage.after,
                hasMore: rawPage.hasMore,
                history: rawPage.history
            )
        }
    }

    private func jumpAndRevealPositionKey(_ positionKey: String?) async {
        guard let positionKey, !positionKey.isEmpty, let item = selectedItem else { return }
        let capturedRevision = store.revision
        let capturedSession = item.sessionKey

        // Unfold single target block inside Codex 60/3 compact history if needed
        if provider == .codex, revealedPositionKey != positionKey {
            revealedPositionKey = positionKey
            let slot = threadStore.slot(for: item.sessionKey)
            if !slot.blocks.isEmpty {
                let rebuilt = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: showAllHistory || slot.showAllHistory,
                    planFallbackTitle: item.title,
                    subagentsSupported: store.capabilities.threadSubagents,
                    revealedBlockId: positionKey
                )
                coordinatorRef?.updateRowsPreservingAnchor(rebuilt, preferAutoFollowBottom: false)
                rows = rebuilt
            }
        }

        if coordinatorRef?.scrollToPositionKey(positionKey, reduceMotion: motionObserver.reduceMotion) == true {
            return
        }

        // Bounded backward paging until positionKey is loaded or cursor stops advancing (AGENTS.md:37, 130)
        var attempts = 0
        while attempts < 25, hasMoreOlder, store.revision == capturedRevision, selectedItem?.sessionKey == capturedSession {
            attempts += 1
            let loaded = await loadOlderPageIfNeeded()
            if !loaded { break }
            if coordinatorRef?.scrollToPositionKey(positionKey, reduceMotion: motionObserver.reduceMotion) == true {
                return
            }
        }
    }

    private func handleSearchResultSelected(_ result: SessionSearchResult) {
        if selectedItem?.sessionKey != result.sessionKey {
            pendingJumpPositionKey = result.positionKey
            selectedKey = result.sessionKey
            mobileDetailOpen = true
            if supportsArchive && !activeItems.contains(where: { $0.sessionKey == result.sessionKey }) {
                archivedOpen = true
            }
        } else {
            Task { await jumpAndRevealPositionKey(result.positionKey) }
        }
    }

    private func mapCodexSummary(_ s: ThreadSummary, archived: Bool) -> UnifiedSessionSummaryItem {
        let displayTitle = threadStore.effectiveTitle(
            threadId: s.id,
            serverTitle: s.title.isEmpty ? (s.firstUserMessage ?? s.id) : s.title
        )
        let running = (s.status == "Running" || s.activeTurnId != nil)
        let stText: String = switch s.status {
        case "Running": "运行中"
        case "ReplyNeeded": "待回复"
        case "Recoverable": "可恢复"
        case "Archived": "已归档"
        default: archived ? "已归档" : "近期"
        }
        return UnifiedSessionSummaryItem(
            sessionKey: s.id,
            nativeId: s.id,
            title: displayTitle,
            cwd: s.cwd ?? "",
            path: s.rolloutPath ?? "",
            statusText: stText,
            running: running,
            archived: archived || s.status == "Archived",
            canRename: true,
            renameBlockReason: nil,
            canArchive: store.capabilities.threadArchiveActions,
            canDelete: true,
            storageSize: s.storageSize
        )
    }

    private func mapProviderSummary(_ s: ProviderSessionSummary) -> UnifiedSessionSummaryItem {
        UnifiedSessionSummaryItem(
            sessionKey: s.sessionKey,
            nativeId: s.id,
            title: s.title.isEmpty ? s.id : s.title,
            cwd: s.cwd,
            path: s.path,
            statusText: s.archived ? "已归档" : (s.status == "running" ? "运行中" : "近期"),
            running: s.status == "running",
            archived: s.archived,
            canRename: s.canRename,
            renameBlockReason: s.renameBlockReason,
            canArchive: s.canArchive,
            canDelete: s.canDelete,
            storageSize: s.storageSize
        )
    }
}
