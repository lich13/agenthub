import AppKit
import SwiftUI

public struct GithubCiCardView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    @Binding public var draft: ProbeSettingsDraft?
    private var externalRules: Binding<ProbeDisclosureRules>?
    public var saving: Bool
    public var savingCurrentScope: Bool
    public var saveMessage: (tone: StatusChipTone, text: String)?
    public let onSave: () -> Void

    @State private var localRules = ProbeDisclosureRules()
    @State private var snapshot: GithubCiSnapshot?
    @State private var loading: Bool = false
    @State private var refreshing: Bool = false
    @State private var fetchEpoch: UInt64 = 0
    @State private var errorMessage: String?
    @State private var repoQuery: String = ""

    public init(
        store: RuntimeConnectionStore,
        draft: Binding<ProbeSettingsDraft?>,
        disclosureRules: Binding<ProbeDisclosureRules>? = nil,
        saving: Bool,
        savingCurrentScope: Bool? = nil,
        saveMessage: (tone: StatusChipTone, text: String)?,
        onSave: @escaping () -> Void
    ) {
        self.store = store
        self._draft = draft
        self.externalRules = disclosureRules
        self.saving = saving
        self.savingCurrentScope = savingCurrentScope ?? saving
        self.saveMessage = saveMessage
        self.onSave = onSave
    }

    private var rulesBinding: Binding<ProbeDisclosureRules> {
        externalRules ?? $localRules
    }

    private var expandedBinding: Binding<Bool> {
        Binding(
            get: { rulesBinding.wrappedValue.isExpanded(.ci) },
            set: { next in
                var updated = rulesBinding.wrappedValue
                updated.resetIfMachineChanged(store.machineScope)
                updated.setExpanded(.ci, next)
                rulesBinding.wrappedValue = updated
            }
        )
    }

    private var reposExpandedBinding: Binding<Bool> {
        Binding(
            get: { rulesBinding.wrappedValue.isExpanded(.ciRepos) },
            set: { next in
                var updated = rulesBinding.wrappedValue
                updated.resetIfMachineChanged(store.machineScope)
                updated.setExpanded(.ciRepos, next)
                rulesBinding.wrappedValue = updated
            }
        )
    }

    private var notifyCiBinding: Binding<Bool> {
        Binding(
            get: { draft?.notifyGithubCi ?? false },
            set: { val in
                if var current = draft, current.notifyGithubCi != val {
                    current.notifyGithubCi = val
                    draft = current
                }
            }
        )
    }

    private var filterBotsBinding: Binding<Bool> {
        Binding(
            get: { draft?.githubCiFilterBots ?? true },
            set: { val in
                if var current = draft, current.githubCiFilterBots != val {
                    current.githubCiFilterBots = val
                    draft = current
                }
            }
        )
    }

    private var busy: Bool {
        loading || refreshing || (snapshot?.refreshing == true)
    }

    private var configuredRepos: [String] {
        (draft?.githubCiRepositories ?? [])
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    private var catalog: [GithubCiRepoOption] {
        GithubCiDomain.buildCatalog(
            repositories: snapshot?.repositories ?? [],
            runs: snapshot?.runs ?? [],
            configured: configuredRepos
        )
    }

    private var filteredCatalog: [GithubCiRepoOption] {
        let q = repoQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return catalog }
        return catalog.filter { $0.fullName.lowercased().contains(q) }
    }

    private var visibleRuns: [GithubCiRun] {
        let activeTokens = configuredRepos.flatMap { entry -> [String] in
            if let matched = catalog.first(where: {
                $0.fullName.lowercased() == entry.lowercased() || $0.repoId.lowercased() == entry.lowercased()
            }) {
                return [matched.fullName, matched.repoId]
            }
            return [entry]
        }
        return GithubCiDomain.visibleRuns(
            runs: snapshot?.runs ?? [],
            repositories: activeTokens,
            filterBots: draft?.githubCiFilterBots ?? true
        )
    }

    private var failedRunCount: Int {
        visibleRuns.filter { run in
            GithubCiDomain.runStatus(run).tone == .danger
        }.count
    }

    private var repoSelectorSummary: String {
        if configuredRepos.isEmpty {
            return "监控全部仓库"
        }
        let preview = configuredRepos.prefix(2).joined(separator: "、")
        if configuredRepos.count > 2 {
            return "已选 \(configuredRepos.count) 个仓库（\(preview) 等）"
        }
        return "已选 \(configuredRepos.count) 个仓库（\(preview)）"
    }

    public var body: some View {
        let accountText: String = if let login = snapshot?.account?.login {
            "@\(login)"
        } else if busy {
            "读取账号中"
        } else if !(draft?.notifyGithubCi ?? false) && errorMessage == nil && snapshot?.error == nil {
            "待启用扫描"
        } else {
            "未登录"
        }

        CollapsiblePanelCard(
            title: "GitHub CI",
            icon: .github,
            isExpanded: expandedBinding,
            summaryBadges: {
                StatusChip(accountText, tone: snapshot?.account != nil ? .info : .neutral)
                    .help(snapshot?.account?.login ?? accountText)
                StatusChip("\(visibleRuns.count) 次运行", tone: .neutral)
                if let err = errorMessage ?? snapshot?.error, !err.isEmpty {
                    StatusChip("异常", tone: .danger)
                } else if failedRunCount > 0 {
                    StatusChip("失败 \(failedRunCount)", tone: .danger)
                }
            },
            headerTrailing: {
                HStack(spacing: 6) {
                    Button {
                        Task { await fetchStatus(forceRefresh: true) }
                    } label: {
                        LucideIcon(.refreshCw, size: 16)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 32))
                    .disabled(busy)
                    .help("刷新 GitHub CI")

                    Button {
                        onSave()
                    } label: {
                        HStack(spacing: 5) {
                            LucideIcon(.check, size: 15)
                            Text(savingCurrentScope ? "保存中" : "保存")
                        }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(draft == nil || saving)
                }
            }
        ) {
            VStack(alignment: .leading, spacing: 10) {
                FlowButtonRow(spacing: 16, lineSpacing: 8) {
                    HStack(spacing: 8) {
                        Text("CI 监控通知")
                            .font(DesignFonts.body(13, weight: .medium))
                            .foregroundStyle(palette.textPrimary.color)
                        CapsuleSwitch(isOn: notifyCiBinding, label: "CI 监控通知")
                            .disabled(draft == nil || saving)
                    }
                    .frame(minHeight: LayoutMetrics.formControlHeight)

                    HStack(spacing: 8) {
                        Text("过滤 Bot")
                            .font(DesignFonts.body(13, weight: .medium))
                            .foregroundStyle(palette.textPrimary.color)
                        CapsuleSwitch(isOn: filterBotsBinding, label: "过滤 Bot")
                            .disabled(draft == nil || saving)
                    }
                    .frame(minHeight: LayoutMetrics.formControlHeight)
                }

                VStack(alignment: .leading, spacing: 8) {
                    Button {
                        reposExpandedBinding.wrappedValue.toggle()
                    } label: {
                        HStack(spacing: 8) {
                            LucideIcon(.folderGit2, size: 15)
                                .foregroundStyle(palette.accent.color)
                            Text(repoSelectorSummary)
                                .font(DesignFonts.body(13))
                                .foregroundStyle(palette.textPrimary.color)
                                .lineLimit(1)
                            Spacer(minLength: 8)
                            LucideIcon(reposExpandedBinding.wrappedValue ? .chevronDown : .chevronRight, size: 15)
                                .foregroundStyle(palette.textMuted.color)
                        }
                        .padding(.horizontal, 10)
                        .frame(maxWidth: .infinity, minHeight: LayoutMetrics.formControlHeight, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(palette.elevated.color))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(PlainHitButtonStyle())

                    if reposExpandedBinding.wrappedValue {
                        VStack(alignment: .leading, spacing: 8) {
                            HStack(spacing: 6) {
                                LucideIcon(.search, size: 14)
                                    .foregroundStyle(palette.textMuted.color)
                                TextField("搜索或输入 owner/repo", text: $repoQuery)
                                    .textFieldStyle(.plain)
                                    .font(DesignFonts.body(13))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .onSubmit { addQueryRepo() }
                            }
                            .padding(.horizontal, 10)
                            .frame(maxWidth: .infinity, minHeight: LayoutMetrics.formControlHeight)
                            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(palette.surface.color))
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                            )

                            ScrollView {
                                FlowButtonRow(spacing: 8, lineSpacing: 8) {
                                    let allSelected = configuredRepos.isEmpty
                                    Button {
                                        if var current = draft {
                                            current.githubCiRepositories = []
                                            draft = current
                                        }
                                    } label: {
                                        HStack(spacing: 5) {
                                            LucideIcon(.check, size: 12)
                                                .foregroundStyle((allSelected ? palette.accent : palette.textMuted).color)
                                            Text("全部仓库")
                                                .font(DesignFonts.body(13, weight: .medium))
                                                .foregroundStyle((allSelected ? palette.accent : palette.textSecondary).color)
                                        }
                                        .padding(.horizontal, 12)
                                        .padding(.vertical, 5)
                                        .background(Capsule().fill((allSelected ? palette.accentSoft : palette.surface).color))
                                        .overlay(Capsule().strokeBorder((allSelected ? palette.accentBorder : palette.borderStrong).color, lineWidth: 1))
                                    }
                                    .buttonStyle(PlainHitButtonStyle())

                                    ForEach(filteredCatalog) { option in
                                        let selected = GithubCiDomain.isSelected(option, configured: configuredRepos)
                                        Button {
                                            if var current = draft {
                                                current.githubCiRepositories = GithubCiDomain.toggleSelection(option, configured: configuredRepos)
                                                draft = current
                                            }
                                        } label: {
                                            HStack(spacing: 5) {
                                                LucideIcon(selected ? .check : .plus, size: 12)
                                                    .foregroundStyle((selected ? palette.accent : palette.textMuted).color)
                                                Text(option.fullName)
                                                    .font(DesignFonts.body(13, weight: .medium))
                                                    .foregroundStyle((selected ? palette.accent : palette.textPrimary).color)
                                                    .lineLimit(1)
                                            }
                                            .padding(.horizontal, 12)
                                            .padding(.vertical, 5)
                                            .background(Capsule().fill((selected ? palette.accentSoft : palette.surface).color))
                                            .overlay(Capsule().strokeBorder((selected ? palette.accentBorder : palette.borderStrong).color, lineWidth: 1))
                                        }
                                        .buttonStyle(PlainHitButtonStyle())
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .frame(maxHeight: 180)
                        }
                        .padding(10)
                        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(palette.elevated.color))
                        .overlay(
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .strokeBorder(palette.borderSubtle.color, lineWidth: 1)
                        )
                    }
                }

                if let err = errorMessage ?? snapshot?.error, !err.isEmpty {
                    Text(err)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                }

                if let saveMessage {
                    Text(saveMessage.text)
                        .font(DesignFonts.body(12))
                        .foregroundStyle((saveMessage.tone == .success ? palette.okFg : palette.errFg).color)
                }

                if visibleRuns.isEmpty {
                    Text(busy ? "正在读取 CI" : "暂无工作流运行")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textMuted.color)
                        .padding(.vertical, 6)
                } else {
                    VStack(spacing: 0) {
                        ForEach(Array(visibleRuns.enumerated()), id: \.element.id) { index, run in
                            if index > 0 {
                                Rectangle()
                                    .fill(palette.borderSubtle.color)
                                    .frame(height: 1)
                            }
                            ciRunRow(run)
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
        .task(id: store.machineScope) {
            fetchEpoch &+= 1
            loading = false
            refreshing = false
            snapshot = nil
            errorMessage = nil
            repoQuery = ""
            if externalRules == nil {
                localRules.reset(machineScope: store.machineScope)
            }
            await fetchStatus(forceRefresh: false)
            while !Task.isCancelled {
                let intervalNs: UInt64 = (snapshot?.refreshing == true) ? 2_000_000_000 : 15_000_000_000
                try? await Task.sleep(nanoseconds: intervalNs)
                guard !Task.isCancelled else { break }
                if store.isWindowVisible {
                    await fetchStatus(forceRefresh: false)
                }
            }
        }
    }

    @ViewBuilder
    private func ciRunRow(_ run: GithubCiRun) -> some View {
        let runState = GithubCiDomain.runStatus(run)
        let workflowTitle: String = {
            let primary = (run.name ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            return primary.isEmpty ? "工作流" : primary
        }()
        let shortSha = String(run.headSha.prefix(7))
        let formattedTime = GithubCiDomain.formatRunUpdatedAt(run.updatedAt)

        VStack(alignment: .leading, spacing: 5) {
            HStack(alignment: .top, spacing: 10) {
                HStack(alignment: .top, spacing: 8) {
                    Circle()
                        .fill(statusDotColor(for: runState.tone))
                        .frame(width: 8, height: 8)
                        .padding(.top, 5)

                    if let url = URL(string: run.htmlUrl), !run.htmlUrl.isEmpty {
                        Button {
                            NSWorkspace.shared.open(url)
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                Text(workflowTitle)
                                    .font(DesignFonts.body(14, weight: .semibold))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .multilineTextAlignment(.leading)
                                    .fixedSize(horizontal: false, vertical: true)
                                LucideIcon(.externalLink, size: 13)
                                    .foregroundStyle(palette.textMuted.color)
                            }
                        }
                        .buttonStyle(PlainHitButtonStyle())
                        .help(workflowTitle)
                    } else {
                        Text(workflowTitle)
                            .font(DesignFonts.body(14, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                            .multilineTextAlignment(.leading)
                            .fixedSize(horizontal: false, vertical: true)
                            .help(workflowTitle)
                    }

                    if run.isBot {
                        StatusChip("Bot", tone: .neutral)
                    }
                }
                Spacer(minLength: 8)
                StatusChip(runState.label, tone: runState.tone)
            }

            HStack(spacing: 8) {
                Text(run.repositoryName)
                    .font(DesignFonts.body(13, weight: .medium))
                    .foregroundStyle(palette.textSecondary.color)
                    .lineLimit(1)
                    .help(run.repositoryName)

                HStack(spacing: 4) {
                    LucideIcon(.gitBranch, size: 12)
                        .foregroundStyle(palette.textMuted.color)
                    Text(run.headBranch?.isEmpty == false ? run.headBranch! : "—")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textSecondary.color)
                        .lineLimit(1)
                }
                .padding(.horizontal, 7)
                .padding(.vertical, 1)
                .background(RoundedRectangle(cornerRadius: 6).fill(palette.surface.color))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
                .help(run.headBranch ?? "")

                if !shortSha.isEmpty {
                    Text(shortSha)
                        .font(DesignFonts.mono(11))
                        .foregroundStyle(palette.textSecondary.color)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(RoundedRectangle(cornerRadius: 6).fill(palette.codeBg.color))
                        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
                        .help(run.headSha)
                }

                Spacer(minLength: 4)

                Text(formattedTime)
                    .font(DesignFonts.body(13))
                    .foregroundStyle(palette.textMuted.color)
                    .lineLimit(1)
                    .help(run.updatedAt)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }

    private func statusDotColor(for tone: StatusChipTone) -> Color {
        switch tone {
        case .success: palette.okFg.color
        case .danger: palette.errFg.color
        case .warning: palette.warnFg.color
        default: palette.textMuted.color
        }
    }

    private func addQueryRepo() {
        let q = repoQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty, var current = draft else { return }
        current.githubCiRepositories = GithubCiDomain.addSelection(query: q, catalog: catalog, configured: configuredRepos)
        draft = current
        repoQuery = ""
    }

    private func fetchStatus(forceRefresh: Bool) async {
        let rev = store.revision
        let scope = store.machineScope
        fetchEpoch &+= 1
        let token = fetchEpoch
        if forceRefresh { refreshing = true } else if snapshot == nil { loading = true }
        defer {
            if fetchEpoch == token {
                loading = false
                refreshing = false
            }
        }
        do {
            let args: [String: JSONValue] = [
                "refresh": .bool(forceRefresh),
                "cached_only": .bool(false),
            ]
            let next: GithubCiSnapshot = try await store.rpc("github.ci.status", args: args, at: rev)
            guard store.revision == rev, fetchEpoch == token else { return }
            snapshot = next
            errorMessage = nil
            syncCiIssueSignature(machineScope: scope)
        } catch {
            guard store.revision == rev, fetchEpoch == token else { return }
            errorMessage = error.localizedDescription
            syncCiIssueSignature(machineScope: scope)
        }
    }

    private func syncCiIssueSignature(machineScope: String) {
        let sig = ProbeDisclosureRules.ciIssueSignature(
            errorMessage: errorMessage,
            snapshotError: snapshot?.error,
            visibleRuns: visibleRuns
        )
        var updated = rulesBinding.wrappedValue
        updated.observeIssue(machineScope: machineScope, section: .ci, issueSignature: sig)
        rulesBinding.wrappedValue = updated
    }
}
