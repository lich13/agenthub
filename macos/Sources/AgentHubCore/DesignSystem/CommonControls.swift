import AppKit
import SwiftUI

// MARK: - Full-Rectangle Hit-Target Button Styles (Never bare .plain)

struct PrimaryButtonStyle: ButtonStyle {
    init() {}

    func makeBody(configuration: Configuration) -> some View {
        InnerBody(configuration: configuration)
    }

    private struct InnerBody: View {
        @Environment(\.agentHubPalette) private var palette
        @Environment(\.isEnabled) private var isEnabled
        let configuration: Configuration

        var body: some View {
            configuration.label
                .font(DesignFonts.body(13, weight: .medium))
                .foregroundStyle(palette.buttonFg.color)
                .padding(.horizontal, 12)
                .frame(minHeight: LayoutMetrics.formControlHeight)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill((configuration.isPressed ? palette.buttonHover : palette.buttonBg).color)
                )
                .contentShape(Rectangle())
                .opacity(isEnabled ? 1.0 : 0.45)
        }
    }
}

struct SecondaryButtonStyle: ButtonStyle {
    init() {}

    func makeBody(configuration: Configuration) -> some View {
        InnerBody(configuration: configuration)
    }

    private struct InnerBody: View {
        @Environment(\.agentHubPalette) private var palette
        @Environment(\.isEnabled) private var isEnabled
        let configuration: Configuration

        var body: some View {
            configuration.label
                .font(DesignFonts.body(13, weight: .medium))
                .foregroundStyle(palette.textPrimary.color)
                .padding(.horizontal, 12)
                .frame(minHeight: LayoutMetrics.formControlHeight)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill((configuration.isPressed ? palette.hover : palette.elevated).color)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                )
                .contentShape(Rectangle())
                .opacity(isEnabled ? 1.0 : 0.45)
        }
    }
}

struct DangerButtonStyle: ButtonStyle {
    var soft: Bool

    init(soft: Bool = false) {
        self.soft = soft
    }

    func makeBody(configuration: Configuration) -> some View {
        InnerBody(configuration: configuration, soft: soft)
    }

    private struct InnerBody: View {
        @Environment(\.agentHubPalette) private var palette
        @Environment(\.isEnabled) private var isEnabled
        let configuration: Configuration
        let soft: Bool

        var body: some View {
            let fg = soft ? palette.errFg : palette.buttonFg
            let bg = soft ? palette.errBg : palette.errFg
            let border = soft ? palette.errBorder : palette.errFg
            return configuration.label
                .font(DesignFonts.body(13, weight: .medium))
                .foregroundStyle(fg.color)
                .padding(.horizontal, 12)
                .frame(minHeight: LayoutMetrics.formControlHeight)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(bg.color.opacity(configuration.isPressed ? 0.85 : 1.0))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(border.color, lineWidth: 1)
                )
                .contentShape(Rectangle())
                .opacity(isEnabled ? 1.0 : 0.45)
        }
    }
}

struct IconButtonStyle: ButtonStyle {
    var dimension: CGFloat

    init(dimension: CGFloat = 32) {
        self.dimension = dimension
    }

    func makeBody(configuration: Configuration) -> some View {
        InnerBody(configuration: configuration, dimension: dimension)
    }

    private struct InnerBody: View {
        @Environment(\.agentHubPalette) private var palette
        @Environment(\.isEnabled) private var isEnabled
        let configuration: Configuration
        let dimension: CGFloat

        var body: some View {
            configuration.label
                .foregroundStyle((configuration.isPressed ? palette.textPrimary : palette.textMuted).color)
                .frame(width: dimension, height: dimension)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(configuration.isPressed ? palette.hover.color : Color.clear)
                )
                .contentShape(Rectangle())
                .opacity(isEnabled ? 1.0 : 0.45)
        }
    }
}

struct FullWidthRowButtonStyle: ButtonStyle {
    var selected: Bool
    var multiSelected: Bool
    var cornerRadius: CGFloat

    init(selected: Bool = false, multiSelected: Bool = false, cornerRadius: CGFloat = 8) {
        self.selected = selected
        self.multiSelected = multiSelected
        self.cornerRadius = cornerRadius
    }

    func makeBody(configuration: Configuration) -> some View {
        InnerBody(
            configuration: configuration,
            selected: selected,
            multiSelected: multiSelected,
            cornerRadius: cornerRadius
        )
    }

    private struct InnerBody: View {
        @Environment(\.agentHubPalette) private var palette
        @Environment(\.isEnabled) private var isEnabled
        let configuration: Configuration
        let selected: Bool
        let multiSelected: Bool
        let cornerRadius: CGFloat

