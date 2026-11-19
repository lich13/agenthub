import Foundation

// MARK: - History Cursors & Resume Commands (historyCursor.ts, claudeSession.ts, GrokWorkspace.tsx)

public enum HistoryAndResumeHelpers {
    public static func historyCursorIndex(_ cursor: String?) -> Double {
        guard let cursor, !cursor.isEmpty else { return .infinity }
        let parts = cursor.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count >= 2, let num = Double(parts[1]), num.isFinite else { return .infinity }
        return num
    }

    public static func stableHistoryCursor(
        history: HistoryMetadata?,
        blocks: [MessageBlock],
        beforeCursor: String?
    ) -> String? {
        guard let beforeCursor else { return nil }
        if beforeCursor.hasPrefix("p:") { return beforeCursor }
        guard history != nil, let first = blocks.first else { return beforeCursor }
        let idx = historyCursorIndex(beforeCursor)
        guard idx.isFinite else { return beforeCursor }
        return "b:\(Int(idx)):\(first.id)"
    }

    public static func threadResumeCommand(_ threadId: String?) -> String? {
        guard let id = threadId?.trimmingCharacters(in: .whitespacesAndNewlines), !id.isEmpty else {
            return nil
        }
        return "codex resume \(id)"
    }

    public static func shellSingleQuote(_ value: String) -> String {
        "'\(value.replacingOccurrences(of: "'", with: "'\\''"))'"
    }

    public static func claudeResumeCommand(cwd: String, id: String) -> String {
        let prefix = cwd.isEmpty ? "" : "cd -- \(shellSingleQuote(cwd)) && "
        return "\(prefix)claude --resume \(shellSingleQuote(id))"
    }

    public static func grokResumeCommand(cwd: String, id: String) -> String {
        let prefix = cwd.isEmpty ? "" : "cd -- \(shellSingleQuote(cwd)) && "
        return "\(prefix)grok --resume \(shellSingleQuote(id))"
    }

    public static func grokHandoffPath(_ session: GrokSessionSummary) -> String {
        if let hp = session.handoffPath?.trimmingCharacters(in: .whitespacesAndNewlines), !hp.isEmpty {
            return hp
        }
        return session.path
    }
}

// MARK: - User Timeline Rail ViewModel (timelineViewModel.ts)

public struct TimelineSourceItem: Equatable, Sendable {
    public var id: String
    public var role: String?
    public var kind: String?
    public var text: String?
    public var userMessage: UserMessageContent?

    public init(
        id: String,
        role: String? = nil,
        kind: String? = nil,
        text: String? = nil,
        userMessage: UserMessageContent? = nil
    ) {
        self.id = id
        self.role = role
        self.kind = kind
        self.text = text
        self.userMessage = userMessage
    }
}

public enum TimelineViewModel {
    public static func matchesTimelinePosition(
        _ positionKey: String?,
        id: String?,
        aliases: [String] = []
    ) -> Bool {
        guard let positionKey, !positionKey.isEmpty else { return false }
        if id == positionKey || aliases.contains(positionKey) { return true }
        if let id {
            if id.contains(":\(positionKey):") || id.hasSuffix(":\(positionKey)") {
                return true
            }
        }
        return false
    }

    public static func userTimelineEntries(
        provider: SessionProvider,
        sources: [TimelineSourceItem]
    ) -> [TimelineEntry] {
        var seen = Set<String>()
        var entries: [TimelineEntry] = []

        for source in sources {
            let isUser: Bool = if provider == .codex {
                source.role?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == "user"
            } else if provider == .grok {
                source.kind == "user_message_chunk"
            } else {
                source.kind == "user_message"
            }
            if !isUser { continue }

            let identity = source.userMessage?.id ?? source.id
            if seen.contains(identity) { continue }
            seen.insert(identity)

            let rawText = source.userMessage?.text ?? source.text ?? ""
            let segments = UserContextEngine.userMessageSegments(rawText, contextBlocks: source.userMessage?.contextBlocks)
            let extracted = segments.compactMap { seg -> String? in
                switch seg {
                case let .text(_, t): t
                case let .reply(_, _, a): a
                }
            }.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)

            let attachments = source.userMessage?.attachments ?? []
            if extracted.isEmpty && attachments.isEmpty { continue }

            let compact = extracted.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            let ns = compact as NSString
            let title = ns.length > 0 ? ns.substring(to: min(80, ns.length)) : "附件"
            let preview: String? = if ns.length > 80 {
                ns.substring(with: NSRange(location: 80, length: min(180, ns.length - 80)))
            } else {
                nil
            }
            entries.append(TimelineEntry(id: source.id, title: title, preview: preview))
        }
        return entries
    }

    public static func activeUserTimelineId(
        anchors: [(id: String, top: CGFloat)],
        viewportTop: CGFloat
    ) -> String? {
        guard !anchors.isEmpty else { return nil }
        var low = 0
        var high = anchors.count
        while low < high {
            let mid = (low + high) / 2
            if anchors[mid].top <= viewportTop {
                low = mid + 1
            } else {
                high = mid
            }
        }
        return anchors[max(0, low - 1)].id
    }
}

