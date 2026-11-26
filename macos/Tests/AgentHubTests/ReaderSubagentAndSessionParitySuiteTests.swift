@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class ReaderSubagentAndSessionParitySuiteTests: XCTestCase {
    // MARK: - 1. MessageStream.test.tsx (10 cases) — 0.0pt Side-Hover Copy Button & Plan Card

    func testPorted_messageStream() throws {
        try runAsyncParityCheck {
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
                    return Data(#"{"filename":"plan.md"}"#.utf8)
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
                if case let .prose(attr) = seg {
                    return acc + ReaderSelectableTextView.measureHeight(attributed: attr, width: 768) + 8
                }
                return acc
            }
            let measuredRowHeight = coord.measureRowHeight(replyRow, width: 768)
            XCTAssertEqual(measuredRowHeight, pureProseHeight + 8, accuracy: 0.001, "Copy button must add 0.0pt vertical height")

            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("MsgCell"))
            cell.frame = NSRect(x: 0, y: 0, width: 920, height: measuredRowHeight)
            cell.configure(item: replyRow, leadingInset: 76, trailingInset: 76, contentWidth: 768, highlighted: false, coordinator: coord)
            cell.layoutSubtreeIfNeeded()
            let floatBtn = try XCTUnwrap(cell.floatingSideCopyButton)
            XCTAssertEqual(floatBtn.frame.size, NSSize(width: 30, height: 30))
            XCTAssertEqual(floatBtn.frame.origin.x, 76 + 768 + 6, accuracy: 0.001, "Must sit at right: -36pt in right margin")
            XCTAssertEqual(floatBtn.alphaValue, 0.0, "Hidden until row hover or focus on desktop")

            cell.setSimulatedHoverForTesting(true)
            XCTAssertEqual(floatBtn.alphaValue, 1.0)
            XCTAssertEqual(coord.measureRowHeight(replyRow, width: 768), measuredRowHeight, accuracy: 0.001, "Hovering row must NEVER alter row height (0.0pt)")

            // Narrow viewport clamps floating copy button inside viewport bounds
            let clampedX = ConversationRowCellView.floatingSideCopyButtonX(leadingInset: 32, contentWidth: 320, cellWidth: 360)
            XCTAssertLessThanOrEqual(clampedX + 30, 360)

            floatBtn.handleClick()
            XCTAssertTrue(floatBtn.isShowingCopied)
            XCTAssertEqual(lastToast, "已复制答复")

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
            let dlBtn = try XCTUnwrap(planButtons.first(where: { $0.toolTip == "下载计划 Markdown" }) as? IconFeedbackActionButton)
            dlBtn.handleClick()
            try await Task.sleep(nanoseconds: 30_000_000)
            XCTAssertEqual(savedPlanFilename.value, "Migration Plan.md")
            XCTAssertEqual(lastToast, "已保存到下载文件夹")

            let copyPlanBtn = try XCTUnwrap(planButtons.first(where: { $0.toolTip == "复制计划" }) as? IconFeedbackActionButton)
            copyPlanBtn.handleClick()
            XCTAssertEqual(lastToast, "已复制")

            // Question & QuestionResult rows
            let qRow = ConversationRowItem(
                id: "q-1",
                timelineId: "q-1",
                kind: .question(header: "turn-1", questions: [UserInputQuestion(id: "q1", header: "Env", question: "Deploy?", options: [UserInputOption(label: "Yes", description: "Now")])])
            )
            XCTAssertGreaterThan(coord.measureRowHeight(qRow, width: 768), 100)

            let qrRow = ConversationRowItem(
                id: "qr-1",
                timelineId: "qr-1",
                kind: .questionResult(statusText: "completed", questions: [], answers: [UserInputAnswer(questionId: "q1", answers: ["Yes"])])
            )
            XCTAssertGreaterThan(coord.measureRowHeight(qrRow, width: 768), 80)

            let hcRow = ConversationRowItem(
                id: "hc-1",
                timelineId: "hc-1",
                kind: .historyCollapse(eyebrow: "Earlier messages", label: "42 messages folded", expanded: false)
            )
            XCTAssertEqual(coord.measureRowHeight(hcRow, width: 768), 64)
        }
    }

    // MARK: - 2. MarkdownContent.test.tsx (9 cases) — Unclipped Code Blocks & Coalesced Prose Selection

    func testPorted_markdownContent() throws {
        try runAsyncParityCheck {
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
            if case let .prose(attr) = segs[0] {
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

            // 4) Scheme safety: javascript: and data: links stripped, https: and file paths kept
            XCTAssertNil(GfmLayoutCache.safeRenderableHref("javascript:alert(1)"))
            XCTAssertNil(GfmLayoutCache.safeRenderableHref("data:text/html,<script>"))
            XCTAssertEqual(GfmLayoutCache.safeRenderableHref("https://example.invalid/docs"), "https://example.invalid/docs")
            XCTAssertEqual(GfmLayoutCache.safeRenderableHref("/Users/example/file.rs#L10"), "/Users/example/file.rs#L10")

            // 5) Zero emoji in GFM image fallback
            let imgSegs = cache.getOrRender(markdown: "![Architecture Diagram](https://example.invalid/arch.png)", foldInstructions: false, scheme: .light, workspace: .codex, transport: transport)
            if case let .prose(imgAttr)? = imgSegs.first {
                XCTAssertFalse(imgAttr.string.contains("🖼"))
                XCTAssertTrue(imgAttr.string.contains("[Architecture Diagram]"))
            }
        }
    }

    // MARK: - 3. SubagentActivity.test.tsx (8 cases)

    func testPorted_subagentActivity() throws {
        try runAsyncParityCheck {
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
            }

            // Historical eventKind overrides later status!
            let startedThenDone = SubagentActivity(eventKind: "started", eventId: "ev-1", agentId: "a-1", name: "Worker", status: "completed", available: true)
            XCTAssertEqual(ConversationRowCellView.subagentActivityVerb(startedThenDone), "开始工作")
            let interacted = SubagentActivity(eventKind: "interacted", eventId: "ev-2", agentId: "a-1", name: "Worker", status: "failed", available: true)
            XCTAssertEqual(ConversationRowCellView.subagentActivityVerb(interacted), "已交互")

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
        }
    }

    // MARK: - 4. SubagentPanel.test.tsx (16 cases) — Status Sort & <960pt Focus Trap

    func testPorted_subagentPanel() throws {
        try runAsyncParityCheck {
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

            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: nil, backward: false) === b1)
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b1, backward: false) === b2)
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b2, backward: false) === b3)
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b3, backward: false) === b1)
            XCTAssertTrue(SubagentFocusTrapEngine.nextFocusTarget(in: drawerRoot, current: b1, backward: true) === b3)

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
            let outsideFocusable = FocusTargetView(frame: NSRect(x: 10, y: 40, width: 300, height: 60))
            let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 600), styleMask: [.borderless], backing: .buffered, defer: false)
            let container = NSView(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
            container.addSubview(triggerButton)
            container.addSubview(outsideFocusable)
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
            XCTAssertFalse(trappedTargets.contains(where: { $0 === outsideFocusable || $0 === triggerButton }), "Focus trap must NEVER leak to outside responder views or triggerButton")

            // Unmounting SwiftUI overlay triggers dismantleNSView and restores focus to triggerButton!
            panelHost.rootView = AnyView(EmptyView())
            panelHost.layoutSubtreeIfNeeded()
            try? await Task.sleep(nanoseconds: 35_000_000)
            XCTAssertTrue(win.firstResponder === triggerButton, "Closing <960pt subagent drawer must restore focus to summary trigger button")

            // Claude flat subagent summary bar renders "本会话子智能体" (DESIGN.md:103); Codex renders "子智能体" (SubagentSummary.tsx:76)
            XCTAssertEqual(SubagentSummaryBarView.summaryHeaderTitle(provider: .claudeCode, totalCount: 1), "本会话子智能体")
            XCTAssertEqual(SubagentSummaryBarView.summaryHeaderTitle(provider: .codex, totalCount: 3), "子智能体")
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

            // Nested Codex child subagent detail summary visibility check
            let nestedCol = SubagentCollection(agents: [SubagentActivity(agentId: "grandchild-1", name: "Grandchild", status: "running", available: true)], counts: ["running": 1], complete: true)
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: nestedCol, isSupported: true))
        }
    }

    // MARK: - 5. SubagentSummary.test.tsx (5 cases)

    func testPorted_subagentSummary() throws {
        try runAsyncParityCheck {
            // 1) Hidden when 0 subagents and complete == true
            let emptyComplete = SubagentCollection(agents: [], counts: [:], complete: true)
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: emptyComplete, isSupported: true))
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .claudeCode, collection: emptyComplete, isSupported: true))

            // 2) Shown for Codex ONLY when capabilitiesReady && !isSupported && collection == nil (upgrade note);
            //    NEVER when isSupported == true and collection == nil (loading/error) or before capabilitiesReady!
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: nil, isSupported: false, capabilitiesReady: true))
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: nil, isSupported: false, capabilitiesReady: false))
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: nil, isSupported: true, capabilitiesReady: true))

            // 3) Hidden for Claude when collection is nil
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .claudeCode, collection: nil, isSupported: true))
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .claudeCode, collection: nil, isSupported: false))

            // 4) Shown when incomplete even if agents empty
            let incomplete = SubagentCollection(agents: [], counts: [:], complete: false)
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: incomplete, isSupported: true))

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

            // 6) Optional/extension field decode resilience across ThreadDetail, MessageBlock & sibling detail DTOs
            let minimalThreadDetail = try JSONDecoder().decode(
                ThreadDetail.self,
                from: Data(#"{"summary":{"id":"th-min","title":"Minimal","status":"Recent","message_count":1},"blocks":[{"id":"b1","role":"user","kind":"userMessage","text":"hi","user_message":{"id":"b1","text":"hi"}}]}"#.utf8)
            )
            XCTAssertEqual(minimalThreadDetail.messages, [])
            XCTAssertEqual(minimalThreadDetail.rawEventCount, 1)
            XCTAssertEqual(minimalThreadDetail.blocks.first?.questions, [])
            XCTAssertEqual(minimalThreadDetail.blocks.first?.answers, [])
            XCTAssertEqual(minimalThreadDetail.blocks.first?.userMessage?.attachments, [])

            let minimalClaudeDetail = try JSONDecoder().decode(
                ClaudeSessionDetail.self,
                from: Data(#"{"summary":{"id":"c1","sessionKey":"claude:c1","title":"Claude Min"}}"#.utf8)
            )
            XCTAssertEqual(minimalClaudeDetail.events, [])
            XCTAssertEqual(minimalClaudeDetail.totalEvents, 0)
            XCTAssertFalse(minimalClaudeDetail.hasMore)

            let minimalGrokDetail = try JSONDecoder().decode(
                GrokSessionDetail.self,
                from: Data(#"{"summary":{"id":"g1","title":"Grok Min"}}"#.utf8)
            )
            XCTAssertEqual(minimalGrokDetail.events, [])

            let minimalProviderDetail = try JSONDecoder().decode(
                ProviderSessionDetail.self,
                from: Data(#"{"summary":{"id":"p1","sessionKey":"p1","title":"Provider Min"}}"#.utf8)
            )
            XCTAssertEqual(minimalProviderDetail.events, [])
            XCTAssertEqual(minimalProviderDetail.totalEvents, 0)

            XCTAssertThrowsError(
                try JSONDecoder().decode(
                    ThreadDetail.self,
                    from: Data(#"{"summary":{"id":"th-bad","title":"Bad","status":"Recent","message_count":0},"blocks":"corrupted"}"#.utf8)
                )
            )
        }
    }

    // MARK: - 6. subagentCapabilities.test.ts (3 cases)

    func testPorted_subagentCapabilities() throws {
        try runAsyncParityCheck {
            let bootstrapCaps = RuntimeCapabilityMatrix.from(nil)
            XCTAssertFalse(bootstrapCaps.isReady)
            let decodedCaps = try JSONDecoder().decode(SystemCapabilitiesResponse.self, from: protocol2CapabilitiesFixture(remote: false))
            let liveCaps = RuntimeCapabilityMatrix.from(decodedCaps)
            XCTAssertTrue(liveCaps.isReady)
            XCTAssertFalse(liveCaps.updatePrune)
            XCTAssertFalse(SubagentSummaryBarView.shouldDisplay(provider: .claudeCode, collection: SubagentCollection(agents: [SubagentActivity(agentId: "a1", name: "W", status: "running")], counts: [:], complete: true), isSupported: false))
            XCTAssertTrue(SubagentSummaryBarView.shouldDisplay(provider: .codex, collection: SubagentCollection(agents: [SubagentActivity(agentId: "a1", name: "W", status: "running")], counts: [:], complete: true), isSupported: false, capabilitiesReady: true))

            // Remote machine switching: local (supported) -> handshake (isReady=false) -> legacy webd (thread_subagents=false) -> modern webd (thread_subagents=true, detail loading)
            let (capsWait, capsYield) = AsyncStream<Void>.makeStream()
            let (capsProceed, capsRelease) = AsyncStream<Void>.makeStream()
            let switchSuite = "com.lich13.agenthub.subcap.switch.\(UUID().uuidString)"
            let switchDefaults = UserDefaults(suiteName: switchSuite)!
            defer { switchDefaults.removePersistentDomain(forName: switchSuite) }
            let switchMock = MockRuntimeTransport { command, args in
                let obj = (try? JSONSerialization.jsonObject(with: Data(args.utf8)) as? [String: Any]) ?? [:]
                let req = obj["request"] as? [String: Any] ?? [:]
                let rev = req["revision"] as? Int ?? 0
                switch command {
                case "remote.get":
                    return Data(#"{"target":"local","revision":1,"configured":true}"#.utf8)
                case "system.capabilities":
                    return protocol2CapabilitiesFixture(remote: false)
                case "remote.select":
                    let id = req["machineId"] as? String ?? "m"
                    return try JSONSerialization.data(withJSONObject: [
                        "target": "remote", "configured": true, "revision": rev + 1,
                        "activeMachineId": id, "sshHost": id,
                    ])
                case "remote.invoke":
                    let innerCmd = req["command"] as? String
                    if innerCmd == "system.capabilities" {
                        if rev == 2 {
                            // Old server without thread_subagents
                            return Data(#"{"api_version":2,"host_surface":"linux_server_api","capabilities":{"thread_subagents":false}}"#.utf8)
                        }
                        capsYield.yield()
                        var it = capsProceed.makeAsyncIterator()
                        _ = await it.next()
                        return protocol2CapabilitiesFixture(remote: true)
                    }
                    return Data("{}".utf8)
                default:
                    return Data("{}".utf8)
                }
            }
            let switchStore = RuntimeConnectionStore(transport: switchMock, defaults: switchDefaults)
            await switchStore.bootstrap()
            XCTAssertTrue(switchStore.capabilities.isReady)
            XCTAssertTrue(switchStore.capabilities.threadSubagents)
            XCTAssertFalse(
                SubagentSummaryBarView.shouldDisplay(
                    provider: .codex,
                    collection: nil,
                    isSupported: switchStore.capabilities.threadSubagents,
                    capabilitiesReady: switchStore.capabilities.isReady
                ),
                "Modern local Codex with nil collection (loading) must NEVER render upgrade note"
            )

            try await switchStore.selectMachine(target: .remote, machineId: "legacy-webd")
            XCTAssertTrue(switchStore.capabilities.isReady)
            XCTAssertFalse(switchStore.capabilities.threadSubagents)
            XCTAssertTrue(
                SubagentSummaryBarView.shouldDisplay(
                    provider: .codex,
                    collection: nil,
                    isSupported: switchStore.capabilities.threadSubagents,
                    capabilitiesReady: switchStore.capabilities.isReady
                ),
                "Legacy remote webd with thread_subagents==false MUST render upgrade note"
            )

            let switchTask = Task { try await switchStore.selectMachine(target: .remote, machineId: "modern-webd") }
            var waitIt = capsWait.makeAsyncIterator()
            _ = await waitIt.next()
            XCTAssertFalse(switchStore.capabilities.isReady, "During SSH handshake capabilities.isReady must be false")
            XCTAssertFalse(
                SubagentSummaryBarView.shouldDisplay(
                    provider: .codex,
                    collection: nil,
                    isSupported: switchStore.capabilities.threadSubagents,
                    capabilitiesReady: switchStore.capabilities.isReady
                ),
                "During handshake (isReady==false) SubagentSummaryBarView must NEVER flash upgrade note"
            )
            capsRelease.yield()
            try await switchTask.value
            XCTAssertTrue(switchStore.capabilities.isReady)
            XCTAssertTrue(switchStore.capabilities.threadSubagents)
            XCTAssertFalse(
                SubagentSummaryBarView.shouldDisplay(
                    provider: .codex,
                    collection: nil,
                    isSupported: switchStore.capabilities.threadSubagents,
                    capabilitiesReady: switchStore.capabilities.isReady
                ),
                "Modern remote webd with nil collection (loading/error) must NEVER render upgrade note"
            )
        }
    }

    // MARK: - 7. subagentPolling.test.ts (5 cases)

    func testPorted_subagentPolling() throws {
        try runAsyncParityCheck {
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
            }
        }
    }

    // MARK: - 8. subagents.test.ts (15 cases)

    func testPorted_subagentsQuery() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { cmd, args in
                if cmd == "threads.subagentDetail" {
                    XCTAssertTrue(args.contains("\"rootThreadId\":\"root-1\""))
                    return Data("""
                    {"summary":{"id":"child-1","title":"Child","status":"Running","message_count":1},"messages":[],"blocks":[{"id":"cb-1","role":"assistant","kind":"message","text":"Child reply","questions":[],"answers":[]}],"raw_event_count":1,"total_blocks":1,"has_more_blocks":false}
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

            struct ClaudeSubArgs: Encodable { let sessionKey: String; let agentId: String; let limit: Int }
            let claudeDetail: ClaudeSessionDetail = try await store.rpc(
                "claude.detail",
                args: ClaudeSubArgs(sessionKey: "sk1", agentId: "cl-child", limit: 160)
            )
            XCTAssertEqual(claudeDetail.events.first?.text, "Claude child")

            let panel = SubagentPanelView(
                store: store,
                provider: .codex,
                rootSessionKey: "root-1",
                collection: nil,
                scheme: .light,
                workspace: .codex,
                onClose: {}
            )
            for (_, st) in ["creating", "running", "completed", "failed", "interrupted", "unknown"].enumerated() {
                XCTAssertFalse(panel.subagentStatusLabel(st).isEmpty)
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

            // 6 distinct subagent query/model contract cases
            let cursorReq = SubagentDetailRequest(rootThreadId: "r-1", agentId: "a-1", limit: 120, before: "b-40")
            let cursorJson = String(decoding: try JSONEncoder().encode(cursorReq), as: UTF8.self)
            XCTAssertTrue(cursorJson.contains("\"before\":\"b-40\""))

            let headReq = SubagentDetailRequest(rootThreadId: "r-1", agentId: "a-1", limit: 120, before: nil)
            let headJson = String(decoding: try JSONEncoder().encode(headReq), as: UTF8.self)
            XCTAssertFalse(headJson.contains("\"before\""))

            let colWithCounts = SubagentCollection(agents: [], counts: ["running": 2, "completed": 3], complete: true, warning: "部分子代理尚未归档")
            XCTAssertEqual(colWithCounts.counts.values.reduce(0, +), 5)
            XCTAssertEqual(colWithCounts.warning, "部分子代理尚未归档")

            let colFallback = SubagentCollection(agents: [SubagentActivity(agentId: "x", name: "W", status: "running")], counts: [:], complete: true)
            XCTAssertEqual(colFallback.agents.count, 1)

            let richAct = try JSONDecoder().decode(
                SubagentActivity.self,
                from: Data("{\"agentId\":\"ag-1\",\"name\":\"Verifier\",\"role\":\"audit\",\"status\":\"running\",\"available\":true,\"delegation\":\"Verify parity\"}".utf8)
            )
            XCTAssertEqual(richAct.role, "audit")
            XCTAssertEqual(richAct.delegation, "Verify parity")

            let stableSorted = SubagentPanelView.sortByStatusPriority([
                SubagentActivity(agentId: "r-a", name: "First Running", status: "running", available: true),
                SubagentActivity(agentId: "r-b", name: "Second Running", status: "running", available: true),
            ])
            XCTAssertEqual(stableSorted.map(\.agentId), ["r-a", "r-b"])
        }
    }

    // MARK: - 9. attachments.test.ts (5 cases)

    func testPorted_attachmentsQuery() throws {
        try runAsyncParityCheck {
            let png1x1Base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
            let transport = MockRuntimeTransport { cmd, args in
                XCTAssertEqual(cmd, "sessions.attachmentRead")
                struct Envelope: Decodable { let request: SessionAttachmentRequest }
                let request = try JSONDecoder().decode(Envelope.self, from: Data(args.utf8)).request
                XCTAssertEqual(request.rootThreadId, "root-9")
                return Data("{\"mimeType\":\"image/png\",\"base64\":\"\(png1x1Base64)\"}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.att.\(UUID().uuidString)")!)
            let mgr = AttachmentViewportManager()
            let req = SessionAttachmentRequest(provider: .codex, sessionKey: "child-1", messageId: "m-1", attachmentId: "att-1", rootThreadId: "root-9")
            let loaded = XCTestExpectation(description: "Attachment entered the viewport cache")
            mgr.onImageUpdated = { key in
                XCTAssertEqual(key, "k1")
                loaded.fulfill()
            }
            mgr.ensureLoaded(key: "k1", request: req, store: store)
            let completion = await XCTWaiter.fulfillment(of: [loaded], timeout: 5)
            XCTAssertEqual(completion, .completed)
            XCTAssertNotNil(mgr.image(for: "k1"))

            // Duplicate ensureLoaded while cached does not refetch
            let callsBefore = transport.recordedCalls.count
            mgr.ensureLoaded(key: "k1", request: req, store: store)
            XCTAssertEqual(transport.recordedCalls.count, callsBefore)

            mgr.evictOutside(visibleKeys: ["k1"])
            XCTAssertEqual(mgr.loadedCount, 1)

            mgr.evictOutside(visibleKeys: [])
            XCTAssertEqual(mgr.loadedCount, 0)

            XCTAssertEqual(AttachmentViewportManager.viewportMargin, 240)
        }
    }

    // MARK: - 10 & 11. api/claude.test.ts (3) & query/claude.test.ts (1)

    func testPorted_claudeApi() throws {
        try runAsyncParityCheck {
            let p1 = ClaudeDeletePreview(sessionKey: "sk-1", id: "id-1", title: "T", path: "/tmp/a.jsonl", paths: ["/tmp/a.jsonl", "/tmp/a/sub"], fingerprint: "fp", fileCount: 2, bytes: 2048, warnings: ["warn-1"])
            XCTAssertEqual(p1.paths, ["/tmp/a.jsonl", "/tmp/a/sub"])

            let diagData = Data("""
            {"ownership":"verified","reasonCodes":[],"processes":[{"pid":42,"role":"cli","registrationVerified":true,"mismatchedFields":[]}]}
            """.utf8)
            let diag = try JSONDecoder().decode(ClaudeOwnershipDiagnostics.self, from: diagData)
            XCTAssertEqual(diag.processes.first?.pid, 42)
        }
    }

    func testPorted_claudeQuery() throws {
        let cmd = HistoryAndResumeHelpers.claudeResumeCommand(cwd: "/Users/example/project", id: "claude-uuid-1")
        XCTAssertTrue(cmd.contains("claude --resume"))
        XCTAssertTrue(cmd.contains("claude-uuid-1"))
    }

    // MARK: - 12. ClaudeWorkspace.test.tsx (4 cases) — Unconditional canDelete & Delete Paths Modal

    func testPorted_claudeWorkspace() throws {
        try runAsyncParityCheck {
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

            let batchPreview = SessionBatchPreview(
                provider: .claudeCode,
                operation: .delete,
                items: [
                    SessionBatchItem(
                        sessionKey: "proj/1.jsonl",
                        id: "1",
                        title: "Claude Session",
                        paths: ["/Users/example/.claude/1.jsonl", "/Users/example/.claude/1/subagents"],
                        bytes: 4096,
                        allowed: true,
                        warnings: ["活动进程提示"]
                    )
                ]
            )
            let dialog = SessionBatchPreviewDialogContentView(
                preview: batchPreview,
                busy: false,
                onCancel: {},
                onConfirm: {}
            )
            let host = NSHostingView(rootView: dialog.frame(width: 480))
            host.layoutSubtreeIfNeeded()
            let minimalPreview = SessionBatchPreview(
                provider: .claudeCode,
                operation: .delete,
                items: [
                    SessionBatchItem(
                        sessionKey: "proj/1.jsonl",
                        id: "1",
                        title: "Claude Session",
                        paths: [],
                        bytes: 0,
                        allowed: true
                    )
                ]
            )
            let minimalDialog = SessionBatchPreviewDialogContentView(
                preview: minimalPreview,
                busy: false,
                onCancel: {},
                onConfirm: {}
            )
            let minimalHost = NSHostingView(rootView: minimalDialog.frame(width: 480))
            minimalHost.layoutSubtreeIfNeeded()
            XCTAssertEqual(batchPreview.items[0].paths, ["/Users/example/.claude/1.jsonl", "/Users/example/.claude/1/subagents"])
            XCTAssertGreaterThan(host.fittingSize.height, minimalHost.fittingSize.height + 20, "Monospaced target paths and warnings must expand unified batch delete dialog layout height")
            XCTAssertEqual(batchPreview.items[0].warnings, ["活动进程提示"])
        }
    }

    // MARK: - 13 & 14. ArchivableProviderWorkspace.test.tsx (5) & GrokBotWorkspace.test.tsx (3)

    func testPorted_archivableProviderWorkspace() throws {
        try runAsyncParityCheck {
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

            for (prov, id) in [(SessionProvider.antigravity, "ag"), (.dsh, "dsh")] {
                let activeMenu = ProviderMenuMatrixBuilder.buildActions(
                    provider: prov, isHeaderMenu: false, isLocalMachine: true,
                    id: id, path: "/tmp/p", cwd: "/tmp", archived: false,
                    canRename: true, canArchive: true, canDelete: true, busy: false,
                    latestReplyText: nil, onRename: {}, onToggleArchive: {},
                    onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
                )
                XCTAssertTrue(activeMenu.contains(where: { $0.label == "归档" }))

                let archivedHeaderMenu = ProviderMenuMatrixBuilder.buildActions(
                    provider: prov, isHeaderMenu: true, isLocalMachine: true,
                    id: id, path: "/tmp/p", cwd: "/tmp", archived: true,
                    canRename: true, canArchive: true, canDelete: true, busy: false,
                    latestReplyText: "ans", onRename: {}, onToggleArchive: {},
                    onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
                )
                XCTAssertTrue(archivedHeaderMenu.contains(where: { $0.label == "取消归档" }))
            }
        }
    }

    func testPorted_grokBotWorkspace() throws {
        let menuWithPath = ProviderMenuMatrixBuilder.buildActions(
            provider: .grokbot, isHeaderMenu: false, isLocalMachine: true,
            id: "gb-1", path: "/Users/example/cache.json", cwd: "",
            archived: false, canRename: false, canArchive: false, canDelete: false, busy: false,
            latestReplyText: nil, onRename: nil, onToggleArchive: nil,
            onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: nil
        )
        XCTAssertEqual(menuWithPath.map(\.label), ["复制 ID", "复制路径", "在 Finder 中显示"])
        XCTAssertFalse(menuWithPath[1].disabled)

        let menuRemote = ProviderMenuMatrixBuilder.buildActions(
            provider: .grokbot, isHeaderMenu: false, isLocalMachine: false,
            id: "gb-1", path: "/remote/cache.json", cwd: "",
            archived: false, canRename: false, canArchive: false, canDelete: false, busy: false,
            latestReplyText: nil, onRename: nil, onToggleArchive: nil,
            onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: nil
        )
        XCTAssertTrue(menuRemote[2].disabled, "Finder reveal disabled on remote target")

        let gbHeader = ProviderMenuMatrixBuilder.buildActions(
            provider: .grokbot, isHeaderMenu: true, isLocalMachine: true,
            id: "gb-1", path: "", cwd: "",
            archived: false, canRename: false, canArchive: false, canDelete: false, busy: false,
            latestReplyText: nil, onRename: nil, onToggleArchive: nil,
            onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: nil
        )
        XCTAssertEqual(gbHeader.first?.label, "复制答复")
        XCTAssertEqual(gbHeader.first?.disabled, true)
    }

    // MARK: - 15. RenameableSession.test.tsx (18 cases) — Row Visuals, Multi-Select, RunningIndicator(13) & Ctrl+Click NSMenu

    func testPorted_renameableSession() throws {
        try runAsyncParityCheck {
            var triggeredMenuIds: [String] = []
            let rowActions = [
                SessionMenuAction(id: "rename", label: "改名", icon: .pencil, run: { triggeredMenuIds.append("rename") }),
                SessionMenuAction(id: "copy-id", label: "复制 ID", icon: .copy, run: { triggeredMenuIds.append("copy-id") }),
                SessionMenuAction(id: "reveal", label: "在 Finder 中显示", icon: .folderOpen, disabled: true, run: { triggeredMenuIds.append("reveal") }),
                SessionMenuAction(id: "delete", label: "永久删除", icon: .trash2, danger: true, run: { triggeredMenuIds.append("delete") }),
            ]

            // 1) Action descriptors preserve labels, enabled state, danger styling, and invoke action!
            XCTAssertEqual(rowActions.map(\.label), ["改名", "复制 ID", "在 Finder 中显示", "永久删除"])
            XCTAssertFalse(rowActions[0].disabled)
            XCTAssertTrue(rowActions[2].disabled)
            XCTAssertTrue(rowActions[3].danger)
            rowActions[1].run()
            XCTAssertEqual(triggeredMenuIds, ["copy-id"])

            // 2) Ctrl+Click does NOT switch detail or mutate selection
            var part = PartitionedSessionSelectionState(activeScope: "s#a", archivedScope: "s#z")
            _ = part.click(partition: .active, visible: ["s1", "s2"], key: "s1", click: SessionClick(detail: 1, metaKey: true))
            let shouldOpen = part.click(partition: .active, visible: ["s1", "s2"], key: "s2", click: SessionClick(detail: 1, ctrlKey: true), macPlatform: true)
            XCTAssertFalse(shouldOpen)
            XCTAssertEqual(part.active.keys, ["s1"])

            // 3) Keyboard F2 rename trigger vs modifier rejection
            XCTAssertTrue(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: []))
            XCTAssertFalse(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: [.command]))
            XCTAssertFalse(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: [.control]))
            XCTAssertFalse(ProviderMenuMatrixBuilder.shouldTriggerRenameKey(keyCode: 120, charactersIgnoringModifiers: "\u{F705}", modifierFlags: [.option]))

            // 4) RunningIndicator default 13x13pt + multi-selected row styling (--as + 1.5pt --a) vs single selected (--sl + 2.5pt bar)
            let spinner = RunningIndicator()
            XCTAssertEqual(spinner.size, 13)

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
                onRename: { _ in }
            ).environment(\.agentHubPalette, palette)

            let hSingle = NSHostingView(rootView: rowSingle.frame(width: 272))
            hSingle.layoutSubtreeIfNeeded()
            let hMulti = NSHostingView(rootView: rowMulti.frame(width: 272))
            hMulti.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(hSingle.fittingSize.height, 48)
            XCTAssertGreaterThan(hMulti.fittingSize.height, 40)

            // Verify storage badge resolves plain text color (not capsule bg) across 4 tones
            let mib: Int64 = 1024 * 1024
            let toneCases: [(Int64?, RGBAColor)] = [
                (nil, palette.textMuted),
                (10 * mib, palette.okFg),
                (150 * mib, palette.warnFg),
                (350 * mib, palette.errFg),
            ]
            for (bytes, expectedColor) in toneCases {
                let sz = bytes.map { SessionStorageSize(bytes: $0, scope: "file", status: "complete") }
                XCTAssertEqual(SessionStorageBadgeView.resolvedColor(for: sz, palette: palette), expectedColor)
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

            XCTAssertTrue("   \n ".trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            let busyMenu = ProviderMenuMatrixBuilder.buildActions(
                provider: .codex, isHeaderMenu: false, isLocalMachine: true,
                id: "cx-1", path: "/tmp/cx.jsonl", cwd: "/tmp",
                archived: false, canRename: true, canArchive: true, canDelete: true, busy: true,
                latestReplyText: nil, onRename: {}, onToggleArchive: {},
                onCopy: { _, _ in }, onRevealInFinder: { _ in }, onDelete: {}
            )
            XCTAssertEqual(busyMenu.first(where: { $0.id == "rename" })?.disabled, true)

            var selGuard = PartitionedSessionSelectionState(activeScope: "s#a")
            let dblClickOpens = selGuard.click(partition: .active, visible: ["s1", "s2"], key: "s1", click: SessionClick(detail: 2))
            XCTAssertTrue(dblClickOpens)
            XCTAssertFalse(selGuard.active.selecting)

            _ = selGuard.click(partition: .active, visible: ["s1", "s2"], key: "s1", click: SessionClick(detail: 1, metaKey: true))
            XCTAssertTrue(selGuard.active.selecting, "Entering multi-selection cancels single-session rename mode")
        }
    }

    // MARK: - 16 & 17. SessionBatchControls.test.tsx (2) & query/sessions.test.tsx (21) — 100-Item Chunked Partial Failure

    func testPorted_sessionBatchControls() throws {
        try runAsyncParityCheck {
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
                result: partialResult,
                onSelectAll: {},
                onClear: {},
                onPrepareOperation: { _ in }
            )
            let barHost = NSHostingView(rootView: barView.frame(width: 280))
            barHost.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(barHost.fittingSize.height, 90)
        }
    }

    func testPorted_sessionsQuery() throws {
        try runAsyncParityCheck {
            // 1) Verify hasSubagentTree rules across providers & operations
            XCTAssertTrue(SessionBatchChunkExecutor.hasSubagentTree(provider: .codex, operation: .delete))
            XCTAssertTrue(SessionBatchChunkExecutor.hasSubagentTree(provider: .antigravity, operation: .delete))
            XCTAssertTrue(SessionBatchChunkExecutor.hasSubagentTree(provider: .dsh, operation: .delete))
            XCTAssertFalse(SessionBatchChunkExecutor.hasSubagentTree(provider: .codex, operation: .archive))
            XCTAssertFalse(SessionBatchChunkExecutor.hasSubagentTree(provider: .claudeCode, operation: .delete))

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

            // 100..<200 marked failed with "请求中断，结果未确认"
            XCTAssertTrue(partial.items[100 ..< 200].allSatisfy { $0.status == "failed" && $0.message == "请求中断，结果未确认" })

            // 200..<250 marked failed with "前序请求中断，未执行"
            XCTAssertTrue(partial.items[200 ..< 250].allSatisfy { $0.status == "failed" && $0.message == "前序请求中断，未执行" })

            // Selection removes ONLY confirmed succeeded 0..<100, keeping 100..<250 selected!
            var partState = PartitionedSessionSelectionState(activeScope: "s#active")
            _ = partState.selectAll(activeVisible: keys250, archivedVisible: [], supportsArchive: true, archivedOpen: false)
            XCTAssertEqual(partState.active.keys.count, 250)
            partState.applyBatchResult(partial, partition: .active)
            XCTAssertEqual(partState.active.keys, Array(keys250[100 ..< 250]), "Failed/unconfirmed items MUST remain selected for retry!")
            XCTAssertTrue(partState.active.selecting)

            // 5) Chunk 0 immediate failure returns nil result and keeps ALL 250 items selected!
            let chunk0Fail = await SessionBatchChunkExecutor.execute(preview: preview250) { _ in
                throw NSError(domain: "Transport", code: -1, userInfo: [NSLocalizedDescriptionKey: "首包失败"])
            }
            XCTAssertNil(chunk0Fail.result)
            XCTAssertTrue(chunk0Fail.confirmedSucceededKeys.isEmpty)
            XCTAssertEqual(chunk0Fail.errorMessage, "首包失败")

            // 6) Full 3-chunk success clears all 250 keys and exits selecting mode
            let fullOk = await SessionBatchChunkExecutor.execute(preview: preview250) { req in
                SessionBatchResult(items: req.items.map { SessionBatchItemResult(sessionKey: $0.sessionKey, status: "succeeded") })
            }
            var fullSel = PartitionedSessionSelectionState(activeScope: "s#active")
            _ = fullSel.selectAll(activeVisible: keys250, archivedVisible: [], supportsArchive: true, archivedOpen: false)
            fullSel.applyBatchResult(try XCTUnwrap(fullOk.result), partition: .active)
            XCTAssertTrue(fullSel.active.keys.isEmpty)
            XCTAssertFalse(fullSel.active.selecting)

            // 7) deletedIds also count as confirmed succeeded keys (removing descendant rows from selection)
            let withDescendants = SessionBatchResult(items: [
                SessionBatchItemResult(sessionKey: "root-1", status: "succeeded", deletedIds: ["root-1", "desc-1", "desc-2"]),
                SessionBatchItemResult(sessionKey: "root-2", status: "failed", message: "占用中"),
            ])
            let confirmed = SessionBatchChunkExecutor.confirmedSucceededKeys(from: withDescendants)
            XCTAssertEqual(confirmed, ["root-1", "desc-1", "desc-2"])

            // 8-14) 7 distinct selection & batch edge cases
            let clearedAnchor = SessionSelectionEngine.applyBatchSucceeded(
                SessionSelectionState(scope: "s", selecting: true, keys: ["a", "b"], anchor: "a"),
                succeededKeys: ["a"]
            )
            XCTAssertEqual(clearedAnchor.keys, ["b"])
            XCTAssertNil(clearedAnchor.anchor, "Succeeded anchor must be cleared")

            let keptAnchor = SessionSelectionEngine.applyBatchSucceeded(
                SessionSelectionState(scope: "s", selecting: true, keys: ["a", "b"], anchor: "a"),
                succeededKeys: ["b"]
            )
            XCTAssertEqual(keptAnchor.keys, ["a"])
            XCTAssertEqual(keptAnchor.anchor, "a", "Unsucceeded anchor must be preserved")

            var partIso = PartitionedSessionSelectionState(activeScope: "s#a", archivedScope: "s#z")
            _ = partIso.click(partition: .archived, visible: ["z1", "z2"], key: "z1", click: SessionClick(detail: 1, metaKey: true))
            _ = partIso.click(partition: .archived, visible: ["z1", "z2"], key: "z2", click: SessionClick(detail: 1, metaKey: true))
            partIso.applyBatchResult(SessionBatchResult(items: [SessionBatchItemResult(sessionKey: "z1", status: "succeeded")]), partition: .archived)
            XCTAssertEqual(partIso.archived.keys, ["z2"])
            XCTAssertTrue(partIso.active.keys.isEmpty)

            let preview80Rec = ChunkRecorder()
            _ = try await SessionBatchChunkExecutor.preview(provider: .codex, operation: .restore, sessionKeys: Array(keys250.prefix(80))) { req in
                preview80Rec.sizes.append(req.sessionKeys.count)
                return SessionBatchPreview(provider: .codex, operation: .restore, items: [])
            }
            XCTAssertEqual(preview80Rec.sizes, [80])

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
        }
    }

    @MainActor
    func testThreadMessageStoreResetTimelineEpochAndGfmNonCachingOnParseFailure() async throws {
        // 1. ThreadMessageStore clears prepended blocks when changes[].reset == true or totalBlocks shrinks
        let ts = ThreadMessageStore()
        let summary = ThreadSummary(id: "th-reset", title: "Reset", status: "Recent", messageCount: 4)
        let initialDetail = ThreadDetail(
            summary: summary,
            blocks: [
                MessageBlock(id: "b3", role: "user", kind: "userMessage", text: "Q3"),
                MessageBlock(id: "b4", role: "assistant", kind: "agentMessage", text: "A4"),
            ],
            history: HistoryMetadata(epoch: "e1", revision: "r1"),
            totalBlocks: 4,
            hasMoreBlocks: true,
            beforeCursor: "c-b3"
        )
        _ = ts.applyDetail(initialDetail)
        _ = ts.applyBlockPage(
            ThreadBlockPage(
                threadId: "th-reset",
                blocks: [
                    MessageBlock(id: "b1", role: "user", kind: "userMessage", text: "Q1"),
                    MessageBlock(id: "b2", role: "assistant", kind: "agentMessage", text: "A2"),
                ],
                totalBlocks: 4,
                hasMoreBlocks: false,
                beforeCursor: nil,
                history: HistoryMetadata(epoch: "e1", revision: "r1")
            ),
            expectedCursor: "c-b3"
        )
        XCTAssertEqual(ts.slot(for: "th-reset").blocks.map(\.id), ["b1", "b2", "b3", "b4"])

        // Shrinking totalBlocks (e.g. truncated/rewritten rollout with same epoch) drops stale older blocks
        let shrunkDetail = ThreadDetail(
            summary: summary,
            blocks: [MessageBlock(id: "b-new", role: "user", kind: "userMessage", text: "Fresh")],
            history: HistoryMetadata(epoch: "e1", revision: "r2"),
            totalBlocks: 1,
            hasMoreBlocks: false,
            beforeCursor: nil
        )
        let afterShrink = ts.applyDetail(shrunkDetail)
        XCTAssertEqual(afterShrink.blocks.map { $0.id }, ["b-new"])

        // Stalled older cursor halts hasMoreBlocks to prevent infinite 200pt scroll loops
        _ = ts.applyDetail(initialDetail)
        let stalledSlot = ts.applyBlockPage(
            ThreadBlockPage(
                threadId: "th-reset",
                blocks: [MessageBlock(id: "b3", role: "user", kind: "userMessage", text: "Q3")],
                totalBlocks: 4,
                hasMoreBlocks: true,
                beforeCursor: "c-b3",
                history: HistoryMetadata(epoch: "e1", revision: "r1")
            ),
            expectedCursor: "c-b3"
        )
        XCTAssertFalse(stalledSlot.hasMoreBlocks)
        XCTAssertNil(stalledSlot.beforeCursor)

        // 2. TimelineWindowController halts stalled older cursor & discards mismatched epoch pages
        let tw = TimelineWindowController<MessageBlock>(idOf: \.id)
        await tw.jump(
            to: "b3",
            in: "local:codex:th-reset",
            currentHistory: HistoryMetadata(epoch: "e1", revision: "r1"),
            matchesTarget: { $0.id == $1 }
        ) { _ in
            TimelineWindowPage(
                items: [MessageBlock(id: "b3", role: "user", kind: "userMessage", text: "Q3")],
                before: "w:b3",
                after: "n:b3",
                hasMore: true,
                history: HistoryMetadata(epoch: "e1", revision: "r1"),
                firstNativeId: "b3",
                lastNativeId: "b3",
                nativeIds: ["b3"]
            )
        }
        XCTAssertTrue(tw.hasMoreOlder)
        await tw.loadOlder { _ in
            TimelineWindowPage(
                items: [],
                before: "w:b3",
                after: nil,
                hasMore: true,
                history: HistoryMetadata(epoch: "e1", revision: "r1"),
                firstNativeId: nil,
                lastNativeId: nil,
                nativeIds: []
            )
        }
        XCTAssertFalse(tw.hasMoreOlder, "Empty older window page must clear hasMoreOlder")

        // 3. GfmLayoutCache falls back to plain paragraph on empty FFI Data() without poisoning cache
        final class ToggleMarkdownTransport: RuntimeTransport, @unchecked Sendable {
            var failParse = true
            func invokeRawJSON(command: String, argsJSON: String) async throws -> Data { Data("{}".utf8) }
            func parseGfmMarkdown(_ markdown: String) -> Data {
                if failParse { return Data() }
                return NativeMarkdown.parse(markdown)
            }
            func shutdown() {}
        }
        let toggleTransport = ToggleMarkdownTransport()
        let cache = GfmLayoutCache()
        let mdSample = "## Uncached Fallback Heading \(UUID().uuidString)\n\nWith `inline` code."
        let fallbackSegs = cache.getOrRender(
            markdown: mdSample,
            foldInstructions: false,
            scheme: .light,
            workspace: .codex,
            transport: toggleTransport
        )
        XCTAssertEqual(fallbackSegs.count, 1, "Failed parse must still render visible plain paragraph fallback")
        XCTAssertEqual(cache.totalBytes, 0, "Failed parse fallback must not be cached")
        toggleTransport.failParse = false
        let recoveredSegs = cache.getOrRender(
            markdown: mdSample,
            foldInstructions: false,
            scheme: .light,
            workspace: .codex,
            transport: toggleTransport
        )
        if case let .prose(attr) = recoveredSegs[0] {
            XCTAssertFalse(attr.string.contains("## "), "Recovered parse must not be poisoned by cached fallback")
        } else {
            XCTFail("Expected prose segment after recovery")
        }
        XCTAssertGreaterThan(cache.totalBytes, 0, "Recovered parse must populate GfmLayoutCache")
    }
}