        var body: some View {
            let fillColor: Color = if multiSelected {
                palette.accentSoft.color
            } else if selected {
                palette.selected.color
            } else if configuration.isPressed {
                palette.hover.color
            } else {
                Color.clear
            }

            configuration.label
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(
                    RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                        .fill(fillColor)
                )
                .overlay {
                    if multiSelected {
                        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                            .strokeBorder(palette.accent.color, lineWidth: 1.5)
                    }
                }
                .overlay(alignment: .leading) {
                    if selected && !multiSelected {
                        RoundedRectangle(cornerRadius: 1.5)
                            .fill(palette.accent.color)
                            .frame(width: 2.5)
                            .padding(.vertical, 4)
                    }
                }
                .contentShape(Rectangle())
                .opacity(isEnabled ? 1.0 : 0.45)
        }
    }
}

// MARK: - 38x22pt Capsule Switch

struct CapsuleSwitch: View {
    @Environment(\.agentHubPalette) private var palette
    @Environment(\.isEnabled) private var isEnabled
    @Binding var isOn: Bool
    var accessibilityLabel: String

    init(isOn: Binding<Bool>, label: String = "") {
        self._isOn = isOn
        self.accessibilityLabel = label
    }

    var body: some View {
        Button {
            isOn.toggle()
        } label: {
            ZStack(alignment: isOn ? .trailing : .leading) {
                Capsule(style: .continuous)
                    .fill((isOn ? palette.accent : palette.trackOff).color)
                    .overlay {
                        if !isOn {
                            Capsule(style: .continuous)
                                .strokeBorder(palette.textMuted.color, lineWidth: 1)
                        }
                    }
                    .frame(width: 38, height: 22)
                Circle()
                    .fill(
                        (isOn && palette.scheme == .dark
                            ? RGBAColor(hex: 0x111318)
                            : RGBAColor(hex: 0xFFFFFF)
                        ).color
                    )
                    .frame(width: 18, height: 18)
                    .shadow(color: Color.black.opacity(0.28), radius: 1.5, x: 0, y: 1)
                    .padding(.horizontal, 2)
            }
            .frame(width: 38, height: 22)
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
        .opacity(isEnabled ? 1.0 : 0.48)
        .accessibilityLabel(accessibilityLabel)
        .accessibilityValue(isOn ? "已开启" : "已关闭")
    }
}

struct PlainHitButtonStyle: ButtonStyle {
    init() {}
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .contentShape(Rectangle())
    }
}

// MARK: - Status Chips, Metrics, Panels & RunningIndicator

public enum StatusChipTone: String, Sendable {
    case neutral
    case muted
    case info
    case success
    case warning
    case danger
}

struct StatusChip: View {
    @Environment(\.agentHubPalette) private var palette
    let text: String
    var tone: StatusChipTone

    init(_ text: String, tone: StatusChipTone = .neutral) {
        self.text = text
        self.tone = tone
    }

    var body: some View {
        let colors: (fg: RGBAColor, bg: RGBAColor, border: RGBAColor) = switch tone {
        case .success: (palette.okFg, palette.okBg, palette.okBorder)
        case .warning: (palette.warnFg, palette.warnBg, palette.warnBorder)
        case .danger: (palette.errFg, palette.errBg, palette.errBorder)
        case .info: (palette.accent, palette.accentSoft, palette.accentBorder)
        case .neutral, .muted: (palette.textMuted, palette.elevated, palette.borderStrong)
        }
        Text(text)
            .font(DesignFonts.body(12, weight: .medium))
            .foregroundStyle(colors.fg.color)
            .lineLimit(1)
            .padding(.horizontal, 9)
            .padding(.vertical, 3)
            .background(Capsule(style: .continuous).fill(colors.bg.color))
            .overlay(Capsule(style: .continuous).strokeBorder(colors.border.color, lineWidth: 1))
    }
}

struct MetricCard: View {
    @Environment(\.agentHubPalette) private var palette
    let label: String
    let value: String
    var tone: StatusChipTone?

    init(label: String, value: String, tone: StatusChipTone? = nil) {
        self.label = label
        self.value = value
        self.tone = tone
    }

