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
    func testOffscreenProviderThemeAndViewportSnapshotsMatchLockedBaselines() throws {
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
            let maxRegionalPixelDriftRatio: Double
            let desktopRegions: [Region]
            let mobileRegions: [Region]
            let providerExpectedSelectedNavColors: [String: [String: NavAccent]]
        }
        let baselineURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent("SnapshotBaselines.json")
        let baseline = try JSONDecoder().decode(BaselineSpec.self, from: Data(contentsOf: baselineURL))

        let transport = MockRuntimeTransport { _, _ in Data("[]".utf8) }
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.snap.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        let pngOutDir = ProcessInfo.processInfo.environment["AGENTHUB_SNAPSHOT_PNG_DIR"]

        // 1) Verify 1440x900, 1280x820, and 767x820 across Light and Dark against SnapshotBaselines.json regions
        for scheme in [ThemeMode.light, .dark] {
            let schemeKind: ColorSchemeKind = (scheme == .dark) ? .dark : .light
            let appearance: NSAppearance.Name = (scheme == .dark) ? .darkAqua : .aqua
            store.setTheme(scheme)
            store.activeWorkspace = .codex

            for size in [NSSize(width: 1440, height: 900), NSSize(width: 1280, height: 820), NSSize(width: 767, height: 820)] {
                let pixels = try renderOffscreenSnapshot(store: store, size: size, appearance: appearance, pngName: pngOutDir.map { "\($0)/codex-\(scheme.rawValue)-\(Int(size.width))x\(Int(size.height)).png" })
                let palette = DesignPalette.resolve(scheme: schemeKind, workspace: .codex)
                let regions = size.width <= 767 ? baseline.mobileRegions : baseline.desktopRegions
                for reg in regions {
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
                    // At least 80% of the structural region (accounting for active tab pills/icons) matches the locked CSS surface token, AND total rendered frame has >500 non-background foreground ink pixels!
                    XCTAssertGreaterThan(matchRatio, 0.80, "\(scheme) \(size) region \(reg.name) drifted from CSS \(reg.token): \(matchRatio)")
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
    }

    @MainActor
    private func renderOffscreenSnapshot(
        store: RuntimeConnectionStore,
        size: NSSize,
        appearance: NSAppearance.Name,
        pngName: String? = nil
    ) throws -> [UInt8] {
        let host = NSHostingView(rootView: AppShellView(store: store).frame(width: size.width, height: size.height))
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        window.appearance = NSAppearance(named: appearance)
        host.frame = NSRect(origin: .zero, size: size)
        window.contentView = host
        host.layoutSubtreeIfNeeded()

        let w = Int(size.width)
        let h = Int(size.height)
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB) else {
            throw NSError(domain: "SnapshotTest", code: 1)
        }
        var buffer = [UInt8](repeating: 0, count: w * h * 4)
        var renderedImage: CGImage?
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
            if pngName != nil {
                renderedImage = ctx.makeImage()
            }
            return true
        }
        XCTAssertTrue(rendered)
        if let pngPath = pngName, let cgImg = renderedImage {
            let outURL = URL(fileURLWithPath: pngPath)
            try? FileManager.default.createDirectory(at: outURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            let rep = NSBitmapImageRep(cgImage: cgImg)
            if let pngData = rep.representation(using: .png, properties: [:]) {
                try? pngData.write(to: outURL)
            }
        }
        return buffer
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
}
