import Foundation

extension UnifiedCleanupPlan {
    mutating func updateTotals() {
        totalCandidates = scopes.reduce(0) { $0 + $1.candidateCount }
        totalDeletable = scopes.filter(\.scanned).reduce(0) { $0 + $1.deletableCount }
        totalEstimatedBytes = scopes.filter(\.scanned).reduce(0) { $0 + $1.estimatedBytes }
    }

    func executionRequest(for selected: Set<String>) -> UnifiedCleanupExecuteRequest {
        let eligible = scopes.filter { selected.contains($0.scope) && $0.supported && $0.scanned && $0.deletableCount > 0 }
        let targets = eligible.flatMap { scope in
            scope.candidates.filter { $0.blockedReason == nil }.map {
                UnifiedCleanupTargetSelection(scope: scope.scope, id: $0.id, fingerprint: $0.fingerprint)
            }
        }
        return UnifiedCleanupExecuteRequest(confirmed: true, scopes: eligible.map(\.scope), targets: targets)
    }

    mutating func replaceHidden(_ scope: UnifiedCleanupScopePlan) {
        if let index = scopes.firstIndex(where: { $0.scope == scope.scope }) {
            scopes[index] = scope
        } else {
            scopes.append(scope)
        }
        updateTotals()
    }

    mutating func apply(_ result: UnifiedCleanupResult) {
        for outcome in result.scopes {
            guard outcome.deletedCount > 0,
                  let index = scopes.firstIndex(where: { $0.scope == outcome.scope }) else { continue }
            if outcome.deletedCount == scopes[index].deletableCount {
                scopes[index].candidates.removeAll { $0.blockedReason == nil }
                scopes[index].candidateCount = scopes[index].candidates.count
                scopes[index].deletableCount = 0
                scopes[index].estimatedBytes = 0
            } else {
                // The result reports counts per scope, not the IDs of individual successes.
                scopes[index].scanned = false
                scopes[index].candidates.removeAll { $0.blockedReason == nil }
                scopes[index].candidateCount = max(0, scopes[index].candidateCount - outcome.deletedCount)
                scopes[index].deletableCount = max(0, scopes[index].deletableCount - outcome.deletedCount)
                scopes[index].estimatedBytes = max(0, scopes[index].estimatedBytes - outcome.deletedBytes)
            }
        }
        updateTotals()
    }
}

struct HiddenScanRequest: Encodable {
    let mode: String
    let scanId: String?
    let cursor: Int?
    let limit = 100
}

extension RuntimeConnectionStore {
    func saveAutoClearAccountScope(_ enabled: Bool, at revision: UInt64) async throws -> Bool {
        let saved: ProbeSettingsView = try await rpc(
            "probe.settings.save",
            args: ["settings": .object(["probe": .object(["hooks": .object(["clear_account_scope": .bool(enabled)])])])],
            at: revision
        )
        return saved.probe.hooks.clearAccountScope
    }

    func clearAccountScope(at revision: UInt64, progress: (CodexAccountScopeResult) -> Void) async throws {
        var totalThreads = 0
        var totalFiles = 0
        while true {
            var batch: CodexAccountScopeResult = try await rpc("threads.accountScopeClear", at: revision)
            let shouldContinue = batch.remainingFiles > 0 && batch.clearedFiles > 0
            totalThreads += batch.clearedThreads
            totalFiles += batch.clearedFiles
            batch.clearedThreads = totalThreads
            batch.clearedFiles = totalFiles
            progress(batch)
            if !shouldContinue { return }
        }
    }
}

@MainActor
final class HiddenCleanupScan {
    typealias Send = @MainActor (HiddenScanRequest) async throws -> HiddenThreadDeletePlan
    private let send: Send
    private var startRequest: Task<HiddenThreadDeletePlan, Error>?
    private var cancelRequest: Task<Void, Never>?
    private var scanId: String?
    private var stopped = false
    private var finished = false
    private var cursor = 0

    init(send: @escaping Send) { self.send = send }

    /// A stop during start still needs the server-assigned ID to release that scan.
    @discardableResult
    func stop() -> Task<Void, Never>? {
        stopped = true
        if let cancelRequest { return cancelRequest }
        guard !finished else { return nil }
        let pending = startRequest
        let knownId = scanId
        let position = cursor
        let cancel = Task {
            let id: String?
            if let knownId { id = knownId } else { id = try? await pending?.value.scan.id }
            if let id {
                _ = try? await send(HiddenScanRequest(mode: "cancel", scanId: id, cursor: position))
            }
        }
        cancelRequest = cancel
        return cancel
    }

    func run(update: (HiddenScanProgress, UnifiedCleanupScopePlan) -> Void) async throws {
        guard !stopped else { return }
        let start = Task { try await send(HiddenScanRequest(mode: "start", scanId: nil, cursor: nil)) }
        startRequest = start
        var candidates: [UnifiedCleanupCandidate] = []
        var allowedCount = 0
        do {
            var page = try await start.value
            scanId = page.scan.id
            startRequest = nil
            while !stopped {
                let progress = page.scan
                guard progress.id == scanId, progress.cursor == cursor,
                      progress.nextCursor == cursor + page.candidates.count,
                      progress.nextCursor <= progress.processed, progress.processed <= progress.total,
                      !progress.hasMore || !page.candidates.isEmpty,
                      progress.phase != .complete || progress.processed == progress.total else {
                    throw AgentHubFFIError("扫描游标未正确推进，请重新扫描")
                }
                cursor = progress.nextCursor
                candidates.append(contentsOf: page.candidates.map(\.unified))
                allowedCount += page.candidates.filter(\.allowed).count
                let complete = progress.phase == .complete && !progress.hasMore
                guard !complete || cursor == progress.total else {
                    throw AgentHubFFIError("扫描结果不完整，请重新扫描")
                }
                update(progress, UnifiedCleanupScopePlan(
                    scope: "codex_hidden", supported: true, scanned: complete, unavailableReason: nil,
                    candidateCount: candidates.count, deletableCount: allowedCount,
                    estimatedBytes: 0, candidates: candidates, issues: []
                ))
                if progress.phase == .failed {
                    throw AgentHubFFIError(progress.error ?? "隐藏线程扫描失败")
                }
                if complete || progress.phase == .stopped { break }
                if !progress.hasMore { try await Task.sleep(nanoseconds: 250_000_000) }
                guard !stopped else { break }
                page = try await send(HiddenScanRequest(mode: "poll", scanId: progress.id, cursor: cursor))
            }
        } catch {
            await stop()?.value
            throw error
        }
        await cancelRequest?.value
        finished = true
    }
}
