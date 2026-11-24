import AppKit
import SwiftUI

// MARK: - Provider Menu Action Builder (Exact 6-Platform Matrix from §2.3)

public struct SessionMenuAction: Identifiable {
    public let id: String
    public let label: String
    public let icon: LucideIconName
    public let danger: Bool
    public let disabled: Bool
    public let reason: String?
    public let run: () -> Void

    public init(
        id: String,
        label: String,
        icon: LucideIconName,
        danger: Bool = false,
        disabled: Bool = false,
        reason: String? = nil,
        run: @escaping () -> Void
    ) {
        self.id = id
        self.label = label
        self.icon = icon
        self.danger = danger
        self.disabled = disabled
        self.reason = reason
        self.run = run
    }
}

public enum ProviderMenuMatrixBuilder {
    public static func buildActions(
        provider: SessionProvider,
        isHeaderMenu: Bool,
        isLocalMachine: Bool,
        id: String,
        path: String,
        cwd: String,
        archived: Bool,
        canRename: Bool,
        renameBlockReason: String? = nil,
        canArchive: Bool,
        archiveBlockReason: String? = nil,
        canDelete: Bool,
        deleteBlockReason: String? = nil,
        busy: Bool,
        latestReplyText: String?,
        onRename: (() -> Void)?,
        onToggleArchive: (() -> Void)?,
        onCopy: @escaping (String, String) -> Void,
        onRevealInFinder: @escaping (String) -> Void,
        onDelete: (() -> Void)?
    ) -> [SessionMenuAction] {
        var list: [SessionMenuAction] = []
        let resolvedRenameReason: String? = !canRename
            ? (renameBlockReason ?? (archived ? "已归档线程不可改名" : "当前线程不可改名"))
            : nil
        let revealReason: String? = !isLocalMachine
            ? "仅可在本机 Finder 中显示本机会话"
            : (path.isEmpty ? "当前会话暂无本地文件路径" : nil)

        func appendHeaderCopyReplyIfNeeded() {
            guard isHeaderMenu else { return }
            if let reply = latestReplyText, !reply.isEmpty {
                list.append(SessionMenuAction(
                    id: "copy-reply",
                    label: "复制答复",
                    icon: .copy,
                    run: { onCopy(reply, "已复制答复") }
                ))
            } else {
                list.append(SessionMenuAction(
                    id: "copy-reply",
                    label: "复制答复",
                    icon: .copy,
                    disabled: true,
                    reason: "当前会话暂无可复制的助手答复",
                    run: {}
                ))
            }
        }

        switch provider {
        case .codex:
            if let onRename {
                list.append(SessionMenuAction(
                    id: "rename",
                    label: "改名",
                    icon: .pencil,
                    disabled: !canRename || busy,
                    reason: resolvedRenameReason,
                    run: onRename
                ))
            }
            if let onToggleArchive {
                let archiveLabel = archived ? (isHeaderMenu ? "取消归档" : "恢复") : "归档"
                list.append(SessionMenuAction(
                    id: archived ? "restore" : "archive",
                    label: archiveLabel,
                    icon: archived ? .archiveRestore : .archive,
                    disabled: !canArchive || busy,
                    reason: !canArchive ? archiveBlockReason : nil,
                    run: onToggleArchive
                ))
            }
            appendHeaderCopyReplyIfNeeded()
            list.append(SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { onCopy(id, "已复制") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制") }))
            if let cmd = HistoryAndResumeHelpers.threadResumeCommand(id) {
                list.append(SessionMenuAction(id: "copy-resume", label: "复制恢复命令", icon: .copy, run: { onCopy(cmd, "已复制") }))
            }
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                reason: revealReason,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(
                    id: "delete",
                    label: "永久删除",
                    icon: .trash2,
                    danger: true,
                    disabled: !canDelete || busy,
                    reason: !canDelete ? deleteBlockReason : nil,
                    run: onDelete
                ))
            }

        case .claudeCode:
            if let onRename {
                list.append(SessionMenuAction(
                    id: "rename",
                    label: "改名",
                    icon: .pencil,
                    disabled: !canRename || busy,
                    reason: resolvedRenameReason,
                    run: onRename
                ))
            }
            appendHeaderCopyReplyIfNeeded()
            list.append(SessionMenuAction(id: "copy-id", label: "复制线程 ID", icon: .copy, run: { onCopy(id, "已复制") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制文件路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制") }))
            let cmd = HistoryAndResumeHelpers.claudeResumeCommand(cwd: cwd, id: id)
            list.append(SessionMenuAction(id: "copy-resume", label: "复制恢复命令", icon: .terminal, run: { onCopy(cmd, "已复制") }))
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                reason: revealReason,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(
                    id: "delete",
                    label: "删除任务文件",
                    icon: .trash2,
                    danger: true,
                    disabled: !canDelete || busy,
                    run: onDelete
                ))
            }

        case .grok:
            if let onRename {
                list.append(SessionMenuAction(
                    id: "rename",
                    label: "改名",
                    icon: .pencil,
                    disabled: !canRename || busy,
                    reason: resolvedRenameReason,
                    run: onRename
                ))
            }
            appendHeaderCopyReplyIfNeeded()
            list.append(SessionMenuAction(id: "copy-id", label: "复制线程 ID", icon: .copy, run: { onCopy(id, "已复制线程 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制路径") }))
            let cmd = HistoryAndResumeHelpers.grokResumeCommand(cwd: cwd, id: id)
            list.append(SessionMenuAction(id: "copy-resume", label: "复制恢复命令", icon: .terminal, run: { onCopy(cmd, "已复制恢复命令") }))
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                reason: revealReason,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(
                    id: "delete",
                    label: "删除任务文件",
                    icon: .trash2,
                    danger: true,
                    disabled: !canDelete || busy,
                    run: onDelete
                ))
            }

        case .antigravity, .dsh:
            if let onRename {
                list.append(SessionMenuAction(
                    id: "rename",
                    label: "改名",
                    icon: .pencil,
                    disabled: !canRename || busy,
                    reason: resolvedRenameReason,
                    run: onRename
                ))
            }
            if let onToggleArchive {
                let archiveLabel = archived ? (isHeaderMenu ? "取消归档" : "恢复") : "归档"
                list.append(SessionMenuAction(
                    id: archived ? "restore" : "archive",
                    label: archiveLabel,
                    icon: archived ? .archiveRestore : .archive,
                    disabled: !canArchive || busy,
                    reason: !canArchive ? archiveBlockReason : nil,
                    run: onToggleArchive
                ))
            }
            appendHeaderCopyReplyIfNeeded()
            list.append(SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { onCopy(id, "已复制 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制路径") }))
            // Antigravity & DSH explicitly omit 复制恢复命令
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                reason: revealReason,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(
                    id: "delete",
                    label: "永久删除",
                    icon: .trash2,
                    danger: true,
                    disabled: !canDelete || busy,
                    reason: !canDelete ? deleteBlockReason : nil,
                    run: onDelete
                ))
            }

        case .grokbot:
            appendHeaderCopyReplyIfNeeded()
            list.append(SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { onCopy(id, "已复制 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制路径") }))
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                reason: revealReason,
                run: { onRevealInFinder(path) }
            ))
        }

        return list
    }

    public static func buildBatchContextMenuActions(
        selectedCount: Int,
        operations: [SessionOperation],
        busy: Bool,
        onPrepare: @escaping (SessionOperation) -> Void,
        onClear: @escaping () -> Void
    ) -> [SessionMenuAction] {
        guard selectedCount >= 2 else { return [] }
        var actions: [SessionMenuAction] = operations.map { op in
            let label = (op == .delete)
                ? "删除所选 \(selectedCount) 项…"
                : "\(op.label)所选 \(selectedCount) 项"
            let icon: LucideIconName = switch op {
            case .delete: .trash2
            case .restore: .archiveRestore
            case .archive: .archive
            }
            return SessionMenuAction(
                id: "batch-\(op.rawValue)",
                label: label,
                icon: icon,
                danger: op == .delete,
                disabled: busy,
                run: { onPrepare(op) }
            )
        }
        actions.append(SessionMenuAction(
            id: "batch-clear",
            label: "取消选择",
            icon: .x,
            disabled: busy,
            run: onClear
        ))
        return actions
    }

    public static func shouldTriggerBatchDeleteKey(
        keyCode: UInt16,
        charactersIgnoringModifiers: String?,
        modifierFlags: NSEvent.ModifierFlags
    ) -> Bool {
        let cmdPressed = modifierFlags.contains(.command)
        // Forward Delete is keyCode 117 or Unicode \u{F728}; Backspace is keyCode 51 (\u{7F})
        if keyCode == 117 || charactersIgnoringModifiers == "\u{F728}" {
            return true
        }
        if (keyCode == 51 || charactersIgnoringModifiers == "\u{7F}") && cmdPressed {
            return true
        }
        return false
    }

    public static func shouldTriggerRenameKey(
        keyCode: UInt16,
        charactersIgnoringModifiers: String?,
        modifierFlags: NSEvent.ModifierFlags
    ) -> Bool {
        let disallowed: NSEvent.ModifierFlags = [.command, .control, .option]
        guard modifierFlags.intersection(disallowed).isEmpty else { return false }
        return keyCode == 120 || charactersIgnoringModifiers == "\u{F705}"
    }
}

