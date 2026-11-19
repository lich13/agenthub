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

public enum VisibleMarkdownEngine {
    public static func literalMarkdownRanges(_ text: String, includeInline: Bool = true, includeQuotes: Bool = true) -> [UTF16Range] {
        let ns = text as NSString
        let total = ns.length
        if total == 0 { return [] }

        var ranges: [UTF16Range] = []
        let lineRegex = try! NSRegularExpression(pattern: "[^\\n]*\\n|[^\\n]+$")
        let fenceRegex = try! NSRegularExpression(pattern: "^ {0,3}(`{3,}|~{3,})(.*)")
        let indentedOrQuoteRegex = try! NSRegularExpression(
            pattern: includeQuotes ? "^( {4}| *\\t| {0,3}>)" : "^( {4}| *\\t)"
        )

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

        let commentRegex = try! NSRegularExpression(pattern: "<!--[\\s\\S]*?(?:-->|$)")
        for match in commentRegex.matches(in: text, range: NSRange(location: 0, length: total)) {
            ranges.append(UTF16Range(start: match.range.location, end: match.range.location + match.range.length))
        }

        return mergeRanges(ranges)
    }

    public static func memoryMetadataRanges(_ text: String, protectedRanges: [UTF16Range]? = nil) -> [UTF16Range] {
        let ns = text as NSString
        let total = ns.length
        guard text.range(of: "oai-mem-citation|citation_entries|rollout_ids|memory_citation", options: [.regularExpression, .caseInsensitive]) != nil else {
            return []
        }

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
        let tagRegex = try! NSRegularExpression(
            pattern: "(?:<|&lt;)(\\/?)(oai-mem-citation|citation_entries|rollout_ids)(?:\\s+[^<>]*?)?\\s*(?:>|&gt;)",
            options: [.caseInsensitive]
        )
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

        let fieldRegex = try! NSRegularExpression(
            pattern: "^[ \\t]*(?:[\"']memory_citation[\"']|memory_citation)[ \\t]*[:=][ \\t]*",
            options: [.caseInsensitive, .anchorsMatchLines]
        )
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
        guard text.contains("external_codex_apps_open_page") else { return [] }
        let ns = text as NSString
        let literal = protectedRanges ?? literalMarkdownRanges(text, includeInline: true, includeQuotes: true)
        var ranges: [UTF16Range] = []
        let regex = try! NSRegularExpression(pattern: "<external_codex_apps_open_page>([\\s\\S]*?)<\\/external_codex_apps_open_page>")
        for match in regex.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
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
        return ranges
    }

    public static func visibleMarkdown(_ text: String, protectedRanges: [UTF16Range]? = nil) -> String {
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
        let regex = try! NSRegularExpression(
            pattern: "(?:^|[\\s/\\\\`\"'(=:])agents\\.md(?=$|[\\s`\"'),:;\\]}>])",
            options: [.caseInsensitive]
        )
        return values.contains { value in
            guard let value, !value.isEmpty else { return false }
            let ns = value as NSString
            return regex.firstMatch(in: value, range: NSRange(location: 0, length: ns.length)) != nil
        }
    }

