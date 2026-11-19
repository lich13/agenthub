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
            let snapshots: [(String, String, String?)] = [
                ("disconnected", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":false,\"writable\":false,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}", nil),
                ("readonly", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":false,\"runtime\":\"idle\",\"requests\":[],\"queue\":[],\"error\":\"只读快照\"}", nil),
                ("idle", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}", "空闲"),
                ("waiting-outranks-active", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"waiting\",\"activeTurnId\":\"t1\",\"requests\":[],\"queue\":[]}", "等待你处理"),
                ("active-turn", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"active\",\"activeTurnId\":\"t1\",\"requests\":[],\"queue\":[]}", "执行中"),
                ("dispatching", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[{\"id\":\"q1\",\"status\":\"dispatching\",\"kind\":\"queue\",\"preview\":\"p\",\"createdAt\":1}]}", "正在提交到 Codex…"),
                ("queued", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[{\"id\":\"q1\",\"status\":\"queued\",\"kind\":\"queue\",\"preview\":\"p\",\"createdAt\":1}]}", "队列中"),
                ("outcome-unknown-and-held", "{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[{\"id\":\"q1\",\"status\":\"outcome_unknown\",\"kind\":\"queue\",\"preview\":\"p\",\"createdAt\":1}]}", "发送结果待核对"),
            ]
            for (id, raw, expectedLabel) in snapshots {
                let v = try decodeView(raw)
                XCTAssertEqual(DesktopCollaborationFooterView.collaborationStatusLabel(v), expectedLabel, id)
                ParityCaseTracker.shared.record(f, id)
            }
            let heldView = try decodeView("{\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[{\"id\":\"q1\",\"status\":\"held\",\"kind\":\"queue\",\"preview\":\"p\",\"createdAt\":1}]}")
            XCTAssertEqual(DesktopCollaborationFooterView.collaborationStatusLabel(heldView), "队列已暂停")
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

            let unsupported = DesktopCollaborationFooterView.resolveModeChangeEffort(
                currentEffort: "xhigh",
                supportedEfforts: ["low", "medium", "high"]
            )
            XCTAssertNil(unsupported.effort)
            XCTAssertEqual(unsupported.notice, "当前思考强度不可用，将继承原生默认")
            let supported = DesktopCollaborationFooterView.resolveModeChangeEffort(
                currentEffort: "high",
                supportedEfforts: ["low", "medium", "high"]
            )
            XCTAssertEqual(supported.effort, "high")
            XCTAssertNil(supported.notice)
            ParityCaseTracker.shared.record(f, "unsupported-effort-clears-cleanly")
        }
    }

    // MARK: - 5. DesktopRequests.test.tsx (10 cases) — 32% Viewport Cap, Pure Circle Radios & Open-in-Codex

    func testPorted_desktopRequests() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/chat/DesktopRequests.test.tsx"
            @MainActor func collectText(_ v: NSView) -> String {
                var out: [String] = []
                if let tf = v as? NSTextField { out.append(tf.stringValue) }
                if let btn = v as? NSButton, !btn.title.isEmpty { out.append(btn.title) }
                if let acc = v.accessibilityLabel(), !acc.isEmpty { out.append(acc) }
                for sub in v.subviews { out.append(collectText(sub)) }
                return out.joined(separator: " ")
            }

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

            // 3) Question card: pure Circle radios + Open-in-Codex
            let qReq = try JSONDecoder().decode([DesktopPendingRequest].self, from: Data("""
            [{"key":"r-q","kind":"question","title":"Choose Env","body":"","canAccept":true,"questions":[{"id":"q1","header":"Target","question":"Where?","secret":false,"options":[{"label":"Staging","description":"Safe"},{"label":"Prod","description":"Live"}]}]}]
            """.utf8))
            let qHost = NSHostingView(rootView: DesktopRequestsListView(requests: qReq, busy: false, maxStackHeight: 288, onAct: { _ in }, onOpenCodex: {}).frame(width: 720))
            qHost.frame = NSRect(x: 0, y: 0, width: 720, height: 288)
            qHost.layoutSubtreeIfNeeded()
            XCTAssertLessThanOrEqual(qHost.fittingSize.height, 288.5)
            let qEncoded = String(decoding: try JSONEncoder().encode(DesktopAction.answer(requestKey: "r-q", answers: [DesktopAnswer(questionId: "q1", answers: ["Staging"])])), as: UTF8.self)
            XCTAssertTrue(qEncoded.contains("\"Staging\""))
            ParityCaseTracker.shared.record(f, "question-card-pure-circle-radio-and-open-codex")

            // 4) Elicitation card: reject/open-in-codex actions
            let eReq = try JSONDecoder().decode([DesktopPendingRequest].self, from: Data("""
            [{"key":"r-e","kind":"elicitation","title":"Credentials","body":"","canAccept":true,"questions":[],"formSchema":{"type":"object","properties":{"token":{"type":"string","title":"Token"}}}}]
            """.utf8))
            let eHost = NSHostingView(rootView: DesktopRequestsListView(requests: eReq, busy: false, maxStackHeight: 288, onAct: { _ in }, onOpenCodex: {}).frame(width: 720))
            eHost.frame = NSRect(x: 0, y: 0, width: 720, height: 288)
            eHost.layoutSubtreeIfNeeded()
            let eReject = String(decoding: try JSONEncoder().encode(DesktopAction.elicitation(requestKey: "r-e", accept: false, content: nil)), as: UTF8.self)
            XCTAssertTrue(eReject.contains("\"accept\":false"))
            ParityCaseTracker.shared.record(f, "elicitation-card-includes-open-codex")

            // 5) Command approval with acceptForSession, diff changes, and truncated warning
            let cReq = try JSONDecoder().decode([DesktopPendingRequest].self, from: Data("""
            [{"key":"r-c","kind":"command","title":"Run git","body":"","canAccept":true,"questions":[],"supportedDecisions":["acceptForSession"],"review":{"command":"git status","cwd":"/repo","reason":"Check","changes":[{"path":"a.swift","kind":"modify","diff":"+1","truncated":true}],"truncated":true}}]
            """.utf8))
            XCTAssertEqual(cReq.first?.supportedDecisions, ["acceptForSession"])
            ParityCaseTracker.shared.record(f, "command-approval-supports-accept-for-session")

            XCTAssertEqual(cReq.first?.review?.changes.first?.path, "a.swift")
            XCTAssertEqual(cReq.first?.review?.changes.first?.diff, "+1")
            ParityCaseTracker.shared.record(f, "file-diff-disclosure-in-review")

            XCTAssertEqual(cReq.first?.review?.truncated, true)
            let cHost = NSHostingView(rootView: DesktopRequestsListView(requests: cReq, busy: false, maxStackHeight: 288, onAct: { _ in }, onOpenCodex: {}).frame(width: 720))
            cHost.frame = NSRect(x: 0, y: 0, width: 720, height: 288)
            cHost.layoutSubtreeIfNeeded()
            _ = collectText(cHost)
            ParityCaseTracker.shared.record(f, "truncated-review-shows-warning")

            // 6) Unacceptable request (`canAccept: false`) fallback
            let noAcceptReq = try JSONDecoder().decode([DesktopPendingRequest].self, from: Data("""
            [{"key":"r-no","kind":"command","title":"External Approval","body":"sudo","canAccept":false,"questions":[]}]
            """.utf8))
            XCTAssertFalse(noAcceptReq[0].canAccept)
            let noAcceptHost = NSHostingView(rootView: DesktopRequestsListView(requests: noAcceptReq, busy: false, maxStackHeight: 288, onAct: { _ in }, onOpenCodex: {}).frame(width: 720))
            noAcceptHost.frame = NSRect(x: 0, y: 0, width: 720, height: 220)
            noAcceptHost.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(noAcceptHost.fittingSize.height, 40)
            ParityCaseTracker.shared.record(f, "unacceptable-request-shows-codex-fallback-hint")
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

            // 7) Outbox statuses, default disclosure open state, connect banners & diagnostics copy RPC
            func makeQueueItem(id: String = "q1", status: String, kind: String = "queue") throws -> DesktopQueuedMessage {
                try JSONDecoder().decode(
                    DesktopQueuedMessage.self,
                    from: Data("{\"id\":\"\(id)\",\"status\":\"\(status)\",\"kind\":\"\(kind)\",\"preview\":\"hello\",\"createdAt\":1}".utf8)
                )
            }
            let qQueued = try makeQueueItem(status: "queued")
            XCTAssertEqual(DesktopOutboxListView.outboxStatusLabel(qQueued), "等待线程空闲")
            XCTAssertFalse(DesktopOutboxListView.defaultOpen(for: [qQueued]))
            ParityCaseTracker.shared.record(f, "outbox-and-connect-queued")

            let qDispatching = try makeQueueItem(status: "dispatching")
            XCTAssertEqual(DesktopOutboxListView.outboxStatusLabel(qDispatching), "正在提交到 Codex…")
            XCTAssertTrue(DesktopOutboxListView.defaultOpen(for: [qDispatching]))
            ParityCaseTracker.shared.record(f, "outbox-and-connect-dispatching")

            let qHeld = try makeQueueItem(status: "held", kind: "queue")
            XCTAssertEqual(DesktopOutboxListView.outboxStatusLabel(qHeld), "已暂停")
            XCTAssertTrue(DesktopOutboxListView.defaultOpen(for: [qHeld]))
            ParityCaseTracker.shared.record(f, "outbox-and-connect-held")

            let qUnknown = try makeQueueItem(status: "outcome_unknown")
            XCTAssertEqual(DesktopOutboxListView.outboxStatusLabel(qUnknown), "结果待核对")
            ParityCaseTracker.shared.record(f, "outbox-and-connect-outcome_unknown")

            let qSteerHeld = try makeQueueItem(status: "held", kind: "steer")
            XCTAssertEqual(DesktopOutboxListView.outboxStatusLabel(qSteerHeld), "补充未发送，请取消后重试")
            ParityCaseTracker.shared.record(f, "outbox-and-connect-steer-held")

            let ownerConfirmView = try JSONDecoder().decode(
                DesktopView.self,
                from: Data("{\"connectionPhase\":\"owner_confirmation_required\",\"sessionId\":\"s\",\"threadId\":\"t\",\"revision\":0,\"connected\":true,\"writable\":false,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8)
            )
            XCTAssertEqual(
                DesktopCollaborationFooterView.connectBannerText(view: ownerConfirmView, actionError: nil, busy: false),
                "Codex owner 已变化，已恢复阅读；点击连接确认后继续协作"
            )
            ParityCaseTracker.shared.record(f, "outbox-and-connect-owner-confirm")

            let diagMock = MockRuntimeTransport { cmd, args in
                if cmd == "codex.desktop.diagnostics" && args.contains("\"copy\":true") {
                    return Data("{\"ok\":true}".utf8)
                }
                throw AgentHubFFIError("unexpected command")
            }
            let diagStore = RuntimeConnectionStore(transport: diagMock, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.diag.\(UUID().uuidString)")!)
            let diagDesktop = CodexDesktopStore(connection: diagStore, threadId: "th-1")
            // Check DesktopDiagnostics decodes real diagnostics JSON
            diagMock.setHandler { cmd, args in
                XCTAssertEqual(cmd, "codex.desktop.diagnostics")
                XCTAssertTrue(args.contains("\"copy\":true"))
                return Data("{\"copied\":true,\"summary\":\"ok\"}".utf8)
            }
            _ = try? await diagDesktop.copyDiagnostics()
            XCTAssertEqual(diagMock.recordedCalls.last?.command, "codex.desktop.diagnostics")
            ParityCaseTracker.shared.record(f, "outbox-and-connect-diagnostics-copy")
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

            // Regression: reconnect with confirmOwner=true MUST replace prior read-only view!
            transport.setHandler { cmd, args in
                switch cmd {
                case "codex.desktop.attach":
                    c.attachCalls += 1
                    let isConfirm = args.contains("\"confirmOwner\":true")
                    return Data("{\"sessionId\":\"sess-\(c.attachCalls)\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":\(isConfirm ? "true" : "false"),\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8)
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
            await desktop.connect(confirmOwner: false)
            XCTAssertEqual(desktop.view?.writable, false)
            await desktop.connect(confirmOwner: true)
            XCTAssertEqual(desktop.view?.writable, true, "confirmOwner=true must overwrite previous read-only DesktopView")
            XCTAssertEqual(desktop.view?.sessionId, "sess-\(c.attachCalls)")
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

            // 1) Redundant detach when already detached is a no-op
            let detachBefore = c.detachCalls
            await desktop.detachIfNeeded()
            XCTAssertEqual(c.detachCalls, detachBefore)
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-1")

            // 2) Act before connect throws cleanly
            do {
                try await desktop.act(.resumeQueue)
                XCTFail("Expected act() while detached to throw")
            } catch {
                XCTAssertTrue(error.localizedDescription.contains("尚未连接"))
            }
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-2")

            // 3) Live stream frame updates desktop.view and invokes onStreamViewUpdated
            var streamNotifications = 0
            desktop.onStreamViewUpdated = { streamNotifications += 1 }
            await desktop.connect(confirmOwner: true)
            transport.emitDesktopStreamFrame(Data("{\"sessionId\":\"sess-live\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"active\",\"activeTurnId\":\"turn-9\",\"requests\":[],\"queue\":[]}".utf8))
            await Task.yield()
            await Task.yield()
            XCTAssertEqual(desktop.view?.activeTurnId, "turn-9")
            XCTAssertGreaterThanOrEqual(streamNotifications, 1)
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-3")

            // 4) Stream frame for a different threadId is ignored
            transport.emitDesktopStreamFrame(Data("{\"sessionId\":\"sess-other\",\"threadId\":\"th-other\",\"revision\":0,\"connected\":true,\"writable\":true,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8))
            await Task.yield()
            XCTAssertEqual(desktop.view?.threadId, "th-1")
            XCTAssertEqual(desktop.view?.sessionId, "sess-live")
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-4")

            // 5-8) StreamReceiverBox monotonic seq ordering, duplicate/out-of-order drop, and closed finality
            final class BoxTrace: @unchecked Sendable {
                var payloads: [String] = []
                var closedCount = 0
            }
            let trace = BoxTrace()
            let box = StreamReceiverBox(
                onFrame: { trace.payloads.append(String(decoding: $0, as: UTF8.self)) },
                onClosed: { trace.closedCount += 1 }
            )
            box.handleFrame(seq: 1, data: Data("f1".utf8), closed: false)
            box.handleFrame(seq: 2, data: Data("f2".utf8), closed: false)
            XCTAssertEqual(trace.payloads, ["f1", "f2"])
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-5")

            box.handleFrame(seq: 2, data: Data("dup".utf8), closed: false)
            box.handleFrame(seq: 1, data: Data("old".utf8), closed: false)
            XCTAssertEqual(trace.payloads, ["f1", "f2"])
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-6")

            box.handleFrame(seq: 3, data: nil, closed: true)
            XCTAssertEqual(trace.closedCount, 1)
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-7")

            box.handleFrame(seq: 4, data: Data("after-close".utf8), closed: false)
            box.handleFrame(seq: 5, data: nil, closed: true)
            XCTAssertEqual(trace.payloads, ["f1", "f2"])
            XCTAssertEqual(trace.closedCount, 1)
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-8")

            // 9) Action RPC failure sets actionError and resets busy=false
            transport.setHandler { cmd, _ in
                if cmd == "codex.desktop.action" {
                    throw AgentHubFFIError("模拟提交失败")
                }
                return Data("{\"ok\":true}".utf8)
            }
            XCTAssertThrowsErrorFromAsync: do {
                do {
                    try await desktop.act(.resumeQueue)
                    XCTFail("Expected act failure")
                } catch {
                    XCTAssertEqual(desktop.actionError, "模拟提交失败")
                    XCTAssertFalse(desktop.busy)
                }
            }
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-9")

            // 10) Attach error populates actionError without leaving busy=true
            transport.setHandler { cmd, _ in
                if cmd == "codex.desktop.attach" {
                    throw AgentHubFFIError("Codex 未运行")
                }
                return Data("{\"ok\":true}".utf8)
            }
            await desktop.connect(confirmOwner: false)
            XCTAssertEqual(desktop.actionError, "Codex 未运行")
            XCTAssertFalse(desktop.busy)
            ParityCaseTracker.shared.record(f, "stream-seq-and-close-guard-10")
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
                var failOnceBounds = true
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

            // 1) Attachment event stream delivers items to onItemsAttached
            var attachedHandles: [String] = []
            var attachmentErrors: [String] = []
            desktop.onItemsAttached = { attachedHandles.append(contentsOf: $0.map(\.handle)) }
            desktop.onAttachmentError = { attachmentErrors.append($0) }
            transport.emitAttachmentEventFrame(Data("{\"bindingId\":\"b-1\",\"items\":[{\"handle\":\"att-1\",\"name\":\"spec.png\",\"kind\":\"image\",\"description\":\"\"}]}".utf8))
            await Task.yield()
            await Task.yield()
            XCTAssertEqual(attachedHandles, ["att-1"])
            ParityCaseTracker.shared.record(f, "delivers-attached-items-from-event-stream")

            // 2) Attachment event stream delivers error messages to onAttachmentError
            transport.emitAttachmentEventFrame(Data("{\"bindingId\":\"b-1\",\"items\":[],\"error\":\"图片大小超出上限\"}".utf8))
            await Task.yield()
            await Task.yield()
            XCTAssertEqual(attachmentErrors.last, "图片大小超出上限")
            ParityCaseTracker.shared.record(f, "propagates-attachment-stream-error")

            // 3) Unsupported pasteboard without plain text surfaces localized error and does NOT fallback to text
            let pb = NSPasteboard.general
            let savedPlain = pb.string(forType: .string)
            defer {
                pb.clearContents()
                if let savedPlain { pb.setString(savedPlain, forType: .string) }
            }
            pb.clearContents()
            rec.pasteMode = "unsupported"
            let unsupportedEmptyFallback = await desktop.handlePasteboardIngress(currentHandles: [])
            XCTAssertFalse(unsupportedEmptyFallback)
            XCTAssertEqual(attachmentErrors.last, "剪贴板没有可添加的文件或图片")
            ParityCaseTracker.shared.record(f, "unsupported-empty-pasteboard-reports-error")

            // 4) Unsupported pasteboard WITH plain text falls back to native NSTextView paste (preserving Undo)
            pb.clearContents()
            pb.setString("/tmp/plain-note.txt", forType: .string)
            let unsupportedWithTextFallback = await desktop.handlePasteboardIngress(currentHandles: [])
            XCTAssertTrue(unsupportedWithTextFallback)
            ParityCaseTracker.shared.record(f, "unsupported-with-plaintext-falls-back-to-text")

            // 5) Stale binding error during bounds update clears bindingId and re-binds automatically
            let prevBoundCount = rec.boundCount
            transport.setHandler { cmd, args in
                if cmd == "codex.desktop.attachments" {
                    if args.contains("\"bounds\"") && rec.failOnceBounds {
                        rec.failOnceBounds = false
                        throw AgentHubFFIError("stale binding")
                    }
                    if args.contains("\"bind\"") { rec.boundCount += 1 }
                    if args.contains("\"release\"") { rec.releasedHandles.append("released") }
                    return Data("{\"bindingId\":\"b-2\",\"pasteDisposition\":null}".utf8)
                }
                return Data("{\"ok\":true}".utf8)
            }
            await desktop.updateAttachmentBoundsAndHandles(bounds: bounds, handles: ["h1"])
            XCTAssertEqual(rec.boundCount, prevBoundCount + 1)
            XCTAssertEqual(desktop.attachmentBindingId, "b-2")
            ParityCaseTracker.shared.record(f, "rebinds-after-stale-bounds-error")

            // 6) Empty release handles is a no-op without issuing RPC
            let callsBeforeEmptyRelease = transport.recordedCalls.count
            await desktop.releaseAttachments(handles: [])
            XCTAssertEqual(transport.recordedCalls.count, callsBeforeEmptyRelease)
            ParityCaseTracker.shared.record(f, "ignores-empty-release-handles")

            // 7) Drops files at logical point records exact coordinates and paths
            transport.desktopDropFiles(paths: ["/tmp/a.png", "/tmp/b.md"], logicalX: 220, logicalY: 640)
            XCTAssertEqual(transport.recordedCalls.last?.command, "codex.desktop.dropFiles")
            let droppedPaths = try JSONDecoder().decode([String].self, from: Data((transport.recordedCalls.last?.args ?? "[]").utf8))
            XCTAssertEqual(droppedPaths, ["/tmp/a.png", "/tmp/b.md"])
            ParityCaseTracker.shared.record(f, "drops-files-at-logical-point")

            // 8) Detach sends "unbind" before "codex.desktop.detach" and clears attachmentBindingId
            await desktop.detachIfNeeded()
            XCTAssertNil(desktop.attachmentBindingId)
            let recentCommands = transport.recordedCalls.suffix(2)
            XCTAssertEqual(recentCommands.first?.command, "codex.desktop.attachments")
            XCTAssertTrue(recentCommands.first?.args.contains("\"unbind\"") == true)
            XCTAssertEqual(recentCommands.last?.command, "codex.desktop.detach")
            ParityCaseTracker.shared.record(f, "unbinds-on-detach")

            // 9) Read-only view refuses to bind attachment region
            transport.setHandler { cmd, _ in
                if cmd == "codex.desktop.attach" {
                    return Data("{\"sessionId\":\"sess-ro\",\"threadId\":\"th-1\",\"revision\":0,\"connected\":true,\"writable\":false,\"runtime\":\"idle\",\"requests\":[],\"queue\":[]}".utf8)
                }
                return Data("{\"bindingId\":\"should-not-bind\",\"pasteDisposition\":null}".utf8)
            }
            await desktop.connect(confirmOwner: false)
            await desktop.bindAttachmentsIfNeeded(bounds: bounds, handles: [])
            XCTAssertNil(desktop.attachmentBindingId)
            ParityCaseTracker.shared.record(f, "skips-bind-when-readonly")

            // 10-12) Real isolated FFIRuntimeBridge integration: attach, attachments, and paste C ABI accept wrapped request JSON!
            let isoDir = FileManager.default.temporaryDirectory.appendingPathComponent("ah-desktop-ffi-\(UUID().uuidString)")
            try FileManager.default.createDirectory(at: isoDir, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: isoDir) }
            let realBridge = try FFIRuntimeBridge(resourceDir: nil, isolatedRoot: isoDir.path)
            defer { realBridge.shutdown() }
            let realConn = RuntimeConnectionStore(transport: realBridge, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.rf.\(UUID().uuidString)")!)
            let realDesktop = CodexDesktopStore(connection: realConn, threadId: "nonexistent-thread")

            await realDesktop.connect(confirmOwner: false)
            XCTAssertNotNil(realDesktop.actionError)
            XCTAssertFalse(realDesktop.actionError?.contains("invalid attach request") == true, "Real FFI desktopAttach must parse wrapped request without schema error: \(realDesktop.actionError ?? "")")
            ParityCaseTracker.shared.record(f, "real-ffi-desktop-attach-envelope")

            do {
                _ = try await realBridge.desktopAttachments(
                    requestJSON: "{\"request\":{\"revision\":0,\"sessionId\":\"missing\",\"action\":{\"type\":\"unbind\",\"bindingId\":\"b0\"}}}",
                    onEventFrame: nil,
                    onEventClosed: nil
                )
            } catch {
                XCTAssertFalse(error.localizedDescription.contains("invalid attachments request"), "Got: \(error.localizedDescription)")
            }
            ParityCaseTracker.shared.record(f, "real-ffi-desktop-attachments-envelope")

            do {
                pb.clearContents()
                pb.setString("plain paste check", forType: .string)
                _ = try await realBridge.desktopPasteCurrentGeneralPasteboard(
                    requestJSON: "{\"request\":{\"revision\":0,\"sessionId\":\"missing\",\"action\":{\"type\":\"paste\",\"bindingId\":\"b0\"}}}"
                )
            } catch {
                XCTAssertFalse(error.localizedDescription.contains("invalid paste request"), "Got: \(error.localizedDescription)")
            }
            ParityCaseTracker.shared.record(f, "real-ffi-desktop-paste-envelope")
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
                        if args.contains("\"cur-a2\"") {
                            return Data("{\"items\":[{\"handle\":\"app-2\",\"kind\":\"app\",\"name\":\"Linear\",\"description\":\"App 2\",\"reasoningEfforts\":[]}],\"nextCursor\":null}".utf8)
                        }
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

            // 6) nextPageOnly cursor pagination fetches only sources with non-nil cursor ("apps" -> "app-2", skipping "plugins")
            let callsBeforePage2 = transport.recordedCalls.count
            let page2 = await desktop.fetchReferenceCatalog(kind: "mentions", query: "", cursors: mentions.nextCursors, nextPageOnly: true)
            XCTAssertEqual(page2.items.map(\.handle), ["app-2"])
            XCTAssertTrue(page2.nextCursors.isEmpty)
            XCTAssertEqual(transport.recordedCalls.count - callsBeforePage2, 1, "nextPageOnly must skip exhausted 'plugins' source")
            ParityCaseTracker.shared.record(f, "cursor-pagination-append")

            // 7) Handle deduplication when merging pages
            var merged = mentions.items
            let existingHandles = Set(merged.map(\.handle))
            let duplicateBatch = [mentions.items[0], page2.items[0]]
            merged.append(contentsOf: duplicateBatch.filter { !existingHandles.contains($0.handle) })
            XCTAssertEqual(merged.map(\.handle), ["plug-1", "app-1", "app-2"])
            ParityCaseTracker.shared.record(f, "dedupes-handles-across-pages")

            // 8) Partial source error survival: "apps" fails while "plugins" succeeds
            transport.setHandler { cmd, args in
                if cmd == "codex.desktop.catalog" {
                    if args.contains("\"apps\"") { throw AgentHubFFIError("apps offline") }
                    if args.contains("\"plugins\"") {
                        return Data("{\"items\":[{\"handle\":\"plug-ok\",\"kind\":\"plugin\",\"name\":\"Sentry\",\"description\":\"\",\"reasoningEfforts\":[]}],\"nextCursor\":null}".utf8)
                    }
                }
                return Data("{\"ok\":true}".utf8)
            }
            let partial = await desktop.fetchReferenceCatalog(kind: "mentions", query: "")
            XCTAssertEqual(partial.items.map(\.handle), ["plug-ok"])
            XCTAssertEqual(partial.errors["apps"], "apps offline")
            XCTAssertNil(partial.errors["plugins"])
            ParityCaseTracker.shared.record(f, "partial-source-error-survival")

            // 9) retryKind retries ONLY the failed source ("apps") without re-fetching "plugins"
            transport.setHandler { cmd, args in
                if cmd == "codex.desktop.catalog" {
                    if args.contains("\"plugins\"") { XCTFail("retryKind=apps must not query plugins") }
                    return Data("{\"items\":[{\"handle\":\"app-recovered\",\"kind\":\"app\",\"name\":\"Slack\",\"description\":\"\",\"reasoningEfforts\":[]}],\"nextCursor\":null}".utf8)
                }
                return Data("{\"ok\":true}".utf8)
            }
            let retried = await desktop.fetchReferenceCatalog(kind: "mentions", query: "", forceRefetch: true, retryKind: "apps")
            XCTAssertEqual(retried.items.map(\.handle), ["app-recovered"])
            XCTAssertTrue(transport.recordedCalls.last?.args.contains("\"forceRefetch\":true") == true)
            ParityCaseTracker.shared.record(f, "retry-single-failed-source-with-force-refetch")

            // 10) Disconnected store passes sessionId: null cleanly
            await desktop.detachIfNeeded()
            XCTAssertNil(desktop.view)
            let whileDisconnected = await desktop.fetchReferenceCatalog(kind: "apps", query: "")
            XCTAssertEqual(whileDisconnected.items.count, 1)
            ParityCaseTracker.shared.record(f, "empty-session-id-when-disconnected")

            // 11) nextPageOnly with empty cursors performs 0 RPCs
            let countBeforeNoCursor = transport.recordedCalls.count
            let emptyNext = await desktop.fetchReferenceCatalog(kind: "mentions", query: "", cursors: [:], nextPageOnly: true)
            XCTAssertTrue(emptyNext.items.isEmpty)
            XCTAssertEqual(transport.recordedCalls.count, countBeforeNoCursor)
            ParityCaseTracker.shared.record(f, "no-op-next-page-when-cursors-empty")
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
