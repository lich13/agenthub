import AppKit
import SwiftUI

public struct GithubCiCardView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    @Binding public var draft: ProbeSettingsDraft?
    public var saving: Bool
    public var saveMessage: (tone: StatusChipTone, text: String)?
    public let onSave: () -> Void

    @State private var snapshot: GithubCiSnapshot?
    @State private var loading: Bool = false
    @State private var refreshing: Bool = false
    @State private var errorMessage: String?
    @State private var repoQuery: String = ""

    private var notifyCiOn: Bool {
        draft?.notifyGithubCi ?? false
    }

    private var filterBotsOn: Bool {
        draft?.githubCiFilterBots ?? true
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

    public init(
        store: RuntimeConnectionStore,
        draft: Binding<ProbeSettingsDraft?>,
        saving: Bool,
        saveMessage: (tone: StatusChipTone, text: String)?,
        onSave: @escaping () -> Void
    ) {
        self.store = store
        self._draft = draft
        self.saving = saving
        self.saveMessage = saveMessage
        self.onSave = onSave
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
            filterBots: filterBotsOn
        )
    }

    public var body: some View {
        PanelCard(
            title: "GitHub CI",
            icon: .github,
            headerAction: {
                FlowButtonRow(spacing: 8, lineSpacing: 6) {
                    let accountText: String = if let login = snapshot?.account?.login {
                        "@\(login)"
                    } else if busy {
                        "读取账号中"
                    } else if !(draft?.notifyGithubCi ?? false) && errorMessage == nil && snapshot?.error == nil {
                        "待启用扫描"
                    } else {
                        "未登录"
                    }
                    StatusChip(accountText, tone: snapshot?.account != nil ? .info : .neutral)
                        .help(snapshot?.account?.login ?? accountText)
                    StatusChip("\(visibleRuns.count) 次运行", tone: .neutral)

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
                        HStack(spacing: 6) {
                            LucideIcon(.check, size: 15)
                            Text(saving ? "保存中" : "保存")
                        }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(draft == nil || saving)
                }
            }
        ) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 12) {
                    HStack(spacing: 10) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .fill((notifyCiOn ? palette.accentSoft : palette.surface).color)
                                .frame(width: 28, height: 28)
                            LucideIcon(.bell, size: 15)
                                .foregroundStyle((notifyCiOn ? palette.accent : palette.textMuted).color)
                        }
                        Text("CI 监控通知")
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                        Spacer(minLength: 6)
                        StatusChip(notifyCiOn ? "已开启" : "已关闭", tone: notifyCiOn ? .info : .neutral)
                        CapsuleSwitch(isOn: notifyCiBinding, label: "CI 监控通知")
                            .disabled(draft == nil || saving)
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10)
                            .strokeBorder((notifyCiOn ? palette.accentBorder : palette.borderSubtle).color, lineWidth: 1)
                    )

                    HStack(spacing: 10) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .fill((filterBotsOn ? palette.accentSoft : palette.surface).color)
                                .frame(width: 28, height: 28)
                            LucideIcon(.bot, size: 15)
                                .foregroundStyle((filterBotsOn ? palette.accent : palette.textMuted).color)
                        }
                        Text("过滤 Bot")
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                        Spacer(minLength: 6)
                        StatusChip(filterBotsOn ? "已开启" : "已关闭", tone: filterBotsOn ? .info : .neutral)
                        CapsuleSwitch(isOn: filterBotsBinding, label: "过滤 Bot")
                            .disabled(draft == nil || saving)
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10)
                            .strokeBorder((filterBotsOn ? palette.accentBorder : palette.borderSubtle).color, lineWidth: 1)
                    )
                }

                // Repository scope card matching WebUI .ci-repo-card
                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 8) {
                        LucideIcon(.folderGit2, size: 14)
                            .foregroundStyle(palette.accent.color)
                        Text("监控仓库范围")
                            .font(DesignFonts.body(13, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                        StatusChip(
                            configuredRepos.isEmpty ? "监控全部仓库" : "已选 \(configuredRepos.count) 个仓库",
                            tone: configuredRepos.isEmpty ? .neutral : .info
                        )
                        Spacer(minLength: 8)

                        HStack(spacing: 6) {
                            LucideIcon(.search, size: 13)
                                .foregroundStyle(palette.textMuted.color)
                            TextField("搜索或输入 owner/repo", text: $repoQuery)
                                .textFieldStyle(.plain)
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.textPrimary.color)
                                .onSubmit { addQueryRepo() }
                        }
                        .padding(.horizontal, 10)
                        .frame(width: 230, height: 32)
                        .background(RoundedRectangle(cornerRadius: 8).fill(palette.surface.color))
                        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
                    }

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
                                    .font(DesignFonts.body(12, weight: .medium))
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
                                        .font(DesignFonts.body(12, weight: .medium))
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
                }
                .padding(12)
                .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderSubtle.color, lineWidth: 1))

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
                        .padding(.vertical, 8)
                } else {
                    VStack(spacing: 8) {
                        ForEach(visibleRuns, id: \.id) { run in
                            let runState = GithubCiDomain.runStatus(run)
                            let workflowTitle = {
                                let primary = (run.name ?? run.workflowName).trimmingCharacters(in: .whitespacesAndNewlines)
                                return primary.isEmpty ? "工作流" : primary
                            }()
                            let shortSha = String(run.headSha.prefix(7))
                            let formattedTime = GithubCiDomain.formatRunUpdatedAt(run.updatedAt)

                            VStack(alignment: .leading, spacing: 6) {
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
                                        .font(DesignFonts.body(12, weight: .medium))
                                        .foregroundStyle(palette.textSecondary.color)
                                        .lineLimit(1)
                                        .help(run.repositoryName)

                                    HStack(spacing: 4) {
                                        LucideIcon(.gitBranch, size: 12)
                                            .foregroundStyle(palette.textMuted.color)
                                        Text(run.headBranch?.isEmpty == false ? run.headBranch! : "—")
                                            .font(DesignFonts.body(12))
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
                                        .font(DesignFonts.body(12))
                                        .foregroundStyle(palette.textMuted.color)
                                        .lineLimit(1)
                                        .help(run.updatedAt)
                                }
                            }
                            .padding(.horizontal, 14)
                            .padding(.vertical, 11)
                            .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
                        }
                    }
                }
            }
        }
        .task(id: store.machineScope) {
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
        if forceRefresh { refreshing = true } else if snapshot == nil { loading = true }
        defer {
            loading = false
            refreshing = false
        }
        do {
            let args: [String: JSONValue] = [
                "refresh": .bool(forceRefresh),
                "cached_only": .bool(false),
            ]
            let next: GithubCiSnapshot = try await store.rpc("github.ci.status", args: args)
            snapshot = next
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
