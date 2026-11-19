import AppKit
import SwiftUI

public struct MachineButtonView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public var collapsed: Bool
    public let onConfigure: () -> Void

    @State private var localError: String?
    @State private var hovered: Bool = false

    public init(store: RuntimeConnectionStore, collapsed: Bool = false, onConfigure: @escaping () -> Void) {
        self.store = store
        self.collapsed = collapsed
        self.onConfigure = onConfigure
    }

    private var menuEntries: [NativeMenuEntry] {
        var entries: [NativeMenuEntry] = [
            NativeMenuEntry(id: "local", title: "本机") { selectTarget(id: "local") },
        ]
        if !store.machines.isEmpty {
            entries.append(.separator("sep-machines"))
            for machine in store.machines {
                entries.append(NativeMenuEntry(id: machine.id, title: machine.name) {
                    selectTarget(id: machine.id)
                })
            }
        }
        entries.append(.separator("sep-config"))
        entries.append(NativeMenuEntry(id: "config", title: "管理远程机器…") { onConfigure() })
        return entries
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 4) {
                AnchoredMenuButton(entries: menuEntries, accessibilityLabel: "当前机器") {
                    HStack(spacing: 8) {
                        LucideIcon(store.target == .local ? .monitor : .cloud, size: 15)
                            .foregroundStyle(palette.accent.color)
                        if !collapsed {
                            Text(store.machineDisplayLabel)
                                .font(DesignFonts.body(12, weight: .medium))
                                .foregroundStyle(palette.textPrimary.color)
                                .lineLimit(1)
                                .frame(maxWidth: .infinity, alignment: .leading)
                            Circle()
                                .fill((store.target == .local ? palette.okFg : palette.accent).color)
                                .frame(width: 7.5, height: 7.5)
                            LucideIcon(.chevronDown, size: 13)
                                .foregroundStyle(palette.textMuted.color)
                        }
                    }
                    .padding(.horizontal, collapsed ? 0 : 10)
                    .frame(
                        maxWidth: collapsed ? 36 : .infinity,
                        minHeight: 34,
                        maxHeight: 34
                    )
                    .background(
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .fill((hovered ? palette.hover : palette.elevated).color)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .strokeBorder((hovered ? palette.accentBorder : palette.borderStrong).color, lineWidth: 1)
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())
                .onHover { hovered = $0 }
                .disabled(store.isBusyForSwitch)
                .help(store.machineDisplayLabel)
                .accessibilityLabel("当前机器")

                if store.target == .remote {
                    Button {
                        selectTarget(id: store.activeMachineId ?? "")
                    } label: {
                        LucideIcon(.refreshCw, size: 15)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 32))
                    .disabled(store.isBusyForSwitch)
                    .help("重新连接")
                    .accessibilityLabel("重新连接当前机器")
                }
            }

            if let localError, !localError.isEmpty {
                Text(localError)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }
        }
    }

    private func selectTarget(id: String) {
        localError = nil
        Task { @MainActor in
            do {
                if id == "local" {
                    try await store.selectMachine(target: .local)
                } else {
                    try await store.selectMachine(target: .remote, machineId: id)
                }
            } catch {
                localError = error.localizedDescription
            }
        }
    }
}

