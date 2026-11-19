import AppKit
import SwiftUI

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
        onClose: @escaping () -> Void
    ) {
        self.store = store
        self.provider = provider
        self.rootSessionKey = rootSessionKey
        self.collection = collection
        self.initialAgent = initialAgent
        self.scheme = scheme
        self.workspace = workspace
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
        .frame(width: LayoutMetrics.subagentDrawerWidth)
        .frame(maxHeight: .infinity)
        .background(palette.background.color)
        .overlay(alignment: .leading) {
            Rectangle().fill(palette.borderSubtle.color).frame(width: 1)
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
