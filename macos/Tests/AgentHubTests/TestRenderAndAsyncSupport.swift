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

    @MainActor
    public static func collectVisibleText(
        in view: NSView,
        includeButtons: Bool = false,
        separator: String = "\n"
    ) -> String {
        var out: [String] = []
        if let tf = view as? NSTextField { out.append(tf.stringValue) }
        if let tv = view as? NSTextView { out.append(tv.string) }
        if includeButtons, let btn = view as? NSButton {
            out.append(btn.title)
            out.append(btn.attributedTitle.string)
        }
        for sub in view.subviews {
            out.append(collectVisibleText(in: sub, includeButtons: includeButtons, separator: separator))
        }
        return out.joined(separator: separator)
    }

    public static func countNonBackgroundPixels(
        _ pixels: [UInt8],
        imageWidth: Int? = nil,
        region: (x: Int, y: Int, w: Int, h: Int)? = nil,
        bg: (UInt8, UInt8, UInt8),
        threshold: Int = 24
    ) -> Int {
        guard let imageWidth, let region else {
            let count = pixels.count / 4
            var hits = 0
            for i in 0 ..< count {
                let idx = i * 4
                if abs(Int(pixels[idx]) - Int(bg.0)) > threshold
                    || abs(Int(pixels[idx + 1]) - Int(bg.1)) > threshold
                    || abs(Int(pixels[idx + 2]) - Int(bg.2)) > threshold
                {
                    hits += 1
                }
            }
            return hits
        }
        let totalRows = pixels.count / max(1, imageWidth * 4)
        var hits = 0
        for py in max(0, region.y) ..< min(totalRows, region.y + region.h) {
            for px in max(0, region.x) ..< min(imageWidth, region.x + region.w) {
                let idx = (py * imageWidth + px) * 4
                if abs(Int(pixels[idx]) - Int(bg.0)) > threshold
                    || abs(Int(pixels[idx + 1]) - Int(bg.1)) > threshold
                    || abs(Int(pixels[idx + 2]) - Int(bg.2)) > threshold
                {
                    hits += 1
                }
            }
        }
        return hits
    }

    public static func containsPixel(
        _ pixels: [UInt8],
        imageWidth: Int? = nil,
        region: (x: Int, y: Int, w: Int, h: Int)? = nil,
        matching target: (UInt8, UInt8, UInt8),
        tolerance: Int = 2
    ) -> Bool {
        guard let imageWidth, let region else {
            let count = pixels.count / 4
            for i in 0 ..< count {
                let idx = i * 4
                if abs(Int(pixels[idx]) - Int(target.0)) <= tolerance
                    && abs(Int(pixels[idx + 1]) - Int(target.1)) <= tolerance
                    && abs(Int(pixels[idx + 2]) - Int(target.2)) <= tolerance
                {
                    return true
                }
            }
            return false
        }
        let totalRows = pixels.count / max(1, imageWidth * 4)
        for py in max(0, region.y) ..< min(totalRows, region.y + region.h) {
            for px in max(0, region.x) ..< min(imageWidth, region.x + region.w) {
                let idx = (py * imageWidth + px) * 4
                if abs(Int(pixels[idx]) - Int(target.0)) <= tolerance
                    && abs(Int(pixels[idx + 1]) - Int(target.1)) <= tolerance
                    && abs(Int(pixels[idx + 2]) - Int(target.2)) <= tolerance
                {
                    return true
                }
            }
        }
        return false
    }
}
