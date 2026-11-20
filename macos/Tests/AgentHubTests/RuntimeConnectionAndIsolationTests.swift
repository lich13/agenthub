@testable import AgentHubCore
import Foundation
import XCTest

extension RuntimeTransport where Self == MockRuntimeTransport {
    static var emptyDesktop: MockRuntimeTransport {
        MockRuntimeTransport { command, _ in
            switch command {
            case "remote.get":
                Data(#"{"target":"local","revision":1,"configured":false,"machines":[]}"#.utf8)
            case "system.capabilities":
                Data(#"{"host_surface":"desktop_embedded_tauri","capabilities":{"grokbot_sessions":true}}"#.utf8)
            default:
                Data("[]".utf8)
            }
        }
    }
}

final class RuntimeConnectionAndIsolationTests: XCTestCase {
    func testTwoParallelIsolatedFFIRuntimesCoexistWithoutFlockConflict() async throws {
        let rootA = FileManager.default.temporaryDirectory
            .appendingPathComponent("agenthub-iso-a-\(UUID().uuidString)")
        let rootB = FileManager.default.temporaryDirectory
            .appendingPathComponent("agenthub-iso-b-\(UUID().uuidString)")
        defer {
            try? FileManager.default.removeItem(at: rootA)
            try? FileManager.default.removeItem(at: rootB)
        }

        let bridgeA = try FFIRuntimeBridge(resourceDir: nil, isolatedRoot: rootA.path)
        let bridgeB = try FFIRuntimeBridge(resourceDir: nil, isolatedRoot: rootB.path)
        defer {
            bridgeA.shutdown()
            bridgeB.shutdown()
        }

        XCTAssertTrue(bridgeA.isPreviewIsolated)
        XCTAssertTrue(bridgeB.isPreviewIsolated)
        XCTAssertTrue(FileManager.default.fileExists(atPath: rootA.appendingPathComponent("config.toml").path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: rootB.appendingPathComponent("config.toml").path))

        // Verify config.toml mode is 0600
        let attrsA = try FileManager.default.attributesOfItem(atPath: rootA.appendingPathComponent("config.toml").path)
        let posixA = (attrsA[.posixPermissions] as? NSNumber)?.intValue ?? 0
        XCTAssertEqual(posixA & 0o777, 0o600)

        let capsDataA = try await bridgeA.invokeRawJSON(command: "system.capabilities")
        let capsDataB = try await bridgeB.invokeRawJSON(command: "system.capabilities")
        let capsA = try JSONDecoder().decode(SystemCapabilitiesResponse.self, from: capsDataA)
        let capsB = try JSONDecoder().decode(SystemCapabilitiesResponse.self, from: capsDataB)
        XCTAssertEqual(capsA.apiVersion, 2)
        XCTAssertEqual(capsB.apiVersion, 2)

        // Isolated mode must block updates.install safely
        do {
            _ = try await bridgeA.invokeRawJSON(command: "updates.install")
            XCTFail("Expected isolated updates.install to be blocked")
        } catch {
            XCTAssertTrue(error.localizedDescription.contains("预览隔离模式"))
        }
    }

    @MainActor
    func testRuntimeConnectionStoreRoutesRemoteRpcAndDropsStaleRevisions() async throws {
        let suite = "com.lich13.agenthub.conn.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let mock = MockRuntimeTransport { command, _ in
            switch command {
            case "remote.get":
                return Data("""
                {"target":"remote","revision":3,"sshHost":"dev-box","configured":true,"activeMachineId":"m1","activeMachineName":"Dev Box","machines":[{"id":"m1","name":"Dev Box","sshHost":"dev-box","useSudo":true}]}
                """.utf8)
            case "remote.invoke":
                return Data("""
                {"api_version":2,"host_surface":"linux_server_api","capabilities":{"threads":true,"jobs":true,"probe":true,"settings":true,"job_history":true,"app_updater":true,"thread_cleanup":true,"thread_archive_actions":true,"systemd":true,"linux_update_job":true,"prune_backups":true,"github_ci":true,"grokbot_sessions":false}}
                """.utf8)
            case "remote.select":
                return Data("""
                {"target":"local","revision":4,"sshHost":null,"configured":true,"activeMachineId":null,"activeMachineName":null,"machines":[]}
                """.utf8)
            case "system.capabilities":
                return Data("""
                {"api_version":2,"host_surface":"desktop_embedded_tauri","capabilities":{"threads":true,"jobs":true,"probe":true,"settings":true,"job_history":true,"app_updater":true,"thread_cleanup":true,"thread_archive_actions":true,"systemd":false,"linux_update_job":false,"prune_backups":false,"grokbot_sessions":true}}
                """.utf8)
            default:
                return Data("{}".utf8)
            }
        }

        let store = RuntimeConnectionStore(transport: mock, defaults: defaults)
        store.activeWorkspace = .grokbot
        await store.bootstrap()

        XCTAssertEqual(store.target, .remote)
        XCTAssertEqual(store.revision, 3)
        XCTAssertEqual(store.machineScope, "remote:m1:dev-box")
        // Remote server disables local-only grokbot workspace and redirects to codex
        XCTAssertEqual(store.activeWorkspace, .codex)
        XCTAssertTrue(store.capabilities.updatePrune)

        var cleanupCalled = false
        let token = store.registerBeforeMachineChange {
            cleanupCalled = true
        }
        try await store.selectMachine(target: .local)
        store.unregisterBeforeMachineChange(token)

        XCTAssertTrue(cleanupCalled)
        XCTAssertEqual(store.target, .local)
        XCTAssertEqual(store.revision, 4)
        XCTAssertEqual(store.machineScope, "local:")
        XCTAssertTrue(store.ready)
        XCTAssertNil(store.capabilitiesError)
        XCTAssertTrue(store.capabilities.grokbotSessions)
        XCTAssertFalse(store.capabilities.updatePrune)
    }

    @MainActor
    func testSwitchRetryReconnectAndLateReadsWithIdenticalThreadKeys() async throws {
        let (firstDetail, detailStarted) = AsyncStream<Void>.makeStream()
        let (releaseDetail, detailReply) = AsyncStream<Void>.makeStream()
        let (secondCaps, capsStarted) = AsyncStream<Void>.makeStream()
        let (releaseCaps, capsReply) = AsyncStream<Void>.makeStream()
        let (write, writeStarted) = AsyncStream<Void>.makeStream()
        let (releaseWrite, writeReply) = AsyncStream<Void>.makeStream()
        let mock = MockRuntimeTransport { command, args in
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(args.utf8)) as? [String: Any])
            let req = object["request"] as? [String: Any] ?? [:]
            let revision = req["revision"] as? Int ?? 0
            switch command {
            case "remote.get":
                return Data(#"{"target":"local","revision":1,"configured":true}"#.utf8)
            case "system.capabilities":
                return Data(#"{"host_surface":"desktop_embedded_tauri","capabilities":{"grokbot_sessions":true}}"#.utf8)
            case "remote.select":
                let target = try XCTUnwrap(req["target"] as? String)
                let id = req["machineId"] as? String ?? ""
                if id == "unreachable" { throw AgentHubFFIError("connection refused") }
                return try JSONSerialization.data(withJSONObject: [
                    "target": target, "configured": true, "revision": revision + 1,
                    "activeMachineId": id, "sshHost": id,
                ])
            case "remote.invoke":
                switch req["command"] as? String {
                case "system.capabilities":
                    if revision == 3 {
                        capsStarted.yield()
                        var it = releaseCaps.makeAsyncIterator()
                        _ = await it.next()
                    }
                    return try JSONSerialization.data(withJSONObject: [
                        "host_surface": "linux_server_api", "capabilities": ["prune_backups": revision == 2],
                    ])
                case "threads.detail":
                    XCTAssertEqual((req["args"] as? [String: String])?["id"], "shared-id")
                    if revision == 2 {
                        detailStarted.yield()
                        var it = releaseDetail.makeAsyncIterator()
                        _ = await it.next()
                    }
                    return try JSONEncoder().encode("origin-\(revision)")
                case "cleanup.unifiedExecute":
                    writeStarted.yield()
                    var it = releaseWrite.makeAsyncIterator()
                    _ = await it.next()
                    return Data("true".utf8)
                default:
                    throw AgentHubFFIError("unexpected remote request")
                }
            default:
                throw AgentHubFFIError("unexpected local request")
            }
        }
        let store = RuntimeConnectionStore(transport: mock, defaults: UserDefaults(suiteName: UUID().uuidString)!)
        await store.bootstrap()
        try await store.selectMachine(target: .remote, machineId: "a")
        XCTAssertTrue(store.capabilities.updatePrune)
        let source = store.revision
        let lateRead = Task { () -> String in try await store.rpc("threads.detail", args: ["id": "shared-id"]) }
        var detail = firstDetail.makeAsyncIterator()
        _ = await detail.next()
        do {
            try await store.selectMachine(target: .remote, machineId: "unreachable")
            XCTFail("A failed connection must be retryable")
        } catch {
            XCTAssertFalse(store.changing)
            XCTAssertEqual(store.revision, source)
            XCTAssertEqual(store.activeMachineId, "a")
        }
        let switchToB = Task { try await store.selectMachine(target: .remote, machineId: "b") }
        var caps = secondCaps.makeAsyncIterator()
        _ = await caps.next()
        XCTAssertFalse(store.ready)
        capsReply.yield()
        try await switchToB.value
        XCTAssertTrue(store.ready)
        XCTAssertFalse(store.changing)
        XCTAssertNil(store.capabilitiesError)
        XCTAssertFalse(store.capabilities.updatePrune)
        let current: String = try await store.rpc("threads.detail", args: ["id": "shared-id"])
        detailReply.yield()
        do { _ = try await lateRead.value; XCTFail("published the old machine's detail") }
        catch { XCTAssertEqual(current, "origin-3") }

        let confirmation = try CleanupInteractionVectors.load().preview.executionRequest(for: ["claude_empty"])
        let request = CleanupRequestEnvelope(request: confirmation)
        XCTAssertFalse(confirmation.targets.isEmpty)
        do {
            let _: Bool = try await store.rpc("cleanup.unifiedExecute", args: request, at: source)
            XCTFail("sent an old confirmation")
        } catch {
            XCTAssertFalse(mock.recordedCalls.contains { $0.args.contains("cleanup.unifiedExecute") })
        }
        let accepted = Task { () -> Bool in try await store.rpc("cleanup.unifiedExecute", args: request, at: store.revision) }
        var pending = write.makeAsyncIterator()
        _ = await pending.next()
        do { try await store.selectMachine(target: .remote, machineId: "b"); XCTFail("switched during a write") }
        catch { XCTAssertTrue(store.isBusyForSwitch) }
        writeReply.yield()
        _ = try await accepted.value
        try await store.selectMachine(target: .remote, machineId: "b")
        XCTAssertEqual(store.revision, 4)
        try await store.selectMachine(target: .local)
        XCTAssertTrue(store.capabilities.grokbotSessions)
        XCTAssertEqual(mock.recordedCalls.filter { $0.args.contains("cleanup.unifiedExecute") }.count, 1)
    }

    @MainActor
    func testSwitchDuringFirstScanPageCancelsTheSourceBeforeDisconnect() async throws {
        let vectors = try CleanupInteractionVectors.load()
        let started = AsyncStream<Void>.makeStream()
        let reply = AsyncStream<Void>.makeStream()
        let stopping = AsyncStream<Void>.makeStream()
        let mock = MockRuntimeTransport { command, args in
            switch command {
            case "cleanup.hiddenDryRun":
                if args.contains(#""start""#) {
                    started.continuation.yield()
                    var waiting = reply.stream.makeAsyncIterator()
                    _ = await waiting.next()
                    return try JSONEncoder().encode(vectors.hiddenStart)
                }
                XCTAssertTrue(args.contains(#""cancel""#))
                XCTAssertTrue(args.contains(vectors.hiddenStart.scan.id))
                return try JSONEncoder().encode(vectors.hiddenPage)
            case "remote.select":
                return Data(#"{"target":"remote","revision":1,"configured":true,"activeMachineId":"next"}"#.utf8)
            case "remote.invoke":
                XCTAssertTrue(args.contains("system.capabilities"))
                return Data(#"{"host_surface":"linux_server_api","capabilities":{}}"#.utf8)
            default:
                throw AgentHubFFIError("unexpected command")
            }
        }
        let store = RuntimeConnectionStore(transport: mock, defaults: UserDefaults(suiteName: UUID().uuidString)!)
        let source = store.revision
        let scan = HiddenCleanupScan { request in
            try await store.rpc("cleanup.hiddenDryRun", args: CleanupRequestEnvelope(request: request), at: source)
        }
        let token = store.registerBeforeMachineChange {
            let done = scan.stop()
            stopping.continuation.yield()
            await done?.value
        }
        defer { store.unregisterBeforeMachineChange(token) }
        let run = Task { try await scan.run { _, _ in XCTFail("published a stopped scan") } }
        var opening = started.stream.makeAsyncIterator()
        _ = await opening.next()
        let switching = Task { try await store.selectMachine(target: .remote, machineId: "next") }
        var cancelling = stopping.stream.makeAsyncIterator()
        _ = await cancelling.next()
        XCTAssertTrue(store.isBusyForSwitch)
        do { try await store.selectMachine(target: .remote, machineId: "other"); XCTFail("concurrent switch") }
        catch { XCTAssertEqual(store.revision, source) }
        reply.continuation.yield()
        try await run.value
        try await switching.value
        XCTAssertTrue(store.ready)
        XCTAssertNil(store.capabilitiesError)
        XCTAssertEqual(mock.recordedCalls.map(\.command), ["cleanup.hiddenDryRun", "cleanup.hiddenDryRun", "remote.select", "remote.invoke"])
    }
}
