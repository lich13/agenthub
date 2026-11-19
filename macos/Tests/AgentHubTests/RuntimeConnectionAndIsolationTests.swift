@testable import AgentHubCore
import Foundation
import XCTest

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
    }
}
