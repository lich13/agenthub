import AppKit
import SwiftUI
import XCTest
@testable import AgentHubCore

final class ReaderGeometryScrollAndCleanupTests: XCTestCase {
    private static func countForegroundInkPixels(
        pixels: [UInt8],
        imageWidth: Int,
        region: (x: Int, y: Int, w: Int, h: Int),
        bg: RGBAColor,
        minColorDelta: Int = 24
    ) -> Int {
        OffscreenRenderHelper.countNonBackgroundPixels(
            pixels,
            imageWidth: imageWidth,
            region: region,
            bg: (bg.r, bg.g, bg.b),
            threshold: minColorDelta - 1
        )
    }

    private static func countColorMatchPixels(
        pixels: [UInt8],
        imageWidth: Int,
        region: (x: Int, y: Int, w: Int, h: Int),
        target: RGBAColor,
        tolerance: Int = 5
    ) -> Int {
        var count = 0
        let totalRows = pixels.count / max(1, imageWidth * 4)
        for py in max(0, region.y) ..< min(totalRows, region.y + region.h) {
            for px in max(0, region.x) ..< min(imageWidth, region.x + region.w) {
                let idx = (py * imageWidth + px) * 4
                let dr = abs(Int(pixels[idx]) - Int(target.r))
                let dg = abs(Int(pixels[idx + 1]) - Int(target.g))
                let db = abs(Int(pixels[idx + 2]) - Int(target.b))
                if max(dr, max(dg, db)) <= tolerance {
                    count += 1
                }
            }
        }
        return count
    }

