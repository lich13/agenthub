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

    public func handleFrame(seq: UInt64, data: Data?, closed: Bool) {
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

final class HostBridgeContext: @unchecked Sendable {
    private let condition = NSCondition()
    private var runtimePtr: OpaquePointer?
    private var borrowers = 0
    private var nextRequestId: UInt64 = 1
    @MainActor private var activePanels: [UInt64: NSOpenPanel] = [:]

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
                try? self.withRuntime { rt, _ in
                    if response == .OK, let path = panel.url?.path {
                        path.withCString { agenthub_ffi_complete_choose_local_path(rt, opId, $0) }
                    } else {
                        agenthub_ffi_complete_choose_local_path(rt, opId, nil)
                    }
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
        try? ctx.withRuntime { rt, _ in
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
        try? ctx.withRuntime { rt, _ in
            agenthub_ffi_complete_write_pasteboard(rt, opId, ok)
        }
    }
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

    public func desktopAttach(
        requestJSON: String,
        onStreamFrame: @escaping @Sendable (Data) -> Void,
        onStreamClosed: (@Sendable () -> Void)? = nil
    ) async throws -> Data {
        try await submit { rt, reqId, rpcOpaque in
            let streamBox = StreamReceiverBox(onFrame: onStreamFrame, onClosed: onStreamClosed)
            let streamOpaque = Unmanaged.passRetained(streamBox).toOpaque()
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
        try await submit { rt, reqId, rpcOpaque in
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
            requestJSON.withCString { reqPtr in
                agenthub_ffi_desktop_attachments(rt, reqPtr, reqId, cRpcCallback, rpcOpaque, streamFn, streamOpaque)
            }
        }
    }

    @MainActor
    public func desktopPasteCurrentGeneralPasteboard(requestJSON: String) async throws -> Data {
        let pb = NSPasteboard.general
        let countBefore = Int64(pb.changeCount)
        let items = pb.pasteboardItems ?? []
        let totalItemCount = items.count

        let limits = agenthub_ffi_pasteboard_limits()
        // A single UTF-8 arena keeps each URL slice alive for the synchronous C call.
        var urlBytesArena: [CChar] = []
        var urlSlices: [(offset: Int, utf8Len: Int, utf16Len: Int)] = []
        for item in items.prefix(limits.files) {
            if let urlStr = item.string(forType: .fileURL) {
                let units = (urlStr as NSString).length
                let bytes = units <= limits.path_utf16_units ? urlStr.utf8.map { CChar(bitPattern: $0) } : []
                let offset = urlBytesArena.count
                urlBytesArena.append(contentsOf: bytes)
                urlSlices.append((offset: offset, utf8Len: bytes.count, utf16Len: units))
            }
        }

        // Raw image bytes without any Swift decoding/transcoding (<=64MiB passed to Rust ImageIO)
        let maxRawImageBytes = limits.image_bytes
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
        let stringUtf8: [UInt8]? = (pbString != nil && stringUtf16Len <= limits.text_utf16_units) ? Array(pbString!.utf8) : nil
        let countAfter = Int64(pb.changeCount)

        return try await submit { rt, reqId, rpcOpaque in
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
        guard let jsonData = try? JSONEncoder().encode(paths),
              let json = String(data: jsonData, encoding: .utf8)
        else { return }
        try? hostContext.withRuntime { rt, _ in
            json.withCString { agenthub_ffi_desktop_drop_files(rt, $0, logicalX, logicalY) }
        }
    }

    public func setMainWindowState(visible: Bool, focused: Bool) {
        try? hostContext.withRuntime { rt, _ in
            agenthub_ffi_set_main_window_state(rt, visible, focused)
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
        Array(markdown.utf8).withUnsafeBufferPointer { bytes in
            let ptr = bytes.baseAddress.map { UnsafeRawPointer($0).assumingMemoryBound(to: CChar.self) }
            guard let json = agenthub_ffi_parse_gfm_markdown(ptr, bytes.count) else {
                return Data(#"{"blocks":[]}"#.utf8)
            }
            defer { agenthub_ffi_string_free(json) }
            return Data(String(cString: json).utf8)
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
