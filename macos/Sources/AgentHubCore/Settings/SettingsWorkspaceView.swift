import SwiftUI

public struct SettingsWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    public init(store: RuntimeConnectionStore) {
        self.store = store
    }

    public var body: some View {
        GeometryReader { geo in
            let isNarrow = geo.size.width <= LayoutMetrics.narrowBreakpoint
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    // Settings Header & .settings-tabs Underline Tab Bar (styles.css:544-549)
                    VStack(alignment: .leading, spacing: 12) {
                        Text("设置")
                            .font(DesignFonts.body(20, weight: .bold))
                            .foregroundStyle(palette.textPrimary.color)

                        HStack(spacing: 20) {
                            tabButton(title: "更新与维护", section: .maintenance)
                            tabButton(title: "远程连接", section: .remote)
                            Spacer()
                        }
                        .overlay(alignment: .bottom) {
                            Rectangle()
                                .fill(palette.borderSubtle.color)
                                .frame(height: 1)
                        }
                    }

                    if store.settingsSection == .remote {
                        RemoteConnectionSettingsView(store: store)
                    } else {
                        MaintenanceOpsView(store: store, isNarrow: isNarrow)
                    }
                }
                .frame(maxWidth: LayoutMetrics.consolePageMaxWidth, alignment: .leading)
                .padding(.vertical, isNarrow ? 18 : 28)
                .padding(.horizontal, isNarrow ? 14 : 32)
                .frame(maxWidth: .infinity)
            }
        }
    }

    private func tabButton(title: String, section: RuntimeConnectionStore.SettingsTabSection) -> some View {
        let active = (store.settingsSection == section)
        return Button {
            store.settingsSection = section
        } label: {
            Text(title)
                .font(DesignFonts.body(13, weight: active ? .semibold : .medium))
                .foregroundStyle((active ? palette.textPrimary : palette.textMuted).color)
                .padding(.horizontal, 2)
                .padding(.vertical, 10)
                .overlay(alignment: .bottom) {
                    if active {
                        Rectangle()
                            .fill(palette.textPrimary.color)
                            .frame(height: 2)
                    }
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
    }
}

public struct MaintenanceOpsView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public var isNarrow: Bool

    @State private var updateStatus: UpdateStatus?
    private let sourceRevision: UInt64
    @State private var startingUpdate = false
    @State private var updateLoading: Bool = false
    @State private var updateError: String?
    @State private var lastUpdateAction: UnifiedUpdateActionKind?
    @State private var activeUpdateJobId: String?

    public init(store: RuntimeConnectionStore, isNarrow: Bool = false) {
        self.store = store
        self.sourceRevision = store.revision
        self.isNarrow = isNarrow
    }

    private var updateJob: JobRecord? {
        guard let id = activeUpdateJobId else { return nil }
        return store.trackedJobs[id]
    }

    private var updateBusy: Bool {
        if startingUpdate { return true }
        guard let id = activeUpdateJobId else { return false }
        guard let job = store.trackedJobs[id] else { return true }
        return !job.isTerminal
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            // .ops-header ("设置与维护" + Refresh, OpsWorkspace.tsx:67-85)
            HStack(spacing: 10) {
                HStack(spacing: 10) {
                    LucideIcon(.slidersHorizontal, size: 18)
                        .foregroundStyle(palette.accent.color)
                    Text("设置与维护")
                        .font(DesignFonts.body(22, weight: .bold))
                        .foregroundStyle(palette.textPrimary.color)
                }
                Spacer()
                Button {
                    Task { await fetchUpdateStatus() }
                } label: {
                    LucideIcon(.refreshCw, size: 17)
                }
                .buttonStyle(IconButtonStyle(dimension: 32))
                .disabled(updateLoading)
                .help("刷新版本与状态")
                .accessibilityLabel("刷新版本与状态")
            }

            // 3-Column .appearance-segmented Radio Group (styles.css:551-562)
            VStack(alignment: .leading, spacing: 10) {
                Text("外观")
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)

                HStack(spacing: 4) {
                    ForEach(ThemeMode.allCases, id: \.self) { option in
                        let active = (store.themeMode == option)
                        let icon: LucideIconName = switch option {
                        case .system: .monitor
                        case .dark: .moon
                        case .light: .sun
                        }
                        Button {
                            store.setTheme(option)
                        } label: {
                            HStack(spacing: 8) {
                                LucideIcon(icon, size: 16)
                                Text(option.label)
                                    .font(DesignFonts.body(13, weight: active ? .semibold : .medium))
                            }
                            .foregroundStyle((active ? palette.textPrimary : palette.textMuted).color)
                            .frame(maxWidth: .infinity, minHeight: 36)
                            .background(
                                RoundedRectangle(cornerRadius: 9, style: .continuous)
                                    .fill((active ? palette.hover.color : Color.clear))
                            )
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(PlainHitButtonStyle())
                    }
                }
                .padding(4)
                .frame(maxWidth: 520)
                .background(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(palette.surface.color)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                )
            }

            // Update Panel
            updatePanelSection

            // Unified 6-Scope Cleanup + Hidden Scan
            if store.capabilities.isReady {
                UnifiedThreadCleanupSection(store: store, isNarrow: isNarrow)
            }

            // Codex Account Scope
            if store.capabilities.isReady && store.capabilities.codexAccountScope {
                CodexAccountScopeSection(store: store)
            }

            // Lazy Execution History
            JobHistorySectionView(store: store, title: "执行记录")
        }
        .task(id: store.machineScope) {
            activeUpdateJobId = nil
            lastUpdateAction = nil
            await fetchUpdateStatus()
        }
        .onChange(of: updateJob?.status) { nextStatus in
            if nextStatus == "succeeded" || nextStatus == "failed" {
                Task { await fetchUpdateStatus() }
            }
        }
    }

    @ViewBuilder
    private var updatePanelSection: some View {
        let title = UpdateActionViewItem.panelTitle(capabilities: store.capabilities)
        let working = updateBusy || updateStatus?.state == "checking" || updateStatus?.state == "installing"
        let failure = updateError ?? (updateJob?.status == "failed" ? (updateJob?.error ?? "更新操作失败") : nil)
        let versionLabel: String = if let st = updateStatus {
            UpdateVersionFormatter.displayVersion(st.currentVersion)
        } else if updateLoading {
            "读取中…"
        } else {
            UpdateVersionFormatter.displayVersion(LayoutMetrics.fallbackAppVersion)
        }

        PanelCard(title: title, icon: .refreshCw) {
            VStack(alignment: .leading, spacing: 12) {
                FlowButtonRow(spacing: 28, lineSpacing: 14) {
                    HStack(spacing: 8) {
                        Text("当前版本")
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.textMuted.color)
                        Text(versionLabel)
                            .font(DesignFonts.tabularBody(14, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                    }
                    .frame(minWidth: 120, alignment: .leading)

                    if updateStatus?.updateAvailable == true,
                       let latest = updateStatus?.latestVersion,
                       !latest.isEmpty
                    {
                        HStack(spacing: 8) {
                            Text("可用版本")
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.textMuted.color)
                            Text(UpdateVersionFormatter.displayVersion(latest))
                                .font(DesignFonts.tabularBody(14, weight: .semibold))
                                .foregroundStyle(palette.accent.color)
                        }
                        .frame(minWidth: 120, alignment: .leading)
                    }

                    HStack(spacing: 8) {
                        ForEach(UpdateActionViewItem.actions(status: updateError == nil ? updateStatus : nil, capabilities: store.capabilities)) { item in
                            let icon: LucideIconName = switch item.action {
                            case .check: .refreshCw
                            case .install: .download
                            case .prune: .trash2
                            }
                            Button {
                                Task { await triggerUpdateAction(item.action) }
                            } label: {
                                HStack(spacing: 8) {
                                    LucideIcon(icon, size: 17)
                                    Text(item.label)
                                }
                            }
                            .buttonStyle(
                                item.tone == "primary"
                                    ? AnyButtonStyle(PrimaryButtonStyle())
                                    : item.tone == "danger"
                                        ? AnyButtonStyle(DangerButtonStyle(soft: true))
                                        : AnyButtonStyle(SecondaryButtonStyle())
                            )
                            .disabled(working || item.disabled)
                        }
                    }
                }

                if let failure, !failure.isEmpty {
                    Text(failure)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                } else if working {
                    HStack(spacing: 8) {
                        RunningIndicator(size: 14)
                        Text(
                            lastUpdateAction == .prune
                                ? "正在清理更新备份…"
                                : (lastUpdateAction == .install ? "正在下载并安装…" : "正在检查更新…")
                        )
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textMuted.color)
                    }
                } else if updateJob?.status == "succeeded" {
                    let msg: String? = switch lastUpdateAction {
                    case .prune: "更新备份已清理"
                    case .install: "更新完成"
                    case .check: (updateStatus?.updateAvailable == false) ? "已是最新版本" : nil
                    case .none: nil
                    }
                    if let msg {
                        Text(msg)
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.okFg.color)
                    }
                }
            }
        }
    }

    private func fetchUpdateStatus() async {
        updateLoading = true
        defer { updateLoading = false }
        do {
            let status: UpdateStatus = try await store.rpc("updates.status")
            updateStatus = status
            updateError = nil
        } catch {
            updateError = error.localizedDescription
        }
    }

    private func triggerUpdateAction(_ action: UnifiedUpdateActionKind) async {
        guard !updateBusy else { return }
        startingUpdate = true
        defer { startingUpdate = false }
        updateError = nil
        lastUpdateAction = action
        let command = switch action {
        case .check: "updates.check"
        case .install: "updates.install"
        case .prune: "updates.prune"
        }
        do {
            let res: StartedJobResponse = try await store.rpc(command, at: sourceRevision)
            activeUpdateJobId = res.jobId
            store.trackStartedJob(res.jobId)
            await fetchUpdateStatus()
        } catch {
            updateError = error.localizedDescription
        }
    }
}

