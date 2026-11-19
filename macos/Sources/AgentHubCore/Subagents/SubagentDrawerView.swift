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

    public static func summaryHeaderTitle(provider: SessionProvider, totalCount: Int) -> String {
        _ = totalCount
        return provider == .claudeCode ? "本会话子智能体" : "子智能体"
    }

    @State private var isHovered: Bool = false

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
                let baseFg = (isHovered ? palette.textPrimary : palette.textMuted).color

                Button {
                    let responder = triggerAnchorView?.window?.firstResponder ?? triggerAnchorView
                    onTogglePanel(responder)
                } label: {
                    HStack(spacing: 8) {
                        LucideIcon(.bot, size: 16)
                            .foregroundStyle(baseFg)

                        if provider == .claudeCode {
                            Text(Self.summaryHeaderTitle(provider: provider, totalCount: totalCount))
                                .font(DesignFonts.body(12, weight: .medium))
                                .foregroundStyle(baseFg)
                            Text("\(totalCount)")
                                .font(DesignFonts.tabularBody(12, weight: .medium))
                                .foregroundStyle(baseFg)
                        } else {
                            Text(Self.summaryHeaderTitle(provider: provider, totalCount: totalCount))
                                .font(DesignFonts.body(12, weight: .medium))
                                .foregroundStyle(baseFg)

                            if runningCount > 0 {
                                HStack(spacing: 6) {
                                    RunningIndicator(size: 13)
                                    Text("\(runningCount) 个运行中")
                                        .font(DesignFonts.tabularBody(12, weight: .semibold))
                                        .foregroundStyle(palette.accent.color)
                                }
                            }
                            if completedCount > 0 {
                                Text("\(completedCount) 已完成")
                                    .font(DesignFonts.tabularBody(12, weight: .medium))
                                    .foregroundStyle(baseFg)
                            }
                            if creatingCount > 0 {
                                Text("\(creatingCount) 正在创建")
                                    .font(DesignFonts.tabularBody(12, weight: .medium))
                                    .foregroundStyle(baseFg)
                            }
                            if failedCount > 0 {
                                Text("\(failedCount) 失败")
                                    .font(DesignFonts.tabularBody(12, weight: .medium))
                                    .foregroundStyle(palette.errFg.color)
                            }
                            if interruptedCount > 0 {
                                Text("\(interruptedCount) 已中断")
                                    .font(DesignFonts.tabularBody(12, weight: .medium))
                                    .foregroundStyle(baseFg)
                            }
                            if unknownCount > 0 {
                                Text("\(unknownCount) 状态未知")
                                    .font(DesignFonts.tabularBody(12, weight: .medium))
                                    .foregroundStyle(baseFg)
                            }
                            if unavailableCount > 0 {
                                Text("\(unavailableCount) 不可用")
                                    .font(DesignFonts.tabularBody(12, weight: .medium))
                                    .foregroundStyle(palette.errFg.color)
                            }
                            LucideIcon(.chevronRight, size: 14)
                                .foregroundStyle(baseFg)
                        }

                        Spacer(minLength: 0)
                    }
                    .frame(minHeight: 28)
                    .padding(.vertical, 6)
                    .padding(.horizontal, 18)
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
                .onHover { isHovered = $0 }
                .accessibilityLabel(provider == .claudeCode ? "本会话子智能体" : "查看直属子智能体")
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

    public enum NavigationFrame: Equatable {
        case list(parent: SubagentActivity?, collection: SubagentCollection?)
        case detail(SubagentActivity)
    }

    @State private var frameStack: [NavigationFrame] = []
    @State private var scrollAnchorStack: [ConversationTableCoordinator.ScrollAnchorSnapshot?] = []
    @State private var pendingRestoreAnchor: ConversationTableCoordinator.ScrollAnchorSnapshot?
    @State private var currentDetailSubagents: SubagentCollection?
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
        if case let .detail(agent)? = frameStack.last {
            return agent
        }
        return nil
    }

    private var activeListCollection: SubagentCollection? {
        if case let .list(_, nestedCollection)? = frameStack.last {
            return nestedCollection ?? collection
        }
        return collection
    }

    public var body: some View {
        VStack(spacing: 0) {
            // Drawer Header
            HStack(spacing: 8) {
                if !frameStack.isEmpty {
                    Button {
                        popAgent()
                    } label: {
                        LucideIcon(.arrowLeft, size: 16)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 30))
                    .background(SubagentKeyTargetView(enabled: true, onActivate: { popAgent() }))
                    .accessibilityLabel("返回上一级")
                }

                LucideIcon(.bot, size: 16)
                    .foregroundStyle(palette.accent.color)

                Text(currentAgent?.name ?? (provider == .claudeCode ? "本会话子智能体 (\(activeListCollection?.agents.count ?? 0))" : "直属子智能体 (\(activeListCollection?.agents.count ?? 0))"))
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
                .background(SubagentKeyTargetView(enabled: true, onActivate: onClose))
                .accessibilityLabel("关闭子智能体面板")
            }
            .padding(.horizontal, 14)
            .frame(height: 48)
            .background(palette.surface.color)
            .overlay(alignment: .bottom) {
                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
            }

            if let current = currentAgent {
                // Child Codex subagent uses its OWN direct subagents collection (DESIGN.md:113, SubagentPanel.tsx:145)
                if provider == .codex,
                   let childCol = currentDetailSubagents,
                   SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: childCol, isSupported: true)
                {
                    SubagentSummaryBarView(
                        provider: .codex,
                        collection: childCol,
                        isSupported: true
                    ) { _ in
                        scrollAnchorStack.append(coordinatorRef?.captureScrollAnchor())
                        frameStack.append(.list(parent: current, collection: childCol))
                    }
                }

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
                        .background(SubagentKeyTargetView(enabled: true, onActivate: { delegationExpanded.toggle() }))

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
                            isSubagent: true,
                            canLoadOlder: hasMoreOlder && beforeCursor != nil,
                            isLoadingOlder: loadingOlder
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
                        let agents = Self.sortByStatusPriority(activeListCollection?.agents ?? [])
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
                                .background(
                                    SubagentKeyTargetView(enabled: agent.available) {
                                        pushAgent(agent)
                                    }
                                )
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
                    DispatchQueue.main.async {
                        if activeTimelineId != id { activeTimelineId = id }
                    }
                }
                coordinatorRef = c
            }
            if let initialAgent, frameStack.isEmpty {
                pushAgent(initialAgent)
            }
        }
        .task(id: currentAgent?.id) {
            guard let agent = currentAgent else { return }
            while !Task.isCancelled {
                if store.isWindowVisible {
                    await fetchSubagentDetail(agent)
                }
                let pollNs = Self.pollingIntervalNanoseconds(provider: provider, status: agent.status)
                try? await Task.sleep(nanoseconds: pollNs)
            }
        }
    }

    public static func pollingIntervalNanoseconds(provider: SessionProvider, status: String) -> UInt64 {
        if provider == .claudeCode {
            return 5_000_000_000
        }
        return (status == "running" || status == "creating") ? 2_000_000_000 : 5_000_000_000
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
              !frameStack.contains(where: {
                  if case let .detail(existing) = $0 { return existing.agentId == nextId }
                  return false
              })
        else { return }
        if !frameStack.isEmpty {
            scrollAnchorStack.append(coordinatorRef?.captureScrollAnchor())
        }
        pendingRestoreAnchor = nil
        delegationExpanded = false
        currentDetailSubagents = nil
        frameStack.append(.detail(agent))
        rows = []
        timelineEntries = []
        beforeCursor = nil
        hasMoreOlder = false
        errorText = nil
    }

    public func popAgent() {
        guard !frameStack.isEmpty else { return }
        frameStack.removeLast()
        delegationExpanded = false
        currentDetailSubagents = nil
        rows = []
        timelineEntries = []
        beforeCursor = nil
        hasMoreOlder = false
        errorText = nil
        pendingRestoreAnchor = scrollAnchorStack.isEmpty ? nil : scrollAnchorStack.removeLast()
    }

    private func fetchSubagentDetail(_ agent: SubagentActivity) async {
        guard let agentId = agent.agentId, !agentId.isEmpty else { return }
        if rows.isEmpty { loading = true }
        defer { if loading { loading = false } }
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
                let nextSubs = slot.subagents ?? resp.detail.subagents
                if currentDetailSubagents != nextSubs { currentDetailSubagents = nextSubs }
                if hasMoreOlder != slot.hasMoreBlocks { hasMoreOlder = slot.hasMoreBlocks }
                if beforeCursor != slot.beforeCursor { beforeCursor = slot.beforeCursor }
                let nextRows = ConversationRowBuilder.buildCodexRows(
                    blocks: slot.blocks,
                    showAllHistory: true,
                    planFallbackTitle: resp.agent.name,
                    subagentsSupported: true
                )
                if rows != nextRows { rows = nextRows }
                let nextTimeline = slot.timeline ?? TimelineViewModel.userTimelineEntries(
                    provider: .codex,
                    sources: slot.blocks.map { TimelineSourceItem(id: $0.id, role: $0.role, kind: $0.kind, text: $0.text, userMessage: $0.userMessage) }
                )
                if timelineEntries != nextTimeline { timelineEntries = nextTimeline }
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
                if currentDetailSubagents != nil { currentDetailSubagents = nil }
                if hasMoreOlder != slot.hasMore { hasMoreOlder = slot.hasMore }
                if beforeCursor != slot.beforeCursor { beforeCursor = slot.beforeCursor }
                let nextRows = ConversationRowBuilder.buildClaudeRows(
                    events: slot.events,
                    recordedOnly: true,
                    attachmentsAsNamesOnly: true
                )
                if rows != nextRows { rows = nextRows }
                if !timelineEntries.isEmpty { timelineEntries = [] }
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
public final class SubagentOverlayRootNSView: NSView {}

@MainActor
public final class SubagentKeyTargetNSView: NSView {
    public var isTargetEnabled: Bool = true
    public var onActivate: (() -> Void)?

    override public var acceptsFirstResponder: Bool { isTargetEnabled }
    override public var canBecomeKeyView: Bool { isTargetEnabled }

    override public func hitTest(_ point: NSPoint) -> NSView? {
        nil
    }

    override public func keyDown(with event: NSEvent) {
        if isTargetEnabled && (event.keyCode == 36 || event.keyCode == 76 || event.keyCode == 49) {
            onActivate?()
            return
        }
        super.keyDown(with: event)
    }
}

private struct SubagentKeyTargetView: NSViewRepresentable {
    var enabled: Bool
    var onActivate: () -> Void

    func makeNSView(context: Context) -> SubagentKeyTargetNSView {
        let v = SubagentKeyTargetNSView(frame: .zero)
        v.isTargetEnabled = enabled
        v.onActivate = onActivate
        return v
    }

    func updateNSView(_ nsView: SubagentKeyTargetNSView, context: Context) {
        nsView.isTargetEnabled = enabled
        nsView.onActivate = onActivate
    }
}

/// Dedicated AppKit container for `<960pt` modal SubagentPanelView so Tab focus never leaks into AppShellView.
public struct SubagentOverlayFocusContainerView<Content: View>: NSViewRepresentable {
    public let palette: DesignPalette
    @ViewBuilder public let content: () -> Content

    public init(palette: DesignPalette, @ViewBuilder content: @escaping () -> Content) {
        self.palette = palette
        self.content = content
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    public func makeNSView(context: Context) -> SubagentOverlayRootNSView {
        let root = SubagentOverlayRootNSView(frame: .zero)
        let host = NSHostingView(rootView: AnyView(content().environment(\.agentHubPalette, palette)))
        host.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(host)
        NSLayoutConstraint.activate([
            host.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            host.topAnchor.constraint(equalTo: root.topAnchor),
            host.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])
        context.coordinator.hostingView = host
        ModalPresentationRegistry.shared.register(context.coordinator.modalToken, container: root)
        return root
    }

    public func updateNSView(_ nsView: SubagentOverlayRootNSView, context: Context) {
        context.coordinator.hostingView?.rootView = AnyView(content().environment(\.agentHubPalette, palette))
        ModalPresentationRegistry.shared.register(context.coordinator.modalToken, container: nsView)
    }

    public static func dismantleNSView(_ nsView: SubagentOverlayRootNSView, coordinator: Coordinator) {
        ModalPresentationRegistry.shared.unregister(coordinator.modalToken)
    }

    @MainActor
    public final class Coordinator {
        let modalToken = UUID()
        var hostingView: NSHostingView<AnyView>?
    }
}

@MainActor
public enum SubagentFocusTrapEngine {
    public static func focusableViews(in root: NSView) -> [NSView] {
        var result: [NSView] = []
        func walk(_ view: NSView) {
            guard !view.isHidden, view.alphaValue > 0.01 else { return }
            if let keyTarget = view as? SubagentKeyTargetNSView {
                if keyTarget.isTargetEnabled, keyTarget.bounds.width > 1, keyTarget.bounds.height > 1 {
                    result.append(keyTarget)
                }
                return
            }
            if let control = view as? NSControl, !control.isEnabled {
                return
            }
            if !(view is NSScrollView),
               !(view is NSClipView),
               !(view is NSTableView),
               !(view is SubagentOverlayRootNSView),
               view.canBecomeKeyView || (view is NSButton) || (view is NSTextField) || (view is NSTextView)
            {
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
            var nearestHosting: NSView?
            while let v = cur {
                if v is SubagentOverlayRootNSView {
                    return v
                }
                if nearestHosting == nil, String(describing: type(of: v)).contains("HostingView") {
                    nearestHosting = v
                }
                cur = v.superview
            }
            return nearestHosting ?? anchorView?.superview
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
