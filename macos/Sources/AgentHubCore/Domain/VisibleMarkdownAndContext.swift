import Foundation

// MARK: - UTF-16 Range & String Slicing Helpers (100% Parity with JS String Indexing)

public struct UTF16Range: Equatable, Sendable {
    public var start: Int
    public var end: Int

    public init(start: Int, end: Int) {
        self.start = start
        self.end = end
    }
}

extension String {
    func utf16Substring(from start: Int, to end: Int) -> String {
        let ns = self as NSString
        let clampedStart = max(0, min(start, ns.length))
        let clampedEnd = max(clampedStart, min(end, ns.length))
        return ns.substring(with: NSRange(location: clampedStart, length: clampedEnd - clampedStart))
    }
}

// MARK: - VisibleMarkdown & Instruction Segments (visibleMarkdown.ts)

public struct InstructionSegment: Codable, Equatable, Sendable, Identifiable {
    public var kind: String // "markdown" | "instructions"
    public var id: String
    public var text: String
    public var lines: Int
    public var bytes: Int

    public init(kind: String, id: String, text: String, lines: Int, bytes: Int) {
        self.kind = kind
        self.id = id
        self.text = text
        self.lines = lines
        self.bytes = bytes
    }
}

private extension String {
    func matches(_ pattern: NSRegularExpression) -> Bool {
        pattern.firstMatch(in: self, range: NSRange(startIndex..., in: self)) != nil
    }

    func replacing(_ pattern: NSRegularExpression, with template: String) -> String {
        pattern.stringByReplacingMatches(in: self, range: NSRange(startIndex..., in: self), withTemplate: template)
    }
}

private enum MarkdownPatterns {
    static let memoryMarker = try! NSRegularExpression(pattern: "oai-mem-citation|citation_entries|rollout_ids|memory_citation", options: [.caseInsensitive])
    static let headingPrefix = try! NSRegularExpression(pattern: "^#{1,6}\\s+")
    static let closingHashes = try! NSRegularExpression(pattern: "\\s+#+$")
    static let labelPunctuation = try! NSRegularExpression(pattern: "^[`*]+|[`*:]+$")
    static let trailingWhitespace = try! NSRegularExpression(pattern: "\\s+$")
    static let finalNewline = try! NSRegularExpression(pattern: "\\r?\\n$")
    static let leadingHashes = try! NSRegularExpression(pattern: "^#+\\s*")
    static let codeDelimiters = try! NSRegularExpression(pattern: "^`|`$")
    static let urlScheme = try! NSRegularExpression(pattern: "^[a-z][a-z0-9+.-]*:", options: [.caseInsensitive])
    static let explicitRelativePath = try! NSRegularExpression(pattern: "^(?:/|\\.{1,2}/|~/ )")
    static let fileName = try! NSRegularExpression(pattern: "^[^?#]+(?:/|\\.[\\p{L}\\d_-]+(?:[:#]|$))")
    static let lineFragment = try! NSRegularExpression(pattern: "#L\\d+(?:C\\d+)?(?:-L?C?\\d+(?:C\\d+)?)?$", options: [.caseInsensitive])
    static let lineSuffix = try! NSRegularExpression(pattern: ":\\d+(?::\\d+)?$")
    static let markdownLink = try! NSRegularExpression(pattern: "\\[([^\\]]+)\\]\\([^)]*\\)")
    static let htmlTag = try! NSRegularExpression(pattern: "<[^>]*>")
    static let emphasis = try! NSRegularExpression(pattern: "[*_`~]+")
    static let illegalFileCharacters = try! NSRegularExpression(pattern: "[\\x00-\\x1f\\x7f<>:\"/\\\\|?*]+")
    static let whitespace = try! NSRegularExpression(pattern: "\\s+")
    static let fileNameEdges = try! NSRegularExpression(pattern: "^[.\\s]+|[.\\s]+$")
    static let markdownExtension = try! NSRegularExpression(pattern: "\\.md$", options: [.caseInsensitive])
    static let fileNameEnd = try! NSRegularExpression(pattern: "[.\\s]+$")
    static let reservedFileName = try! NSRegularExpression(pattern: "^(con|prn|aux|nul|com[1-9]|lpt[1-9])$", options: [.caseInsensitive])
    static let instructionLabels = [
        "^(?:[a-z]:[/\\\\]|[/\\\\]|\\.{1,2}[/\\\\]|~[/\\\\])(?:[^<>\\n]*[/\\\\])?agents\\.md$",
        "^(?:[\\w.-]+[/\\\\])+agents\\.md$",
        "^agents\\.md(?:\\s+instructions(?:\\s+for\\s+.+)?)?$",
    ].map { try! NSRegularExpression(pattern: $0, options: [.caseInsensitive]) }
    static let quoted = try! NSRegularExpression(pattern: "^( {4}| *\\t| {0,3}>)")
    static let indented = try! NSRegularExpression(pattern: "^( {4}| *\\t)")
    static let lines = try! NSRegularExpression(pattern: "[^\\n]*\\n|[^\\n]+$")
    static let fence = try! NSRegularExpression(pattern: "^ {0,3}(`{3,}|~{3,})(.*)")
    static let comment = try! NSRegularExpression(pattern: "<!--[\\s\\S]*?(?:-->|$)")
    static let memoryTag = try! NSRegularExpression(
        pattern: "(?:<|&lt;)(\\/?)(oai-mem-citation|citation_entries|rollout_ids)(?:\\s+[^<>]*?)?\\s*(?:>|&gt;)",
        options: [.caseInsensitive]
    )
    static let memoryField = try! NSRegularExpression(
        pattern: "^[ \\t]*(?:[\"']memory_citation[\"']|memory_citation)[ \\t]*[:=][ \\t]*",
        options: [.caseInsensitive, .anchorsMatchLines]
    )
    static let pageMetadata = try! NSRegularExpression(pattern: "<external_codex_apps_open_page>([\\s\\S]*?)<\\/external_codex_apps_open_page>")
    static let selectedPageSnapshotMetadata = try! NSRegularExpression(
        pattern: "<external_codex_apps_selected_page_part_(\\d+)_of_(\\d+)>Companion Page snapshot part (\\d+) of (\\d+):([\\s\\S]*?)<\\/external_codex_apps_selected_page_part_(\\d+)_of_(\\d+)>"
    )
    static let instructionFile = try! NSRegularExpression(
        pattern: "(?:^|[\\s/\\\\`\"'(=:])agents\\.md(?=$|[\\s`\"'),:;\\]}>])",
        options: [.caseInsensitive]
    )
    static let instructionLines = try! NSRegularExpression(pattern: "[^\\n]*(?:\\n|$)")
    static let heading = try! NSRegularExpression(pattern: "^ {0,3}(#{1,6})\\s")
    static let openInstructions = try! NSRegularExpression(pattern: "^\\s*<INSTRUCTIONS>[ \\t]*(?:\\r?\\n|$)", options: [.caseInsensitive])
    static let closeInstructions = try! NSRegularExpression(pattern: "<\\/INSTRUCTIONS\\s*>", options: [.caseInsensitive])
    static let ordinaryTag = try! NSRegularExpression(pattern: "<([a-z][a-z0-9_:-]*)", options: [.caseInsensitive])
    static let replyEnvelope = try! NSRegularExpression(
        pattern: "^[ \\t]{0,3}<send_user_message_question_reply>([\\s\\S]*?)<\\/send_user_message_question_reply>[ \\t]*(?=\\r?$)",
        options: [.anchorsMatchLines]
    )
    static let fileHeader = try! NSRegularExpression(pattern: "# Files mentioned by the user:\\r?\\n")
    static let fileLine = try! NSRegularExpression(pattern: "^## .+: .+$")
    static let agentsPath = try! NSRegularExpression(pattern: "^[^\\s]+[/\\\\]agents\\.md$")
    static let planHeading = try! NSRegularExpression(pattern: "^ {0,3}#\\s+(.+?)\\s*#*\\s*$", options: [.anchorsMatchLines])
    static let fileLinkSpan = try! NSRegularExpression(pattern: "(?:\\[([^\\]\\n]+)\\]\\(([^)\\s]+)\\))|(?:<(file:[^>\\s]+)>)")
}

