@testable import AgentHubCore
import AppKit
import SwiftUI
import XCTest

final class DesignSystemAndSRGBTests: XCTestCase {
    func testFFIHeaderMatchesRustCrateHeaderByteForByte() throws {
        let testFile = URL(fileURLWithPath: #filePath)
        let repoRoot = testFile
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let rustHeader = try Data(contentsOf: repoRoot.appendingPathComponent("crates/agenthub-ffi/include/agenthub_ffi.h"))
        let swiftHeader = try Data(contentsOf: repoRoot.appendingPathComponent("macos/Sources/CAgentHubFFI/include/agenthub_ffi.h"))
        XCTAssertEqual(rustHeader, swiftHeader, "CAgentHubFFI header drifted from crates/agenthub-ffi/include/agenthub_ffi.h")
    }

    func testAll53LucideIconsParseToNonEmptyPaths() {
        XCTAssertEqual(LucideIconName.allCases.count, 53)
        for icon in LucideIconName.allCases {
            let shapes = LucideVectorCatalog.shapes(for: icon)
            XCTAssertFalse(shapes.isEmpty, "Icon \(icon.rawValue) has no shapes")
            for shape in shapes {
                if case let .path(d) = shape {
                    let parsed = SVGPathParser.parse(d)
                    XCTAssertFalse(parsed.isEmpty, "SVG path empty for \(icon.rawValue)")
                }
            }
        }
    }

    func testWCAGContrastRatiosAcrossLightAndDarkWorkspaces() {
        for scheme in [ColorSchemeKind.light, .dark] {
            for ws in WorkspaceId.allCases {
                let p = DesignPalette.resolve(scheme: scheme, workspace: ws)
                XCTAssertGreaterThanOrEqual(
                    RGBAColor.contrastRatio(p.textPrimary, p.background),
                    4.5,
                    "Primary text contrast failed for \(scheme)-\(ws)"
                )
                XCTAssertGreaterThanOrEqual(
                    RGBAColor.contrastRatio(p.textSecondary, p.background),
                    4.5,
                    "Secondary text contrast failed for \(scheme)-\(ws)"
                )
                XCTAssertGreaterThanOrEqual(
                    RGBAColor.contrastRatio(p.textMuted, p.background),
                    4.5,
                    "Muted text contrast failed for \(scheme)-\(ws)"
                )
                XCTAssertGreaterThanOrEqual(
                    RGBAColor.contrastRatio(p.accent, p.background),
                    3.0,
                    "Focus/accent contrast failed for \(scheme)-\(ws)"
                )
            }
        }
    }

    @MainActor
    func testOffscreenSRGBSamplingMatchesStylesCSSWithin1Over255() throws {
        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }
        for (scheme, appearanceName) in [(ColorSchemeKind.light, NSAppearance.Name.aqua), (.dark, .darkAqua)] {
            for ws in WorkspaceId.allCases {
                let palette = DesignPalette.resolve(scheme: scheme, workspace: ws)
                let swatches: [(String, RGBAColor)] = [
                    ("--r", palette.railBg),
                    ("--bg", palette.background),
                    ("--s", palette.surface),
                    ("--e", palette.elevated),
                    ("--sl", palette.selected),
                    ("--a", palette.accent),
                    ("--as", palette.accentSoft),
                    ("--ok-bg", palette.okBg),
                    ("--w-bg", palette.warnBg),
                    ("--er-bg", palette.errBg),
                ]
                for (name, expected) in swatches {
                    let sampled = try sampleSolidColor(expected.nsColor, appearanceName: appearanceName)
                    XCTAssertLessThanOrEqual(abs(Int(sampled.r) - Int(expected.r)), 1, "\(scheme) \(ws) \(name) R")
                    XCTAssertLessThanOrEqual(abs(Int(sampled.g) - Int(expected.g)), 1, "\(scheme) \(ws) \(name) G")
                    XCTAssertLessThanOrEqual(abs(Int(sampled.b) - Int(expected.b)), 1, "\(scheme) \(ws) \(name) B")
                }
            }
        }
    }

    @MainActor
    private func sampleSolidColor(_ color: NSColor, appearanceName: NSAppearance.Name) throws -> (r: UInt8, g: UInt8, b: UInt8) {
        let width = 16
        let height = 16
        guard let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) else {
            throw NSError(domain: "SRGBTest", code: 1)
        }
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let ok = pixels.withUnsafeMutableBytes { rawBuffer -> Bool in
            guard let baseAddress = rawBuffer.baseAddress,
                  let ctx = CGContext(
                      data: baseAddress,
                      width: width,
                      height: height,
                      bitsPerComponent: 8,
                      bytesPerRow: width * 4,
                      space: colorSpace,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else {
                return false
            }
            let drawBlock = {
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
                let swatchView = NSView(frame: NSRect(x: 0, y: 0, width: width, height: height))
                swatchView.wantsLayer = true
                swatchView.layer?.backgroundColor = color.usingColorSpace(.sRGB)?.cgColor
                swatchView.layer?.render(in: ctx)
                NSGraphicsContext.restoreGraphicsState()
            }
            if let appearance = NSAppearance(named: appearanceName) {
                appearance.performAsCurrentDrawingAppearance(drawBlock)
            } else {
                drawBlock()
            }
            return true
        }
        guard ok else {
            throw NSError(domain: "SRGBTest", code: 2)
        }
        let centerIndex = (8 * width + 8) * 4
        return (r: pixels[centerIndex], g: pixels[centerIndex + 1], b: pixels[centerIndex + 2])
    }
}
