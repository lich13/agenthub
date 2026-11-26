import AppKit
import Foundation

extension NSAttributedString.Key {
    public static let agentHubBlockQuote = NSAttributedString.Key("AgentHubBlockQuote")
    public static let agentHubInlineCode = NSAttributedString.Key("AgentHubInlineCode")
}

// MARK: - Rendered GFM Segment & Bounded 256-entry / 2 MiB Cache

public enum GfmRenderedSegment {
    case prose(attributed: NSAttributedString)
    case codeBlock(language: String?, code: String)
    case table(alignments: [GfmAlignment], headers: [NSAttributedString], rows: [[NSAttributedString]])
    case thematicBreak
    case instructionsFold(id: String, text: String, lines: Int, bytes: Int)
}

@MainActor
public final class GfmLayoutCache {
    public static let maxEntries = 256
    public static let maxBytes = 2 * 1024 * 1024
    public static let shared = GfmLayoutCache()
    private static let gfmDecoder = JSONDecoder()

    private struct CacheKey: Hashable {
        let scheme: ColorSchemeKind
        let workspace: WorkspaceId
        let foldInstructions: Bool
        let utf8Count: Int
        let cleaned: String
    }

    private struct Entry {
        let segments: [GfmRenderedSegment]
        let estimatedBytes: Int
        var lastSeq: UInt64
    }

    private var entries: [CacheKey: Entry] = [:]
    private var accessSeq: UInt64 = 0
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
        let key = CacheKey(
            scheme: scheme,
            workspace: workspace,
            foldInstructions: foldInstructions,
            utf8Count: cleaned.utf8.count,
            cleaned: cleaned
        )
        if var hit = entries[key] {
            accessSeq &+= 1
            hit.lastSeq = accessSeq
            entries[key] = hit
            return hit.segments
        }

