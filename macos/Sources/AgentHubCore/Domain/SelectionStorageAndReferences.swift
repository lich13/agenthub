import Foundation

// MARK: - Session Storage Formatting (sessionStorage.ts)

public struct FormattedSessionStorage: Codable, Equatable, Sendable {
    public var label: String
    public var tone: String // "success" | "warning" | "danger" | "muted"
    public var title: String

    public init(label: String, tone: String, title: String) {
        self.label = label
        self.tone = tone
        self.title = title
    }
}

public enum SessionStorageFormatter {
    private static let maxExactDoubleBytes: Int64 = (1 << 53) - 1
    private static let thousandsLock = NSLock()
    private static let thousandsFormatter: NumberFormatter = {
        let formatter = NumberFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.numberStyle = .decimal
        formatter.usesGroupingSeparator = true
        formatter.groupingSeparator = ","
        formatter.groupingSize = 3
        return formatter
    }()

    public static func format(_ size: SessionStorageSize?) -> FormattedSessionStorage {
        let scope = (size?.scope == "directory") ? "会话目录" : "会话文件"
        let unknown = FormattedSessionStorage(label: "—", tone: "muted", title: "会话大小暂不可用")
        if size?.status == "pending" {
            return FormattedSessionStorage(label: "—", tone: "muted", title: "\(scope) · 正在统计")
        }
        guard let size,
              size.status == "complete" || size.status == "partial",
              let bytes = size.bytes,
              bytes >= 0,
              bytes <= maxExactDoubleBytes
        else {
            return unknown
        }

        let gib: Int64 = 1024 * 1024 * 1024
        let mib: Int64 = 1024 * 1024
        let kib: Int64 = 1024

        let unit: String
        let divisor: Double
        if bytes >= gib {
            unit = "G"
            divisor = Double(gib)
        } else if bytes >= mib {
            unit = "M"
            divisor = Double(mib)
        } else {
            unit = "K"
            divisor = Double(kib)
        }

        let tenths = floor((Double(bytes) / divisor) * 10.0)
        let amount = tenths / 10.0
        let amountStr: String = if tenths.truncatingRemainder(dividingBy: 10.0) == 0 {
            "\(Int64(tenths / 10.0))"
        } else {
            String(format: "%.1f", amount)
        }

        var label = (bytes > 0 && bytes < kib) ? "<1K" : "\(amountStr)\(unit)"
        let exact = formatThousands(bytes)
        if size.status == "partial" {
            label = (bytes > 0 && bytes < kib) ? "≥0K" : "≥\(label)"
        }

        let tone: String = if bytes >= 300 * mib {
            "danger"
        } else if bytes >= 100 * mib {
            "warning"
        } else {
            "success"
        }

        let title = (size.status == "partial")
            ? "\(scope) · 至少 \(exact) 字节（统计不完整）"
            : "\(scope) · \(exact) 字节"

        return FormattedSessionStorage(label: label, tone: tone, title: title)
    }

    public static func formatThousands(_ value: Int64) -> String {
        thousandsLock.lock()
        defer { thousandsLock.unlock() }
        return thousandsFormatter.string(from: NSNumber(value: value)) ?? "\(value)"
    }
}

// MARK: - Multi-Select Session Selection State Machine (sessionSelection.ts)

public struct SessionSelectionState: Codable, Equatable, Sendable {
    public var scope: String
    public var selecting: Bool
    public var keys: [String]
    public var anchor: String?
    public var notice: String?

    public init(
        scope: String,
        selecting: Bool = false,
        keys: [String] = [],
        anchor: String? = nil,
        notice: String? = nil
    ) {
        self.scope = scope
        self.selecting = selecting
        self.keys = keys
        self.anchor = anchor
        self.notice = notice
    }
}

public struct SessionClick: Codable, Equatable, Sendable {
    public var detail: Int
    public var metaKey: Bool
    public var shiftKey: Bool
    public var ctrlKey: Bool

    public init(detail: Int = 1, metaKey: Bool = false, shiftKey: Bool = false, ctrlKey: Bool = false) {
        self.detail = detail
        self.metaKey = metaKey
        self.shiftKey = shiftKey
        self.ctrlKey = ctrlKey
    }
}

public enum SessionSelectionEngine {
    public static func empty(scope: String) -> SessionSelectionState {
        SessionSelectionState(scope: scope, selecting: false, keys: [], anchor: nil, notice: nil)
    }

    public static func isToggleSelectionClick(_ click: SessionClick, macPlatform: Bool = true) -> Bool {
        if click.detail == 0 { return false }
        if macPlatform {
            return click.metaKey && !click.ctrlKey
        }
        return click.metaKey || click.ctrlKey
    }

