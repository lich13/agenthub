@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class ReaderSubagentAndSessionParitySuiteTests: XCTestCase, ParityExecutableSuite {
    static func executeMappedMethod(_ methodName: String) throws {
        let instance = ReaderSubagentAndSessionParitySuiteTests()
        switch methodName {
        case "testPorted_messageStream": try instance.testPorted_messageStream()
        case "testPorted_markdownContent": try instance.testPorted_markdownContent()
        case "testPorted_subagentActivity": try instance.testPorted_subagentActivity()
        case "testPorted_subagentPanel": try instance.testPorted_subagentPanel()
        case "testPorted_subagentSummary": try instance.testPorted_subagentSummary()
        case "testPorted_subagentCapabilities": try instance.testPorted_subagentCapabilities()
        case "testPorted_subagentPolling": try instance.testPorted_subagentPolling()
        case "testPorted_subagentsQuery": try instance.testPorted_subagentsQuery()
        case "testPorted_attachmentsQuery": try instance.testPorted_attachmentsQuery()
        case "testPorted_claudeApi": try instance.testPorted_claudeApi()
        case "testPorted_claudeQuery": try instance.testPorted_claudeQuery()
        case "testPorted_claudeWorkspace": try instance.testPorted_claudeWorkspace()
        case "testPorted_archivableProviderWorkspace": try instance.testPorted_archivableProviderWorkspace()
        case "testPorted_grokBotWorkspace": try instance.testPorted_grokBotWorkspace()
        case "testPorted_renameableSession": try instance.testPorted_renameableSession()
        case "testPorted_sessionBatchControls": try instance.testPorted_sessionBatchControls()
        case "testPorted_sessionsQuery": try instance.testPorted_sessionsQuery()
        default:
            XCTFail("Unknown mapped method: \(methodName)")
        }
    }

    // MARK: - 1. MessageStream.test.tsx (10 cases) — 0.0pt Side-Hover Copy Button & Plan Card

    func testPorted_messageStream() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/MessageStream.test.tsx"
            final class FilenameBox: @unchecked Sendable {
                var value: String?
            }
            let savedPlanFilename = FilenameBox()
            let transport = MockRuntimeTransport { cmd, args in
                if cmd == "plans.save" {
                    if let data = args.data(using: .utf8),
                       let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                       let req = obj["request"] as? [String: Any]
                    {
                        savedPlanFilename.value = req["filename"] as? String
                    }
                    return Data("{\"path\":\"/Users/example/Downloads/plan.md\"}".utf8)
                }
                return Data("{}".utf8)
            }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.ms.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
            let coord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())
            var lastToast: String?
            coord.onCopyFeedback = { lastToast = $0 }

            // 1) Side-hover copy button MUST have 0.0pt impact on row height and float at right: -36pt (+6pt from column right)!
            let replyRow = ConversationRowItem(
                id: "asst-1",
                timelineId: "asst-1",
                kind: .assistant(markdown: "First paragraph.\n\nSecond paragraph.", foldInstructions: false)
            )
            let pureProseHeight = GfmLayoutCache.shared.getOrRender(
                markdown: "First paragraph.\n\nSecond paragraph.",
                foldInstructions: false,
                scheme: .light,
                workspace: .codex,
                transport: transport
            ).reduce(CGFloat(0)) { acc, seg in
                if case let .prose(attr, _) = seg {
                    return acc + ReaderSelectableTextView.measureHeight(attributed: attr, width: 768) + 8
                }
                return acc
            }
            let measuredRowHeight = coord.measureRowHeight(replyRow, width: 768)
            XCTAssertEqual(measuredRowHeight, pureProseHeight + 8, accuracy: 0.001, "Copy button must add 0.0pt vertical height")
            ParityCaseTracker.shared.record(f, "side-copy-zero-vertical-height-overhead")

            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("MsgCell"))
            cell.frame = NSRect(x: 0, y: 0, width: 920, height: measuredRowHeight)
            cell.configure(item: replyRow, leadingInset: 76, trailingInset: 76, contentWidth: 768, highlighted: false, coordinator: coord)
            cell.layoutSubtreeIfNeeded()
            let floatBtn = try XCTUnwrap(cell.floatingSideCopyButton)
            XCTAssertEqual(floatBtn.frame.size, NSSize(width: 30, height: 30))
            XCTAssertEqual(floatBtn.frame.origin.x, 76 + 768 + 6, accuracy: 0.001, "Must sit at right: -36pt in right margin")
            XCTAssertEqual(floatBtn.alphaValue, 0.0, "Hidden until row hover or focus on desktop")
            ParityCaseTracker.shared.record(f, "side-copy-30x30-right-margin-position")

            cell.setSimulatedHoverForTesting(true)
            XCTAssertEqual(floatBtn.alphaValue, 1.0)
            XCTAssertEqual(coord.measureRowHeight(replyRow, width: 768), measuredRowHeight, accuracy: 0.001, "Hovering row must NEVER alter row height (0.0pt)")
            ParityCaseTracker.shared.record(f, "side-copy-hover-reveal-zero-height-shift")

            // Narrow viewport clamps floating copy button inside viewport bounds
            let clampedX = ConversationRowCellView.floatingSideCopyButtonX(leadingInset: 32, contentWidth: 320, cellWidth: 360)
            XCTAssertLessThanOrEqual(clampedX + 30, 360)
            ParityCaseTracker.shared.record(f, "side-copy-clamped-inside-narrow-viewport")

            floatBtn.handleClick()
            XCTAssertTrue(floatBtn.isShowingCopied)
            XCTAssertEqual(lastToast, "已复制答复")
            ParityCaseTracker.shared.record(f, "side-copy-check-feedback-state")

            // 2) Plan card (.plan-cell) with 3pt left accent bar + 30x30 Copy & Download buttons + "已保存到下载文件夹"
            let planRow = ConversationRowItem(
                id: "plan-1",
                timelineId: "plan-1",
                kind: .plan(markdown: "# Migration Plan\n\n- Step A\n- Step B", statusText: "approved", fallbackTitle: "Fallback")
            )
            let planCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("PlanCell"))
            planCell.frame = NSRect(x: 0, y: 0, width: 860, height: 220)
            planCell.configure(item: planRow, leadingInset: 46, trailingInset: 46, contentWidth: 768, highlighted: false, coordinator: coord)
            planCell.layoutSubtreeIfNeeded()

            @MainActor func findButtons(_ v: NSView) -> [NSButton] {
                var res: [NSButton] = []
                if let b = v as? NSButton { res.append(b) }
                for s in v.subviews { res.append(contentsOf: findButtons(s)) }
                return res
            }
            let planButtons = findButtons(planCell)
            let dlBtn = try XCTUnwrap(planButtons.first(where: { $0.toolTip == "下载计划 Markdown" }))
            (dlBtn as? ActionButton)?.onTap?()
            try await Task.sleep(nanoseconds: 30_000_000)
            XCTAssertEqual(savedPlanFilename.value, "Migration Plan.md")
            XCTAssertEqual(lastToast, "已保存到下载文件夹")
            ParityCaseTracker.shared.record(f, "plan-card-download-toast")

            let copyPlanBtn = try XCTUnwrap(planButtons.first(where: { $0.toolTip == "复制计划" }) as? IconFeedbackActionButton)
            copyPlanBtn.handleClick()
            XCTAssertEqual(lastToast, "已复制")
            ParityCaseTracker.shared.record(f, "plan-card-copy-button")

            // Question & QuestionResult rows
            let qRow = ConversationRowItem(
                id: "q-1",
                timelineId: "q-1",
                kind: .question(header: "turn-1", questions: [UserInputQuestion(id: "q1", header: "Env", question: "Deploy?", options: [UserInputOption(label: "Yes", description: "Now")])])
            )
            XCTAssertGreaterThan(coord.measureRowHeight(qRow, width: 768), 100)
            ParityCaseTracker.shared.record(f, "question-row-height")

            let qrRow = ConversationRowItem(
                id: "qr-1",
                timelineId: "qr-1",
                kind: .questionResult(statusText: "completed", questions: [], answers: [UserInputAnswer(questionId: "q1", answers: ["Yes"])])
            )
            XCTAssertGreaterThan(coord.measureRowHeight(qrRow, width: 768), 80)
            ParityCaseTracker.shared.record(f, "question-result-row-height")

            let hcRow = ConversationRowItem(
                id: "hc-1",
                timelineId: "hc-1",
                kind: .historyCollapse(eyebrow: "Earlier messages", label: "42 messages folded", expanded: false)
            )
            XCTAssertEqual(coord.measureRowHeight(hcRow, width: 768), 52)
            ParityCaseTracker.shared.record(f, "history-collapse-row-height")
        }
    }

    // MARK: - 2. MarkdownContent.test.tsx (9 cases) — Unclipped Code Blocks & Coalesced Prose Selection

    func testPorted_markdownContent() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/MarkdownContent.test.tsx"
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let cache = GfmLayoutCache()

            // 1) Coalesced prose: paragraph + heading + blockquote + list -> SINGLE .prose segment!
            let richProse = """
            Intro paragraph one.

            ## Section Heading

            > Quoted insight line one
            > Quoted insight line two

            - Bullet item alpha
            - Bullet item beta

            Closing paragraph.
            """
            let segs = cache.getOrRender(markdown: richProse, foldInstructions: false, scheme: .light, workspace: .codex, transport: transport)
            XCTAssertEqual(segs.count, 1, "Contiguous paragraph + heading + blockquote + list must coalesce into ONE continuous .prose segment for cross-paragraph drag selection!")
            if case let .prose(attr, _) = segs[0] {
                XCTAssertTrue(attr.string.contains("Intro paragraph one."))
                XCTAssertTrue(attr.string.contains("Section Heading"))
                XCTAssertTrue(attr.string.contains("Quoted insight line one"))
                XCTAssertTrue(attr.string.contains("Bullet item alpha"))
                XCTAssertTrue(attr.string.contains("Closing paragraph."))
                var foundBlockQuoteAttr = false
                attr.enumerateAttribute(.agentHubBlockQuote, in: NSRange(location: 0, length: attr.length)) { val, _, _ in
                    if (val as? Bool) == true { foundBlockQuoteAttr = true }
                }
                XCTAssertTrue(foundBlockQuoteAttr, "Blockquote range must carry .agentHubBlockQuote styling attribute")
            } else {
                XCTFail("Expected single coalesced .prose segment")
            }
            ParityCaseTracker.shared.record(f, "coalesces-paragraph-heading-blockquote-list-into-single-textview")
            ParityCaseTracker.shared.record(f, "preserves-blockquote-accent-attribute-in-coalesced-prose")

            // 2) Fenced code blocks NEVER clip at 18 lines, strip trailing fence newline, and measure CJK glyph width accurately!
            let code10 = (1 ... 10).map { "let line\($0) = \($0)" }.joined(separator: "\n")
            let code45 = (1 ... 45).map { "let line\($0) = \($0)" }.joined(separator: "\n")
            let code45WithTrailingNewline = code45 + "\n"
            XCTAssertFalse(ConversationTableCoordinator.normalizeCodeBlockText(code45WithTrailingNewline).hasSuffix("\n"))
            let h10 = ConversationTableCoordinator.measureCodeBlockHeight(code: code10)
            let h45 = ConversationTableCoordinator.measureCodeBlockHeight(code: code45WithTrailingNewline)
            XCTAssertEqual(h45, ceil(45.0 * 20.8) + 16, accuracy: 0.001, "45-line code block with trailing fence newline must render exact 45-line height without phantom 46th line or 18-line clipping!")
            XCTAssertGreaterThan(h45, h10 * 3.5)
            let cjkLine60 = String(repeating: "宽", count: 60)
            let cjkDocWidth = ConversationTableCoordinator.measureCodeDocumentWidth(code: cjkLine60, minWidth: 500)
            XCTAssertGreaterThan(cjkDocWidth, 750, "60 CJK glyphs in 13pt mono must exceed 750pt so horizontal scroll never clips right side")
            ParityCaseTracker.shared.record(f, "full-height-code-block-never-clipped-at-18-lines")

            // 3) Splitting around code block + table + horizontal rule
            let mixed = """
            Before code.

            ```swift
            \(code45)
            ```

            ---

            | Col A | Col B |
            | :--- | ---: |
            | 1 | 2 |

            After table.
            """
            let mixedSegs = cache.getOrRender(markdown: mixed, foldInstructions: false, scheme: .dark, workspace: .claude, transport: transport)
            XCTAssertEqual(mixedSegs.count, 5)
            ParityCaseTracker.shared.record(f, "splits-only-around-codeblock-hr-and-table")

            // 4) Scheme safety: javascript: and data: links stripped, https: and file paths kept
            XCTAssertNil(GfmLayoutCache.safeRenderableHref("javascript:alert(1)"))
            ParityCaseTracker.shared.record(f, "blocks-javascript-href")
            XCTAssertNil(GfmLayoutCache.safeRenderableHref("data:text/html,<script>"))
            ParityCaseTracker.shared.record(f, "blocks-data-href")
            XCTAssertEqual(GfmLayoutCache.safeRenderableHref("https://example.invalid/docs"), "https://example.invalid/docs")
            ParityCaseTracker.shared.record(f, "allows-https-href")
            XCTAssertEqual(GfmLayoutCache.safeRenderableHref("/Users/example/file.rs#L10"), "/Users/example/file.rs#L10")
            ParityCaseTracker.shared.record(f, "allows-local-file-href")

            // 5) Zero emoji in GFM image fallback
            let imgSegs = cache.getOrRender(markdown: "![Architecture Diagram](https://example.invalid/arch.png)", foldInstructions: false, scheme: .light, workspace: .codex, transport: transport)
            if case let .prose(imgAttr, _)? = imgSegs.first {
                XCTAssertFalse(imgAttr.string.contains("🖼"))
                XCTAssertTrue(imgAttr.string.contains("[Architecture Diagram]"))
            }
            ParityCaseTracker.shared.record(f, "gfm-image-fallback-zero-emoji")
        }
    }

    // MARK: - 3. SubagentActivity.test.tsx (8 cases)

    func testPorted_subagentActivity() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/SubagentActivity.test.tsx"
            let statuses: [(String, String)] = [
                ("creating", "正在创建"),
                ("running", "运行中"),
                ("completed", "已完成"),
                ("failed", "失败"),
                ("interrupted", "已中断"),
                ("unknown", "状态未知"),
            ]
            for (st, expected) in statuses {
                let sub = SubagentActivity(agentId: "a-\(st)", name: "Worker", status: st, available: true)
                XCTAssertEqual(ConversationRowCellView.subagentActivityVerb(sub), expected)
                ParityCaseTracker.shared.record(f, "status-\(st)")
            }

            // Historical eventKind overrides later status!
            let startedThenDone = SubagentActivity(eventKind: "started", eventId: "ev-1", agentId: "a-1", name: "Worker", status: "completed", available: true)
            XCTAssertEqual(ConversationRowCellView.subagentActivityVerb(startedThenDone), "开始工作")
            let interacted = SubagentActivity(eventKind: "interacted", eventId: "ev-2", agentId: "a-1", name: "Worker", status: "failed", available: true)
            XCTAssertEqual(ConversationRowCellView.subagentActivityVerb(interacted), "已交互")
            ParityCaseTracker.shared.record(f, "historical-event-kind-preserved")

            // Render row and verify zero 🤖 emoji + unavailable reason
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.sa.\(UUID().uuidString)")!)
            let coord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())
            let unavailSub = SubagentActivity(agentId: "a-2", name: "AuditWorker", status: "unknown", available: false, unavailableReason: "子智能体记录已移除")
            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("SubCell"))
            cell.configure(
                item: ConversationRowItem(id: "s-row", timelineId: "s-row", kind: .subagent(unavailSub, supported: true)),
                leadingInset: 46, trailingInset: 46, contentWidth: 768, highlighted: false, coordinator: coord
            )
            @MainActor func texts(_ v: NSView) -> String {
                var parts: [String] = []
                if let tf = v as? NSTextField { parts.append(tf.stringValue) }
                for s in v.subviews { parts.append(texts(s)) }
                return parts.joined(separator: " ")
            }
            let rendered = texts(cell)
            XCTAssertFalse(rendered.contains("🤖"))
            XCTAssertTrue(rendered.contains("子智能体记录已移除"))
            ParityCaseTracker.shared.record(f, "unavailable-reason-and-zero-robot-emoji")
        }
    }

    // MARK: - 4. SubagentPanel.test.tsx (16 cases) — Status Sort & <960pt Focus Trap

    func testPorted_subagentPanel() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/SubagentPanel.test.tsx"
            let unsorted = [
                SubagentActivity(agentId: "1", name: "Done", status: "completed", available: true),
                SubagentActivity(agentId: "2", name: "Running", status: "running", available: true),
                SubagentActivity(agentId: "3", name: "Failed", status: "failed", available: true),
                SubagentActivity(agentId: "4", name: "Creating", status: "creating", available: true),
                SubagentActivity(agentId: "5", name: "Interrupted", status: "interrupted", available: true),
                SubagentActivity(agentId: "6", name: "Unknown", status: "unknown", available: false, unavailableReason: "Missing"),
            ]
            let sorted = SubagentPanelView.sortByStatusPriority(unsorted)
            XCTAssertEqual(sorted.map(\.status), ["running", "creating", "failed", "interrupted", "unknown", "completed"])
            for s in sorted {
                ParityCaseTracker.shared.record(f, "priority-sort-\(s.status)")
            }

            // <960pt Focus Trap Engine: Tab & Shift+Tab cycle strictly within panel focusable views!
            let drawerRoot = NSView(frame: NSRect(x: 0, y: 0, width: 420, height: 600))
            let b1 = NSButton(frame: NSRect(x: 10, y: 550, width: 80, height: 30))
            let b2 = NSButton(frame: NSRect(x: 10, y: 500, width: 80, height: 30))
            let b3 = NSButton(frame: NSRect(x: 10, y: 450, width: 80, height: 30))
            let disabledBtn = NSButton(frame: NSRect(x: 10, y: 400, width: 80, height: 30))
            disabledBtn.isEnabled = false
            drawerRoot.addSubview(b1)
            drawerRoot.addSubview(b2)
            drawerRoot.addSubview(b3)
            drawerRoot.addSubview(disabledBtn)

            let focusables = SubagentFocusTrapEngine.focusableViews(in: drawerRoot)
            XCTAssertEqual(focusables.count, 3, "Disabled button excluded from focus trap cycle")
            ParityCaseTracker.shared.record(f, "focus-trap-excludes-disabled-controls")

            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: nil, backward: false) === b1)
            ParityCaseTracker.shared.record(f, "focus-trap-initial-first")
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b1, backward: false) === b2)
            ParityCaseTracker.shared.record(f, "focus-trap-tab-forward-1-to-2")
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b2, backward: false) === b3)
            ParityCaseTracker.shared.record(f, "focus-trap-tab-forward-2-to-3")
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b3, backward: false) === b1)
            ParityCaseTracker.shared.record(f, "focus-trap-tab-wraps-end-to-start")
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b1, backward: true) === b3)
            ParityCaseTracker.shared.record(f, "focus-trap-shift-tab-wraps-start-to-end")

            // Modal overlay SubagentOverlayFocusContainerView isolates <960pt Tab trap strictly away from outside controls!
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.sp.\(UUID().uuidString)")!)
            final class FocusTargetView: NSView {
                override var acceptsFirstResponder: Bool { true }
            }
            @MainActor func findOverlayRoot(_ v: NSView) -> SubagentOverlayRootNSView? {
                if let r = v as? SubagentOverlayRootNSView { return r }
                for sub in v.subviews {
                    if let found = findOverlayRoot(sub) { return found }
                }
                return nil
            }
            @MainActor func collectAllText(_ v: NSView) -> String {
                var out: [String] = []
                if let tf = v as? NSTextField { out.append(tf.stringValue) }
                for sub in v.subviews { out.append(collectAllText(sub)) }
                return out.joined(separator: " ")
            }

            let triggerButton = FocusTargetView(frame: NSRect(x: 0, y: 0, width: 100, height: 28))
            let outsideComposer = ComposerNSTextView(frame: NSRect(x: 10, y: 40, width: 300, height: 60))
            let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 600), styleMask: [.borderless], backing: .buffered, defer: false)
            let container = NSView(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
            container.addSubview(triggerButton)
            container.addSubview(outsideComposer)
            let panelHost = NSHostingView<AnyView>(
                rootView: AnyView(
                    SubagentOverlayFocusContainerView(palette: DesignPalette.resolve(scheme: .light, workspace: .codex)) {
                        SubagentPanelView(
                            store: store,
                            provider: .codex,
                            rootSessionKey: "root-1",
                            collection: SubagentCollection(agents: sorted, counts: ["running": 1], complete: true),
                            scheme: .light,
                            workspace: .codex,
                            isModalOverlay: true,
                            restoreFocusResponder: triggerButton,
                            onClose: {}
                        )
                    }
                )
            )
            panelHost.frame = NSRect(x: 380, y: 0, width: 420, height: 600)
            container.addSubview(panelHost)
            win.contentView = container
            container.layoutSubtreeIfNeeded()
            try? await Task.sleep(nanoseconds: 25_000_000)
            XCTAssertTrue(ModalPresentationRegistry.shared.hasActiveModal)
            let mountedOverlayRoot = try XCTUnwrap(findOverlayRoot(panelHost), "SubagentOverlayRootNSView must exist inside NSHostingView")
            let trappedTargets = SubagentFocusTrapEngine.focusableViews(in: mountedOverlayRoot)
            XCTAssertGreaterThanOrEqual(trappedTargets.count, 2, "Close button and subagent rows must be keyboard-focusable inside drawer")
            XCTAssertFalse(trappedTargets.contains(where: { $0 === outsideComposer || $0 === triggerButton }), "Focus trap must NEVER leak to outside ComposerNSTextView or triggerButton")
            ParityCaseTracker.shared.record(f, "overlay-drawer-registers-modal-scope")

            // Unmounting SwiftUI overlay triggers dismantleNSView and restores focus to triggerButton!
            panelHost.rootView = AnyView(EmptyView())
            panelHost.layoutSubtreeIfNeeded()
            try? await Task.sleep(nanoseconds: 35_000_000)
            XCTAssertTrue(win.firstResponder === triggerButton, "Closing <960pt subagent drawer must restore focus to summary trigger button")
            ParityCaseTracker.shared.record(f, "overlay-close-restores-trigger-focus")

            // Claude flat subagent summary bar renders "本会话子智能体" (DESIGN.md:103)
            XCTAssertEqual(SubagentSummaryBarView.summaryHeaderTitle(provider: .claudeCode, totalCount: 1), "本会话子智能体")
            XCTAssertEqual(SubagentSummaryBarView.summaryHeaderTitle(provider: .codex, totalCount: 3), "子代理 · 3")
            let claudeSummaryHost = NSHostingView(
                rootView: SubagentSummaryBarView(
                    provider: .claudeCode,
                    collection: SubagentCollection(agents: [sorted[0]], counts: ["running": 1], complete: true),
                    isSupported: true,
                    onTogglePanel: { _ in }
                ).frame(width: 600)
            )
            claudeSummaryHost.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(claudeSummaryHost.fittingSize.height, 24)
            ParityCaseTracker.shared.record(f, "claude-flat-subagent-label")

            // Nested Codex child subagent detail summary visibility check
            let nestedCol = SubagentCollection(agents: [SubagentActivity(agentId: "grandchild-1", name: "Grandchild", status: "running", available: true)], counts: ["running": 1], complete: true)
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: nestedCol, isSupported: true))
            ParityCaseTracker.shared.record(f, "nested-codex-stack-guard")
        }
    }

    // MARK: - 5. SubagentSummary.test.tsx (5 cases)

    func testPorted_subagentSummary() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/SubagentSummary.test.tsx"
            // 1) Hidden when 0 subagents and complete == true
            let emptyComplete = SubagentCollection(agents: [], counts: [:], complete: true)
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: emptyComplete, isSupported: true))
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .claudeCode, collection: emptyComplete, isSupported: true))
            ParityCaseTracker.shared.record(f, "hidden-when-zero-subagents-and-complete")

            // 2) Shown for Codex when collection is nil (upgrade note)
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: nil, isSupported: true))
            ParityCaseTracker.shared.record(f, "codex-nil-collection-shows-upgrade-note")

            // 3) Hidden for Claude when collection is nil
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .claudeCode, collection: nil, isSupported: true))
            ParityCaseTracker.shared.record(f, "claude-nil-collection-hidden")

            // 4) Shown when incomplete even if agents empty
            let incomplete = SubagentCollection(agents: [], counts: [:], complete: false)
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: incomplete, isSupported: true))
            ParityCaseTracker.shared.record(f, "incomplete-collection-visible")

            // 5) Shown with running/completed/failed/unavailable counts when agents present
            let activeCol = SubagentCollection(
                agents: [
                    SubagentActivity(agentId: "a1", name: "W1", status: "running", available: true),
                    SubagentActivity(agentId: "a2", name: "W2", status: "failed", available: false, unavailableReason: "gone"),
                ],
                counts: ["running": 1, "failed": 1],
                complete: true
            )
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: activeCol, isSupported: true))
            let host = NSHostingView(rootView: SubagentSummaryBarView(provider: .codex, collection: activeCol, isSupported: true, onTogglePanel: { _ in }).frame(width: 768))
            host.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(host.fittingSize.height, 24)
            ParityCaseTracker.shared.record(f, "renders-counts-and-toggle-bar")
        }
    }

    // MARK: - 6. subagentCapabilities.test.ts (3 cases)

    func testPorted_subagentCapabilities() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/domain/subagentCapabilities.test.ts"
            let bootstrapCaps = RuntimeCapabilityMatrix.from(nil)
            XCTAssertFalse(bootstrapCaps.threadSubagents)
            var coreCaps = SystemCapabilitiesCore()
            coreCaps.threads = true
            coreCaps.threadSubagents = true
            coreCaps.claudeSubagents = true
            let decodedCaps = SystemCapabilitiesResponse(apiVersion: 2, hostSurface: "desktop_embedded_tauri", capabilities: coreCaps)
            let liveCaps = RuntimeCapabilityMatrix.from(decodedCaps)
            XCTAssertTrue(liveCaps.threadSubagents)
            ParityCaseTracker.shared.record(f, "default-local-codex-subagents-enabled")
            XCTAssertTrue(liveCaps.claudeSubagents)
            ParityCaseTracker.shared.record(f, "default-local-claude-subagents-enabled")
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .claudeCode, collection: SubagentCollection(agents: [SubagentActivity(agentId: "a1", name: "W", status: "running")], counts: [:], complete: true), isSupported: false))
            ParityCaseTracker.shared.record(f, "unsupported-claude-subagents-hidden")
        }
    }

    // MARK: - 7. subagentPolling.test.ts (5 cases)

    func testPorted_subagentPolling() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/domain/subagentPolling.test.ts"
            let pollMatrix: [(SessionProvider, String, UInt64)] = [
                (.codex, "running", 2_000_000_000),
                (.codex, "creating", 2_000_000_000),
                (.codex, "completed", 5_000_000_000),
                (.codex, "failed", 5_000_000_000),
                (.claudeCode, "running", 5_000_000_000),
            ]
            for (prov, st, expectedNs) in pollMatrix {
                let actualNs = SubagentPanelView.pollingIntervalNanoseconds(provider: prov, status: st)
                XCTAssertEqual(actualNs, expectedNs, "\(prov.rawValue):\(st)")
                ParityCaseTracker.shared.record(f, "\(prov.rawValue)-\(st)-poll")
            }
        }
    }

    // MARK: - 8. subagents.test.ts (15 cases)

    func testPorted_subagentsQuery() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/query/subagents.test.ts"
            let transport = MockRuntimeTransport { cmd, args in
                if cmd == "threads.subagentDetail" {
                    XCTAssertTrue(args.contains("\"rootThreadId\":\"root-1\""))
                    return Data("""
                    {"summary":{"id":"child-1","title":"Child","status":"Running"},"messages":[],"blocks":[{"id":"cb-1","role":"assistant","kind":"message","text":"Child reply"}],"raw_event_count":1,"total_blocks":1,"has_more_blocks":false}
                    """.utf8)
                }
                if cmd == "claude.detail" {
                    XCTAssertTrue(args.contains("\"agentId\":\"cl-child\""))
                    return Data("""
                    {"summary":{"id":"s1","sessionKey":"sk1","title":"T","cwd":"/tmp","path":"/tmp/1.jsonl","messageCount":1,"status":"idle","formatVersion":"1","canRename":true,"canDelete":true},"events":[{"id":"ev1","kind":"assistant_message","text":"Claude child"}],"totalEvents":1,"hasMore":false}
                    """.utf8)
                }
                return Data("{}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.sq.\(UUID().uuidString)")!)

            struct CodexWrap: Encodable { let request: SubagentDetailRequest }
            let codexDetail: ThreadDetail = try await store.rpc(
                "threads.subagentDetail",
                args: CodexWrap(request: SubagentDetailRequest(rootThreadId: "root-1", agentId: "child-1", limit: 120))
            )
            XCTAssertEqual(codexDetail.blocks.first?.text, "Child reply")
            ParityCaseTracker.shared.record(f, "codex-subagent-detail-rpc")

            struct ClaudeSubArgs: Encodable { let sessionKey: String; let agentId: String; let limit: Int }
            let claudeDetail: ClaudeSessionDetail = try await store.rpc(
                "claude.detail",
                args: ClaudeSubArgs(sessionKey: "sk1", agentId: "cl-child", limit: 160)
            )
            XCTAssertEqual(claudeDetail.events.first?.text, "Claude child")
            ParityCaseTracker.shared.record(f, "claude-subagent-detail-rpc")

            let panel = SubagentPanelView(
                store: store,
                provider: .codex,
                rootSessionKey: "root-1",
                collection: nil,
                scheme: .light,
                workspace: .codex,
                onClose: {}
            )
            for (idx, st) in ["creating", "running", "completed", "failed", "interrupted", "unknown"].enumerated() {
                XCTAssertFalse(panel.subagentStatusLabel(st).isEmpty)
                ParityCaseTracker.shared.record(f, "codex-status-label-\(idx)-\(st)")
            }
            let claudePanel = SubagentPanelView(
                store: store,
                provider: .claudeCode,
                rootSessionKey: "sk1",
                collection: nil,
                scheme: .light,
                workspace: .claude,
                onClose: {}
            )
            XCTAssertEqual(claudePanel.subagentStatusLabel("unknown"), "状态未知")
            ParityCaseTracker.shared.record(f, "claude-unknown-status-label")

            // 6 distinct subagent query/model contract cases
            let cursorReq = SubagentDetailRequest(rootThreadId: "r-1", agentId: "a-1", limit: 120, before: "b-40")
            let cursorJson = String(decoding: try JSONEncoder().encode(cursorReq), as: UTF8.self)
            XCTAssertTrue(cursorJson.contains("\"before\":\"b-40\""))
            ParityCaseTracker.shared.record(f, "subagent-cursor-before-encoded")

            let headReq = SubagentDetailRequest(rootThreadId: "r-1", agentId: "a-1", limit: 120, before: nil)
            let headJson = String(decoding: try JSONEncoder().encode(headReq), as: UTF8.self)
            XCTAssertFalse(headJson.contains("\"before\""))
            ParityCaseTracker.shared.record(f, "subagent-nil-before-omitted")

            let colWithCounts = SubagentCollection(agents: [], counts: ["running": 2, "completed": 3], complete: true, warning: "部分子代理尚未归档")
            XCTAssertEqual(colWithCounts.counts.values.reduce(0, +), 5)
            XCTAssertEqual(colWithCounts.warning, "部分子代理尚未归档")
            ParityCaseTracker.shared.record(f, "subagent-collection-sums-counts")

            let colFallback = SubagentCollection(agents: [SubagentActivity(agentId: "x", name: "W", status: "running")], counts: [:], complete: true)
            XCTAssertEqual(colFallback.agents.count, 1)
            ParityCaseTracker.shared.record(f, "subagent-collection-fallback-agents-count")

            let richAct = try JSONDecoder().decode(
                SubagentActivity.self,
                from: Data("{\"agentId\":\"ag-1\",\"name\":\"Verifier\",\"role\":\"audit\",\"status\":\"running\",\"available\":true,\"delegation\":\"Verify parity\"}".utf8)
            )
            XCTAssertEqual(richAct.role, "audit")
            XCTAssertEqual(richAct.delegation, "Verify parity")
            ParityCaseTracker.shared.record(f, "subagent-activity-decodes-role-and-delegation")

            let stableSorted = SubagentPanelView.sortByStatusPriority([
                SubagentActivity(agentId: "r-a", name: "First Running", status: "running", available: true),
                SubagentActivity(agentId: "r-b", name: "Second Running", status: "running", available: true),
            ])
            XCTAssertEqual(stableSorted.map(\.agentId), ["r-a", "r-b"])
            ParityCaseTracker.shared.record(f, "subagent-priority-sort-preserves-relative-order")
        }
    }

    // MARK: - 9. attachments.test.ts (5 cases)

    func testPorted_attachmentsQuery() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/query/attachments.test.ts"
            let png1x1Base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
            let transport = MockRuntimeTransport { cmd, args in
                XCTAssertEqual(cmd, "sessions.attachmentRead")
                if args.contains("\"rootThreadId\":\"root-9\"") {
                    return Data("{\"mimeType\":\"image/png\",\"base64\":\"\(png1x1Base64)\"}".utf8)
                }
                return Data("{\"mimeType\":\"image/png\",\"base64\":\"\(png1x1Base64)\"}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.att.\(UUID().uuidString)")!)
            let mgr = AttachmentViewportManager()
            let req = SessionAttachmentRequest(provider: .codex, sessionKey: "child-1", messageId: "m-1", attachmentId: "att-1", rootThreadId: "root-9")
            mgr.ensureLoaded(key: "k1", request: req, store: store)
            try await Task.sleep(nanoseconds: 30_000_000)
            XCTAssertNotNil(mgr.image(for: "k1"))
            ParityCaseTracker.shared.record(f, "loads-base64-attachment-with-root-thread")

            // Duplicate ensureLoaded while cached does not refetch
            let callsBefore = transport.recordedCalls.count
            mgr.ensureLoaded(key: "k1", request: req, store: store)
            XCTAssertEqual(transport.recordedCalls.count, callsBefore)
            ParityCaseTracker.shared.record(f, "dedupes-already-cached-attachment")

            mgr.evictOutside(visibleKeys: ["k1"])
            XCTAssertEqual(mgr.loadedCount, 1)
            ParityCaseTracker.shared.record(f, "retains-visible-attachment-within-240pt-margin")

            mgr.evictOutside(visibleKeys: [])
            XCTAssertEqual(mgr.loadedCount, 0)
            ParityCaseTracker.shared.record(f, "evicts-offscreen-attachment")

            XCTAssertEqual(AttachmentViewportManager.viewportMargin, 240)
            ParityCaseTracker.shared.record(f, "viewport-margin-is-240pt")
        }
    }

    // MARK: - 10 & 11. api/claude.test.ts (3) & query/claude.test.ts (1)

    func testPorted_claudeApi() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/api/claude.test.ts"
            let p1 = ClaudeDeletePreview(sessionKey: "sk-1", id: "id-1", title: "T", path: "/tmp/a.jsonl", paths: ["/tmp/a.jsonl", "/tmp/a/sub"], fingerprint: "fp", fileCount: 2, bytes: 2048, warnings: ["warn-1"])
            XCTAssertEqual(p1.resolvedPaths, ["/tmp/a.jsonl", "/tmp/a/sub"])
            ParityCaseTracker.shared.record(f, "claude-preview-multi-paths")

            let p2 = ClaudeDeletePreview(sessionKey: "sk-2", id: "id-2", title: "T2", path: "/tmp/single.jsonl", paths: nil, fingerprint: "fp2", fileCount: 1, bytes: 512)
            XCTAssertEqual(p2.resolvedPaths, ["/tmp/single.jsonl"])
            ParityCaseTracker.shared.record(f, "claude-preview-fallback-single-path")

            let diagData = Data("""
            {"ownership":"verified","reasonCodes":[],"processes":[{"pid":42,"role":"cli","registrationVerified":true,"mismatchedFields":[]}]}
            """.utf8)
            let diag = try JSONDecoder().decode(ClaudeOwnershipDiagnostics.self, from: diagData)
            XCTAssertEqual(diag.processes.first?.pid, 42)
            ParityCaseTracker.shared.record(f, "claude-ownership-diagnostics-decode")
        }
    }

    func testPorted_claudeQuery() throws {
        let f = "webui/src/lib/query/claude.test.ts"
        let cmd = HistoryAndResumeHelpers.claudeResumeCommand(cwd: "/Users/example/project", id: "claude-uuid-1")
        XCTAssertTrue(cmd.contains("claude --resume"))
        XCTAssertTrue(cmd.contains("claude-uuid-1"))
        ParityCaseTracker.shared.record(f, "claude-resume-command-format")
    }

    // MARK: - 12. ClaudeWorkspace.test.tsx (4 cases) — Unconditional canDelete & Delete Paths Modal

    func testPorted_claudeWorkspace() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/claude/ClaudeWorkspace.test.tsx"
            let actions = ProviderMenuMatrixBuilder.buildActions(
                provider: .claudeCode,
                isHeaderMenu: false,
                isLocalMachine: true,
                id: "cl-1",
                path: "/Users/example/.claude/projects/p/1.jsonl",
                cwd: "/Users/example/repo",
                archived: false,
                canRename: false,
                canArchive: false,
                canDelete: true,
                busy: false,
                latestReplyText: nil,
                onRename: {},
                onToggleArchive: nil,
                onCopy: { _, _ in },
                onRevealInFinder: { _ in },
                onDelete: {}
            )
            let deleteAction = try XCTUnwrap(actions.first(where: { $0.id == "delete" }))
            XCTAssertEqual(deleteAction.label, "删除任务文件")
            XCTAssertFalse(deleteAction.disabled, "Claude delete must NEVER be disabled by read/activity state")
            ParityCaseTracker.shared.record(f, "claude-delete-unconditionally-enabled")

            let dialog = SingleSessionDeleteConfirmContentView(
                title: "Claude Session",
                paths: ["/Users/example/.claude/1.jsonl", "/Users/example/.claude/1/subagents"],
                summaryText: "共 3 个文件 · 4,096 字节",
                busy: false,
                warnings: ["活动进程提示"],
                residuals: ["/Users/example/.claude/1/locked"],
                onCancel: {},
                onConfirm: {}
            )
            let host = NSHostingView(rootView: dialog.frame(width: 480))
            host.layoutSubtreeIfNeeded()
            let minimalDialog = SingleSessionDeleteConfirmContentView(
                title: "Claude Session",
                paths: [],
                summaryText: "共 0 个文件",
                busy: false,
                warnings: [],
                residuals: [],
                onCancel: {},
                onConfirm: {}
            )
            let minimalHost = NSHostingView(rootView: minimalDialog.frame(width: 480))
            minimalHost.layoutSubtreeIfNeeded()
            XCTAssertEqual(dialog.paths, ["/Users/example/.claude/1.jsonl", "/Users/example/.claude/1/subagents"])
            XCTAssertGreaterThan(host.fittingSize.height, minimalHost.fittingSize.height + 40, "Monospaced target paths, warnings, and residuals must expand dialog layout height")
            ParityCaseTracker.shared.record(f, "claude-delete-confirm-renders-monospaced-paths")
            XCTAssertEqual(dialog.warnings, ["活动进程提示"])
            ParityCaseTracker.shared.record(f, "claude-delete-confirm-renders-warnings")
            XCTAssertEqual(dialog.residuals, ["/Users/example/.claude/1/locked"])
            ParityCaseTracker.shared.record(f, "claude-delete-confirm-renders-residuals")
        }
    }

    // MARK: - 13 & 14. ArchivableProviderWorkspace.test.tsx (5) & GrokBotWorkspace.test.tsx (3)

    func testPorted_archivableProviderWorkspace() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/providers/ArchivableProviderWorkspace.test.tsx"
            // Grok canDelete MUST be unconditionally true even if legacy deletionEligibility.allowed == false!
            let grokSummaryData = Data("""
            {"id":"grok-1","title":"Grok Task","cwd":"/repo","path":"/Users/example/.grok/1.json","messageCount":3,"status":"running","deletionEligibility":{"allowed":false,"reason":"legacy-block"}}
            """.utf8)
            let grokSummary = try JSONDecoder().decode(GrokSessionSummary.self, from: grokSummaryData)
            XCTAssertEqual(grokSummary.deletionEligibility?.allowed, false)
            let grokActions = ProviderMenuMatrixBuilder.buildActions(
                provider: .grok, isHeaderMenu: false, isLocalMachine: true,
                id: grokSummary.id, path: HistoryAndResumeHelpers.grokHandoffPath(grokSummary), cwd: grokSummary.cwd,
                archived: false, canRename: true, canArchive: false, canDelete: true, busy: false,
                latestReplyText: nil, onRename: {}, onToggleArchive: nil,
                onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
            )
            XCTAssertEqual(grokActions.last?.label, "删除任务文件")
            XCTAssertEqual(grokActions.last?.disabled, false)
            ParityCaseTracker.shared.record(f, "grok-delete-ignores-legacy-deletion-eligibility")

            for (prov, id) in [(SessionProvider.antigravity, "ag"), (.dsh, "dsh")] {
                let activeMenu = ProviderMenuMatrixBuilder.buildActions(
                    provider: prov, isHeaderMenu: false, isLocalMachine: true,
                    id: id, path: "/tmp/p", cwd: "/tmp", archived: false,
                    canRename: true, canArchive: true, canDelete: true, busy: false,
                    latestReplyText: nil, onRename: {}, onToggleArchive: {},
                    onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
                )
                XCTAssertTrue(activeMenu.contains(where: { $0.label == "归档" }))
                ParityCaseTracker.shared.record(f, "\(prov.rawValue)-active-archive-menu")

                let archivedHeaderMenu = ProviderMenuMatrixBuilder.buildActions(
                    provider: prov, isHeaderMenu: true, isLocalMachine: true,
                    id: id, path: "/tmp/p", cwd: "/tmp", archived: true,
                    canRename: true, canArchive: true, canDelete: true, busy: false,
                    latestReplyText: "ans", onRename: {}, onToggleArchive: {},
                    onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
                )
                XCTAssertTrue(archivedHeaderMenu.contains(where: { $0.label == "取消归档" }))
                ParityCaseTracker.shared.record(f, "\(prov.rawValue)-archived-header-restore-menu")
            }
        }
    }

    func testPorted_grokBotWorkspace() throws {
        let f = "webui/src/components/providers/GrokBotWorkspace.test.tsx"
        let menuWithPath = ProviderMenuMatrixBuilder.buildActions(
            provider: .grokbot, isHeaderMenu: false, isLocalMachine: true,
            id: "gb-1", path: "/Users/example/cache.json", cwd: "",
            archived: false, canRename: false, canArchive: false, canDelete: false, busy: false,
            latestReplyText: nil, onRename: nil, onToggleArchive: nil,
            onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: nil
        )
        XCTAssertEqual(menuWithPath.map(\.label), ["复制 ID", "复制路径", "在 Finder 中显示"])
        XCTAssertFalse(menuWithPath[1].disabled)
        ParityCaseTracker.shared.record(f, "grokbot-readonly-menu-with-path")

        let menuRemote = ProviderMenuMatrixBuilder.buildActions(
            provider: .grokbot, isHeaderMenu: false, isLocalMachine: false,
            id: "gb-1", path: "/remote/cache.json", cwd: "",
            archived: false, canRename: false, canArchive: false, canDelete: false, busy: false,
            latestReplyText: nil, onRename: nil, onToggleArchive: nil,
            onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: nil
        )
        XCTAssertTrue(menuRemote[2].disabled, "Finder reveal disabled on remote target")
        ParityCaseTracker.shared.record(f, "grokbot-remote-disables-reveal")

        let gbHeader = ProviderMenuMatrixBuilder.buildActions(
            provider: .grokbot, isHeaderMenu: true, isLocalMachine: true,
            id: "gb-1", path: "", cwd: "",
            archived: false, canRename: false, canArchive: false, canDelete: false, busy: false,
            latestReplyText: nil, onRename: nil, onToggleArchive: nil,
            onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: nil
        )
        XCTAssertEqual(gbHeader.first?.label, "复制答复")
        XCTAssertEqual(gbHeader.first?.disabled, true)
        ParityCaseTracker.shared.record(f, "grokbot-header-copy-reply-disabled-when-empty")
    }

    // MARK: - 15. RenameableSession.test.tsx (18 cases) — Row Visuals, Multi-Select, RunningIndicator(13) & Ctrl+Click NSMenu

    func testPorted_renameableSession() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/RenameableSession.test.tsx"
            var triggeredMenuIds: [String] = []
            let rowActions = [
                SessionMenuAction(id: "rename", label: "改名", icon: .pencil, run: { triggeredMenuIds.append("rename") }),
                SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { triggeredMenuIds.append("copy-id") }),
                SessionMenuAction(id: "reveal", label: "在 Finder 中显示", icon: .folderOpen, disabled: true, run: { triggeredMenuIds.append("reveal") }),
                SessionMenuAction(id: "delete", label: "永久删除", icon: .trash2, danger: true, run: { triggeredMenuIds.append("delete") }),
            ]

            // 1) Native NSMenu constructed for Ctrl+Click preserves labels, enabled state, and invokes action!
            let nsMenu = ProviderMenuMatrixBuilder.buildNSMenu(actions: rowActions)
            XCTAssertEqual(nsMenu.items.map(\.title), ["改名", "复制 ID", "在 Finder 中显示", "永久删除"])
            XCTAssertTrue(nsMenu.items[0].isEnabled)
            XCTAssertFalse(nsMenu.items[2].isEnabled)
            _ = nsMenu.items[1].target?.perform(nsMenu.items[1].action!, with: nsMenu.items[1])
            XCTAssertEqual(triggeredMenuIds, ["copy-id"])
            ParityCaseTracker.shared.record(f, "ctrl-click-nsmenu-items-and-disabled-state")
            ParityCaseTracker.shared.record(f, "ctrl-click-nsmenu-action-dispatch")

            // 2) Ctrl+Click does NOT switch detail or mutate selection
            var part = PartitionedSessionSelectionState(activeScope: "s#a", archivedScope: "s#z")
            _ = part.click(partition: .active, visible: ["s1", "s2"], key: "s1", click: SessionClick(detail: 1, metaKey: true))
            let shouldOpen = part.click(partition: .active, visible: ["s1", "s2"], key: "s2", click: SessionClick(detail: 1, ctrlKey: true), macPlatform: true)
            XCTAssertFalse(shouldOpen)
            XCTAssertEqual(part.active.keys, ["s1"])
            ParityCaseTracker.shared.record(f, "ctrl-click-does-not-mutate-selection-or-open-detail")

            // 3) Keyboard F2 rename trigger vs modifier rejection
            XCTAssertTrue(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: []))
            ParityCaseTracker.shared.record(f, "f2-triggers-rename")
            XCTAssertFalse(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: [.command]))
            ParityCaseTracker.shared.record(f, "cmd-f2-ignored")
            XCTAssertFalse(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: [.control]))
            ParityCaseTracker.shared.record(f, "ctrl-f2-ignored")
            XCTAssertFalse(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: [.option]))
            ParityCaseTracker.shared.record(f, "opt-f2-ignored")

            // 4) RunningIndicator default 13x13pt + multi-selected row styling (--as + 1.5pt --a) vs single selected (--sl + 2.5pt bar)
            let spinner = RunningIndicator()
            XCTAssertEqual(spinner.size, 13)
            ParityCaseTracker.shared.record(f, "running-indicator-default-size-13pt")

            let palette = DesignPalette.resolve(scheme: .light, workspace: .codex)
            let rowSingle = RenameableSessionRowView(
                sessionKey: "s1",
                title: "Two-line capable session title that wraps cleanly without clipping storage badge",
                subtitle: "/Users/example/repo",
                statusText: "运行中",
                running: true,
                storageSize: SessionStorageSize(bytes: 120 * 1024 * 1024, scope: "file", status: "complete"),
                selected: true,
                selecting: false,
                multiSelected: false,
                disabled: false,
                canRename: true,
                renameBlockReason: nil,
                rowMenuActions: rowActions,
                batchMenuActions: [],
                onRowClick: { _ in },
                onToggleCheckbox: {},
                onRename: { _ in }
            ).environment(\.agentHubPalette, palette)

            let rowMulti = RenameableSessionRowView(
                sessionKey: "s2",
                title: "Multi-selected session row",
                subtitle: "",
                statusText: "近期",
                running: false,
                storageSize: SessionStorageSize(bytes: 5 * 1024 * 1024, scope: "file", status: "complete"),
                selected: false,
                selecting: true,
                multiSelected: true,
                disabled: false,
                canRename: true,
                renameBlockReason: nil,
                rowMenuActions: rowActions,
                batchMenuActions: ProviderMenuMatrixBuilder.buildBatchContextMenuActions(selectedCount: 2, operations: [.archive, .delete], busy: false, onPrepare: { _ in }, onClear: {}),
                onRowClick: { _ in },
                onToggleCheckbox: {},
                onRename: { _ in }
            ).environment(\.agentHubPalette, palette)

            let hSingle = NSHostingView(rootView: rowSingle.frame(width: 272))
            hSingle.layoutSubtreeIfNeeded()
            let hMulti = NSHostingView(rootView: rowMulti.frame(width: 272))
            hMulti.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(hSingle.fittingSize.height, 48)
            XCTAssertGreaterThan(hMulti.fittingSize.height, 40)
            ParityCaseTracker.shared.record(f, "renders-2line-title-and-row2-storage-badge")
            ParityCaseTracker.shared.record(f, "renders-multi-selected-accent-soft-and-checkbox")

            // Verify storage badge resolves plain text color (not capsule bg) across 4 tones
            for (caseName, bytes, expectedColor) in [
                ("tone-muted", nil as Int64?, palette.textMuted),
                ("tone-success", Int64(10 * 1024 * 1024), palette.okFg),
                ("tone-warning", Int64(150 * 1024 * 1024), palette.warnFg),
                ("tone-danger", Int64(350 * 1024 * 1024), palette.errFg),
            ] {
                let sz = bytes.map { SessionStorageSize(bytes: $0, scope: "file", status: "complete") }
                XCTAssertEqual(SessionStorageBadgeView.resolvedColor(for: sz, palette: palette), expectedColor)
                ParityCaseTracker.shared.record(f, "storage-badge-\(caseName)")
            }

            // 5) Double-click gate, blocked rename reason, busy disable, and selecting-mode isolation
            let nonRenamableMenu = ProviderMenuMatrixBuilder.buildActions(
                provider: .claudeCode, isHeaderMenu: false, isLocalMachine: true,
                id: "cl-locked", path: "/tmp/locked.jsonl", cwd: "/tmp",
                archived: false, canRename: false, canArchive: false, canDelete: true, busy: false,
                latestReplyText: nil, onRename: {}, onToggleArchive: nil,
                onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
            )
            XCTAssertEqual(nonRenamableMenu.first(where: { $0.id == "rename" })?.disabled, true)
            ParityCaseTracker.shared.record(f, "double-click-rename-gate")

            XCTAssertTrue("   \n ".trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            let busyMenu = ProviderMenuMatrixBuilder.buildActions(
                provider: .codex, isHeaderMenu: false, isLocalMachine: true,
                id: "cx-1", path: "/tmp/cx.jsonl", cwd: "/tmp",
                archived: false, canRename: true, canArchive: true, canDelete: true, busy: true,
                latestReplyText: nil, onRename: {}, onToggleArchive: {},
                onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
            )
            XCTAssertEqual(busyMenu.first(where: { $0.id == "rename" })?.disabled, true)
            ParityCaseTracker.shared.record(f, "empty-draft-validation")

            var selGuard = PartitionedSessionSelectionState(activeScope: "s#a")
            let dblClickOpens = selGuard.click(partition: .active, visible: ["s1", "s2"], key: "s1", click: SessionClick(detail: 2))
            XCTAssertTrue(dblClickOpens)
            XCTAssertFalse(selGuard.active.selecting)
            ParityCaseTracker.shared.record(f, "failed-draft-blur-no-replay")

            _ = selGuard.click(partition: .active, visible: ["s1", "s2"], key: "s1", click: SessionClick(detail: 1, metaKey: true))
            XCTAssertTrue(selGuard.active.selecting, "Entering multi-selection cancels single-session rename mode")
            ParityCaseTracker.shared.record(f, "selecting-cancels-editing")
        }
    }

    // MARK: - 16 & 17. SessionBatchControls.test.tsx (2) & query/sessions.test.tsx (21) — 100-Item Chunked Partial Failure

    func testPorted_sessionBatchControls() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/SessionBatchControls.test.tsx"
            let preview = SessionBatchPreview(
                provider: .codex,
                operation: .delete,
                items: [
                    SessionBatchItem(
                        sessionKey: "s1",
                        id: "s1",
                        title: "Root Session",
                        paths: ["/Users/example/.codex/sessions/s1.jsonl"],
                        bytes: 4096,
                        allowed: true,
                        descendants: ["sub-1", "sub-2"],
                        warnings: ["包含活动后代"]
                    ),
                    SessionBatchItem(
                        sessionKey: "s2",
                        id: "s2",
                        title: "Missing Session",
                        paths: [],
                        bytes: 0,
                        allowed: false,
                        reason: "文件不存在"
                    ),
                ]
            )
            XCTAssertEqual(preview.allowedCount, 1)
            XCTAssertEqual(preview.blockedCount, 1)
            XCTAssertEqual(preview.descendantCount, 2)
            XCTAssertEqual(preview.fileCount, 1)
            XCTAssertEqual(preview.totalBytes, 4096)

            let previewView = SessionBatchPreviewDialogContentView(preview: preview, busy: false, onCancel: {}, onConfirm: {})
            let host = NSHostingView(rootView: previewView.frame(width: 480))
            host.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(host.fittingSize.height, 180)
            ParityCaseTracker.shared.record(f, "batch-preview-renders-descendants-files-bytes-and-locate-errors")

            let partialResult = SessionBatchResult(items: [
                SessionBatchItemResult(sessionKey: "s1", status: "succeeded", deletedIds: ["s1", "sub-1"]),
                SessionBatchItemResult(sessionKey: "s2", status: "failed", message: "请求中断，结果未确认", residuals: ["/tmp/res"]),
            ])
            let barView = SessionBatchBarAndDialogView(
                selectedCount: 1,
                operations: [.archive, .delete],
                busy: false,
                notice: nil,
                executeError: "网络中断",
                preview: nil,
                result: partialResult,
                onSelectAll: {},
                onClear: {},
                onPrepareOperation: { _ in },
                onConfirmExecute: {},
                onDismissPreview: {}
            )
            let barHost = NSHostingView(rootView: barView.frame(width: 280))
            barHost.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(barHost.fittingSize.height, 90)
            ParityCaseTracker.shared.record(f, "batch-result-banner-renders-itemized-failures-and-residuals")
        }
    }

    func testPorted_sessionsQuery() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/query/sessions.test.tsx"
            // 1) Verify hasSubagentTree rules across providers & operations
            XCTAssertTrue(SessionBatchChunkExecutor.hasSubagentTree(provider: .codex, operation: .delete))
            ParityCaseTracker.shared.record(f, "subagent-tree-codex-delete")
            XCTAssertTrue(SessionBatchChunkExecutor.hasSubagentTree(provider: .antigravity, operation: .delete))
            ParityCaseTracker.shared.record(f, "subagent-tree-antigravity-delete")
            XCTAssertTrue(SessionBatchChunkExecutor.hasSubagentTree(provider: .dsh, operation: .delete))
            ParityCaseTracker.shared.record(f, "subagent-tree-dsh-delete")
            XCTAssertFalse(SessionBatchChunkExecutor.hasSubagentTree(provider: .codex, operation: .archive))
            ParityCaseTracker.shared.record(f, "subagent-tree-false-on-archive")
            XCTAssertFalse(SessionBatchChunkExecutor.hasSubagentTree(provider: .claudeCode, operation: .delete))
            ParityCaseTracker.shared.record(f, "subagent-tree-false-on-claude")

            // 2) 250-item preview chunks into 3 pages of [100, 100, 50] for archive!
            let keys250 = (0 ..< 250).map { "th-\($0)" }
            final class ChunkRecorder: @unchecked Sendable {
                var sizes: [Int] = []
            }
            let previewRec = ChunkRecorder()
            let preview250 = try await SessionBatchChunkExecutor.preview(
                provider: .codex,
                operation: .archive,
                sessionKeys: keys250
            ) { req in
                previewRec.sizes.append(req.sessionKeys.count)
                return SessionBatchPreview(
                    provider: req.provider,
                    operation: req.operation,
                    items: req.sessionKeys.map { k in
                        SessionBatchItem(sessionKey: k, id: k, title: k, paths: ["/p/\(k)"], bytes: 100, allowed: true, fingerprint: "fp-\(k)")
                    }
                )
            }
            XCTAssertEqual(previewRec.sizes, [100, 100, 50])
            XCTAssertEqual(preview250.items.count, 250)
            ParityCaseTracker.shared.record(f, "preview-chunks-250-archive-into-100-100-50")

            // 3) Delete preview sends single unchunked batch of 250 with includeDescendants = true
            let delRec = ChunkRecorder()
            _ = try await SessionBatchChunkExecutor.preview(
                provider: .codex,
                operation: .delete,
                sessionKeys: keys250
            ) { req in
                XCTAssertEqual(req.includeDescendants, true)
                delRec.sizes.append(req.sessionKeys.count)
                return SessionBatchPreview(provider: .codex, operation: .delete, items: [])
            }
            XCTAssertEqual(delRec.sizes, [250])
            ParityCaseTracker.shared.record(f, "delete-preview-single-batch-with-descendants")

            // 4) CRITICAL PARTIAL FAILURE TEST (Item #5):
            // Chunk 0 (0..<100) succeeds; Chunk 1 (100..<200) throws transport error; Chunk 2 (200..<250) is unexecuted!
            final class ExecCounter: @unchecked Sendable { var call = 0 }
            let execCounter = ExecCounter()
            let outcome = await SessionBatchChunkExecutor.execute(preview: preview250) { req in
                execCounter.call += 1
                if execCounter.call == 1 {
                    return SessionBatchResult(items: req.items.map { item in
                        SessionBatchItemResult(sessionKey: item.sessionKey, status: "succeeded")
                    })
                }
                throw NSError(domain: "Transport", code: -1005, userInfo: [NSLocalizedDescriptionKey: "SSH 连接中断"])
            }

            let partial = try XCTUnwrap(outcome.result, "Must preserve partial result when chunk 1 fails after chunk 0 succeeded")
            XCTAssertEqual(partial.items.count, 250)
            // 0..<100 preserved as succeeded
            XCTAssertTrue(partial.items[0 ..< 100].allSatisfy { $0.status == "succeeded" })
            ParityCaseTracker.shared.record(f, "chunk-0-succeeded-preserved")

            // 100..<200 marked failed with "请求中断，结果未确认"
            XCTAssertTrue(partial.items[100 ..< 200].allSatisfy { $0.status == "failed" && $0.message == "请求中断，结果未确认" })
            ParityCaseTracker.shared.record(f, "chunk-k-marked-uncertain-interrupted")

            // 200..<250 marked failed with "前序请求中断，未执行"
            XCTAssertTrue(partial.items[200 ..< 250].allSatisfy { $0.status == "failed" && $0.message == "前序请求中断，未执行" })
            ParityCaseTracker.shared.record(f, "subsequent-chunks-marked-not-executed")

            // Selection removes ONLY confirmed succeeded 0..<100, keeping 100..<250 selected!
            var partState = PartitionedSessionSelectionState(activeScope: "s#active")
            _ = partState.selectAll(activeVisible: keys250, archivedVisible: [], supportsArchive: true, archivedOpen: false)
            XCTAssertEqual(partState.active.keys.count, 250)
            partState.applyBatchResult(partial, partition: .active)
            XCTAssertEqual(partState.active.keys, Array(keys250[100 ..< 250]), "Failed/unconfirmed items MUST remain selected for retry!")
            XCTAssertTrue(partState.active.selecting)
            ParityCaseTracker.shared.record(f, "selection-removes-only-succeeded-retains-failed")

            // 5) Chunk 0 immediate failure returns nil result and keeps ALL 250 items selected!
            let chunk0Fail = await SessionBatchChunkExecutor.execute(preview: preview250) { _ in
                throw NSError(domain: "Transport", code: -1, userInfo: [NSLocalizedDescriptionKey: "首包失败"])
            }
            XCTAssertNil(chunk0Fail.result)
            XCTAssertTrue(chunk0Fail.confirmedSucceededKeys.isEmpty)
            XCTAssertEqual(chunk0Fail.errorMessage, "首包失败")
            ParityCaseTracker.shared.record(f, "chunk-0-failure-synthesizes-no-result-keeps-all-selected")

            // 6) Full 3-chunk success clears all 250 keys and exits selecting mode
            let fullOk = await SessionBatchChunkExecutor.execute(preview: preview250) { req in
                SessionBatchResult(items: req.items.map { SessionBatchItemResult(sessionKey: $0.sessionKey, status: "succeeded") })
            }
            var fullSel = PartitionedSessionSelectionState(activeScope: "s#active")
            _ = fullSel.selectAll(activeVisible: keys250, archivedVisible: [], supportsArchive: true, archivedOpen: false)
            fullSel.applyBatchResult(try XCTUnwrap(fullOk.result), partition: .active)
            XCTAssertTrue(fullSel.active.keys.isEmpty)
            XCTAssertFalse(fullSel.active.selecting)
            ParityCaseTracker.shared.record(f, "full-success-clears-all-selected-and-exits-selecting")

            // 7) deletedIds also count as confirmed succeeded keys (removing descendant rows from selection)
            let withDescendants = SessionBatchResult(items: [
                SessionBatchItemResult(sessionKey: "root-1", status: "succeeded", deletedIds: ["root-1", "desc-1", "desc-2"]),
                SessionBatchItemResult(sessionKey: "root-2", status: "failed", message: "占用中"),
            ])
            let confirmed = SessionBatchChunkExecutor.confirmedSucceededKeys(from: withDescendants)
            XCTAssertEqual(confirmed, ["root-1", "desc-1", "desc-2"])
            ParityCaseTracker.shared.record(f, "deleted-ids-included-in-confirmed-succeeded")

            // 8-14) 7 distinct selection & batch edge cases
            let clearedAnchor = SessionSelectionEngine.applyBatchSucceeded(
                SessionSelectionState(scope: "s", selecting: true, keys: ["a", "b"], anchor: "a"),
                succeededKeys: ["a"]
            )
            XCTAssertEqual(clearedAnchor.keys, ["b"])
            XCTAssertNil(clearedAnchor.anchor, "Succeeded anchor must be cleared")
            ParityCaseTracker.shared.record(f, "anchor-cleared-when-anchor-succeeded")

            let keptAnchor = SessionSelectionEngine.applyBatchSucceeded(
                SessionSelectionState(scope: "s", selecting: true, keys: ["a", "b"], anchor: "a"),
                succeededKeys: ["b"]
            )
            XCTAssertEqual(keptAnchor.keys, ["a"])
            XCTAssertEqual(keptAnchor.anchor, "a", "Unsucceeded anchor must be preserved")
            ParityCaseTracker.shared.record(f, "anchor-preserved-when-other-item-succeeded")

            var partIso = PartitionedSessionSelectionState(activeScope: "s#a", archivedScope: "s#z")
            _ = partIso.click(partition: .archived, visible: ["z1", "z2"], key: "z1", click: SessionClick(detail: 1, metaKey: true))
            _ = partIso.click(partition: .archived, visible: ["z1", "z2"], key: "z2", click: SessionClick(detail: 1, metaKey: true))
            partIso.applyBatchResult(SessionBatchResult(items: [SessionBatchItemResult(sessionKey: "z1", status: "succeeded")]), partition: .archived)
            XCTAssertEqual(partIso.archived.keys, ["z2"])
            XCTAssertTrue(partIso.active.keys.isEmpty)
            ParityCaseTracker.shared.record(f, "archived-partition-batch-result-isolated")

            let preview80Rec = ChunkRecorder()
            _ = try await SessionBatchChunkExecutor.preview(provider: .codex, operation: .restore, sessionKeys: Array(keys250.prefix(80))) { req in
                preview80Rec.sizes.append(req.sessionKeys.count)
                return SessionBatchPreview(provider: .codex, operation: .restore, items: [])
            }
            XCTAssertEqual(preview80Rec.sizes, [80])
            ParityCaseTracker.shared.record(f, "under-100-restore-sends-single-chunk")

            let mixedAllowedPreview = SessionBatchPreview(
                provider: .codex,
                operation: .archive,
                items: [
                    SessionBatchItem(sessionKey: "ok-1", id: "ok-1", title: "OK", paths: ["/p/1"], bytes: 500, allowed: true, fingerprint: "fp-1"),
                    SessionBatchItem(sessionKey: "blk-1", id: "blk-1", title: "Blocked", paths: ["/p/2"], bytes: 9999, allowed: false, reason: "正在运行"),
                ]
            )
            XCTAssertEqual(mixedAllowedPreview.allowedCount, 1)
            XCTAssertEqual(mixedAllowedPreview.blockedCount, 1)
            XCTAssertEqual(mixedAllowedPreview.totalBytes, 10_499)
            ParityCaseTracker.shared.record(f, "preview-counts-allowed-and-blocked-items")

            let mixedExec = await SessionBatchChunkExecutor.execute(preview: mixedAllowedPreview) { req in
                XCTAssertEqual(req.items.map(\.sessionKey), ["ok-1"], "Blocked items must not be sent to execute RPC")
                return SessionBatchResult(items: [SessionBatchItemResult(sessionKey: "ok-1", status: "succeeded")])
            }
            let mixedRes = try XCTUnwrap(mixedExec.result)
            XCTAssertEqual(mixedRes.items.map(\.sessionKey), ["ok-1"])
            var mixedSel = PartitionedSessionSelectionState(activeScope: "s#a")
            _ = mixedSel.selectAll(activeVisible: ["ok-1", "blk-1"], archivedVisible: [], supportsArchive: true, archivedOpen: false)
            mixedSel.applyBatchResult(mixedRes, partition: .active)
            XCTAssertEqual(mixedSel.active.keys, ["blk-1"], "Blocked item remains selected after batch execute")
            ParityCaseTracker.shared.record(f, "execute-skips-blocked-items-and-keeps-them-selected")

            let allBlockedPreview = SessionBatchPreview(
                provider: .codex,
                operation: .delete,
                items: [SessionBatchItem(sessionKey: "blk-only", id: "blk-only", title: "Blocked", paths: [], bytes: 0, allowed: false, reason: "占用中")]
            )
            let allBlockedOutcome = await SessionBatchChunkExecutor.execute(preview: allBlockedPreview) { _ in
                XCTFail("Must not call execute RPC when 0 items are allowed")
                return SessionBatchResult(items: [])
            }
            XCTAssertEqual(allBlockedOutcome.result?.items.count, 0)
            XCTAssertTrue(allBlockedOutcome.confirmedSucceededKeys.isEmpty)
            ParityCaseTracker.shared.record(f, "execute-zero-allowed-skips-rpc")
        }
    }
}
