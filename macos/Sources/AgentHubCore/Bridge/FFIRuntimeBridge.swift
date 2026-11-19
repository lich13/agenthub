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
    func desktopAttach(
        requestJSON: String,
        onStreamFrame: @escaping @Sendable (Data) -> Void,
        onStreamClosed: (@Sendable () -> Void)?
    ) async throws -> Data
    func desktopAttachments(
        requestJSON: String,
        onEventFrame: (@Sendable (Data) -> Void)?,
        onEventClosed: (@Sendable () -> Void)?
    ) async throws -> Data
    @MainActor func desktopPasteCurrentGeneralPasteboard(requestJSON: String) async throws -> Data
    func desktopDropFiles(paths: [String], logicalX: Double, logicalY: Double)
    func setMainWindowState(visible: Bool, focused: Bool)
    func parseGfmMarkdown(_ markdown: String) -> Data
    func shutdown()
}

private final class RpcContinuationBox: @unchecked Sendable {
    let continuation: CheckedContinuation<Data, Error>

    init(_ continuation: CheckedContinuation<Data, Error>) {
        self.continuation = continuation
    }
}

public final class StreamReceiverBox: @unchecked Sendable {
    private let lock = NSLock()
    private var lastSeq: UInt64 = 0
    private var isClosed = false
    private let onFrame: @Sendable (Data) -> Void
    private let onClosed: (@Sendable () -> Void)?

    public init(
        onFrame: @escaping @Sendable (Data) -> Void,
        onClosed: (@Sendable () -> Void)? = nil
    ) {
        self.onFrame = onFrame
        self.onClosed = onClosed
    }

    fileprivate func handleFrame(seq: UInt64, data: Data?, closed: Bool) {
        var shouldDeliver: Data?
        var shouldClose = false
        lock.lock()
        if !isClosed && seq > lastSeq {
            lastSeq = seq
            if closed {
                isClosed = true
                shouldClose = true
            } else if let data {
                shouldDeliver = data
            }
        } else if closed && !isClosed {
            isClosed = true
            shouldClose = true
        }
        lock.unlock()

        if let payload = shouldDeliver {
            onFrame(payload)
        }
        if shouldClose {
            onClosed?()
        }
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

private let cStreamCallback: @convention(c) (
    UInt64,
    UnsafePointer<CChar>?,
    Int,
    Bool,
    UnsafeMutableRawPointer?
) -> Void = { seq, ptr, len, closed, ctx in
    guard let ctx else { return }
    if closed {
        let box = Unmanaged<StreamReceiverBox>.fromOpaque(ctx).takeRetainedValue()
        box.handleFrame(seq: seq, data: nil, closed: true)
    } else {
        let box = Unmanaged<StreamReceiverBox>.fromOpaque(ctx).takeUnretainedValue()
        let payload: Data? = (len > 0 && ptr != nil) ? Data(bytes: ptr!, count: len) : nil
        box.handleFrame(seq: seq, data: payload, closed: false)
    }
}

private final class HostBridgeContext: @unchecked Sendable {
    private let lock = NSLock()
    private var runtimePtr: OpaquePointer?
    @MainActor private var activePanels: [UInt64: NSOpenPanel] = [:]

    func bindRuntime(_ ptr: OpaquePointer?) {
        lock.lock()
        runtimePtr = ptr
        lock.unlock()
    }

    func currentRuntime() -> OpaquePointer? {
        lock.lock()
        defer { lock.unlock() }
        return runtimePtr
    }

    @MainActor
    func presentOpenPanel(opId: UInt64, directory: Bool, prompt: String) {
        let panel = NSOpenPanel()
        panel.message = prompt
        panel.prompt = "选择"
        panel.canChooseDirectories = directory
        panel.canChooseFiles = !directory
        panel.allowsMultipleSelection = false
        panel.resolvesAliases = true
        activePanels[opId] = panel

        panel.begin { [weak self] response in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.activePanels.removeValue(forKey: opId)
                guard let rt = self.currentRuntime() else { return }
                if response == .OK, let path = panel.url?.path {
                    path.withCString { cPath in
                        agenthub_ffi_complete_choose_local_path(rt, opId, cPath)
                    }
                } else {
                    agenthub_ffi_complete_choose_local_path(rt, opId, nil)
                }
            }
        }
    }

    @MainActor
    func cancelOpenPanel(opId: UInt64) {
        if let panel = activePanels.removeValue(forKey: opId) {
            panel.cancel(nil)
        }
    }
}

private func copyUTF8(_ ptr: UnsafePointer<CChar>?, _ len: Int) -> String {
    guard len > 0, let ptr else { return "" }
    return String(decoding: UnsafeBufferPointer(start: UnsafeRawPointer(ptr).assumingMemoryBound(to: UInt8.self), count: len), as: UTF8.self)
}

private let cOpenExternalUrl: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?, Int) -> Void = { _, ptr, len in
    let urlString = copyUTF8(ptr, len)
    guard let url = URL(string: urlString),
          let scheme = url.scheme?.lowercased(),
          ["https", "http", "codex"].contains(scheme)
    else { return }
    Task { @MainActor in
        NSWorkspace.shared.open(url)
    }
}

