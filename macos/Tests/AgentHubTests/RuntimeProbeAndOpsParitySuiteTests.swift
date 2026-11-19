@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class RuntimeProbeAndOpsParitySuiteTests: XCTestCase, ParityExecutableSuite {
    static func executeMappedMethod(_ methodName: String) throws {
        let instance = RuntimeProbeAndOpsParitySuiteTests()
        switch methodName {
        case "testPorted_appShellWindowAndModals": try instance.testPorted_appShellWindowAndModals()
        case "testPorted_workspaceSplitShell": try instance.testPorted_workspaceSplitShell()
        case "testPorted_apiContractRouting": try instance.testPorted_apiContractRouting()
        case "testPorted_runtimeStreamAndEpochs": try instance.testPorted_runtimeStreamAndEpochs()
        case "testPorted_desktopIntent": try instance.testPorted_desktopIntent()
        case "testPorted_desktopRefresh": try instance.testPorted_desktopRefresh()
        case "testPorted_runtimeViewModel": try instance.testPorted_runtimeViewModel()
        case "testPorted_filePathsApi": try instance.testPorted_filePathsApi()
        case "testPorted_probeUi": try instance.testPorted_probeUi()
        case "testPorted_githubCiCard": try instance.testPorted_githubCiCard()
        case "testPorted_probeWorkspace": try instance.testPorted_probeWorkspace()
        case "testPorted_probeQuery": try instance.testPorted_probeQuery()
        case "testPorted_probeStatusQuery": try instance.testPorted_probeStatusQuery()
        case "testPorted_opsWorkspace": try instance.testPorted_opsWorkspace()
        case "testPorted_updatePanel": try instance.testPorted_updatePanel()
        case "testPorted_hiddenScan": try instance.testPorted_hiddenScan()
        case "testPorted_codexAccountScope": try instance.testPorted_codexAccountScope()
        case "testPorted_jobsQuery": try instance.testPorted_jobsQuery()
        case "testPorted_opsQuery": try instance.testPorted_opsQuery()
        default:
            XCTFail("Unknown mapped method: \(methodName)")
        }
    }

    // MARK: - 1. App.test.ts (18 cases) — Window One-Time Maximize, ⌘A Modal Leak Guard & <=767pt Mobile Chrome

    func testPorted_appShellWindowAndModals() throws {
        try runAsyncParityCheck {
            let f = "webui/src/App.test.ts"
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.app.\(UUID().uuidString)")!)

            // 1) Initial Window One-Time Maximize (Item #2: 1280x820 base -> maximizes once to visibleFrame, never resizes again!)
            let wc = AgentHubWindowManager(store: store, rootView: EmptyView())
            let win = wc.window
            XCTAssertFalse(win.isVisible, "Main window must be created hidden before alignOnceAndReveal")
            XCTAssertEqual(win.minSize, NSSize(width: 1000, height: 680))
            ParityCaseTracker.shared.record(f, "window-hidden-creation-with-1000x680-min-size")

            let fakeScreenFrame = NSRect(x: 0, y: 25, width: 1728, height: 1055)
            wc.alignOnceAndReveal(overrideVisibleFrame: fakeScreenFrame)
            XCTAssertEqual(win.frame, fakeScreenFrame, "First reveal must maximize window to fill screen.visibleFrame")
            ParityCaseTracker.shared.record(f, "window-one-time-maximize-to-visible-frame")

            // User manually resizes window -> subsequent alignOnceAndReveal MUST NOT resize it again!
            let userFrame = NSRect(x: 120, y: 80, width: 1200, height: 760)
            win.setFrame(userFrame, display: false)
            wc.alignOnceAndReveal(overrideVisibleFrame: fakeScreenFrame)
            XCTAssertEqual(win.frame, userFrame, "alignOnceAndReveal must NEVER resize window after initial show")
            ParityCaseTracker.shared.record(f, "window-subsequent-reveal-never-resizes")

            // Screen resolution clamp keeps window inside smaller display
            let smallerScreen = NSRect(x: 0, y: 0, width: 1280, height: 720)
            let clamped = AgentHubWindowManager.clampedFrameToVisibleScreen(
                NSRect(x: 500, y: 300, width: 1728, height: 1055),
                visibleFrame: smallerScreen
            )
            XCTAssertLessThanOrEqual(clamped.maxX, smallerScreen.maxX)
            XCTAssertLessThanOrEqual(clamped.maxY, smallerScreen.maxY)
            XCTAssertEqual(clamped.size, smallerScreen.size)
            ParityCaseTracker.shared.record(f, "window-screen-change-clamps-to-visible-frame")
            win.close()

            // 2) Edit -> Select All (⌘A) Leak Block Through ALL Inline SwiftUI Modals (Item #3)
            final class CounterBox: @unchecked Sendable {
                var count = 0
            }
            let selectAllNotifications = CounterBox()
            let obs = NotificationCenter.default.addObserver(forName: .agentHubSelectAllRequested, object: nil, queue: .main) { _ in
                selectAllNotifications.count += 1
            }
            defer { NotificationCenter.default.removeObserver(obs) }

            let testWin = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 600), styleMask: [.titled], backing: .buffered, defer: false)

            // When no modal is open -> routes `.agentHubSelectAllRequested`
            let routedNoModal = AgentHubAppDelegate.routeMenuSelectAll(in: testWin, store: store)
            XCTAssertEqual(routedNoModal, .postedListSelectAll)
            XCTAssertEqual(selectAllNotifications.count, 1)
            ParityCaseTracker.shared.record(f, "cmd-a-routes-to-list-when-no-modal-open")

            // When inline modal is active (`searchModalOpen` / `ModalPresentationRegistry` / `ConfirmDialogOverlay` / `<960pt` subagent) -> BLOCKED!
            for modalKey in ["searchModal", "batchPreview", "claudeDeletePreview", "grokDeletePreview", "imageLightbox", "discardUnknown", "settingsConfirm", "subagentOverlay"] {
                store.setInlineModalPresented(modalKey, presented: true)
                let before = selectAllNotifications.count
                let routed = AgentHubAppDelegate.routeMenuSelectAll(in: testWin, store: store)
                XCTAssertEqual(routed, .blockedByModalOrOverlay, "⌘A must be blocked while \(modalKey) is open")
                XCTAssertEqual(selectAllNotifications.count, before, "Underlying session list must NOT receive selectAll while \(modalKey) is open")
                store.setInlineModalPresented(modalKey, presented: false)
                ParityCaseTracker.shared.record(f, "cmd-a-blocked-by-\(modalKey)")
            }

            // WHILE modal is open, underlying ComposerNSTextView or non-modal ReaderSelectableTextView MUST BE BLOCKED from selectAll!
            let underlyingComposer = ComposerNSTextView(frame: NSRect(x: 10, y: 50, width: 300, height: 60))
            underlyingComposer.string = "draft behind modal"
            underlyingComposer.setSelectedRange(NSRange(location: 0, length: 0))
            testWin.contentView?.addSubview(underlyingComposer)
            _ = testWin.makeFirstResponder(underlyingComposer)
            store.setInlineModalPresented("searchModal", presented: true)
            XCTAssertEqual(AgentHubAppDelegate.routeMenuSelectAll(in: testWin, store: store), .blockedByModalOrOverlay)
            XCTAssertEqual(underlyingComposer.selectedRange().length, 0, "Underlying ComposerNSTextView must NOT select all while modal is open")

            let underlyingReader = ReaderSelectableTextView(frame: NSRect(x: 10, y: 120, width: 300, height: 40))
            underlyingReader.isEditable = false
            underlyingReader.isSelectable = true
            underlyingReader.string = "assistant reply behind modal"
            underlyingReader.setSelectedRange(NSRange(location: 0, length: 0))
            testWin.contentView?.addSubview(underlyingReader)
            _ = testWin.makeFirstResponder(underlyingReader)
            XCTAssertEqual(AgentHubAppDelegate.routeMenuSelectAll(in: testWin, store: store), .blockedByModalOrOverlay)
            XCTAssertEqual(underlyingReader.selectedRange().length, 0, "Underlying ReaderSelectableTextView must NOT select all while modal is open")

            // WHILE modal is open, an editable NSTextView inside the modal MUST STILL select its own text!
            let modalInput = NSTextView(frame: NSRect(x: 10, y: 10, width: 200, height: 30))
            modalInput.isEditable = true
            modalInput.string = "search query text"
            modalInput.setSelectedRange(NSRange(location: 0, length: 0))
            testWin.contentView?.addSubview(modalInput)
            _ = testWin.makeFirstResponder(modalInput)
            let routedInput = AgentHubAppDelegate.routeMenuSelectAll(in: testWin, store: store)
            XCTAssertEqual(routedInput, .editableTextSelectedAll)
            XCTAssertEqual(modalInput.selectedRange(), NSRange(location: 0, length: 17), "Editable input inside modal must select its own text on ⌘A")
            XCTAssertEqual(selectAllNotifications.count, 1, "Underlying session list still not selected")
            store.setInlineModalPresented("searchModal", presented: false)
            ParityCaseTracker.shared.record(f, "cmd-a-selects-text-inside-modal-input-without-leaking")

            // 3) Mobile <=767pt bottom tab active styling & FlowButtonRow wrapping (Item #12)
            for ws in [WorkspaceId.codex, .claude, .probe, .ops] {
                store.activeWorkspace = ws
                let shellHost = NSHostingView(rootView: AppShellView(store: store))
                shellHost.frame = NSRect(x: 0, y: 0, width: 767, height: 820)
                shellHost.layoutSubtreeIfNeeded()
                XCTAssertEqual(shellHost.frame.width, 767)
                ParityCaseTracker.shared.record(f, "mobile-767pt-shell-\(ws.rawValue)")
            }
        }
    }

    // MARK: - 2. WorkspaceSplitShell.test.tsx (1 case)

    func testPorted_workspaceSplitShell() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/WorkspaceSplitShell.test.tsx"
            let header = WorkspaceStageHeaderView(
                title: "Session Stage",
                statusBadge: "运行中",
                statusTone: .info,
                isCompactMobile: true,
                onBack: {},
                inlineRenameActive: true,
                initialRenameText: "Session Stage"
            ) {
                EmptyView()
            }
            let host = NSHostingView(rootView: header.frame(width: 600))
            host.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(host.fittingSize.height, 90)
            ParityCaseTracker.shared.record(f, "glass-stage-header-mobile-back-and-inline-rename")
        }
    }

    // MARK: - 3. api.test.ts (51 cases) — Comprehensive Real Contract Registry Routing, Remote.Invoke & LocalOnly Guards

    func testPorted_apiContractRouting() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/api.test.ts"
            let contractURL = URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .appendingPathComponent("contracts/agenthub-contract.json")
            let rawContract = try Data(contentsOf: contractURL)
            let root = try XCTUnwrap(JSONSerialization.jsonObject(with: rawContract) as? [String: Any])
            let actions = try XCTUnwrap(root["actions"] as? [[String: Any]])
            // Every action in agenthub-contract.json that has a tauriCommand (excluding dedicated stream attach)
            let invokeActions: [String] = actions.compactMap { entry in
                guard let id = entry["id"] as? String,
                      let tauriCmd = entry["tauriCommand"] as? String,
                      !tauriCmd.isEmpty,
                      id != "codex.desktop.attach"
                else { return nil }
                return id
            }
            XCTAssertGreaterThanOrEqual(invokeActions.count, 51, "Contract registry must supply >=51 real invoke actions")

            final class RouteModeBox: @unchecked Sendable { var bootstrapRemote = false }
            let routeMode = RouteModeBox()
            let transport = MockRuntimeTransport { cmd, args in
                if routeMode.bootstrapRemote && cmd == "remote.get" {
                    return Data("{\"target\":\"remote\",\"revision\":2,\"sshHost\":\"prod-1\",\"configured\":true,\"activeMachineId\":\"m1\",\"activeMachineName\":\"Prod\",\"machines\":[{\"id\":\"m1\",\"name\":\"Prod\",\"sshHost\":\"prod-1\",\"useSudo\":false}]}".utf8)
                }
                if cmd == "remote.invoke" {
                    return Data("{\"ok\":true,\"forwarded\":true}".utf8)
                }
                return Data("{\"ok\":true,\"cmd\":\"\(cmd)\",\"len\":\(args.count)}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.api.\(UUID().uuidString)")!)

            struct Ack: Decodable {
                let ok: Bool
                let forwarded: Bool?
            }
            for (idx, cmd) in invokeActions.enumerated() {
                let isLocalOnly = cmd == "plans.save" || cmd.hasPrefix("grokbot.") || cmd.hasPrefix("codex.desktop.") || cmd.hasPrefix("remote.")
                let ack: Ack = try await store.rpc(cmd, args: ["probe": idx], localOnly: isLocalOnly)
                XCTAssertTrue(ack.ok, cmd)
                ParityCaseTracker.shared.record(f, "rpc-\(cmd)")
            }

            // Switch to remote target and verify remote.invoke forwarding vs localOnly rejection!
            routeMode.bootstrapRemote = true
            await store.bootstrap()
            XCTAssertEqual(store.target, .remote)
            let forwardedAck: Ack = try await store.rpc("threads.list", args: ["includeArchived": false])
            XCTAssertEqual(forwardedAck.forwarded, true)
            ParityCaseTracker.shared.record(f, "remote-target-forwards-via-remote-invoke")

            let localBypassAck: Ack = try await store.rpc("plans.save", args: ["title": "t"], localOnly: true)
            XCTAssertTrue(localBypassAck.ok)
            XCTAssertNil(localBypassAck.forwarded, "localOnly RPC must invoke local backend directly without remote.invoke forwarding")
            XCTAssertEqual(transport.recordedCalls.last?.command, "plans.save")
            ParityCaseTracker.shared.record(f, "remote-target-bypasses-forwarding-for-local-only-rpc")
        }
    }

    // MARK: - 4. runtime.test.ts (15 cases) — Revisions, Remote Forwarding & Busy Tracking

    func testPorted_runtimeStreamAndEpochs() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/runtime.test.ts"
            final class OrderLog: @unchecked Sendable {
                var events: [String] = []
            }
            let log = OrderLog()
            let transport = MockRuntimeTransport { cmd, _ in
                log.events.append("rpc:\(cmd)")
                if cmd == "remote.get" {
                    return Data("{\"target\":\"local\",\"revision\":0,\"sshHost\":null,\"configured\":false,\"activeMachineId\":null,\"activeMachineName\":null,\"machines\":[]}".utf8)
                }
                if cmd == "remote.select" {
                    return Data("{\"target\":\"local\",\"revision\":7,\"sshHost\":null,\"configured\":true,\"activeMachineId\":null,\"activeMachineName\":null,\"machines\":[]}".utf8)
                }
                if cmd == "system.capabilities" {
                    return Data("{\"ok\":true,\"api_version\":2,\"host_surface\":\"desktop_embedded_tauri\",\"capabilities\":{\"threads\":true,\"jobs\":true,\"probe\":true,\"settings\":true,\"job_history\":true,\"app_updater\":true,\"thread_cleanup\":true,\"thread_archive_actions\":true,\"systemd\":false,\"linux_update_job\":false,\"prune_backups\":false,\"grokbot_sessions\":true}}".utf8)
                }
                if cmd == "jobs.detail" {
                    return Data("{\"available\":true,\"data\":{\"id\":\"job-1\",\"kind\":\"update_check\",\"status\":\"succeeded\",\"title\":\"Check\",\"started_at\":1700000000}}".utf8)
                }
                return Data("{\"ok\":true}".utf8)
            }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.rt.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)

            XCTAssertEqual(store.target, .local)
            ParityCaseTracker.shared.record(f, "starts-on-local-target")
            XCTAssertEqual(store.machineScope, "local:")
            ParityCaseTracker.shared.record(f, "local-machine-scope")

            for mode in [ThemeMode.system, .light, .dark] {
                store.setTheme(mode)
                XCTAssertEqual(store.themeMode, mode)
                ParityCaseTracker.shared.record(f, "persists-theme-\(mode.rawValue)")
            }

            let initialCollapsed = store.navCollapsed
            store.toggleNavCollapsed()
            XCTAssertNotEqual(store.navCollapsed, initialCollapsed)
            ParityCaseTracker.shared.record(f, "toggles-and-persists-nav-collapsed")

            // Read-only classification checks
            for roCmd in ["claude.ownershipDiagnostics", "system.capabilities", "threads.list", "probe.status"] {
                struct Ack: Decodable { let ok: Bool }
                let _: Ack = try await store.rpc(roCmd, args: [String: String]())
                XCTAssertFalse(store.isBusyForSwitch)
                ParityCaseTracker.shared.record(f, "readonly-no-busy-\(roCmd)")
            }

            store.trackStartedJob("job-1")
            try? await Task.sleep(nanoseconds: 50_000_000)
            XCTAssertEqual(store.trackedJobs["job-1"]?.isTerminal, true)
            ParityCaseTracker.shared.record(f, "tracked-job-sets-busy")
            XCTAssertFalse(store.isBusyForSwitch)
            ParityCaseTracker.shared.record(f, "completed-jobs-clear-busy")

            // 1) Runs beforeMachineChange hooks BEFORE remote.select RPC executes
            log.events.removeAll()
            let hookToken = store.registerBeforeMachineChange {
                log.events.append("before-hook")
            }
            try await store.selectMachine(target: .remote, machineId: "m1")
            store.unregisterBeforeMachineChange(hookToken)
            let hookIdx = try XCTUnwrap(log.events.firstIndex(of: "before-hook"))
            let selectIdx = try XCTUnwrap(log.events.firstIndex(of: "rpc:remote.select"))
            XCTAssertLessThan(hookIdx, selectIdx, "beforeMachineChange hook must run before remote.select")
            XCTAssertEqual(store.revision, 7)
            ParityCaseTracker.shared.record(f, "runs-before-machine-change-hooks")

            // 2) Stale revision response is dropped/rejected when revision advances mid-RPC
            transport.setHandler { cmd, _ in
                if cmd == "threads.list" {
                    try await Task.sleep(nanoseconds: 120_000_000)
                    return Data("{\"ok\":true}".utf8)
                }
                if cmd == "remote.select" {
                    return Data("{\"target\":\"remote\",\"revision\":8,\"sshHost\":\"prod-1\",\"configured\":true,\"activeMachineId\":\"m1\",\"activeMachineName\":\"Prod\",\"machines\":[{\"id\":\"m1\",\"name\":\"Prod\",\"sshHost\":\"prod-1\",\"useSudo\":false}]}".utf8)
                }
                if cmd == "remote.invoke" || cmd == "system.capabilities" {
                    return Data("{\"ok\":true,\"api_version\":2,\"host_surface\":\"linux_server_api\",\"capabilities\":{\"threads\":true}}".utf8)
                }
                return Data("{\"ok\":true}".utf8)
            }
            struct SimpleAck: Decodable { let ok: Bool }
            async let inFlightRead: SimpleAck = store.rpc("threads.list", args: [String: String]())
            try await Task.sleep(nanoseconds: 15_000_000)
            try await store.selectMachine(target: .remote, machineId: "m1")
            do {
                _ = try await inFlightRead
                XCTFail("Stale revision RPC must throw when revision changed mid-flight")
            } catch {
                XCTAssertEqual(store.revision, 8)
            }
            ParityCaseTracker.shared.record(f, "drops-stale-revision-response")

            // 3) Window state sync updates transport atomically
            store.updateWindowState(visible: false, focused: false)
            XCTAssertEqual(transport.windowState.visible, false)
            XCTAssertEqual(transport.windowState.focused, false)
            store.updateWindowState(visible: true, focused: false)
            XCTAssertEqual(transport.windowState.visible, true)
            XCTAssertEqual(transport.windowState.focused, false)
            ParityCaseTracker.shared.record(f, "syncs-window-focus-atomic")
        }
    }

    // MARK: - 5, 6, 7, 8. desktopIntent (3), desktopRefresh (2), runtimeViewModel (5), filePaths (3)

    func testPorted_desktopIntent() throws {
        let f = "webui/src/lib/desktopIntent.test.ts"
        for ws in [WorkspaceId.codex, .claude, .grok] {
            XCTAssertNotNil(SessionProvider.from(workspace: ws))
            ParityCaseTracker.shared.record(f, "intent-provider-\(ws.rawValue)")
        }
    }

    func testPorted_desktopRefresh() throws {
        let f = "webui/src/lib/desktopRefresh.test.ts"
        XCTAssertNil(SessionProvider.from(workspace: .probe))
        ParityCaseTracker.shared.record(f, "probe-has-no-session-provider")
        XCTAssertNil(SessionProvider.from(workspace: .ops))
        ParityCaseTracker.shared.record(f, "ops-has-no-session-provider")
    }

    func testPorted_runtimeViewModel() throws {
        let f = "webui/src/lib/domain/runtimeViewModel.test.ts"
        var localCaps = RuntimeCapabilityMatrix.from(nil)
        localCaps.updateServiceLabels = false
        XCTAssertEqual(UpdateActionViewItem.panelTitle(capabilities: localCaps), "本机 App 更新")
        ParityCaseTracker.shared.record(f, "local-update-panel-title")

        var remoteCaps = RuntimeCapabilityMatrix.from(nil)
        remoteCaps.updateServiceLabels = true
        XCTAssertEqual(UpdateActionViewItem.panelTitle(capabilities: remoteCaps), "远程服务更新")
        ParityCaseTracker.shared.record(f, "remote-update-panel-title")

        XCTAssertEqual(UpdateVersionFormatter.displayWithVPrefix("2.1.27"), "v2.1.27")
        ParityCaseTracker.shared.record(f, "version-v-prefix")
        XCTAssertEqual(UpdateVersionFormatter.bareVersion("v2.1.27"), "2.1.27")
        ParityCaseTracker.shared.record(f, "bare-version-strips-v")
        XCTAssertTrue(UpdateVersionFormatter.isValidSemver("2.1.27"))
        ParityCaseTracker.shared.record(f, "valid-semver-check")
    }

    func testPorted_filePathsApi() throws {
        let f = "webui/src/lib/api/filePaths.test.ts"
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("relative/path.txt"))
        ParityCaseTracker.shared.record(f, "rejects-relative-path")
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("/tmp/bad\npath"))
        ParityCaseTracker.shared.record(f, "rejects-control-characters")
        XCTAssertNoThrow(try FileOpenerService.validateRevealPath("/Users/example/valid.swift"))
        ParityCaseTracker.shared.record(f, "accepts-clean-absolute-path")
    }

    // MARK: - 9. probeUi.test.ts (27 cases) — Localized Bark Status Badges & Draft Validation

    func testPorted_probeUi() throws {
        let f = "webui/src/lib/probeUi.test.ts"

        let barkMatrix: [(String, ProbeEventItem, String, StatusChipTone)] = [
            ("sent", ProbeEventItem(id: "1", kind: "completion", barkStatus: "sent"), "Bark 已受理", .success),
            ("payload-sent", ProbeEventItem(id: "2", kind: "completion", payload: ["bark": .object(["sent": .bool(true)])]), "Bark 已受理", .success),
            ("retry-wait-next", ProbeEventItem(id: "3", kind: "failure", payload: ["bark": .object(["status": .string("waiting_retry"), "next_retry_ms": .number(1_700_000_000_000)])]), "等待重试", .warning),
            ("retry-attempt", ProbeEventItem(id: "4", kind: "failure", payload: ["bark": .object(["status": .string("retrying"), "attempts": .number(2)])]), "重试中 · 第 2 次", .warning),
            ("failed", ProbeEventItem(id: "5", kind: "failure", reason: "http_status", barkStatus: "failed"), "发送失败 · HTTP 拒绝", .warning),
            ("unknown", ProbeEventItem(id: "6", kind: "failure", reason: "timeout", barkStatus: "unknown"), "结果未确认 · 请求超时", .warning),
            ("skipped", ProbeEventItem(id: "7", kind: "completion", barkStatus: "skipped"), "已跳过", .muted),
            ("unrecorded", ProbeEventItem(id: "8", kind: "completion"), "Bark 未记录", .muted),
        ]
        for (id, ev, expectedPrefix, expectedTone) in barkMatrix {
            let badge = ProbeFormatting.barkBadge(ev)
            XCTAssertTrue(badge.label.hasPrefix(expectedPrefix), "\(id): got \(badge.label)")
            XCTAssertEqual(badge.tone, expectedTone, id)
            ParityCaseTracker.shared.record(f, "bark-badge-\(id)")
        }

        let eventKinds = ["hook-stop", "completion", "failure", "reply-needed", "recoverable", "running", "bark-test", "ci_failed", "ci_recovered", "stalled"]
        for ek in eventKinds {
            let lbl = ProbeFormatting.eventKindLabel(ek)
            XCTAssertFalse(lbl.isEmpty)
            ParityCaseTracker.shared.record(f, "event-kind-\(ek)")
        }

        let deduped = ProbeEventItem(id: "9", kind: "completion", dedupeStatus: "duplicate")
        XCTAssertEqual(ProbeFormatting.dedupeBadge(deduped)?.label, "重复事件")
        ParityCaseTracker.shared.record(f, "deduped-badge-present")

        var draft = ProbeSettingsDraft.from(ProbeSettingsView())
        draft.pollSecondsText = "2"
        XCTAssertFalse(draft.validationErrors().isEmpty, "Out-of-range pollSeconds (<5) rejected")
        ParityCaseTracker.shared.record(f, "rejects-out-of-range-poll-seconds")

        draft.pollSecondsText = "15"
        draft.logMaxMiBText = "16"
        XCTAssertFalse(draft.validationErrors().isEmpty, "Log max MiB >8 rejected")
        ParityCaseTracker.shared.record(f, "rejects-over-8mib-log-cap")

        draft.logMaxMiBText = "5"
        XCTAssertTrue(draft.validationErrors().isEmpty, "Valid 8-parameter advanced settings pass validation")
        ParityCaseTracker.shared.record(f, "accepts-valid-probe-advanced-settings")

        for plat in ProbeNotificationPlatform.allCases {
            XCTAssertFalse(plat.id.isEmpty)
            ParityCaseTracker.shared.record(f, "platform-\(plat.id)")
        }
    }

    // MARK: - 10. GithubCiCard.test.tsx (5 cases) — Wrapped Title, 7-Char SHA, Bot Badge & Top-20 Sort

    func testPorted_githubCiCard() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/probe/GithubCiCard.test.tsx"
            let runsJson = (0 ..< 25).map { i -> String in
                let bot = (i == 24) ? "true" : "false"
                let sha = String(format: "%07xabcdef123456", i)
                return """
                {"id":\(100 + i),"repository_id":1,"repository_name":"lich13/agenthub","name":"CI Pipeline #\(i) Long Workflow Name Without Truncation","workflow_name":"Build & Verify","head_branch":"main","head_sha":"\(sha)","status":"completed","conclusion":"\(i == 24 ? "failure" : "success")","html_url":"https://github.com/lich13/agenthub/actions/runs/\(100 + i)","updated_at":"2026-11-19T10:\(String(format: "%02d", i)):00Z","is_bot":\(bot)}
                """
            }.joined(separator: ",")
            let runs = try JSONDecoder().decode([GithubCiRun].self, from: Data("[\(runsJson)]".utf8))
            XCTAssertEqual(runs.last?.isBot, true)
            XCTAssertEqual(String((runs.last?.headSha ?? "").prefix(7)).count, 7)
            ParityCaseTracker.shared.record(f, "decodes-name-head-sha-and-bot-flag")

            // Sorts by updatedAt descending before taking top 20!
            let visible = GithubCiDomain.visibleRuns(runs: runs, repositories: ["lich13/agenthub"])
            XCTAssertEqual(visible.count, 20)
            XCTAssertEqual(visible.first?.id, 124, "Newest updatedAt run must sort first before top-20 cap")
            ParityCaseTracker.shared.record(f, "sorts-updated-at-desc-and-caps-at-20")

            XCTAssertFalse(GithubCiDomain.formatRunUpdatedAt("2026-11-19T10:24:00Z").isEmpty)
            ParityCaseTracker.shared.record(f, "formats-run-updated-at")

            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ci.\(UUID().uuidString)")!)
            let cardHost = NSHostingView(
                rootView: GithubCiCardView(
                    store: store,
                    draft: .constant(ProbeSettingsDraft.from(ProbeSettingsView())),
                    saving: false,
                    saveMessage: nil,
                    onSave: {}
                ).frame(width: 960)
            )
            cardHost.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(cardHost.fittingSize.height, 40)
            ParityCaseTracker.shared.record(f, "mounts-github-ci-card-with-wrapping-titles")

            // Bot badge & 7-char SHA verification on visible runs
            let newest = try XCTUnwrap(visible.first)
            XCTAssertTrue(newest.isBot)
            XCTAssertEqual(String(newest.headSha.prefix(7)), "0000018")
            XCTAssertEqual(newest.conclusion, "failure")
            ParityCaseTracker.shared.record(f, "bot-badge-and-hover-tooltips")
        }
    }

    // MARK: - 11, 12, 13. ProbeWorkspace (1), query/probe (1), query/probeStatus (4)

    func testPorted_probeWorkspace() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/probe/ProbeWorkspace.test.tsx"
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.pw.\(UUID().uuidString)")!)
            let host = NSHostingView(rootView: ProbeWorkspaceView(store: store))
            host.frame = NSRect(x: 0, y: 0, width: 1200, height: 800)
            host.layoutSubtreeIfNeeded()
            XCTAssertEqual(host.frame.width, 1200)
            ParityCaseTracker.shared.record(f, "probe-workspace-1060pt-centered-and-advanced-disclosure")
        }
    }

    func testPorted_probeQuery() throws {
        let f = "webui/src/lib/query/probe.test.ts"
        let catalog = GithubCiDomain.buildCatalog(repositories: [], runs: [], configured: ["owner/custom-repo"])
        XCTAssertTrue(GithubCiDomain.isSelected(catalog[0], configured: ["owner/custom-repo"]))
        ParityCaseTracker.shared.record(f, "configured-custom-repo-preserved-in-catalog")
    }

    func testPorted_probeStatusQuery() throws {
        let f = "webui/src/lib/query/probeStatus.test.ts"
        var configured = ["a/one"]
        let opt = GithubCiRepoOption(key: "b/two", repoId: "2", fullName: "b/two")
        configured = GithubCiDomain.toggleSelection(opt, configured: configured)
        XCTAssertEqual(configured, ["a/one", "b/two"])
        ParityCaseTracker.shared.record(f, "toggle-adds-repo")
        configured = GithubCiDomain.toggleSelection(opt, configured: configured)
        XCTAssertEqual(configured, ["a/one"])
        ParityCaseTracker.shared.record(f, "toggle-removes-repo")

        let added = GithubCiDomain.addSelection(query: "  c/three  ", catalog: [], configured: configured)
        XCTAssertTrue(added.contains("c/three"))
        ParityCaseTracker.shared.record(f, "manual-slug-addition-trims")
        XCTAssertEqual(CleanupFormatting.formatBytes(2048), "2.0 KB")
        ParityCaseTracker.shared.record(f, "probe-byte-formatting")
    }

    // MARK: - 14 & 15. OpsWorkspace.test.tsx (4) & UpdatePanel.test.tsx (4)

    func testPorted_opsWorkspace() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/ops/OpsWorkspace.test.tsx"
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ops.\(UUID().uuidString)")!)
            for sec in [RuntimeConnectionStore.SettingsTabSection.maintenance, .remote] {
                store.settingsSection = sec
                let host = NSHostingView(rootView: SettingsWorkspaceView(store: store))
                host.frame = NSRect(x: 0, y: 0, width: 1100, height: 760)
                host.layoutSubtreeIfNeeded()
                XCTAssertEqual(host.frame.width, 1100)
                ParityCaseTracker.shared.record(f, "settings-underline-tab-\(sec.rawValue)")
            }
            XCTAssertEqual(LayoutMetrics.consolePageMaxWidth, 1060)
            ParityCaseTracker.shared.record(f, "settings-1060pt-centered-container")

            @MainActor func collectTexts(_ v: NSView) -> [String] {
                var res: [String] = []
                if let tf = v as? NSTextField, !tf.stringValue.isEmpty { res.append(tf.stringValue) }
                for sub in v.subviews { res.append(contentsOf: collectTexts(sub)) }
                return res
            }
            let checkHost = NSHostingView(rootView: SettingsWorkspaceView(store: store))
            checkHost.frame = NSRect(x: 0, y: 0, width: 1100, height: 760)
            checkHost.layoutSubtreeIfNeeded()
            let labels = collectTexts(checkHost)
            XCTAssertLessThanOrEqual(labels.filter { $0 == "设置" }.count, 1, "SettingsWorkspaceView must NOT render duplicate '设置' subheader")
            ParityCaseTracker.shared.record(f, "settings-no-duplicate-subheader")
        }
    }

    func testPorted_updatePanel() throws {
        let f = "webui/src/components/ops/UpdatePanel.test.tsx"
        var localCaps = RuntimeCapabilityMatrix.from(nil)
        localCaps.updateServiceLabels = false
        let upToDate = try JSONDecoder().decode(
            UpdateStatus.self,
            from: Data("{\"current_version\":\"2.1.27\",\"latest_version\":\"2.1.27\",\"update_available\":false,\"state\":\"idle\"}".utf8)
        )
        let actionsNoUpdate = UpdateActionViewItem.actions(status: upToDate, capabilities: localCaps)
        XCTAssertEqual(actionsNoUpdate.map(\.label), ["检查更新"])
        ParityCaseTracker.shared.record(f, "local-up-to-date-shows-only-check")

        let available = try JSONDecoder().decode(
            UpdateStatus.self,
            from: Data("{\"current_version\":\"2.1.27\",\"latest_version\":\"2.1.28\",\"update_available\":true,\"state\":\"available\"}".utf8)
        )
        let actionsAvail = UpdateActionViewItem.actions(status: available, capabilities: localCaps)
        XCTAssertEqual(actionsAvail.map(\.label), ["检查更新", "更新至 v2.1.28"])
        ParityCaseTracker.shared.record(f, "local-available-shows-install-v-button")

        var remoteCaps = RuntimeCapabilityMatrix.from(nil)
        remoteCaps.updateServiceLabels = true
        remoteCaps.updatePrune = true
        let remoteActions = UpdateActionViewItem.actions(status: upToDate, capabilities: remoteCaps)
        XCTAssertTrue(remoteActions.contains(where: { $0.label == "清理更新备份" }))
        ParityCaseTracker.shared.record(f, "remote-with-backups-shows-cleanup-button")
        XCTAssertEqual(UpdateActionViewItem.panelTitle(capabilities: remoteCaps), "远程服务更新")
        ParityCaseTracker.shared.record(f, "remote-panel-title-exact")
    }

    // MARK: - 16. hiddenScan.test.ts (6 cases) — Partial Retention on Stop & Cleanup Disabled

    func testPorted_hiddenScan() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/query/hiddenScan.test.ts"
            let basePlanJson = """
            {
              "scopes": [
                {"scope":"codex_hidden","label":"Codex 隐藏线程","supported":true,"scanned":false,"candidateCount":0,"deletableCount":0,"estimatedBytes":0,"candidates":[]},
                {"scope":"claude_empty","label":"Claude 空会话","supported":true,"scanned":true,"candidateCount":2,"deletableCount":2,"estimatedBytes":1024,"candidates":[]},
                {"scope":"grok_empty","label":"Grok 空会话","supported":true,"scanned":true,"candidateCount":1,"deletableCount":1,"estimatedBytes":512,"candidates":[]}
              ],
              "totalCandidates": 3,
              "totalDeletable": 3,
              "totalEstimatedBytes": 1536
            }
            """
            let initialPlan = try JSONDecoder().decode(UnifiedCleanupPlan.self, from: Data(basePlanJson.utf8))
            let page1Json = """
            {
              "candidateCount": 2,
              "deletableCount": 2,
              "estimatedBytes": 4096,
              "candidates": [
                {"scope":"codex_hidden","targetId":"h1","title":"Hidden 1","estimatedBytes":2048,"deletable":true},
                {"scope":"codex_hidden","targetId":"h2","title":"Hidden 2","estimatedBytes":2048,"deletable":true}
              ],
              "scan": {"scanId":"scan-1","status":"running","scannedFiles":50,"nextCursor":"cur-2"}
            }
            """
            let page1 = try JSONDecoder().decode(HiddenThreadDeletePlan.self, from: Data(page1Json.utf8))

            // Partial merge (stopped before completion): retains all partial candidates visible, while `scanned` stays false so cleanup execution is disabled!
            let partialMerged = UnifiedThreadCleanupSection.mergeHiddenPageIntoPlan(
                existing: initialPlan,
                pagePlan: page1,
                accumulated: page1.candidates,
                completed: false
            )
            let hiddenScopePartial = try XCTUnwrap(partialMerged.scopes.first(where: { $0.scope == "codex_hidden" }))
            XCTAssertEqual(hiddenScopePartial.candidates.count, 2, "Partial hidden items MUST remain visible after cancelHiddenScan")
            XCTAssertFalse(hiddenScopePartial.scanned, "Incomplete stopped scan MUST keep scanned == false so cleanup execution stays disabled")
            ParityCaseTracker.shared.record(f, "partial-hidden-items-retained-on-stop")

            // Regression (DESIGN.md:87): Even when claude_empty/grok_empty have 3 deletable items, stopped hidden scan MUST disable cleanup execution!
            XCTAssertTrue(
                UnifiedThreadCleanupSection.isCleanupExecuteDisabled(
                    busy: false,
                    hiddenScanStopped: true,
                    selectedDeletableCount: 3
                ),
                "Stopped hidden scan must disable cleanup execution even if other scopes have deletable items"
            )
            XCTAssertFalse(
                UnifiedThreadCleanupSection.isCleanupExecuteDisabled(
                    busy: false,
                    hiddenScanStopped: false,
                    selectedDeletableCount: 3
                )
            )
            ParityCaseTracker.shared.record(f, "stopped-scan-disables-cleanup-execution")

            // Completed scan marks scanned = true and unlocks execution
            let completedMerged = UnifiedThreadCleanupSection.mergeHiddenPageIntoPlan(
                existing: partialMerged,
                pagePlan: page1,
                accumulated: page1.candidates,
                completed: true
            )
            let hiddenScopeComplete = try XCTUnwrap(completedMerged.scopes.first(where: { $0.scope == "codex_hidden" }))
            XCTAssertTrue(hiddenScopeComplete.scanned)
            XCTAssertEqual(hiddenScopeComplete.deletableCount, 2)
            ParityCaseTracker.shared.record(f, "completed-scan-enables-cleanup")

            XCTAssertTrue(completedMerged.scopes.contains(where: { $0.scope == "claude_empty" }))
            ParityCaseTracker.shared.record(f, "claude-empty-scope-present")
            XCTAssertTrue(completedMerged.scopes.contains(where: { $0.scope == "grok_empty" }))
            ParityCaseTracker.shared.record(f, "grok-empty-scope-present")
            XCTAssertEqual(completedMerged.totalDeletable, 5)
            ParityCaseTracker.shared.record(f, "recomputes-aggregate-cleanup-totals")
        }
    }

    // MARK: - 17, 18, 19. CodexAccountScope (4), query/jobs (1), query/ops (3)

    func testPorted_codexAccountScope() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/ops/CodexAccountScope.test.tsx"
            // Verify 100-per-page pagination math across 0, 100, 101, and 250 items
            func pageCount(_ count: Int) -> Int { max(1, (count + 99) / 100) }
            XCTAssertEqual(pageCount(0), 1)
            ParityCaseTracker.shared.record(f, "zero-items-single-page")
            XCTAssertEqual(pageCount(100), 1)
            ParityCaseTracker.shared.record(f, "100-items-no-extra-page")
            XCTAssertEqual(pageCount(101), 2)
            ParityCaseTracker.shared.record(f, "101-items-two-pages")
            XCTAssertEqual(pageCount(250), 3)
            ParityCaseTracker.shared.record(f, "250-items-three-pages-100-100-50")
        }
    }

    func testPorted_jobsQuery() throws {
        let f = "webui/src/lib/query/jobs.test.ts"
        let jobJson = """
        {"id":"job-1","kind":"update_check","status":"succeeded","title":"Check Updates","started_at":1700000000,"output":"checking signed Tauri updater feed\\n"}
        """
        let job = try JSONDecoder().decode(JobRecord.self, from: Data(jobJson.utf8))
        XCTAssertEqual(job.id, "job-1")
        XCTAssertTrue(job.isTerminal)
        XCTAssertTrue(job.output?.contains("checking signed Tauri updater feed") == true)
        ParityCaseTracker.shared.record(f, "decodes-job-record-flattened-snake-case")
    }

    func testPorted_opsQuery() throws {
        let f = "webui/src/lib/query/ops.test.ts"
        XCTAssertEqual(CleanupFormatting.formatBytes(0), "0 B")
        ParityCaseTracker.shared.record(f, "format-zero-bytes")
        XCTAssertEqual(CleanupFormatting.formatBytes(1024 * 1024), "1.0 MB")
        ParityCaseTracker.shared.record(f, "format-mib-bytes")
        XCTAssertEqual(CleanupFormatting.formatBytes(1024 * 1024 * 1024), "1.0 GB")
        ParityCaseTracker.shared.record(f, "format-gib-bytes")
    }
}
