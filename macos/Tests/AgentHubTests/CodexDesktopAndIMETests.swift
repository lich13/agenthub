@testable import AgentHubCore
import AppKit
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
}
