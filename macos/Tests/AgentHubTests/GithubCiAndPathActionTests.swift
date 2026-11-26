@testable import AgentHubCore
import AppKit
import SwiftUI
@preconcurrency import UserNotifications
import XCTest

private final class BackgroundCallOutNotificationCenter: UserNotificationCenterScheduling, @unchecked Sendable {
    private let callOutQueue = DispatchQueue(
        label: "com.apple.usernotifications.UNUserNotificationServiceConnection.call-out"
    )
    private let lock = NSLock()
    var authorizationGranted = true
    var authorizationError: (any Error)?
    private(set) var authCallCount = 0
    private(set) var requestedOptions: UNAuthorizationOptions = []
    private(set) var addedRequests: [UNNotificationRequest] = []
    var onFinished: (@Sendable () -> Void)?

    func reset(granted: Bool = true, error: (any Error)? = nil, onFinished: (@Sendable () -> Void)? = nil) {
        lock.lock()
        authorizationGranted = granted
        authorizationError = error
        authCallCount = 0
        requestedOptions = []
        addedRequests.removeAll()
        self.onFinished = onFinished
        lock.unlock()
    }

    func requestAuthorization(
        options: UNAuthorizationOptions,
        completionHandler: @escaping @Sendable (Bool, (any Error)?) -> Void
    ) {
        lock.lock()
        authCallCount += 1
        requestedOptions = options
        let granted = authorizationGranted
        let error = authorizationError
        let finish = onFinished
        lock.unlock()

        callOutQueue.async {
            precondition(!Thread.isMainThread)
            completionHandler(granted, error)
            if !granted || error != nil {
                finish?()
            }
        }
    }

    func add(
        _ request: UNNotificationRequest,
        withCompletionHandler completionHandler: (@Sendable ((any Error)?) -> Void)?
    ) {
        callOutQueue.async { [weak self] in
            precondition(!Thread.isMainThread)
            completionHandler?(nil)
            guard let self else { return }
            self.lock.lock()
            self.addedRequests.append(request)
            let finish = self.onFinished
            self.lock.unlock()
            finish?()
        }
    }
}

private final class BackgroundWorkspaceOpener: WorkspaceApplicationOpening, @unchecked Sendable {
    private let callbackQueue = DispatchQueue(
        label: "test.nsworkspace.openApplication",
        attributes: .concurrent
    )
    private let lock = NSLock()
    var simulatedError: (any Error)?
    private(set) var openedURLs: [URL] = []
    private(set) var createsNewInstanceFlags: [Bool] = []
    var afterCompletion: (@Sendable () -> Void)?