    var body: some View {
        let valueColor: RGBAColor = switch tone {
        case .success: palette.okFg
        case .warning: palette.warnFg
        case .danger: palette.errFg
        case .info: palette.accent
        default: palette.textPrimary
        }
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(DesignFonts.body(12))
                .foregroundStyle(palette.textMuted.color)
                .lineLimit(1)
            Text(value)
                .font(DesignFonts.body(14, weight: .semibold))
                .foregroundStyle(valueColor.color)
                .lineLimit(2)
                .minimumScaleFactor(0.85)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
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

struct PanelCard<HeaderAction: View, Content: View>: View {
    @Environment(\.agentHubPalette) private var palette
    let title: String
    var icon: LucideIconName?
    @ViewBuilder let headerAction: () -> HeaderAction
    @ViewBuilder let content: () -> Content

    init(
        title: String,
        icon: LucideIconName? = nil,
        @ViewBuilder headerAction: @escaping () -> HeaderAction = { EmptyView() },
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.title = title
        self.icon = icon
        self.headerAction = headerAction
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                if let icon {
                    LucideIcon(icon, size: 18)
                        .foregroundStyle(palette.textMuted.color)
                }
                Text(title)
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                Spacer(minLength: 8)
                headerAction()
            }
            content()
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(palette.surface.color)
                .shadow(
                    color: palette.shadow1.color.color,
                    radius: palette.shadow1.radius,
                    x: palette.shadow1.x,
                    y: palette.shadow1.y
                )
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(palette.borderSubtle.color, lineWidth: 1)
        )
    }
}

struct CollapsiblePanelCard<SummaryBadges: View, HeaderTrailing: View, Content: View>: View {
    @Environment(\.agentHubPalette) private var palette
    let title: String
    var icon: LucideIconName?
    @Binding var isExpanded: Bool
    @ViewBuilder let summaryBadges: () -> SummaryBadges
    @ViewBuilder let headerTrailing: () -> HeaderTrailing
    @ViewBuilder let content: () -> Content

    init(
        title: String,
        icon: LucideIconName? = nil,
        isExpanded: Binding<Bool>,
        @ViewBuilder summaryBadges: @escaping () -> SummaryBadges = { EmptyView() },
        @ViewBuilder headerTrailing: @escaping () -> HeaderTrailing = { EmptyView() },
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.title = title
        self.icon = icon
        self._isExpanded = isExpanded
        self.summaryBadges = summaryBadges
        self.headerTrailing = headerTrailing
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Button {
                    isExpanded.toggle()
                } label: {
                    HStack(spacing: 8) {
                        LucideIcon(isExpanded ? .chevronDown : .chevronRight, size: 15)
                            .foregroundStyle(palette.textMuted.color)
                        if let icon {
                            LucideIcon(icon, size: 17)
                                .foregroundStyle(palette.textMuted.color)
                        }
                        Text(title)
                            .font(DesignFonts.body(14, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                            .lineLimit(1)
                        summaryBadges()
                        Spacer(minLength: 4)
                    }
                    .frame(maxWidth: .infinity, minHeight: 48, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())

                headerTrailing()
                    .padding(.vertical, 6)
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 48)

            if isExpanded {
                Rectangle()
                    .fill(palette.borderSubtle.color)
                    .frame(height: 1)
                VStack(alignment: .leading, spacing: 12) {
                    content()
                }
                .padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(palette.surface.color)
                .shadow(
                    color: palette.shadow1.color.color,
                    radius: palette.shadow1.radius,
                    x: palette.shadow1.x,
                    y: palette.shadow1.y
                )
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(palette.borderSubtle.color, lineWidth: 1)
        )
    }
}

struct RunningIndicator: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject private var motionObserver = AccessibilityMotionObserver.shared
    @State private var angle: Double = 0
    var size: CGFloat
    var reduceMotionOverride: Bool?

    private var effectiveReduceMotion: Bool {
        reduceMotionOverride ?? motionObserver.reduceMotion
    }

    init(size: CGFloat = 13, reduceMotion: Bool? = nil) {
        self.size = size
        self.reduceMotionOverride = reduceMotion
    }

    var body: some View {
        ZStack {
            // 2pt --bs (borderStrong) full ring track underneath (styles.css:329-332)
            Circle()
                .stroke(palette.borderStrong.color, lineWidth: 2)
            Circle()
                .trim(from: 0, to: 0.28)
                .stroke(
                    palette.accent.color,
                    style: StrokeStyle(lineWidth: 2, lineCap: .round)
                )
                .rotationEffect(.degrees(effectiveReduceMotion ? -90 : angle - 90))
        }
        .frame(width: size, height: size)
        .onAppear {
            restartAnimationIfNeeded()
        }
        .onChange(of: effectiveReduceMotion) { _ in
            restartAnimationIfNeeded()
        }
        .accessibilityLabel("运行中")
    }

    private func restartAnimationIfNeeded() {
        var reset = Transaction()
        reset.disablesAnimations = true
        withTransaction(reset) {
            angle = 0
        }
        guard !effectiveReduceMotion else { return }
        withAnimation(.linear(duration: 0.9).repeatForever(autoreverses: false)) {
            angle = 360
        }
    }
}

// MARK: - Inline Modal Presentation Registry (Blocks Underlying ⌘A Select-All Leak)

private final class WeakNSViewRef {
    weak var view: NSView?
    init(_ view: NSView?) { self.view = view }
}

@MainActor
final class ModalPresentationRegistry {
    static let shared = ModalPresentationRegistry()
    private var activeTokens: Set<UUID> = []
    private var activeContainers: [UUID: WeakNSViewRef] = [:]

