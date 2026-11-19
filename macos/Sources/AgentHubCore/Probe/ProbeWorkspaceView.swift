import SwiftUI

public struct ProbeWorkspaceView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    @State private var status: ProbeStatus?
    @State private var settingsView: ProbeSettingsView?
    @State private var draft: ProbeSettingsDraft?
    @State private var events: [ProbeEventItem] = []
    @State private var eventsAvailable: Bool = false
    @State private var loadingStatus: Bool = false
    @State private var saving: Bool = false
    @State private var saveTarget: String = "settings"
    @State private var saveMessage: (tone: StatusChipTone, text: String)?
    @State private var actionMessage: (tone: StatusChipTone, text: String)?

    @State private var activeJobId: String?
    @State private var activeJobAction: String?

    @State private var hookStatuses: [String: ClaudeHooksStatus] = [:]
    @State private var pendingHookDialog: (provider: String, name: String, action: String, path: String)?
    @State private var hookBusy: Bool = false
    @State private var advancedSettingsOpen: Bool = true

    public init(store: RuntimeConnectionStore) {
        self.store = store
    }

    private var jobBusy: Bool {
        guard let id = activeJobId else { return false }
        if let tracked = store.trackedJobs[id] {
            return !tracked.isTerminal
        }
        return true
    }

    private var validationErrors: [String] {
        draft?.validationErrors() ?? []
    }

    private var configuredDeviceKey: Bool {
        guard let draft else { return false }
        return draft.barkDeviceKeyConfigured || !draft.barkDeviceKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var body: some View {
        GeometryReader { geo in
            let isNarrow = geo.size.width <= LayoutMetrics.mobileBreakpoint
            ZStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        // Header
                        HStack {
                            Text("Probe")
                                .font(DesignFonts.body(22, weight: .semibold))
                                .foregroundStyle(palette.textPrimary.color)
                            Spacer()
                            Button {
                                Task { await refreshAll(force: true) }
                            } label: {
                                LucideIcon(.refreshCw, size: 17)
                            }
                            .buttonStyle(IconButtonStyle(dimension: 32))
                            .help("刷新 Probe")
                        }

                        // 1. Full-width GitHub CI Panel
                        if store.capabilities.githubCi {
                            GithubCiCardView(
                                store: store,
                                draft: $draft,
                                saving: saving,
                                saveMessage: saveTarget == "ci" ? saveMessage : nil,
                                onSave: { Task { await saveSettings(target: "ci") } }
                            )
                        }

                        // 2 & 3. Notification & Runtime Settings Panel (6 metrics + switches + 8 advanced fields)
                        PanelCard(title: "通知配置与快捷开关", icon: .slidersHorizontal) {
                            if let actionMessage {
                                Text(actionMessage.text)
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle((actionMessage.tone == .success ? palette.okFg : palette.errFg).color)
                            }
                            if let d = draft {
                                runtimeSettingsBody(draft: d, isNarrow: isNarrow)
                            } else {
                                Text(loadingStatus ? "正在读取设置" : "设置不可用")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textMuted.color)
                            }
                        }

                        // 3b. Bark Panel
                        if let d = draft {
                            barkPanelBody(draft: d)
                        }

                        // 4. 5-Platform Host Matrix
                        hostMatrixSection(isNarrow: isNarrow)

                        // 5. Live Overview (top 12)
                        liveOverviewPanel

                        // 6. Recent Events
                        recentEventsPanel

                        // 7. Execution History (default collapsed)
                        JobHistorySectionView(store: store, title: "执行记录")
                    }
                    .frame(maxWidth: 1060, alignment: .leading)
                    .padding(.vertical, isNarrow ? 18 : 28)
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

                            Button(dialog.action == "install" ? "确认写入" : "确认移除") {
                                let captured = dialog
                                pendingHookDialog = nil
                                Task { await executeHookDialog(captured) }
                            }
                            .buttonStyle(dialog.action == "remove" ? AnyButtonStyle(DangerButtonStyle(soft: false)) : AnyButtonStyle(PrimaryButtonStyle()))
                            .disabled(hookBusy || jobBusy)
                        }
                    }
                }
            }
        }
        .task(id: store.machineScope) {
            draft = nil
            status = nil
            events = []
            hookStatuses = [:]
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
    private func runtimeSettingsBody(draft d: ProbeSettingsDraft, isNarrow: Bool) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            // 6 Status Metric Cards
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                MetricCard(
                    label: "探针",
                    value: d.probeEnabled ? "已启用" : "已停用",
                    tone: d.probeEnabled ? .success : .warning
                )
                MetricCard(
                    label: "通知",
                    value: d.barkEnabled ? "已启用" : "已停用",
                    tone: d.barkEnabled ? .success : .warning
                )
                MetricCard(
                    label: "错误监控",
                    value: probeStateLabel(status?.errorMonitorStatus ?? (d.errorMonitorEnabled ? "enabled" : "disabled")),
                    tone: d.errorMonitorEnabled ? .success : .warning
                )
                MetricCard(
                    label: "错误事件",
                    value: String(status?.errorMonitorIncidentCount ?? 0)
                )
                MetricCard(
                    label: "Codex Home",
                    value: status?.codexHome ?? (d.codexHome.isEmpty ? "auto" : d.codexHome)
                )
                MetricCard(
                    label: "Discovery",
                    value: (status?.discoveryWarnings?.isEmpty == false)
                        ? status!.discoveryWarnings!.joined(separator: "；")
                        : "正常"
                )
            }

            // Probe & Error Monitor switches
            LazyVGrid(
                columns: isNarrow ? [GridItem(.flexible())] : [GridItem(.flexible()), GridItem(.flexible())],
                spacing: 10
            ) {
                toggleRow("启用 Probe", isOn: d.probeEnabled) { next in
                    updateDraft { $0.probeEnabled = next }
                }
                toggleRow("终止错误监控", isOn: d.errorMonitorEnabled) { next in
                    updateDraft { $0.errorMonitorEnabled = next }
                }
            }

            // 6 Platform Notification One-Click Switches
            VStack(alignment: .leading, spacing: 8) {
                Text("平台通知一键开关")
                    .font(DesignFonts.body(13, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                let visiblePlatforms = ProbeNotificationPlatform.allCases.filter { p in
                    switch p {
                    case .antigravity: store.capabilities.antigravityNotifications
                    case .grokbot: store.capabilities.grokbotNotifications
                    case .dsh: store.capabilities.dshNotifications
                    default: true
                    }
                }
                let platformCols = isNarrow
                    ? [GridItem(.flexible())]
                    : [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())]
                LazyVGrid(columns: platformCols, spacing: 8) {
                    ForEach(visiblePlatforms) { platform in
                        let streams = status?.providerNotifications?.first(where: { $0.provider == platform.rawValue })?.streams
                        HStack(spacing: 8) {
                            AgentBrandIcon(platform.brand, size: 17)
                                .foregroundStyle(palette.textPrimary.color)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(platform.label)
                                    .font(DesignFonts.body(13, weight: .semibold))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .lineLimit(1)
                                if let streams {
                                    Text("\(streams) 个会话")
                                        .font(DesignFonts.body(11))
                                        .foregroundStyle(palette.textMuted.color)
                                }
                            }
                            Spacer(minLength: 4)
                            CapsuleSwitch(
                                isOn: Binding(
                                    get: { d.isPlatformEnabled(platform) },
                                    set: { val in updateDraft { $0.setPlatformEnabled(platform, enabled: val) } }
                                ),
                                label: "\(platform.label) 通知"
                            )
                        }
                        .padding(10)
                        .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
                    }
                }
            }

            // Default-expanded 8 Advanced Parameters Disclosure (styles.css:868)
            VStack(alignment: .leading, spacing: 10) {
                Button {
                    advancedSettingsOpen.toggle()
                } label: {
                    HStack(spacing: 6) {
                        LucideIcon(advancedSettingsOpen ? .chevronDown : .chevronRight, size: 15)
                            .foregroundStyle(palette.textMuted.color)
                        Text("高级设置")
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textSecondary.color)
                        Spacer()
                    }
                    .padding(.vertical, 4)
                    .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())

                if advancedSettingsOpen {
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
                }
            }
            .padding(.top, 10)
            .overlay(alignment: .top) {
                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
            }

            if let firstError = validationErrors.first {
                Text(firstError)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }
            if saveTarget == "settings", let saveMessage {
                Text(saveMessage.text)
                    .font(DesignFonts.body(12))
                    .foregroundStyle((saveMessage.tone == .success ? palette.okFg : palette.errFg).color)
            }

            Button {
                Task { await saveSettings(target: "settings") }
            } label: {
                HStack(spacing: 6) {
                    LucideIcon(.checkCircle2, size: 16)
                    Text("保存设置")
                }
            }
            .buttonStyle(PrimaryButtonStyle())
            .disabled(saving || !validationErrors.isEmpty)
        }
    }

    @ViewBuilder
    private func barkPanelBody(draft d: ProbeSettingsDraft) -> some View {
        PanelCard(title: "Bark", icon: .cloud) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    MetricCard(
                        label: "通道状态",
                        value: d.barkEnabled ? "已启用" : "已停用",
                        tone: d.barkEnabled ? .success : .warning
                    )
                    MetricCard(
                        label: "密钥状态",
                        value: configuredDeviceKey ? "已配置" : "未配置",
                        tone: configuredDeviceKey ? .success : .warning
                    )
                }

                toggleRow("启用 Bark", isOn: d.barkEnabled) { next in
                    updateDraft { $0.barkEnabled = next }
                }

                HStack(spacing: 12) {
                    labeledField("Bark 服务 URL", placeholder: "https://api.day.app", value: d.barkServerUrl) { v in
                        updateDraft { $0.barkServerUrl = v }
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Device Key")
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.textMuted.color)
                        SecureField(
                            configuredDeviceKey ? "已配置，留空保持不变" : "粘贴 Bark Device Key",
                            text: Binding(
                                get: { d.barkDeviceKey },
                                set: { v in updateDraft { $0.barkDeviceKey = v } }
                            )
                        )
                        .textFieldStyle(.plain)
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textPrimary.color)
                        .padding(.horizontal, 10)
                        .frame(height: LayoutMetrics.formControlHeight)
                        .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
                        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))
                    }
                    .frame(maxWidth: .infinity)
                }

                FlowButtonRow(spacing: 8, lineSpacing: 8) {
                    Button {
                        Task { await saveSettings(target: "bark") }
                    } label: {
                        HStack(spacing: 6) {
                            LucideIcon(.checkCircle2, size: 16)
                            Text("保存")
                        }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(saving)

                    Button {
                        Task { await startProbeJob("probe.barkTest", label: "AgentHub 推送测试") }
                    } label: {
                        HStack(spacing: 6) {
                            LucideIcon(.cloud, size: 16)
                            Text("测试推送")
                        }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(!configuredDeviceKey || jobBusy)
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

                if saveTarget == "bark", let saveMessage {
                    Text(saveMessage.text)
                        .font(DesignFonts.body(12))
                        .foregroundStyle((saveMessage.tone == .success ? palette.okFg : palette.errFg).color)
                }
            }
        }
    }

    @ViewBuilder
    private func hostMatrixSection(isNarrow: Bool) -> some View {
        let cols = isNarrow ? [GridItem(.flexible())] : [GridItem(.flexible()), GridItem(.flexible())]
        LazyVGrid(columns: cols, spacing: 12) {
            // Codex Host
            PanelCard(title: "Codex Host", icon: .gitFork) {
                let hookStatus = status?.hookStatus
                let managed = (hookStatus == "managed")
                let actualCount = status?.actualCommands?.count ?? 0
                let staleCount = status?.staleCommandCount ?? 0
                let canRemove = managed || actualCount > 0 || staleCount > 0
                let path = status?.codexHooksPath ?? "未返回路径"

                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        AgentBrandIcon(.codex, size: 16)
                        Text("Codex").font(DesignFonts.body(13, weight: .semibold))
                        Spacer()
                        Text(path)
                            .font(DesignFonts.mono(11))
                            .foregroundStyle(palette.textMuted.color)
                            .lineLimit(1)
                    }
                    HStack(spacing: 8) {
                        MetricCard(label: "Codex Hook", value: probeStateLabel(hookStatus), tone: managed ? .success : .warning)
                        MetricCard(label: "实际命令", value: "\(actualCount) 条")
                    }
                    FlowButtonRow(spacing: 8, lineSpacing: 8) {
                        Button(managed ? "重新写入 Hook" : "写入通知 Hook") {
                            pendingHookDialog = ("codex", "Codex", "install", status?.codexHooksPath ?? "")
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(jobBusy)

                        Button("移除 Hook") {
                            pendingHookDialog = ("codex", "Codex", "remove", status?.codexHooksPath ?? "")
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(jobBusy || !canRemove)
                    }
                }
            }

            // Claude, Grok, Antigravity Hosts
            providerHookCard(provider: "claude", brand: .claude, title: "Claude Code Host", name: "Claude Code")
            providerHookCard(provider: "grok", brand: .grok, title: "Grok Build Host", name: "Grok Build")
            if store.capabilities.antigravityNotifications {
                providerHookCard(provider: "antigravity", brand: .antigravity, title: "Antigravity Host", name: "Antigravity")
            }
            if store.capabilities.dshNotifications {
                PanelCard(title: "DeepSeek Harness Host", icon: .gitFork) {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack {
                            AgentBrandIcon(.dsh, size: 16)
                            Text("DeepSeek Harness").font(DesignFonts.body(13, weight: .semibold))
                            Spacer()
                            Text(status?.dshSessionsPath ?? "原生日志直连")
                                .font(DesignFonts.mono(11))
                                .foregroundStyle(palette.textMuted.color)
                                .lineLimit(1)
                        }
                        HStack(spacing: 8) {
                            MetricCard(label: "接入方式", value: "原生直连", tone: .success)
                            MetricCard(label: "Hook 配置", value: "无需写入")
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func providerHookCard(provider: String, brand: AgentBrandId, title: String, name: String) -> some View {
        PanelCard(title: title, icon: .gitFork) {
            if store.target == .remote {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        AgentBrandIcon(brand, size: 16)
                        Text(name).font(DesignFonts.body(13, weight: .semibold))
                        Spacer()
                        Text("远端只读").font(DesignFonts.mono(11)).foregroundStyle(palette.textMuted.color)
                    }
                    HStack(spacing: 8) {
                        MetricCard(label: "Hook 状态", value: "仅限本机")
                        MetricCard(label: "宿主模式", value: "远端只读轮询")
                    }
                }
            } else {
                let st = hookStatuses[provider]
                let installed = st?.installed ?? false
                let count = st?.managedCount ?? 0
                let path = st?.configPath ?? "未返回路径"
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        AgentBrandIcon(brand, size: 16)
                        Text(name).font(DesignFonts.body(13, weight: .semibold))
                        Spacer()
                        Text(path).font(DesignFonts.mono(11)).foregroundStyle(palette.textMuted.color).lineLimit(1)
                    }
                    HStack(spacing: 8) {
                        MetricCard(label: "Hook 状态", value: installed ? "已安装" : "未安装", tone: installed ? .success : .warning)
                        MetricCard(label: "托管条目", value: "\(count) 条")
                    }
                    FlowButtonRow(spacing: 8, lineSpacing: 8) {
                        Button(installed ? "重新写入 Hook" : "写入通知 Hook") {
                            pendingHookDialog = (provider, name, "install", st?.configPath ?? "")
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(hookBusy)

                        Button("移除 Hook") {
                            pendingHookDialog = (provider, name, "remove", st?.configPath ?? "")
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(hookBusy || (!installed && count == 0))
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var liveOverviewPanel: some View {
        let replyNeeded = status?.replyNeededThreads ?? []
        let recoverable = status?.recoverableThreads ?? []
        let running = status?.runningThreads ?? []
        let combined: [(session: ProbeLiveSession, badge: String, tone: StatusChipTone)] =
            replyNeeded.map { ($0, "待回复", .warning) } +
            recoverable.map { ($0, "异常", .danger) } +
            running.map { ($0, "运行中", .info) }
        let top12 = Array(combined.prefix(12))

        PanelCard(title: "实时态势", icon: .activity) {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("全平台实时态势")
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                        Text(status?.hostLabel ?? draft?.hostLabel ?? "本机")
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.textMuted.color)
                    }
                    Spacer()
                    HStack(spacing: 6) {
                        StatusChip("运行中 \(running.count)", tone: .info)
                        StatusChip("待回复 \(replyNeeded.count)", tone: .warning)
                        StatusChip("异常 \(recoverable.count)", tone: .danger)
                    }
                }

                if top12.isEmpty {
                    Text("当前所有已启用平台均无运行中、待回复或异常会话")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textMuted.color)
                } else {
                    ForEach(top12, id: \.session.uniqueKey) { entry in
                        HStack(spacing: 10) {
                            Text(entry.session.title ?? entry.session.id)
                                .font(DesignFonts.body(13, weight: .medium))
                                .foregroundStyle(palette.textPrimary.color)
                                .lineLimit(1)
                            Spacer()
                            StatusChip(entry.badge, tone: entry.tone)
                        }
                        .padding(.vertical, 6)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var recentEventsPanel: some View {
        PanelCard(title: "最近事件", icon: .terminalSquare) {
            if !eventsAvailable {
                Text(loadingStatus ? "正在读取事件" : "事件接口不可用")
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

                        VStack(alignment: .leading, spacing: 6) {
                            HStack(alignment: .top, spacing: 8) {
                                Text(headline == kindLabel ? headline : "\(kindLabel) · \(headline)")
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

    private func toggleRow(_ label: String, isOn: Bool, onChange: @escaping @MainActor (Bool) -> Void) -> some View {
        ProbeDraftToggleRow(label: label, value: isOn, onChange: onChange)
    }

    private func labeledField(_ label: String, placeholder: String, value: String, onChange: @escaping @MainActor (String) -> Void) -> some View {
        ProbeDraftTextField(label: label, placeholder: placeholder, value: value, onChange: onChange)
    }

    private func updateDraft(_ mutate: (inout ProbeSettingsDraft) -> Void) {
        guard var current = draft else { return }
        mutate(&current)
        draft = current
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

    private func refreshAll(force: Bool) async {
        loadingStatus = true
        defer { loadingStatus = false }
        await refreshStatusAndEvents(force: force)
        if let envelope: OptionalResultEnvelope<ProbeSettingsView> = try? await store.rpc("probe.settings.get"),
           let loaded = envelope.data
        {
            settingsView = loaded
            if draft == nil || force {
                draft = ProbeSettingsDraft.from(loaded)
            }
        }
        if store.target == .local {
            for provider in ["claude", "grok", "antigravity"] {
                if let st: ClaudeHooksStatus = try? await store.rpc(
                    "claude.hooks",
                    args: [
                        "request": .object(["action": .string("status"), "confirmed": .bool(false)]),
                        "provider": .string(provider),
                    ],
                    localOnly: true
                ) {
                    hookStatuses[provider] = st
                }
            }
        }
    }

    private func refreshStatusAndEvents(force: Bool = false) async {
        if let stEnv: OptionalResultEnvelope<ProbeStatus> = try? await store.rpc(
            "probe.status",
            args: force ? ["refresh": .bool(true)] : [:]
        ) {
            status = stEnv.data
        }
        if let evEnv: OptionalResultEnvelope<ProbeEventsResponse> = try? await store.rpc(
            "probe.events",
            args: ["limit": .number(10)]
        ) {
            eventsAvailable = evEnv.available
            events = evEnv.data?.events ?? []
        }
    }

    private func saveSettings(target: String) async {
        guard let currentDraft = draft, !saving else { return }
        saving = true
        saveTarget = target
        saveMessage = nil
        defer { saving = false }
        do {
            let payload = try currentDraft.buildSavePayload()
            let savedEnv: OptionalResultEnvelope<ProbeSettingsView> = try await store.rpc(
                "probe.settings.save",
                args: ["settings": .object(payload)]
            )
            if let saved = savedEnv.data {
                settingsView = saved
                var refreshed = ProbeSettingsDraft.from(saved)
                if !currentDraft.barkDeviceKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    refreshed.barkDeviceKeyConfigured = true
                }
                draft = refreshed
            }
            saveMessage = (.success, "设置已保存")
        } catch {
            saveMessage = (.danger, error.localizedDescription)
        }
    }

    private func startProbeJob(_ command: String, label: String) async {
        actionMessage = nil
        activeJobAction = command
        do {
            let res: StartedJobResponse = try await store.rpc(command)
            activeJobId = res.jobId
            store.trackStartedJob(res.jobId)
            if command != "probe.barkTest" {
                actionMessage = (.success, "\(label) 已开始")
            }
        } catch {
            actionMessage = (.danger, "\(label) 失败: \(error.localizedDescription)")
        }
    }

    private func executeHookDialog(_ dialog: (provider: String, name: String, action: String, path: String)) async {
        if dialog.provider == "codex" {
            let cmd = dialog.action == "install" ? "probe.installHooks" : "probe.removeHooks"
            await startProbeJob(cmd, label: dialog.action == "install" ? "写入 Codex Hook" : "移除 Codex Hook")
            await refreshStatusAndEvents(force: true)
            return
        }
        hookBusy = true
        defer { hookBusy = false }
        do {
            let st: ClaudeHooksStatus = try await store.rpc(
                "claude.hooks",
                args: [
                    "request": .object(["action": .string(dialog.action), "confirmed": .bool(true)]),
                    "provider": .string(dialog.provider),
                ],
                localOnly: true
            )
            hookStatuses[dialog.provider] = st
            actionMessage = (.success, "\(dialog.name) Hook 已更新")
        } catch {
            actionMessage = (.danger, error.localizedDescription)
        }
    }
}

public struct AnyButtonStyle: ButtonStyle {
    private let makeBodyClosure: (Configuration) -> AnyView

    public init<S: ButtonStyle>(_ style: S) {
        self.makeBodyClosure = { config in AnyView(style.makeBody(configuration: config)) }
    }

    public func makeBody(configuration: Configuration) -> some View {
        makeBodyClosure(configuration)
    }
}

private struct ProbeDraftToggleRow: View {
    @Environment(\.agentHubPalette) private var palette
    let label: String
    let value: Bool
    let onChange: @MainActor (Bool) -> Void
    @State private var localValue: Bool

    init(label: String, value: Bool, onChange: @escaping @MainActor (Bool) -> Void) {
        self.label = label
        self.value = value
        self.onChange = onChange
        self._localValue = State(initialValue: value)
    }

    var body: some View {
        HStack {
            Text(label)
                .font(DesignFonts.body(13))
                .foregroundStyle(palette.textPrimary.color)
            Spacer()
            CapsuleSwitch(isOn: $localValue, label: label)
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
        .onChange(of: value) { next in
            if localValue != next { localValue = next }
        }
        .onChange(of: localValue) { next in
            if next != value { onChange(next) }
        }
    }
}

private struct ProbeDraftTextField: View {
    @Environment(\.agentHubPalette) private var palette
    let label: String
    let placeholder: String
    let value: String
    let onChange: @MainActor (String) -> Void
    @State private var localText: String

    init(label: String, placeholder: String, value: String, onChange: @escaping @MainActor (String) -> Void) {
        self.label = label
        self.placeholder = placeholder
        self.value = value
        self.onChange = onChange
        self._localText = State(initialValue: value)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label)
                .font(DesignFonts.body(12))
                .foregroundStyle(palette.textMuted.color)
            TextField(placeholder, text: $localText)
                .textFieldStyle(.plain)
                .font(DesignFonts.body(13))
                .foregroundStyle(palette.textPrimary.color)
                .padding(.horizontal, 10)
                .frame(height: LayoutMetrics.formControlHeight)
                .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))
        }
        .onChange(of: value) { next in
            if localText != next { localText = next }
        }
        .onChange(of: localText) { next in
            if next != value { onChange(next) }
        }
    }
}
