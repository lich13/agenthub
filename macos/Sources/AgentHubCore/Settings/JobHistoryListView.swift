import SwiftUI

struct JobHistorySectionView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject var store: RuntimeConnectionStore
    var title: String
    private var externalExpanded: Binding<Bool>?

    @State private var internalExpanded: Bool = false
    @State private var jobs: [JobRecord] = []
    @State private var errorMessage: String?
    @State private var expandedJobIds: Set<String> = []
    @State private var lastMachineScope: String = ""

    init(
        store: RuntimeConnectionStore,
        title: String = "执行记录",
        isExpanded: Binding<Bool>? = nil
    ) {
        self.store = store
        self.title = title
        self.externalExpanded = isExpanded
    }

    private var expandedBinding: Binding<Bool> {
        externalExpanded ?? $internalExpanded
    }

    private var isExpanded: Bool {
        expandedBinding.wrappedValue
    }

    var body: some View {
        CollapsiblePanelCard(
            title: title,
            icon: .terminalSquare,
            isExpanded: expandedBinding,
            summaryBadges: {
                if !jobs.isEmpty {
                    StatusChip("\(jobs.count)", tone: .neutral)
                }
                if let latest = jobs.first, latest.status == "running" {
                    StatusChip("运行中", tone: .warning)
                } else if let latest = jobs.first, latest.status == "failed" {
                    StatusChip("失败", tone: .danger)
                }
            }
        ) {
            if let errorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }
            if jobs.isEmpty {
                Text("暂无作业记录")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.textMuted.color)
            } else {
                VStack(spacing: 8) {
                    ForEach(jobs) { job in
                        JobRecordRowView(
                            store: store,
                            job: job,
                            expanded: expandedJobIds.contains(job.id),
                            onToggle: {
                                if expandedJobIds.contains(job.id) {
                                    expandedJobIds.remove(job.id)
                                } else {
                                    expandedJobIds.insert(job.id)
                                }
                            }
                        )
                    }
                }
            }
        }
        .task(id: store.machineScope) {
            if lastMachineScope != store.machineScope {
                lastMachineScope = store.machineScope
                jobs = []
                errorMessage = nil
                expandedJobIds = []
                if externalExpanded == nil {
                    internalExpanded = false
                }
            }
        }
        .task(id: "\(store.machineScope):\(isExpanded)") {
            guard isExpanded else { return }
            await refreshJobs()
            while !Task.isCancelled && isExpanded {
                try? await Task.sleep(nanoseconds: 5_000_000_000)
                guard !Task.isCancelled && isExpanded else { break }
                if store.isWindowVisible {
                    await refreshJobs()
                }
            }
        }
    }

    private func refreshJobs() async {
        let rev = store.revision
        do {
            let rows: [JobRecord] = try await store.rpc(
                "jobs.list",
                args: ["limit": .number(30)],
                at: rev
            )
            guard store.revision == rev else { return }
            jobs = rows
            errorMessage = nil
        } catch {
            guard store.revision == rev else { return }
            errorMessage = error.localizedDescription
        }
    }
}

struct JobRecordRowView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject var store: RuntimeConnectionStore
    let job: JobRecord
    var expanded: Bool
    @State private var detail: JobRecord?
    @State private var loading = false
    @State private var outputError: String?
    let onToggle: () -> Void

    init(store: RuntimeConnectionStore, job: JobRecord, expanded: Bool, onToggle: @escaping () -> Void) {
        self.store = store
        self.job = job
        self.expanded = expanded
        self.onToggle = onToggle
    }

    var body: some View {
        let tone: StatusChipTone = switch job.status {
        case "succeeded": .success
        case "failed": .danger
        case "running": .warning
        default: .neutral
        }
        VStack(alignment: .leading, spacing: 8) {
            Button(action: onToggle) {
                HStack(spacing: 8) {
                    LucideIcon(expanded ? .chevronDown : .chevronRight, size: 14)
                        .foregroundStyle(palette.textMuted.color)
                    Text(job.title)
                        .font(DesignFonts.body(13, weight: .medium))
                        .foregroundStyle(palette.textPrimary.color)
                        .lineLimit(1)
                    Spacer(minLength: 8)
                    StatusChip(job.status, tone: tone)
                }
                .frame(minHeight: 28)
                .contentShape(Rectangle())
            }
            .buttonStyle(PlainHitButtonStyle())

            if let err = job.error, !err.isEmpty {
                Text(err)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }

            if expanded {
                if let analysis = (detail ?? job).failureAnalysis {
                    Text(analysis.explanation).font(DesignFonts.body(12))
                    ForEach(analysis.suggestions, id: \.self) { Text($0).font(DesignFonts.body(12)) }
                }
                let text = outputError ?? (loading && detail == nil ? "正在读取输出…" : (detail ?? job).output ?? "")
                ScrollView(.horizontal) {
                    Text(text.isEmpty ? "（无输出）" : text)
                        .font(DesignFonts.mono(12))
                        .foregroundStyle(palette.textSecondary.color)
                        .textSelection(.enabled)
                        .padding(10)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .task(id: "\(store.machineScope):\(job.id):\(job.status):\(store.isWindowVisible)") { await fetchOutput() }
                .background(RoundedRectangle(cornerRadius: 8).fill(palette.codeBg.color))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
    }

    private func fetchOutput() async {
        guard store.isWindowVisible || detail == nil else { return }
        let rev = store.revision
        repeat {
            loading = true
            do {
                let record: JobRecord? = try await store.rpc(
                    "jobs.detail",
                    args: ["id": .string(job.id), "includeOutput": .bool(true)],
                    at: rev
                )
                guard !Task.isCancelled, store.revision == rev else { return }
                detail = record
                outputError = nil
            } catch {
                guard !Task.isCancelled, store.revision == rev else { return }
                outputError = error.localizedDescription
            }
            loading = false
            guard (detail ?? job).status == "running", store.isWindowVisible else { return }
            try? await Task.sleep(nanoseconds: 2_000_000_000)
        } while !Task.isCancelled && store.revision == rev && store.isWindowVisible
    }
}
