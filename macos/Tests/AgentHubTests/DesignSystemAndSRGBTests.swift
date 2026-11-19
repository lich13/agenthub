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
    func testOffscreenSRGBSamplingMatchesStylesCSSGroundTruthAndBadgesWithin1Over255() throws {
        // 1. Independent locked ground-truth hex table from webui/src/styles.css:1-42
        let lightRootHex: [String: String] = [
            "--r": "#f8f9fb", "--bg": "#fff", "--s": "#f3f5f8", "--e": "#fff",
            "--sl": "#e4e8ef", "--a": "#0369a1", "--as": "#e0f2fe",
            "--ok": "#14532d", "--ob": "#ebfbf2", "--w": "#7c2d12", "--wb": "#fef5e7",
            "--er": "#9f1239", "--eb": "#fef1f4", "--m": "#4b5563",
        ]
        let darkRootHex: [String: String] = [
            "--r": "#111318", "--bg": "#161920", "--s": "#1b1f27", "--e": "#222731",
            "--sl": "#2a313d", "--a": "#38bdf8", "--as": "#0c2538",
            "--ok": "#86efac", "--ob": "#132e21", "--w": "#fcd34d", "--wb": "#332611",
            "--er": "#fda4af", "--eb": "#361923", "--m": "#94a3b8",
        ]
        let lightWorkspaceAccents: [WorkspaceId: (a: String, asSoft: String)] = [
            .codex: ("#0d0d0d", "#f2f2f2"),
            .claude: ("#a63d1c", "#fdf1ec"),
            .grok: ("#171717", "#eee"),
            .antigravity: ("#185abc", "#e8f0fe"),
            .dsh: ("#2442c8", "#ecf0ff"),
        ]
        let darkWorkspaceAccents: [WorkspaceId: (a: String, asSoft: String)] = [
            .codex: ("#ececec", "#262626"),
            .claude: ("#f3a084", "#2c1e19"),
            .grok: ("#f8fafc", "#262c37"),
            .antigravity: ("#8ab4f8", "#19263c"),
            .dsh: ("#7d95ff", "#1a2342"),
        ]

        guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else {
            return
        }

        for (scheme, appearanceName) in [(ColorSchemeKind.light, NSAppearance.Name.aqua), (.dark, .darkAqua)] {
            let rootMap = (scheme == .light) ? lightRootHex : darkRootHex
            let wsMap = (scheme == .light) ? lightWorkspaceAccents : darkWorkspaceAccents

            for ws in WorkspaceId.allCases {
                let palette = DesignPalette.resolve(scheme: scheme, workspace: ws)
                let expectedAccentHex = wsMap[ws]?.a ?? rootMap["--a"]!
                let expectedAccentSoftHex = wsMap[ws]?.asSoft ?? rootMap["--as"]!

                let checks: [(String, RGBAColor, String)] = [
                    ("--r", palette.railBg, rootMap["--r"]!),
                    ("--bg", palette.background, rootMap["--bg"]!),
                    ("--s", palette.surface, rootMap["--s"]!),
                    ("--e", palette.elevated, rootMap["--e"]!),
                    ("--sl", palette.selected, rootMap["--sl"]!),
                    ("--a", palette.accent, expectedAccentHex),
                    ("--as", palette.accentSoft, expectedAccentSoftHex),
                    ("--ob", palette.okBg, rootMap["--ob"]!),
                    ("--wb", palette.warnBg, rootMap["--wb"]!),
                    ("--eb", palette.errBg, rootMap["--eb"]!),
                ]
                for (name, actualToken, cssHex) in checks {
                    let cssRGB = try parseCSSHex(cssHex)
                    let sampled = try sampleSolidColor(actualToken.nsColor, appearanceName: appearanceName)
                    XCTAssertLessThanOrEqual(abs(Int(sampled.r) - Int(cssRGB.r)), 1, "\(scheme) \(ws) \(name) R vs CSS \(cssHex)")
                    XCTAssertLessThanOrEqual(abs(Int(sampled.g) - Int(cssRGB.g)), 1, "\(scheme) \(ws) \(name) G vs CSS \(cssHex)")
                    XCTAssertLessThanOrEqual(abs(Int(sampled.b) - Int(cssRGB.b)), 1, "\(scheme) \(ws) \(name) B vs CSS \(cssHex)")
                }
            }

            // Render actual SwiftUI StatusChip & 3-tier SessionStorageBadgeView and verify sRGB pixels!
            let probePalette = DesignPalette.resolve(scheme: scheme, workspace: .probe)
            for (tone, bgHex) in [
                (StatusChipTone.success, rootMap["--ob"]!),
                (.warning, rootMap["--wb"]!),
                (.danger, rootMap["--eb"]!),
            ] {
                let expectedBg = try parseCSSHex(bgHex)
                let chipPixels = try renderSwiftUIViewPixels(
                    StatusChip("状态", tone: tone).environment(\.agentHubPalette, probePalette),
                    size: NSSize(width: 80, height: 28),
                    appearanceName: appearanceName
                )
                XCTAssertTrue(
                    containsSRGBPixel(chipPixels, matching: expectedBg, tolerance: 2),
                    "Rendered StatusChip(\(tone)) in \(scheme) missing expected CSS background \(bgHex)"
                )
            }

            // 3-tier SessionStorageBadgeView: <5 MiB muted (--m), >=5 MiB warning (--w), >=20 MiB danger (--er)
            for (bytes, fgHex) in [
                (Int64(512 * 1024), rootMap["--m"]!),
                (Int64(6 * 1024 * 1024), rootMap["--w"]!),
                (Int64(25 * 1024 * 1024), rootMap["--er"]!),
            ] {
                let sizeObj = SessionStorageSize(bytes: bytes, scope: "file", status: "complete")
                let badgePixels = try renderSwiftUIViewPixels(
                    SessionStorageBadgeView(size: sizeObj)
                        .padding(4)
                        .background(probePalette.background.color)
                        .environment(\.agentHubPalette, probePalette),
                    size: NSSize(width: 90, height: 26),
                    appearanceName: appearanceName
                )
                let bgRGB = try parseCSSHex(rootMap["--bg"]!)
                let nonBgCount = countPixelsDifferingFrom(badgePixels, bg: bgRGB, threshold: 12)
                XCTAssertGreaterThan(nonBgCount, 10, "SessionStorageBadgeView (\(bytes)B, \(fgHex)) rendered blank in \(scheme)")
            }
        }
    }

    private func parseCSSHex(_ raw: String) throws -> (r: UInt8, g: UInt8, b: UInt8) {
        let hex = String(raw.trimmingCharacters(in: .whitespacesAndNewlines).dropFirst(raw.hasPrefix("#") ? 1 : 0))
        let expanded: String
        if hex.count == 3 {
            expanded = hex.map { "\($0)\($0)" }.joined()
        } else {
            expanded = hex
        }
        guard expanded.count == 6, let val = UInt32(expanded, radix: 16) else {
            throw NSError(domain: "CSSHex", code: 1)
        }
        return (
            r: UInt8((val >> 16) & 0xFF),
            g: UInt8((val >> 8) & 0xFF),
            b: UInt8(val & 0xFF)
        )
    }

    @MainActor
    private func renderSwiftUIViewPixels<V: View>(_ view: V, size: NSSize, appearanceName: NSAppearance.Name) throws -> [UInt8] {
        let host = NSHostingView(rootView: view.frame(width: size.width, height: size.height))
        let win = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.borderless], backing: .buffered, defer: false)
        win.appearance = NSAppearance(named: appearanceName)
        win.contentView = host
        host.layoutSubtreeIfNeeded()

        let w = Int(size.width)
        let h = Int(size.height)
        let cs = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let ok = pixels.withUnsafeMutableBytes { raw -> Bool in
            guard let base = raw.baseAddress,
                  let ctx = CGContext(
                      data: base, width: w, height: h,
                      bitsPerComponent: 8, bytesPerRow: w * 4,
                      space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else { return false }
            host.wantsLayer = true
            host.layer?.render(in: ctx)
            return true
        }
        XCTAssertTrue(ok)
        return pixels
    }

    private func containsSRGBPixel(_ pixels: [UInt8], matching target: (r: UInt8, g: UInt8, b: UInt8), tolerance: Int) -> Bool {
        let count = pixels.count / 4
        for i in 0 ..< count {
            let idx = i * 4
            if abs(Int(pixels[idx]) - Int(target.r)) <= tolerance
                && abs(Int(pixels[idx + 1]) - Int(target.g)) <= tolerance
                && abs(Int(pixels[idx + 2]) - Int(target.b)) <= tolerance
            {
                return true
            }
        }
        return false
    }

    private func countPixelsDifferingFrom(_ pixels: [UInt8], bg: (r: UInt8, g: UInt8, b: UInt8), threshold: Int) -> Int {
        let count = pixels.count / 4
        var hits = 0
        for i in 0 ..< count {
            let idx = i * 4
            if abs(Int(pixels[idx]) - Int(bg.r)) > threshold
                || abs(Int(pixels[idx + 1]) - Int(bg.g)) > threshold
                || abs(Int(pixels[idx + 2]) - Int(bg.b)) > threshold
            {
                hits += 1
            }
        }
        return hits
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
