import Foundation
@testable import AgentHubCore

public final class MockRuntimeTransport: RuntimeTransport, @unchecked Sendable {
    public typealias Handler = @Sendable (String, String) async throws -> Data
    private let lock = NSLock()
    private var handler: Handler
    public private(set) var recordedCalls: [(command: String, args: String)] = []
    public private(set) var windowState: (visible: Bool, focused: Bool) = (true, true)

    private var desktopStreamFrameHandler: (@Sendable (Data) -> Void)?
    private var desktopStreamClosedHandler: (@Sendable () -> Void)?
    private var attachmentEventFrameHandler: (@Sendable (Data) -> Void)?
    private var attachmentEventClosedHandler: (@Sendable () -> Void)?

    public init(handler: @escaping Handler) {
        self.handler = handler
    }

    public func setHandler(_ next: @escaping Handler) {
        lock.lock()
        handler = next
        lock.unlock()
    }

    public func emitDesktopStreamFrame(_ data: Data) {
        lock.lock()
        let cb = desktopStreamFrameHandler
        lock.unlock()
        cb?(data)
    }

    public func closeDesktopStream() {
        lock.lock()
        let cb = desktopStreamClosedHandler
        desktopStreamFrameHandler = nil
        desktopStreamClosedHandler = nil
        lock.unlock()
        cb?()
    }

    public func emitAttachmentEventFrame(_ data: Data) {
        lock.lock()
        let cb = attachmentEventFrameHandler
        lock.unlock()
        cb?(data)
    }

    private func recordAndFetchHandler(command: String, argsJSON: String) -> Handler {
        lock.lock()
        defer { lock.unlock() }
        recordedCalls.append((command, argsJSON))
        return handler
    }

    public func invokeRawJSON(command: String, argsJSON: String) async throws -> Data {
        let currentHandler = recordAndFetchHandler(command: command, argsJSON: argsJSON)
        return try await currentHandler(command, argsJSON)
    }

    private func storeDesktopStreamHandlers(
        onFrame: @escaping @Sendable (Data) -> Void,
        onClosed: (@Sendable () -> Void)?
    ) {
        lock.lock()
        desktopStreamFrameHandler = onFrame
        desktopStreamClosedHandler = onClosed
        lock.unlock()
    }

    private func storeAttachmentEventHandlers(
        onFrame: @escaping @Sendable (Data) -> Void,
        onClosed: (@Sendable () -> Void)?
    ) {
        lock.lock()
        attachmentEventFrameHandler = onFrame
        attachmentEventClosedHandler = onClosed
        lock.unlock()
    }

    public func desktopAttach(
        requestJSON: String,
        onStreamFrame: @escaping @Sendable (Data) -> Void,
        onStreamClosed: (@Sendable () -> Void)? = nil
    ) async throws -> Data {
        storeDesktopStreamHandlers(onFrame: onStreamFrame, onClosed: onStreamClosed)
        return try await invokeRawJSON(command: "codex.desktop.attach", argsJSON: requestJSON)
    }

    public func desktopAttachments(
        requestJSON: String,
        onEventFrame: (@Sendable (Data) -> Void)? = nil,
        onEventClosed: (@Sendable () -> Void)? = nil
    ) async throws -> Data {
        if let onEventFrame {
            storeAttachmentEventHandlers(onFrame: onEventFrame, onClosed: onEventClosed)
        }
        return try await invokeRawJSON(command: "codex.desktop.attachments", argsJSON: requestJSON)
    }

    @MainActor
    public func desktopPasteCurrentGeneralPasteboard(requestJSON: String) async throws -> Data {
        try await invokeRawJSON(command: "codex.desktop.paste", argsJSON: requestJSON)
    }

    public func desktopDropFiles(paths: [String], logicalX: Double, logicalY: Double) {
        let encoded = (try? JSONEncoder().encode(paths)).flatMap { String(data: $0, encoding: .utf8) } ?? "[]"
        _ = recordAndFetchHandler(command: "codex.desktop.dropFiles", argsJSON: encoded)
    }

    public func setMainWindowState(visible: Bool, focused: Bool) {
        lock.lock()
        windowState = (visible, focused)
        lock.unlock()
    }

    public func parseGfmMarkdown(_ markdown: String) -> Data {
        NativeMarkdown.parse(markdown)
    }

    public func shutdown() {}
}