        let palette = DesignPalette.resolve(scheme: scheme, workspace: workspace)
        let (rendered, cacheable) = Self.buildSegments(
            cleaned: cleaned,
            foldInstructions: foldInstructions,
            palette: palette,
            transport: transport
        )
        let estBytes = max(64, cleaned.utf8.count * 3 + rendered.count * 96)
        if cacheable && estBytes <= Self.maxBytes {
            while entries.count >= Self.maxEntries || totalBytes + estBytes > Self.maxBytes {
                guard let oldestKey = entries.min(by: { $0.value.lastSeq < $1.value.lastSeq })?.key else { break }
                if let removed = entries.removeValue(forKey: oldestKey) {
                    totalBytes = max(0, totalBytes - removed.estimatedBytes)
                }
            }
            accessSeq &+= 1
            entries[key] = Entry(segments: rendered, estimatedBytes: estBytes, lastSeq: accessSeq)
            totalBytes += estBytes
        }
        return rendered
    }

    public func clear() {
        entries.removeAll()
        accessSeq = 0
        totalBytes = 0
    }

    private static func buildSegments(
        cleaned: String,
        foldInstructions: Bool,
        palette: DesignPalette,
        transport: any RuntimeTransport
    ) -> (segments: [GfmRenderedSegment], cacheable: Bool) {
        if cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return ([], true)
        }

        let topSlices: [InstructionSegment] = if foldInstructions {
            VisibleMarkdownEngine.instructionSegments(cleaned)
        } else {
            [InstructionSegment(kind: "markdown", id: "text", text: cleaned, lines: 1, bytes: cleaned.utf8.count)]
        }

        var output: [GfmRenderedSegment] = []
        var allDecoded = true
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
            if !astData.isEmpty, let doc = try? gfmDecoder.decode(GfmDocument.self, from: astData) {
                output.append(contentsOf: renderBlocks(doc.blocks, palette: palette))
            } else {
                allDecoded = false
                if !slice.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    output.append(contentsOf: renderBlocks([.paragraph(inlines: [.text(slice.text)])], palette: palette))
                }
            }
        }
        return (output, allDecoded)
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
                output.append(.prose(attributed: NSAttributedString(attributedString: proseBuffer)))
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
                    lineHeightMultiple: 1.65,
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
                        lineHeightMultiple: 1.65,
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
                for (qIdx, qb) in quoteBlocks.enumerated() {
                    let qInlines = flattenItemBlocksToInlines([qb])
                    let isLastQuotePara = isLast && (qIdx == quoteBlocks.count - 1)
                    let attr = NSMutableAttributedString(
                        attributedString: renderInlines(
                            qInlines,
                            baseSize: 15,
                            baseWeight: .regular,
                            lineHeightMultiple: 1.65,
                            paragraphSpacingBefore: (qIdx == 0 && !isFirst) ? 4 : 0,
                            paragraphSpacingAfter: isLastQuotePara ? 0 : (qIdx == quoteBlocks.count - 1 ? 12 : 8),
                            firstLineHeadIndent: 14,
                            headIndent: 14,
                            textColor: palette.textSecondary.nsColor,
                            palette: palette
                        )
                    )
                    if attr.length > 0 {
                        attr.addAttribute(
                            .agentHubBlockQuote,
                            value: true,
                            range: NSRange(location: 0, length: attr.length)
                        )
                        proseBuffer.append(attr)
                        proseBuffer.append(NSAttributedString(string: "\n"))
                    }
                }

            case let .codeBlock(language, code):
                flushProse()
                output.append(.codeBlock(language: language, code: code))

            case let .table(alignments, headers, rows):
                flushProse()
                let headerAttrs = headers.enumerated().map { colIdx, cellInlines in
                    let align = colIdx < alignments.count ? alignments[colIdx] : .none
                    return renderInlines(
                        cellInlines,
                        baseSize: 13,
                        baseWeight: .semibold,
                        lineHeightMultiple: 1.4,
                        paragraphSpacingBefore: 0,
                        paragraphSpacingAfter: 0,
                        headIndent: 0,
                        alignment: nsTextAlignment(for: align),
                        textColor: palette.textPrimary.nsColor,
                        palette: palette
                    )
                }
                let rowAttrs = rows.map { row in
                    row.enumerated().map { colIdx, cellInlines in
                        let align = colIdx < alignments.count ? alignments[colIdx] : .none
                        return renderInlines(
                            cellInlines,
                            baseSize: 13,
                            baseWeight: .regular,
                            lineHeightMultiple: 1.4,
                            paragraphSpacingBefore: 0,
                            paragraphSpacingAfter: 0,
                            headIndent: 0,
                            alignment: nsTextAlignment(for: align),
                            textColor: palette.textPrimary.nsColor,
                            palette: palette
                        )
                    }
                }
                output.append(.table(alignments: alignments, headers: headerAttrs, rows: rowAttrs))

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

    public static func safeRenderableHref(_ raw: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if FileLinksAndPlanExport.fileLinkTarget(trimmed) != nil {
            return trimmed
        }
        if trimmed.hasPrefix("#") {
            return trimmed
        }
        if let comps = URLComponents(string: trimmed),
           let scheme = comps.scheme?.lowercased(),
           ["https", "http", "mailto"].contains(scheme)
        {
            return trimmed
        }
        return nil
    }

    private static func nsTextAlignment(for alignment: GfmAlignment) -> NSTextAlignment {
        switch alignment {
        case .left, .none: .left
        case .center: .center
        case .right: .right
        }
    }

    private static func renderInlines(
        _ inlines: [GfmInline],
        baseSize: CGFloat,
        baseWeight: NSFont.Weight,
        lineHeightMultiple: CGFloat,
        paragraphSpacingBefore: CGFloat,
        paragraphSpacingAfter: CGFloat,
        firstLineHeadIndent: CGFloat? = nil,
        headIndent: CGFloat,
        alignment: NSTextAlignment = .left,
        textColor: NSColor,
        palette: DesignPalette
    ) -> NSAttributedString {
        let para = NSMutableParagraphStyle()
        let exactLineHeight = (baseSize * lineHeightMultiple * 10.0).rounded() / 10.0
        para.minimumLineHeight = exactLineHeight
        para.maximumLineHeight = exactLineHeight
        para.paragraphSpacingBefore = paragraphSpacingBefore
        para.paragraphSpacing = paragraphSpacingAfter
        para.firstLineHeadIndent = firstLineHeadIndent ?? (headIndent > 0 ? max(0, headIndent - 14) : 0)
        para.headIndent = headIndent
        para.alignment = alignment

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
                        // Preserve literal source href string so file paths never invent a web origin
                        attrs[.link] = linkURL
                    }
                    result.append(NSAttributedString(string: str, attributes: attrs))

                case let .code(codeStr):
                    var attrs: [NSAttributedString.Key: Any] = [
                        .font: DesignFonts.nsMonoFont(size: max(12, baseSize - 2), weight: .regular),
                        .foregroundColor: linkURL != nil ? palette.accent.nsColor : palette.textPrimary.nsColor,
                        .agentHubInlineCode: palette.hover.nsColor,
                        .paragraphStyle: para,
                    ]
                    if strike {
                        attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
                    }
                    if let linkURL {
                        attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
                        attrs[.link] = linkURL
                    }
                    result.append(NSAttributedString(string: "\u{202F}\(codeStr)\u{202F}", attributes: attrs))

                case let .strong(children):
                    appendList(children, bold: true, italic: italic, strike: strike, linkURL: linkURL)

                case let .emphasis(children):
                    appendList(children, bold: bold, italic: true, strike: strike, linkURL: linkURL)

                case let .strikethrough(children):
                    appendList(children, bold: bold, italic: italic, strike: true, linkURL: linkURL)

                case let .link(url, _, children):
                    let safeHref = safeRenderableHref(url)
                    appendList(children, bold: bold, italic: italic, strike: strike, linkURL: safeHref)
                    if let safeHref, FileLinksAndPlanExport.fileLinkTarget(safeHref) != nil {
                        result.append(NSAttributedString(string: "\u{202F}", attributes: [
                            .font: DesignFonts.nsBodyFont(size: baseSize, weight: baseWeight),
                            .paragraphStyle: para,
                        ]))
                        let attachment = NSTextAttachment()
                        attachment.image = LucideVectorCatalog.nsImage(
                            for: .copy,
                            size: 13,
                            color: palette.textMuted.nsColor
                        )
                        attachment.bounds = NSRect(x: 0, y: -2, width: 13, height: 13)
                        let iconStr = NSMutableAttributedString(attachment: attachment)
                        iconStr.addAttributes([
                            .link: "agenthub-copy-file:\(safeHref)",
                            .paragraphStyle: para,
                        ], range: NSRange(location: 0, length: iconStr.length))
                        result.append(iconStr)
                    }

                case let .image(url, alt, _):
                    let label = alt.isEmpty ? url : "[\(alt)]"
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
