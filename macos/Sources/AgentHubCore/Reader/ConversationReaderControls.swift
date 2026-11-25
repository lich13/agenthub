import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

@MainActor
final class HoverSummaryRowView: NSView {
    var isEnabled: Bool
    var onTap: (() -> Void)?
    private let open: Bool
    private let hoverFill: NSColor
    private let openFill: NSColor
    private var tracking: NSTrackingArea?

    init(open: Bool, isEnabled: Bool, hoverFill: NSColor, openFill: NSColor) {
        self.open = open
        self.isEnabled = isEnabled
        self.hoverFill = hoverFill
        self.openFill = openFill
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = (open ? openFill : NSColor.clear).cgColor
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var acceptsFirstResponder: Bool { isEnabled }

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
        guard isEnabled else { return }
        layer?.backgroundColor = hoverFill.cgColor
    }

    override func mouseExited(with event: NSEvent) {
        layer?.backgroundColor = (open ? openFill : NSColor.clear).cgColor
    }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        onTap?()
    }

    override func keyDown(with event: NSEvent) {
        guard isEnabled else {
            super.keyDown(with: event)
            return
        }
        if event.keyCode == 36 || event.keyCode == 76 || event.keyCode == 49 {
            onTap?()
            return
        }
        super.keyDown(with: event)
    }
}

public final class ActionButton: NSButton {
    public var onTap: (() -> Void)?

    override public init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        isBordered = false
        bezelStyle = .regularSquare
        target = self
        action = #selector(handlePress)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    @objc private func handlePress() {
        onTap?()
    }
}

@MainActor
public final class IconFeedbackActionButton: NSButton {
    public private(set) var isShowingCopied: Bool = false
    public var onFeedbackStateChanged: (() -> Void)?
    private let dimension: CGFloat
    private let iconSize: CGFloat
    private let idleIcon: LucideIconName
    private let idleColor: NSColor
    private let okColor: NSColor
    private let baseBg: NSColor
    private let hoverBg: NSColor
    private let onActivate: () -> Void
    private var resetTask: Task<Void, Never>?
    private var tracking: NSTrackingArea?

    public init(
        dimension: CGFloat = 30,
        iconSize: CGFloat = 17,
        idleIcon: LucideIconName = .copy,
        palette: DesignPalette,
        borderedSurface: NSColor? = nil,
        borderColor: NSColor? = nil,
        toolTipText: String,
        onActivate: @escaping () -> Void
    ) {
        self.dimension = dimension
        self.iconSize = iconSize
        self.idleIcon = idleIcon
        self.idleColor = palette.textMuted.nsColor
        self.okColor = palette.okFg.nsColor
        self.baseBg = borderedSurface ?? .clear
        self.hoverBg = palette.hover.nsColor
        self.onActivate = onActivate
        super.init(frame: NSRect(x: 0, y: 0, width: dimension, height: dimension))
        isBordered = false
        bezelStyle = .regularSquare
        imagePosition = .imageOnly
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = baseBg.cgColor
        if let borderColor {
            layer?.borderWidth = 1
            layer?.borderColor = borderColor.cgColor
        }
        toolTip = toolTipText
        setAccessibilityLabel(toolTipText)
        target = self
        action = #selector(handleClick)
        translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: dimension),
            heightAnchor.constraint(equalToConstant: dimension),
        ])
        refreshIcon()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override public var acceptsFirstResponder: Bool { isEnabled }
    override public var canBecomeKeyView: Bool { isEnabled }

    override public func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFeedbackStateChanged?() }
        return ok
    }

    override public func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok {
            DispatchQueue.main.async { [weak self] in
                self?.onFeedbackStateChanged?()
            }
        }
        return ok
    }

    override public func updateTrackingAreas() {
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

    override public func mouseEntered(with event: NSEvent) {
        layer?.backgroundColor = hoverBg.cgColor
    }

    override public func mouseExited(with event: NSEvent) {
        layer?.backgroundColor = baseBg.cgColor
    }

    @objc public func handleClick() {
        onActivate()
        resetTask?.cancel()
        isShowingCopied = true
        refreshIcon()
        onFeedbackStateChanged?()
        resetTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            guard !Task.isCancelled, let self else { return }
            self.isShowingCopied = false
            self.refreshIcon()
            self.onFeedbackStateChanged?()
        }
    }

    private func refreshIcon() {
        let icon = isShowingCopied ? LucideIconName.check : idleIcon
        let color = isShowingCopied ? okColor : idleColor
        image = LucideVectorCatalog.nsImage(for: icon, size: iconSize, color: color)
    }
}