    private static func isInstructionLabel(_ line: String) -> Bool {
        var label = line.trimmingCharacters(in: .whitespacesAndNewlines)
        label = label.replacingOccurrences(of: "^#{1,6}\\s+", with: "", options: .regularExpression)
        label = label.replacingOccurrences(of: "\\s+#+$", with: "", options: .regularExpression)
        label = label.replacingOccurrences(of: "^[`*]+|[`*:]+$", with: "", options: .regularExpression)
        let patterns = [
            "^(?:[a-z]:[/\\\\]|[/\\\\]|\\.{1,2}[/\\\\]|~[/\\\\])(?:[^<>\\n]*[/\\\\])?agents\\.md$",
            "^(?:[\\w.-]+[/\\\\])+agents\\.md$",
            "^agents\\.md(?:\\s+instructions(?:\\s+for\\s+.+)?)?$",
        ]
        let ns = label as NSString
        return patterns.contains { pat in
            let re = try! NSRegularExpression(pattern: pat, options: [.caseInsensitive])
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
        let lineRegex = try! NSRegularExpression(pattern: "[^\\n]*(?:\\n|$)")
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
            let trimmedEnd = val.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
            result.append(InstructionSegment(
                kind: kind,
                id: "\(kind):\(start)",
                text: val,
                lines: countLines(trimmedEnd),
                bytes: val.utf8.count
            ))
        }

        var cursor = 0
        let headingRegex = try! NSRegularExpression(pattern: "^ {0,3}(#{1,6})\\s")
        let openInstructionsRegex = try! NSRegularExpression(pattern: "^\\s*<INSTRUCTIONS>[ \\t]*(?:\\r?\\n|$)", options: [.caseInsensitive])
        let closeInstructionsRegex = try! NSRegularExpression(pattern: "<\\/INSTRUCTIONS\\s*>", options: [.caseInsensitive])

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

    public static func visibleContextText(_ text: String) -> String {
        let ranges = VisibleMarkdownEngine.literalMarkdownRanges(text, includeInline: true, includeQuotes: true)
        return VisibleMarkdownEngine.visibleMarkdown(text, protectedRanges: ranges)
    }

    public static func projectUserMessage(_ text: String, contextBlocks: [UserContextBlock]? = nil) -> UserMessageProjection {
        if let contextBlocks {
            return UserMessageProjection(text: text, contextBlocks: contextBlocks)
        }
        if !text.contains("<") && !text.contains("# Files mentioned by the user:") {
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

        let advance: (Int) -> Void = { end in
            let slice = text.utf16Substring(from: cursor, to: end) as NSString
            let nl = slice.range(of: "\n", options: .backwards)
            if nl.location != NSNotFound {
                lineStart = cursor + nl.location + 1
            }
            cursor = end
        }

        let ordinaryTagRegex = try! NSRegularExpression(pattern: "^<([a-z][a-z0-9_:-]*)", options: [.caseInsensitive])

        while cursor < total {
            if let literal = VisibleMarkdownEngine.protectedRange(ranges, at: cursor) {
                advance(literal.end)
                adjacent = false
                continue
            }
            let prefixSpaces = text.utf16Substring(from: lineStart, to: cursor)
            let boundary = (cursor - lineStart <= 3) && prefixSpaces.allSatisfy { $0 == " " }

            if adjacent || boundary {
                let tailFromCursor = text.utf16Substring(from: cursor, to: total)
                let isFileHeader = tailFromCursor.hasPrefix("# Files mentioned by the user:")
                if let fileEnd = isFileHeader ? fileHeaderEnd(ns: ns, start: cursor) : nil {
                    let pre = text.utf16Substring(from: 0, to: cursor)
                    let contextStart = (kept == 0 && pre.allSatisfy { $0 == "\r" || $0 == "\n" }) ? 0 : cursor
                    bodyParts.append(text.utf16Substring(from: kept, to: contextStart))
                    blocks.append(UserContextBlock(kind: "files", text: text.utf16Substring(from: contextStart, to: fileEnd)))
                    advance(fileEnd)
                    kept = fileEnd
                    adjacent = true
                    continue
                }
                if isFileHeader { break }

                var matched: (name: String, kind: String, end: Int)?
                if let instEnd = instructionEnd(ns: ns, lowerNS: lowerNS, start: cursor) {
                    matched = ("instructions", "instructions", instEnd)
                } else {
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
                    bodyParts.append(text.utf16Substring(from: kept, to: cursor))
                    blocks.append(UserContextBlock(kind: matched.kind, text: text.utf16Substring(from: cursor, to: closeEnd)))
                    advance(closeEnd)
                    kept = closeEnd
                    adjacent = true
                    continue
                }

                let tailNS = tailFromCursor as NSString
                if let m = ordinaryTagRegex.firstMatch(in: tailFromCursor, range: NSRange(location: 0, length: min(tailNS.length, 80))) {
                    let tagName = tailNS.substring(with: m.range(at: 1)).lowercased()
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

        bodyParts.append(text.utf16Substring(from: kept, to: total))
        return UserMessageProjection(text: bodyParts.joined(), contextBlocks: blocks)
    }

    public static func userMessageSegments(_ text: String, contextBlocks: [UserContextBlock]? = nil) -> [UserMessageSegment] {
        let projected = projectUserMessage(text, contextBlocks: contextBlocks).text
        let ns = projected as NSString
        let total = ns.length
        var result: [UserMessageSegment] = []
        var occurrences: [String: Int] = [:]

        let uniqueId: (String) -> String = { key in
            let count = occurrences[key, default: 0]
            occurrences[key] = count + 1
            return "\(key):\(count)"
        }

        let protectedRanges = projected.contains("<send_user_message_question_reply>")
            ? VisibleMarkdownEngine.literalMarkdownRanges(projected, includeInline: true, includeQuotes: true)
            : []
        let envelopeRegex = try! NSRegularExpression(
            pattern: "^[ \\t]{0,3}<send_user_message_question_reply>([\\s\\S]*?)<\\/send_user_message_question_reply>[ \\t]*(?=\\r?$)",
            options: [.anchorsMatchLines]
        )

        var cursor = 0
        let addText: (Int) -> Void = { end in
            let slice = projected.utf16Substring(from: cursor, to: end)
            let cleaned = VisibleMarkdownEngine.visibleMarkdown(slice)
            if !cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                result.append(.text(id: "text:\(cursor)", text: cleaned))
            }
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

    private static func fileHeaderEnd(ns: NSString, start: Int) -> Int? {
        let total = ns.length
        let rest = ns.substring(from: start)
        let headerRegex = try! NSRegularExpression(pattern: "^# Files mentioned by the user:\\r?\\n")
        guard let m = headerRegex.firstMatch(in: rest, range: NSRange(location: 0, length: min((rest as NSString).length, 64))) else {
            return nil
        }
        var cursor = start + m.range.length
        var files = 0
        var disclaimer = false
        let fileLineRegex = try! NSRegularExpression(pattern: "^## .+: .+$")

        while cursor < total {
            let nl = ns.range(of: "\n", options: [], range: NSRange(location: cursor, length: total - cursor))
            let end = nl.location == NSNotFound ? total : nl.location + 1
            let raw = ns.substring(with: NSRange(location: cursor, length: end - cursor))
                .replacingOccurrences(of: "\\r?\\n$", with: "", options: .regularExpression)
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
        label = label.replacingOccurrences(of: "^#+\\s*", with: "", options: .regularExpression)
        label = label.replacingOccurrences(of: "^`|`$", with: "", options: .regularExpression)
            .lowercased()

        let pathPattern = try! NSRegularExpression(pattern: "^[^\\s]+[/\\\\]agents\\.md$")
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
        } else if source.range(of: "^[a-z][a-z0-9+.-]*:", options: [.regularExpression, .caseInsensitive]) != nil {
            return nil
        } else {
            let explicitRel = source.range(of: "^(?:/|\\.{1,2}/|~/ )", options: .regularExpression) != nil
                || source.hasPrefix("/") || source.hasPrefix("./") || source.hasPrefix("../") || source.hasPrefix("~/")
            let looksLikeFile = source.range(
                of: "^[^?#]+(?:/|\\.[\\p{L}\\d_-]+(?:[:#]|$))",
                options: .regularExpression
            ) != nil
            if !explicitRel && !looksLikeFile { return nil }
        }

        source = source.replacingOccurrences(
            of: "#L\\d+(?:C\\d+)?(?:-L?C?\\d+(?:C\\d+)?)?$",
            with: "",
            options: [.regularExpression, .caseInsensitive]
        )
        source = source.replacingOccurrences(of: ":\\d+(?::\\d+)?$", with: "", options: .regularExpression)
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
        let headingRegex = try! NSRegularExpression(pattern: "^ {0,3}#\\s+(.+?)\\s*#*\\s*$", options: [.anchorsMatchLines])
        let ns = markdown as NSString
        if let match = headingRegex.firstMatch(in: markdown, range: NSRange(location: 0, length: ns.length)) {
            heading = ns.substring(with: match.range(at: 1))
        }
        let firstLine = markdown
            .components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first { !$0.isEmpty }
        let rawTitle = (heading ?? firstLine ?? (fallback.isEmpty ? nil : fallback) ?? "计划")
            .replacingOccurrences(of: "^#{1,6}\\s+", with: "", options: .regularExpression)

        var title = rawTitle
            .replacingOccurrences(of: "\\[([^\\]]+)\\]\\([^)]*\\)", with: "$1", options: .regularExpression)
            .replacingOccurrences(of: "<[^>]*>", with: "", options: .regularExpression)
            .replacingOccurrences(of: "[*_`~]+", with: "", options: .regularExpression)
            .replacingOccurrences(of: "[\\x00-\\x1f\\x7f<>:\"/\\\\|?*]+", with: " ", options: .regularExpression)
            .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            .replacingOccurrences(of: "^[.\\s]+|[.\\s]+$", with: "", options: .regularExpression)
        title = title.replacingOccurrences(of: "\\.md$", with: "", options: [.regularExpression, .caseInsensitive])

        var stem = ""
        for ch in title {
            let next = stem + String(ch)
            if next.utf8.count > 200 { break }
            stem = next
        }
        stem = stem.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "[.\\s]+$", with: "", options: .regularExpression)
        if stem.isEmpty { stem = "计划" }
        let reserved = stem.range(of: "^(con|prn|aux|nul|com[1-9]|lpt[1-9])$", options: [.regularExpression, .caseInsensitive]) != nil
        return "\(reserved ? "_\(stem)" : stem).md"
    }

    private static func containsControlCharacters(_ value: String) -> Bool {
        value.unicodeScalars.contains { $0.value <= 0x1F || $0.value == 0x7F }
    }
}
