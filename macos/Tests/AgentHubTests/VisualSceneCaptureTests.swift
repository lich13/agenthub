@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class VisualSceneCaptureTests: XCTestCase {
    @MainActor
    func testCapturePopulatedSwiftVisualScenesIfRequested() async throws {
        guard let outDir = ProcessInfo.processInfo.environment["AGENTHUB_VISUAL_CAPTURE_DIR"], !outDir.isEmpty else {
            return
        }

        let transport = MockRuntimeTransport { cmd, args in
            Self.mockResponse(cmd: cmd, args: args)
        }
        let suiteName = "com.lich13.agenthub.visual.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        let size = NSSize(width: 1440, height: 960)
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        let host = NSHostingView(rootView: AppShellView(store: store).frame(width: size.width, height: size.height))
        host.frame = NSRect(origin: .zero, size: size)
        window.contentView = host
        window.orderBack(nil)

        // Scene 1: Light Codex Top
        store.setTheme(.light)
        window.appearance = NSAppearance(named: .aqua)
        store.activeWorkspace = .codex
        try await pumpMainRunLoop(ms: 450)
        scrollConversationTable(in: host, fraction: 0.0)
        try await pumpMainRunLoop(ms: 200)
        XCTAssertGreaterThanOrEqual(conversationTableRowCount(in: host), 6, "Codex scene 1 must decode threads.detail and mount conversation rows")
        let scene1Strings = collectAllRenderedStrings(in: host)
        XCTAssertTrue(scene1Strings.contains("搜索标题、ID 或工作目录"), "Codex search bar must use unified workspace placeholder")
        XCTAssertFalse(scene1Strings.contains(where: { $0.contains("未能读取数据") }), "Codex scene 1 must have zero DecodingError text")
        XCTAssertTrue(scene1Strings.contains(where: { $0.contains("12 条历史对话已折叠") || $0.contains("请核对线程详情") }), "Codex conversation stage must mount decoded message text views")
        let rep1 = try captureWindow(window, host: host, to: "\(outDir)/swift-01-codex-light-top.png")
        if let rep1 {
            XCTAssertGreaterThan(
                countDarkInkPixels(in: rep1, logicalSize: size, topLeftRect: NSRect(x: 514, y: 29, width: 190, height: 18)),
                40,
                "Codex stage header must render /workspace/AgentHub subtitle glyphs"
            )
            XCTAssertGreaterThan(
                countErrorRedPixels(in: rep1, logicalSize: size, topLeftRect: NSRect(x: 510, y: 56, width: 340, height: 38)),
                10,
                "Codex subagent summary bar must render populated '1 失败' count (not muted upgrade notice)"
            )
            XCTAssertEqual(
                countErrorRedPixels(in: rep1, logicalSize: size, topLeftRect: NSRect(x: 206, y: 88, width: 220, height: 22)),
                0,
                "Sidebar strip below Codex search bar must have zero errFg red error pixels"
            )
        }

        // Scene 2: Light Codex Mid (QuestionResultCell + Question-Reply + AGENTS.md fold)
        scrollConversationTable(in: host, fraction: 0.45)
        try await pumpMainRunLoop(ms: 200)
        try captureWindow(window, host: host, to: "\(outDir)/swift-02-codex-light-mid.png")

        // Scene 3: Dark Codex Expanded ExecutionGroup & User Context
        store.setTheme(.dark)
        window.appearance = NSAppearance(named: .darkAqua)
        ActivityDisclosureStore.shared.setOpen(
            scope: "\(store.machineScope):codex:th-visual-1",
            stateKey: "group:codex:turn-1:c1:call-1:0",
            isOpen: true
        )
        ActivityDisclosureStore.shared.setOpen(
            scope: "\(store.machineScope):codex:th-visual-1",
            stateKey: "codex:turn-1:c1:call-1:0",
            isOpen: true
        )
        ActivityDisclosureStore.shared.setOpen(
            scope: "\(store.machineScope):codex:th-visual-1",
            stateKey: "u-prompt-1:context",
            isOpen: true
        )
        try await pumpMainRunLoop(ms: 350)
        reloadAllTables(in: host)
        scrollConversationTable(in: host, fraction: 0.0)
        try await pumpMainRunLoop(ms: 250)
        try captureWindow(window, host: host, to: "\(outDir)/swift-03-codex-dark-expanded.png")

        // Scene 4: Dark Codex Bottom (AGENTS.md fold + Markdown + CodeBlock + Table)
        scrollConversationTable(in: host, fraction: 1.0)
        try await pumpMainRunLoop(ms: 250)
        try captureWindow(window, host: host, to: "\(outDir)/swift-04-codex-dark-bottom.png")

        // Scene 5: Light Codex Bottom (Read-only stage without collaboration footer)
        store.setTheme(.light)
        window.appearance = NSAppearance(named: .aqua)
        try await pumpMainRunLoop(ms: 250)
        scrollConversationTable(in: host, fraction: 1.0)
        try await pumpMainRunLoop(ms: 200)
        let scene5Strings = collectAllRenderedStrings(in: host)
        XCTAssertFalse(scene5Strings.contains(where: { $0.contains("未能读取数据") }), "Codex bottom scene 5 must have zero DecodingError text")
        try captureWindow(window, host: host, to: "\(outDir)/swift-05-codex-light-bottom.png")

        // Scene 6: Light Claude Workspace (Expanded ClaudeProcessGroup)
        ActivityDisclosureStore.shared.setOpen(
            scope: "\(store.machineScope):claude_code:claude:cl-visual-1",
            stateKey: "claude-process:t1:cl-th1",
            isOpen: true
        )
        store.activeWorkspace = .claude
        try await pumpMainRunLoop(ms: 450)
        reloadAllTables(in: host)
        scrollConversationTable(in: host, fraction: 0.0)
        try await pumpMainRunLoop(ms: 200)
        try captureWindow(window, host: host, to: "\(outDir)/swift-06-claude-expanded.png")

        // Scene 7: Probe Workspace (Light & Dark × Default Collapsed / All-Expanded / Error Banner)
        store.setTheme(.light)
        window.appearance = NSAppearance(named: .aqua)
        store.activeWorkspace = .probe
        try await pumpMainRunLoop(ms: 450)
        try captureWindow(window, host: host, to: "\(outDir)/swift-07-probe-light.png")

        store.setTheme(.dark)
        window.appearance = NSAppearance(named: .darkAqua)
        try await pumpMainRunLoop(ms: 350)
        try captureWindow(window, host: host, to: "\(outDir)/swift-07b-probe-dark.png")

        // Standalone 1280x960 all-expanded Probe captures (Light & Dark)
        let expandedSize = NSSize(width: 1280, height: 1120)
        for (expScheme, expAppearance, expFile) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "swift-07c-probe-light-expanded.png"),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "swift-07d-probe-dark-expanded.png"),
        ] {
            let expWin = NSWindow(
                contentRect: NSRect(origin: .zero, size: expandedSize),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            expWin.appearance = NSAppearance(named: expAppearance)
            let expPalette = DesignPalette.resolve(scheme: expScheme, workspace: .probe)
            let expHost = NSHostingView(
                rootView: ProbeWorkspaceView(
                    store: store,
                    initialOpenSections: Set(ProbeDisclosureSection.allCases)
                )
                .environment(\.agentHubPalette, expPalette)
                .frame(width: expandedSize.width, height: expandedSize.height)
            )
            expHost.frame = NSRect(origin: .zero, size: expandedSize)
            expWin.contentView = expHost
            expWin.orderBack(nil)
            try await pumpMainRunLoop(ms: 350)
            try captureWindow(expWin, host: expHost, to: "\(outDir)/\(expFile)")
            expWin.orderOut(nil)
        }

        // Standalone 960x680 narrow Probe error banner capture
        let errTransport = MockRuntimeTransport { cmd, args in
            if cmd == "probe.status" {
                throw AgentHubFFIError("Probe 状态读取失败：模拟远端服务暂不可用")
            }
            return Self.mockResponse(cmd: cmd, args: args)
        }
        let errStore = RuntimeConnectionStore(transport: errTransport, defaults: defaults)
        await errStore.bootstrap()
        let errSize = NSSize(width: 960, height: 680)
        let errWin = NSWindow(
            contentRect: NSRect(origin: .zero, size: errSize),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        errWin.appearance = NSAppearance(named: .aqua)
        let errHost = NSHostingView(
            rootView: ProbeWorkspaceView(store: errStore)
                .environment(\.agentHubPalette, DesignPalette.resolve(scheme: .light, workspace: .probe))
                .frame(width: errSize.width, height: errSize.height)
        )
        errHost.frame = NSRect(origin: .zero, size: errSize)
        errWin.contentView = errHost
        errWin.orderBack(nil)
        try await pumpMainRunLoop(ms: 350)
        try captureWindow(errWin, host: errHost, to: "\(outDir)/swift-07e-probe-error-banner.png")
        errWin.orderOut(nil)

        store.setTheme(.light)
        window.appearance = NSAppearance(named: .aqua)

        // Scene 8: Settings Workspace
        store.activeWorkspace = .ops
        try await pumpMainRunLoop(ms: 450)
        try captureWindow(window, host: host, to: "\(outDir)/swift-08-settings-light.png")

        // Scene 9: Right-Click ContextTaskMenu Card (.task-menu-items)
        let menuActions = ProviderMenuMatrixBuilder.buildActions(
            provider: .codex,
            isHeaderMenu: false,
            isLocalMachine: true,
            id: "th-visual-1",
            path: "/workspace/.codex/sessions/rollout-1.jsonl",
            cwd: "/workspace/AgentHub",
            archived: false,
            canRename: true,
            canArchive: true,
            canDelete: true,
            busy: false,
            latestReplyText: nil,
            onRename: {},
            onToggleArchive: {},
            onCopy: { _, _ in },
            onRevealInFinder: { _ in },
            onDelete: {}
        )
        let menuPalette = DesignPalette.resolve(scheme: .light, workspace: .codex)
        AgentHubTaskMenuPresenter.shared.presentAtScreenTopLeft(
            NSPoint(x: 240, y: 760),
            in: window,
            actions: menuActions,
            palette: menuPalette
        )
        try await pumpMainRunLoop(ms: 80)
        if let panel = window.childWindows?.first, let menuView = panel.contentView {
            try captureWindow(panel, host: menuView, to: "\(outDir)/swift-09-codex-context-menu.png")
        }
        AgentHubTaskMenuPresenter.shared.dismiss()

        // Scene 10: GitHub CI 20-Run Bounded Scroll Card (Light & Dark)
        let runs20Parts: [String] = (1...20).map { (idx: Int) -> String in
            let runId = 2000 + idx
            let statusStr = (idx == 1) ? "in_progress" : "completed"
            let conclusionJson: String
            if idx == 1 {
                conclusionJson = "null"
            } else if idx == 2 {
                conclusionJson = "\"failure\""
            } else {
                conclusionJson = "\"success\""
            }
            return "{\"id\":\(runId),\"run_attempt\":1,\"name\":\"Backend & contract checks #\(idx)\",\"head_branch\":\"main\",\"head_sha\":\"c008e00a1234\",\"status\":\"\(statusStr)\",\"conclusion\":\(conclusionJson),\"html_url\":\"https://example.invalid/runs/\(idx)\",\"created_at\":\"2026-11-25T10:00:00Z\",\"updated_at\":\"2026-11-25T10:05:00Z\",\"repository_id\":1,\"repository_name\":\"octocat/AgentHub\",\"is_bot\":false}"
        }
        let runs20Json = runs20Parts.joined(separator: ",")
        let ci20Transport = MockRuntimeTransport { cmd, args in
            if cmd == "github.ci.status" {
                return Data("{\"account\":{\"id\":1,\"login\":\"octocat\"},\"enabled\":true,\"refreshing\":false,\"error\":null,\"monitored_repositories\":[\"octocat/AgentHub\"],\"repositories\":[{\"id\":1,\"full_name\":\"octocat/AgentHub\",\"archived\":false},{\"id\":2,\"full_name\":\"octocat/swift-kit\",\"archived\":false}],\"runs\":[\(runs20Json)]}".utf8)
            }
            return Self.mockResponse(cmd: cmd, args: args)
        }
        let ci20Store = RuntimeConnectionStore(transport: ci20Transport, defaults: defaults)
        await ci20Store.bootstrap()
        let ciSize = NSSize(width: 1020, height: 680)
        let ciCases: [(ColorSchemeKind, NSAppearance.Name, String)] = [
            (.light, .aqua, "swift-10a-ci-20runs-light.png"),
            (.dark, .darkAqua, "swift-10b-ci-20runs-dark.png"),
        ]
        for (ciScheme, ciAppearance, ciFile) in ciCases {
            var ciDraft: ProbeSettingsDraft? = ProbeSettingsDraft.from(try ProbeSettingsFixture.settings())
            var ciRules = ProbeDisclosureRules(machineScope: ci20Store.machineScope)
            ciRules.setExpanded(.ci, true)
            let ciWin = NSWindow(
                contentRect: NSRect(origin: .zero, size: ciSize),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            ciWin.appearance = NSAppearance(named: ciAppearance)
            let ciHost = NSHostingView(
                rootView: GithubCiCardView(
                    store: ci20Store,
                    draft: Binding(get: { ciDraft }, set: { ciDraft = $0 }),
                    disclosureRules: Binding(get: { ciRules }, set: { ciRules = $0 }),
                    saving: false,
                    saveMessage: nil,
                    onSave: {}
                )
                .padding(16)
                .background(DesignPalette.resolve(scheme: ciScheme, workspace: .probe).background.color)
                .environment(\.agentHubPalette, DesignPalette.resolve(scheme: ciScheme, workspace: .probe))
                .frame(width: ciSize.width)
            )
            ciHost.frame = NSRect(origin: .zero, size: ciSize)
            ciWin.contentView = ciHost
            ciWin.orderBack(nil)
            try await pumpMainRunLoop(ms: 250)
            try captureWindow(ciWin, host: ciHost, to: "\(outDir)/\(ciFile)")
            ciWin.orderOut(nil)
        }

        // Scene 11: Subagent Drawer List (Claude Right-Chevron & Status Chips, Light & Dark)
        let subCollection = SubagentCollection(
            agents: [
                SubagentActivity(agentId: "csub-1", name: "Explore subagent", role: "research", status: "completed", available: true),
                SubagentActivity(agentId: "csub-2", name: "Layout verifier", role: "worker", status: "running", available: true),
                SubagentActivity(agentId: "csub-3", name: "Archived child", role: "worker", status: "failed", available: false, unavailableReason: "子会话记录已清理"),
            ],
            counts: ["completed": 1, "running": 1, "failed": 1],
            complete: true
        )
        let drawerSize = NSSize(width: 440, height: 520)
        let drawerCases: [(ColorSchemeKind, NSAppearance.Name, String)] = [
            (.light, .aqua, "swift-11a-subagent-drawer-light.png"),
            (.dark, .darkAqua, "swift-11b-subagent-drawer-dark.png"),
        ]
        for (drScheme, drAppearance, drFile) in drawerCases {
            let drPalette = DesignPalette.resolve(scheme: drScheme, workspace: .claude)
            let drWin = NSWindow(
                contentRect: NSRect(origin: .zero, size: drawerSize),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            drWin.appearance = NSAppearance(named: drAppearance)
            let drHost = NSHostingView(
                rootView: SubagentPanelView(
                    store: store,
                    provider: .claudeCode,
                    rootSessionKey: "claude:cl-visual-1",
                    collection: subCollection,
                    scheme: drScheme,
                    workspace: .claude,
                    onClose: {}
                )
                .environment(\.agentHubPalette, drPalette)
                .frame(width: drawerSize.width, height: drawerSize.height)
            )
            drHost.frame = NSRect(origin: .zero, size: drawerSize)
            drWin.contentView = drHost
            drWin.orderBack(nil)
            try await pumpMainRunLoop(ms: 250)
            try captureWindow(drWin, host: drHost, to: "\(outDir)/\(drFile)")
            drWin.orderOut(nil)
        }
    }

    @MainActor
    func testSlice4StatusChipsWritebackCursorAndAppendContracts() async throws {
        // 1. Claude & Grok status chips cover reply_needed, waiting, failed, unknown
        let decoder = JSONDecoder()
        let claudeReplyRaw = try decoder.decode(
            ClaudeSessionSummary.self,
            from: Data(#"{"id":"c1","sessionKey":"p/c1.jsonl","title":"T","cwd":"/tmp","path":"/tmp/c1.jsonl","status":"reply_needed"}"#.utf8)
        )
        let claudeReply = ProviderWorkspaceStore.mapClaudeSummaryStatic(claudeReplyRaw)
        XCTAssertEqual(claudeReply.statusChipBadge, SessionStatusLabels.replyNeeded)
        XCTAssertEqual(claudeReply.statusChipTone, StatusChipTone.warning)

        let claudeFailedRaw = try decoder.decode(
            ClaudeSessionSummary.self,
            from: Data(#"{"id":"c2","sessionKey":"p/c2.jsonl","title":"T","cwd":"/tmp","path":"/tmp/c2.jsonl","status":"failed"}"#.utf8)
        )
        let claudeFailed = ProviderWorkspaceStore.mapClaudeSummaryStatic(claudeFailedRaw)
        XCTAssertEqual(claudeFailed.statusChipBadge, "异常")
        XCTAssertEqual(claudeFailed.statusChipTone, StatusChipTone.danger)

        let grokWaitingRaw = try decoder.decode(
            GrokSessionSummary.self,
            from: Data(#"{"id":"g1","title":"G","cwd":"/tmp","path":"/tmp/g1","status":"waiting"}"#.utf8)
        )
        let grokWaiting = ProviderWorkspaceStore.mapGrokSummaryStatic(grokWaitingRaw)
        XCTAssertEqual(grokWaiting.statusChipBadge, SessionStatusLabels.waiting)
        XCTAssertEqual(grokWaiting.statusChipTone, StatusChipTone.warning)

        let grokUnknownRaw = try decoder.decode(
            GrokSessionSummary.self,
            from: Data(#"{"id":"g2","title":"G","cwd":"/tmp","path":"/tmp/g2","status":"unknown"}"#.utf8)
        )
        let grokUnknown = ProviderWorkspaceStore.mapGrokSummaryStatic(grokUnknownRaw)
        XCTAssertEqual(grokUnknown.statusChipBadge, "状态未知")
        XCTAssertEqual(grokUnknown.statusChipTone, StatusChipTone.neutral)

        // 2. stableHistoryCursor attaches :block_id when history == nil, and applyDetail preserves "p:" cursor
        let nilHistCursor = HistoryAndResumeHelpers.stableHistoryCursor(
            history: nil,
            blocks: [MessageBlock(id: "msg-42", role: "user", kind: "message", text: "hello")],
            beforeCursor: "b:42"
        )
        XCTAssertEqual(nilHistCursor, "b:42:msg-42")

        let tStore = ThreadMessageStore()
        _ = tStore.applyDetail(
            ThreadDetail(
                summary: ThreadSummary(id: "th-p", title: "T", status: "Recent"),
                blocks: [MessageBlock(id: "m-10", role: "user", kind: "message", text: "10")],
                totalBlocks: 20,
                hasMoreBlocks: true,
                beforeCursor: "p:m-10"
            )
        )
        let afterHeadPoll = tStore.applyDetail(
            ThreadDetail(
                summary: ThreadSummary(id: "th-p", title: "T", status: "Recent"),
                blocks: [MessageBlock(id: "m-11", role: "assistant", kind: "message", text: "11")],
                totalBlocks: 20,
                hasMoreBlocks: true,
                beforeCursor: "b:19"
            )
        )
        XCTAssertEqual(afterHeadPoll.beforeCursor, "p:m-10", "Head poll b: cursor must not clobber active p: seek cursor")

        // 3. UnifiedWorkspaceHubStore immediate stage writeback guarded by machine revision & sequence
        let suiteName = "com.lich13.agenthub.s4.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let store = RuntimeConnectionStore(transport: MockRuntimeTransport { cmd, args in Self.mockResponse(cmd: cmd, args: args) }, defaults: defaults)
        await store.bootstrap()
        let hub = UnifiedWorkspaceHubStore(store: store)
        await hub.fetchAllSessions()
        guard var claudeEntry = hub.activeItems.first(where: { $0.provider == .claudeCode })?.item else {
            XCTFail("Expected Claude item in UnifiedWorkspaceHubStore")
            return
        }
        claudeEntry.statusText = SessionStatusLabels.replyNeeded
        claudeEntry.statusChipBadge = SessionStatusLabels.replyNeeded
        claudeEntry.statusChipTone = .warning
        // Wrong revision must be ignored
        hub.applyStageSummaryWriteback(provider: .claudeCode, updated: claudeEntry, revision: store.revision &+ 99)
        XCTAssertNotEqual(hub.activeItems.first(where: { $0.provider == .claudeCode })?.item.statusChipBadge, SessionStatusLabels.replyNeeded)
        // Matching revision updates immediately
        hub.applyStageSummaryWriteback(provider: .claudeCode, updated: claudeEntry, revision: store.revision)
        XCTAssertEqual(hub.activeItems.first(where: { $0.provider == .claudeCode })?.item.statusChipBadge, SessionStatusLabels.replyNeeded)

        // 4. Streaming tail split (.assistant -> .executionGroup + .assistant + .user) keeps row heights & anchors synced
        let coord = ConversationTableCoordinator(store: store)
        coord.sessionScopeKey = "test:tail-split"
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 1160, height: 720))
        let table = NSTableView(frame: scroll.bounds)
        let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("ConversationRowCell"))
        col.width = 1160
        table.addTableColumn(col)
        table.headerView = nil
        table.delegate = coord
        table.dataSource = coord
        scroll.documentView = table
        coord.scrollView = scroll
        coord.tableView = table
        coord.column = col

        let initialRows: [ConversationRowItem] = [
            ConversationRowItem(id: "u1", timelineId: "u1", kind: .user(message: nil, rawText: "First prompt", attachmentsAsNamesOnly: false)),
            ConversationRowItem(id: "a1", timelineId: "a1", kind: .assistant(markdown: "First reply", foldInstructions: false)),
            ConversationRowItem(id: "u2", timelineId: "u2", kind: .user(message: nil, rawText: "Second prompt", attachmentsAsNamesOnly: false)),
            ConversationRowItem(id: "a2-stream", timelineId: "a2-stream", kind: .assistant(markdown: "Streaming commentary...", foldInstructions: false)),
        ]
        coord.updateRowsPreservingAnchor(initialRows, forceScrollToBottom: true)
        XCTAssertEqual(table.numberOfRows, 4)

        let splitCmd = ExecutionCommand(
            id: "cmd-1",
            sourceIds: ["cmd-1"],
            title: "bash",
            preview: "cargo test",
            status: .completed,
            sections: [.init(label: "结果", text: "ok")]
        )
        let splitGrp = ExecutionGroup(
            id: "exec-2",
            mergedIds: [],
            commands: [splitCmd],
            kind: "command",
            provider: "Codex",
            running: false,
            recordedOnly: false,
            failedCount: 0
        )
        let splitRows: [ConversationRowItem] = [
            initialRows[0],
            initialRows[1],
            initialRows[2],
            ConversationRowItem(
                id: "exec-2",
                timelineId: "exec-2",
                kind: .executionGroup(splitGrp)
            ),
            ConversationRowItem(id: "a2-final", timelineId: "a2-final", kind: .assistant(markdown: "Final answer.", foldInstructions: false)),
            ConversationRowItem(id: "u3", timelineId: "u3", kind: .user(message: nil, rawText: "Follow-up prompt", attachmentsAsNamesOnly: false)),
        ]
        coord.updateRowsPreservingAnchor(splitRows, preferAutoFollowBottom: true)
        XCTAssertEqual(table.numberOfRows, 6)
        XCTAssertEqual(coord.rows.map(\.id), ["u1", "a1", "u2", "exec-2", "a2-final", "u3"])

        // 5. Mixed ASCII+CJK wide code line is never evicted by narrower 3-byte-per-char pure CJK lines
        let pureCjkLine = String(repeating: "汉", count: 32) // 96 UTF-8 bytes, ~64 cols
        let wideMixedLine = String(repeating: "x", count: 82) + " // 中文" // 91 UTF-8 bytes, ~89 cols
        let sampleCode = ([pureCjkLine, pureCjkLine, pureCjkLine, pureCjkLine, wideMixedLine]).joined(separator: "\n")
        let mixedDocW = ConversationTableCoordinator.measureCodeDocumentWidth(code: sampleCode, minWidth: 100)
        let soloWideW = ConversationTableCoordinator.measureCodeDocumentWidth(code: wideMixedLine, minWidth: 100)
        XCTAssertEqual(mixedDocW, soloWideW, "Wide mixed ASCII+CJK line must be sampled even when shorter in UTF-8 bytes than pure-CJK lines")
    }

    @MainActor
    func testRenderV2142NarrowProbeLongSidebarChineseCwdAndMultilineQuestions() async throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        let outDir = ProcessInfo.processInfo.environment["AGENTHUB_VISUAL_CAPTURE_DIR"]
        let suiteName = "com.lich13.agenthub.v2142.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let transport = MockRuntimeTransport { cmd, args in
            if cmd == "dsh.list" {
                return Data(#"""
                [{"id":"dsh-cjk-1","sessionKey":"dsh-cjk-1","title":"DeepSeek Harness 中文路径与多行问答核验","cwd":"/Users/example/Documents/程序开发/AgentHub","workspaceRoot":"/Users/example/Documents/程序开发/AgentHub","path":"/Users/example/.dsh/sessions/dsh-cjk-1.jsonl","status":"running","updatedAt":"2026-11-28T06:00:00Z","canRename":true,"canArchive":true,"canDelete":false,"deleteBlockReason":"会话正在运行"}]
                """#.utf8)
            }
            if cmd == "dsh.detail" {
                return Data(#"""
                {"summary":{"id":"dsh-cjk-1","sessionKey":"dsh-cjk-1","title":"DeepSeek Harness 中文路径与多行问答核验","cwd":"/Users/example/Documents/程序开发/AgentHub","workspaceRoot":"/Users/example/Documents/程序开发/AgentHub","path":"/Users/example/.dsh/sessions/dsh-cjk-1.jsonl","status":"running","updatedAt":"2026-11-28T06:00:00Z","canRename":true,"canArchive":true,"canDelete":false},"events":[{"id":"d-u1","kind":"user_message","text":"核对 /Users/example/Documents/程序开发/AgentHub 路径字距与 296pt 顶栏标题完整展示。"},{"id":"d-a1","kind":"assistant_message","text":"已完成核对，`DeepSeek Harness` 顶栏零截断，中文工作目录无中西文异常空隙。"}],"totalEvents":2,"hasMore":false}
                """#.utf8)
            }
            if cmd == "claude.list" {
                return Data(#"""
                [{"id":"cl-cjk-1","sessionKey":"claude:cl-cjk-1","title":"Claude Code 中文目录会话状态与禁改名提示","cwd":"/Users/example/Documents/程序开发/AgentHub","workspaceRoot":"/Users/example/Documents/程序开发/AgentHub","path":"/Users/example/.claude/projects/cl-cjk-1.jsonl","status":"reply_needed","formatVersion":"2.1.284","updatedAt":"2026-11-28T06:05:00Z","messageCount":4,"canRename":false,"renameBlockReason":"会话存在未决交互，暂不可改名","canDelete":true}]
                """#.utf8)
            }
            if cmd == "claude.detail" {
                return Data(#"""
                {"summary":{"id":"cl-cjk-1","sessionKey":"claude:cl-cjk-1","title":"Claude Code 中文目录会话状态与禁改名提示","cwd":"/Users/example/Documents/程序开发/AgentHub","workspaceRoot":"/Users/example/Documents/程序开发/AgentHub","path":"/Users/example/.claude/projects/cl-cjk-1.jsonl","status":"reply_needed","formatVersion":"2.1.284","updatedAt":"2026-11-28T06:05:00Z","messageCount":4,"canRename":false,"renameBlockReason":"会话存在未决交互，暂不可改名","canDelete":true},"events":[{"id":"c-u1","turnId":"t1","kind":"user_message","text":"请在中文目录 /Users/example/Documents/程序开发/AgentHub 下验证状态胶囊。"},{"id":"c-a1","turnId":"t1","kind":"assistant_message","assistantPhase":"final","text":"顶栏已同步显示需回复胶囊，禁改名提示收敛为紧凑单行条。"}],"totalEvents":2,"hasMore":false}
                """#.utf8)
            }
            return Self.mockResponse(cmd: cmd, args: args)
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        // A. 1000pt Narrow AppShell Probe Window (contentWidth = 804pt < 880pt, Light & Dark)
        let narrowSize = NSSize(width: 1000, height: 1320)
        let narrowWin = NSWindow(contentRect: NSRect(origin: .zero, size: narrowSize), styleMask: [.borderless], backing: .buffered, defer: false)
        for (scheme, appName, fileName) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "swift-12a-probe-1000x680-light.png"),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "swift-12b-probe-1000x680-dark.png"),
        ] {
            store.setTheme(scheme == .light ? .light : .dark)
            narrowWin.appearance = NSAppearance(named: appName)
            let probePalette = DesignPalette.resolve(scheme: scheme, workspace: .probe)
            let narrowHost = NSHostingView(
                rootView: HStack(spacing: 0) {
                    Color(nsColor: probePalette.railBg.nsColor).frame(width: LayoutMetrics.sideNavExpandedWidth)
                    ProbeWorkspaceView(store: store, initialOpenSections: Set(ProbeDisclosureSection.allCases))
                        .environment(\.agentHubPalette, probePalette)
                }
                .frame(width: narrowSize.width, height: narrowSize.height)
            )
            narrowHost.frame = NSRect(origin: .zero, size: narrowSize)
            narrowWin.contentView = narrowHost
            narrowWin.orderBack(nil)
            try await pumpMainRunLoop(ms: 350)
            let rendered = collectAllRenderedStrings(in: narrowHost)
            XCTAssertTrue(rendered.contains(where: { $0.contains("owner/repo") }), "1000pt narrow Probe must mount CI repository filter field")
            XCTAssertTrue(rendered.contains(where: { $0.contains("example.invalid") }), "1000pt narrow Probe must mount Bark URL field")
            XCTAssertTrue(rendered.contains(where: { $0.contains("Bark") }), "1000pt narrow Probe must mount Bark Device Key placeholder")
            if let rep = try writeOrCacheDisplay(window: narrowWin, host: narrowHost, outDir: outDir, fileName: fileName) {
                XCTAssertGreaterThan(
                    countForegroundContrastPixels(in: rep, logicalSize: narrowSize, topLeftRect: NSRect(x: 210, y: 16, width: 760, height: 1240), bg: probePalette.background),
                    800,
                    "1000pt narrow Probe (\(scheme)) must render non-blank CI, Bark, and Hook rows"
                )
            }
        }
        narrowWin.orderOut(nil)

        // B. 296pt DeepSeek Harness & Claude Code Sidebar Headers + Chinese cwd Path (Light & Dark)
        let wsSize = NSSize(width: 1280, height: 820)
        let wsWin = NSWindow(contentRect: NSRect(origin: .zero, size: wsSize), styleMask: [.borderless], backing: .buffered, defer: false)
        let shellHost = NSHostingView(rootView: AppShellView(store: store).frame(width: wsSize.width, height: wsSize.height))
        shellHost.frame = NSRect(origin: .zero, size: wsSize)
        wsWin.contentView = shellHost
        wsWin.orderBack(nil)

        for (ws, expectedTitle, prefix) in [
            (WorkspaceId.dsh, "DeepSeek Harness", "swift-13-dsh-296header-cjk"),
            (WorkspaceId.claude, "Claude Code", "swift-13-claude-296header-cjk"),
        ] {
            store.activeWorkspace = ws
            for (scheme, appName, suffix) in [
                (ColorSchemeKind.light, NSAppearance.Name.aqua, "light"),
                (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "dark"),
            ] {
                store.setTheme(scheme == .light ? .light : .dark)
                wsWin.appearance = NSAppearance(named: appName)
                try await pumpMainRunLoop(ms: 420)
                let allTexts = collectAllRenderedStrings(in: shellHost)
                XCTAssertTrue(allTexts.contains("搜索标题、ID 或工作目录"), "\(expectedTitle) sidebar search field must mount")
                XCTAssertTrue(
                    allTexts.contains(where: { $0.contains("程序开发/AgentHub") || $0.contains("顶栏") }),
                    "\(expectedTitle) conversation stage must mount decoded Chinese cwd message rows"
                )
                let pal = DesignPalette.resolve(scheme: scheme, workspace: ws)
                if let rep = try writeOrCacheDisplay(window: wsWin, host: shellHost, outDir: outDir, fileName: "\(prefix)-\(suffix).png") {
                    XCTAssertGreaterThan(
                        countForegroundContrastPixels(in: rep, logicalSize: wsSize, topLeftRect: NSRect(x: 206, y: 12, width: 190, height: 30), bg: pal.surface),
                        60,
                        "296pt sidebar header must render non-blank \(expectedTitle) glyphs (\(scheme))"
                    )
                    if ws == .dsh {
                        XCTAssertGreaterThan(
                            countForegroundContrastPixels(in: rep, logicalSize: wsSize, topLeftRect: NSRect(x: 325, y: 14, width: 32, height: 24), bg: pal.surface),
                            18,
                            "DeepSeek Harness trailing 'ness' glyphs must render without truncation in 296pt header (\(scheme))"
                        )
                    }
                    XCTAssertGreaterThan(
                        countForegroundContrastPixels(in: rep, logicalSize: wsSize, topLeftRect: NSRect(x: 510, y: 26, width: 320, height: 22), bg: pal.background),
                        40,
                        "Stage header must render Chinese cwd /Users/example/Documents/程序开发/AgentHub glyphs (\(scheme))"
                    )
                }
            }
        }
        wsWin.orderOut(nil)

        // C. Multi-Line Question Options & Multi-Line Answers Without Clipping (Light & Dark)
        let multilineQuestions = [
            UserInputQuestion(
                id: "arch-choice",
                header: "部署策略确认",
                question: "当前远程机器同时挂载了多个工作区目录，请选择跨机器同步与离线索引校验策略（支持多行长问题折行测量）：",
                options: [
                    UserInputOption(
                        label: "(Recommended) 启用单连接零双缓冲流式透传与增量缓存",
                        description: "服务端通过 write_raw_response 三段写出帧头与 RawValue 字节，客户端直接透传 UTF-8 原文并保留断线单次读重试与机器修订号隔离保护。"
                    ),
                    UserInputOption(
                        label: "保持完整离线快照回退兼容",
                        description: "不对存量 SQLite 表执行破坏性变更，确保任意时刻回滚至旧版本二进制仍可无损读取全部配置、加密通知队列与原生会话索引。"
                    ),
                ]
            ),
        ]
        let multilineAnswers = [
            UserInputAnswer(
                questionId: "arch-choice",
                answers: [
                    "(Recommended) 启用单连接零双缓冲流式透传与增量缓存",
                    "附加说明：同时在 1000×680 窄窗与浅深双色下校验多行问答卡片底部绝不裁切文字，保留完整内边距与圆角边框。",
                ]
            ),
        ]
        let qRow = ConversationRowItem(
            id: "q-multi-1",
            timelineId: "q-multi-1",
            kind: .questionResult(statusText: "已回复", questions: multilineQuestions, answers: multilineAnswers)
        )
        let cardWidth: CGFloat = 560
        for (scheme, appName, fileName) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "swift-14a-multiline-questions-light.png"),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "swift-14b-multiline-questions-dark.png"),
        ] {
            let coord = ConversationTableCoordinator(store: store)
            coord.scheme = scheme
            coord.workspace = .codex
            coord.sessionScopeKey = "test:multiline:\(scheme)"
            let measuredH = ceil(coord.measureRowHeight(qRow, width: cardWidth))
            XCTAssertGreaterThan(measuredH, 250, "Multi-line question/answer card height must expand dynamically beyond old fixed 56pt/44pt constants")

            let cellSize = NSSize(width: cardWidth + 48, height: measuredH + 12)
            let qWin = NSWindow(contentRect: NSRect(origin: .zero, size: cellSize), styleMask: [.borderless], backing: .buffered, defer: false)
            qWin.appearance = NSAppearance(named: appName)
            let pal = DesignPalette.resolve(scheme: scheme, workspace: .codex)
            let bgBox = NSView(frame: NSRect(origin: .zero, size: cellSize))
            bgBox.wantsLayer = true
            bgBox.layer?.backgroundColor = pal.background.nsColor.cgColor
            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("MultilineQCell"))
            cell.frame = NSRect(x: 0, y: 6, width: cellSize.width, height: measuredH)
            cell.configure(item: qRow, leadingInset: 24, trailingInset: 24, contentWidth: cardWidth, highlighted: false, coordinator: coord)
            bgBox.addSubview(cell)
            qWin.contentView = bgBox
            qWin.orderBack(nil)
            bgBox.layoutSubtreeIfNeeded()
            let stackFittingH = cell.containerStack.fittingSize.height
            XCTAssertLessThanOrEqual(stackFittingH, measuredH + 2.0, "Measured row height (\(measuredH)) must not clip containerStack fitting height (\(stackFittingH))")

            if let rep = try writeOrCacheDisplay(window: qWin, host: bgBox, outDir: outDir, fileName: fileName) {
                XCTAssertGreaterThan(
                    countForegroundContrastPixels(in: rep, logicalSize: cellSize, topLeftRect: NSRect(x: 32, y: 18, width: cardWidth - 16, height: 64), bg: pal.surface),
                    80,
                    "Multi-line question top header must render non-blank glyphs (\(scheme))"
                )
                XCTAssertGreaterThan(
                    countForegroundContrastPixels(in: rep, logicalSize: cellSize, topLeftRect: NSRect(x: 32, y: measuredH - 88, width: cardWidth - 16, height: 72), bg: pal.elevated),
                    60,
                    "Multi-line answer bottom box must render non-blank unclipped glyphs (\(scheme))"
                )
            }
            qWin.orderOut(nil)
        }
    }

    @MainActor
    private func writeOrCacheDisplay(
        window: NSWindow,
        host: NSView,
        outDir: String?,
        fileName: String
    ) throws -> NSBitmapImageRep? {
        if let outDir, !outDir.isEmpty {
            return try captureWindow(window, host: host, to: "\(outDir)/\(fileName)")
        }
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
            XCTFail("Failed to allocate bitmapImageRep for \(fileName)")
            return nil
        }
        host.cacheDisplay(in: host.bounds, to: rep)
        return rep
    }

    private func countForegroundContrastPixels(
        in rep: NSBitmapImageRep,
        logicalSize: NSSize,
        topLeftRect: NSRect,
        bg: RGBAColor
    ) -> Int {
        let scaleX = CGFloat(rep.pixelsWide) / max(1, logicalSize.width)
        let scaleY = CGFloat(rep.pixelsHigh) / max(1, logicalSize.height)
        let minX = max(0, Int(floor(topLeftRect.minX * scaleX)))
        let maxX = min(rep.pixelsWide, Int(ceil(topLeftRect.maxX * scaleX)))
        let minY = max(0, Int(floor(topLeftRect.minY * scaleY)))
        let maxY = min(rep.pixelsHigh, Int(ceil(topLeftRect.maxY * scaleY)))
        guard minX < maxX, minY < maxY else { return 0 }
        let bgR = CGFloat(bg.r) / 255.0
        let bgG = CGFloat(bg.g) / 255.0
        let bgB = CGFloat(bg.b) / 255.0
        var count = 0
        for py in minY ..< maxY {
            for px in minX ..< maxX {
                guard let c = rep.colorAt(x: px, y: py)?.usingColorSpace(.sRGB) else { continue }
                let dist = abs(c.redComponent - bgR) + abs(c.greenComponent - bgG) + abs(c.blueComponent - bgB)
                if dist > 0.18 {
                    count += 1
                }
            }
        }
        return count
    }

    @MainActor
    private func pumpMainRunLoop(ms: UInt64) async throws {
        try await Task.sleep(nanoseconds: ms * 1_000_000)
        await Task.yield()
    }

    @MainActor
    private func conversationTableRowCount(in root: NSView) -> Int {
        if let tv = root as? NSTableView {
            return tv.numberOfRows
        }
        for sub in root.subviews {
            let count = conversationTableRowCount(in: sub)
            if count > 0 { return count }
        }
        return 0
    }

    @MainActor
    private func collectVisibleStrings(in root: NSView) -> [String] {
        guard !root.isHidden, root.alphaValue > 0.01 else { return [] }
        var out: [String] = []
        if let tf = root as? NSTextField {
            if !tf.stringValue.isEmpty { out.append(tf.stringValue) }
            if let ph = tf.placeholderString, !ph.isEmpty { out.append(ph) }
        }
        if let tv = root as? NSTextView, !tv.string.isEmpty {
            out.append(tv.string)
        }
        for sub in root.subviews {
            out.append(contentsOf: collectVisibleStrings(in: sub))
        }
        return out
    }

    @MainActor
    private func collectAccessibilityStrings(from element: AnyObject, visited: inout Set<ObjectIdentifier>) -> [String] {
        let oid = ObjectIdentifier(element)
        guard visited.insert(oid).inserted else { return [] }
        var out: [String] = []
        if let ax = element as? NSAccessibilityProtocol {
            if let lbl = ax.accessibilityLabel(), !lbl.isEmpty { out.append(lbl) }
            if let title = ax.accessibilityTitle(), !title.isEmpty { out.append(title) }
            if let val = ax.accessibilityValue() as? String, !val.isEmpty { out.append(val) }
            if let ph = ax.accessibilityPlaceholderValue(), !ph.isEmpty { out.append(ph) }
            for child in (ax.accessibilityChildren() ?? []) {
                out.append(contentsOf: collectAccessibilityStrings(from: child as AnyObject, visited: &visited))
            }
        }
        if let view = element as? NSView {
            for sub in view.subviews {
                out.append(contentsOf: collectAccessibilityStrings(from: sub, visited: &visited))
            }
        }
        return out
    }

    @MainActor
    private func collectAllRenderedStrings(in root: NSView) -> [String] {
        var visited = Set<ObjectIdentifier>()
        return collectVisibleStrings(in: root) + collectAccessibilityStrings(from: root, visited: &visited)
    }

    private func countDarkInkPixels(
        in rep: NSBitmapImageRep,
        logicalSize: NSSize,
        topLeftRect: NSRect
    ) -> Int {
        let scaleX = CGFloat(rep.pixelsWide) / max(1, logicalSize.width)
        let scaleY = CGFloat(rep.pixelsHigh) / max(1, logicalSize.height)
        let minX = max(0, Int(floor(topLeftRect.minX * scaleX)))
        let maxX = min(rep.pixelsWide, Int(ceil(topLeftRect.maxX * scaleX)))
        let minY = max(0, Int(floor(topLeftRect.minY * scaleY)))
        let maxY = min(rep.pixelsHigh, Int(ceil(topLeftRect.maxY * scaleY)))
        guard minX < maxX, minY < maxY else { return 0 }
        var inkCount = 0
        for py in minY ..< maxY {
            for px in minX ..< maxX {
                guard let c = rep.colorAt(x: px, y: py)?.usingColorSpace(.sRGB) else { continue }
                if c.redComponent < 0.78 || c.greenComponent < 0.80 || c.blueComponent < 0.84 {
                    inkCount += 1
                }
            }
        }
        return inkCount
    }

    private func countErrorRedPixels(
        in rep: NSBitmapImageRep,
        logicalSize: NSSize,
        topLeftRect: NSRect
    ) -> Int {
        let scaleX = CGFloat(rep.pixelsWide) / max(1, logicalSize.width)
        let scaleY = CGFloat(rep.pixelsHigh) / max(1, logicalSize.height)
        let minX = max(0, Int(floor(topLeftRect.minX * scaleX)))
        let maxX = min(rep.pixelsWide, Int(ceil(topLeftRect.maxX * scaleX)))
        let minY = max(0, Int(floor(topLeftRect.minY * scaleY)))
        let maxY = min(rep.pixelsHigh, Int(ceil(topLeftRect.maxY * scaleY)))
        guard minX < maxX, minY < maxY else { return 0 }
        var redCount = 0
        for py in minY ..< maxY {
            for px in minX ..< maxX {
                guard let c = rep.colorAt(x: px, y: py)?.usingColorSpace(.sRGB) else { continue }
                // Light-mode palette.errFg is crimson #9F1239 (R=0.62, G=0.07, B=0.22 -> R-G > 0.16 && B > G),
                // cleanly separating errFg from neutral grays (R <= G <= B) and amber warnFg #7C2D12 (G > B).
                if (c.redComponent - c.greenComponent) > 0.16 && c.blueComponent > c.greenComponent {
                    redCount += 1
                }
            }
        }
        return redCount
    }

    @MainActor
    private func reloadAllTables(in root: NSView) {
        if let tv = root as? NSTableView {
            tv.reloadData()
            tv.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0 ..< tv.numberOfRows))
            tv.layoutSubtreeIfNeeded()
        }
        for sub in root.subviews {
            reloadAllTables(in: sub)
        }
    }

    @MainActor
    private func scrollConversationTable(in root: NSView, fraction: CGFloat) {
        if let scroll = root as? NSScrollView, let tv = scroll.documentView as? NSTableView {
            tv.layoutSubtreeIfNeeded()
            let clip = scroll.contentView
            let minY = -scroll.contentInsets.top
            let maxY = max(minY, tv.frame.height - clip.bounds.height + scroll.contentInsets.bottom)
            let targetY = minY + (maxY - minY) * max(0, min(1, fraction))
            clip.setBoundsOrigin(NSPoint(x: 0, y: targetY))
            scroll.reflectScrolledClipView(clip)
            return
        }
        for sub in root.subviews {
            scrollConversationTable(in: sub, fraction: fraction)
        }
    }

    @MainActor
    @discardableResult
    private func captureWindow(_ window: NSWindow, host: NSView, to path: String) throws -> NSBitmapImageRep? {
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
            XCTFail("Failed to allocate bitmapImageRep")
            return nil
        }
        host.cacheDisplay(in: host.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else {
            XCTFail("Failed to encode PNG")
            return nil
        }
        let url = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try png.write(to: url)
        return rep
    }

    private static func mockResponse(cmd: String, args: String) -> Data {
        let mib = 1024 * 1024
        switch cmd {
        case "remote.get":
            return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
        case "system.capabilities":
            return protocol2CapabilitiesFixture(remote: false)
        case "threads.list":
            if args.contains("\"archived\":true") { return Data("[]".utf8) }
            return Data("""
            [
              {"id":"th-visual-1","title":"Plan Mode 与线程详情视觉对齐核查","status":"ReplyNeeded","message_count":14,"latest_message":"已完成全部视觉层级核查，请确认。","rollout_path":"/workspace/.codex/sessions/rollout-1.jsonl","cwd":"/workspace/AgentHub","updated_at":"2026-11-19T12:00:00Z","storageSize":{"bytes":\(145 * mib),"scope":"file","status":"complete"}},
              {"id":"th-visual-2","title":"活动库审阅链路与长日志回归测试","status":"Running","message_count":18,"latest_message":"正在逐项审计脚本输出。","rollout_path":"/workspace/.codex/sessions/rollout-2.jsonl","cwd":"/workspace/AgentHub","updated_at":"2026-11-19T11:55:00Z","storageSize":{"bytes":\(18 * mib),"scope":"file","status":"complete"}},
              {"id":"th-visual-3","title":"超大压缩历史与存储阈值徽章检查","status":"Recent","message_count":42,"latest_message":"仓库状态干净。","rollout_path":"/workspace/.codex/sessions/rollout-3.jsonl","cwd":"/workspace/AgentHub","updated_at":"2026-11-19T10:30:00Z","storageSize":{"bytes":\(340 * mib),"scope":"file","status":"complete"}}
            ]
            """.utf8)
        case "threads.detail":
            return Data(#"""
            {
              "summary":{"id":"th-visual-1","title":"Plan Mode 与线程详情视觉对齐核查","status":"ReplyNeeded","message_count":14,"latest_message":"已完成全部视觉层级核查，请确认。","rollout_path":"/workspace/.codex/sessions/rollout-1.jsonl","cwd":"/workspace/AgentHub","updated_at":"2026-11-19T12:00:00Z"},
              "messages":[],
              "raw_event_count":9,
              "total_blocks":9,
              "has_more_blocks":false,
              "before_cursor":null,
              "subagents":{
                "complete":true,
                "counts":{"creating":0,"running":1,"completed":2,"failed":1,"interrupted":0,"unknown":0},
                "agents":[
                  {"agentId":"sub-1","name":"视觉对齐审查子智能体","status":"completed","available":true},
                  {"agentId":"sub-2","name":"排版度量核验器","status":"running","available":true},
                  {"agentId":"sub-3","name":"快照比对器","status":"failed","available":true},
                  {"agentId":"sub-4","name":"契约与线报文审查器","status":"completed","available":true}
                ]
              },
              "blocks":[
                {"id":"chat-history-collapsed","role":"tool","kind":"chat_history_collapsed","status":"completed","text":"12 条历史对话已折叠","summary":"12 条历史对话已折叠","tool_name":"chat_history","truncated":false,"questions":[],"answers":[]},
                {"id":"u-prompt-1","role":"user","kind":"userMessage","text":"<environment_context>\n  <cwd>/workspace/AgentHub</cwd>\n  <shell>zsh</shell>\n</environment_context>\n请核对线程详情的全部视觉层级：过程与工具折叠行、代码块、行内代码、表格、引用、计划卡片与问答。","user_message":{"id":"u-prompt-1","text":"请核对线程详情的全部视觉层级：过程与工具折叠行、代码块、行内代码、表格、引用、计划卡片与问答。","contextBlocks":[{"kind":"environment","text":"<environment_context>\n  <cwd>/workspace/AgentHub</cwd>\n  <shell>zsh</shell>\n</environment_context>"}],"attachments":[{"id":"att-1","name":"DESIGN.md","kind":"file","path":"/workspace/AgentHub/DESIGN.md"}]},"questions":[],"answers":[]},
                {"id":"call-1","role":"tool","kind":"function_call_output","tool_name":"read_file","call_id":"c1","turn_id":"turn-1","status":"completed","input":"{\"path\":\"/workspace/AgentHub/DESIGN.md\"}","text":"Read 180 lines from DESIGN.md","questions":[],"answers":[]},
                {"id":"call-2","role":"tool","kind":"function_call_output","tool_name":"exec_command","call_id":"c2","turn_id":"turn-1","status":"completed","input":"{\"cmd\":\"rg -n 'execution-group|markdown-content' webui/src/styles.css\"}","text":"451:.markdown-content {\n479:.execution-group > summary {","questions":[],"answers":[]},
                {"id":"sub-row-1","role":"tool","kind":"subagent","status":"completed","questions":[],"answers":[],"subagent":{"agentId":"sub-1","name":"视觉对齐审查子智能体","status":"completed","eventKind":"completed","available":true}},
                {"id":"plan-1","role":"assistant","kind":"plan","display_kind":"plan","turn_id":"turn-1","item_id":"plan-1","status":"completed","plan_status":"completed","resolved":true,"text":"<proposed_plan>1. 对齐 `.execution-group` 无边框行与左侧 `-20px` 图标\n2. 修复行内 `code` 圆角底色、代码块与表格列宽\n3. 实机双视图 PNG 逐帧核验</proposed_plan>","questions":[],"answers":[]},
                {"id":"q-res-1","role":"assistant","kind":"request_user_input_result","display_kind":"question_result","turn_id":"turn-1","status":"completed","resolved":true,"questions":[{"id":"mode","question":"选择验收对比基线","options":[{"label":"真实 WebUI 渲染截图","description":"使用 WKWebView 同源渲染逐像素比对"},{"label":"仅跑单元测试","description":"无法发现排版与样式偏差"}]}],"answers":[{"question_id":"mode","answers":["真实 WebUI 渲染截图"]}]},
                {"id":"u-reply-1","role":"user","kind":"userMessage","text":"<send_user_message_question_reply>[{\"questionItemId\":\"q-mode\",\"question\":\"请确认是否同时检查浅色与深色模式下的全部折叠展开态？\",\"answer\":\"确认，浅色、深色、折叠态与展开态全部并排比对。\"}]</send_user_message_question_reply>","questions":[],"answers":[]},
                {"id":"a-rich-1","role":"assistant","kind":"agentMessage","text":"# AGENTS.md instructions for /workspace/AgentHub\n\n<INSTRUCTIONS>\nFollow DESIGN.md and styles.css with zero visual drift.\n</INSTRUCTIONS>\n\n## 线程详情视觉核查结论\n\n已完成对 `Conversation.tsx`、[DESIGN.md](/workspace/AgentHub/DESIGN.md) 与 [在线规范](https://example.invalid/spec) 的逐项比对，重点修复 **折叠行边框冗余** 与 `inline code` 背景高度：\n\n- 工具与思考折叠行默认透明无外框，左侧 `-20px` 悬挂强调色图标；\n- 展开态左侧绘制 `2px solid var(--bs)` 纵向引导线；\n- 助教回复右侧悬浮 `30×30px` 复制按钮，零行高占用。\n\n> 最高品质原则：所有配色、字级、行高、间距与折叠交互，必须与现有桌面版 100% 一致。\n\n```rust\npub fn assert_zero_visual_drift(diff_ratio: f64) -> bool {\n    diff_ratio <= 0.005\n}\n```\n\n| 检查模块 | 字号 / 行高 | 状态 |\n| :--- | :---: | :--- |\n| 助教正文 | `15px / 1.72` | 已对齐 |\n| 代码块 | `13px / 1.60` | 无截断 |","questions":[],"answers":[]}
              ]
            }
            """#.utf8)
        case "claude.list":
            return Data("""
            [{"id":"cl-visual-1","sessionKey":"claude:cl-visual-1","title":"Claude 思考与工具过程组折叠验证","cwd":"/workspace/AgentHub","path":"/workspace/.claude/projects/cl-visual-1.jsonl","status":"recent","formatVersion":"2.1.284","updatedAt":"2026-11-19T12:00:00Z","messageCount":6,"storageSize":{"bytes":\(42 * mib),"scope":"file","status":"complete"},"canRename":true,"canDelete":true}]
            """.utf8)
        case "claude.detail":
            return Data(#"""
            {
              "summary":{"id":"cl-visual-1","sessionKey":"claude:cl-visual-1","title":"Claude 思考与工具过程组折叠验证","cwd":"/workspace/AgentHub","path":"/workspace/.claude/projects/cl-visual-1.jsonl","status":"recent","formatVersion":"2.1.284","updatedAt":"2026-11-19T12:00:00Z","messageCount":6,"canRename":true,"canDelete":true},
              "events":[
                {"id":"cl-u1","turnId":"t1","kind":"user_message","text":"检查 Claude 过程折叠与元数据组渲染。"},
                {"id":"cl-th1","turnId":"t1","kind":"thinking","text":"先读取样式表并对比 `.claude-process-group` 与 `.execution-rows` 的层级。"},
                {"id":"cl-tc1","turnId":"t1","kind":"tool_call","callId":"ctc1","role":"Read","status":"completed","detail":"{\"file_path\":\"/workspace/AgentHub/webui/src/styles.css\"}"},
                {"id":"cl-tr1","turnId":"t1","kind":"tool_result","callId":"ctc1","status":"completed","result":"Loaded 1312 lines"},
                {"id":"cl-sys1","turnId":"t1","kind":"system","role":"turn_duration","text":"耗时 1.2s","detail":"duration_ms: 1200"},
                {"id":"cl-a1","turnId":"t1","kind":"assistant_message","assistantPhase":"final","text":"## Claude 过程折叠完成\n\n已将思考步骤与 `Read` 工具合并进单个 `已思考 · 1 项工具` 折叠组。"}
              ],
              "totalEvents":6,
              "hasMore":false,
              "subagents":{"complete":true,"counts":{"creating":0,"running":0,"completed":1,"failed":0,"interrupted":0,"unknown":0},"agents":[{"agentId":"csub-1","name":"Explore subagent","status":"completed","available":true}]}
            }
            """#.utf8)
        case "probe.status":
            return Data(#"{"label":"Probe","enabled":true,"available":true,"platform":"macos","service_kind":"launchd","service_name":"com.lich13.agenthub.probe","flavor":"builtin","hook_status":"managed","bark_status":"configured","recent_event_count":2,"running_count":1,"reply_needed_count":1,"recoverable_count":0,"running_threads":[],"reply_needed_threads":[],"recoverable_threads":[],"lifecycle_status":"ok","doctor_status":"ok","runtime_version":"2.1.27","config_path":"~/Library/Application Support/AgentHub/config.toml","codex_home":"~/.codex","configured_codex_home":"~/.codex","resolved_codex_home":"~/.codex","codex_home_source":"default","logs_db_source":"resolved_codex_home","host_label":"local","snapshot_age_seconds":1,"is_refreshing":false,"snapshot_status":"fresh"}"#.utf8)
        case "probe.settings.get":
            return ProbeSettingsFixture.data
        case "probe.events":
            return Data(#"{"limit":10,"events":[]}"#.utf8)
        case "github.ci.status":
            return Data(#"{"account":{"id":1,"login":"octocat"},"enabled":true,"refreshing":false,"error":null,"last_scan_ms":1732017600000,"monitored_repositories":["octocat/AgentHub"],"repositories":[{"id":1,"full_name":"octocat/AgentHub","archived":false},{"id":2,"full_name":"octocat/swift-kit","archived":false}],"runs":[{"id":101,"run_attempt":1,"name":"Build and validate macOS Swift 6 native bundle","head_branch":"feat/macos-swift-native","head_sha":"c008e00a","status":"completed","conclusion":"success","html_url":"https://example.invalid/runs/101","created_at":"2026-11-19T11:50:00Z","updated_at":"2026-11-19T11:58:00Z","repository_id":1,"repository_name":"octocat/AgentHub","is_bot":false}]}"#.utf8)
        case "claude.hooks":
            return Data(#"{"installed":true,"managedCount":2,"changed":false,"configPath":"~/.claude/settings.json"}"#.utf8)
        case "updates.status":
            return Data(#"{"current_version":"2.1.27","latest_version":"v2.1.28","update_available":true,"channel":"stable","method":"macos_tauri_updater","state":"idle","failure_category":null,"recommended_action":null}"#.utf8)
        case "cleanup.unifiedDryRun":
            return Data(#"{"totalCandidates":4,"totalDeletable":4,"totalEstimatedBytes":32768,"scopes":[{"scope":"codex_archived","provider":"codex","label":"Codex 已归档线程","supported":true,"scanned":true,"unavailableReason":null,"candidateCount":2,"deletableCount":2,"estimatedBytes":24576,"candidates":[],"issues":[]},{"scope":"codex_hidden","provider":"codex","label":"Codex 隐藏线程","supported":true,"scanned":false,"unavailableReason":null,"candidateCount":0,"deletableCount":0,"estimatedBytes":0,"candidates":[],"issues":[]},{"scope":"claude_empty","provider":"claude","label":"Claude Code 空残余会话","supported":true,"scanned":true,"unavailableReason":null,"candidateCount":2,"deletableCount":2,"estimatedBytes":8192,"candidates":[],"issues":[]}]}"#.utf8)
        case "threads.accountScopeStatus":
            return Data(#"{"scopedThreads":2,"scopedFiles":3}"#.utf8)
        case "jobs.list":
            return Data("[]".utf8)
        default:
            return Data("{}".utf8)
        }
    }
}