@MainActor
final class ActionRowContainerView: NSView {
    var isEnabled: Bool = true
    var onTap: (() -> Void)?

    override var acceptsFirstResponder: Bool { isEnabled }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        onTap?()
    }

    override func keyDown(with event: NSEvent) {
        guard isEnabled else {
            super.keyDown(with: event)
            return
        }
        if event.keyCode == 36 || event.keyCode == 76 || event.keyCode == 49 {
            onTap?()
            return
        }
        super.keyDown(with: event)
    }
}

@MainActor
final class AppKitRunningRingView: NSView {
    private let size: CGFloat
    private let trackColor: NSColor
    private let accentColor: NSColor

    init(size: CGFloat = 13, trackColor: NSColor, accentColor: NSColor) {
        self.size = size
        self.trackColor = trackColor
        self.accentColor = accentColor
        super.init(frame: NSRect(x: 0, y: 0, width: size, height: size))
        translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: size),
            heightAnchor.constraint(equalToConstant: size),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func draw(_ dirtyRect: NSRect) {
        let inset = bounds.insetBy(dx: 1.5, dy: 1.5)
        let ring = NSBezierPath(ovalIn: inset)
        ring.lineWidth = 2.0
        trackColor.setStroke()
        ring.stroke()

        let arc = NSBezierPath()
        let center = NSPoint(x: bounds.midX, y: bounds.midY)
        arc.appendArc(withCenter: center, radius: inset.width * 0.5, startAngle: 20, endAngle: 120)
        arc.lineWidth = 2.0
        arc.lineCapStyle = .round
        accentColor.setStroke()
        arc.stroke()
    }
}

@MainActor
final class PassthroughInnerScrollView: NSScrollView {
    let allowsVerticalScroll: Bool

    init(allowsVerticalScroll: Bool) {
        self.allowsVerticalScroll = allowsVerticalScroll
        super.init(frame: .zero)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func scrollWheel(with event: NSEvent) {
        if !allowsVerticalScroll {
            // Horizontal-only code blocks & tables must NEVER trap vertical conversation scrolling!
            if !hasHorizontalScroller || abs(event.scrollingDeltaY) >= abs(event.scrollingDeltaX) {
                nextResponder?.scrollWheel(with: event)
                return
            }
            super.scrollWheel(with: event)
            return
        }

        let docH = documentView?.frame.height ?? 0
        let clipH = contentView.bounds.height
        let maxScroll = max(0, docH - clipH)
        // Forward momentum scrolls or at-boundary vertical scrolls to the outer conversation NSScrollView
        if !hasVerticalScroller || maxScroll <= 0.5 || event.momentumPhase != [] {
            nextResponder?.scrollWheel(with: event)
            return
        }
        let originY = contentView.bounds.origin.y
        if (event.scrollingDeltaY > 0 && originY <= 0.5)
            || (event.scrollingDeltaY < 0 && originY >= maxScroll - 0.5)
        {
            nextResponder?.scrollWheel(with: event)
            return
        }
        super.scrollWheel(with: event)
    }
}

@MainActor
public final class ReaderOuterScrollView: NSScrollView {
    public weak var coordinator: ConversationTableCoordinator?

    override public init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        verticalScrollElasticity = .allowed
        horizontalScrollElasticity = .none
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override public func scrollWheel(with event: NSEvent) {
        coordinator?.handleOuterScrollWheel(
            deltaY: event.scrollingDeltaY,
            phase: event.phase,
            momentumPhase: event.momentumPhase
        )
        super.scrollWheel(with: event)
    }

    override public func keyDown(with event: NSEvent) {
        if event.keyCode == 126 || event.keyCode == 116 || event.keyCode == 115 {
            coordinator?.handleOuterUpwardKeyNavigation()
        }
        super.keyDown(with: event)
    }
}

// MARK: - SwiftUI Representable Wrapping NSScrollView + Headerless View-Based NSTableView
