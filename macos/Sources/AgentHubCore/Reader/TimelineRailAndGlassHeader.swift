import AppKit
import CoreImage
import SwiftUI

// MARK: - Left 22pt Timeline Rail with Virtualized 12pt Ticks & min(288pt, 60vw) Hover/Focus Preview

public struct TimelineRailView: View {
    @Environment(\.agentHubPalette) private var palette
    public let entries: [TimelineEntry]
    public let activeId: String?
    public let availableWidth: CGFloat
    public var loadOlder: (() -> Void)?
    public var loadingOlder: Bool = false
    public let onSelectEntry: (TimelineEntry) -> Void

    @State private var hoveredIndex: Int?
    @State private var focusedIndex: Int?
    @State private var windowStart: Int = 0
    @State private var railHeightState: CGFloat = 600

    public init(
        entries: [TimelineEntry],
        activeId: String?,
        availableWidth: CGFloat,
        loadOlder: (() -> Void)? = nil,
        loadingOlder: Bool = false,
        onSelectEntry: @escaping (TimelineEntry) -> Void
    ) {
        self.entries = entries
        self.activeId = activeId
        self.availableWidth = availableWidth
        self.loadOlder = loadOlder
        self.loadingOlder = loadingOlder
        self.onSelectEntry = onSelectEntry
    }

