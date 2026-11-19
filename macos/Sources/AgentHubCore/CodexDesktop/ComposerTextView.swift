import AppKit
import SwiftUI

// MARK: - AppKit ComposerNSTextView with 3-Layer IME Marked-Text Guard & Paste/Drop Ingress

public final class ComposerNSTextView: NSTextView {
    public static let minManualHeight: CGFloat = LayoutMetrics.composerMinHeight // 54pt
    public static let maxManualHeight: CGFloat = LayoutMetrics.composerMaxHeight // 160pt

    public var placeholderText: String = "" {
        didSet { needsDisplay = true }
    }

    public var onTextAndSelectionChanged: ((String, Int, Int) -> Void)?
    public var onCommandEnterSend: (() -> Void)?
    public var onInlineCompletionKey: ((InlineKeyAction) -> Bool)?
    /// Async paste hook returning `true` when disposition == "text" (fall back to `super.paste` preserving Undo).
    public var onInterceptPaste: ((@escaping (Bool) -> Void) -> Void)?
    public var onFilesDropped: (([String], NSPoint) -> Void)?

    public enum InlineKeyAction: Equatable, Sendable {
        case arrowUp
        case arrowDown
        case confirmSelection
        case escapeDismiss
    }

    /// Captures `hasMarkedText()` BEFORE `inputContext?.handleEvent(event)` clears composition on candidate commit!
    public private(set) var wasComposingBeforeKeyDown: Bool = false

    public static func clampManualHeight(_ proposed: CGFloat) -> CGFloat {
        min(maxManualHeight, max(minManualHeight, round(proposed)))
    }

    public convenience init() {
        self.init(frame: .zero)
        isEditable = true
        isSelectable = true
        isRichText = false
        importsGraphics = false
        allowsUndo = true
        drawsBackground = false
        font = DesignFonts.nsBodyFont(size: 14)
        textContainerInset = NSSize(width: 8, height: 8)
        textContainer?.lineFragmentPadding = 2
        registerForDraggedTypes([.fileURL])
    }

    override public func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        if string.isEmpty && !hasMarkedText() && !placeholderText.isEmpty {
            let attrs: [NSAttributedString.Key: Any] = [
                .font: font ?? DesignFonts.nsBodyFont(size: 14),
                .foregroundColor: NSColor.placeholderTextColor,
            ]
            let rect = NSRect(x: 10, y: 8, width: max(40, bounds.width - 20), height: max(20, bounds.height - 16))
            (placeholderText as NSString).draw(in: rect, withAttributes: attrs)
        }
    }

    // MARK: - Layer 2 IME Guard: performKeyEquivalent Intercepts ⌘+Return / Ctrl+Return

    override public func performKeyEquivalent(with event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let isCmdOrCtrl = flags.contains(.command) || flags.contains(.control)
        let hasShift = flags.contains(.shift)
        let isReturnKey = (event.keyCode == 36 || event.keyCode == 76)

        if isCmdOrCtrl && !hasShift && isReturnKey {
            guard window?.firstResponder === self else {
                return super.performKeyEquivalent(with: event)
            }
            // CRITICAL: If IME composition (marked text) is active, route to inputContext and NEVER send!
            if hasMarkedText() {
                _ = inputContext?.handleEvent(event)
                return true
            }
            if isEditable {
                onCommandEnterSend?()
            }
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    // MARK: - Layer 3 IME Guard: Snapshot hasMarkedText() BEFORE inputContext?.handleEvent(event)

    override public func keyDown(with event: NSEvent) {
        let preSnapshot = hasMarkedText()
        wasComposingBeforeKeyDown = preSnapshot
        defer { wasComposingBeforeKeyDown = false }

        if !preSnapshot {
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            let hasCmdOrCtrl = flags.contains(.command) || flags.contains(.control)
            let hasShift = flags.contains(.shift)

            if !hasCmdOrCtrl {
                switch event.keyCode {
                case 126: // Up arrow
                    if onInlineCompletionKey?(.arrowUp) == true { return }
                case 125: // Down arrow
                    if onInlineCompletionKey?(.arrowDown) == true { return }
                case 36, 76, 48: // Return, Enter, Tab
                    if !hasShift, onInlineCompletionKey?(.confirmSelection) == true { return }
                case 53: // Escape
                    if onInlineCompletionKey?(.escapeDismiss) == true { return }
                default:
                    break
                }
            } else if !hasShift && (event.keyCode == 36 || event.keyCode == 76) {
                onCommandEnterSend?()
                return
            }
        }

        super.keyDown(with: event)
    }

    override public func doCommand(by selector: Selector) {
        if wasComposingBeforeKeyDown || hasMarkedText() {
            super.doCommand(by: selector)
            return
        }
        super.doCommand(by: selector)
    }

    // MARK: - Pasteboard Raw Snapshot Ingress (Preserves Plain-Text Undo on "text" Disposition)

    override public func paste(_ sender: Any?) {
        guard let intercept = onInterceptPaste else {
            super.paste(sender)
            return
        }
        intercept { [weak self] shouldFallbackToPlainTextPaste in
            guard let self else { return }
            if shouldFallbackToPlainTextPaste {
                self.invokeSuperPaste(sender)
            }
        }
    }

    private func invokeSuperPaste(_ sender: Any?) {
        super.paste(sender)
    }

    // MARK: - Native File Drag & Drop Forwarding to Rust

    override public func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        if sender.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) {
            return .copy
        }
        return super.draggingEntered(sender)
    }

    override public func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        if let urls = sender.draggingPasteboard.readObjects(
            forClasses: [NSURL.self],
            options: [.urlReadingFileURLsOnly: true]
        ) as? [URL], !urls.isEmpty {
            let paths = urls.map(\.path)
            let topLeftPt: NSPoint
            if let win = window {
                let winPt = sender.draggingLocation
                let winH = win.contentView?.bounds.height ?? win.frame.height
                topLeftPt = NSPoint(x: max(0, winPt.x), y: max(0, winH - winPt.y))
            } else {
                let loc = convert(sender.draggingLocation, from: nil)
                topLeftPt = NSPoint(x: max(0, loc.x), y: max(0, loc.y))
            }
            onFilesDropped?(paths, topLeftPt)
            return true
        }
        return super.performDragOperation(sender)
    }

    public static func topLeftWindowBounds(for view: NSView) -> DesktopAttachmentBounds? {
        guard let win = view.window else { return nil }
        let contentRect = win.contentView?.bounds ?? NSRect(origin: .zero, size: win.frame.size)
        let rectInWin = view.convert(view.bounds, to: nil)
        let winW = max(1.0, Double(contentRect.width))
        let winH = max(1.0, Double(contentRect.height))
        let rawLeft = Double(rectInWin.minX)
        let rawRight = Double(rectInWin.maxX)
        let rawTop = winH - Double(rectInWin.maxY)
        let rawBottom = winH - Double(rectInWin.minY)
        let x = max(0.0, rawLeft)
        let y = max(0.0, rawTop)
        let width = min(winW, rawRight) - x
        let height = min(winH, rawBottom) - y
        guard width > 0, height > 0, width <= 65536, height <= 65536 else { return nil }
        return DesktopAttachmentBounds(x: x, y: y, width: width, height: height)
    }
}

