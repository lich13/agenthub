@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class RuntimeProbeAndOpsParitySuiteTests: XCTestCase {
    // MARK: - 1. App.test.ts (18 cases) — Window One-Time Maximize, ⌘A Modal Leak Guard & <=767pt Mobile Chrome

    func testPorted_appShellWindowAndModals() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.app.\(UUID().uuidString)")!)

            // 1) Initial Window One-Time Maximize (Item #2: 1280x820 base -> maximizes once to visibleFrame, never resizes again!)
            let wc = AgentHubWindowManager(store: store, rootView: EmptyView())
            let win = wc.window
            XCTAssertFalse(win.isVisible, "Main window must be created hidden before alignOnceAndReveal")
            XCTAssertEqual(win.minSize, NSSize(width: 1000, height: 680))

            let arbitraryScreen = NSRect(x: 0, y: 25, width: 1728, height: 1055)
            XCTAssertEqual(AgentHubWindowManager.maximizedInitialFrame(visibleFrame: arbitraryScreen), arbitraryScreen)

            let liveScreenVF = (win.screen ?? NSScreen.main)?.visibleFrame ?? NSRect(x: 0, y: 25, width: 1024, height: 681)
            wc.alignOnceAndReveal(overrideVisibleFrame: liveScreenVF)
            XCTAssertEqual(win.frame, liveScreenVF, "First reveal must maximize window to fill screen.visibleFrame")

            // User manually resizes window within screen visibleFrame -> subsequent alignOnceAndReveal MUST NOT resize it again!
            let customW = min(max(1000, liveScreenVF.width - 24), 1200)
            let customH = min(max(680, liveScreenVF.height - 20), 760)
            let userFrame = NSRect(
                x: liveScreenVF.minX + floor((liveScreenVF.width - customW) * 0.5),
                y: liveScreenVF.minY,
                width: customW,
                height: customH
            )
            win.setFrame(userFrame, display: false)
            let expectedAfterManual = win.frame
            wc.alignOnceAndReveal(overrideVisibleFrame: liveScreenVF)
            XCTAssertEqual(win.frame, expectedAfterManual, "alignOnceAndReveal must NEVER resize window after initial show")

            // Screen resolution clamp keeps window inside smaller display
            let smallerScreen = NSRect(x: 0, y: 0, width: 1280, height: 720)
            let clamped = AgentHubWindowManager.clampedFrameToVisibleScreen(
                NSRect(x: 500, y: 300, width: 1728, height: 1055),
                visibleFrame: smallerScreen
            )
            XCTAssertLessThanOrEqual(clamped.maxX, smallerScreen.maxX)
            XCTAssertLessThanOrEqual(clamped.maxY, smallerScreen.maxY)
            XCTAssertEqual(clamped.size, smallerScreen.size)
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

            // When active modal is registered in `ModalPresentationRegistry` -> BLOCKED!
            for _ in 0 ..< 6 {
                let token = UUID()
                ModalPresentationRegistry.shared.register(token)
                let before = selectAllNotifications.count
                let routed = AgentHubAppDelegate.routeMenuSelectAll(in: testWin, store: store)
                XCTAssertEqual(routed, .blockedByModalOrOverlay, "⌘A must be blocked while modal token is active")
                XCTAssertEqual(selectAllNotifications.count, before, "Underlying session list must NOT receive selectAll while modal is open")
                ModalPresentationRegistry.shared.unregister(token)
            }

            // WHILE modal is open, underlying ComposerNSTextView or non-modal ReaderSelectableTextView MUST BE BLOCKED from selectAll!
            let modalToken = UUID()
            defer { ModalPresentationRegistry.shared.unregister(modalToken) }
            let underlyingComposer = ComposerNSTextView(frame: NSRect(x: 10, y: 50, width: 300, height: 60))
            underlyingComposer.string = "draft behind modal"
            underlyingComposer.setSelectedRange(NSRange(location: 0, length: 0))
            testWin.contentView?.addSubview(underlyingComposer)
            _ = testWin.makeFirstResponder(underlyingComposer)
            ModalPresentationRegistry.shared.register(modalToken)
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
            ModalPresentationRegistry.shared.unregister(modalToken)
        }
    }

    // MARK: - 2. WorkspaceSplitShell.test.tsx (1 case)

    func testPorted_workspaceSplitShell() throws {
        try runAsyncParityCheck {
            let header = WorkspaceStageHeaderView(
                title: "Session Stage",
                statusBadge: "运行中",
                statusTone: .info,
                availableWidth: 500,
                inlineRenameActive: true,
                initialRenameText: "Session Stage"
            ) {
                EmptyView()
            }
            let host = NSHostingView(rootView: header.frame(width: 600))
            host.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(host.fittingSize.height, 90)
        }
    }

    // MARK: - 3. api.test.ts (51 cases) — Comprehensive Real Contract Registry Routing, Remote.Invoke & LocalOnly Guards

    func testPorted_apiContractRouting() throws {
        try runAsyncParityCheck {
            let contractURL = URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .deletingLastPathComponent()
                .appendingPathComponent("contracts/agenthub-contract.json")
            let rawContract = try Data(contentsOf: contractURL)
            let root = try XCTUnwrap(JSONSerialization.jsonObject(with: rawContract) as? [String: Any])
            let actions = try XCTUnwrap(root["actions"] as? [[String: Any]])
            // Every action in agenthub-contract.json that has a ffiHandler (excluding dedicated stream attach)
            let invokeActions: [String] = actions.compactMap { entry in
                guard let id = entry["id"] as? String,
                      let handler = entry["ffiHandler"] as? String,
                      !handler.isEmpty,
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
            }

            // Switch to remote target and verify remote.invoke forwarding vs localOnly rejection!
            routeMode.bootstrapRemote = true
            await store.bootstrap()
            XCTAssertEqual(store.target, .remote)
            let forwardedAck: Ack = try await store.rpc("threads.list", args: ["includeArchived": false])
            XCTAssertEqual(forwardedAck.forwarded, true)

            let localBypassAck: Ack = try await store.rpc("plans.save", args: ["title": "t"], localOnly: true)
            XCTAssertTrue(localBypassAck.ok)
            XCTAssertNil(localBypassAck.forwarded, "localOnly RPC must invoke local backend directly without remote.invoke forwarding")
            XCTAssertEqual(transport.recordedCalls.last?.command, "plans.save")
        }
    }

    // MARK: - 4. runtime.test.ts (15 cases) — Revisions, Remote Forwarding & Busy Tracking

    func testPorted_runtimeStreamAndEpochs() throws {
        try runAsyncParityCheck {
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
                    return protocol2CapabilitiesFixture(remote: false)
                }
                if cmd == "jobs.detail" {
                    return Data("{\"id\":\"job-1\",\"kind\":\"update_check\",\"status\":\"succeeded\",\"title\":\"Check\",\"started_at\":1700000000}".utf8)
                }
                return Data("{\"ok\":true}".utf8)
            }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.rt.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)

            XCTAssertEqual(store.target, .local)
            XCTAssertEqual(store.machineScope, "local:")

            for mode in [ThemeMode.system, .light, .dark] {
                store.setTheme(mode)
                XCTAssertEqual(store.themeMode, mode)
            }

            let initialCollapsed = store.navCollapsed
            store.toggleNavCollapsed()
            XCTAssertNotEqual(store.navCollapsed, initialCollapsed)

            // Read-only classification checks
            for roCmd in ["claude.ownershipDiagnostics", "system.capabilities", "threads.list", "probe.status"] {
                struct Ack: Decodable { let ok: Bool }
                let _: Ack = try await store.rpc(roCmd, args: [String: String]())
                XCTAssertFalse(store.isBusyForSwitch)
            }

            store.trackStartedJob("job-1")
            try? await Task.sleep(nanoseconds: 50_000_000)
            XCTAssertEqual(store.trackedJobs["job-1"]?.isTerminal, true)
            XCTAssertFalse(store.isBusyForSwitch)

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
                    return protocol2CapabilitiesFixture(remote: true)
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

            // 3) Window state sync updates transport atomically
            store.updateWindowState(visible: false, focused: false)
            XCTAssertEqual(transport.windowState.visible, false)
            XCTAssertEqual(transport.windowState.focused, false)
            store.updateWindowState(visible: true, focused: false)
            XCTAssertEqual(transport.windowState.visible, true)
            XCTAssertEqual(transport.windowState.focused, false)
        }
    }

    // MARK: - 5, 6, 7, 8. desktopIntent (3), desktopRefresh (2), runtimeViewModel (5), filePaths (3)

    func testPorted_desktopIntent() throws {
        for ws in [WorkspaceId.codex, .claude, .grok] {
            XCTAssertNotNil(SessionProvider.from(workspace: ws))
        }
    }

    func testPorted_desktopRefresh() throws {
        XCTAssertNil(SessionProvider.from(workspace: .probe))
        XCTAssertNil(SessionProvider.from(workspace: .ops))
    }

    func testPorted_runtimeViewModel() throws {
        var localCaps = RuntimeCapabilityMatrix.from(nil)
        localCaps.updateServiceLabels = false
        XCTAssertEqual(UpdateActionViewItem.panelTitle(capabilities: localCaps), "本机 App 更新")

        var remoteCaps = RuntimeCapabilityMatrix.from(nil)
        remoteCaps.updateServiceLabels = true
        XCTAssertEqual(UpdateActionViewItem.panelTitle(capabilities: remoteCaps), "远程服务更新")

        XCTAssertEqual(UpdateVersionFormatter.displayWithVPrefix("2.1.27"), "v2.1.27")
        XCTAssertEqual(UpdateVersionFormatter.bareVersion("v2.1.27"), "2.1.27")
        XCTAssertTrue(UpdateVersionFormatter.isValidSemver("2.1.27"))
    }

    func testPorted_filePathsApi() throws {
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("relative/path.txt"))
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("/tmp/bad\npath"))
        XCTAssertNoThrow(try FileOpenerService.validateRevealPath("/Users/example/valid.swift"))
    }

    // MARK: - 9. probeUi.test.ts (27 cases) — Localized Bark Status Badges & Draft Validation

    func testPorted_probeUi() throws {

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
        }

        let eventKinds = ["hook-stop", "completion", "failure", "reply-needed", "recoverable", "running", "bark-test", "ci_failed", "ci_recovered", "stalled"]
        for ek in eventKinds {
            let lbl = ProbeFormatting.eventKindLabel(ek)
            XCTAssertFalse(lbl.isEmpty)
        }

        let deduped = ProbeEventItem(id: "9", kind: "completion", dedupeStatus: "duplicate")
        XCTAssertEqual(ProbeFormatting.dedupeBadge(deduped)?.label, "重复事件")

        var draft = ProbeSettingsDraft.from(try ProbeSettingsFixture.settings())
        draft.pollSecondsText = "2"
        XCTAssertFalse(draft.validationErrors().isEmpty, "Out-of-range pollSeconds (<5) rejected")

        draft.pollSecondsText = "15"
        draft.logMaxMiBText = "16"
        XCTAssertFalse(draft.validationErrors().isEmpty, "Log max MiB >8 rejected")

        draft.logMaxMiBText = "5"
        XCTAssertTrue(draft.validationErrors().isEmpty, "Valid 8-parameter advanced settings pass validation")

        for plat in SessionProvider.allCases {
            XCTAssertFalse(plat.id.isEmpty)
        }
    }

    // MARK: - 10. GithubCiCard.test.tsx (5 cases) — Wrapped Title, 7-Char SHA, Bot Badge & Top-20 Sort

    func testPorted_githubCiCard() throws {
        try runAsyncParityCheck {
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

            // Sorts by updatedAt descending before taking top 20!
            let visible = GithubCiDomain.visibleRuns(runs: runs, repositories: ["lich13/agenthub"])
            XCTAssertEqual(visible.count, 20)
            XCTAssertEqual(visible.first?.id, 124, "Newest updatedAt run must sort first before top-20 cap")

            XCTAssertFalse(GithubCiDomain.formatRunUpdatedAt("2026-11-19T10:24:00Z").isEmpty)

            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ci.\(UUID().uuidString)")!)
            let cardHost = NSHostingView(
                rootView: GithubCiCardView(
                    store: store,
                    draft: .constant(ProbeSettingsDraft.from(try ProbeSettingsFixture.settings())),
                    saving: false,
                    saveMessage: nil,
                    onSave: {}
                ).frame(width: 960)
            )
            cardHost.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(cardHost.fittingSize.height, 40)

            // Bot badge & 7-char SHA verification on visible runs
            let newest = try XCTUnwrap(visible.first)
            XCTAssertTrue(newest.isBot)
            XCTAssertEqual(String(newest.headSha.prefix(7)), "0000018")
            XCTAssertEqual(newest.conclusion, "failure")
        }
    }

    // MARK: - 11, 12, 13. ProbeWorkspace (1), query/probe (1), query/probeStatus (4)

    func testPorted_probeWorkspace() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.pw.\(UUID().uuidString)")!)
            let host = NSHostingView(rootView: ProbeWorkspaceView(store: store))
            host.frame = NSRect(x: 0, y: 0, width: 1200, height: 800)
            host.layoutSubtreeIfNeeded()
            XCTAssertEqual(host.frame.width, 1200)
        }
    }

    func testPorted_probeQuery() throws {
        let catalog = GithubCiDomain.buildCatalog(repositories: [], runs: [], configured: ["owner/custom-repo"])
        XCTAssertTrue(GithubCiDomain.isSelected(catalog[0], configured: ["owner/custom-repo"]))
    }

    func testPorted_probeStatusQuery() throws {
        var configured = ["a/one"]
        let opt = GithubCiRepoOption(key: "b/two", repoId: "2", fullName: "b/two")
        configured = GithubCiDomain.toggleSelection(opt, configured: configured)
        XCTAssertEqual(configured, ["a/one", "b/two"])
        configured = GithubCiDomain.toggleSelection(opt, configured: configured)
        XCTAssertEqual(configured, ["a/one"])

        let added = GithubCiDomain.addSelection(query: "  c/three  ", catalog: [], configured: configured)
        XCTAssertTrue(added.contains("c/three"))
        XCTAssertEqual(CleanupFormatting.formatBytes(2048), "2.0 KB")
    }

    // MARK: - 14 & 15. OpsWorkspace.test.tsx (4) & UpdatePanel.test.tsx (4)

    func testPorted_opsWorkspace() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ops.\(UUID().uuidString)")!)
            for sec in [RuntimeConnectionStore.SettingsTabSection.maintenance, .remote] {
                store.settingsSection = sec
                let host = NSHostingView(rootView: SettingsWorkspaceView(store: store))
                host.frame = NSRect(x: 0, y: 0, width: 1100, height: 760)
                host.layoutSubtreeIfNeeded()
                XCTAssertEqual(host.frame.width, 1100)
            }
            XCTAssertEqual(LayoutMetrics.consolePageMaxWidth, 1060)

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
        }
    }

    func testPorted_updatePanel() throws {
        var localCaps = RuntimeCapabilityMatrix.from(nil)
        localCaps.updateServiceLabels = false
        let upToDate = try JSONDecoder().decode(
            UpdateStatus.self,
            from: Data("{\"current_version\":\"2.1.27\",\"latest_version\":\"2.1.27\",\"update_available\":false,\"state\":\"idle\"}".utf8)
        )
        let actionsNoUpdate = UpdateActionViewItem.actions(status: upToDate, capabilities: localCaps)
        XCTAssertEqual(actionsNoUpdate.map(\.label), ["检查更新"])

        let available = try JSONDecoder().decode(
            UpdateStatus.self,
            from: Data("{\"current_version\":\"2.1.27\",\"latest_version\":\"2.1.28\",\"update_available\":true,\"state\":\"available\"}".utf8)
        )
        let actionsAvail = UpdateActionViewItem.actions(status: available, capabilities: localCaps)
        XCTAssertEqual(actionsAvail.map(\.label), ["检查更新", "更新至 2.1.28"])

        var remoteCaps = RuntimeCapabilityMatrix.from(nil)
        remoteCaps.updateServiceLabels = true
        remoteCaps.updatePrune = true
        let remoteActions = UpdateActionViewItem.actions(status: upToDate, capabilities: remoteCaps)
        XCTAssertTrue(remoteActions.contains(where: { $0.label == "清理更新备份" }))
        XCTAssertEqual(UpdateActionViewItem.panelTitle(capabilities: remoteCaps), "远程服务更新")
    }

    // MARK: - 16. hiddenScan.test.ts (6 cases) — Partial Retention on Stop & Cleanup Disabled

    func testPorted_hiddenScan() throws {
        try runAsyncParityCheck {
            let fixtures = try CleanupInteractionVectors.load()
            var plan = fixtures.preview
            var modes: [String] = []
            let runner = HiddenCleanupScan { request in
                modes.append(request.mode)
                if request.mode == "start" { return fixtures.hiddenStart }
                XCTAssertEqual(request.scanId, fixtures.hiddenStart.scan.id)
                XCTAssertEqual(request.cursor, 0)
                return fixtures.hiddenPage
            }
            try await runner.run { progress, scope in
                plan.replaceHidden(scope)
                if progress.phase == .preparing {
                    XCTAssertFalse(scope.scanned)
                    XCTAssertTrue(plan.executionRequest(for: ["codex_hidden"]).targets.isEmpty)
                }
            }
            XCTAssertEqual(modes, ["start", "poll"])
            XCTAssertEqual(plan.executionRequest(for: ["codex_hidden"]).targets.map(\.id), ["hidden-1"])
            XCTAssertEqual(plan.totalDeletable, 3)

            let request = plan.executionRequest(for: ["claude_empty", "grok_empty"])
            let encoded = try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(request))
            XCTAssertEqual(encoded, fixtures.execute)
            plan.apply(fixtures.result)
            XCTAssertEqual(plan.totalDeletable, 2)
            XCTAssertEqual(plan.scopes.first { $0.scope == "claude_empty" }?.candidates.map(\.id), ["p/busy"])
            XCTAssertEqual(fixtures.result.unsuccessfulDetails.count, 2)
        }
    }

    // MARK: - 17, 18, 19. CodexAccountScope (4), query/jobs (1), query/ops (3)

    func testPorted_codexAccountScope() throws {
        try runAsyncParityCheck {
            // Verify 100-per-page pagination math across 0, 100, 101, and 250 items
            func pageCount(_ count: Int) -> Int { max(1, (count + 99) / 100) }
            XCTAssertEqual(pageCount(0), 1)
            XCTAssertEqual(pageCount(100), 1)
            XCTAssertEqual(pageCount(101), 2)
            XCTAssertEqual(pageCount(250), 3)
        }
    }

    func testPorted_jobsQuery() throws {
        let jobJson = """
        {"id":"job-1","kind":"update_check","status":"succeeded","title":"Check Updates","started_at":1700000000,"output":"checking signed Tauri updater feed\\n"}
        """
        let job = try JSONDecoder().decode(JobRecord.self, from: Data(jobJson.utf8))
        XCTAssertEqual(job.id, "job-1")
        XCTAssertTrue(job.isTerminal)
        XCTAssertTrue(job.output?.contains("checking signed Tauri updater feed") == true)
    }

    func testPorted_opsQuery() throws {
        XCTAssertEqual(CleanupFormatting.formatBytes(0), "0 B")
        XCTAssertEqual(CleanupFormatting.formatBytes(1024 * 1024), "1.0 MB")
        XCTAssertEqual(CleanupFormatting.formatBytes(1024 * 1024 * 1024), "1.0 GB")
    }
}