// MARK: - Unified 6-Scope Cleanup + Hidden Scan (100-row pagination)

public struct UnifiedThreadCleanupSection: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public var isNarrow: Bool

    public static let scopesMeta: [(scope: String, legacyAlias: String?, agentTitle: String, scopeBadge: String, label: String, brand: AgentBrandId, workspace: WorkspaceId)] = [
        ("codex_archived", nil, "Codex", "已归档线程", "Codex 已归档线程", .codex, .codex),
        ("codex_hidden", nil, "Codex", "隐藏线程", "Codex 隐藏线程", .codex, .codex),
        ("antigravity_archived", nil, "Antigravity", "已归档与孤立子会话", "Antigravity 已归档与孤立子会话", .antigravity, .antigravity),
        ("dsh_archived", nil, "DeepSeek Harness", "已归档线程", "DeepSeek Harness 已归档线程", .dsh, .dsh),
        ("claude_empty", "claude_inactive", "Claude Code", "空残余会话", "Claude Code 空残余会话", .claude, .claude),
        ("grok_empty", "grok_inactive", "Grok Build", "空残余会话", "Grok Build 空残余会话", .grok, .grok),
    ]

    @State private var plan: UnifiedCleanupPlan?
    @State private var hiddenScan: HiddenScanProgress?
    @State private var hiddenScanStopped: Bool = false
    @State private var result: UnifiedCleanupResult?
    @State private var selectedScopes: Set<String> = []
    @State private var confirmation: (revision: UInt64, request: UnifiedCleanupExecuteRequest)?
    private var armed: Bool { confirmation != nil }
    @State private var expandedScope: String?
    @State private var candidatePage: Int = 0
    @State private var resultPage: Int = 0
    @State private var previewTask: Task<Void, Never>?
    @State private var hiddenRunner: HiddenCleanupScan?
    private var scanning: Bool { previewTask != nil || hiddenRunner != nil }
    @State private var executing: Bool = false
    @State private var errorMessage: String?
    @State private var hiddenError: String?

    public init(store: RuntimeConnectionStore, isNarrow: Bool = false) {
        self.store = store
        self.isNarrow = isNarrow
    }

    private var busy: Bool { scanning || executing }

    private func lookupScopePlan(_ meta: (scope: String, legacyAlias: String?, agentTitle: String, scopeBadge: String, label: String, brand: AgentBrandId, workspace: WorkspaceId)) -> UnifiedCleanupScopePlan? {
        plan?.scopes.first(where: { $0.scope == meta.scope || (meta.legacyAlias != nil && $0.scope == meta.legacyAlias) })
    }

    private var activeSelectedScopes: [String] {
        guard let plan else { return [] }
        return plan.scopes
            .filter { selectedScopes.contains($0.scope) && $0.supported && $0.scanned && $0.deletableCount > 0 }
            .map(\.scope)
    }

    private var selectedDeletableCount: Int {
        guard let plan else { return 0 }
        return plan.scopes
            .filter { activeSelectedScopes.contains($0.scope) }
            .reduce(0) { $0 + $1.deletableCount }
    }

    private var selectedEstimatedBytes: Int64 {
        guard let plan else { return 0 }
        return plan.scopes
            .filter { activeSelectedScopes.contains($0.scope) }
            .reduce(0) { $0 + $1.estimatedBytes }
    }

    public var body: some View {
        PanelCard(
            title: "全平台线程清理",
            icon: .sparkles,
            headerAction: {
                let stageText: String = executing ? "执行中" : scanning ? "扫描中" : armed ? "等待确认" : hiddenScanStopped ? "已停止" : (plan != nil ? (selectedDeletableCount > 0 ? "可清理" : "无可清理") : "待扫描")
                let tone: StatusChipTone = executing || scanning ? .warning : armed ? .danger : (plan != nil ? (selectedDeletableCount > 0 ? .warning : .success) : .neutral)
                StatusChip(stageText, tone: tone)
            }
        ) {
            VStack(alignment: .leading, spacing: 12) {
                let metricCols = isNarrow ? [GridItem(.flexible()), GridItem(.flexible())] : [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())]
                LazyVGrid(columns: metricCols, spacing: 10) {
                    MetricCard(
                        label: "可清理会话",
                        value: plan.map { "\(selectedDeletableCount) / \($0.totalDeletable)" } ?? "待扫描预览",
                        tone: selectedDeletableCount > 0 ? .warning : nil
                    )
                    MetricCard(
                        label: "预计释放空间",
                        value: plan != nil ? CleanupFormatting.formatBytes(selectedEstimatedBytes) : "待扫描预览"
                    )
                    MetricCard(
                        label: "已选平台范围",
                        value: plan.map { "\(activeSelectedScopes.count) / \($0.scopes.filter(\.supported).count)" } ?? "待扫描预览"
                    )
                }

                let scopeCols = isNarrow ? [GridItem(.flexible())] : [GridItem(.flexible()), GridItem(.flexible())]
                LazyVGrid(columns: scopeCols, spacing: 10) {
                    ForEach(Self.scopesMeta, id: \.scope) { meta in
                        let sp = lookupScopePlan(meta)
                        let resolvedKey = sp?.scope ?? meta.scope
                        let scopePalette = DesignPalette.resolve(scheme: palette.scheme, workspace: meta.workspace)
                        let canSelect = (sp?.supported == true && sp?.scanned == true && (sp?.deletableCount ?? 0) > 0)
                        let isSelected = activeSelectedScopes.contains(resolvedKey)
                        let totalEst = max(Int64(1), plan?.totalEstimatedBytes ?? 0)
                        let ratioPct: CGFloat = (sp?.scanned == true && (sp?.deletableCount ?? 0) > 0 && (plan?.totalEstimatedBytes ?? 0) > 0)
                            ? max(0.06, min(1.0, CGFloat(sp?.estimatedBytes ?? 0) / CGFloat(totalEst)))
                            : 0

                        VStack(alignment: .leading, spacing: 10) {
                            HStack(alignment: .top, spacing: 10) {
                                AgentBrandIcon(meta.brand, size: 17)
                                    .frame(width: 24, height: 24)
                                    .background(RoundedRectangle(cornerRadius: 7).fill(scopePalette.accentSoft.color))

                                VStack(alignment: .leading, spacing: 3) {
                                    HStack(spacing: 6) {
                                        Text(meta.agentTitle)
                                            .font(DesignFonts.body(13, weight: .semibold))
                                            .foregroundStyle(palette.textPrimary.color)
                                        Text(meta.scopeBadge)
                                            .font(DesignFonts.body(11, weight: .medium))
                                            .foregroundStyle(palette.textSecondary.color)
                                            .padding(.horizontal, 7)
                                            .padding(.vertical, 1)
                                            .background(Capsule().fill(palette.surface.color))
                                            .overlay(Capsule().strokeBorder(palette.borderStrong.color, lineWidth: 1))
                                    }
                                    Text(scopeSummary(meta.scope, scopePlan: sp))
                                        .font(DesignFonts.body(11))
                                        .foregroundStyle(((sp?.deletableCount ?? 0) > 0 ? palette.warnFg : palette.textMuted).color)
                                        .lineLimit(2)
                                }

                                Spacer(minLength: 4)

                                // 17x17pt Checkbox (styles.css:631)
                                Button {
                                    confirmation = nil
                                    if isSelected {
                                        selectedScopes.remove(resolvedKey)
                                    } else if canSelect {
                                        selectedScopes.insert(resolvedKey)
                                    }
                                } label: {
                                    ZStack {
                                        RoundedRectangle(cornerRadius: 4, style: .continuous)
                                            .fill((isSelected ? scopePalette.accent : palette.elevated).color)
                                            .frame(width: 17, height: 17)
                                            .overlay(
                                                RoundedRectangle(cornerRadius: 4, style: .continuous)
                                                    .strokeBorder((isSelected ? scopePalette.accent : palette.borderStrong).color, lineWidth: 1)
                                            )
                                        if isSelected {
                                            LucideIcon(.check, size: 11)
                                                .foregroundStyle(palette.buttonFg.color)
                                        }
                                    }
                                    .frame(width: 24, height: 24)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(PlainHitButtonStyle())
                                .disabled(!canSelect || busy)
                                .accessibilityLabel("选择 \(meta.label)")
                            }

                            // 5pt .unified-scope-bar usage ratio bar (styles.css:634-637)
                            if let sp, sp.scanned, !sp.issues.isEmpty {
                                Text(sp.issues.joined(separator: "\n"))
                                    .font(DesignFonts.body(11))
                                    .foregroundStyle(palette.warnFg.color)
                            }
                            if ratioPct > 0 {
                                GeometryReader { barGeo in
                                    ZStack(alignment: .leading) {
                                        Capsule()
                                            .fill(palette.hover.color)
                                            .frame(height: 5)
                                        Capsule()
                                            .fill(scopePalette.accent.color)
                                            .frame(width: max(8, barGeo.size.width * ratioPct), height: 5)
                                    }
                                }
                                .frame(height: 5)
                            }

                            if let sp, !sp.candidates.isEmpty {
                                HStack {
                                    Text("共 \(sp.candidates.count) 条候选")
                                        .font(DesignFonts.body(11))
                                        .foregroundStyle(palette.textMuted.color)
                                    Spacer()
                                    Button(expandedScope == resolvedKey ? "收起" : "详情") {
                                        candidatePage = 0
                                        expandedScope = (expandedScope == resolvedKey) ? nil : resolvedKey
                                    }
                                    .font(DesignFonts.body(11, weight: .medium))
                                    .foregroundStyle(palette.textSecondary.color)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 3)
                                    .background(Capsule().fill(palette.surface.color))
                                    .overlay(Capsule().strokeBorder(palette.borderStrong.color, lineWidth: 1))
                                    .buttonStyle(PlainHitButtonStyle())
                                }
                                .padding(.top, 4)
                                .overlay(alignment: .top) {
                                    Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                                }
                            }
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 13)
                        .background(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .fill((sp?.supported == false ? palette.surface : palette.elevated).color)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder((isSelected ? scopePalette.accentBorder : palette.borderStrong).color, lineWidth: 1)
                        )
                        // 2.5pt --a top accent border when checked (styles.css:627-628)
                        .overlay(alignment: .top) {
                            if isSelected {
                                UnevenTopAccentBar(color: scopePalette.accent.color, height: 2.5, cornerRadius: 12)
                            }
                        }
                        .opacity(sp?.supported == false ? 0.78 : 1.0)
                    }
                }

                if let expanded = expandedScope,
                   let sp = plan?.scopes.first(where: { $0.scope == expanded }),
                   !sp.candidates.isEmpty
                {
                    let slice = paginate(sp.candidates, page: candidatePage)
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(slice) { cand in
                            HStack {
                                Text(cand.title.isEmpty ? cand.id : cand.title)
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .lineLimit(1)
                                Spacer()
                                Text(cand.blockedReason ?? CleanupFormatting.formatBytes(cand.estimatedBytes))
                                    .font(DesignFonts.mono(11))
                                    .foregroundStyle(palette.textMuted.color)
                            }
                        }
                        if sp.candidates.count > 100 {
                            paginationBar(page: $candidatePage, totalCount: sp.candidates.count)
                        }
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
                }

                if let result {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("已清理 \(result.deletedCount) 项 · 释放 \(CleanupFormatting.formatBytes(result.deletedBytes)) · 跳过 \(result.skippedCount) 项 · 失败 \(result.failedCount) 项")
                            .font(DesignFonts.body(12, weight: .medium))
                            .foregroundStyle((result.failedCount == 0 ? palette.okFg : palette.warnFg).color)
                        let failures = result.unsuccessfulDetails
                        if !failures.isEmpty {
                            ForEach(paginate(failures, page: resultPage).indices, id: \.self) { index in
                                Text(failures[index])
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.errFg.color)
                            }
                            if failures.count > 100 {
                                paginationBar(page: $resultPage, totalCount: failures.count)
                            }
                        }
                    }
                }

                if let hiddenError {
                    Text(hiddenError).font(DesignFonts.body(12)).foregroundStyle(palette.errFg.color)
                }
                if let errorMessage, !errorMessage.isEmpty {
                    Text(errorMessage)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                }

                FlowButtonRow(spacing: 8, lineSpacing: 8) {
                    Button {
                        runPreview(includeUnified: true)
                    } label: {
                        HStack(spacing: 6) {
                            LucideIcon(.search, size: 15)
                            Text("全平台扫描预览")
                        }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(busy)

                    if store.capabilities.isReady && store.capabilities.hiddenCleanupScan {
                        if hiddenRunner != nil {
                            Button("取消隐藏扫描") {
                                cancelHiddenScan()
                            }
                            .buttonStyle(SecondaryButtonStyle())
                        } else {
                            Button("扫描隐藏线程") {
                                runPreview(includeUnified: false)
                            }
                            .buttonStyle(SecondaryButtonStyle())
                            .disabled(busy)
                        }
                    }

                    if !armed {
                        Button("一键清理所选平台") {
                            guard let request = plan?.executionRequest(for: selectedScopes), !request.targets.isEmpty else { return }
                            confirmation = (store.revision, request)
                        }
                        .buttonStyle(DangerButtonStyle(soft: true))
                        .disabled(busy || selectedDeletableCount == 0)
                    } else {
                        Button("确认清理所选") {
                            Task { await executeCleanup() }
                        }
                        .buttonStyle(DangerButtonStyle(soft: false))
                        .disabled(busy)

                        Button("取消") {
                            confirmation = nil
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(busy)
                    }
                }
            }
        }
        .onDisappear {
            previewTask?.cancel()
            cancelHiddenScan()
        }
    }

    private func scopeSummary(_ scope: String, scopePlan: UnifiedCleanupScopePlan?) -> String {
        guard let sp = scopePlan else {
            return plan == nil ? "点击下方全平台扫描预览" : "未扫描"
        }
        if !sp.supported { return sp.unavailableReason ?? "当前环境不支持" }
        if !sp.scanned {
            if scope == "codex_hidden" {
                if scanning {
                    let scanned = hiddenScan?.processed ?? sp.candidates.count
                    return scanned > 0 ? "正在深度扫描隐藏线程（已扫 \(scanned) 个文件）…" : "正在深度扫描隐藏线程…"
                }
                if hiddenScanStopped && !sp.candidates.isEmpty {
                    return "已停止扫描 · 已保留 \(sp.candidates.count) 条已扫候选（需完整扫描后方可清理）"
                }
            }
            return sp.issues.first ?? "未深度扫描"
        }
        return sp.deletableCount > 0
            ? "\(sp.deletableCount) 项可清理 · \(CleanupFormatting.formatBytes(sp.estimatedBytes))"
            : "无可清理项"
    }

    private func paginate<T>(_ items: [T], page: Int) -> ArraySlice<T> {
        let start = max(0, page * 100)
        let end = min(items.count, start + 100)
        return start < end ? items[start..<end] : []
    }

    private func paginationBar(page: Binding<Int>, totalCount: Int) -> some View {
        let pages = max(1, Int(ceil(Double(totalCount) / 100.0)))
        return FlowButtonRow(spacing: 8, lineSpacing: 6) {
            Button("上一页") { page.wrappedValue = max(0, page.wrappedValue - 1) }
                .buttonStyle(SecondaryButtonStyle())
                .disabled(page.wrappedValue == 0)
            Text("第 \(page.wrappedValue + 1) / \(pages) 页")
                .font(DesignFonts.body(12))
                .foregroundStyle(palette.textMuted.color)
            Button("下一页") { page.wrappedValue = min(pages - 1, page.wrappedValue + 1) }
                .buttonStyle(SecondaryButtonStyle())
                .disabled(page.wrappedValue + 1 >= pages)
        }
    }

    private func runPreview(includeUnified: Bool) {
        let revision = store.revision
        confirmation = nil
        result = nil
        errorMessage = nil
        hiddenError = nil
        hiddenScan = nil
        hiddenScanStopped = false
        candidatePage = 0
        resultPage = 0
        selectedScopes.remove("codex_hidden")
        if includeUnified {
            plan = nil
            expandedScope = nil
            selectedScopes = []
            previewTask = Task { @MainActor in
                defer { if !Task.isCancelled { previewTask = nil } }
                do {
                    var unified: UnifiedCleanupPlan = try await store.rpc("cleanup.unifiedDryRun")
                    guard store.revision == revision, !Task.isCancelled else { return }
                    if let hidden = plan?.scopes.first(where: { $0.scope == "codex_hidden" }) {
                        unified.replaceHidden(hidden)
                    }
                    plan = unified
                    selectedScopes = Set(unified.scopes.filter { $0.supported && $0.scanned && $0.deletableCount > 0 }.map(\.scope))
                    expandedScope = expandedScope ?? unified.scopes.first(where: { !$0.candidates.isEmpty })?.scope
                } catch {
                    if store.revision == revision, !Task.isCancelled { errorMessage = error.localizedDescription }
                }
            }
        } else {
            plan?.replaceHidden(UnifiedCleanupScopePlan(scope: "codex_hidden", supported: true, scanned: false, candidateCount: 0,
                                                       deletableCount: 0, estimatedBytes: 0, candidates: [], issues: []))
        }
        guard store.capabilities.isReady else { return }
        let runner = HiddenCleanupScan { request in
            try await store.rpc("cleanup.hiddenDryRun", args: CleanupRequestEnvelope(request: request), at: revision)
        }
        hiddenRunner = runner
        let cleanupToken = store.registerBeforeMachineChange {
            if hiddenRunner === runner { await cancelHiddenScan()?.value }
            else { await runner.stop()?.value }
        }
        Task { @MainActor in
            defer {
                store.unregisterBeforeMachineChange(cleanupToken)
                if hiddenRunner === runner { hiddenRunner = nil }
            }
            do {
                try await runner.run { progress, scope in
                    guard store.revision == revision, hiddenRunner === runner else { return }
                    hiddenScan = progress
                    hiddenScanStopped = progress.phase == .stopped
                    var merged = plan ?? UnifiedCleanupPlan(totalCandidates: 0, totalDeletable: 0, totalEstimatedBytes: 0, scopes: [])
                    merged.replaceHidden(scope)
                    plan = merged
                    if scope.scanned && scope.deletableCount > 0 {
                        selectedScopes.insert(scope.scope)
                        expandedScope = expandedScope ?? scope.scope
                    }
                }
            } catch {
                if store.revision == revision, hiddenRunner === runner { hiddenError = error.localizedDescription }
            }
        }
    }

    @discardableResult
    private func cancelHiddenScan() -> Task<Void, Never>? {
        guard let runner = hiddenRunner else { return nil }
        selectedScopes.remove("codex_hidden")
        confirmation = nil
        hiddenScanStopped = true
        let cancellation = runner.stop()
        // Keep the scan busy until the server has received the cancellation.
        return cancellation
    }

    private func executeCleanup() async {
        guard let pending = confirmation, pending.revision == store.revision, !executing else { return }
        confirmation = nil
        executing = true
        errorMessage = nil
        defer { executing = false }
        do {
            let outcome: UnifiedCleanupResult = try await store.rpc(
                "cleanup.unifiedExecute", args: CleanupRequestEnvelope(request: pending.request), at: pending.revision
            )
            result = outcome
            plan?.apply(outcome)
            selectedScopes = Set(activeSelectedScopes)
            expandedScope = plan?.scopes.first(where: { !$0.candidates.isEmpty })?.scope
            resultPage = 0
            candidatePage = 0
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}

// MARK: - Codex Account Scope Section (with 20s iterative progress loop)

public struct CodexAccountScopeSection: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    private let sourceRevision: UInt64
    @State private var scopedThreads: Int?
    @State private var result: CodexAccountScopeResult?
    @State private var autoEnabled: Bool = false
    @State private var savingAuto = false
    @State private var hookInstalled: Bool?
    @State private var checking: Bool = false
    @State private var clearing: Bool = false
    @State private var armed: Bool = false
    @State private var page: Int = 0
    @State private var errorMessage: String?

    public init(store: RuntimeConnectionStore) {
        self.store = store
        self.sourceRevision = store.revision
    }

    private var busy: Bool { checking || clearing }

    public var body: some View {
        let stage: (label: String, tone: StatusChipTone) = if clearing {
            ("解除中", .warning)
        } else if checking {
            ("检查中", .warning)
        } else if armed {
            ("等待确认", .danger)
        } else if let scoped = scopedThreads {
            scoped > 0 ? ("\(scoped) 个会话", .warning) : ("无隔离会话", .success)
        } else {
            ("待检查", .neutral)
        }

        PanelCard(
            title: "Codex 账号隔离",
            icon: .lockKeyholeOpen,
            headerAction: {
                StatusChip(stage.label, tone: stage.tone)
            }
        ) {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("回合结束后自动解除")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textPrimary.color)
                    if hookInstalled == false {
                        StatusChip("未安装 Codex Hook", tone: .neutral)
                    }
                    Spacer()
                    CapsuleSwitch(
                        isOn: Binding(
                            get: { autoEnabled },
                            set: { val in Task { await setAutoClear(val) } }
                        ),
                        label: "回合结束后自动解除"
                    )
                    .disabled(hookInstalled != true || busy || savingAuto)
                }

                if let result {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 12) {
                            Text("已解除 \(result.clearedThreads) 个会话")
                                .font(DesignFonts.body(12, weight: .semibold))
                                .foregroundStyle(palette.textPrimary.color)
                            if result.remainingFiles > 0 {
                                Text("剩余 \(result.remainingFiles) 个文件")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textMuted.color)
                            }
                        }
                        let start = page * 100
                        let end = min(result.items.count, start + 100)
                        if start < end {
                            ForEach(result.items[start..<end]) { item in
                                Text("\(item.threadId)：\(item.reasonLabel)\(item.detail.map { "（\($0)）" } ?? "")")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textSecondary.color)
                            }
                        }
                        if result.items.count > 100 {
                            let pages = max(1, Int(ceil(Double(result.items.count) / 100.0)))
                            FlowButtonRow(spacing: 8, lineSpacing: 6) {
                                Button("上一页") { page = max(0, page - 1) }
                                    .buttonStyle(SecondaryButtonStyle())
                                    .disabled(page == 0)
                                Text("第 \(page + 1) / \(pages) 页")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textMuted.color)
                                Button("下一页") { page = min(pages - 1, page + 1) }
                                    .buttonStyle(SecondaryButtonStyle())
                                    .disabled(page + 1 >= pages)
                            }
                        }
                    }
                }

                if let errorMessage, !errorMessage.isEmpty {
                    Text(errorMessage)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                }

                FlowButtonRow(spacing: 8, lineSpacing: 8) {
                    Button {
                        Task { await runCheck() }
                    } label: {
                        HStack(spacing: 6) {
                            LucideIcon(.search, size: 15)
                            Text("检查")
                        }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(busy)

                    if !armed {
                        Button {
                            armed = true
                        } label: {
                            HStack(spacing: 6) {
                                LucideIcon(.lockKeyholeOpen, size: 16)
                                Text("解除")
                            }
                        }
                        .buttonStyle(DangerButtonStyle(soft: true))
                        .disabled(busy || (scopedThreads ?? 0) == 0)
                    } else {
                        Button {
                            armed = false
                            Task { await runIterativeClear() }
                        } label: {
                            HStack(spacing: 6) {
                                LucideIcon(.lockKeyholeOpen, size: 16)
                                Text("确认解除")
                            }
                        }
                        .buttonStyle(DangerButtonStyle(soft: false))
                        .disabled(busy)

                        Button("取消") {
                            armed = false
                        }
                        .buttonStyle(SecondaryButtonStyle())
                    }
                }
            }
        }
        .task(id: store.machineScope) {
            scopedThreads = nil
            result = nil
            hookInstalled = nil
            armed = false
            page = 0
            errorMessage = nil
            await loadHookAndAutoState()
        }
    }

    private func loadHookAndAutoState() async {
        do {
            let st: ProbeStatus = try await store.rpc("probe.status")
            if st.available == false {
                hookInstalled = nil
                errorMessage = st.error ?? "Probe 状态不可用"
            } else {
                hookInstalled = (st.hookStatus == "managed")
            }
        } catch {
            hookInstalled = nil
            errorMessage = error.localizedDescription
        }
        do {
            let loaded: ProbeSettingsView = try await store.rpc("probe.settings.get")
            let d = ProbeSettingsDraft.from(loaded)
            autoEnabled = d.clearAccountScope
        } catch {
            if errorMessage == nil {
                errorMessage = error.localizedDescription
            }
        }
    }

    private func setAutoClear(_ next: Bool) async {
        guard !savingAuto else { return }
        savingAuto = true
        defer { savingAuto = false }
        errorMessage = nil
        do {
            autoEnabled = try await store.saveAutoClearAccountScope(next, at: sourceRevision)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func runCheck() async {
        armed = false
        page = 0
        result = nil
        errorMessage = nil
        checking = true
        defer { checking = false }
        do {
            let status: CodexAccountScopeStatus = try await store.rpc("threads.accountScopeStatus")
            scopedThreads = status.scopedThreads
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func runIterativeClear() async {
        clearing = true
        errorMessage = nil
        page = 0
        defer { clearing = false }
        do {
            try await store.clearAccountScope(at: sourceRevision) { result = $0 }
            let status: CodexAccountScopeStatus = try await store.rpc("threads.accountScopeStatus", at: sourceRevision)
            scopedThreads = status.scopedThreads
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
