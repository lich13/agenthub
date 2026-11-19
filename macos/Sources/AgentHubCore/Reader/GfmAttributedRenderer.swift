import AppKit
import Foundation

// MARK: - Rendered GFM Segment & Bounded 256-entry / 2 MiB Cache

public enum GfmRenderedSegment {
    case prose(attributed: NSAttributedString, isQuote: Bool)
    case codeBlock(language: String?, code: String)
    case table(alignments: [GfmAlignment], headers: [String], rows: [[String]])
    case thematicBreak
    case instructionsFold(id: String, text: String, lines: Int, bytes: Int)
}

@MainActor
public final class GfmLayoutCache {
    public static let maxEntries = 256
    public static let maxBytes = 2 * 1024 * 1024
    public static let shared = GfmLayoutCache()

    private struct Entry {
        let segments: [GfmRenderedSegment]
        let estimatedBytes: Int
    }

    private var entries: [String: Entry] = [:]
    private var order: [String] = []
    public private(set) var totalBytes: Int = 0

    public init() {}

    public func getOrRender(
        markdown: String,
        foldInstructions: Bool,
        scheme: ColorSchemeKind,
        workspace: WorkspaceId,
        transport: any RuntimeTransport
    ) -> [GfmRenderedSegment] {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(markdown)
        let key = "\(scheme.rawValue):\(workspace.rawValue):\(foldInstructions ? 1 : 0):\(cleaned)"
        if let hit = entries[key] {
            if let idx = order.firstIndex(of: key) {
                order.remove(at: idx)
                order.append(key)
            }
            return hit.segments
        }

        let palette = DesignPalette.resolve(scheme: scheme, workspace: workspace)
        let rendered = Self.buildSegments(
            cleaned: cleaned,
            foldInstructions: foldInstructions,
            palette: palette,
            transport: transport
        )
        let estBytes = max(64, cleaned.utf8.count * 3 + rendered.count * 96)
        if estBytes <= Self.maxBytes {
            while entries.count >= Self.maxEntries || totalBytes + estBytes > Self.maxBytes {
                guard let oldest = order.first else { break }
                order.removeFirst()
                if let removed = entries.removeValue(forKey: oldest) {
                    totalBytes = max(0, totalBytes - removed.estimatedBytes)
                }
            }
            entries[key] = Entry(segments: rendered, estimatedBytes: estBytes)
            order.append(key)
            totalBytes += estBytes
        }
        return rendered
    }

    public func clear() {
        entries.removeAll()
        order.removeAll()
        totalBytes = 0
    }

    private static func buildSegments(
        cleaned: String,
        foldInstructions: Bool,
        palette: DesignPalette,
        transport: any RuntimeTransport
    ) -> [GfmRenderedSegment] {
        if cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return []
        }

        let topSlices: [InstructionSegment] = if foldInstructions {
            VisibleMarkdownEngine.instructionSegments(cleaned)
        } else {
            [InstructionSegment(kind: "markdown", id: "text", text: cleaned, lines: 1, bytes: cleaned.utf8.count)]
        }