private let cDeliverCiNotifications: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?, Int) -> Void = { _, ptr, len in
    guard len > 0, let ptr else { return }
    let data = Data(bytes: ptr, count: len)
    Task { @MainActor in
        CiNotificationCenterBridge.shared.deliverAuthorized(eventsJSON: data)
    }
}

private let cRequestAppRestart: @convention(c) (UnsafeMutableRawPointer?) -> Void = { _ in
    Task { @MainActor in
        let bundleURL = Bundle.main.bundleURL
        if bundleURL.pathExtension == "app" {
            let config = NSWorkspace.OpenConfiguration()
            config.createsNewApplicationInstance = true
            NSWorkspace.shared.openApplication(at: bundleURL, configuration: config) { _, _ in
                Task { @MainActor in
                    NSApp.terminate(nil)
                }
            }
        }
    }
}

private let cBeginChooseLocalPath: @convention(c) (UnsafeMutableRawPointer?, UInt64, Bool, UnsafePointer<CChar>?, Int) -> Void = { hostCtx, opId, directory, ptr, len in
    guard let hostCtx else { return }
    let ctx = Unmanaged<HostBridgeContext>.fromOpaque(hostCtx).takeUnretainedValue()
    let prompt = copyUTF8(ptr, len)
    Task { @MainActor in
        ctx.presentOpenPanel(opId: opId, directory: directory, prompt: prompt)
    }
}

private let cCancelChooseLocalPath: @convention(c) (UnsafeMutableRawPointer?, UInt64) -> Void = { hostCtx, opId in
    guard let hostCtx else { return }
    let ctx = Unmanaged<HostBridgeContext>.fromOpaque(hostCtx).takeUnretainedValue()
    Task { @MainActor in
        ctx.cancelOpenPanel(opId: opId)
    }
}

private let cBeginPasteboardChangeCount: @convention(c) (UnsafeMutableRawPointer?, UInt64) -> Void = { hostCtx, opId in
    guard let hostCtx else { return }
    let ctx = Unmanaged<HostBridgeContext>.fromOpaque(hostCtx).takeUnretainedValue()
    Task { @MainActor in
        let count = Int64(NSPasteboard.general.changeCount)
        if let rt = ctx.currentRuntime() {
            agenthub_ffi_complete_pasteboard_change_count(rt, opId, count)
        }
    }
}