public enum VisibleMarkdownEngine {
    private static let utf8MemoryMarkers: [[UInt8]] = [
        Array("oai-mem-citation".utf8),
        Array("citation_entries".utf8),
        Array("rollout_ids".utf8),
        Array("memory_citation".utf8),
    ]
    private static let utf8PageMarker: [UInt8] = Array("external_codex_apps_".utf8)

    private static func containsASCIICaseInsensitiveBytes(_ buf: UnsafeBufferPointer<UInt8>, needleLower: [UInt8]) -> Bool {
        let nLen = needleLower.count
        guard nLen > 0, buf.count >= nLen else { return false }
        let firstLower = needleLower[0]
        let firstUpper = (firstLower >= 0x61 && firstLower <= 0x7A) ? (firstLower - 32) : firstLower
        let maxStart = buf.count - nLen
        var i = 0
        while i <= maxStart {
            let b = buf[i]
            if b == firstLower || b == firstUpper {
                var match = true
                var k = 1
                while k < nLen {
                    let hb = buf[i + k]
                    let lowerHb = (hb >= 0x41 && hb <= 0x5A) ? (hb + 32) : hb
                    if lowerHb != needleLower[k] {
                        match = false
                        break
                    }
                    k += 1
                }
                if match { return true }
            }
            i += 1
        }
        return false
    }

    private static func withUTF8Buffer(_ text: String, _ body: (UnsafeBufferPointer<UInt8>) -> Bool) -> Bool {
        if let res = text.utf8.withContiguousStorageIfAvailable(body) {
            return res
        }
        var copy = text
        return copy.withUTF8(body)
    }

    private static func mightContainMemoryMetadata(_ text: String) -> Bool {
        withUTF8Buffer(text) { buf in
            guard buf.count >= 11 else { return false }
            for marker in utf8MemoryMarkers {
                if containsASCIICaseInsensitiveBytes(buf, needleLower: marker) {
                    return true
                }
            }
            return false
        }
    }

    private static func mightContainAnyHiddenMetadata(_ text: String) -> Bool {
        withUTF8Buffer(text) { buf in
            guard buf.count >= 11 else { return false }
            if containsASCIICaseInsensitiveBytes(buf, needleLower: utf8PageMarker) {
                return true
            }
            for marker in utf8MemoryMarkers {
                if containsASCIICaseInsensitiveBytes(buf, needleLower: marker) {
                    return true
                }
            }
            return false
        }
    }

    public static func literalMarkdownRanges(_ text: String, includeInline: Bool = true, includeQuotes: Bool = true) -> [UTF16Range] {
        let ns = text as NSString
        let total = ns.length
        if total == 0 { return [] }

        var ranges: [UTF16Range] = []
        let lineRegex = MarkdownPatterns.lines
        let fenceRegex = MarkdownPatterns.fence
        let indentedOrQuoteRegex = includeQuotes ? MarkdownPatterns.quoted : MarkdownPatterns.indented

        var fence: (marker: unichar, count: Int)?
        for match in lineRegex.matches(in: text, range: NSRange(location: 0, length: total)) {
            let lineRange = match.range
            let line = ns.substring(with: lineRange)
            let lineNS = line as NSString
            let markerMatch = fenceRegex.firstMatch(in: line, range: NSRange(location: 0, length: lineNS.length))
            let wasFenced = (fence != nil)

            if let activeFence = fence {
                if let m = markerMatch {
                    let markerStr = lineNS.substring(with: m.range(at: 1)) as NSString
                    let rest = lineNS.substring(with: m.range(at: 2)).trimmingCharacters(in: .whitespacesAndNewlines)
                    if markerStr.length >= activeFence.count && markerStr.character(at: 0) == activeFence.marker && rest.isEmpty {
                        fence = nil
                    }
                }
            } else if let m = markerMatch {
                let markerStr = lineNS.substring(with: m.range(at: 1)) as NSString
                fence = (marker: markerStr.character(at: 0), count: markerStr.length)
            }

            let isIndentedOrQuote = indentedOrQuoteRegex.firstMatch(in: line, range: NSRange(location: 0, length: lineNS.length)) != nil
            if wasFenced || markerMatch != nil || isIndentedOrQuote {
                ranges.append(UTF16Range(start: lineRange.location, end: lineRange.location + lineRange.length))
            }
        }

        ranges = mergeRanges(ranges)

        if includeInline {
            var inlineRanges: [UTF16Range] = []
            var cursor = 0
            let backtick: unichar = 0x60 // '`'
            while cursor < total {
                if let prot = protectedRange(ranges, at: cursor) {
                    cursor = prot.end
                    continue
                }
                if ns.character(at: cursor) != backtick {
                    cursor += 1
                    continue
                }
                var run = 1
                while cursor + run < total && ns.character(at: cursor + run) == backtick {
                    run += 1
                }
                var end = cursor + run
                var matched = false
                while end < total && protectedRange(ranges, at: end) == nil {
                    if ns.character(at: end) != backtick {
                        end += 1
                        continue
                    }
                    var next = 1
                    while end + next < total && ns.character(at: end + next) == backtick {
                        next += 1
                    }
                    if next == run {
                        inlineRanges.append(UTF16Range(start: cursor, end: end + next))
                        cursor = end + next
                        matched = true
                        break
                    }
                    end += next
                }
                if !matched {
                    cursor += run
                }
            }
            ranges.append(contentsOf: inlineRanges)
        }

        let commentRegex = MarkdownPatterns.comment
        for match in commentRegex.matches(in: text, range: NSRange(location: 0, length: total)) {
            ranges.append(UTF16Range(start: match.range.location, end: match.range.location + match.range.length))
        }

        return mergeRanges(ranges)
    }