    public static func sync(
        _ state: SessionSelectionState,
        scope: String,
        visible: [String]
    ) -> SessionSelectionState {
        if scope != state.scope {
            return empty(scope: scope)
        }
        let visibleSet = Set(visible)
        let keys = state.keys.filter { visibleSet.contains($0) }
        let selecting = !keys.isEmpty && state.selecting
        let anchor = (state.anchor != nil && visibleSet.contains(state.anchor!)) ? state.anchor : nil
        if keys.count == state.keys.count && selecting == state.selecting && anchor == state.anchor {
            return state
        }
        return SessionSelectionState(
            scope: state.scope,
            selecting: selecting,
            keys: keys,
            anchor: anchor,
            notice: state.notice
        )
    }

    public static func add(_ state: SessionSelectionState, keys newKeys: [String]) -> SessionSelectionState {
        var seen = Set<String>()
        var combined: [String] = []
        for k in state.keys + newKeys where !seen.contains(k) {
            seen.insert(k)
            combined.append(k)
        }
        return SessionSelectionState(
            scope: state.scope,
            selecting: !combined.isEmpty,
            keys: combined,
            anchor: state.anchor,
            notice: nil
        )
    }

    public static func click(
        _ state: SessionSelectionState,
        visible: [String],
        key: String,
        click: SessionClick,
        activeKey: String? = nil,
        macPlatform: Bool = true
    ) -> SessionSelectionState {
        if macPlatform && click.ctrlKey && click.detail != 0 {
            return state
        }
        let shift = click.detail != 0 && click.shiftKey
        let toggle = isToggleSelectionClick(click, macPlatform: macPlatform)

        if shift {
            let resolvedAnchor: String? = if let a = state.anchor, visible.contains(a) {
                a
            } else if let act = activeKey, visible.contains(act) {
                act
            } else {
                nil
            }
            let start = resolvedAnchor.flatMap { visible.firstIndex(of: $0) } ?? -1
            let end = visible.firstIndex(of: key) ?? -1
            let range: [String] = if start >= 0 && end >= 0 {
                Array(visible[min(start, end) ... max(start, end)])
            } else {
                [key]
            }
            var updated = add(state, keys: range)
            updated.anchor = (start >= 0) ? resolvedAnchor : key
            return updated
        }

        if toggle {
            if state.keys.contains(key) {
                let nextKeys = state.keys.filter { $0 != key }
                return SessionSelectionState(
                    scope: state.scope,
                    selecting: !nextKeys.isEmpty,
                    keys: nextKeys,
                    anchor: key,
                    notice: nil
                )
            }
            let seed: [String] = if !state.selecting,
                                    state.keys.isEmpty,
                                    let activeKey,
                                    activeKey != key,
                                    visible.contains(activeKey)
            {
                [activeKey, key]
            } else {
                [key]
            }
            var updated = add(state, keys: seed)
            updated.anchor = key
            return updated
        }

        // Plain click clears multi-select and opens the target session detail.
        return SessionSelectionState(
            scope: state.scope,
            selecting: false,
            keys: [],
            anchor: key,
            notice: nil
        )
    }

    public static func applyBatchSucceeded(
        _ state: SessionSelectionState,
        succeededKeys: Set<String>
    ) -> SessionSelectionState {
        let remaining = state.keys.filter { !succeededKeys.contains($0) }
        let nextAnchor = state.anchor.flatMap { succeededKeys.contains($0) ? nil : $0 }
        return SessionSelectionState(
            scope: state.scope,
            selecting: !remaining.isEmpty && state.selecting,
            keys: remaining,
            anchor: nextAnchor,
            notice: state.notice
        )
    }
}

// MARK: - Strictly Exclusive Active vs Archived Partitioned Selection & Chunked Batch Engine

public enum SessionSelectionPartition: String, Codable, Equatable, Sendable {
    case active
    case archived
}

public struct PartitionedSessionSelectionState: Codable, Equatable, Sendable {
    public var active: SessionSelectionState
    public var archived: SessionSelectionState
    public var focusPartition: SessionSelectionPartition

    public init(
        activeScope: String,
        archivedScope: String? = nil,
        focusPartition: SessionSelectionPartition = .active
    ) {
        self.active = SessionSelectionEngine.empty(scope: activeScope)
        self.archived = SessionSelectionEngine.empty(scope: archivedScope ?? "\(activeScope)#archived")
        self.focusPartition = focusPartition
    }