public struct WorkspaceKeyboardShortcutMonitorView: NSViewRepresentable {
    public var isModalOpen: Bool
    public var onRenameSelected: () -> Bool
    public var onEscapeClearSelection: () -> Bool
    public var onBatchDelete: () -> Bool

    public init(
        isModalOpen: Bool,
        onRenameSelected: @escaping () -> Bool,
        onEscapeClearSelection: @escaping () -> Bool,
        onBatchDelete: @escaping () -> Bool
    ) {
        self.isModalOpen = isModalOpen
        self.onRenameSelected = onRenameSelected
        self.onEscapeClearSelection = onEscapeClearSelection
        self.onBatchDelete = onBatchDelete
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    public func makeNSView(context: Context) -> NSView {
        let view = NSView(frame: .zero)
        context.coordinator.attach(to: view)
        return view
    }

    public func updateNSView(_ nsView: NSView, context: Context) {
        context.coordinator.parent = self
        context.coordinator.attach(to: nsView)
    }

    public static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
        coordinator.detach()
    }

    @MainActor
    public final class Coordinator {
        var parent: WorkspaceKeyboardShortcutMonitorView
        private weak var hostView: NSView?
        private var monitor: Any?

        init(parent: WorkspaceKeyboardShortcutMonitorView) {
            self.parent = parent
        }

        func attach(to view: NSView) {
            hostView = view
            guard monitor == nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard let self else { return event }
                return self.handleKeyDown(event) ? nil : event
            }
        }

        func detach() {
            if let monitor {
                NSEvent.removeMonitor(monitor)
                self.monitor = nil
            }
        }

        private func handleKeyDown(_ event: NSEvent) -> Bool {
            guard !parent.isModalOpen,
                  !ModalPresentationRegistry.shared.hasActiveModal,
                  let win = hostView?.window,
                  event.window === win,
                  win.attachedSheet == nil,
                  NSApp.modalWindow == nil
            else { return false }

            if let tv = win.firstResponder as? NSTextView, tv.isEditable {
                return false
            }

            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            if ProviderMenuMatrixBuilder.shouldTriggerRenameKey(
                keyCode: event.keyCode,
                charactersIgnoringModifiers: event.charactersIgnoringModifiers,
                modifierFlags: flags
            ) {
                return parent.onRenameSelected()
            }
            if event.keyCode == 53 && flags.isEmpty {
                return parent.onEscapeClearSelection()
            }
            if ProviderMenuMatrixBuilder.shouldTriggerBatchDeleteKey(
                keyCode: event.keyCode,
                charactersIgnoringModifiers: event.charactersIgnoringModifiers,
                modifierFlags: flags
            ) {
                return parent.onBatchDelete()
            }
            return false
        }
    }
}

// MARK: - Session Storage Badge & Renameable Session Row

public struct SessionStorageBadgeView: View {
    @Environment(\.agentHubPalette) private var palette
    public let size: SessionStorageSize?

    public init(size: SessionStorageSize?) {
        self.size = size
    }

    public static func resolvedColor(for size: SessionStorageSize?, palette: DesignPalette) -> RGBAColor {
        let formatted = SessionStorageFormatter.format(size)
        switch formatted.tone {
        case "danger": return palette.errFg
        case "warning": return palette.warnFg
        case "success": return palette.okFg
        default: return palette.textMuted
        }
    }

    public var body: some View {
        let formatted = SessionStorageFormatter.format(size)
        let fg = Self.resolvedColor(for: size, palette: palette)
        Text(formatted.label)
            .font(DesignFonts.tabularBody(11, weight: .medium))
            .foregroundStyle(fg.color)
            .multilineTextAlignment(.trailing)
            .lineLimit(1)
            .frame(minWidth: 42, alignment: .trailing)
            .fixedSize(horizontal: true, vertical: false)
            .help(formatted.title)
    }
}

public struct DeletePathCodeBlockView: View {
    @Environment(\.agentHubPalette) private var palette
    public let path: String

    public init(_ path: String) {
        self.path = path
    }

    public var body: some View {
        Text(path)
            .font(DesignFonts.mono(12))
            .foregroundStyle(palette.textPrimary.color)
            .textSelection(.enabled)
            .padding(.vertical, 10)
            .padding(.horizontal, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(palette.codeBg.color)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(palette.borderSubtle.color, lineWidth: 1)
            )
    }
}

// MARK: - Custom WebUI-Parity TaskMenu Popover (.task-menu-items / .context-task-menu)

@MainActor
public final class AgentHubTaskMenuPresenter {
    public static let shared = AgentHubTaskMenuPresenter()

