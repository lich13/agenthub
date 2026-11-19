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

    public init(selected: Bool = false) {
        self.selected = selected
    }

    public func makeBody(configuration: Configuration) -> some View {
        InnerBody(configuration: configuration, selected: selected)
    }

    private struct InnerBody: View {
        @Environment(\.agentHubPalette) private var palette
        @Environment(\.isEnabled) private var isEnabled
        let configuration: Configuration
        let selected: Bool

        var body: some View {
            configuration.label
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(
                            selected
                                ? palette.selected.color
                                : (configuration.isPressed ? palette.hover.color : Color.clear)
                        )
                )
                .overlay(alignment: .leading) {
                    if selected {
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

    public init(size: CGFloat = 14, reduceMotion: Bool = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion) {
        self.size = size
        self.reduceMotion = reduceMotion
    }

    public var body: some View {
        Circle()
            .trim(from: 0.12, to: 0.88)
            .stroke(
                palette.accent.color,
                style: StrokeStyle(lineWidth: 2, lineCap: .round)
            )
            .frame(width: size, height: size)
            .rotationEffect(.degrees(reduceMotion ? 0 : angle))
            .onAppear {
                guard !reduceMotion else { return }
                withAnimation(.linear(duration: 0.9).repeatForever(autoreverses: false)) {
                    angle = 360
                }
            }
            .accessibilityLabel("运行中")
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
