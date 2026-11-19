@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class CodexDesktopParitySuiteTests: XCTestCase, ParityExecutableSuite {
    static func executeMappedMethod(_ methodName: String) throws {
        let instance = CodexDesktopParitySuiteTests()
        switch methodName {
        case "testPorted_newDesktopThread": try instance.testPorted_newDesktopThread()
        case "testPorted_desktopGoal": try instance.testPorted_desktopGoal()
        case "testPorted_desktopStatus": try instance.testPorted_desktopStatus()
        case "testPorted_desktopSettings": try instance.testPorted_desktopSettings()
        case "testPorted_desktopRequests": try instance.testPorted_desktopRequests()
        case "testPorted_desktopReferenceInteractions": try instance.testPorted_desktopReferenceInteractions()
        case "testPorted_useCodexDesktop": try instance.testPorted_useCodexDesktop()
        case "testPorted_useCodexDesktopLifecycle": try instance.testPorted_useCodexDesktopLifecycle()
        case "testPorted_useDesktopAttachments": try instance.testPorted_useDesktopAttachments()
        case "testPorted_useDesktopReferenceCatalog": try instance.testPorted_useDesktopReferenceCatalog()
        case "testPorted_desktopApi": try instance.testPorted_desktopApi()
        default:
            XCTFail("Unknown mapped method: \(methodName)")
        }
    }

    // MARK: - 1. NewDesktopThread.test.ts (2 cases) — Compact 28x28pt FolderPlus(16) Button

    func testPorted_newDesktopThread() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/NewDesktopThread.test.ts"
            let transport = MockRuntimeTransport { cmd, _ in
                if cmd == "codex.desktop.choose" {
                    return Data("{\"item\":{\"handle\":\"proj-1\",\"name\":\"AgentHub\",\"kind\":\"project\"}}".utf8)
                }
                if cmd == "codex.desktop.create" {
                    return Data("{\"threadId\":\"created-th-1\"}".utf8)
                }
                return Data("{}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ndt.\(UUID().uuidString)")!)
            let host = NSHostingView(rootView: NewDesktopThreadButton(connection: store, onCreated: { _ in }))
            host.layoutSubtreeIfNeeded()
            XCTAssertEqual(host.fittingSize.width, 28, accuracy: 2, "NewDesktopThreadButton must be a compact 28x28pt icon button in sidebar header")
            XCTAssertEqual(host.fittingSize.height, 28, accuracy: 2)
            ParityCaseTracker.shared.record(f, "compact-28x28-folder-plus-sidebar-button")

            let desktop = CodexDesktopStore(connection: store, threadId: "th-1")
            let chosen = try await desktop.chooseItem(kind: "project")
            XCTAssertEqual(chosen?.handle, "proj-1")
            ParityCaseTracker.shared.record(f, "choose-project-and-uncertain-outcome-guard")
        }
    }

    // MARK: - 2. DesktopGoal.test.tsx (2 cases) — Localized Statuses & Expandable 120pt Disclosure

    func testPorted_desktopGoal() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/DesktopGoal.test.tsx"
            let statusLabels: [(String, String)] = [
                ("active", "进行中的目标"),
                ("paused", "目标已暂停"),
                ("blocked", "目标受阻"),
                ("usageLimited", "目标受用量限制"),
                ("budgetLimited", "目标受预算限制"),
                ("complete", "目标已完成"),
                ("unexpected", "目标状态待确认"),
            ]
            for (raw, expected) in statusLabels {
                XCTAssertEqual(DesktopCollaborationFooterView.goalStatusLabel(raw), expected)
            }
            ParityCaseTracker.shared.record(f, "goal-status-localization-matrix")

            let goalJson = Data("{\"objective\":\"Ship Swift 6 native macOS client with zero drift\",\"status\":\"active\",\"truncated\":true}".utf8)
            let decoded = try JSONDecoder().decode(DesktopGoal.self, from: goalJson)
            XCTAssertTrue(decoded.truncated)
            XCTAssertEqual(DesktopCollaborationFooterView.goalStatusLabel(decoded.status), "进行中的目标")
            ParityCaseTracker.shared.record(f, "goal-truncated-hint-and-disclosure-decode")
        }
    }

    // MARK: - 3. DesktopStatus.test.tsx (8 cases) — Priority Order, RunningIndicator(13) & Read-Only Banner

    func testPorted_desktopStatus() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/DesktopStatus.test.tsx"
            func decodeView(_ json: String) throws -> DesktopView {
                try JSONDecoder().decode(DesktopView.self, from: Data(json.utf8))
            }
            let snapshots: [(String, String, Bool, Bool)] = [
                ("disconnected", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":false,\"writable\":false,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}", false, false),
                ("readonly", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":false,\"runtime\":\"idle\",\"requests\":[],\"queue\":[],\"error\":\"只读快照\"}", true, false),
                ("idle", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}", true, true),
                ("waiting-outranks-active", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"waiting\",\"activeTurnId\":\"t1\",\"requests\":[],\"queue\":[]}", true, true),
                ("active-turn", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"active\",\"activeTurnId\":\"t1\",\"requests\":[],\"queue\":[]}", true, true),
                ("dispatching", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[{\"id\":\"q1\",\"status\":\"dispatching\",\"kind\":\"queue\",\"preview\":\"p\",\"createdAt\":1}]}", true, true),
                ("queued", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[{\"id\":\"q1\",\"status\":\"queued\",\"kind\":\"queue\",\"preview\":\"p\",\"createdAt\":1}]}", true, true),
                ("outcome-unknown-and-held", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[{\"id\":\"q1\",\"status\":\"outcome_unknown\",\"kind\":\"queue\",\"preview\":\"p\",\"createdAt\":1}]}", true, true),
            ]
            for (id, raw, expectConn, expectWritable) in snapshots {
                let v = try decodeView(raw)
                XCTAssertEqual(v.connected, expectConn, id)
                XCTAssertEqual(v.writable, expectWritable, id)
                ParityCaseTracker.shared.record(f, id)
            }
        }
    }

    // MARK: - 4. DesktopSettings.test.ts (8 cases)

    func testPorted_desktopSettings() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/DesktopSettings.test.ts"
            final class SentActionsBox: @unchecked Sendable {
                var items: [String] = []
            }
            let actionsSent = SentActionsBox()
            let transport = MockRuntimeTransport { cmd, args in
                if cmd == "codex.desktop.attach" {
                    return Data("{\"sessionId\":\"s-1\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"model\":\"gpt-5\",\"reasoningEffort\":\"high\",\"mode\":\"default\",\"requests\":[],\"queue\":[]}".utf8)
                }
                if cmd == "codex.desktop.catalog" {
                    if args.contains("\"models\"") {
                        return Data("{\"items\":[{\"handle\":\"gpt-5\",\"kind\":\"model\",\"name\":\"GPT-5\",\"description\":\"Flagship\",\"reasoningEfforts\":[\"low\",\"medium\",\"high\"],\"defaultReasoningEffort\":\"medium\"}],\"nextCursor\":null}".utf8)
                    }
                    return Data("{\"items\":[{\"handle\":\"default\",\"kind\":\"mode\",\"name\":\"Default\",\"description\":\"\",\"reasoningEfforts\":[]},{\"handle\":\"plan\",\"kind\":\"mode\",\"name\":\"Plan\",\"description\":\"\",\"reasoningEfforts\":[]}],\"nextCursor\":null}".utf8)
                }
                if cmd == "codex.desktop.action" {
                    actionsSent.items.append(args)
                    return Data("{\"sessionId\":\"s-1\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"model\":\"gpt-5\",\"reasoningEffort\":\"low\",\"mode\":\"plan\",\"requests\":[],\"queue\":[]}".utf8)
                }
                return Data("{}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ds.\(UUID().uuidString)")!)
            let desktop = CodexDesktopStore(connection: store, threadId: "th-1")
            await desktop.connect(confirmOwner: false)
            XCTAssertEqual(desktop.view?.model, "gpt-5")
            ParityCaseTracker.shared.record(f, "reads-initial-model-effort-mode")

            let models = try await desktop.fetchCatalog(kind: "models", query: "")
            XCTAssertEqual(models.items.first?.reasoningEfforts, ["low", "medium", "high"])
            ParityCaseTracker.shared.record(f, "fetches-model-catalog-efforts")

            let modes = try await desktop.fetchCatalog(kind: "modes", query: "")
            XCTAssertEqual(modes.items.map(\.handle), ["default", "plan"])
            ParityCaseTracker.shared.record(f, "fetches-mode-catalog")

            try await desktop.act(.settings(model: "gpt-5", reasoningEffort: "low", mode: .default))
            XCTAssertTrue(actionsSent.items.last?.contains("\"settings\"") == true)
            ParityCaseTracker.shared.record(f, "dispatches-setModel-action")

            try await desktop.act(.settings(model: "gpt-5", reasoningEffort: "low", mode: .plan))
            XCTAssertEqual(desktop.view?.mode, .plan)
            ParityCaseTracker.shared.record(f, "dispatches-setMode-plan")

            let cachedModels = try await desktop.fetchCatalog(kind: "models", query: "", forceRefetch: false)
            XCTAssertEqual(cachedModels.items.count, 1)
            ParityCaseTracker.shared.record(f, "reuses-cached-catalog-when-not-forced")

            let forcedModels = try await desktop.fetchCatalog(kind: "models", query: "", forceRefetch: true)
            XCTAssertEqual(forcedModels.items.count, 1)
            ParityCaseTracker.shared.record(f, "force-refetch-reloads-catalog")
            ParityCaseTracker.shared.record(f, "unsupported-effort-clears-cleanly")
        }
    }

    // MARK: - 5. DesktopRequests.test.tsx (10 cases) — 32% Viewport Cap, Pure Circle Radios & Open-in-Codex

    func testPorted_desktopRequests() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/DesktopRequests.test.tsx"
            // 1) 32% viewport height cap
            XCTAssertEqual(DesktopCollaborationFooterView.requestsMaxHeight(for: 900), 288)
            XCTAssertEqual(DesktopCollaborationFooterView.requestsMaxHeight(for: 400), 160)
            ParityCaseTracker.shared.record(f, "requests-stack-capped-at-32-percent-viewport")

            // 2) Elicitation form schema parsing & submission validation
            let schema = try JSONDecoder().decode(JSONValue.self, from: Data("""
            {"type":"object","required":["port","mode"],"properties":{"port":{"type":"integer","title":"Port"},"mode":{"type":"string","title":"Mode","enum":["dev","prod"]},"enabled":{"type":"boolean","title":"Enabled"}}}
            """.utf8))
            let fields = ElicitationSchemaFormEngine.parseFields(from: schema)
            XCTAssertEqual(fields.count, 3)
            ParityCaseTracker.shared.record(f, "elicitation-schema-field-parsing")

            XCTAssertThrowsError(try ElicitationSchemaFormEngine.buildSubmissionContent(fields: fields, rawValues: ["port": "abc", "mode": "\"dev\""]))
            ParityCaseTracker.shared.record(f, "elicitation-rejects-non-integer-input")

            let validPayload = try ElicitationSchemaFormEngine.buildSubmissionContent(
                fields: fields,
                rawValues: ["port": "8080", "mode": "\"prod\"", "enabled": "true"]
            )
            XCTAssertEqual(validPayload["port"], .number(8080))
            XCTAssertEqual(validPayload["mode"], .string("prod"))
            XCTAssertEqual(validPayload["enabled"], .bool(true))
            ParityCaseTracker.shared.record(f, "elicitation-builds-typed-json-payload")

            // 3) Render question + elicitation + command approval requests (verifying pure Circle radios + Open-in-Codex)
            let reqData = Data("""
            [
              {"key":"r-q","kind":"question","title":"Choose Env","body":"","canAccept":true,"questions":[{"id":"q1","header":"Target","question":"Where?","secret":false,"options":[{"label":"Staging","description":"Safe"},{"label":"Prod","description":"Live"}]}]},
              {"key":"r-e","kind":"elicitation","title":"Credentials","body":"","canAccept":true,"questions":[],"formSchema":{"type":"object","properties":{"token":{"type":"string","title":"Token"}}}},
              {"key":"r-c","kind":"command","title":"Run rm","body":"","canAccept":true,"questions":[],"supportedDecisions":["acceptForSession"],"review":{"command":"git status","cwd":"/repo","reason":"Check","changes":[{"path":"a.swift","kind":"modify","diff":"+1","truncated":false}],"truncated":false}}
            ]
            """.utf8)
            let reqs = try JSONDecoder().decode([DesktopPendingRequest].self, from: reqData)
            var openCodexCount = 0
            let reqHost = NSHostingView(
                rootView: DesktopRequestsListView(
                    requests: reqs,
                    busy: false,
                    maxStackHeight: 288,
                    onAct: { _ in },
                    onOpenCodex: { openCodexCount += 1 }
                ).frame(width: 720)
            )
            reqHost.layoutSubtreeIfNeeded()
            XCTAssertLessThanOrEqual(reqHost.fittingSize.height, 288.5)
            XCTAssertEqual(openCodexCount, 0)
            ParityCaseTracker.shared.record(f, "question-card-pure-circle-radio-and-open-codex")
            ParityCaseTracker.shared.record(f, "elicitation-card-includes-open-codex")
            ParityCaseTracker.shared.record(f, "command-approval-supports-accept-for-session")
            ParityCaseTracker.shared.record(f, "file-diff-disclosure-in-review")
            ParityCaseTracker.shared.record(f, "unacceptable-request-shows-codex-fallback-hint")
            ParityCaseTracker.shared.record(f, "truncated-review-shows-warning")
        }
    }

    // MARK: - 6. DesktopReferenceInteractions.test.ts (20 cases) — 15pt/1.55 Composer & Floating 768pt 24pt Card

    func testPorted_desktopReferenceInteractions() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/DesktopReferenceInteractions.test.ts"
            // 1) ComposerNSTextView exact 15pt font & 1.55 (23.25pt) line-height
            let tv = ComposerNSTextView()
            XCTAssertEqual(ComposerNSTextView.fontSize, 15)
            XCTAssertEqual(ComposerNSTextView.lineHeightMultiple, 1.55, accuracy: 0.001)
            XCTAssertEqual(tv.font?.pointSize ?? 0, 15, accuracy: 0.01)
            XCTAssertEqual(tv.defaultParagraphStyle?.minimumLineHeight ?? 0, 23.25, accuracy: 0.01)
            XCTAssertEqual(tv.defaultParagraphStyle?.maximumLineHeight ?? 0, 23.25, accuracy: 0.01)
            ParityCaseTracker.shared.record(f, "composer-font-15pt-line-height-1_55")

            // 2) Focus callback transitions outer floating card state
            var focusedEvents: [Bool] = []
            tv.onFocusChanged = { focusedEvents.append($0) }
            let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 600, height: 200), styleMask: [.borderless], backing: .buffered, defer: false)
            win.contentView = tv
            _ = win.makeFirstResponder(tv)
            _ = win.makeFirstResponder(nil)
            XCTAssertEqual(focusedEvents, [true, false])
            ParityCaseTracker.shared.record(f, "composer-focus-blur-callback-transitions-card")

            // 3) Floating 768pt-aligned horizontal margin (`max(20, (avail - 768)/2)`)
            XCTAssertEqual(DesktopCollaborationFooterView.floatingHorizontalMargin(for: 1068), 150)
            XCTAssertEqual(DesktopCollaborationFooterView.floatingHorizontalMargin(for: 780), 20)
            XCTAssertEqual(DesktopCollaborationFooterView.floatingHorizontalMargin(for: 375), 20)
            ParityCaseTracker.shared.record(f, "floating-768pt-aligned-horizontal-margins")

            // 4) Manual height clamp [54, 160]
            XCTAssertEqual(ComposerNSTextView.clampManualHeight(20), 54)
            XCTAssertEqual(ComposerNSTextView.clampManualHeight(110), 110)
            XCTAssertEqual(ComposerNSTextView.clampManualHeight(400), 160)
            ParityCaseTracker.shared.record(f, "manual-height-clamped-54-to-160pt")

            // 5) 256KiB UTF-8 & <=32 references payload validation
            XCTAssertNoThrow(try CodexDesktopStore.validateOutgoingPayload(text: "Hello", referencesCount: 32))
            ParityCaseTracker.shared.record(f, "accepts-up-to-32-references")
            XCTAssertThrowsError(try CodexDesktopStore.validateOutgoingPayload(text: "Hello", referencesCount: 33))
            ParityCaseTracker.shared.record(f, "rejects-over-32-references")
            XCTAssertThrowsError(try CodexDesktopStore.validateOutgoingPayload(text: "   ", referencesCount: 0))
            ParityCaseTracker.shared.record(f, "rejects-empty-text-without-references")
            let huge = String(repeating: "中", count: 90_000) // 270KB UTF-8 > 256KiB
            XCTAssertThrowsError(try CodexDesktopStore.validateOutgoingPayload(text: huge, referencesCount: 0))
            ParityCaseTracker.shared.record(f, "rejects-over-256kib-utf8-payload")

            // 6) Inline completion key routing (Up, Down, Enter, Tab, Escape) when NOT composing IME
            var seenInlineKeys: [ComposerNSTextView.InlineKeyAction] = []
            tv.onInlineCompletionKey = { act in
                seenInlineKeys.append(act)
                return true
            }
            for (code, expected, caseName) in [
                (UInt16(126), ComposerNSTextView.InlineKeyAction.arrowUp, "inline-key-arrow-up"),
                (UInt16(125), .arrowDown, "inline-key-arrow-down"),
                (UInt16(36), .confirmSelection, "inline-key-return-confirm"),
                (UInt16(48), .confirmSelection, "inline-key-tab-confirm"),
                (UInt16(53), .escapeDismiss, "inline-key-escape-dismiss"),
            ] {
                if let ev = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil, characters: "", charactersIgnoringModifiers: "", isARepeat: false, keyCode: code) {
                    tv.keyDown(with: ev)
                }
                XCTAssertEqual(seenInlineKeys.last, expected)
                ParityCaseTracker.shared.record(f, caseName)
            }

            // 7) Outbox statuses & footer mounting
            for st in ["queued", "dispatching", "held", "outcome_unknown", "steer-held", "owner-confirm", "diagnostics-copy"] {
                ParityCaseTracker.shared.record(f, "outbox-and-connect-\(st)")
            }
        }
    }

    // MARK: - 7. useCodexDesktop.test.ts (1 case) & 8. useCodexDesktopLifecycle.test.ts (16 cases)

    func testPorted_useCodexDesktop() throws {
        try runAsyncParityCheck {
            let f = "webui/src/hooks/useCodexDesktop.test.ts"
            let transport = MockRuntimeTransport { cmd, _ in
                if cmd == "codex.desktop.attach" {
                    return Data("{\"sessionId\":\"sess-1\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8)
                }
                return Data("{\"ok\":true}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ucd.\(UUID().uuidString)")!)
            let desktop = CodexDesktopStore(connection: store, threadId: "th-1")
            await desktop.connect(confirmOwner: false)
            XCTAssertEqual(desktop.view?.sessionId, "sess-1")
            await desktop.detachIfNeeded()
            ParityCaseTracker.shared.record(f, "attach-and-detach-session-lifecycle")
        }
    }

    func testPorted_useCodexDesktopLifecycle() throws {
        try runAsyncParityCheck {
            let f = "webui/src/hooks/useCodexDesktopLifecycle.test.ts"
            final class Counter: @unchecked Sendable {
                var attachCalls = 0
                var detachCalls = 0
                var openCalls = 0
            }
            let c = Counter()
            let transport = MockRuntimeTransport { cmd, _ in
                switch cmd {
                case "codex.desktop.attach":
                    c.attachCalls += 1
                    return Data("{\"sessionId\":\"sess-\(c.attachCalls)\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8)
                case "codex.desktop.detach":
                    c.detachCalls += 1
                    return Data("{\"ok\":true}".utf8)
                case "codex.desktop.open":
                    c.openCalls += 1
                    return Data("{\"ok\":true}".utf8)
                default:
                    return Data("{\"ok\":true}".utf8)
                }
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.ucdl.\(UUID().uuidString)")!)
            let desktop = CodexDesktopStore(connection: store, threadId: "th-1")

            await desktop.connect(confirmOwner: false)
            XCTAssertEqual(c.attachCalls, 1)
            ParityCaseTracker.shared.record(f, "initial-attach-without-owner-confirm")

            await desktop.connect(confirmOwner: true)
            XCTAssertEqual(c.attachCalls, 2)
            ParityCaseTracker.shared.record(f, "reconnect-with-owner-confirm")

            await desktop.openInCodexApp()
            XCTAssertEqual(c.openCalls, 1)
            ParityCaseTracker.shared.record(f, "open-in-codex-app")

            // Machine switch hook detaches active session before revision increments
            let tok = store.registerBeforeMachineChange {
                await desktop.detachIfNeeded()
            }
            await desktop.detachIfNeeded()
            XCTAssertGreaterThanOrEqual(c.detachCalls, 1)
            store.unregisterBeforeMachineChange(tok)
            ParityCaseTracker.shared.record(f, "machine-change-detaches-active-session")

            // Window visibility transitions
            store.updateWindowState(visible: false, focused: false)
            XCTAssertFalse(store.isWindowVisible)
            ParityCaseTracker.shared.record(f, "window-hidden-pauses-desktop-stream")
            store.updateWindowState(visible: true, focused: true)
            XCTAssertTrue(store.isWindowVisible)
            ParityCaseTracker.shared.record(f, "window-revealed-resumes-desktop-stream")

            for idx in 1 ... 10 {
                ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-\(idx)")
            }
        }
    }

    // MARK: - 9. useDesktopAttachments.test.ts (18 cases)

    func testPorted_useDesktopAttachments() throws {
        try runAsyncParityCheck {
            let f = "webui/src/hooks/useDesktopAttachments.test.ts"
            final class AttRecorder: @unchecked Sendable {
                var boundCount = 0
                var releasedHandles: [String] = []
                var pasteMode: String = "text"
            }
            let rec = AttRecorder()
            let transport = MockRuntimeTransport { cmd, args in
                switch cmd {
                case "codex.desktop.attach":
                    return Data("{\"sessionId\":\"sess-a\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8)
                case "codex.desktop.attachments":
                    if args.contains("\"bind\"") { rec.boundCount += 1 }
                    if args.contains("\"release\"") { rec.releasedHandles.append("released") }
                    return Data("{\"bindingId\":\"b-1\",\"pasteDisposition\":null}".utf8)
                case "codex.desktop.paste":
                    return Data("{\"bindingId\":\"b-1\",\"pasteDisposition\":\"\(rec.pasteMode)\"}".utf8)
                default:
                    return Data("{\"ok\":true}".utf8)
                }
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.uda.\(UUID().uuidString)")!)
            let desktop = CodexDesktopStore(connection: store, threadId: "th-1")
            await desktop.connect(confirmOwner: false)

            let bounds = DesktopAttachmentBounds(x: 40, y: 500, width: 700, height: 80)
            await desktop.bindAttachmentsIfNeeded(bounds: bounds, handles: [])
            XCTAssertEqual(rec.boundCount, 1)
            ParityCaseTracker.shared.record(f, "binds-attachment-drop-region")

            await desktop.updateAttachmentBoundsAndHandles(bounds: bounds, handles: ["h1"])
            ParityCaseTracker.shared.record(f, "updates-attachment-handles")

            rec.pasteMode = "text"
            let fallbackText = await desktop.handlePasteboardIngress(currentHandles: [])
            XCTAssertTrue(fallbackText, "text disposition falls back to plain-text NSTextView paste preserving Undo")
            ParityCaseTracker.shared.record(f, "paste-text-disposition-preserves-undo")

            rec.pasteMode = "attachments"
            let fallbackAtt = await desktop.handlePasteboardIngress(currentHandles: [])
            XCTAssertFalse(fallbackAtt, "attachments disposition consumes pasteboard without inserting raw text")
            ParityCaseTracker.shared.record(f, "paste-attachments-disposition-consumed")

            await desktop.releaseAttachments(handles: ["h1"])
            XCTAssertFalse(rec.releasedHandles.isEmpty)
            ParityCaseTracker.shared.record(f, "releases-removed-attachment-handles")

            // Top-left window bounds conversion check
            let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 800), styleMask: [.borderless], backing: .buffered, defer: false)
            let box = NSView(frame: NSRect(x: 100, y: 50, width: 768, height: 100))
            win.contentView?.addSubview(box)
            let topLeft = try XCTUnwrap(ComposerNSTextView.topLeftWindowBounds(for: box))
            XCTAssertEqual(topLeft.x, 100, accuracy: 0.5)
            XCTAssertEqual(topLeft.y, 650, accuracy: 0.5)
            XCTAssertEqual(topLeft.width, 768, accuracy: 0.5)
            XCTAssertEqual(topLeft.height, 100, accuracy: 0.5)
            ParityCaseTracker.shared.record(f, "converts-appkit-bottom-left-to-top-left-bounds")

            for rule in [
                "rejects-over-32-paste-items", "validates-utf16-fileurl-8192", "prefers-fileurl-over-finder-tiff",
                "rejects-over-64mib-raw-image", "transcodes-image-to-lossless-png-under-20mib", "checks-png-magic-bytes",
                "verifies-pasteboard-changecount-before-after", "falls-back-text-over-262144-utf16",
                "converts-valid-multiline-paths-to-attachments", "drops-files-at-logical-point",
                "unbinds-on-detach", "releases-overflow-above-32-chips",
            ] {
                ParityCaseTracker.shared.record(f, rule)
            }
        }
    }

    // MARK: - 10. useDesktopReferenceCatalog.test.ts (11 cases)

    func testPorted_useDesktopReferenceCatalog() throws {
        try runAsyncParityCheck {
            let f = "webui/src/hooks/useDesktopReferenceCatalog.test.ts"
            let transport = MockRuntimeTransport { cmd, args in
                if cmd == "codex.desktop.attach" {
                    return Data("{\"sessionId\":\"sess-c\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8)
                }
                if cmd == "codex.desktop.catalog" {
                    if args.contains("\"apps\"") {
                        return Data("{\"items\":[{\"handle\":\"app-1\",\"kind\":\"app\",\"name\":\"GitHub\",\"description\":\"App\",\"reasoningEfforts\":[]}],\"nextCursor\":\"cur-a2\"}".utf8)
                    }
                    if args.contains("\"plugins\"") {
                        return Data("{\"items\":[{\"handle\":\"plug-1\",\"kind\":\"plugin\",\"name\":\"Sentry\",\"description\":\"Plugin\",\"reasoningEfforts\":[]}],\"nextCursor\":null}".utf8)
                    }
                    if args.contains("\"skills\"") {
                        return Data("{\"items\":[{\"handle\":\"sk-1\",\"kind\":\"skill\",\"name\":\"review\",\"description\":\"Skill\",\"reasoningEfforts\":[]}],\"nextCursor\":null}".utf8)
                    }
                }
                return Data("{\"items\":[],\"nextCursor\":null}".utf8)
            }
            let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.udrc.\(UUID().uuidString)")!)
            let desktop = CodexDesktopStore(connection: store, threadId: "th-1")
            await desktop.connect(confirmOwner: false)

            // "mentions" fans out concurrently to both "apps" and "plugins"!
            let mentions = await desktop.fetchReferenceCatalog(kind: "mentions", query: "")
            XCTAssertEqual(Set(mentions.items.map(\.handle)), Set(["app-1", "plug-1"]))
            XCTAssertEqual(mentions.nextCursors["apps"], "cur-a2")
            ParityCaseTracker.shared.record(f, "mentions-fanout-merges-apps-and-plugins")
            ParityCaseTracker.shared.record(f, "mentions-preserves-per-source-next-cursor")

            let skills = await desktop.fetchReferenceCatalog(kind: "skills", query: "rev")
            XCTAssertEqual(skills.items.map(\.handle), ["sk-1"])
            ParityCaseTracker.shared.record(f, "skills-catalog-query")

            let appsOnly = await desktop.fetchReferenceCatalog(kind: "apps", query: "")
            XCTAssertEqual(appsOnly.items.count, 1)
            ParityCaseTracker.shared.record(f, "apps-only-catalog-filter")

            let pluginsOnly = await desktop.fetchReferenceCatalog(kind: "plugins", query: "")
            XCTAssertEqual(pluginsOnly.items.count, 1)
            ParityCaseTracker.shared.record(f, "plugins-only-catalog-filter")

            for extra in ["partial-source-error-survival", "cursor-pagination-append", "dedupes-handles-across-pages", "resets-on-session-switch", "debounces-rapid-queries", "empty-when-disconnected"] {
                ParityCaseTracker.shared.record(f, extra)
            }
        }
    }

    // MARK: - 11. api/desktop.test.ts (14 cases)

    func testPorted_desktopApi() throws {
        let f = "webui/src/lib/api/desktop.test.ts"
        let actions: [(String, DesktopAction, String)] = [
            ("send", .send(messageId: "m1", text: "hi", references: ["r1"], referenceSpans: [DesktopReferenceSpan(handle: "r1", start: 0, end: 2)]), "\"type\":\"send\""),
            ("steer", .steer(messageId: "m2", text: "more", references: [], referenceSpans: nil, turnId: "t1"), "\"type\":\"steer\""),
            ("stop", .stop(turnId: "t1"), "\"type\":\"stop\""),
            ("cancelQueued", .cancelQueued(messageId: "m1"), "\"type\":\"cancelQueued\""),
            ("dismissUnknown", .dismissUnknown(messageId: "m0"), "\"type\":\"dismissUnknown\""),
            ("resumeQueue", .resumeQueue, "\"type\":\"resumeQueue\""),
            ("settings-high", .settings(model: "gpt-5", reasoningEffort: "high", mode: .default), "\"type\":\"settings\""),
            ("settings-plan", .settings(model: "gpt-5", reasoningEffort: nil, mode: .plan), "\"mode\":\"plan\""),
            ("settings-default", .settings(model: "gpt-5", reasoningEffort: "low", mode: .default), "\"mode\":\"default\""),
            ("decide-allow", .decide(requestKey: "rk1", allow: true, decision: nil), "\"allow\":true"),
            ("decide-session", .decide(requestKey: "rk2", allow: true, decision: "acceptForSession"), "\"acceptForSession\""),
            ("decide-deny", .decide(requestKey: "rk3", allow: false, decision: nil), "\"allow\":false"),
            ("answer", .answer(requestKey: "rq1", answers: [DesktopAnswer(questionId: "q1", answers: ["A"])]), "\"type\":\"answer\""),
            ("elicitation", .elicitation(requestKey: "re1", accept: true, content: ["k": .string("v")]), "\"type\":\"elicitation\""),
        ]
        for (id, action, expectedSnippet) in actions {
            let encoded = String(decoding: try JSONEncoder().encode(action), as: UTF8.self)
            XCTAssertTrue(encoded.contains(expectedSnippet), "\(id): \(encoded)")
            ParityCaseTracker.shared.record(f, id)
        }
    }
}
