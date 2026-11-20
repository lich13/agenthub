@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class CodexDesktopAndIMETests: XCTestCase {
    // MARK: - 1. Synthetic setMarkedText IME Composition Guard (⌘+Return, Return, Tab, Escape -> ZERO False Sends)

    @MainActor
    func testComposerThreeLayerIMEGuardBlocksCommandReturnAndCandidateCommitKeysDuringMarkedText() throws {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 240),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        let tv = ComposerNSTextView(frame: NSRect(x: 0, y: 0, width: 560, height: 54))
        window.contentView = tv
        window.makeFirstResponder(tv)

        var sendCount = 0
        var completionEvents: [ComposerNSTextView.InlineKeyAction] = []
        tv.onCommandEnterSend = { sendCount += 1 }
        tv.onInlineCompletionKey = { action in
            completionEvents.append(action)
            return true
        }

        // Simulate active Pinyin/Zhuyin IME marked text ("nihao")
        tv.setMarkedText(
            "nihao",
            selectedRange: NSRange(location: 5, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
        XCTAssertTrue(tv.hasMarkedText())

        // 1) ⌘+Return via performKeyEquivalent during IME composition -> MUST NOT send!
        let cmdReturnEvent = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [.command],
            timestamp: 0,
            windowNumber: window.windowNumber,
            context: nil,
            characters: "\r",
            charactersIgnoringModifiers: "\r",
            isARepeat: false,
            keyCode: 36
        ))
        XCTAssertTrue(tv.performKeyEquivalent(with: cmdReturnEvent))
        XCTAssertEqual(sendCount, 0, "⌘+Return fired send while IME marked text was active!")

        // 2) Plain Return / Tab / Escape via keyDown during IME composition -> MUST NOT trigger inline completion or dismiss!
        for (code, ch) in [(UInt16(36), "\r"), (UInt16(48), "\t"), (UInt16(53), "\u{1B}")] {
            tv.setMarkedText(
                "nihao",
                selectedRange: NSRange(location: 5, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
            let ev = try XCTUnwrap(NSEvent.keyEvent(
                with: .keyDown,
                location: .zero,
                modifierFlags: [],
                timestamp: 0,
                windowNumber: window.windowNumber,
                context: nil,
                characters: ch,
                charactersIgnoringModifiers: ch,
                isARepeat: false,
                keyCode: code
            ))
            tv.keyDown(with: ev)
        }
        XCTAssertEqual(sendCount, 0)
        XCTAssertTrue(completionEvents.isEmpty, "Inline $/ @ completion intercepted IME candidate commit key!")

        // 3) Commit composition (unmarkText) -> now ⌘+Return sends once, and plain Return triggers inline completion!
        tv.unmarkText()
        tv.string = "你好 $skill"
        XCTAssertFalse(tv.hasMarkedText())

        XCTAssertTrue(tv.performKeyEquivalent(with: cmdReturnEvent))
        XCTAssertEqual(sendCount, 1, "⌘+Return should send once after IME composition finishes")

        let plainReturn = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [],
            timestamp: 0,
            windowNumber: window.windowNumber,
            context: nil,
            characters: "\r",
            charactersIgnoringModifiers: "\r",
            isARepeat: false,
            keyCode: 36
        ))
        tv.keyDown(with: plainReturn)
        XCTAssertEqual(completionEvents, [.confirmSelection])
    }

    // MARK: - 2. Manual Resize Height Bounds (54pt–160pt) & 256 KiB UTF-8 Outgoing Payload Guard

    @MainActor
    func testComposerManualHeightClampingAnd256KiBUTF8PayloadValidation() throws {
        XCTAssertEqual(ComposerNSTextView.clampManualHeight(12), 54)
        XCTAssertEqual(ComposerNSTextView.clampManualHeight(54), 54)
        XCTAssertEqual(ComposerNSTextView.clampManualHeight(108.4), 108)
        XCTAssertEqual(ComposerNSTextView.clampManualHeight(320), 160)

        // 90,000 CJK characters = 90,000 UTF-16 code units (<= 131,072) BUT 270,000 UTF-8 bytes (> 256 KiB = 262,144)!
        let over256KiBCJK = String(repeating: "中", count: 90_000)
        XCTAssertLessThanOrEqual((over256KiBCJK as NSString).length, CodexDesktopStore.maxMessageUTF16Units)
        XCTAssertGreaterThan(over256KiBCJK.utf8.count, CodexDesktopStore.maxMessageUTF8Bytes)
        XCTAssertThrowsError(try CodexDesktopStore.validateOutgoingPayload(text: over256KiBCJK, referencesCount: 0))

        // Valid payload within 256 KiB UTF-8 & <= 32 references succeeds
        try CodexDesktopStore.validateOutgoingPayload(text: "请帮我检查这段代码", referencesCount: 2)

        // > 32 references rejected
        XCTAssertThrowsError(try CodexDesktopStore.validateOutgoingPayload(text: "ok", referencesCount: 33))
    }

    // MARK: - 3. DesktopAction Wire Serialization Parity (dismissUnknown, decide, answer, settings)

    func testDesktopActionWireJSONMatchesBackendTaggedSchema() throws {
        let dismissData = try JSONEncoder().encode(DesktopAction.dismissUnknown(messageId: "msg-99"))
        let dismissObj = try XCTUnwrap(JSONSerialization.jsonObject(with: dismissData) as? [String: Any])
        XCTAssertEqual(dismissObj["type"] as? String, "dismissUnknown")
        XCTAssertEqual(dismissObj["messageId"] as? String, "msg-99")

        let decideData = try JSONEncoder().encode(DesktopAction.decide(requestKey: "req-1", allow: true, decision: "acceptForSession"))
        let decideObj = try XCTUnwrap(JSONSerialization.jsonObject(with: decideData) as? [String: Any])
        XCTAssertEqual(decideObj["type"] as? String, "decide")
        XCTAssertEqual(decideObj["requestKey"] as? String, "req-1")
        XCTAssertEqual(decideObj["allow"] as? Bool, true)
        XCTAssertEqual(decideObj["decision"] as? String, "acceptForSession")
    }

    // MARK: - 4. Inline Search/Rename Field Editor Pre-keyDown IME Guard, Mentions Fanout, Bounds Sync & Elicitation Schema

    @MainActor
    func testInlineSearchFieldEditorIMEGuardBlocksSubmitCancelAndArrowsDuringMarkedText() throws {
        var arrowMoves: [String] = []
        var submitted = 0
        var textVal = ""
        let binding = SwiftUI.Binding<String>(get: { textVal }, set: { textVal = $0 })
        let host = NSHostingView(
            rootView: IMEGuardedInlineTextField(
                text: binding,
                placeholder: "搜索",
                onSubmit: { submitted += 1 },
                onCancel: {},
                onMoveUp: { arrowMoves.append("up") },
                onMoveDown: { arrowMoves.append("down") }
            )
            .frame(width: 320, height: 28)
        )
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 360, height: 80), styleMask: [.titled], backing: .buffered, defer: false)
        host.frame = NSRect(x: 0, y: 0, width: 360, height: 80)
        win.contentView = host
        host.layoutSubtreeIfNeeded()

        func findTextField(_ root: NSView) -> IMEGuardedNSTextField? {
            if let tf = root as? IMEGuardedNSTextField { return tf }
            for sub in root.subviews {
                if let found = findTextField(sub) { return found }
            }
            return nil
        }
        let field = try XCTUnwrap(findTextField(host))
        win.makeFirstResponder(field)
        let editor = try XCTUnwrap(field.currentEditor() as? IMEFieldEditorTextView)

        // Active IME marked text -> Down/Return MUST NOT move search result selection or submit!
        editor.setMarkedText("zhong", selectedRange: NSRange(location: 5, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertTrue(editor.hasMarkedText())
        let downEvent = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
            windowNumber: win.windowNumber, context: nil, characters: "", charactersIgnoringModifiers: "",
            isARepeat: false, keyCode: 125
        ))
        editor.keyDown(with: downEvent)
        XCTAssertTrue(arrowMoves.isEmpty, "Arrow key during IME candidate navigation must not move search list!")
        XCTAssertEqual(submitted, 0)

        // Commit IME composition -> Down arrow now navigates search results!
        editor.unmarkText()
        _ = field.delegate?.control?(field, textView: editor, doCommandBy: #selector(NSResponder.moveDown(_:)))
        XCTAssertEqual(arrowMoves, ["down"])
    }

    @MainActor
    func testLeavingBeforeAttachCompletesReleasesItsSessionAndIgnoresLaterFrames() async throws {
        let started = AsyncStream<Void>.makeStream()
        let replies = AsyncStream<Void>.makeStream()
        let view = DesktopView(sessionId: "fixture-stream", threadId: "fixture", connected: true, writable: true)
        let payload = try JSONEncoder().encode(view)
        let transport = MockRuntimeTransport { command, _ in
            if command == "codex.desktop.attach" {
                started.continuation.yield()
                var it = replies.stream.makeAsyncIterator()
                _ = await it.next()
                return payload
            }
            XCTAssertEqual(command, "codex.desktop.detach")
            return Data(#"{"ok":true}"#.utf8)
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: UUID().uuidString)!)
        let desktop = CodexDesktopStore(connection: store, threadId: view.threadId)
        let connecting = Task { await desktop.connect() }
        var it = started.stream.makeAsyncIterator()
        _ = await it.next()
        await desktop.detachIfNeeded()
        replies.continuation.yield()
        await connecting.value
        transport.emitDesktopStreamFrame(payload)
        await Task.yield()
        XCTAssertNil(desktop.view)
        XCTAssertFalse(desktop.busy)
        XCTAssertEqual(transport.recordedCalls.map(\.command), ["codex.desktop.attach", "codex.desktop.detach"])
        XCTAssertTrue(transport.recordedCalls.last?.args.contains(view.sessionId) == true)
    }

    @MainActor
    func testSearchReturnSelectsLatestQueryAfterOlderResponseArrives() async throws {
        let first = expectation(description: "first query sent")
        let newer = expectation(description: "second query returned")
        let retired = expectation(description: "first query returned")
        let release = AsyncStream<Void>.makeStream()
        defer { release.continuation.finish() }
        let transport = MockRuntimeTransport { _, args in
            struct Request: Decodable { let request: SessionSearchRequest }
            let request = try JSONDecoder().decode(Request.self, from: Data(args.utf8)).request
            if request.query == "first" {
                first.fulfill()
                var iterator = release.stream.makeAsyncIterator()
                _ = await iterator.next()
                retired.fulfill()
            } else {
                newer.fulfill()
            }
            let key = request.query
            let result = SessionSearchResult(resultId: key, sessionKey: key, nativeId: key, title: key,
                                             matchKind: "metadata", positionKey: "session", snippet: key)
            return try JSONEncoder().encode(SessionSearchResponse(provider: .codex, scope: .provider, query: key,
                                                                  results: [result], truncated: false))
        }
        let store = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: UUID().uuidString)!)
        var selection: String?
        let host = NSHostingView(rootView: SessionSearchModalView(store: store, provider: .codex, currentSessionKey: nil,
                                                                  onSelectResult: { selection = $0.sessionKey }, onClose: {}))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 680, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        func input(_ root: NSView) -> IMEGuardedNSTextField? {
            if let field = root as? IMEGuardedNSTextField { return field }
            return root.subviews.lazy.compactMap(input).first
        }
        let field = try XCTUnwrap(input(host))
        window.makeFirstResponder(field)
        let editor = try XCTUnwrap(field.currentEditor() as? NSTextView)
        func enter(_ text: String) {
            field.stringValue = text
            field.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: field))
            host.layoutSubtreeIfNeeded()
        }
        enter("first")
        await fulfillment(of: [first], timeout: 3)
        enter("second")
        await fulfillment(of: [newer], timeout: 3)
        release.continuation.yield()
        await fulfillment(of: [retired], timeout: 3)
        for _ in 0 ..< 50 where selection == nil {
            host.layoutSubtreeIfNeeded()
            _ = field.delegate?.control?(field, textView: editor, doCommandBy: #selector(NSResponder.insertNewline(_:)))
            await Task.yield()
        }
        XCTAssertEqual(selection, "second")
    }

    @MainActor
    func testMentionsCatalogFanoutAttachmentBoundsSyncAndElicitationSchemaEncoding() async throws {
        let suite = "com.lich13.agenthub.desktop.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let mock = MockRuntimeTransport { command, argsJSON in
            if command == "codex.desktop.attach" {
                return Data("""
                {"sessionId":"sess-1","threadId":"th-1","revision":0,"connected":true,"writable":true,"runtime":"idle","requests":[],"queue":[]}
                """.utf8)
            }
            if command == "codex.desktop.attachments" {
                return Data("{\"bindingId\":\"bind-1\",\"pasteDisposition\":null}".utf8)
            }
            if command == "codex.desktop.catalog" {
                if let data = argsJSON.data(using: .utf8),
                   let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   let req = root["request"] as? [String: Any],
                   let kind = req["kind"] as? String
                {
                    let itemJSON = """
                    {"items":[{"handle":"h-\(kind)","kind":"\(kind == "plugins" ? "plugin" : "app")","name":"\(kind)-item","description":"desc","reasoningEfforts":[]}],"nextCursor":null}
                    """
                    return Data(itemJSON.utf8)
                }
            }
            return Data("{\"ok\":true}".utf8)
        }

        let conn = RuntimeConnectionStore(transport: mock, defaults: defaults)
        let desktop = CodexDesktopStore(connection: conn, threadId: "th-1")
        await desktop.connect(confirmOwner: true)
        XCTAssertEqual(desktop.view?.sessionId, "sess-1")

        // 1) "@" mentions catalog MUST fan out to ["plugins", "apps"] (never sending invalid "mentions" to Rust!)
        let batch = await desktop.fetchReferenceCatalog(kind: "mentions", query: "git")
        let catalogKinds = mock.recordedCalls
            .filter { $0.command == "codex.desktop.catalog" }
            .compactMap { call -> String? in
                guard let d = call.args.data(using: .utf8),
                      let root = try? JSONSerialization.jsonObject(with: d) as? [String: Any],
                      let req = root["request"] as? [String: Any]
                else { return nil }
                return req["kind"] as? String
            }
        XCTAssertEqual(catalogKinds, ["plugins", "apps"])
        XCTAssertEqual(batch.items.map(\.handle), ["h-plugins", "h-apps"])

        // 2) Live attachment bounds & handles sync sends "bind" then "bounds"
        await desktop.updateAttachmentBoundsAndHandles(
            bounds: DesktopAttachmentBounds(x: 512, y: 620, width: 780, height: 74),
            handles: ["h-1"]
        )
        await desktop.updateAttachmentBoundsAndHandles(
            bounds: DesktopAttachmentBounds(x: 512, y: 580, width: 780, height: 114),
            handles: ["h-1", "h-2"]
        )
        let attachmentActionTypes = mock.recordedCalls
            .filter { $0.command == "codex.desktop.attachments" }
            .compactMap { call -> String? in
                guard let d = call.args.data(using: .utf8),
                      let root = try? JSONSerialization.jsonObject(with: d) as? [String: Any],
                      let req = root["request"] as? [String: Any],
                      let act = req["action"] as? [String: Any]
                else { return nil }
                return act["type"] as? String
            }
        XCTAssertEqual(attachmentActionTypes, ["bind", "bounds"])

        // 3) Top-left window coordinate conversion matches attachment bounds space
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1280, height: 820), styleMask: [.borderless], backing: .buffered, defer: false)
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 1280, height: 820))
        let composerView = NSView(frame: NSRect(x: 512, y: 60, width: 720, height: 80)) // bottom-left y=60..140 -> top-left y=680..760!
        container.addSubview(composerView)
        win.contentView = container
        let topLeftBounds = try XCTUnwrap(ComposerNSTextView.topLeftWindowBounds(for: composerView))
        XCTAssertEqual(topLeftBounds, DesktopAttachmentBounds(x: 512, y: 680, width: 720, height: 80))

        // 4) ElicitationSchemaFormEngine primitive types, enums, booleans, required validation & "__proto__" safety
        let schemaJSON = """
        {
          "properties": {
            "__proto__": { "type": "string", "title": "Proto Key" },
            "approved": { "type": "boolean", "title": "Approve" },
            "count": { "type": "integer", "title": "Count" },
            "tier": { "type": "string", "enum": ["fast", "deep"], "enumNames": ["快速", "深度"] }
          },
          "required": ["count"]
        }
        """
        let schemaVal = try JSONDecoder().decode(JSONValue.self, from: Data(schemaJSON.utf8))
        let fields = ElicitationSchemaFormEngine.parseFields(from: schemaVal)
        XCTAssertEqual(fields.map(\.key), ["__proto__", "approved", "count", "tier"])
        XCTAssertThrowsError(try ElicitationSchemaFormEngine.buildSubmissionContent(fields: fields, rawValues: [:]), "Missing required count must fail")
        XCTAssertThrowsError(try ElicitationSchemaFormEngine.buildSubmissionContent(fields: fields, rawValues: ["count": "not-an-int"]))

        let validContent = try ElicitationSchemaFormEngine.buildSubmissionContent(
            fields: fields,
            rawValues: [
                "__proto__": "safe-val",
                "approved": "true",
                "count": "42",
                "tier": "\"deep\"",
            ]
        )
        XCTAssertEqual(validContent["__proto__"], .string("safe-val"))
        XCTAssertEqual(validContent["approved"], .bool(true))
        XCTAssertEqual(validContent["count"], .number(42))
        XCTAssertEqual(validContent["tier"], .string("deep"))
    }
}
