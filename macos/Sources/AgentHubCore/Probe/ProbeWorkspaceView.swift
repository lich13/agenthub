import Foundation
import SwiftUI

private struct PendingHookDialog: Equatable, Sendable {
    let provider: String
    let name: String
    let action: String
    let path: String
    let revision: UInt64
}

private enum HostIntegrationEntryKind: Equatable {
    case codex(hookStatus: String?, actualCount: Int, staleCount: Int, path: String, unavailable: Bool)
    case localProvider(provider: String, installed: Bool, count: Int, path: String, error: String?)
    case remoteReadOnly
    case dshDirect(path: String)
}

private struct HostIntegrationEntry: Identifiable, Equatable {
    let id: String
    let brand: AgentBrandId
    let name: String
    let isReady: Bool
    let hasError: Bool
    let needsRepair: Bool
    let kind: HostIntegrationEntryKind
}

struct ProbeWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject var store: RuntimeConnectionStore

    @State private var disclosureRules = ProbeDisclosureRules()
    @State private var status: ProbeStatus?
    @State private var statusError: String?
    @State private var settingsSync = ProbeSettingsSyncState()
    @State private var events: [ProbeEventItem] = []
    @State private var eventsLoaded: Bool = false
    @State private var eventsError: String?
    @State private var loadingStatus: Bool = false
    @State private var saving: Bool = false
    @State private var saveTarget: ProbeSettingsSaveScope = .settings
    @State private var saveMessage: (tone: StatusChipTone, text: String)?
    @State private var actionMessage: (tone: StatusChipTone, text: String)?

    @State private var activeJobId: String?
    @State private var startingJob = false
    @State private var activeJobAction: String?

    @State private var hookStatuses: [String: ClaudeHooksStatus] = [:]
    @State private var hookErrors: [String: String] = [:]
    @State private var pendingHookDialog: PendingHookDialog?
    @State private var hookBusy: Bool = false
    @State private var machineEpoch: UInt64 = 0
    @State private var refreshToken: UInt64 = 0
    @State private var statusEventsSeq: UInt64 = 0

    init(
        store: RuntimeConnectionStore,
        initialOpenSections: Set<ProbeDisclosureSection>? = nil
    ) {
        self.store = store
        let rules = if let initialOpenSections {
            ProbeDisclosureRules(machineScope: store.machineScope, openSections: initialOpenSections)
        } else {
            ProbeDisclosureRules(machineScope: store.machineScope)
        }
        self._disclosureRules = State(initialValue: rules)
        self._settingsSync = State(initialValue: ProbeSettingsSyncState(revision: store.revision))
    }

    private var draft: ProbeSettingsDraft? {
        settingsSync.draft
    }

    private var settingsError: String? {
        settingsSync.settingsError
    }

    private var ciDraftBinding: Binding<ProbeSettingsDraft?> {
        Binding(
            get: { settingsSync.draft },
            set: { next in
                if let old = settingsSync.draft, let next, !old.subsetEquals(next, scope: .ci),
                   saveTarget == .ci, saveMessage?.tone == .danger
                {
                    saveMessage = nil
                }
                settingsSync.draft = next
            }
        )
    }

    private func sectionBinding(_ section: ProbeDisclosureSection) -> Binding<Bool> {
        Binding(
            get: { disclosureRules.isExpanded(section) },
            set: { next in
                disclosureRules.resetIfMachineChanged(store.machineScope)
                disclosureRules.setExpanded(section, next)
            }
        )
    }

    private var jobBusy: Bool {
        if startingJob { return true }
        guard let id = activeJobId else { return false }
        return store.isTrackedJobActive(id)
    }

    private var settingsValidationErrors: [String] {
        draft?.validationErrors(for: .settings) ?? []
    }

    private var configuredDeviceKey: Bool {
        guard let draft else { return false }
        return draft.barkDeviceKeyConfigured || !draft.barkDeviceKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var visibleHostEntries: [HostIntegrationEntry] {
        var items: [HostIntegrationEntry] = []

        let codexHookStatus = status?.hookStatus
        let codexManaged = (codexHookStatus == "managed")
        let codexUnavailable = (codexHookStatus == nil && statusError != nil)
        let codexStale = (codexHookStatus == "stale")
        items.append(
            HostIntegrationEntry(
                id: "codex",
                brand: .codex,
                name: "Codex",
                isReady: codexManaged,
                hasError: codexUnavailable,
                needsRepair: codexStale,
                kind: .codex(
                    hookStatus: codexHookStatus,
                    actualCount: status?.actualCommands?.count ?? 0,
                    staleCount: status?.staleCommandCount ?? 0,
                    path: status?.codexHooksPath ?? "未返回路径",
                    unavailable: codexUnavailable
                )
            )
        )

        var localCapableProviders: [(id: String, brand: AgentBrandId, name: String)] = [
            ("claude", .claude, "Claude Code"),
            ("grok", .grok, "Grok Build"),
        ]
        if store.capabilities.antigravityNotifications {
            localCapableProviders.append(("antigravity", .antigravity, "Antigravity"))
        }
        for p in localCapableProviders {
            if store.target == .remote {
                items.append(
                    HostIntegrationEntry(
                        id: p.id,
                        brand: p.brand,
                        name: p.name,
                        isReady: statusError == nil,
                        hasError: false,
                        needsRepair: false,
                        kind: .remoteReadOnly
                    )
                )
            } else {
                let st = hookStatuses[p.id]
                let err = hookErrors[p.id]
                let installed = st?.installed ?? false
                let count = st?.managedCount ?? 0
                let path = st?.configPath ?? (err == nil ? "未返回路径" : "状态不可用")
                items.append(
                    HostIntegrationEntry(
                        id: p.id,
                        brand: p.brand,
                        name: p.name,
                        isReady: installed && err == nil,
                        hasError: err != nil,
                        needsRepair: false,
                        kind: .localProvider(provider: p.id, installed: installed, count: count, path: path, error: err)
                    )
                )
            }
        }

        if store.capabilities.dshNotifications {
            items.append(
                HostIntegrationEntry(
                    id: "dsh",
                    brand: .dsh,
                    name: "DeepSeek Harness",
                    isReady: statusError == nil,
                    hasError: false,
                    needsRepair: false,
                    kind: .dshDirect(path: status?.dshSessionsPath ?? "原生日志直连")
                )
            )
        }
        return items
    }

    var body: some View {
        GeometryReader { geo in
            let isNarrow = geo.size.width <= LayoutMetrics.narrowBreakpoint
            ZStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        headerBar

                        if let statusError, !statusError.isEmpty {
                            Text(statusError)
                                .font(DesignFonts.body(12, weight: .medium))
                                .foregroundStyle(palette.errFg.color)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 10)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(
                                    RoundedRectangle(cornerRadius: 8)
                                        .fill(palette.errBg.color)
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 8)
                                                .strokeBorder(palette.errBorder.color, lineWidth: 1)
                                        )
                                )
                        }

                        if store.capabilities.isReady && store.capabilities.githubCi {
                            GithubCiCardView(
                                store: store,
                                draft: ciDraftBinding,
                                disclosureRules: $disclosureRules,
                                isNarrow: isNarrow,
                                saving: saving,
                                savingCurrentScope: saving && saveTarget == .ci,
                                saveMessage: saveTarget == .ci ? saveMessage : nil,
                                onSave: { Task { await saveSettings(scope: .ci) } }
                            )
                        }

                        notificationSettingsCard(isNarrow: isNarrow)

                        barkPanelBody(draft: draft, isNarrow: isNarrow)

                        hostMatrixSection(isNarrow: isNarrow)

                        liveOverviewPanel

                        recentEventsPanel

                        JobHistorySectionView(
                            store: store,
                            title: "执行记录",
                            isExpanded: sectionBinding(.jobs)
                        )
                    }
                    .frame(maxWidth: LayoutMetrics.consolePageMaxWidth, alignment: .leading)
                    .padding(.vertical, isNarrow ? 16 : 20)
                    .padding(.horizontal, isNarrow ? 14 : 32)
                    .frame(maxWidth: .infinity)
                }

                if let dialog = pendingHookDialog {
                    ConfirmDialogOverlay(
                        title: dialog.action == "install" ? "写入 \(dialog.name) 通知 Hook" : "移除 \(dialog.name) 通知 Hook",
                        busy: hookBusy || jobBusy,
                        onCancel: { pendingHookDialog = nil }
                    ) {
                        if !dialog.path.isEmpty {
                            Text(dialog.path)
                                .font(DesignFonts.mono(12))
                                .foregroundStyle(palette.textSecondary.color)
                                .padding(8)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(RoundedRectangle(cornerRadius: 8).fill(palette.codeBg.color))
                        }
                        FlowButtonRow(spacing: 8, lineSpacing: 8) {
                            Button("取消") {
                                pendingHookDialog = nil
                            }
                            .buttonStyle(SecondaryButtonStyle())
                            .disabled(hookBusy || jobBusy)

                            if dialog.action == "remove" {
                                Button("确认移除") {
                                    let captured = dialog
                                    pendingHookDialog = nil
                                    Task { await executeHookDialog(captured) }
                                }
                                .buttonStyle(DangerButtonStyle(soft: false))
                                .disabled(hookBusy || jobBusy)
                            } else {
                                Button("确认写入") {
                                    let captured = dialog
                                    pendingHookDialog = nil
                                    Task { await executeHookDialog(captured) }
                                }
                                .buttonStyle(PrimaryButtonStyle())
                                .disabled(hookBusy || jobBusy)
                            }
                        }
                    }
                }
            }
            .background(palette.background.color)
        }
        .onChange(of: activeJobId.flatMap { store.trackedJobs[$0]?.status }) { jobStatus in
            if jobStatus == "failed", let id = activeJobId {
                disclosureRules.observeIssue(
                    machineScope: store.machineScope,
                    section: .jobs,
                    issueSignature: "job:\(id):failed"
                )
                if activeJobAction == "probe.barkTest" {
                    disclosureRules.observeIssue(
                        machineScope: store.machineScope,
                        section: .bark,
                        issueSignature: "barkTest:\(id):failed"
                    )
                }
            }
            guard jobStatus == "succeeded" || jobStatus == "failed",
                  activeJobAction == "probe.installHooks" || activeJobAction == "probe.removeHooks"
            else { return }
            Task { await refreshAll(force: true) }
        }
        .task(id: store.machineScope) {
            machineEpoch &+= 1
            refreshToken &+= 1
            statusEventsSeq &+= 1
            loadingStatus = false
            saving = false
            startingJob = false
            hookBusy = false
            disclosureRules.resetIfMachineChanged(store.machineScope)
            settingsSync.reset(revision: store.revision)
            status = nil
            statusError = nil
            events = []
            eventsLoaded = false
            eventsError = nil
            saveMessage = nil
            actionMessage = nil
            activeJobId = nil
            activeJobAction = nil
            pendingHookDialog = nil
            hookStatuses = [:]
            hookErrors = [:]
            await refreshAll(force: false)
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 15_000_000_000)
                guard !Task.isCancelled else { break }
                if store.isWindowVisible {
                    await refreshStatusAndEvents()
                }
            }
        }
    }
    @ViewBuilder
    private var headerBar: some View {
        HStack(spacing: 8) {
            Text("Probe")
                .font(DesignFonts.body(22, weight: .semibold))
                .foregroundStyle(palette.textPrimary.color)
            if let incidentCount = status?.errorMonitorIncidentCount, incidentCount > 0 {
                StatusChip("错误 \(incidentCount)", tone: .danger)
            }
            if let warnings = status?.discoveryWarnings, !warnings.isEmpty {
                StatusChip("目录告警 \(warnings.count)", tone: .warning)
                    .help(warnings.joined(separator: "；"))
            }
            Spacer()
            Button {
                Task { await refreshAll(force: true) }
            } label: {
                LucideIcon(.refreshCw, size: 17)
            }
            .buttonStyle(IconButtonStyle(dimension: 32))
            .help("刷新 Probe")
        }
        .frame(minHeight: 32)
    }

    @ViewBuilder
    private func notificationSettingsCard(isNarrow: Bool) -> some View {
        PanelCard(
            title: "通知配置",
            icon: .slidersHorizontal,
            headerAction: {
                HStack(spacing: 8) {
                    if saveTarget == .settings, let saveMessage {
                        Text(saveMessage.text)
                            .font(DesignFonts.body(12))
                            .foregroundStyle((saveMessage.tone == .success ? palette.okFg : palette.errFg).color)
                            .lineLimit(1)
                    }
                    Button {
                        Task { await saveSettings(scope: .settings) }
                    } label: {
                        HStack(spacing: 5) {
                            LucideIcon(.checkCircle2, size: 15)
                            Text(saving && saveTarget == .settings ? "保存中" : "保存设置")
                        }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(saving || draft == nil)
                }
            }
        ) {
            VStack(alignment: .leading, spacing: 8) {
                if let actionMessage {
                    Text(actionMessage.text)
                        .font(DesignFonts.body(12))
                        .foregroundStyle((actionMessage.tone == .success ? palette.okFg : palette.errFg).color)
                }
                if let settingsError, !settingsError.isEmpty {
                    Text(settingsError)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                }
                if let d = draft {
                    runtimeSettingsBody(draft: d, isNarrow: isNarrow)
                } else if settingsError == nil {
                    Text(loadingStatus ? "正在读取设置" : "设置不可用")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textMuted.color)
                }
            }
        }
    }

    @ViewBuilder
    private func runtimeSettingsBody(draft d: ProbeSettingsDraft, isNarrow: Bool) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            LazyVGrid(
                columns: isNarrow ? [GridItem(.flexible())] : [GridItem(.flexible()), GridItem(.flexible())],
                spacing: 8
            ) {
                compactSwitchTile(
                    label: "启用 Probe",
                    isOn: Binding(
                        get: { d.probeEnabled },
                        set: { next in updateDraft { $0.probeEnabled = next } }
                    )
                )
                compactSwitchTile(
                    label: "终止错误监控",
                    isOn: Binding(
                        get: { d.errorMonitorEnabled },
                        set: { next in updateDraft { $0.errorMonitorEnabled = next } }
                    )
                )
            }

            let visiblePlatforms = SessionProvider.allCases.filter { p in
                switch p {
                case .antigravity: store.capabilities.antigravityNotifications
                case .grokbot: store.capabilities.grokbotNotifications
                case .dsh: store.capabilities.dshNotifications
                default: true
                }
            }
            let platformCols = isNarrow
                ? [GridItem(.flexible(), alignment: .top)]
                : [
                    GridItem(.flexible(), alignment: .top),
                    GridItem(.flexible(), alignment: .top),
                    GridItem(.flexible(), alignment: .top),
                ]
            LazyVGrid(columns: platformCols, spacing: 6) {
                ForEach(visiblePlatforms) { platform in
                    let streams = status?.providerNotifications?.first(where: { $0.provider == platform.rawValue })?.streams
                    let masterOn = d.isPlatformEnabled(platform)
                    let categories = ProbeSettingsDraft.supportedCategories(for: platform)
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 7) {
                            AgentBrandIcon(platform.brand, size: 15)
                                .foregroundStyle(palette.textPrimary.color)
                            Text(platform.label)
                                .font(DesignFonts.body(13, weight: .medium))
                                .foregroundStyle(palette.textPrimary.color)
                                .lineLimit(1)
                            if let streams, streams > 0 {
                                Text("\(streams)")
                                    .font(DesignFonts.body(11))
                                    .foregroundStyle(palette.textMuted.color)
                                    .lineLimit(1)
                            }
                            Spacer(minLength: 4)
                            CapsuleSwitch(
                                isOn: Binding(
                                    get: { masterOn },
                                    set: { val in updateDraft { $0.setPlatformEnabled(platform, enabled: val) } }
                                ),
                                label: "\(platform.label) 通知"
                            )
                        }
                        .frame(minHeight: 24)

                        if masterOn && !categories.isEmpty {
                            FlowButtonRow(spacing: 5, lineSpacing: 5) {
                                ForEach(categories, id: \.self) { category in
                                    let catOn = d.isCategoryEnabled(platform, category)
                                    Button {
                                        updateDraft { $0.setCategoryEnabled(platform, category, enabled: !catOn) }
                                    } label: {
                                        HStack(spacing: 3) {
                                            LucideIcon(catOn ? .check : .plus, size: 10)
                                                .foregroundStyle((catOn ? palette.accent : palette.textMuted).color)
                                            Text(category.label)
                                                .font(DesignFonts.body(11, weight: .medium))
                                                .foregroundStyle((catOn ? palette.accent : palette.textMuted).color)
                                                .lineLimit(1)
                                        }
                                        .padding(.horizontal, 7)
                                        .frame(height: 22)
                                        .background(Capsule().fill((catOn ? palette.accentSoft : palette.surface).color))
                                        .overlay(
                                            Capsule().strokeBorder(
                                                (catOn ? palette.accentBorder : palette.borderStrong).color,
                                                lineWidth: 1
                                            )
                                        )
                                        .contentShape(Rectangle())
                                    }
                                    .buttonStyle(PlainHitButtonStyle())
                                    .accessibilityLabel("\(platform.label) \(category.label)通知")
                                    .accessibilityValue(catOn ? "已开启" : "已关闭")
                                }
                            }
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .frame(maxWidth: .infinity, minHeight: 34, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(palette.elevated.color))
                    .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(palette.borderStrong.color, lineWidth: 1))
                }
            }

            let advOpen = disclosureRules.isExpanded(.advanced)
            VStack(alignment: .leading, spacing: 8) {
                Button {
                    sectionBinding(.advanced).wrappedValue.toggle()
                } label: {
                    HStack(spacing: 6) {
                        LucideIcon(advOpen ? .chevronDown : .chevronRight, size: 15)
                            .foregroundStyle(palette.textMuted.color)
                        Text("高级设置")
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textSecondary.color)
                        Spacer()
                        let homeSummary = status?.codexHome ?? (d.codexHome.isEmpty ? "auto" : d.codexHome)
                        Text("\(d.hostLabel) · \(homeSummary)")
                            .font(DesignFonts.mono(11))
                            .foregroundStyle(palette.textMuted.color)
                            .lineLimit(1)
                    }
                    .padding(.vertical, 2)
                    .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())

                if advOpen {
                    let advCols = isNarrow
                        ? [GridItem(.flexible())]
                        : [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())]
                    LazyVGrid(columns: advCols, spacing: 10) {
                        labeledField("Codex Home", placeholder: "auto", value: d.codexHome) { v in
                            updateDraft { $0.codexHome = v }
                        }
                        labeledField("主机标签", placeholder: "AgentHub", value: d.hostLabel) { v in
                            updateDraft { $0.hostLabel = v }
                        }
                        labeledField("轮询秒数", placeholder: "5..3600", value: d.pollSecondsText) { v in
                            updateDraft { $0.pollSecondsText = v }
                        }
                        labeledField("最近事件数", placeholder: "1..500", value: d.recentLimitText) { v in
                            updateDraft { $0.recentLimitText = v }
                        }
                        labeledField("通知事件保留天数", placeholder: "1..3650", value: d.eventRetentionDaysText) { v in
                            updateDraft { $0.eventRetentionDaysText = v }
                        }
                        labeledField("Hook 事件行数", placeholder: "10..10000", value: d.hookEventMaxLinesText) { v in
                            updateDraft { $0.hookEventMaxLinesText = v }
                        }
                        labeledField("冷却行数", placeholder: "10..10000", value: d.hookCooldownMaxLinesText) { v in
                            updateDraft { $0.hookCooldownMaxLinesText = v }
                        }
                        labeledField("日志上限 MiB", placeholder: "0.00390625..8", value: d.logMaxMiBText) { v in
                            updateDraft { $0.logMaxMiBText = v }
                        }
                    }

                    if let firstError = settingsValidationErrors.first {
                        Text(firstError)
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.errFg.color)
                    }
                }
            }
            .padding(.top, 6)
            .overlay(alignment: .top) {
                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
            }
        }
    }

    @ViewBuilder
    private func barkPanelBody(draft: ProbeSettingsDraft?, isNarrow: Bool) -> some View {
        let trimmedUrl = (draft?.barkServerUrl ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let hostText: String? = {
            guard !trimmedUrl.isEmpty else { return nil }
            return URL(string: trimmedUrl)?.host ?? trimmedUrl
        }()

        CollapsiblePanelCard(
            title: "Bark",
            icon: .cloud,
            isExpanded: sectionBinding(.bark),
            summaryBadges: {
                if let d = draft {
                    StatusChip(d.barkEnabled ? "已启用" : "已停用", tone: d.barkEnabled ? .success : .neutral)
                    StatusChip(configuredDeviceKey ? "密钥已配" : "未配密钥", tone: configuredDeviceKey ? .success : .warning)
                    if let hostText, !hostText.isEmpty {
                        StatusChip(hostText, tone: .neutral)
                    }
                } else {
                    StatusChip(
                        loadingStatus ? "读取中" : "不可用",
                        tone: settingsError == nil ? .neutral : .danger
                    )
                }
            },
            headerTrailing: {
                HStack(spacing: 6) {
                    Button {
                        let rev = store.revision
                        Task { await startProbeJob("probe.barkTest", label: "AgentHub 推送测试", revision: rev) }
                    } label: {
                        HStack(spacing: 5) {
                            LucideIcon(.cloud, size: 15)
                            Text("测试推送")
                        }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(!configuredDeviceKey || jobBusy)

                    Button {
                        Task { await saveSettings(scope: .bark) }
                    } label: {
                        HStack(spacing: 5) {
                            LucideIcon(.check, size: 15)
                            Text(saving && saveTarget == .bark ? "保存中" : "保存")
                        }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(draft == nil || saving)
                }
            }
        ) {
            if let d = draft {
                let barkSwitch = compactSwitchTile(
                    label: "启用 Bark",
                    isOn: Binding(
                        get: { d.barkEnabled },
                        set: { next in updateDraft(scope: .bark) { $0.barkEnabled = next } }
                    )
                )
                let urlField = labeledField("Bark 服务 URL", placeholder: "https://api.day.app", value: d.barkServerUrl) { v in
                    updateDraft(scope: .bark) { $0.barkServerUrl = v }
                }
                let keyField = ProbeDraftField(
                    label: "Device Key",
                    placeholder: configuredDeviceKey ? "已配置，留空保持不变" : "粘贴 Bark Device Key",
                    value: d.barkDeviceKey,
                    isSecure: true
                ) { v in
                    updateDraft(scope: .bark) {
                        let wasEmpty = $0.barkDeviceKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        $0.barkDeviceKey = v
                        if wasEmpty && !v.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !$0.barkEnabled {
                            $0.barkEnabled = true
                        }
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    if isNarrow {
                        barkSwitch
                        urlField
                        keyField
                    } else {
                        HStack(alignment: .bottom, spacing: 12) {
                            barkSwitch
                                .frame(width: 152, height: LayoutMetrics.formControlHeight)
                            urlField
                                .frame(maxWidth: .infinity)
                            keyField
                                .frame(maxWidth: .infinity)
                        }
                    }

                    if activeJobAction == "probe.barkTest", let id = activeJobId {
                        let statusText: String = if let job = store.trackedJobs[id] {
                            if job.status == "succeeded" {
                                "Bark 已受理"
                            } else if job.status == "failed" {
                                "发送失败，请查看执行记录"
                            } else {
                                "正在发送，遇到临时故障将自动重试…"
                            }
                        } else {
                            "正在发送，遇到临时故障将自动重试…"
                        }
                        Text(statusText)
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.textMuted.color)
                    }

                    if saveTarget == .bark, let saveMessage {
                        Text(saveMessage.text)
                            .font(DesignFonts.body(12))
                            .foregroundStyle((saveMessage.tone == .success ? palette.okFg : palette.errFg).color)
                    }
                }
            } else {
                let fallbackMessage: String = settingsError ?? (loadingStatus ? "正在读取设置" : "设置不可用")
                Text(fallbackMessage)
                    .font(DesignFonts.body(12))
                    .foregroundStyle((settingsError == nil ? palette.textMuted : palette.errFg).color)
            }
        }
    }

    @ViewBuilder
    private func hostMatrixSection(isNarrow: Bool) -> some View {
        let entries = visibleHostEntries
        let readyCount = entries.filter(\.isReady).count
        let anyError = entries.contains(where: \.hasError)
        let anyRepair = entries.contains(where: \.needsRepair)
        let summaryTone: StatusChipTone = if anyError {
            .danger
        } else if readyCount == entries.count {
            .success
        } else {
            .warning
        }

        CollapsiblePanelCard(
            title: "Hooks 与宿主接入",
            icon: .gitFork,
            isExpanded: sectionBinding(.hosts),
            summaryBadges: {
                StatusChip("已就绪 \(readyCount)/\(entries.count)", tone: summaryTone)
                if anyError {
                    StatusChip("读取失败", tone: .danger)
                } else if anyRepair {
                    StatusChip("需修复", tone: .warning)
                }
            }
        ) {
            VStack(spacing: 0) {
                ForEach(Array(entries.enumerated()), id: \.element.id) { index, entry in
                    if index > 0 {
                        Rectangle()
                            .fill(palette.borderSubtle.color)
                            .frame(height: 1)
                    }
                    hostEntryRow(entry, isNarrow: isNarrow)
                }
            }
            .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(palette.elevated.color))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(palette.borderSubtle.color, lineWidth: 1)
            )
        }
    }

    @ViewBuilder
    private func hostEntryRow(_ entry: HostIntegrationEntry, isNarrow: Bool) -> some View {
        switch entry.kind {
        case let .codex(hookStatus, actualCount, staleCount, path, unavailable):
            let managed = (hookStatus == "managed")
            let canRemove = managed || actualCount > 0 || staleCount > 0
            let writeTitle = (hookStatus == "stale" || hookStatus == "missing")
                ? "修复 Hook"
                : (managed ? "重新写入 Hook" : "写入通知 Hook")
            let writeBtn = Button(writeTitle) {
                pendingHookDialog = PendingHookDialog(
                    provider: "codex",
                    name: "Codex",
                    action: "install",
                    path: status?.codexHooksPath ?? "",
                    revision: store.revision
                )
            }
            .buttonStyle(SecondaryButtonStyle())
            .disabled(jobBusy)

            let removeBtn = Button("移除 Hook") {
                pendingHookDialog = PendingHookDialog(
                    provider: "codex",
                    name: "Codex",
                    action: "remove",
                    path: status?.codexHooksPath ?? "",
                    revision: store.revision
                )
            }
            .buttonStyle(SecondaryButtonStyle())
            .disabled(jobBusy || !canRemove)

            if isNarrow {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        AgentBrandIcon(entry.brand, size: 16)
                        Text(entry.name)
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                        StatusChip(
                            unavailable ? "状态未知" : probeStateLabel(hookStatus),
                            tone: unavailable ? .danger : (managed ? .success : .warning)
                        )
                        StatusChip("\(actualCount) 条命令", tone: .neutral)
                        Spacer(minLength: 8)
                        Text(path)
                            .font(DesignFonts.mono(11))
                            .foregroundStyle(palette.textMuted.color)
                            .lineLimit(1)
                            .help(path)
                    }
                    FlowButtonRow(spacing: 8, lineSpacing: 8) {
                        writeBtn
                        removeBtn
                    }
                }
                .padding(12)
            } else {
                HStack(spacing: 8) {
                    AgentBrandIcon(entry.brand, size: 16)
                    Text(entry.name)
                        .font(DesignFonts.body(13, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                        .lineLimit(1)
                        .fixedSize(horizontal: true, vertical: false)
                    StatusChip(
                        unavailable ? "状态未知" : probeStateLabel(hookStatus),
                        tone: unavailable ? .danger : (managed ? .success : .warning)
                    )
                    StatusChip("\(actualCount) 条命令", tone: .neutral)
                    Text(path)
                        .font(DesignFonts.mono(11))
                        .foregroundStyle(palette.textMuted.color)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                        .help(path)
                    HStack(spacing: 6) {
                        writeBtn
                        removeBtn
                    }
                    .fixedSize(horizontal: true, vertical: false)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
            }

        case let .localProvider(provider, installed, count, path, hookErr):
            let writeBtn = Button(installed ? "重新写入 Hook" : "写入通知 Hook") {
                pendingHookDialog = PendingHookDialog(
                    provider: provider,
                    name: entry.name,
                    action: "install",
                    path: hookStatuses[provider]?.configPath ?? "",
                    revision: store.revision
                )
            }
            .buttonStyle(SecondaryButtonStyle())
            .disabled(hookBusy)

            let removeBtn = Button("移除 Hook") {
                pendingHookDialog = PendingHookDialog(
                    provider: provider,
                    name: entry.name,
                    action: "remove",
                    path: hookStatuses[provider]?.configPath ?? "",
                    revision: store.revision
                )
            }
            .buttonStyle(SecondaryButtonStyle())
            .disabled(hookBusy || (!installed && count == 0))

            if isNarrow || (hookErr?.isEmpty == false) {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        AgentBrandIcon(entry.brand, size: 16)
                        Text(entry.name)
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                        if hookErr != nil {
                            StatusChip("读取失败", tone: .danger)
                        } else {
                            StatusChip(installed ? "已安装" : "未安装", tone: installed ? .success : .warning)
                        }
                        StatusChip("\(count) 条托管", tone: .neutral)
                        Spacer(minLength: 8)
                        Text(path)
                            .font(DesignFonts.mono(11))
                            .foregroundStyle(palette.textMuted.color)
                            .lineLimit(1)
                            .help(path)
                    }
                    if let hookErr, !hookErr.isEmpty {
                        Text(hookErr)
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.errFg.color)
                    }
                    FlowButtonRow(spacing: 8, lineSpacing: 8) {
                        writeBtn
                        removeBtn
                    }
                }
                .padding(12)
            } else {
                HStack(spacing: 8) {
                    AgentBrandIcon(entry.brand, size: 16)
                    Text(entry.name)
                        .font(DesignFonts.body(13, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                        .lineLimit(1)
                        .fixedSize(horizontal: true, vertical: false)
                    StatusChip(installed ? "已安装" : "未安装", tone: installed ? .success : .warning)
                    StatusChip("\(count) 条托管", tone: .neutral)
                    Text(path)
                        .font(DesignFonts.mono(11))
                        .foregroundStyle(palette.textMuted.color)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                        .help(path)
                    HStack(spacing: 6) {
                        writeBtn
                        removeBtn
                    }
                    .fixedSize(horizontal: true, vertical: false)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
            }

        case .remoteReadOnly:
            HStack(spacing: 8) {
                AgentBrandIcon(entry.brand, size: 16)
                Text(entry.name)
                    .font(DesignFonts.body(13, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                StatusChip("仅限本机", tone: .neutral)
                StatusChip("远端只读轮询", tone: .info)
                Spacer(minLength: 8)
                Text("远端只读")
                    .font(DesignFonts.mono(11))
                    .foregroundStyle(palette.textMuted.color)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 9)

        case let .dshDirect(path):
            HStack(spacing: 8) {
                AgentBrandIcon(entry.brand, size: 16)
                Text(entry.name)
                    .font(DesignFonts.body(13, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                StatusChip("原生直连", tone: .success)
                StatusChip("无需写入", tone: .neutral)
                Spacer(minLength: 8)
                Text(path)
                    .font(DesignFonts.mono(11))
                    .foregroundStyle(palette.textMuted.color)
                    .lineLimit(1)
                    .help(path)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
        }
    }

    @ViewBuilder
    private var liveOverviewPanel: some View {
        let sessions = status?.activeSessions ?? []
        let replyNeeded = sessions.filter { $0.bucket == "reply_needed" }
        let recoverable = sessions.filter { $0.bucket == "recoverable" }
        let running = sessions.filter { $0.bucket == "running" }
        let combined: [(session: ProbeLiveSession, badge: String, tone: StatusChipTone)] = {
            var list: [(session: ProbeLiveSession, badge: String, tone: StatusChipTone)] = []
            list.reserveCapacity(replyNeeded.count + recoverable.count + running.count)
            for item in replyNeeded { list.append((session: item, badge: "待回复", tone: .warning)) }
            for item in recoverable { list.append((session: item, badge: "异常", tone: .danger)) }
            for item in running { list.append((session: item, badge: "运行中", tone: .info)) }
            return list
        }()
        let top12 = Array(combined.prefix(12))
        let resolvedHostLabel: String = status?.hostLabel ?? (draft?.hostLabel ?? "本机")

        CollapsiblePanelCard(
            title: "全平台实时态势",
            icon: .activity,
            isExpanded: sectionBinding(.live),
            summaryBadges: {
                StatusChip("运行中 \(running.count)", tone: running.isEmpty ? .neutral : .info)
                StatusChip("待回复 \(replyNeeded.count)", tone: replyNeeded.isEmpty ? .neutral : .warning)
                StatusChip("异常 \(recoverable.count)", tone: recoverable.isEmpty ? .neutral : .danger)
            }
        ) {
            VStack(alignment: .leading, spacing: 8) {
                Text(resolvedHostLabel)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.textMuted.color)

                if top12.isEmpty {
                    Text("当前所有已启用平台均无运行中、待回复或异常会话")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textMuted.color)
                } else {
                    VStack(spacing: 0) {
                        ForEach(Array(top12.enumerated()), id: \.element.session.uniqueKey) { index, entry in
                            if index > 0 {
                                Rectangle()
                                    .fill(palette.borderSubtle.color)
                                    .frame(height: 1)
                            }
                            HStack(spacing: 10) {
                                Text(entry.session.title ?? entry.session.id)
                                    .font(DesignFonts.body(13, weight: .medium))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .lineLimit(1)
                                Spacer()
                                StatusChip(entry.badge, tone: entry.tone)
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 7)
                        }
                    }
                    .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(palette.elevated.color))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(palette.borderSubtle.color, lineWidth: 1)
                    )
                }
            }
        }
    }

    @ViewBuilder
    private var recentEventsPanel: some View {
        CollapsiblePanelCard(
            title: "最近事件",
            icon: .terminalSquare,
            isExpanded: sectionBinding(.events),
            summaryBadges: {
                if eventsError != nil {
                    StatusChip("读取失败", tone: .danger)
                } else if eventsLoaded {
                    StatusChip("\(events.count) 条", tone: .neutral)
                    if let latest = events.first {
                        let bark = ProbeFormatting.barkBadge(latest)
                        StatusChip(bark.label, tone: bark.tone)
                    }
                }
            }
        ) {
            if let eventsError, !eventsError.isEmpty {
                Text(eventsError)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            } else if !eventsLoaded {
                Text(loadingStatus ? "正在读取事件" : "暂无事件数据")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.textMuted.color)
            } else if events.isEmpty {
                Text("暂无最近通知与 Hook 事件")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.textMuted.color)
            } else {
                VStack(spacing: 8) {
                    ForEach(events) { ev in
                        let bark = ProbeFormatting.barkBadge(ev)
                        let dedupe = ProbeFormatting.dedupeBadge(ev)
                        let kindLabel = ProbeFormatting.eventKindLabel(ev.kind)
                        let headline = (ev.title?.isEmpty == false ? ev.title! : kindLabel)
                        let displayHeadline: String = (headline == kindLabel) ? headline : "\(kindLabel) · \(headline)"

                        VStack(alignment: .leading, spacing: 6) {
                            HStack(alignment: .top, spacing: 8) {
                                Text(displayHeadline)
                                    .font(DesignFonts.body(13, weight: .semibold))
                                    .foregroundStyle(palette.textPrimary.color)
                                Spacer(minLength: 8)
                                FlowButtonRow(spacing: 6, lineSpacing: 4) {
                                    StatusChip(bark.label, tone: bark.tone)
                                    if let dedupe {
                                        StatusChip(dedupe.label, tone: dedupe.tone)
                                    }
                                }
                            }
                            if let summary = ev.summary ?? ev.message, !summary.isEmpty {
                                Text(summary)
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textSecondary.color)
                                    .lineLimit(3)
                            }
                        }
                        .padding(10)
                        .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
                    }
                }
            }
        }
    }
    // MARK: - Helpers & Actions

    private func compactSwitchTile(label: String, isOn: Binding<Bool>) -> some View {
        HStack(spacing: 8) {
            Text(label)
                .font(DesignFonts.body(13, weight: .medium))
                .foregroundStyle(palette.textPrimary.color)
            Spacer(minLength: 4)
            CapsuleSwitch(isOn: isOn, label: label)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .frame(minHeight: 34)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
    }

    private func labeledField(_ label: String, placeholder: String, value: String, onChange: @escaping @MainActor (String) -> Void) -> some View {
        ProbeDraftField(label: label, placeholder: placeholder, value: value, isSecure: false, onChange: onChange)
    }

    private func updateDraft(scope: ProbeSettingsSaveScope = .settings, _ mutate: (inout ProbeSettingsDraft) -> Void) {
        guard var current = settingsSync.draft else { return }
        mutate(&current)
        if current != settingsSync.draft, saveTarget == scope, saveMessage?.tone == .danger {
            saveMessage = nil
        }
        settingsSync.draft = current
    }

    private func probeStateLabel(_ raw: String?) -> String {
        switch raw {
        case "managed", "enabled", "ok", "active": "正常"
        case "missing": "未配置"
        case "stale": "需修复"
        case "disabled": "已停用"
        default: raw ?? "未知"
        }
    }

    private func syncAutoOpenIssues(machineScope: String) {
        disclosureRules.observeIssue(
            machineScope: machineScope,
            section: .hosts,
            issueSignature: ProbeDisclosureRules.hostsIssueSignature(
                codexHookStatus: status?.hookStatus,
                statusError: statusError,
                hookErrors: hookErrors
            )
        )
        disclosureRules.observeIssue(
            machineScope: machineScope,
            section: .live,
            issueSignature: ProbeDisclosureRules.liveIssueSignature(
                sessions: status?.activeSessions ?? []
            )
        )
        disclosureRules.observeIssue(
            machineScope: machineScope,
            section: .events,
            issueSignature: ProbeDisclosureRules.eventsIssueSignature(
                eventsError: eventsError
            )
        )
    }

    private func fetchSettingsResult(at rev: UInt64) async -> Result<ProbeSettingsView, Error> {
        do {
            let loaded: ProbeSettingsView = try await store.rpc("probe.settings.get", at: rev)
            return .success(loaded)
        } catch {
            return .failure(error)
        }
    }

    private func fetchSingleHookResult(provider: String, at rev: UInt64) async -> Result<ClaudeHooksStatus, Error> {
        do {
            let st: ClaudeHooksStatus = try await store.rpc(
                "claude.hooks",
                args: [
                    "request": .object(["action": .string("status"), "confirmed": .bool(false)]),
                    "provider": .string(provider),
                ],
                localOnly: true,
                at: rev
            )
            return .success(st)
        } catch {
            return .failure(error)
        }
    }

    private func fetchLocalHookStatuses(at rev: UInt64) async -> [String: Result<ClaudeHooksStatus, Error>] {
        // Unconditionally fetch all 3 local-capable hook providers so startup capability races
        // never transiently render Antigravity as "未安装".
        async let claude = fetchSingleHookResult(provider: "claude", at: rev)
        async let grok = fetchSingleHookResult(provider: "grok", at: rev)
        async let antigravity = fetchSingleHookResult(provider: "antigravity", at: rev)
        return [
            "claude": await claude,
            "grok": await grok,
            "antigravity": await antigravity,
        ]
    }

    private func refreshAll(force: Bool) async {
        let rev = store.revision
        let scope = store.machineScope
        let isLocal = (store.target == .local)
        refreshToken &+= 1
        let token = refreshToken
        let readFence = settingsSync.beginSettingsRead()
        loadingStatus = true
        defer {
            if refreshToken == token {
                loadingStatus = false
            }
        }

        async let statusAndEvents: Void = refreshStatusAndEvents(force: force, boundRevision: rev, boundScope: scope)
        async let settingsOutcome = fetchSettingsResult(at: rev)
        async let hooksOutcome: [String: Result<ClaudeHooksStatus, Error>] = isLocal
            ? fetchLocalHookStatuses(at: rev)
            : [:]

        _ = await statusAndEvents
        let resolvedSettings = await settingsOutcome
        let resolvedHooks = await hooksOutcome
        guard store.revision == rev, refreshToken == token else { return }

        switch resolvedSettings {
        case let .success(loaded):
            settingsSync.applyLoadedSettings(loaded, readFence: readFence, revision: rev)
        case let .failure(error):
            settingsSync.applySettingsReadError(error.localizedDescription, readFence: readFence)
        }

        if isLocal {
            for (provider, result) in resolvedHooks {
                switch result {
                case let .success(st):
                    hookStatuses[provider] = st
                    hookErrors.removeValue(forKey: provider)
                case let .failure(error):
                    hookStatuses.removeValue(forKey: provider)
                    hookErrors[provider] = error.localizedDescription
                }
            }
        } else {
            hookStatuses.removeAll()
            hookErrors.removeAll()
        }
        syncAutoOpenIssues(machineScope: scope)
    }

    private func refreshStatusAndEvents(
        force: Bool = false,
        boundRevision: UInt64? = nil,
        boundScope: String? = nil
    ) async {
        let rev = boundRevision ?? store.revision
        let scope = boundScope ?? store.machineScope
        statusEventsSeq &+= 1
        let seq = statusEventsSeq

        async let statusResult: Result<ProbeStatus, Error> = {
            do {
                let st: ProbeStatus = try await store.rpc(
                    "probe.status",
                    args: force ? ["refresh": .bool(true)] : [:],
                    at: rev
                )
                return .success(st)
            } catch {
                return .failure(error)
            }
        }()
        async let eventsResult: Result<ProbeEventsResponse, Error> = {
            do {
                let ev: ProbeEventsResponse = try await store.rpc(
                    "probe.events",
                    args: ["limit": .number(10)],
                    at: rev
                )
                return .success(ev)
            } catch {
                return .failure(error)
            }
        }()

        let resolvedStatus = await statusResult
        let resolvedEvents = await eventsResult
        guard store.revision == rev, statusEventsSeq == seq else { return }

        switch resolvedStatus {
        case let .success(st):
            if st.available == false {
                status = nil
                statusError = st.error ?? "Probe 状态不可用"
            } else {
                status = st
                statusError = nil
            }
        case let .failure(error):
            statusError = error.localizedDescription
        }

        switch resolvedEvents {
        case let .success(ev):
            eventsLoaded = true
            eventsError = nil
            events = ev.events
        case let .failure(error):
            eventsError = error.localizedDescription
        }
        syncAutoOpenIssues(machineScope: scope)
    }

    private func saveSettings(scope: ProbeSettingsSaveScope) async {
        guard let submittedDraft = settingsSync.draft, !saving, store.revision == settingsSync.draftRevision else { return }
        let boundRevision = settingsSync.draftRevision
        let epoch = machineEpoch
        saveTarget = scope
        saveMessage = nil

        if scope == .settings {
            let errs = submittedDraft.validationErrors(for: .settings)
            if let first = errs.first {
                disclosureRules.resetIfMachineChanged(store.machineScope)
                disclosureRules.setExpanded(.advanced, true)
                saveMessage = (.danger, first)
                return
            }
        }

        saving = true
        defer {
            if machineEpoch == epoch {
                saving = false
            }
        }
        do {
            let payload = try submittedDraft.buildSavePayload(for: scope)
            let saved: ProbeSettingsView = try await store.rpc(
                "probe.settings.save",
                args: ["settings": .object(payload)],
                at: boundRevision
            )
            guard store.revision == boundRevision, machineEpoch == epoch else { return }
            settingsSync.applySavedSettings(
                saved,
                submittedDraft: submittedDraft,
                scope: scope,
                revision: boundRevision
            )
            saveMessage = (.success, "设置已保存")
        } catch {
            guard store.revision == boundRevision, machineEpoch == epoch else { return }
            saveMessage = (.danger, error.localizedDescription)
            if scope == .bark {
                disclosureRules.observeIssue(
                    machineScope: store.machineScope,
                    section: .bark,
                    issueSignature: "saveErr:\(error.localizedDescription)"
                )
            }
        }
    }

    private func startProbeJob(_ command: String, label: String, revision: UInt64) async {
        guard !jobBusy, store.revision == revision else { return }
        let epoch = machineEpoch
        startingJob = true
        defer {
            if machineEpoch == epoch {
                startingJob = false
            }
        }
        actionMessage = nil
        activeJobAction = command
        do {
            let res: StartedJobResponse = try await store.rpc(command, at: revision)
            guard store.revision == revision, machineEpoch == epoch else { return }
            activeJobId = res.jobId
            store.trackStartedJob(res.jobId)
            if command != "probe.barkTest" {
                actionMessage = (.success, "\(label) 已开始")
            }
        } catch {
            guard store.revision == revision, machineEpoch == epoch else { return }
            actionMessage = (.danger, "\(label) 失败: \(error.localizedDescription)")
        }
    }

    private func executeHookDialog(_ dialog: PendingHookDialog) async {
        guard store.revision == dialog.revision else { return }
        if dialog.provider == "codex" {
            let cmd = dialog.action == "install" ? "probe.installHooks" : "probe.removeHooks"
            await startProbeJob(
                cmd,
                label: dialog.action == "install" ? "写入 Codex Hook" : "移除 Codex Hook",
                revision: dialog.revision
            )
            return
        }
        let epoch = machineEpoch
        hookBusy = true
        defer {
            if machineEpoch == epoch {
                hookBusy = false
            }
        }
        do {
            let st: ClaudeHooksStatus = try await store.rpc(
                "claude.hooks",
                args: [
                    "request": .object(["action": .string(dialog.action), "confirmed": .bool(true)]),
                    "provider": .string(dialog.provider),
                ],
                localOnly: true,
                at: dialog.revision
            )
            guard store.revision == dialog.revision, machineEpoch == epoch else { return }
            hookStatuses[dialog.provider] = st
            hookErrors.removeValue(forKey: dialog.provider)
            actionMessage = (.success, "\(dialog.name) Hook 已更新")
        } catch {
            guard store.revision == dialog.revision, machineEpoch == epoch else { return }
            actionMessage = (.danger, error.localizedDescription)
        }
    }
}

private struct ProbeDraftField: View {
    @Environment(\.agentHubPalette) private var palette
    let label: String
    let placeholder: String
    let value: String
    let isSecure: Bool
    let onChange: @MainActor (String) -> Void
    @State private var localText: String

    init(
        label: String,
        placeholder: String,
        value: String,
        isSecure: Bool = false,
        onChange: @escaping @MainActor (String) -> Void
    ) {
        self.label = label
        self.placeholder = placeholder
        self.value = value
        self.isSecure = isSecure
        self.onChange = onChange
        self._localText = State(initialValue: value)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label)
                .font(DesignFonts.body(12))
                .foregroundStyle(palette.textMuted.color)
            Group {
                if isSecure {
                    SecureField(placeholder, text: $localText)
                } else {
                    TextField(placeholder, text: $localText)
                }
            }
            .textFieldStyle(.plain)
            .font(DesignFonts.body(13))
            .foregroundStyle(palette.textPrimary.color)
            .padding(.horizontal, 10)
            .frame(height: LayoutMetrics.formControlHeight)
            .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .onChange(of: value) { next in
            if localText != next { localText = next }
        }
        .onChange(of: localText) { next in
            if next != value { onChange(next) }
        }
    }
}