    init() {}

    var hasActiveModal: Bool {
        !activeTokens.isEmpty
    }

    func register(_ token: UUID, container: NSView? = nil) {
        activeTokens.insert(token)
        if let container {
            activeContainers[token] = WeakNSViewRef(container)
        }
    }

    func updateContainer(_ container: NSView?, for token: UUID) {
        guard activeTokens.contains(token) else { return }
        if let container {
            activeContainers[token] = WeakNSViewRef(container)
        }
    }

    func unregister(_ token: UUID) {
        activeTokens.remove(token)
        activeContainers.removeValue(forKey: token)
    }

    func isInsideActiveModalContainer(_ responderView: NSView) -> Bool {
        for box in activeContainers.values {
            if let root = box.view, responderView === root || responderView.isDescendant(of: root) {
                return true
            }
        }
        return false
    }
}

struct ModalScopePresenceView: NSViewRepresentable {
    var modalContainer: NSView?

    init(modalContainer: NSView? = nil) {
        self.modalContainer = modalContainer
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSView {
        let view = NSView(frame: .zero)
        context.coordinator.activate(container: modalContainer)
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        context.coordinator.activate(container: modalContainer)
    }

    static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
        coordinator.deactivate()
    }

    @MainActor
    final class Coordinator {
        let token = UUID()
        var active = false

        func activate(container: NSView?) {
            if !active {
                active = true
                ModalPresentationRegistry.shared.register(token, container: container)
            } else if let container {
                ModalPresentationRegistry.shared.updateContainer(container, for: token)
            }
        }

        func deactivate() {
            guard active else { return }
            active = false
            ModalPresentationRegistry.shared.unregister(token)
        }
    }
}

struct ModalEscapeAndFocusMonitorView: NSViewRepresentable {
    var onEscape: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onEscape: onEscape)
    }

    func makeNSView(context: Context) -> NSView {
        let view = NSView(frame: .zero)
        context.coordinator.attach(anchor: view)
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        context.coordinator.onEscape = onEscape
        context.coordinator.attach(anchor: nsView)
    }

    static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
        coordinator.detach()
    }

    @MainActor
    final class Coordinator {
        weak var anchorView: NSView?
        weak var targetWindow: NSWindow?
        weak var savedFirstResponder: NSResponder?
        var didCaptureResponder = false
        var onEscape: () -> Void
        private var monitor: Any?

        init(onEscape: @escaping () -> Void) {
            self.onEscape = onEscape
        }

        func attach(anchor: NSView) {
            anchorView = anchor
            if !didCaptureResponder, let win = anchor.window ?? NSApp.keyWindow {
                didCaptureResponder = true
                targetWindow = win
                savedFirstResponder = win.firstResponder
            }
            guard monitor == nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard let self,
                      let win = self.anchorView?.window ?? self.targetWindow,
                      event.window === win
                else { return event }
                let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
                if event.keyCode == 53 && flags.isEmpty {
                    guard !AppInputCompositionGuard.responderHasMarkedText(win.firstResponder) else {
                        return event
                    }
                    self.onEscape()
                    return nil
                }
                return event
            }
        }

        func detach() {
            if let monitor {
                NSEvent.removeMonitor(monitor)
                self.monitor = nil
            }
            if let win = targetWindow ?? anchorView?.window, let resp = savedFirstResponder {
                if let respView = resp as? NSView {
                    if respView.window === win {
                        win.makeFirstResponder(respView)
                    }
                } else if resp === win {
                    win.makeFirstResponder(win)
                }
            }
            savedFirstResponder = nil
            targetWindow = nil
        }
    }
}

// MARK: - Natural Width Wrapping Action Button Row (DESIGN.md:154)

