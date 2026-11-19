import AppKit
import SwiftUI

// MARK: - Provider Menu Action Builder (Exact 6-Platform Matrix from §2.3)

public struct SessionMenuAction: Identifiable {
    public let id: String
    public let label: String
    public let icon: LucideIconName
    public let danger: Bool
    public let disabled: Bool
    public let run: () -> Void

    public init(
        id: String,
        label: String,
        icon: LucideIconName,
        danger: Bool = false,
        disabled: Bool = false,
        run: @escaping () -> Void
    ) {
        self.id = id
        self.label = label
        self.icon = icon
        self.danger = danger
        self.disabled = disabled
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
        canArchive: Bool,
        canDelete: Bool,
        busy: Bool,
        latestReplyText: String?,
        onRename: (() -> Void)?,
        onToggleArchive: (() -> Void)?,
        onCopy: @escaping (String, String) -> Void,
        onRevealInFinder: @escaping (String) -> Void,
        onDelete: (() -> Void)?
    ) -> [SessionMenuAction] {
        var list: [SessionMenuAction] = []

        if isHeaderMenu, let onCopyReply = latestReplyText {
            list.append(SessionMenuAction(
                id: "copy-reply",
                label: "复制答复",
                icon: .copy,
                disabled: onCopyReply.isEmpty,
                run: { onCopy(onCopyReply, "已复制答复") }
            ))
        } else if isHeaderMenu {
            list.append(SessionMenuAction(
                id: "copy-reply",
                label: "复制答复",
                icon: .copy,
                disabled: true,
                run: {}
            ))
        }

        switch provider {
        case .codex:
            if let onRename {
                list.append(SessionMenuAction(id: "rename", label: "改名", icon: .pencil, disabled: !canRename || busy, run: onRename))
            }
            if let onToggleArchive {
                let archiveLabel = archived ? (isHeaderMenu ? "取消归档" : "恢复") : "归档"
                list.append(SessionMenuAction(
                    id: archived ? "restore" : "archive",
                    label: archiveLabel,
                    icon: archived ? .archiveRestore : .archive,
                    disabled: !canArchive || busy,
                    run: onToggleArchive
                ))
            }
            list.append(SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { onCopy(id, "已复制 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制路径") }))
            if let cmd = HistoryAndResumeHelpers.threadResumeCommand(id) {
                list.append(SessionMenuAction(id: "copy-resume", label: "复制恢复命令", icon: .terminal, run: { onCopy(cmd, "已复制恢复命令") }))
            }
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(id: "delete", label: "永久删除", icon: .trash2, danger: true, disabled: !canDelete || busy, run: onDelete))
            }

        case .claudeCode:
            if let onRename {
                list.append(SessionMenuAction(id: "rename", label: "改名", icon: .pencil, disabled: !canRename || busy, run: onRename))
            }
            list.append(SessionMenuAction(id: "copy-id", label: "复制线程 ID", icon: .copy, run: { onCopy(id, "已复制线程 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制文件路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制文件路径") }))
            let cmd = HistoryAndResumeHelpers.claudeResumeCommand(cwd: cwd, id: id)
            list.append(SessionMenuAction(id: "copy-resume", label: "复制恢复命令", icon: .terminal, run: { onCopy(cmd, "已复制恢复命令") }))
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(id: "delete", label: "删除任务文件", icon: .trash2, danger: true, disabled: !canDelete || busy, run: onDelete))
            }

        case .grok:
            if let onRename {
                list.append(SessionMenuAction(id: "rename", label: "改名", icon: .pencil, disabled: !canRename || busy, run: onRename))
            }
            list.append(SessionMenuAction(id: "copy-id", label: "复制线程 ID", icon: .copy, run: { onCopy(id, "已复制线程 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制路径") }))
            let cmd = HistoryAndResumeHelpers.grokResumeCommand(cwd: cwd, id: id)
            list.append(SessionMenuAction(id: "copy-resume", label: "复制恢复命令", icon: .terminal, run: { onCopy(cmd, "已复制恢复命令") }))
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(id: "delete", label: "删除任务文件", icon: .trash2, danger: true, disabled: !canDelete || busy, run: onDelete))
            }

        case .antigravity, .dsh:
            if let onRename {
                list.append(SessionMenuAction(id: "rename", label: "改名", icon: .pencil, disabled: !canRename || busy, run: onRename))
            }
            if let onToggleArchive {
                let archiveLabel = archived ? (isHeaderMenu ? "取消归档" : "恢复") : "归档"
                list.append(SessionMenuAction(
                    id: archived ? "restore" : "archive",
                    label: archiveLabel,
                    icon: archived ? .archiveRestore : .archive,
                    disabled: !canArchive || busy,
                    run: onToggleArchive
                ))
            }
            list.append(SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { onCopy(id, "已复制 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制路径") }))
            // Antigravity & DSH explicitly omit 复制恢复命令
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
                run: { onRevealInFinder(path) }
            ))
            if let onDelete {
                list.append(SessionMenuAction(id: "delete", label: "永久删除", icon: .trash2, danger: true, disabled: !canDelete || busy, run: onDelete))
            }

        case .grokbot:
            list.append(SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { onCopy(id, "已复制 ID") }))
            list.append(SessionMenuAction(id: "copy-path", label: "复制路径", icon: .copy, disabled: path.isEmpty, run: { onCopy(path, "已复制路径") }))
            list.append(SessionMenuAction(
                id: "reveal",
                label: "在 Finder 中显示",
                icon: .folderOpen,
                disabled: !isLocalMachine || path.isEmpty,
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

    @MainActor
    public static func buildNSMenu(actions: [SessionMenuAction]) -> NSMenu {
        let menu = NSMenu(title: "线程操作")
        menu.autoenablesItems = false
        for act in actions {
            let item = NSMenuItem(title: act.label, action: #selector(SessionMenuActionBox.invoke(_:)), keyEquivalent: "")
            let box = SessionMenuActionBox(action: act.run)
            item.target = box
            item.representedObject = box
            item.isEnabled = !act.disabled
            menu.addItem(item)
        }
        return menu
    }
}

@MainActor
private final class SessionMenuActionBox: NSObject {
    let action: () -> Void

    init(action: @escaping () -> Void) {
        self.action = action
    }

    @objc func invoke(_ sender: Any?) {
        action()
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
            .font(DesignFonts.tabularBody(11))
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

public struct RenameableSessionRowView: View {
    @Environment(\.agentHubPalette) private var palette
    public let sessionKey: String
    public let title: String
    public let subtitle: String
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
    public let rowMenuActions: [SessionMenuAction]
    public let batchMenuActions: [SessionMenuAction]
    public let onRowClick: (SessionClick) -> Void
    public let onToggleCheckbox: () -> Void
    public let onRename: ((String) async throws -> Void)?

    @State private var editing: Bool = false
    @State private var draft: String = ""
    @State private var saving: Bool = false
    @State private var errorText: String?
    @State private var failedDraft: String?

    public var body: some View {
        let useBatchMenu = multiSelected && !batchMenuActions.isEmpty
        let suppressSingleEllipsis = (selecting && !useBatchMenu) || multiSelected
        let contextActions: [SessionMenuAction] = useBatchMenu
            ? batchMenuActions
            : (!selecting ? rowMenuActions : [])

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
                HStack(spacing: 8) {
                    if selecting {
                        Button(action: onToggleCheckbox) {
                            ZStack {
                                RoundedRectangle(cornerRadius: 4)
                                    .fill((multiSelected ? palette.accent : palette.elevated).color)
                                    .frame(width: 16, height: 16)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                                    )
                                if multiSelected {
                                    LucideIcon(.check, size: 11)
                                        .foregroundStyle(palette.buttonFg.color)
                                }
                            }
                            .frame(width: 24, height: 24)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(PlainHitButtonStyle())
                    }

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
                            // Ctrl+Click opens native NSMenu at pointer without mutating selection or opening detail.
                            onRowClick(click)
                            if !contextActions.isEmpty {
                                let menu = ProviderMenuMatrixBuilder.buildNSMenu(actions: contextActions)
                                menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
                            }
                        } else if click.detail >= 2 && !click.metaKey && !click.shiftKey {
                            beginRename()
                        } else {
                            onRowClick(click)
                        }
                    } label: {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(title)
                                .font(DesignFonts.body(13, weight: .medium))
                                .foregroundStyle(palette.textPrimary.color)
                                .lineSpacing(2.6)
                                .lineLimit(2)
                                .multilineTextAlignment(.leading)

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
                                }
                                Spacer(minLength: 6)
                                SessionStorageBadgeView(size: storageSize)
                            }
                        }
                        .padding(.vertical, 9)
                        .padding(.leading, 11)
                        .padding(.trailing, 8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(FullWidthRowButtonStyle(
                        selected: selected && !multiSelected,
                        multiSelected: multiSelected,
                        cornerRadius: 10
                    ))
                    .disabled(disabled)
                    .contextMenu {
                        ForEach(contextActions) { act in
                            Button(act.label, action: act.run)
                                .disabled(act.disabled)
                        }
                    }

                    if !suppressSingleEllipsis && !disabled && !rowMenuActions.isEmpty {
                        Menu {
                            ForEach(rowMenuActions) { act in
                                Button(act.label, action: act.run)
                                    .disabled(act.disabled)
                            }
                        } label: {
                            LucideIcon(.moreHorizontal, size: 16)
                                .foregroundStyle(palette.textMuted.color)
                                .frame(width: 28, height: 28)
                                .contentShape(Rectangle())
                        }
                        .menuStyle(.borderlessButton)
                        .frame(width: 28, height: 28)
                        .accessibilityLabel("线程操作")
                    }
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
            if isSel { editing = false; errorText = nil }
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
                                ForEach(failedItems, id: \.sessionKey) { item in
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

public struct ModalConfirmDialogView<Content: View>: View {
    @Environment(\.agentHubPalette) private var palette
    public let busy: Bool
    public let onCancel: () -> Void
    @ViewBuilder public let content: () -> Content

    public init(busy: Bool, onCancel: @escaping () -> Void, @ViewBuilder content: @escaping () -> Content) {
        self.busy = busy
        self.onCancel = onCancel
        self.content = content
    }

    public var body: some View {
        GeometryReader { geo in
            ZStack {
                ModalScopePresenceView()
                    .frame(width: 0, height: 0)

                // Backdrop rgba(0,0,0,.55) — explicitly does NOT close on backdrop click!
                Color.black.opacity(0.55)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture {}

                VStack(alignment: .leading, spacing: 14) {
                    content()
                }
                .padding(24)
                .frame(width: min(520, max(280, geo.size.width - 32)))
                .background(
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .fill(palette.elevated.color)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                )
                .shadow(color: Color.black.opacity(0.30), radius: 18, x: 0, y: 8)
            }
        }
        .onExitCommand {
            if !busy { onCancel() }
        }
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
                    ForEach(preview.items, id: \.sessionKey) { item in
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

public struct SingleSessionDeleteConfirmContentView: View {
    @Environment(\.agentHubPalette) private var palette
    public let title: String
    public let paths: [String]
    public let summaryText: String
    public let busy: Bool
    public let errorText: String?
    public let warnings: [String]
    public let residuals: [String]
    public let onCancel: () -> Void
    public let onConfirm: () -> Void

    public init(
        title: String,
        paths: [String],
        summaryText: String,
        busy: Bool,
        errorText: String? = nil,
        warnings: [String] = [],
        residuals: [String] = [],
        onCancel: @escaping () -> Void,
        onConfirm: @escaping () -> Void
    ) {
        self.title = title
        self.paths = paths
        self.summaryText = summaryText
        self.busy = busy
        self.errorText = errorText
        self.warnings = warnings
        self.residuals = residuals
        self.onCancel = onCancel
        self.onConfirm = onConfirm
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("删除任务文件")
                .font(DesignFonts.body(16, weight: .semibold))
                .foregroundStyle(palette.textPrimary.color)

            Text(title)
                .font(DesignFonts.body(13, weight: .semibold))
                .foregroundStyle(palette.textPrimary.color)

            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(paths, id: \.self) { path in
                        DeletePathCodeBlockView(path)
                    }
                }
            }
            .frame(maxHeight: 160)

            Text(summaryText)
                .font(DesignFonts.body(13))
                .foregroundStyle(palette.textSecondary.color)

            if let errorText, !errorText.isEmpty {
                Text(errorText)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }

            ForEach(warnings, id: \.self) { warning in
                Text(warning)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }

            ForEach(residuals, id: \.self) { residual in
                DeletePathCodeBlockView(residual)
            }

            HStack(spacing: 8) {
                Spacer()
                Button("取消", action: onCancel)
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(busy)

                Button(action: onConfirm) {
                    HStack(spacing: 6) {
                        LucideIcon(.trash2, size: 16)
                        Text(busy ? "正在删除..." : "确认删除任务文件")
                    }
                }
                .buttonStyle(DangerButtonStyle())
                .disabled(busy)
            }
        }
    }
}