    public var body: some View {
        let olderOffset: CGFloat = (loadOlder != nil) ? 18 : 0
        let topPadding: CGFloat = 20 + olderOffset
        let usableHeight = max(24, railHeightState - topPadding - 20)
        let windowCapacity = Self.virtualWindowCapacity(usableHeight: usableHeight)
        let first = min(windowStart, max(0, entries.count - 1))
        let last = min(entries.count, first + windowCapacity)
        let visibleSlice: ArraySlice<TimelineEntry> = (first < last) ? entries[first ..< last] : []
        let previewIdx = hoveredIndex ?? focusedIndex
        let previewEntry: TimelineEntry? = if let idx = previewIdx, entries.indices.contains(idx) {
            entries[idx]
        } else {
            nil
        }

        GeometryReader { geo in
            ZStack(alignment: .topLeading) {
                if let loadOlder {
                    Button(action: loadOlder) {
                        Text("↑")
                            .font(DesignFonts.body(11, weight: .semibold))
                            .foregroundStyle(palette.textMuted.color)
                            .frame(width: LayoutMetrics.timelineRailWidth, height: 18)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(PlainHitButtonStyle())
                    .disabled(loadingOlder)
                    .padding(.top, 2)
                    .padding(.leading, LayoutMetrics.timelineRailLeft)
                    .accessibilityLabel("加载更早的时间线记录")
                }

                ScrollViewReader { scrollProxy in
                    ScrollView(.vertical, showsIndicators: false) {
                        VStack(alignment: .leading, spacing: 0) {
                            if first > 0 {
                                Color.clear
                                    .frame(width: LayoutMetrics.timelineRailWidth, height: CGFloat(first) * 12)
                            }

                            let effectiveActiveId = (activeId != nil && entries.contains(where: { $0.id == activeId }))
                                ? activeId
                                : entries.last?.id
                            ForEach(Array(visibleSlice.enumerated()), id: \.element.id) { offset, entry in
                                let globalIndex = first + offset
                                let isActive = (entry.id == effectiveActiveId)
                                let isHighlighted = (hoveredIndex == globalIndex) || (focusedIndex == globalIndex)
                                TimelineRailTickRow(
                                    index: globalIndex,
                                    totalCount: entries.count,
                                    title: entry.title,
                                    isActive: isActive,
                                    isHighlighted: isHighlighted,
                                    isFocused: focusedIndex == globalIndex,
                                    activeColor: palette.accent,
                                    inactiveColor: palette.trackOff,
                                    onHoverChanged: { hovering in
                                        if hovering {
                                            hoveredIndex = globalIndex
                                        } else if hoveredIndex == globalIndex {
                                            hoveredIndex = nil
                                        }
                                    },
                                    onFocusChanged: { focused in
                                        if focused {
                                            focusedIndex = globalIndex
                                        } else if focusedIndex == globalIndex {
                                            focusedIndex = nil
                                        }
                                    },
                                    onNavigateKey: { keyCode in
                                        if let next = Self.nextKeyboardFocusIndex(
                                            currentIndex: globalIndex,
                                            count: entries.count,
                                            keyCode: keyCode
                                        ) {
                                            if next < first || next >= last {
                                                windowStart = max(0, min(max(0, entries.count - windowCapacity), next - windowCapacity / 2))
                                            }
                                            focusedIndex = next
                                            if entries.indices.contains(next) {
                                                scrollProxy.scrollTo(entries[next].id, anchor: .center)
                                            }
                                            return true
                                        }
                                        return false
                                    },
                                    onActivate: {
                                        onSelectEntry(entry)
                                    }
                                )
                                .id(entry.id)
                            }

                            if last < entries.count {
                                Color.clear
                                    .frame(width: LayoutMetrics.timelineRailWidth, height: CGFloat(entries.count - last) * 12)
                            }
                        }
                    }
                    .frame(width: LayoutMetrics.timelineRailWidth, height: max(24, geo.size.height - topPadding - 20))
                    .padding(.top, topPadding)
                    .padding(.leading, LayoutMetrics.timelineRailLeft)
                    .onChange(of: activeId) { nextActiveId in
                        guard hoveredIndex == nil, focusedIndex == nil,
                              let nextActiveId,
                              let activeIdx = entries.firstIndex(where: { $0.id == nextActiveId })
                        else { return }
                        if activeIdx < first || activeIdx >= last {
                            windowStart = max(0, min(max(0, entries.count - windowCapacity), activeIdx - windowCapacity / 2))
                        }
                        scrollProxy.scrollTo(nextActiveId, anchor: .center)
                    }
                }
            }
            .onAppear {
                if geo.size.height > 1, abs(railHeightState - geo.size.height) > 0.5 {
                    let h = geo.size.height
                    DispatchQueue.main.async { railHeightState = h }
                }
            }
            .onChange(of: geo.size.height) { nextH in
                if nextH > 1, abs(railHeightState - nextH) > 0.5 {
                    DispatchQueue.main.async { railHeightState = nextH }
                }
            }
        }
        .frame(width: 34)
        // Render hover/focus preview OUTSIDE the 34pt GeometryReader so width sizes naturally up to min(288pt, 60vw)!
        .overlay(alignment: .topLeading) {
            if let hovered = previewEntry, let idx = previewIdx {
                let previewMax = Self.previewMaxWidth(for: availableWidth)
                let visibleOffset = max(0, idx - first)
                let centerY = Self.previewTopY(
                    forTickIndex: visibleOffset,
                    topPadding: topPadding,
                    tickHeight: 12,
                    railHeight: railHeightState
                )
                let estimatedHalfHeight: CGFloat = (hovered.preview?.isEmpty == false) ? 28 : 16
                let cardTopY = max(6, min(max(6, railHeightState - estimatedHalfHeight * 2 - 6), centerY - estimatedHalfHeight))

                Color.clear
                    .frame(width: previewMax, height: max(48, railHeightState), alignment: .topLeading)
                    .overlay(alignment: .topLeading) {
                        TimelinePreviewCardView(entry: hovered, maxWidth: previewMax)
                            .offset(x: 28, y: cardTopY)
                    }
                    .allowsHitTesting(false)
            }
        }
        .onChange(of: entries) { _ in
            hoveredIndex = nil
            focusedIndex = nil
            windowStart = 0
        }
    }

    public static func previewMaxWidth(for availableWidth: CGFloat) -> CGFloat {
        min(LayoutMetrics.timelinePreviewMaxWidth, max(160, availableWidth * 0.60))
    }

    public static func virtualWindowCapacity(usableHeight: CGFloat) -> Int {
        max(100, Int(ceil(max(24, usableHeight) / 12.0)) + 20)
    }

    public static func previewTopY(
        forTickIndex index: Int,
        topPadding: CGFloat = 20,
        tickHeight: CGFloat = 12,
        railHeight: CGFloat
    ) -> CGFloat {
        let rawCenter = topPadding + CGFloat(max(0, index)) * tickHeight + tickHeight * 0.5
        return max(12, min(max(12, railHeight - 12), rawCenter))
    }

    /// Maps macOS keyCode (`126` Up, `125` Down, `115` Home, `119` End) to next tick index.
    public static func nextKeyboardFocusIndex(currentIndex: Int, count: Int, keyCode: UInt16) -> Int? {
        guard count > 0 else { return nil }
        switch keyCode {
        case 126: // ArrowUp
            return max(0, currentIndex - 1)
        case 125: // ArrowDown
            return min(count - 1, currentIndex + 1)
        case 115: // Home
            return 0
        case 119: // End
            return count - 1
        default:
            return nil
        }
    }
}

public struct TimelinePreviewCardView: View {
    @Environment(\.agentHubPalette) private var palette
    public let entry: TimelineEntry
    public let maxWidth: CGFloat

