@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class ProbeAndSettingsConsoleTests: XCTestCase {
    func testGithubCiCatalogSelectionAndTop20RunFilter() {
        let repos = [
            GithubCiRepository(id: 1, fullName: "org/active", archived: false),
            GithubCiRepository(id: 2, fullName: "org/archived", archived: true),
        ]
        let runs: [GithubCiRun] = (1...25).map { idx in
            GithubCiRun(
                id: UInt64(idx),
                repositoryId: 1,
                repositoryName: "org/active",
                workflowName: "CI",
                status: "completed",
                conclusion: idx == 2 ? "failure" : "success",
                htmlUrl: "https://github.com/org/active/actions/runs/\(idx)",
                updatedAt: "2026-11-19T00:00:00Z",
                isBot: idx == 1
            )
        }

        let catalog = GithubCiDomain.buildCatalog(repositories: repos, runs: runs, configured: ["org/custom"])
        XCTAssertEqual(catalog.map(\.fullName), ["org/active", "org/custom"])

        let filteredNoBots = GithubCiDomain.visibleRuns(runs: runs, repositories: ["org/active"], filterBots: true)
        XCTAssertEqual(filteredNoBots.count, 20)
        XCTAssertFalse(filteredNoBots.contains(where: { $0.id == 1 }))
        XCTAssertEqual(GithubCiDomain.runStatus(filteredNoBots[0]).label, "失败")
        XCTAssertEqual(GithubCiDomain.runStatus(filteredNoBots[0]).tone, .danger)

        let selected = GithubCiDomain.addSelection(query: "active", catalog: catalog, configured: [])
        XCTAssertEqual(selected, ["org/active"])
        let toggledOff = GithubCiDomain.toggleSelection(catalog[0], configured: selected)
        XCTAssertTrue(toggledOff.isEmpty)
    }

    func testProbeSettingsDraftValidationAndSavePayloadContract() throws {
        var draft = ProbeSettingsDraft.from(ProbeSettingsView())
        XCTAssertTrue(draft.validationErrors().isEmpty)

        draft.pollSecondsText = "4"
        XCTAssertFalse(draft.validationErrors().isEmpty)
        draft.pollSecondsText = "15"

        draft.logMaxMiBText = "0.001"
        XCTAssertFalse(draft.validationErrors().isEmpty)
        draft.logMaxMiBText = "0.00390625" // 4 KiB lower bound
        XCTAssertTrue(draft.validationErrors().isEmpty)

        draft.codexHome = "auto"
        draft.barkDeviceKey = "fixture-bark-key"
        let payload = try draft.buildSavePayload()
        if case let .object(codex)? = payload["codex"] {
            XCTAssertEqual(codex["home"], .null)
        } else {
            XCTFail("Missing codex object")
        }
        if case let .object(notif)? = payload["notifications"] {
            XCTAssertEqual(notif["device_key"], .string("fixture-bark-key"))
        } else {
            XCTFail("Expected top-level notifications.device_key when non-empty")
        }
    }

    @MainActor
    func testOffscreenShellGeometryAndHitTargetsZeroPointDrift() throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }

        // Verify window initial clamp without exceeding visibleFrame
        let clamped = AgentHubWindowManager.clampedInitialFrame(
            visibleFrame: NSRect(x: 0, y: 25, width: 1440, height: 875)
        )
        XCTAssertEqual(clamped.width, 1280, accuracy: 0.0)
        XCTAssertEqual(clamped.height, 820, accuracy: 0.0)

        // Verify reading column horizontal insets exact math
        let desktopRailInsets = LayoutMetrics.readingHorizontalInsets(availableWidth: 1000, hasTimelineRail: true)
        XCTAssertEqual(desktopRailInsets.leading, 116.0, accuracy: 0.0)
        XCTAssertEqual(desktopRailInsets.trailing, 116.0, accuracy: 0.0)

        let narrowRailInsets = LayoutMetrics.readingHorizontalInsets(availableWidth: 820, hasTimelineRail: true)
        XCTAssertEqual(narrowRailInsets.leading, 42.0, accuracy: 0.0)
        XCTAssertEqual(narrowRailInsets.trailing, 46.0, accuracy: 0.0)

        let mobileInsets = LayoutMetrics.readingHorizontalInsets(availableWidth: 767, hasTimelineRail: true)
        XCTAssertEqual(mobileInsets.leading, 32.0, accuracy: 0.0)
        XCTAssertEqual(mobileInsets.trailing, 16.0, accuracy: 0.0)

        // Mount AppShellView in offscreen NSWindow across 3 viewports: 1440x900, 1280x820, 767x820
        let suite = "com.lich13.agenthub.geom.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let mock = MockRuntimeTransport { _, _ in Data("{}".utf8) }
        let store = RuntimeConnectionStore(transport: mock, defaults: defaults)

        for size in [
            NSSize(width: 1440, height: 900),
            NSSize(width: 1280, height: 820),
            NSSize(width: 767, height: 820),
        ] {
            for workspace in [WorkspaceId.probe, .ops, .codex] {
                store.activeWorkspace = workspace
                let win = NSWindow(
                    contentRect: NSRect(origin: .zero, size: size),
                    styleMask: [.borderless],
                    backing: .buffered,
                    defer: false
                )
                let hosting = NSHostingView(rootView: AppShellView(store: store))
                hosting.frame = NSRect(origin: .zero, size: size)
                win.contentView = hosting
                hosting.layoutSubtreeIfNeeded()

                XCTAssertEqual(hosting.bounds.width, size.width, accuracy: 0.0)
                XCTAssertEqual(hosting.bounds.height, size.height, accuracy: 0.0)
                XCTAssertNotNil(hosting.hitTest(NSPoint(x: 20, y: size.height - 20)))
            }
        }
    }
}