    private var activePanel: NSPanel?
    private weak var parentWindow: NSWindow?
    private weak var activeAnchorView: NSView?
    private weak var suppressedToggleAnchorView: NSView?
    private var suppressedToggleUntil: TimeInterval = 0
    private weak var returnFocusResponder: NSResponder?
    private var itemViews: [TaskMenuItemRowView] = []
    private var focusedIndex: Int = -1
    private var mouseDownMonitor: Any?
    private var scrollMonitor: Any?
    private var keyMonitor: Any?
    private var windowObservers: [NSObjectProtocol] = []

    private init() {}

    public var isPresented: Bool { activePanel != nil }

    public func dismiss(restoreFocus: Bool = false) {
        if let m = mouseDownMonitor { NSEvent.removeMonitor(m); mouseDownMonitor = nil }
        if let s = scrollMonitor { NSEvent.removeMonitor(s); scrollMonitor = nil }
        if let k = keyMonitor { NSEvent.removeMonitor(k); keyMonitor = nil }
        for obs in windowObservers { NotificationCenter.default.removeObserver(obs) }
        windowObservers.removeAll()

        if let panel = activePanel {
            parentWindow?.removeChildWindow(panel)
            panel.orderOut(nil)
        }
        activePanel = nil
        itemViews.removeAll()
        focusedIndex = -1

        if restoreFocus, let win = parentWindow, let resp = returnFocusResponder {
            win.makeFirstResponder(resp)
        }
        activeAnchorView = nil
        parentWindow = nil
        returnFocusResponder = nil
    }

    public func presentAtScreenTopLeft(
        _ topLeftScreenPoint: NSPoint,
        in hostWindow: NSWindow?,
        returnFocus: NSResponder? = nil,
        actions: [SessionMenuAction],
        palette: DesignPalette
    ) {
        guard !actions.isEmpty else { return }
        dismiss(restoreFocus: false)

        let targetWin = hostWindow ?? NSApp.keyWindow ?? NSApp.mainWindow
        self.parentWindow = targetWin
        self.returnFocusResponder = returnFocus ?? targetWin?.firstResponder

        let (cardView, rows, size) = buildMenuCard(actions: actions, palette: palette)
        self.itemViews = rows

        let screenFrame = (targetWin?.screen ?? NSScreen.main)?.visibleFrame
            ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let clampBounds = targetWin?.frame.intersection(screenFrame) ?? screenFrame
        let effectiveBounds = (clampBounds.width >= size.width + 16 && clampBounds.height >= size.height + 16)
            ? clampBounds
            : screenFrame

        let minX = effectiveBounds.minX + 8
        let maxX = max(minX, effectiveBounds.maxX - size.width - 8)
        let clampedX = min(max(topLeftScreenPoint.x, minX), maxX)

        let desiredOriginY = topLeftScreenPoint.y - size.height
        let minY = effectiveBounds.minY + 8
        let maxY = max(minY, effectiveBounds.maxY - size.height - 8)
        let clampedY = min(max(desiredOriginY, minY), maxY)

        let panel = NSPanel(
            contentRect: NSRect(x: clampedX, y: clampedY, width: size.width, height: size.height),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .popUpMenu
        panel.hidesOnDeactivate = true
        panel.contentView = cardView

        if let firstEnabled = rows.firstIndex(where: { !$0.menuAction.disabled }) {
            setFocusedIndex(firstEnabled)
        }

        if let targetWin {
            targetWin.addChildWindow(panel, ordered: .above)
            let resignObs = NotificationCenter.default.addObserver(
                forName: NSWindow.didResignKeyNotification,
                object: targetWin,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in self?.dismiss(restoreFocus: false) }
            }
            let resizeObs = NotificationCenter.default.addObserver(
                forName: NSWindow.didResizeNotification,
                object: targetWin,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in self?.dismiss(restoreFocus: false) }
            }
            windowObservers = [resignObs, resizeObs]
        }
        panel.orderFront(nil)
        activePanel = panel
        installEventMonitors()
    }

    public func presentBelowAnchorView(
        _ anchorView: NSView,
        rightAligned: Bool,
        actions: [SessionMenuAction],
        palette: DesignPalette
    ) {
        if suppressedToggleAnchorView === anchorView,
           ProcessInfo.processInfo.systemUptime < suppressedToggleUntil
        {
            suppressedToggleAnchorView = nil
            return
        }
        if activePanel != nil, activeAnchorView === anchorView {
            dismiss(restoreFocus: true)
            return
        }
        guard let win = anchorView.window, !actions.isEmpty else { return }
        let rectInWin = anchorView.convert(anchorView.bounds, to: nil)
        let rectOnScreen = win.convertToScreen(rectInWin)
        let (_, _, size) = buildMenuCard(actions: actions, palette: palette)
        let topLeftX = rightAligned ? (rectOnScreen.maxX - size.width) : rectOnScreen.minX
        let topLeftY = rectOnScreen.minY - 4
        presentAtScreenTopLeft(
            NSPoint(x: topLeftX, y: topLeftY),
            in: win,
            returnFocus: anchorView,
            actions: actions,
            palette: palette
        )
        activeAnchorView = anchorView
    }

    private func setFocusedIndex(_ next: Int) {
        focusedIndex = next
        for (idx, v) in itemViews.enumerated() {
            v.isKeyboardFocused = (idx == next)
        }
    }

    private func installEventMonitors() {
        mouseDownMonitor = NSEvent.addLocalMonitorForEvents(
            matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]
        ) { [weak self] event in
            guard let self, let panel = self.activePanel else { return event }
            if event.window === panel {
                return event
            }
            if event.type == .leftMouseDown,
               let anchor = self.activeAnchorView,
               let win = anchor.window,
               event.window === win
            {
                let ptInAnchor = anchor.convert(event.locationInWindow, from: nil)
                if anchor.bounds.insetBy(dx: -4, dy: -4).contains(ptInAnchor) {
                    self.suppressedToggleAnchorView = anchor
                    self.suppressedToggleUntil = ProcessInfo.processInfo.systemUptime + 0.35
                }
            }
            // Outside click / right-click: close immediately AND pass event through in 1 click!
            self.dismiss(restoreFocus: false)
            return event
        }

        scrollMonitor = NSEvent.addLocalMonitorForEvents(matching: [.scrollWheel]) { [weak self] event in
            guard let self, let panel = self.activePanel else { return event }
            if event.window === panel {
                return event
            }
            self.dismiss(restoreFocus: false)
            return event
        }

        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown]) { [weak self] event in
            guard let self, self.activePanel != nil else { return event }
            let enabledIndices = self.itemViews.enumerated().compactMap { $0.element.menuAction.disabled ? nil : $0.offset }
            switch event.keyCode {
            case 53: // Escape
                self.dismiss(restoreFocus: true)
                return nil
            case 48: // Tab
                self.dismiss(restoreFocus: true)
                return nil
            case 125: // ArrowDown
                guard !enabledIndices.isEmpty else { return nil }
                if let pos = enabledIndices.firstIndex(of: self.focusedIndex) {
                    self.setFocusedIndex(enabledIndices[(pos + 1) % enabledIndices.count])
                } else {
                    self.setFocusedIndex(enabledIndices[0])
                }
                return nil
            case 126: // ArrowUp
                guard !enabledIndices.isEmpty else { return nil }
                if let pos = enabledIndices.firstIndex(of: self.focusedIndex) {
                    self.setFocusedIndex(enabledIndices[(pos + enabledIndices.count - 1) % enabledIndices.count])
                } else if let last = enabledIndices.last {
                    self.setFocusedIndex(last)
                }
                return nil
            case 115: // Home
                if let first = enabledIndices.first { self.setFocusedIndex(first) }
                return nil
            case 119: // End
                if let last = enabledIndices.last { self.setFocusedIndex(last) }
                return nil
            case 36, 76, 49: // Return, Enter, Space
                if self.focusedIndex >= 0,
                   self.focusedIndex < self.itemViews.count,
                   !self.itemViews[self.focusedIndex].menuAction.disabled
                {
                    let run = self.itemViews[self.focusedIndex].menuAction.run
                    self.dismiss(restoreFocus: true)
                    run()
                }
                return nil
            default:
                return event
            }
        }
    }

    private func buildMenuCard(
        actions: [SessionMenuAction],
        palette: DesignPalette
    ) -> (NSView, [TaskMenuItemRowView], NSSize) {
        let font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        var maxTextW: CGFloat = 136
        for act in actions {
            let w = ceil((act.label as NSString).size(withAttributes: [.font: font]).width)
            maxTextW = max(maxTextW, w)
        }
        let cardWidth = min(280, max(200, maxTextW + 58))
        let itemWidth = cardWidth - 12
        let rowHeight: CGFloat = 34
        let cardHeight = CGFloat(actions.count) * rowHeight + CGFloat(max(0, actions.count - 1)) * 2 + 12

        let card = NSView(frame: NSRect(x: 0, y: 0, width: cardWidth, height: cardHeight))
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.masksToBounds = true
        card.layer?.backgroundColor = palette.elevated.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderStrong.nsColor.cgColor

        var rows: [TaskMenuItemRowView] = []
        var yTop = cardHeight - 6
        for act in actions {
            yTop -= rowHeight
            let row = TaskMenuItemRowView(
                frame: NSRect(x: 6, y: yTop, width: itemWidth, height: rowHeight),
                action: act,
                palette: palette
            ) { [weak self] in
                self?.dismiss(restoreFocus: true)
                act.run()
            }
            card.addSubview(row)
            rows.append(row)
            yTop -= 2
        }
        return (card, rows, NSSize(width: cardWidth, height: cardHeight))
    }
}