    public init(entry: TimelineEntry, maxWidth: CGFloat) {
        self.entry = entry
        self.maxWidth = maxWidth
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(entry.title)
                .font(DesignFonts.body(12, weight: .semibold))
                .foregroundStyle(palette.textPrimary.color)
                .lineLimit(1)
            if let preview = entry.preview, !preview.isEmpty {
                Text(preview)
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.textSecondary.color)
                    .lineLimit(3)
            }
        }
        .padding(.vertical, 8)
        .padding(.leading, 12)
        .padding(.trailing, 10)
        .frame(maxWidth: maxWidth, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(palette.elevated.color)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(palette.borderStrong.color, lineWidth: 1)
        )
        .overlay(alignment: .leading) {
            Rectangle()
                .fill(palette.accent.color)
                .frame(width: 3)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .shadow(color: Color.black.opacity(0.14), radius: 8, x: 0, y: 2)
    }
}

private struct TimelineRailTickRow: NSViewRepresentable {
    let index: Int
    let totalCount: Int
    let title: String
    let isActive: Bool
    let isHighlighted: Bool
    let isFocused: Bool
    let activeColor: RGBAColor
    let inactiveColor: RGBAColor
    let onHoverChanged: (Bool) -> Void
    let onFocusChanged: (Bool) -> Void
    let onNavigateKey: (UInt16) -> Bool
    let onActivate: () -> Void

    func makeNSView(context: Context) -> TimelineTickNSButton {
        let v = TimelineTickNSButton()
        v.apply(self)
        return v
    }

    func updateNSView(_ nsView: TimelineTickNSButton, context: Context) {
        nsView.apply(self)
    }
}

private final class TimelineTickNSButton: NSView {
    private var spec: TimelineRailTickRow?
    private var tracking: NSTrackingArea?

    override var acceptsFirstResponder: Bool { true }
    override var intrinsicContentSize: NSSize {
        NSSize(width: LayoutMetrics.timelineRailWidth, height: 12)
    }

    func apply(_ next: TimelineRailTickRow) {
        let becameFocused = next.isFocused && !(spec?.isFocused ?? false)
        spec = next
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("时间线：\(next.title)")
        needsDisplay = true
        if becameFocused, let win = window, win.firstResponder !== self {
            DispatchQueue.main.async { [weak self, weak win] in
                guard let self, let win, self.window === win else { return }
                win.makeFirstResponder(self)
            }
        }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) {
        spec?.onHoverChanged(true)
    }

    override func mouseExited(with event: NSEvent) {
        spec?.onHoverChanged(false)
    }

