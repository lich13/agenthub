import AppKit
import CAgentHubFFI
import Foundation

public struct AgentHubFFIError: LocalizedError, Equatable, Sendable {
    public let message: String

    public init(_ message: String) {
        self.message = message
    }

    public var errorDescription: String? { message }
}

public protocol RuntimeTransport: Sendable {
    func invokeRawJSON(command: String, argsJSON: String) async throws -> Data
    func parseGfmMarkdown(_ markdown: String) -> Data
    func shutdown()
}

private final class RpcContinuationBox: @unchecked Sendable {
    let continuation: CheckedContinuation<Data, Error>

    init(_ continuation: CheckedContinuation<Data, Error>) {
        self.continuation = continuation
    }
}

private let cRpcCallback: @convention(c) (
    UInt64,
    Bool,
    UnsafePointer<CChar>?,
    Int,
    UnsafeMutableRawPointer?
) -> Void = { _, ok, ptr, len, ctx in
    guard let ctx else { return }
    let box = Unmanaged<RpcContinuationBox>.fromOpaque(ctx).takeRetainedValue()
    let data: Data
    if len > 0, let ptr {
        data = Data(bytes: ptr, count: len)
    } else {
        data = Data()
    }
    if ok {
        box.continuation.resume(returning: data)
    } else {
        let msg = String(data: data, encoding: .utf8) ?? "桌面操作失败"
        box.continuation.resume(throwing: AgentHubFFIError(msg))
    }
}

final class HostBridgeContext: @unchecked Sendable {
    private let condition = NSCondition()
    private var runtimePtr: OpaquePointer?
    private var borrowers = 0
    private var nextRequestId: UInt64 = 1

    func bindRuntime(_ ptr: OpaquePointer) {
        condition.lock()
        runtimePtr = ptr
        condition.unlock()
    }

    func withRuntime<T>(_ body: (OpaquePointer, UInt64) -> T) throws -> T {
        condition.lock()
        guard let ptr = runtimePtr else {
            condition.unlock()
            throw AgentHubFFIError("应用正在关闭")
        }
        borrowers += 1
        let id = nextRequestId
        nextRequestId &+= 1
        condition.unlock()
        defer {
            condition.lock()
            borrowers -= 1
            if borrowers == 0 { condition.broadcast() }
            condition.unlock()
        }
        return body(ptr, id)
    }

    func takeRuntime() -> OpaquePointer? {
        condition.lock()
        defer { condition.unlock() }
        guard let ptr = runtimePtr else { return nil }
        runtimePtr = nil
        while borrowers != 0 { condition.wait() }
        return ptr
    }
}

private let cDeliverCiNotifications: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?, Int) -> Void = { _, ptr, len in
    guard len > 0, let ptr else { return }
    let data = Data(bytes: ptr, count: len)
    Task { @MainActor in
        CiNotificationCenterBridge.shared.deliverAuthorized(eventsJSON: data)
    }
}

protocol WorkspaceApplicationOpening: AnyObject, Sendable {
    func openApplication(
        at applicationURL: URL,
        configuration: NSWorkspace.OpenConfiguration,
        completionHandler: (@Sendable (NSRunningApplication?, (any Error)?) -> Void)?
    )
}

private final class SystemWorkspaceApplicationOpener: WorkspaceApplicationOpening {
    func openApplication(
        at applicationURL: URL,
        configuration: NSWorkspace.OpenConfiguration,
        completionHandler: (@Sendable (NSRunningApplication?, (any Error)?) -> Void)?
    ) {
        NSWorkspace.shared.openApplication(
            at: applicationURL,
            configuration: configuration,
            completionHandler: completionHandler
        )
    }
}

enum AppRestartLauncher {
    static let manualRestartNoticeMessage = "更新已安装，请手动重新打开应用"

