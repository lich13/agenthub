import AppKit
import SwiftUI

// MARK: - .subagent-summary Bar Directly Below Detail Glass Header (DESIGN.md:113, styles.css:790-794)

public struct SubagentSummaryBarView: View {
    @Environment(\.agentHubPalette) private var palette
    public let provider: SessionProvider
    public let collection: SubagentCollection?
    public let isSupported: Bool
    public let onTogglePanel: (NSResponder?) -> Void

    @State private var triggerAnchorView: NSView?

    public init(
        provider: SessionProvider,
        collection: SubagentCollection?,
        isSupported: Bool = true,
        onTogglePanel: @escaping (NSResponder?) -> Void
    ) {
        self.provider = provider
        self.collection = collection
        self.isSupported = isSupported
        self.onTogglePanel = onTogglePanel
    }

    public static func shouldDisplay(
        provider: SessionProvider,
        collection: SubagentCollection?,
        isSupported: Bool
    ) -> Bool {
        guard isSupported else {
            return provider == .codex && collection == nil
        }
        guard let collection else {
            return provider == .codex
        }
        if collection.agents.isEmpty && collection.complete {
            return false
        }
        return !collection.agents.isEmpty || !collection.complete
    }

    public var body: some View {
        if Self.shouldDisplay(provider: provider, collection: collection, isSupported: isSupported) {
            if let collection {
                let totalCount = collection.agents.count
                let counts = collection.counts
                let countFor: (String) -> Int = { key in
                    counts[key] ?? collection.agents.filter { $0.status == key }.count
                }
                let runningCount = countFor("running")
                let completedCount = countFor("completed")
                let creatingCount = countFor("creating")
                let failedCount = countFor("failed")
                let interruptedCount = countFor("interrupted")
                let unknownCount = countFor("unknown")
                let unavailableCount = collection.agents.filter { !$0.available }.count

                Button {
                    let responder = triggerAnchorView?.window?.firstResponder ?? triggerAnchorView
                    onTogglePanel(responder)
                } label: {
                    HStack(spacing: 8) {
                        LucideIcon(.bot, size: 16)
                            .foregroundStyle(palette.accent.color)

                        Text("子代理 · \(totalCount)")
                            .font(DesignFonts.body(12, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)

                        if runningCount > 0 {
                            HStack(spacing: 4) {
                                RunningIndicator(size: 13)
                                Text("\(runningCount) 个运行中")
                                    .font(DesignFonts.tabularBody(12))
                                    .foregroundStyle(palette.textSecondary.color)
                            }
                        }
                        if completedCount > 0 {
                            Text("\(completedCount) 已完成")
                                .font(DesignFonts.tabularBody(12))
                                .foregroundStyle(palette.textSecondary.color)
                        }
                        if creatingCount > 0 {
                            Text("\(creatingCount) 正在创建")
                                .font(DesignFonts.tabularBody(12))
                                .foregroundStyle(palette.textSecondary.color)
                        }
                        if failedCount > 0 {
                            Text("\(failedCount) 失败")
                                .font(DesignFonts.tabularBody(12, weight: .medium))
                                .foregroundStyle(palette.errFg.color)
                        }
                        if interruptedCount > 0 {
                            Text("\(interruptedCount) 已中断")
                                .font(DesignFonts.tabularBody(12))
                                .foregroundStyle(palette.textMuted.color)
                        }
                        if unknownCount > 0 {
                            Text("\(unknownCount) 状态未知")
                                .font(DesignFonts.tabularBody(12))
                                .foregroundStyle(palette.textMuted.color)
                        }
                        if unavailableCount > 0 {
                            Text("\(unavailableCount) 不可用")
                                .font(DesignFonts.tabularBody(12, weight: .medium))
                                .foregroundStyle(palette.errFg.color)
                        }

                        Spacer(minLength: 6)

                        LucideIcon(.chevronRight, size: 14)
                            .foregroundStyle(palette.textMuted.color)
                    }
                    .padding(.vertical, 8)
                    .padding(.horizontal, 20)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.surface.color)
                    .overlay(alignment: .bottom) {
                        Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                    }
                    .background(
                        SubagentResponderProbeView { v in
                            triggerAnchorView = v
                        }
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())
                .accessibilityLabel("查看直属子智能体")
            } else if provider == .codex {
                HStack {
                    Text("更新当前机器服务后可查看子智能体汇总。")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textMuted.color)
                    Spacer()
                }
                .padding(.vertical, 7)
                .padding(.horizontal, 20)
                .background(palette.surface.color)
                .overlay(alignment: .bottom) {
                    Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                }
            }
        }
    }
}