    public static func memoryMetadataRanges(_ text: String, protectedRanges: [UTF16Range]? = nil) -> [UTF16Range] {
        guard mightContainMemoryMetadata(text) else {
            return []
        }
        let ns = text as NSString
        let total = ns.length

        if let data = text.data(using: .utf8),
           let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           json.count == 1,
           json["memory_citation"] != nil
        {
            return [UTF16Range(start: 0, end: total)]
        }

        let code = protectedRanges ?? literalMarkdownRanges(text, includeInline: true, includeQuotes: false)
        let isProtected = { (offset: Int) in
            code.contains { $0.start <= offset && offset < $0.end }
        }

        var ranges: [UTF16Range] = []
        var stack: [(name: String, start: Int)] = []
        let tagRegex = MarkdownPatterns.memoryTag
        for match in tagRegex.matches(in: text, range: NSRange(location: 0, length: total)) {
            let start = match.range.location
            let end = start + match.range.length
            if isProtected(start) && stack.isEmpty { continue }
            let slash = ns.substring(with: match.range(at: 1))
            let name = ns.substring(with: match.range(at: 2)).lowercased()
            if slash.isEmpty {
                stack.append((name: name, start: start))
            } else if let idx = stack.lastIndex(where: { $0.name == name }) {
                ranges.append(UTF16Range(start: stack[idx].start, end: end))
                stack.removeSubrange(idx...)
            } else {
                ranges.append(UTF16Range(start: start, end: end))
            }
        }
        for item in stack {
            ranges.append(UTF16Range(start: item.start, end: total))
        }

        let fieldRegex = MarkdownPatterns.memoryField
        for match in fieldRegex.matches(in: text, range: NSRange(location: 0, length: total)) {
            let start = match.range.location
            let matchText = ns.substring(with: match.range) as NSString
            var firstNonSpace = 0
            while firstNonSpace < matchText.length && (matchText.character(at: firstNonSpace) == 0x20 || matchText.character(at: firstNonSpace) == 0x09) {
                firstNonSpace += 1
            }
            if isProtected(start + firstNonSpace) || ranges.contains(where: { $0.start <= start && start < $0.end }) {
                continue
            }
            var end = start + match.range.length
            var depth = 0
            var quote: unichar = 0
            var escaped = false
            while end < total {
                let ch = ns.character(at: end)
                if escaped {
                    escaped = false
                    end += 1
                    continue
                }
                if quote != 0 {
                    if ch == 0x5C { escaped = true }
                    else if ch == quote { quote = 0 }
                } else if (ch == 0x22 || ch == 0x27) && (depth > 0 || end == start + match.range.length) {
                    quote = ch
                } else if ch == 0x7B || ch == 0x5B {
                    depth += 1
                } else if (ch == 0x7D || ch == 0x5D) && depth > 0 {
                    depth -= 1
                } else if ch == 0x0A && depth == 0 {
                    break
                }
                end += 1
            }
            ranges.append(UTF16Range(start: start, end: end))
        }

        return ranges.sorted { $0.start != $1.start ? $0.start < $1.start : $0.end > $1.end }
    }

    private static func pageMetadataRanges(_ text: String, protectedRanges: [UTF16Range]? = nil) -> [UTF16Range] {
        guard text.contains("external_codex_apps_") else { return [] }
        let ns = text as NSString
        let fullRange = NSRange(location: 0, length: ns.length)
        let literal = protectedRanges ?? literalMarkdownRanges(text, includeInline: true, includeQuotes: true)
        var ranges: [UTF16Range] = []

        if text.contains("external_codex_apps_open_page") {
            let regex = MarkdownPatterns.pageMetadata
            for match in regex.matches(in: text, range: fullRange) {
                let start = match.range.location
                let end = start + match.range.length
                if literal.contains(where: { $0.start < end && start < $0.end }) { continue }
                let inner = ns.substring(with: match.range(at: 1))
                guard let data = inner.data(using: .utf8),
                      let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                      obj.count == 1,
                      let val = obj["page_id"],
                      (val is NSNull || val is String)
                else { continue }
                ranges.append(UTF16Range(start: start, end: end))
            }
        }

        if text.contains("external_codex_apps_selected_page_part_") {
            let snapRegex = MarkdownPatterns.selectedPageSnapshotMetadata
            for match in snapRegex.matches(in: text, range: fullRange) {
                let start = match.range.location
                let end = start + match.range.length
                if literal.contains(where: { $0.start < end && start < $0.end }) { continue }
                let openPart = ns.substring(with: match.range(at: 1))
                let openTotal = ns.substring(with: match.range(at: 2))
                let headPart = ns.substring(with: match.range(at: 3))
                let headTotal = ns.substring(with: match.range(at: 4))
                let closePart = ns.substring(with: match.range(at: 6))
                let closeTotal = ns.substring(with: match.range(at: 7))
                guard openPart == headPart,
                      openPart == closePart,
                      openTotal == headTotal,
                      openTotal == closeTotal,
                      let partNum = Int(openPart),
                      let totalNum = Int(openTotal),
                      partNum >= 1,
                      totalNum >= partNum
                else { continue }
                let jsonStr = ns.substring(with: match.range(at: 5)).trimmingCharacters(in: .whitespacesAndNewlines)
                guard let data = jsonStr.data(using: .utf8),
                      let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                      obj["document_view"] != nil,
                      let pageVal = obj["page_id"],
                      (pageVal is NSNull || pageVal is String)
                else { continue }
                ranges.append(UTF16Range(start: start, end: end))
            }
        }
        return ranges
    }

