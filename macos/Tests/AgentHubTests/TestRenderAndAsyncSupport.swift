@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

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
        XCTAssertTrue(state.finished, "Async check timed out")
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

public final class UnconstrainedOffscreenWindow: NSWindow {
    public override init(
        contentRect: NSRect,
        styleMask style: NSWindow.StyleMask,
        backing backingStoreType: NSWindow.BackingStoreType,
        defer flag: Bool
    ) {
        super.init(contentRect: contentRect, styleMask: style, backing: backingStoreType, defer: flag)
        self.isReleasedWhenClosed = false
    }

    public override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect {
        frameRect
    }
}

public enum OffscreenRenderHelper {
    @MainActor
    public static func renderSwiftUIView<V: View>(
        _ rootView: V,
        size: NSSize,
        appearance: NSAppearance.Name,
        bottomOriginRows: Bool = true,
        pngPath: String? = nil
    ) throws -> [UInt8] {
        let host = NSHostingView(rootView: rootView.frame(width: size.width, height: size.height))
        return try renderNSView(
            host,
            size: size,
            appearance: appearance,
            bottomOriginRows: bottomOriginRows,
            pngPath: pngPath
        )
    }

    @MainActor
    public static func renderNSView(
        _ targetView: NSView,
        size: NSSize,
        appearance: NSAppearance.Name,
        bottomOriginRows: Bool = true,
        pngPath: String? = nil
    ) throws -> [UInt8] {
        let ownedWindow: UnconstrainedOffscreenWindow?
        let window: NSWindow
        if let existing = targetView.window {
            ownedWindow = nil
            window = existing
        } else {
            let created = UnconstrainedOffscreenWindow(
                contentRect: NSRect(origin: .zero, size: size),
                styleMask: [.borderless],
                backing: .buffered,
                defer: false
            )
            ownedWindow = created
            window = created
        }
        defer {
            if let ownedWindow {
                ownedWindow.contentView = nil
                ownedWindow.orderOut(nil)
            }
        }
        let nsAppearance = NSAppearance(named: appearance)
        window.appearance = nsAppearance
        targetView.appearance = nsAppearance
        targetView.frame = NSRect(origin: .zero, size: size)
        targetView.wantsLayer = true
        if ownedWindow != nil {
            window.contentView = targetView
        }

        targetView.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        targetView.displayIfNeeded()

        let w = Int(size.width)
        let h = Int(size.height)
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let srgbNS = NSColorSpace(cgColorSpace: cs),
              let rawRep = NSBitmapImageRep(
                  bitmapDataPlanes: nil,
                  pixelsWide: w,
                  pixelsHigh: h,
                  bitsPerSample: 8,
                  samplesPerPixel: 4,
                  hasAlpha: true,
                  isPlanar: false,
                  colorSpaceName: .deviceRGB,
                  bytesPerRow: w * 4,
                  bitsPerPixel: 32
              ),
              let rep = rawRep.converting(to: srgbNS, renderingIntent: .default)
        else {
            throw NSError(domain: "OffscreenRenderHelper", code: 1)
        }
        rep.size = size
        if let nsAppearance {
            nsAppearance.performAsCurrentDrawingAppearance {
                targetView.cacheDisplay(in: targetView.bounds, to: rep)
            }
        } else {
            targetView.cacheDisplay(in: targetView.bounds, to: rep)
        }
        guard let cgImg = rep.cgImage else {
            throw NSError(domain: "OffscreenRenderHelper", code: 2)
        }

        var buffer = [UInt8](repeating: 0, count: w * h * 4)
        let ok = buffer.withUnsafeMutableBytes { raw -> Bool in
            guard let base = raw.baseAddress,
                  let ctx = CGContext(
                      data: base,
                      width: w,
                      height: h,
                      bitsPerComponent: 8,
                      bytesPerRow: w * 4,
                      space: cs,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else { return false }
            if bottomOriginRows {
                ctx.translateBy(x: 0, y: CGFloat(h))
                ctx.scaleBy(x: 1, y: -1)
            }
            ctx.draw(cgImg, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        XCTAssertTrue(ok)

        if let pngPath {
            let outURL = URL(fileURLWithPath: pngPath)
            try? FileManager.default.createDirectory(at: outURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            if let pngData = rep.representation(using: .png, properties: [:]) {
                try? pngData.write(to: outURL)
            }
        }
        return buffer
    }
}