private final class SubagentFocusAnchorNSView: NSView {
    override var acceptsFirstResponder: Bool { true }
}

private struct SubagentResponderProbeView: NSViewRepresentable {
    let onAttach: (NSView) -> Void

    func makeNSView(context: Context) -> NSView {
        let v = SubagentFocusAnchorNSView(frame: .zero)
        DispatchQueue.main.async { onAttach(v) }
        return v
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}

// MARK: - 420pt Subagent Docked / Overlay Drawer (Codex Nested Stack & Claude FlatrecordedOnly)

public struct SubagentPanelView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public let provider: SessionProvider
    public let rootSessionKey: String
    public let collection: SubagentCollection?
    public let initialAgent: SubagentActivity?
    public let scheme: ColorSchemeKind
    public let workspace: WorkspaceId
    public var isModalOverlay: Bool
    public var restoreFocusResponder: NSResponder?
    public let onClose: () -> Void

    @StateObject private var subagentThreadStore = ThreadMessageStore()
    @StateObject private var subagentClaudeStore = PagedEventHistoryStore<ClaudeHistoryEvent> { ev, _ in ev.id }

    @State private var agentStack: [SubagentActivity] = []
    @State private var scrollAnchorStack: [ConversationTableCoordinator.ScrollAnchorSnapshot?] = []
    @State private var pendingRestoreAnchor: ConversationTableCoordinator.ScrollAnchorSnapshot?
    @State private var rows: [ConversationRowItem] = []
    @State private var timelineEntries: [TimelineEntry] = []
    @State private var activeTimelineId: String?
    @State private var beforeCursor: String?
    @State private var hasMoreOlder: Bool = false
    @State private var loadingOlder: Bool = false
    @State private var delegationExpanded: Bool = false
    @State private var loading: Bool = false
    @State private var errorText: String?
    @State private var coordinatorRef: ConversationTableCoordinator?

    public init(
        store: RuntimeConnectionStore,
        provider: SessionProvider,
        rootSessionKey: String,
        collection: SubagentCollection?,
        initialAgent: SubagentActivity? = nil,
        scheme: ColorSchemeKind,
        workspace: WorkspaceId,
        isModalOverlay: Bool = false,
        restoreFocusResponder: NSResponder? = nil,
        onClose: @escaping () -> Void
    ) {
        self.store = store
        self.provider = provider
        self.rootSessionKey = rootSessionKey
        self.collection = collection
        self.initialAgent = initialAgent
        self.scheme = scheme
        self.workspace = workspace
        self.isModalOverlay = isModalOverlay
        self.restoreFocusResponder = restoreFocusResponder
        self.onClose = onClose
    }