    public static func visibleMarkdown(_ text: String, protectedRanges: [UTF16Range]? = nil) -> String {
        guard mightContainAnyHiddenMetadata(text) else { return text }
        let ranges = (memoryMetadataRanges(text, protectedRanges: protectedRanges) + pageMetadataRanges(text, protectedRanges: protectedRanges))
            .sorted { $0.start != $1.start ? $0.start < $1.start : $0.end > $1.end }
        guard !ranges.isEmpty else { return text }
        let ns = text as NSString
        var result = ""
        var cursor = 0
        for r in ranges {
            if r.start > cursor {
                result += ns.substring(with: NSRange(location: cursor, length: r.start - cursor))
            }
            cursor = max(cursor, r.end)
        }
        if cursor < ns.length {
            result += ns.substring(with: NSRange(location: cursor, length: ns.length - cursor))
        }
        return result.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public static func isInstructionFileActivity(_ values: String?...) -> Bool {
        isInstructionFileActivity(values)
    }

    public static func isInstructionFileActivity(_ values: [String?]) -> Bool {
        let regex = MarkdownPatterns.instructionFile
        return values.contains { value in
            guard let value, !value.isEmpty else { return false }
            let ns = value as NSString
            return regex.firstMatch(in: value, range: NSRange(location: 0, length: ns.length)) != nil
        }
    }

    private static func isInstructionLabel(_ line: String) -> Bool {
        var label = line.trimmingCharacters(in: .whitespacesAndNewlines)
        label = label.replacing(MarkdownPatterns.headingPrefix, with: "")
        label = label.replacing(MarkdownPatterns.closingHashes, with: "")
        label = label.replacing(MarkdownPatterns.labelPunctuation, with: "")
        let ns = label as NSString
        return MarkdownPatterns.instructionLabels.contains { re in
            return re.firstMatch(in: label, range: NSRange(location: 0, length: ns.length)) != nil
        }
    }

    public static func instructionSegments(_ text: String) -> [InstructionSegment] {
        let ns = text as NSString
        let total = ns.length
        if text.range(of: "agents.md", options: .caseInsensitive) == nil {
            return [InstructionSegment(
                kind: "markdown",
                id: "text",
                text: text,
                lines: countLines(text),
                bytes: text.utf8.count
            )]
        }

        let code = literalMarkdownRanges(text, includeInline: false, includeQuotes: true)
        let lineRegex = MarkdownPatterns.instructionLines
        let allLines = lineRegex.matches(in: text, range: NSRange(location: 0, length: total)).compactMap { m -> (index: Int, text: String)? in
            guard m.range.length > 0 else { return nil }
            return (index: m.range.location, text: ns.substring(with: m.range))
        }
        let markers = allLines.filter { line in
            !code.contains(where: { $0.start <= line.index && line.index < $0.end }) && isInstructionLabel(line.text)
        }
        if markers.isEmpty {
            return [InstructionSegment(
                kind: "markdown",
                id: "text",
                text: text,
                lines: countLines(text),
                bytes: text.utf8.count
            )]
        }

        var result: [InstructionSegment] = []
        let add: (String, Int, Int) -> Void = { kind, start, end in
            guard end > start else { return }
            let val = text.utf16Substring(from: start, to: end)
            guard !val.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
            let trimmedEnd = val.replacing(MarkdownPatterns.trailingWhitespace, with: "")
            result.append(InstructionSegment(
                kind: kind,
                id: "\(kind):\(start)",
                text: val,
                lines: countLines(trimmedEnd),
                bytes: val.utf8.count
            ))
        }

        var cursor = 0
        let headingRegex = MarkdownPatterns.heading
        let openInstructionsRegex = MarkdownPatterns.openInstructions
        let closeInstructionsRegex = MarkdownPatterns.closeInstructions

        for marker in markers {
            let start = marker.index
            if start < cursor { continue }
            add("markdown", cursor, start)

            let markerNS = marker.text as NSString
            let depth = headingRegex.firstMatch(in: marker.text, range: NSRange(location: 0, length: markerNS.length))
                .map { $0.range(at: 1).length } ?? 0

            let nextLine = allLines.first { line in
                guard line.index > start,
                      !code.contains(where: { $0.start <= line.index && line.index < $0.end })
                else { return false }
                let lineNS = line.text as NSString
                let lineDepth = headingRegex.firstMatch(in: line.text, range: NSRange(location: 0, length: lineNS.length))
                    .map { $0.range(at: 1).length } ?? 7
                return lineDepth <= depth || isInstructionLabel(line.text)
            }
            var end = nextLine?.index ?? total

            let bodyStart = start + markerNS.length
            let tail = text.utf16Substring(from: bodyStart, to: total)
            let tailNS = tail as NSString
            if openInstructionsRegex.firstMatch(in: tail, range: NSRange(location: 0, length: tailNS.length)) != nil {
                let literal = literalMarkdownRanges(text, includeInline: true, includeQuotes: true)
                let closes = closeInstructionsRegex.matches(in: text, range: NSRange(location: 0, length: total))
                if let close = closes.first(where: { m in
                    m.range.location >= bodyStart && !literal.contains(where: { $0.start <= m.range.location && m.range.location < $0.end })
                }) {
                    end = close.range.location + close.range.length
                }
            }

            add("instructions", start, end)
            cursor = end
        }
        add("markdown", cursor, total)
        return result
    }

    private static func countLines(_ s: String) -> Int {
        var count = 1
        let ns = s as NSString
        var i = 0
        while i < ns.length {
            if ns.character(at: i) == 0x0A { count += 1 }
            i += 1
        }
        return count
    }

    fileprivate static func mergeRanges(_ input: [UTF16Range]) -> [UTF16Range] {
        let sorted = input.sorted { $0.start != $1.start ? $0.start < $1.start : $0.end < $1.end }
        var out: [UTF16Range] = []
        for r in sorted {
            if let last = out.last, last.end >= r.start {
                out[out.count - 1].end = max(last.end, r.end)
            } else {
                out.append(r)
            }
        }
        return out
    }

    fileprivate static func protectedRange(_ ranges: [UTF16Range], at offset: Int) -> UTF16Range? {
        var low = 0
        var high = ranges.count
        while low < high {
            let mid = (low + high) / 2
            if ranges[mid].end <= offset { low = mid + 1 }
            else { high = mid }
        }
        guard low < ranges.count, ranges[low].start <= offset else { return nil }
        return ranges[low]
    }
}

// MARK: - UserContext Projection & UserMessageSegments (userContext.ts & userMessageViewModel.ts)

public struct UserMessageProjection: Codable, Equatable, Sendable {
    public var text: String
    public var contextBlocks: [UserContextBlock]

