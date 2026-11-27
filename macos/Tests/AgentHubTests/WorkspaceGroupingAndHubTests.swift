@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class WorkspaceGroupingAndHubTests: XCTestCase {
    func testPathNormalizationNFCAndCaseSensitivityAndWorktreeCoalescing() {
        // NFD ("e\u{0301}") and NFC ("\u{00E9}") coalesce to identical NFC directory key
        let nfd = "/workspace/caf\u{0065}\u{0301}//AgentHub///"
        let nfc = "/workspace/caf\u{00E9}/AgentHub"
        let (k1, sub1) = ProjectWorkspaceEngine.resolveWorkspace(cwd: nfd, workspaceRoot: nil)
        let (k2, sub2) = ProjectWorkspaceEngine.resolveWorkspace(cwd: nfc, workspaceRoot: nil)
        XCTAssertEqual(k1, .directory(nfc))
        XCTAssertEqual(k1, k2)
        XCTAssertNil(sub1)
        XCTAssertNil(sub2)

        // Linux case-sensitivity preserved
        let (upperKey, _) = ProjectWorkspaceEngine.resolveWorkspace(cwd: "/workspace/Repo", workspaceRoot: nil)
        let (lowerKey, _) = ProjectWorkspaceEngine.resolveWorkspace(cwd: "/workspace/repo", workspaceRoot: nil)
        XCTAssertNotEqual(upperKey, lowerKey)

        // Git worktree coalescing from Rust workspaceRoot & fallback path patterns
        let (wtKey, wtSub) = ProjectWorkspaceEngine.resolveWorkspace(
            cwd: "/workspace/orca/worktrees/feat-ui",
            workspaceRoot: "/workspace/repos/AgentHub"
        )
        XCTAssertEqual(wtKey, .directory("/workspace/repos/AgentHub"))
        XCTAssertEqual(wtSub, "worktree · feat-ui")
        XCTAssertEqual(ProjectWorkspaceEngine.compactDisplayName(for: wtKey), "repos/AgentHub")

        let (claudeWtKey, claudeWtSub) = ProjectWorkspaceEngine.resolveWorkspace(
            cwd: "/workspace/repos/AgentHub/.claude/worktrees/fix-probe/crates",
            workspaceRoot: nil
        )
        XCTAssertEqual(claudeWtKey, .directory("/workspace/repos/AgentHub"))
        XCTAssertEqual(claudeWtSub, "worktree · fix-probe")

        let (emptyKey, _) = ProjectWorkspaceEngine.resolveWorkspace(cwd: "   ", workspaceRoot: nil)
        XCTAssertEqual(emptyKey, .unassigned)
        XCTAssertEqual(ProjectWorkspaceEngine.compactDisplayName(for: emptyKey), "未记录工作目录")
    }

    func testBoundarySafeWorkspaceFilterAndPriorityStatusSorting() {
        let filterExact = ProjectWorkspaceFilter.workspace(.directory("/workspace/repo"), includeSubdirectories: false)
        let filterSub = ProjectWorkspaceFilter.workspace(.directory("/workspace/repo"), includeSubdirectories: true)

        XCTAssertTrue(ProjectWorkspaceEngine.matchesFilter(filterExact, cwd: "/workspace/repo", workspaceRoot: nil))
        XCTAssertFalse(ProjectWorkspaceEngine.matchesFilter(filterExact, cwd: "/workspace/repo/packages/api", workspaceRoot: nil))
        XCTAssertTrue(ProjectWorkspaceEngine.matchesFilter(filterSub, cwd: "/workspace/repo/packages/api", workspaceRoot: nil))
        // Strict `/` boundary: `/workspace/repo-other` must NEVER match `/workspace/repo`!
        XCTAssertFalse(ProjectWorkspaceEngine.matchesFilter(filterSub, cwd: "/workspace/repo-other", workspaceRoot: nil))

        let items: [UnifiedSessionSummaryItem] = [
            UnifiedSessionSummaryItem(
                sessionKey: "u-empty", nativeId: "u-empty", title: "无目录会话", cwd: "", path: "",
                updatedAt: "2026-11-24T12:00:00Z", statusText: "最近", running: false, archived: false,
                canRename: true, canArchive: true, canDelete: true
            ),
            UnifiedSessionSummaryItem(
                sessionKey: "a-old", nativeId: "a-old", title: "旧仓库运行中", cwd: "/workspace/alpha", path: "",
                updatedAt: "2026-11-24T09:00:00Z", statusText: "运行中", statusChipBadge: "运行中", statusChipTone: .info,
                running: true, archived: false, canRename: true, canArchive: true, canDelete: true
            ),
            UnifiedSessionSummaryItem(
                sessionKey: "b-new", nativeId: "b-new", title: "新仓库待回复", cwd: "/workspace/beta/.codex/worktrees/w1",
                workspaceRoot: "/workspace/beta", path: "", updatedAt: "2026-11-24T11:00:00Z",
                statusText: "待回复", statusChipBadge: "待回复", statusChipTone: .warning,
                running: false, archived: false, canRename: true, canArchive: true, canDelete: true
            ),
        ]
        let groups = ProjectWorkspaceEngine.buildGroups(
            items: items,
            idOf: \.sessionKey,
            cwdOf: \.cwd,
            workspaceRootOf: \.workspaceRoot,
            updatedAtMsOf: { ProjectWorkspaceEngine.parseTimestampMs($0.updatedAt) },
            statusOf: ProjectWorkspaceEngine.itemPriorityStatus
        )
        XCTAssertEqual(groups.map(\.key), [
            .directory("/workspace/beta"),
            .directory("/workspace/alpha"),
            .unassigned,
        ])
        XCTAssertEqual(groups[0].aggregateStatus, .attention)
        XCTAssertEqual(groups[1].aggregateStatus, .running)
        XCTAssertEqual(groups[2].aggregateStatus, .idle)
    }

    @MainActor
    func testCodexReplyNeededNotClobberedByActiveTurnAndRemoteCapabilitiesDecoded() async throws {
        let replyThread = try JSONDecoder().decode(
            ThreadSummary.self,
            from: Data(#"{"id":"th-reply","title":"等待用户确认方案","status":"ReplyNeeded","message_count":1,"updated_at":"2026-11-24T10:00:00Z","cwd":"/workspace/AgentHub","active_turn_id":"turn-open-1"}"#.utf8)
        )

        let transport = MockRuntimeTransport { cmd, _ in
            if cmd == "system.capabilities" { return protocol2CapabilitiesFixture(remote: true) }
            return Data("{}".utf8)
        }
        let suite = "com.lich13.agenthub.ws-test.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let conn = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await conn.bootstrap()

        XCTAssertTrue(conn.capabilities.githubCi)
        XCTAssertTrue(conn.capabilities.codexAccountScope)
        XCTAssertTrue(conn.capabilities.hiddenCleanupScan)
        XCTAssertTrue(conn.capabilities.antigravitySessions)
        XCTAssertTrue(conn.capabilities.antigravityNotifications)
        XCTAssertTrue(conn.capabilities.dshSessions)
        XCTAssertFalse(conn.capabilities.dshNotifications)
        XCTAssertTrue(conn.capabilities.visibleWorkspaces.first == .workspaces)

        // Legacy remote server omitting `dsh_sessions` decodes `dshSessions == false` cleanly
        let legacyTransport = MockRuntimeTransport { cmd, _ in
            if cmd == "system.capabilities" {
                return Data(#"{"api_version":2,"host_surface":"linux_server_api","capabilities":{"threads":true,"dsh_sessions":false}}"#.utf8)
            }
            return Data("{}".utf8)
        }
        let legacyConn = RuntimeConnectionStore(transport: legacyTransport, defaults: defaults)
        await legacyConn.bootstrap()
        XCTAssertFalse(legacyConn.capabilities.dshSessions)

        let mapped = ProviderWorkspaceStore.mapCodexSummaryStatic(
            replyThread,
            archived: false,
            store: conn,
            threadStore: nil
        )
        XCTAssertFalse(mapped.running)
        XCTAssertEqual(mapped.statusText, "待回复")
        XCTAssertEqual(mapped.statusChipTone, .warning)
    }

    @MainActor
    func testVisualOrderShiftClickCollapsePrunesSelectionAndFilterResetsSelection() async throws {
        let transport = MockRuntimeTransport { cmd, args in
            switch cmd {
            case "remote.get":
                return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
            case "system.capabilities":
                return protocol2CapabilitiesFixture(remote: false)
            case "threads.list":
                if args.contains("\"archived\":true") { return Data("[]".utf8) }
                return Data(#"""
                [
                  {"id":"t1","title":"Alpha 1","status":"Recent","message_count":2,"cwd":"/workspace/alpha","updated_at":"2026-11-24T12:00:00Z"},
                  {"id":"t2","title":"Beta 1","status":"Recent","message_count":2,"cwd":"/workspace/beta","updated_at":"2026-11-24T11:00:00Z"},
                  {"id":"t3","title":"Alpha 2","status":"Recent","message_count":2,"cwd":"/workspace/alpha","updated_at":"2026-11-24T10:00:00Z"}
                ]
                """#.utf8)
            default:
                return Data("[]".utf8)
            }
        }
        let suite = "com.lich13.agenthub.sel-test.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        let wsStore = ProviderWorkspaceStore(store: store, workspace: .codex)
        wsStore.groupingMode = .byWorkspace
        await wsStore.fetchSessionList()

        // Visual order in byWorkspace mode groups `/workspace/alpha` (`t1`, `t3`) before `/workspace/beta` (`t2`)!
        XCTAssertEqual(wsStore.visualOrderedActiveKeys, ["t1", "t3", "t2"])

        _ = wsStore.partitionedSelection.click(
            partition: .active,
            visible: wsStore.visualOrderedActiveKeys,
            key: "t1",
            click: SessionClick(detail: 1, metaKey: true)
        )
        _ = wsStore.partitionedSelection.click(
            partition: .active,
            visible: wsStore.visualOrderedActiveKeys,
            key: "t3",
            click: SessionClick(detail: 1, shiftKey: true)
        )
        XCTAssertEqual(wsStore.partitionedSelection.active.keys, ["t1", "t3"])

        // Collapsing `/workspace/alpha` immediately prunes hidden selected rows (`t1`, `t3`)!
        wsStore.collapsedWorkspaceKeys.insert(.directory("/workspace/alpha"))
        XCTAssertTrue(wsStore.partitionedSelection.active.keys.isEmpty)
        XCTAssertFalse(wsStore.partitionedSelection.active.selecting)

        // Selecting `t2` then changing workspaceFilter resets multi-selection
        _ = wsStore.partitionedSelection.click(
            partition: .active,
            visible: wsStore.visualOrderedActiveKeys,
            key: "t2",
            click: SessionClick(detail: 1, metaKey: true)
        )
        XCTAssertEqual(wsStore.partitionedSelection.active.keys, ["t2"])
        wsStore.workspaceFilter = .workspace(.directory("/workspace/beta"), includeSubdirectories: false)
        XCTAssertTrue(wsStore.partitionedSelection.active.keys.isEmpty)
    }

    @MainActor
    func testCrossProviderWorkspaceHubAggregationBatchPreviewAndLightDarkOffscreenRenders() async throws {
        let mib = 1024 * 1024
        let transport = MockRuntimeTransport { cmd, args in
            switch cmd {
            case "remote.get":
                return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
            case "system.capabilities":
                return protocol2CapabilitiesFixture(remote: false)
            case "threads.list":
                if args.contains("\"archived\":true") { return Data("[]".utf8) }
                return Data("""
                [{"id":"th-hub-1","title":"Codex 主仓库架构清理","status":"Running","message_count":8,"latest_message":"正在检查跨层契约。","rollout_path":"/workspace/.codex/sessions/r1.jsonl","cwd":"/workspace/repos/AgentHub","updated_at":"2026-11-24T12:05:00Z","storageSize":{"bytes":\(28 * mib),"scope":"file","status":"complete"}}]
                """.utf8)
            case "threads.detail":
                return Data(#"""
                {
                  "summary":{"id":"th-hub-1","title":"Codex 主仓库架构清理","status":"Running","message_count":2,"latest_message":"正在检查跨层契约。","rollout_path":"/workspace/.codex/sessions/r1.jsonl","cwd":"/workspace/repos/AgentHub","updated_at":"2026-11-24T12:05:00Z"},
                  "messages":[],
                  "subagents":{"agents":[],"counts":{},"complete":true},
                  "raw_event_count":2,"total_blocks":2,"has_more_blocks":false,"before_cursor":null,
                  "blocks":[
                    {"id":"u1","role":"user","kind":"userMessage","text":"参考 Orca 增加按工作区的线程管理。","questions":[],"answers":[]},
                    {"id":"a1","role":"assistant","kind":"agentMessage","text":"已完成跨平台 `工作区` 总览页与 `.git` worktree 归并引擎，正在校验深浅色渲染。","questions":[],"answers":[]}
                  ]
                }
                """#.utf8)
            case "claude.list":
                return Data("""
                [{"id":"cl-hub-1","sessionKey":"claude:cl-hub-1","title":"Claude Worktree 通知与状态修复","cwd":"/workspace/orca/worktrees/feat-push","workspaceRoot":"/workspace/repos/AgentHub","path":"/workspace/.claude/projects/cl-hub-1.jsonl","status":"waiting","formatVersion":"2.1.284","updatedAt":"2026-11-24T12:10:00Z","messageCount":5,"storageSize":{"bytes":\(12 * mib),"scope":"file","status":"complete"},"canRename":true,"canDelete":true}]
                """.utf8)
            case "claude.detail":
                return Data(#"""
                {
                  "summary":{"id":"cl-hub-1","sessionKey":"claude:cl-hub-1","title":"Claude Worktree 通知与状态修复","cwd":"/workspace/orca/worktrees/feat-push","workspaceRoot":"/workspace/repos/AgentHub","path":"/workspace/.claude/projects/cl-hub-1.jsonl","status":"waiting","formatVersion":"2.1.284","updatedAt":"2026-11-24T12:10:00Z","messageCount":2,"canRename":true,"canDelete":true},
                  "events":[
                    {"id":"cu1","turnId":"t1","kind":"user_message","text":"修复 agy CLI 与 Claude 运行状态识别。"},
                    {"id":"ca1","turnId":"t1","kind":"assistant_message","text":"已补齐 `statusUpdatedAt` 因果收敛与树级锁继承，请确认交付范围。"}
                  ],
                  "totalEvents":2,"hasMore":false,"beforeCursor":null
                }
                """#.utf8)
            case "grok.list":
                return Data("""
                [{"id":"gk-hub-1","title":"Grok 独立仓库性能基准","cwd":"/workspace/services/gateway-api","path":"/workspace/.grok/sessions/gk-hub-1","status":"recent","updatedAt":"2026-11-24T11:30:00Z","messageCount":4,"storageSize":{"bytes":\(6 * mib),"scope":"directory","status":"complete"}}]
                """.utf8)
            case "antigravity.list":
                if args.contains("\"archived\":true") { return Data("[]".utf8) }
                return Data("""
                [{"id":"ag-hub-1","sessionKey":"ag-hub-1","title":"agy CLI 离线 Bark 推送闭环","cwd":"/workspace/repos/AgentHub","path":"/workspace/.gemini/antigravity-cli/conversations/ag-hub-1.db","archived":false,"updatedAt":"2026-11-24T12:08:00Z","messageCount":6,"status":"reply_needed","formatVersion":"agy-cli-v1","canRename":true,"canArchive":true,"canDelete":true,"storageSize":{"bytes":\(19 * mib),"scope":"file","status":"complete"}}]
                """.utf8)
            case "dsh.list":
                if args.contains("\"archived\":true") { return Data("[]".utf8) }
                return Data("""
                [{"id":"dsh-hub-1","sessionKey":"dsh-hub-1","title":"临时命令行草稿验证","cwd":"","path":"/workspace/.dsh/sessions/dsh-hub-1/session.v4.jsonl.zstd","archived":false,"updatedAt":"2026-11-24T10:00:00Z","messageCount":2,"status":"idle","formatVersion":"dsh-v4","canRename":true,"canArchive":true,"canDelete":true}]
                """.utf8)
            case "sessions.bulkPreview":
                if args.contains("\"codex\"") {
                    return Data(#"{"provider":"codex","operation":"delete","items":[{"sessionKey":"th-hub-1","id":"th-hub-1","title":"Codex 主仓库架构清理","paths":["/workspace/.codex/sessions/r1.jsonl"],"fileCount":1,"bytes":1024,"allowed":true}]}"#.utf8)
                }
                return Data(#"{"provider":"claude_code","operation":"delete","items":[{"sessionKey":"claude:cl-hub-1","id":"cl-hub-1","title":"Claude Worktree 通知与状态修复","paths":["/workspace/.claude/projects/cl-hub-1.jsonl"],"fileCount":1,"bytes":2048,"allowed":true}]}"#.utf8)
            case "sessions.bulkExecute":
                if args.contains("\"codex\"") {
                    return Data(#"{"provider":"codex","operation":"delete","items":[{"sessionKey":"th-hub-1","id":"th-hub-1","status":"succeeded","bytes":1024}]}"#.utf8)
                }
                return Data(#"{"provider":"claude_code","operation":"delete","items":[{"sessionKey":"claude:cl-hub-1","id":"cl-hub-1","status":"succeeded","bytes":2048}]}"#.utf8)
            default:
                return Data("{}".utf8)
            }
        }

        let suite = "com.lich13.agenthub.hub-render.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        let hub = UnifiedWorkspaceHubStore(store: store)
        await hub.fetchAllSessions()
        XCTAssertEqual(hub.activeItems.count, 5)
        XCTAssertEqual(hub.activeWorkspaceGroups.map(\.key), [
            .directory("/workspace/repos/AgentHub"),
            .directory("/workspace/services/gateway-api"),
            .unassigned,
        ])
        // `/workspace/repos/AgentHub` coalesces Codex + Claude worktree + Antigravity CLI (3 items)
        XCTAssertEqual(hub.activeWorkspaceGroups[0].items.count, 3)
        XCTAssertEqual(hub.activeWorkspaceGroups[0].aggregateStatus, .attention)
        XCTAssertEqual(
            hub.activeWorkspaceGroups[0].subtitlesByItemId["claude_code::claude:cl-hub-1"],
            "worktree · feat-push"
        )

        // Cross-provider batch delete preview merges Codex + Claude into a single confirmation plan
        await hub.prepareBatch(
            operation: .delete,
            compositeKeys: ["codex::th-hub-1", "claude_code::claude:cl-hub-1"],
            partition: .active
        )
        XCTAssertEqual(hub.batchPlan?.mergedPreview.items.count, 2)
        XCTAssertEqual(hub.batchPlan?.mergedPreview.totalBytes, 3072)

        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }

        let outDir = ProcessInfo.processInfo.environment["AGENTHUB_VISUAL_CAPTURE_DIR"]
        let size = NSSize(width: 1280, height: 820)
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

        store.activeWorkspace = .workspaces
        for (scheme, appearance, filename) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "workspace-hub-light.png"),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "workspace-hub-dark.png"),
        ] {
            store.setTheme(scheme == .light ? .light : .dark)
            window.appearance = NSAppearance(named: appearance)
            try await Task.sleep(nanoseconds: 450_000_000)
            await Task.yield()
            host.layoutSubtreeIfNeeded()
            window.displayIfNeeded()

            guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
                XCTFail("Failed to allocate bitmapImageRep for \(filename)")
                continue
            }
            host.cacheDisplay(in: host.bounds, to: rep)

            // Assert non-blank glyph/icon pixels in left rail, workspace group list, and reading column
            let palette = DesignPalette.resolve(scheme: scheme, workspace: .workspaces)
            assertRegionHasForegroundPixels(
                rep: rep,
                rect: NSRect(x: 12, y: 40, width: 175, height: 340),
                background: palette.railBg,
                label: "nav-rail-\(scheme)"
            )
            assertRegionHasForegroundPixels(
                rep: rep,
                rect: NSRect(x: 206, y: 40, width: 276, height: 520),
                background: palette.surface,
                label: "workspace-groups-sidebar-\(scheme)"
            )
            assertRegionHasForegroundPixels(
                rep: rep,
                rect: NSRect(x: 520, y: 40, width: 680, height: 420),
                background: palette.background,
                label: "reading-stage-\(scheme)"
            )

            if let outDir, !outDir.isEmpty,
               let png = rep.representation(using: .png, properties: [:])
            {
                let url = URL(fileURLWithPath: outDir).appendingPathComponent(filename)
                try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
                try png.write(to: url)
            }
        }

        // Also render per-provider byWorkspace grouped view with active filter pill in Light & Dark
        let codexStore = ProviderWorkspaceStore(store: store, workspace: .codex)
        codexStore.groupingMode = .byWorkspace
        codexStore.workspaceFilter = .workspace(.directory("/workspace/repos/AgentHub"), includeSubdirectories: true)
        await codexStore.fetchSessionList()
        codexStore.selectedKey = "th-hub-1"
        if let selected = codexStore.selectedItem {
            await codexStore.fetchSessionDetail(for: selected)
        }
        for (scheme, appearance, filename) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "provider-grouped-light.png"),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "provider-grouped-dark.png"),
        ] {
            let pal = DesignPalette.resolve(scheme: scheme, workspace: .codex)
            let groupedWin = NSWindow(
                contentRect: NSRect(origin: .zero, size: size),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            groupedWin.isOpaque = true
            groupedWin.backgroundColor = NSColor(
                srgbRed: CGFloat(pal.background.r) / 255.0,
                green: CGFloat(pal.background.g) / 255.0,
                blue: CGFloat(pal.background.b) / 255.0,
                alpha: 1.0
            )
            groupedWin.appearance = NSAppearance(named: appearance)
            let groupedHost = NSHostingView(
                rootView: UnifiedProviderWorkspaceView(model: codexStore, scheme: scheme)
                    .background(pal.background.color)
                    .environment(\.agentHubPalette, pal)
                    .environment(\.colorScheme, scheme == .dark ? .dark : .light)
                    .frame(width: size.width, height: size.height)
            )
            groupedHost.frame = NSRect(origin: .zero, size: size)
            groupedWin.contentView = groupedHost
            groupedWin.orderBack(nil)
            try await Task.sleep(nanoseconds: 400_000_000)
            await Task.yield()
            groupedHost.layoutSubtreeIfNeeded()
            groupedWin.displayIfNeeded()
            if let rep = groupedHost.bitmapImageRepForCachingDisplay(in: groupedHost.bounds) {
                groupedHost.cacheDisplay(in: groupedHost.bounds, to: rep)
                assertRegionHasForegroundPixels(
                    rep: rep,
                    rect: NSRect(x: 10, y: 12, width: 276, height: 220),
                    background: pal.surface,
                    label: "provider-grouped-\(scheme)"
                )
                assertRegionHasForegroundPixels(
                    rep: rep,
                    rect: NSRect(x: 330, y: 40, width: 680, height: 420),
                    background: pal.background,
                    label: "provider-grouped-stage-\(scheme)"
                )
                if let outDir, !outDir.isEmpty,
                   let png = rep.representation(using: .png, properties: [:])
                {
                    let url = URL(fileURLWithPath: outDir).appendingPathComponent(filename)
                    try png.write(to: url)
                }
            }
            groupedWin.orderOut(nil)
        }

        // Render UnifiedWorkspaceHubView with active batch confirmation dialog + archived drawer expanded in Light & Dark,
        // and verify right-aligned TaskMenuTriggerButton anchor geometry.
        hub.archivedOpen = true
        for (scheme, appearance, modalName, menuName) in [
            (ColorSchemeKind.light, NSAppearance.Name.aqua, "hub-confirm-modal-light.png", "hub-filter-menu-light.png"),
            (ColorSchemeKind.dark, NSAppearance.Name.darkAqua, "hub-confirm-modal-dark.png", "hub-filter-menu-dark.png"),
        ] {
            let pal = DesignPalette.resolve(scheme: scheme, workspace: .workspaces)
            let modalWin = UnconstrainedOffscreenWindow(
                contentRect: NSRect(x: 120, y: 120, width: size.width, height: size.height),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            modalWin.isOpaque = true
            modalWin.backgroundColor = pal.background.nsColor
            modalWin.appearance = NSAppearance(named: appearance)
            let modalHost = NSHostingView(
                rootView: UnifiedWorkspaceHubView(hubModel: hub, scheme: scheme)
                    .background(pal.background.color)
                    .environment(\.agentHubPalette, pal)
                    .environment(\.colorScheme, scheme == .dark ? .dark : .light)
                    .frame(width: size.width, height: size.height)
            )
            modalHost.frame = NSRect(origin: .zero, size: size)
            modalWin.contentView = modalHost
            modalWin.orderBack(nil)
            try await Task.sleep(nanoseconds: 250_000_000)
            await Task.yield()
            modalHost.layoutSubtreeIfNeeded()
            modalWin.displayIfNeeded()
            if let rep = modalHost.bitmapImageRepForCachingDisplay(in: modalHost.bounds) {
                modalHost.cacheDisplay(in: modalHost.bounds, to: rep)
                assertRegionHasForegroundPixels(
                    rep: rep,
                    rect: NSRect(x: 380, y: 240, width: 520, height: 320),
                    background: pal.elevated,
                    label: "hub-confirm-modal-\(scheme)"
                )
                if let outDir, !outDir.isEmpty,
                   let png = rep.representation(using: .png, properties: [:])
                {
                    let url = URL(fileURLWithPath: outDir).appendingPathComponent(modalName)
                    try png.write(to: url)
                }
            }

            // Verify right-aligned menu presentation below the real MenuAnchorProbeNSView header button anchor
            func findFilterMenuAnchor(_ view: NSView) -> NSView? {
                if String(reflecting: type(of: view)).contains("MenuAnchorProbeNSView") {
                    return view
                }
                for sub in view.subviews {
                    if let found = findFilterMenuAnchor(sub) {
                        return found
                    }
                }
                return nil
            }
            let anchorButton = try XCTUnwrap(findFilterMenuAnchor(modalHost), "Expected MenuAnchorProbeNSView in UnifiedWorkspaceHubView header")
            let filterActions = [
                SessionMenuAction(id: "all", label: "全部工作区", icon: .check, run: {}),
                SessionMenuAction(id: "sub", label: "包含子目录：已开启", icon: .check, run: {}),
                SessionMenuAction(id: "ws1", label: "repos/AgentHub (3)", icon: .folderOpen, reason: "/workspace/repos/AgentHub", run: {}),
            ]
            AgentHubTaskMenuPresenter.shared.presentBelowAnchorView(
                anchorButton,
                rightAligned: true,
                actions: filterActions,
                palette: pal
            )
            try await Task.sleep(nanoseconds: 60_000_000)
            if let panel = modalWin.childWindows?.first, let menuView = panel.contentView {
                let anchorScreen = modalWin.convertToScreen(anchorButton.convert(anchorButton.bounds, to: nil))
                XCTAssertEqual(panel.frame.maxX, anchorScreen.maxX, accuracy: 1.5, "Right-aligned menu maxX must align with anchor button maxX")
                if let rep = menuView.bitmapImageRepForCachingDisplay(in: menuView.bounds) {
                    menuView.cacheDisplay(in: menuView.bounds, to: rep)
                    assertRegionHasForegroundPixels(
                        rep: rep,
                        rect: menuView.bounds,
                        background: pal.elevated,
                        label: "hub-filter-menu-\(scheme)"
                    )
                    if let outDir, !outDir.isEmpty,
                       let png = rep.representation(using: .png, properties: [:])
                    {
                        let url = URL(fileURLWithPath: outDir).appendingPathComponent(menuName)
                        try png.write(to: url)
                    }
                }
            } else {
                XCTFail("Expected child menu panel to be presented for \(scheme)")
            }
            AgentHubTaskMenuPresenter.shared.dismiss()
            modalWin.orderOut(nil)
        }
    }

    @MainActor
    func testWorkspaceHubPartialBatchFailureSearchSelectionSyncAndStageSeedDetailSync() async throws {
        final class MockState: @unchecked Sendable {
            let lock = NSLock()
            var codexDeleted = false
            var dshShouldThrow = false
            var claudeExecuteCalls = 0
        }
        let mock = MockState()
        let transport = MockRuntimeTransport { cmd, args in
            switch cmd {
            case "remote.get":
                return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
            case "system.capabilities":
                return protocol2CapabilitiesFixture(remote: false)
            case "threads.list":
                if args.contains("\"archived\":true") { return Data("[]".utf8) }
                let isDeleted = mock.lock.withLock { mock.codexDeleted }
                if isDeleted {
                    return Data(#"[{"id":"th-2","title":"Codex 第二会话","status":"Recent","message_count":2,"cwd":"/workspace/other","updated_at":"2026-11-24T11:00:00Z"}]"#.utf8)
                }
                return Data(#"""
                [
                  {"id":"th-1","title":"Codex 第一会话","status":"Running","message_count":2,"cwd":"/workspace/repo","updated_at":"2026-11-24T12:00:00Z"},
                  {"id":"th-2","title":"Codex 第二会话","status":"Recent","message_count":2,"cwd":"/workspace/other","updated_at":"2026-11-24T11:00:00Z"}
                ]
                """#.utf8)
            case "threads.detail":
                return Data(#"""
                {
                  "summary":{"id":"th-1","title":"Codex 第一会话（已收敛）","status":"ReplyNeeded","message_count":2,"cwd":"/workspace/repo","updated_at":"2026-11-24T12:01:00Z"},
                  "messages":[],"subagents":{"agents":[],"counts":{},"complete":true},
                  "raw_event_count":1,"total_blocks":1,"has_more_blocks":false,"before_cursor":null,
                  "blocks":[{"id":"b1","role":"assistant","kind":"agentMessage","text":"请确认方案？","questions":[],"answers":[]}]
                }
                """#.utf8)
            case "dsh.list":
                if args.contains("\"archived\":true") { return Data("[]".utf8) }
                return Data(#"[{"id":"dsh-1","sessionKey":"dsh-1","title":"DSH 锁定会话","cwd":"/workspace/repo","path":"/workspace/.dsh/sessions/dsh-1","archived":false,"updatedAt":"2026-11-24T11:50:00Z","messageCount":2,"status":"running","formatVersion":"dsh-v4","canRename":false,"canArchive":false,"canDelete":false}]"#.utf8)
            case "claude.list":
                return Data(#"[{"id":"cl-1","sessionKey":"claude:cl-1","title":"Claude 待删会话","cwd":"/workspace/repo","path":"/workspace/.claude/cl-1.jsonl","status":"recent","formatVersion":"2.1.284","updatedAt":"2026-11-24T11:40:00Z","messageCount":2,"canRename":true,"canDelete":true}]"#.utf8)
            case "grok.list", "antigravity.list":
                return Data("[]".utf8)
            case "sessions.bulkPreview":
                if args.contains("\"codex\"") {
                    return Data(#"{"provider":"codex","operation":"delete","items":[{"sessionKey":"th-1","id":"th-1","title":"Codex 第一会话","paths":["/workspace/.codex/r1.jsonl"],"fileCount":1,"bytes":512,"allowed":true}]}"#.utf8)
                }
                if args.contains("\"dsh\"") {
                    return Data(#"{"provider":"dsh","operation":"delete","items":[{"sessionKey":"dsh-1","id":"dsh-1","title":"DSH 锁定会话","paths":["/workspace/.dsh/dsh-1"],"fileCount":1,"bytes":256,"allowed":true}]}"#.utf8)
                }
                return Data(#"{"provider":"claude_code","operation":"delete","items":[{"sessionKey":"claude:cl-1","id":"cl-1","title":"Claude 待删会话","paths":["/workspace/.claude/cl-1.jsonl"],"fileCount":1,"bytes":128,"allowed":true}]}"#.utf8)
            case "sessions.bulkExecute":
                if args.contains("\"codex\"") {
                    mock.lock.withLock { mock.codexDeleted = true }
                    return Data(#"{"provider":"codex","operation":"delete","items":[{"sessionKey":"th-1","id":"th-1","status":"succeeded","bytes":512}]}"#.utf8)
                }
                if args.contains("\"dsh\"") {
                    let shouldThrow = mock.lock.withLock { mock.dshShouldThrow }
                    if shouldThrow {
                        throw NSError(domain: "Test", code: 500, userInfo: [NSLocalizedDescriptionKey: "DSH 正在运行，请先退出"])
                    }
                    return Data(#"{"provider":"dsh","operation":"delete","items":[{"sessionKey":"dsh-1","id":"dsh-1","status":"failed","message":"会话已被占用"}]}"#.utf8)
                }
                mock.lock.withLock { mock.claudeExecuteCalls += 1 }
                return Data(#"{"provider":"claude_code","operation":"delete","items":[{"sessionKey":"claude:cl-1","id":"cl-1","status":"succeeded","bytes":128}]}"#.utf8)
            default:
                return Data("{}".utf8)
            }
        }

        let suite = "com.lich13.agenthub.hub-partial.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        let hub = UnifiedWorkspaceHubStore(store: store)
        await hub.fetchAllSessions()

        // 1. Embedded stage detail poll updates `externalSeedItem` immediately even when sidebar is hidden (`activeItems == []`)
        let embeddedStage = ProviderWorkspaceStore(store: store, workspace: .codex)
        embeddedStage.skipSidebarListFetch = true
        let initialSeed = try XCTUnwrap(hub.activeItems.first(where: { $0.compositeKey == "codex::th-1" })?.item)
        embeddedStage.externalSeedItem = initialSeed
        embeddedStage.selectedKey = "th-1"
        embeddedStage.onExternalSelectSession = { [weak hub] nextKey in
            hub?.handleStageSessionSelected(provider: .codex, sessionKey: nextKey)
        }
        await embeddedStage.fetchSessionDetail(for: initialSeed)
        XCTAssertEqual(embeddedStage.selectedItem?.statusText, "待回复")
        XCTAssertEqual(embeddedStage.selectedItem?.title, "Codex 第一会话（已收敛）")

        // 2. Search selection inside embedded stage updates `hub.selectedCompositeKey` and clears restrictive workspace filter
        hub.workspaceFilter = .workspace(.directory("/workspace/repo"), includeSubdirectories: false)
        let searchHitJson = Data(#"{"resultId":"r1","sessionKey":"th-2","nativeId":"th-2","title":"Codex 第二会话","cwd":"/workspace/other","matchKind":"user_message","positionKey":"b1","snippet":"hit"}"#.utf8)
        let searchHit = try JSONDecoder().decode(SessionSearchResult.self, from: searchHitJson)
        embeddedStage.handleSearchResultSelected(searchHit)
        XCTAssertEqual(hub.selectedCompositeKey, "codex::th-2")
        XCTAssertEqual(hub.workspaceFilter, .all)

        // 3. Cross-provider batch delete where Codex succeeds, DSH throws interruption, and Claude follows:
        //    Codex item is deselected (not rolled back), DSH gets "请求中断，结果未确认", Claude is NOT executed ("前序请求中断，未执行"), and both failed items remain selected!
        mock.lock.withLock { mock.dshShouldThrow = true }
        hub.selectAllVisible()
        XCTAssertEqual(Set(hub.partitionedSelection.active.keys), Set(["codex::th-1", "dsh::dsh-1", "claude_code::claude:cl-1", "codex::th-2"]))
        await hub.prepareBatch(
            operation: .delete,
            compositeKeys: ["codex::th-1", "dsh::dsh-1", "claude_code::claude:cl-1"],
            partition: .active
        )
        await hub.executePreparedBatch()
        let claudeCalls = mock.lock.withLock { mock.claudeExecuteCalls }
        XCTAssertEqual(claudeCalls, 0, "Subsequent provider write RPC must not fire after request interruption")
        XCTAssertFalse(hub.partitionedSelection.active.keys.contains("codex::th-1"), "Confirmed succeeded Codex item must be deselected")
        XCTAssertTrue(hub.partitionedSelection.active.keys.contains("dsh::dsh-1"), "Interrupted DSH item must remain selected")
        XCTAssertTrue(hub.partitionedSelection.active.keys.contains("claude_code::claude:cl-1"), "Unexecuted Claude item must remain selected")
        let batchItems = try XCTUnwrap(hub.batchResult?.items)
        XCTAssertEqual(batchItems.count, 3)
        XCTAssertEqual(batchItems[0].status, "succeeded")
        XCTAssertEqual(batchItems[1].message, "请求中断，结果未确认")
        XCTAssertEqual(batchItems[2].message, "前序请求中断，未执行")
    }

    @MainActor
    func testUnifiedProviderWorkspaceViewSyncsExternalBindingsAcrossAppearAndUpdates() async throws {
        let transport = MockRuntimeTransport { cmd, _ in
            if cmd == "remote.get" {
                return Data(#"{"target":"local","revision":1,"sshHost":null,"configured":false,"activeMachineId":null,"activeMachineName":null,"machines":[]}"#.utf8)
            }
            if cmd == "system.capabilities" {
                return protocol2CapabilitiesFixture(remote: false)
            }
            return Data("[]".utf8)
        }
        let suite = "com.lich13.agenthub.ws-bindings.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
        await store.bootstrap()

        @MainActor
        final class WrapperModel: ObservableObject {
            @Published var hideSidebar: Bool = false
            @Published var generation: Int = 1
            var recordedMutations: [Int] = []
            var recordedSelections: [(Int, String?)] = []
        }

        struct HostWrapper: View {
            @ObservedObject var store: RuntimeConnectionStore
            @ObservedObject var wrapper: WrapperModel
            let retainedModel: ProviderWorkspaceStore
            let seedA: UnifiedSessionSummaryItem
            let seedB: UnifiedSessionSummaryItem

            var body: some View {
                let gen = wrapper.generation
                UnifiedProviderWorkspaceView(
                    model: retainedModel,
                    scheme: .dark,
                    hideSidebar: wrapper.hideSidebar,
                    externalSeedItem: gen < 3 ? seedA : seedB,
                    onExternalMutation: {
                        wrapper.recordedMutations.append(gen)
                    },
                    onExternalSelectSession: { key in
                        wrapper.recordedSelections.append((gen, key))
                    }
                )
            }
        }

        let seedA = UnifiedSessionSummaryItem(
            sessionKey: "th-a",
            nativeId: "th-a",
            title: "Seed A",
            cwd: "/workspace/a",
            workspaceRoot: "/workspace/a",
            path: "/tmp/a.jsonl",
            updatedAt: "2026-01-01T00:00:00Z",
            statusText: "空闲",
            statusChipTone: .neutral,
            running: false,
            archived: false,
            canRename: true,
            canArchive: true,
            canDelete: true
        )
        let seedB = UnifiedSessionSummaryItem(
            sessionKey: "th-b",
            nativeId: "th-b",
            title: "Seed B",
            cwd: "/workspace/b",
            workspaceRoot: "/workspace/b",
            path: "/tmp/b.jsonl",
            updatedAt: "2026-01-01T00:01:00Z",
            statusText: "空闲",
            statusChipTone: .neutral,
            running: false,
            archived: false,
            canRename: true,
            canArchive: true,
            canDelete: true
        )

        let wrapper = WrapperModel()
        let retainedModel = ProviderWorkspaceStore(store: store, workspace: .codex)
        let win = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 960, height: 640),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        let hosting = NSHostingView(
            rootView: HostWrapper(
                store: store,
                wrapper: wrapper,
                retainedModel: retainedModel,
                seedA: seedA,
                seedB: seedB
            )
        )
        // Flip skipSidebarListFetch opposite to hideSidebar=false before .onAppear fires
        retainedModel.skipSidebarListFetch = true
        win.contentView = hosting
        win.orderBack(nil)
        defer { win.orderOut(nil) }
        hosting.layoutSubtreeIfNeeded()
        try await Task.sleep(nanoseconds: 50_000_000)

        // 1) .onAppear synced skipSidebarListFetch=false, initial seedA, and generation=1 closures
        let initialSkip = retainedModel.skipSidebarListFetch
        XCTAssertEqual(initialSkip, false)
        XCTAssertEqual(retainedModel.externalSeedItem?.sessionKey, "th-a")
        XCTAssertEqual(retainedModel.selectedKey, "th-a")
        retainedModel.onExternalMutation?()
        retainedModel.onExternalSelectSession?("k1")
        let mut1 = wrapper.recordedMutations
        let sel1 = wrapper.recordedSelections
        XCTAssertEqual(mut1, [1])
        XCTAssertEqual(sel1.count, 1)
        XCTAssertEqual(sel1[0].0, 1)
        XCTAssertEqual(sel1[0].1, "k1")

        // 2) .onChange(of: hideSidebar) syncs skipSidebarListFetch=true and fresh generation=2 closures onto retained @StateObject
        wrapper.generation = 2
        wrapper.hideSidebar = true
        hosting.layoutSubtreeIfNeeded()
        try await Task.sleep(nanoseconds: 50_000_000)

        let updatedSkip = retainedModel.skipSidebarListFetch
        XCTAssertEqual(updatedSkip, true)
        retainedModel.onExternalMutation?()
        retainedModel.onExternalSelectSession?("k2")
        let mut2 = wrapper.recordedMutations
        let sel2 = wrapper.recordedSelections
        XCTAssertEqual(mut2, [1, 2])
        XCTAssertEqual(sel2.count, 2)
        XCTAssertEqual(sel2[1].0, 2)
        XCTAssertEqual(sel2[1].1, "k2")

        // 3) .onChange(of: externalSeedItem) also refreshes closures to generation=3 even when hideSidebar stays true
        wrapper.generation = 3
        hosting.layoutSubtreeIfNeeded()
        try await Task.sleep(nanoseconds: 50_000_000)

        let finalKey = retainedModel.selectedKey
        XCTAssertEqual(finalKey, "th-b")
        retainedModel.onExternalMutation?()
        retainedModel.onExternalSelectSession?("k3")
        let mut3 = wrapper.recordedMutations
        let sel3 = wrapper.recordedSelections
        XCTAssertEqual(mut3, [1, 2, 3])
        XCTAssertEqual(sel3.count, 3)
        XCTAssertEqual(sel3[2].0, 3)
        XCTAssertEqual(sel3[2].1, "k3")

        // 4) Pure parent re-render (neither hideSidebar nor externalSeedItem changed) still updates closures to generation=4
        wrapper.generation = 4
        hosting.layoutSubtreeIfNeeded()
        try await Task.sleep(nanoseconds: 50_000_000)

        retainedModel.onExternalMutation?()
        retainedModel.onExternalSelectSession?("k4")
        let mut4 = wrapper.recordedMutations
        let sel4 = wrapper.recordedSelections
        XCTAssertEqual(mut4, [1, 2, 3, 4])
        XCTAssertEqual(sel4.count, 4)
        XCTAssertEqual(sel4[3].0, 4)
        XCTAssertEqual(sel4[3].1, "k4")
    }

    private func assertRegionHasForegroundPixels(
        rep: NSBitmapImageRep,
        rect: NSRect,
        background: RGBAColor,
        label: String
    ) {
        let scaleX = CGFloat(rep.pixelsWide) / max(1, rep.size.width)
        let scaleY = CGFloat(rep.pixelsHigh) / max(1, rep.size.height)
        let minX = max(0, Int(rect.minX * scaleX))
        let maxX = min(rep.pixelsWide - 1, Int(rect.maxX * scaleX))
        let minY = max(0, Int(rect.minY * scaleY))
        let maxY = min(rep.pixelsHigh - 1, Int(rect.maxY * scaleY))
        var contrasting = 0
        for y in stride(from: minY, through: maxY, by: 2) {
            for x in stride(from: minX, through: maxX, by: 2) {
                guard let c = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
                let dr = abs(Int((c.redComponent * 255.0).rounded()) - Int(background.r))
                let dg = abs(Int((c.greenComponent * 255.0).rounded()) - Int(background.g))
                let db = abs(Int((c.blueComponent * 255.0).rounded()) - Int(background.b))
                if dr + dg + db >= 36 {
                    contrasting += 1
                }
            }
        }
        XCTAssertGreaterThan(contrasting, 80, "Expected rendered glyphs/icons in \(label), got only \(contrasting) contrasting pixels")
    }
}
