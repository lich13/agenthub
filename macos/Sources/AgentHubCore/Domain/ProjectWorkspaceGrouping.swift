import Foundation

// MARK: - Project Workspace Path Normalization, Worktree Coalescing & Grouping Engine

enum ProjectWorkspaceGroupingMode: String, Codable, Equatable, Sendable {
    case flat
    case byWorkspace
}

enum ProjectWorkspaceKey: Hashable, Codable, Sendable {
    case directory(String)
    case unassigned

    var path: String? {
        switch self {
        case let .directory(path): path
        case .unassigned: nil
        }
    }

    var scopeToken: String {
        switch self {
        case let .directory(path): "dir:\(path)"
        case .unassigned: "unassigned"
        }
    }
}

enum ProjectWorkspaceFilter: Equatable, Sendable {
    case all
    case workspace(ProjectWorkspaceKey, includeSubdirectories: Bool)

    var scopeToken: String {
        switch self {
        case .all:
            return "all"
        case let .workspace(key, includeSubdirectories):
            return "\(key.scopeToken)#sub=\(includeSubdirectories ? 1 : 0)"
        }
    }

    var selectedKey: ProjectWorkspaceKey? {
        switch self {
        case .all: nil
        case let .workspace(key, _): key
        }
    }

    var includeSubdirectories: Bool {
        switch self {
        case .all: false
        case let .workspace(_, includeSub): includeSub
        }
    }
}

enum ProjectWorkspaceAggregateStatus: Int, Comparable, Equatable, Sendable {
    case idle = 0
    case failed = 1
    case running = 2
    case attention = 3

    static func < (lhs: ProjectWorkspaceAggregateStatus, rhs: ProjectWorkspaceAggregateStatus) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

struct ProjectWorkspaceGroup<Item: Sendable>: Identifiable, Equatable, Sendable where Item: Equatable {
    var key: ProjectWorkspaceKey
    var displayName: String
    var fullPath: String?
    var items: [Item]
    var subtitlesByItemId: [String: String]
    var aggregateStatus: ProjectWorkspaceAggregateStatus
    var latestUpdatedMs: Int64

    var id: String { key.scopeToken }

    init(
        key: ProjectWorkspaceKey,
        displayName: String,
        fullPath: String?,
        items: [Item],
        subtitlesByItemId: [String: String],
        aggregateStatus: ProjectWorkspaceAggregateStatus,
        latestUpdatedMs: Int64
    ) {
        self.key = key
        self.displayName = displayName
        self.fullPath = fullPath
        self.items = items
        self.subtitlesByItemId = subtitlesByItemId
        self.aggregateStatus = aggregateStatus
        self.latestUpdatedMs = latestUpdatedMs
    }
}

protocol ProjectWorkspaceGroupableItem: Equatable, Sendable {
    var workspaceGroupItemId: String { get }
    var workspaceSummaryItem: UnifiedSessionSummaryItem { get }
}

extension UnifiedSessionSummaryItem: ProjectWorkspaceGroupableItem {
    var workspaceGroupItemId: String { sessionKey }
    var workspaceSummaryItem: UnifiedSessionSummaryItem { self }
}

extension ProjectWorkspaceEngine {
    static func filterItems<Item: ProjectWorkspaceGroupableItem>(
        _ items: [Item],
        by filter: ProjectWorkspaceFilter,
        enabled: Bool = true
    ) -> [Item] {
        guard enabled, filter != .all else { return items }
        return items.filter {
            let summary = $0.workspaceSummaryItem
            return matchesFilter(filter, cwd: summary.cwd, workspaceRoot: summary.workspaceRoot)
        }
    }

    static func buildGroups<Item: ProjectWorkspaceGroupableItem>(
        items: [Item]
    ) -> [ProjectWorkspaceGroup<Item>] {
        buildGroups(
            items: items,
            idOf: \.workspaceGroupItemId,
            cwdOf: { $0.workspaceSummaryItem.cwd },
            workspaceRootOf: { $0.workspaceSummaryItem.workspaceRoot },
            updatedAtMsOf: { $0.workspaceSummaryItem.updatedAtMs },
            statusOf: { itemPriorityStatus($0.workspaceSummaryItem) }
        )
    }