    public init(text: String, contextBlocks: [UserContextBlock] = []) {
        self.text = text
        self.contextBlocks = contextBlocks
    }
}

public enum UserMessageSegment: Equatable, Sendable, Identifiable {
    case text(id: String, text: String)
    case reply(id: String, question: String, answer: String)

    public var id: String {
        switch self {
        case let .text(id, _): id
        case let .reply(id, _, _): id
        }
    }
}

public enum UserContextEngine {
    private static let knownTags: [(name: String, kind: String)] = [
        ("local-command-stdout", "runtime"),
        ("local-command-stderr", "runtime"),
        ("local-command-caveat", "runtime"),
        ("task-notification", "runtime"),
        ("system-reminder", "runtime"),
        ("system_reminder", "runtime"),
        ("environment_context", "environment"),
        ("permissions instructions", "runtime"),
        ("app-context", "runtime"),
        ("collaboration_mode", "runtime"),
        ("skills_instructions", "runtime"),
        ("plugins_instructions", "runtime"),
        ("subagent_notification", "runtime"),
        ("subagent_context", "runtime"),
        ("codex_internal_context", "runtime"),
        ("goal_context", "runtime"),
        ("additional_context", "runtime"),
        ("user_instructions", "instructions"),
        ("legacy_unified_exec_process_limit_warning", "runtime"),
        ("legacy_apply_patch_exec_command_warning", "runtime"),
        ("legacy_model_mismatch_warning", "runtime"),
    ]

    private struct ParsedUserMessageCacheKey: Hashable {
        let utf8Count: Int
        let text: String
        let contextBlocks: [UserContextBlock]?
    }

    private struct ParsedUserMessageCacheEntry {
        let projection: UserMessageProjection
        let segments: [UserMessageSegment]
        let byteWeight: Int
        var seq: UInt64
    }

    private static let cacheLock = NSLock()
    nonisolated(unsafe) private static var parsedCache: [ParsedUserMessageCacheKey: ParsedUserMessageCacheEntry] = [:]
    nonisolated(unsafe) private static var parsedCacheTotalBytes: Int = 0
    nonisolated(unsafe) private static var parsedCacheSeq: UInt64 = 0
    private static let maxParsedCacheEntries = 256
    private static let maxParsedCacheEntryBytes = 262_144
    private static let maxParsedCacheTotalBytes = 8 * 1_048_576
    private static let fileHeaderPrefix = "# Files mentioned by the user:"
    private static let fileHeaderPrefixLength = (fileHeaderPrefix as NSString).length

    private static func contextBlocksByteCount(_ blocks: [UserContextBlock]?) -> Int {
        guard let blocks else { return 0 }
        var total = 0
        for block in blocks {
            total += block.kind.utf8.count + block.text.utf8.count + 32
        }
        return total
    }

    private static func segmentsByteCount(_ segments: [UserMessageSegment]) -> Int {
        var total = 0
        for segment in segments {
            switch segment {
            case let .text(id, s):
                total += id.utf8.count + s.utf8.count + 16
            case let .reply(id, question, answer):
                total += id.utf8.count + question.utf8.count + answer.utf8.count + 32
            }
        }
        return total
    }

    public static func _parsedCacheStatsForTesting() -> (count: Int, totalBytes: Int) {
        cacheLock.lock()
        defer { cacheLock.unlock() }
        return (parsedCache.count, parsedCacheTotalBytes)
    }

    public static func _resetParsedCacheForTesting() {
        cacheLock.lock()
        parsedCache.removeAll()
        parsedCacheTotalBytes = 0
        parsedCacheSeq = 0
        cacheLock.unlock()
    }

    public static func visibleContextText(_ text: String) -> String {
        let ranges = VisibleMarkdownEngine.literalMarkdownRanges(text, includeInline: true, includeQuotes: true)
        return VisibleMarkdownEngine.visibleMarkdown(text, protectedRanges: ranges)
    }

    public static func projectAndSegmentUserMessage(
        _ text: String,
        contextBlocks: [UserContextBlock]? = nil
    ) -> (projection: UserMessageProjection, segments: [UserMessageSegment]) {
        let inputContextBytes = contextBlocksByteCount(contextBlocks)
        let inputBytes = text.utf8.count + inputContextBytes
        let key = ParsedUserMessageCacheKey(utf8Count: text.utf8.count, text: text, contextBlocks: contextBlocks)
        if inputBytes <= maxParsedCacheEntryBytes {
            cacheLock.lock()
            if var hit = parsedCache[key] {
                parsedCacheSeq &+= 1
                hit.seq = parsedCacheSeq
                parsedCache[key] = hit
                cacheLock.unlock()
                return (hit.projection, hit.segments)
            }
            cacheLock.unlock()
        }

        let proj = computeProjectUserMessage(text, contextBlocks: contextBlocks)
        let segs = computeUserMessageSegments(fromProjectedText: proj.text)

        let projectedBytes = proj.text.utf8.count
            + contextBlocksByteCount(proj.contextBlocks)
            + segmentsByteCount(segs)
        let entryBytes = inputBytes + projectedBytes + 128
        if inputBytes <= maxParsedCacheEntryBytes
            && projectedBytes <= maxParsedCacheEntryBytes
            && entryBytes <= maxParsedCacheTotalBytes
        {
            cacheLock.lock()
            if let existing = parsedCache.removeValue(forKey: key) {
                parsedCacheTotalBytes = max(0, parsedCacheTotalBytes - existing.byteWeight)
            }
            while !parsedCache.isEmpty
                && (parsedCache.count >= maxParsedCacheEntries
                    || parsedCacheTotalBytes + entryBytes > maxParsedCacheTotalBytes),
                let oldestKey = parsedCache.min(by: { $0.value.seq < $1.value.seq })?.key
            {
                if let removed = parsedCache.removeValue(forKey: oldestKey) {
                    parsedCacheTotalBytes = max(0, parsedCacheTotalBytes - removed.byteWeight)
                }
            }
            parsedCacheSeq &+= 1
            parsedCache[key] = ParsedUserMessageCacheEntry(
                projection: proj,
                segments: segs,
                byteWeight: entryBytes,
                seq: parsedCacheSeq
            )
            parsedCacheTotalBytes += entryBytes
            cacheLock.unlock()
        }
        return (proj, segs)
    }