    override func mouseDown(with event: NSEvent) {
        spec?.onActivate()
    }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { spec?.onFocusChanged(true) }
        needsDisplay = true
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { spec?.onFocusChanged(false) }
        needsDisplay = true
        return ok
    }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 76 || event.keyCode == 49 { // Return / Enter / Space
            spec?.onActivate()
            return
        }
        if spec?.onNavigateKey(event.keyCode) == true {
            return
        }
        super.keyDown(with: event)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let spec else { return }
        let barW: CGFloat = spec.isActive ? 14 : (spec.isHighlighted ? 12 : 8)
        let barH: CGFloat = spec.isActive ? 3 : 2
        let rect = NSRect(x: 0, y: floor((bounds.height - barH) * 0.5), width: barW, height: barH)
        let path = NSBezierPath(roundedRect: rect, xRadius: barH * 0.5, yRadius: barH * 0.5)
        (spec.isActive ? spec.activeColor.nsColor : spec.inactiveColor.nsColor).setFill()
        path.fill()
    }
}

public struct HistoryLoadingBarView: View {
    @Environment(\.agentHubPalette) private var palette
    public let loading: Bool
    public let error: String?
    public let onRetry: () -> Void

    public init(loading: Bool, error: String?, onRetry: @escaping () -> Void) {
        self.loading = loading
        self.error = error
        self.onRetry = onRetry
    }

    public var body: some View {
        if let error, !error.isEmpty {
            HStack(spacing: 4) {
                Spacer(minLength: 0)
                Text("加载失败，")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
                Button(action: onRetry) {
                    Text("重试")
                        .underline()
                        .padding(.horizontal, 4)
                        .padding(.vertical, 2)
                        .contentShape(Rectangle())
                }
                .font(DesignFonts.body(12, weight: .medium))
                .foregroundStyle(palette.accent.color)
                .buttonStyle(PlainHitButtonStyle())
                Spacer(minLength: 0)
            }
            .padding(.vertical, 4)
            .padding(.horizontal, 40)
            .background(palette.background.color.opacity(0.92))
        } else if loading {
            HStack {
                Spacer(minLength: 0)
                Text("正在加载较早消息…")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.textMuted.color)
                Spacer(minLength: 0)
            }
            .padding(.vertical, 4)
            .padding(.horizontal, 40)
            .background(palette.background.color.opacity(0.92))
        }
    }
}

public struct TimelineHistoryStatusView: View {
    @Environment(\.agentHubPalette) private var palette
    public let active: Bool
    public let busy: Bool
    public let error: String?
    public let hasNewer: Bool
    public let onLoadNewer: () -> Void
    public let onReturnLatest: () -> Void

    public init(
        active: Bool,
        busy: Bool,
        error: String?,
        hasNewer: Bool,
        onLoadNewer: @escaping () -> Void,
        onReturnLatest: @escaping () -> Void
    ) {
        self.active = active
        self.busy = busy
        self.error = error
        self.hasNewer = hasNewer
        self.onLoadNewer = onLoadNewer
        self.onReturnLatest = onReturnLatest
    }

    public var body: some View {
        if busy || (error?.isEmpty == false) || active {
            VStack(spacing: 6) {
                if busy {
                    Text("正在定位历史…")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textMuted.color)
                }
                if let error, !error.isEmpty {
                    Text(error)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.errFg.color)
                }
                if active {
                    HStack(spacing: 12) {
                        if hasNewer {
                            Button(action: onLoadNewer) {
                                Text("加载较新记录")
                                    .underline()
                                    .padding(.horizontal, 4)
                                    .padding(.vertical, 2)
                                    .contentShape(Rectangle())
                            }
                            .font(DesignFonts.body(12, weight: .medium))
                            .foregroundStyle(palette.accent.color)
                            .buttonStyle(PlainHitButtonStyle())
                            .disabled(busy)
                        }
                        Button(action: onReturnLatest) {
                            Text("返回最新")
                                .underline()
                                .padding(.horizontal, 4)
                                .padding(.vertical, 2)
                                .contentShape(Rectangle())
                        }
                        .font(DesignFonts.body(12, weight: .medium))
                        .foregroundStyle(palette.accent.color)
                        .buttonStyle(PlainHitButtonStyle())
                    }
                    .padding(.vertical, 6)
                    .padding(.horizontal, 14)
                    .background(Capsule().fill(palette.elevated.color))
                    .overlay(Capsule().strokeBorder(palette.borderStrong.color, lineWidth: 1))
                }
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 16)
        }
    }
}