    static func visualOrderedKeys<Item: ProjectWorkspaceGroupableItem>(
        groups: [ProjectWorkspaceGroup<Item>],
        collapsedKeys: Set<ProjectWorkspaceKey>
    ) -> [String] {
        visualOrderedKeys(
            groups: groups,
            collapsedKeys: collapsedKeys,
            idOf: \.workspaceGroupItemId
        )
    }
}

enum SharedTimestampFormatters {
    private static let lock = NSLock()
    nonisolated(unsafe) private static let isoFractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
    nonisolated(unsafe) private static let isoBasic: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()
    private static let timeOfDayFormatter: DateFormatter = {
        let df = DateFormatter()
        df.locale = Locale(identifier: "en_US_POSIX")
        df.calendar = Calendar(identifier: .gregorian)
        df.dateFormat = "HH:mm:ss"
        return df
    }()
    private static let monthDayTimeFormatter: DateFormatter = {
        let df = DateFormatter()
        df.locale = Locale(identifier: "en_US_POSIX")
        df.calendar = Calendar(identifier: .gregorian)
        df.dateFormat = "MM-dd HH:mm"
        return df
    }()
    private static let sessionStatusFormatter: DateFormatter = {
        let df = DateFormatter()
        df.locale = Locale(identifier: "en_US_POSIX")
        df.calendar = Calendar(identifier: .gregorian)
        df.dateFormat = "yyyy/M/d HH:mm:ss"
        return df
    }()

    static func parseISO8601(_ raw: String?) -> Date? {
        guard let raw, !raw.isEmpty else { return nil }
        lock.lock()
        defer { lock.unlock() }
        return isoFractional.date(from: raw) ?? isoBasic.date(from: raw)
    }

    static func formatTimeOfDay(_ date: Date) -> String {
        lock.lock()
        defer { lock.unlock() }
        return timeOfDayFormatter.string(from: date)
    }

    static func formatMonthDayTime(_ date: Date) -> String {
        lock.lock()
        defer { lock.unlock() }
        return monthDayTimeFormatter.string(from: date)
    }

    static func formatSessionStatusTimestamp(_ date: Date) -> String {
        lock.lock()
        defer { lock.unlock() }
        return sessionStatusFormatter.string(from: date)
    }
}

enum ProjectWorkspaceEngine {

    /// Canonicalizes a path string using Unicode NFC, single `/` separators, and no trailing `/`
    /// (except root `/`), while preserving case sensitivity for Linux SSH targets.
    static func normalizePath(_ raw: String?) -> String {
        guard let raw, !raw.isEmpty else { return "" }
        // Zero-allocation fast path for canonical pure-ASCII paths (no outer whitespace, no `//`, no trailing `/`).
        if let firstByte = raw.utf8.first,
           firstByte > 0x20,
           let lastByte = raw.utf8.last,
           lastByte > 0x20,
           raw.utf8.count == 1 || lastByte != 0x2F
        {
            var cleanASCII = true
            var prevSlash = false
            for b in raw.utf8 {
                if b >= 0x80 {
                    cleanASCII = false
                    break
                }
                if b == 0x2F {
                    if prevSlash {
                        cleanASCII = false
                        break
                    }
                    prevSlash = true
                } else {
                    prevSlash = false
                }
            }
            if cleanASCII {
                return raw
            }
        }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "" }
        let nfc = trimmed.precomposedStringWithCanonicalMapping
        var collapsed = ""
        collapsed.reserveCapacity(nfc.count)
        var prevSlash = false
        for ch in nfc {
            if ch == "/" {
                if !prevSlash {
                    collapsed.append(ch)
                }
                prevSlash = true
            } else {
                collapsed.append(ch)
                prevSlash = false
            }
        }
        while collapsed.count > 1 && collapsed.hasSuffix("/") {
            collapsed.removeLast()
        }
        return collapsed
    }