    public static func projectUserMessage(_ text: String, contextBlocks: [UserContextBlock]? = nil) -> UserMessageProjection {
        projectAndSegmentUserMessage(text, contextBlocks: contextBlocks).projection
    }

    private static func computeProjectUserMessage(_ text: String, contextBlocks: [UserContextBlock]? = nil) -> UserMessageProjection {
        if let contextBlocks {
            return UserMessageProjection(text: text, contextBlocks: contextBlocks)
        }
        if !text.contains("<") && !text.contains(fileHeaderPrefix) {
            return UserMessageProjection(text: text, contextBlocks: [])
        }

        let ns = text as NSString
        let total = ns.length
        let ranges = VisibleMarkdownEngine.literalMarkdownRanges(text, includeInline: true, includeQuotes: true)
        let lower = asciiLowercased(ns)
        let lowerNS = lower as NSString

        var blocks: [UserContextBlock] = []
        var bodyParts: [String] = []
        var cursor = 0
        var kept = 0
        var adjacent = false
        var lineStart = 0

        let substring: (Int, Int) -> String = { start, end in
            let s = max(0, min(start, total))
            let e = max(s, min(end, total))
            return s < e ? ns.substring(with: NSRange(location: s, length: e - s)) : ""
        }

        let advance: (Int) -> Void = { end in
            if end > cursor {
                let nl = ns.range(of: "\n", options: .backwards, range: NSRange(location: cursor, length: end - cursor))
                if nl.location != NSNotFound {
                    lineStart = nl.location + 1
                }
            }
            cursor = end
        }

        let ordinaryTagRegex = MarkdownPatterns.ordinaryTag

        while cursor < total {
            if let literal = VisibleMarkdownEngine.protectedRange(ranges, at: cursor) {
                advance(literal.end)
                adjacent = false
                continue
            }
            var boundary = (cursor - lineStart <= 3)
            if boundary && cursor > lineStart {
                for idx in lineStart ..< cursor where ns.character(at: idx) != 0x20 {
                    boundary = false
                    break
                }
            }

            if adjacent || boundary {
                let isFileHeader = (cursor + fileHeaderPrefixLength <= total)
                    && ns.compare(
                        fileHeaderPrefix,
                        options: [.literal],
                        range: NSRange(location: cursor, length: fileHeaderPrefixLength)
                    ) == .orderedSame
                if let fileEnd = isFileHeader ? fileHeaderEnd(text: text, ns: ns, start: cursor) : nil {
                    var onlyLeadingNewlines = (kept == 0)
                    if onlyLeadingNewlines {
                        for idx in 0 ..< cursor {
                            let cu = ns.character(at: idx)
                            if cu != 0x0D && cu != 0x0A {
                                onlyLeadingNewlines = false
                                break
                            }
                        }
                    }
                    let contextStart = onlyLeadingNewlines ? 0 : cursor
                    bodyParts.append(substring(kept, contextStart))
                    blocks.append(UserContextBlock(kind: "files", text: substring(contextStart, fileEnd)))
                    advance(fileEnd)
                    kept = fileEnd
                    adjacent = true
                    continue
                }
                if isFileHeader { break }

                var matched: (name: String, kind: String, end: Int)?
                if let instEnd = instructionEnd(ns: ns, lowerNS: lowerNS, start: cursor) {
                    matched = ("instructions", "instructions", instEnd)
                } else if ns.character(at: cursor) == 0x3C {
                    for (name, kind) in knownTags {
                        if let openEnd = openingEnd(lowerNS: lowerNS, start: cursor, name: name) {
                            matched = (name, kind, openEnd)
                            break
                        }
                    }
                }

                if let matched {
                    guard let closeEnd = closingEnd(lowerNS: lowerNS, cursor: matched.end, name: matched.name, ranges: ranges) else {
                        break
                    }
                    bodyParts.append(substring(kept, cursor))
                    blocks.append(UserContextBlock(kind: matched.kind, text: substring(cursor, closeEnd)))
                    advance(closeEnd)
                    kept = closeEnd
                    adjacent = true
                    continue
                }

                if ns.character(at: cursor) == 0x3C,
                   let m = ordinaryTagRegex.firstMatch(
                       in: text,
                       options: [.anchored],
                       range: NSRange(location: cursor, length: min(total - cursor, 80))
                   )
                {
                    let tagName = ns.substring(with: m.range(at: 1)).lowercased()
                    if let openEnd = openingEnd(lowerNS: lowerNS, start: cursor, name: tagName) {
                        guard let closeEnd = closingEnd(lowerNS: lowerNS, cursor: openEnd, name: tagName, ranges: ranges) else {
                            break
                        }
                        advance(closeEnd)
                        adjacent = false
                        continue
                    }
                    break
                }
            }

            let ch = ns.character(at: cursor)
            if !isWhitespaceCodeUnit(ch) {
                adjacent = false
            }
            if ch == 0x0A {
                lineStart = cursor + 1
            }
            cursor += 1
        }

        if kept == 0 && bodyParts.isEmpty {
            return UserMessageProjection(text: text, contextBlocks: blocks)
        }
        bodyParts.append(substring(kept, total))
        return UserMessageProjection(text: bodyParts.joined(), contextBlocks: blocks)
    }

    public static func userMessageSegments(_ text: String, contextBlocks: [UserContextBlock]? = nil) -> [UserMessageSegment] {
        projectAndSegmentUserMessage(text, contextBlocks: contextBlocks).segments
    }