// MARK: - Exact --g 88% + CIGaussianBlur(radius: 10) Stage Header & .inline-rename Bar

public struct BackdropGaussianBlurView: NSViewRepresentable {
    public var radius: Double

    public init(radius: Double = 10) {
        self.radius = radius
    }

    public func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.wantsLayer = true
        view.layer?.masksToBounds = true
        view.layerUsesCoreImageFilters = true
        if let blur = CIFilter(name: "CIGaussianBlur") {
            blur.setValue(radius, forKey: kCIInputRadiusKey)
            view.backgroundFilters = [blur]
        }
        return view
    }

    public func updateNSView(_ nsView: NSView, context: Context) {}
}

public struct WorkspaceStageHeaderView<TrailingContent: View>: View {
    @Environment(\.agentHubPalette) private var palette
    public let title: String
    public let subtitle: String?
    public let statusBadge: String?
    public let statusTone: StatusChipTone
    public let availableWidth: CGFloat
    public let inlineRenameActive: Bool
    public let initialRenameText: String
    public let onCommitInlineRename: ((String) async throws -> Void)?
    public let onCancelInlineRename: (() -> Void)?
    @ViewBuilder public let trailing: () -> TrailingContent

    @State private var renameDraft: String = ""
    @State private var renameSaving: Bool = false
    @State private var renameError: String?
    @State private var didSeedCurrentRenameSession: Bool = false

    public init(
        title: String,
        subtitle: String? = nil,
        statusBadge: String? = nil,
        statusTone: StatusChipTone = .neutral,
        availableWidth: CGFloat = 1200,
        inlineRenameActive: Bool = false,
        initialRenameText: String = "",
        onCommitInlineRename: ((String) async throws -> Void)? = nil,
        onCancelInlineRename: (() -> Void)? = nil,
        @ViewBuilder trailing: @escaping () -> TrailingContent
    ) {
        self.title = title
        self.subtitle = subtitle
        self.statusBadge = statusBadge
        self.statusTone = statusTone
        self.availableWidth = availableWidth
        self.inlineRenameActive = inlineRenameActive
        self.initialRenameText = initialRenameText
        self.onCommitInlineRename = onCommitInlineRename
        self.onCancelInlineRename = onCancelInlineRename
        self.trailing = trailing
    }

    public var body: some View {
        let isNarrowHeader = availableWidth <= 520
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(DesignFonts.body(15, weight: .bold))
                        .foregroundStyle(palette.textPrimary.color)
                        .lineLimit(2)
                        .fixedSize(horizontal: false, vertical: true)

                    if let subtitle, !subtitle.isEmpty {
                        Text(subtitle)
                            .font(DesignFonts.mono(12))
                            .foregroundStyle(palette.textMuted.color)
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                            .help(subtitle)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                HStack(spacing: 8) {
                    if !isNarrowHeader, let statusBadge, !statusBadge.isEmpty {
                        StatusChip(statusBadge, tone: statusTone)
                    }
                    trailing()
                }
                .fixedSize(horizontal: true, vertical: false)
            }
            .padding(.horizontal, isNarrowHeader ? 16 : 24)
            .padding(.vertical, 10)
            .frame(minHeight: 54)
            .background(
                ZStack {
                    BackdropGaussianBlurView(radius: 10)
                    palette.glass.color
                }
            )
            .overlay(alignment: .bottom) {
                Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
            }

            if inlineRenameActive {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        IMEGuardedInlineTextField(
                            text: $renameDraft,
                            maxLength: 200,
                            placeholder: "输入会话名称",
                            autoFocus: true,
                            onSubmit: { Task { await submitInlineRename() } },
                            onCancel: { onCancelInlineRename?() }
                        )
                        .frame(height: 32)
                        .padding(.horizontal, 8)
                        .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
                        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))

