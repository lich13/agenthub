@testable import AgentHubCore
import Foundation
import XCTest

final class GoldenVectorsAndDomainParityTests: XCTestCase {
    private var repoRoot: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }

    // MARK: - 1. Shared Golden Vector #1: visible_markdown.json

    func testSharedGoldenVectorsVisibleMarkdown() throws {
        struct VectorFile: Decodable {
            struct CleanCase: Decodable { let name: String; let input: String; let expected: String }
            struct ActivityCase: Decodable { let inputs: [String]; let expected: Bool }
            struct SegmentCase: Decodable { let name: String; let input: String; let expected: [InstructionSegment] }
            let cleanCases: [CleanCase]
            let instructionActivityCases: [ActivityCase]
            let instructionSegmentCases: [SegmentCase]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/visible_markdown.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))

        for c in vectors.cleanCases {
            XCTAssertEqual(VisibleMarkdownEngine.visibleMarkdown(c.input), c.expected, "cleanCase: \(c.name)")
        }
        for (idx, c) in vectors.instructionActivityCases.enumerated() {
            XCTAssertEqual(
                VisibleMarkdownEngine.isInstructionFileActivity(c.inputs.map { Optional($0) }),
                c.expected,
                "activityCase #\(idx)"
            )
        }
        for c in vectors.instructionSegmentCases {
            XCTAssertEqual(VisibleMarkdownEngine.instructionSegments(c.input), c.expected, "segmentCase: \(c.name)")
        }
    }

    // MARK: - 3. Shared Golden Vector #2: user_context.json

    func testSharedGoldenVectorsUserContext() throws {
        struct VectorFile: Decodable {
            struct CaseItem: Decodable {
                let name: String
                let input: String
                let expected: UserMessageProjection
            }
            let cases: [CaseItem]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/user_context.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        for c in vectors.cases {
            let actual = UserContextEngine.projectUserMessage(c.input)
            XCTAssertEqual(actual, c.expected, "userContext case: \(c.name)")
        }
    }

    // MARK: - 4. Shared Golden Vector #3: session_storage.json

    func testSharedGoldenVectorsSessionStorage() throws {
        struct VectorFile: Decodable {
            struct CaseItem: Decodable {
                let name: String
                let input: SessionStorageSize?
                let expected: FormattedSessionStorage
            }
            let cases: [CaseItem]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/session_storage.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        for c in vectors.cases {
            XCTAssertEqual(SessionStorageFormatter.format(c.input), c.expected, "sessionStorage case: \(c.name)")
        }
    }

    // MARK: - 5. Shared Golden Vector #4: session_selection.json

    func testSharedGoldenVectorsSessionSelection() throws {
        struct VectorFile: Decodable {
            struct StepItem: Decodable {
                let name: String
                let initial: SessionSelectionState
                let activeKey: String?
                let key: String
                let click: SessionClick
                let expected: SessionSelectionState
            }
            let visible: [String]
            let steps: [StepItem]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/session_selection.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        for step in vectors.steps {
            let actual = SessionSelectionEngine.click(
                step.initial,
                visible: vectors.visible,
                key: step.key,
                click: step.click,
                activeKey: step.activeKey,
                macPlatform: true
            )
            XCTAssertEqual(actual, step.expected, "sessionSelection step: \(step.name)")
        }
    }

    // MARK: - 7. Domain Compaction, 2000-Key FIFO Disclosure, 120s Optimistic Title & Plan Export

    @MainActor
    func testHistoryCompaction60And3Disclosure2000FIFOAndOptimistic120sTitle() {
        // 65 chat messages + 5 plan blocks -> compacts to 60 chat + 3 plan + 2 collapse markers
        var blocks: [MessageBlock] = []
        for i in 0 ..< 5 {
            blocks.append(MessageBlock(id: "plan-\(i)", role: "assistant", kind: "plan", status: "completed", text: "# Plan \(i)"))
        }
        for i in 0 ..< 65 {
            blocks.append(MessageBlock(id: "msg-\(i)", role: i.isMultiple(of: 2) ? "user" : "assistant", kind: "message", text: "Hello \(i)"))
        }
        let compacted = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: false)
        XCTAssertTrue(compacted.contains { $0.id == "action-history-collapsed" })
        XCTAssertTrue(compacted.contains { $0.id == "chat-history-collapsed" })
        let uncompacted = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: true)
        XCTAssertEqual(uncompacted.count, 70)

        // ActivityDisclosureStore 2000-key FIFO cap
        let disc = ActivityDisclosureStore()
        for i in 0 ..< 2005 {
            disc.setOpen(scope: "s", stateKey: "k-\(i)", isOpen: true)
        }
        XCTAssertEqual(disc.count, 2000)
        XCTAssertFalse(disc.isOpen(scope: "s", stateKey: "k-0", initiallyOpen: false), "Oldest key k-0 should be evicted")
        XCTAssertTrue(disc.isOpen(scope: "s", stateKey: "k-2004", initiallyOpen: false))

        // 120s optimistic Codex title cache
        let store = ThreadMessageStore()
        let t0 = Date(timeIntervalSince1970: 1_700_000_000)
        store.recordOptimisticTitle(threadId: "th-1", title: "新标题", now: t0)
        XCTAssertEqual(store.effectiveTitle(threadId: "th-1", serverTitle: "旧标题", now: t0.addingTimeInterval(60)), "新标题")
        XCTAssertEqual(store.effectiveTitle(threadId: "th-1", serverTitle: "旧标题", now: t0.addingTimeInterval(121)), "旧标题")

        // Plan filename sanitization & Windows reserved stem
        XCTAssertEqual(FileLinksAndPlanExport.planFilename(markdown: "# CON\nBody", fallbackTitle: ""), "_CON.md")
        XCTAssertEqual(
            FileLinksAndPlanExport.fileLinkTarget("src/main.rs:42:10", cwd: "/Users/example/repo"),
            FileLinkTarget(path: "/Users/example/repo/src/main.rs", resolved: true)
        )
    }

    // MARK: - 8. PagedEventHistoryStore Reverse Pagination + Poll Retention & 60/3 Search Single-Item Reveal

    @MainActor
    func testPagedEventHistoryPrependRetentionAndSearchSingleTargetReveal() throws {
        // 1) 60/3 single-item reveal (`searchHistoryReveal.test.ts` parity): reveals ONLY `msg-2` without unfolding all 65 chat blocks!
        var blocks: [MessageBlock] = []
        for i in 0 ..< 5 {
            blocks.append(MessageBlock(id: "plan-\(i)", role: "assistant", kind: "plan", status: "completed", text: "# Plan \(i)"))
        }
        for i in 0 ..< 65 {
            blocks.append(MessageBlock(id: "msg-\(i)", role: i.isMultiple(of: 2) ? "user" : "assistant", kind: "message", text: "Hello \(i)"))
        }
        let singleRevealed = ConversationViewModel.compactConversationBlocks(
            blocks,
            showAllHistory: false,
            revealedBlockId: "msg-2"
        )
        XCTAssertTrue(singleRevealed.contains { $0.id == "msg-2" }, "Target older block msg-2 must be revealed")
        XCTAssertFalse(singleRevealed.contains { $0.id == "msg-0" }, "Unrelated folded older blocks must remain compacted")
        XCTAssertTrue(singleRevealed.contains { $0.id == "chat-history-collapsed" }, "Collapse banner must remain with decremented count")

        // 2) PagedEventHistoryStore (Claude / Grok / Antigravity / DSH / GrokBot parity):
        // Load head page [10..19], prepend older page [0..9], then poll updated head [10..20] -> older [0..9] MUST be retained!
        struct MockPagedEvent: Equatable, Sendable {
            var id: String
            var text: String
        }
        let historyStore = PagedEventHistoryStore<MockPagedEvent> { ev, _ in ev.id }
        let head10to19 = (10 ..< 20).map { i in
            MockPagedEvent(id: "ev-\(i)", text: "Body \(i)")
        }
        let epochMeta = try JSONDecoder().decode(HistoryMetadata.self, from: Data("{\"epoch\":\"e1\",\"revision\":\"r1\",\"reset\":false}".utf8))
        let slot1 = historyStore.applyHeadPoll(
            key: "local::claude:s1",
            headEvents: head10to19,
            history: epochMeta,
            totalEvents: 20,
            beforeCursor: "10",
            hasMore: true,
            timeline: nil
        )
        XCTAssertEqual(slot1.events.count, 10)
        XCTAssertTrue(slot1.hasMore)
        XCTAssertEqual(slot1.beforeCursor, "10")

        let older0to9 = (0 ..< 10).map { i in
            MockPagedEvent(id: "ev-\(i)", text: "Older \(i)")
        }
        let slot2 = historyStore.applyOlderPage(
            key: "local::claude:s1",
            expectedCursor: "10",
            olderEvents: older0to9,
            history: epochMeta,
            totalEvents: 20,
            beforeCursor: nil,
            hasMore: false
        )
        XCTAssertEqual(slot2.events.map(\.id), (0 ..< 20).map { "ev-\($0)" })
        XCTAssertFalse(slot2.hasMore)

        // Subsequent poll brings updated ev-19 and new ev-20: older ev-0..ev-9 MUST remain intact!
        var head10to20 = head10to19
        head10to20[9].text = "Body 19 updated"
        head10to20.append(MockPagedEvent(id: "ev-20", text: "New tail 20"))
        let slot3 = historyStore.applyHeadPoll(
            key: "local::claude:s1",
            headEvents: head10to20,
            history: epochMeta,
            totalEvents: 21,
            beforeCursor: "10",
            hasMore: true,
            timeline: nil
        )
        XCTAssertEqual(slot3.events.count, 21)
        XCTAssertEqual(slot3.events.first?.id, "ev-0")
        XCTAssertEqual(slot3.events[19].text, "Body 19 updated")
        XCTAssertEqual(slot3.events.last?.id, "ev-20")
        XCTAssertFalse(slot3.hasMore, "All 21 events are already in memory")

        // 3) SubagentPanelView status priority sorting: running/creating -> failed -> interrupted -> completed
        let unsorted = [
            SubagentActivity(agentId: "a1", name: "Done", status: "completed"),
            SubagentActivity(agentId: "a2", name: "Err", status: "failed"),
            SubagentActivity(agentId: "a3", name: "Active", status: "running"),
            SubagentActivity(agentId: "a4", name: "Stopped", status: "interrupted"),
        ]
        XCTAssertEqual(SubagentPanelView.sortByStatusPriority(unsorted).map(\.id), ["a3", "a2", "a4", "a1"])
    }
}