    public init(
        active: SessionSelectionState,
        archived: SessionSelectionState,
        focusPartition: SessionSelectionPartition = .active
    ) {
        self.active = active
        self.archived = archived
        self.focusPartition = focusPartition
    }

    public func state(for partition: SessionSelectionPartition) -> SessionSelectionState {
        switch partition {
        case .active: active
        case .archived: archived
        }
    }

    public mutating func setState(_ state: SessionSelectionState, for partition: SessionSelectionPartition) {
        switch partition {
        case .active:
            active = state
            if state.selecting || !state.keys.isEmpty {
                archived = SessionSelectionEngine.empty(scope: archived.scope)
                focusPartition = .active
            }
        case .archived:
            archived = state
            if state.selecting || !state.keys.isEmpty {
                active = SessionSelectionEngine.empty(scope: active.scope)
                focusPartition = .archived
            }
        }
    }

    public mutating func sync(
        activeScope: String,
        activeVisible: [String],
        archivedScope: String,
        archivedVisible: [String],
        archivedOpen: Bool
    ) {
        active = SessionSelectionEngine.sync(active, scope: activeScope, visible: activeVisible)
        if archivedOpen {
            archived = SessionSelectionEngine.sync(archived, scope: archivedScope, visible: archivedVisible)
        } else {
            archived = SessionSelectionEngine.empty(scope: archivedScope)
            if focusPartition == .archived {
                focusPartition = .active
            }
        }
    }

    public mutating func setArchivedOpen(_ isOpen: Bool) {
        if !isOpen {
            archived = SessionSelectionEngine.empty(scope: archived.scope)
            focusPartition = .active
        }
    }

    /// Handles row click inside `partition`, enforcing strict exclusivity between Active and Archived
    /// and never allowing Shift range selection to cross partitions.
    @discardableResult
    public mutating func click(
        partition: SessionSelectionPartition,
        visible: [String],
        key: String,
        click: SessionClick,
        activeKey: String? = nil,
        macPlatform: Bool = true
    ) -> Bool {
        if macPlatform && click.ctrlKey && click.detail != 0 {
            return false
        }
        let modified = click.detail != 0
            && (click.shiftKey || SessionSelectionEngine.isToggleSelectionClick(click, macPlatform: macPlatform))
        switch partition {
        case .active:
            archived = SessionSelectionEngine.empty(scope: archived.scope)
            focusPartition = .active
            active = SessionSelectionEngine.click(
                active,
                visible: visible,
                key: key,
                click: click,
                activeKey: activeKey,
                macPlatform: macPlatform
            )
        case .archived:
            active = SessionSelectionEngine.empty(scope: active.scope)
            focusPartition = .archived
            archived = SessionSelectionEngine.click(
                archived,
                visible: visible,
                key: key,
                click: click,
                activeKey: activeKey,
                macPlatform: macPlatform
            )
        }
        return !modified
    }

    @discardableResult
    public mutating func selectAll(
        activeVisible: [String],
        archivedVisible: [String],
        supportsArchive: Bool,
        archivedOpen: Bool
    ) -> SessionSelectionPartition {
        let targetArchive = supportsArchive
            && archivedOpen
            && (archived.selecting || !archived.keys.isEmpty || focusPartition == .archived)
        if targetArchive {
            active = SessionSelectionEngine.empty(scope: active.scope)
            focusPartition = .archived
            guard !archivedVisible.isEmpty else { return .archived }
            archived = SessionSelectionState(
                scope: archived.scope,
                selecting: true,
                keys: archivedVisible,
                anchor: archivedVisible.first,
                notice: nil
            )
            return .archived
        } else {
            archived = SessionSelectionEngine.empty(scope: archived.scope)
            focusPartition = .active
            guard !activeVisible.isEmpty else { return .active }
            active = SessionSelectionState(
                scope: active.scope,
                selecting: true,
                keys: activeVisible,
                anchor: activeVisible.first,
                notice: nil
            )
            return .active
        }
    }

    public mutating func clear(partition: SessionSelectionPartition) {
        switch partition {
        case .active:
            active = SessionSelectionEngine.empty(scope: active.scope)
        case .archived:
            archived = SessionSelectionEngine.empty(scope: archived.scope)
        }
    }

    public mutating func applyBatchResult(_ result: SessionBatchResult, partition: SessionSelectionPartition) {
        let succeeded = Set(SessionBatchChunkExecutor.confirmedSucceededKeys(from: result))
        switch partition {
        case .active:
            active = SessionSelectionEngine.applyBatchSucceeded(active, succeededKeys: succeeded)
        case .archived:
            archived = SessionSelectionEngine.applyBatchSucceeded(archived, succeededKeys: succeeded)
        }
    }
}

