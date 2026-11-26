import Foundation

// MARK: - History Cursors & Resume Commands (historyCursor.ts, claudeSession.ts, GrokWorkspace.tsx)

enum HistoryAndResumeHelpers {
    static func historyCursorIndex(_ cursor: String?) -> Double {
        guard let cursor, !cursor.isEmpty else { return .infinity }
        let parts = cursor.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count >= 2, let num = Double(parts[1]), num.isFinite else { return .infinity }
        return num
    }

    static func stableHistoryCursor(
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

    static func threadResumeCommand(_ threadId: String?) -> String? {
        guard let id = threadId?.trimmingCharacters(in: .whitespacesAndNewlines), !id.isEmpty else {
            return nil
        }
        return "codex resume \(id)"
    }

    static func shellSingleQuote(_ value: String) -> String {
        "'\(value.replacingOccurrences(of: "'", with: "'\\''"))'"
    }

    static func claudeResumeCommand(cwd: String, id: String) -> String {
        let prefix = cwd.isEmpty ? "" : "cd -- \(shellSingleQuote(cwd)) && "
        return "\(prefix)claude --resume \(shellSingleQuote(id))"
    }

    static func grokResumeCommand(cwd: String, id: String) -> String {
        let prefix = cwd.isEmpty ? "" : "cd -- \(shellSingleQuote(cwd)) && "
        return "\(prefix)grok --resume \(shellSingleQuote(id))"
    }

    static func grokHandoffPath(_ session: GrokSessionSummary) -> String {
        if let hp = session.handoffPath?.trimmingCharacters(in: .whitespacesAndNewlines), !hp.isEmpty {
            return hp
        }
        return session.path
    }
}

// MARK: - User Timeline Rail ViewModel (timelineViewModel.ts)

struct TimelineSourceItem: Equatable, Sendable {
    var id: String
    var role: String?
    var kind: String?
    var text: String?
    var userMessage: UserMessageContent?