@MainActor
private final class TaskMenuItemRowView: NSView {
    let menuAction: SessionMenuAction
    private let palette: DesignPalette
    private let onInvoke: () -> Void
    private var isHovered: Bool = false { didSet { refreshBackground() } }
    var isKeyboardFocused: Bool = false { didSet { refreshBackground() } }
    private var tracking: NSTrackingArea?

    init(
        frame: NSRect,
        action: SessionMenuAction,
        palette: DesignPalette,
        onInvoke: @escaping () -> Void
    ) {
        self.menuAction = action
        self.palette = palette
        self.onInvoke = onInvoke
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 8
        alphaValue = action.disabled ? 0.48 : 1.0
        toolTip = action.reason

        let fgColor = (action.danger ? palette.errFg : palette.textPrimary).nsColor
        let iconColor = (action.danger ? palette.errFg : palette.textMuted).nsColor

        let iconView = NSImageView(
            image: LucideVectorCatalog.nsImage(for: action.icon, size: 15, color: iconColor)
        )
        iconView.frame = NSRect(x: 10, y: floor((frame.height - 15) * 0.5), width: 15, height: 15)
        addSubview(iconView)

        let lbl = NSTextField(labelWithString: action.label)
        lbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        lbl.textColor = fgColor
        lbl.lineBreakMode = .byTruncatingTail
        let textH = ceil(lbl.fittingSize.height)
        lbl.frame = NSRect(
            x: 35,
            y: floor((frame.height - textH) * 0.5),
            width: max(40, frame.width - 45),
            height: textH
        )
        addSubview(lbl)
        refreshBackground()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    private func refreshBackground() {
        guard !menuAction.disabled, (isHovered || isKeyboardFocused) else {
            layer?.backgroundColor = NSColor.clear.cgColor
            return
        }
        layer?.backgroundColor = (menuAction.danger ? palette.errBg : palette.hover).nsColor.cgColor
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) {
        if !menuAction.disabled { isHovered = true }
    }

    override func mouseExited(with event: NSEvent) {
        isHovered = false
    }

    override func resetCursorRects() {
        if !menuAction.disabled {
            addCursorRect(bounds, cursor: .pointingHand)
        }
    }

    override func mouseDown(with event: NSEvent) {
        guard !menuAction.disabled else { return }
        onInvoke()
    }
}

public struct TaskMenuTriggerButton<Label: View>: View {
    @Environment(\.agentHubPalette) private var palette
    public let rightAligned: Bool
    public let accessibilityLabel: String
    public let makeActions: () -> [SessionMenuAction]
    @ViewBuilder public let label: () -> Label
    @State private var anchorView: NSView?

    public init(
        rightAligned: Bool = false,
        accessibilityLabel: String,
        makeActions: @escaping () -> [SessionMenuAction],
        @ViewBuilder label: @escaping () -> Label
    ) {
        self.rightAligned = rightAligned
        self.accessibilityLabel = accessibilityLabel
        self.makeActions = makeActions
        self.label = label
    }

    public var body: some View {
        Button {
            guard let rawAnchor = anchorView else { return }
            let resolved = MenuAnchorProbeNSView.resolveAnchorView(rawAnchor)
            AgentHubTaskMenuPresenter.shared.presentBelowAnchorView(
                resolved,
                rightAligned: rightAligned,
                actions: makeActions(),
                palette: palette
            )
        } label: {
            label()
                .background(MenuAnchorProbeNSView(capturedView: $anchorView))
        }
        .buttonStyle(PlainHitButtonStyle())
        .accessibilityLabel(accessibilityLabel)
    }
}

private struct SessionRowRightClickCaptureRepresentable: NSViewRepresentable {
    var onSecondaryTrigger: (NSPoint, NSView) -> Void

    func makeNSView(context: Context) -> SessionRowRightClickCaptureNSView {
        let v = SessionRowRightClickCaptureNSView()
        v.onSecondaryTrigger = onSecondaryTrigger
        return v
    }