@available(macOS 13.0, *)
private struct NaturalWrappingRowLayout: Layout {
    var horizontalSpacing: CGFloat
    var verticalSpacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxW = proposal.width ?? .greatestFiniteMagnitude
        var currentX: CGFloat = 0
        var currentRowHeight: CGFloat = 0
        var totalHeight: CGFloat = 0
        var maxUsedWidth: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if currentX > 0 && currentX + size.width > maxW {
                totalHeight += currentRowHeight + verticalSpacing
                maxUsedWidth = max(maxUsedWidth, currentX - horizontalSpacing)
                currentX = 0
                currentRowHeight = 0
            }
            currentX += size.width + horizontalSpacing
            currentRowHeight = max(currentRowHeight, size.height)
        }
        if currentRowHeight > 0 {
            totalHeight += currentRowHeight
            maxUsedWidth = max(maxUsedWidth, max(0, currentX - horizontalSpacing))
        }
        return CGSize(
            width: proposal.width != nil ? min(maxUsedWidth, maxW) : maxUsedWidth,
            height: totalHeight
        )
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let maxW = max(1, bounds.width)
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0
        var rowItems: [(LayoutSubviews.Element, CGSize)] = []

        func flushRow() {
            guard !rowItems.isEmpty else { return }
            var cx = bounds.minX
            for (item, size) in rowItems {
                let centeredY = y + floor((rowHeight - size.height) / 2.0)
                item.place(
                    at: CGPoint(x: cx, y: centeredY),
                    proposal: ProposedViewSize(width: min(size.width, maxW), height: size.height)
                )
                cx += min(size.width, maxW) + horizontalSpacing
            }
            y += rowHeight + verticalSpacing
            x = bounds.minX
            rowHeight = 0
            rowItems.removeAll(keepingCapacity: true)
        }

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if !rowItems.isEmpty && (x - bounds.minX) + size.width > maxW {
                flushRow()
            }
            rowItems.append((subview, size))
            x += size.width + horizontalSpacing
            rowHeight = max(rowHeight, size.height)
        }
        flushRow()
    }
}

struct FlowButtonRow<Content: View>: View {
    var spacing: CGFloat
    var lineSpacing: CGFloat
    @ViewBuilder let content: () -> Content

    init(
        spacing: CGFloat = 8,
        lineSpacing: CGFloat = 8,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.spacing = spacing
        self.lineSpacing = lineSpacing
        self.content = content
    }

    var body: some View {
        if #available(macOS 13.0, *) {
            NaturalWrappingRowLayout(horizontalSpacing: spacing, verticalSpacing: lineSpacing) {
                content()
            }
        } else {
            HStack(spacing: spacing) {
                content()
            }
        }
    }
}

// MARK: - ConfirmDialogOverlay (backdrop click NEVER dismisses; Esc dismisses only when !busy)

struct ConfirmDialogOverlay<Content: View>: View {
    @Environment(\.agentHubPalette) private var palette
    let title: String?
    var busy: Bool
    let onCancel: () -> Void
    @ViewBuilder let content: () -> Content

    init(
        title: String? = nil,
        busy: Bool = false,
        onCancel: @escaping () -> Void,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.title = title
        self.busy = busy
        self.onCancel = onCancel
        self.content = content
    }

    var body: some View {
        ZStack {
            ModalScopePresenceView()
                .frame(width: 0, height: 0)

            Color.black.opacity(0.55)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                // Intentionally absorbs clicks without dismissing.
                .onTapGesture {}

            VStack(alignment: .leading, spacing: 14) {
                if let title, !title.isEmpty {
                    Text(title)
                        .font(DesignFonts.body(16, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                }
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(24)
            .frame(maxWidth: 520, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(palette.elevated.color)
                    .shadow(
                        color: palette.shadow2.color.color,
                        radius: palette.shadow2.radius,
                        x: palette.shadow2.x,
                        y: palette.shadow2.y
                    )
            )
            .overlay(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .strokeBorder(palette.borderStrong.color, lineWidth: 1)
            )
            .padding(.horizontal, 16)
        }
        .accessibilityElement(children: .contain)
        .onExitCommand {
            if !busy, !AppInputCompositionGuard.responderHasMarkedText() {
                onCancel()
            }
        }
    }
}

@MainActor
enum AppInputCompositionGuard {
    static func responderHasMarkedText(_ responder: NSResponder? = NSApp.keyWindow?.firstResponder) -> Bool {
        (responder as? NSTextInputClient)?.hasMarkedText() == true
    }
}

// MARK: - Top Accent Bar (clipped to container cornerRadius, macOS 12+ safe)

struct UnevenTopAccentBar: View {
    let color: Color
    var height: CGFloat
    var cornerRadius: CGFloat

    init(color: Color, height: CGFloat = 2.5, cornerRadius: CGFloat = 12) {
        self.color = color
        self.height = height
        self.cornerRadius = cornerRadius
    }

    var body: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(color)
                .frame(height: height)
            Spacer(minLength: 0)
        }
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        .allowsHitTesting(false)
    }
}

// MARK: - Custom-Styled Button Popping Up Native NSMenu Without NSPopUpButton '⌄' Chevron Hijack

struct NativeMenuEntry: Identifiable {
    let id: String
    let title: String
    var disabled: Bool
    var isSeparator: Bool
    var isSelected: Bool = false
    var action: () -> Void