    public var currentAgent: SubagentActivity? {
        agentStack.last
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Drawer Header
            HStack(spacing: 8) {
                if !agentStack.isEmpty {
                    Button {
                        popAgent()
                    } label: {
                        LucideIcon(.arrowLeft, size: 16)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 30))
                    .accessibilityLabel("返回上一级")
                }

                LucideIcon(.bot, size: 16)
                    .foregroundStyle(palette.accent.color)

                Text(currentAgent?.name ?? "子智能体 (\(collection?.agents.count ?? 0))")
                    .font(DesignFonts.body(13, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                    .lineLimit(1)

                if let current = currentAgent {
                    StatusChip(subagentStatusLabel(current.status), tone: subagentTone(current.status))
                }

                Spacer(minLength: 4)

                Button(action: onClose) {
                    LucideIcon(.x, size: 16)
                }
                .buttonStyle(IconButtonStyle(dimension: 30))
                .accessibilityLabel("关闭子智能体面板")
            }
            .padding(.horizontal, 14)
            .frame(height: 48)
            .background(palette.surface.color)
            .overlay(alignment: .bottom) {
                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
            }

            if let current = currentAgent {
                if let del = current.delegation, !del.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Button {
                            delegationExpanded.toggle()
                        } label: {
                            HStack(spacing: 6) {
                                LucideIcon(delegationExpanded ? .chevronDown : .chevronRight, size: 14)
                                Text("委派内容")
                                    .font(DesignFonts.body(12, weight: .medium))
                                Spacer()
                            }
                            .foregroundStyle(palette.textSecondary.color)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(PlainHitButtonStyle())

                        if delegationExpanded {
                            Text(VisibleMarkdownEngine.visibleMarkdown(del))
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.textSecondary.color)
                                .textSelection(.enabled)
                                .lineLimit(8)
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(palette.surface.color)
                    .overlay(alignment: .bottom) {
                        Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                    }
                }
                if let errorText {
                    Text(errorText)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                        .padding(12)
                }
                if let coord = coordinatorRef {
                    ZStack(alignment: .leading) {
                        ConversationReaderTableView(
                            coordinator: coord,
                            rows: rows,
                            scheme: scheme,
                            workspace: workspace,
                            sessionScopeKey: "\(store.machineScope):subagent:\(rootSessionKey):\(current.agentId ?? current.id)",
                            sessionProvider: provider,
                            sessionKey: current.agentId ?? rootSessionKey,
                            rootThreadId: provider == .codex ? rootSessionKey : nil,
                            hasTimelineRail: !timelineEntries.isEmpty,
                            isSubagent: true
                        )
                        if !timelineEntries.isEmpty {
                            TimelineRailView(
                                entries: timelineEntries,
                                activeId: activeTimelineId,
                                availableWidth: LayoutMetrics.subagentDrawerWidth
                            ) { entry in
                                _ = coord.scrollToPositionKey(entry.id)
                            }
                        }
                    }
                } else {
                    Spacer()
                }
            } else {
                // Agent List (sorted by status priority; omits generic collection.warning per AGENTS.md:109)
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        let agents = Self.sortByStatusPriority(collection?.agents ?? [])
                        if agents.isEmpty {
                            Text("暂无直属子智能体")
                                .font(DesignFonts.body(13))
                                .foregroundStyle(palette.textMuted.color)
                                .padding(16)
                        } else {
                            ForEach(agents) { agent in
                                Button {
                                    pushAgent(agent)
                                } label: {
                                    VStack(alignment: .leading, spacing: 4) {
                                        HStack {
                                            Text(VisibleMarkdownEngine.visibleMarkdown(agent.name))
                                                .font(DesignFonts.body(13, weight: .semibold))
                                                .foregroundStyle(palette.textPrimary.color)
                                            if let role = agent.role, !role.isEmpty {
                                                Text(role)
                                                    .font(DesignFonts.body(11))
                                                    .foregroundStyle(palette.textMuted.color)
                                            }
                                            Spacer()
                                            if agent.status == "running" {
                                                RunningIndicator(size: 12)
                                            }
                                            StatusChip(subagentStatusLabel(agent.status), tone: subagentTone(agent.status))
                                        }
                                        if let del = agent.delegation, !del.isEmpty {
                                            Text(VisibleMarkdownEngine.visibleMarkdown(del))
                                                .font(DesignFonts.body(12))
                                                .foregroundStyle(palette.textSecondary.color)
                                                .lineLimit(2)
                                        }
                                        if !agent.available, let reason = agent.unavailableReason {
                                            Text(reason)
                                                .font(DesignFonts.body(11))
                                                .foregroundStyle(palette.errFg.color)
                                        }
                                    }
                                    .padding(10)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(RoundedRectangle(cornerRadius: 10).fill(palette.surface.color))
                                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(PlainHitButtonStyle())
                                .disabled(!agent.available)
                            }
                        }
                    }
                    .padding(12)
                }
            }
        }
        .frame(maxWidth: LayoutMetrics.subagentDrawerWidth)
        .frame(maxHeight: .infinity)
        .background(palette.background.color)
        .overlay(alignment: .leading) {
            Rectangle().fill(palette.borderSubtle.color).frame(width: 1)
        }
        .background {
            if isModalOverlay {
                ZStack {
                    ModalScopePresenceView()
                    SubagentModalFocusTrapView(
                        restoreFocusResponder: restoreFocusResponder,
                        onEscape: onClose
                    )
                }
                .frame(width: 0, height: 0)
            }
        }
        .onExitCommand { onClose() }
        .onAppear {
            if coordinatorRef == nil {
                let c = ConversationTableCoordinator(store: store)
                c.onOpenSubagent = { nextSub in
                    if provider == .codex {
                        pushAgent(nextSub)
                    }
                }
                c.onLoadOlderRequested = {
                    Task { await loadOlderSubagentPageIfNeeded() }
                }
                c.onActiveTimelineChanged = { id in
                    if activeTimelineId != id { activeTimelineId = id }
                }
                coordinatorRef = c
            }
            if let initialAgent, agentStack.isEmpty {
                pushAgent(initialAgent)
            }
        }
        .task(id: currentAgent?.id) {
            guard let agent = currentAgent else { return }
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await fetchSubagentDetail(agent)
                }
                let pollNs: UInt64 = if provider == .claudeCode {
                    5_000_000_000
                } else {
                    (agent.status == "running" || agent.status == "creating") ? 2_000_000_000 : 5_000_000_000
                }
                try? await Task.sleep(nanoseconds: pollNs)
            }
        }
    }

    public static func sortByStatusPriority(_ agents: [SubagentActivity]) -> [SubagentActivity] {
        func rank(_ s: String) -> Int {
            switch s {
            case "running": 0
            case "creating": 1
            case "failed": 2
            case "interrupted": 3
            case "unknown": 4
            case "completed": 5
            default: 4
            }
        }
        return agents.sorted { rank($0.status) < rank($1.status) }
    }

    public func pushAgent(_ agent: SubagentActivity) {
        guard agent.available, let nextId = agent.agentId, !nextId.isEmpty,
              !agentStack.contains(where: { $0.agentId == nextId })
        else { return }
        if !agentStack.isEmpty {
            scrollAnchorStack.append(coordinatorRef?.captureScrollAnchor())
        }
        pendingRestoreAnchor = nil
        delegationExpanded = false
        agentStack.append(agent)
        rows = []
        timelineEntries = []
        beforeCursor = nil
        hasMoreOlder = false
        errorText = nil
    }

    public func popAgent() {
        guard !agentStack.isEmpty else { return }
        agentStack.removeLast()
        delegationExpanded = false
        rows = []
        timelineEntries = []
        beforeCursor = nil
        hasMoreOlder = false
        errorText = nil
        pendingRestoreAnchor = scrollAnchorStack.isEmpty ? nil : scrollAnchorStack.removeLast()
    }

    private func fetchSubagentDetail(_ agent: SubagentActivity) async {
        guard let agentId = agent.agentId, !agentId.isEmpty else { return }
        loading = true
        defer { loading = false }
        let scopeKey = "\(store.machineScope):sub:\(rootSessionKey):\(agentId)"
        do {
            if provider == .codex {
                struct ReqWrap: Encodable { let request: SubagentDetailRequest }
                let resp: SubagentDetailResponse = try await store.rpc(
                    "threads.subagentDetail",
                    args: ReqWrap(request: SubagentDetailRequest(rootThreadId: rootSessionKey, agentId: agentId, limit: 120))
                )
                guard resp.rootThreadId == rootSessionKey, resp.agent.agentId == agentId else {
                    throw AgentHubFFIError("子智能体关联已变化，请重新打开")
                }
                let slot = subagentThreadStore.applyDetail(resp.detail)
                hasMoreOlder = slot.hasMoreBlocks
                beforeCursor = slot.beforeCursor
                let nextRows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: true,
                    planFallbackTitle: resp.agent.name,
                    subagentsSupported: true
                )
                rows = nextRows
                timelineEntries = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .codex,
                    sources: slot.blocks.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
            } else if provider == .claudeCode {
                struct ClaudeSubReq: Encodable {
                    let sessionKey: String
                    let agentId: String
                    let limit: Int
                }
                let detail: ClaudeSessionDetail = try await store.rpc(
                    "claude.detail",
                    args: ClaudeSubReq(sessionKey: rootSessionKey, agentId: agentId, limit: 160)
                )
                let slot = subagentClaudeStore.applyHeadPoll(
                    key: scopeKey,
                    headEvents: detail.events,
                    history: detail.history,
                    totalEvents: detail.totalEvents,
                    beforeCursor: detail.beforeCursor,
                    hasMore: detail.hasMore,
                    timeline: detail.timeline
                )
                hasMoreOlder = slot.hasMore
                beforeCursor = slot.beforeCursor
                rows = ConversationRowBuilder.buildClaudeRows(
                    events: slot.events,
                    recordedOnly: true,
                    attachmentsAsNamesOnly: true
                )
                timelineEntries = []
            }
            if let restore = pendingRestoreAnchor {
                pendingRestoreAnchor = nil
                coordinatorRef?.updateRowsPreservingAnchor(rows, preferAutoFollowBottom: false)
                coordinatorRef?.restoreExplicitAnchor(restore)
            }
        } catch {
            errorText = error.localizedDescription
        }
    }

    private func loadOlderSubagentPageIfNeeded() async {
        guard hasMoreOlder, !loadingOlder, let cursor = beforeCursor,
              let agent = currentAgent, let agentId = agent.agentId, !agentId.isEmpty
        else { return }
        loadingOlder = true
        defer { loadingOlder = false }
        let scopeKey = "\(store.machineScope):sub:\(rootSessionKey):\(agentId)"

        if provider == .codex {
            struct ReqWrap: Encodable { let request: SubagentDetailRequest }
            guard let resp: SubagentDetailResponse = try? await store.rpc(
                "threads.subagentDetail",
                args: ReqWrap(request: SubagentDetailRequest(rootThreadId: rootSessionKey, agentId: agentId, limit: 120, before: cursor))
            ) else { return }
            let page = ThreadBlockPage(
                threadId: resp.detail.summary.id,
                blocks: resp.detail.blocks,
                totalBlocks: resp.detail.totalBlocks ?? resp.detail.blocks.count,
                hasMoreBlocks: resp.detail.hasMoreBlocks ?? false,
                beforeCursor: resp.detail.beforeCursor,
                history: resp.detail.history
            )
            let slot = subagentThreadStore.applyBlockPage(page, expectedCursor: cursor)
            hasMoreOlder = slot.hasMoreBlocks && slot.beforeCursor != cursor
            beforeCursor = hasMoreOlder ? slot.beforeCursor : nil
            let nextRows = ConversationRowBuilder.buildCodexRows(
                blocks: slot.blocks,
                showAllHistory: true,
                planFallbackTitle: resp.agent.name,
                subagentsSupported: true
            )
            coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: false)
            rows = nextRows
        } else if provider == .claudeCode {
            struct ClaudeSubReq: Encodable {
                let sessionKey: String
                let agentId: String
                let limit: Int
                let before: String
            }
            guard let page: ClaudeSessionDetail = try? await store.rpc(
                "claude.detail",
                args: ClaudeSubReq(sessionKey: rootSessionKey, agentId: agentId, limit: 160, before: cursor)
            ) else { return }
            let slot = subagentClaudeStore.applyOlderPage(
                key: scopeKey,
                expectedCursor: cursor,
                olderEvents: page.events,
                history: page.history,
                totalEvents: page.totalEvents,
                beforeCursor: page.beforeCursor,
                hasMore: page.hasMore
            )
            hasMoreOlder = slot.hasMore
            beforeCursor = slot.beforeCursor
            let nextRows = ConversationRowBuilder.buildClaudeRows(
                events: slot.events,
                recordedOnly: true,
                attachmentsAsNamesOnly: true
            )
            coordinatorRef?.updateRowsPreservingAnchor(nextRows, preferAutoFollowBottom: false)
            rows = nextRows
        }
    }

    public func subagentStatusLabel(_ status: String) -> String {
        if provider == .claudeCode && status == "unknown" {
            return "状态未知"
        }
        return switch status {
        case "creating": "创建中"
        case "running": "运行中"
        case "completed": "已完成"
        case "failed": "失败"
        case "interrupted": "已中断"
        default: "状态未知"
        }
    }

    private func subagentTone(_ status: String) -> StatusChipTone {
        switch status {
        case "running", "creating": .info
        case "completed": .success
        case "failed": .danger
        case "interrupted": .warning
        default: .neutral
        }
    }
}