public final class ComposerScrollView: NSScrollView {
    public var onWindowBoundsChanged: ((DesktopAttachmentBounds) -> Void)?
    private var lastReportedBounds: DesktopAttachmentBounds?
    private var pendingReport = false

    override public func layout() {
        super.layout()
        scheduleBoundsReport()
    }

    override public func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        NotificationCenter.default.removeObserver(self, name: NSWindow.didResizeNotification, object: nil)
        if let window {
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(windowDidResize(_:)),
                name: NSWindow.didResizeNotification,
                object: window
            )
        }
        scheduleBoundsReport()
    }

    @objc private func windowDidResize(_ notification: Notification) {
        scheduleBoundsReport()
    }

    private func scheduleBoundsReport() {
        guard !pendingReport else { return }
        pendingReport = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.pendingReport = false
            guard let next = ComposerNSTextView.topLeftWindowBounds(for: self),
                  next != self.lastReportedBounds
            else { return }
            self.lastReportedBounds = next
            self.onWindowBoundsChanged?(next)
        }
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }
}

// MARK: - SwiftUI Wrapper + Manual Drag-Handle Vertical Resize (54pt–160pt Only, Never Auto-Grows)

public struct ComposerTextEditorRepresentable: NSViewRepresentable {
    public let text: String
    public let caretOffset: Int?
    public let placeholder: String
    public let isEnabled: Bool
    public let palette: DesignPalette
    public let onTextAndSelectionChanged: (String, Int, Int) -> Void
    public let onCommandEnterSend: () -> Void
    public let onInlineCompletionKey: (ComposerNSTextView.InlineKeyAction) -> Bool
    public let onInterceptPaste: (@escaping (Bool) -> Void) -> Void
    public let onFilesDropped: ([String], NSPoint) -> Void
    public var onWindowBoundsChanged: ((DesktopAttachmentBounds) -> Void)? = nil

    public func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    public func makeNSView(context: Context) -> ComposerScrollView {
        let scroll = ComposerScrollView()
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.onWindowBoundsChanged = onWindowBoundsChanged

        let tv = ComposerNSTextView()
        tv.delegate = context.coordinator
        tv.isVerticallyResizable = true
        tv.isHorizontallyResizable = false
        tv.autoresizingMask = [.width]
        tv.textContainer?.widthTracksTextView = true
        scroll.documentView = tv
        context.coordinator.textView = tv
        applyProps(to: tv, coordinator: context.coordinator)
        return scroll
    }

