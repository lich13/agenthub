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

// MARK: - Universal Paged History Store for Claude, Grok, Antigravity, DSH & GrokBot (usePagedHistory.ts parity)

public struct PagedEventSlot<Event: Equatable & Sendable>: Equatable, Sendable {
    public var events: [Event]
    public var history: HistoryMetadata?
    public var totalEvents: Int?
    public var beforeCursor: String?
    public var hasMore: Bool
    public var timeline: [TimelineEntry]?
    public var subagents: SubagentCollection?

    public init() {
        self.events = []
        self.hasMore = false
    }
}

@MainActor
public final class PagedEventHistoryStore<Event: Equatable & Sendable>: ObservableObject {
    private var slots: [String: PagedEventSlot<Event>] = [:]
    private let idOf: (Event, Int) -> String

    public init(idOf: @escaping (Event, Int) -> String) {
        self.idOf = idOf
    }

    public func slot(for key: String) -> PagedEventSlot<Event> {
        slots[key] ?? PagedEventSlot()
    }

    public func clearAll() {
        slots.removeAll()
    }

    @discardableResult
    public func applyHeadPoll(
        key: String,
        headEvents: [Event],
        history: HistoryMetadata?,
        totalEvents: Int?,
        beforeCursor: String?,
        hasMore: Bool,
        timeline: [TimelineEntry]? = nil,
        subagents: SubagentCollection? = nil
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

        if rewritten || current.events.isEmpty {
            current.events = headEvents
            current.beforeCursor = beforeCursor
            current.hasMore = hasMore && beforeCursor != nil
        } else {
            current.events = mergeEvents(existing: current.events, incoming: headEvents, prepend: false)
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
        slots[key] = current
        return current
    }

    @discardableResult
    public func applyOlderPage(
        key: String,
        expectedCursor: String,
        olderEvents: [Event],
        history: HistoryMetadata?,
        totalEvents: Int?,
        beforeCursor: String?,
        hasMore: Bool
    ) -> PagedEventSlot<Event> {
        var current = slot(for: key)
        if let oldH = current.history, let pageH = history, oldH.epoch != pageH.epoch {
            return current
        }
        if let sb = current.beforeCursor, sb != expectedCursor {
            return current
        }
        current.events = mergeEvents(existing: current.events, incoming: olderEvents, prepend: true)
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
        slots[key] = current
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

public struct TimelineWindowPage<Item: Sendable>: Sendable {
    public var items: [Item]
    public var before: String?
    public var after: String?
    public var hasMore: Bool
    public var history: HistoryMetadata?

    public init(
        items: [Item],
        before: String? = nil,
        after: String? = nil,
        hasMore: Bool = false,
        history: HistoryMetadata? = nil
    ) {
        self.items = items
        self.before = before
        self.after = after
        self.hasMore = hasMore
        self.history = history
    }

    public static func from(threadDetail page: ThreadDetail) -> TimelineWindowPage<MessageBlock> {
        let blocks = !page.blocks.isEmpty ? page.blocks : page.messages.enumerated().map { idx, m in
            MessageBlock(id: "legacy-\(idx)", role: m.role, kind: m.kind, text: m.text, createdAt: m.createdAt)
        }
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
            history: page.history
        )
    }

    public static func from(claudeDetail page: ClaudeSessionDetail) -> TimelineWindowPage<ClaudeHistoryEvent> {
        TimelineWindowPage<ClaudeHistoryEvent>(
            items: page.events,
            before: page.beforeCursor,
            after: page.afterCursor,
            hasMore: page.hasMore,
            history: page.history
        )
    }

    public static func from(grokDetail page: GrokSessionDetail) -> TimelineWindowPage<GrokHistoryEvent> {
        TimelineWindowPage<GrokHistoryEvent>(
            items: page.events,
            before: page.beforeCursor,
            after: page.afterCursor,
            hasMore: page.hasMore ?? false,
            history: page.history
        )
    }

    public static func from(providerDetail page: ProviderSessionDetail) -> TimelineWindowPage<ProviderHistoryEvent> {
        TimelineWindowPage<ProviderHistoryEvent>(
            items: page.events,
            before: page.beforeCursor,
            after: page.afterCursor,
            hasMore: page.hasMore,
            history: page.history
        )
    }
}

@MainActor
public final class TimelineWindowController<Item: Sendable>: ObservableObject {
    @Published public private(set) var scope: String = ""
    @Published public private(set) var pages: [TimelineWindowPage<Item>] = []
    @Published public private(set) var windowHistory: HistoryMetadata?
    @Published public private(set) var busy: Bool = false
    @Published public private(set) var error: String?
    @Published public private(set) var pendingTargetId: String?
    @Published public private(set) var generation: Int = 0

    private let idOf: (Item) -> String
    private var sequence: Int = 0

    public init(idOf: @escaping (Item) -> String) {
        self.idOf = idOf
    }

    public var isActive: Bool {
        !pages.isEmpty
    }

    public var beforeCursor: String? {
        pages.first?.before
    }

    public var hasMoreOlder: Bool {
        pages.first?.hasMore ?? false
    }

    public func hasNewer(latestTailId: String?) -> Bool {
        guard let lastPage = pages.last else { return false }
        if let after = lastPage.after, !after.isEmpty {
            return true
        }
        if let lastItem = lastPage.items.last,
           let latestTailId,
           !latestTailId.isEmpty,
           idOf(lastItem) != latestTailId
        {
            return true
        }
        return false
    }

    public var items: [Item]? {
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

    public func syncScope(_ nextScope: String, currentHistory: HistoryMetadata?) {
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

    public func jump(
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

    public func clearPendingTarget() {
        pendingTargetId = nil
    }

    public func loadNewer(
        latestTailId: String?,
        read: @Sendable (String) async throws -> TimelineWindowPage<Item>
    ) async {
        guard isActive, !busy, let lastPage = pages.last else { return }
        let lastId = lastPage.items.last.map(idOf)
        let cursor: String? = if let after = lastPage.after, !after.isEmpty {
            after
        } else if let lastId, let latestTailId, lastId != latestTailId {
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
            pages.append(page)
            if let h = page.history { windowHistory = h }
        } catch {
            guard token == sequence else { return }
            self.error = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    public func loadOlder(
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
            pages.insert(page, at: 0)
        } catch {
            guard token == sequence else { return }
            self.error = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    public func reconcilePollHistory(
        incoming: HistoryMetadata?,
        read: @Sendable (String) async throws -> TimelineWindowPage<Item>
    ) async {
        guard isActive, !busy, error == nil, let incoming else { return }
        if incoming.revision == windowHistory?.revision { return }

        let prevRev = windowHistory?.revision
        let prevIdx = incoming.changes?.firstIndex(where: { $0.revision == prevRev }) ?? -1
        let journalReset = prevIdx >= 0
            && (incoming.changes?[(prevIdx + 1)...].contains(where: { $0.reset == true }) ?? false)
        if incoming.reset == true || journalReset {
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
            for page in pages {
                guard token == sequence, isActive else { return }
                let affected = (changedIds == nil) || page.items.contains(where: { changedIds!.contains(idOf($0)) })
                if affected, let first = page.items.first {
                    let refreshed = try await read("w:\(idOf(first))")
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

    public func returnToLatest() {
        sequence += 1
        pages = []
        windowHistory = nil
        busy = false
        error = nil
        pendingTargetId = nil
    }
}
