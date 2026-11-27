import AppKit
import SwiftUI

extension Notification.Name {
    static let agentHubSelectAllRequested = Notification.Name("AgentHubSelectAllRequested")
    static let agentHubSearchRequested = Notification.Name("AgentHubSearchRequested")
}

struct AppShellView: View {
    @ObservedObject var store: RuntimeConnectionStore
    @Environment(\.colorScheme) private var systemColorScheme

    init(store: RuntimeConnectionStore) {
        self.store = store
    }

    var effectiveScheme: ColorSchemeKind {
        switch store.themeMode {
        case .dark: .dark
        case .light: .light
        case .system: (systemColorScheme == .dark) ? .dark : .light
        }
    }

    var palette: DesignPalette {
        DesignPalette.resolve(scheme: effectiveScheme, workspace: store.activeWorkspace)
    }

    var body: some View {
        desktopLayout(palette: palette)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.background.color)
            .environment(\.agentHubPalette, palette)
    }

    @ViewBuilder
    private func desktopLayout(palette: DesignPalette) -> some View {
        let navWidth = store.navCollapsed
            ? LayoutMetrics.sideNavCollapsedWidth
            : LayoutMetrics.sideNavExpandedWidth

        HStack(spacing: 0) {
            SideNavigationRailView(
                store: store,
                collapsed: store.navCollapsed,
                onToggleCollapse: { store.toggleNavCollapsed() },
                onConfigureRemote: {
                    store.activeWorkspace = .ops
                    store.settingsSection = .remote
                }
            )
            .frame(width: navWidth)
            .frame(maxHeight: .infinity)
            .background(palette.railBg.color)
            .overlay(alignment: .trailing) {
                Rectangle()
                    .fill(palette.borderSubtle.color)
                    .frame(width: 1)
            }

            VStack(spacing: 0) {
                errorBannerIfNeeded(palette: palette)
                workspaceStage
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.background.color)
        }
    }

    @ViewBuilder
    private func errorBannerIfNeeded(palette: DesignPalette) -> some View {
        if let err = store.connectionError ?? store.capabilitiesError, !err.isEmpty {
            HStack(spacing: 8) {
                LucideIcon(.triangleAlert, size: 15)
                    .foregroundStyle(palette.errFg.color)
                Text(err)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
                    .lineLimit(2)
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .background(palette.errBg.color)
        }
    }

    @ViewBuilder
    private var workspaceStage: some View {
        if store.ready {
            workspaceContent
                .id(store.revision)
                .disabled(store.changing)
        }
    }

    @ViewBuilder
    private var workspaceContent: some View {
        switch store.activeWorkspace {
        case .workspaces:
            UnifiedWorkspaceHubView(
                store: store,
                scheme: effectiveScheme
            )
            .id(WorkspaceId.workspaces)
        case .probe:
            ProbeWorkspaceView(store: store)
        case .ops:
            SettingsWorkspaceView(store: store)
        case .codex, .claude, .grok, .grokbot, .antigravity, .dsh:
            UnifiedProviderWorkspaceView(
                store: store,
                workspace: store.activeWorkspace,
                scheme: effectiveScheme
            )
            .id(store.activeWorkspace)
        }
    }
}

struct SideNavigationRailView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject var store: RuntimeConnectionStore
    var collapsed: Bool
    let onToggleCollapse: () -> Void
    let onConfigureRemote: () -> Void

    var body: some View {
        let items = store.capabilities.visibleWorkspaces
        VStack(alignment: collapsed ? .center : .leading, spacing: 6) {
            HStack(spacing: 8) {
                if !collapsed {
                    AgentHubLogoView(size: 22)
                    Text("AgentHub")
                        .font(DesignFonts.body(14, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                }
                Button(action: onToggleCollapse) {
                    LucideIcon(collapsed ? .panelLeftOpen : .panelLeftClose, size: 17)
                }
                .buttonStyle(IconButtonStyle(dimension: 32))
                .help(collapsed ? "展开导航" : "折叠导航")
            }
            .padding(.horizontal, collapsed ? 0 : 4)
            .padding(.top, 2)
            .padding(.bottom, 6)
            .frame(minHeight: 34)

            MachineButtonView(
                store: store,
                collapsed: collapsed,
                onConfigure: onConfigureRemote
            )
            .padding(.horizontal, collapsed ? 0 : 2)
            .padding(.bottom, 6)

            VStack(spacing: 3) {
                ForEach(items, id: \.self) { item in
                    if item == .codex || item == .probe {
                        Rectangle()
                            .fill(palette.borderSubtle.color)
                            .frame(height: 1)
                            .padding(.vertical, 4)
                            .padding(.horizontal, 6)
                    }
                    let active = (store.activeWorkspace == item)
                    SideNavRowButton(
                        item: item,
                        active: active,
                        collapsed: collapsed,
                        palette: palette
                    ) {
                        store.activeWorkspace = item
                    }
                }
            }

            Spacer(minLength: 0)
        }
        .padding(.horizontal, collapsed ? 6 : 10)
        .padding(.vertical, 12)
    }
}

private struct SideNavRowButton: View {
    let item: WorkspaceId
    let active: Bool
    let collapsed: Bool
    let palette: DesignPalette
    let action: () -> Void

    @State private var hovered: Bool = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(palette.accentSoft.color)
                        .frame(width: 24, height: 24)
                    navBadgeIcon(item)
                        .foregroundStyle(palette.accent.color)
                }
                .frame(width: 24, height: 24)

                if !collapsed {
                    Text(item.label)
                        .font(DesignFonts.body(13, weight: active ? .semibold : .medium))
                        .foregroundStyle((active || hovered ? palette.textPrimary : palette.textMuted).color)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, collapsed ? 0 : 9)
            .padding(.vertical, 6)
            .frame(maxWidth: collapsed ? 36 : .infinity, minHeight: 36, alignment: collapsed ? .center : .leading)
            .background(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(active ? palette.selected.color : (hovered ? palette.hover.color : Color.clear))
            )
            .overlay(alignment: .leading) {
                if active {
                    UnevenLeadingAccentBar(color: palette.accent.color, width: 2.5, cornerRadius: 8)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
        .onHover { hovered = $0 }
        .help(item.label)
        .accessibilityLabel(item.label)
    }

    @ViewBuilder
    private func navBadgeIcon(_ id: WorkspaceId) -> some View {
        switch id {
        case .workspaces: LucideIcon(.folderGit2, size: 16)
        case .codex: AgentBrandIcon(.codex, size: 16)
        case .claude: AgentBrandIcon(.claude, size: 16)
        case .grok: AgentBrandIcon(.grok, size: 16)
        case .grokbot: AgentBrandIcon(.grokbot, size: 16)
        case .antigravity: AgentBrandIcon(.antigravity, size: 16)
        case .dsh: AgentBrandIcon(.dsh, size: 16)
        case .probe: LucideIcon(.activity, size: 16)
        case .ops: LucideIcon(.slidersHorizontal, size: 16)
        }
    }
}

struct UnevenLeadingAccentBar: View {
    let color: Color
    var width: CGFloat
    var cornerRadius: CGFloat

    init(color: Color, width: CGFloat = 2.5, cornerRadius: CGFloat = 8) {
        self.color = color
        self.width = width
        self.cornerRadius = cornerRadius
    }

    var body: some View {
        HStack(spacing: 0) {
            Rectangle()
                .fill(color)
                .frame(width: width)
            Spacer(minLength: 0)
        }
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        .allowsHitTesting(false)
    }
}