    @MainActor
    static func resolveSheetHostWindow(
        from windows: [NSWindow],
        keyWindow: NSWindow?,
        mainWindow: NSWindow?
    ) -> NSWindow? {
        let candidate = keyWindow
            ?? mainWindow
            ?? windows.first(where: { $0.isVisible && !$0.isSheet && $0.styleMask.contains(.titled) })
        guard var host = candidate else { return nil }
        while let parent = host.sheetParent {
            host = parent
        }
        return host.styleMask.contains(.titled) ? host : nil
    }

    @MainActor
    static func presentDefaultManualRestartAlert(_ message: String) {
        guard let app = NSApp else { return }
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = message
        alert.addButton(withTitle: "好")
        if let window = resolveSheetHostWindow(
            from: app.windows,
            keyWindow: app.keyWindow,
            mainWindow: app.mainWindow
        ) {
            alert.beginSheetModal(for: window)
        } else if app.activationPolicy() != .prohibited {
            alert.runModal()
        }
    }

    // Built in a `nonisolated` scope with an explicit `@Sendable` completion handler so AppKit's
    // concurrent completion queue never triggers a Swift 6 SE-0423 `@MainActor` trap.
    static func launchReplacementAndTerminate(
        bundleURL: URL,
        workspace: any WorkspaceApplicationOpening = SystemWorkspaceApplicationOpener(),
        onLaunchFailure: @escaping @MainActor @Sendable (String) -> Void = { message in
            presentDefaultManualRestartAlert(message)
        },
        terminate: @escaping @MainActor @Sendable () -> Void = {
            guard NSApp != nil else { return }
            NSApp.terminate(nil)
        }
    ) {
        guard bundleURL.pathExtension == "app" else { return }
        let config = NSWorkspace.OpenConfiguration()
        config.createsNewApplicationInstance = true
        let notice = Self.manualRestartNoticeMessage
        let onCompleted: @Sendable (NSRunningApplication?, (any Error)?) -> Void = { app, error in
            guard app != nil, error == nil else {
                Task { @MainActor in
                    onLaunchFailure(notice)
                }
                return
            }
            Task { @MainActor in
                terminate()
            }
        }
        workspace.openApplication(
            at: bundleURL,
            configuration: config,
            completionHandler: onCompleted
        )
    }
}

private let cRequestAppRestart: @convention(c) (UnsafeMutableRawPointer?) -> Void = { _ in
    AppRestartLauncher.launchReplacementAndTerminate(bundleURL: Bundle.main.bundleURL)
}

public final class FFIRuntimeBridge: RuntimeTransport, @unchecked Sendable {
    private let hostContext: HostBridgeContext
    public let isPreviewIsolated: Bool

    public init(resourceDir: String? = Bundle.main.resourcePath, isolatedRoot: String? = nil) throws {
        let envIsolated = ProcessInfo.processInfo.environment["AGENTHUB_ISOLATED_DATA_DIR"]
        let bundleRoot = Bundle.main.object(forInfoDictionaryKey: "AgentHubIsolatedDataRoot") as? String
        let effectiveIsolatedRoot: String? = if let isolatedRoot, !isolatedRoot.isEmpty {
            isolatedRoot
        } else if let envIsolated, !envIsolated.isEmpty {
            envIsolated
        } else if let bundleRoot {
            (bundleRoot as NSString).expandingTildeInPath
        } else {
            nil
        }
        self.isPreviewIsolated = (effectiveIsolatedRoot != nil)

        let ctx = HostBridgeContext()
        self.hostContext = ctx

        let callbacks = AgentHubHostCallbacks(
            host_ctx: Unmanaged.passUnretained(ctx).toOpaque(),
            retain_host_ctx: { ptr in
                if let ptr { _ = Unmanaged<HostBridgeContext>.fromOpaque(ptr).retain() }
            },
            release_host_ctx: { ptr in
                if let ptr { Unmanaged<HostBridgeContext>.fromOpaque(ptr).release() }
            },
            deliver_ci_notifications: cDeliverCiNotifications,
            request_app_restart: cRequestAppRestart
        )

        var outRuntime: OpaquePointer?
        var outError: UnsafeMutablePointer<CChar>?

        let rc: Int32 = Self.withOptionalCStrings(resourceDir, effectiveIsolatedRoot) { resPtr, isoPtr in
            agenthub_ffi_init(resPtr, isoPtr, callbacks, &outRuntime, &outError)
        }

        guard rc == 0, let outRuntime else {
            let message: String
            if let outError {
                message = String(cString: outError)
                agenthub_ffi_string_free(outError)
            } else {
                message = "初始化 AgentHub Rust 运行时失败"
            }
            throw AgentHubFFIError(message)
        }

        ctx.bindRuntime(outRuntime)
    }