                        Button {
                            Task { await submitInlineRename() }
                        } label: {
                            LucideIcon(.check, size: 16)
                        }
                        .buttonStyle(IconButtonStyle(dimension: 32))
                        .disabled(renameSaving)
                        .accessibilityLabel("保存名称")

                        Button {
                            onCancelInlineRename?()
                        } label: {
                            LucideIcon(.x, size: 16)
                        }
                        .buttonStyle(IconButtonStyle(dimension: 32))
                        .disabled(renameSaving)
                        .accessibilityLabel("取消改名")
                    }
                    if let renameError {
                        Text(renameError)
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.errFg.color)
                    }
                }
                .padding(.horizontal, 24)
                .padding(.vertical, 12)
                .background(palette.surface.color)
                .overlay(alignment: .bottom) {
                    Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
                }
                .onAppear {
                    if !didSeedCurrentRenameSession {
                        didSeedCurrentRenameSession = true
                        renameDraft = initialRenameText
                        renameError = nil
                    }
                }
            }
        }
        .onChange(of: inlineRenameActive) { active in
            if active {
                if !didSeedCurrentRenameSession {
                    didSeedCurrentRenameSession = true
                    renameDraft = initialRenameText
                    renameError = nil
                }
            } else {
                didSeedCurrentRenameSession = false
                renameError = nil
            }
        }
    }

    private func submitInlineRename() async {
        guard let onCommitInlineRename, !renameSaving else { return }
        let trimmed = renameDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            renameError = "名称不能为空"
            return
        }
        renameSaving = true
        renameError = nil
        defer { renameSaving = false }
        do {
            try await onCommitInlineRename(trimmed)
        } catch {
            renameError = error.localizedDescription
        }
    }
}

// MARK: - AppKit NSTextField + Custom Field Editor Capturing Pre-keyDown IME Marked-Text State

public final class IMEFieldEditorTextView: NSTextView {
    public private(set) var wasComposingBeforeKeyDown: Bool = false

    override public func keyDown(with event: NSEvent) {
        let pre = hasMarkedText()
        wasComposingBeforeKeyDown = pre
        defer { wasComposingBeforeKeyDown = false }
        super.keyDown(with: event)
    }
}

private final class IMEGuardedTextFieldCell: NSTextFieldCell {
    private lazy var customFieldEditor: IMEFieldEditorTextView = {
        let editor = IMEFieldEditorTextView(frame: .zero)
        editor.isFieldEditor = true
        editor.isRichText = false
        editor.importsGraphics = false
        editor.allowsUndo = true
        return editor
    }()

    override func fieldEditor(for controlView: NSView) -> NSTextView? {
        customFieldEditor
    }
}

public final class IMEGuardedNSTextField: NSTextField {
    override public class var cellClass: AnyClass? {
        get { IMEGuardedTextFieldCell.self }
        set {}
    }

    public var autoFocusOnWindowAttach: Bool = false
    private var didAutoFocus: Bool = false

    override public func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if autoFocusOnWindowAttach, !didAutoFocus, let window {
            didAutoFocus = true
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, self.window === window else { return }
                window.makeFirstResponder(self)
            }
        }
    }
}

public struct IMEGuardedInlineTextField: NSViewRepresentable {
    @Binding public var text: String
    public var maxLength: Int
    public var placeholder: String
    public var autoFocus: Bool
    public var onSubmit: () -> Void
    public var onCancel: () -> Void
    public var onBlur: (() -> Void)?
    public var onMoveUp: (() -> Void)?
    public var onMoveDown: (() -> Void)?

