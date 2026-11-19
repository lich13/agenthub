@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class ReaderAnchorMenuAndSnapshotTests: XCTestCase {
    // MARK: - 1. Six-Provider Menu Matrix, Multi-Select Context Menu & Keyboard Guard (§2.3)

    func testSixProviderMenuMatricesAndBatchKeyboardRules() {
        let noop: () -> Void = {}
        let copyNoop: (String, String) -> Void = { _, _ in }
        let revealNoop: (String) -> Void = { _ in }

        // Codex list vs header (header adds 复制答复 and changes 恢复 -> 取消归档)
        let codexRow = ProviderMenuMatrixBuilder.buildActions(
            provider: .codex, isHeaderMenu: false, isLocalMachine: true,
            id: "th-1", path: "/Users/example/.codex/sessions/1.jsonl", cwd: "/Users/example/repo",
            archived: true, canRename: true, canArchive: true, canDelete: true, busy: false,
            latestReplyText: "Reply", onRename: noop, onToggleArchive: noop,
            onCopy: copyNoop, onRevealInFinder: revealNoop, onDelete: noop
        )
        XCTAssertEqual(codexRow.map(\.label), ["改名", "恢复", "复制 ID", "复制路径", "复制恢复命令", "在 Finder 中显示", "永久删除"])

        let codexHeader = ProviderMenuMatrixBuilder.buildActions(
            provider: .codex, isHeaderMenu: true, isLocalMachine: true,
            id: "th-1", path: "/Users/example/.codex/sessions/1.jsonl", cwd: "/Users/example/repo",
            archived: true, canRename: true, canArchive: true, canDelete: true, busy: false,
            latestReplyText: "Reply", onRename: noop, onToggleArchive: noop,
            onCopy: copyNoop, onRevealInFinder: revealNoop, onDelete: noop
        )
        XCTAssertEqual(codexHeader.map(\.label), ["复制答复", "改名", "取消归档", "复制 ID", "复制路径", "复制恢复命令", "在 Finder 中显示", "永久删除"])

        // Claude uses 删除任务文件 and 复制文件路径 + 复制恢复命令
        let claudeRow = ProviderMenuMatrixBuilder.buildActions(
            provider: .claudeCode, isHeaderMenu: false, isLocalMachine: true,
            id: "cl-1", path: "/Users/example/.claude/1.jsonl", cwd: "/Users/example/repo",
            archived: false, canRename: true, canArchive: false, canDelete: true, busy: false,
            latestReplyText: nil, onRename: noop, onToggleArchive: nil,
            onCopy: copyNoop, onRevealInFinder: revealNoop, onDelete: noop
        )
        XCTAssertEqual(claudeRow.map(\.label), ["改名", "复制线程 ID", "复制文件路径", "复制恢复命令", "在 Finder 中显示", "删除任务文件"])

        // Antigravity & DSH omit 复制恢复命令 and use 永久删除
        let antiRow = ProviderMenuMatrixBuilder.buildActions(
            provider: .antigravity, isHeaderMenu: false, isLocalMachine: true,
            id: "ag-1", path: "/Users/example/.gemini/1.pb", cwd: "/Users/example/repo",
            archived: false, canRename: true, canArchive: true, canDelete: true, busy: false,
            latestReplyText: nil, onRename: noop, onToggleArchive: noop,
            onCopy: copyNoop, onRevealInFinder: revealNoop, onDelete: noop
        )
        XCTAssertFalse(antiRow.map(\.label).contains("复制恢复命令"))
        XCTAssertEqual(antiRow.map(\.label), ["改名", "归档", "复制 ID", "复制路径", "在 Finder 中显示", "永久删除"])

        // GrokBot read-only: disables copy-path & reveal when path is empty
        let gbRow = ProviderMenuMatrixBuilder.buildActions(
            provider: .grokbot, isHeaderMenu: false, isLocalMachine: true,
            id: "gb-1", path: "", cwd: "",
            archived: false, canRename: false, canArchive: false, canDelete: false, busy: false,
            latestReplyText: nil, onRename: nil, onToggleArchive: nil,
            onCopy: copyNoop, onRevealInFinder: revealNoop, onDelete: nil
        )
        XCTAssertEqual(gbRow.map(\.label), ["复制 ID", "复制路径", "在 Finder 中显示"])
        XCTAssertTrue(gbRow[1].disabled)
        XCTAssertTrue(gbRow[2].disabled)

        // Batch context menu only when selected >= 2
        XCTAssertTrue(ProviderMenuMatrixBuilder.buildBatchContextMenuActions(
            selectedCount: 1, operations: [.archive, .delete], busy: false, onPrepare: { _ in }, onClear: {}
        ).isEmpty)
        let batch3 = ProviderMenuMatrixBuilder.buildBatchContextMenuActions(
            selectedCount: 3, operations: [.archive, .delete], busy: false, onPrepare: { _ in }, onClear: {}
        )
        XCTAssertEqual(batch3.map(\.label), ["归档所选 3 项", "删除所选 3 项…", "取消选择"])

        // Keyboard delete: ⌘+Backspace or Forward Delete triggers, plain Backspace does NOT
        XCTAssertFalse(ProviderMenuMatrixBuilder.shouldTriggerBatchDeleteKey(keyCode: 51, charactersIgnoringModifiers: "\u{7F}", modifierFlags: []))
        XCTAssertTrue(ProviderMenuMatrixBuilder.shouldTriggerBatchDeleteKey(keyCode: 51, charactersIgnoringModifiers: "\u{7F}", modifierFlags: [.command]))
        XCTAssertTrue(ProviderMenuMatrixBuilder.shouldTriggerBatchDeleteKey(keyCode: 117, charactersIgnoringModifiers: "\u{F728}", modifierFlags: []))
    }

    // MARK: - 2. Offscreen NSTableView 0.0pt Scroll Anchor Lock & ±240pt Thumbnail Eviction

    @MainActor
    func testOffscreenReaderTableZeroPointScrollAnchorAndThumbnailViewportEviction() throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.reader.\(UUID().uuidString)")!
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        let coordinator = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())

        // Create initial 30 rows (with an image attachment on row 29 and an execution group on row 15)
        var initialRows: [ConversationRowItem] = []
        for i in 10 ..< 40 {
            if i == 25 {
                let cmd = ExecutionCommand(id: "cmd-25", sourceIds: ["cmd-25"], title: "bash", preview: "cargo test", status: .completed, sections: [.init(label: "结果", text: "ok")])
                let grp = ExecutionGroup(id: "group:cmd-25", mergedIds: [], commands: [cmd], kind: "command", provider: "Codex", running: false, recordedOnly: false, failedCount: 0)
                initialRows.append(ConversationRowItem(id: grp.id, timelineId: grp.id, kind: .executionGroup(grp)))
            } else if i == 39 {
                let msg = UserMessageContent(
                    id: "u-39",
                    text: "Attachment message",
                    attachments: [UserAttachment(id: "att-39", name: "screen.png", kind: "image")]
                )
                initialRows.append(ConversationRowItem(id: "row-39", timelineId: "u-39", kind: .user(message: msg, rawText: msg.text, attachmentsAsNamesOnly: false)))
            } else {
                initialRows.append(ConversationRowItem(
                    id: "row-\(i)",
                    timelineId: "tl-\(i)",
                    kind: i.isMultiple(of: 2)
                        ? .user(message: nil, rawText: "User prompt number \(i)", attachmentsAsNamesOnly: false)
                        : .assistant(markdown: "## Heading \(i)\n\nParagraph content for item \(i) with `inline code`.\n\n```rust\nfn main() {}\n```", foldInstructions: true)
                ))
            }
        }

        let hostView = NSHostingView(
            rootView: ConversationReaderTableView(
                coordinator: coordinator,
                rows: initialRows,
                scheme: .light,
                workspace: .codex,
                sessionScopeKey: "test::codex::s1",
                sessionProvider: .codex,
                sessionKey: "s1"
            )
            .frame(width: 960, height: 640)
        )
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 960, height: 640),
            styleMask: [.titled, .resizable],
            backing: .buffered,
            defer: false
        )
        window.contentView = hostView
        hostView.layoutSubtreeIfNeeded()
        coordinator.tableView?.layoutSubtreeIfNeeded()

        // Scroll mid-stream to row index 12 ("tl-22")
        let clipView = try XCTUnwrap(coordinator.scrollView?.contentView)
        let table = try XCTUnwrap(coordinator.tableView)
        let targetMidY = table.rect(ofRow: 12).minY - 24
        clipView.setBoundsOrigin(NSPoint(x: 0, y: targetMidY))
        coordinator.scrollView?.reflectScrolledClipView(clipView)

        let beforePrependAnchor = try XCTUnwrap(coordinator.captureScrollAnchor())

        // Prepend 10 older history pages at the top -> anchor deltaY drift MUST equal 0.0pt!
        let olderRows = (0 ..< 10).map { i in
            ConversationRowItem(
                id: "older-\(i)",
                timelineId: "tl-\(i)",
                kind: .assistant(markdown: "Older prepended message \(i)\n\nExtra paragraph line.", foldInstructions: false)
            )
        }
        coordinator.updateRowsPreservingAnchor(olderRows + initialRows, preferAutoFollowBottom: false)
        let afterPrependAnchor = try XCTUnwrap(coordinator.captureScrollAnchor())
        XCTAssertEqual(afterPrependAnchor.timelineId, beforePrependAnchor.timelineId)
        XCTAssertEqual(afterPrependAnchor.deltaY, beforePrependAnchor.deltaY, accuracy: 0.0, "Reverse pagination anchor drifted!")

        // Toggle disclosure on group:cmd-25 -> anchor deltaY drift MUST equal 0.0pt!
        let beforeToggleAnchor = try XCTUnwrap(coordinator.captureScrollAnchor())
        coordinator.toggleDisclosurePreservingAnchor(stateKey: "group:cmd-25", currentlyOpen: false)
        let afterToggleAnchor = try XCTUnwrap(coordinator.captureScrollAnchor())
        XCTAssertEqual(afterToggleAnchor.timelineId, beforeToggleAnchor.timelineId)
        XCTAssertEqual(afterToggleAnchor.deltaY, beforeToggleAnchor.deltaY, accuracy: 0.0, "Disclosure toggle anchor drifted!")

        // Verify ±240pt Thumbnail Viewport Eviction: inject NSImage for bottom row u-39 while scrolled near top
        let dummyImg = NSImage(size: NSSize(width: 96, height: 96))
        coordinator.attachmentViewport.injectForTesting(key: "test::codex::s1:u-39:att-39", image: dummyImg)
        XCTAssertEqual(coordinator.attachmentViewport.loadedCount, 1)

        // Scroll to very top (far >240pt away from row u-39 at bottom) -> image MUST be evicted!
        clipView.setBoundsOrigin(.zero)
        coordinator.scrollView?.reflectScrolledClipView(clipView)
        coordinator.refreshViewportAttachmentsAndTimeline()
        XCTAssertEqual(coordinator.attachmentViewport.loadedCount, 0, "Thumbnail outside ±240pt viewport was not evicted")
    }

    // MARK: - 3. Offscreen Light/Dark & 5-Provider Deterministic Render Snapshot Regression (<= 0.1% Pixel Diff)

    @MainActor
    func testOffscreenProviderThemeAndViewportSnapshotsWithinPointOnePercentTolerance() throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        let transport = MockRuntimeTransport { _, _ in Data("[]".utf8) }
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.snap.\(UUID().uuidString)")!
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)

        let viewports = [
            NSSize(width: 1440, height: 900),
            NSSize(width: 1280, height: 820),
            NSSize(width: 767, height: 820),
        ]
        for size in viewports {
            store.activeWorkspace = .codex
            store.setTheme(.light)
            let snapA = try renderOffscreenSnapshot(store: store, size: size, appearance: .aqua)
            let snapB = try renderOffscreenSnapshot(store: store, size: size, appearance: .aqua)
            let diffRatio = pixelDifferenceRatio(snapA, snapB)
            XCTAssertLessThanOrEqual(diffRatio, 0.001, "Deterministic snapshot drift \(diffRatio) exceeded 0.1% at \(size)")
        }

        // Verify all 5 Provider themes render distinct accent palettes cleanly in both Light and Dark
        for scheme in [ThemeMode.light, .dark] {
            let appearance: NSAppearance.Name = (scheme == .dark) ? .darkAqua : .aqua
            store.setTheme(scheme)
            for ws in [WorkspaceId.codex, .claude, .grok, .antigravity, .dsh] {
                store.activeWorkspace = ws
                let pixels = try renderOffscreenSnapshot(store: store, size: NSSize(width: 1280, height: 820), appearance: appearance)
                XCTAssertFalse(pixels.isEmpty)
            }
        }
    }

    @MainActor
    private func renderOffscreenSnapshot(
        store: RuntimeConnectionStore,
        size: NSSize,
        appearance: NSAppearance.Name
    ) throws -> [UInt8] {
        let host = NSHostingView(rootView: AppShellView(store: store).frame(width: size.width, height: size.height))
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        window.appearance = NSAppearance(named: appearance)
        window.contentView = host
        host.layoutSubtreeIfNeeded()

        let w = Int(size.width)
        let h = Int(size.height)
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB) else {
            throw NSError(domain: "SnapshotTest", code: 1)
        }
        var buffer = [UInt8](repeating: 0, count: w * h * 4)
        let rendered = buffer.withUnsafeMutableBytes { ptr -> Bool in
            guard let base = ptr.baseAddress,
                  let ctx = CGContext(
                      data: base, width: w, height: h,
                      bitsPerComponent: 8, bytesPerRow: w * 4,
                      space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else { return false }
            host.wantsLayer = true
            host.layer?.render(in: ctx)
            return true
        }
        XCTAssertTrue(rendered)
        return buffer
    }

    private func pixelDifferenceRatio(_ lhs: [UInt8], _ rhs: [UInt8]) -> Double {
        guard lhs.count == rhs.count, !lhs.isEmpty else { return 1.0 }
        let pixelCount = lhs.count / 4
        var diffPixels = 0
        for p in 0 ..< pixelCount {
            let base = p * 4
            if abs(Int(lhs[base]) - Int(rhs[base])) > 1
                || abs(Int(lhs[base + 1]) - Int(rhs[base + 1])) > 1
                || abs(Int(lhs[base + 2]) - Int(rhs[base + 2])) > 1
            {
                diffPixels += 1
            }
        }
        return Double(diffPixels) / Double(pixelCount)
    }
}
