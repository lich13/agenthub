import SwiftUI

public enum LucideIconName: String, CaseIterable, Sendable {
    case activity = "Activity"
    case archive = "Archive"
    case archiveRestore = "ArchiveRestore"
    case arrowDown = "ArrowDown"
    case arrowLeft = "ArrowLeft"
    case arrowUp = "ArrowUp"
    case atSign = "AtSign"
    case bell = "Bell"
    case blocks = "Blocks"
    case bookOpen = "BookOpen"
    case bot = "Bot"
    case brain = "Brain"
    case check = "Check"
    case checkCircle2 = "CheckCircle2"
    case chevronDown = "ChevronDown"
    case chevronLeft = "ChevronLeft"
    case chevronRight = "ChevronRight"
    case cloud = "Cloud"
    case copy = "Copy"
    case database = "Database"
    case download = "Download"
    case externalLink = "ExternalLink"
    case file = "File"
    case folderGit2 = "FolderGit2"
    case folderOpen = "FolderOpen"
    case folderPlus = "FolderPlus"
    case gitBranch = "GitBranch"
    case gitFork = "GitFork"
    case github = "Github"
    case imageOff = "ImageOff"
    case lockKeyholeOpen = "LockKeyholeOpen"
    case menu = "Menu"
    case monitor = "Monitor"
    case moon = "Moon"
    case moreHorizontal = "MoreHorizontal"
    case panelLeftClose = "PanelLeftClose"
    case panelLeftOpen = "PanelLeftOpen"
    case paperclip = "Paperclip"
    case pencil = "Pencil"
    case plus = "Plus"
    case refreshCw = "RefreshCw"
    case search = "Search"
    case slidersHorizontal = "SlidersHorizontal"
    case sparkles = "Sparkles"
    case square = "Square"
    case sun = "Sun"
    case target = "Target"
    case terminal = "Terminal"
    case terminalSquare = "TerminalSquare"
    case trash2 = "Trash2"
    case triangleAlert = "TriangleAlert"
    case wandSparkles = "WandSparkles"
    case x = "X"
}

public enum LucideVectorShape: Sendable, Equatable {
    case path(String)
    case circle(cx: CGFloat, cy: CGFloat, r: CGFloat)
    case ellipse(cx: CGFloat, cy: CGFloat, rx: CGFloat, ry: CGFloat)
    case rect(x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat, rx: CGFloat, ry: CGFloat)
    case line(x1: CGFloat, y1: CGFloat, x2: CGFloat, y2: CGFloat)
}

