import SwiftUI

public struct JobHistorySectionView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public var title: String

    @State private var isExpanded: Bool = false
    @State private var jobs: [JobRecord] = []
    @State private var errorMessage: String?
    @State private var expandedJobIds: Set<String> = []

    public init(store: RuntimeConnectionStore, title: String = "执行记录") {
        self.store = store
        self.title = title
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Button {
                isExpanded.toggle()
            } label: {
                HStack(spacing: 8) {
                    LucideIcon(isExpanded ? .chevronDown : .chevronRight, size: 15)
                        .foregroundStyle(palette.textMuted.color)
                    Text(title)
                        .font(DesignFonts.body(13, weight: .semibold))
                        .foregroundStyle(palette.textPrimary.color)
                    if !jobs.isEmpty {
                        StatusChip("\(jobs.count)", tone: .neutral)
                    }
                    Spacer()
                }
                .padding(.vertical, 6)
            }
            .buttonStyle(PlainHitButtonStyle())

            if isExpanded {
                PanelCard(title: title, icon: .terminalSquare) {
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
        }
    }

    private func refreshJobs() async {
        do {
            let envelope: OptionalResultEnvelope<[JobRecord]> = try await store.rpc(
                "jobs.list",
                args: ["limit": .number(30)]
            )
            jobs = envelope.data ?? []
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

}

public struct JobRecordRowView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public let job: JobRecord
    public var expanded: Bool
    @State private var detail: JobRecord?
    @State private var loading = false
    @State private var outputError: String?
    public let onToggle: () -> Void

    public init(store: RuntimeConnectionStore, job: JobRecord, expanded: Bool, onToggle: @escaping () -> Void) {
        self.store = store
        self.job = job
        self.expanded = expanded
        self.onToggle = onToggle
    }

    public var body: some View {
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
                .task(id: job.status) { await fetchOutput() }
                .background(RoundedRectangle(cornerRadius: 8).fill(palette.codeBg.color))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
    }

    private func fetchOutput() async {
        repeat {
            loading = true
            do {
                let record: JobRecord? = try await store.rpc(
                    "jobs.detail", args: ["id": .string(job.id), "includeOutput": .bool(true)]
                )
                guard !Task.isCancelled else { return }
                detail = record
                outputError = nil
            } catch {
                guard !Task.isCancelled else { return }
                outputError = error.localizedDescription
            }
            loading = false
            guard (detail ?? job).status == "running" else { return }
            try? await Task.sleep(nanoseconds: 2_000_000_000)
        } while !Task.isCancelled
    }
}