    private static func computeUserMessageSegments(fromProjectedText projected: String) -> [UserMessageSegment] {
        let ns = projected as NSString
        let total = ns.length
        var result: [UserMessageSegment] = []
        var occurrences: [String: Int] = [:]

        let uniqueId: (String) -> String = { key in
            let count = occurrences[key, default: 0]
            occurrences[key] = count + 1
            return "\(key):\(count)"
        }

        let hasReplyTag = projected.contains("<send_user_message_question_reply>")
        let protectedRanges = hasReplyTag
            ? VisibleMarkdownEngine.literalMarkdownRanges(projected, includeInline: true, includeQuotes: true)
            : []
        let envelopeRegex = MarkdownPatterns.replyEnvelope

        var cursor = 0
        let addText: (Int) -> Void = { end in
            guard end > cursor else { return }
            let slice = (cursor == 0 && end == total)
                ? projected
                : ns.substring(with: NSRange(location: cursor, length: end - cursor))
            let cleaned = VisibleMarkdownEngine.visibleMarkdown(slice)
            if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                result.append(.text(id: "text:\(cursor)", text: cleaned))
            }
        }

        guard hasReplyTag else {
            addText(total)
            return result
        }

        struct ReplyEnvelopeItem: Decodable {
            let questionItemId: String
            let question: String
            let answer: String
        }

        for match in envelopeRegex.matches(in: projected, range: NSRange(location: 0, length: total)) {
            let start = match.range.location
            if protectedRanges.contains(where: { $0.start <= start && start < $0.end }) { continue }
            let rawJson = ns.substring(with: match.range(at: 1))
            guard let data = rawJson.data(using: .utf8),
                  let replies = try? JSONDecoder().decode([ReplyEnvelopeItem].self, from: data),
                  !replies.isEmpty,
                  replies.allSatisfy({ !$0.questionItemId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty })
            else { continue }

            addText(start)
            for reply in replies {
                result.append(.reply(
                    id: uniqueId("reply:\(reply.questionItemId)"),
                    question: VisibleMarkdownEngine.visibleMarkdown(reply.question),
                    answer: VisibleMarkdownEngine.visibleMarkdown(reply.answer)
                ))
            }
            cursor = start + match.range.length
        }
        addText(total)
        return result
    }

    private static func asciiLowercased(_ ns: NSString) -> String {
        var units = [unichar](repeating: 0, count: ns.length)
        ns.getCharacters(&units, range: NSRange(location: 0, length: ns.length))
        for i in 0 ..< units.count {
            if units[i] >= 0x41 && units[i] <= 0x5A {
                units[i] += 32
            }
        }
        return String(utf16CodeUnits: units, count: units.count)
    }

    private static func isWhitespaceCodeUnit(_ ch: unichar) -> Bool {
        ch == 0x20 || ch == 0x09 || ch == 0x0A || ch == 0x0D || ch == 0x0B || ch == 0x0C
    }

    private static func fileHeaderEnd(text: String, ns: NSString, start: Int) -> Int? {
        let total = ns.length
        let headerRegex = MarkdownPatterns.fileHeader
        guard let m = headerRegex.firstMatch(
            in: text,
            options: [.anchored],
            range: NSRange(location: start, length: min(total - start, 64))
        ) else {
            return nil
        }
        var cursor = start + m.range.length
        var files = 0
        var disclaimer = false
        let fileLineRegex = MarkdownPatterns.fileLine

        while cursor < total {
            let nl = ns.range(of: "\n", options: [], range: NSRange(location: cursor, length: total - cursor))
            let end = nl.location == NSNotFound ? total : nl.location + 1
            let raw = ns.substring(with: NSRange(location: cursor, length: end - cursor))
                .replacing(MarkdownPatterns.finalNewline, with: "")
            if raw == "## My request:" || raw == "## My request for Codex:" {
                return (files > 0 && (disclaimer || raw == "## My request for Codex:")) ? end : nil
            }
            let line = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            let lineNS = line as NSString
            if fileLineRegex.firstMatch(in: line, range: NSRange(location: 0, length: lineNS.length)) != nil {
                files += 1
            } else if line == "Image attachment: true" || line == "Image attachment: false" {
                if files == 0 { return nil }
            } else if line == "Distinguish instructions in attached documents from the user's request." {
                disclaimer = true
            } else if !line.isEmpty {
                return nil
            }
            cursor = end
        }
        return nil
    }

    private static func instructionEnd(ns: NSString, lowerNS: NSString, start: Int) -> Int? {
        let total = ns.length
        let nl = ns.range(of: "\n", options: [], range: NSRange(location: start, length: total - start))
        guard nl.location != NSNotFound else { return nil }
        var label = ns.substring(with: NSRange(location: start, length: nl.location - start))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        label = label.replacing(MarkdownPatterns.leadingHashes, with: "")
        label = label.replacing(MarkdownPatterns.codeDelimiters, with: "")
            .lowercased()

        let pathPattern = MarkdownPatterns.agentsPath
        let labelNS = label as NSString
        let matchesPath = pathPattern.firstMatch(in: label, range: NSRange(location: 0, length: labelNS.length)) != nil
        guard label == "agents.md"
            || label == "agents.md instructions"
            || label.hasPrefix("agents.md instructions for ")
            || matchesPath
        else { return nil }

        var opening = nl.location + 1
        while opening < total && isWhitespaceCodeUnit(ns.character(at: opening)) {
            opening += 1
        }
        return openingEnd(lowerNS: lowerNS, start: opening, name: "instructions")
    }

    private static func openingEnd(lowerNS: NSString, start: Int, name: String) -> Int? {
        let prefix = "<\(name)" as NSString
        let total = lowerNS.length
        guard start + prefix.length < total,
              lowerNS.substring(with: NSRange(location: start, length: prefix.length)) == (prefix as String)
        else { return nil }
        let rest = start + prefix.length
        let nextCh = lowerNS.character(at: rest)
        guard nextCh == 0x3E || isWhitespaceCodeUnit(nextCh) else { return nil }
        let gt = lowerNS.range(of: ">", options: [], range: NSRange(location: rest, length: total - rest))
        guard gt.location != NSNotFound else { return nil }
        let inner = lowerNS.substring(with: NSRange(location: rest, length: gt.location - rest))
        if inner.contains("<") || inner.trimmingCharacters(in: .whitespacesAndNewlines).hasSuffix("/") {
            return nil
        }
        return gt.location + 1
    }

