import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

struct ConversationReaderTableView: NSViewRepresentable {
    let coordinator: ConversationTableCoordinator
    let rows: [ConversationRowItem]
    let scheme: ColorSchemeKind
    let workspace: WorkspaceId
    let sessionScopeKey: String
    let sessionProvider: SessionProvider
    let sessionKey: String
    let rootThreadId: String?
    let cwd: String?
    let hasTimelineRail: Bool
    let isSubagent: Bool
    var canLoadOlder: Bool
    var isLoadingOlder: Bool
    var isLoadOlderBlockedByError: Bool

    init(
        coordinator: ConversationTableCoordinator,
        rows: [ConversationRowItem],
        scheme: ColorSchemeKind,
        workspace: WorkspaceId,
        sessionScopeKey: String,
        sessionProvider: SessionProvider,
        sessionKey: String,
        rootThreadId: String? = nil,
        cwd: String? = nil,
        hasTimelineRail: Bool = true,
        isSubagent: Bool = false,
        canLoadOlder: Bool = true,
        isLoadingOlder: Bool = false,
        isLoadOlderBlockedByError: Bool = false
    ) {
        self.coordinator = coordinator
        self.rows = rows
        self.scheme = scheme
        self.workspace = workspace
        self.sessionScopeKey = sessionScopeKey
        self.sessionProvider = sessionProvider
        self.sessionKey = sessionKey
        self.rootThreadId = rootThreadId
        self.cwd = cwd
        self.hasTimelineRail = hasTimelineRail
        self.isSubagent = isSubagent
        self.canLoadOlder = canLoadOlder
        self.isLoadingOlder = isLoadingOlder
        self.isLoadOlderBlockedByError = isLoadOlderBlockedByError
    }

    func makeCoordinator() -> ConversationTableCoordinator {
        coordinator
    }

    private func syncCoordinatorProperties() {
        coordinator.scheme = scheme
        coordinator.workspace = workspace
        coordinator.sessionScopeKey = sessionScopeKey
        coordinator.sessionProvider = sessionProvider
        coordinator.sessionKey = sessionKey
        coordinator.rootThreadId = rootThreadId
        coordinator.cwd = cwd
        coordinator.hasTimelineRail = hasTimelineRail
        coordinator.isSubagent = isSubagent
        coordinator.canLoadOlder = canLoadOlder
        coordinator.isLoadingOlder = isLoadingOlder
        coordinator.isLoadOlderBlockedByError = isLoadOlderBlockedByError
    }

    func makeNSView(context: Context) -> NSScrollView {
        syncCoordinatorProperties()
        let scroll = ReaderOuterScrollView(frame: .zero)
        scroll.coordinator = coordinator
        scroll.wantsLayer = true
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = coordinator.palette.background.nsColor
        scroll.contentInsets = NSEdgeInsets(top: isSubagent ? 16 : 22, left: 0, bottom: isSubagent ? 16 : 32, right: 0)

        let table = NSTableView()
        table.headerView = nil
        table.style = .plain
        table.intercellSpacing = NSSize(width: 0, height: 12)
        table.selectionHighlightStyle = .none
        table.backgroundColor = .clear
        table.usesAutomaticRowHeights = false

        let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("MainConversationColumn"))
        col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.dataSource = coordinator
        table.delegate = coordinator

        scroll.documentView = table
        scroll.postsFrameChangedNotifications = true
        scroll.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            coordinator,
            selector: #selector(ConversationTableCoordinator.scrollViewFrameDidChange(_:)),
            name: NSView.frameDidChangeNotification,
            object: scroll
        )
        NotificationCenter.default.addObserver(
            coordinator,
            selector: #selector(ConversationTableCoordinator.clipViewBoundsDidChange(_:)),
            name: NSView.boundsDidChangeNotification,
            object: scroll.contentView
        )

        coordinator.scrollView = scroll
        coordinator.tableView = table
        coordinator.column = col
        coordinator.updateRowsPreservingAnchor(rows, preferAutoFollowBottom: true)
        return scroll
    }

    func updateNSView(_ nsView: NSScrollView, context: Context) {
        (nsView as? ReaderOuterScrollView)?.coordinator = coordinator
        let sessionChanged = (coordinator.sessionScopeKey != sessionScopeKey)
        let themeOrWorkspaceChanged = (coordinator.scheme != scheme || coordinator.workspace != workspace)
        syncCoordinatorProperties()
        nsView.backgroundColor = coordinator.palette.background.nsColor

        if sessionChanged || themeOrWorkspaceChanged || coordinator.rows != rows {
            coordinator.updateRowsPreservingAnchor(
                rows,
                preferAutoFollowBottom: !themeOrWorkspaceChanged,
                forceScrollToBottom: sessionChanged
            )
        }
    }
}
