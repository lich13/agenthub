@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class AntigravitySubagentAndStatusTests: XCTestCase {
    @MainActor
    func testAntigravityBadgesSidebarSyncAndPagedSubagentUpdates() async throws {
        final class StatusState: @unchecked Sendable {
            var currentStatus = "running"
            var childStatus = "running"
        }
        let state = StatusState()
        let rootId = "11111111-1111-4111-8111-111111111111"
        let subId = "22222222-2222-4222-8222-222222222222"

        let suite = "com.lich13.agenthub.ag.status.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let transport = MockRuntimeTransport { cmd, args in
            switch cmd {
            case "system.capabilities":
                return protocol2CapabilitiesFixture(remote: false)
            case "antigravity.list":
                if args.contains("\"archived\":true") { return Data("[]".utf8) }
                return Data("""
                [
                  {"id":"\(rootId)","sessionKey":"\(rootId)","title":"AG 主线程","cwd":"/workspace/demo","path":"/workspace/demo/root.db","archived":false,"updatedAt":"2026-11-24T01:00:00Z","messageCount":3,"status":"running","canRename":true,"canArchive":true,"canDelete":false,"deleteBlockReason":"会话正在运行","storageSize":{"bytes":4096,"scope":"directory","status":"complete"}},
                  {"id":"w1","sessionKey":"w1","title":"AG 等待审批","cwd":"/workspace/demo","path":"/workspace/demo/w1.db","archived":false,"updatedAt":"2026-11-24T01:00:00Z","messageCount":2,"status":"waiting","canRename":true,"canArchive":true,"canDelete":false},
                  {"id":"r1","sessionKey":"r1","title":"AG 待确认计划","cwd":"/workspace/demo","path":"/workspace/demo/r1.db","archived":false,"updatedAt":"2026-11-24T01:00:00Z","messageCount":2,"status":"reply_needed","canRename":true,"canArchive":true,"canDelete":true},
                  {"id":"f1","sessionKey":"f1","title":"AG 执行异常","cwd":"/workspace/demo","path":"/workspace/demo/f1.db","archived":false,"updatedAt":"2026-11-24T01:00:00Z","messageCount":2,"status":"failed","canRename":true,"canArchive":true,"canDelete":true}
                ]
                """.utf8)
            case "antigravity.detail":
                return Data("""
                {
                  "summary": {"id":"\(rootId)","sessionKey":"\(rootId)","title":"AG 主线程","cwd":"/workspace/demo","path":"/workspace/demo/root.db","archived":false,"updatedAt":"2026-11-24T01:00:05Z","messageCount":4,"status":"\(state.currentStatus)","canRename":true,"canArchive":true,"canDelete":\(state.currentStatus != "running" && state.currentStatus != "waiting")},
                  "subagents": {
                    "agents": [{"agentId":"\(subId)","name":"架构摸底子代理","role":"research","status":"\(state.childStatus)","available":true,"delegation":"核对 Connect-RPC 与 SQLite 状态"}],
                    "counts": {"creating":0,"running":\(state.childStatus == "running" ? 1 : 0),"completed":\(state.childStatus == "completed" ? 1 : 0),"failed":0,"interrupted":0,"unknown":0},
                    "complete": true
                  },
                  "subagentUpdates": {
                    "older-spawn": {"eventKind":"started","eventId":"older-spawn","agentId":"\(subId)","name":"架构摸底子代理","role":"research","status":"\(state.childStatus)","available":true}
                  },
                  "history": {"epoch":"\(rootId)","revision":"rev-\(state.childStatus)","changedIds":[],"reset":false},
                  "events": [
                    {"id":"ev-tail","kind":"assistant_message","role":"assistant","text":"已汇总子代理结论。","status":"completed"}
                  ],
                  "totalEvents": 2,
                  "hasMore": false
                }
                """.utf8)
            default:
                return Data("{}".utf8)
            }
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()
        XCTAssertTrue(store.capabilities.antigravitySubagents)

        let ws = ProviderWorkspaceStore(store: store, workspace: .antigravity)
        await ws.fetchSessionList()
        XCTAssertEqual(ws.activeItems.count, 4)
        XCTAssertEqual(ws.activeItems[0].statusChipBadge, "运行中")
        XCTAssertEqual(ws.activeItems[0].statusChipTone, .info)
        XCTAssertTrue(ws.activeItems[0].running)
        XCTAssertEqual(ws.activeItems[1].statusChipBadge, "等待处理")
        XCTAssertEqual(ws.activeItems[1].statusChipTone, .warning)
        XCTAssertFalse(ws.activeItems[1].running)
        XCTAssertEqual(ws.activeItems[2].statusChipBadge, "待回复")
        XCTAssertEqual(ws.activeItems[2].statusChipTone, .warning)
        XCTAssertEqual(ws.activeItems[3].statusChipBadge, "异常")
        XCTAssertEqual(ws.activeItems[3].statusChipTone, .danger)

        // Seed an already-loaded older page event with inline subagent in providerHistoryStore
        let historyScope = "\(store.machineScope):antigravity:\(rootId)"
        _ = ws.providerHistoryStore.applyHeadPoll(
            key: historyScope,
            headEvents: [
                ProviderHistoryEvent(
                    id: "older-spawn",
                    kind: "tool_call",
                    role: "invoke_subagent",
                    text: "invoke_subagent",
                    subagent: SubagentActivity(
                        eventKind: "started",
                        eventId: "older-spawn",
                        agentId: subId,
                        name: "架构摸底子代理",
                        role: "research",
                        status: "running",
                        available: true,
                        delegation: "原始委派任务说明"
                    )
                ),
            ],
            history: HistoryMetadata(epoch: rootId, revision: "rev-0"),
            totalEvents: 2,
            beforeCursor: nil,
            hasMore: false
        )

        // Transition session to waiting + child subagent to completed -> fetchSessionDetail syncs sidebar & older inline card!
        state.currentStatus = "waiting"
        state.childStatus = "completed"
        ws.selectedKey = rootId
        await ws.fetchSessionDetail(for: ws.activeItems[0])

        XCTAssertEqual(ws.activeItems[0].statusChipBadge, "等待处理")
        XCTAssertEqual(ws.activeItems[0].statusText, "等待处理")
        XCTAssertFalse(ws.activeItems[0].running)
        XCTAssertEqual(ws.activeItems[0].storageSize?.bytes, 4096, "in-place sidebar sync must preserve existing storageSize")
        XCTAssertEqual(ws.subagents?.agents.first?.status, "completed")

        let inlineSubRow = ws.rows.compactMap { row -> (SubagentActivity, Bool)? in
            if case let .subagent(sub, supported) = row.kind { return (sub, supported) }
            return nil
        }.first
        XCTAssertEqual(inlineSubRow?.0.status, "completed", "subagentUpdates must update already-loaded older page inline card")
        XCTAssertEqual(inlineSubRow?.0.delegation, "原始委派任务说明", "wire update without delegation must preserve original delegation")
        XCTAssertEqual(inlineSubRow?.1, true)
    }

    @MainActor
    func testOldRemoteCapabilityGatingAndNestedDrawerPopPropagation() async throws {
        let rootId = "11111111-1111-4111-8111-111111111111"
        let l1Id = "22222222-2222-4222-8222-222222222222"
        let l2Id = "33333333-3333-4333-8333-333333333333"

        let suite = "com.lich13.agenthub.ag.legacycap.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        // Simulate older remote agenthub-webd omitting antigravity_subagents in system.capabilities
        let transport = MockRuntimeTransport { cmd, _ in
            switch cmd {
            case "system.capabilities":
                return Data(#"{"api_version":2,"host_surface":"linux_server_api","capabilities":{"thread_subagents":false,"claude_subagents":false}}"#.utf8)
            case "antigravity.list":
                return Data("""
                [{"id":"\(rootId)","sessionKey":"\(rootId)","title":"Remote AG","cwd":"/srv","path":"/srv/root.db","archived":false,"status":"idle","messageCount":1,"canRename":true,"canArchive":true,"canDelete":true}]
                """.utf8)
            case "antigravity.detail":
                return Data("""
                {
                  "summary": {"id":"\(rootId)","sessionKey":"\(rootId)","title":"Remote AG","cwd":"/srv","path":"/srv/root.db","archived":false,"status":"idle","messageCount":1,"canRename":true,"canArchive":true,"canDelete":true},
                  "subagents": {"agents":[{"agentId":"\(l1Id)","name":"L1","status":"completed","available":true}],"counts":{"completed":1},"complete":true},
                  "events": [{"id":"ev1","kind":"tool_call","role":"invoke_subagent","text":"invoke_subagent","subagent":{"eventKind":"started","eventId":"ev1","agentId":"\(l1Id)","name":"L1","status":"completed","available":true}}],
                  "totalEvents": 1,
                  "hasMore": false
                }
                """.utf8)
            default:
                return Data("{}".utf8)
            }
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()
        XCTAssertTrue(store.capabilities.isReady)
        XCTAssertNil(store.capabilitiesError)
        XCTAssertFalse(store.capabilities.antigravitySubagents)

        let ws = ProviderWorkspaceStore(store: store, workspace: .antigravity)
        await ws.fetchSessionList()
        ws.selectedKey = rootId
        await ws.fetchSessionDetail(for: ws.activeItems[0])
        XCTAssertFalse(ws.subagentsSupportedForCurrentProvider)
        XCTAssertNil(ws.subagents, "Missing antigravity_subagents capability must hide subagent collection")
        let inlineSupported = ws.rows.compactMap { row -> Bool? in
            if case let .subagent(_, supported) = row.kind { return supported }
            return nil
        }.first
        XCTAssertEqual(inlineSupported, false)

        // Verify nested L0 -> detail(L1) -> list(L1's children) -> detail(L2) pop propagation
        let l1Agent = SubagentActivity(agentId: l1Id, name: "L1", status: "completed", available: true)
        let l2Initial = SubagentActivity(agentId: l2Id, name: "L2", status: "running", available: true, delegation: "核验叶子节点")
        let nestedList = SubagentCollection(agents: [l2Initial], counts: ["running": 1], complete: true)
        var stack: [SubagentPanelView.NavigationFrame] = [
            .detail(l1Agent),
            .list(parent: l1Agent, collection: nestedList),
            .detail(SubagentActivity(agentId: l2Id, name: "L2", status: "completed", available: true)),
        ]
        let popped = stack.removeLast()
        SubagentPanelView.propagateUpdatedChildToParentList(popped: popped, into: &stack)
        guard case let .list(_, .some(updatedCol)) = stack.last else {
            return XCTFail("Expected top frame to remain .list")
        }
        XCTAssertEqual(updatedCol.agents.first?.status, "completed")
        XCTAssertEqual(updatedCol.agents.first?.delegation, "核验叶子节点")
        XCTAssertEqual(updatedCol.counts["completed"], 1)
        XCTAssertNil(updatedCol.counts["running"])
    }

    @MainActor
    func testOffscreenAntigravitySubagentDrawerAndProbeErrorLightAndDarkRender() async throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        let rootId = "11111111-1111-4111-8111-111111111111"
        let l1Id = "22222222-2222-4222-8222-222222222222"
        let l2Id = "33333333-3333-4333-8333-333333333333"
        let captureDir = ProcessInfo.processInfo.environment["AGENTHUB_CAPTURE_AG_RENDER_DIR"]

        final class SubPollState: @unchecked Sendable {
            var l1Status = "running"
        }
        let pollState = SubPollState()

        let suite = "com.lich13.agenthub.ag.render.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let transport = MockRuntimeTransport { cmd, args in
            switch cmd {
            case "system.capabilities":
                return protocol2CapabilitiesFixture(remote: false)
            case "antigravity.detail":
                if args.contains(l1Id) {
                    return Data("""
                    {
                      "summary": {"id":"\(l1Id)","sessionKey":"\(l1Id)","title":"架构摸底 L1","cwd":"/workspace/AgentHub","path":"/workspace/AgentHub/l1.db","archived":false,"updatedAt":"2026-11-24T01:00:05Z","messageCount":3,"status":"\(pollState.l1Status == "running" ? "running" : "idle")","canRename":false,"canArchive":false,"canDelete":false},
                      "agent": {"agentId":"\(l1Id)","name":"架构摸底 L1","role":"research","status":"\(pollState.l1Status)","available":true,"delegation":"梳理 Antigravity 双存储与在线 Connect-RPC 链路"},
                      "subagents": {
                        "agents": [{"agentId":"\(l2Id)","name":"叶节点校验 L2","role":"self","status":"completed","available":true,"delegation":"核验拓扑逆序删除"}],
                        "counts": {"creating":0,"running":0,"completed":1,"failed":0,"interrupted":0,"unknown":0},
                        "complete": true
                      },
                      "history": {"epoch":"a1b2c3d4e5f60001:\(l1Id)","revision":"r1","changedIds":[],"reset":false},
                      "events": [
                        {"id":"a1b2c3d4e5f60001:ev1","kind":"user_message","role":"user","text":"核查 Connect-RPC 与 SQLite 状态","status":"completed"},
                        {"id":"a1b2c3d4e5f60001:ev2","kind":"tool_call","role":"invoke_subagent","text":"invoke_subagent","status":"completed","subagent":{"eventKind":"started","eventId":"a1b2c3d4e5f60001:ev2","agentId":"\(l2Id)","name":"叶节点校验 L2","role":"self","status":"completed","available":true,"delegation":"核验拓扑逆序删除"}},
                        {"id":"a1b2c3d4e5f60001:ev3","kind":"assistant_message","role":"assistant","text":"已完成双存储与子代理链路核查。","status":"completed"}
                      ],
                      "totalEvents": 3,
                      "hasMore": false
                    }
                    """.utf8)
                }
                return Data("{}".utf8)
            case "probe.status":
                return Data(#"{"label":"Probe","enabled":true,"available":false,"flavor":"builtin","error":"配置文件 TOML 第 12 行语法错误，请检查 config.toml"}"#.utf8)
            case "probe.settings.get":
                return ProbeSettingsFixture.data
            case "probe.events":
                return Data(#"{"limit":10,"events":[]}"#.utf8)
            case "github.ci.status":
                return Data(#"{"enabled":false,"refreshing":false,"selected_repositories":[],"repositories":[],"runs":[]}"#.utf8)
            case "claude.hooks":
                return Data(#"{"installed":false,"managedCount":0}"#.utf8)
            default:
                return Data("{}".utf8)
            }
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        let rootCollection = SubagentCollection(
            agents: [
                SubagentActivity(agentId: l1Id, name: "架构摸底 L1", role: "research", status: "running", available: true, delegation: "梳理双存储"),
            ],
            counts: ["running": 1, "completed": 0],
            complete: true
        )

        for (scheme, appearance, tag, bg) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "light", (UInt8(255), UInt8(255), UInt8(255))),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "dark", (UInt8(17), UInt8(20), UInt8(26))),
        ] {
            let palette = DesignPalette.resolve(scheme: scheme, workspace: .antigravity)
            let summaryBar = SubagentSummaryBarView(
                provider: .antigravity,
                collection: rootCollection,
                isSupported: true,
                onTogglePanel: { _ in }
            )
            .environment(\.agentHubPalette, palette)

            let barPng = captureDir.map { "\($0)/ag-summary-\(tag).png" }
            let barPixels = try OffscreenRenderHelper.renderSwiftUIView(
                summaryBar,
                size: NSSize(width: 720, height: 42),
                appearance: appearance,
                pngPath: barPng
            )
            XCTAssertGreaterThan(countNonBgPixels(barPixels, width: 720, height: 42, bg: bg), 120, "Summary bar glyphs must render non-blank ink in \(tag)")

            // Verify live frameStack write-back: initialAgent starts "running", detail RPC returns "completed" -> header StatusChip updates to "已完成"
            pollState.l1Status = "completed"
            let drawerView = SubagentPanelView(
                store: store,
                provider: .antigravity,
                rootSessionKey: rootId,
                collection: rootCollection,
                initialAgent: rootCollection.agents[0],
                scheme: scheme,
                workspace: .antigravity,
                onClose: {}
            )
            .environment(\.agentHubPalette, palette)

            let host = NSHostingView(rootView: drawerView.frame(width: 420, height: 640))
            let win = UnconstrainedOffscreenWindow(
                contentRect: NSRect(x: 0, y: 0, width: 420, height: 640),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            win.appearance = NSAppearance(named: appearance)
            win.contentView = host
            host.layoutSubtreeIfNeeded()
            for _ in 0 ..< 25 {
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            let drawerPng = captureDir.map { "\($0)/ag-drawer-\(tag).png" }
            let drawerPixels = try OffscreenRenderHelper.renderNSView(
                host,
                size: NSSize(width: 420, height: 640),
                appearance: appearance,
                pngPath: drawerPng
            )
            // Verify header title + nested L2 summary bar + conversation rows all rendered real glyph ink!
            XCTAssertGreaterThan(countNonBgPixels(drawerPixels, width: 420, height: 640, bg: bg, yRange: 592 ..< 640), 150, "Drawer header must render non-blank ink in \(tag)")
            XCTAssertGreaterThan(countNonBgPixels(drawerPixels, width: 420, height: 640, bg: bg, yRange: 550 ..< 592), 80, "Nested L2 SubagentSummaryBar inside L1 drawer must render ink in \(tag)")
            XCTAssertGreaterThan(countNonBgPixels(drawerPixels, width: 420, height: 640, bg: bg, yRange: 60 ..< 540), 400, "Subagent conversation reader rows must render ink in \(tag)")
            win.contentView = nil
            win.orderOut(nil)
            pollState.l1Status = "running"

            // Also verify ProbeWorkspaceView renders explicit ProbeStatus.error card in both Light & Dark
            let probeView = ProbeWorkspaceView(store: store)
                .environment(\.agentHubPalette, DesignPalette.resolve(scheme: scheme, workspace: .probe))
            let probeHost = NSHostingView(rootView: probeView.frame(width: 960, height: 680))
            let probeWin = UnconstrainedOffscreenWindow(
                contentRect: NSRect(x: 0, y: 0, width: 960, height: 680),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            probeWin.appearance = NSAppearance(named: appearance)
            probeWin.contentView = probeHost
            probeHost.layoutSubtreeIfNeeded()
            for _ in 0 ..< 20 {
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            let probePng = captureDir.map { "\($0)/probe-error-\(tag).png" }
            let probePixels = try OffscreenRenderHelper.renderNSView(
                probeHost,
                size: NSSize(width: 960, height: 680),
                appearance: appearance,
                pngPath: probePng
            )
            XCTAssertGreaterThan(countNonBgPixels(probePixels, width: 960, height: 680, bg: bg), 800, "ProbeWorkspaceView with explicit status.error must render ink in \(tag)")
            XCTAssertGreaterThan(countDangerPixels(probePixels, width: 960, yRange: 560 ..< 625), 100, "Top ProbeStatus.error banner must render visible red/danger ink above the fold in \(tag)")
            probeWin.contentView = nil
            probeWin.orderOut(nil)
        }
    }

    @MainActor
    func testAntigravityUserRowNoContextCardAndMenuChevronLightAndDarkRender() async throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        let suite = "com.lich13.agenthub.ag.purifyrender.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let transport = MockRuntimeTransport { cmd, _ in
            switch cmd {
            case "system.capabilities":
                return protocol2CapabilitiesFixture(remote: false)
            case "remote.sshHosts":
                return Data(#"{"hosts":["dev-box.example.invalid","prod-box.example.invalid"]}"#.utf8)
            default:
                return Data("{}".utf8)
            }
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        @MainActor func collectTexts(_ view: NSView) -> String {
            var out: [String] = []
            if let tf = view as? NSTextField { out.append(tf.stringValue) }
            if let tv = view as? NSTextView { out.append(tv.string) }
            if let btn = view as? NSButton {
                out.append(btn.title)
                out.append(btn.attributedTitle.string)
            }
            for sub in view.subviews {
                out.append(collectTexts(sub))
            }
            return out.joined(separator: " ")
        }

        @MainActor func containsPopUpButton(_ view: NSView) -> Bool {
            if view is NSPopUpButton { return true }
            return view.subviews.contains { containsPopUpButton($0) }
        }

        @MainActor func findMenuAnchorView(_ view: NSView) -> NSView? {
            if String(reflecting: type(of: view)).contains("MenuAnchorProbeNSView") {
                return view
            }
            for sub in view.subviews {
                if let found = findMenuAnchorView(sub) {
                    return found
                }
            }
            return nil
        }

        let literalUserText = "/boost /plan 用 $de-gpt 深度清理这个老项目\n<environment_context>字面量示例</environment_context>"
        let agEvents = [
            ProviderHistoryEvent(
                id: "ag-u1",
                kind: "user_message",
                role: "user",
                text: literalUserText,
                userMessage: UserMessageContent(id: "ag-u1", text: literalUserText, attachments: [], contextBlocks: nil)
            ),
        ]
        let agRows = ConversationRowBuilder.buildProviderRows(
            events: agEvents,
            providerLabel: "Antigravity",
            sessionRunning: false,
            subagentsSupported: true
        )
        let agRow = try XCTUnwrap(agRows.first)

        // Verify Codex preserves its existing "上下文" disclosure card while Antigravity suppresses it.
        let codexCoord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())
        codexCoord.sessionScopeKey = "codex:c1"
        let codexCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("CodexCtxCell"))
        let codexRow = ConversationRowItem(
            id: "codex-u1",
            timelineId: "codex-u1",
            kind: .user(
                message: UserMessageContent(
                    id: "codex-u1",
                    text: "检查配置",
                    contextBlocks: [UserContextBlock(kind: "environment", text: "<environment_context>cwd=/workspace</environment_context>")]
                ),
                rawText: "检查配置",
                attachmentsAsNamesOnly: false
            )
        )
        let codexH = codexCoord.measureRowHeight(codexRow, width: 720)
        codexCell.frame = NSRect(x: 0, y: 0, width: 720, height: codexH)
        codexCell.configure(item: codexRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: codexCoord)
        XCTAssertTrue(collectTexts(codexCell).contains("上下文"))

        let approvalReq = DesktopPendingRequest(
            key: "req-1",
            kind: "file",
            title: "审批文件修改",
            canAccept: true,
            supportedDecisions: ["acceptForSession"],
            review: DesktopReview(
                command: "cargo test --workspace",
                cwd: "/workspace/demo",
                reason: "需要更新测试夹具",
                changes: [
                    DesktopFileReview(path: "src/lib.rs", kind: "update", diff: "+let clean = true;\n", truncated: false),
                ],
                extra: "sandbox=workspace-write"
            )
        )

        for (scheme, appearance, tag, bg) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "light", (UInt8(255), UInt8(255), UInt8(255))),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "dark", (UInt8(17), UInt8(20), UInt8(26))),
        ] {
            let palette = DesignPalette.resolve(scheme: scheme, workspace: .antigravity)
            let coord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())
            coord.scheme = scheme
            coord.workspace = .antigravity
            coord.sessionScopeKey = "antigravity:s1"
            let rowH = coord.measureRowHeight(agRow, width: 720)
            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("AgUserCell-\(tag)"))
            cell.frame = NSRect(x: 0, y: 0, width: 720, height: rowH)
            cell.configure(item: agRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            cell.layoutSubtreeIfNeeded()

            let cellText = collectTexts(cell)
            XCTAssertFalse(cellText.contains("上下文"), "Antigravity user row must never render a 上下文 placeholder card in \(tag)")
            XCTAssertTrue(cellText.contains("/boost /plan 用 $de-gpt 深度清理这个老项目"))
            XCTAssertTrue(cellText.contains("字面量示例"))
            let cellPixels = try OffscreenRenderHelper.renderNSView(
                cell,
                size: NSSize(width: 720, height: max(rowH, 64)),
                appearance: appearance
            )
            XCTAssertGreaterThan(countNonBgPixels(cellPixels, width: 720, height: Int(max(rowH, 64)), bg: bg), 180, "Antigravity user bubble must render non-blank glyph ink in \(tag)")

            let approvalView = DesktopRequestsListView(
                requests: [approvalReq],
                busy: false,
                onAct: { _ in },
                onOpenCodex: {}
            )
            .environment(\.agentHubPalette, DesignPalette.resolve(scheme: scheme, workspace: .codex))
            let approvalHost = NSHostingView(rootView: approvalView.frame(width: 640, height: 260))
            approvalHost.appearance = NSAppearance(named: appearance)
            approvalHost.frame = NSRect(x: 0, y: 0, width: 640, height: 260)
            approvalHost.layoutSubtreeIfNeeded()
            let approvalStrings = collectTexts(approvalHost)
            XCTAssertFalse(approvalStrings.contains("▾"))
            XCTAssertFalse(approvalStrings.contains("▸"))
            let approvalPixels = try OffscreenRenderHelper.renderNSView(
                approvalHost,
                size: NSSize(width: 640, height: 260),
                appearance: appearance
            )
            XCTAssertGreaterThan(countNonBgPixels(approvalPixels, width: 640, height: 260, bg: bg), 250, "Codex approval card with vector chevrons must render ink in \(tag)")

            let remoteView = RemoteConnectionSettingsView(store: store)
                .environment(\.agentHubPalette, palette)
            let remoteHost = NSHostingView(rootView: remoteView.frame(width: 720, height: 360))
            let remoteWin = UnconstrainedOffscreenWindow(
                contentRect: NSRect(x: 0, y: 0, width: 720, height: 360),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            remoteWin.appearance = NSAppearance(named: appearance)
            remoteWin.contentView = remoteHost
            remoteHost.layoutSubtreeIfNeeded()
            for _ in 0 ..< 10 {
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            XCTAssertFalse(containsPopUpButton(remoteHost), "RemoteConnectionSettingsView must use NSMenu.popUp instead of SwiftUI.Menu NSPopUpButton")
            if let sshAnchor = findMenuAnchorView(remoteHost) {
                let rectInHost = sshAnchor.convert(sshAnchor.bounds, to: remoteHost)
                XCTAssertGreaterThanOrEqual(rectInHost.width, 200)
                XCTAssertEqual(rectInHost.height, LayoutMetrics.formControlHeight, accuracy: 1.0)
                XCTAssertNotNil(remoteHost.hitTest(NSPoint(x: rectInHost.minX + 2, y: rectInHost.minY + 2)))
                XCTAssertNotNil(remoteHost.hitTest(NSPoint(x: rectInHost.maxX - 2, y: rectInHost.maxY - 2)))
            } else {
                XCTFail("Expected AnchoredMenuButton MenuAnchorProbeNSView for SSH 连接 in \(tag)")
            }
            let remotePixels = try OffscreenRenderHelper.renderNSView(
                remoteHost,
                size: NSSize(width: 720, height: 360),
                appearance: appearance
            )
            XCTAssertGreaterThan(countNonBgPixels(remotePixels, width: 720, height: 360, bg: bg), 400, "RemoteConnectionSettingsView must render non-blank ink in \(tag)")
            remoteWin.contentView = nil
            remoteWin.orderOut(nil)
        }
    }

    private func countDangerPixels(_ pixels: [UInt8], width: Int, yRange: Range<Int>) -> Int {
        var count = 0
        for y in yRange {
            for x in 0 ..< width {
                let idx = (y * width + x) * 4
                let r = Int(pixels[idx])
                let g = Int(pixels[idx + 1])
                let b = Int(pixels[idx + 2])
                if r - g >= 24 && r - b >= 24 {
                    count += 1
                }
            }
        }
        return count
    }

    private func countNonBgPixels(
        _ pixels: [UInt8],
        width: Int,
        height: Int,
        bg: (UInt8, UInt8, UInt8),
        yRange: Range<Int>? = nil
    ) -> Int {
        let rows = yRange ?? (0 ..< height)
        var count = 0
        for y in rows {
            for x in 0 ..< width {
                let idx = (y * width + x) * 4
                let dr = abs(Int(pixels[idx]) - Int(bg.0))
                let dg = abs(Int(pixels[idx + 1]) - Int(bg.1))
                let db = abs(Int(pixels[idx + 2]) - Int(bg.2))
                if dr > 12 || dg > 12 || db > 12 {
                    count += 1
                }
            }
        }
        return count
    }
}