// MARK: - Thread Message Store & 120s Optimistic Codex Rename Cache (threadMessageStore.ts)

public struct ThreadMessageSlot: Equatable, Sendable {
    public var timeline: [TimelineEntry]?
    public var history: HistoryMetadata?
    public var subagents: SubagentCollection?
    public var subagentUpdates: [String: SubagentActivity]?
    public var summary: ThreadSummary?
    public var blocks: [MessageBlock]
    public var totalBlocks: Int
    public var hasMoreBlocks: Bool
    public var beforeCursor: String?
    public var visibleUpdateRevision: Int
    public var bottomFollowRevision: Int
    public var loadingEarlier: Bool
    public var loadError: String?
    public var feedback: String?
    public var showAllHistory: Bool

    public init() {
        self.blocks = []
        self.totalBlocks = 0
        self.hasMoreBlocks = false
        self.visibleUpdateRevision = 0
        self.bottomFollowRevision = 0
        self.loadingEarlier = false
        self.showAllHistory = false
    }
}

@MainActor
public final class ThreadMessageStore: ObservableObject {
    public static let optimisticRenameTTLSeconds: TimeInterval = 120.0

    @Published public private(set) var activeThreadId: String?
    @Published public private(set) var revision: UInt64 = 0
    private var slots: [String: ThreadMessageSlot] = [:]
    private var optimisticTitles: [String: (title: String, expiresAt: Date)] = [:]

    public init() {}

    public func setActive(_ threadId: String?) {
        activeThreadId = threadId
        if let threadId, slots[threadId] == nil {
            slots[threadId] = ThreadMessageSlot()
        }
        revision &+= 1
    }

    public func slot(for threadId: String) -> ThreadMessageSlot {
        if let existing = slots[threadId] { return existing }
        let created = ThreadMessageSlot()
        slots[threadId] = created
        return created
    }

    public func recordOptimisticTitle(threadId: String, title: String, now: Date = Date()) {
        optimisticTitles[threadId] = (title: title, expiresAt: now.addingTimeInterval(Self.optimisticRenameTTLSeconds))
        if var s = slots[threadId], var sum = s.summary {
            sum.title = title
            s.summary = sum
            s.visibleUpdateRevision += 1
            slots[threadId] = s
        }
        revision &+= 1
    }

    public func effectiveTitle(threadId: String, serverTitle: String, now: Date = Date()) -> String {
        if let entry = optimisticTitles[threadId] {
            if entry.expiresAt <= now || entry.title == serverTitle {
                optimisticTitles.removeValue(forKey: threadId)
                return serverTitle
            }
            return entry.title
        }
        return serverTitle
    }

    @discardableResult
    public func applyDetail(_ detail: ThreadDetail, now: Date = Date()) -> ThreadMessageSlot {
        let threadId = detail.summary.id
        var slot = self.slot(for: threadId)
        let reset = (detail.history?.reset == true)
            || (slot.history != nil && detail.history != nil && slot.history?.epoch != detail.history?.epoch)
        if reset {
            slot.blocks = []
            slot.beforeCursor = nil
            slot.hasMoreBlocks = false
        }

        let nextTimeline = detail.timeline ?? (reset ? nil : slot.timeline)
        let incoming: [MessageBlock] = if !detail.blocks.isEmpty {
            detail.blocks
        } else {
            detail.messages.enumerated().map { index, m in
                MessageBlock(id: "legacy-\(index)", role: m.role, kind: m.kind, text: m.text, createdAt: m.createdAt)
            }
        }

        let merged = Self.mergeSubagentUpdates(
            Self.mergeBlocks(existing: slot.blocks, incoming: incoming, mode: .appendOrUpdate),
            updates: detail.subagentUpdates
        )
        let blocksChanged = (merged != slot.blocks)

        var nextSummary = detail.summary
        nextSummary.title = effectiveTitle(threadId: threadId, serverTitle: detail.summary.title, now: now)

        let nextTotal = detail.totalBlocks ?? max(slot.totalBlocks, merged.count)
        var nextHasMore = detail.hasMoreBlocks ?? slot.hasMoreBlocks
        var nextBefore = slot.beforeCursor

        if merged.count >= nextTotal {
            nextBefore = nil
            nextHasMore = false
        } else if let incomingBefore = detail.beforeCursor {
            if slot.beforeCursor == nil
                || HistoryAndResumeHelpers.historyCursorIndex(incomingBefore) < HistoryAndResumeHelpers.historyCursorIndex(slot.beforeCursor)
            {
                nextBefore = HistoryAndResumeHelpers.stableHistoryCursor(
                    history: detail.history,
                    blocks: incoming,
                    beforeCursor: incomingBefore
                )
            }
        } else if merged.isEmpty {
            nextBefore = nil
            nextHasMore = false
        }

        let changed = blocksChanged
            || slot.timeline != nextTimeline
            || slot.subagents != detail.subagents
            || slot.summary != nextSummary
            || slot.totalBlocks != nextTotal
            || slot.hasMoreBlocks != nextHasMore
            || slot.beforeCursor != nextBefore

        slot.timeline = nextTimeline
        slot.history = detail.history
        slot.summary = nextSummary
        slot.subagents = detail.subagents
        slot.subagentUpdates = detail.subagentUpdates
        slot.blocks = merged
        slot.totalBlocks = nextTotal
        slot.hasMoreBlocks = nextHasMore
        slot.beforeCursor = nextBefore
        if changed { slot.visibleUpdateRevision += 1 }
        if blocksChanged { slot.bottomFollowRevision += 1 }

        slots[threadId] = slot
        if activeThreadId == threadId && changed {
            revision &+= 1
        }
        return slot
    }