    init(id: String, title: String, disabled: Bool = false, selected: Bool = false, action: @escaping () -> Void) {
        self.id = id
        self.title = title
        self.disabled = disabled
        self.isSelected = selected
        self.isSeparator = false
        self.action = action
    }

    static func separator(_ id: String = UUID().uuidString) -> NativeMenuEntry {
        NativeMenuEntry(id: id, title: "", disabled: true, isSeparator: true, action: {})
    }

    private init(id: String, title: String, disabled: Bool, isSeparator: Bool, action: @escaping () -> Void) {
        self.id = id
        self.title = title
        self.disabled = disabled
        self.isSeparator = isSeparator
        self.action = action
    }
}

@MainActor
private final class MenuTargetRelay: NSObject {
    let entries: [NativeMenuEntry]

    init(entries: [NativeMenuEntry]) {
        self.entries = entries
    }

    @objc func invokeEntry(_ sender: NSMenuItem) {
        guard entries.indices.contains(sender.tag) else { return }
        entries[sender.tag].action()
    }
}

struct MenuAnchorProbeNSView: NSViewRepresentable {
    @Binding var capturedView: NSView?

    init(capturedView: Binding<NSView?>) {
        self._capturedView = capturedView
    }

    static func resolveAnchorView(_ probeView: NSView) -> NSView {
        if probeView.bounds.isEmpty,
           let sup = probeView.superview,
           !sup.bounds.isEmpty,
           sup.bounds.width <= 400,
           sup.bounds.height <= 120
        {
            return sup
        }
        return probeView
    }

    func makeNSView(context: Context) -> NSView {
        let v = NSView(frame: .zero)
        DispatchQueue.main.async {
            if capturedView !== v {
                capturedView = v
            }
        }
        return v
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        if capturedView !== nsView {
            DispatchQueue.main.async {
                if capturedView !== nsView {
                    capturedView = nsView
                }
            }
        }
    }
}

struct AnchoredMenuButton<Label: View>: View {
    let entries: [NativeMenuEntry]
    var alignRight: Bool
    var accessibilityLabel: String?
    @ViewBuilder let label: () -> Label

    @State private var anchorView: NSView?
    @State private var activeRelay: MenuTargetRelay?

    init(
        entries: [NativeMenuEntry],
        alignRight: Bool = false,
        accessibilityLabel: String? = nil,
        @ViewBuilder label: @escaping () -> Label
    ) {
        self.entries = entries
        self.alignRight = alignRight
        self.accessibilityLabel = accessibilityLabel
        self.label = label
    }

    var body: some View {
        Button {
            popMenu()
        } label: {
            label()
                .background(MenuAnchorProbeNSView(capturedView: $anchorView))
        }
        .buttonStyle(PlainHitButtonStyle())
        .accessibilityLabel(accessibilityLabel ?? "菜单")
    }