    /// Resolves canonical workspace key and optional worktree/subpath label when coalesced.
    static func resolveWorkspace(
        cwd: String,
        workspaceRoot: String?
    ) -> (key: ProjectWorkspaceKey, coalescedSubtitle: String?) {
        let normCwd = normalizePath(cwd)
        let normRoot = normalizePath(workspaceRoot)

        if !normRoot.isEmpty {
            if normCwd.isEmpty || normCwd == normRoot {
                return (.directory(normRoot), nil)
            }
            return (.directory(normRoot), worktreeBadge(cwd: normCwd, mainRoot: normRoot))
        }

        guard !normCwd.isEmpty else {
            return (.unassigned, nil)
        }

        if let (fallbackRoot, wtName) = extractKnownWorktreePattern(normCwd) {
            return (.directory(fallbackRoot), "worktree · \(wtName)")
        }

        return (.directory(normCwd), nil)
    }

    /// Compact Orca-style two-segment directory label (`parent/repo`), preserving full path for tooltips/copy.
    static func compactDisplayName(for key: ProjectWorkspaceKey) -> String {
        switch key {
        case .unassigned:
            return "未记录工作目录"
        case let .directory(path):
            if path == "/" { return "/" }
            let parts = path.split(separator: "/", omittingEmptySubsequences: true)
            switch parts.count {
            case 0:
                return path
            case 1:
                return path.hasPrefix("/") ? "/\(parts[0])" : String(parts[0])
            default:
                return "\(parts[parts.count - 2])/\(parts[parts.count - 1])"
            }
        }
    }

    static func matchesFilter(
        _ filter: ProjectWorkspaceFilter,
        cwd: String,
        workspaceRoot: String?
    ) -> Bool {
        switch filter {
        case .all:
            return true
        case let .workspace(targetKey, includeSubdirectories):
            let (resolvedKey, _) = resolveWorkspace(cwd: cwd, workspaceRoot: workspaceRoot)
            if resolvedKey == targetKey {
                return true
            }
            guard includeSubdirectories,
                  case let .directory(basePath) = targetKey
            else {
                return false
            }
            let normCwd = normalizePath(cwd)
            if isStrictDescendantPath(normCwd, under: basePath) {
                return true
            }
            if case let .directory(resolvedPath) = resolvedKey,
               isStrictDescendantPath(resolvedPath, under: basePath)
            {
                return true
            }
            return false
        }
    }

    static func isStrictDescendantPath(_ candidate: String, under base: String) -> Bool {
        guard !candidate.isEmpty, !base.isEmpty else { return false }
        if candidate == base { return true }
        if base == "/" { return candidate.hasPrefix("/") }
        return candidate.hasPrefix(base + "/")
    }

    static func parseTimestampMs(_ iso: String?) -> Int64 {
        guard let date = SharedTimestampFormatters.parseISO8601(iso) else { return Int64.min }
        return Int64((date.timeIntervalSince1970 * 1000.0).rounded())
    }

    static func itemPriorityStatus(_ item: UnifiedSessionSummaryItem) -> ProjectWorkspaceAggregateStatus {
        if item.archived { return .idle }
        if item.statusChipBadge == "等待处理"
            || item.statusChipBadge == "待回复"
            || item.statusText == "等待处理"
            || item.statusText == "待回复"
            || item.statusChipTone == .warning
        {
            return .attention
        }
        if item.running {
            return .running
        }
        if item.statusChipTone == .danger || item.readError != nil {
            return .failed
        }
        return .idle
    }

    private struct WorkspaceGroupBucket<Item> {
        var key: ProjectWorkspaceKey
        var firstIndex: Int
        var items: [Item]
        var subtitles: [String: String]
        var status: ProjectWorkspaceAggregateStatus
        var latestMs: Int64
    }

    static func buildGroups<Item: Equatable & Sendable>(
        items: [Item],
        idOf: (Item) -> String,
        cwdOf: (Item) -> String,
        workspaceRootOf: (Item) -> String?,
        updatedAtOf: (Item) -> String?,
        statusOf: (Item) -> ProjectWorkspaceAggregateStatus
    ) -> [ProjectWorkspaceGroup<Item>] {
        buildGroups(
            items: items,
            idOf: idOf,
            cwdOf: cwdOf,
            workspaceRootOf: workspaceRootOf,
            updatedAtMsOf: { parseTimestampMs(updatedAtOf($0)) },
            statusOf: statusOf
        )
    }