private let cBeginWritePasteboard: @convention(c) (UnsafeMutableRawPointer?, UInt64, UnsafePointer<CChar>?, Int) -> Void = { hostCtx, opId, ptr, len in
    guard let hostCtx else { return }
    let ctx = Unmanaged<HostBridgeContext>.fromOpaque(hostCtx).takeUnretainedValue()
    let text = copyUTF8(ptr, len)
    Task { @MainActor in
        let pb = NSPasteboard.general
        pb.clearContents()
        let ok = pb.setString(text, forType: .string)
        if let rt = ctx.currentRuntime() {
            agenthub_ffi_complete_write_pasteboard(rt, opId, ok)
        }
    }
}

public final class FFIRuntimeBridge: RuntimeTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var runtimePtr: OpaquePointer?
    private var nextRequestId: UInt64 = 1
    private let hostContext: HostBridgeContext
    private var hostContextRetained: Unmanaged<HostBridgeContext>?
    public let isPreviewIsolated: Bool

    public init(resourceDir: String? = Bundle.main.resourcePath, isolatedRoot: String? = nil) throws {
        let envIsolated = ProcessInfo.processInfo.environment["AGENTHUB_ISOLATED_DATA_DIR"]
        let previewBundle = Bundle.main.bundleIdentifier == "com.lich13.agenthub.preview"
        let effectiveIsolatedRoot: String? = if let isolatedRoot, !isolatedRoot.isEmpty {
            isolatedRoot
        } else if let envIsolated, !envIsolated.isEmpty {
            envIsolated
        } else if previewBundle {
            FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Application Support/AgentHubPreview")
                .path
        } else {
            nil
        }
        self.isPreviewIsolated = (effectiveIsolatedRoot != nil)

        let ctx = HostBridgeContext()
        let retained = Unmanaged.passRetained(ctx)
        self.hostContext = ctx
        self.hostContextRetained = retained

        let callbacks = AgentHubHostCallbacks(
            host_ctx: retained.toOpaque(),
            open_external_url: cOpenExternalUrl,
            deliver_ci_notifications: cDeliverCiNotifications,
            request_app_restart: cRequestAppRestart,
            begin_choose_local_path: cBeginChooseLocalPath,
            cancel_choose_local_path: cCancelChooseLocalPath,
            begin_pasteboard_change_count: cBeginPasteboardChangeCount,
            begin_write_pasteboard: cBeginWritePasteboard
        )

        var outRuntime: OpaquePointer?
        var outError: UnsafeMutablePointer<CChar>?

        let rc: Int32 = Self.withOptionalCStrings(resourceDir, effectiveIsolatedRoot) { resPtr, isoPtr in
            agenthub_ffi_init(resPtr, isoPtr, callbacks, &outRuntime, &outError)
        }

        if rc != 0 || outRuntime == nil {
            let message: String
            if let outError {
                message = String(cString: outError)
                agenthub_ffi_string_free(outError)
            } else {
                message = "初始化 AgentHub Rust 运行时失败"
            }
            retained.release()
            self.hostContextRetained = nil
            throw AgentHubFFIError(message)
        }

        ctx.bindRuntime(outRuntime)
        self.runtimePtr = outRuntime
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

    private func nextIdAndRuntime() throws -> (UInt64, OpaquePointer) {
        lock.lock()
        defer { lock.unlock() }
        guard let rt = runtimePtr else {
            throw AgentHubFFIError("应用正在关闭")
        }
        let id = nextRequestId
        nextRequestId &+= 1
        return (id, rt)
    }

    public func invokeRawJSON(command: String, argsJSON: String = "{}") async throws -> Data {
        if command == "plugin:opener|reveal_item_in_dir" {
            try await FileOpenerService.revealFromArgsJSON(argsJSON)
            return Data("null".utf8)
        }
        let (reqId, rt) = try nextIdAndRuntime()
        return try await withCheckedThrowingContinuation { continuation in
            let box = RpcContinuationBox(continuation)
            let opaque = Unmanaged.passRetained(box).toOpaque()
            command.withCString { cmdPtr in
                argsJSON.withCString { argsPtr in
                    agenthub_ffi_invoke(rt, cmdPtr, argsPtr, reqId, cRpcCallback, opaque)
                }
            }
        }
    }

    public func desktopAttach(
        requestJSON: String,
        onStreamFrame: @escaping @Sendable (Data) -> Void,
        onStreamClosed: (@Sendable () -> Void)? = nil
    ) async throws -> Data {
        let (reqId, rt) = try nextIdAndRuntime()
        let streamBox = StreamReceiverBox(onFrame: onStreamFrame, onClosed: onStreamClosed)
        let streamOpaque = Unmanaged.passRetained(streamBox).toOpaque()
        return try await withCheckedThrowingContinuation { continuation in
            let rpcBox = RpcContinuationBox(continuation)
            let rpcOpaque = Unmanaged.passRetained(rpcBox).toOpaque()
            requestJSON.withCString { reqPtr in
                agenthub_ffi_desktop_attach(rt, reqPtr, reqId, cRpcCallback, rpcOpaque, cStreamCallback, streamOpaque)
            }
        }
    }

    public func desktopAttachments(
        requestJSON: String,
        onEventFrame: (@Sendable (Data) -> Void)? = nil,
        onEventClosed: (@Sendable () -> Void)? = nil
    ) async throws -> Data {
        let (reqId, rt) = try nextIdAndRuntime()
        let streamOpaque: UnsafeMutableRawPointer?
        let streamFn: AgentHubStreamCallback?
        if let onEventFrame {
            let box = StreamReceiverBox(onFrame: onEventFrame, onClosed: onEventClosed)
            streamOpaque = Unmanaged.passRetained(box).toOpaque()
            streamFn = cStreamCallback
        } else {
            streamOpaque = nil
            streamFn = nil
        }
        return try await withCheckedThrowingContinuation { continuation in
            let rpcBox = RpcContinuationBox(continuation)
            let rpcOpaque = Unmanaged.passRetained(rpcBox).toOpaque()
            requestJSON.withCString { reqPtr in
                agenthub_ffi_desktop_attachments(rt, reqPtr, reqId, cRpcCallback, rpcOpaque, streamFn, streamOpaque)
            }
        }
    }

    @MainActor
    public func desktopPasteCurrentGeneralPasteboard(requestJSON: String) async throws -> Data {
        let (reqId, rt) = try nextIdAndRuntime()
        let pb = NSPasteboard.general
        let countBefore = Int64(pb.changeCount)
        let items = pb.pasteboardItems ?? []
        let totalItemCount = items.count

        // Up to 33 file URLs packed into a single contiguous UTF-8 arena so every pointer stays valid across the C call
        var urlBytesArena: [CChar] = []
        var urlSlices: [(offset: Int, utf8Len: Int, utf16Len: Int)] = []
        for item in items.prefix(33) {
            if let urlStr = item.string(forType: .fileURL) {
                let bytes = Array(urlStr.utf8).map { CChar(bitPattern: $0) }
                let offset = urlBytesArena.count
                urlBytesArena.append(contentsOf: bytes)
                urlSlices.append((offset: offset, utf8Len: bytes.count, utf16Len: (urlStr as NSString).length))
            }
        }

        // Raw image bytes without any Swift decoding/transcoding (<=64MiB passed to Rust ImageIO)
        let maxRawImageBytes = 64 * 1024 * 1024
        var imageKind = AGENTHUB_RAW_IMAGE_NONE
        var imageDeclaredBytes = 0
        var imageData: Data?
        if let png = pb.data(forType: .png) {
            imageKind = AGENTHUB_RAW_IMAGE_PNG
            imageDeclaredBytes = png.count
            if png.count <= maxRawImageBytes { imageData = png }
        } else if let tiff = pb.data(forType: .tiff) {
            imageKind = AGENTHUB_RAW_IMAGE_TIFF
            imageDeclaredBytes = tiff.count
            if tiff.count <= maxRawImageBytes { imageData = tiff }
        } else if let jpeg = pb.data(forType: NSPasteboard.PasteboardType("public.jpeg")) {
            imageKind = AGENTHUB_RAW_IMAGE_JPEG
            imageDeclaredBytes = jpeg.count
            if jpeg.count <= maxRawImageBytes { imageData = jpeg }
        }

        // Raw string (<=262,144 UTF-16 code units passed to Rust path batch check)
        let pbString = pb.string(forType: .string)
        let hasString = (pbString != nil)
        let stringUtf16Len = pbString.map { ($0 as NSString).length } ?? 0
        let stringUtf8: [UInt8]? = (pbString != nil && stringUtf16Len <= 262_144) ? Array(pbString!.utf8) : nil
        let countAfter = Int64(pb.changeCount)

        return try await withCheckedThrowingContinuation { continuation in
            let rpcBox = RpcContinuationBox(continuation)
            let rpcOpaque = Unmanaged.passRetained(rpcBox).toOpaque()

            urlBytesArena.withUnsafeBufferPointer { arenaBuf in
                let cFileUrls: [AgentHubRawFileUrl] = urlSlices.map { slice in
                    let ptr = (slice.utf8Len > 0 && arenaBuf.baseAddress != nil)
                        ? arenaBuf.baseAddress!.advanced(by: slice.offset)
                        : nil
                    return AgentHubRawFileUrl(utf8_ptr: ptr, utf8_len: slice.utf8Len, utf16_len: slice.utf16Len)
                }
                cFileUrls.withUnsafeBufferPointer { urlsBuf in
                    let invokeWithImageAndString = { (imgPtr: UnsafePointer<UInt8>?, imgLen: Int, strPtr: UnsafePointer<CChar>?, strLen: Int) in
                        var snapshot = AgentHubRawPasteboard(
                            change_count_before: countBefore,
                            change_count_after: countAfter,
                            total_item_count: totalItemCount,
                            file_urls_ptr: urlsBuf.baseAddress,
                            file_urls_len: urlsBuf.count,
                            image_kind: imageKind,
                            image_declared_bytes: imageDeclaredBytes,
                            image_bytes_ptr: imgPtr,
                            image_bytes_len: imgLen,
                            has_string: hasString,
                            string_utf16_len: stringUtf16Len,
                            string_utf8_ptr: strPtr,
                            string_utf8_len: strLen
                        )
                        requestJSON.withCString { reqPtr in
                            withUnsafePointer(to: &snapshot) { snapPtr in
                                agenthub_ffi_desktop_paste(rt, reqPtr, snapPtr, reqId, cRpcCallback, rpcOpaque)
                            }
                        }
                    }

                    let withStr = { (imgPtr: UnsafePointer<UInt8>?, imgLen: Int) in
                        if let stringUtf8, !stringUtf8.isEmpty {
                            stringUtf8.withUnsafeBufferPointer { sBuf in
                                let cPtr = UnsafeRawPointer(sBuf.baseAddress!).assumingMemoryBound(to: CChar.self)
                                invokeWithImageAndString(imgPtr, imgLen, cPtr, sBuf.count)
                            }
                        } else {
                            invokeWithImageAndString(imgPtr, imgLen, nil, 0)
                        }
                    }

                    if let imageData, !imageData.isEmpty {
                        imageData.withUnsafeBytes { rawBytes in
                            withStr(rawBytes.bindMemory(to: UInt8.self).baseAddress, imageData.count)
                        }
                    } else {
                        withStr(nil, 0)
                    }
                }
            }
        }
    }

    public func desktopDropFiles(paths: [String], logicalX: Double, logicalY: Double) {
        lock.lock()
        let rt = runtimePtr
        lock.unlock()
        guard let rt, let jsonData = try? JSONEncoder().encode(paths),
              let json = String(data: jsonData, encoding: .utf8)
        else { return }
        json.withCString { ptr in
            agenthub_ffi_desktop_drop_files(rt, ptr, logicalX, logicalY)
        }
    }

    public func setMainWindowState(visible: Bool, focused: Bool) {
        lock.lock()
        let rt = runtimePtr
        lock.unlock()
        if let rt {
            agenthub_ffi_set_main_window_state(rt, visible, focused)
        }
    }

    public func parseGfmMarkdown(_ markdown: String) -> Data {
        let utf8 = Array(markdown.utf8)
        if utf8.isEmpty {
            guard let ptr = agenthub_ffi_parse_gfm_markdown(nil, 0) else {
                return Data("{\"blocks\":[]}".utf8)
            }
            defer { agenthub_ffi_string_free(ptr) }
            return Data(String(cString: ptr).utf8)
        }
        return utf8.withUnsafeBufferPointer { buf in
            let charPtr = UnsafeRawPointer(buf.baseAddress!).assumingMemoryBound(to: CChar.self)
            guard let outPtr = agenthub_ffi_parse_gfm_markdown(charPtr, buf.count) else {
                return Data("{\"blocks\":[]}".utf8)
            }
            defer { agenthub_ffi_string_free(outPtr) }
            return Data(String(cString: outPtr).utf8)
        }
    }

    public func shutdown() {
        lock.lock()
        let rt = runtimePtr
        runtimePtr = nil
        let retainedCtx = hostContextRetained
        hostContextRetained = nil
        lock.unlock()

        hostContext.bindRuntime(nil)
        if let rt {
            agenthub_ffi_shutdown(rt)
        }
        retainedCtx?.release()
    }
}

