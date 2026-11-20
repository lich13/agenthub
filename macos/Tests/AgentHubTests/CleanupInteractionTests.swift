@testable import AgentHubCore
import Foundation
import XCTest

struct CleanupInteractionVectors: Decodable {
    let preview: UnifiedCleanupPlan
    let execute: JSONValue
    let result: UnifiedCleanupResult
    let hiddenStart: HiddenThreadDeletePlan
    let hiddenPage: HiddenThreadDeletePlan

    static func load() throws -> Self {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return try JSONDecoder().decode(Self.self, from: Data(contentsOf: root.appendingPathComponent("contracts/parity-vectors/cleanup.json")))
    }
}

final class CleanupInteractionTests: XCTestCase {
    @MainActor func testAccountScopeToggleAndProgressUseOnlyTheRequestedWireFields() async throws {
        let rounds = AsyncStream.makeStream(of: Data.self)
        rounds.continuation.yield(Data(#"{"clearedThreads":0,"clearedFiles":1,"remainingFiles":2,"items":[]}"#.utf8))
        rounds.continuation.yield(Data(#"{"clearedThreads":1,"clearedFiles":1,"remainingFiles":0,"items":[]}"#.utf8))
        rounds.continuation.finish()
        let mock = MockRuntimeTransport { command, args in
            if command == "probe.settings.save" {
                let payload = try JSONDecoder().decode(JSONValue.self, from: Data(args.utf8))
                XCTAssertEqual(payload, .object(["settings": .object(["probe": .object(["hooks": .object(["clear_account_scope": .bool(true)])])])]))
                return Data(#"{"probe":{"hooks":{"clear_account_scope":true}}}"#.utf8)
            }
            XCTAssertEqual(command, "threads.accountScopeClear")
            var replies = rounds.stream.makeAsyncIterator()
            guard let reply = await replies.next() else { throw AgentHubFFIError("unexpected batch") }
            return reply
        }
        let store = RuntimeConnectionStore(transport: mock, defaults: UserDefaults(suiteName: UUID().uuidString)!)
        let enabled = try await store.saveAutoClearAccountScope(true, at: store.revision)
        XCTAssertTrue(enabled)
        var updates: [CodexAccountScopeResult] = []
        try await store.clearAccountScope(at: store.revision) { updates.append($0) }
        XCTAssertEqual(updates.map(\.clearedFiles), [1, 2])
        XCTAssertEqual(updates.last?.clearedThreads, 1)
        XCTAssertEqual(updates.last?.remainingFiles, 0)
    }

    func testDirectProbeStatusWithAvailabilityKeepsAllActiveProviders() throws {
        let data = Data(#"{"available":true,"hook_status":"managed","active_sessions":[{"provider":"grok","id":"shared","session_key":"g","bucket":"running"},{"provider":"claude_code","id":"shared","session_key":"c","bucket":"reply_needed"}]}"#.utf8)
        let status = try XCTUnwrap(JSONDecoder().decode(OptionalResultEnvelope<ProbeStatus>.self, from: data).data)
        XCTAssertEqual(status.hookStatus, "managed")
        XCTAssertEqual(status.activeSessions?.map(\.uniqueKey), ["grok:g:running", "claude_code:c:reply_needed"])
        let unavailable = try JSONDecoder().decode(OptionalResultEnvelope<ProbeStatus>.self, from: Data(#"{"available":false,"data":null}"#.utf8))
        XCTAssertFalse(unavailable.available)
        XCTAssertNil(unavailable.data)
    }

    @MainActor func testEqualLengthPagesAppendAndPartialScansCannotExecute() async throws {
        let vectors = try CleanupInteractionVectors.load()
        var requests: [Int] = []
        let runner = HiddenCleanupScan { req in
            if req.mode == "start" { return vectors.hiddenStart }
            let offset = try XCTUnwrap(req.cursor)
            requests.append(offset)
            XCTAssertEqual(req.limit, 100)
            let count = min(100, 201 - offset)
            var page = vectors.hiddenPage
            page.scan = HiddenScanProgress(id: page.scan.id, phase: .complete, processed: 201, total: 201,
                                           cursor: offset, nextCursor: offset + count, hasMore: offset + count < 201)
            page.candidates = (offset ..< offset + count).map {
                HiddenThreadCandidate(id: String($0), title: "Fixture", allowed: $0 != 7, reason: $0 == 7 ? "占用" : nil)
            }
            return page
        }
        var plan = vectors.preview
        try await runner.run { progress, scope in
            plan.replaceHidden(scope)
            let selection = plan.executionRequest(for: ["codex_hidden"])
            if progress.nextCursor < 201 { XCTAssertTrue(selection.targets.isEmpty) }
            else {
                XCTAssertEqual(selection.targets.map(\.id), (0 ..< 201).filter { $0 != 7 }.map(String.init))
            }
        }
        XCTAssertEqual(requests, [0, 100, 200])
    }

    @MainActor func testStopBeforeFirstReplyCancelsOriginalIdExactlyOnce() async throws {
        let vectors = try CleanupInteractionVectors.load()
        let (started, signal) = AsyncStream<Void>.makeStream()
        var startReply: CheckedContinuation<HiddenThreadDeletePlan, Never>?
        var commands: [String] = []
        let runner = HiddenCleanupScan { request in
            commands.append(request.mode)
            if request.mode == "start" {
                return await withCheckedContinuation { reply in
                    startReply = reply
                    signal.yield()
                }
            }
            XCTAssertEqual(request.scanId, vectors.hiddenStart.scan.id)
            return vectors.hiddenPage
        }
        let run = Task { try await runner.run { _, _ in XCTFail("A stopped page must not be published") } }
        var iterator = started.makeAsyncIterator()
        _ = await iterator.next()
        let cancel = runner.stop()
        _ = runner.stop()
        startReply?.resume(returning: vectors.hiddenStart)
        await cancel?.value
        try await run.value
        XCTAssertEqual(commands, ["start", "cancel"])
    }

    @MainActor func testEmptyCompleteScanAndIndependentRescan() async throws {
        let vectors = try CleanupInteractionVectors.load()
        for count in [1, 0] {
            var page = vectors.hiddenPage
            page.candidates = Array(page.candidates.prefix(count))
            page.scan.processed = count
            page.scan.total = count
            page.scan.nextCursor = count
            let runner = HiddenCleanupScan { request in
                XCTAssertEqual(request.mode, "start")
                return page
            }
            try await runner.run { _, scope in
                XCTAssertTrue(scope.scanned)
                XCTAssertEqual(scope.deletableCount, count)
                XCTAssertEqual(scope.candidates.count, count)
            }
        }
    }

    @MainActor func testStopKeepsPublishedPageAndOtherCompletedScopesCanExecute() async throws {
        let fixture = try CleanupInteractionVectors.load()
        var page = fixture.hiddenPage
        page.scan.phase = .scanning
        page.scan.total = 2
        let running = page
        var cancelled = false
        let runner = HiddenCleanupScan { request in
            if request.mode == "cancel" {
                XCTAssertEqual(request.scanId, running.scan.id)
                XCTAssertEqual(request.cursor, 1)
                cancelled = true
            } else {
                XCTAssertEqual(request.mode, "start")
            }
            return running
        }
        var plan = fixture.preview
        try await runner.run { _, hidden in
            plan.replaceHidden(hidden)
            runner.stop()
        }
        XCTAssertTrue(cancelled)
        XCTAssertEqual(plan.scopes.last?.candidates.map(\.id), ["hidden-1"])
        XCTAssertEqual(plan.executionRequest(for: ["codex_hidden", "claude_empty"]).targets.map(\.id), ["p/fixture"])
    }
}