    static func buildGroups<Item: Equatable & Sendable>(
        items: [Item],
        idOf: (Item) -> String,
        cwdOf: (Item) -> String,
        workspaceRootOf: (Item) -> String?,
        updatedAtMsOf: (Item) -> Int64,
        statusOf: (Item) -> ProjectWorkspaceAggregateStatus
    ) -> [ProjectWorkspaceGroup<Item>] {
        var buckets: [ProjectWorkspaceKey: WorkspaceGroupBucket<Item>] = [:]
        for (idx, item) in items.enumerated() {
            let cwd = cwdOf(item)
            let wsRoot = workspaceRootOf(item)
            let (key, coalescedSub) = resolveWorkspace(cwd: cwd, workspaceRoot: wsRoot)
            let tsMs = updatedAtMsOf(item)
            let st = statusOf(item)
            let itemId = idOf(item)

            if var existing = buckets[key] {
                existing.items.append(item)
                if let coalescedSub, !coalescedSub.isEmpty {
                    existing.subtitles[itemId] = coalescedSub
                }
                if st > existing.status {
                    existing.status = st
                }
                if tsMs > existing.latestMs {
                    existing.latestMs = tsMs
                }
                buckets[key] = existing
            } else {
                var subs: [String: String] = [:]
                if let coalescedSub, !coalescedSub.isEmpty {
                    subs[itemId] = coalescedSub
                }
                buckets[key] = WorkspaceGroupBucket(
                    key: key,
                    firstIndex: idx,
                    items: [item],
                    subtitles: subs,
                    status: st,
                    latestMs: tsMs
                )
            }
        }

        let sorted = buckets.values.sorted { lhs, rhs in
            switch (lhs.key, rhs.key) {
            case (.unassigned, .directory):
                return false
            case (.directory, .unassigned):
                return true
            default:
                if lhs.latestMs != rhs.latestMs {
                    return lhs.latestMs > rhs.latestMs
                }
                if lhs.firstIndex != rhs.firstIndex {
                    return lhs.firstIndex < rhs.firstIndex
                }
                return (lhs.key.path ?? "") < (rhs.key.path ?? "")
            }
        }

        return sorted.map { bucket in
            ProjectWorkspaceGroup(
                key: bucket.key,
                displayName: compactDisplayName(for: bucket.key),
                fullPath: bucket.key.path,
                items: bucket.items,
                subtitlesByItemId: bucket.subtitles,
                aggregateStatus: bucket.status,
                latestUpdatedMs: bucket.latestMs
            )
        }
    }

    static func visualOrderedKeys<Item: Equatable & Sendable>(
        groups: [ProjectWorkspaceGroup<Item>],
        collapsedKeys: Set<ProjectWorkspaceKey>,
        idOf: (Item) -> String
    ) -> [String] {
        var result: [String] = []
        for group in groups where !collapsedKeys.contains(group.key) {
            for item in group.items {
                result.append(idOf(item))
            }
        }
        return result
    }

    private static func worktreeBadge(cwd: String, mainRoot: String) -> String {
        if let (_, wtName) = extractKnownWorktreePattern(cwd) {
            return "worktree · \(wtName)"
        }
        if mainRoot != "/", cwd.hasPrefix(mainRoot + "/") {
            let rel = String(cwd.dropFirst(mainRoot.count + 1))
            return rel
        }
        let parts = cwd.split(separator: "/", omittingEmptySubsequences: true)
        if let worktreesIdx = parts.lastIndex(of: "worktrees"), worktreesIdx + 1 < parts.count {
            let tail = parts[(worktreesIdx + 1)...].joined(separator: "/")
            return "worktree · \(tail)"
        }
        if let last = parts.last {
            return "worktree · \(last)"
        }
        return cwd
    }

    private static func extractKnownWorktreePattern(_ normPath: String) -> (root: String, name: String)? {
        for marker in ["/.claude/worktrees/", "/.codex/worktrees/", "/.orca/worktrees/"] {
            if let range = normPath.range(of: marker) {
                let prefix = String(normPath[..<range.lowerBound])
                let root = prefix.isEmpty ? "/" : prefix
                let rest = String(normPath[range.upperBound...])
                if let firstSeg = rest.split(separator: "/", omittingEmptySubsequences: true).first {
                    return (root, String(firstSeg))
                }
            }
        }
        return nil
    }
}
