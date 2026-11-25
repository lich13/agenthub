import Foundation
@testable import AgentHubCore

public final class MockRuntimeTransport: RuntimeTransport, @unchecked Sendable {
    public typealias Handler = @Sendable (String, String) async throws -> Data
    private let lock = NSLock()
    private var handler: Handler
    public private(set) var recordedCalls: [(command: String, args: String)] = []

    public init(handler: @escaping Handler) {
        self.handler = handler
    }

    public func setHandler(_ next: @escaping Handler) {
        lock.lock()
        handler = next
        lock.unlock()
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

    public func parseGfmMarkdown(_ markdown: String) -> Data {
        NativeMarkdown.parse(markdown)
    }

    public func shutdown() {}
}
