import Foundation

// MARK: - Project Workspace Path Normalization, Worktree Coalescing & Grouping Engine

public enum ProjectWorkspaceGroupingMode: String, Codable, Equatable, Sendable {
    case flat
    case byWorkspace
}

public enum ProjectWorkspaceKey: Hashable, Codable, Sendable {
    case directory(String)
    case unassigned

    public var path: String? {
        switch self {
        case let .directory(path): path
        case .unassigned: nil
        }
    }

    public var scopeToken: String {
        switch self {
        case let .directory(path): "dir:\(path)"
        case .unassigned: "unassigned"
        }
    }
}

public enum ProjectWorkspaceFilter: Equatable, Sendable {
    case all
    case workspace(ProjectWorkspaceKey, includeSubdirectories: Bool)

    public var scopeToken: String {
        switch self {
        case .all:
            return "all"
        case let .workspace(key, includeSubdirectories):
            return "\(key.scopeToken)#sub=\(includeSubdirectories ? 1 : 0)"
        }
    }

    public var selectedKey: ProjectWorkspaceKey? {
        switch self {
        case .all: nil
        case let .workspace(key, _): key
        }
    }

    public var includeSubdirectories: Bool {
        switch self {
        case .all: false
        case let .workspace(_, includeSub): includeSub
        }
    }
}

public enum ProjectWorkspaceAggregateStatus: Int, Comparable, Equatable, Sendable {
    case idle = 0
    case failed = 1
    case running = 2
    case attention = 3

    public static func < (lhs: ProjectWorkspaceAggregateStatus, rhs: ProjectWorkspaceAggregateStatus) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

public struct ProjectWorkspaceGroup<Item: Sendable>: Identifiable, Equatable, Sendable where Item: Equatable {
    public var key: ProjectWorkspaceKey
    public var displayName: String
    public var fullPath: String?
    public var items: [Item]
    public var subtitlesByItemId: [String: String]
    public var aggregateStatus: ProjectWorkspaceAggregateStatus
    public var latestUpdatedMs: Int64

    public var id: String { key.scopeToken }

    public init(
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

public enum ProjectWorkspaceEngine {
    private static let isoFractionalLock = NSLock()
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

    /// Canonicalizes a path string using Unicode NFC, single `/` separators, and no trailing `/`
    /// (except root `/`), while preserving case sensitivity for Linux SSH targets.
    public static func normalizePath(_ raw: String?) -> String {
        guard let raw else { return "" }
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
    public static func resolveWorkspace(
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
    public static func compactDisplayName(for key: ProjectWorkspaceKey) -> String {
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

    public static func matchesFilter(
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

    public static func isStrictDescendantPath(_ candidate: String, under base: String) -> Bool {
        guard !candidate.isEmpty, !base.isEmpty else { return false }
        if candidate == base { return true }
        if base == "/" { return candidate.hasPrefix("/") }
        return candidate.hasPrefix(base + "/")
    }

    public static func parseTimestampMs(_ iso: String?) -> Int64 {
        guard let iso, !iso.isEmpty else { return Int64.min }
        isoFractionalLock.lock()
        let date = isoFractional.date(from: iso) ?? isoBasic.date(from: iso)
        isoFractionalLock.unlock()
        guard let date else { return Int64.min }
        return Int64((date.timeIntervalSince1970 * 1000.0).rounded())
    }

    public static func itemPriorityStatus(_ item: UnifiedSessionSummaryItem) -> ProjectWorkspaceAggregateStatus {
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

    public static func buildGroups<Item: Equatable & Sendable>(
        items: [Item],
        idOf: (Item) -> String,
        cwdOf: (Item) -> String,
        workspaceRootOf: (Item) -> String?,
        updatedAtOf: (Item) -> String?,
        statusOf: (Item) -> ProjectWorkspaceAggregateStatus
    ) -> [ProjectWorkspaceGroup<Item>] {
        var buckets: [ProjectWorkspaceKey: WorkspaceGroupBucket<Item>] = [:]
        for (idx, item) in items.enumerated() {
            let cwd = cwdOf(item)
            let wsRoot = workspaceRootOf(item)
            let (key, coalescedSub) = resolveWorkspace(cwd: cwd, workspaceRoot: wsRoot)
            let tsMs = parseTimestampMs(updatedAtOf(item))
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

    public static func visualOrderedKeys<Item: Equatable & Sendable>(
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
