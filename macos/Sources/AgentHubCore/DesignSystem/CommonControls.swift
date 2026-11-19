import AppKit
import SwiftUI

// MARK: - Full-Rectangle Hit-Target Button Styles (Never bare .plain)

public struct PrimaryButtonStyle: ButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
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

public struct SecondaryButtonStyle: ButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
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

public struct DangerButtonStyle: ButtonStyle {
    public var soft: Bool

    public init(soft: Bool = false) {
        self.soft = soft
    }

    public func makeBody(configuration: Configuration) -> some View {
        InnerBody(configuration: configuration, soft: soft)
    }

    private struct InnerBody: View {
        @Environment(\.agentHubPalette) private var palette
        @Environment(\.isEnabled) private var isEnabled
        let configuration: Configuration
        let soft: Bool

        var body: some View {
            let fg = soft ? palette.errFg : RGBAColor(hex: 0xFFFFFF)
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

public struct IconButtonStyle: ButtonStyle {
    public var dimension: CGFloat

    public init(dimension: CGFloat = 32) {
        self.dimension = dimension
    }

    public func makeBody(configuration: Configuration) -> some View {
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

public struct FullWidthRowButtonStyle: ButtonStyle {
    public var selected: Bool
    public var multiSelected: Bool
    public var cornerRadius: CGFloat

    public init(selected: Bool = false, multiSelected: Bool = false, cornerRadius: CGFloat = 8) {
        self.selected = selected
        self.multiSelected = multiSelected
        self.cornerRadius = cornerRadius
    }

    public func makeBody(configuration: Configuration) -> some View {
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

public struct CapsuleSwitch: View {
    @Environment(\.agentHubPalette) private var palette
    @Environment(\.isEnabled) private var isEnabled
    @Binding public var isOn: Bool
    public var accessibilityLabel: String

    public init(isOn: Binding<Bool>, label: String = "") {
        self._isOn = isOn
        self.accessibilityLabel = label
    }

    public var body: some View {
        Button {
            isOn.toggle()
        } label: {
            ZStack(alignment: isOn ? .trailing : .leading) {
                Capsule(style: .continuous)
                    .fill((isOn ? palette.accent : palette.trackOff).color)
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

public struct PlainHitButtonStyle: ButtonStyle {
    public init() {}
    public func makeBody(configuration: Configuration) -> some View {
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

public struct StatusChip: View {
    @Environment(\.agentHubPalette) private var palette
    public let text: String
    public var tone: StatusChipTone

    public init(_ text: String, tone: StatusChipTone = .neutral) {
        self.text = text
        self.tone = tone
    }

    public var body: some View {
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

public struct MetricCard: View {
    @Environment(\.agentHubPalette) private var palette
    public let label: String
    public let value: String
    public var tone: StatusChipTone?

    public init(label: String, value: String, tone: StatusChipTone? = nil) {
        self.label = label
        self.value = value
        self.tone = tone
    }

    public var body: some View {
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

public struct PanelCard<HeaderAction: View, Content: View>: View {
    @Environment(\.agentHubPalette) private var palette
    public let title: String
    public var icon: LucideIconName?
    @ViewBuilder public let headerAction: () -> HeaderAction
    @ViewBuilder public let content: () -> Content

    public init(
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

    public var body: some View {
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

public struct RunningIndicator: View {
    @Environment(\.agentHubPalette) private var palette
    @State private var angle: Double = 0
    public var size: CGFloat
    public var reduceMotion: Bool

    public init(size: CGFloat = 13, reduceMotion: Bool = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion) {
        self.size = size
        self.reduceMotion = reduceMotion
    }

    public var body: some View {
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
                .rotationEffect(.degrees(reduceMotion ? -90 : angle - 90))
        }
        .frame(width: size, height: size)
        .onAppear {
            guard !reduceMotion else { return }
            withAnimation(.linear(duration: 0.9).repeatForever(autoreverses: false)) {
                angle = 360
            }
        }
        .accessibilityLabel("运行中")
    }
}

// MARK: - Inline Modal Presentation Registry (Blocks Underlying ⌘A Select-All Leak)

@MainActor
public final class ModalPresentationRegistry {
    public static let shared = ModalPresentationRegistry()
    private var activeTokens: Set<UUID> = []

    public init() {}

    public var hasActiveModal: Bool {
        !activeTokens.isEmpty
    }

    public func register(_ token: UUID) {
        activeTokens.insert(token)
    }

    public func unregister(_ token: UUID) {
        activeTokens.remove(token)
    }

    public func resetForTesting() {
        activeTokens.removeAll()
    }
}

public struct ModalScopePresenceView: NSViewRepresentable {
    public init() {}

    public func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    public func makeNSView(context: Context) -> NSView {
        let view = NSView(frame: .zero)
        context.coordinator.activate()
        return view
    }

    public func updateNSView(_ nsView: NSView, context: Context) {
        context.coordinator.activate()
    }

    public static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
        coordinator.deactivate()
    }

    @MainActor
    public final class Coordinator {
        let token = UUID()
        var active = false

        func activate() {
            guard !active else { return }
            active = true
            ModalPresentationRegistry.shared.register(token)
        }

        func deactivate() {
            guard active else { return }
            active = false
            ModalPresentationRegistry.shared.unregister(token)
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

public struct FlowButtonRow<Content: View>: View {
    public var spacing: CGFloat
    public var lineSpacing: CGFloat
    @ViewBuilder public let content: () -> Content

    public init(
        spacing: CGFloat = 8,
        lineSpacing: CGFloat = 8,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.spacing = spacing
        self.lineSpacing = lineSpacing
        self.content = content
    }

    public var body: some View {
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

public struct ConfirmDialogOverlay<Content: View>: View {
    @Environment(\.agentHubPalette) private var palette
    public let title: String
    public var busy: Bool
    public let onCancel: () -> Void
    @ViewBuilder public let content: () -> Content

    public init(
        title: String,
        busy: Bool = false,
        onCancel: @escaping () -> Void,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.title = title
        self.busy = busy
        self.onCancel = onCancel
        self.content = content
    }

    public var body: some View {
        ZStack {
            ModalScopePresenceView()
                .frame(width: 0, height: 0)

            Color.black.opacity(0.55)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                // Intentionally absorbs clicks without dismissing.
                .onTapGesture {}

            VStack(alignment: .leading, spacing: 14) {
                Text(title)
                    .font(DesignFonts.body(16, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                content()
            }
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
            if !busy {
                onCancel()
            }
        }
    }
}

// MARK: - Top Accent Bar (clipped to container cornerRadius, macOS 12+ safe)

public struct UnevenTopAccentBar: View {
    public let color: Color
    public var height: CGFloat
    public var cornerRadius: CGFloat

    public init(color: Color, height: CGFloat = 2.5, cornerRadius: CGFloat = 12) {
        self.color = color
        self.height = height
        self.cornerRadius = cornerRadius
    }

    public var body: some View {
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

// MARK: - Glass Header Surface (--g 88% + Gaussian Blur radius 10)

public struct GlassHeaderSurface: NSViewRepresentable {
    public let tint: RGBAColor

    public init(tint: RGBAColor) {
        self.tint = tint
    }

    public func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.wantsLayer = true
        updateLayer(view)
        return view
    }

    public func updateNSView(_ nsView: NSView, context: Context) {
        updateLayer(nsView)
    }

    private func updateLayer(_ view: NSView) {
        guard let layer = view.layer else { return }
        layer.backgroundColor = tint.nsColor.cgColor
        if let blur = CIFilter(name: "CIGaussianBlur", parameters: [kCIInputRadiusKey: 10.0]) {
            layer.backgroundFilters = [blur]
        }
    }
}