public struct RemoteConnectionSettingsView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore

    @State private var hosts: [String] = []
    @State private var sshHost: String = ""
    @State private var name: String = ""
    @State private var useSudo: Bool = true
    @State private var editingId: String?
    @State private var errorMessage: String?
    @State private var noticeMessage: String?
    @State private var installing: Bool = false
    @State private var deletingAlias: String?

    public init(store: RuntimeConnectionStore) {
        self.store = store
    }

    private var busy: Bool {
        store.isBusyForSwitch || installing
    }

    private var mergedHostOptions: [String] {
        var seen = Set<String>()
        var result: [String] = []
        for h in hosts + (sshHost.isEmpty ? [] : [sshHost]) where !h.isEmpty {
            if seen.insert(h).inserted {
                result.append(h)
            }
        }
        return result
    }

    public var body: some View {
        ZStack {
            VStack(alignment: .leading, spacing: 16) {
                if !store.machines.isEmpty {
                    PanelCard(title: "远程机器") {
                        VStack(alignment: .leading, spacing: 10) {
                            ForEach(store.machines) { machine in
                                VStack(alignment: .leading, spacing: 10) {
                                    HStack(alignment: .top, spacing: 12) {
                                        VStack(alignment: .leading, spacing: 2) {
                                            Text(machine.name)
                                                .font(DesignFonts.body(14, weight: .semibold))
                                                .foregroundStyle(palette.textPrimary.color)
                                            if machine.name != machine.sshHost {
                                                Text(machine.sshHost)
                                                    .font(DesignFonts.mono(12))
                                                    .foregroundStyle(palette.textMuted.color)
                                            }
                                        }
                                        Spacer(minLength: 8)
                                    }

                                    FlowButtonRow(spacing: 8, lineSpacing: 8) {
                                        Button(
                                            store.target == .remote && store.activeMachineId == machine.id
                                                ? "重新连接" : "连接"
                                        ) {
                                            runAction {
                                                try await store.selectMachine(target: .remote, machineId: machine.id)
                                            }
                                        }
                                        .buttonStyle(SecondaryButtonStyle())
                                        .disabled(busy)

                                        Button("编辑") {
                                            editingId = machine.id
                                            sshHost = machine.sshHost
                                            name = machine.name
                                            useSudo = machine.useSudo
                                            errorMessage = nil
                                            noticeMessage = nil
                                        }
                                        .buttonStyle(SecondaryButtonStyle())
                                        .disabled(busy)

                                        Button {
                                            runAction {
                                                _ = try await store.removeRemoteMachine(machineId: machine.id)
                                                if editingId == machine.id {
                                                    editingId = nil
                                                    name = ""
                                                }
                                            }
                                        } label: {
                                            HStack(spacing: 4) {
                                                LucideIcon(.trash2, size: 15)
                                                Text("移除")
                                            }
                                        }
                                        .buttonStyle(DangerButtonStyle(soft: true))
                                        .disabled(busy)
                                    }
                                }
                                .padding(12)
                                .background(
                                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                                        .fill(palette.elevated.color)
                                )
                                .overlay(
                                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                                        .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                                )
                            }
                        }
                    }
                }

                PanelCard(title: editingId != nil ? "编辑连接" : "新增 SSH 连接") {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(alignment: .top, spacing: 12) {
                            VStack(alignment: .leading, spacing: 6) {
                                Text("SSH 连接")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textMuted.color)
                                Menu {
                                    Button("选择已保存的连接") {
                                        sshHost = ""
                                        errorMessage = nil
                                        noticeMessage = nil
                                    }
                                    ForEach(mergedHostOptions, id: \.self) { host in
                                        Button(host) {
                                            sshHost = host
                                            errorMessage = nil
                                            noticeMessage = nil
                                        }
                                    }
                                } label: {
                                    HStack {
                                        Text(sshHost.isEmpty ? "选择已保存的连接" : sshHost)
                                            .font(DesignFonts.body(13))
                                            .foregroundStyle(
                                                (sshHost.isEmpty ? palette.textMuted : palette.textPrimary).color
                                            )
                                            .lineLimit(1)
                                        Spacer()
                                        LucideIcon(.chevronDown, size: 15)
                                            .foregroundStyle(palette.textMuted.color)
                                    }
                                    .padding(.horizontal, 10)
                                    .frame(height: LayoutMetrics.formControlHeight)
                                    .background(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .fill(palette.elevated.color)
                                    )
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                                    )
                                    .contentShape(Rectangle())
                                }
                                .menuStyle(.borderlessButton)
                                .menuIndicator(.hidden)
                                .disabled(busy)
                            }
                            .frame(maxWidth: .infinity)

                            VStack(alignment: .leading, spacing: 6) {
                                Text("名称")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textMuted.color)
                                TextField(sshHost.isEmpty ? "远程机器名称" : sshHost, text: $name)
                                    .textFieldStyle(.plain)
                                    .font(DesignFonts.body(13))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .padding(.horizontal, 10)
                                    .frame(height: LayoutMetrics.formControlHeight)
                                    .background(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .fill(palette.elevated.color)
                                    )
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                                    )
                                    .disabled(busy)
                            }
                            .frame(maxWidth: .infinity)
                        }

                        HStack {
                            Text("使用 sudo")
                                .font(DesignFonts.body(13))
                                .foregroundStyle(palette.textPrimary.color)
                            Spacer()
                            CapsuleSwitch(isOn: $useSudo, label: "使用 sudo")
                                .disabled(busy)
                        }
                        .padding(.top, 8)

                        FlowButtonRow(spacing: 8, lineSpacing: 8) {
                            Button("保存并连接") {
                                submit(install: false)
                            }
                            .buttonStyle(PrimaryButtonStyle())
                            .disabled(sshHost.isEmpty || busy)

                            Button(installing ? "正在安装…" : "安装并连接") {
                                submit(install: true)
                            }
                            .buttonStyle(SecondaryButtonStyle())
                            .disabled(sshHost.isEmpty || busy)

                            Button {
                                Task { await reloadHosts() }
                            } label: {
                                HStack(spacing: 4) {
                                    LucideIcon(.refreshCw, size: 15)
                                    Text("刷新 SSH 配置")
                                }
                            }
                            .buttonStyle(SecondaryButtonStyle())
                            .disabled(busy)

                            Button {
                                errorMessage = nil
                                deletingAlias = sshHost
                            } label: {
                                HStack(spacing: 4) {
                                    LucideIcon(.trash2, size: 15)
                                    Text("删除 SSH 配置")
                                }
                            }
                            .buttonStyle(DangerButtonStyle(soft: true))
                            .disabled(sshHost.isEmpty || busy)

                            if editingId != nil {
                                Button("新增连接") {
                                    editingId = nil
                                    name = ""
                                }
                                .buttonStyle(SecondaryButtonStyle())
                                .disabled(busy)
                            }
                        }

                        if let errorMessage, !errorMessage.isEmpty {
                            Text(errorMessage)
                                .font(DesignFonts.body(13))
                                .foregroundStyle(palette.errFg.color)
                        }
                        if let noticeMessage, !noticeMessage.isEmpty {
                            Text(noticeMessage)
                                .font(DesignFonts.body(13))
                                .foregroundStyle(palette.okFg.color)
                        }
                    }
                }
            }

            if let alias = deletingAlias {
                ConfirmDialogOverlay(
                    title: "删除 SSH 配置",
                    busy: busy,
                    onCancel: { deletingAlias = nil }
                ) {
                    Text("删除“\(alias)”及其在 AgentHub 中的连接记录？")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textSecondary.color)
                    if let errorMessage, !errorMessage.isEmpty {
                        Text(errorMessage)
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.errFg.color)
                    }
                    HStack(spacing: 8) {
                        Spacer()
                        Button("取消") {
                            deletingAlias = nil
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(busy)

                        Button {
                            runAction {
                                _ = try await store.deleteSshHost(alias: alias)
                                deletingAlias = nil
                                editingId = nil
                                name = ""
                                sshHost = ""
                                await reloadHosts()
                                noticeMessage = "SSH 配置已删除"
                            }
                        } label: {
                            HStack(spacing: 4) {
                                LucideIcon(.trash2, size: 15)
                                Text("确认删除")
                            }
                        }
                        .buttonStyle(DangerButtonStyle(soft: false))
                        .disabled(busy)
                    }
                }
            }
        }
        .task {
            await reloadHosts()
        }
    }

    private func reloadHosts() async {
        do {
            let loaded = try await store.listSshHosts()
            hosts = loaded
            if !loaded.contains(sshHost) {
                sshHost = loaded.first ?? ""
            }
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func submit(install: Bool) {
        runAction {
            let creds = RemoteConnectionCredentials(
                revision: store.revision,
                sshHost: sshHost,
                useSudo: useSudo,
                id: editingId,
                name: name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? sshHost : name
            )
            installing = install
            defer { installing = false }
            let view = install
                ? try await store.installRemoteMachine(creds)
                : try await store.saveRemoteMachine(creds)
            editingId = view.activeMachineId
            noticeMessage = "已连接"
        }
    }

    private func runAction(_ work: @escaping @MainActor () async throws -> Void) {
        errorMessage = nil
        noticeMessage = nil
        Task { @MainActor in
            do {
                try await work()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }
}
