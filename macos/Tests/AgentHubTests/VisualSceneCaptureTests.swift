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

        let writableBox = WritableModeBox()
        let transport = MockRuntimeTransport { cmd, args in
            Self.mockResponse(cmd: cmd, args: args, writable: writableBox.writable)
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
        try captureWindow(window, host: host, to: "\(outDir)/swift-01-codex-light-top.png")

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

        // Scene 5: Light Codex Writable Composer (Goal + Outbox + Request + Toolbar)
        writableBox.writable = true
        store.setTheme(.light)
        window.appearance = NSAppearance(named: .aqua)
        DesktopOpenIntentRegistry.request(threadId: "th-visual-1")
        store.activeWorkspace = .claude
        try await pumpMainRunLoop(ms: 150)
        store.activeWorkspace = .codex
        try await pumpMainRunLoop(ms: 450)
        scrollConversationTable(in: host, fraction: 1.0)
        try await pumpMainRunLoop(ms: 200)
        try captureWindow(window, host: host, to: "\(outDir)/swift-05-codex-writable-composer.png")
        writableBox.writable = false

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

        // Scene 7: Probe Workspace
        store.activeWorkspace = .probe
        try await pumpMainRunLoop(ms: 450)
        try captureWindow(window, host: host, to: "\(outDir)/swift-07-probe-light.png")

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
    }

    private final class WritableModeBox: @unchecked Sendable {
        var writable = false
    }

    @MainActor
    private func pumpMainRunLoop(ms: UInt64) async throws {
        try await Task.sleep(nanoseconds: ms * 1_000_000)
        await Task.yield()
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
    private func captureWindow(_ window: NSWindow, host: NSView, to path: String) throws {
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
            XCTFail("Failed to allocate bitmapImageRep")
            return
        }
        host.cacheDisplay(in: host.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else {
            XCTFail("Failed to encode PNG")
            return
        }
        let url = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try png.write(to: url)
    }

    private static func mockResponse(cmd: String, args: String, writable: Bool) -> Data {
        let mib = 1024 * 1024
        switch cmd {
        case "remote.get":
            return Data(#"{"target":"local","revision":0,"sshHost":null,"configured":false,"machines":[]}"#.utf8)
        case "system.capabilities":
            return Data(#"{"protocol_version":2,"service_mode":"macos_desktop","capabilities":{"codex_sessions":true,"codex_desktop":true,"thread_subagents":true,"claude_sessions":true,"claude_subagents":true,"grok_sessions":true,"grokbot_sessions":true,"antigravity_sessions":true,"dsh_sessions":true,"session_search":true,"github_ci":true,"codex_account_scope":true}}"#.utf8)
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
                  {"agentId":"sub-3","name":"快照比对器","status":"failed","available":true}
                ]
              },
              "blocks":[
                {"id":"chat-history-collapsed","role":"tool","kind":"chat_history_collapsed","status":"completed","text":"12 条历史对话已折叠","summary":"12 条历史对话已折叠","tool_name":"chat_history","truncated":false,"questions":[]},
                {"id":"u-prompt-1","role":"user","kind":"userMessage","text":"<environment_context>\n  <cwd>/workspace/AgentHub</cwd>\n  <shell>zsh</shell>\n</environment_context>\n请核对线程详情的全部视觉层级：过程与工具折叠行、代码块、行内代码、表格、引用、计划卡片与问答。","user_message":{"id":"u-prompt-1","text":"请核对线程详情的全部视觉层级：过程与工具折叠行、代码块、行内代码、表格、引用、计划卡片与问答。","contextBlocks":[{"kind":"environment_context","text":"<environment_context>\n  <cwd>/workspace/AgentHub</cwd>\n  <shell>zsh</shell>\n</environment_context>"}],"attachments":[{"id":"att-1","name":"DESIGN.md","kind":"file","path":"/workspace/AgentHub/DESIGN.md"}]},"questions":[]},
                {"id":"call-1","role":"tool","kind":"function_call_output","tool_name":"read_file","call_id":"c1","turn_id":"turn-1","status":"completed","input":"{\"path\":\"/workspace/AgentHub/DESIGN.md\"}","text":"Read 180 lines from DESIGN.md","questions":[]},
                {"id":"call-2","role":"tool","kind":"function_call_output","tool_name":"exec_command","call_id":"c2","turn_id":"turn-1","status":"completed","input":"{\"cmd\":\"rg -n 'execution-group|markdown-content' webui/src/styles.css\"}","text":"451:.markdown-content {\n479:.execution-group > summary {","questions":[]},
                {"id":"sub-row-1","role":"tool","kind":"subagent","status":"completed","questions":[],"subagent":{"agentId":"sub-1","name":"视觉对齐审查子智能体","status":"completed","eventKind":"completed","available":true}},
                {"id":"plan-1","role":"assistant","kind":"plan","display_kind":"plan","turn_id":"turn-1","item_id":"plan-1","status":"completed","plan_status":"completed","resolved":true,"text":"<proposed_plan>1. 对齐 `.execution-group` 无边框行与左侧 `-20px` 图标\n2. 修复行内 `code` 圆角底色、代码块与表格列宽\n3. 实机双视图 PNG 逐帧核验</proposed_plan>","questions":[]},
                {"id":"q-res-1","role":"assistant","kind":"request_user_input_result","display_kind":"question_result","turn_id":"turn-1","status":"completed","resolved":true,"questions":[{"id":"mode","question":"选择验收对比基线","options":[{"label":"真实 WebUI 渲染截图","description":"使用 WKWebView 同源渲染逐像素比对"},{"label":"仅跑单元测试","description":"无法发现排版与样式偏差"}]}],"answers":[{"question_id":"mode","answers":["真实 WebUI 渲染截图"]}]},
                {"id":"u-reply-1","role":"user","kind":"userMessage","text":"<send_user_message_question_reply>[{\"questionItemId\":\"q-mode\",\"question\":\"请确认是否同时检查浅色与深色模式下的全部折叠展开态？\",\"answer\":\"确认，浅色、深色、折叠态与展开态全部并排比对。\"}]</send_user_message_question_reply>","questions":[]},
                {"id":"a-rich-1","role":"assistant","kind":"agentMessage","text":"# AGENTS.md instructions for /workspace/AgentHub\n\n<INSTRUCTIONS>\nFollow DESIGN.md and styles.css with zero visual drift.\n</INSTRUCTIONS>\n\n## 线程详情视觉核查结论\n\n已完成对 `Conversation.tsx`、[DESIGN.md](/workspace/AgentHub/DESIGN.md) 与 [在线规范](https://example.invalid/spec) 的逐项比对，重点修复 **折叠行边框冗余** 与 `inline code` 背景高度：\n\n- 工具与思考折叠行默认透明无外框，左侧 `-20px` 悬挂强调色图标；\n- 展开态左侧绘制 `2px solid var(--bs)` 纵向引导线；\n- 助教回复右侧悬浮 `30×30px` 复制按钮，零行高占用。\n\n> 最高品质原则：所有配色、字级、行高、间距与折叠交互，必须与现有桌面版 100% 一致。\n\n```rust\npub fn assert_zero_visual_drift(diff_ratio: f64) -> bool {\n    diff_ratio <= 0.005\n}\n```\n\n| 检查模块 | 字号 / 行高 | 状态 |\n| :--- | :---: | :--- |\n| 助教正文 | `15px / 1.72` | 已对齐 |\n| 代码块 | `13px / 1.60` | 无截断 |","questions":[]}
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
        case "codex.desktop.attach", "codex.desktop.connect":
            if writable {
                return Data(#"""
                {
                  "sessionId":"sess-rw",
                  "threadId":"th-visual-1",
                  "revision":2,
                  "connected":true,
                  "writable":true,
                  "runtime":"waiting",
                  "activeTurnId":"turn-live-1",
                  "model":"gpt-5",
                  "reasoningEffort":"high",
                  "mode":"plan",
                  "goal":{"status":"active","objective":"保持桌面原生 Swift 6 与 WebUI 零视觉漂移并完成逐帧截图核验。","truncated":false},
                  "queue":[{"id":"q-msg-1","kind":"queue","status":"queued","preview":"补充检查深色模式代码块与表格边框"}],
                  "requests":[{"key":"req-cmd-1","kind":"command","title":"需要确认执行命令","body":"","canAccept":true,"supportedDecisions":["acceptForSession"],"review":{"command":"swift test --package-path macos","cwd":"/workspace/AgentHub","reason":"运行原生排版与快照门禁","changes":[]},"questions":[]}]
                }
                """#.utf8)
            }
            return Data(#"{"sessionId":"sess-ro","threadId":"th-visual-1","revision":0,"connected":false,"writable":false,"runtime":"idle","requests":[],"queue":[]}"#.utf8)
        case "probe.status":
            return Data(#"{"available":true,"data":{"label":"Probe","enabled":true,"available":true,"platform":"macos","service_kind":"launchd","service_name":"com.lich13.agenthub.probe","flavor":"builtin","hook_status":"managed","bark_status":"configured","recent_event_count":2,"running_count":1,"reply_needed_count":1,"recoverable_count":0,"running_threads":[],"reply_needed_threads":[],"recoverable_threads":[],"lifecycle_status":"ok","doctor_status":"ok","runtime_version":"2.1.27","config_path":"~/Library/Application Support/AgentHub/config.toml","codex_home":"~/.codex","configured_codex_home":"~/.codex","resolved_codex_home":"~/.codex","codex_home_source":"default","logs_db_source":"resolved_codex_home","host_label":"local","snapshot_age_seconds":1,"is_refreshing":false,"snapshot_status":"fresh"}}"#.utf8)
        case "probe.settings.get":
            return Data(#"{"available":true,"data":{"codex":{"home":"~/.codex","configured_codex_home":"~/.codex","resolved_codex_home":"~/.codex","codex_home_source":"default","logs_db_source":"resolved_codex_home","discovery_warnings":[],"workspace":"~/Documents","host_label":"local"},"probe":{"enabled":true,"poll_seconds":15,"recent_limit":50,"clear_account_scope":false,"providers":{"codex":true,"claude":true,"grok":true,"antigravity":true,"dsh":true,"grokbot":false}},"notifications":{"enabled":true,"device_key_configured":true,"server_url":"https://api.day.app","group":"AgentHub"}}}"#.utf8)
        case "probe.events":
            return Data(#"{"available":true,"data":{"limit":10,"events":[]}}"#.utf8)
        case "github.ci.status":
            return Data(#"{"account":{"id":1,"login":"octocat"},"enabled":true,"refreshing":false,"error":null,"last_scan_ms":1732017600000,"selected_repositories":["octocat/AgentHub"],"repositories":[{"id":1,"full_name":"octocat/AgentHub","archived":false},{"id":2,"full_name":"octocat/swift-kit","archived":false}],"runs":[{"id":101,"run_attempt":1,"name":"Build and validate macOS Swift 6 native bundle","head_branch":"feat/macos-swift-native","head_sha":"c008e00a","status":"completed","conclusion":"success","html_url":"https://example.invalid/runs/101","created_at":"2026-11-19T11:50:00Z","updated_at":"2026-11-19T11:58:00Z","repository_id":1,"repository_name":"octocat/AgentHub","is_bot":false}]}"#.utf8)
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
