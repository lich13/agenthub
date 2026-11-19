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

    public var id: String { sessionKey }
}

// MARK: - Unified 6-Provider Workspace View (Codex, Claude, Grok, Antigravity, DSH, GrokBot)

public struct UnifiedProviderWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public let workspace: WorkspaceId
    public let isCompactMobile: Bool
    public let scheme: ColorSchemeKind
    public var codexNewThreadSlot: (@MainActor (@escaping (String) -> Void) -> AnyView)?
    public var codexCollaborationFooterSlot: (@MainActor (String, String?, @escaping @MainActor () -> Void) -> AnyView)?

    @StateObject private var threadStore = ThreadMessageStore()
    @StateObject private var claudeHistoryStore = PagedEventHistoryStore<ClaudeHistoryEvent> { ev, _ in ev.id }
    @StateObject private var grokHistoryStore = PagedEventHistoryStore<GrokHistoryEvent> { ev, idx in
        ev.id ?? ev.userMessage?.id ?? "\(ev.kind):\(ev.callId ?? ""):\(ev.timestamp ?? ""):\(idx)"
    }
    @StateObject private var providerHistoryStore = PagedEventHistoryStore<ProviderHistoryEvent> { ev, _ in ev.id }
    @StateObject private var motionObserver = AccessibilityMotionObserver()

    @State private var query: String = ""
    @State private var activeItems: [UnifiedSessionSummaryItem] = []
    @State private var archivedItems: [UnifiedSessionSummaryItem] = []
    @State private var archivedOpen: Bool = false
    @State private var selectedKey: String?
    @State private var mobileDetailOpen: Bool = false

    @State private var rows: [ConversationRowItem] = []
    @State private var timelineEntries: [TimelineEntry] = []
    @State private var activeTimelineId: String?
    @State private var subagents: SubagentCollection?
    @State private var beforeCursor: String?
    @State private var hasMoreOlder: Bool = false
    @State private var loadingOlder: Bool = false
    @State private var showAllHistory: Bool = false
    @State private var revealedPositionKey: String?
    @State private var pendingJumpPositionKey: String?
    @State private var latestReplyText: String?
    @State private var renameRequestSeqByKey: [String: Int] = [:]
    @State private var codexTailRefreshTask: Task<Void, Never>?

    @State private var selectionState = SessionSelectionEngine.empty(scope: "init")
    @State private var batchBusy: Bool = false
    @State private var batchPreview: SessionBatchPreview?
    @State private var batchResult: SessionBatchResult?
    @State private var claudeDeletePreview: ClaudeDeletePreview?
    @State private var grokDeletePreview: GrokDeletePreview?

    @State private var headerInlineRename: Bool = false
    @State private var searchModalOpen: Bool = false
    @State private var subagentDrawerOpen: Bool = false
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
        codexNewThreadSlot: (@MainActor (@escaping (String) -> Void) -> AnyView)? = nil,
        codexCollaborationFooterSlot: (@MainActor (String, String?, @escaping @MainActor () -> Void) -> AnyView)? = nil
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

    public var batchOperations: [SessionOperation] {
        if provider == .grokbot { return [] }
        if supportsArchive {
            return selectedItem?.archived == true ? [.restore, .delete] : [.archive, .delete]
        }
        return [.delete]
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
                        stageArea(availableWidth: stageWidth)
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
                    let allowedCount = preview.items.filter(\.allowed).count
                    Text("\(preview.operation.label)所选线程")
                        .font(DesignFonts.body(15, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                    Text("已定位 \(allowedCount) 项，定位失败 \(preview.items.count - allowedCount) 项。")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textSecondary.color)
                    HStack {
                        Spacer()
                        Button("取消") { batchPreview = nil }
                            .buttonStyle(SecondaryButtonStyle())
                            .disabled(batchBusy)
                        if preview.operation == .delete {
                            Button(batchBusy ? "正在处理…" : "确认\(preview.operation.label) \(allowedCount) 项") {
                                Task { await executePreparedBatch() }
                            }
                            .buttonStyle(DangerButtonStyle())
                            .disabled(batchBusy || allowedCount == 0)
                        } else {
                            Button(batchBusy ? "正在处理…" : "确认\(preview.operation.label) \(allowedCount) 项") {
                                Task { await executePreparedBatch() }
                            }
                            .buttonStyle(PrimaryButtonStyle())
                            .disabled(batchBusy || allowedCount == 0)
                        }
                    }
                }
            }

            if let claudePreview = claudeDeletePreview {
                ModalConfirmDialogView(busy: batchBusy, onCancel: { claudeDeletePreview = nil }) {
                    Text("删除任务文件")
                        .font(DesignFonts.body(15, weight: .semibold))
                    Text("确认删除 \(claudePreview.title)（\(SessionStorageFormatter.formatThousands(claudePreview.bytes)) 字节）？")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textSecondary.color)
                    HStack {
                        Spacer()
                        Button("取消") { claudeDeletePreview = nil }.buttonStyle(SecondaryButtonStyle())
                        Button("确认删除") { Task { await confirmClaudeDelete(claudePreview) } }
                            .buttonStyle(DangerButtonStyle())
                            .disabled(batchBusy)
                    }
                }
            }

            if let grokPreview = grokDeletePreview {
                ModalConfirmDialogView(busy: batchBusy, onCancel: { grokDeletePreview = nil }) {
                    Text("删除任务文件")
                        .font(DesignFonts.body(15, weight: .semibold))
                    Text("确认删除 \(grokPreview.title)（\(SessionStorageFormatter.formatThousands(grokPreview.bytes)) 字节）？")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textSecondary.color)
                    HStack {
                        Spacer()
                        Button("取消") { grokDeletePreview = nil }.buttonStyle(SecondaryButtonStyle())
                        Button("确认删除") { Task { await confirmGrokDelete(grokPreview) } }
                            .buttonStyle(DangerButtonStyle())
                            .disabled(batchBusy)
                    }
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
                isModalOpen: searchModalOpen || batchPreview != nil || claudeDeletePreview != nil || grokDeletePreview != nil || previewImageItem != nil,
                onRenameSelected: {
                    guard provider != .grokbot, !selectionState.selecting, let item = selectedItem else { return false }
                    renameRequestSeqByKey[item.sessionKey, default: 0] += 1
                    return true
                },
                onEscapeClearSelection: {
                    guard selectionState.selecting else { return false }
                    selectionState = SessionSelectionEngine.empty(scope: selectionState.scope)
                    return true
                },
                onBatchDelete: {
                    guard provider != .grokbot, !batchBusy, selectionState.selecting, !selectionState.keys.isEmpty else { return false }
                    Task { await prepareBatch(operation: .delete, keys: selectionState.keys) }
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
            if provider != .grokbot && !searchModalOpen && batchPreview == nil {
                selectAllVisible()
            }
        }
        .onChange(of: store.revision) { _ in
            subagentDrawerOpen = false
            selectedSubagent = nil
            revealedPositionKey = nil
            pendingJumpPositionKey = nil
            claudeHistoryStore.clearAll()
            grokHistoryStore.clearAll()
            providerHistoryStore.clearAll()
        }
        .onChange(of: selectedItem?.sessionKey) { _ in
            revealedPositionKey = nil
        }
        // 5s Session List Polling Clock (paused when window hidden)
        .task(id: "\(store.revision):\(workspace.rawValue):\(query):\(archivedOpen)") {
            let scope = "\(store.revision):\(workspace.rawValue):\(query)"
            selectionState = SessionSelectionEngine.sync(selectionState, scope: scope, visible: allVisibleItems.map(\.sessionKey))
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
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                AgentBrandIcon(brandId, size: 17)
                Text(workspace.label)
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                Text("\(activeItems.count)")
                    .font(DesignFonts.body(11, weight: .medium))
                    .foregroundStyle(palette.textMuted.color)
                Spacer()
                Button {
                    Task { await fetchSessionList() }
                } label: {
                    LucideIcon(.refreshCw, size: 15)
                }
                .buttonStyle(IconButtonStyle(dimension: 30))
                .accessibilityLabel("刷新列表")
            }

            if workspace == .codex, store.target == .local, store.capabilities.codexDesktop,
               let slot = codexNewThreadSlot
            {
                slot { newId in
                    query = ""
                    selectedKey = newId
                    mobileDetailOpen = true
                    Task { await fetchSessionList() }
                }
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
                        sessionRowView(item)
                    }

                    if supportsArchive {
                        Button {
                            archivedOpen.toggle()
                        } label: {
                            HStack(spacing: 6) {
                                LucideIcon(archivedOpen ? .chevronDown : .chevronRight, size: 14)
                                Text("已归档\(archivedOpen ? " (\(archivedItems.count))" : "")")
                                    .font(DesignFonts.body(12, weight: .medium))
                                Spacer()
                            }
                            .foregroundStyle(palette.textMuted.color)
                            .padding(.vertical, 6)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(PlainHitButtonStyle())

                        if archivedOpen {
                            ForEach(archivedItems) { item in
                                sessionRowView(item)
                            }
                        }
                    }
                }
            }

            if provider != .grokbot {
                SessionBatchBarAndDialogView(
                    selectedCount: selectionState.keys.count,
                    operations: batchOperations,
                    busy: batchBusy,
                    notice: selectionState.notice,
                    preview: batchPreview,
                    result: batchResult,
                    onSelectAll: selectAllVisible,
                    onClear: { selectionState = SessionSelectionEngine.empty(scope: selectionState.scope) },
                    onPrepareOperation: { op in Task { await prepareBatch(operation: op, keys: selectionState.keys) } },
                    onConfirmExecute: { Task { await executePreparedBatch() } },
                    onDismissPreview: { batchPreview = nil }
                )
            }
        }
        .padding(12)
    }

    private func sessionRowView(_ item: UnifiedSessionSummaryItem) -> some View {
        let isSelected = (selectedItem?.sessionKey == item.sessionKey)
        let isMulti = selectionState.keys.contains(item.sessionKey)
        let batchMenu = ProviderMenuMatrixBuilder.buildBatchContextMenuActions(
            selectedCount: selectionState.keys.count,
            operations: batchOperations,
            busy: batchBusy,
            onPrepare: { op in Task { await prepareBatch(operation: op, keys: selectionState.keys) } },
            onClear: { selectionState = SessionSelectionEngine.empty(scope: selectionState.scope) }
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
            selecting: selectionState.selecting,
            multiSelected: isMulti,
            disabled: batchBusy,
            canRename: item.canRename,
            renameBlockReason: item.renameBlockReason,
            externalRenameRequestId: renameRequestSeqByKey[item.sessionKey, default: 0],
            rowMenuActions: rowActions,
            batchMenuActions: batchMenu,
            onRowClick: { click in
                if provider == .grokbot {
                    selectedKey = item.sessionKey
                    mobileDetailOpen = true
                    return
                }
                let next = SessionSelectionEngine.click(
                    selectionState,
                    visible: allVisibleItems.map(\.sessionKey),
                    key: item.sessionKey,
                    click: click,
                    activeKey: selectedItem?.sessionKey
                )
                selectionState = next
                if !next.selecting && !(click.ctrlKey && click.detail != 0) {
                    selectedKey = item.sessionKey
                    mobileDetailOpen = true
                    headerInlineRename = false
                }
            },
            onToggleCheckbox: {
                selectionState = SessionSelectionEngine.click(
                    selectionState,
                    visible: allVisibleItems.map(\.sessionKey),
                    key: item.sessionKey,
                    click: SessionClick(detail: 1, metaKey: true),
                    activeKey: selectedItem?.sessionKey
                )
            },
            onRename: provider == .grokbot ? nil : { nextTitle in
                try await performRename(item: item, newTitle: nextTitle)
            }
        )
    }

    // MARK: - Stage Conversation Reading Column + Subagents + Floating Search

    @ViewBuilder
    private func stageArea(availableWidth: CGFloat) -> some View {
        if let item = selectedItem {
            let canDockSubagents = availableWidth >= LayoutMetrics.subagentDockThreshold
            let subagentsSupported = (provider == .codex && store.capabilities.threadSubagents)
                || (provider == .claudeCode && store.capabilities.claudeSubagents)

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
                            HStack(spacing: 6) {
                                if subagentsSupported, let count = subagents?.agents.count, count > 0 {
                                    Button {
                                        selectedSubagent = nil
                                        subagentDrawerOpen.toggle()
                                    } label: {
                                        HStack(spacing: 4) {
                                            LucideIcon(.bot, size: 15)
                                            Text("\(count)")
                                                .font(DesignFonts.body(12, weight: .semibold))
                                        }
                                        .padding(.horizontal, 8)
                                        .frame(height: 30)
                                        .background(Capsule().fill(palette.surface.color))
                                        .contentShape(Rectangle())
                                    }
                                    .buttonStyle(PlainHitButtonStyle())
                                    .accessibilityLabel("子智能体面板")
                                }

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
                        }

                        if let fb = feedbackMessage, !fb.isEmpty {
                            Text(fb)
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.okFg.color)
                                .padding(.vertical, 4)
                        }

                        ZStack(alignment: .bottomTrailing) {
                            ZStack(alignment: .leading) {
                                if let coord = coordinatorRef {
                                    ConversationReaderTableView(
                                        coordinator: coord,
                                        rows: rows,
                                        scheme: scheme,
                                        workspace: workspace,
                                        sessionScopeKey: "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)",
                                        sessionProvider: provider,
                                        sessionKey: item.sessionKey,
                                        cwd: item.cwd,
                                        hasTimelineRail: !timelineEntries.isEmpty,
                                        isSubagent: false
                                    )
                                }

                                if !timelineEntries.isEmpty {
                                    TimelineRailView(
                                        entries: timelineEntries,
                                        activeId: activeTimelineId,
                                        availableWidth: availableWidth
                                    ) { entry in
                                        Task { await jumpAndRevealPositionKey(entry.id) }
                                    }
                                }
                            }

                            FloatingSearchPillButton {
                                searchModalOpen = true
                            }
                            .padding(.trailing, 16)
                            .padding(.bottom, 16)
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)

                        if workspace == .codex, store.target == .local, store.capabilities.codexDesktop,
                           let footerSlot = codexCollaborationFooterSlot
                        {
                            footerSlot(item.sessionKey, item.cwd.isEmpty ? nil : item.cwd) {
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
                            onClose: { subagentDrawerOpen = false }
                        )
                    }
                }

                // <960pt Overlay Subagent Drawer with .34 Backdrop
                if subagentDrawerOpen && !canDockSubagents {
                    Color.black.opacity(0.34)
                        .ignoresSafeArea()
                        .onTapGesture { subagentDrawerOpen = false }

                    SubagentPanelView(
                        store: store,
                        provider: provider,
                        rootSessionKey: item.sessionKey,
                        collection: subagents,
                        initialAgent: selectedSubagent,
                        scheme: scheme,
                        workspace: workspace,
                        onClose: { subagentDrawerOpen = false }
                    )
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
        c.onLoadOlderRequested = { Task { await loadOlderPageIfNeeded() } }
        c.onShowAllHistory = {
            showAllHistory = true
            if let item = selectedItem, provider == .codex {
                threadStore.setHistoryExpanded(threadId: item.sessionKey, expanded: true)
            }
        }
        c.onOpenSubagent = { sub in
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

    private func selectAllVisible() {
        selectionState = SessionSelectionEngine.add(selectionState, keys: allVisibleItems.map(\.sessionKey))
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
                    UnifiedSessionSummaryItem(
                        sessionKey: item.sessionKey,
                        nativeId: item.id,
                        title: item.title.isEmpty ? item.id : item.title,
                        cwd: item.cwd,
                        path: item.path,
                        statusText: item.status == "running" ? "运行中" : "近期",
                        running: item.status == "running",
                        archived: false,
                        canRename: item.canRename,
                        renameBlockReason: item.renameBlockReason,
                        canArchive: false,
                        canDelete: item.canDelete,
                        storageSize: item.storageSize
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
                        canDelete: item.deletionEligibility?.allowed ?? true,
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
        do {
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
        guard hasMoreOlder, !loadingOlder, let cursor = beforeCursor, let item = selectedItem else { return false }
        loadingOlder = true
        defer { loadingOlder = false }
        let historyScope = "\(store.machineScope):\(provider.rawValue):\(item.sessionKey)"

        switch provider {
        case .codex:
            struct BlocksArgs: Encodable {
                struct Opts: Encodable { let limit: Int; let before: String }
                let id: String
                let options: Opts
            }
            guard let page: ThreadBlockPage = try? await store.rpc(
                "threads.blocks",
                args: BlocksArgs(id: item.sessionKey, options: .init(limit: 120, before: cursor))
            ) else { return false }
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
            guard let page: ClaudeSessionDetail = try? await store.rpc(
                "claude.detail",
                args: Req(sessionKey: item.sessionKey, limit: 160, before: cursor)
            ) else { return false }
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
            guard let page: GrokSessionDetail = try? await store.rpc(
                "grok.detail",
                args: Req(id: item.sessionKey, limit: 160, before: cursor)
            ) else { return false }
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
            guard let page: ProviderSessionDetail = try? await store.rpc(
                cmd,
                args: Wrap(request: .init(sessionKey: item.sessionKey, limit: 160, before: cursor)),
                localOnly: provider == .grokbot
            ) else { return false }
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
            struct Args: Encodable { let sessionKey: String }
            claudeDeletePreview = try? await store.rpc("claude.deletePreview", args: Args(sessionKey: item.sessionKey))
        } else if provider == .grok {
            struct Args: Encodable { let id: String }
            grokDeletePreview = try? await store.rpc("grok.deletePreview", args: Args(id: item.sessionKey))
        } else {
            await prepareBatch(operation: .delete, keys: [item.sessionKey])
        }
    }

    private func confirmClaudeDelete(_ preview: ClaudeDeletePreview) async {
        batchBusy = true
        defer { batchBusy = false }
        struct Wrap: Encodable {
            struct Inner: Encodable { let sessionKey: String; let confirmed: Bool; let fingerprint: String }
            let request: Inner
        }
        if let _: ClaudeDeleteResult = try? await store.rpc(
            "claude.deleteExecute",
            args: Wrap(request: .init(sessionKey: preview.sessionKey, confirmed: true, fingerprint: preview.fingerprint))
        ) {
            claudeDeletePreview = nil
            if selectedKey == preview.sessionKey { selectedKey = nil }
            await fetchSessionList()
        }
    }

    private func confirmGrokDelete(_ preview: GrokDeletePreview) async {
        batchBusy = true
        defer { batchBusy = false }
        struct Wrap: Encodable {
            struct Inner: Encodable { let id: String; let confirmed: Bool; let fingerprint: String }
            let request: Inner
        }
        if let _: GrokDeleteResult = try? await store.rpc(
            "grok.deleteExecute",
            args: Wrap(request: .init(id: preview.id, confirmed: true, fingerprint: preview.fingerprint))
        ) {
            grokDeletePreview = nil
            if selectedKey == preview.id { selectedKey = nil }
            await fetchSessionList()
        }
    }

    // Chunked 100-item archive/restore & single-batch delete
    private func prepareBatch(operation: SessionOperation, keys: [String]) async {
        guard !keys.isEmpty else { return }
        batchBusy = true
        defer { batchBusy = false }
        let includeDescendants = supportsArchive && operation == .delete
        let chunkSize = (operation == .delete) ? max(1, keys.count) : 100
        var mergedItems: [SessionBatchItem] = []
        struct Wrap: Encodable { let request: SessionBatchRequest }

        do {
            var start = 0
            while start < keys.count {
                let slice = Array(keys[start ..< min(keys.count, start + chunkSize)])
                let req = SessionBatchRequest(
                    provider: provider,
                    operation: operation,
                    sessionKeys: slice,
                    includeDescendants: includeDescendants ? true : nil
                )
                let page: SessionBatchPreview = try await store.rpc("sessions.bulkPreview", args: Wrap(request: req))
                mergedItems.append(contentsOf: page.items)
                start += chunkSize
            }
            batchPreview = SessionBatchPreview(provider: provider, operation: operation, items: mergedItems)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func executePreparedBatch() async {
        guard let preview = batchPreview else { return }
        batchPreview = nil
        batchBusy = true
        defer { batchBusy = false }
        let allowed = preview.items.filter(\.allowed).map { item in
            SessionBatchExecuteItem(
                sessionKey: item.sessionKey,
                fingerprint: preview.operation == .delete ? nil : item.fingerprint,
                descendants: preview.operation == .delete ? item.descendants : nil
            )
        }
        let includeDescendants = supportsArchive && preview.operation == .delete
        let chunkSize = (preview.operation == .delete) ? max(1, allowed.count) : 100
        var results: [SessionBatchItemResult] = []
        struct Wrap: Encodable { let request: SessionBatchExecuteRequest }

        do {
            var start = 0
            while start < allowed.count {
                let slice = Array(allowed[start ..< min(allowed.count, start + chunkSize)])
                let req = SessionBatchExecuteRequest(
                    provider: preview.provider,
                    operation: preview.operation,
                    items: slice,
                    confirmed: true,
                    includeDescendants: includeDescendants ? true : nil
                )
                let res: SessionBatchResult = try await store.rpc("sessions.bulkExecute", args: Wrap(request: req))
                results.append(contentsOf: res.items)
                start += chunkSize
            }
            batchResult = SessionBatchResult(items: results)
            let okSet = Set(results.filter { $0.status == "succeeded" }.map(\.sessionKey))
            if let sel = selectedKey, okSet.contains(sel) { selectedKey = nil }
            selectionState = SessionSelectionEngine.empty(scope: selectionState.scope)
            await fetchSessionList()
        } catch {
            errorMessage = error.localizedDescription
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