public struct SessionBatchExecutionOutcome: Sendable {
    public var result: SessionBatchResult?
    public var confirmedSucceededKeys: [String]
    public var errorMessage: String?

    public init(
        result: SessionBatchResult?,
        confirmedSucceededKeys: [String],
        errorMessage: String?
    ) {
        self.result = result
        self.confirmedSucceededKeys = confirmedSucceededKeys
        self.errorMessage = errorMessage
    }
}

public enum SessionBatchChunkExecutor {
    public static let archiveRestoreChunkSize = 100

    public static func hasSubagentTree(provider: SessionProvider, operation: SessionOperation) -> Bool {
        (provider == .codex || provider == .antigravity || provider == .dsh) && operation == .delete
    }

    public static func confirmedSucceededKeys(from result: SessionBatchResult) -> [String] {
        var seen = Set<String>()
        var ordered: [String] = []
        for item in result.items {
            if item.status == "succeeded", seen.insert(item.sessionKey).inserted {
                ordered.append(item.sessionKey)
            }
            for deletedId in item.deletedIds ?? [] where seen.insert(deletedId).inserted {
                ordered.append(deletedId)
            }
        }
        return ordered
    }

    public static func preview(
        provider: SessionProvider,
        operation: SessionOperation,
        sessionKeys: [String],
        sendChunk: @Sendable (SessionBatchRequest) async throws -> SessionBatchPreview
    ) async throws -> SessionBatchPreview {
        let includeDescendants = hasSubagentTree(provider: provider, operation: operation)
        let size = (operation == .delete) ? max(1, sessionKeys.count) : archiveRestoreChunkSize
        var collected: [SessionBatchItem] = []
        var start = 0
        while start < sessionKeys.count {
            let end = min(start + size, sessionKeys.count)
            let req = SessionBatchRequest(
                provider: provider,
                operation: operation,
                sessionKeys: Array(sessionKeys[start ..< end]),
                includeDescendants: includeDescendants ? true : nil
            )
            let page = try await sendChunk(req)
            collected.append(contentsOf: page.items)
            start = end
        }
        return SessionBatchPreview(provider: provider, operation: operation, items: collected)
    }

    public static func execute(
        preview: SessionBatchPreview,
        sendChunk: @Sendable (SessionBatchExecuteRequest) async throws -> SessionBatchResult
    ) async -> SessionBatchExecutionOutcome {
        let executable = preview.items.filter(\.allowed).map { item in
            SessionBatchExecuteItem(
                sessionKey: item.sessionKey,
                fingerprint: preview.operation == .delete ? nil : item.fingerprint,
                descendants: preview.operation == .delete ? item.descendants : nil
            )
        }
        let includeDescendants = hasSubagentTree(provider: preview.provider, operation: preview.operation)
        let size = (preview.operation == .delete) ? max(1, executable.count) : archiveRestoreChunkSize
        var accumulated: [SessionBatchItemResult] = []
        var start = 0
        do {
            while start < executable.count {
                let end = min(start + size, executable.count)
                let req = SessionBatchExecuteRequest(
                    provider: preview.provider,
                    operation: preview.operation,
                    items: Array(executable[start ..< end]),
                    confirmed: true,
                    includeDescendants: includeDescendants ? true : nil
                )
                let batch = try await sendChunk(req)
                accumulated.append(contentsOf: batch.items)
                start = end
            }
            let result = SessionBatchResult(items: accumulated)
            return SessionBatchExecutionOutcome(
                result: result,
                confirmedSucceededKeys: confirmedSucceededKeys(from: result),
                errorMessage: nil
            )
        } catch {
            let errText = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            if !accumulated.isEmpty {
                let remaining = Array(executable[start...])
                for (index, item) in remaining.enumerated() {
                    accumulated.append(
                        SessionBatchItemResult(
                            sessionKey: item.sessionKey,
                            status: "failed",
                            message: index < size ? "请求中断，结果未确认" : "前序请求中断，未执行"
                        )
                    )
                }
                let partial = SessionBatchResult(items: accumulated)
                return SessionBatchExecutionOutcome(
                    result: partial,
                    confirmedSucceededKeys: confirmedSucceededKeys(from: partial),
                    errorMessage: errText
                )
            }
            return SessionBatchExecutionOutcome(
                result: nil,
                confirmedSucceededKeys: [],
                errorMessage: errText
            )
        }
    }
}
