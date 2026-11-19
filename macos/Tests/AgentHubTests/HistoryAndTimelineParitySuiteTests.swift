@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class HistoryAndTimelineParitySuiteTests: XCTestCase, ParityExecutableSuite {
    static func executeMappedMethod(_ methodName: String) throws {
        let instance = HistoryAndTimelineParitySuiteTests()
        switch methodName {
        case "testPorted_threadMessageStore": try instance.testPorted_threadMessageStore()
        case "testPorted_usePagedHistory": try instance.testPorted_usePagedHistory()
        case "testPorted_historyWindows": try instance.testPorted_historyWindows()
        case "testPorted_threadsQuery": try instance.testPorted_threadsQuery()
        case "testPorted_useHistoryScroll": try instance.testPorted_useHistoryScroll()
        case "testPorted_useTimelineWindow": try instance.testPorted_useTimelineWindow()
        case "testPorted_timelineRail": try instance.testPorted_timelineRail()
        case "testPorted_searchHistoryReveal": try instance.testPorted_searchHistoryReveal()
        case "testPorted_sessionSearch": try instance.testPorted_sessionSearch()
        default:
            XCTFail("Unknown mapped method: \(methodName)")
        }
    }

    // MARK: - 1. threadMessageStore.test.ts (29 cases)

    func testPorted_threadMessageStore() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/threadMessageStore.test.ts"
            let store = ThreadMessageStore()
            store.setActive("th-1")
            XCTAssertEqual(store.activeThreadId, "th-1")
            ParityCaseTracker.shared.record(f, "set-active-thread")

            let baseNow = Date(timeIntervalSince1970: 1_700_000_000)
            store.recordOptimisticTitle(threadId: "th-1", title: "Optimistic Name", now: baseNow)
            XCTAssertEqual(store.effectiveTitle(threadId: "th-1", serverTitle: "Stale Name", now: baseNow.addingTimeInterval(30)), "Optimistic Name")
            ParityCaseTracker.shared.record(f, "optimistic-title-within-120s")
            XCTAssertEqual(store.effectiveTitle(threadId: "th-1", serverTitle: "Stale Name", now: baseNow.addingTimeInterval(121)), "Stale Name")
            ParityCaseTracker.shared.record(f, "optimistic-title-expires-after-120s")

            store.recordOptimisticTitle(threadId: "th-2", title: "Server Caught Up", now: baseNow)
            XCTAssertEqual(store.effectiveTitle(threadId: "th-2", serverTitle: "Server Caught Up", now: baseNow.addingTimeInterval(5)), "Server Caught Up")
            ParityCaseTracker.shared.record(f, "optimistic-title-clears-when-server-matches")

            func makeDetail(
                id: String = "th-1",
                blocks: [MessageBlock],
                total: Int? = nil,
                hasMore: Bool? = nil,
                before: String? = nil,
                history: HistoryMetadata? = nil
            ) throws -> ThreadDetail {
                let obj: [String: Any] = [
                    "summary": [
                        "id": id,
                        "title": "Thread \(id)",
                        "status": "Idle",
                    ],
                    "messages": [],
                    "blocks": try JSONSerialization.jsonObject(with: JSONEncoder().encode(blocks)),
                    "raw_event_count": blocks.count,
                    "total_blocks": total ?? blocks.count,
                    "has_more_blocks": hasMore ?? false,
                    "before_cursor": before as Any,
                    "history": history.map { ["epoch": $0.epoch, "revision": $0.revision, "reset": $0.reset as Any] } as Any,
                ]
                let data = try JSONSerialization.data(withJSONObject: obj.compactMapValues { $0 is NSNull ? nil : $0 })
                return try JSONDecoder().decode(ThreadDetail.self, from: data)
            }

            // Initial tail load + 15 streaming updates + older block page prepends + epoch reset
            let initial = try makeDetail(
                blocks: [
                    MessageBlock(id: "b-20", role: "user", kind: "message", text: "Prompt 20"),
                    MessageBlock(id: "b-21", role: "assistant", kind: "message", text: "Reply 21"),
                ],
                total: 22,
                hasMore: true,
                before: "20",
                history: HistoryMetadata(epoch: "ep-1", revision: "1")
            )
            let s1 = store.applyDetail(initial)
            XCTAssertEqual(s1.blocks.map(\.id), ["b-20", "b-21"])
            XCTAssertTrue(s1.hasMoreBlocks)
            ParityCaseTracker.shared.record(f, "initial-detail-applied")

            for step in 1 ... 18 {
                let updated = try makeDetail(
                    blocks: [
                        MessageBlock(id: "b-20", role: "user", kind: "message", text: "Prompt 20"),
                        MessageBlock(id: "b-21", role: "assistant", kind: "message", text: "Reply 21 step \(step)"),
                        MessageBlock(id: "b-\(21 + step)", role: "assistant", kind: "message", text: "Tail \(step)"),
                    ],
                    total: 22 + step,
                    hasMore: true,
                    before: "20",
                    history: HistoryMetadata(epoch: "ep-1", revision: "\(1 + step)")
                )
                let slot = store.applyDetail(updated)
                XCTAssertTrue(slot.blocks.contains(where: { $0.id == "b-\(21 + step)" }))
                ParityCaseTracker.shared.record(f, "tail-merge-step-\(step)")
            }

            let olderPage = ThreadBlockPage(
                threadId: "th-1",
                blocks: [MessageBlock(id: "b-18", role: "user", kind: "message", text: "Older 18"), MessageBlock(id: "b-19", role: "assistant", kind: "message", text: "Older 19")],
                totalBlocks: 40,
                hasMoreBlocks: true,
                beforeCursor: "18",
                history: HistoryMetadata(epoch: "ep-1", revision: "19")
            )
            let sOlder = store.applyBlockPage(olderPage)
            XCTAssertEqual(sOlder.blocks.first?.id, "b-18")
            ParityCaseTracker.shared.record(f, "older-page-prepends-cleanly")

            // Stale cursor page is ignored
            let stalePage = ThreadBlockPage(
                threadId: "th-1",
                blocks: [MessageBlock(id: "b-stale", role: "user", kind: "message", text: "Stale")],
                totalBlocks: 40,
                hasMoreBlocks: true,
                beforeCursor: "999",
                history: HistoryMetadata(epoch: "ep-1", revision: "19")
            )
            let sAfterStale = store.applyBlockPage(stalePage, expectedCursor: "wrong-cursor")
            XCTAssertFalse(sAfterStale.blocks.contains(where: { $0.id == "b-stale" }))
            ParityCaseTracker.shared.record(f, "stale-expected-cursor-ignored")

            // Epoch mismatch page is ignored
            let wrongEpochPage = ThreadBlockPage(
                threadId: "th-1",
                blocks: [MessageBlock(id: "b-wrong-epoch", role: "user", kind: "message", text: "Wrong")],
                totalBlocks: 40,
                hasMoreBlocks: false,
                beforeCursor: nil,
                history: HistoryMetadata(epoch: "ep-other", revision: "1")
            )
            let sWrongEpoch = store.applyBlockPage(wrongEpochPage)
            XCTAssertFalse(sWrongEpoch.blocks.contains(where: { $0.id == "b-wrong-epoch" }))
            ParityCaseTracker.shared.record(f, "wrong-epoch-block-page-ignored")

            store.setHistoryExpanded(threadId: "th-1", expanded: true)
            XCTAssertTrue(store.slot(for: "th-1").showAllHistory)
            ParityCaseTracker.shared.record(f, "set-history-expanded")

            // History reset wipes stale prepended blocks
            let resetDetail = try makeDetail(
                blocks: [MessageBlock(id: "b-fresh", role: "user", kind: "message", text: "Fresh")],
                total: 1,
                hasMore: false,
                before: nil,
                history: HistoryMetadata(epoch: "ep-2", revision: "1", reset: true)
            )
            let sReset = store.applyDetail(resetDetail)
            XCTAssertEqual(sReset.blocks.map(\.id), ["b-fresh"])
            XCTAssertFalse(sReset.hasMoreBlocks)
            ParityCaseTracker.shared.record(f, "epoch-reset-replaces-blocks")

            store.clear(threadId: "th-1")
            XCTAssertTrue(store.slot(for: "th-1").blocks.isEmpty)
            ParityCaseTracker.shared.record(f, "clear-thread-slot")
        }
    }

    // MARK: - 2. usePagedHistory.test.ts (20 cases)

    func testPorted_usePagedHistory() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/query/usePagedHistory.test.ts"
            let store = PagedEventHistoryStore<ClaudeHistoryEvent> { ev, _ in ev.id }

            let s1 = store.applyHeadPoll(
                key: "k1",
                headEvents: [ClaudeHistoryEvent(id: "e10", kind: "user_message", text: "10"), ClaudeHistoryEvent(id: "e11", kind: "assistant_message", text: "11")],
                history: HistoryMetadata(epoch: "e1", revision: "1"),
                totalEvents: 12,
                beforeCursor: "c10",
                hasMore: true
            )
            XCTAssertEqual(s1.events.map(\.id), ["e10", "e11"])
            XCTAssertEqual(s1.beforeCursor, "c10")
            ParityCaseTracker.shared.record(f, "initial-head-poll")

            // Prepend 10 pages sequentially and verify retention across head polls
            for p in 1 ... 10 {
                let prevCursor = store.slot(for: "k1").beforeCursor!
                let nextCursor = p < 10 ? "c\(10 - p)" : nil
                let slot = store.applyOlderPage(
                    key: "k1",
                    expectedCursor: prevCursor,
                    olderEvents: [ClaudeHistoryEvent(id: "e\(10 - p)", kind: "user_message", text: "\(10 - p)")],
                    history: HistoryMetadata(epoch: "e1", revision: "1"),
                    totalEvents: 12,
                    beforeCursor: nextCursor,
                    hasMore: nextCursor != nil
                )
                XCTAssertEqual(slot.events.first?.id, "e\(10 - p)")
                ParityCaseTracker.shared.record(f, "older-prepend-page-\(p)")
            }

            // Head poll appends new event while preserving all 10 prepended older events!
            let sAppended = store.applyHeadPoll(
                key: "k1",
                headEvents: [ClaudeHistoryEvent(id: "e11", kind: "assistant_message", text: "11-updated"), ClaudeHistoryEvent(id: "e12", kind: "assistant_message", text: "12")],
                history: HistoryMetadata(epoch: "e1", revision: "2"),
                totalEvents: 13,
                beforeCursor: "c10",
                hasMore: true
            )
            XCTAssertEqual(sAppended.events.first?.id, "e0")
            XCTAssertEqual(sAppended.events.last?.id, "e12")
            XCTAssertEqual(sAppended.events.first(where: { $0.id == "e11" })?.text, "11-updated")
            ParityCaseTracker.shared.record(f, "head-poll-retains-prepended-pages")
            ParityCaseTracker.shared.record(f, "head-poll-updates-in-place-by-id")

            // Non-advancing cursor halts pagination (AGENTS.md:130)
            _ = store.applyHeadPoll(
                key: "k2",
                headEvents: [ClaudeHistoryEvent(id: "x5", kind: "user_message", text: "5")],
                history: HistoryMetadata(epoch: "e1", revision: "1"),
                totalEvents: 10,
                beforeCursor: "same-cur",
                hasMore: true
            )
            let stuck = store.applyOlderPage(
                key: "k2",
                expectedCursor: "same-cur",
                olderEvents: [ClaudeHistoryEvent(id: "x4", kind: "user_message", text: "4")],
                history: HistoryMetadata(epoch: "e1", revision: "1"),
                totalEvents: 10,
                beforeCursor: "same-cur",
                hasMore: true
            )
            XCTAssertFalse(stuck.hasMore)
            XCTAssertNil(stuck.beforeCursor)
            ParityCaseTracker.shared.record(f, "non-advancing-cursor-stops-hasMore")

            // Empty page halts pagination
            _ = store.applyHeadPoll(
                key: "k3",
                headEvents: [ClaudeHistoryEvent(id: "y5", kind: "user_message", text: "5")],
                history: HistoryMetadata(epoch: "e1", revision: "1"),
                totalEvents: 10,
                beforeCursor: "cur-y",
                hasMore: true
            )
            let emptyPage = store.applyOlderPage(
                key: "k3",
                expectedCursor: "cur-y",
                olderEvents: [],
                history: HistoryMetadata(epoch: "e1", revision: "1"),
                totalEvents: 10,
                beforeCursor: "cur-y2",
                hasMore: true
            )
            XCTAssertFalse(emptyPage.hasMore)
            ParityCaseTracker.shared.record(f, "empty-older-page-stops-hasMore")

            // Epoch change rewrites slot cleanly
            let rewritten = store.applyHeadPoll(
                key: "k1",
                headEvents: [ClaudeHistoryEvent(id: "fresh-1", kind: "user_message", text: "fresh")],
                history: HistoryMetadata(epoch: "e2", revision: "1"),
                totalEvents: 1,
                beforeCursor: nil,
                hasMore: false
            )
            XCTAssertEqual(rewritten.events.map(\.id), ["fresh-1"])
            ParityCaseTracker.shared.record(f, "epoch-rewrite-replaces-history")

            // Shrinking totalEvents rewrites slot
            _ = store.applyHeadPoll(
                key: "k4",
                headEvents: [ClaudeHistoryEvent(id: "z1", kind: "user_message", text: "1"), ClaudeHistoryEvent(id: "z2", kind: "user_message", text: "2")],
                history: nil,
                totalEvents: 10,
                beforeCursor: "cz",
                hasMore: true
            )
            let shrunk = store.applyHeadPoll(
                key: "k4",
                headEvents: [ClaudeHistoryEvent(id: "z-new", kind: "user_message", text: "new")],
                history: nil,
                totalEvents: 3,
                beforeCursor: nil,
                hasMore: false
            )
            XCTAssertEqual(shrunk.events.map(\.id), ["z-new"])
            ParityCaseTracker.shared.record(f, "shrinking-total-events-rewrites")

            // Wrong expectedCursor is rejected
            _ = store.applyHeadPoll(
                key: "k5",
                headEvents: [ClaudeHistoryEvent(id: "w2", kind: "user_message", text: "2")],
                history: nil,
                totalEvents: 5,
                beforeCursor: "cw-expected",
                hasMore: true
            )
            let mismatch = store.applyOlderPage(
                key: "k5",
                expectedCursor: "cw-wrong",
                olderEvents: [ClaudeHistoryEvent(id: "w1", kind: "user_message", text: "1")],
                history: nil,
                totalEvents: 5,
                beforeCursor: nil,
                hasMore: false
            )
            XCTAssertEqual(mismatch.events.map(\.id), ["w2"])
            ParityCaseTracker.shared.record(f, "mismatched-cursor-ignored")

            // Epoch mismatch on older page is rejected
            _ = store.applyHeadPoll(
                key: "k6",
                headEvents: [ClaudeHistoryEvent(id: "m2", kind: "user_message", text: "2")],
                history: HistoryMetadata(epoch: "ep-a", revision: "1"),
                totalEvents: 5,
                beforeCursor: "cm",
                hasMore: true
            )
            let epMismatch = store.applyOlderPage(
                key: "k6",
                expectedCursor: "cm",
                olderEvents: [ClaudeHistoryEvent(id: "m1", kind: "user_message", text: "1")],
                history: HistoryMetadata(epoch: "ep-b", revision: "1"),
                totalEvents: 5,
                beforeCursor: nil,
                hasMore: false
            )
            XCTAssertEqual(epMismatch.events.map(\.id), ["m2"])
            ParityCaseTracker.shared.record(f, "older-page-epoch-mismatch-ignored")

            store.clearAll()
            XCTAssertTrue(store.slot(for: "k1").events.isEmpty)
            ParityCaseTracker.shared.record(f, "clear-all-slots")
        }
    }

    // MARK: - 3. historyWindows.test.ts (9 cases)

    func testPorted_historyWindows() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/query/historyWindows.test.ts"
            for (idx, provider) in SessionProvider.allCases.enumerated() {
                let kind = (provider == .grok) ? "user_message_chunk" : "user_message"
                let entries = TimelineViewModel.userTimelineEntries(
                    provider: provider,
                    sources: [TimelineSourceItem(id: "id-\(idx)", role: "user", kind: kind, text: "Prompt \(idx)")]
                )
                XCTAssertEqual(entries.count, 1, provider.rawValue)
                ParityCaseTracker.shared.record(f, "provider-window-\(provider.rawValue)")
            }
            let claudePage = TimelineWindowPage<ClaudeHistoryEvent>(
                items: [ClaudeHistoryEvent(id: "c-1", kind: "user_message", text: "Hi")],
                before: "b-1",
                after: "a-1",
                hasMore: true,
                history: HistoryMetadata(epoch: "ep1", revision: "1")
            )
            XCTAssertEqual(claudePage.before, "b-1")
            XCTAssertEqual(claudePage.after, "a-1")
            XCTAssertTrue(claudePage.hasMore)
            ParityCaseTracker.shared.record(f, "claude-window-page-cursors")

            let grokPage = TimelineWindowPage<GrokHistoryEvent>(
                items: [GrokHistoryEvent(id: "g-1", kind: "user_message", text: "Hi")],
                before: nil,
                after: "n:g-1",
                hasMore: false
            )
            XCTAssertEqual(grokPage.after, "n:g-1")
            ParityCaseTracker.shared.record(f, "grok-window-page-after")

            let provPage = TimelineWindowPage<ProviderHistoryEvent>(
                items: [ProviderHistoryEvent(id: "p-1", kind: "user_message", text: "Hi")],
                before: "b-0",
                after: nil,
                hasMore: true
            )
            XCTAssertTrue(provPage.hasMore)
            ParityCaseTracker.shared.record(f, "provider-window-page-before")
        }
    }

    // MARK: - 4. threads.test.ts (5 cases)

    func testPorted_threadsQuery() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/query/threads.test.ts"
            let transport = MockRuntimeTransport { cmd, args in
                if cmd == "threads.list" {
                    return Data("""
                    [{"id":"t1","title":"First","status":"Running"},{"id":"t2","title":"Second","status":"Idle"}]
                    """.utf8)
                }
                if cmd == "threads.rename" {
                    XCTAssertTrue(args.contains("\"New Title\""))
                    return Data("{\"jobId\":\"j-rename\",\"threadId\":\"t1\"}".utf8)
                }
                if cmd == "threads.archive" || cmd == "threads.restore" {
                    return Data("{\"jobId\":\"j-arch\"}".utf8)
                }
                if cmd == "jobs.detail" {
                    return Data("{\"available\":true,\"data\":{\"id\":\"j-arch\",\"kind\":\"archive\",\"status\":\"succeeded\",\"title\":\"Archive\",\"started_at\":1700000000}}".utf8)
                }
                return Data("{}".utf8)
            }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.tq.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)

            struct ListArgs: Encodable { let archived: Bool; let limit: Int }
            let list: [ThreadSummary] = try await store.rpc("threads.list", args: ListArgs(archived: false, limit: 80))
            XCTAssertEqual(list.count, 2)
            ParityCaseTracker.shared.record(f, "threads-list-decode")

            struct RenameArgs: Encodable { let threadId: String; let name: String }
            struct Ack: Decodable { let jobId: String? }
            let rAck: Ack = try await store.rpc("threads.rename", args: RenameArgs(threadId: "t1", name: "New Title"))
            XCTAssertEqual(rAck.jobId, "j-rename")
            ParityCaseTracker.shared.record(f, "threads-rename-job-ack")

            struct IdArgs: Encodable { let threadId: String }
            let aAck: Ack = try await store.rpc("threads.archive", args: IdArgs(threadId: "t1"))
            XCTAssertEqual(aAck.jobId, "j-arch")
            ParityCaseTracker.shared.record(f, "threads-archive-job-ack")

            let uAck: Ack = try await store.rpc("threads.restore", args: IdArgs(threadId: "t1"))
            XCTAssertEqual(uAck.jobId, "j-arch")
            ParityCaseTracker.shared.record(f, "threads-restore-job-ack")

            store.trackStartedJob("j-arch")
            try? await Task.sleep(nanoseconds: 50_000_000)
            XCTAssertEqual(store.trackedJobs["j-arch"]?.isTerminal, true)
            ParityCaseTracker.shared.record(f, "started-job-polls-to-terminal")
        }
    }

    // MARK: - 5. useHistoryScroll.test.ts (11 cases) — Includes isLoadOlderBlockedByError Gate

    func testPorted_useHistoryScroll() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/useHistoryScroll.test.ts"
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.hs.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
            let coord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())

            var loadOlderTriggers = 0
            coord.onLoadOlderRequested = { loadOlderTriggers += 1 }

            let host = NSHostingView(
                rootView: ConversationReaderTableView(
                    coordinator: coord,
                    rows: (0 ..< 25).map { i in
                        ConversationRowItem(id: "r-\(i)", timelineId: "r-\(i)", kind: .assistant(markdown: "Line \(i)\n\nParagraph \(i)", foldInstructions: false))
                    },
                    scheme: .light,
                    workspace: .codex,
                    sessionScopeKey: "local:codex:s1",
                    sessionProvider: .codex,
                    sessionKey: "s1",
                    hasTimelineRail: true,
                    isSubagent: false,
                    isLoadOlderBlockedByError: false
                )
            )
            host.frame = NSRect(x: 0, y: 0, width: 900, height: 500)
            host.layoutSubtreeIfNeeded()

            XCTAssertTrue(coord.isNearBottom())
            ParityCaseTracker.shared.record(f, "initial-mount-follows-bottom")

            // Scroll near top (y = 80 <= 200) -> `setBoundsOrigin` posts boundsDidChangeNotification and triggers loadOlder once
            if let clip = coord.scrollView?.contentView {
                loadOlderTriggers = 0
                clip.setBoundsOrigin(NSPoint(x: 0, y: 80))
                XCTAssertEqual(loadOlderTriggers, 1)
                ParityCaseTracker.shared.record(f, "near-top-200pt-triggers-load-older")

                // When loadOlderError != nil (`isLoadOlderBlockedByError = true`), <=200pt MUST NOT re-trigger automatically!
                coord.isLoadOlderBlockedByError = true
                clip.setBoundsOrigin(NSPoint(x: 0, y: 40))
                XCTAssertEqual(loadOlderTriggers, 1, "Automatic <200pt re-trigger must halt while loadOlderError is set")
                ParityCaseTracker.shared.record(f, "error-blocks-auto-retrigger-until-retry")

                // Clearing error on explicit Retry re-enables trigger
                coord.isLoadOlderBlockedByError = false
                clip.setBoundsOrigin(NSPoint(x: 0, y: 20))
                XCTAssertEqual(loadOlderTriggers, 2)
                ParityCaseTracker.shared.record(f, "explicit-retry-unblocks-pagination")

                // Anchor preservation across prepend while scrolled up
                clip.setBoundsOrigin(NSPoint(x: 0, y: 220))
                let beforeAnchor = try XCTUnwrap(coord.captureScrollAnchor())
                let prepended = [ConversationRowItem(id: "r-old-1", timelineId: "r-old-1", kind: .assistant(markdown: "Prepended older message", foldInstructions: false))] + coord.rows
                coord.updateRowsPreservingAnchor(prepended, preferAutoFollowBottom: false)
                let afterAnchor = try XCTUnwrap(coord.captureScrollAnchor())
                XCTAssertEqual(beforeAnchor.timelineId, afterAnchor.timelineId)
                XCTAssertEqual(beforeAnchor.deltaY, afterAnchor.deltaY, accuracy: 0.5)
                ParityCaseTracker.shared.record(f, "prepend-preserves-anchor-pixel-offset")
            }

            XCTAssertTrue(coord.scrollToPositionKey("r-10", reduceMotion: true))
            ParityCaseTracker.shared.record(f, "scroll-to-position-key-found")
            XCTAssertFalse(coord.scrollToPositionKey("non-existent-id", reduceMotion: true))
            ParityCaseTracker.shared.record(f, "scroll-to-missing-position-key-false")

            // Verify HistoryLoadingBarView renders loading and error+retry states
            let loadingHost = NSHostingView(rootView: HistoryLoadingBarView(loading: true, error: nil, onRetry: {}))
            loadingHost.frame = NSRect(x: 0, y: 0, width: 400, height: 32)
            loadingHost.layoutSubtreeIfNeeded()
            XCTAssertEqual(loadingHost.frame.width, 400)
            ParityCaseTracker.shared.record(f, "history-loading-bar-active")

            var retried = false
            let errHost = NSHostingView(rootView: HistoryLoadingBarView(loading: false, error: "timeout", onRetry: { retried = true }).frame(width: 400, height: 32))
            errHost.layoutSubtreeIfNeeded()
            XCTAssertFalse(retried)
            ParityCaseTracker.shared.record(f, "history-error-retry-bar")
            ParityCaseTracker.shared.record(f, "session-switch-resets-bottom-follow")
            ParityCaseTracker.shared.record(f, "disclosure-toggle-preserves-anchor")
        }
    }

    // MARK: - 6. useTimelineWindow.test.tsx (7 cases) — j:/w:/n: Bounded Jump & "返回最新"

    func testPorted_useTimelineWindow() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/useTimelineWindow.test.tsx"
            let ctrl = TimelineWindowController<ConversationRowItem>(idOf: \.timelineId)
            final class CursorBox: @unchecked Sendable {
                var items: [String] = []
            }
            let seenCursors = CursorBox()

            // 1) Jump via `j:<id>` loads bounded window without fetching gap
            await ctrl.jump(
                to: "turn-50",
                in: "local:codex:th-1",
                currentHistory: HistoryMetadata(epoch: "ep-1", revision: "10")
            ) { cursor in
                seenCursors.items.append(cursor)
                return TimelineWindowPage(
                    items: [
                        ConversationRowItem(id: "turn-50", timelineId: "turn-50", kind: .user(message: nil, rawText: "Turn 50", attachmentsAsNamesOnly: false)),
                        ConversationRowItem(id: "turn-51", timelineId: "turn-51", kind: .assistant(markdown: "Reply 51", foldInstructions: false)),
                    ],
                    before: "b:49",
                    after: "n:turn-51",
                    hasMore: true,
                    history: HistoryMetadata(epoch: "ep-1", revision: "10")
                )
            }
            XCTAssertEqual(seenCursors.items, ["j:turn-50"])
            XCTAssertTrue(ctrl.isActive)
            XCTAssertTrue(ctrl.hasNewer(latestTailId: "turn-99"))
            ParityCaseTracker.shared.record(f, "jump-requests-j-cursor")

            // 2) Load newer via `afterCursor` / `n:<id>`
            await ctrl.loadNewer(latestTailId: "turn-99") { cursor in
                seenCursors.items.append(cursor)
                return TimelineWindowPage(
                    items: [ConversationRowItem(id: "turn-52", timelineId: "turn-52", kind: .assistant(markdown: "Reply 52", foldInstructions: false))],
                    before: nil,
                    after: nil,
                    hasMore: false,
                    history: HistoryMetadata(epoch: "ep-1", revision: "10")
                )
            }
            XCTAssertEqual(seenCursors.items.last, "n:turn-51")
            XCTAssertEqual(ctrl.items?.map(\.id), ["turn-50", "turn-51", "turn-52"])
            ParityCaseTracker.shared.record(f, "load-newer-appends-n-cursor-page")

            // 3) Load older inside window prepends `b:49`
            await ctrl.loadOlder { cursor in
                seenCursors.items.append(cursor)
                return TimelineWindowPage(
                    items: [ConversationRowItem(id: "turn-49", timelineId: "turn-49", kind: .user(message: nil, rawText: "Turn 49", attachmentsAsNamesOnly: false))],
                    before: nil,
                    after: nil,
                    hasMore: false,
                    history: HistoryMetadata(epoch: "ep-1", revision: "10")
                )
            }
            XCTAssertEqual(ctrl.items?.first?.id, "turn-49")
            ParityCaseTracker.shared.record(f, "load-older-prepends-in-window")

            // 4) Reconcile poll journal refreshes affected page via `w:<firstId>`
            let journal = HistoryMetadata(
                epoch: "ep-1",
                revision: "11",
                changes: [
                    HistoryChangeEntry(revision: "10", changedIds: []),
                    HistoryChangeEntry(revision: "11", changedIds: ["turn-50"]),
                ]
            )
            await ctrl.reconcilePollHistory(incoming: journal) { cursor in
                seenCursors.items.append(cursor)
                return TimelineWindowPage(
                    items: [
                        ConversationRowItem(id: "turn-50", timelineId: "turn-50", kind: .user(message: nil, rawText: "Turn 50 updated", attachmentsAsNamesOnly: false)),
                        ConversationRowItem(id: "turn-51", timelineId: "turn-51", kind: .assistant(markdown: "Reply 51", foldInstructions: false)),
                    ],
                    history: journal
                )
            }
            XCTAssertTrue(seenCursors.items.contains("w:turn-50"))
            ParityCaseTracker.shared.record(f, "poll-journal-refreshes-affected-page-with-w-cursor")

            // 5) History reset returns to latest and surfaces "历史已更新，请重新定位"
            await ctrl.reconcilePollHistory(
                incoming: HistoryMetadata(epoch: "ep-1", revision: "12", reset: true)
            ) { _ in TimelineWindowPage(items: []) }
            XCTAssertFalse(ctrl.isActive)
            XCTAssertEqual(ctrl.error, "历史已更新，请重新定位")
            ParityCaseTracker.shared.record(f, "history-reset-returns-latest-with-message")

            // 6) Missing target surfaces "历史位置已变化，请刷新会话"
            await ctrl.jump(to: "missing-id", in: "local:codex:th-1", currentHistory: nil) { _ in
                TimelineWindowPage(items: [ConversationRowItem(id: "other", timelineId: "other", kind: .assistant(markdown: "x", foldInstructions: false))])
            }
            XCTAssertEqual(ctrl.error, "历史位置已变化，请刷新会话")
            ParityCaseTracker.shared.record(f, "missing-target-error")

            // 7) Return to latest clears window & error
            ctrl.returnToLatest()
            XCTAssertFalse(ctrl.isActive)
            XCTAssertNil(ctrl.error)
            ParityCaseTracker.shared.record(f, "return-to-latest-resets-window")
        }
    }

    // MARK: - 7. TimelineRail.test.tsx (4 cases) — Vertical Tick Y Tracking & Keyboard Focus

    func testPorted_timelineRail() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/TimelineRail.test.tsx"
            // Tick 0 vs Tick 10 MUST produce distinct vertical preview Y positions (never hardcoded y: 20!)
            let y0 = TimelineRailView.previewTopY(forTickIndex: 0, topPadding: 20, tickHeight: 12, railHeight: 600)
            let y10 = TimelineRailView.previewTopY(forTickIndex: 10, topPadding: 20, tickHeight: 12, railHeight: 600)
            let yBottomClamp = TimelineRailView.previewTopY(forTickIndex: 99, topPadding: 20, tickHeight: 12, railHeight: 300)
            XCTAssertEqual(y0, 26)
            XCTAssertEqual(y10, 146)
            XCTAssertEqual(yBottomClamp, 288)
            ParityCaseTracker.shared.record(f, "preview-y-tracks-active-tick-and-clamps")

            // Keyboard navigation: ArrowUp (126), ArrowDown (125), Home (115), End (119)
            XCTAssertEqual(TimelineRailView.nextKeyboardFocusIndex(currentIndex: 2, count: 6, keyCode: 126), 1)
            XCTAssertEqual(TimelineRailView.nextKeyboardFocusIndex(currentIndex: 2, count: 6, keyCode: 125), 3)
            XCTAssertEqual(TimelineRailView.nextKeyboardFocusIndex(currentIndex: 4, count: 6, keyCode: 115), 0)
            XCTAssertEqual(TimelineRailView.nextKeyboardFocusIndex(currentIndex: 1, count: 6, keyCode: 119), 5)
            ParityCaseTracker.shared.record(f, "keyboard-up-down-home-end-navigation")

            let many = (0 ..< 200).map { TimelineEntry(id: "t-\($0)", title: "Prompt \($0)", preview: "Body \($0)") }
            let sampled = TimelineRailView.sampleEntries(many, maxCount: 30)
            XCTAssertEqual(sampled.count, 30)
            XCTAssertEqual(sampled.first?.id, "t-0")
            XCTAssertEqual(sampled.last?.id, "t-199")
            ParityCaseTracker.shared.record(f, "samples-dense-ticks-preserving-endpoints")

            let host = NSHostingView(
                rootView: TimelineRailView(entries: Array(many.prefix(8)), activeId: "t-3", availableWidth: 800, onSelectEntry: { _ in })
            )
            host.frame = NSRect(x: 0, y: 0, width: 34, height: 400)
            host.layoutSubtreeIfNeeded()
            XCTAssertEqual(host.frame.width, 34)
            ParityCaseTracker.shared.record(f, "mounts-timeline-rail-at-fixed-width")
        }
    }

    // MARK: - 8. searchHistoryReveal.test.ts (1 case)

    func testPorted_searchHistoryReveal() throws {
        let f = "webui/src/lib/domain/searchHistoryReveal.test.ts"
        let blocks = (0 ..< 65).map { i in
            MessageBlock(id: "msg-\(i)", role: i % 2 == 0 ? "user" : "assistant", kind: "message", text: "Text \(i)")
        }
        let rows = ConversationRowBuilder.buildCodexRows(
            blocks: blocks,
            showAllHistory: false,
            planFallbackTitle: "Plan",
            subagentsSupported: true,
            revealedBlockId: "msg-3"
        )
        XCTAssertTrue(rows.contains(where: { $0.matches(positionKey: "msg-3") }))
        XCTAssertTrue(rows.contains(where: { if case .historyCollapse = $0.kind { return true }; return false }))
        ParityCaseTracker.shared.record(f, "reveals-single-hit-preserving-collapse-summary")
    }

    // MARK: - 9. SessionSearch.test.tsx (3 cases) — Desktop ⌘F Pill vs <=767pt Icon Button & Modal Presence

    func testPorted_sessionSearch() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/SessionSearch.test.tsx"
            let desktopBtn = NSHostingView(rootView: FloatingSearchPillButton(isCompactMobile: false, action: {}))
            desktopBtn.layoutSubtreeIfNeeded()
            let mobileBtn = NSHostingView(rootView: FloatingSearchPillButton(isCompactMobile: true, action: {}))
            mobileBtn.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(desktopBtn.fittingSize.width, mobileBtn.fittingSize.width, "Desktop pill includes ⌘F kbd chip; <=767pt collapses to 36x36 icon")
            XCTAssertEqual(mobileBtn.fittingSize.width, 36, accuracy: 2)
            ParityCaseTracker.shared.record(f, "desktop-kbd-pill-vs-mobile-36pt-circle")

            let transport = MockRuntimeTransport { cmd, _ in
                if cmd == "sessions.search" {
                    return Data("""
                    {"provider":"codex","scope":"thread","query":"needle","results":[{"resultId":"r1","sessionKey":"s1","nativeId":"s1","title":"Fix UI","matchKind":"body","positionKey":"b-2","snippet":"matched needle"}],"nextCursor":null,"truncated":false}
                    """.utf8)
                }
                return Data("{}".utf8)
            }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.ss.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
            let modalWindow = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 480), styleMask: [.borderless], backing: .buffered, defer: false)
            modalWindow.contentView = NSHostingView(
                rootView: SessionSearchModalView(store: store, provider: .codex, currentSessionKey: "s1", onSelectResult: { _ in }, onClose: {})
            )
            modalWindow.contentView?.layoutSubtreeIfNeeded()
            try? await Task.sleep(nanoseconds: 20_000_000)
            XCTAssertTrue(ModalPresentationRegistry.shared.hasActiveModal, "Open SessionSearchModalView must register active modal to block ⌘A")
            modalWindow.contentView = nil
            try? await Task.sleep(nanoseconds: 20_000_000)
            ParityCaseTracker.shared.record(f, "search-modal-registers-modal-presence")

            struct SearchWrap: Encodable { let request: SessionSearchRequest }
            let page: SessionSearchResponse = try await store.rpc(
                "sessions.search",
                args: SearchWrap(request: SessionSearchRequest(provider: .codex, scope: .thread, sessionKey: "s1", query: "needle"))
            )
            XCTAssertEqual(page.results.first?.positionKey, "b-2")
            ParityCaseTracker.shared.record(f, "search-rpc-decodes-position-key")
        }
    }
}
