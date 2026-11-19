@testable import AgentHubCore
import Foundation
import ObjectiveC.runtime
import XCTest

public struct ParityManifestEntry: Codable, Equatable, Sendable {
    public let webuiCases: Int
    public let swiftTestSuite: String
    public let swiftMethods: [String]
}

public struct ParityManifestDocument: Codable, Equatable, Sendable {
    public let schemaVersion: Int
    public let milestone: String
    public let requiredSourceTestFileCount: Int
    public let totalBaselineWebUICases: Int
    public let retiredPreviewOnlyFiles: [String]
    public let mappedUnitTestFiles: [String: ParityManifestEntry]
}

public final class ParityCaseTracker: @unchecked Sendable {
    public static let shared = ParityCaseTracker()

    private let lock = NSLock()
    private var executedByFile: [String: Set<String>] = [:]

    private init() {}

    public func record(_ webuiFile: String, _ caseId: String) {
        lock.lock()
        executedByFile[webuiFile, default: []].insert(caseId)
        lock.unlock()
    }

    public func count(for webuiFile: String) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return executedByFile[webuiFile]?.count ?? 0
    }

    public func cases(for webuiFile: String) -> Set<String> {
        lock.lock()
        defer { lock.unlock() }
        return executedByFile[webuiFile] ?? []
    }
}

public protocol ParityExecutableSuite: XCTestCase {
    static func executeMappedMethod(_ methodName: String) throws
}

public enum ParitySourceTestScanner {
    /// Counts static `it(...)`, `test(...)`, `it.each(...)`, `test.each(...)`, `test.for(...)` blocks in a WebUI test file.
    public static func countStaticTestBlocks(in content: String) -> Int {
        let pattern = #"(?:^|[\s;(])(?:it|test)(?:\.(?:each|for)\s*\([\s\S]*?\))?\s*\("#
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.anchorsMatchLines]) else {
            return 0
        }
        let ns = content as NSString
        return regex.numberOfMatches(in: content, options: [], range: NSRange(location: 0, length: ns.length))
    }

    public static func discoverXCTestMethods(on cls: AnyClass) -> Set<String> {
        var count: UInt32 = 0
        guard let list = class_copyMethodList(cls, &count) else { return [] }
        defer { free(list) }
        var names = Set<String>()
        for i in 0 ..< Int(count) {
            let sel = method_getName(list[i])
            let raw = NSStringFromSelector(sel)
            if raw.hasPrefix("test") {
                if raw.hasSuffix("AndReturnError:") {
                    names.insert(String(raw.dropLast("AndReturnError:".count)))
                } else if !raw.contains(":") {
                    names.insert(raw)
                }
            }
        }
        return names
    }
}

public func runAsyncParityCheck(_ block: @escaping @MainActor @Sendable () async throws -> Void) throws {
    if Thread.isMainThread {
        final class StateBox: @unchecked Sendable {
            var capturedError: Error?
            var finished = false
        }
        let state = StateBox()
        Task { @MainActor in
            do {
                try await block()
            } catch {
                state.capturedError = error
            }
            state.finished = true
        }
        let deadline = Date().addingTimeInterval(10.0)
        while !state.finished && Date() < deadline {
            RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.005))
        }
        if let err = state.capturedError {
            throw err
        }
        XCTAssertTrue(state.finished, "Async parity check timed out")
    } else {
        let sema = DispatchSemaphore(value: 0)
        final class ErrorBox: @unchecked Sendable { var error: Error? }
        let box = ErrorBox()
        Task { @MainActor in
            do {
                try await block()
            } catch {
                box.error = error
            }
            sema.signal()
        }
        _ = sema.wait(timeout: .now() + 10.0)
        if let err = box.error {
            throw err
        }
    }
}