    @MainActor
    private func popMenu() {
        guard !entries.isEmpty else { return }
        let relay = MenuTargetRelay(entries: entries)
        activeRelay = relay
        let menu = NSMenu()
        menu.autoenablesItems = false
        for (idx, item) in entries.enumerated() {
            if item.isSeparator {
                menu.addItem(.separator())
                continue
            }
            let mi = NSMenuItem(title: item.title, action: #selector(MenuTargetRelay.invokeEntry(_:)), keyEquivalent: "")
            mi.target = relay
            mi.tag = idx
            mi.state = item.isSelected ? .on : .off
            mi.isEnabled = !item.disabled
            menu.addItem(mi)
        }
        if let rawAnchor = anchorView, rawAnchor.window != nil {
            let resolved = MenuAnchorProbeNSView.resolveAnchorView(rawAnchor)
            let y: CGFloat = resolved.isFlipped ? resolved.bounds.maxY + 4 : -4
            let x: CGFloat = alignRight ? (resolved.bounds.maxX - menu.size.width) : 0
            menu.popUp(positioning: nil, at: NSPoint(x: x, y: y), in: resolved)
        } else {
            menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
        }
    }
}

// MARK: - Bounded Inner Scroll View with Boundary & Flyover Momentum Passthrough (macOS 12+)

@MainActor
final class BoundedInnerScrollView: NSScrollView {
    private(set) var trackingInnerGesture = false
    private(set) var trackingInnerMomentum = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        drawsBackground = false
        borderType = .noBorder
        autohidesScrollers = true
        hasHorizontalScroller = false
        horizontalScrollElasticity = .none
        verticalScrollElasticity = .none
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    /// Returns `true` when the scroll wheel event should be consumed by this inner scroll view,
    /// or `false` when it should pass through to `nextResponder` (outer page scroll view).
    @discardableResult
    func shouldConsumeVerticalScroll(
        deltaY: CGFloat,
        deltaX: CGFloat = 0,
        phase: NSEvent.Phase,
        momentumPhase: NSEvent.Phase
    ) -> Bool {
        let docH = documentView?.frame.height ?? 0
        let clipH = contentView.bounds.height
        let maxScroll = max(0, docH - clipH)
        guard maxScroll > 0.5 else {
            trackingInnerGesture = false
            trackingInnerMomentum = false
            return false
        }
        if abs(deltaX) > abs(deltaY) && abs(deltaY) < 0.01 {
            return false
        }

        let originY = contentView.bounds.origin.y
        let atTopGoingUp = deltaY > 0 && originY <= 0.5
        let atBottomGoingDown = deltaY < 0 && originY >= maxScroll - 0.5
        let atBoundaryOutward = atTopGoingUp || atBottomGoingDown

        if momentumPhase != [] {
            if momentumPhase == .began {
                trackingInnerMomentum = trackingInnerGesture && !atBoundaryOutward
                trackingInnerGesture = false
            }
            guard trackingInnerMomentum, !atBoundaryOutward else {
                trackingInnerMomentum = false
                return false
            }
            if momentumPhase == .ended || momentumPhase == .cancelled {
                trackingInnerMomentum = false
            }
            return true
        }

        if phase == .began || phase == .mayBegin {
            trackingInnerGesture = false
            trackingInnerMomentum = false
            if deltaY == 0 {
                return true
            }
        }
        if phase == .cancelled {
            trackingInnerGesture = false
            trackingInnerMomentum = false
            return false
        }
        if atBoundaryOutward {
            trackingInnerGesture = false
            return false
        }
        if abs(deltaY) > 0.001 {
            trackingInnerGesture = true
        }
        return true
    }

    override func scrollWheel(with event: NSEvent) {
        let consumeInner = shouldConsumeVerticalScroll(
            deltaY: event.scrollingDeltaY,
            deltaX: event.scrollingDeltaX,
            phase: event.phase,
            momentumPhase: event.momentumPhase
        )
        if consumeInner {
            super.scrollWheel(with: event)
        } else {
            nextResponder?.scrollWheel(with: event)
        }
    }
}

@MainActor
private final class BoundedHostingView<Content: View>: NSHostingView<Content> {
    var onIntrinsicSizeInvalidated: (() -> Void)?

    override func invalidateIntrinsicContentSize() {
        super.invalidateIntrinsicContentSize()
        onIntrinsicSizeInvalidated?()
    }
}

@MainActor
private final class FlippedBoundedDocumentView: NSView {
    override var isFlipped: Bool { true }
}

@MainActor
final class BoundedPassthroughContainerView<Content: View>: NSView {
    let scrollView = BoundedInnerScrollView(frame: .zero)
    private let documentContainer = FlippedBoundedDocumentView(frame: .zero)
    fileprivate let hostingView: BoundedHostingView<Content>
    private var widthConstraint: NSLayoutConstraint?
    var maxHeight: CGFloat {
        didSet {
            if abs(maxHeight - oldValue) > 0.5 {
                remeasureAndSync(forceInvalidate: true)
            }
        }
    }
    private(set) var measuredContentHeight: CGFloat = 1
    private var clampedHeight: CGFloat = 1
    private var lastMeasuredWidth: CGFloat = 0
    private var isMeasuring = false

    override var isFlipped: Bool { true }