        var output: [GfmRenderedSegment] = []
        for slice in topSlices {
            if slice.kind == "instructions" {
                output.append(.instructionsFold(
                    id: slice.id,
                    text: slice.text,
                    lines: slice.lines,
                    bytes: slice.bytes
                ))
                continue
            }
            let astData = transport.parseGfmMarkdown(slice.text)
            let doc = (try? JSONDecoder().decode(GfmDocument.self, from: astData)) ?? GfmDocument(blocks: [])
            output.append(contentsOf: renderBlocks(doc.blocks, palette: palette))
        }
        return output
    }

    private static func renderBlocks(
        _ blocks: [GfmBlock],
        palette: DesignPalette
    ) -> [GfmRenderedSegment] {
        var output: [GfmRenderedSegment] = []
        var proseBuffer = NSMutableAttributedString()

        func flushProse() {
            guard proseBuffer.length > 0 else { return }
            while proseBuffer.string.hasSuffix("\n") {
                proseBuffer.deleteCharacters(in: NSRange(location: proseBuffer.length - 1, length: 1))
            }
            if proseBuffer.length > 0 {
                output.append(.prose(attributed: NSAttributedString(attributedString: proseBuffer), isQuote: false))
            }
            proseBuffer = NSMutableAttributedString()
        }

        for (index, block) in blocks.enumerated() {
            let isFirst = (index == 0 && proseBuffer.length == 0)
            let isLast = (index == blocks.count - 1)

            switch block {
            case let .paragraph(inlines):
                let attr = renderInlines(
                    inlines,
                    baseSize: 15,
                    baseWeight: .regular,
                    lineHeightMultiple: 1.72,
                    paragraphSpacingBefore: 0,
                    paragraphSpacingAfter: isLast ? 0 : 12,
                    headIndent: 0,
                    textColor: palette.textPrimary.nsColor,
                    palette: palette
                )
                proseBuffer.append(attr)
                proseBuffer.append(NSAttributedString(string: "\n"))

            case let .heading(level, inlines):
                let fontSize: CGFloat = switch level {
                case 1: 22
                case 2: 19
                default: 16
                }
                let attr = renderInlines(
                    inlines,
                    baseSize: fontSize,
                    baseWeight: .semibold,
                    lineHeightMultiple: 1.4,
                    paragraphSpacingBefore: isFirst ? 0 : 20,
                    paragraphSpacingAfter: 10,
                    headIndent: 0,
                    textColor: palette.textPrimary.nsColor,
                    palette: palette
                )
                proseBuffer.append(attr)
                proseBuffer.append(NSAttributedString(string: "\n"))

            case let .list(ordered, start, items):
                let startNum = Int(start ?? 1)
                for (itemIdx, item) in items.enumerated() {
                    let bullet: String = if let checked = item.checked {
                        checked ? "☑ " : "☐ "
                    } else if ordered {
                        "\(startNum + itemIdx). "
                    } else {
                        "• "
                    }
                    let flatInlines = flattenItemBlocksToInlines(item.blocks)
                    var prefixed: [GfmInline] = [.text(bullet)]
                    prefixed.append(contentsOf: flatInlines)
                    let isLastItem = isLast && (itemIdx == items.count - 1)
                    let attr = renderInlines(
                        prefixed,
                        baseSize: 15,
                        baseWeight: .regular,
                        lineHeightMultiple: 1.72,
                        paragraphSpacingBefore: 0,
                        paragraphSpacingAfter: isLastItem ? 0 : (itemIdx == items.count - 1 ? 12 : 4),
                        headIndent: 22,
                        textColor: palette.textPrimary.nsColor,
                        palette: palette
                    )
                    proseBuffer.append(attr)
                    proseBuffer.append(NSAttributedString(string: "\n"))
                }

            case let .blockQuote(quoteBlocks):
                flushProse()
                let quoteText = NSMutableAttributedString()
                for (qIdx, qb) in quoteBlocks.enumerated() {
                    let qInlines = flattenItemBlocksToInlines([qb])
                    let attr = renderInlines(
                        qInlines,
                        baseSize: 15,
                        baseWeight: .regular,
                        lineHeightMultiple: 1.68,
                        paragraphSpacingBefore: 0,
                        paragraphSpacingAfter: qIdx == quoteBlocks.count - 1 ? 0 : 8,
                        headIndent: 0,
                        textColor: palette.textSecondary.nsColor,
                        palette: palette
                    )
                    quoteText.append(attr)
                    if qIdx < quoteBlocks.count - 1 {
                        quoteText.append(NSAttributedString(string: "\n"))
                    }
                }
                if quoteText.length > 0 {
                    output.append(.prose(attributed: quoteText, isQuote: true))
                }

            case let .codeBlock(language, code):
                flushProse()
                output.append(.codeBlock(language: language, code: code))

            case let .table(alignments, headers, rows):
                flushProse()
                let headerTexts = headers.map(plainTextFromInlines)
                let rowTexts = rows.map { $0.map(plainTextFromInlines) }
                output.append(.table(alignments: alignments, headers: headerTexts, rows: rowTexts))

            case .thematicBreak:
                flushProse()
                output.append(.thematicBreak)
            }
        }

        flushProse()
        return output
    }

    private static func flattenItemBlocksToInlines(_ blocks: [GfmBlock]) -> [GfmInline] {
        var out: [GfmInline] = []
        for (idx, b) in blocks.enumerated() {
            if idx > 0 { out.append(.softBreak) }
            switch b {
            case let .paragraph(inlines), let .heading(_, inlines):
                out.append(contentsOf: inlines)
            case let .codeBlock(_, code):
                out.append(.code(code))
            case let .blockQuote(inner):
                out.append(contentsOf: flattenItemBlocksToInlines(inner))
            case let .list(_, _, items):
                for item in items {
                    out.append(.softBreak)
                    out.append(.text("• "))
                    out.append(contentsOf: flattenItemBlocksToInlines(item.blocks))
                }
            case .table, .thematicBreak:
                break
            }
        }
        return out
    }

    public static func plainTextFromInlines(_ inlines: [GfmInline]) -> String {
        var out = ""
        for inline in inlines {
            switch inline {
            case let .text(t): out += t
            case let .code(c): out += c
            case let .strong(ch), let .emphasis(ch), let .strikethrough(ch):
                out += plainTextFromInlines(ch)
            case let .link(_, _, ch):
                out += plainTextFromInlines(ch)
            case let .image(_, alt, _):
                out += alt.isEmpty ? "[图片]" : alt
            case .softBreak, .hardBreak:
                out += "\n"
            }
        }
        return out
    }

    private static func renderInlines(
        _ inlines: [GfmInline],
        baseSize: CGFloat,
        baseWeight: NSFont.Weight,
        lineHeightMultiple: CGFloat,
        paragraphSpacingBefore: CGFloat,
        paragraphSpacingAfter: CGFloat,
        headIndent: CGFloat,
        textColor: NSColor,
        palette: DesignPalette
    ) -> NSAttributedString {
        let para = NSMutableParagraphStyle()
        let exactLineHeight = (baseSize * lineHeightMultiple * 10.0).rounded() / 10.0
        para.minimumLineHeight = exactLineHeight
        para.maximumLineHeight = exactLineHeight
        para.paragraphSpacingBefore = paragraphSpacingBefore
        para.paragraphSpacing = paragraphSpacingAfter
        para.firstLineHeadIndent = headIndent > 0 ? max(0, headIndent - 14) : 0
        para.headIndent = headIndent

        let result = NSMutableAttributedString()

        func appendList(
            _ list: [GfmInline],
            bold: Bool,
            italic: Bool,
            strike: Bool,
            linkURL: String?
        ) {
            for item in list {
                switch item {
                case let .text(str):
                    let weight: NSFont.Weight = bold ? .semibold : baseWeight
                    var font = DesignFonts.nsBodyFont(size: baseSize, weight: weight)
                    if italic {
                        font = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask)
                    }
                    var attrs: [NSAttributedString.Key: Any] = [
                        .font: font,
                        .foregroundColor: linkURL != nil ? palette.accent.nsColor : textColor,
                        .paragraphStyle: para,
                    ]
                    if strike {
                        attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
                    }
                    if let linkURL {
                        attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
                        if let parsed = URL(string: linkURL) {
                            attrs[.link] = parsed
                        } else {
                            attrs[.link] = linkURL
                        }
                    }
                    result.append(NSAttributedString(string: str, attributes: attrs))

                case let .code(codeStr):
                    let attrs: [NSAttributedString.Key: Any] = [
                        .font: DesignFonts.nsMonoFont(size: 13, weight: .regular),
                        .foregroundColor: palette.textPrimary.nsColor,
                        .backgroundColor: palette.hover.nsColor,
                        .paragraphStyle: para,
                    ]
                    result.append(NSAttributedString(string: " \(codeStr) ", attributes: attrs))

                case let .strong(children):
                    appendList(children, bold: true, italic: italic, strike: strike, linkURL: linkURL)

                case let .emphasis(children):
                    appendList(children, bold: bold, italic: true, strike: strike, linkURL: linkURL)

                case let .strikethrough(children):
                    appendList(children, bold: bold, italic: italic, strike: true, linkURL: linkURL)

                case let .link(url, _, children):
                    appendList(children, bold: bold, italic: italic, strike: strike, linkURL: url)

                case let .image(url, alt, _):
                    let label = alt.isEmpty ? url : "🖼 \(alt)"
                    let attrs: [NSAttributedString.Key: Any] = [
                        .font: DesignFonts.nsBodyFont(size: 13, weight: .medium),
                        .foregroundColor: palette.accent.nsColor,
                        .paragraphStyle: para,
                    ]
                    result.append(NSAttributedString(string: label, attributes: attrs))

                case .softBreak:
                    result.append(NSAttributedString(string: "\n", attributes: [
                        .font: DesignFonts.nsBodyFont(size: baseSize, weight: baseWeight),
                        .paragraphStyle: para,
                    ]))

                case .hardBreak:
                    result.append(NSAttributedString(string: "\n", attributes: [
                        .font: DesignFonts.nsBodyFont(size: baseSize, weight: baseWeight),
                        .paragraphStyle: para,
                    ]))
                }
            }
        }

        appendList(inlines, bold: false, italic: false, strike: false, linkURL: nil)
        return result
    }
}