    public func updateNSView(_ nsView: ComposerScrollView, context: Context) {
        context.coordinator.parent = self
        nsView.onWindowBoundsChanged = onWindowBoundsChanged
        if let tv = context.coordinator.textView {
            applyProps(to: tv, coordinator: context.coordinator)
        }
    }

    private func applyProps(to tv: ComposerNSTextView, coordinator: Coordinator) {
        tv.isEditable = isEnabled
        tv.textColor = palette.textPrimary.nsColor
        tv.insertionPointColor = palette.accent.nsColor
        tv.placeholderText = placeholder
        tv.onTextAndSelectionChanged = onTextAndSelectionChanged
        tv.onCommandEnterSend = onCommandEnterSend
        tv.onInlineCompletionKey = onInlineCompletionKey
        tv.onInterceptPaste = onInterceptPaste
        tv.onFilesDropped = onFilesDropped

        if !tv.hasMarkedText() && tv.string != text {
            coordinator.suppressCallbacks = true
            tv.string = text
            if let caret = caretOffset {
                let clamped = max(0, min(caret, (text as NSString).length))
                tv.setSelectedRange(NSRange(location: clamped, length: 0))
            }
            coordinator.suppressCallbacks = false
        }
    }

    @MainActor
    public final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ComposerTextEditorRepresentable
        weak var textView: ComposerNSTextView?
        var suppressCallbacks = false

        init(parent: ComposerTextEditorRepresentable) {
            self.parent = parent
        }

        public func textDidChange(_ notification: Notification) {
            guard !suppressCallbacks, let tv = textView, !tv.hasMarkedText() else { return }
            var current = tv.string
            let ns = current as NSString
            if ns.length > CodexDesktopStore.maxMessageUTF16Units {
                current = ns.substring(to: CodexDesktopStore.maxMessageUTF16Units)
                suppressCallbacks = true
                tv.string = current
                suppressCallbacks = false
            }
            let sel = tv.selectedRange()
            parent.onTextAndSelectionChanged(current, sel.location, sel.location + sel.length)
        }

        public func textViewDidChangeSelection(_ notification: Notification) {
            guard !suppressCallbacks, let tv = textView, !tv.hasMarkedText() else { return }
            let sel = tv.selectedRange()
            parent.onTextAndSelectionChanged(tv.string, sel.location, sel.location + sel.length)
        }
    }
}

public struct ManualResizableComposerBox: View {
    @Environment(\.agentHubPalette) private var palette
    public let text: String
    public let caretOffset: Int?
    public let placeholder: String
    public let isEnabled: Bool
    public let onTextAndSelectionChanged: (String, Int, Int) -> Void
    public let onCommandEnterSend: () -> Void
    public let onInlineCompletionKey: (ComposerNSTextView.InlineKeyAction) -> Bool
    public let onInterceptPaste: (@escaping (Bool) -> Void) -> Void
    public let onFilesDropped: ([String], NSPoint) -> Void
    public var onWindowBoundsChanged: ((DesktopAttachmentBounds) -> Void)? = nil

    @State private var manualHeight: CGFloat = ComposerNSTextView.minManualHeight // 54pt initial
    @State private var dragStartHeight: CGFloat?

    public var body: some View {
        VStack(spacing: 0) {
            ComposerTextEditorRepresentable(
                text: text,
                caretOffset: caretOffset,
                placeholder: placeholder,
                isEnabled: isEnabled,
                palette: palette,
                onTextAndSelectionChanged: onTextAndSelectionChanged,
                onCommandEnterSend: onCommandEnterSend,
                onInlineCompletionKey: onInlineCompletionKey,
                onInterceptPaste: onInterceptPaste,
                onFilesDropped: onFilesDropped,
                onWindowBoundsChanged: onWindowBoundsChanged
            )
            .frame(height: manualHeight)

            // Explicit bottom resize grip (manual vertical resize ONLY between 54pt and 160pt)
            HStack {
                Spacer()
                Capsule()
                    .fill(palette.borderStrong.color)
                    .frame(width: 32, height: 4)
                    .padding(.vertical, 3)
                Spacer()
            }
            .frame(height: 10)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { value in
                        let base = dragStartHeight ?? manualHeight
                        if dragStartHeight == nil { dragStartHeight = manualHeight }
                        manualHeight = ComposerNSTextView.clampManualHeight(base + value.translation.height)
                    }
                    .onEnded { _ in
                        dragStartHeight = nil
                    }
            )
            .accessibilityLabel("调整输入框高度")
        }
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(palette.borderStrong.color, lineWidth: 1))
    }
}
