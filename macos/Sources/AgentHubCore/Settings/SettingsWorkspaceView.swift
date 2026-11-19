import SwiftUI

public struct SettingsWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    public init(store: RuntimeConnectionStore) {
        self.store = store
    }

    public var body: some View {
        GeometryReader { geo in
            let isNarrow = geo.size.width <= LayoutMetrics.mobileBreakpoint
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
    @State private var updateLoading: Bool = false
    @State private var updateError: String?
    @State private var lastUpdateAction: UnifiedUpdateActionKind?
    @State private var activeUpdateJobId: String?

    public init(store: RuntimeConnectionStore, isNarrow: Bool = false) {
        self.store = store
        self.isNarrow = isNarrow
    }

    private var updateJob: JobRecord? {
        guard let id = activeUpdateJobId else { return nil }
        return store.trackedJobs[id]
    }

    private var updateBusy: Bool {
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
            if store.capabilities.threadCleanup {
                UnifiedThreadCleanupSection(store: store, isNarrow: isNarrow)
            }

            // Codex Account Scope
            if store.capabilities.codexAccountScope {
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
        updateError = nil
        lastUpdateAction = action
        let command = switch action {
        case .check: "updates.check"
        case .install: "updates.install"
        case .prune: "updates.prune"
        }
        do {
            let res: StartedJobResponse = try await store.rpc(command)
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
        ("antigravity_archived", nil, "Antigravity", "已归档线程", "Antigravity 已归档线程", .antigravity, .antigravity),
        ("dsh_archived", nil, "DeepSeek Harness", "已归档线程", "DeepSeek Harness 已归档线程", .dsh, .dsh),
        ("claude_empty", "claude_inactive", "Claude Code", "空残余会话", "Claude Code 空残余会话", .claude, .claude),
        ("grok_empty", "grok_inactive", "Grok Build", "空残余会话", "Grok Build 空残余会话", .grok, .grok),
    ]

    @State private var plan: UnifiedCleanupPlan?
    @State private var hiddenScan: HiddenScanProgress?
    @State private var hiddenScanStopped: Bool = false
    @State private var result: UnifiedCleanupResult?
    @State private var selectedScopes: Set<String> = []
    @State private var armed: Bool = false
    @State private var expandedScope: String?
    @State private var candidatePage: Int = 0
    @State private var resultPage: Int = 0
    @State private var scanning: Bool = false
    @State private var executing: Bool = false
    @State private var errorMessage: String?
    @State private var hiddenScanTask: Task<Void, Never>?

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
                                    armed = false
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
                                Text(cand.title ?? cand.targetId)
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .lineLimit(1)
                                Spacer()
                                Text(CleanupFormatting.formatBytes(cand.estimatedBytes))
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
                        Text("已清理 \(result.deletedCount) 项 · 释放 \(CleanupFormatting.formatBytes(result.freedBytes)) · 失败 \(result.failedCount) 项")
                            .font(DesignFonts.body(12, weight: .medium))
                            .foregroundStyle((result.failedCount == 0 ? palette.okFg : palette.warnFg).color)
                        let failures = result.items.filter { $0.status != "deleted" }
                        if !failures.isEmpty {
                            ForEach(paginate(failures, page: resultPage)) { item in
                                Text("\(item.targetId)：\(item.error ?? item.status)")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.errFg.color)
                            }
                            if failures.count > 100 {
                                paginationBar(page: $resultPage, totalCount: failures.count)
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
                        runPreview(includeUnified: true)
                    } label: {
                        HStack(spacing: 6) {
                            LucideIcon(.search, size: 15)
                            Text("全平台扫描预览")
                        }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(busy)

                    if store.capabilities.hiddenCleanupScan {
                        if hiddenScanTask != nil {
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
                            armed = true
                        }
                        .buttonStyle(DangerButtonStyle(soft: true))
                        .disabled(Self.isCleanupExecuteDisabled(
                            busy: busy,
                            hiddenScanStopped: hiddenScanStopped,
                            selectedDeletableCount: selectedDeletableCount
                        ))
                    } else {
                        Button("确认清理所选") {
                            Task { await executeCleanup() }
                        }
                        .buttonStyle(DangerButtonStyle(soft: false))
                        .disabled(busy || hiddenScanStopped)

                        Button("取消") {
                            armed = false
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(busy)
                    }
                }
            }
        }
        .onChange(of: store.revision) { _ in
            cancelHiddenScan()
            plan = nil
            hiddenScan = nil
            hiddenScanStopped = false
            result = nil
            selectedScopes = []
            armed = false
            expandedScope = nil
            candidatePage = 0
            resultPage = 0
            errorMessage = nil
        }
        .onDisappear {
            cancelHiddenScan()
        }
    }

    public static func isCleanupExecuteDisabled(
        busy: Bool,
        hiddenScanStopped: Bool,
        selectedDeletableCount: Int
    ) -> Bool {
        busy || hiddenScanStopped || selectedDeletableCount == 0
    }

    private func scopeSummary(_ scope: String, scopePlan: UnifiedCleanupScopePlan?) -> String {
        guard let sp = scopePlan else {
            return plan == nil ? "点击下方全平台扫描预览" : "未扫描"
        }
        if !sp.supported { return sp.unavailableReason ?? "当前环境不支持" }
        if !sp.scanned {
            if scope == "codex_hidden" {
                if scanning {
                    let scanned = hiddenScan?.scannedFiles ?? sp.candidates.count
                    return scanned > 0 ? "正在深度扫描隐藏线程（已扫 \(scanned) 个文件）…" : "正在深度扫描隐藏线程…"
                }
                if hiddenScanStopped && !sp.candidates.isEmpty {
                    return "已停止扫描 · 已保留 \(sp.candidates.count) 条已扫候选（需完整扫描后方可清理）"
                }
            }
            return "未深度扫描"
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

    public static func mergeHiddenPageIntoPlan(
        existing: UnifiedCleanupPlan?,
        pagePlan: HiddenThreadDeletePlan,
        accumulated: [UnifiedCleanupCandidate],
        completed: Bool
    ) -> UnifiedCleanupPlan {
        let hiddenScope = UnifiedCleanupScopePlan(
            scope: "codex_hidden",
            supported: true,
            scanned: completed,
            unavailableReason: nil,
            candidateCount: max(accumulated.count, pagePlan.candidateCount),
            deletableCount: completed ? pagePlan.deletableCount : accumulated.filter(\.deletable).count,
            estimatedBytes: completed ? pagePlan.estimatedBytes : accumulated.reduce(Int64(0)) { $0 + $1.estimatedBytes },
            candidates: accumulated
        )
        if var cur = existing {
            if cur.scopes.contains(where: { $0.scope == "codex_hidden" }) {
                cur.scopes = cur.scopes.map { $0.scope == "codex_hidden" ? hiddenScope : $0 }
            } else {
                cur.scopes.append(hiddenScope)
            }
            cur.totalCandidates = cur.scopes.reduce(0) { $0 + $1.candidateCount }
            cur.totalDeletable = cur.scopes.filter(\.scanned).reduce(0) { $0 + $1.deletableCount }
            cur.totalEstimatedBytes = cur.scopes.filter(\.scanned).reduce(Int64(0)) { $0 + $1.estimatedBytes }
            return cur
        }
        return UnifiedCleanupPlan(
            totalCandidates: hiddenScope.candidateCount,
            totalDeletable: completed ? hiddenScope.deletableCount : 0,
            totalEstimatedBytes: completed ? hiddenScope.estimatedBytes : 0,
            scopes: [hiddenScope]
        )
    }

    private func runPreview(includeUnified: Bool) {
        let capturedRevision = store.revision
        armed = false
        result = nil
        errorMessage = nil
        hiddenScanStopped = false
        candidatePage = 0
        resultPage = 0
        hiddenScanTask?.cancel()
        hiddenScanTask = Task { @MainActor in
            scanning = true
            defer {
                scanning = false
                hiddenScanTask = nil
            }
            do {
                if includeUnified {
                    let unified: UnifiedCleanupPlan = try await store.rpc("cleanup.unifiedDryRun")
                    guard store.revision == capturedRevision else { return }
                    plan = unified
                    selectedScopes = Set(unified.scopes.filter { $0.supported && $0.scanned && $0.deletableCount > 0 }.map(\.scope))
                } else if var cur = plan {
                    // Fresh hidden rescan clears old partial hidden candidates first (DESIGN.md:87)
                    cur.scopes = cur.scopes.map {
                        $0.scope == "codex_hidden"
                            ? UnifiedCleanupScopePlan(scope: "codex_hidden", supported: true, scanned: false, unavailableReason: nil, candidateCount: 0, deletableCount: 0, estimatedBytes: 0, candidates: [])
                            : $0
                    }
                    selectedScopes.remove("codex_hidden")
                    plan = cur
                }
                if store.capabilities.hiddenCleanupScan {
                    try await runPaginatedHiddenScan(capturedRevision: capturedRevision)
                }
            } catch {
                if !Task.isCancelled, store.revision == capturedRevision {
                    errorMessage = error.localizedDescription
                }
            }
        }
    }

    private func runPaginatedHiddenScan(capturedRevision: UInt64) async throws {
        var mode = "start"
        var scanId: String?
        var cursor: String?
        var accumulated: [UnifiedCleanupCandidate] = []

        while !Task.isCancelled && store.revision == capturedRevision {
            var reqObj: [String: JSONValue] = [
                "mode": .string(mode),
                "limit": .number(100),
            ]
            if let scanId { reqObj["scanId"] = .string(scanId) }
            if let cursor { reqObj["cursor"] = .string(cursor) }

            let pagePlan: HiddenThreadDeletePlan = try await store.rpc(
                "cleanup.hiddenDryRun",
                args: ["request": .object(reqObj)]
            )
            guard !Task.isCancelled, store.revision == capturedRevision else { return }
            accumulated = pagePlan.candidates.count >= accumulated.count
                ? pagePlan.candidates
                : (accumulated + pagePlan.candidates.filter { item in !accumulated.contains(where: { $0.targetId == item.targetId }) })
            hiddenScan = pagePlan.scan

            let isComplete = (pagePlan.scan?.status == "complete" || pagePlan.scan?.status == "completed")
                && pagePlan.scan?.nextCursor == nil
            plan = Self.mergeHiddenPageIntoPlan(
                existing: plan,
                pagePlan: pagePlan,
                accumulated: accumulated,
                completed: isComplete
            )
            if expandedScope == nil && !accumulated.isEmpty {
                expandedScope = "codex_hidden"
            }

            if !isComplete, let prog = pagePlan.scan, let next = prog.nextCursor {
                selectedScopes.remove("codex_hidden")
                mode = "poll"
                scanId = prog.scanId
                cursor = next
            } else {
                if isComplete && pagePlan.deletableCount > 0 {
                    selectedScopes.insert("codex_hidden")
                } else {
                    selectedScopes.remove("codex_hidden")
                }
                break
            }
        }
    }

    private func cancelHiddenScan() {
        let wasRunning = (hiddenScanTask != nil || scanning)
        let scanId = hiddenScan?.scanId
        hiddenScanTask?.cancel()
        hiddenScanTask = nil
        scanning = false
        if wasRunning {
            hiddenScanStopped = true
            armed = false
            // Keep already-fetched partial hiddenItems visible in `plan`, but disable cleanup on incomplete scan!
            selectedScopes.remove("codex_hidden")
            if var cur = plan {
                cur.scopes = cur.scopes.map { sp in
                    guard sp.scope == "codex_hidden" else { return sp }
                    var copy = sp
                    copy.scanned = false
                    return copy
                }
                cur.totalDeletable = cur.scopes.filter(\.scanned).reduce(0) { $0 + $1.deletableCount }
                cur.totalEstimatedBytes = cur.scopes.filter(\.scanned).reduce(Int64(0)) { $0 + $1.estimatedBytes }
                plan = cur
            }
        }
        if let scanId {
            Task {
                let _: HiddenThreadDeletePlan? = try? await store.rpc(
                    "cleanup.hiddenDryRun",
                    args: ["request": .object(["mode": .string("cancel"), "scanId": .string(scanId)])]
                )
            }
        }
    }

    private func executeCleanup() async {
        guard let currentPlan = plan else { return }
        armed = false
        executing = true
        errorMessage = nil
        defer { executing = false }
        let scopesToRun = activeSelectedScopes
        var targets: [JSONValue] = []
        for sp in currentPlan.scopes where scopesToRun.contains(sp.scope) {
            for c in sp.candidates where c.deletable {
                var obj: [String: JSONValue] = [
                    "scope": .string(sp.scope),
                    "targetId": .string(c.targetId),
                ]
                if let fp = c.fingerprint { obj["fingerprint"] = .string(fp) }
                targets.append(.object(obj))
            }
        }
        do {
            let res: UnifiedCleanupResult = try await store.rpc(
                "cleanup.unifiedExecute",
                args: [
                    "request": .object([
                        "confirmed": .bool(true),
                        "scopes": .array(scopesToRun.map { .string($0) }),
                        "targets": .array(targets),
                    ]),
                ]
            )
            result = res
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}

// MARK: - Codex Account Scope Section (with 20s iterative progress loop)

public struct CodexAccountScopeSection: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    @State private var scopedThreads: Int?
    @State private var result: CodexAccountScopeResult?
    @State private var autoEnabled: Bool = false
    @State private var hookInstalled: Bool = false
    @State private var checking: Bool = false
    @State private var clearing: Bool = false
    @State private var armed: Bool = false
    @State private var page: Int = 0
    @State private var errorMessage: String?

    public init(store: RuntimeConnectionStore) {
        self.store = store
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
                    if !hookInstalled {
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
                    .disabled(!hookInstalled || busy)
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
            armed = false
            page = 0
            errorMessage = nil
            await loadHookAndAutoState()
        }
    }

    private func loadHookAndAutoState() async {
        if let stEnv: OptionalResultEnvelope<ProbeStatus> = try? await store.rpc("probe.status") {
            hookInstalled = (stEnv.data?.hookStatus == "managed")
        }
        if let setEnv: OptionalResultEnvelope<ProbeSettingsView> = try? await store.rpc("probe.settings.get"),
           let loaded = setEnv.data
        {
            let d = ProbeSettingsDraft.from(loaded)
            autoEnabled = d.clearAccountScope
        }
    }

    private func setAutoClear(_ next: Bool) async {
        errorMessage = nil
        do {
            let setEnv: OptionalResultEnvelope<ProbeSettingsView> = try await store.rpc("probe.settings.get")
            guard let loaded = setEnv.data else { return }
            var d = ProbeSettingsDraft.from(loaded)
            d.clearAccountScope = next
            let payload = try d.buildSavePayload()
            let _: OptionalResultEnvelope<ProbeSettingsView> = try await store.rpc(
                "probe.settings.save",
                args: ["settings": .object(payload)]
            )
            autoEnabled = next
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
        var totalCleared = 0
        var latestItems: [CodexAccountScopeItem] = []
        var latestRemaining = 0

        do {
            while !Task.isCancelled {
                let batch: CodexAccountScopeResult = try await store.rpc("threads.accountScopeClear")
                totalCleared += batch.clearedThreads
                latestRemaining = batch.remainingFiles
                latestItems = batch.items
                result = CodexAccountScopeResult(
                    clearedThreads: totalCleared,
                    remainingFiles: latestRemaining,
                    items: latestItems
                )
                if batch.remainingFiles <= 0 || batch.clearedThreads <= 0 {
                    break
                }
            }
            let status: CodexAccountScopeStatus = try await store.rpc("threads.accountScopeStatus")
            scopedThreads = status.scopedThreads
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
