import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

@MainActor
final class HoverSummaryRowView: NSView {
    var isEnabled: Bool {
        didSet { window?.invalidateCursorRects(for: self) }
    }
    var onTap: (() -> Void)?
    var focusRingColor: NSColor
    var accessibilityExpandedState: Bool?
    private let open: Bool
    private let hoverFill: NSColor
    private let openFill: NSColor
    private var tracking: NSTrackingArea?

    init(
        open: Bool,
        isEnabled: Bool,
        hoverFill: NSColor,
        openFill: NSColor,
        focusRingColor: NSColor = .keyboardFocusIndicatorColor,
        accessibilityExpanded: Bool? = nil,
        accessibilityLabelText: String? = nil
    ) {
        self.open = open
        self.isEnabled = isEnabled
        self.hoverFill = hoverFill
        self.openFill = openFill
        self.focusRingColor = focusRingColor
        self.accessibilityExpandedState = accessibilityExpanded ?? open
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = (open ? openFill : NSColor.clear).cgColor
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        if let accessibilityLabelText {
            setAccessibilityLabel(accessibilityLabelText)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var acceptsFirstResponder: Bool { isEnabled }
    override var canBecomeKeyView: Bool { isEnabled }

    override func isAccessibilityExpanded() -> Bool {
        accessibilityExpandedState ?? open
    }

    override func accessibilityPerformPress() -> Bool {
        guard isEnabled else { return false }
        onTap?()
        return true
    }

    override func resetCursorRects() {
        super.resetCursorRects()
        if isEnabled {
            addCursorRect(bounds, cursor: .pointingHand)
        }
    }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { syncFocusRing(focused: true) }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { syncFocusRing(focused: false) }
        return ok
    }

    private func syncFocusRing(focused: Bool) {
        layer?.borderWidth = focused ? 1.5 : 0
        layer?.borderColor = focused ? focusRingColor.cgColor : nil
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

final class ActionButton: NSButton {
    var onTap: (() -> Void)?

    override init(frame frameRect: NSRect) {
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
final class IconFeedbackActionButton: NSButton {
    private(set) var isShowingCopied: Bool = false
    var onFeedbackStateChanged: (() -> Void)?
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

    init(
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

    override var acceptsFirstResponder: Bool { isEnabled }
    override var canBecomeKeyView: Bool { isEnabled }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFeedbackStateChanged?() }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok {
            DispatchQueue.main.async { [weak self] in
                self?.onFeedbackStateChanged?()
            }
        }
        return ok
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
        layer?.backgroundColor = hoverBg.cgColor
    }

    override func mouseExited(with event: NSEvent) {
        layer?.backgroundColor = baseBg.cgColor
    }

    @objc func handleClick() {
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
    var isEnabled: Bool = true {
        didSet { window?.invalidateCursorRects(for: self) }
    }
    var focusRingColor: NSColor = .keyboardFocusIndicatorColor
    var accessibilityExpandedState: Bool?
    var onTap: (() -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.cornerRadius = 6
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var acceptsFirstResponder: Bool { isEnabled }
    override var canBecomeKeyView: Bool { isEnabled }

    override func isAccessibilityExpanded() -> Bool {
        accessibilityExpandedState ?? false
    }

    override func accessibilityPerformPress() -> Bool {
        guard isEnabled else { return false }
        onTap?()
        return true
    }

    override func resetCursorRects() {
        super.resetCursorRects()
        if isEnabled {
            addCursorRect(bounds, cursor: .pointingHand)
        }
    }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok {
            layer?.borderWidth = 1.5
            layer?.borderColor = focusRingColor.cgColor
        }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok {
            layer?.borderWidth = 0
            layer?.borderColor = nil
        }
        return ok
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

@MainActor
final class AppKitRunningRingView: NSView {
    private let size: CGFloat
    private let trackLayer = CAShapeLayer()
    private let arcLayer = CAShapeLayer()
    nonisolated(unsafe) private var accessibilityToken: NSObjectProtocol?
    nonisolated(unsafe) private var occlusionToken: NSObjectProtocol?

    init(size: CGFloat = 13, trackColor: NSColor, accentColor: NSColor) {
        self.size = size
        super.init(frame: NSRect(x: 0, y: 0, width: size, height: size))
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: size),
            heightAnchor.constraint(equalToConstant: size),
        ])
        setAccessibilityElement(true)
        setAccessibilityRole(.progressIndicator)
        setAccessibilityLabel("运行中")

        let boundsRect = CGRect(x: 0, y: 0, width: size, height: size)
        let inset = boundsRect.insetBy(dx: 1.5, dy: 1.5)
        let ringPath = CGPath(ellipseIn: inset, transform: nil)
        trackLayer.frame = boundsRect
        trackLayer.path = ringPath
        trackLayer.fillColor = nil
        trackLayer.strokeColor = trackColor.cgColor
        trackLayer.lineWidth = 2.0

        let arcPath = CGMutablePath()
        arcPath.addArc(
            center: CGPoint(x: boundsRect.midX, y: boundsRect.midY),
            radius: inset.width * 0.5,
            startAngle: 20 * .pi / 180,
            endAngle: 120 * .pi / 180,
            clockwise: false
        )
        arcLayer.bounds = boundsRect
        arcLayer.anchorPoint = CGPoint(x: 0.5, y: 0.5)
        arcLayer.position = CGPoint(x: boundsRect.midX, y: boundsRect.midY)
        arcLayer.path = arcPath
        arcLayer.fillColor = nil
        arcLayer.strokeColor = accentColor.cgColor
        arcLayer.lineWidth = 2.0
        arcLayer.lineCap = .round

        layer?.addSublayer(trackLayer)
        layer?.addSublayer(arcLayer)

        accessibilityToken = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.syncAnimationState()
            }
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    deinit {
        if let accessibilityToken {
            NSWorkspace.shared.notificationCenter.removeObserver(accessibilityToken)
        }
        if let occlusionToken {
            NotificationCenter.default.removeObserver(occlusionToken)
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let occlusionToken {
            NotificationCenter.default.removeObserver(occlusionToken)
            self.occlusionToken = nil
        }
        if let win = window {
            occlusionToken = NotificationCenter.default.addObserver(
                forName: NSWindow.didChangeOcclusionStateNotification,
                object: win,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in
                    self?.syncAnimationState()
                }
            }
        }
        syncAnimationState()
    }

    override func viewDidHide() {
        super.viewDidHide()
        syncAnimationState()
    }

    override func viewDidUnhide() {
        super.viewDidUnhide()
        syncAnimationState()
    }

    func syncAnimationState(reduceMotionOverride: Bool? = nil) {
        let reduceMotion = reduceMotionOverride ?? AccessibilityMotionObserver.shared.reduceMotion
        let canSpin: Bool = if let win = window {
            !isHiddenOrHasHiddenAncestor
                && !reduceMotion
                && win.isVisible
                && win.occlusionState.contains(.visible)
        } else {
            false
        }
        if canSpin {
            guard arcLayer.animation(forKey: "spin") == nil else { return }
            let anim = CABasicAnimation(keyPath: "transform.rotation.z")
            anim.fromValue = 0.0
            anim.toValue = -Double.pi * 2.0
            anim.duration = 0.9
            anim.repeatCount = .infinity
            anim.isRemovedOnCompletion = false
            arcLayer.add(anim, forKey: "spin")
        } else {
            arcLayer.removeAnimation(forKey: "spin")
        }
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
final class ReaderOuterScrollView: NSScrollView {
    weak var coordinator: ConversationTableCoordinator?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        verticalScrollElasticity = .allowed
        horizontalScrollElasticity = .none
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func scrollWheel(with event: NSEvent) {
        coordinator?.handleOuterScrollWheel(
            deltaY: event.scrollingDeltaY,
            phase: event.phase,
            momentumPhase: event.momentumPhase
        )
        super.scrollWheel(with: event)
    }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 126 || event.keyCode == 116 || event.keyCode == 115 {
            coordinator?.handleOuterUpwardKeyNavigation()
        }
        super.keyDown(with: event)
    }
}

// MARK: - SwiftUI Representable Wrapping NSScrollView + Headerless View-Based NSTableView
