@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class GithubCiAndPathActionTests: XCTestCase {
    func testCiNotificationClickIdentifierExtractsHTTPSRunURL() {
        let id = "https://github.com/lich13/agenthub/actions/runs/12345#agenthub-ci:repo:12345:1"
        XCTAssertEqual(
            CiNotificationCenterBridge.extractClickTargetURL(from: id)?.absoluteString,
            "https://github.com/lich13/agenthub/actions/runs/12345"
        )
        XCTAssertNil(CiNotificationCenterBridge.extractClickTargetURL(from: "file:///etc/passwd#agenthub-ci:1"))
        XCTAssertNil(CiNotificationCenterBridge.extractClickTargetURL(from: "https://github.com/no-marker"))
    }

    func testFinderRevealPathValidationRejectsRelativeAndControlChars() {
        XCTAssertNoThrow(try FileOpenerService.validateRevealPath("/tmp/fixture/session.jsonl"))
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("relative/path.jsonl"))
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("/tmp/bad\npath"))
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath(""))
    }

    @MainActor
    func testBoundedInnerScrollViewPassthroughAndMomentumStateMachine() {
        let scroll = BoundedInnerScrollView(frame: NSRect(x: 0, y: 0, width: 400, height: 260))
        let doc = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 180))
        scroll.documentView = doc

        // 1. Non-overflowing content (180 <= 260) always passes through to outer scroll view
        XCTAssertFalse(scroll.shouldConsumeVerticalScroll(deltaY: -12, phase: .began, momentumPhase: []))
        XCTAssertFalse(scroll.shouldConsumeVerticalScroll(deltaY: -12, phase: .changed, momentumPhase: []))

        // 2. Overflowing content (1160 > 260, maxScroll = 900)
        doc.frame = NSRect(x: 0, y: 0, width: 400, height: 1160)
        scroll.contentView.setBoundsOrigin(.zero)

        // At top (originY == 0), scrolling UP (deltaY > 0) passes through to outer Probe page
        XCTAssertFalse(scroll.shouldConsumeVerticalScroll(deltaY: 14, phase: .changed, momentumPhase: []))
        XCTAssertFalse(scroll.trackingInnerGesture)

        // Flyover outer momentum (momentumPhase = .changed without prior inner gesture) passes through
        XCTAssertFalse(scroll.shouldConsumeVerticalScroll(deltaY: -18, phase: [], momentumPhase: .changed))
        XCTAssertFalse(scroll.trackingInnerMomentum)

        // Inner trackpad flick: .began -> .changed (downward) -> .ended -> momentum .began/.changed/.ended
        XCTAssertTrue(scroll.shouldConsumeVerticalScroll(deltaY: 0, phase: .began, momentumPhase: []))
        XCTAssertTrue(scroll.shouldConsumeVerticalScroll(deltaY: -24, phase: .changed, momentumPhase: []))
        XCTAssertTrue(scroll.trackingInnerGesture)
        scroll.contentView.setBoundsOrigin(NSPoint(x: 0, y: 120))
        XCTAssertTrue(scroll.shouldConsumeVerticalScroll(deltaY: 0, phase: .ended, momentumPhase: []))
        XCTAssertTrue(scroll.shouldConsumeVerticalScroll(deltaY: -16, phase: [], momentumPhase: .began))
        XCTAssertTrue(scroll.trackingInnerMomentum)
        XCTAssertTrue(scroll.shouldConsumeVerticalScroll(deltaY: -12, phase: [], momentumPhase: .changed))

        // Hitting bottom boundary (originY == maxScroll == 900) hands off to outer scroll view
        scroll.contentView.setBoundsOrigin(NSPoint(x: 0, y: 900))
        XCTAssertFalse(scroll.shouldConsumeVerticalScroll(deltaY: -8, phase: [], momentumPhase: .changed))
        XCTAssertFalse(scroll.trackingInnerMomentum)
        XCTAssertFalse(scroll.shouldConsumeVerticalScroll(deltaY: -10, phase: .changed, momentumPhase: []))
        // Scrolling back UP from bottom consumes internally
        XCTAssertTrue(scroll.shouldConsumeVerticalScroll(deltaY: 10, phase: .changed, momentumPhase: []))
    }

    private static func makeRunsJSON(count: Int) -> String {
        guard count > 0 else { return "" }
        return (1...count).map { idx in
            #"{"id":\#(5000 + idx),"run_attempt":1,"name":"Backend & contract checks \#(idx)","head_branch":"main","head_sha":"0123456789ab","status":"completed","conclusion":"success","html_url":"https://example.invalid/runs/\#(idx)","created_at":"2026-11-25T10:00:00Z","updated_at":"2026-11-25T10:05:00Z","repository_id":1,"repository_name":"octocat/AgentHub","is_bot":false}"#
        }.joined(separator: ",")
    }

    @MainActor
    func testGithubCiCardBounded20RunsAndAdaptiveShortListHeights() async throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }

        let reposJSON = (1...18).map { i in
            #"{"id":\#(i),"full_name":"octocat/repo-\#(i)","archived":false}"#
        }.joined(separator: ",")

        let suite = "com.lich13.agenthub.cibounded.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        var draftOpt: ProbeSettingsDraft? = ProbeSettingsDraft.from(try ProbeSettingsFixture.settings())
        var rules = ProbeDisclosureRules(machineScope: "local")

        func measureCard(runCount: Int, simulatedError: String? = nil, width: CGFloat, isNarrow: Bool) async throws -> CGFloat {
            let runsJSON = Self.makeRunsJSON(count: runCount)
            let transport = MockRuntimeTransport { cmd, _ in
                switch cmd {
                case "remote.get":
                    return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
                case "system.capabilities":
                    return protocol2CapabilitiesFixture(remote: false)
                case "github.ci.status":
                    let errField = simulatedError.map { "\"\($0)\"" } ?? "null"
                    return Data("{\"account\":{\"id\":1,\"login\":\"octocat\"},\"enabled\":true,\"refreshing\":false,\"error\":\(errField),\"monitored_repositories\":[],\"repositories\":[\(reposJSON)],\"runs\":[\(runsJSON)]}".utf8)
                default:
                    return Data("{}".utf8)
                }
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
            await store.bootstrap()

            let win = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: width, height: 1200),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            let host = NSHostingView(
                rootView: GithubCiCardView(
                    store: store,
                    draft: Binding(get: { draftOpt }, set: { draftOpt = $0 }),
                    disclosureRules: Binding(get: { rules }, set: { rules = $0 }),
                    isNarrow: isNarrow,
                    saving: false,
                    saveMessage: nil,
                    onSave: {}
                )
                .environment(\.agentHubPalette, DesignPalette.resolve(scheme: .light, workspace: .probe))
                .frame(width: width)
            )
            win.contentView = host
            win.orderBack(nil)
            try await Task.sleep(nanoseconds: 180_000_000)
            host.layoutSubtreeIfNeeded()
            let h = host.fittingSize.height
            win.orderOut(nil)
            return h
        }

        // 1. Adaptive 2-run height has zero extra blank padding (< 265pt total card)
        let twoRunsHeight = try await measureCard(runCount: 2, width: 1020, isNarrow: false)
        XCTAssertLessThan(twoRunsHeight, 265.0, "2-run card must wrap tightly to actual content height (got \(twoRunsHeight)pt)")

        // 2. 20-run desktop card height is strictly capped at <= 387pt
        let twentyRunsHeight = try await measureCard(runCount: 20, width: 1020, isNarrow: false)
        XCTAssertGreaterThan(twentyRunsHeight, twoRunsHeight + 80.0)
        XCTAssertLessThanOrEqual(twentyRunsHeight, 387.0, "20-run desktop GithubCiCardView must stay <= 387pt (got \(twentyRunsHeight)pt)")

        // 3. Narrow window + error banner + both repo selector & 20-run list open stays bounded
        rules.setExpanded(.ciRepos, true)
        let narrowDualOpenHeight = try await measureCard(
            runCount: 20,
            simulatedError: "GitHub 请求限流，稍后刷新",
            width: 640,
            isNarrow: true
        )
        XCTAssertLessThan(narrowDualOpenHeight, 720.0, "Narrow dual-scroll card with error must remain bounded (got \(narrowDualOpenHeight)pt)")
    }
}