public enum LucideVectorCatalog {
    public static func shapes(for icon: LucideIconName) -> [LucideVectorShape] {
        switch icon {
        case .activity:
            return [.path("M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2")]
        case .archive:
            return [.rect(x: 2, y: 3, width: 20, height: 5, rx: 1, ry: 1), .path("M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"), .path("M10 12h4")]
        case .archiveRestore:
            return [.rect(x: 2, y: 3, width: 20, height: 5, rx: 1, ry: 1), .path("M4 8v11a2 2 0 0 0 2 2h2"), .path("M20 8v11a2 2 0 0 1-2 2h-2"), .path("m9 15 3-3 3 3"), .path("M12 12v9")]
        case .arrowDown:
            return [.path("M12 5v14"), .path("m19 12-7 7-7-7")]
        case .arrowLeft:
            return [.path("m12 19-7-7 7-7"), .path("M19 12H5")]
        case .arrowUp:
            return [.path("m5 12 7-7 7 7"), .path("M12 19V5")]
        case .atSign:
            return [.circle(cx: 12, cy: 12, r: 4), .path("M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8")]
        case .bell:
            return [.path("M10.268 21a2 2 0 0 0 3.464 0"), .path("M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326")]
        case .blocks:
            return [.path("M10 22V7a1 1 0 0 0-1-1H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-5a1 1 0 0 0-1-1H2"), .rect(x: 14, y: 2, width: 8, height: 8, rx: 1, ry: 1)]
        case .bookOpen:
            return [.path("M12 7v14"), .path("M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z")]
        case .bot:
            return [.path("M12 8V4H8"), .rect(x: 4, y: 8, width: 16, height: 12, rx: 2, ry: 2), .path("M2 14h2"), .path("M20 14h2"), .path("M15 13v2"), .path("M9 13v2")]
        case .brain:
            return [.path("M12 18V5"), .path("M15 13a4.17 4.17 0 0 1-3-4 4.17 4.17 0 0 1-3 4"), .path("M17.598 6.5A3 3 0 1 0 12 5a3 3 0 1 0-5.598 1.5"), .path("M17.997 5.125a4 4 0 0 1 2.526 5.77"), .path("M18 18a4 4 0 0 0 2-7.464"), .path("M19.967 17.483A4 4 0 1 1 12 18a4 4 0 1 1-7.967-.517"), .path("M6 18a4 4 0 0 1-2-7.464"), .path("M6.003 5.125a4 4 0 0 0-2.526 5.77")]
        case .check:
            return [.path("M20 6 9 17l-5-5")]
        case .checkCircle2:
            return [.circle(cx: 12, cy: 12, r: 10), .path("m9 12 2 2 4-4")]
        case .chevronDown:
            return [.path("m6 9 6 6 6-6")]
        case .chevronLeft:
            return [.path("m15 18-6-6 6-6")]
        case .chevronRight:
            return [.path("m9 18 6-6-6-6")]
        case .cloud:
            return [.path("M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z")]
        case .copy:
            return [.rect(x: 8, y: 8, width: 14, height: 14, rx: 2, ry: 2), .path("M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2")]
        case .database:
            return [.ellipse(cx: 12, cy: 5, rx: 9, ry: 3), .path("M3 5V19A9 3 0 0 0 21 19V5"), .path("M3 12A9 3 0 0 0 21 12")]
        case .download:
            return [.path("M12 15V3"), .path("M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"), .path("m7 10 5 5 5-5")]
        case .externalLink:
            return [.path("M15 3h6v6"), .path("M10 14 21 3"), .path("M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6")]
        case .file:
            return [.path("M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"), .path("M14 2v5a1 1 0 0 0 1 1h5")]
        case .folderGit2:
            return [.path("M18 19a5 5 0 0 1-5-5v8"), .path("M9 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v5"), .circle(cx: 13, cy: 12, r: 2), .circle(cx: 20, cy: 19, r: 2)]
        case .folderOpen:
            return [.path("m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2")]
        case .folderPlus:
            return [.path("M12 10v6"), .path("M9 13h6"), .path("M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z")]
        case .gitBranch:
            return [.path("M15 6a9 9 0 0 0-9 9V3"), .circle(cx: 18, cy: 6, r: 3), .circle(cx: 6, cy: 18, r: 3)]
        case .gitFork:
            return [.circle(cx: 12, cy: 18, r: 3), .circle(cx: 6, cy: 6, r: 3), .circle(cx: 18, cy: 6, r: 3), .path("M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9"), .path("M12 12v3")]
        case .github:
            return [.path("M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"), .path("M9 18c-4.51 2-5-2-7-2")]
        case .imageOff:
            return [.line(x1: 2, y1: 2, x2: 22, y2: 22), .path("M10.41 10.41a2 2 0 1 1-2.83-2.83"), .line(x1: 13.5, y1: 13.5, x2: 6, y2: 21), .line(x1: 18, y1: 12, x2: 21, y2: 15), .path("M3.59 3.59A1.99 1.99 0 0 0 3 5v14a2 2 0 0 0 2 2h14c.55 0 1.052-.22 1.41-.59"), .path("M21 15V5a2 2 0 0 0-2-2H9")]
        case .lockKeyholeOpen:
            return [.circle(cx: 12, cy: 16, r: 1), .rect(x: 3, y: 10, width: 18, height: 12, rx: 2, ry: 2), .path("M7 10V7a5 5 0 0 1 9.33-2.5")]
        case .menu:
            return [.path("M4 5h16"), .path("M4 12h16"), .path("M4 19h16")]
        case .monitor:
            return [.rect(x: 2, y: 3, width: 20, height: 14, rx: 2, ry: 2), .line(x1: 8, y1: 21, x2: 16, y2: 21), .line(x1: 12, y1: 17, x2: 12, y2: 21)]
        case .moon:
            return [.path("M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401")]
        case .moreHorizontal:
            return [.circle(cx: 12, cy: 12, r: 1), .circle(cx: 19, cy: 12, r: 1), .circle(cx: 5, cy: 12, r: 1)]
        case .panelLeftClose:
            return [.rect(x: 3, y: 3, width: 18, height: 18, rx: 2, ry: 2), .path("M9 3v18"), .path("m16 15-3-3 3-3")]
        case .panelLeftOpen:
            return [.rect(x: 3, y: 3, width: 18, height: 18, rx: 2, ry: 2), .path("M9 3v18"), .path("m14 9 3 3-3 3")]
        case .paperclip:
            return [.path("m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551")]
        case .pencil:
            return [.path("M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"), .path("m15 5 4 4")]
        case .plus:
            return [.path("M5 12h14"), .path("M12 5v14")]
        case .refreshCw:
            return [.path("M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"), .path("M21 3v5h-5"), .path("M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"), .path("M8 16H3v5")]
        case .search:
            return [.path("m21 21-4.34-4.34"), .circle(cx: 11, cy: 11, r: 8)]
        case .slidersHorizontal:
            return [.path("M10 5H3"), .path("M12 19H3"), .path("M14 3v4"), .path("M16 17v4"), .path("M21 12h-9"), .path("M21 19h-5"), .path("M21 5h-7"), .path("M8 10v4"), .path("M8 12H3")]
        case .sparkles:
            return [.path("M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z"), .path("M20 2v4"), .path("M22 4h-4"), .circle(cx: 4, cy: 20, r: 2)]
        case .square:
            return [.rect(x: 3, y: 3, width: 18, height: 18, rx: 2, ry: 2)]
        case .sun:
            return [.circle(cx: 12, cy: 12, r: 4), .path("M12 2v2"), .path("M12 20v2"), .path("m4.93 4.93 1.41 1.41"), .path("m17.66 17.66 1.41 1.41"), .path("M2 12h2"), .path("M20 12h2"), .path("m6.34 17.66-1.41 1.41"), .path("m19.07 4.93-1.41 1.41")]
        case .target:
            return [.circle(cx: 12, cy: 12, r: 10), .circle(cx: 12, cy: 12, r: 6), .circle(cx: 12, cy: 12, r: 2)]
        case .terminal:
            return [.path("M12 19h8"), .path("m4 17 6-6-6-6")]
        case .terminalSquare:
            return [.path("m7 11 2-2-2-2"), .path("M11 13h4"), .rect(x: 3, y: 3, width: 18, height: 18, rx: 2, ry: 2)]
        case .trash2:
            return [.path("M10 11v6"), .path("M14 11v6"), .path("M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"), .path("M3 6h18"), .path("M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2")]
        case .triangleAlert:
            return [.path("m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"), .path("M12 9v4"), .path("M12 17h.01")]
        case .wandSparkles:
            return [.path("m21.64 3.64-1.28-1.28a1.21 1.21 0 0 0-1.72 0L2.36 18.64a1.21 1.21 0 0 0 0 1.72l1.28 1.28a1.2 1.2 0 0 0 1.72 0L21.64 5.36a1.2 1.2 0 0 0 0-1.72"), .path("m14 7 3 3"), .path("M5 6v4"), .path("M19 14v4"), .path("M10 2v2"), .path("M7 8H3"), .path("M21 16h-4"), .path("M11 3H9")]
        case .x:
            return [.path("M18 6 6 18"), .path("m6 6 12 12")]
        }
    }
}

