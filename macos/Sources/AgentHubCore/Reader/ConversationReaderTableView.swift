import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

public struct ConversationReaderTableView: NSViewRepresentable {
    public let coordinator: ConversationTableCoordinator
    public let rows: [ConversationRowItem]
    public let scheme: ColorSchemeKind
    public let workspace: WorkspaceId
    public let sessionScopeKey: String
    public let sessionProvider: SessionProvider
    public let sessionKey: String
    public let rootThreadId: String?
    public let cwd: String?
    public let hasTimelineRail: Bool
    public let isSubagent: Bool
    public var canLoadOlder: Bool
    public var isLoadingOlder: Bool
    public var isLoadOlderBlockedByError: Bool

    public init(
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

    public func makeCoordinator() -> ConversationTableCoordinator {
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

    public func makeNSView(context: Context) -> NSScrollView {
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

    public func updateNSView(_ nsView: NSScrollView, context: Context) {
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