    func updateNSView(_ nsView: SessionRowRightClickCaptureNSView, context: Context) {
        nsView.onSecondaryTrigger = onSecondaryTrigger
    }
}

private final class SessionRowRightClickCaptureNSView: NSView {
    var onSecondaryTrigger: ((NSPoint, NSView) -> Void)?

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard bounds.contains(point), let ev = NSApp.currentEvent else { return nil }
        if ev.type == .rightMouseDown || ev.type == .rightMouseUp {
            return self
        }
        if (ev.type == .leftMouseDown || ev.type == .leftMouseUp) && ev.modifierFlags.contains(.control) {
            return self
        }
        return nil
    }

    override func rightMouseDown(with event: NSEvent) {
        onSecondaryTrigger?(NSEvent.mouseLocation, self)
    }

    override func mouseDown(with event: NSEvent) {
        if event.modifierFlags.contains(.control) {
            onSecondaryTrigger?(NSEvent.mouseLocation, self)
            return
        }
        super.mouseDown(with: event)
    }
}

public struct RenameableSessionRowView: View {
    @Environment(\.agentHubPalette) private var palette
    public let sessionKey: String
    public var leadingBrandIcon: AgentBrandId? = nil
    public let title: String
    public var idLine: String? = nil
    public let subtitle: String
    public var inlinePreview: String = ""
    public var isCodexCompactRow: Bool = false
    public let statusText: String
    public let running: Bool
    public let storageSize: SessionStorageSize?
    public let selected: Bool
    public let selecting: Bool
    public let multiSelected: Bool
    public let disabled: Bool
    public let canRename: Bool
    public let renameBlockReason: String?
    public var externalRenameRequestId: Int = 0
    public var rowMenuActions: [SessionMenuAction] = []
    public var batchMenuActions: [SessionMenuAction] = []
    public var hasRowMenu: Bool = true
    public var batchSelectedCount: Int = 0
    public var makeRowMenuActions: (() -> [SessionMenuAction])? = nil
    public var makeBatchMenuActions: (() -> [SessionMenuAction])? = nil
    public let onRowClick: (SessionClick) -> Void
    public var onToggleCheckbox: () -> Void = {}
    public let onRename: ((String) async throws -> Void)?

    @State private var editing: Bool = false
    @State private var draft: String = ""
    @State private var saving: Bool = false
    @State private var errorText: String?
    @State private var failedDraft: String?
    @State private var isRowHovered: Bool = false

    private func resolvedRowActions() -> [SessionMenuAction] {
        makeRowMenuActions?() ?? rowMenuActions
    }

    private func resolvedBatchActions() -> [SessionMenuAction] {
        makeBatchMenuActions?() ?? batchMenuActions
    }

    public var body: some View {
        let effectiveBatchAvailable = makeBatchMenuActions != nil ? (batchSelectedCount >= 2) : !batchMenuActions.isEmpty
        let useBatchMenu = multiSelected && effectiveBatchAvailable
        let suppressSingleEllipsis = (selecting && !useBatchMenu) || multiSelected
        let showRowEllipsis = !suppressSingleEllipsis && !disabled && (makeRowMenuActions != nil ? hasRowMenu : !rowMenuActions.isEmpty)

        VStack(alignment: .leading, spacing: 4) {
            if editing {
                VStack(alignment: .leading, spacing: 4) {
                    IMEGuardedInlineTextField(
                        text: $draft,
                        maxLength: 200,
                        placeholder: "线程名称",
                        autoFocus: true,
                        onSubmit: { Task { await saveRename(restoreFocus: true) } },
                        onCancel: {
                            editing = false
                            errorText = nil
                        },
                        onBlur: { Task { await saveRename(restoreFocus: false) } }
                    )
                    .frame(height: 30)
                    .padding(.horizontal, 10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(palette.background.color))
                    .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))

                    Text(saving ? "正在保存…" : "Enter 保存 · Esc 取消")
                        .font(DesignFonts.body(10))
                        .foregroundStyle(palette.textMuted.color)
                }
                .padding(.vertical, 8)
                .padding(.horizontal, 10)
                .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.accentBorder.color, lineWidth: 1))
            } else {
                ZStack(alignment: .topTrailing) {
                    Button {
                        let currentEvent = NSApp.currentEvent
                        let flags = currentEvent?.modifierFlags ?? []
                        let click = SessionClick(
                            detail: max(1, currentEvent?.clickCount ?? 1),
                            metaKey: flags.contains(.command),
                            shiftKey: flags.contains(.shift),
                            ctrlKey: flags.contains(.control)
                        )
                        if click.ctrlKey {
                            return
                        } else if !selecting && click.detail >= 2 && !click.metaKey && !click.shiftKey {
                            beginRename()
                        } else {
                            onRowClick(click)
                        }
                    } label: {
                        VStack(alignment: .leading, spacing: 3) {
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                if let leadingBrandIcon {
                                    AgentBrandIcon(leadingBrandIcon, size: 14)
                                        .foregroundStyle(palette.textSecondary.color)
                                        .frame(width: 14, height: 14)
                                        .alignmentGuide(.firstTextBaseline) { d in d[.bottom] - 2 }
                                }
                                Text(title)
                                    .font(DesignFonts.body(13, weight: .medium))
                                    .foregroundStyle(palette.textPrimary.color)
                                    .lineSpacing(2.6)
                                    .lineLimit(2)
                                    .multilineTextAlignment(.leading)
                            }

                            if isCodexCompactRow {
                                if !subtitle.isEmpty {
                                    Text(subtitle)
                                        .font(DesignFonts.body(11))
                                        .foregroundStyle(palette.textMuted.color)
                                        .lineLimit(1)
                                }
                                HStack(spacing: 6) {
                                    if running {
                                        RunningIndicator(size: 13)
                                    } else {
                                        Text(statusText)
                                            .font(DesignFonts.body(11))
                                            .foregroundStyle(palette.textMuted.color)
                                            .lineLimit(1)
                                            .fixedSize(horizontal: true, vertical: false)
                                    }
                                    if !inlinePreview.isEmpty {
                                        Text(inlinePreview)
                                            .font(DesignFonts.body(12))
                                            .foregroundStyle(palette.textMuted.color)
                                            .lineLimit(1)
                                    }
                                    Spacer(minLength: 4)
                                    SessionStorageBadgeView(size: storageSize)
                                }
                            } else {
                                if let idLine, !idLine.isEmpty {
                                    Text(idLine)
                                        .font(DesignFonts.mono(11))
                                        .foregroundStyle(palette.textMuted.color)
                                        .lineLimit(1)
                                }
                                if !subtitle.isEmpty {
                                    Text(subtitle)
                                        .font(DesignFonts.body(12))
                                        .foregroundStyle(palette.textMuted.color)
                                        .lineLimit(1)
                                }
                                HStack(spacing: 6) {
                                    if running {
                                        RunningIndicator(size: 13)
                                    } else {
                                        Text(statusText)
                                            .font(DesignFonts.body(12))
                                            .foregroundStyle(palette.textMuted.color)
                                            .lineLimit(1)
                                    }
                                    Spacer(minLength: 4)
                                    SessionStorageBadgeView(size: storageSize)
                                }
                            }
                        }
                        .padding(.vertical, 9)
                        .padding(.leading, 11)
                        .padding(.trailing, suppressSingleEllipsis ? 11 : 34)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(FullWidthRowButtonStyle(
                        selected: selected && !multiSelected,
                        multiSelected: multiSelected,
                        cornerRadius: 10
                    ))
                    .disabled(disabled)
                    .overlay(
                        SessionRowRightClickCaptureRepresentable { screenPoint, view in
                            guard !disabled else { return }
                            if selecting && !useBatchMenu { return }
                            let actions = useBatchMenu ? resolvedBatchActions() : resolvedRowActions()
                            guard !actions.isEmpty else { return }
                            AgentHubTaskMenuPresenter.shared.presentAtScreenTopLeft(
                                screenPoint,
                                in: view.window,
                                returnFocus: view,
                                actions: actions,
                                palette: palette
                            )
                        }
                    )

                    if showRowEllipsis {
                        TaskMenuTriggerButton(
                            rightAligned: false,
                            accessibilityLabel: "线程操作",
                            makeActions: { resolvedRowActions() }
                        ) {
                            LucideIcon(.moreHorizontal, size: 17)
                                .foregroundStyle(palette.textMuted.color)
                                .frame(width: 28, height: 28)
                                .background(
                                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                                        .fill(palette.hover.color.opacity(isRowHovered ? 0.65 : 0))
                                )
                                .contentShape(Rectangle())
                        }
                        .padding(.top, 8)
                        .padding(.trailing, 6)
                        .opacity(isRowHovered ? 1.0 : 0.0)
                        .allowsHitTesting(isRowHovered)
                    }
                }
                .onHover { hovered in
                    if isRowHovered != hovered { isRowHovered = hovered }
                }
            }

            if let errorText {
                Text(errorText)
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.errFg.color)
                    .padding(.vertical, 4)
                    .padding(.horizontal, 10)
            }
        }
        .onChange(of: selecting) { isSel in
            if isSel {
                editing = false
                errorText = nil
                if !useBatchMenu { AgentHubTaskMenuPresenter.shared.dismiss() }
            }
        }
        .onChange(of: multiSelected) { isMulti in
            if isMulti { editing = false; errorText = nil }
        }
        .onChange(of: externalRenameRequestId) { reqId in
            if reqId > 0 {
                beginRename()
            }
        }
    }

    public func beginRename() {
        guard onRename != nil, !disabled, !selecting, !multiSelected else { return }
        if !canRename {
            errorText = renameBlockReason ?? "当前线程不可改名"
            return
        }
        failedDraft = nil
        errorText = nil
        draft = title
        editing = true
    }

    private func saveRename(restoreFocus: Bool) async {
        guard let onRename, editing, !saving else { return }
        let next = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !next.isEmpty else {
            errorText = "名称不能为空"
            return
        }
        if next == title {
            editing = false
            errorText = nil
            return
        }
        // Blur must NOT replay a write that already failed for this exact draft!
        if !restoreFocus && next == failedDraft {
            editing = false
            errorText = nil
            return
        }
        saving = true
        errorText = nil
        defer { saving = false }
        do {
            try await onRename(next)
            editing = false
        } catch {
            failedDraft = next
            errorText = error.localizedDescription
        }
    }
}

