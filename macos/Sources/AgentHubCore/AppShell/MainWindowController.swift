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

    public static func clampedInitialFrame(
        desired: NSSize = NSSize(width: LayoutMetrics.windowDefaultWidth, height: LayoutMetrics.windowDefaultHeight),
        visibleFrame: NSRect
    ) -> NSRect {
        let w = min(max(desired.width, LayoutMetrics.windowMinWidth), visibleFrame.width)
        let h = min(max(desired.height, LayoutMetrics.windowMinHeight), visibleFrame.height)
        let x = visibleFrame.origin.x + floor((visibleFrame.width - w) / 2.0)
        let y = visibleFrame.origin.y + floor((visibleFrame.height - h) / 2.0)
        return NSRect(x: x, y: y, width: w, height: h)
    }

    public func alignOnceAndReveal() {
        if !alignedInitialFrame {
            alignedInitialFrame = true
            if let screen = window.screen ?? NSScreen.main {
                let frame = Self.clampedInitialFrame(visibleFrame: screen.visibleFrame)
                window.setFrame(frame, display: false)
            } else {
                window.center()
            }
        }
        revealWithoutResize()
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
        let focused = window.isKeyWindow && NSApp.isActive
        store.updateWindowState(visible: visible, focused: focused)
    }

    public func windowDidBecomeKey(_ notification: Notification) { syncWindowState() }
    public func windowDidResignKey(_ notification: Notification) { syncWindowState() }
    public func windowDidMiniaturize(_ notification: Notification) { syncWindowState() }
    public func windowDidDeminiaturize(_ notification: Notification) { syncWindowState() }
    public func windowDidChangeOcclusionState(_ notification: Notification) { syncWindowState() }

    public func windowShouldClose(_ sender: NSWindow) -> Bool {
        sender.orderOut(nil)
        store.updateWindowState(visible: false, focused: false)
        return false
    }
}

@MainActor
public final class AgentHubAppDelegate: NSObject, NSApplicationDelegate {
    private var bridge: FFIRuntimeBridge?
    private var store: RuntimeConnectionStore?
    private var windowManager: AgentHubWindowManager?
    public var providerContentBuilder: (@MainActor (WorkspaceId, RuntimeConnectionStore, Bool) -> AnyView)?

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

    @objc private func handleMenuSelectAll(_ sender: Any?) {
        guard let keyWin = NSApp.keyWindow else { return }
        if keyWin.attachedSheet != nil || NSApp.modalWindow != nil {
            return
        }
        if let textView = keyWin.firstResponder as? NSTextView {
            if textView.isEditable {
                textView.selectAll(sender)
                return
            }
            if textView.isSelectable && textView.selectedRange().length > 0 {
                textView.selectAll(sender)
                return
            }
        }
        NotificationCenter.default.post(name: .agentHubSelectAllRequested, object: nil)
    }

    @objc private func handleMenuFind(_ sender: Any?) {
        guard NSApp.modalWindow == nil else { return }
        NotificationCenter.default.post(name: .agentHubSearchRequested, object: nil)
    }
}