    func openApplication(
        at applicationURL: URL,
        configuration: NSWorkspace.OpenConfiguration,
        completionHandler: (@Sendable (NSRunningApplication?, (any Error)?) -> Void)?
    ) {
        lock.lock()
        openedURLs.append(applicationURL)
        createsNewInstanceFlags.append(configuration.createsNewApplicationInstance)
        let err = simulatedError
        let done = afterCompletion
        lock.unlock()

        callbackQueue.async {
            precondition(!Thread.isMainThread)
            completionHandler?(nil, err)
            done?()
        }
    }
}

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

    @MainActor
    func testCiNotificationDeliverySurvivesBackgroundCallOutQueueAndEdgeCases() async {
        let bridge = CiNotificationCenterBridge()
        let center = BackgroundCallOutNotificationCenter()
        bridge.notificationCenterOverride = center

        // 1. Malformed JSON sets decode error without touching UNUserNotificationCenter
        bridge.deliverAuthorized(eventsJSON: Data("{invalid".utf8))
        XCTAssertEqual(bridge.lastDecodeError, "CI 通知事件解码失败")
        XCTAssertEqual(center.authCallCount, 0)

        // 2. Empty JSON array clears prior decode error and short-circuits before authorization
        bridge.deliverAuthorized(eventsJSON: Data("[]".utf8))
        XCTAssertNil(bridge.lastDecodeError)
        XCTAssertEqual(center.authCallCount, 0)

        let twoEventsJSON = Data(#"""
        [
          {"identifier":"https://example.invalid/runs/1#agenthub-ci:1","title":"CI 失败","body":"Backend checks failed","htmlUrl":"https://example.invalid/runs/1"},
          {"identifier":"https://example.invalid/runs/2#agenthub-ci:2","title":"CI 恢复","body":"macOS checks passed","htmlUrl":"https://example.invalid/runs/2"}
        ]
        """#.utf8)

        // 3. Granted authorization invokes both requestAuthorization & add callbacks on background call-out queue
        let deliveredTwo = expectation(description: "two CI notifications scheduled on background call-out queue")
        deliveredTwo.expectedFulfillmentCount = 2
        center.reset(granted: true, error: nil) {
            deliveredTwo.fulfill()
        }
        bridge.deliverAuthorized(eventsJSON: twoEventsJSON)
        await fulfillment(of: [deliveredTwo], timeout: 3.0)

        XCTAssertNil(bridge.lastDecodeError)
        XCTAssertEqual(center.authCallCount, 1)
        XCTAssertEqual(center.requestedOptions, [.alert, .sound])
        XCTAssertEqual(center.addedRequests.count, 2)
        XCTAssertEqual(
            Set(center.addedRequests.map(\.identifier)),
            [
                "https://example.invalid/runs/1#agenthub-ci:1",
                "https://example.invalid/runs/2#agenthub-ci:2"
            ]
        )
        XCTAssertTrue(center.addedRequests.allSatisfy { $0.content.sound == .default })

        // 4. Denied authorization runs callback on background queue and schedules nothing
        let deniedDone = expectation(description: "denied authorization callback completed")
        center.reset(granted: false, error: nil) {
            deniedDone.fulfill()
        }
        bridge.deliverAuthorized(eventsJSON: twoEventsJSON)
        await fulfillment(of: [deniedDone], timeout: 3.0)
        XCTAssertEqual(center.authCallCount, 1)
        XCTAssertTrue(center.addedRequests.isEmpty)

        // 5. Authorization error runs callback on background queue and schedules nothing
        let errorDone = expectation(description: "errored authorization callback completed")
        center.reset(granted: true, error: AgentHubFFIError("notifications unavailable")) {
            errorDone.fulfill()
        }
        bridge.deliverAuthorized(eventsJSON: twoEventsJSON)
        await fulfillment(of: [errorDone], timeout: 3.0)
        XCTAssertEqual(center.authCallCount, 1)
        XCTAssertTrue(center.addedRequests.isEmpty)
    }

    @MainActor
    func testAppRestartLauncherHandlesBackgroundWorkspaceCallbackAndEdgeCases() async {
        let opener = BackgroundWorkspaceOpener()
        var terminateCount = 0

        // 1. Non-.app path is ignored
        AppRestartLauncher.launchReplacementAndTerminate(
            bundleURL: URL(fileURLWithPath: "/usr/local/bin/agenthub"),
            workspace: opener,
            terminate: { terminateCount += 1 }
        )
        XCTAssertTrue(opener.openedURLs.isEmpty)
        XCTAssertEqual(terminateCount, 0)

        let appURL = URL(fileURLWithPath: "/Applications/AgentHub.app")

        // 2. Launch failure on background queue does not terminate current process
        let failedCallback = expectation(description: "failed launch callback executed")
        opener.simulatedError = AgentHubFFIError("launch failed")
        opener.afterCompletion = { failedCallback.fulfill() }
        AppRestartLauncher.launchReplacementAndTerminate(
            bundleURL: appURL,
            workspace: opener,
            terminate: { terminateCount += 1 }
        )
        await fulfillment(of: [failedCallback], timeout: 3.0)
        try? await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(opener.openedURLs, [appURL])
        XCTAssertEqual(opener.createsNewInstanceFlags, [true])
        XCTAssertEqual(terminateCount, 0)

        // 3. Successful launch on background concurrent queue hops back to MainActor and terminates
        let terminated = expectation(description: "terminated on MainActor after background callback")
        opener.simulatedError = nil
        opener.afterCompletion = nil
        AppRestartLauncher.launchReplacementAndTerminate(
            bundleURL: appURL,
            workspace: opener,
            terminate: {
                XCTAssertTrue(Thread.isMainThread)
                terminateCount += 1
                terminated.fulfill()
            }
        )
        await fulfillment(of: [terminated], timeout: 3.0)
        XCTAssertEqual(terminateCount, 1)
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