    @discardableResult
    public func applyBlockPage(_ page: ThreadBlockPage, expectedCursor: String? = nil) -> ThreadMessageSlot {
        let threadId = page.threadId
        var slot = self.slot(for: threadId)
        if let sh = slot.history, let ph = page.history, sh.epoch != ph.epoch {
            return slot
        }
        if let expectedCursor, let sb = slot.beforeCursor, sb != expectedCursor {
            return slot
        }

        let merged = Self.mergeSubagentUpdates(
            Self.mergeBlocks(existing: slot.blocks, incoming: page.blocks, mode: .prepend),
            updates: slot.subagentUpdates
        )
        let changed = (merged != slot.blocks)
        slot.blocks = merged
        slot.totalBlocks = max(page.totalBlocks, merged.count)
        slot.hasMoreBlocks = page.hasMoreBlocks
        slot.beforeCursor = HistoryAndResumeHelpers.stableHistoryCursor(
            history: page.history ?? slot.history,
            blocks: page.blocks,
            beforeCursor: page.beforeCursor
        )
        slot.loadingEarlier = false
        slot.loadError = nil
        if changed { slot.visibleUpdateRevision += 1 }
        slots[threadId] = slot
        if activeThreadId == threadId { revision &+= 1 }
        return slot
    }

    public func setHistoryExpanded(threadId: String, expanded: Bool) {
        var slot = self.slot(for: threadId)
        slot.showAllHistory = expanded
        slot.visibleUpdateRevision += 1
        slots[threadId] = slot
        if activeThreadId == threadId { revision &+= 1 }
    }

    public func clear(threadId: String) {
        slots.removeValue(forKey: threadId)
        optimisticTitles.removeValue(forKey: threadId)
        if activeThreadId == threadId { activeThreadId = nil }
        revision &+= 1
    }

    private enum MergeMode {
        case prepend
        case appendOrUpdate
    }

    private static func mergeBlocks(
        existing: [MessageBlock],
        incoming: [MessageBlock],
        mode: MergeMode
    ) -> [MessageBlock] {
        if existing.isEmpty { return incoming }
        if incoming.isEmpty { return existing }

        switch mode {
        case .prepend:
            var existingById: [String: Int] = [:]
            for (idx, b) in existing.enumerated() { existingById[b.id] = idx }
            var updatedExisting = existing
            var prepended: [MessageBlock] = []
            for b in incoming {
                if let idx = existingById[b.id] {
                    updatedExisting[idx] = b
                } else {
                    prepended.append(b)
                }
            }
            return prepended + updatedExisting

        case .appendOrUpdate:
            var out = existing
            var indexById: [String: Int] = [:]
            for (idx, b) in out.enumerated() { indexById[b.id] = idx }
            for b in incoming {
                if let idx = indexById[b.id] {
                    out[idx] = b
                } else {
                    indexById[b.id] = out.count
                    out.append(b)
                }
            }
            return out
        }
    }

    private static func mergeSubagentUpdates(
        _ blocks: [MessageBlock],
        updates: [String: SubagentActivity]?
    ) -> [MessageBlock] {
        guard let updates, !updates.isEmpty else { return blocks }
        return blocks.map { block in
            guard let sub = block.subagent else { return block }
            if let key = sub.eventId ?? sub.agentId, let nextSub = updates[key] {
                var copy = block
                copy.subagent = nextSub
                return copy
            }
            return block
        }
    }
}