    private static func countLineFragments(attributed: NSAttributedString, width: CGFloat) -> Int {
        guard attributed.length > 0, width > 1 else { return 0 }
        let storage = NSTextStorage(attributedString: attributed)
        let container = NSTextContainer(size: NSSize(width: width, height: .greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        let manager = NSLayoutManager()
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        let glyphRange = manager.glyphRange(forCharacterRange: NSRange(location: 0, length: storage.length), actualCharacterRange: nil)
        var lines = 0
        manager.enumerateLineFragments(forGlyphRange: glyphRange) { _, _, _, _, _ in
            lines += 1
        }
        return lines
    }

    @MainActor
    func testLegacyHorizontalScrollerWideTablePreservesTopHeaderRowInLightAndDark() throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        ConversationTableCoordinator.scrollerStyleOverrideForTesting = .legacy
        defer { ConversationTableCoordinator.scrollerStyleOverrideForTesting = nil }

        let wideMarkdown = """
        | 项目 | 提交 | 状态 | 入口类型 | 命令语法 | 配置文件 | 默认分支 | 关键职责与长说明 |
        | --- | --- | --- | --- | --- | --- | --- | --- |
        | agenthub-cli | a1b2c3d4 | 正常 | 原生二进制 | `agenthub-webd rpc-stdio` | `config.toml` | `main` | 负责远端 SSH 管理与探针轮询 |
        | orca-worker | e5f60718 | 正常 | 终端命令 | `orca worktree list --json` | `orca.json` | `main` | 负责多工作树编排与后台会话管理 |
        """
        let row = ConversationRowItem(
            id: "codex-wide-table",
            timelineId: "codex-wide-table",
            kind: .assistant(markdown: wideMarkdown, foldInstructions: false)
        )

        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.legacytable.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)

        for (scheme, appearance) in [(ColorSchemeKind.light, NSAppearance.Name.aqua), (.dark, .darkAqua)] {
            let coord = ConversationTableCoordinator(store: store)
            coord.scheme = scheme
            coord.workspace = .codex
            coord.sessionScopeKey = "local:codex:legacy-table-\(scheme.rawValue)"

            let contentWidth: CGFloat = 680
            let measuredH = ceil(coord.measureRowHeight(row, width: contentWidth))
            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("WideTableCell"))
            let cellW: CGFloat = 760
            cell.frame = NSRect(x: 0, y: 0, width: cellW, height: measuredH)
            cell.configure(
                item: row,
                leadingInset: 40,
                trailingInset: 40,
                contentWidth: contentWidth,
                highlighted: false,
                coordinator: coord
            )
            cell.layoutSubtreeIfNeeded()

            // Locate inner horizontal table NSScrollView and verify FlippedDocumentView + scroller height compensation
            func findInnerTableScroll(_ v: NSView) -> NSScrollView? {
                if let sv = v as? NSScrollView, sv.hasHorizontalScroller { return sv }
                for sub in v.subviews {
                    if let found = findInnerTableScroll(sub) { return found }
                }
                return nil
            }
            let tableScroll = try XCTUnwrap(findInnerTableScroll(cell), "Wide 8-column table must enable horizontal scroller")
            let docView = try XCTUnwrap(tableScroll.documentView)
            XCTAssertTrue(docView.isFlipped, "Table documentView must be flipped so Row 0 anchors at y=0 top edge")
            let extra = ConversationTableCoordinator.horizontalScrollerExtraHeight(needsHorizontalScroll: true)
            XCTAssertGreaterThanOrEqual(extra, 15, "Legacy horizontal scroller must reserve >=15pt extra height")
            XCTAssertGreaterThanOrEqual(
                tableScroll.bounds.height,
                docView.bounds.height + extra - 0.5,
                "ScrollView height must include legacy scroller height so Row 0 and bottom row are never clipped"
            )

            // Render tableScroll offscreen (top-down rows: y=0 is top edge of Row 0 header `项目 | 提交`)
            let palette = DesignPalette.resolve(scheme: scheme, workspace: .codex)
            let renderSize = NSSize(width: floor(tableScroll.bounds.width), height: floor(tableScroll.bounds.height))
            let pixels = try OffscreenRenderHelper.renderNSView(
                tableScroll,
                size: renderSize,
                appearance: appearance,
                bottomOriginRows: false
            )
            // Row 0 header has height >= 35pt and surface background `palette.surface`.
            // Both the upper half of Row 0 glyphs (y: 6..17) AND lower half (y: 18..29) must contain non-blank glyph ink!
            let upperHalfInk = Self.countForegroundInkPixels(
                pixels: pixels,
                imageWidth: Int(renderSize.width),
                region: (x: 10, y: 6, w: Int(renderSize.width) - 20, h: 11),
                bg: palette.surface
            )
            let lowerHalfInk = Self.countForegroundInkPixels(
                pixels: pixels,
                imageWidth: Int(renderSize.width),
                region: (x: 10, y: 18, w: Int(renderSize.width) - 20, h: 11),
                bg: palette.surface
            )
            XCTAssertGreaterThan(upperHalfInk, 30, "\(scheme) wide table Row 0 upper half was clipped/blank!")
            XCTAssertGreaterThan(lowerHalfInk, 30, "\(scheme) wide table Row 0 lower half was clipped/blank!")
        }
    }

    @MainActor
    func testTableMaxWidthFittingMultiSegmentHeightsAndSelectedPageSnapshotCleanup() throws {
        ConversationTableCoordinator.scrollerStyleOverrideForTesting = .legacy
        defer { ConversationTableCoordinator.scrollerStyleOverrideForTesting = nil }

        // 1) 4-column table with moderately long prose shrinks proportionally to fit maxWidth=768 without horizontal overflow
        let fitHeaders = ["模块", "状态", "耗时", "验证说明"].map {
            NSAttributedString(string: $0, attributes: [.font: DesignFonts.nsBodyFont(size: 14, weight: .semibold)])
        }
        let fitRows = [[
            "claude/reader.rs",
            "已完成",
            "0.40ms",
            "保留物理 JSON 行号游标同时裁剪 progress 与文件历史快照内存权重，消除缓存颠簸。",
        ].map {
            NSAttributedString(string: $0, attributes: [.font: DesignFonts.nsBodyFont(size: 14)])
        }]
        let fitLayout = ConversationTableCoordinator.measureTableLayout(headers: fitHeaders, rows: fitRows, maxWidth: 768)
        XCTAssertLessThanOrEqual(fitLayout.totalWidth, 768.0 + 0.001, "4-column prose table must fit within maxWidth=768 without horizontal scrollbar")
        XCTAssertEqual(
            Self.countLineFragments(attributed: fitRows[0][0], width: fitLayout.colWidths[0] - 24),
            1,
            "claude/reader.rs token floor must keep identifier unbroken on 1 line"
        )

        // 1b) 8-column technical table at width=700 and width=768 protects unbreakable ASCII/code tokens and enables horizontal scroll
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.multiseg.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)
        let eightColMd = """
        | 项目 | 提交 | 状态 | 入口类型 | 命令语法 | 配置文件 | 默认分支 | 关键职责与说明 |
        | --- | --- | --- | --- | --- | --- | --- | --- |
        | agenthub-webd | 9c4a083 | 正常 | 守护进程 | `agenthub-webd rpc-stdio` | `config.toml` | `main` | 零端口 SSH 远端会话与通知探针服务 |
        | agenthub-app | 3e0812d | 正常 | 原生桌面 | `AgentHub.app` | `Preferences` | `main` | Swift 6 + AppKit 六平台统一阅读工作台 |
        """
        let eightSegs = GfmLayoutCache.shared.getOrRender(
            markdown: eightColMd,
            foldInstructions: false,
            scheme: .dark,
            workspace: .codex,
            transport: store.transport
        )
        guard case let .table(_, eightHeaders, eightRows) = try XCTUnwrap(eightSegs.first) else {
            return XCTFail("Expected rendered GFM table segment")
        }
        for maxW in [CGFloat(700), CGFloat(768)] {
            let layout8 = ConversationTableCoordinator.measureTableLayout(headers: eightHeaders, rows: eightRows, maxWidth: maxW)
            XCTAssertGreaterThan(layout8.totalWidth, maxW + 1, "8-column technical table must scroll horizontally at maxWidth=\(maxW) instead of crushing tokens")
            // Verify single-token cells (`agenthub-webd`, `agenthub-app`, `AgentHub.app`, `config.toml`, `Preferences`) stay on 1 unbroken line
            XCTAssertEqual(Self.countLineFragments(attributed: eightRows[0][0], width: layout8.colWidths[0] - 24), 1, "agenthub-webd must not wrap mid-token")
            XCTAssertEqual(Self.countLineFragments(attributed: eightRows[1][0], width: layout8.colWidths[0] - 24), 1, "agenthub-app must not wrap mid-token")
            XCTAssertEqual(Self.countLineFragments(attributed: eightRows[1][4], width: layout8.colWidths[4] - 24), 1, "AgentHub.app must not wrap mid-token")
            XCTAssertEqual(Self.countLineFragments(attributed: eightRows[0][5], width: layout8.colWidths[5] - 24), 1, "config.toml must not wrap mid-token")
            XCTAssertEqual(Self.countLineFragments(attributed: eightRows[1][5], width: layout8.colWidths[5] - 24), 1, "Preferences must not wrap mid-token")
        }

        // 1c) 5-column mixed code + CJK prose table fits maxWidth=768 without horizontal overflow while still protecting single-token columns
        let fiveColMd = """
        | 组件 | 启动命令 | 配置项 | 分支 | 详细职责说明 |
        | --- | --- | --- | --- | --- |
        | agenthub-webd | `agenthub-webd rpc-stdio` | `Preferences` | `main` | 负责远端 SSH 会话阅读、加密通知投递与状态同步，收缩时仅折行中文长句而不压碎英文词元。 |
        """
        let fiveSegs = GfmLayoutCache.shared.getOrRender(
            markdown: fiveColMd,
            foldInstructions: false,
            scheme: .light,
            workspace: .codex,
            transport: store.transport
        )
        guard case let .table(_, fiveHeaders, fiveRows) = try XCTUnwrap(fiveSegs.first) else {
            return XCTFail("Expected 5-col table segment")
        }
        let layout5 = ConversationTableCoordinator.measureTableLayout(headers: fiveHeaders, rows: fiveRows, maxWidth: 768)
        XCTAssertLessThanOrEqual(layout5.totalWidth, 768.0 + 0.001, "5-column mixed prose table must fit maxWidth=768")
        XCTAssertEqual(Self.countLineFragments(attributed: fiveRows[0][0], width: layout5.colWidths[0] - 24), 1, "agenthub-webd stays 1 line")
        XCTAssertEqual(Self.countLineFragments(attributed: fiveRows[0][2], width: layout5.colWidths[2] - 24), 1, "Preferences stays 1 line")
        XCTAssertLessThanOrEqual(Self.countLineFragments(attributed: fiveRows[0][1], width: layout5.colWidths[1] - 24), 2, "2-word command wraps at most at space")

        // 1d) Kinsoku CJK punctuation (`（Preferences）`, `` `config.toml`， ``, `` 「`agenthub-webd`」 ``, `见`Preferences`说明`),
        //     Emoji ZWJ sequences, variation selectors, and decomposed Latin accents never crush tokens mid-word
        let kinsokuMd = """
        | 组件 | 括号配置 | 句读配置 | 引用入口 | 详细说明 |
        | --- | --- | --- | --- | --- |
        | caf\u{0065}\u{0301}_mod.rs | （`Preferences`） | `config.toml`， | 「`agenthub-webd`」 | 状态 👨‍👩‍👧‍👦 正常\u{FE0F}，收缩时见`Preferences`说明且绝不半词折断。 |
        """
        let kinsokuSegs = GfmLayoutCache.shared.getOrRender(
            markdown: kinsokuMd,
            foldInstructions: false,
            scheme: .dark,
            workspace: .codex,
            transport: store.transport
        )
        guard case let .table(_, kinsokuHeaders, kinsokuRows) = try XCTUnwrap(kinsokuSegs.first) else {
            return XCTFail("Expected Kinsoku 5-col table segment")
        }
        let layoutKinsoku = ConversationTableCoordinator.measureTableLayout(headers: kinsokuHeaders, rows: kinsokuRows, maxWidth: 768)
        XCTAssertLessThanOrEqual(layoutKinsoku.totalWidth, 768.0 + 0.001, "5-column Kinsoku + prose table must still fit maxWidth=768")
        XCTAssertEqual(Self.countLineFragments(attributed: kinsokuRows[0][0], width: layoutKinsoku.colWidths[0] - 24), 1, "Decomposed accented identifier stays 1 line")
        XCTAssertEqual(Self.countLineFragments(attributed: kinsokuRows[0][1], width: layoutKinsoku.colWidths[1] - 24), 1, "（`Preferences`） must not wrap mid-word")
        XCTAssertEqual(Self.countLineFragments(attributed: kinsokuRows[0][2], width: layoutKinsoku.colWidths[2] - 24), 1, "`config.toml`， must not wrap mid-word")
        XCTAssertEqual(Self.countLineFragments(attributed: kinsokuRows[0][3], width: layoutKinsoku.colWidths[3] - 24), 1, "「`agenthub-webd`」 must not wrap mid-word")

        // 1e) Ragged/header-short rows and narrow drawer width (360pt) preserve all columns without out-of-bounds
        let raggedHeaders = [NSAttributedString(string: "列一", attributes: [.font: DesignFonts.nsBodyFont(size: 13, weight: .semibold)])]
        let raggedRows = [[
            NSAttributedString(string: "值A", attributes: [.font: DesignFonts.nsBodyFont(size: 13)]),
            NSAttributedString(string: "extra_column.rs", attributes: [.font: DesignFonts.nsBodyFont(size: 13)]),
            NSAttributedString(string: "窄视口说明文本正常折行。", attributes: [.font: DesignFonts.nsBodyFont(size: 13)]),
        ]]
        let raggedLayout = ConversationTableCoordinator.measureTableLayout(headers: raggedHeaders, rows: raggedRows, maxWidth: 360)
        XCTAssertEqual(raggedLayout.colWidths.count, 3, "Ragged row with more cells than header must measure all columns")
        XCTAssertLessThanOrEqual(raggedLayout.totalWidth, 360.0 + 0.001, "3-column narrow table fits maxWidth=360")
        XCTAssertEqual(Self.countLineFragments(attributed: raggedRows[0][1], width: raggedLayout.colWidths[1] - 24), 1, "extra_column.rs stays unbroken at narrow width")

        // 1f) Oversized unbreakable token (>180 ASCII chars) caps token floor at 168pt so 3-column prose tables still fit maxWidth=768
        let hugeToken = String(repeating: "a", count: 200)
        let capHeaders = ["键", "值", "备注"].map {
            NSAttributedString(string: $0, attributes: [.font: DesignFonts.nsBodyFont(size: 13, weight: .semibold)])
        }
        let capRows = [[
            "digest",
            hugeToken,
            "超长单词元列下限封顶 168pt，保持三列表格不强制横向滚动。",
        ].map {
            NSAttributedString(string: $0, attributes: [.font: DesignFonts.nsBodyFont(size: 13)])
        }]
        let capLayout = ConversationTableCoordinator.measureTableLayout(headers: capHeaders, rows: capRows, maxWidth: 768)
        XCTAssertLessThanOrEqual(capLayout.totalWidth, 768.0 + 0.001, "3-column table with oversized token must cap floor at 168pt and fit maxWidth=768")

        // 2) Multi-code-block .assistant & .plan rows never clip stacked segments
        let coord = ConversationTableCoordinator(store: store)

        let multiBlockMd = """
        First block:

        ```bash
        cargo test --workspace --locked -- --nocapture --exact very_long_flag_line_that_exceeds_column_width_for_legacy_scroller_compensation_check_1234567890
        ```

        Second block:

        ```swift
        let ok = true
        print(ok)
        ```

        Final paragraph.
        """
        for item in [
            ConversationRowItem(id: "m-asst", timelineId: "m-asst", kind: .assistant(markdown: multiBlockMd, foldInstructions: false)),
            ConversationRowItem(id: "m-plan", timelineId: "m-plan", kind: .plan(markdown: multiBlockMd, statusText: "approved", fallbackTitle: "Plan")),
        ] {
            let measured = ceil(coord.measureRowHeight(item, width: 520))
            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("MultiSeg"))
            cell.frame = NSRect(x: 0, y: 0, width: 600, height: measured)
            cell.configure(item: item, leadingInset: 40, trailingInset: 40, contentWidth: 520, highlighted: false, coordinator: coord)
            cell.layoutSubtreeIfNeeded()
            let stackFittingH = cell.subviews.first?.fittingSize.height ?? 0
            XCTAssertGreaterThanOrEqual(measured, stackFittingH, "Measured height (\(measured)) must never clip multi-segment cell stack (\(stackFittingH)) for \(item.id)")
        }

        // 3) <external_codex_apps_selected_page_part_1_of_1> stripped from user bubbles & visibleMarkdown, preserved in code
        let snap = """
        <external_codex_apps_selected_page_part_1_of_1>Companion Page snapshot part 1 of 1:
        {"page_id":null,"document_view":null}
        </external_codex_apps_selected_page_part_1_of_1>
        """
        XCTAssertEqual(VisibleMarkdownEngine.visibleMarkdown(snap).trimmingCharacters(in: .whitespacesAndNewlines), "")
        XCTAssertEqual(VisibleMarkdownEngine.visibleMarkdown("\(snap)\n盘点命令型工具"), "盘点命令型工具")
        let fenced = "```xml\n\(snap)\n```"
        XCTAssertEqual(VisibleMarkdownEngine.visibleMarkdown(fenced), fenced)

        let snapOnlyBlock = MessageBlock(
            id: "u-snap-only",
            role: "user",
            kind: "message",
            text: snap,
            userMessage: UserMessageContent(id: "u-snap-only", text: snap, attachments: [], contextBlocks: nil)
        )
        XCTAssertFalse(ConversationViewModel.shouldRenderConversationMessage(snapOnlyBlock))
        XCTAssertTrue(
            ConversationRowBuilder.buildCodexRows(
                blocks: [snapOnlyBlock],
                showAllHistory: true,
                planFallbackTitle: "Fallback",
                subagentsSupported: true
            ).isEmpty
        )
    }

    @MainActor
    func testUnderfilledHugeSessionUpwardScrollRubberBandNativeCursorsAndExpandedRow0Anchor() async throws {
        // 1) Underfilled 1-row (34pt < 700pt viewport) ReaderOuterScrollView triggers onLoadOlderRequested
        //    on upward wheel/key and preserves upwardScrollIntent across top rubber-band snapback!
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.upwardscroll.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)
        let coord = ConversationTableCoordinator(store: store)
        coord.canLoadOlder = true
        coord.isLoadingOlder = false
        var loadOlderCount = 0
        coord.onLoadOlderRequested = { loadOlderCount += 1 }

        let outerScroll = ReaderOuterScrollView(frame: NSRect(x: 0, y: 0, width: 800, height: 700))
        XCTAssertEqual(outerScroll.verticalScrollElasticity, .allowed)
        outerScroll.coordinator = coord
        let table = NSTableView(frame: NSRect(x: 0, y: 0, width: 800, height: 700))
        let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("ConversationCol"))
        col.width = 800
        table.addTableColumn(col)
        table.headerView = nil
        table.delegate = coord
        table.dataSource = coord
        outerScroll.documentView = table
        outerScroll.contentView.postsBoundsChangedNotifications = true
        coord.tableView = table
        coord.scrollView = outerScroll
        NotificationCenter.default.addObserver(
            coord,
            selector: #selector(ConversationTableCoordinator.clipViewBoundsDidChange(_:)),
            name: NSView.boundsDidChangeNotification,
            object: outerScroll.contentView
        )
        defer { NotificationCenter.default.removeObserver(coord) }

        let collapsedRow = ConversationRowItem(
            id: "claude-proc-tail",
            timelineId: "claude-proc-tail",
            timelineAliases: ["raw-tail-1"],
            kind: .thinking(title: "思考中", text: "short", running: true)
        )
        coord.updateRowsPreservingAnchor([collapsedRow], forceScrollToBottom: false)
        XCTAssertLessThan(coord.renderedContentHeight, 200, "Single collapsed row rendered content height is far shorter than 700pt viewport")
        XCTAssertTrue(coord.isViewportUnderfilled)

        // User scrolls upward on trackpad/wheel while contentHeight < viewportHeight (originY stays 0)
        coord.handleOuterScrollWheel(deltaY: 16.0, phase: .changed, momentumPhase: [])
        XCTAssertEqual(loadOlderCount, 1, "Upward wheel on underfilled viewport must immediately request older history")

        // Simulate rubber-band pull past top (y = -18) snapping back to y = 0 (deltaY < 0 near top)
        outerScroll.contentView.setBoundsOrigin(NSPoint(x: 0, y: -18))
        NotificationCenter.default.post(name: NSView.boundsDidChangeNotification, object: outerScroll.contentView)
        // Notice: rubber-band pull + snapback triggers clipViewBoundsDidChange while upwardScrollIntent remains true!
        let countAfterPull = loadOlderCount
        outerScroll.contentView.setBoundsOrigin(NSPoint(x: 0, y: 0))
        NotificationCenter.default.post(name: NSView.boundsDidChangeNotification, object: outerScroll.contentView)
        XCTAssertGreaterThanOrEqual(loadOlderCount, countAfterPull + 1, "Top rubber-band snapback must NOT clear upwardScrollIntent")

        // Subsequent upward key navigation must also request older history on underfilled viewport
        let countBeforeKey = loadOlderCount
        coord.handleOuterUpwardKeyNavigation()
        XCTAssertEqual(loadOlderCount, countBeforeKey + 1, "Upward key navigation must also request older history on underfilled viewport")

        // 2) Expanded Row 0 bottom-edge (maxY) prepend anchor lock:
        //    Suppose Row 0 is expanded to 500pt and scrolled to y=180 (top above viewport, bottom visible).
        //    Prepending a 120pt older row BEFORE Row 0 must keep old Row 0's bottom edge locked!
        let tallMarkdown = (0 ..< 28).map { "Expanded paragraph line \($0) with prose text." }.joined(separator: "\n\n")
        let expandedRow0 = ConversationRowItem(
            id: "row-expanded-0",
            timelineId: "row-expanded-0",
            kind: .assistant(markdown: tallMarkdown, foldInstructions: false)
        )
        let trailingRow = ConversationRowItem(
            id: "row-trailing-1",
            timelineId: "row-trailing-1",
            kind: .assistant(markdown: tallMarkdown, foldInstructions: false)
        )
        coord.updateRowsPreservingAnchor([expandedRow0, trailingRow], forceScrollToBottom: false)

        let oldRect0 = table.rect(ofRow: 0)
        XCTAssertGreaterThan(oldRect0.height, 400)
        outerScroll.contentView.scroll(to: NSPoint(x: 0, y: 180))
        outerScroll.reflectScrolledClipView(outerScroll.contentView)
        let beforeBottomDelta = oldRect0.maxY - outerScroll.contentView.bounds.origin.y

        let prependedOlder = ConversationRowItem(
            id: "row-prepended-older",
            timelineId: "row-prepended-older",
            kind: .assistant(markdown: "Older prepended reply\n\nWith two paragraphs.", foldInstructions: false)
        )
        coord.updateRowsPreservingAnchor([prependedOlder, expandedRow0, trailingRow], forceScrollToBottom: false)
        let newRectOldRow = table.rect(ofRow: 1) // expandedRow0 is now at index 1
        let afterBottomDelta = newRectOldRow.maxY - outerScroll.contentView.bounds.origin.y
        XCTAssertEqual(afterBottomDelta, beforeBottomDelta, accuracy: 1.0, "Expanded Row 0 visible bottom edge must remain locked at 0pt drift across prepend")

        // 3) TimelineWindowController sends raw native IDs (firstNativeId / lastNativeId) on w: and n: cursors,
        //    NEVER synthetic folded group IDs (`claude-proc-tail`)!
        let seenBox = NSLock()
        nonisolated(unsafe) var seenCursors: [String] = []
        let recordCursor: @Sendable (String) -> Void = { c in
            seenBox.lock()
            seenCursors.append(c)
            seenBox.unlock()
        }
        let hist0 = HistoryMetadata(epoch: "ep-1", revision: "ep-1:100", reset: false)
        let model = TimelineWindowController<ConversationRowItem>(idOf: \.timelineId)
        await model.jump(
            to: "raw-tail-1",
            in: "local::claude:s1",
            currentHistory: hist0,
            matchesTarget: { item, target in item.timelineAliases.contains(target) }
        ) { cursor in
            recordCursor(cursor)
            return TimelineWindowPage(
                items: [collapsedRow],
                before: "raw-older-cursor",
                hasMore: true,
                history: hist0,
                firstNativeId: "raw-native-first",
                lastNativeId: "raw-native-last",
                nativeIds: ["raw-tail-1", "raw-native-first", "raw-native-last"]
            )
        }
        XCTAssertTrue(model.hasNewer(latestTailId: "raw-native-newest"), "Jump window whose lastNativeId != latestTailId must report hasNewer == true")
        let hist1 = HistoryMetadata(epoch: "ep-1", revision: "ep-1:120", reset: false)
        await model.loadNewer(latestTailId: "raw-native-newest") { cursor in
            recordCursor(cursor)
            return TimelineWindowPage(
                items: [collapsedRow],
                before: nil,
                hasMore: false,
                history: hist1,
                firstNativeId: "raw-native-last",
                lastNativeId: "raw-native-newest",
                nativeIds: ["raw-native-last", "raw-native-newest"]
            )
        }
        let hist2 = HistoryMetadata(epoch: "ep-1", revision: "ep-1:140", reset: false)
        await model.reconcilePollHistory(incoming: hist2) { cursor in
            recordCursor(cursor)
            return TimelineWindowPage(
                items: [collapsedRow],
                before: nil,
                hasMore: false,
                history: hist2,
                firstNativeId: "raw-native-first",
                lastNativeId: "raw-native-newest",
                nativeIds: ["raw-native-first", "raw-native-newest"]
            )
        }
        XCTAssertTrue(seenCursors.contains("n:raw-native-last"), "loadNewer must pass raw lastNativeId, saw: \(seenCursors)")
        XCTAssertTrue(seenCursors.contains("w:raw-native-first"), "reconcilePollHistory must pass raw firstNativeId, saw: \(seenCursors)")
        XCTAssertFalse(seenCursors.contains(where: { $0.contains("claude-proc-tail") }), "Must NEVER send synthetic folded group ID to backend seek_window")
    }

    @MainActor
    func testImeMarkedTextCompositionGuardPreventsAccidentalModalEscapeDismiss() {
        let win = UnconstrainedOffscreenWindow(
            contentRect: NSRect(x: 0, y: 0, width: 300, height: 200),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        defer { win.orderOut(nil) }
        let inputView = NSTextView(frame: NSRect(x: 0, y: 0, width: 200, height: 40))
        inputView.isEditable = true
        win.contentView = inputView
        XCTAssertTrue(win.makeFirstResponder(inputView))

        XCTAssertFalse(AppInputCompositionGuard.responderHasMarkedText(win.firstResponder))
        inputView.setMarkedText("中文", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: 0, length: 0))
        XCTAssertTrue(AppInputCompositionGuard.responderHasMarkedText(win.firstResponder))
        inputView.unmarkText()
        XCTAssertFalse(AppInputCompositionGuard.responderHasMarkedText(win.firstResponder))
    }

    @MainActor
    func testProviderWorkspaceStoreBackfillsFoldedClaudeTailPagesUntilConversationalHistory() async throws {
        struct DetailArgs: Decodable {
            let sessionKey: String
            let limit: Int?
            let before: String?
        }
        let reqLock = NSLock()
        nonisolated(unsafe) var recordedLimits: [Int] = []
        nonisolated(unsafe) var recordedBefores: [String] = []

        let transport = MockRuntimeTransport { cmd, argsJSON in
            guard cmd == "claude.detail" else { return Data("{}".utf8) }
            let parsed = try? JSONDecoder().decode(DetailArgs.self, from: Data(argsJSON.utf8))
            reqLock.withLock {
                recordedLimits.append(parsed?.limit ?? -1)
                recordedBefores.append(parsed?.before ?? "HEAD")
            }

            let before = parsed?.before
            let events: [[String: Any]]
            let nextBefore: Any
            let hasMore: Bool
            if before == nil {
                // Tail page: 25 running/completed tool events in turn-99 -> folds to 1 single 34pt row!
                events = (0 ..< 25).map { idx in
                    [
                        "id": "tail-tool-\(idx)",
                        "kind": "tool_call",
                        "role": "Bash",
                        "callId": "c-tail-\(idx)",
                        "turnId": "turn-99",
                        "status": idx == 24 ? "running" : "completed",
                        "text": "Bash",
                        "detail": "{\"command\":\"echo \(idx)\"}",
                    ]
                }
                nextBefore = "cursor-page-2"
                hasMore = true
            } else if before == "cursor-page-2" {
                // Older page 2: still more tool events from turn-99 -> STILL 1 single 34pt row!
                events = (0 ..< 20).map { idx in
                    [
                        "id": "mid-tool-\(idx)",
                        "kind": "tool_call",
                        "role": "Read",
                        "callId": "c-mid-\(idx)",
                        "turnId": "turn-99",
                        "status": "completed",
                        "text": "Read",
                        "detail": "{\"file_path\":\"src/file_\(idx).rs\"}",
                    ]
                }
                nextBefore = "cursor-page-3"
                hasMore = true
            } else {
                // Older page 3: contains the user prompt & prior assistant reply!
                events = [
                    [
                        "id": "u-98",
                        "kind": "user_message",
                        "turnId": "turn-98",
                        "text": "请修复 SSH 实例切换问题",
                        "userMessage": [
                            "id": "u-98",
                            "text": "请修复 SSH 实例切换问题",
                            "attachments": [],
                        ],
                    ],
                    [
                        "id": "a-98",
                        "kind": "assistant_message",
                        "turnId": "turn-98",
                        "assistantPhase": "final",
                        "text": (0 ..< 24).map { "已完成第 \($0) 步验证并确认无回归。" }.joined(separator: "\n\n"),
                    ],
                ]
                nextBefore = NSNull()
                hasMore = false
            }
            let resp: [String: Any] = [
                "summary": [
                    "id": "claude-huge-1",
                    "sessionKey": "proj/claude-huge-1",
                    "title": "vela-instance-switch-and-ssh-fixes",
                    "cwd": "/workspace/example.invalid",
                    "path": "/workspace/example.invalid/huge.jsonl",
                    "messageCount": 120,
                    "status": "running",
                    "formatVersion": "2.1.76",
                    "canRename": true,
                    "canDelete": true,
                ],
                "events": events,
                "totalEvents": 5000,
                "beforeCursor": nextBefore,
                "hasMore": hasMore,
                "history": [
                    "epoch": "ep-huge-1",
                    "revision": "ep-huge-1:900000",
                    "reset": false,
                ],
            ]
            return try JSONSerialization.data(withJSONObject: resp)
        }

        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.hugebackfill.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        let ws = ProviderWorkspaceStore(store: store, workspace: .claude)

        // Attach a 700pt tall ReaderOuterScrollView + Coordinator just like UnifiedProviderWorkspaceView
        let coord = ConversationTableCoordinator(store: store)
        let outerScroll = ReaderOuterScrollView(frame: NSRect(x: 0, y: 0, width: 820, height: 700))
        let table = NSTableView(frame: NSRect(x: 0, y: 0, width: 820, height: 700))
        let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("ConversationCol"))
        col.width = 820
        table.addTableColumn(col)
        table.headerView = nil
        table.delegate = coord
        table.dataSource = coord
        outerScroll.documentView = table
        coord.tableView = table
        coord.scrollView = outerScroll
        ws.coordinatorRef = coord

        let seed = UnifiedSessionSummaryItem(
            sessionKey: "proj/claude-huge-1",
            nativeId: "claude-huge-1",
            title: "vela-instance-switch-and-ssh-fixes",
            cwd: "/workspace/example.invalid",
            path: "/workspace/example.invalid/huge.jsonl",
            statusText: "运行中",
            running: true,
            archived: false,
            canRename: true,
            canArchive: false,
            canDelete: true
        )
        ws.activeItems = [seed]
        ws.selectedKey = seed.sessionKey

        await ws.fetchSessionDetail(for: seed)
        // Initial fetch automatically backfills across folded 1-row tool pages until user/assistant rows appear!
        XCTAssertGreaterThanOrEqual(ws.rows.count, 3, "Folded 34pt tail must backfill until user/assistant conversation rows are visible")
        XCTAssertTrue(recordedLimits.contains(500), "Folded tail continuation must escalate page limit to 500, saw: \(recordedLimits)")
        XCTAssertEqual(recordedBefores, ["HEAD", "cursor-page-2", "cursor-page-3"])
    }

    @MainActor
    func testDynamicScrollerStyleToggleMergedProcessRow0LockAndRapidSessionSwitchGenerationGuard() async throws {
        defer { ConversationTableCoordinator.scrollerStyleOverrideForTesting = nil }

        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.dynamicguard.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)
        let disclosureStore = ActivityDisclosureStore()
        let coord = ConversationTableCoordinator(store: store, disclosureStore: disclosureStore)
        coord.workspace = .codex

        let outerScroll = ReaderOuterScrollView(frame: NSRect(x: 0, y: 0, width: 760, height: 520))
        let table = NSTableView(frame: NSRect(x: 0, y: 0, width: 760, height: 520))
        let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("MainCol"))
        col.width = 760
        table.addTableColumn(col)
        table.headerView = nil
        table.delegate = coord
        table.dataSource = coord
        outerScroll.documentView = table
        coord.tableView = table
        coord.scrollView = outerScroll

        // 1) Dynamic scroller style toggle (.overlay -> .legacy) while wide Codex table is mounted in NSTableView
        ConversationTableCoordinator.scrollerStyleOverrideForTesting = .overlay
        let wideMd = """
        | 仓库完整标识符 | 最新提交哈希 | 组件分类 | 守护进程运行入口 | 核心配置文件名 | 默认发布分支 | 平台兼容架构矩阵 | 关键变更说明 | 验证门禁状态 | 归属负责人 |
        | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
        | agenthub-core | 1a2b3c4d5e6f | 后端核心 | `agenthub-webd rpc-stdio` | `config.toml` | `main` | `aarch64-darwin / x86_64-linux` | 动态切换滚动条样式不裁切表头 | 全部通过 | core-team |
        """
        let tableRow = ConversationRowItem(
            id: "dyn-table",
            timelineId: "dyn-table",
            kind: .assistant(markdown: wideMd, foldInstructions: false)
        )
        coord.updateRowsPreservingAnchor([tableRow], forceScrollToBottom: false)
        let overlayRowH = table.rect(ofRow: 0).height

        ConversationTableCoordinator.scrollerStyleOverrideForTesting = .legacy
        NotificationCenter.default.post(name: NSScroller.preferredScrollerStyleDidChangeNotification, object: nil)
        let legacyRowH = table.rect(ofRow: 0).height
        let expectedExtra = ConversationTableCoordinator.horizontalScrollerExtraHeight(needsHorizontalScroll: true)
        XCTAssertEqual(legacyRowH - overlayRowH, expectedExtra, accuracy: 1.0, "Toggling scroller style to .legacy must expand mounted wide table row height by legacy scroller thickness")

        // 2) Expanded ClaudeProcessGroup at Row 0 absorbing older prepended thinking/tool events from the same turn:
        //    firstSid shifts AND Row 0 height grows, while visible bottom edge (maxY - originY) stays locked!
        coord.sessionScopeKey = "local:claude:merged-row0"
        let tailTools = (10 ..< 22).map { idx in
            ClaudeHistoryEvent(
                id: "ev-\(idx)",
                kind: idx % 2 == 0 ? "thinking" : "tool_call",
                role: idx % 2 == 0 ? nil : "Bash",
                text: idx % 2 == 0 ? "推理步骤 \(idx)：核对会话滚动锚点与高度变化。" : "Bash",
                turnId: "turn-1",
                callId: idx % 2 == 0 ? nil : "call-\(idx)",
                status: "completed",
                detail: idx % 2 == 0 ? nil : "{\"command\":\"echo step \(idx)\"}"
            )
        }
        let trailingReply = ClaudeHistoryEvent(
            id: "asst-final",
            kind: "assistant_message",
            text: (0 ..< 20).map { "段落 \($0) 内容保持可见锚点稳定。" }.joined(separator: "\n\n"),
            turnId: "turn-2",
            assistantPhase: "final"
        )
        let initialRows = ConversationRowBuilder.buildClaudeRows(events: tailTools + [trailingReply])
        guard case let .claudeProcessGroup(proc0) = initialRows[0].kind else {
            return XCTFail("Expected ClaudeProcessGroup at Row 0")
        }
        let aliases0 = proc0.sourceIds.map { "claude-process:\(proc0.turnId):\($0)" }
        disclosureStore.setOpen(scope: coord.sessionScopeKey, stateKey: proc0.stateKey, aliases: aliases0, isOpen: true)
        coord.updateRowsPreservingAnchor(initialRows, forceScrollToBottom: false)
        let initialRow0Rect = table.rect(ofRow: 0)
        XCTAssertGreaterThan(initialRow0Rect.height, 300)
        outerScroll.contentView.setBoundsOrigin(NSPoint(x: 0, y: 150))
        outerScroll.reflectScrolledClipView(outerScroll.contentView)
        let savedBottomEdgeOffset = initialRow0Rect.maxY - outerScroll.contentView.bounds.origin.y

        let olderToolsSameTurn = (0 ..< 10).map { idx in
            ClaudeHistoryEvent(
                id: "ev-\(idx)",
                kind: idx % 2 == 0 ? "thinking" : "tool_call",
                role: idx % 2 == 0 ? nil : "Read",
                text: idx % 2 == 0 ? "较早推理步骤 \(idx)：追踪前置翻页合并。" : "Read",
                turnId: "turn-1",
                callId: idx % 2 == 0 ? nil : "call-\(idx)",
                status: "completed",
                detail: idx % 2 == 0 ? nil : "{\"file_path\":\"src/mod_\(idx).rs\"}"
            )
        }
        let mergedRows = ConversationRowBuilder.buildClaudeRows(events: olderToolsSameTurn + tailTools + [trailingReply])
        XCTAssertNotEqual(mergedRows[0].id, initialRows[0].id, "Prepending older events in same turn shifts Row 0 firstSid")
        coord.updateRowsPreservingAnchor(mergedRows, preferAutoFollowBottom: false)
        let grownRow0Rect = table.rect(ofRow: 0)
        XCTAssertGreaterThan(grownRow0Rect.height, initialRow0Rect.height + 200, "Expanded Row 0 must remain open and grow when older steps merge in")
        let restoredBottomEdgeOffset = grownRow0Rect.maxY - outerScroll.contentView.bounds.origin.y
        XCTAssertEqual(restoredBottomEdgeOffset, savedBottomEdgeOffset, accuracy: 1.0, "Merged expanded Row 0 bottom edge must stay locked")

        // 3) TimelineWindowController epoch rotation & clearError retry + A -> B -> A selection generation guard
        let winCtrl = TimelineWindowController<ConversationRowItem>(idOf: \.timelineId)
        await winCtrl.jump(
            to: "dyn-table",
            in: "local:codex:s1",
            currentHistory: HistoryMetadata(epoch: "ep-old", revision: "ep-old:10", reset: false)
        ) { _ in
            TimelineWindowPage(
                items: [tableRow],
                before: "b-1",
                hasMore: true,
                history: HistoryMetadata(epoch: "ep-old", revision: "ep-old:10", reset: false),
                firstNativeId: "dyn-table",
                lastNativeId: "dyn-table",
                nativeIds: ["dyn-table"]
            )
        }
        XCTAssertTrue(winCtrl.isActive)
        await winCtrl.reconcilePollHistory(
            incoming: HistoryMetadata(epoch: "ep-rotated", revision: "ep-rotated:4", reset: false)
        ) { _ in
            XCTFail("Must not issue w: refresh against a rotated epoch")
            return TimelineWindowPage(items: [])
        }
        XCTAssertFalse(winCtrl.isActive)
        XCTAssertEqual(winCtrl.error, "历史已更新，请重新定位")
        winCtrl.clearError()
        XCTAssertNil(winCtrl.error)

        let ws = ProviderWorkspaceStore(store: store, workspace: .claude)
        let itemA = UnifiedSessionSummaryItem(
            sessionKey: "proj/a", nativeId: "a", title: "A", cwd: "", path: "", statusText: "",
            running: false, archived: false, canRename: true, canArchive: false, canDelete: true
        )
        let itemB = UnifiedSessionSummaryItem(
            sessionKey: "proj/b", nativeId: "b", title: "B", cwd: "", path: "", statusText: "",
            running: false, archived: false, canRename: true, canArchive: false, canDelete: true
        )
        ws.activeItems = [itemA, itemB]
        ws.selectedKey = itemA.sessionKey
        let firstVisitGen = ws.detailSelectionGeneration
        XCTAssertTrue(ws.isCurrentDetail(itemA, generation: firstVisitGen))
        ws.selectedKey = itemB.sessionKey
        ws.resetDetailStateForSelectionChange()
        ws.selectedKey = itemA.sessionKey
        ws.resetDetailStateForSelectionChange()
        XCTAssertFalse(ws.isCurrentDetail(itemA, generation: firstVisitGen), "A -> B -> A must invalidate stale first-visit generation")
        XCTAssertTrue(ws.isCurrentDetail(itemA, generation: ws.detailSelectionGeneration))
    }

    @MainActor
    func testWrappedInlineCodePillsAlignPerLineFragmentInLightAndDark() throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.wrappedpill.\(UUID().uuidString)")!
        defer { defaults.removePersistentDomain(forName: defaults.description) }
        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)
        let pngOutDir = ProcessInfo.processInfo.environment["AGENTHUB_SNAPSHOT_PNG_DIR"]

        // Two paragraphs in a narrow 148pt column:
        // - Paragraph 1 has `paragraphSpacingAfter = 12` and wraps multi-token inline code across >=3 lines
        // - Paragraph 2 wraps a single long unbreakable inline-code identifier across >=3 lines
        let wrappedMd = """
        远端命令 `agenthub-webd rpc-stdio --socket /run/agenthub/webd.sock` 已就绪。

        偏好项 `PreferencesWindowControllerConfiguration` 已校验。
        """

        for (scheme, appearance) in [(ColorSchemeKind.light, NSAppearance.Name.aqua), (.dark, .darkAqua)] {
            let palette = DesignPalette.resolve(scheme: scheme, workspace: .codex)
            let segs = GfmLayoutCache.shared.getOrRender(
                markdown: wrappedMd,
                foldInstructions: false,
                scheme: scheme,
                workspace: .codex,
                transport: store.transport
            )
            guard case let .prose(attributed) = try XCTUnwrap(segs.first) else {
                return XCTFail("Expected rendered prose segment")
            }

            let textWidth: CGFloat = 148
            let textHeight = ceil(ReaderSelectableTextView.measureHeight(attributed: attributed, width: textWidth)) + 8
            let stageSize = NSSize(width: 180, height: textHeight + 24)
            let stage = FlippedDocumentView(frame: NSRect(origin: .zero, size: stageSize))
            stage.wantsLayer = true
            stage.layer?.backgroundColor = palette.background.nsColor.cgColor

            let tv = ReaderSelectableTextView(frame: NSRect(x: 16, y: 12, width: textWidth, height: textHeight))
            tv.applyAttributedText(attributed, width: textWidth)
            stage.addSubview(tv)

            let pixels = try OffscreenRenderHelper.renderNSView(
                stage,
                size: stageSize,
                appearance: appearance,
                bottomOriginRows: false,
                pngPath: pngOutDir.map { "\($0)/wrapped-inline-code-\(scheme.rawValue).png" }
            )

            let storage = try XCTUnwrap(tv.textStorage)
            let manager = try XCTUnwrap(tv.layoutManager)
            let container = try XCTUnwrap(tv.textContainer)
            manager.ensureLayout(for: container)

            struct LinePillBand {
                let minX: Int
                let width: Int
                let pillH: CGFloat
                let pillMinY: CGFloat
                let pillMaxY: CGFloat
                let glyphMidY: CGFloat
                let wrappedFromPrev: Bool
            }

            var spanLineBands: [[LinePillBand]] = []
            let fullRange = NSRange(location: 0, length: storage.length)
            storage.enumerateAttribute(.agentHubInlineCode, in: fullRange, options: []) { value, charRange, _ in
                guard value != nil, charRange.length > 0 else { return }
                let glyphRange = manager.glyphRange(forCharacterRange: charRange, actualCharacterRange: nil)
                let rawFont = storage.attribute(.font, at: charRange.location, effectiveRange: nil) as? NSFont
                let font = DesignFonts.nsMonoFont(size: rawFont?.pointSize ?? 13, weight: .regular)
                let para = storage.attribute(.paragraphStyle, at: charRange.location, effectiveRange: nil) as? NSParagraphStyle
                var bands: [LinePillBand] = []
                manager.enumerateLineFragments(forGlyphRange: glyphRange) { lineRect, _, lineContainer, lineGlyphRange, _ in
                    guard lineContainer === container else { return }
                    let subGlyph = NSIntersectionRange(glyphRange, lineGlyphRange)
                    guard subGlyph.length > 0 else { return }
                    let nominalH = (para?.maximumLineHeight ?? 0) > 0 ? min(lineRect.height, para!.maximumLineHeight) : lineRect.height
                    let pillH = max(12, min(nominalH - 2.2, max(16, ceil(font.ascender - font.descender) + 5)))
                    let baselineY = lineRect.minY + manager.location(forGlyphAt: subGlyph.location).y
                    let glyphMidY = baselineY - (font.ascender + font.descender) * 0.5
                    let pillMinY = 12.0 + glyphMidY - pillH * 0.5
                    let bbox = manager.boundingRect(forGlyphRange: subGlyph, in: container)
                    bands.append(LinePillBand(
                        minX: 16 + Int(floor(bbox.minX)),
                        width: max(8, Int(ceil(bbox.width))),
                        pillH: pillH,
                        pillMinY: pillMinY,
                        pillMaxY: pillMinY + pillH,
                        glyphMidY: 12.0 + glyphMidY,
                        wrappedFromPrev: subGlyph.location > glyphRange.location
                    ))
                }
                spanLineBands.append(bands)
            }

            XCTAssertEqual(spanLineBands.count, 2, "Expected 2 inline-code spans in \(scheme)")
            for (spanIdx, bands) in spanLineBands.enumerated() {
                XCTAssertGreaterThanOrEqual(bands.count, 3, "Inline-code span \(spanIdx) must wrap across >=3 lines in \(scheme)")
                let expectedPillH = bands[0].pillH
                for (lineIdx, band) in bands.enumerated() {
                    XCTAssertEqual(band.pillH, expectedPillH, accuracy: 0.01, "\(scheme) span \(spanIdx) pillH must stay invariant across wrapped lines")
                    let centerRegion = (
                        x: band.minX,
                        y: Int(floor(band.glyphMidY - 5)),
                        w: band.width,
                        h: 10
                    )
                    let pillPx = Self.countColorMatchPixels(
                        pixels: pixels,
                        imageWidth: Int(stageSize.width),
                        region: centerRegion,
                        target: palette.hover,
                        tolerance: 5
                    )
                    let inkPx = Self.countForegroundInkPixels(
                        pixels: pixels,
                        imageWidth: Int(stageSize.width),
                        region: centerRegion,
                        bg: palette.background,
                        minColorDelta: 40
                    )
                    XCTAssertGreaterThan(
                        pillPx,
                        25,
                        "\(scheme) span \(spanIdx) wrapped line \(lineIdx) missing inline-code pill background (got \(pillPx)px)"
                    )
                    XCTAssertGreaterThan(
                        inkPx,
                        10,
                        "\(scheme) span \(spanIdx) wrapped line \(lineIdx) missing glyph ink (got \(inkPx)px)"
                    )

                    if lineIdx + 1 < bands.count {
                        let gapY = Int(((band.pillMaxY + bands[lineIdx + 1].pillMinY) * 0.5).rounded())
                        let gapPillPx = Self.countColorMatchPixels(
                            pixels: pixels,
                            imageWidth: Int(stageSize.width),
                            region: (x: 16, y: gapY, w: Int(textWidth), h: 1),
                            target: palette.hover,
                            tolerance: 5
                        )
                        XCTAssertEqual(
                            gapPillPx,
                            0,
                            "\(scheme) span \(spanIdx) merged pill across inter-line gap at y=\(gapY) between line \(lineIdx) and \(lineIdx + 1)"
                        )
                    }
                }
            }

            // Also verify paragraphSpacingAfter=12 below paragraph 1's last inline-code line contains 0 drifted pill pixels
            if let para1Last = spanLineBands.first?.last {
                let spacingGapPillPx = Self.countColorMatchPixels(
                    pixels: pixels,
                    imageWidth: Int(stageSize.width),
                    region: (x: 16, y: Int(ceil(para1Last.pillMaxY)) + 2, w: Int(textWidth), h: 6),
                    target: palette.hover,
                    tolerance: 5
                )
                XCTAssertEqual(spacingGapPillPx, 0, "\(scheme) paragraphSpacingAfter must not pull inline-code pill downward into inter-paragraph gap")
            }

            // Also verify H1 heading (`paragraphSpacingBefore = 20`), mixed ASCII+CJK inline code,
            // file-link inline code with trailing NSTextAttachment copy icon, and Bidi RTL+LTR inline code
            let edgeMd = """
            导语说明。

            # 标题含 `ConfigurationManagerService 更新与维护` 检查

            文件 [`PreferencesWindowController.swift`](/workspace/src/PreferencesWindowController.swift) 与双向 `cmd --name مرحبا` 校验。
            """
            let edgeSegs = GfmLayoutCache.shared.getOrRender(
                markdown: edgeMd,
                foldInstructions: false,
                scheme: scheme,
                workspace: .codex,
                transport: store.transport
            )
            guard case let .prose(edgeAttr) = try XCTUnwrap(edgeSegs.first) else {
                return XCTFail("Expected rendered edge-case prose segment")
            }
            let edgeW: CGFloat = 190
            let edgeH = ceil(ReaderSelectableTextView.measureHeight(attributed: edgeAttr, width: edgeW)) + 8
            let edgeStageSize = NSSize(width: 222, height: edgeH + 24)
            let edgeStage = FlippedDocumentView(frame: NSRect(origin: .zero, size: edgeStageSize))
            edgeStage.wantsLayer = true
            edgeStage.layer?.backgroundColor = palette.background.nsColor.cgColor
            let edgeTv = ReaderSelectableTextView(frame: NSRect(x: 16, y: 12, width: edgeW, height: edgeH))
            edgeTv.applyAttributedText(edgeAttr, width: edgeW)
            edgeStage.addSubview(edgeTv)
            let edgePixels = try OffscreenRenderHelper.renderNSView(
                edgeStage,
                size: edgeStageSize,
                appearance: appearance,
                bottomOriginRows: false,
                pngPath: pngOutDir.map { "\($0)/wrapped-inline-code-edges-\(scheme.rawValue).png" }
            )
            let edgeStorage = try XCTUnwrap(edgeTv.textStorage)
            let edgeManager = try XCTUnwrap(edgeTv.layoutManager)
            let edgeContainer = try XCTUnwrap(edgeTv.textContainer)
            edgeManager.ensureLayout(for: edgeContainer)
            var verifiedEdgeSpans = 0
            edgeStorage.enumerateAttribute(.agentHubInlineCode, in: NSRange(location: 0, length: edgeStorage.length)) { val, cr, _ in
                guard val != nil, cr.length > 0 else { return }
                verifiedEdgeSpans += 1
                let gr = edgeManager.glyphRange(forCharacterRange: cr, actualCharacterRange: nil)
                let rf = edgeStorage.attribute(.font, at: cr.location, effectiveRange: nil) as? NSFont
                let f = DesignFonts.nsMonoFont(size: rf?.pointSize ?? 13, weight: .regular)
                let p = edgeStorage.attribute(.paragraphStyle, at: cr.location, effectiveRange: nil) as? NSParagraphStyle
                var fragHeights: [CGFloat] = []
                edgeManager.enumerateLineFragments(forGlyphRange: gr) { lr, _, lc, lgr, _ in
                    guard lc === edgeContainer else { return }
                    let sub = NSIntersectionRange(gr, lgr)
                    guard sub.length > 0 else { return }
                    let nomH = (p?.maximumLineHeight ?? 0) > 0 ? min(lr.height, p!.maximumLineHeight) : lr.height
                    let pH = max(12, min(nomH - 2.2, max(16, ceil(f.ascender - f.descender) + 5)))
                    fragHeights.append(pH)
                    let base = lr.minY + edgeManager.location(forGlyphAt: sub.location).y
                    let mid = 12.0 + base - (f.ascender + f.descender) * 0.5
                    let bb = edgeManager.boundingRect(forGlyphRange: sub, in: edgeContainer)
                    let reg = (x: 16 + Int(floor(bb.minX)), y: Int(floor(mid - 5)), w: max(8, Int(ceil(bb.width))), h: 10)
                    let pPx = Self.countColorMatchPixels(pixels: edgePixels, imageWidth: Int(edgeStageSize.width), region: reg, target: palette.hover, tolerance: 5)
                    XCTAssertGreaterThan(pPx, 15, "\(scheme) edge span \(verifiedEdgeSpans) missing pill fill")
                }
                if let h0 = fragHeights.first {
                    for h in fragHeights {
                        XCTAssertEqual(h, h0, accuracy: 0.01, "\(scheme) H1/CJK/link/bidi inline-code pillH must stay uniform across wrapped lines")
                    }
                }
            }
            XCTAssertEqual(verifiedEdgeSpans, 3, "Expected H1+CJK, file-link, and Bidi inline-code spans")

            // Verify 8-column table + prose cell at contentWidth=700 under .legacy scroller mounts horizontal scroller without crushing tokens
            ConversationTableCoordinator.scrollerStyleOverrideForTesting = .legacy
            defer { ConversationTableCoordinator.scrollerStyleOverrideForTesting = nil }
            let reproMd = """
            超宽多列表格（启用 `.legacy` 常驻横向滚动条时首行表头与末行数据均完整保留）：

            | 项目 | 提交 | 状态 | 入口类型 | 命令语法 | 配置文件 | 默认分支 | 关键职责与说明 |
            | --- | --- | --- | --- | --- | --- | --- | --- |
            | agenthub-webd | 9c4a083 | 正常 | 守护进程 | `agenthub-webd rpc-stdio` | `config.toml` | `main` | 零端口 SSH 远端会话与通知探针服务 |
            | agenthub-app | 3e0812d | 正常 | 原生桌面 | `AgentHub.app` | `Preferences` | `main` | Swift 6 + AppKit 六平台统一阅读工作台 |
            """
            let reproRow = ConversationRowItem(
                id: "repro-wide-700-\(scheme.rawValue)",
                timelineId: "repro-wide-700-\(scheme.rawValue)",
                kind: .assistant(markdown: reproMd, foldInstructions: false)
            )
            let coord = ConversationTableCoordinator(store: store)
            coord.scheme = scheme
            coord.workspace = .codex
            coord.sessionScopeKey = "local:codex:repro-700-\(scheme.rawValue)"
            let contentW: CGFloat = 700
            let cellH = ceil(coord.measureRowHeight(reproRow, width: contentW))
            let cellStageSize = NSSize(width: 780, height: cellH + 16)
            let cellStage = FlippedDocumentView(frame: NSRect(origin: .zero, size: cellStageSize))
            cellStage.wantsLayer = true
            cellStage.layer?.backgroundColor = palette.background.nsColor.cgColor

            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ReproWideCell"))
            cell.frame = NSRect(x: 0, y: 8, width: 780, height: cellH)
            cell.configure(
                item: reproRow,
                leadingInset: 40,
                trailingInset: 40,
                contentWidth: contentW,
                highlighted: false,
                coordinator: coord
            )
            cellStage.addSubview(cell)
            cellStage.layoutSubtreeIfNeeded()

            func findHorizontalScrollerView(_ v: NSView) -> NSScrollView? {
                if let sv = v as? NSScrollView, sv.hasHorizontalScroller { return sv }
                for sub in v.subviews {
                    if let found = findHorizontalScrollerView(sub) { return found }
                }
                return nil
            }
            let reproScroll = try XCTUnwrap(
                findHorizontalScrollerView(cell),
                "\(scheme) 8-column table at contentWidth=700 must enable horizontal scroller instead of crushing ASCII/code tokens"
            )
            XCTAssertGreaterThan(
                reproScroll.documentView?.bounds.width ?? 0,
                contentW + 40,
                "\(scheme) 8-column table document width must exceed 700pt viewport"
            )
            let reproPixels = try OffscreenRenderHelper.renderNSView(
                cellStage,
                size: cellStageSize,
                appearance: appearance,
                bottomOriginRows: false,
                pngPath: pngOutDir.map { "\($0)/repro-wide-legacy-scroller-\(scheme.rawValue).png" }
            )
            let totalInk = Self.countForegroundInkPixels(
                pixels: reproPixels,
                imageWidth: Int(cellStageSize.width),
                region: (x: 20, y: 8, w: 740, h: Int(cellH)),
                bg: palette.background
            )
            XCTAssertGreaterThan(totalInk, 800, "\(scheme) 8-column table stage must render rich non-blank ink")
        }
    }

    @MainActor
    func testExternalDisclosureRevisionAndNestedClaudeAndTableLinkEdgeCases() throws {
        let suiteName = "com.lich13.agenthub.tests.disclosureedge.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)
        let sharedDisclosure = ActivityDisclosureStore()

        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 840, height: 600))
        let table = NSTableView(frame: NSRect(x: 0, y: 0, width: 840, height: 600))
        let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("MainCol"))
        col.width = 840
        table.addTableColumn(col)
        table.headerView = nil
        table.rowSizeStyle = .custom
        table.intercellSpacing = .zero
        scroll.documentView = table

        let coord = ConversationTableCoordinator(store: store, disclosureStore: sharedDisclosure)
        coord.scrollView = scroll
        coord.tableView = table
        coord.column = col
        coord.sessionScopeKey = "local:claude:edge-1"
        table.dataSource = coord
        table.delegate = coord

        let nestedCmd = ExecutionCommand(
            id: "cmd-nested-1",
            sourceIds: ["src-cmd-1"],
            title: "cargo test --workspace",
            preview: nil,
            status: .completed,
            instructionFile: false,
            sections: [ExecutionSection(label: "stdout", text: String(repeating: "ok line\n", count: 8))]
        )
        let nestedGroup = ExecutionGroup(
            id: "grp-nested-1",
            mergedIds: [],
            commands: [nestedCmd],
            kind: "command",
            provider: "claude",
            running: false,
            recordedOnly: true,
            failedCount: 0,
            turnId: "t-1"
        )
        let nestedMeta = ClaudeMetadataGroup(
            id: "meta-1",
            sourceIds: ["m-src-1"],
            label: "会话上下文与元数据",
            entries: [.group(nestedGroup)]
        )
        let claudeProc = ClaudeProcessGroup(
            id: "cproc-1",
            stateKey: "claude-process:t-1",
            turnId: "t-1",
            sourceIds: ["p-src-1"],
            entries: [.metadata(nestedMeta)],
            segmentCount: 1,
            toolCount: 1,
            failedCount: 0,
            running: false,
            completed: true,
            recordedOnly: true,
            needsReply: false,
            needsPermission: false,
            hasPermission: false
        )
        let agentsMd = """
        # AGENTS.md instructions for /workspace/demo

        <INSTRUCTIONS>
        Line 1 rule
        Line 2 rule
        Line 3 rule
        Line 4 rule
        </INSTRUCTIONS>

        请查看表格结果：

        | 文件 | 说明 |
        | --- | --- |
        | [README.md](https://example.invalid/readme) | 含 `inline_code` 与超链接 |
        """
        let row0 = ConversationRowItem(
            id: "row-cproc",
            timelineId: "row-cproc",
            kind: .claudeProcessGroup(claudeProc)
        )
        let row1 = ConversationRowItem(
            id: "row-assistant",
            timelineId: "row-assistant",
            kind: .assistant(markdown: agentsMd, foldInstructions: true)
        )
        let row2 = ConversationRowItem(
            id: "row-anchor",
            timelineId: "row-anchor",
            kind: .assistant(markdown: "底部锚点正文", foldInstructions: false)
        )
        coord.updateRowsPreservingAnchor([row0, row1, row2], forceScrollToBottom: false)

        let closedProcH = table.rect(ofRow: 0).height
        let closedAssistH = table.rect(ofRow: 1).height
        XCTAssertEqual(closedProcH, 34, accuracy: 1.0)

        // Mutate row 0 externally on the shared ActivityDisclosureStore (simulating another pane bumping revision),
        // then toggle row 1's AGENTS.md fold via toggleDisclosurePreservingAnchor: row 0 MUST NOT keep stale height 34!
        sharedDisclosure.setOpen(scope: coord.sessionScopeKey, stateKey: "claude-process:t-1", isOpen: true)
        sharedDisclosure.setOpen(scope: coord.sessionScopeKey, stateKey: "meta-1", isOpen: true)
        sharedDisclosure.setOpen(scope: coord.sessionScopeKey, stateKey: "grp-nested-1", isOpen: true)
        sharedDisclosure.setOpen(scope: coord.sessionScopeKey, stateKey: "cmd-nested-1", isOpen: true)

        let instrSlices = VisibleMarkdownEngine.instructionSegments(VisibleMarkdownEngine.visibleMarkdown(agentsMd))
        let instrId = try XCTUnwrap(instrSlices.first(where: { $0.kind == "instructions" })?.id)
        coord.toggleDisclosurePreservingAnchor(
            stateKey: "row-assistant:\(instrId)",
            currentlyOpen: false
        )

        let openProcH = table.rect(ofRow: 0).height
        let openAssistH = table.rect(ofRow: 1).height
        XCTAssertGreaterThan(openProcH, closedProcH + 80, "Externally opened nested Claude process group must re-measure height instead of keeping stale cached 34pt")
        XCTAssertGreaterThan(openAssistH, closedAssistH + 40, "Toggled AGENTS.md fold in row 1 must expand row height")

        // Verify TableGridDocumentView mounts ReaderSelectableTextView cells with working link callback
        let row1Cell = try XCTUnwrap(table.view(atColumn: 0, row: 1, makeIfNecessary: true) as? ConversationRowCellView)
        func findTextViews(_ v: NSView) -> [ReaderSelectableTextView] {
            var out: [ReaderSelectableTextView] = []
            if let tv = v as? ReaderSelectableTextView { out.append(tv) }
            for s in v.subviews { out.append(contentsOf: findTextViews(s)) }
            return out
        }
        let textViews = findTextViews(row1Cell)
        XCTAssertTrue(textViews.contains(where: { $0.onLinkActivated != nil }), "TableGridDocumentView cells must wire onLinkActivated")

        // Verify >4096 char scratch measurement returns positive height and remains repeatable
        let hugeAttr = NSAttributedString(
            string: String(repeating: "长文本高度测量基准测试 ", count: 500),
            attributes: [.font: DesignFonts.nsBodyFont(size: 14)]
        )
        let h1 = ReaderSelectableTextView.measureHeight(attributed: hugeAttr, width: 420)
        let h2 = ReaderSelectableTextView.measureHeight(attributed: NSAttributedString(string: "short"), width: 420)
        XCTAssertGreaterThan(h1, 200)
        XCTAssertGreaterThan(h2, 10)
        XCTAssertLessThan(h2, 40)
    }
}
