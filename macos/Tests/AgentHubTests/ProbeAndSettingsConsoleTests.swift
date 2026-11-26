@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class ProbeAndSettingsConsoleTests: XCTestCase {
    func testProtocolTwoRequiredFieldsAndConfiguredHome() throws {
        let decoder = JSONDecoder()
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: ProbeSettingsFixture.data) as? [String: Any])
        var settings = fixture
        var codex = try XCTUnwrap(settings["codex"] as? [String: Any])
        codex["configured_codex_home"] = "/fixture/profile"
        settings["codex"] = codex
        let view = try decoder.decode(ProbeSettingsView.self, from: JSONSerialization.data(withJSONObject: settings))
        XCTAssertEqual(ProbeSettingsDraft.from(view).codexHome, "/fixture/profile")
        var probe = try XCTUnwrap(settings["probe"] as? [String: Any])
        for invalid in [nil, "15"] {
            probe["poll_seconds"] = invalid
            settings["probe"] = probe
            let data = try JSONSerialization.data(withJSONObject: settings)
            XCTAssertThrowsError(try decoder.decode(ProbeSettingsView.self, from: data))
        }
        let summary = #"{"id":"fixture","title":"T","status":"Recent","message_count":0}"#
        XCTAssertNil(try decoder.decode(ThreadSummary.self, from: Data(summary.utf8)).storageSize)
        XCTAssertThrowsError(try decoder.decode(ThreadSummary.self, from: Data(summary.replacingOccurrences(of: "message_count", with: "messageCount").utf8)))
    }

    func testGithubCiCatalogSelectionAndTop20RunFilter() {
        let repos = [
            GithubCiRepository(id: 1, fullName: "org/active", archived: false),
            GithubCiRepository(id: 2, fullName: "org/archived", archived: true),
        ]
        let runs: [GithubCiRun] = (1...25).map { (idx: Int) -> GithubCiRun in
            let conclusion: String = (idx == 2) ? "failure" : "success"
            let runUrl: String = "https://github.com/org/active/actions/runs/\(idx)"
            let isBotRun: Bool = (idx == 1)
            return GithubCiRun(
                id: UInt64(idx),
                repositoryId: 1,
                repositoryName: "org/active",
                name: "CI",
                status: "completed",
                conclusion: conclusion,
                htmlUrl: runUrl,
                updatedAt: "2026-11-19T00:00:00Z",
                isBot: isBotRun
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
        var draft = ProbeSettingsDraft.from(try ProbeSettingsFixture.settings())
        XCTAssertTrue(draft.validationErrors().isEmpty)

        draft.pollSecondsText = "4"
        XCTAssertFalse(draft.validationErrors().isEmpty)
        draft.pollSecondsText = "15"

        draft.logMaxMiBText = "1e30"
        XCTAssertFalse(draft.validationErrors().isEmpty, "Huge float must fail range validation without trapping")
        draft.logMaxMiBText = "-1e30"
        XCTAssertFalse(draft.validationErrors().isEmpty)

        draft.logMaxMiBText = "0.001"
        XCTAssertFalse(draft.validationErrors().isEmpty)
        draft.logMaxMiBText = "0.00390625" // 4 KiB lower bound
        XCTAssertTrue(draft.validationErrors().isEmpty)

        draft.codexHome = "auto"
        draft.barkDeviceKey = "fixture-bark-key"
        draft.notifyGithubCi = true
        draft.githubCiRepositories = [" lich13/agenthub "]

        let settingsPayload = try draft.buildSavePayload(for: .settings)
        if case let .object(codex)? = settingsPayload["codex"] {
            XCTAssertEqual(codex["home"], .null)
        } else {
            XCTFail("Missing codex object")
        }
        XCTAssertNil(settingsPayload["notifications"], "Settings scope must not send duplicate top-level notifications")

        let barkPayload = try draft.buildSavePayload(for: .bark)
        XCTAssertNil(barkPayload["codex"], "Bark scoped save must not submit codex settings")
        XCTAssertNil(barkPayload["notifications"], "Bark scoped save must send device_key once inside probe.notifications")
        if case let .object(probe)? = barkPayload["probe"],
           case let .object(notif)? = probe["notifications"]
        {
            XCTAssertEqual(notif["device_key"], .string("fixture-bark-key"))
            XCTAssertNil(notif["notify_github_ci"])
        } else {
            XCTFail("Expected probe.notifications.device_key in bark scoped payload")
        }

        let ciPayload = try draft.buildSavePayload(for: .ci)
        XCTAssertNil(ciPayload["codex"])
        if case let .object(probe)? = ciPayload["probe"],
           case let .object(notif)? = probe["notifications"]
        {
            XCTAssertEqual(notif["notify_github_ci"], .bool(true))
            XCTAssertEqual(notif["github_ci_repositories"], .array([.string("lich13/agenthub")]))
            XCTAssertNil(notif["device_key"])
        } else {
            XCTFail("Expected probe.notifications CI fields in CI scoped payload")
        }
    }

    @MainActor
    func testOffscreenShellGeometryAndHitTargetsZeroPointDrift() async throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }

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

        let store = RuntimeConnectionStore(transport: .emptyDesktop, defaults: defaults)
        await store.bootstrap()
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
        // Collapsed SideNav is 48.0pt (bottom-origin y = 200 is 700pt from top, below all 10 nav items): x = 24..45 is railBg, x = 52..342 is 296pt SessionList, x = 348 is Stage (48+296=344)!
        assertPixelNear(collapsedPixels, width: 1440, x: 24, y: 200, expected: (248, 249, 251), label: "Collapsed 48pt SideNav")
        assertPixelNear(collapsedPixels, width: 1440, x: 45, y: 200, expected: (248, 249, 251), label: "Left of 48pt collapsed divider")
        assertPixelNear(collapsedPixels, width: 1440, x: 52, y: 200, expected: (243, 245, 248), label: "Right of 48pt collapsed divider")
        assertPixelNear(collapsedPixels, width: 1440, x: 342, y: 200, expected: (243, 245, 248), label: "Inside 48+296=344pt sidebar")
        assertPixelNear(collapsedPixels, width: 1440, x: 348, y: 200, expected: (255, 255, 255), label: "Right of 344pt sidebar divider")
        store.toggleNavCollapsed()

        XCTAssertEqual(LayoutMetrics.subagentDrawerWidth, 420.0, accuracy: 0.0)
        XCTAssertEqual(LayoutMetrics.formControlHeight, 36.0, accuracy: 0.0)

        // 2. Verify 420.0pt SubagentPanelView docked width & 36.0pt button control row height + full-rect edge hit-testing
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

    func testProbeDisclosureRulesSingleShotAutoOpenAndManualCollapseAcrossPolls() {
        var rules = ProbeDisclosureRules(machineScope: "local")
        for sec in ProbeDisclosureSection.allCases {
            XCTAssertFalse(rules.isExpanded(sec), "\(sec.rawValue) must default to collapsed")
        }

        // First observation of a CI failure auto-opens once
        let run102 = GithubCiRun(
            id: 102,
            repositoryId: 1,
            repositoryName: "org/repo",
            name: "CI",
            status: "completed",
            conclusion: "failure",
            htmlUrl: "https://example.invalid/runs/102",
            updatedAt: "2026-11-25T10:00:00Z"
        )
        let sig102 = ProbeDisclosureRules.ciIssueSignature(errorMessage: nil, snapshotError: nil, visibleRuns: [run102])
        XCTAssertTrue(rules.observeIssue(machineScope: "local", section: .ci, issueSignature: sig102))
        XCTAssertTrue(rules.isExpanded(.ci))

        // User manually collapses CI -> subsequent 15s polls with same signature MUST NOT reopen
        rules.setExpanded(.ci, false)
        XCTAssertFalse(rules.isExpanded(.ci))
        XCTAssertFalse(rules.observeIssue(machineScope: "local", section: .ci, issueSignature: sig102))
        XCTAssertFalse(rules.isExpanded(.ci))

        // New issue signature (#103 alongside #102) auto-opens once
        let run103 = GithubCiRun(
            id: 103,
            repositoryId: 1,
            repositoryName: "org/repo",
            name: "CI",
            status: "completed",
            conclusion: "timed_out",
            htmlUrl: "https://example.invalid/runs/103",
            updatedAt: "2026-11-25T10:05:00Z"
        )
        let sigBoth = ProbeDisclosureRules.ciIssueSignature(errorMessage: nil, snapshotError: nil, visibleRuns: [run102, run103])
        XCTAssertTrue(rules.observeIssue(machineScope: "local", section: .ci, issueSignature: sigBoth))
        XCTAssertTrue(rules.isExpanded(.ci))

        // User collapses CI; when #102 resolves or scrolls off the 20-run window leaving [#103], it MUST stay collapsed
        rules.setExpanded(.ci, false)
        let sig103Only = ProbeDisclosureRules.ciIssueSignature(errorMessage: nil, snapshotError: nil, visibleRuns: [run103])
        XCTAssertFalse(rules.observeIssue(machineScope: "local", section: .ci, issueSignature: sig103Only))
        XCTAssertFalse(rules.isExpanded(.ci))

        // Clearing all issues resets active tokens without opening; a future recurrence of #102 re-opens once
        XCTAssertFalse(rules.observeIssue(machineScope: "local", section: .ci, issueSignature: nil))
        XCTAssertFalse(rules.isExpanded(.ci))
        XCTAssertTrue(rules.observeIssue(machineScope: "local", section: .ci, issueSignature: sig102))
        XCTAssertTrue(rules.isExpanded(.ci))

        // Switching machineScope resets all open sections & seen issue signatures
        rules.resetIfMachineChanged("remote:prod")
        XCTAssertFalse(rules.isExpanded(.ci))
        XCTAssertTrue(rules.observeIssue(machineScope: "remote:prod", section: .ci, issueSignature: sig103Only))
        XCTAssertTrue(rules.isExpanded(.ci))
    }

    @MainActor
    func testAttachmentViewportManagerCancelsStaleScopeReadsWithoutLeakingImages() async throws {
        let oneByOnePngBase64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
        let suite = "com.lich13.agenthub.attscope.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let mock = MockRuntimeTransport { command, _ in
            if command == "sessions.attachmentRead" {
                try? await Task.sleep(nanoseconds: 80_000_000)
                return Data(#"{"mime_type":"image/png","base64":"\#(oneByOnePngBase64)"}"#.utf8)
            }
            return Data("{}".utf8)
        }
        let store = RuntimeConnectionStore(transport: mock, defaults: defaults)
        let manager = AttachmentViewportManager()
        manager.resetScope("local::codex::s1")

        let req = SessionAttachmentRequest(
            provider: .codex,
            sessionKey: "s1",
            messageId: "u-1",
            attachmentId: "att-1"
        )
        manager.ensureLoaded(key: "local::codex::s1:u-1:att-1", request: req, store: store)
        XCTAssertEqual(manager.inFlightCount, 1)

        // Rapidly switch to Session 2 before Session 1's RPC finishes
        manager.resetScope("local::codex::s2")
        XCTAssertEqual(manager.inFlightCount, 0)
        XCTAssertEqual(manager.loadedCount, 0)

        try await Task.sleep(nanoseconds: 130_000_000)
        XCTAssertEqual(manager.loadedCount, 0, "Stale session attachment completion must not populate loadedImages")
    }

    func testScopedSavePreservesOtherPanelsUncommittedEdits() throws {
        let base = ProbeSettingsDraft.from(try ProbeSettingsFixture.settings())
        var loaded = base
        var draft = base

        // Simultaneously edit CI, Settings, and Bark panels
        draft.notifyGithubCi = true
        draft.githubCiRepositories = ["org/repo"]
        draft.pollSecondsText = "30"
        draft.barkGroup = "CustomGroup"

        XCTAssertFalse(draft.subsetEquals(loaded, scope: .ci))
        XCTAssertFalse(draft.subsetEquals(loaded, scope: .settings))
        XCTAssertFalse(draft.subsetEquals(loaded, scope: .bark))

        // Simulate server returning refreshed settings after saving ONLY `.ci`
        var serverSavedCi = base
        serverSavedCi.notifyGithubCi = true
        serverSavedCi.githubCiRepositories = ["org/repo"]
        loaded.applySubset(from: serverSavedCi, scope: .ci)
        draft.applySubset(from: serverSavedCi, scope: .ci)

        XCTAssertTrue(draft.subsetEquals(loaded, scope: .ci), "Saved CI panel must now be clean")
        XCTAssertFalse(draft.subsetEquals(loaded, scope: .settings), "Uncommitted Settings edits must remain dirty")
        XCTAssertFalse(draft.subsetEquals(loaded, scope: .bark), "Uncommitted Bark edits must remain dirty")
        XCTAssertEqual(draft.pollSecondsText, "30")
        XCTAssertEqual(draft.barkGroup, "CustomGroup")

        // Verify every sub-group in `.settings` equality participates independently
        let mutations: [(inout ProbeSettingsDraft) -> Void] = [
            { $0.clearAccountScope.toggle() },
            { $0.logMaxMiBText = "64" },
            { $0.notifyCodex.toggle() },
            { $0.notifyGrokbot.toggle() },
            { $0.notifyClaudeReplyNeeded.toggle() },
            { $0.notifyAntigravityAuthorization.toggle() },
            { $0.notifyDshAuthorization.toggle() },
        ]
        for mutate in mutations {
            var candidate = base
            mutate(&candidate)
            XCTAssertFalse(candidate.subsetEquals(base, scope: .settings))
            XCTAssertTrue(candidate.subsetEquals(base, scope: .ci))
            XCTAssertTrue(candidate.subsetEquals(base, scope: .bark))
        }
    }

    @MainActor
    func testProbeWorkspaceCollapsedHeightDangerContrastAndNarrowSearchModalFit() async throws {
        // 1. Contrast check for DangerButtonStyle(soft: false) & CapsuleSwitch off-border across Light and Dark
        for scheme in [ColorSchemeKind.light, ColorSchemeKind.dark] {
            let pal = DesignPalette.resolve(scheme: scheme, workspace: .probe)
            XCTAssertGreaterThan(
                RGBAColor.contrastRatio(pal.errFg, pal.buttonFg),
                7.0,
                "DangerButtonStyle(soft: false) contrast must exceed 7:1 in \(scheme)"
            )
            XCTAssertGreaterThan(
                RGBAColor.contrastRatio(pal.textMuted, pal.surface),
                3.0,
                "CapsuleSwitch off-state border contrast must exceed 3:1 in \(scheme)"
            )
        }

        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }

        // 2. Default healthy collapsed ProbeWorkspaceView content height fits inside < 680pt single screen!
        let suite = "com.lich13.agenthub.probelayout.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let transport = MockRuntimeTransport { cmd, _ in
            switch cmd {
            case "remote.get":
                return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
            case "system.capabilities":
                return protocol2CapabilitiesFixture(remote: false)
            case "probe.status":
                return Data(#"{"available":true,"hook_status":"managed","error_monitor_status":"ok","error_monitor_incident_count":0,"running_count":1,"reply_needed_count":0,"recoverable_count":0}"#.utf8)
            case "probe.settings.get":
                return ProbeSettingsFixture.data
            case "probe.events":
                return Data(#"{"limit":10,"events":[]}"#.utf8)
            case "github.ci.status":
                return Data(#"{"enabled":true,"refreshing":false,"monitored_repositories":["octocat/AgentHub"],"repositories":[],"runs":[]}"#.utf8)
            case "claude.hooks", "grok.hooks", "antigravity.hooks":
                return Data(#"{"installed":true,"managedCount":1,"changed":false,"configPath":"~/.config/settings.json"}"#.utf8)
            case "jobs.list":
                return Data("[]".utf8)
            default:
                return Data("{}".utf8)
            }
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        let win = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1084, height: 820),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        let host = NSHostingView(
            rootView: ProbeWorkspaceView(store: store)
                .environment(\.agentHubPalette, DesignPalette.resolve(scheme: .light, workspace: .probe))
                .frame(width: 1084, height: 820)
        )
        win.contentView = host
        win.orderBack(nil)
        try await Task.sleep(nanoseconds: 250_000_000)
        host.layoutSubtreeIfNeeded()

        func probeScrollDocumentHeight(in view: NSView) -> CGFloat? {
            if let sv = view as? NSScrollView, let doc = sv.documentView {
                doc.layoutSubtreeIfNeeded()
                return doc.fittingSize.height > 0 ? doc.fittingSize.height : doc.bounds.height
            }
            for sub in view.subviews {
                if let h = probeScrollDocumentHeight(in: sub) { return h }
            }
            return nil
        }
        let collapsedDocHeight = try XCTUnwrap(probeScrollDocumentHeight(in: host))
        XCTAssertLessThan(collapsedDocHeight, 680.0, "Default healthy ProbeWorkspaceView content must fit under 680pt (got \(collapsedDocHeight)pt)")

        // 3. Narrow 1000x680 SessionSearchModalView fits within 680pt height without vertical overflow
        let modalHost = NSHostingView(
            rootView: SessionSearchModalView(
                store: store,
                provider: .codex,
                currentSessionKey: "th-1",
                onSelectResult: { _ in },
                onClose: {}
            )
            .environment(\.agentHubPalette, DesignPalette.resolve(scheme: .light, workspace: .codex))
            .frame(width: 1000, height: 680)
        )
        modalHost.frame = NSRect(x: 0, y: 0, width: 1000, height: 680)
        modalHost.layoutSubtreeIfNeeded()
        XCTAssertEqual(modalHost.bounds.height, 680.0, accuracy: 0.0)
    }

    func testOutOfOrderSettingsReadIsFencedAfterScopedSaveSucceeds() throws {
        let initialView = try ProbeSettingsFixture.settings()
        var sync = ProbeSettingsSyncState(revision: 3)

        // 1. Initial load populates draft & baseline
        let f0 = sync.beginSettingsRead()
        XCTAssertTrue(sync.applyLoadedSettings(initialView, readFence: f0, revision: 3))
        XCTAssertEqual(sync.draft?.barkServerUrl, initialView.notifications.serverUrl)

        // 2. User edits Bark server URL AND leaves an uncommitted edit in CI panel
        sync.draft?.barkServerUrl = "https://bark.example.invalid"
        sync.draft?.githubCiRepositories = ["org/uncommitted"]
        let submitted = try XCTUnwrap(sync.draft)

        // 3. Concurrent refreshAll starts `probe.settings.get` BEFORE `saveSettings` finishes
        let staleReadFence = sync.beginSettingsRead()

        // 4. `saveSettings(scope: .bark)` finishes FIRST with updated server URL
        var savedView = initialView
        savedView.notifications.serverUrl = "https://bark.example.invalid"
        sync.applySavedSettings(savedView, submittedDraft: submitted, scope: .bark, revision: 3)
        XCTAssertEqual(sync.draft?.barkServerUrl, "https://bark.example.invalid")
        XCTAssertEqual(sync.loadedDraft?.barkServerUrl, "https://bark.example.invalid")
        XCTAssertEqual(sync.draft?.githubCiRepositories, ["org/uncommitted"])

        // 5. In-flight stale `probe.settings.get` returns AFTER save (carrying old serverUrl!)
        //    Must be fenced/rejected so the newly saved `.bark` subset is NOT overwritten!
        XCTAssertFalse(sync.applyLoadedSettings(initialView, readFence: staleReadFence, revision: 3))
        XCTAssertFalse(sync.applySettingsReadError("transient read error", readFence: staleReadFence))
        XCTAssertNil(sync.settingsError)
        XCTAssertEqual(sync.draft?.barkServerUrl, "https://bark.example.invalid")
        XCTAssertEqual(sync.loadedDraft?.barkServerUrl, "https://bark.example.invalid")
        XCTAssertEqual(sync.draft?.githubCiRepositories, ["org/uncommitted"])

        // 6. A fresh read started AFTER the save succeeds and preserves dirty `.ci` while keeping `.bark`
        let freshFence = sync.beginSettingsRead()
        XCTAssertTrue(sync.applyLoadedSettings(savedView, readFence: freshFence, revision: 3))
        XCTAssertEqual(sync.draft?.barkServerUrl, "https://bark.example.invalid")
        XCTAssertEqual(sync.draft?.githubCiRepositories, ["org/uncommitted"])

        // 7. Hook install/remove flag updates also advance `settingsFence` so stale reads cannot revert `manageStopHook`
        let staleHookFence = sync.beginSettingsRead()
        sync.applyHookManageFlag(true)
        XCTAssertEqual(sync.draft?.manageStopHook, true)
        var oldHookView = savedView
        oldHookView.probe.hooks.manageStopHook = false
        XCTAssertFalse(sync.applyLoadedSettings(oldHookView, readFence: staleHookFence, revision: 3))
        XCTAssertEqual(sync.draft?.manageStopHook, true)
    }

    @MainActor
    func testMissingTrackedJobPublishesTerminalEntryAndClearsActiveBusyState() async throws {
        let suite = "com.lich13.agenthub.missingjob.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let transport = MockRuntimeTransport { cmd, payload in
            if cmd == "remote.get" {
                return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
            }
            if cmd == "jobs.detail" {
                if payload.contains("job-null") {
                    return Data("null".utf8)
                }
                if payload.contains("job-404") {
                    throw AgentHubFFIError("job not found: job-404")
                }
            }
            return Data("{}".utf8)
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        store.trackStartedJob("job-null")
        store.trackStartedJob("job-404")
        XCTAssertTrue(store.isTrackedJobActive("job-null"))
        XCTAssertTrue(store.isTrackedJobActive("job-404"))

        try await Task.sleep(nanoseconds: 80_000_000)

        XCTAssertFalse(store.isTrackedJobActive("job-null"), "nil jobs.detail must clear active job busy state")
        XCTAssertFalse(store.isTrackedJobActive("job-404"), "job not found must clear active job busy state")
        XCTAssertEqual(store.trackedJobs["job-null"]?.isTerminal, true)
        XCTAssertEqual(store.trackedJobs["job-404"]?.isTerminal, true)
    }

    func testParsedUserMessageCacheCountsContextBlocksAndEnforcesTotalByteBudget() {
        UserContextEngine._resetParsedCacheForTesting()
        defer { UserContextEngine._resetParsedCacheForTesting() }

        // 1. Short `text` with > 256 KiB `contextBlocks` must NOT be cached
        let oversizedContext = [
            UserContextBlock(kind: "files", text: String(repeating: "x", count: 300_000)),
        ]
        let (projBig, _) = UserContextEngine.projectAndSegmentUserMessage(
            "fix this",
            contextBlocks: oversizedContext
        )
        XCTAssertEqual(projBig.text, "fix this")
        XCTAssertEqual(projBig.contextBlocks.count, 1)
        let afterOversized = UserContextEngine._parsedCacheStatsForTesting()
        XCTAssertEqual(afterOversized.count, 0, "oversized contextBlocks entry must bypass parsedCache")
        XCTAssertEqual(afterOversized.totalBytes, 0)

        // 2. Inserting 100 x ~120 KiB entries (~12 MiB > 8 MiB budget) evicts oldest LRU entries within 8 MiB
        let mediumChunk = String(repeating: "a", count: 120_000)
        for idx in 0..<100 {
            _ = UserContextEngine.projectAndSegmentUserMessage(
                "msg-\(idx)",
                contextBlocks: [UserContextBlock(kind: "instructions", text: "\(idx)-\(mediumChunk)")]
            )
        }
        let stats = UserContextEngine._parsedCacheStatsForTesting()
        XCTAssertGreaterThan(stats.count, 0)
        XCTAssertLessThan(stats.count, 100, "LRU eviction must trim entries before 256 count cap when 8 MiB budget is reached")
        XCTAssertLessThanOrEqual(stats.totalBytes, 8 * 1_048_576, "parsedCache total bytes must never exceed 8 MiB")
    }

    @MainActor
    func testCellConfigSignatureReconfiguresWhenImageAttachmentBeyondIndex64FinishesLoading() async throws {
        let suite = "com.lich13.agenthub.att64.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let oneByOnePngBase64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
        let transport = MockRuntimeTransport { cmd, _ in
            if cmd == "sessions.attachmentRead" {
                throw AgentHubFFIError("deferred")
            }
            return Data("{}".utf8)
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        let coordinator = ConversationTableCoordinator(store: store)
        coordinator.sessionScopeKey = "local::codex::s64"
        coordinator.attachmentViewport.resetScope("local::codex::s64")

        let attachments: [UserAttachment] = (0..<70).map { (idx: Int) -> UserAttachment in
            UserAttachment(
                id: "att-\(idx)",
                name: "img-\(idx).png",
                kind: "image"
            )
        }
        let item = ConversationRowItem(
            id: "u-70",
            timelineId: "u-70",
            kind: .user(
                message: UserMessageContent(id: "u-70", text: "70 attachments", attachments: attachments),
                rawText: "70 attachments",
                attachmentsAsNamesOnly: false
            )
        )

        let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("test-att-65"))
        cell.configure(
            item: item,
            leadingInset: 24,
            trailingInset: 24,
            contentWidth: 680,
            highlighted: false,
            coordinator: coordinator
        )
        try await Task.sleep(nanoseconds: 40_000_000)

        let stateBefore = coordinator.loadedAttachmentState(for: item)
        XCTAssertEqual(stateBefore.lowMask, 0)
        XCTAssertTrue(stateBefore.overflowIndices.isEmpty)

        // Now allow ONLY attachment at index 65 (>= 64) to succeed!
        transport.setHandler { cmd, payload in
            if cmd == "sessions.attachmentRead", payload.contains("att-65") {
                return Data(#"{"mimeType":"image/png","base64":"\#(oneByOnePngBase64)"}"#.utf8)
            }
            if cmd == "sessions.attachmentRead" {
                throw AgentHubFFIError("deferred")
            }
            return Data("{}".utf8)
        }
        let req65 = SessionAttachmentRequest(
            provider: .codex,
            sessionKey: "s64",
            messageId: "u-70",
            attachmentId: "att-65"
        )
        coordinator.attachmentViewport.ensureLoaded(
            key: "local::codex::s64:u-70:att-65",
            request: req65,
            store: store
        )
        try await Task.sleep(nanoseconds: 60_000_000)

        let stateAfter = coordinator.loadedAttachmentState(for: item)
        XCTAssertNotEqual(stateBefore, stateAfter, "Loaded attachment at index >= 64 must change LoadedAttachmentState")
        XCTAssertEqual(stateAfter.lowMask, 0)
        XCTAssertEqual(stateAfter.overflowIndices, [65])

        let previousFirstArranged = cell.containerStack.arrangedSubviews.first
        cell.configure(
            item: item,
            leadingInset: 24,
            trailingInset: 24,
            contentWidth: 680,
            highlighted: false,
            coordinator: coordinator
        )
        XCTAssertFalse(
            cell.containerStack.arrangedSubviews.first === previousFirstArranged,
            "CellConfigSignature change at attachment index 65 must rebuild cell subviews"
        )
    }

    @MainActor
    func testMeasureFileAttachmentCardSizeChromeCopyAndClampingBoundaries() {
        let withPath = UserAttachment(id: "f1", name: "spec.md", kind: "file", path: "/workspace/spec.md")
        let emptyPath = UserAttachment(id: "f2", name: "spec.md", kind: "file", path: "")
        let withReason = UserAttachment(
            id: "f3",
            name: "a.txt",
            kind: "file",
            path: "/workspace/a.txt",
            reason: "附件体积超过上限，已跳过内联预览"
        )

        let copyable = ConversationTableCoordinator.measureFileAttachmentCardSize(
            att: withPath,
            namesOnly: false,
            maxWidth: 500
        )
        let namesOnly = ConversationTableCoordinator.measureFileAttachmentCardSize(
            att: withPath,
            namesOnly: true,
            maxWidth: 500
        )
        let blankPath = ConversationTableCoordinator.measureFileAttachmentCardSize(
            att: emptyPath,
            namesOnly: false,
            maxWidth: 500
        )
        XCTAssertEqual(copyable.height, 40)
        XCTAssertEqual(namesOnly.width, blankPath.width, accuracy: 0.001)
        if namesOnly.width > 120 && copyable.width < 340 {
            XCTAssertEqual(copyable.width - namesOnly.width, 36, accuracy: 0.001, "Copy action adds 8pt spacing + 28pt button")
        }

        let reasoned = ConversationTableCoordinator.measureFileAttachmentCardSize(
            att: withReason,
            namesOnly: false,
            maxWidth: 500
        )
        XCTAssertEqual(reasoned.height, 52)
        XCTAssertGreaterThan(reasoned.width, copyable.width)
        XCTAssertLessThanOrEqual(reasoned.width, 340)

        let narrow = ConversationTableCoordinator.measureFileAttachmentCardSize(
            att: withReason,
            namesOnly: false,
            maxWidth: 88
        )
        XCTAssertEqual(narrow.width, 88, accuracy: 0.001, "Card width must never exceed narrow maxWidth < 120")
        XCTAssertTrue(
            ConversationTableCoordinator.layoutUserAttachmentRows(attachments: [], namesOnly: false, maxWidth: 400).isEmpty
        )
    }

    @MainActor
    private func renderShellPixels(store: RuntimeConnectionStore, size: NSSize) throws -> [UInt8] {
        try OffscreenRenderHelper.renderSwiftUIView(AppShellView(store: store), size: size, appearance: .aqua)
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
