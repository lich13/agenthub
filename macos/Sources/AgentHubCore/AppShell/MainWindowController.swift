import AppKit
import SwiftUI

@MainActor
public final class AgentHubWindowManager: NSObject, NSWindowDelegate {
    public let window: NSWindow
    public let store: RuntimeConnectionStore
    private var alignedInitialFrame = false

    public init<Content: View>(store: RuntimeConnectionStore, rootView: Content) {
        self.store = store
        let initialRect = NSRect(
            x: 0,
            y: 0,
            width: LayoutMetrics.windowDefaultWidth,
            height: LayoutMetrics.windowDefaultHeight
        )
        let win = NSWindow(
            contentRect: initialRect,
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        win.title = "AgentHub"
        win.minSize = NSSize(width: LayoutMetrics.windowMinWidth, height: LayoutMetrics.windowMinHeight)
        win.isReleasedWhenClosed = false
        win.contentView = NSHostingView(rootView: rootView)
        // Created hidden per architecture rule: align once before initial reveal.
        self.window = win
        super.init()
        win.delegate = self
    }

    public static func maximizedInitialFrame(visibleFrame: NSRect) -> NSRect {
        guard visibleFrame.width > 0, visibleFrame.height > 0 else {
            return NSRect(
                x: 0,
                y: 0,
                width: LayoutMetrics.windowDefaultWidth,
                height: LayoutMetrics.windowDefaultHeight
            )
        }
        return visibleFrame
    }

    public static func clampedFrameToVisibleScreen(_ frame: NSRect, visibleFrame: NSRect) -> NSRect {
        guard visibleFrame.width > 0, visibleFrame.height > 0 else { return frame }
        let w = min(max(frame.width, min(LayoutMetrics.windowMinWidth, visibleFrame.width)), visibleFrame.width)
        let h = min(max(frame.height, min(LayoutMetrics.windowMinHeight, visibleFrame.height)), visibleFrame.height)
        let x = min(max(frame.origin.x, visibleFrame.minX), visibleFrame.maxX - w)
        let y = min(max(frame.origin.y, visibleFrame.minY), visibleFrame.maxY - h)
        return NSRect(x: x, y: y, width: w, height: h)
    }

    public static func clampedInitialFrame(
        desired: NSSize = NSSize(width: LayoutMetrics.windowDefaultWidth, height: LayoutMetrics.windowDefaultHeight),
        visibleFrame: NSRect
    ) -> NSRect {
        guard visibleFrame.width > 0, visibleFrame.height > 0 else {
            return NSRect(origin: .zero, size: desired)
        }
        let w = min(desired.width, visibleFrame.width)
        let h = min(desired.height, visibleFrame.height)
        let x = visibleFrame.minX + floor((visibleFrame.width - w) * 0.5)
        let y = visibleFrame.minY + floor((visibleFrame.height - h) * 0.5)
        return NSRect(x: x, y: y, width: w, height: h)
    }

    public func alignOnceAndReveal(overrideVisibleFrame: NSRect? = nil) {
        if !alignedInitialFrame {
            alignedInitialFrame = true
            if let vf = overrideVisibleFrame ?? (window.screen ?? NSScreen.main)?.visibleFrame,
               vf.width > 0, vf.height > 0
            {
                let frame = Self.maximizedInitialFrame(visibleFrame: vf)
                window.setFrame(frame, display: false)
            } else {
                window.zoom(nil)
            }
        }
        revealWithoutResize()
    }

    public func windowDidChangeScreen(_ notification: Notification) {
        guard alignedInitialFrame, let vf = window.screen?.visibleFrame else { return }
        let clamped = Self.clampedFrameToVisibleScreen(window.frame, visibleFrame: vf)
        if clamped != window.frame {
            window.setFrame(clamped, display: true)
        }
    }

    public func revealWithoutResize() {
        if window.isMiniaturized {
            window.deminiaturize(nil)
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        syncWindowState()
    }

    public func syncWindowState() {
        let visible = window.isVisible && !window.isMiniaturized && window.occlusionState.contains(.visible)
        store.updateWindowState(visible: visible)
    }

    public func windowDidBecomeKey(_ notification: Notification) { syncWindowState() }
    public func windowDidResignKey(_ notification: Notification) { syncWindowState() }
    public func windowDidMiniaturize(_ notification: Notification) { syncWindowState() }
    public func windowDidDeminiaturize(_ notification: Notification) { syncWindowState() }
    public func windowDidChangeOcclusionState(_ notification: Notification) { syncWindowState() }

    public func windowShouldClose(_ sender: NSWindow) -> Bool {
        sender.orderOut(nil)
        store.updateWindowState(visible: false)
        return false
    }
}

@MainActor
public final class AgentHubAppDelegate: NSObject, NSApplicationDelegate {
    private var bridge: FFIRuntimeBridge?
    private var store: RuntimeConnectionStore?
    private var windowManager: AgentHubWindowManager?
    public var providerContentBuilder: (@MainActor (WorkspaceId, RuntimeConnectionStore) -> AnyView)?

    public override init() {
        super.init()
    }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        installMainMenu()
        CiNotificationCenterBridge.shared.installIfAvailable()

        do {
            let ffi = try FFIRuntimeBridge()
            let connStore = RuntimeConnectionStore(transport: ffi)
            let rootView = AppShellView(
                store: connStore,
                providerContentBuilder: providerContentBuilder
            )
            let manager = AgentHubWindowManager(store: connStore, rootView: rootView)
            self.bridge = ffi
            self.store = connStore
            self.windowManager = manager

            Task { @MainActor in
                await connStore.bootstrap()
            }
            manager.alignOnceAndReveal()
        } catch {
            let alert = NSAlert()
            alert.alertStyle = .critical
            alert.messageText = "AgentHub 启动失败"
            alert.informativeText = error.localizedDescription
            alert.runModal()
            NSApp.terminate(nil)
        }
    }

    public func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        windowManager?.revealWithoutResize()
        return true
    }

