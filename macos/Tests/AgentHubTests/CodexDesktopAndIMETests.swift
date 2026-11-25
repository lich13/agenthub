@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class CodexDesktopAndIMETests: XCTestCase {
    // MARK: - Inline Search/Rename Field Editor Pre-keyDown IME Guard & Async Search Race Protection

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
}