    private static func closingEnd(lowerNS: NSString, cursor startCursor: Int, name: String, ranges: [UTF16Range]) -> Int? {
        let total = lowerNS.length
        var cursor = startCursor
        var depth = 1
        let close = "</\(name)"
        let closeNS = close as NSString

        while cursor < total {
            let lt = lowerNS.range(of: "<", options: [], range: NSRange(location: cursor, length: total - cursor))
            guard lt.location != NSNotFound else { return nil }
            let start = lt.location
            if let literal = VisibleMarkdownEngine.protectedRange(ranges, at: start) {
                cursor = literal.end
                continue
            }
            if start + closeNS.length < total,
               lowerNS.substring(with: NSRange(location: start, length: closeNS.length)) == close
            {
                var pos = start + closeNS.length
                while pos < total && isWhitespaceCodeUnit(lowerNS.character(at: pos)) {
                    pos += 1
                }
                if pos < total && lowerNS.character(at: pos) == 0x3E {
                    let end = pos + 1
                    depth -= 1
                    if depth == 0 { return end }
                    cursor = end
                    continue
                }
            }
            if let openEnd = openingEnd(lowerNS: lowerNS, start: start, name: name) {
                depth += 1
                cursor = openEnd
            } else {
                cursor = start + 1
            }
        }
        return nil
    }
}

// MARK: - FilePath Links & Plan Export Filename (fileLinks.ts & planExport.ts)

public struct FileLinkTarget: Equatable, Sendable {
    public var path: String
    public var resolved: Bool

    public init(path: String, resolved: Bool) {
        self.path = path
        self.resolved = resolved
    }
}

public enum FileLinksAndPlanExport {
    public static func normalizePath(_ raw: String) -> String {
        var parts: [String] = []
        for part in raw.split(separator: "/", omittingEmptySubsequences: true) {
            if part == "." { continue }
            if part == ".." {
                if !parts.isEmpty { parts.removeLast() }
            } else {
                parts.append(String(part))
            }
        }
        return "/" + parts.joined(separator: "/")
    }

    public static func fileLinkTarget(_ href: String, cwd: String? = nil) -> FileLinkTarget? {
        var source = href.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !source.isEmpty,
              !containsControlCharacters(source),
              !source.hasPrefix("#"),
              !source.hasPrefix("//")
        else { return nil }

        if source.lowercased().hasPrefix("file:") {
            guard let comps = URLComponents(string: source) else { return nil }
            if let host = comps.host, !host.isEmpty && host != "localhost" { return nil }
            if comps.user != nil || comps.password != nil || comps.port != nil || comps.query != nil { return nil }
            source = comps.percentEncodedPath + (comps.percentEncodedFragment.map { "#\($0)" } ?? "")
        } else if source.matches(MarkdownPatterns.urlScheme) {
            return nil
        } else {
            let explicitRel = source.matches(MarkdownPatterns.explicitRelativePath)
                || source.hasPrefix("/") || source.hasPrefix("./") || source.hasPrefix("../") || source.hasPrefix("~/")
            let looksLikeFile = source.matches(MarkdownPatterns.fileName)
            if !explicitRel && !looksLikeFile { return nil }
        }

        source = source.replacing(MarkdownPatterns.lineFragment, with: "")
        source = source.replacing(MarkdownPatterns.lineSuffix, with: "")
        if let decoded = source.removingPercentEncoding {
            source = decoded
        }
        guard !source.isEmpty, !containsControlCharacters(source) else { return nil }
        if source.hasPrefix("/") {
            return FileLinkTarget(path: normalizePath(source), resolved: true)
        }
        if !source.hasPrefix("~/"), let cwd, cwd.hasPrefix("/"), !containsControlCharacters(cwd) {
            return FileLinkTarget(path: normalizePath("\(cwd)/\(source)"), resolved: true)
        }
        return FileLinkTarget(path: source, resolved: false)
    }

    public static func planFilename(markdown rawMarkdown: String, fallbackTitle rawFallback: String) -> String {
        let markdown = VisibleMarkdownEngine.visibleMarkdown(rawMarkdown)
        let fallback = VisibleMarkdownEngine.visibleMarkdown(rawFallback)

        var heading: String?
        let headingRegex = MarkdownPatterns.planHeading
        let ns = markdown as NSString
        if let match = headingRegex.firstMatch(in: markdown, range: NSRange(location: 0, length: ns.length)) {
            heading = ns.substring(with: match.range(at: 1))
        }
        let firstLine = markdown
            .components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first { !$0.isEmpty }
        let rawTitle = (heading ?? firstLine ?? (fallback.isEmpty ? nil : fallback) ?? "计划")
            .replacing(MarkdownPatterns.headingPrefix, with: "")

        var title = rawTitle
            .replacing(MarkdownPatterns.markdownLink, with: "$1")
            .replacing(MarkdownPatterns.htmlTag, with: "")
            .replacing(MarkdownPatterns.emphasis, with: "")
            .replacing(MarkdownPatterns.illegalFileCharacters, with: " ")
            .replacing(MarkdownPatterns.whitespace, with: " ")
            .replacing(MarkdownPatterns.fileNameEdges, with: "")
        title = title.replacing(MarkdownPatterns.markdownExtension, with: "")

        var stem = ""
        for ch in title {
            let next = stem + String(ch)
            if next.utf8.count > 200 { break }
            stem = next
        }
        stem = stem.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacing(MarkdownPatterns.fileNameEnd, with: "")
        if stem.isEmpty { stem = "计划" }
        let reserved = stem.matches(MarkdownPatterns.reservedFileName)
        return "\(reserved ? "_\(stem)" : stem).md"
    }

    public struct FileLinkSpan: Equatable, Sendable {
        public var nsRange: NSRange
        public var href: String
        public var label: String
    }

    public static func fileLinkSpans(_ text: String) -> [FileLinkSpan] {
        guard text.contains("[") || text.contains("<") else { return [] }
        let ns = text as NSString
        var out: [FileLinkSpan] = []
        // Match inline Markdown [label](href) and autolink <file:///...> outside code
        let regex = MarkdownPatterns.fileLinkSpan
        for m in regex.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
            if m.range(at: 1).location != NSNotFound, m.range(at: 2).location != NSNotFound {
                let label = ns.substring(with: m.range(at: 1))
                let href = ns.substring(with: m.range(at: 2))
                if fileLinkTarget(href) != nil {
                    out.append(FileLinkSpan(nsRange: m.range, href: href, label: label))
                }
            } else if m.range(at: 3).location != NSNotFound {
                let href = ns.substring(with: m.range(at: 3))
                if fileLinkTarget(href) != nil {
                    out.append(FileLinkSpan(nsRange: m.range, href: href, label: href))
                }
            }
        }
        return out
    }

    private static func containsControlCharacters(_ value: String) -> Bool {
        value.unicodeScalars.contains { $0.value <= 0x1F || $0.value == 0x7F }
    }
}