    init(
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

enum TimelineViewModel {
    private static let disallowedBareStems: Set<String> = [
        "turn", "process", "event", "group", "claude-process", "claude-metadata",
    ]

    private static func stripCollisionIndex(_ value: String) -> String {
        guard let colonIdx = value.lastIndex(of: ":") else { return value }
        let suffix = value[value.index(after: colonIdx)...]
        guard !suffix.isEmpty, suffix.allSatisfy(\.isNumber) else { return value }
        let base = String(value[..<colonIdx])
        guard !base.isEmpty, !disallowedBareStems.contains(base) else { return value }
        return base
    }

    static func matchesTimelinePosition(
        _ positionKey: String?,
        id: String?,
        aliases: [String] = []
    ) -> Bool {
        guard let positionKey, !positionKey.isEmpty else { return false }
        if id == positionKey || aliases.contains(positionKey) { return true }
        var candidates: [String] = []
        if let id {
            candidates.append(id)
        }
        candidates.append(contentsOf: aliases)
        for candidate in candidates {
            if candidate.hasPrefix("group:")
                || candidate.hasPrefix("process:")
                || candidate.hasPrefix("event:")
                || candidate.hasPrefix("claude-process")
                || candidate.hasPrefix("claude-metadata:")
                || candidate.contains(":process:")
                || candidate.contains(":event:")
            {
                continue
            }
            if candidate.hasPrefix("turn:"), String(candidate.dropFirst(5)) == positionKey {
                return true
            }
            let base = stripCollisionIndex(candidate)
            if base == positionKey { return true }
            if base.hasPrefix("turn:"), String(base.dropFirst(5)) == positionKey {
                return true
            }
        }
        return false
    }

    static func userTimelineEntries(
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
            let contextBlocks: [UserContextBlock]? = provider == .antigravity
                ? []
                : source.userMessage?.contextBlocks
            let segments = UserContextEngine.userMessageSegments(rawText, contextBlocks: contextBlocks)
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

    static func activeUserTimelineId(
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

struct ThreadMessageSlot: Equatable, Sendable {
    var timeline: [TimelineEntry]?
    var history: HistoryMetadata?
    var subagents: SubagentCollection?
    var subagentUpdates: [String: SubagentActivity]?
    var summary: ThreadSummary?
    var blocks: [MessageBlock]
    var totalBlocks: Int
    var hasMoreBlocks: Bool
    var beforeCursor: String?
    var visibleUpdateRevision: Int
    var bottomFollowRevision: Int
    var loadingEarlier: Bool
    var loadError: String?
    var feedback: String?
    var showAllHistory: Bool

    init() {
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
final class ThreadMessageStore: ObservableObject {
    static let optimisticRenameTTLSeconds: TimeInterval = 120.0
    static let maxCachedSessions = 8
    static let maxCachedBytes = 16 * 1024 * 1024

    @Published private(set) var revision: UInt64 = 0
    private var slots = BoundedLRUCache<String, ThreadMessageSlot>(
        maxEntries: maxCachedSessions,
        maxBytes: maxCachedBytes
    )
    private var optimisticTitles: [String: (title: String, expiresAt: Date)] = [:]

    init() {}

    var cachedSessionCount: Int { slots.count }
    var cachedBytes: Int { slots.totalBytes }

    func slot(for threadId: String) -> ThreadMessageSlot {
        slots.peek(threadId) ?? ThreadMessageSlot()
    }

    private func storeSlot(_ slot: ThreadMessageSlot, for threadId: String) {
        slots.setValue(
            slot,
            forKey: threadId,
            cost: Self.estimateSlotBytes(slot),
            allowSingleOversized: true
        )
    }

    private static func estimateSlotBytes(_ slot: ThreadMessageSlot) -> Int {
        var total = 256 + (slot.timeline?.count ?? 0) * 96
        for b in slot.blocks {
            total += 128
            total += (b.text?.utf8.count ?? 0)
            total += (b.summary?.utf8.count ?? 0)
            total += (b.input?.utf8.count ?? 0)
            if let um = b.userMessage {
                total += um.text.utf8.count + (um.contextBlocks?.reduce(0) { $0 + $1.text.utf8.count } ?? 0)
            }
        }
        return total
    }

    func recordOptimisticTitle(threadId: String, title: String, now: Date = Date()) {
        optimisticTitles[threadId] = (title: title, expiresAt: now.addingTimeInterval(Self.optimisticRenameTTLSeconds))
        if var s = slots.value(forKey: threadId), var sum = s.summary {
            sum.title = title
            s.summary = sum
            s.visibleUpdateRevision += 1
            storeSlot(s, for: threadId)
        }
        revision &+= 1
    }

    func effectiveTitle(threadId: String, serverTitle: String, now: Date = Date()) -> String {
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
    func applyDetail(_ detail: ThreadDetail, now: Date = Date()) -> ThreadMessageSlot {
        let threadId = detail.summary.id
        var slot = self.slot(for: threadId)
        let reset: Bool = {
            if detail.history?.reset == true { return true }
            if let oldH = slot.history, let newH = detail.history {
                if oldH.epoch != newH.epoch { return true }
                if let changes = newH.changes,
                   let idx = changes.firstIndex(where: { $0.revision == oldH.revision }),
                   changes[(idx + 1)...].contains(where: { $0.reset == true })
                {
                    return true
                }
            }
            if let newTotal = detail.totalBlocks, slot.totalBlocks > 0, newTotal < slot.totalBlocks {
                return true
            }
            return false
        }()
        if reset {
            slot.blocks = []
            slot.beforeCursor = nil
            slot.hasMoreBlocks = false
        }

        let nextTimeline = detail.timeline ?? (reset ? nil : slot.timeline)
        let incoming = detail.blocks

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

        storeSlot(slot, for: threadId)
        if changed {
            revision &+= 1
        }
        return slot
    }

    @discardableResult
    func applyBlockPage(_ page: ThreadBlockPage, expectedCursor: String? = nil) -> ThreadMessageSlot {
        let threadId = page.threadId
        var slot = self.slot(for: threadId)
        if let sh = slot.history, let ph = page.history, sh.epoch != ph.epoch {
            if slot.loadingEarlier {
                slot.loadingEarlier = false
                storeSlot(slot, for: threadId)
            }
            return slot
        }
        if let expectedCursor, let sb = slot.beforeCursor, sb != expectedCursor {
            if slot.loadingEarlier {
                slot.loadingEarlier = false
                storeSlot(slot, for: threadId)
            }
            return slot
        }

        let merged = Self.mergeSubagentUpdates(
            Self.mergeBlocks(existing: slot.blocks, incoming: page.blocks, mode: .prepend),
            updates: slot.subagentUpdates
        )
        let changed = (merged != slot.blocks)
        slot.blocks = merged
        slot.totalBlocks = max(page.totalBlocks, merged.count)
        let resolvedBefore = HistoryAndResumeHelpers.stableHistoryCursor(
            history: page.history ?? slot.history,
            blocks: page.blocks,
            beforeCursor: page.beforeCursor
        )
        if page.blocks.isEmpty
            || resolvedBefore == nil
            || resolvedBefore == expectedCursor
            || resolvedBefore == slot.beforeCursor
        {
            slot.beforeCursor = nil
            slot.hasMoreBlocks = false
        } else {
            slot.beforeCursor = resolvedBefore
            slot.hasMoreBlocks = page.hasMoreBlocks
        }
        slot.loadingEarlier = false
        slot.loadError = nil
        if changed { slot.visibleUpdateRevision += 1 }
        storeSlot(slot, for: threadId)
        if changed { revision &+= 1 }
        return slot
    }

    func setHistoryExpanded(threadId: String, expanded: Bool) {
        var slot = self.slot(for: threadId)
        slot.showAllHistory = expanded
        slot.visibleUpdateRevision += 1
        storeSlot(slot, for: threadId)
        revision &+= 1
    }

    func clear(threadId: String) {
        _ = slots.removeValue(forKey: threadId)
        optimisticTitles.removeValue(forKey: threadId)
        revision &+= 1
    }

    func clearAll() {
        slots.removeAll()
        optimisticTitles.removeAll()
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
            if let key = sub.eventId ?? sub.agentId, var nextSub = updates[key] {
                if nextSub.delegation == nil {
                    nextSub.delegation = sub.delegation
                }
                var copy = block
                copy.subagent = nextSub
                return copy
            }
            return block
        }
    }
}

// MARK: - Universal Paged History Store for Claude, Grok, Antigravity, DSH & GrokBot (usePagedHistory.ts parity)

struct PagedEventSlot<Event: Equatable & Sendable>: Equatable, Sendable {
    var events: [Event]
    var history: HistoryMetadata?
    var totalEvents: Int?
    var beforeCursor: String?
    var hasMore: Bool
    var timeline: [TimelineEntry]?
    var subagents: SubagentCollection?
    var subagentUpdates: [String: SubagentActivity]?

    init() {
        self.events = []
        self.hasMore = false
    }
}

@MainActor
final class PagedEventHistoryStore<Event: Equatable & Sendable>: ObservableObject {
    static var maxCachedSessions: Int { 8 }
    static var maxCachedBytes: Int { 16 * 1024 * 1024 }

    private var slots = BoundedLRUCache<String, PagedEventSlot<Event>>(
        maxEntries: 8,
        maxBytes: 16 * 1024 * 1024
    )
    private let idOf: (Event, Int) -> String
    private let mergeSubagentUpdate: ((Event, [String: SubagentActivity]) -> Event)?

    var cachedSessionCount: Int { slots.count }
    var cachedBytes: Int { slots.totalBytes }

    init(
        mergeSubagentUpdate: ((Event, [String: SubagentActivity]) -> Event)? = nil,
        idOf: @escaping (Event, Int) -> String
    ) {
        self.mergeSubagentUpdate = mergeSubagentUpdate
        self.idOf = idOf
    }

    static func forProviderEvents() -> PagedEventHistoryStore<ProviderHistoryEvent> {
        PagedEventHistoryStore<ProviderHistoryEvent>(
            mergeSubagentUpdate: { ev, updates in
                guard let sub = ev.subagent,
                      let key = sub.eventId ?? Optional(ev.id),
                      var nextSub = updates[key] ?? (sub.agentId.flatMap { updates[$0] })
                else { return ev }
                if nextSub.delegation == nil {
                    nextSub.delegation = sub.delegation
                }
                var copy = ev
                copy.subagent = nextSub
                return copy
            },
            idOf: { ev, _ in ev.id }
        )
    }

    func slot(for key: String) -> PagedEventSlot<Event> {
        slots.peek(key) ?? PagedEventSlot()
    }

    func clearAll() {
        slots.removeAll()
    }

    private func storeSlot(_ slot: PagedEventSlot<Event>, for key: String) {
        slots.setValue(
            slot,
            forKey: key,
            cost: Self.estimateSlotBytes(slot),
            allowSingleOversized: true
        )
    }

    private static func estimateSlotBytes(_ slot: PagedEventSlot<Event>) -> Int {
        var total = 256 + (slot.timeline?.count ?? 0) * 96
        if let claudeEvents = slot.events as? [ClaudeHistoryEvent] {
            for ev in claudeEvents {
                total += 128 + (ev.text?.utf8.count ?? 0) + (ev.detail?.utf8.count ?? 0) + (ev.result?.utf8.count ?? 0)
            }
        } else if let grokEvents = slot.events as? [GrokHistoryEvent] {
            for ev in grokEvents {
                total += 128 + (ev.text?.utf8.count ?? 0) + (ev.detail?.utf8.count ?? 0)
            }
        } else if let provEvents = slot.events as? [ProviderHistoryEvent] {
            for ev in provEvents {
                total += 128 + (ev.text?.utf8.count ?? 0) + (ev.detail?.utf8.count ?? 0) + (ev.result?.utf8.count ?? 0)
            }
        } else {
            total += slot.events.count * 256
        }
        return total
    }

    private func applyUpdates(_ events: [Event], updates: [String: SubagentActivity]?) -> [Event] {
        guard let mergeSubagentUpdate, let updates, !updates.isEmpty else { return events }
        return events.map { mergeSubagentUpdate($0, updates) }
    }

    @discardableResult
    func applyHeadPoll(
        key: String,
        headEvents: [Event],
        history: HistoryMetadata?,
        totalEvents: Int?,
        beforeCursor: String?,
        hasMore: Bool,
        timeline: [TimelineEntry]? = nil,
        subagents: SubagentCollection? = nil,
        subagentUpdates: [String: SubagentActivity]? = nil
    ) -> PagedEventSlot<Event> {
        var current = slot(for: key)
        let rewritten: Bool = {
            if history?.reset == true { return true }
            if let oldH = current.history, let newH = history {
                if oldH.epoch != newH.epoch { return true }
                if let changes = newH.changes,
                   let idx = changes.firstIndex(where: { $0.revision == oldH.revision }),
                   changes[(idx + 1)...].contains(where: { $0.reset == true })
                {
                    return true
                }
            }
            if let oldTotal = current.totalEvents, let newTotal = totalEvents, newTotal < oldTotal {
                return true
            }
            return false
        }()

        let nextUpdates = subagentUpdates ?? (rewritten ? nil : current.subagentUpdates)
        if rewritten || current.events.isEmpty {
            current.events = applyUpdates(headEvents, updates: nextUpdates)
            current.beforeCursor = beforeCursor
            current.hasMore = hasMore && beforeCursor != nil
        } else {
            let merged = mergeEvents(existing: current.events, incoming: headEvents, prepend: false)
            current.events = applyUpdates(merged, updates: nextUpdates)
            if let totalEvents, current.events.count >= totalEvents {
                current.beforeCursor = nil
                current.hasMore = false
            } else if current.beforeCursor == nil, let beforeCursor {
                current.beforeCursor = beforeCursor
                current.hasMore = hasMore
            }
        }
        current.history = history
        current.totalEvents = totalEvents ?? max(current.totalEvents ?? 0, current.events.count)
        current.timeline = timeline ?? (rewritten ? nil : current.timeline)
        current.subagents = subagents ?? (rewritten ? nil : current.subagents)
        current.subagentUpdates = nextUpdates
        storeSlot(current, for: key)
        return current
    }

    @discardableResult
    func applyOlderPage(
        key: String,
        expectedCursor: String,
        olderEvents: [Event],
        history: HistoryMetadata?,
        totalEvents: Int?,
        beforeCursor: String?,
        hasMore: Bool,
        subagentUpdates: [String: SubagentActivity]? = nil
    ) -> PagedEventSlot<Event> {
        var current = slot(for: key)
        if let oldH = current.history, let pageH = history, oldH.epoch != pageH.epoch {
            return current
        }
        if let sb = current.beforeCursor, sb != expectedCursor {
            return current
        }
        let nextUpdates = subagentUpdates ?? current.subagentUpdates
        let merged = mergeEvents(existing: current.events, incoming: olderEvents, prepend: true)
        current.events = applyUpdates(merged, updates: nextUpdates)
        current.subagentUpdates = nextUpdates
        if let totalEvents {
            current.totalEvents = max(totalEvents, current.events.count)
        }
        // Stop pagination if cursor does not advance or page is empty (AGENTS.md:130)
        if olderEvents.isEmpty || beforeCursor == nil || beforeCursor == expectedCursor {
            current.beforeCursor = nil
            current.hasMore = false
        } else {
            current.beforeCursor = beforeCursor
            current.hasMore = hasMore
        }
        storeSlot(current, for: key)
        return current
    }

    private func mergeEvents(existing: [Event], incoming: [Event], prepend: Bool) -> [Event] {
        if existing.isEmpty { return incoming }
        if incoming.isEmpty { return existing }

        var existingIndexById: [String: Int] = [:]
        for (idx, ev) in existing.enumerated() {
            existingIndexById[idOf(ev, idx)] = idx
        }

        if prepend {
            var updatedExisting = existing
            var prepended: [Event] = []
            for (idx, ev) in incoming.enumerated() {
                let id = idOf(ev, idx)
                if let matchIdx = existingIndexById[id] {
                    updatedExisting[matchIdx] = ev
                } else {
                    prepended.append(ev)
                }
            }
            return prepended + updatedExisting
        } else {
            var out = existing
            for (idx, ev) in incoming.enumerated() {
                let id = idOf(ev, existing.count + idx)
                if let matchIdx = existingIndexById[id] {
                    out[matchIdx] = ev
                } else {
                    existingIndexById[id] = out.count
                    out.append(ev)
                }
            }
            return out
        }
    }
}

// MARK: - Bounded Timeline Window Jump Controller (j:/w:/n: Cursors, useTimelineWindow.ts)

struct TimelineWindowPage<Item: Sendable>: Sendable {
    var items: [Item]
    var before: String?
    var after: String?
    var hasMore: Bool
    var history: HistoryMetadata?
    var firstNativeId: String?
    var lastNativeId: String?
    var nativeIds: Set<String>?

    init(
        items: [Item],
        before: String? = nil,
        after: String? = nil,
        hasMore: Bool = false,
        history: HistoryMetadata? = nil,
        firstNativeId: String? = nil,
        lastNativeId: String? = nil,
        nativeIds: Set<String>? = nil
    ) {
        self.items = items
        self.before = before
        self.after = after
        self.hasMore = hasMore
        self.history = history
        self.firstNativeId = firstNativeId
        self.lastNativeId = lastNativeId
        self.nativeIds = nativeIds
    }

    static func from(threadDetail page: ThreadDetail) -> TimelineWindowPage<MessageBlock> {
        let blocks = page.blocks
        let stableBefore = HistoryAndResumeHelpers.stableHistoryCursor(
            history: page.history,
            blocks: blocks,
            beforeCursor: page.beforeCursor
        )
        return TimelineWindowPage<MessageBlock>(
            items: blocks,
            before: stableBefore,
            after: page.afterCursor,
            hasMore: page.hasMoreBlocks ?? false,
            history: page.history,
            firstNativeId: blocks.first?.id,
            lastNativeId: blocks.last?.id,
            nativeIds: Set(blocks.map(\.id))
        )
    }

    static func from(claudeDetail page: ClaudeSessionDetail) -> TimelineWindowPage<ClaudeHistoryEvent> {
        TimelineWindowPage<ClaudeHistoryEvent>(
            items: page.events,
            before: page.beforeCursor,
            after: page.afterCursor,
            hasMore: page.hasMore,
            history: page.history,
            firstNativeId: page.events.first?.id,
            lastNativeId: page.events.last?.id,
            nativeIds: Set(page.events.map(\.id))
        )
    }

    static func from(grokDetail page: GrokSessionDetail) -> TimelineWindowPage<GrokHistoryEvent> {
        let ids = page.events.compactMap { $0.id ?? $0.userMessage?.id }.filter { !$0.isEmpty }
        return TimelineWindowPage<GrokHistoryEvent>(
            items: page.events,
            before: page.beforeCursor,
            after: page.afterCursor,
            hasMore: page.hasMore ?? false,
            history: page.history,
            firstNativeId: ids.first,
            lastNativeId: ids.last,
            nativeIds: Set(ids)
        )
    }

    static func from(providerDetail page: ProviderSessionDetail) -> TimelineWindowPage<ProviderHistoryEvent> {
        TimelineWindowPage<ProviderHistoryEvent>(
            items: page.events,
            before: page.beforeCursor,
            after: page.afterCursor,
            hasMore: page.hasMore,
            history: page.history,
            firstNativeId: page.events.first?.id,
            lastNativeId: page.events.last?.id,
            nativeIds: Set(page.events.map(\.id))
        )
    }
}

@MainActor
final class TimelineWindowController<Item: Sendable>: ObservableObject {
    @Published private(set) var scope: String = ""
    @Published private(set) var pages: [TimelineWindowPage<Item>] = []
    @Published private(set) var windowHistory: HistoryMetadata?
    @Published private(set) var busy: Bool = false
    @Published private(set) var error: String?
    @Published private(set) var pendingTargetId: String?
    @Published private(set) var generation: Int = 0

    private let idOf: (Item) -> String
    private var sequence: Int = 0

    init(idOf: @escaping (Item) -> String) {
        self.idOf = idOf
    }

    var isActive: Bool {
        !pages.isEmpty
    }

    var beforeCursor: String? {
        pages.first?.before
    }

    var hasMoreOlder: Bool {
        pages.first?.hasMore ?? false
    }

    func hasNewer(latestTailId: String?) -> Bool {
        guard let lastPage = pages.last else { return false }
        if let after = lastPage.after, !after.isEmpty {
            return true
        }
        let lastId = lastPage.lastNativeId ?? lastPage.items.last.map(idOf)
        if let lastId,
           !lastId.isEmpty,
           let latestTailId,
           !latestTailId.isEmpty,
           lastId != latestTailId
        {
            return true
        }
        return false
    }

    var items: [Item]? {
        guard isActive else { return nil }
        var indexById: [String: Int] = [:]
        var deduped: [Item] = []
        for page in pages {
            for item in page.items {
                let itemId = idOf(item)
                if let existingIdx = indexById[itemId] {
                    deduped[existingIdx] = item
                } else {
                    indexById[itemId] = deduped.count
                    deduped.append(item)
                }
            }
        }
        return deduped
    }

    func syncScope(_ nextScope: String, currentHistory: HistoryMetadata?) {
        let epochMismatch = (windowHistory != nil && currentHistory != nil && windowHistory?.epoch != currentHistory?.epoch)
        if nextScope != scope || epochMismatch {
            sequence += 1
            scope = nextScope
            pages = []
            windowHistory = nil
            busy = false
            error = nil
            pendingTargetId = nil
        }
    }

    func jump(
        to targetId: String,
        in expectedScope: String,
        currentHistory: HistoryMetadata?,
        matchesTarget: ((Item, String) -> Bool)? = nil,
        read: @Sendable (String) async throws -> TimelineWindowPage<Item>
    ) async {
        syncScope(expectedScope, currentHistory: currentHistory)
        sequence += 1
        let token = sequence
        busy = true
        error = nil
        defer {
            if token == sequence { busy = false }
        }
        do {
            let page = try await read("j:\(targetId)")
            guard token == sequence, scope == expectedScope else { return }
            let matcher = matchesTarget ?? { [idOf] item, id in
                let raw = idOf(item)
                return raw == id || raw.hasSuffix(":\(id)") || id.hasSuffix(":\(raw)")
            }
            guard page.items.contains(where: { matcher($0, targetId) }) else {
                throw NSError(
                    domain: "AgentHub.TimelineWindow",
                    code: 404,
                    userInfo: [NSLocalizedDescriptionKey: "历史位置已变化，请刷新会话"]
                )
            }
            if let pageHistory = page.history, let currentHistory, pageHistory.epoch != currentHistory.epoch {
                throw NSError(
                    domain: "AgentHub.TimelineWindow",
                    code: 409,
                    userInfo: [NSLocalizedDescriptionKey: "会话来源已变化，请刷新后重试"]
                )
            }
            pages = [page]
            windowHistory = page.history ?? currentHistory
            generation = token
            pendingTargetId = targetId
        } catch {
            guard token == sequence else { return }
            self.error = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    func clearPendingTarget() {
        pendingTargetId = nil
    }

    func clearError() {
        error = nil
    }

    func loadNewer(
        latestTailId: String?,
        read: @Sendable (String) async throws -> TimelineWindowPage<Item>
    ) async {
        guard isActive, !busy, let lastPage = pages.last else { return }
        let lastId = lastPage.lastNativeId ?? lastPage.items.last.map(idOf)
        let cursor: String? = if let after = lastPage.after, !after.isEmpty {
            after
        } else if let lastId, !lastId.isEmpty, let latestTailId, lastId != latestTailId {
            "n:\(lastId)"
        } else {
            nil
        }
        guard let cursor else { return }
        sequence += 1
        let token = sequence
        busy = true
        error = nil
        defer {
            if token == sequence { busy = false }
        }
        do {
            let page = try await read(cursor)
            guard token == sequence, isActive else { return }
            if let oldH = windowHistory, let newH = page.history, oldH.epoch != newH.epoch {
                throw NSError(
                    domain: "AgentHub.TimelineWindow",
                    code: 409,
                    userInfo: [NSLocalizedDescriptionKey: "会话来源已变化，请重新定位"]
                )
            }
            if page.items.isEmpty || page.after == cursor {
                if !pages.isEmpty {
                    pages[pages.count - 1].after = nil
                }
                if !page.items.isEmpty {
                    var normalized = page
                    normalized.after = nil
                    pages.append(normalized)
                }
            } else {
                pages.append(page)
            }
            if let h = page.history { windowHistory = h }
        } catch {
            guard token == sequence else { return }
            self.error = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    func loadOlder(
        read: @Sendable (String) async throws -> TimelineWindowPage<Item>
    ) async {
        guard isActive, !busy, hasMoreOlder, let cursor = beforeCursor else { return }
        sequence += 1
        let token = sequence
        busy = true
        error = nil
        defer {
            if token == sequence { busy = false }
        }
        do {
            let page = try await read(cursor)
            guard token == sequence, isActive else { return }
            if let oldH = windowHistory, let newH = page.history, oldH.epoch != newH.epoch {
                throw NSError(
                    domain: "AgentHub.TimelineWindow",
                    code: 409,
                    userInfo: [NSLocalizedDescriptionKey: "会话来源已变化，请重新定位"]
                )
            }
            if page.items.isEmpty || page.before == nil || page.before == cursor {
                if page.items.isEmpty {
                    if !pages.isEmpty {
                        pages[0].before = nil
                        pages[0].hasMore = false
                    }
                } else {
                    var terminated = page
                    terminated.before = nil
                    terminated.hasMore = false
                    pages.insert(terminated, at: 0)
                }
            } else {
                pages.insert(page, at: 0)
            }
            if let h = page.history { windowHistory = h }
        } catch {
            guard token == sequence else { return }
            self.error = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    func reconcilePollHistory(
        incoming: HistoryMetadata?,
        read: @Sendable (String) async throws -> TimelineWindowPage<Item>
    ) async {
        guard isActive, !busy, error == nil, let incoming else { return }
        if incoming.revision == windowHistory?.revision { return }

        let prevRev = windowHistory?.revision
        let epochChanged = (windowHistory != nil && !incoming.epoch.isEmpty && windowHistory?.epoch != incoming.epoch)
        let prevIdx = incoming.changes?.firstIndex(where: { $0.revision == prevRev }) ?? -1
        let journalReset = prevIdx >= 0
            && (incoming.changes?[(prevIdx + 1)...].contains(where: { $0.reset == true }) ?? false)
        if incoming.reset == true || journalReset || epochChanged {
            returnToLatest()
            error = "历史已更新，请重新定位"
            return
        }

        let changedIds: Set<String>? = {
            guard prevRev != nil, let changes = incoming.changes, prevIdx >= 0 else { return nil }
            var ids = Set<String>()
            for change in changes[(prevIdx + 1)...] {
                for id in change.changedIds {
                    ids.insert(id)
                }
            }
            return ids
        }()

        sequence += 1
        let token = sequence
        busy = true
        defer {
            if token == sequence { busy = false }
        }
        do {
            var nextPages: [TimelineWindowPage<Item>] = []
            let expectedEpoch = incoming.epoch.isEmpty ? windowHistory?.epoch : incoming.epoch
            for page in pages {
                guard token == sequence, isActive else { return }
                let affected: Bool = if let changedIds {
                    if let nativeIds = page.nativeIds {
                        !nativeIds.isDisjoint(with: changedIds)
                    } else {
                        page.items.contains(where: { changedIds.contains(idOf($0)) })
                    }
                } else {
                    true
                }
                let anchorId = page.firstNativeId ?? page.items.first.map(idOf)
                if affected, let anchorId, !anchorId.isEmpty {
                    let refreshed = try await read("w:\(anchorId)")
                    guard token == sequence, isActive else { return }
                    if refreshed.history?.reset == true
                        || (expectedEpoch != nil && refreshed.history?.epoch.isEmpty == false && refreshed.history?.epoch != expectedEpoch)
                    {
                        returnToLatest()
                        error = "历史已更新，请重新定位"
                        return
                    }
                    nextPages.append(refreshed)
                } else {
                    nextPages.append(page)
                }
            }
            guard token == sequence, isActive else { return }
            pages = nextPages
            windowHistory = incoming
        } catch {
            guard token == sequence else { return }
            self.error = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    func returnToLatest() {
        sequence += 1
        pages = []
        windowHistory = nil
        busy = false
        error = nil
        pendingTargetId = nil
    }
}
