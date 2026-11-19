import SwiftUI

public struct SettingsWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    public init(store: RuntimeConnectionStore) {
        self.store = store
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                // Settings Header & Tabs
                HStack(spacing: 16) {
                    Text("设置")
                        .font(DesignFonts.body(18, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)

                    HStack(spacing: 4) {
                        tabButton(title: "更新与维护", section: .maintenance)
                        tabButton(title: "远程连接", section: .remote)
                    }
                    Spacer()
                }
                .padding(.bottom, 4)

                if store.settingsSection == .remote {
                    RemoteConnectionSettingsView(store: store)
                } else {
                    MaintenanceOpsView(store: store)
                }
            }
            .padding(24)
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
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(active ? palette.selected.color : Color.clear)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
    }
}

public struct MaintenanceOpsView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    @State private var updateStatus: UpdateStatus?
    @State private var updateLoading: Bool = false
    @State private var updateError: String?
    @State private var lastUpdateAction: UnifiedUpdateActionKind?
    @State private var activeUpdateJobId: String?

    public init(store: RuntimeConnectionStore) {
        self.store = store
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
            // Ops Header
            HStack {
                HStack(spacing: 8) {
                    LucideIcon(.slidersHorizontal, size: 18)
                        .foregroundStyle(palette.textMuted.color)
                    Text("设置与维护")
                        .font(DesignFonts.body(16, weight: .semibold))
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
            }

            // Appearance Segmented Radio Group
            VStack(alignment: .leading, spacing: 8) {
                Text("外观")
                    .font(DesignFonts.body(13, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                HStack(spacing: 6) {
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
                            HStack(spacing: 6) {
                                LucideIcon(icon, size: 15)
                                Text(option.label)
                                    .font(DesignFonts.body(13, weight: .medium))
                            }
                            .foregroundStyle((active ? palette.textPrimary : palette.textMuted).color)
                            .padding(.horizontal, 14)
                            .frame(height: LayoutMetrics.formControlHeight)
                            .background(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .fill((active ? palette.elevated : palette.surface).color)
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .strokeBorder((active ? palette.accentBorder : palette.borderSubtle).color, lineWidth: 1)
                            )
                        }
                        .buttonStyle(PlainHitButtonStyle())
                    }
                }
            }

            // Update Panel
            updatePanelSection

            // Unified 6-Scope Cleanup + Hidden Scan
            if store.capabilities.threadCleanup {
                UnifiedThreadCleanupSection(store: store)
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
        let title = store.capabilities.updateServiceLabels ? "服务更新" : "更新与维护"
        let working = updateBusy || updateStatus?.state == "checking" || updateStatus?.state == "installing"
        let failure = updateError ?? (updateJob?.status == "failed" ? (updateJob?.error ?? "更新操作失败") : nil)

        PanelCard(title: title, icon: .refreshCw) {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 10) {
                    MetricCard(
                        label: "当前版本",
                        value: updateStatus?.currentVersion ?? (updateLoading ? "读取中…" : LayoutMetrics.fallbackAppVersion)
                    )
                    if updateStatus?.updateAvailable == true, let latest = updateStatus?.latestVersion {
                        MetricCard(label: "可用版本", value: latest, tone: .info)
                    }
                    Spacer()
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
                                HStack(spacing: 6) {
                                    LucideIcon(icon, size: 16)
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

    private static let scopesMeta: [(scope: String, label: String, brand: AgentBrandId)] = [
        ("codex_archived", "Codex 归档线程", .codex),
        ("codex_hidden", "Codex 隐藏线程", .codex),
        ("claude_inactive", "Claude 历史清理", .claude),
        ("grok_inactive", "Grok 历史清理", .grok),
        ("antigravity_archived", "Antigravity 归档", .antigravity),
        ("dsh_archived", "DeepSeek Harness 归档", .dsh),
    ]

    @State private var plan: UnifiedCleanupPlan?
    @State private var hiddenScan: HiddenScanProgress?
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

    public init(store: RuntimeConnectionStore) {
        self.store = store
    }

    private var busy: Bool { scanning || executing }

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
                let stageText: String = executing ? "正在清理" : scanning ? "正在扫描" : armed ? "等待确认" : (plan != nil ? "已预览" : "待扫描")
                let tone: StatusChipTone = executing || scanning ? .warning : armed ? .danger : (plan != nil ? .info : .neutral)
                StatusChip(stageText, tone: tone)
            }
        ) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
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

                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                    ForEach(Self.scopesMeta, id: \.scope) { meta in
                        let sp = plan?.scopes.first(where: { $0.scope == meta.scope })
                        let canSelect = (sp?.supported == true && sp?.scanned == true && (sp?.deletableCount ?? 0) > 0)
                        let isSelected = activeSelectedScopes.contains(meta.scope)
                        HStack(spacing: 10) {
                            AgentBrandIcon(meta.brand, size: 16)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(meta.label)
                                    .font(DesignFonts.body(13, weight: .semibold))
                                    .foregroundStyle(palette.textPrimary.color)
                                Text(scopeSummary(meta.scope, scopePlan: sp))
                                    .font(DesignFonts.body(11))
                                    .foregroundStyle(palette.textMuted.color)
                                    .lineLimit(1)
                            }
                            Spacer()
                            if let sp, sp.candidateCount > 0 {
                                Button(expandedScope == meta.scope ? "收起" : "详情") {
                                    candidatePage = 0
                                    expandedScope = (expandedScope == meta.scope) ? nil : meta.scope
                                }
                                .buttonStyle(SecondaryButtonStyle())
                            }
                            CapsuleSwitch(
                                isOn: Binding(
                                    get: { isSelected },
                                    set: { val in
                                        armed = false
                                        if val { selectedScopes.insert(meta.scope) } else { selectedScopes.remove(meta.scope) }
                                    }
                                ),
                                label: meta.label
                            )
                            .disabled(!canSelect || busy)
                        }
                        .padding(10)
                        .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
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

                HStack(spacing: 8) {
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
                        .disabled(busy || selectedDeletableCount == 0)
                    } else {
                        Button("确认清理所选") {
                            Task { await executeCleanup() }
                        }
                        .buttonStyle(DangerButtonStyle(soft: false))
                        .disabled(busy)

                        Button("取消") {
                            armed = false
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(busy)
                    }
                }
            }
        }
        .onDisappear {
            cancelHiddenScan()
        }
    }

    private func scopeSummary(_ scope: String, scopePlan: UnifiedCleanupScopePlan?) -> String {
        guard let sp = scopePlan else {
            return plan == nil ? "点击下方全平台扫描预览" : "未扫描"
        }
        if !sp.supported { return sp.unavailableReason ?? "当前环境不支持" }
        if !sp.scanned { return scope == "codex_hidden" && scanning ? "正在深度扫描隐藏线程…" : "未深度扫描" }
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
        return HStack(spacing: 8) {
            Button("上一页") { page.wrappedValue = max(0, page.wrappedValue - 1) }
                .buttonStyle(SecondaryButtonStyle())
                .disabled(page.wrappedValue == 0)
            Text("\(page.wrappedValue + 1) / \(pages)")
                .font(DesignFonts.body(12))
                .foregroundStyle(palette.textMuted.color)
            Button("下一页") { page.wrappedValue = min(pages - 1, page.wrappedValue + 1) }
                .buttonStyle(SecondaryButtonStyle())
                .disabled(page.wrappedValue + 1 >= pages)
        }
    }

    private func runPreview(includeUnified: Bool) {
        armed = false
        result = nil
        errorMessage = nil
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
                    plan = unified
                    selectedScopes = Set(unified.scopes.filter { $0.supported && $0.scanned && $0.deletableCount > 0 }.map(\.scope))
                }
                if store.capabilities.hiddenCleanupScan {
                    try await runPaginatedHiddenScan()
                }
            } catch {
                if !Task.isCancelled {
                    errorMessage = error.localizedDescription
                }
            }
        }
    }

    private func runPaginatedHiddenScan() async throws {
        var mode = "start"
        var scanId: String?
        var cursor: String?
        var accumulated: [UnifiedCleanupCandidate] = []

        while !Task.isCancelled {
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
            accumulated.append(contentsOf: pagePlan.candidates)
            hiddenScan = pagePlan.scan

            if let prog = pagePlan.scan, prog.status != "completed", let next = prog.nextCursor {
                mode = "poll"
                scanId = prog.scanId
                cursor = next
            } else {
                let hiddenScope = UnifiedCleanupScopePlan(
                    scope: "codex_hidden",
                    supported: true,
                    scanned: true,
                    unavailableReason: nil,
                    candidateCount: pagePlan.candidateCount,
                    deletableCount: pagePlan.deletableCount,
                    estimatedBytes: pagePlan.estimatedBytes,
                    candidates: accumulated
                )
                if var existing = plan {
                    existing.scopes = existing.scopes.map { $0.scope == "codex_hidden" ? hiddenScope : $0 }
                    existing.totalCandidates = existing.scopes.reduce(0) { $0 + $1.candidateCount }
                    existing.totalDeletable = existing.scopes.reduce(0) { $0 + $1.deletableCount }
                    existing.totalEstimatedBytes = existing.scopes.reduce(0) { $0 + $1.estimatedBytes }
                    plan = existing
                } else {
                    plan = UnifiedCleanupPlan(
                        totalCandidates: hiddenScope.candidateCount,
                        totalDeletable: hiddenScope.deletableCount,
                        totalEstimatedBytes: hiddenScope.estimatedBytes,
                        scopes: [hiddenScope]
                    )
                }
                if hiddenScope.deletableCount > 0 {
                    selectedScopes.insert("codex_hidden")
                }
                break
            }
        }
    }

    private func cancelHiddenScan() {
        let scanId = hiddenScan?.scanId
        hiddenScanTask?.cancel()
        hiddenScanTask = nil
        scanning = false
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
                    }
                }

                if let errorMessage, !errorMessage.isEmpty {
                    Text(errorMessage)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                }

                HStack(spacing: 8) {
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
