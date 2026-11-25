import AppKit
import SwiftUI

public enum WorkspaceId: String, CaseIterable, Codable, Sendable {
    case workspaces
    case codex
    case claude
    case grok
    case grokbot
    case antigravity
    case dsh
    case probe
    case ops

    public var label: String {
        switch self {
        case .workspaces: "工作区"
        case .codex: "Codex"
        case .claude: "Claude Code"
        case .grok: "Grok Build"
        case .grokbot: "Grok Bot"
        case .antigravity: "Antigravity"
        case .dsh: "DeepSeek Harness"
        case .probe: "Probe"
        case .ops: "设置"
        }
    }
}

public enum AgentBrandId: String, CaseIterable, Codable, Sendable {
    case codex
    case claude
    case grok
    case grokbot
    case antigravity
    case dsh
}

public enum ThemeMode: String, CaseIterable, Codable, Sendable {
    case system
    case dark
    case light

    public var label: String {
        switch self {
        case .system: "跟随系统"
        case .dark: "深色"
        case .light: "浅色"
        }
    }

    public static func normalize(_ raw: String?) -> ThemeMode {
        switch raw {
        case "dark": .dark
        case "light": .light
        default: .system
        }
    }
}

public enum ColorSchemeKind: String, Sendable {
    case light
    case dark
}

public struct RGBAColor: Equatable, Sendable {
    public let r: UInt8
    public let g: UInt8
    public let b: UInt8
    public let a: Double

    public init(r: UInt8, g: UInt8, b: UInt8, a: Double = 1.0) {
        self.r = r
        self.g = g
        self.b = b
        self.a = a
    }

    public init(hex: UInt32, alpha: Double = 1.0) {
        self.r = UInt8((hex >> 16) & 0xFF)
        self.g = UInt8((hex >> 8) & 0xFF)
        self.b = UInt8(hex & 0xFF)
        self.a = alpha
    }

    public var color: Color {
        Color(
            .sRGB,
            red: Double(r) / 255.0,
            green: Double(g) / 255.0,
            blue: Double(b) / 255.0,
            opacity: a
        )
    }

    public var nsColor: NSColor {
        NSColor(
            srgbRed: CGFloat(r) / 255.0,
            green: CGFloat(g) / 255.0,
            blue: CGFloat(b) / 255.0,
            alpha: CGFloat(a)
        )
    }