// MARK: - <960pt Overlay Subagent Panel Tab Focus Trap & Trigger Focus Restoration

@MainActor
public enum SubagentFocusTrapEngine {
    public static func focusableViews(in root: NSView) -> [NSView] {
        var result: [NSView] = []
        func walk(_ view: NSView) {
            guard !view.isHidden, view.alphaValue > 0.01 else { return }
            if let control = view as? NSControl, !control.isEnabled {
                return
            }
            if view.canBecomeKeyView || (view is NSButton) || (view is NSTextField) || (view is NSTextView) {
                if view.bounds.width > 1 && view.bounds.height > 1 {
                    result.append(view)
                }
            }
            for child in view.subviews {
                walk(child)
            }
        }
        walk(root)
        return result
    }

    public static func nextFocusTarget(in root: NSView, current: NSResponder?, backward: Bool) -> NSView? {
        let candidates = focusableViews(in: root)
        guard !candidates.isEmpty else { return nil }
        guard let currentView = current as? NSView,
              let idx = candidates.firstIndex(where: { $0 === currentView || currentView.isDescendant(of: $0) })
        else {
            return backward ? candidates.last : candidates.first
        }
        let nextIdx = backward
            ? (idx - 1 + candidates.count) % candidates.count
            : (idx + 1) % candidates.count
        return candidates[nextIdx]
    }
}

