import SwiftUI

public struct JobHistorySectionView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public var title: String

    @State private var isExpanded: Bool = false
    @State private var jobs: [JobRecord] = []
    @State private var errorMessage: String?
    @State private var expandedJobIds: Set<String> = []
    @State private var loadedOutputs: [String: String] = [:]

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
                                    cachedOutput: loadedOutputs[job.id],
                                    onToggle: {
                                        if expandedJobIds.contains(job.id) {
                                            expandedJobIds.remove(job.id)
                                        } else {
                                            expandedJobIds.insert(job.id)
                                            Task { await fetchJobOutput(id: job.id) }
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

    private func fetchJobOutput(id: String) async {
        if let envelope: OptionalResultEnvelope<JobRecord> = try? await store.rpc(
            "jobs.detail",
            args: ["id": .string(id), "includeOutput": .bool(true)]
        ), let detail = envelope.data, let out = detail.output {
            loadedOutputs[id] = out
        }
    }
}

public struct JobRecordRowView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public let job: JobRecord
    public var expanded: Bool
    public var cachedOutput: String?
    public let onToggle: () -> Void

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
                let text = cachedOutput ?? job.output ?? "正在读取输出…"
                ScrollView(.horizontal) {
                    Text(text.isEmpty ? "（无输出）" : text)
                        .font(DesignFonts.mono(12))
                        .foregroundStyle(palette.textSecondary.color)
                        .textSelection(.enabled)
                        .padding(10)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 8).fill(palette.codeBg.color))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(palette.borderSubtle.color, lineWidth: 1))
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
    }
}
