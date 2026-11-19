import AppKit
import CoreImage
import SwiftUI

// MARK: - Left 22pt Timeline Rail with Virtualized 12pt Ticks & min(288pt, 60vw) Hover Preview

public struct TimelineRailView: View {
    @Environment(\.agentHubPalette) private var palette
    public let entries: [TimelineEntry]
    public let activeId: String?
    public let availableWidth: CGFloat
    public let onSelectEntry: (TimelineEntry) -> Void

    @State private var hoveredEntry: TimelineEntry?

    public init(
        entries: [TimelineEntry],
        activeId: String?,
        availableWidth: CGFloat,
        onSelectEntry: @escaping (TimelineEntry) -> Void
    ) {
        self.entries = entries
        self.activeId = activeId
        self.availableWidth = availableWidth
        self.onSelectEntry = onSelectEntry
    }

    public var body: some View {
        GeometryReader { geo in
            let usableHeight = max(24, geo.size.height - 40)
            let maxTicks = max(1, Int(floor(usableHeight / 12.0)))
            let sampled = Self.sampleEntries(entries, maxCount: maxTicks)

            ZStack(alignment: .topLeading) {
                VStack(spacing: 0) {
                    ForEach(sampled) { entry in
                        let isActive = (entry.id == activeId)
                        let isHovered = (hoveredEntry?.id == entry.id)
                        Button {
                            onSelectEntry(entry)
                        } label: {
                            HStack(spacing: 0) {
                                Capsule()
                                    .fill((isActive ? palette.accent : palette.trackOff).color)
                                    .frame(width: isActive ? 14 : (isHovered ? 12 : 8), height: isActive ? 3 : 2)
                                Spacer(minLength: 0)
                            }
                            .frame(width: LayoutMetrics.timelineRailWidth, height: 12)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(PlainHitButtonStyle())
                        .onHover { hovering in
                            if hovering {
                                hoveredEntry = entry
                            } else if hoveredEntry?.id == entry.id {
                                hoveredEntry = nil
                            }
                        }
                        .accessibilityLabel(entry.title)
                    }
                }
                .padding(.top, 20)
                .padding(.leading, LayoutMetrics.timelineRailLeft)

                if let hovered = hoveredEntry {
                    let previewMax = min(LayoutMetrics.timelinePreviewMaxWidth, max(160, availableWidth * 0.60))
                    VStack(alignment: .leading, spacing: 4) {
                        Text(hovered.title)
                            .font(DesignFonts.body(12, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                            .lineLimit(1)
                        if let preview = hovered.preview, !preview.isEmpty {
                            Text(preview)
                                .font(DesignFonts.body(11))
                                .foregroundStyle(palette.textSecondary.color)
                                .lineLimit(3)
                        }
                    }
                    .padding(10)
                    .frame(maxWidth: previewMax, alignment: .leading)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(palette.elevated.color)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                    )
                    .shadow(color: Color.black.opacity(0.14), radius: 8, x: 0, y: 2)
                    .offset(x: 28, y: 20)
                    .allowsHitTesting(false)
                }
            }
        }
        .frame(width: 34)
    }

    public static func sampleEntries(_ entries: [TimelineEntry], maxCount: Int) -> [TimelineEntry] {
        guard entries.count > maxCount, maxCount > 1 else { return entries }
        var out: [TimelineEntry] = []
        let step = Double(entries.count - 1) / Double(maxCount - 1)
        for i in 0 ..< maxCount {
            let idx = min(entries.count - 1, Int((Double(i) * step).rounded()))
            if out.last?.id != entries[idx].id {
                out.append(entries[idx])
            }
        }
        return out
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
    public let statusBadge: String?
    public let statusTone: StatusChipTone
    public let isCompactMobile: Bool
    public let onBack: (() -> Void)?
    public let inlineRenameActive: Bool
    public let initialRenameText: String
    public let onCommitInlineRename: ((String) async throws -> Void)?
    public let onCancelInlineRename: (() -> Void)?
    @ViewBuilder public let trailing: () -> TrailingContent

    @State private var renameDraft: String = ""
    @State private var renameSaving: Bool = false
    @State private var renameError: String?

    public init(
        title: String,
        statusBadge: String? = nil,
        statusTone: StatusChipTone = .neutral,
        isCompactMobile: Bool = false,
        onBack: (() -> Void)? = nil,
        inlineRenameActive: Bool = false,
        initialRenameText: String = "",
        onCommitInlineRename: ((String) async throws -> Void)? = nil,
        onCancelInlineRename: (() -> Void)? = nil,
        @ViewBuilder trailing: @escaping () -> TrailingContent
    ) {
        self.title = title
        self.statusBadge = statusBadge
        self.statusTone = statusTone
        self.isCompactMobile = isCompactMobile
        self.onBack = onBack
        self.inlineRenameActive = inlineRenameActive
        self.initialRenameText = initialRenameText
        self.onCommitInlineRename = onCommitInlineRename
        self.onCancelInlineRename = onCancelInlineRename
        self.trailing = trailing
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                if isCompactMobile, let onBack {
                    Button(action: onBack) {
                        LucideIcon(.arrowLeft, size: 17)
                    }
                    .buttonStyle(IconButtonStyle(dimension: 32))
                    .accessibilityLabel("返回列表")
                }

                Text(title)
                    .font(DesignFonts.body(14, weight: .semibold))
                    .foregroundStyle(palette.textPrimary.color)
                    .lineLimit(1)

                if let statusBadge, !statusBadge.isEmpty {
                    StatusChip(statusBadge, tone: statusTone)
                }

                Spacer(minLength: 8)
                trailing()
            }
            .padding(.horizontal, 20)
            .frame(height: 52)
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
                    renameDraft = initialRenameText
                    renameError = nil
                }
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

// MARK: - AppKit NSTextField Subclass Guarding Enter/Esc Against Active IME Marked Text

public struct IMEGuardedInlineTextField: NSViewRepresentable {
    @Binding public var text: String
    public var maxLength: Int
    public var placeholder: String
    public var onSubmit: () -> Void
    public var onCancel: () -> Void
    public var onBlur: (() -> Void)?

    public init(
        text: Binding<String>,
        maxLength: Int = 200,
        placeholder: String = "",
        onSubmit: @escaping () -> Void,
        onCancel: @escaping () -> Void,
        onBlur: (() -> Void)? = nil
    ) {
        self._text = text
        self.maxLength = maxLength
        self.placeholder = placeholder
        self.onSubmit = onSubmit
        self.onCancel = onCancel
        self.onBlur = onBlur
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    public func makeNSView(context: Context) -> NSTextField {
        let tf = NSTextField()
        tf.isBordered = false
        tf.drawsBackground = false
        tf.focusRingType = .none
        tf.font = DesignFonts.nsBodyFont(size: 13)
        tf.placeholderString = placeholder
        tf.stringValue = text
        tf.delegate = context.coordinator
        return tf
    }

    public func updateNSView(_ nsView: NSTextField, context: Context) {
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
            if textView.hasMarkedText() {
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
