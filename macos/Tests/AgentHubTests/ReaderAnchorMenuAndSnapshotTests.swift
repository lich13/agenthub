@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class ReaderAnchorMenuAndSnapshotTests: XCTestCase {
    @MainActor
    func testCollaborationMenuHasOneUnclippedChevronAtTrailingInset() throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else { return }
        for height in [24, 28, 30] {
            let width = 160
            let label = CollaborationMenuLabel(title: "A", height: CGFloat(height), fill: .white)
            let menu = AnchoredMenuButton(entries: [NativeMenuEntry(id: "a", title: "A", selected: true) {}]) { label }
                .foregroundStyle(.black).frame(width: CGFloat(width), height: CGFloat(height)).background(Color.white)
            let pixels = try renderOffscreenView(menu, size: NSSize(width: width, height: height), appearance: .aqua)
            let black = { (x: Int, y: Int) in
                let offset = 4 * (y * width + x)
                return pixels[offset ..< offset + 3].allSatisfy { $0 < 195 }
            }
            var xs: [Int] = [], ys: [Int] = []
            for x in 64 ..< width {
                for y in 0 ..< height where black(x, y) { xs.append(x); ys.append(y) }
            }
            let left = try XCTUnwrap(xs.min()), right = try XCTUnwrap(xs.max())
            let bottom = try XCTUnwrap(ys.min()), top = try XCTUnwrap(ys.max())
            XCTAssertEqual((left + right) / 2, width - 15, accuracy: 2)
            XCTAssertTrue((5 ... 11).contains(right - left), "trailing control must contain one full chevron")
            XCTAssertEqual((bottom + top) / 2, height / 2, accuracy: 2)
            XCTAssertTrue((2 ... 6).contains(top - bottom))
        }
    }

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
        XCTAssertEqual(codexHeader.map(\.label), ["改名", "取消归档", "复制答复", "复制 ID", "复制路径", "复制恢复命令", "在 Finder 中显示", "永久删除"])

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

        let antiBlocked = ProviderMenuMatrixBuilder.buildActions(
            provider: .antigravity, isHeaderMenu: false, isLocalMachine: true,
            id: "ag-locked", path: "/Users/example/.gemini/antigravity-cli/conversations/ag-locked.db", cwd: "/Users/example/repo",
            archived: false, canRename: false, renameBlockReason: "会话正在运行，请先在 Antigravity 中停止后再改名",
            canArchive: false, archiveBlockReason: "会话正在运行，请先在 Antigravity 中停止后再归档",
            canDelete: false, deleteBlockReason: "会话正在运行，请先在 Antigravity 中停止后再删除",
            busy: false, latestReplyText: nil, onRename: noop, onToggleArchive: noop,
            onCopy: copyNoop, onRevealInFinder: revealNoop, onDelete: noop
        )
        XCTAssertTrue(antiBlocked[0].disabled)
        XCTAssertEqual(antiBlocked[0].reason, "会话正在运行，请先在 Antigravity 中停止后再改名")
        XCTAssertTrue(antiBlocked[1].disabled)
        XCTAssertEqual(antiBlocked[1].reason, "会话正在运行，请先在 Antigravity 中停止后再归档")
        XCTAssertTrue(antiBlocked[5].disabled)
        XCTAssertEqual(antiBlocked[5].reason, "会话正在运行，请先在 Antigravity 中停止后再删除")

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

    @MainActor
    func testProviderSummaryMapsArchiveDeleteBlockReasonsAndReadDiagnostics() {
        let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.provider-map.\(UUID().uuidString)")!
        let runtime = RuntimeConnectionStore(transport: transport, defaults: defaults)
        let wsStore = ProviderWorkspaceStore(store: runtime, workspace: .antigravity)

        let summary = ProviderSessionSummary(
            id: "11111111-1111-4111-8111-111111111111",
            sessionKey: "11111111-1111-4111-8111-111111111111",
            title: "CLI 会话",
            cwd: "/Users/example/repo",
            path: "/Users/example/.gemini/antigravity-cli/conversations/11111111-1111-4111-8111-111111111111.db",
            archived: false,
            updatedAt: "2026-10-30T01:00:00Z",
            messageCount: 2,
            status: "idle",
            formatVersion: "sqlite-offline",
            canRename: false,
            renameBlockReason: "禁止改名原因",
            canArchive: false,
            archiveBlockReason: "禁止归档原因",
            canDelete: false,
            deleteBlockReason: "禁止删除原因",
            readError: "索引读取异常",
            readWarning: "部分步骤未加载"
        )
        let mapped = wsStore.mapProviderSummary(summary)
        XCTAssertEqual(mapped.renameBlockReason, "禁止改名原因")
        XCTAssertEqual(mapped.archiveBlockReason, "禁止归档原因")
        XCTAssertEqual(mapped.deleteBlockReason, "禁止删除原因")
        XCTAssertEqual(mapped.readError, "索引读取异常")
        XCTAssertEqual(mapped.readWarning, "部分步骤未加载")
        XCTAssertEqual(mapped.statusChipBadge, "异常")
        XCTAssertEqual(mapped.statusChipTone, .danger)
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
        )
        hostView.frame = NSRect(x: 0, y: 0, width: 960, height: 640)
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

        // Verify explicit anchor restoration (SubagentDrawerView back navigation -> 0.0pt deltaY drift!)
        coordinator.restoreExplicitAnchor(afterToggleAnchor)
        let restoredExplicit = try XCTUnwrap(coordinator.captureScrollAnchor())
        XCTAssertEqual(restoredExplicit.timelineId, afterToggleAnchor.timelineId)
        XCTAssertEqual(restoredExplicit.deltaY, afterToggleAnchor.deltaY, accuracy: 0.0)

        // Verify switching from scrolled-up Session A (near top, NOT near bottom) to Session B (`forceScrollToBottom: true`) scrolls Session B to bottom!
        clipView.setBoundsOrigin(.zero)
        coordinator.scrollView?.reflectScrolledClipView(clipView)
        XCTAssertFalse(coordinator.isNearBottom(), "Precondition: Session A is scrolled to top")
        let sessionBRows = (0 ..< 25).map { idx in
            ConversationRowItem(
                id: "s2-row-\(idx)",
                timelineId: "s2-tl-\(idx)",
                kind: .assistant(markdown: "Session B message \(idx)\n\nLine two\n\nLine three", foldInstructions: false)
            )
        }
        coordinator.updateRowsPreservingAnchor(sessionBRows, preferAutoFollowBottom: true, forceScrollToBottom: true)
        XCTAssertTrue(coordinator.isNearBottom(), "Switching from scrolled-up Session A to Session B must scroll Session B to bottom!")
    }

    // MARK: - 3. GFM Autolinks, Scheme Safety, Rich Table Cells & Per-Message Instruction Fold

    @MainActor
    func testGfmAutolinksJavascriptSchemeBlockRichTablesAndCleanedCopy() throws {
        let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
        let cache = GfmLayoutCache()

        let md = """
        # AGENTS.md instructions for /Users/example/repo

        <INSTRUCTIONS>
        line1
        line2
        </INSTRUCTIONS>

        Visit https://example.com/wiki/Rust_(lang) or www.example.com.
        Malicious [click](javascript:alert(1)) vs safe `code` [link](https://github.com/lich13/agenthub) and [file](/Users/example/src/lib.rs).
        <oai-mem-citation>secret</oai-mem-citation>

        | Key | Value |
        | :--- | ---: |
        | `cargo test` | [Docs](https://example.com/docs) |
        """
        let rendered = cache.getOrRender(
            markdown: md,
            foldInstructions: true,
            scheme: .light,
            workspace: .codex,
            transport: transport
        )

        // 1) First segment is instructionsFold carrying exact lines & UTF-8 byte count
        guard case let .instructionsFold(foldId, _, foldLines, foldBytes) = rendered.first else {
            return XCTFail("Expected first segment to be .instructionsFold")
        }
        XCTAssertEqual(foldId, "instructions:0")
        XCTAssertGreaterThanOrEqual(foldLines, 3)
        XCTAssertGreaterThan(foldBytes, 20)
        XCTAssertNotEqual("msg-a:\(foldId)", "msg-b:\(foldId)")

        // 2) Prose paragraph renders bare URLs & file paths as .link, strips memory_citation, and blocks javascript: scheme!
        guard let prosePiece = rendered.first(where: {
            if case .prose = $0 { return true }
            return false
        }), case let .prose(paraAttr, _) = prosePiece else {
            return XCTFail("Expected .prose segment")
        }
        XCTAssertFalse(paraAttr.string.contains("oai-mem-citation"))
        XCTAssertFalse(paraAttr.string.contains("secret"))
        var foundLinks: [String] = []
        paraAttr.enumerateAttribute(.link, in: NSRange(location: 0, length: paraAttr.length)) { val, _, _ in
            if let s = val as? String { foundLinks.append(s) }
            else if let u = val as? URL { foundLinks.append(u.absoluteString) }
        }
        XCTAssertTrue(foundLinks.contains("https://example.com/wiki/Rust_(lang)"))
        XCTAssertTrue(foundLinks.contains("http://www.example.com"))
        XCTAssertTrue(foundLinks.contains("https://github.com/lich13/agenthub"))
        XCTAssertTrue(foundLinks.contains("/Users/example/src/lib.rs"))
        XCTAssertFalse(foundLinks.contains { $0.lowercased().hasPrefix("javascript:") }, "javascript: scheme must never become clickable .link!")

        // 3) GFM Table preserves NSAttributedString inline code & links instead of stripping to plain text!
        guard let tablePiece = rendered.first(where: {
            if case .table = $0 { return true }
            return false
        }), case let .table(alignments, headers, rows) = tablePiece else {
            return XCTFail("Expected rendered .table segment")
        }
        XCTAssertEqual(headers.map(\.string), ["Key", "Value"])
        XCTAssertEqual(alignments, [.left, .right])
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0][0].string.trimmingCharacters(in: .whitespaces), "cargo test")
        var cellLinks: [String] = []
        rows[0][1].enumerateAttribute(.link, in: NSRange(location: 0, length: rows[0][1].length)) { val, _, _ in
            if let s = val as? String { cellLinks.append(s) }
        }
        XCTAssertEqual(cellLinks, ["https://example.com/docs"])
    }

    // MARK: - 4. Offscreen Light/Dark & 5-Provider Baseline Snapshot Verification (SnapshotBaselines.json)

    @MainActor
    func testOffscreenProviderThemeAndViewportSnapshotsMatchLockedBaselines() async throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        struct BaselineSpec: Decodable {
            struct Region: Decodable {
                let name: String
                let x: Int
                let y: Int
                let width: Int
                let height: Int
                let token: String
            }
            struct NavAccent: Decodable {
                let selected: [UInt8]
                let accent: [UInt8]
            }
            struct SessionRowColors: Decodable {
                let multiSelectedFill: [UInt8]
                let storageSuccess: [UInt8]
                let storageWarning: [UInt8]
                let storageDanger: [UInt8]
            }
            let maxRegionalPixelDriftRatio: Double
            let desktopRegions: [Region]
            let floatingComposerRegions: [Region]
            let sessionRowColors: [String: SessionRowColors]
            let providerExpectedSelectedNavColors: [String: [String: NavAccent]]
        }
        let baselineURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent("SnapshotBaselines.json")
        let baseline = try JSONDecoder().decode(BaselineSpec.self, from: Data(contentsOf: baselineURL))

        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.snap.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)
        await store.bootstrap()
        let pngOutDir = ProcessInfo.processInfo.environment["AGENTHUB_SNAPSHOT_PNG_DIR"]

        // 1) Verify 1440x900 and 1280x820 across Light and Dark against SnapshotBaselines.json regions
        for scheme in [ThemeMode.light, .dark] {
            let schemeKind: ColorSchemeKind = (scheme == .dark) ? .dark : .light
            let appearance: NSAppearance.Name = (scheme == .dark) ? .darkAqua : .aqua
            store.setTheme(scheme)
            store.activeWorkspace = .codex

            for size in [NSSize(width: 1440, height: 900), NSSize(width: 1280, height: 820)] {
                let pixels = try renderOffscreenSnapshot(store: store, size: size, appearance: appearance, pngName: pngOutDir.map { "\($0)/codex-\(scheme.rawValue)-\(Int(size.width))x\(Int(size.height)).png" })
                let palette = DesignPalette.resolve(scheme: schemeKind, workspace: .codex)
                for reg in baseline.desktopRegions {
                    let expectedToken: RGBAColor = switch reg.token {
                    case "railBg": palette.railBg
                    case "surface": palette.surface
                    default: palette.background
                    }
                    let matchRatio = regionBackgroundMatchRatio(
                        pixels: pixels,
                        imageWidth: Int(size.width),
                        region: (reg.x, reg.y, reg.width, reg.height),
                        expected: (expectedToken.r, expectedToken.g, expectedToken.b)
                    )
                    // Structural region (accounting for active tab pills/icons) matches the locked CSS surface token, AND total rendered frame has >500 non-background foreground ink pixels!
                    XCTAssertGreaterThanOrEqual(matchRatio, 0.78, "\(scheme) \(size) region \(reg.name) drifted from CSS \(reg.token): \(matchRatio)")
                }
                let inkPixels = countForegroundInkPixels(pixels, bg: (palette.background.r, palette.background.g, palette.background.b))
                XCTAssertGreaterThan(inkPixels, 500, "\(scheme) \(size) rendered blank without foreground text/icons!")
            }
        }

        // 2) Verify all 5 Provider themes render their exact distinct CSS accent bars & selected nav pill in Light & Dark!
        for (schemeKey, scheme, appearance) in [("light", ThemeMode.light, NSAppearance.Name.aqua), ("dark", ThemeMode.dark, .darkAqua)] {
            store.setTheme(scheme)
            let expectedMap = try XCTUnwrap(baseline.providerExpectedSelectedNavColors[schemeKey])
            var previousAccentSample: [UInt8]? = nil

            for ws in [WorkspaceId.codex, .claude, .grok, .antigravity, .dsh] {
                store.activeWorkspace = ws
                let expected = try XCTUnwrap(expectedMap[ws.rawValue])
                let pixels = try renderOffscreenSnapshot(
                    store: store,
                    size: NSSize(width: 1280, height: 820),
                    appearance: appearance,
                    pngName: pngOutDir.map { "\($0)/\(ws.rawValue)-\(schemeKey)-1280x820.png" }
                )
                // SideNav occupies x=0..196; active nav button renders selected pill + 2.5pt left accent bar!
                XCTAssertTrue(
                    regionContainsColor(pixels: pixels, imageWidth: 1280, region: (0, 200, 196, 600), target: (expected.selected[0], expected.selected[1], expected.selected[2]), tolerance: 2),
                    "\(schemeKey) \(ws.rawValue) missing selected nav pill color \(expected.selected)"
                )
                XCTAssertTrue(
                    regionContainsColor(pixels: pixels, imageWidth: 1280, region: (0, 200, 30, 600), target: (expected.accent[0], expected.accent[1], expected.accent[2]), tolerance: 3),
                    "\(schemeKey) \(ws.rawValue) missing 2.5pt active nav accent bar \(expected.accent)"
                )
                if let prev = previousAccentSample {
                    XCTAssertNotEqual(prev, expected.accent)
                }
                previousAccentSample = expected.accent

                // Re-render same workspace and verify deterministic <= 0.1% pixel stability
                if ws == .claude {
                    let repeatPixels = try renderOffscreenSnapshot(store: store, size: NSSize(width: 1280, height: 820), appearance: appearance, pngName: nil)
                    XCTAssertLessThanOrEqual(pixelDifferenceRatio(pixels, repeatPixels), baseline.maxRegionalPixelDriftRatio)
                }
            }
        }

        // 3) Offscreen Light & Dark Snapshots for Floating 768pt Composer, Side-Hover Copy Button & Session Row Badges (Item #16)
        for (schemeKey, scheme, schemeKind, appearance) in [
            ("light", ThemeMode.light, ColorSchemeKind.light, NSAppearance.Name.aqua),
            ("dark", ThemeMode.dark, ColorSchemeKind.dark, NSAppearance.Name.darkAqua),
        ] {
            store.setTheme(scheme)
            store.activeWorkspace = .codex
            let palette = DesignPalette.resolve(scheme: schemeKind, workspace: .codex)

            // 3a. Floating 768pt 24pt-radius composer over 1000x180 `--bg` stage (left & right margins remain pure `--bg`!)
            let composerStage = ZStack(alignment: .bottom) {
                palette.background.color
                DesktopCollaborationFooterView(
                    connection: store,
                    threadId: "th-snap",
                    availableWidth: 1000,
                    viewportHeight: 820
                )
            }
            .environment(\.agentHubPalette, palette)
            let composerPixels = try renderOffscreenView(
                composerStage,
                size: NSSize(width: 1000, height: 180),
                appearance: appearance,
                pngName: pngOutDir.map { "\($0)/composer-floating-\(schemeKey)-1000x180.png" }
            )
            for reg in baseline.floatingComposerRegions {
                let ratio = regionBackgroundMatchRatio(
                    pixels: composerPixels,
                    imageWidth: 1000,
                    region: (reg.x, reg.y, reg.width, reg.height),
                    expected: (palette.background.r, palette.background.g, palette.background.b)
                )
                XCTAssertGreaterThan(ratio, 0.95, "\(schemeKey) \(reg.name) must remain stage `--bg` outside floating 768pt card")
            }
            XCTAssertTrue(
                regionContainsColor(
                    pixels: composerPixels,
                    imageWidth: 1000,
                    region: (200, 20, 600, 120),
                    target: (palette.surface.r, palette.surface.g, palette.surface.b),
                    tolerance: 2
                ),
                "\(schemeKey) floating composer card must render `--s` surface in center 768pt column"
            )
            let composerRepeat = try renderOffscreenView(composerStage, size: NSSize(width: 1000, height: 180), appearance: appearance, pngName: nil)
            XCTAssertLessThanOrEqual(pixelDifferenceRatio(composerPixels, composerRepeat), baseline.maxRegionalPixelDriftRatio)

            // 3b. Assistant reply with hovered floating side-copy button in right margin (`x = 76 + 768 + 6 = 850`, 0.0pt height overhead)
            let coord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())
            coord.scheme = schemeKind
            coord.workspace = .codex
            let replyItem = ConversationRowItem(
                id: "snap-reply",
                timelineId: "snap-reply",
                kind: .assistant(markdown: "Verified floating side-hover copy button in right margin.", foldInstructions: false)
            )
            let rowHeight = coord.measureRowHeight(replyItem, width: 768)
            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("SnapReply"))
            cell.frame = NSRect(x: 0, y: 0, width: 920, height: max(48, rowHeight))
            cell.configure(item: replyItem, leadingInset: 76, trailingInset: 76, contentWidth: 768, highlighted: false, coordinator: coord)
            cell.setSimulatedHoverForTesting(true)
            let cellPixels = try renderOffscreenNSView(
                cell,
                bg: palette.background,
                size: NSSize(width: 920, height: max(48, ceil(rowHeight))),
                appearance: appearance,
                pngName: pngOutDir.map { "\($0)/assistant-side-copy-\(schemeKey).png" }
            )
            let cellHeight = Int(max(48, ceil(rowHeight)))
            let paper = (palette.background.r, palette.background.g, palette.background.b)
            XCTAssertEqual(countRegionInkPixels(pixels: cellPixels, imageWidth: 920, region: (0, 0, 66, cellHeight), bg: paper), 0)
            XCTAssertGreaterThan(countRegionInkPixels(pixels: cellPixels, imageWidth: 920, region: (76, 0, 80, cellHeight), bg: paper), 10)
            XCTAssertEqual(countRegionInkPixels(pixels: cellPixels, imageWidth: 920, region: (901, 0, 19, cellHeight), bg: paper), 0)
            let rightMarginInk = countRegionInkPixels(
                pixels: cellPixels,
                imageWidth: 920,
                region: (844, 0, 44, Int(max(48, ceil(rowHeight)))),
                bg: (palette.background.r, palette.background.g, palette.background.b)
            )
            XCTAssertGreaterThan(rightMarginInk, 8, "\(schemeKey) hovered side-copy icon button must render visible glyph pixels in right margin")

            // 3c. Session rows: `.multi-selected` (`--as` fill) & row-2 plain tabular storage badges (`<100M`, `100..300M`, `>=300M`)
            let rowExpected = try XCTUnwrap(baseline.sessionRowColors[schemeKey])
            let mib: Int64 = 1024 * 1024
            let rowsStack = VStack(spacing: 4) {
                RenameableSessionRowView(
                    sessionKey: "r-ok", title: "Small session", subtitle: "", statusText: "近期", running: true,
                    storageSize: SessionStorageSize(bytes: 24 * mib, scope: "file", status: "complete"),
                    selected: false, selecting: true, multiSelected: true, disabled: false,
                    canRename: false, renameBlockReason: nil, rowMenuActions: [], batchMenuActions: [],
                    onRowClick: { _ in }, onToggleCheckbox: {}, onRename: { _ in }
                )
                RenameableSessionRowView(
                    sessionKey: "r-warn", title: "Medium session", subtitle: "", statusText: "近期", running: false,
                    storageSize: SessionStorageSize(bytes: 150 * mib, scope: "file", status: "complete"),
                    selected: true, selecting: false, multiSelected: false, disabled: false,
                    canRename: false, renameBlockReason: nil, rowMenuActions: [], batchMenuActions: [],
                    onRowClick: { _ in }, onToggleCheckbox: {}, onRename: { _ in }
                )
                RenameableSessionRowView(
                    sessionKey: "r-err", title: "Large session", subtitle: "", statusText: "近期", running: false,
                    storageSize: SessionStorageSize(bytes: 340 * mib, scope: "file", status: "complete"),
                    selected: false, selecting: false, multiSelected: false, disabled: false,
                    canRename: false, renameBlockReason: nil, rowMenuActions: [], batchMenuActions: [],
                    onRowClick: { _ in }, onToggleCheckbox: {}, onRename: { _ in }
                )
            }
            .padding(8)
            .background(palette.surface.color)
            .environment(\.agentHubPalette, palette)

            let rowsPixels = try renderOffscreenView(
                rowsStack,
                size: NSSize(width: 296, height: 210),
                appearance: appearance,
                pngName: pngOutDir.map { "\($0)/session-rows-badges-\(schemeKey)-296x210.png" }
            )
            XCTAssertTrue(
                regionContainsColor(
                    pixels: rowsPixels,
                    imageWidth: 296,
                    region: (4, 120, 288, 86),
                    target: (rowExpected.multiSelectedFill[0], rowExpected.multiSelectedFill[1], rowExpected.multiSelectedFill[2]),
                    tolerance: 2
                ),
                "\(schemeKey) multi-selected row missing `--as` accentSoft fill"
            )
            XCTAssertTrue(
                regionContainsColor(
                    pixels: rowsPixels,
                    imageWidth: 296,
                    region: (180, 0, 112, 210),
                    target: (rowExpected.storageWarning[0], rowExpected.storageWarning[1], rowExpected.storageWarning[2]),
                    tolerance: 18
                ),
                "\(schemeKey) row-2 storage badge missing `--w` warning text ink"
            )
            XCTAssertTrue(
                regionContainsColor(
                    pixels: rowsPixels,
                    imageWidth: 296,
                    region: (180, 0, 112, 210),
                    target: (rowExpected.storageDanger[0], rowExpected.storageDanger[1], rowExpected.storageDanger[2]),
                    tolerance: 18
                ),
                "\(schemeKey) row-2 storage badge missing `--er` danger text ink"
            )
        }
    }

    @MainActor
    private func renderOffscreenView<V: View>(
        _ rootView: V,
        size: NSSize,
        appearance: NSAppearance.Name,
        pngName: String? = nil
    ) throws -> [UInt8] {
        try OffscreenRenderHelper.renderSwiftUIView(
            rootView,
            size: size,
            appearance: appearance,
            bottomOriginRows: true,
            pngPath: pngName
        )
    }

    @MainActor
    private func renderOffscreenNSView(
        _ view: NSView,
        bg: RGBAColor,
        size: NSSize,
        appearance: NSAppearance.Name,
        pngName: String? = nil
    ) throws -> [UInt8] {
        let wrapper = NSView(frame: NSRect(origin: .zero, size: size))
        wrapper.wantsLayer = true
        wrapper.layer?.backgroundColor = bg.nsColor.cgColor
        view.frame = NSRect(origin: .zero, size: size)
        wrapper.addSubview(view)
        let window = UnconstrainedOffscreenWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        window.appearance = NSAppearance(named: appearance)
        window.contentView = wrapper
        wrapper.layoutSubtreeIfNeeded()
        view.layoutSubtreeIfNeeded()
        window.displayIfNeeded()

        guard let rep = wrapper.bitmapImageRepForCachingDisplay(in: wrapper.bounds) else {
            return try rasterizeNSView(wrapper, size: size, pngName: pngName)
        }
        wrapper.cacheDisplay(in: wrapper.bounds, to: rep)
        guard let cgImg = rep.cgImage,
              let cs = CGColorSpace(name: CGColorSpace.sRGB)
        else {
            return try rasterizeNSView(wrapper, size: size, pngName: pngName)
        }
        let w = Int(size.width)
        let h = Int(size.height)
        var buffer = [UInt8](repeating: 0, count: w * h * 4)
        let ok = buffer.withUnsafeMutableBytes { ptr -> Bool in
            guard let base = ptr.baseAddress,
                  let ctx = CGContext(
                      data: base, width: w, height: h,
                      bitsPerComponent: 8, bytesPerRow: w * 4,
                      space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else { return false }
            ctx.setFillColor(bg.nsColor.cgColor)
            ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
            ctx.draw(cgImg, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        XCTAssertTrue(ok)
        if let pngPath = pngName {
            let outURL = URL(fileURLWithPath: pngPath)
            try? FileManager.default.createDirectory(at: outURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            if let pngData = rep.representation(using: .png, properties: [:]) {
                try? pngData.write(to: outURL)
            }
        }
        return buffer
    }

    @MainActor
    private func rasterizeNSView(_ targetView: NSView, size: NSSize, pngName: String?) throws -> [UInt8] {
        try OffscreenRenderHelper.renderNSView(
            targetView,
            size: size,
            appearance: .aqua,
            bottomOriginRows: true,
            pngPath: pngName
        )
    }

    private func countRegionInkPixels(
        pixels: [UInt8],
        imageWidth: Int,
        region: (x: Int, y: Int, w: Int, h: Int),
        bg: (UInt8, UInt8, UInt8)
    ) -> Int {
        var ink = 0
        for py in region.y ..< (region.y + region.h) {
            for px in region.x ..< (region.x + region.w) {
                let idx = (py * imageWidth + px) * 4
                if abs(Int(pixels[idx]) - Int(bg.0)) > 18
                    || abs(Int(pixels[idx + 1]) - Int(bg.1)) > 18
                    || abs(Int(pixels[idx + 2]) - Int(bg.2)) > 18
                {
                    ink += 1
                }
            }
        }
        return ink
    }

    @MainActor
    private func renderOffscreenSnapshot(
        store: RuntimeConnectionStore,
        size: NSSize,
        appearance: NSAppearance.Name,
        pngName: String? = nil
    ) throws -> [UInt8] {
        try OffscreenRenderHelper.renderSwiftUIView(
            AppShellView(store: store),
            size: size,
            appearance: appearance,
            bottomOriginRows: true,
            pngPath: pngName
        )
    }

    private func regionBackgroundMatchRatio(
        pixels: [UInt8],
        imageWidth: Int,
        region: (x: Int, y: Int, w: Int, h: Int),
        expected: (UInt8, UInt8, UInt8)
    ) -> Double {
        var matched = 0
        let total = max(1, region.w * region.h)
        for py in region.y ..< (region.y + region.h) {
            for px in region.x ..< (region.x + region.w) {
                let idx = (py * imageWidth + px) * 4
                if abs(Int(pixels[idx]) - Int(expected.0)) <= 2
                    && abs(Int(pixels[idx + 1]) - Int(expected.1)) <= 2
                    && abs(Int(pixels[idx + 2]) - Int(expected.2)) <= 2
                {
                    matched += 1
                }
            }
        }
        return Double(matched) / Double(total)
    }

    private func regionContainsColor(
        pixels: [UInt8],
        imageWidth: Int,
        region: (x: Int, y: Int, w: Int, h: Int),
        target: (UInt8, UInt8, UInt8),
        tolerance: Int
    ) -> Bool {
        for py in region.y ..< (region.y + region.h) {
            for px in region.x ..< (region.x + region.w) {
                let idx = (py * imageWidth + px) * 4
                if abs(Int(pixels[idx]) - Int(target.0)) <= tolerance
                    && abs(Int(pixels[idx + 1]) - Int(target.1)) <= tolerance
                    && abs(Int(pixels[idx + 2]) - Int(target.2)) <= tolerance
                {
                    return true
                }
            }
        }
        return false
    }

    private func countForegroundInkPixels(_ pixels: [UInt8], bg: (UInt8, UInt8, UInt8)) -> Int {
        let count = pixels.count / 4
        var ink = 0
        for p in 0 ..< count {
            let idx = p * 4
            if abs(Int(pixels[idx]) - Int(bg.0)) > 24
                || abs(Int(pixels[idx + 1]) - Int(bg.1)) > 24
                || abs(Int(pixels[idx + 2]) - Int(bg.2)) > 24
            {
                ink += 1
            }
        }
        return ink
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

    // MARK: - 5. Populated Thread Detail Glyph-Ink Gate (Prevents TextKit 2->1 Blank-Glyph Regression)

    @MainActor
    func testPopulatedThreadDetailRendersVisibleGlyphInkAcrossAllTextViews() throws {
        let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
        let suiteName = "com.lich13.agenthub.tests.ink.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        let disclosures = ActivityDisclosureStore()
        let scope = "local:codex:ink-thread"
        disclosures.setOpen(scope: scope, stateKey: "u-1:context", isOpen: true)
        disclosures.setOpen(scope: scope, stateKey: "exec-grp-1", isOpen: true)
        disclosures.setOpen(scope: scope, stateKey: "exec-cmd-1", isOpen: true)

        let coordinator = ConversationTableCoordinator(store: store, disclosureStore: disclosures)
        coordinator.sessionScopeKey = scope
        let populatedRows: [ConversationRowItem] = [
            ConversationRowItem(
                id: "u-1",
                timelineId: "u-1",
                kind: .user(
                    message: UserMessageContent(
                        id: "u-1",
                        text: "检查纯文本用户气泡渲染",
                        attachments: [],
                        contextBlocks: [UserContextBlock(kind: "environment_context", text: "<cwd>/workspace/AgentHub</cwd>")]
                    ),
                    rawText: "检查纯文本用户气泡渲染",
                    attachmentsAsNamesOnly: false
                )
            ),
            ConversationRowItem(
                id: "exec-grp-1",
                timelineId: "exec-grp-1",
                kind: .executionGroup(ExecutionGroup(
                    id: "exec-grp-1",
                    mergedIds: ["exec-grp-1"],
                    commands: [
                        ExecutionCommand(
                            id: "exec-cmd-1",
                            sourceIds: ["exec-cmd-1"],
                            title: "read_file",
                            preview: "DESIGN.md",
                            status: .completed,
                            instructionFile: false,
                            sections: [ExecutionSection(label: "Output", text: "Read 180 lines")]
                        ),
                    ],
                    kind: "command",
                    provider: "codex",
                    running: false,
                    recordedOnly: false,
                    failedCount: 0
                ))
            ),
            ConversationRowItem(
                id: "asst-1",
                timelineId: "asst-1",
                kind: .assistant(
                    markdown: "纯文本段落与 `inline_code` 混排。\n\n| 模块 | 状态 |\n| :--- | :--- |\n| 纯文本单元格 | `通过` |",
                    foldInstructions: false
                )
            ),
        ]

        let readerView = ConversationReaderTableView(
            coordinator: coordinator,
            rows: populatedRows,
            scheme: .light,
            workspace: .codex,
            sessionScopeKey: scope,
            sessionProvider: .codex,
            sessionKey: "codex:ink-thread"
        )
        let host = NSHostingView(rootView: readerView.frame(width: 900, height: 900))
        host.frame = NSRect(x: 0, y: 0, width: 900, height: 900)
        let window = NSWindow(
            contentRect: host.frame,
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        window.appearance = NSAppearance(named: .aqua)
        window.contentView = host
        window.orderBack(nil)
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()

        func collectTextViews(in view: NSView) -> [ReaderSelectableTextView] {
            var out: [ReaderSelectableTextView] = []
            if let tv = view as? ReaderSelectableTextView, (tv.textStorage?.length ?? 0) > 0 {
                out.append(tv)
            }
            for sub in view.subviews {
                out.append(contentsOf: collectTextViews(in: sub))
            }
            return out
        }

        let textViews = collectTextViews(in: host)
        XCTAssertGreaterThanOrEqual(textViews.count, 7, "Expected User bubble, Context, Command output, Prose, and 4 Table cells")
        for tv in textViews {
            tv.layoutSubtreeIfNeeded()
            guard let rep = tv.bitmapImageRepForCachingDisplay(in: tv.bounds) else {
                XCTFail("Failed to allocate bitmap rep for \(tv.string)")
                continue
            }
            tv.cacheDisplay(in: tv.bounds, to: rep)
            let w = rep.pixelsWide
            let h = rep.pixelsHigh
            guard let src = rep.bitmapData else {
                XCTFail("Missing bitmapData for \(tv.string)")
                continue
            }
            var nonTransparentInk = 0
            for y in 0 ..< h {
                let rowPtr = src.advanced(by: y * rep.bytesPerRow)
                for x in 0 ..< w {
                    let alpha = rowPtr[x * 4 + 3]
                    if alpha > 24 {
                        nonTransparentInk += 1
                    }
                }
            }
            XCTAssertGreaterThan(
                nonTransparentInk,
                20,
                "ReaderSelectableTextView rendered blank glyphs for text: \(tv.string)"
            )
        }
    }

    // MARK: - 6. TaskMenu Popover & Upward Scroll Anchor Alias / Load-Older Gating

    @MainActor
    func testTaskMenuPopoverAndUpwardScrollAnchorAliasAndLoadGating() throws {
        let palette = DesignPalette.resolve(scheme: .light, workspace: .codex)
        let hostWin = NSWindow(
            contentRect: NSRect(x: 100, y: 100, width: 900, height: 700),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        hostWin.orderBack(nil)

        let menuActions = ProviderMenuMatrixBuilder.buildActions(
            provider: .codex,
            isHeaderMenu: false,
            isLocalMachine: false,
            id: "th-ctx-1",
            path: "/remote/.codex/sessions/1.jsonl",
            cwd: "/remote/repo",
            archived: true,
            canRename: false,
            canArchive: true,
            canDelete: true,
            busy: false,
            latestReplyText: nil,
            onRename: {},
            onToggleArchive: {},
            onCopy: { _, _ in },
            onRevealInFinder: { _ in },
            onDelete: {}
        )
        XCTAssertEqual(menuActions.first(where: { $0.id == "rename" })?.reason, "已归档线程不可改名")
        XCTAssertEqual(menuActions.first(where: { $0.id == "reveal" })?.reason, "仅可在本机 Finder 中显示本机会话")
        XCTAssertEqual(menuActions.first(where: { $0.id == "copy-resume" })?.icon, .copy)

        AgentHubTaskMenuPresenter.shared.presentAtScreenTopLeft(
            NSPoint(x: 240, y: 500),
            in: hostWin,
            actions: menuActions,
            palette: palette
        )
        XCTAssertTrue(AgentHubTaskMenuPresenter.shared.isPresented)
        AgentHubTaskMenuPresenter.shared.dismiss()
        XCTAssertFalse(AgentHubTaskMenuPresenter.shared.isPresented)

        // Upward scroll: verify alias-merged anchor restoration + canLoadOlder gating
        let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.up.\(UUID().uuidString)")!
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        let coordinator = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())

        var loadOlderTriggerCount = 0
        coordinator.onLoadOlderRequested = { loadOlderTriggerCount += 1 }
        coordinator.canLoadOlder = false

        let initialRows: [ConversationRowItem] = (10 ..< 30).map { i in
            ConversationRowItem(
                id: "r-\(i)",
                timelineId: "tl-\(i)",
                kind: .assistant(markdown: "Paragraph \(i)\n\nDetail line \(i)", foldInstructions: false)
            )
        }
        let reader = ConversationReaderTableView(
            coordinator: coordinator,
            rows: initialRows,
            scheme: .light,
            workspace: .codex,
            sessionScopeKey: "local:codex:up-1",
            sessionProvider: .codex,
            sessionKey: "up-1",
            canLoadOlder: false,
            isLoadingOlder: false
        )
        let host = NSHostingView(rootView: reader.frame(width: 860, height: 500))
        host.frame = NSRect(x: 0, y: 0, width: 860, height: 500)
        hostWin.contentView = host
        host.layoutSubtreeIfNeeded()

        let clip = try XCTUnwrap(coordinator.scrollView?.contentView)
        clip.setBoundsOrigin(NSPoint(x: 0, y: 300))
        coordinator.scrollView?.reflectScrolledClipView(clip)
        clip.setBoundsOrigin(NSPoint(x: 0, y: 120))
        coordinator.scrollView?.reflectScrolledClipView(clip)
        XCTAssertEqual(loadOlderTriggerCount, 0, "canLoadOlder == false must NEVER spawn load-older tasks on upward scroll")

        // Prepend 10 older rows AND merge the anchor row's primary timelineId into timelineAliases:
        let anchorBefore = try XCTUnwrap(coordinator.captureScrollAnchor())
        var prepended: [ConversationRowItem] = (0 ..< 10).map { i in
            ConversationRowItem(
                id: "older-\(i)",
                timelineId: "older-tl-\(i)",
                kind: .assistant(markdown: "Older \(i)\n\nMore text \(i)", foldInstructions: false)
            )
        }
        for r in initialRows {
            if r.timelineId == anchorBefore.timelineId {
                prepended.append(ConversationRowItem(
                    id: "merged-\(r.id)",
                    timelineId: "merged-group-id",
                    timelineAliases: [r.timelineId],
                    kind: r.kind
                ))
            } else {
                prepended.append(r)
            }
        }
        coordinator.updateRowsPreservingAnchor(prepended, preferAutoFollowBottom: false)
        let anchorAfter = try XCTUnwrap(coordinator.captureScrollAnchor())
        XCTAssertEqual(anchorAfter.timelineId, "merged-group-id")
        XCTAssertEqual(anchorAfter.deltaY, anchorBefore.deltaY, accuracy: 0.0, "Merged alias row must preserve 0.0pt scroll offset on prepend!")
    }
}