// MARK: - Frameless Collapsible Project Workspace Group Header

public struct ProjectWorkspaceGroupHeaderView: View {
    @Environment(\.agentHubPalette) private var palette
    public let displayName: String
    public let fullPath: String?
    public let count: Int
    public let status: ProjectWorkspaceAggregateStatus
    public let expanded: Bool
    public let isFilteredToThisGroup: Bool
    public let onToggleExpanded: () -> Void
    public var onFilterOnlyThisGroup: (() -> Void)? = nil
    public var onSelectAllInGroup: (() -> Void)? = nil
    public var onCopyPath: ((String) -> Void)? = nil

    @State private var hovered: Bool = false

    public init(
        displayName: String,
        fullPath: String?,
        count: Int,
        status: ProjectWorkspaceAggregateStatus,
        expanded: Bool,
        isFilteredToThisGroup: Bool = false,
        onToggleExpanded: @escaping () -> Void,
        onFilterOnlyThisGroup: (() -> Void)? = nil,
        onSelectAllInGroup: (() -> Void)? = nil,
        onCopyPath: ((String) -> Void)? = nil
    ) {
        self.displayName = displayName
        self.fullPath = fullPath
        self.count = count
        self.status = status
        self.expanded = expanded
        self.isFilteredToThisGroup = isFilteredToThisGroup
        self.onToggleExpanded = onToggleExpanded
        self.onFilterOnlyThisGroup = onFilterOnlyThisGroup
        self.onSelectAllInGroup = onSelectAllInGroup
        self.onCopyPath = onCopyPath
    }