    deinit {
        shutdown()
    }

    private static func withOptionalCStrings<R>(
        _ a: String?,
        _ b: String?,
        body: (UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> R
    ) -> R {
        func withOne(_ s: String?, _ inner: (UnsafePointer<CChar>?) -> R) -> R {
            if let s { return s.withCString(inner) }
            return inner(nil)
        }
        return withOne(a) { ptrA in
            withOne(b) { ptrB in
                body(ptrA, ptrB)
            }
        }
    }

    private func submit(
        isolation: isolated (any Actor)? = #isolation,
        _ register: (OpaquePointer, UInt64, UnsafeMutableRawPointer) -> Void
    ) async throws -> Data {
        try await withCheckedThrowingContinuation { continuation in
            do {
                try hostContext.withRuntime { rt, id in
                    let opaque = Unmanaged.passRetained(RpcContinuationBox(continuation)).toOpaque()
                    register(rt, id, opaque)
                }
            } catch {
                continuation.resume(throwing: error)
            }
        }
    }

    public func invokeRawJSON(command: String, argsJSON: String = "{}") async throws -> Data {
        return try await submit { rt, reqId, opaque in
            command.withCString { cmdPtr in
                argsJSON.withCString { argsPtr in
                    agenthub_ffi_invoke(rt, cmdPtr, argsPtr, reqId, cRpcCallback, opaque)
                }
            }
        }
    }

    public func parseGfmMarkdown(_ markdown: String) -> Data {
        NativeMarkdown.parse(markdown)
    }

    public func shutdown() {
        if let rt = hostContext.takeRuntime() {
            agenthub_ffi_shutdown(rt)
        }
    }
}

enum NativeMarkdown {
    static func parse(_ markdown: String) -> Data {
        func invoke(_ ptr: UnsafePointer<CChar>?, _ count: Int) -> Data {
            var outLen: Int = 0
            guard let json = agenthub_ffi_parse_gfm_markdown(ptr, count, &outLen) else {
                return Data()
            }
            defer { agenthub_ffi_string_free(json) }
            return Data(bytes: json, count: outLen)
        }
        if let result = markdown.utf8.withContiguousStorageIfAvailable({ buf in
            let ptr = buf.baseAddress.map { UnsafeRawPointer($0).assumingMemoryBound(to: CChar.self) }
            return invoke(ptr, buf.count)
        }) {
            return result
        }
        var copy = markdown
        return copy.withUTF8 { buf in
            let ptr = buf.baseAddress.map { UnsafeRawPointer($0).assumingMemoryBound(to: CChar.self) }
            return invoke(ptr, buf.count)
        }
    }
}

public enum FileOpenerService {
    public static func validateRevealPath(_ raw: String) throws -> URL {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              trimmed.hasPrefix("/"),
              !trimmed.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        else {
            throw AgentHubFFIError("文件路径无效，无法在 Finder 中显示")
        }
        return URL(fileURLWithPath: trimmed)
    }

    public static func revealPaths(_ paths: [String]) async throws {
        let urls = try paths.map(validateRevealPath)
        guard !urls.isEmpty else {
            throw AgentHubFFIError("文件路径无效，无法在 Finder 中显示")
        }
        await MainActor.run {
            NSWorkspace.shared.activateFileViewerSelecting(urls)
        }
    }
}
