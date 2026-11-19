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

    @State private var agentStack: [SubagentActivity] = []
    @State private var scrollAnchorStack: [ConversationTableCoordinator.ScrollAnchorSnapshot?] = []
    @State private var rows: [ConversationRowItem] = []
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
                if let errorText {
                    Text(errorText)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                        .padding(12)
                }
                if let coord = coordinatorRef {
                    ConversationReaderTableView(
                        coordinator: coord,
                        rows: rows,
                        scheme: scheme,
                        workspace: workspace,
                        sessionScopeKey: "\(store.machineScope):subagent:\(rootSessionKey):\(current.agentId ?? current.id)",
                        sessionProvider: provider,
                        sessionKey: current.agentId ?? rootSessionKey,
                        rootThreadId: provider == .codex ? rootSessionKey : nil,
                        hasTimelineRail: false,
                        isSubagent: true
                    )
                } else {
                    Spacer()
                }
            } else {
                // Agent List
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        if let warning = collection?.warning, !warning.isEmpty {
                            Text(warning)
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.warnFg.color)
                                .padding(.horizontal, 12)
                        }
                        let agents = collection?.agents ?? []
                        if agents.isEmpty {
                            Text("当前会话暂无子智能体记录")
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
                                            Text(agent.name)
                                                .font(DesignFonts.body(13, weight: .semibold))
                                                .foregroundStyle(palette.textPrimary.color)
                                            if let role = agent.role, !role.isEmpty {
                                                Text(role)
                                                    .font(DesignFonts.body(11))
                                                    .foregroundStyle(palette.textMuted.color)
                                            }
                                            Spacer()
                                            StatusChip(subagentStatusLabel(agent.status), tone: subagentTone(agent.status))
                                        }
                                        if let del = agent.delegation, !del.isEmpty {
                                            Text(del)
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
        .onAppear {
            if coordinatorRef == nil {
                let c = ConversationTableCoordinator(store: store)
                c.onOpenSubagent = { nextSub in
                    if provider == .codex {
                        pushAgent(nextSub)
                    }
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

    private func pushAgent(_ agent: SubagentActivity) {
        if !agentStack.isEmpty {
            scrollAnchorStack.append(coordinatorRef?.captureScrollAnchor())
        }
        agentStack.append(agent)
        rows = []
        errorText = nil
    }

    private func popAgent() {
        guard !agentStack.isEmpty else { return }
        agentStack.removeLast()
        rows = []
        errorText = nil
        if !scrollAnchorStack.isEmpty {
            _ = scrollAnchorStack.removeLast()
        }
    }

    private func fetchSubagentDetail(_ agent: SubagentActivity) async {
        guard let agentId = agent.agentId, !agentId.isEmpty else { return }
        loading = true
        defer { loading = false }
        do {
            if provider == .codex {
                struct ReqWrap: Encodable { let request: SubagentDetailRequest }
                let resp: SubagentDetailResponse = try await store.rpc(
                    "threads.subagentDetail",
                    args: ReqWrap(request: SubagentDetailRequest(rootThreadId: rootSessionKey, agentId: agentId, limit: 120))
                )
                rows = ConversationRowBuilder.buildCodexRows(
                    blocks: resp.detail.blocks,
                    showAllHistory: false,
                    planFallbackTitle: resp.agent.name,
                    subagentsSupported: true
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
                rows = ConversationRowBuilder.buildClaudeRows(
                    events: detail.events,
                    recordedOnly: true,
                    attachmentsAsNamesOnly: true
                )
            }
        } catch {
            errorText = error.localizedDescription
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
