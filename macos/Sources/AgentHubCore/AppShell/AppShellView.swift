import AppKit
import SwiftUI

public extension Notification.Name {
    static let agentHubSelectAllRequested = Notification.Name("AgentHubSelectAllRequested")
    static let agentHubSearchRequested = Notification.Name("AgentHubSearchRequested")
    static let agentHubOpenMobileThreadsRequested = Notification.Name("AgentHubOpenMobileThreadsRequested")
}

public struct AppShellView: View {
    @ObservedObject public var store: RuntimeConnectionStore
    @Environment(\.colorScheme) private var systemColorScheme
    public var providerContentBuilder: (@MainActor (WorkspaceId, RuntimeConnectionStore, Bool) -> AnyView)?

    public init(
        store: RuntimeConnectionStore,
        providerContentBuilder: (@MainActor (WorkspaceId, RuntimeConnectionStore, Bool) -> AnyView)? = nil
    ) {
        self.store = store
        self.providerContentBuilder = providerContentBuilder
    }

    public var effectiveScheme: ColorSchemeKind {
        switch store.themeMode {
        case .dark: .dark
        case .light: .light
        case .system: (systemColorScheme == .dark) ? .dark : .light
        }
    }

    public var palette: DesignPalette {
        DesignPalette.resolve(scheme: effectiveScheme, workspace: store.activeWorkspace)
    }