    public var body: some View {
        Button(action: onToggleExpanded) {
            HStack(spacing: 6) {
                LucideIcon(expanded ? .chevronDown : .chevronRight, size: 13)
                    .foregroundStyle((expanded ? palette.textSecondary : palette.textMuted).color)

                LucideIcon(.folderGit2, size: 13)
                    .foregroundStyle((isFilteredToThisGroup ? palette.accent : palette.textMuted).color)

                Text(displayName)
                    .font(DesignFonts.body(12, weight: .semibold))
                    .foregroundStyle(palette.textSecondary.color)
                    .lineLimit(1)

                Spacer(minLength: 4)

                statusIndicator

                Text("\(count)")
                    .font(DesignFonts.tabularBody(11, weight: .medium))
                    .foregroundStyle(palette.textMuted.color)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 1)
                    .background(Capsule().fill(palette.elevated.color))
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .frame(minHeight: 28)
            .background(
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .fill(hovered ? palette.hover.color.opacity(0.65) : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
        .onHover { hovered = $0 }
        .help(fullPath ?? displayName)
        .accessibilityLabel("\(displayName)，\(count) 个会话")
        .overlay(
            SessionRowRightClickCaptureRepresentable { screenPoint, view in
                let actions = buildContextActions()
                guard !actions.isEmpty else { return }
                AgentHubTaskMenuPresenter.shared.presentAtScreenTopLeft(
                    screenPoint,
                    in: view.window,
                    returnFocus: view,
                    actions: actions,
                    palette: palette
                )
            }
        )
    }

    @ViewBuilder
    private var statusIndicator: some View {
        switch status {
        case .attention:
            Circle()
                .fill(palette.warnFg.color)
                .frame(width: 7, height: 7)
                .accessibilityLabel("待处理")
        case .running:
            RunningIndicator(size: 11)
        case .failed:
            Circle()
                .fill(palette.errFg.color)
                .frame(width: 7, height: 7)
                .accessibilityLabel("异常")
        case .idle:
            EmptyView()
        }
    }

    private func buildContextActions() -> [SessionMenuAction] {
        var actions: [SessionMenuAction] = []
        if let onFilterOnlyThisGroup {
            actions.append(SessionMenuAction(
                id: "filter-workspace",
                label: isFilteredToThisGroup ? "显示全部工作区" : "仅看此工作区",
                icon: isFilteredToThisGroup ? .x : .slidersHorizontal,
                run: onFilterOnlyThisGroup
            ))
        }
        if let onSelectAllInGroup, expanded, count > 0 {
            actions.append(SessionMenuAction(
                id: "select-workspace-all",
                label: "全选此工作区可见会话",
                icon: .checkCircle2,
                run: onSelectAllInGroup
            ))
        }
        if let onCopyPath, let fullPath, !fullPath.isEmpty {
            actions.append(SessionMenuAction(
                id: "copy-workspace-path",
                label: "复制工作区路径",
                icon: .copy,
                run: { onCopyPath(fullPath) }
            ))
        }
        return actions
    }
}

// MARK: - Bottom .session-batch-controls & Backdrop-Safe ConfirmDialog

public struct SessionBatchBarAndDialogView: View {
    @Environment(\.agentHubPalette) private var palette
    public let selectedCount: Int
    public let operations: [SessionOperation]
    public let busy: Bool
    public let notice: String?
    public var prepareError: String? = nil
    public var executeError: String? = nil
    public let preview: SessionBatchPreview?
    public let result: SessionBatchResult?
    public let onSelectAll: () -> Void
    public let onClear: () -> Void
    public let onPrepareOperation: (SessionOperation) -> Void
    public let onConfirmExecute: () -> Void
    public let onDismissPreview: () -> Void

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if selectedCount > 0 {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("已选 \(selectedCount)")
                            .font(DesignFonts.body(12, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 3)
                            .background(Capsule().fill(palette.selected.color))

                        Button("全选", action: onSelectAll)
                            .font(DesignFonts.body(12, weight: .semibold))
                            .foregroundStyle(palette.accent.color)
                            .buttonStyle(PlainHitButtonStyle())
                            .disabled(busy)

                        Spacer()

                        Button(action: onClear) {
                            LucideIcon(.x, size: 15)
                        }
                        .buttonStyle(IconButtonStyle(dimension: 26))
                        .disabled(busy)
                        .accessibilityLabel("取消多选")
                    }

                    FlowButtonRow(spacing: 6, lineSpacing: 6) {
                        ForEach(operations, id: \.rawValue) { op in
                            if op == .delete {
                                Button("\(op.label)所选线程") {
                                    onPrepareOperation(op)
                                }
                                .buttonStyle(DangerButtonStyle())
                                .disabled(busy)
                            } else {
                                Button("\(op.label)所选线程") {
                                    onPrepareOperation(op)
                                }
                                .buttonStyle(SecondaryButtonStyle())
                                .disabled(busy)
                            }
                        }
                    }
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 12).fill(palette.elevated.color))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(palette.borderStrong.color, lineWidth: 1))
            }

            if let notice, !notice.isEmpty {
                Text(notice)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }

            if let prepareError, !prepareError.isEmpty {
                Text(prepareError)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }

            if let executeError, !executeError.isEmpty {
                Text("\(executeError)。请刷新并重新预览。")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }

            if let result {
                let okCount = result.items.filter { $0.status == "succeeded" }.count
                let failedItems = result.items.filter { $0.status != "succeeded" }
                VStack(alignment: .leading, spacing: 6) {
                    Text("已完成 \(okCount) 项，未完成 \(failedItems.count) 项")
                        .font(DesignFonts.body(12, weight: .medium))
                        .foregroundStyle(palette.textPrimary.color)

                    if !failedItems.isEmpty {
                        ScrollView(.vertical) {
                            VStack(alignment: .leading, spacing: 6) {
                                ForEach(Array(failedItems.enumerated()), id: \.offset) { _, item in
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text("\(item.sessionKey)：\(item.message ?? "操作失败")")
                                            .font(DesignFonts.body(12))
                                            .foregroundStyle(palette.errFg.color)
                                        if let deletedIds = item.deletedIds, !deletedIds.isEmpty {
                                            Text("已删除 \(deletedIds.count) 个线程")
                                                .font(DesignFonts.body(11))
                                                .foregroundStyle(palette.errFg.color)
                                        }
                                        ForEach(item.residuals ?? [], id: \.self) { residual in
                                            DeletePathCodeBlockView(residual)
                                        }
                                    }
                                }
                            }
                        }
                        .frame(maxHeight: 160)
                    }
                }
            }
        }
    }
}

// MARK: - Shared Workspace Sidebar Controls & Filter Menu Builder

public enum WorkspaceFilterMenuBuilder {
    public static func buildActions<Item>(
        currentFilter: ProjectWorkspaceFilter,
        groups: [ProjectWorkspaceGroup<Item>],
        idPrefix: String = "ws-filter",
        onSelect: @escaping (ProjectWorkspaceFilter) -> Void
    ) -> [SessionMenuAction] {
        var actions: [SessionMenuAction] = [
            SessionMenuAction(
                id: "\(idPrefix)-all",
                label: "全部工作区",
                icon: currentFilter == .all ? .check : .folderGit2,
                run: { onSelect(.all) }
            )
        ]
        if case let .workspace(selectedKey, includeSub) = currentFilter,
           selectedKey.path != nil
        {
            actions.append(SessionMenuAction(
                id: "\(idPrefix)-sub-toggle",
                label: includeSub ? "包含子目录：已开启" : "包含子目录：已关闭",
                icon: includeSub ? .check : .gitFork,
                run: {
                    onSelect(.workspace(selectedKey, includeSubdirectories: !includeSub))
                }
            ))
        }
        for group in groups {
            let isSelected = (currentFilter.selectedKey == group.key)
            actions.append(SessionMenuAction(
                id: "\(idPrefix)-\(group.id)",
                label: "\(group.displayName) (\(group.items.count))",
                icon: isSelected ? .check : .folderOpen,
                reason: group.fullPath,
                run: {
                    let keepSub = currentFilter.includeSubdirectories && group.key.path != nil
                    onSelect(.workspace(group.key, includeSubdirectories: keepSub))
                }
            ))
        }
        return actions
    }
}

public struct WorkspaceSidebarFilterMenuButton: View {
    @Environment(\.agentHubPalette) private var palette
    public let isFiltered: Bool
    public let makeActions: () -> [SessionMenuAction]

    public init(isFiltered: Bool, makeActions: @escaping () -> [SessionMenuAction]) {
        self.isFiltered = isFiltered
        self.makeActions = makeActions
    }

    public var body: some View {
        TaskMenuTriggerButton(
            rightAligned: true,
            accessibilityLabel: "筛选工作区",
            makeActions: makeActions
        ) {
            LucideIcon(.slidersHorizontal, size: 15)
                .foregroundStyle((isFiltered ? palette.accent : palette.textMuted).color)
                .frame(width: 28, height: 28)
                .background(
                    RoundedRectangle(cornerRadius: 7, style: .continuous)
                        .fill(isFiltered ? palette.accentSoft.color : Color.clear)
                )
                .contentShape(Rectangle())
        }
        .help("按工作区筛选")
    }
}