private struct SubagentModalFocusTrapView: NSViewRepresentable {
    var restoreFocusResponder: NSResponder?
    var onEscape: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(restoreFocusResponder: restoreFocusResponder, onEscape: onEscape)
    }

    func makeNSView(context: Context) -> NSView {
        let v = NSView(frame: .zero)
        context.coordinator.attach(anchor: v)
        return v
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        context.coordinator.onEscape = onEscape
        if let r = restoreFocusResponder, context.coordinator.savedResponder == nil {
            context.coordinator.savedResponder = r
        }
        context.coordinator.attach(anchor: nsView)
    }

    static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
        coordinator.detachAndRestoreFocus()
    }

    @MainActor
    final class Coordinator {
        weak var savedResponder: NSResponder?
        weak var savedWindow: NSWindow?
        weak var anchorView: NSView?
        var onEscape: () -> Void
        private var keyMonitor: Any?
        private var didInitialFocus: Bool = false

        init(restoreFocusResponder: NSResponder?, onEscape: @escaping () -> Void) {
            self.savedResponder = restoreFocusResponder
            self.onEscape = onEscape
        }

        private func drawerContainerRoot() -> NSView? {
            var cur = anchorView?.superview
            var best: NSView?
            while let v = cur {
                if abs(v.bounds.width - LayoutMetrics.subagentDrawerWidth) <= 40 && v.bounds.height > 100 {
                    best = v
                }
                cur = v.superview
            }
            return best ?? anchorView?.superview?.superview
        }

        func attach(anchor: NSView) {
            anchorView = anchor
            DispatchQueue.main.async { [weak self] in
                guard let self, let win = self.anchorView?.window else { return }
                self.savedWindow = win
                if self.savedResponder == nil {
                    self.savedResponder = win.firstResponder
                }
                if !self.didInitialFocus, let root = self.drawerContainerRoot(),
                   let firstTarget = SubagentFocusTrapEngine.nextFocusTarget(in: root, current: nil, backward: false)
                {
                    self.didInitialFocus = true
                    win.makeFirstResponder(firstTarget)
                }
            }

            guard keyMonitor == nil else { return }
            keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard let self,
                      let win = self.anchorView?.window,
                      event.window === win
                else { return event }

                let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
                if event.keyCode == 53 && flags.isEmpty {
                    self.onEscape()
                    return nil
                }
                if event.keyCode == 48 && flags.subtracting([.shift]).isEmpty {
                    if let root = self.drawerContainerRoot(),
                       let target = SubagentFocusTrapEngine.nextFocusTarget(
                           in: root,
                           current: win.firstResponder,
                           backward: flags.contains(.shift)
                       )
                    {
                        win.makeFirstResponder(target)
                        return nil
                    }
                }
                return event
            }
        }

        func detachAndRestoreFocus() {
            if let keyMonitor {
                NSEvent.removeMonitor(keyMonitor)
                self.keyMonitor = nil
            }
            let responder = savedResponder
            let win = savedWindow ?? anchorView?.window ?? (responder as? NSView)?.window
            DispatchQueue.main.async { [weak win, weak responder] in
                guard let win, let responder else { return }
                if let view = responder as? NSView, view.window !== win {
                    return
                }
                win.makeFirstResponder(responder)
            }
        }
    }
}