    public var body: some View {
        GeometryReader { proxy in
            let isCompactMobile = proxy.size.width <= LayoutMetrics.mobileBreakpoint
            Group {
                if isCompactMobile {
                    mobileLayout(palette: palette)
                } else {
                    desktopLayout(palette: palette)
                }
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
            .background(palette.background.color)
            .environment(\.agentHubPalette, palette)
        }
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
                workspaceStage(isCompactMobile: false)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.background.color)
        }
    }

    @ViewBuilder
    private func mobileLayout(palette: DesignPalette) -> some View {
        let items = store.capabilities.visibleWorkspaces
        VStack(spacing: 0) {
            // 52pt Mobile TopBar
            HStack(spacing: 10) {
                Button {
                    NotificationCenter.default.post(name: .agentHubOpenMobileThreadsRequested, object: nil)
                } label: {
                    LucideIcon(.menu, size: 20)
                }
                .buttonStyle(IconButtonStyle(dimension: 32))
                .disabled(store.activeWorkspace != .codex)
                .help(store.activeWorkspace == .codex ? "打开线程" : "线程列表仅用于 Codex")
                .accessibilityLabel("打开线程")

                Spacer()
                Text(store.activeWorkspace.label)
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                Spacer()
                Circle()
                    .fill(palette.accent.color)
                    .frame(width: 8, height: 8)
                    .background(
                        Circle()
                            .fill(palette.accentSoft.color)
                            .frame(width: 14, height: 14)
                    )
                    .padding(.trailing, 8)
            }
            .padding(.horizontal, 12)
            .frame(height: LayoutMetrics.mobileTopBarHeight)
            .background(palette.railBg.color)
            .overlay(alignment: .bottom) {
                Rectangle().fill(palette.borderStrong.color).frame(height: 1)
            }

            errorBannerIfNeeded(palette: palette)

            workspaceStage(isCompactMobile: true)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

            // 58pt Mobile Bottom Tabs (active tab: --sl fill + 2pt --a top inset bar per styles.css:921)
            HStack(spacing: 0) {
                ForEach(items, id: \.self) { id in
                    let active = (store.activeWorkspace == id)
                    Button {
                        store.activeWorkspace = id
                    } label: {
                        VStack(spacing: 3) {
                            workspaceIcon(id, size: 17)
                                .foregroundStyle((active ? palette.textPrimary : palette.textMuted).color)
                            Text(id.label)
                                .font(DesignFonts.body(10, weight: active ? .semibold : .medium))
                                .foregroundStyle((active ? palette.textPrimary : palette.textMuted).color)
                                .lineLimit(1)
                                .minimumScaleFactor(0.75)
                        }
                        .padding(.horizontal, 2)
                        .padding(.vertical, 4)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(active ? palette.selected.color : Color.clear)
                        .overlay(alignment: .top) {
                            if active {
                                Rectangle()
                                    .fill(palette.accent.color)
                                    .frame(height: 2)
                            }
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(PlainHitButtonStyle())
                }
            }
            .frame(height: LayoutMetrics.mobileBottomTabsHeight)
            .background(palette.railBg.color)
            .overlay(alignment: .top) {
                Rectangle().fill(palette.borderStrong.color).frame(height: 1)
            }
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
    private func workspaceStage(isCompactMobile: Bool) -> some View {
        if store.ready {
            workspaceContent(isCompactMobile: isCompactMobile)
                .id(store.revision)
                .disabled(store.changing)
        }
    }

    @ViewBuilder
    private func workspaceContent(isCompactMobile: Bool) -> some View {
        switch store.activeWorkspace {
        case .probe:
            ProbeWorkspaceView(store: store)
        case .ops:
            SettingsWorkspaceView(store: store)
        case .codex, .claude, .grok, .grokbot, .antigravity, .dsh:
            if let builder = providerContentBuilder {
                builder(store.activeWorkspace, store, isCompactMobile)
                    .id(store.activeWorkspace)
            } else {
                UnifiedProviderWorkspaceView(
                    store: store,
                    workspace: store.activeWorkspace,
                    isCompactMobile: isCompactMobile,
                    scheme: effectiveScheme,
                    codexNewThreadSlot: { onCreated, onErrorChanged in
                        AnyView(
                            NewDesktopThreadButton(
                                connection: store,
                                onCreated: onCreated,
                                onErrorChanged: onErrorChanged
                            )
                        )
                    },
                    codexCollaborationFooterSlot: { threadId, _, availWidth, availHeight, onStreamUpdated in
                        AnyView(
                            DesktopCollaborationFooterView(
                                connection: store,
                                threadId: threadId,
                                availableWidth: availWidth,
                                viewportHeight: availHeight,
                                onStreamUpdated: onStreamUpdated
                            )
                            .id("\(store.revision):\(threadId)")
                        )
                    }
                )
                .id(store.activeWorkspace)
            }
        }
    }

    @ViewBuilder
    fileprivate func workspaceIcon(_ id: WorkspaceId, size: CGFloat) -> some View {
        switch id {
        case .codex: AgentBrandIcon(.codex, size: size)
        case .claude: AgentBrandIcon(.claude, size: size)
        case .grok: AgentBrandIcon(.grok, size: size)
        case .grokbot: AgentBrandIcon(.grokbot, size: size)
        case .antigravity: AgentBrandIcon(.antigravity, size: size)
        case .dsh: AgentBrandIcon(.dsh, size: size)
        case .probe: LucideIcon(.activity, size: size)
        case .ops: LucideIcon(.slidersHorizontal, size: size)
        }
    }
}

public struct SideNavigationRailView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public var collapsed: Bool
    public let onToggleCollapse: () -> Void
    public let onConfigureRemote: () -> Void

    public var body: some View {
        let items = store.capabilities.visibleWorkspaces
        VStack(alignment: collapsed ? .center : .leading, spacing: 6) {
            // Brand Row (.nav-brand: min-height 34px, padding 2px 4px 6px, logo 22x22px)
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
                    if item == .probe {
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
                // .nav-icon-badge: 24x24pt rounded-8, background --as, color --a (styles.css:222-225)
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

public struct UnevenLeadingAccentBar: View {
    public let color: Color
    public var width: CGFloat
    public var cornerRadius: CGFloat

    public init(color: Color, width: CGFloat = 2.5, cornerRadius: CGFloat = 8) {
        self.color = color
        self.width = width
        self.cornerRadius = cornerRadius
    }

    public var body: some View {
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

struct DefaultProviderPlaceholderView: View {
    @Environment(\.agentHubPalette) private var palette
    let workspace: WorkspaceId
    let isCompactMobile: Bool

    var body: some View {
        HStack(spacing: 0) {
            if !isCompactMobile {
                VStack(alignment: .leading, spacing: 10) {
                    Text(workspace.label)
                        .font(DesignFonts.body(14, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                    Text("正在读取…")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textMuted.color)
                    Spacer()
                }
                .padding(14)
                .frame(width: LayoutMetrics.sessionListWidth)
                .frame(maxHeight: .infinity)
                .background(palette.surface.color)
                .overlay(alignment: .trailing) {
                    Rectangle().fill(palette.borderSubtle.color).frame(width: 1)
                }
            }
            VStack {
                Spacer()
                Text("选择左侧会话查看对话详情")
                    .font(DesignFonts.body(13))
                    .foregroundStyle(palette.textMuted.color)
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.background.color)
        }
    }
}