    public func applicationDidBecomeActive(_ notification: Notification) {
        windowManager?.syncWindowState()
    }

    public func applicationDidResignActive(_ notification: Notification) {
        windowManager?.syncWindowState()
    }

    public func applicationWillTerminate(_ notification: Notification) {
        bridge?.shutdown()
        bridge = nil
    }

    // MARK: - Native Menu Bar (Edit -> Select All & Find routing without stealing text input)

    private func installMainMenu() {
        let mainMenu = NSMenu()

        // App Menu
        let appMenuItem = NSMenuItem()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "隐藏 AgentHub", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(NSMenuItem.separator())
        appMenu.addItem(withTitle: "退出 AgentHub", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        // Edit Menu
        let editMenuItem = NSMenuItem()
        let editMenu = NSMenu(title: "编辑")
        editMenu.addItem(withTitle: "撤销", action: Selector(("undo:")), keyEquivalent: "z")
        editMenu.addItem(withTitle: "重做", action: Selector(("redo:")), keyEquivalent: "Z")
        editMenu.addItem(NSMenuItem.separator())
        editMenu.addItem(withTitle: "剪切", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        editMenu.addItem(withTitle: "拷贝", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "粘贴", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        let selectAllItem = NSMenuItem(title: "全选", action: #selector(handleMenuSelectAll(_:)), keyEquivalent: "a")
        selectAllItem.target = self
        editMenu.addItem(selectAllItem)

        let findItem = NSMenuItem(title: "查找…", action: #selector(handleMenuFind(_:)), keyEquivalent: "f")
        findItem.target = self
        editMenu.addItem(NSMenuItem.separator())
        editMenu.addItem(findItem)
        editMenuItem.submenu = editMenu
        mainMenu.addItem(editMenuItem)

        // Window Menu
        let winMenuItem = NSMenuItem()
        let winMenu = NSMenu(title: "窗口")
        winMenu.addItem(withTitle: "最小化", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        winMenu.addItem(withTitle: "缩放", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        winMenuItem.submenu = winMenu
        mainMenu.addItem(winMenuItem)

        NSApp.mainMenu = mainMenu
    }

    public enum SelectAllRouteOutcome: Equatable, Sendable {
        case noWindow
        case editableTextSelectedAll
        case selectableTextExpanded
        case blockedByModalOrOverlay
        case postedListSelectAll
    }

    @discardableResult
    public static func routeMenuSelectAll(
        in keyWin: NSWindow?,
        store: RuntimeConnectionStore?,
        sender: Any? = nil
    ) -> SelectAllRouteOutcome {
        guard let keyWin else { return .noWindow }
        if keyWin.attachedSheet != nil || NSApp.modalWindow != nil {
            return .blockedByModalOrOverlay
        }
        let hasVisibleChildWindow = (keyWin.childWindows ?? []).contains { $0.isVisible }
        let hasModal = hasVisibleChildWindow || store?.hasAnyActiveModal == true || ModalPresentationRegistry.shared.hasActiveModal

        if let textView = keyWin.firstResponder as? NSTextView {
            if hasModal {
                // While a modal/overlay is presented, only editable inputs inside modals (e.g. Search)
                // or selectable views explicitly inside an active modal container may handle ⌘A.
                let inChildWin = textView.window !== keyWin
                let inModalContainer = ModalPresentationRegistry.shared.isInsideActiveModalContainer(textView)
                if textView.isEditable {
                    textView.selectAll(sender)
                    return .editableTextSelectedAll
                }
                if textView.isSelectable && textView.selectedRange().length > 0 && (inChildWin || inModalContainer) {
                    textView.selectAll(sender)
                    return .selectableTextExpanded
                }
                return .blockedByModalOrOverlay
            }

            if textView.isEditable {
                textView.selectAll(sender)
                return .editableTextSelectedAll
            }
            if textView.isSelectable && textView.selectedRange().length > 0 {
                textView.selectAll(sender)
                return .selectableTextExpanded
            }
        }

        // Block underlying session-list Select All when any inline modal, confirm dialog, lightbox, <960pt drawer or child popover is open!
        if hasModal {
            return .blockedByModalOrOverlay
        }
        NotificationCenter.default.post(name: .agentHubSelectAllRequested, object: nil)
        return .postedListSelectAll
    }

    @objc private func handleMenuSelectAll(_ sender: Any?) {
        Self.routeMenuSelectAll(in: NSApp.keyWindow ?? windowManager?.window, store: store, sender: sender)
    }

    @objc private func handleMenuFind(_ sender: Any?) {
        guard NSApp.modalWindow == nil,
              (NSApp.keyWindow?.attachedSheet == nil)
        else { return }
        NotificationCenter.default.post(name: .agentHubSearchRequested, object: nil)
    }
}