    init(rootView: Content, maxHeight: CGFloat) {
        self.maxHeight = max(1, maxHeight)
        self.hostingView = BoundedHostingView(rootView: rootView)
        super.init(frame: .zero)
        setContentHuggingPriority(.defaultLow, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        setContentHuggingPriority(.required, for: .vertical)
        setContentCompressionResistancePriority(.required, for: .vertical)

        hostingView.translatesAutoresizingMaskIntoConstraints = false
        documentContainer.addSubview(hostingView)
        let wc = hostingView.widthAnchor.constraint(equalToConstant: 480)
        NSLayoutConstraint.activate([
            hostingView.topAnchor.constraint(equalTo: documentContainer.topAnchor),
            hostingView.leadingAnchor.constraint(equalTo: documentContainer.leadingAnchor),
            wc,
        ])
        self.widthConstraint = wc

        scrollView.documentView = documentContainer
        addSubview(scrollView)

        hostingView.onIntrinsicSizeInvalidated = { [weak self] in
            self?.remeasureAndSync(forceInvalidate: false)
        }
        remeasureAndSync(forceInvalidate: true)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var intrinsicContentSize: NSSize {
        NSSize(width: NSView.noIntrinsicMetric, height: clampedHeight)
    }

    func update(rootView: Content, maxHeight: CGFloat) {
        self.maxHeight = max(1, maxHeight)
        hostingView.rootView = rootView
        remeasureAndSync(forceInvalidate: false)
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        scrollView.frame = bounds
        scrollView.tile()
        let clipW = effectiveClipWidth(fallback: newSize.width)
        if newSize.width > 1 && (abs(newSize.width - lastMeasuredWidth) > 0.5 || abs((widthConstraint?.constant ?? 0) - clipW) > 0.5) {
            remeasureAndSync(forceInvalidate: false)
        } else {
            syncDocumentFrame(for: max(1, clipW))
        }
    }

    override func layout() {
        super.layout()
        scrollView.frame = bounds
        scrollView.tile()
        let clipW = max(1, effectiveClipWidth(fallback: bounds.width))
        if let widthConstraint, abs(widthConstraint.constant - clipW) > 0.5 {
            widthConstraint.constant = clipW
            hostingView.layoutSubtreeIfNeeded()
            measuredContentHeight = max(1, ceil(hostingView.fittingSize.height))
        }
        syncDocumentFrame(for: clipW)
    }

    private func effectiveClipWidth(fallback: CGFloat) -> CGFloat {
        let clipW = scrollView.contentView.bounds.width
        if scrollView.frame.width > 1 && clipW > 1 {
            return clipW
        }
        return fallback
    }

    private func syncDocumentFrame(for width: CGFloat) {
        let docH = max(measuredContentHeight, scrollView.contentView.bounds.height)
        let target = NSRect(x: 0, y: 0, width: width, height: docH)
        if abs(documentContainer.frame.width - target.width) > 0.5 || abs(documentContainer.frame.height - target.height) > 0.5 {
            documentContainer.frame = target
        }
    }

    private func remeasureAndSync(forceInvalidate: Bool) {
        guard !isMeasuring else { return }
        isMeasuring = true
        defer { isMeasuring = false }

        let outerWidth = bounds.width > 1 ? bounds.width : (lastMeasuredWidth > 1 ? lastMeasuredWidth : 640)
        lastMeasuredWidth = outerWidth
        var targetWidth = effectiveClipWidth(fallback: outerWidth)
        if let widthConstraint, abs(widthConstraint.constant - targetWidth) > 0.5 {
            widthConstraint.constant = targetWidth
        }
        hostingView.layoutSubtreeIfNeeded()
        var rawHeight = max(1, ceil(hostingView.fittingSize.height))
        let needsScroller = rawHeight > maxHeight + 0.5
        if scrollView.hasVerticalScroller != needsScroller {
            scrollView.hasVerticalScroller = needsScroller
        }
        if bounds.width > 1 {
            scrollView.tile()
            let updatedClipWidth = effectiveClipWidth(fallback: outerWidth)
            if abs(updatedClipWidth - targetWidth) > 0.5 {
                targetWidth = updatedClipWidth
                widthConstraint?.constant = targetWidth
                hostingView.layoutSubtreeIfNeeded()
                rawHeight = max(1, ceil(hostingView.fittingSize.height))
            }
        }
        measuredContentHeight = rawHeight
        let nextClamped = min(rawHeight, maxHeight)
        syncDocumentFrame(for: targetWidth)
        if forceInvalidate || abs(nextClamped - clampedHeight) > 0.5 {
            clampedHeight = nextClamped
            invalidateIntrinsicContentSize()
        }
    }
}

struct BoundedPassthroughScrollView<Content: View>: NSViewRepresentable {
    var maxHeight: CGFloat
    let content: Content

    init(maxHeight: CGFloat, @ViewBuilder content: () -> Content) {
        self.maxHeight = maxHeight
        self.content = content()
    }

    func makeNSView(context: Context) -> BoundedPassthroughContainerView<Content> {
        BoundedPassthroughContainerView(rootView: content, maxHeight: maxHeight)
    }

    func updateNSView(_ nsView: BoundedPassthroughContainerView<Content>, context: Context) {
        nsView.update(rootView: content, maxHeight: maxHeight)
    }
}