    public init(
        text: Binding<String>,
        maxLength: Int = 200,
        placeholder: String = "",
        autoFocus: Bool = false,
        onSubmit: @escaping () -> Void,
        onCancel: @escaping () -> Void,
        onBlur: (() -> Void)? = nil,
        onMoveUp: (() -> Void)? = nil,
        onMoveDown: (() -> Void)? = nil
    ) {
        self._text = text
        self.maxLength = maxLength
        self.placeholder = placeholder
        self.autoFocus = autoFocus
        self.onSubmit = onSubmit
        self.onCancel = onCancel
        self.onBlur = onBlur
        self.onMoveUp = onMoveUp
        self.onMoveDown = onMoveDown
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    public func makeNSView(context: Context) -> IMEGuardedNSTextField {
        let tf = IMEGuardedNSTextField()
        tf.cell = IMEGuardedTextFieldCell(textCell: "")
        tf.isEditable = true
        tf.isSelectable = true
        tf.isBordered = false
        tf.drawsBackground = false
        tf.focusRingType = .none
        tf.font = DesignFonts.nsBodyFont(size: 13)
        tf.placeholderString = placeholder
        tf.stringValue = text
        tf.autoFocusOnWindowAttach = autoFocus
        tf.delegate = context.coordinator
        return tf
    }

    public func updateNSView(_ nsView: IMEGuardedNSTextField, context: Context) {
        context.coordinator.parent = self
        if (nsView.currentEditor() as? NSTextView)?.hasMarkedText() != true && nsView.stringValue != text {
            nsView.stringValue = text
        }
    }

    @MainActor
    public final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: IMEGuardedInlineTextField

        init(parent: IMEGuardedInlineTextField) {
            self.parent = parent
        }

        public func controlTextDidChange(_ obj: Notification) {
            guard let tf = obj.object as? NSTextField else { return }
            if (tf.currentEditor() as? NSTextView)?.hasMarkedText() == true { return }
            var val = tf.stringValue
            let ns = val as NSString
            if ns.length > parent.maxLength {
                val = ns.substring(to: parent.maxLength)
                tf.stringValue = val
            }
            parent.text = val
        }

        public func controlTextDidEndEditing(_ obj: Notification) {
            parent.onBlur?()
        }

        public func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            let wasComposing = (textView as? IMEFieldEditorTextView)?.wasComposingBeforeKeyDown == true
            if wasComposing || textView.hasMarkedText() {
                return false
            }
            if commandSelector == #selector(NSResponder.insertNewline(_:)) {
                parent.onSubmit()
                return true
            }
            if commandSelector == #selector(NSResponder.cancelOperation(_:)) {
                parent.onCancel()
                return true
            }
            if commandSelector == #selector(NSResponder.moveUp(_:)), let onMoveUp = parent.onMoveUp {
                onMoveUp()
                return true
            }
            if commandSelector == #selector(NSResponder.moveDown(_:)), let onMoveDown = parent.onMoveDown {
                onMoveDown()
                return true
            }
            return false
        }
    }
}

// MARK: - Modal Attachment Image Preview Overlay (Supports Backdrop Click & Escape Close)

public struct ImagePreviewModalView: View {
    @Environment(\.agentHubPalette) private var palette
    public let title: String
    public let image: NSImage
    public let onClose: () -> Void

    public init(title: String, image: NSImage, onClose: @escaping () -> Void) {
        self.title = title
        self.image = image
        self.onClose = onClose
    }

    public var body: some View {
        ZStack {
            ModalScopePresenceView()
                .frame(width: 0, height: 0)

            Color.black.opacity(0.55)
                .ignoresSafeArea()
                .onTapGesture(perform: onClose)

            VStack(spacing: 12) {
                HStack {
                    Text(title)
                        .font(DesignFonts.body(13, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                        .lineLimit(1)
                    Spacer()
                    Button(action: onClose) {
                        LucideIcon(.x, size: 18)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 32))
                    .keyboardShortcut(.cancelAction)
                    .accessibilityLabel("关闭图片预览")
                }
                Image(nsImage: image)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: 880, maxHeight: 620)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
            .padding(18)
            .background(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(palette.elevated.color)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .strokeBorder(palette.borderStrong.color, lineWidth: 1)
            )
            .padding(32)
        }
    }
}