public struct WorkspaceSidebarSearchField: View {
    @Environment(\.agentHubPalette) private var palette
    public let placeholder: String
    @Binding public var query: String

    public init(placeholder: String, query: Binding<String>) {
        self.placeholder = placeholder
        self._query = query
    }

    public var body: some View {
        HStack(spacing: 8) {
            LucideIcon(.search, size: 15)
                .foregroundStyle(palette.textMuted.color)
            TextField(placeholder, text: $query)
                .textFieldStyle(.plain)
                .font(DesignFonts.body(13))
            if !query.isEmpty {
                Button { query = "" } label: { LucideIcon(.x, size: 13) }
                    .buttonStyle(IconButtonStyle(dimension: 22))
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 34)
        .background(RoundedRectangle(cornerRadius: 8).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderStrong.color, lineWidth: 1))
    }
}

public struct WorkspaceActiveFilterPillBanner: View {
    @Environment(\.agentHubPalette) private var palette
    public let filter: ProjectWorkspaceFilter
    public let onClear: () -> Void

    public init(filter: ProjectWorkspaceFilter, onClear: @escaping () -> Void) {
        self.filter = filter
        self.onClear = onClear
    }

    public var body: some View {
        if case let .workspace(filterKey, includeSub) = filter {
            HStack(spacing: 6) {
                LucideIcon(.folderGit2, size: 12)
                    .foregroundStyle(palette.accent.color)
                Text(ProjectWorkspaceEngine.compactDisplayName(for: filterKey) + (includeSub ? "（含子目录）" : ""))
                    .font(DesignFonts.body(11, weight: .medium))
                    .foregroundStyle(palette.textSecondary.color)
                    .lineLimit(1)
                Spacer(minLength: 4)
                Button(action: onClear) {
                    LucideIcon(.x, size: 12)
                        .foregroundStyle(palette.textMuted.color)
                        .frame(width: 20, height: 20)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())
                .accessibilityLabel("清除工作区筛选")
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(RoundedRectangle(cornerRadius: 7, style: .continuous).fill(palette.accentSoft.color))
        }
    }
}

public struct WorkspaceArchivedToggleRow: View {
    @Environment(\.agentHubPalette) private var palette
    @Binding public var isOpen: Bool
    public let archivedCount: Int

    public init(isOpen: Binding<Bool>, archivedCount: Int) {
        self._isOpen = isOpen
        self.archivedCount = archivedCount
    }

    public var body: some View {
        Button {
            isOpen.toggle()
        } label: {
            HStack(spacing: 8) {
                LucideIcon(isOpen ? .chevronDown : .chevronRight, size: 14)
                    .foregroundStyle((isOpen ? palette.accent : palette.textMuted).color)
                Text("已归档")
                    .font(DesignFonts.body(12, weight: .medium))
                    .foregroundStyle(palette.textMuted.color)
                Spacer()
                if isOpen {
                    Text("\(archivedCount)")
                        .font(DesignFonts.tabularBody(11, weight: .medium))
                        .foregroundStyle(palette.textMuted.color)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(palette.elevated.color))
                }
            }
            .padding(.vertical, 6)
            .padding(.horizontal, 10)
            .frame(minHeight: 32)
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
    }
}

// MARK: - Itemized Batch Preview & Single-Session Delete Confirm Dialog Content

public struct SessionBatchPreviewDialogContentView: View {
    @Environment(\.agentHubPalette) private var palette
    public let preview: SessionBatchPreview
    public let busy: Bool
    public let onCancel: () -> Void
    public let onConfirm: () -> Void

    public init(
        preview: SessionBatchPreview,
        busy: Bool,
        onCancel: @escaping () -> Void,
        onConfirm: @escaping () -> Void
    ) {
        self.preview = preview
        self.busy = busy
        self.onCancel = onCancel
        self.onConfirm = onConfirm
    }

    public var body: some View {
        let allowed = preview.allowedCount
        let blocked = preview.blockedCount
        let formattedTotalBytes = SessionStorageFormatter.formatThousands(preview.totalBytes)

        VStack(alignment: .leading, spacing: 12) {
            Text("\(preview.operation.label)所选线程")
                .font(DesignFonts.body(16, weight: .semibold))
                .foregroundStyle(palette.textPrimary.color)

            Text(
                "已定位 \(allowed) 项，定位失败 \(blocked) 项"
                    + (preview.descendantCount > 0 ? "，含 \(preview.descendantCount) 个后代线程" : "")
                    + "，共 \(preview.fileCount) 个文件 · \(formattedTotalBytes) 字节。"
                    + (preview.operation == .delete
                        ? "仅删除下列会话数据，工作目录和配置保留。"
                        : "确认后更新所选线程的归档状态。")
            )
            .font(DesignFonts.body(13))
            .foregroundStyle(palette.textSecondary.color)

            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(Array(preview.items.enumerated()), id: \.offset) { _, item in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.title)
                                .font(DesignFonts.body(13, weight: .semibold))
                                .foregroundStyle(palette.textPrimary.color)
                            Text(item.id)
                                .font(DesignFonts.mono(11))
                                .foregroundStyle(palette.textMuted.color)

                            if let descendants = item.descendants, !descendants.isEmpty {
                                Text("同时删除 \(descendants.count) 个后代线程")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.textSecondary.color)
                                ForEach(descendants, id: \.self) { descId in
                                    Text(descId)
                                        .font(DesignFonts.mono(11))
                                        .foregroundStyle(palette.textMuted.color)
                                }
                            }

                            ForEach(item.paths, id: \.self) { path in
                                DeletePathCodeBlockView(path)
                            }

                            Text("\(SessionStorageFormatter.formatThousands(item.bytes)) 字节")
                                .font(DesignFonts.tabularBody(12))
                                .foregroundStyle(palette.textMuted.color)

                            ForEach(item.warnings ?? [], id: \.self) { warning in
                                Text(warning)
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.errFg.color)
                            }

                            if !item.allowed {
                                Text(item.reason ?? "定位失败")
                                    .font(DesignFonts.body(12))
                                    .foregroundStyle(palette.errFg.color)
                            }
                        }
                        .padding(.vertical, 6)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            .frame(maxHeight: 280)

            HStack(spacing: 8) {
                Spacer()
                Button("取消", action: onCancel)
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(busy)

                if preview.operation == .delete {
                    Button(busy ? "正在处理…" : "确认\(preview.operation.label) \(allowed) 项", action: onConfirm)
                        .buttonStyle(DangerButtonStyle())
                        .disabled(allowed == 0 || busy)
                } else {
                    Button(busy ? "正在处理…" : "确认\(preview.operation.label) \(allowed) 项", action: onConfirm)
                        .buttonStyle(PrimaryButtonStyle())
                        .disabled(allowed == 0 || busy)
                }
            }
        }
    }
}