// MARK: - Safe Native Finder Opener (74* plugin:opener|reveal_item_in_dir)

public enum FileOpenerService {
    private struct RevealArgs: Decodable {
        let paths: [String]?
        let path: String?
    }

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

    public static func revealFromArgsJSON(_ argsJSON: String) async throws {
        let decoded = try JSONDecoder().decode(RevealArgs.self, from: Data(argsJSON.utf8))
        let list = decoded.paths ?? (decoded.path.map { [$0] } ?? [])
        try await revealPaths(list)
    }
}

// MARK: - Mock Transport for Deterministic XCTest & Snapshot Verification

public final class MockRuntimeTransport: RuntimeTransport, @unchecked Sendable {
    public typealias Handler = @Sendable (String, String) async throws -> Data
    private let lock = NSLock()
    private var handler: Handler
    public private(set) var recordedCalls: [(command: String, args: String)] = []
    public private(set) var windowState: (visible: Bool, focused: Bool) = (true, true)

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

    public func desktopAttach(
        requestJSON: String,
        onStreamFrame: @escaping @Sendable (Data) -> Void,
        onStreamClosed: (@Sendable () -> Void)? = nil
    ) async throws -> Data {
        defer { onStreamClosed?() }
        return try await invokeRawJSON(command: "codex.desktop.attach", argsJSON: requestJSON)
    }

    public func desktopAttachments(
        requestJSON: String,
        onEventFrame: (@Sendable (Data) -> Void)? = nil,
        onEventClosed: (@Sendable () -> Void)? = nil
    ) async throws -> Data {
        defer { onEventClosed?() }
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
        let utf8 = Array(markdown.utf8)
        if utf8.isEmpty {
            guard let ptr = agenthub_ffi_parse_gfm_markdown(nil, 0) else {
                return Data("{\"blocks\":[]}".utf8)
            }
            defer { agenthub_ffi_string_free(ptr) }
            return Data(String(cString: ptr).utf8)
        }
        return utf8.withUnsafeBufferPointer { buf in
            let charPtr = UnsafeRawPointer(buf.baseAddress!).assumingMemoryBound(to: CChar.self)
            guard let outPtr = agenthub_ffi_parse_gfm_markdown(charPtr, buf.count) else {
                return Data("{\"blocks\":[]}".utf8)
            }
            defer { agenthub_ffi_string_free(outPtr) }
            return Data(String(cString: outPtr).utf8)
        }
    }

    public func shutdown() {}
}