    public var relativeLuminance: Double {
        func linearize(_ channel: UInt8) -> Double {
            let s = Double(channel) / 255.0
            return s <= 0.03928 ? s / 12.92 : pow((s + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
    }

    public static func contrastRatio(_ fg: RGBAColor, _ bg: RGBAColor) -> Double {
        let l1 = max(fg.relativeLuminance, bg.relativeLuminance)
        let l2 = min(fg.relativeLuminance, bg.relativeLuminance)
        return (l1 + 0.05) / (l2 + 0.05)
    }
}

public struct AccentPalette: Equatable, Sendable {
    public let accent: RGBAColor       // --a
    public let accentSoft: RGBAColor   // --as
    public let accentBorder: RGBAColor // --ab
}

public struct SemanticShadow: Equatable, Sendable {
    public let color: RGBAColor
    public let x: CGFloat
    public let y: CGFloat
    public let radius: CGFloat
}

public struct DesignPalette: Equatable, Sendable {
    public let scheme: ColorSchemeKind
    public let railBg: RGBAColor           // --r
    public let background: RGBAColor       // --bg
    public let surface: RGBAColor          // --s
    public let elevated: RGBAColor         // --e
    public let glass: RGBAColor            // --g (alpha .88)
    public let hover: RGBAColor            // --h
    public let selected: RGBAColor         // --sl
    public let codeBg: RGBAColor           // --c
    public let textPrimary: RGBAColor      // --fg
    public let textSecondary: RGBAColor    // --fg2
    public let textMuted: RGBAColor        // --m
    public let borderSubtle: RGBAColor     // --bd
    public let borderStrong: RGBAColor     // --bs
    public let trackOff: RGBAColor         // --tk
    public let buttonBg: RGBAColor         // --bt
    public let buttonHover: RGBAColor      // --bh
    public let buttonFg: RGBAColor         // --bf
    public let okFg: RGBAColor             // --ok
    public let okBg: RGBAColor             // --ob
    public let okBorder: RGBAColor         // --od
    public let warnFg: RGBAColor           // --w
    public let warnBg: RGBAColor           // --wb
    public let warnBorder: RGBAColor       // --wd
    public let errFg: RGBAColor            // --er
    public let errBg: RGBAColor            // --eb
    public let errBorder: RGBAColor        // --ed
    public let accent: RGBAColor           // --a
    public let accentSoft: RGBAColor       // --as
    public let accentBorder: RGBAColor     // --ab
    public let shadow1: SemanticShadow     // --s1
    public let shadow2: SemanticShadow     // --s2

    public static func resolve(
        scheme: ColorSchemeKind,
        workspace: WorkspaceId = .probe
    ) -> DesignPalette {
        let acc = accentPalette(scheme: scheme, workspace: workspace)
        switch scheme {
        case .light:
            return DesignPalette(
                scheme: .light,
                railBg: RGBAColor(hex: 0xF8F9FB),
                background: RGBAColor(hex: 0xFFFFFF),
                surface: RGBAColor(hex: 0xF3F5F8),
                elevated: RGBAColor(hex: 0xFFFFFF),
                glass: RGBAColor(r: 255, g: 255, b: 255, a: 0.88),
                hover: RGBAColor(hex: 0xEAEDF2),
                selected: RGBAColor(hex: 0xE4E8EF),
                codeBg: RGBAColor(hex: 0xF6F8FA),
                textPrimary: RGBAColor(hex: 0x0F172A),
                textSecondary: RGBAColor(hex: 0x334155),
                textMuted: RGBAColor(hex: 0x4B5563),
                borderSubtle: RGBAColor(hex: 0xE2E8F0),
                borderStrong: RGBAColor(hex: 0xCBD5E1),
                trackOff: RGBAColor(hex: 0x94A3B8),
                buttonBg: RGBAColor(hex: 0x0F172A),
                buttonHover: RGBAColor(hex: 0x1E293B),
                buttonFg: RGBAColor(hex: 0xFFFFFF),
                okFg: RGBAColor(hex: 0x14532D),
                okBg: RGBAColor(hex: 0xEBFBF2),
                okBorder: RGBAColor(hex: 0x86EFAC),
                warnFg: RGBAColor(hex: 0x7C2D12),
                warnBg: RGBAColor(hex: 0xFEF5E7),
                warnBorder: RGBAColor(hex: 0xFDBA74),
                errFg: RGBAColor(hex: 0x9F1239),
                errBg: RGBAColor(hex: 0xFEF1F4),
                errBorder: RGBAColor(hex: 0xFDA4AF),
                accent: acc.accent,
                accentSoft: acc.accentSoft,
                accentBorder: acc.accentBorder,
                shadow1: SemanticShadow(color: RGBAColor(r: 15, g: 23, b: 42, a: 0.04), x: 0, y: 1, radius: 2),
                shadow2: SemanticShadow(color: RGBAColor(r: 15, g: 23, b: 42, a: 0.06), x: 0, y: 2, radius: 8)
            )
        case .dark:
            return DesignPalette(
                scheme: .dark,
                railBg: RGBAColor(hex: 0x111318),
                background: RGBAColor(hex: 0x161920),
                surface: RGBAColor(hex: 0x1B1F27),
                elevated: RGBAColor(hex: 0x222731),
                glass: RGBAColor(r: 22, g: 25, b: 32, a: 0.88),
                hover: RGBAColor(hex: 0x262C37),
                selected: RGBAColor(hex: 0x2A313D),
                codeBg: RGBAColor(hex: 0x11141A),
                textPrimary: RGBAColor(hex: 0xF8FAFC),
                textSecondary: RGBAColor(hex: 0xCBD5E1),
                textMuted: RGBAColor(hex: 0x94A3B8),
                borderSubtle: RGBAColor(hex: 0x262C36),
                borderStrong: RGBAColor(hex: 0x363E4C),
                trackOff: RGBAColor(hex: 0x64748B),
                buttonBg: RGBAColor(hex: 0xF8FAFC),
                buttonHover: RGBAColor(hex: 0xE2E8F0),
                buttonFg: RGBAColor(hex: 0x0F172A),
                okFg: RGBAColor(hex: 0x86EFAC),
                okBg: RGBAColor(hex: 0x132E21),
                okBorder: RGBAColor(hex: 0x225E3E),
                warnFg: RGBAColor(hex: 0xFCD34D),
                warnBg: RGBAColor(hex: 0x332611),
                warnBorder: RGBAColor(hex: 0x6E4F1D),
                errFg: RGBAColor(hex: 0xFDA4AF),
                errBg: RGBAColor(hex: 0x361923),
                errBorder: RGBAColor(hex: 0x783046),
                accent: acc.accent,
                accentSoft: acc.accentSoft,
                accentBorder: acc.accentBorder,
                shadow1: SemanticShadow(color: RGBAColor(r: 0, g: 0, b: 0, a: 0.34), x: 0, y: 1, radius: 2),
                shadow2: SemanticShadow(color: RGBAColor(r: 0, g: 0, b: 0, a: 0.40), x: 0, y: 2, radius: 9)
            )
        }
    }

    public static func accentPalette(scheme: ColorSchemeKind, workspace: WorkspaceId) -> AccentPalette {
        switch (scheme, workspace) {
        case (.light, .codex):
            AccentPalette(accent: RGBAColor(hex: 0x0D0D0D), accentSoft: RGBAColor(hex: 0xF2F2F2), accentBorder: RGBAColor(hex: 0xD4D4D4))
        case (.dark, .codex):
            AccentPalette(accent: RGBAColor(hex: 0xECECEC), accentSoft: RGBAColor(hex: 0x262626), accentBorder: RGBAColor(hex: 0x4A4A4A))
        case (.light, .claude):
            AccentPalette(accent: RGBAColor(hex: 0xA63D1C), accentSoft: RGBAColor(hex: 0xFDF1EC), accentBorder: RGBAColor(hex: 0xEFB9A3))
        case (.dark, .claude):
            AccentPalette(accent: RGBAColor(hex: 0xF3A084), accentSoft: RGBAColor(hex: 0x2C1E19), accentBorder: RGBAColor(hex: 0x693B2A))
        case (.light, .grok):
            AccentPalette(accent: RGBAColor(hex: 0x171717), accentSoft: RGBAColor(hex: 0xEEEEEE), accentBorder: RGBAColor(hex: 0xC4C4C4))
        case (.dark, .grok):
            AccentPalette(accent: RGBAColor(hex: 0xF8FAFC), accentSoft: RGBAColor(hex: 0x262C37), accentBorder: RGBAColor(hex: 0x475569))
        case (.light, .antigravity):
            AccentPalette(accent: RGBAColor(hex: 0x185ABC), accentSoft: RGBAColor(hex: 0xE8F0FE), accentBorder: RGBAColor(hex: 0xA3C2F7))
        case (.dark, .antigravity):
            AccentPalette(accent: RGBAColor(hex: 0x8AB4F8), accentSoft: RGBAColor(hex: 0x19263C), accentBorder: RGBAColor(hex: 0x335284))
        case (.light, .dsh):
            AccentPalette(accent: RGBAColor(hex: 0x2442C8), accentSoft: RGBAColor(hex: 0xECF0FF), accentBorder: RGBAColor(hex: 0xB3C2FF))
        case (.dark, .dsh):
            AccentPalette(accent: RGBAColor(hex: 0x7D95FF), accentSoft: RGBAColor(hex: 0x1A2342), accentBorder: RGBAColor(hex: 0x35498C))
        case (.light, .workspaces), (.light, .probe), (.light, .ops), (.light, .grokbot):
            AccentPalette(accent: RGBAColor(hex: 0x0369A1), accentSoft: RGBAColor(hex: 0xE0F2FE), accentBorder: RGBAColor(hex: 0x7DD3FC))
        case (.dark, .workspaces), (.dark, .probe), (.dark, .ops), (.dark, .grokbot):
            AccentPalette(accent: RGBAColor(hex: 0x38BDF8), accentSoft: RGBAColor(hex: 0x0C2538), accentBorder: RGBAColor(hex: 0x1D4F73))
        }
    }
}

public enum LayoutMetrics {
    public static let windowDefaultWidth: CGFloat = 1280
    public static let windowDefaultHeight: CGFloat = 820
    public static let windowMinWidth: CGFloat = 1000
    public static let windowMinHeight: CGFloat = 680

    public static let sideNavExpandedWidth: CGFloat = 196
    public static let sideNavCollapsedWidth: CGFloat = 48
    public static let sessionListWidth: CGFloat = 296
    public static let readingColumnMaxWidth: CGFloat = 768
    public static let consolePageMaxWidth: CGFloat = 1060
    public static let subagentDrawerWidth: CGFloat = 420
    public static let subagentDockThreshold: CGFloat = 960

    public static let narrowBreakpoint: CGFloat = 767
    public static let formControlHeight: CGFloat = 36

    public static let timelineRailLeft: CGFloat = 8
    public static let timelineRailWidth: CGFloat = 22
    public static let timelinePreviewMaxWidth: CGFloat = 288

    public static let composerMinHeight: CGFloat = 54
    public static let composerMaxHeight: CGFloat = 160
    public static let fallbackAppVersion: String = "2.1.37"

    public static func readingHorizontalInsets(
        availableWidth: CGFloat,
        hasTimelineRail: Bool,
        isSubagent: Bool = false
    ) -> (leading: CGFloat, trailing: CGFloat, top: CGFloat, bottom: CGFloat) {
        if availableWidth <= narrowBreakpoint {
            return (leading: 32, trailing: 16, top: 16, bottom: 16)
        }
        if isSubagent {
            return (leading: 36, trailing: 40, top: 16, bottom: 16)
        }
        let centered = (availableWidth - readingColumnMaxWidth) / 2.0
        let leftFloor: CGFloat = hasTimelineRail ? 42 : 46
        return (
            leading: max(leftFloor, centered),
            trailing: max(46, centered),
            top: 22,
            bottom: 32
        )
    }
}

public enum DesignFonts {
    public static func body(_ size: CGFloat = 14, weight: Font.Weight = .regular) -> Font {
        Font.system(size: size, weight: weight, design: .default).monospacedDigit()
    }

    public static func tabularBody(_ size: CGFloat = 14, weight: Font.Weight = .regular) -> Font {
        body(size, weight: weight)
    }

    public static func mono(_ size: CGFloat = 13, weight: Font.Weight = .regular) -> Font {
        Font.system(size: size, weight: weight, design: .monospaced)
    }

    public static func nsBodyFont(size: CGFloat = 15, weight: NSFont.Weight = .regular) -> NSFont {
        let base = NSFont.systemFont(ofSize: size, weight: weight)
        let pingFang = NSFontDescriptor(fontAttributes: [.name: "PingFang SC", .size: size])
        let descriptor = base.fontDescriptor.addingAttributes([
            .cascadeList: [pingFang],
            .featureSettings: [
                [
                    NSFontDescriptor.FeatureKey.typeIdentifier: kNumberSpacingType,
                    NSFontDescriptor.FeatureKey.selectorIdentifier: kMonospacedNumbersSelector,
                ],
            ],
        ])
        return NSFont(descriptor: descriptor, size: size) ?? base
    }

    public static func nsMonoFont(size: CGFloat = 13, weight: NSFont.Weight = .regular) -> NSFont {
        NSFont.monospacedSystemFont(ofSize: size, weight: weight)
    }
}

private struct DesignPaletteKey: EnvironmentKey {
    static let defaultValue = DesignPalette.resolve(scheme: .light, workspace: .codex)
}

public extension EnvironmentValues {
    var agentHubPalette: DesignPalette {
        get { self[DesignPaletteKey.self] }
        set { self[DesignPaletteKey.self] = newValue }
    }
}

@MainActor
public final class AccessibilityMotionObserver: ObservableObject {
    @Published public private(set) var reduceMotion: Bool

    public init() {
        self.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
            }
        }
    }
}
