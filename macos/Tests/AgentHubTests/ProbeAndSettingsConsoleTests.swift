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

        // Mount AppShellView & individual shell components in offscreen NSWindows and assert exact 0.0pt geometries + edge hit-testing!
        let suite = "com.lich13.agenthub.geom.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let mock = MockRuntimeTransport { _, _ in Data("[]".utf8) }
        let store = RuntimeConnectionStore(transport: mock, defaults: defaults)
        store.setTheme(.light)
        store.activeWorkspace = .codex

        // 1. Verify SideNav expanded (196.0pt) vs collapsed (48.0pt) exact vertical border x-coordinate in rendered 1440x900 shell
        store.navCollapsed = false
        let expandedPixels = try renderShellPixels(store: store, size: NSSize(width: 1440, height: 900))
        // At y = 450, x = 100 is inside SideNav (--r = #f8f9fb = 248,249,251), x = 250 is inside SessionList (--s = #f3f5f8 = 243,245,248), x = 900 is Stage (--bg = #fff = 255,255,255)
        assertPixelNear(expandedPixels, width: 1440, x: 100, y: 450, expected: (248, 249, 251), label: "Expanded SideNav rail")
        assertPixelNear(expandedPixels, width: 1440, x: 250, y: 450, expected: (243, 245, 248), label: "296pt SessionList column")
        assertPixelNear(expandedPixels, width: 1440, x: 900, y: 450, expected: (255, 255, 255), label: "Main stage column")
        // Verify exact boundary: x = 194 is SideNav, x = 198 is SessionList; x = 490 is SessionList (196+294), x = 495 is Stage!
        assertPixelNear(expandedPixels, width: 1440, x: 194, y: 450, expected: (248, 249, 251), label: "Left of 196pt nav divider")
        assertPixelNear(expandedPixels, width: 1440, x: 198, y: 450, expected: (243, 245, 248), label: "Right of 196pt nav divider")
        assertPixelNear(expandedPixels, width: 1440, x: 490, y: 450, expected: (243, 245, 248), label: "Left of 196+296=492pt sidebar divider")
        assertPixelNear(expandedPixels, width: 1440, x: 495, y: 450, expected: (255, 255, 255), label: "Right of 492pt sidebar divider")

        store.toggleNavCollapsed()
        XCTAssertTrue(store.navCollapsed)
        let collapsedPixels = try renderShellPixels(store: store, size: NSSize(width: 1440, height: 900))
        // Collapsed SideNav is 48.0pt: x = 45 is SideNav, x = 52..342 is 296pt SessionList, x = 348 is Stage (48+296=344)!
        assertPixelNear(collapsedPixels, width: 1440, x: 24, y: 450, expected: (248, 249, 251), label: "Collapsed 48pt SideNav")
        assertPixelNear(collapsedPixels, width: 1440, x: 52, y: 450, expected: (243, 245, 248), label: "Right of 48pt collapsed divider")
        assertPixelNear(collapsedPixels, width: 1440, x: 342, y: 450, expected: (243, 245, 248), label: "Inside 48+296=344pt sidebar")
        assertPixelNear(collapsedPixels, width: 1440, x: 348, y: 450, expected: (255, 255, 255), label: "Right of 344pt sidebar divider")
        store.toggleNavCollapsed()

        // 2. Verify Mobile <= 767pt layout: exact 52.0pt top bar (y=768..819) + 58.0pt bottom tabs (y=0..57) pixel boundaries!
        XCTAssertEqual(LayoutMetrics.mobileTopBarHeight, 52.0, accuracy: 0.0)
        XCTAssertEqual(LayoutMetrics.mobileBottomTabsHeight, 58.0, accuracy: 0.0)
        XCTAssertEqual(LayoutMetrics.subagentDrawerWidth, 420.0, accuracy: 0.0)
        XCTAssertEqual(LayoutMetrics.formControlHeight, 36.0, accuracy: 0.0)

        let mobilePixels = try renderShellPixels(store: store, size: NSSize(width: 767, height: 820))
        // CGContext bitmap row y=0 is bottom of window (58pt bottom tabs: y=0..57), y=819 is top of window (52pt top bar: y=768..819)
        assertPixelNear(mobilePixels, width: 767, x: 380, y: 772, expected: (248, 249, 251), label: "Inside 52pt MobileTopBar (y=772)")
        assertPixelNear(mobilePixels, width: 767, x: 380, y: 760, expected: (243, 245, 248), label: "Below 52pt MobileTopBar in SessionList (y=760)")
        assertPixelNear(mobilePixels, width: 767, x: 380, y: 54, expected: (248, 249, 251), label: "Inside 58pt MobileBottomTabs (y=54)")
        assertPixelNear(mobilePixels, width: 767, x: 380, y: 66, expected: (243, 245, 248), label: "Above 58pt MobileBottomTabs in SessionList (y=66)")

        // 3. Verify 420.0pt SubagentPanelView docked width & 36.0pt button control row height + full-rect edge hit-testing
        let drawerHost = NSHostingView(
            rootView: SubagentPanelView(
                store: store,
                provider: .codex,
                rootSessionKey: "th-1",
                collection: nil,
                scheme: .light,
                workspace: .codex,
                onClose: {}
            )
            .frame(width: LayoutMetrics.subagentDrawerWidth, height: 640)
        )
        drawerHost.frame = NSRect(x: 0, y: 0, width: LayoutMetrics.subagentDrawerWidth, height: 640)
        drawerHost.layoutSubtreeIfNeeded()
        XCTAssertEqual(drawerHost.bounds.width, 420.0, accuracy: 0.0)

        let btnWin = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 220, height: 36), styleMask: [.borderless], backing: .buffered, defer: false)
        let btnHost = NSHostingView(
            rootView: Button("连接远程") {}
                .buttonStyle(PrimaryButtonStyle())
                .frame(width: 220)
        )
        btnHost.frame = NSRect(x: 0, y: 0, width: 220, height: 36)
        btnWin.contentView = btnHost
        btnHost.layoutSubtreeIfNeeded()
        XCTAssertEqual(btnHost.fittingSize.height, 36.0, accuracy: 0.0, "PrimaryButtonStyle control height must be 36.0pt")
        // Edge hit-test at (2, 2) and (218, 34) confirms full-rectangle hit target (not glyph-only!)
        XCTAssertNotNil(btnHost.hitTest(NSPoint(x: 2, y: 2)))
        XCTAssertNotNil(btnHost.hitTest(NSPoint(x: 218, y: 34)))
    }

    @MainActor
    func testClaudeReadOnlyRPCClassificationDoesNotIncrementActiveWrites() async throws {
        final class BusyRecorder: @unchecked Sendable {
            var ownershipBusy = false
            var hooksStatusBusy = false
            var hooksInstallBusy = false
            weak var store: RuntimeConnectionStore?
        }
        let rec = BusyRecorder()

        let suite = "com.lich13.agenthub.rpc.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let mock = MockRuntimeTransport { command, argsJSON in
            let busyNow = await MainActor.run { rec.store?.isBusyForSwitch ?? false }
            if command == "claude.ownershipDiagnostics" {
                rec.ownershipBusy = busyNow
            } else if command == "claude.hooks" {
                if argsJSON.contains("\"status\"") {
                    rec.hooksStatusBusy = busyNow
                } else {
                    rec.hooksInstallBusy = busyNow
                }
            }
            return Data("{}".utf8)
        }
        let store = RuntimeConnectionStore(transport: mock, defaults: defaults)
        rec.store = store

        struct EmptyResp: Decodable {}
        struct HookInner: Encodable { let action: String; let confirmed: Bool }
        struct HookReq: Encodable { let request: HookInner }
        let _: EmptyResp = try await store.rpc("claude.ownershipDiagnostics")
        let _: EmptyResp = try await store.rpc("claude.hooks", args: HookReq(request: .init(action: "status", confirmed: false)))
        let _: EmptyResp = try await store.rpc("claude.hooks", args: HookReq(request: .init(action: "install", confirmed: true)))

        XCTAssertFalse(rec.ownershipBusy, "claude.ownershipDiagnostics is a read command and must not increment writes")
        XCTAssertFalse(rec.hooksStatusBusy, "claude.hooks(action: status) is a read check and must not increment writes")
        XCTAssertTrue(rec.hooksInstallBusy, "claude.hooks(action: install) is a mutating write and must increment writes")
    }

    @MainActor
    private func renderShellPixels(store: RuntimeConnectionStore, size: NSSize) throws -> [UInt8] {
        let win = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.borderless], backing: .buffered, defer: false)
        win.appearance = NSAppearance(named: .aqua)
        let hosting = NSHostingView(rootView: AppShellView(store: store).frame(width: size.width, height: size.height))
        hosting.frame = NSRect(origin: .zero, size: size)
        win.contentView = hosting
        hosting.layoutSubtreeIfNeeded()

        let w = Int(size.width)
        let h = Int(size.height)
        let cs = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let ok = pixels.withUnsafeMutableBytes { raw -> Bool in
            guard let base = raw.baseAddress,
                  let ctx = CGContext(
                      data: base, width: w, height: h,
                      bitsPerComponent: 8, bytesPerRow: w * 4,
                      space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else { return false }
            hosting.wantsLayer = true
            hosting.layer?.render(in: ctx)
            return true
        }
        XCTAssertTrue(ok)
        return pixels
    }

    private func assertPixelNear(
        _ pixels: [UInt8],
        width: Int,
        x: Int,
        y: Int,
        expected: (UInt8, UInt8, UInt8),
        tolerance: Int = 2,
        label: String,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        let idx = (y * width + x) * 4
        XCTAssertLessThanOrEqual(abs(Int(pixels[idx]) - Int(expected.0)), tolerance, "\(label) R at (\(x),\(y))", file: file, line: line)
        XCTAssertLessThanOrEqual(abs(Int(pixels[idx + 1]) - Int(expected.1)), tolerance, "\(label) G at (\(x),\(y))", file: file, line: line)
        XCTAssertLessThanOrEqual(abs(Int(pixels[idx + 2]) - Int(expected.2)), tolerance, "\(label) B at (\(x),\(y))", file: file, line: line)
    }
}