public struct LucideIcon: View {
    public let name: LucideIconName
    public var size: CGFloat
    public var strokeWidth: CGFloat

    public init(_ name: LucideIconName, size: CGFloat = 16, strokeWidth: CGFloat = 2) {
        self.name = name
        self.size = size
        self.strokeWidth = strokeWidth
    }

    public var body: some View {
        Canvas { context, canvasSize in
            let scale = min(canvasSize.width, canvasSize.height) / 24.0
            var transform = CGAffineTransform(scaleX: scale, y: scale)
            let style = StrokeStyle(lineWidth: strokeWidth * scale, lineCap: .round, lineJoin: .round)
            for shape in LucideVectorCatalog.shapes(for: name) {
                let rawPath: CGPath = switch shape {
                case let .path(d):
                    SVGPathParser.parse(d)
                case let .circle(cx, cy, r):
                    CGPath(ellipseIn: CGRect(x: cx - r, y: cy - r, width: r * 2, height: r * 2), transform: nil)
                case let .ellipse(cx, cy, rx, ry):
                    CGPath(ellipseIn: CGRect(x: cx - rx, y: cy - ry, width: rx * 2, height: ry * 2), transform: nil)
                case let .rect(x, y, width, height, rx, ry):
                    CGPath(roundedRect: CGRect(x: x, y: y, width: width, height: height), cornerWidth: rx, cornerHeight: ry, transform: nil)
                case let .line(x1, y1, x2, y2):
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: x1, y: y1))
                        p.addLine(to: CGPoint(x: x2, y: y2))
                        return p
                    }()
                }
                if let scaled = rawPath.copy(using: &transform) {
                    context.stroke(Path(scaled), with: .foreground, style: style)
                }
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}
