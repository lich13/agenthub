import AppKit
import SwiftUI

// MARK: - New Local Codex Thread Button (choose("project") -> create)

public struct NewDesktopThreadButton: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var connection: RuntimeConnectionStore
    public let onCreated: (String) -> Void

    @State private var busy: Bool = false
    @State private var uncertainOutcome: Bool = false
    @State private var pendingCreatedId: String?
    @State private var errorText: String?

    public init(connection: RuntimeConnectionStore, onCreated: @escaping (String) -> Void) {
        self.connection = connection
        self.onCreated = onCreated
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button {
                Task { await createThread() }
            } label: {
                HStack(spacing: 6) {
                    LucideIcon(.folderPlus, size: 15)
                    Text("新建 Codex 线程")
                        .font(DesignFonts.body(12, weight: .semibold))
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(SecondaryButtonStyle())
            .disabled(busy || uncertainOutcome)

            if let errorText {
                Text(errorText)
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.errFg.color)
            }
        }
    }

    private func createThread() async {
        guard !busy, !uncertainOutcome else { return }
        busy = true
        errorText = nil
        defer { busy = false }
        do {
            if pendingCreatedId == nil {
                struct ChooseWrap: Encodable {
                    struct Inner: Encodable { let revision: UInt64; let sessionId: String?; let kind: String }
                    let request: Inner
                }
                let choice: DesktopChoice = try await connection.rpc(
                    "codex.desktop.choose",
                    args: ChooseWrap(request: .init(revision: connection.revision, sessionId: nil, kind: "project")),
                    localOnly: true
                )
                guard let chosen = choice.item else { return }

                struct CreateWrap: Encodable {
                    struct Inner: Encodable { let revision: UInt64; let projectHandle: String }
                    let request: Inner
                }
                let created: DesktopCreatedThread = try await connection.rpc(
                    "codex.desktop.create",
                    args: CreateWrap(request: .init(revision: connection.revision, projectHandle: chosen.handle)),
                    localOnly: true
                )
                pendingCreatedId = created.threadId
            }
            if let id = pendingCreatedId {
                onCreated(id)
                pendingCreatedId = nil
            }
        } catch {
            let msg = error.localizedDescription
            if msg.contains("结果未确认") {
                uncertainOutcome = true
            }
            errorText = msg
        }
    }
}

// MARK: - Complete Local Codex Collaboration Footer (Composer, Outbox, Requests & Settings)

public struct DesktopCollaborationFooterView: View {
    @Environment(\.agentHubPalette) private var palette
    @StateObject private var desktop: CodexDesktopStore
    public var onStreamUpdated: (@MainActor () -> Void)?

    @State private var draft = ReferenceDraft()
    @State private var caretStart: Int = 0
    @State private var caretEnd: Int = 0
    @State private var targetCaretToRestore: Int?
    @State private var dismissedInlineKey: String = ""
    @State private var inlineItems: [DesktopCatalogItem] = []
    @State private var inlineNextCursors: [String: String] = [:]
    @State private var inlineErrors: [String: String] = [:]
    @State private var inlineBusy: Bool = false
    @State private var selectedOptionIndex: Int = 0
    @State private var catalogPickerKind: String = ""
    @State private var catalogPickerQuery: String = ""
    @State private var pickerItems: [DesktopCatalogItem] = []
    @State private var pickerNextCursors: [String: String] = [:]
    @State private var pickerErrors: [String: String] = [:]
    @State private var pickerBusy: Bool = false
    @State private var discardMessageId: String?
    @State private var copyDiagState: String = "idle"
    @State private var localError: String?
    @State private var idempotentSendState: (payloadKey: String, messageId: String)?
    @State private var settingsPopoverOpen: Bool = false
    @State private var modelCatalog: [DesktopCatalogItem] = []
    @State private var modeCatalog: [DesktopCatalogItem] = []
    @State private var modelCatalogError: String?
    @State private var modeCatalogError: String?
    @State private var settingsCatalogLoading: Bool = false
    @State private var settingsNotice: String?
    @State private var settingsError: String?
    @State private var cleanupToken: UUID?

    public init(
        connection: RuntimeConnectionStore,
        threadId: String,
        onStreamUpdated: (@MainActor () -> Void)? = nil
    ) {
        self._desktop = StateObject(wrappedValue: CodexDesktopStore(connection: connection, threadId: threadId))
        self.onStreamUpdated = onStreamUpdated
    }

    private var writable: Bool {
        desktop.view?.connected == true && desktop.view?.writable == true
    }

    private var activeInlineQuery: InlineReferenceQuery? {
        guard writable,
              catalogPickerKind.isEmpty,
              let q = DesktopReferencesEngine.inlineQuery(text: draft.text, start: caretStart, end: caretEnd),
              !draft.spans.contains(where: { $0.start <= caretStart && caretStart <= $0.end })
        else { return nil }
        let key = "\(q.kind):\(q.query):\(q.start)"
        return (dismissedInlineKey == key) ? nil : q
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            // Heading Bar: Status + Copy Diagnostics
            HStack(spacing: 8) {
                if let view = desktop.view, view.connected && view.writable {
                    Text(collaborationStatusLabel(view))
                        .font(DesignFonts.body(12, weight: .medium))
                        .foregroundStyle(palette.textSecondary.color)
                    if !view.queue.isEmpty {
                        Text("· \(view.queue.count) 条待处理")
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.textMuted.color)
                    }
                }
                Spacer()
                if copyDiagState == "copied" {
                    Text("诊断已复制").font(DesignFonts.body(11)).foregroundStyle(palette.okFg.color)
                } else if copyDiagState == "failed" {
                    Text("无法复制诊断，请重试").font(DesignFonts.body(11)).foregroundStyle(palette.errFg.color)
                }
                Button {
                    Task {
                        copyDiagState = "copying"
                        do {
                            try await desktop.copyDiagnostics()
                            copyDiagState = "copied"
                        } catch {
                            copyDiagState = "failed"
                        }
                    }
                } label: {
                    LucideIcon(copyDiagState == "copied" ? .check : .copy, size: 14)
                }
                .buttonStyle(IconButtonStyle(dimension: 26))
                .accessibilityLabel("复制诊断")
            }

            // Goal Card
            if let goal = desktop.view?.goal, desktop.view?.connected == true {
                HStack(spacing: 8) {
                    LucideIcon(.target, size: 15).foregroundStyle(palette.accent.color)
                    Text("\(goal.objective) · \(goal.status)")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textSecondary.color)
                        .lineLimit(2)
                    Spacer()
                    Button("在 Codex 中打开") { Task { await desktop.openInCodexApp() } }
                        .buttonStyle(SecondaryButtonStyle())
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 8).fill(palette.surface.color))
            }

            // Outbox Queue (queued | dispatching | held | outcome_unknown)
            if let queue = desktop.view?.queue, !queue.isEmpty {
                DesktopOutboxListView(
                    queue: queue,
                    locked: desktop.busy || !writable,
                    onCancelQueued: { id in Task { try? await desktop.act(.cancelQueued(messageId: id)) } },
                    onPromptDismissUnknown: { id in discardMessageId = id },
                    onResumeQueue: { Task { try? await desktop.act(.resumeQueue) } }
                )
            }

            if !writable {
                // Connect / Owner Confirmation Row
                HStack(spacing: 10) {
                    Text(connectBannerLabel)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textSecondary.color)
                        .lineLimit(2)
                    Spacer()
                    Button("在 Codex 中打开") { Task { await desktop.openInCodexApp() } }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(desktop.busy)
                    Button("连接") { Task { await desktop.connect(confirmOwner: true) } }
                        .buttonStyle(PrimaryButtonStyle())
                        .disabled(desktop.busy)
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 10).fill(palette.surface.color))
            } else {
                // Pending Approvals, Questions & Elicitations
                if let requests = desktop.view?.requests, !requests.isEmpty {
                    DesktopRequestsListView(
                        requests: requests,
                        busy: desktop.busy,
                        onAct: { act in Task { try? await desktop.act(act) } },
                        onOpenCodex: { Task { await desktop.openInCodexApp() } }
                    )
                }

                // <=32 Pure Filename Reference Chips
                if !draft.references.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            ForEach(draft.references) { ref in
                                HStack(spacing: 4) {
                                    LucideIcon(
                                        ref.kind == "skill" ? .wandSparkles : ((ref.kind == "app" || ref.kind == "plugin") ? .atSign : .paperclip),
                                        size: 12
                                    )
                                    Text(ref.name)
                                        .font(DesignFonts.body(11, weight: .medium))
                                        .lineLimit(1)
                                    Button {
                                        Task { await desktop.releaseAttachments(handles: [ref.handle]) }
                                        draft = DesktopReferencesEngine.remove(draft, handle: ref.handle)
                                    } label: {
                                        LucideIcon(.x, size: 11)
                                    }
                                    .buttonStyle(PlainHitButtonStyle())
                                    .disabled(desktop.busy)
                                }
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(Capsule().fill(palette.surface.color))
                                .overlay(Capsule().strokeBorder(palette.borderStrong.color, lineWidth: 1))
                            }
                        }
                    }
                }

                // Inline $ / @ Completion List
                if activeInlineQuery != nil && !inlineItems.isEmpty {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(inlineItems.prefix(8).enumerated()), id: \.element.handle) { idx, item in
                            Button {
                                commitInlineReference(item.asChosenItem)
                            } label: {
                                HStack {
                                    Text(item.name)
                                        .font(DesignFonts.body(12, weight: .semibold))
                                        .foregroundStyle(palette.textPrimary.color)
                                    Text(item.description)
                                        .font(DesignFonts.body(11))
                                        .foregroundStyle(palette.textMuted.color)
                                        .lineLimit(1)
                                    Spacer()
                                }
                                .padding(.horizontal, 8)
                                .padding(.vertical, 5)
                                .background(RoundedRectangle(cornerRadius: 6).fill((idx == selectedOptionIndex ? palette.selected : palette.surface).color))
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(PlainHitButtonStyle())
                        }
                    }
                    .padding(6)
                    .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
                }

                // Manual Resizable 54pt–160pt Composer with 3-Layer IME Guard
                ManualResizableComposerBox(
                    text: draft.text,
                    caretOffset: targetCaretToRestore,
                    placeholder: desktop.view?.activeTurnId != nil ? "添加下一条消息，或补充当前回合…" : "与 Codex 协作…",
                    isEnabled: !desktop.busy,
                    onTextAndSelectionChanged: { nextText, selStart, selEnd in
                        targetCaretToRestore = nil
                        draft = DesktopReferencesEngine.edit(draft, text: nextText)
                        caretStart = selStart
                        caretEnd = selEnd
                        selectedOptionIndex = 0
                    },
                    onCommandEnterSend: {
                        Task { await sendDraft(steer: false) }
                    },
                    onInlineCompletionKey: { keyAction in
                        guard let q = activeInlineQuery else { return false }
                        switch keyAction {
                        case .escapeDismiss:
                            dismissedInlineKey = "\(q.kind):\(q.query):\(q.start)"
                            return true
                        case .arrowDown:
                            guard !inlineItems.isEmpty else { return false }
                            let count = min(8, inlineItems.count)
                            selectedOptionIndex = (selectedOptionIndex + 1) % count
                            return true
                        case .arrowUp:
                            guard !inlineItems.isEmpty else { return false }
                            let count = min(8, inlineItems.count)
                            selectedOptionIndex = (selectedOptionIndex - 1 + count) % count
                            return true
                        case .confirmSelection:
                            let visible = Array(inlineItems.prefix(8))
                            guard visible.indices.contains(selectedOptionIndex) else { return false }
                            commitInlineReference(visible[selectedOptionIndex].asChosenItem)
                            return true
                        }
                    },
                    onInterceptPaste: { completion in
                        let currentHandles = draft.references.map(\.handle)
                        Task { @MainActor in
                            let fallbackToText = await desktop.handlePasteboardIngress(currentHandles: currentHandles)
                            completion(fallbackToText)
                        }
                    },
                    onFilesDropped: { paths, point in
                        desktop.connection.transport.desktopDropFiles(
                            paths: paths,
                            logicalX: Double(point.x),
                            logicalY: Double(point.y)
                        )
                    },
                    onWindowBoundsChanged: { bounds in
                        let handles = draft.references.map(\.handle)
                        Task { await desktop.updateAttachmentBoundsAndHandles(bounds: bounds, handles: handles) }
                    }
                )

                // Composer Bottom Toolbar (NEVER binds .keyboardShortcut for ⌘+Return!)
                HStack(spacing: 8) {
                    Menu {
                        Button("文件或图片") { Task { await chooseAndAttachFile() } }
                            .disabled(desktop.busy || draft.references.count >= 32)
                        Button("技能 ($)") { openCatalogPicker("skills") }
                            .disabled(desktop.busy || draft.references.count >= 32)
                        Button("插件与应用 (@)") { openCatalogPicker("mentions") }
                            .disabled(desktop.busy || draft.references.count >= 32)
                        Button("在 Codex 中管理目标") { Task { await desktop.openInCodexApp() } }
                            .disabled(desktop.busy)
                    } label: {
                        LucideIcon(.plus, size: 17)
                            .frame(width: 30, height: 30)
                            .contentShape(Rectangle())
                    }
                    .menuStyle(.borderlessButton)
                    .frame(width: 30, height: 30)
                    .accessibilityLabel("添加引用或附件")

                    Spacer()

                    // Model / Reasoning Effort / Mode Selector
                    Button {
                        settingsPopoverOpen.toggle()
                        if settingsPopoverOpen {
                            Task { await loadSettingsCatalogs() }
                        }
                    } label: {
                        HStack(spacing: 4) {
                            let selectedModelName = modelCatalog.first(where: { $0.handle == desktop.view?.model })?.name
                            Text(selectedModelName ?? desktop.view?.model ?? "模型设置")
                                .font(DesignFonts.body(12, weight: .medium))
                            if let eff = desktop.view?.reasoningEffort, !eff.isEmpty {
                                Text(eff).font(DesignFonts.body(11)).foregroundStyle(palette.textMuted.color)
                            }
                            if desktop.view?.mode == .plan {
                                Text("Plan").font(DesignFonts.body(11, weight: .semibold)).foregroundStyle(palette.accent.color)
                            }
                            LucideIcon(.chevronDown, size: 12)
                        }
                        .padding(.horizontal, 8)
                        .frame(height: 28)
                        .background(Capsule().fill(palette.surface.color))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(PlainHitButtonStyle())
                    .popover(isPresented: $settingsPopoverOpen) {
                        settingsPopoverContent
                    }

                    if let turnId = desktop.view?.activeTurnId {
                        Button("补充") {
                            Task { await sendDraft(steer: true) }
                        }
                        .buttonStyle(SecondaryButtonStyle())
                        .disabled(desktop.busy || (draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && draft.references.isEmpty))

                        Button {
                            Task { try? await desktop.act(.stop(turnId: turnId)) }
                        } label: {
                            LucideIcon(.square, size: 14)
                        }
                        .buttonStyle(IconButtonStyle(dimension: 30))
                        .disabled(desktop.busy)
                        .accessibilityLabel("停止当前回合并暂停队列")
                    }

                    // Layer 1 IME Guard: NO .keyboardShortcut(.return, modifiers: .command) here!
                    Button {
                        Task { await sendDraft(steer: false) }
                    } label: {
                        LucideIcon(.arrowUp, size: 17)
                            .foregroundStyle(palette.buttonFg.color)
                            .frame(width: 32, height: 32)
                            .background(Circle().fill(palette.buttonBg.color))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(PlainHitButtonStyle())
                    .disabled(desktop.busy || (draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && draft.references.isEmpty))
                    .accessibilityLabel(desktop.view?.activeTurnId != nil ? "排队发送" : "发送")
                }

                if !catalogPickerKind.isEmpty {
                    referenceCatalogPickerPanel
                }
            }

            if let err = localError ?? desktop.actionError, !err.isEmpty {
                Text(err)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background(palette.railBg.color)
        .overlay(alignment: .top) {
            Rectangle().fill(palette.borderSubtle.color).frame(height: 1)
        }
        .overlay {
            if let unknownId = discardMessageId {
                ModalConfirmDialogView(busy: desktop.busy, onCancel: { discardMessageId = nil }) {
                    Text("已在 Codex 中核对发送结果？")
                        .font(DesignFonts.body(15, weight: .semibold))
                    Text("移除此记录后，AgentHub 不会重新发送它。请先确认 Codex 中是否已有这条消息。")
                        .font(DesignFonts.body(13))
                        .foregroundStyle(palette.textSecondary.color)
                    HStack {
                        Spacer()
                        Button("返回") { discardMessageId = nil }.buttonStyle(SecondaryButtonStyle())
                        Button("已核对，移除记录") {
                            Task {
                                try? await desktop.act(.dismissUnknown(messageId: unknownId))
                                discardMessageId = nil
                            }
                        }
                        .buttonStyle(DangerButtonStyle())
                        .disabled(desktop.busy)
                    }
                }
            }
        }
        .onAppear {
            desktop.onStreamViewUpdated = {
                onStreamUpdated?()
            }
            desktop.onItemsAttached = { items in
                let additions = items.filter { item in !draft.references.contains(where: { $0.handle == item.handle }) }
                if draft.references.count + additions.count > 32 {
                    localError = "引用数量超出限制，本次附件未添加"
                    Task { await desktop.releaseAttachments(handles: additions.map(\.handle)) }
                } else {
                    localError = nil
                    draft.references.append(contentsOf: additions)
                }
            }
            desktop.onAttachmentError = { err in localError = err }
            let storeRef = desktop
            cleanupToken = desktop.connection.registerBeforeMachineChange {
                await storeRef.detachIfNeeded()
            }
            Task {
                await desktop.connect(confirmOwner: false)
                await desktop.bindAttachmentsIfNeeded(
                    bounds: desktop.lastAttachmentBounds,
                    handles: draft.references.map(\.handle)
                )
            }
        }
        .onDisappear {
            if let token = cleanupToken {
                desktop.connection.unregisterBeforeMachineChange(token)
                cleanupToken = nil
            }
            Task { await desktop.detachIfNeeded() }
        }
        .onChange(of: desktop.connection.isWindowVisible) { visible in
            if !visible {
                Task { await desktop.detachIfNeeded() }
            } else if desktop.view == nil {
                let handles = draft.references.map(\.handle)
                Task {
                    await desktop.connect(confirmOwner: false)
                    await desktop.bindAttachmentsIfNeeded(bounds: desktop.lastAttachmentBounds, handles: handles)
                }
            }
        }
        .onChange(of: draft.references.map(\.handle)) { handles in
            Task { await desktop.updateAttachmentBoundsAndHandles(handles: handles) }
        }
        .task(id: activeInlineQuery.map { "\($0.kind):\($0.query)" } ?? "") {
            guard let q = activeInlineQuery else {
                inlineItems = []
                inlineNextCursors = [:]
                inlineErrors = [:]
                inlineBusy = false
                return
            }
            try? await Task.sleep(nanoseconds: 250_000_000)
            guard !Task.isCancelled else { return }
            inlineBusy = true
            let batch = await desktop.fetchReferenceCatalog(kind: q.kind, query: q.query)
            guard !Task.isCancelled else { return }
            inlineItems = batch.items
            inlineNextCursors = batch.nextCursors
            inlineErrors = batch.errors
            inlineBusy = false
        }
        .task(id: catalogPickerKind.isEmpty ? "" : "\(catalogPickerKind):\(catalogPickerQuery)") {
            guard !catalogPickerKind.isEmpty, writable else {
                pickerItems = []
                pickerNextCursors = [:]
                pickerErrors = [:]
                pickerBusy = false
                return
            }
            try? await Task.sleep(nanoseconds: 250_000_000)
            guard !Task.isCancelled else { return }
            pickerBusy = true
            let batch = await desktop.fetchReferenceCatalog(kind: catalogPickerKind, query: catalogPickerQuery)
            guard !Task.isCancelled else { return }
            pickerItems = batch.items
            pickerNextCursors = batch.nextCursors
            pickerErrors = batch.errors
            pickerBusy = false
        }
    }

    private var connectBannerLabel: String {
        switch desktop.view?.connectionPhase {
        case "owner_confirmation_required":
            "Codex owner 已变化，已恢复阅读；点击连接确认后继续协作"
        case "resyncing":
            "正在重新同步 Codex…"
        case "reconnecting":
            "正在恢复 Codex 连接…"
        default:
            desktop.actionError ?? (desktop.busy ? "正在连接 Codex…" : "连接本机 Codex 后继续此线程")
        }
    }

    private func collaborationStatusLabel(_ view: DesktopView) -> String {
        let waiting = !view.requests.isEmpty || view.runtime == "waiting"
        let active = view.runtime == "active" || view.activeTurnId != nil
        let dispatching = view.queue.contains { $0.status == "dispatching" }
        let queued = view.queue.contains { $0.status == "queued" }
        let unknown = view.queue.contains { $0.status == "outcome_unknown" }
        if waiting { return "等待你处理" }
        if active { return "执行中" }
        if dispatching { return "正在提交到 Codex…" }
        if queued { return "队列中" }
        if unknown { return "发送结果待核对" }
        if !view.queue.isEmpty { return "队列已暂停" }
        return "空闲"
    }

    private var settingsPopoverContent: some View {
        let selectedModel = modelCatalog.first { $0.handle == desktop.view?.model }
        let availableModes = modeCatalog.filter { $0.handle == "default" || $0.handle == "plan" }
        let currentEffort = desktop.view?.reasoningEffort ?? ""
        let currentMode = desktop.view?.mode ?? .default

        return VStack(alignment: .leading, spacing: 10) {
            Text("模型、思考强度与模式").font(DesignFonts.body(13, weight: .semibold))

            if let modelErr = modelCatalogError {
                HStack(spacing: 6) {
                    Text("模型：\(modelErr)").font(DesignFonts.body(11)).foregroundStyle(palette.errFg.color)
                    Button("重试模型目录") {
                        Task { await loadSettingsCatalogs(forceModels: true, forceModes: false) }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(desktop.busy || settingsCatalogLoading)
                }
            }
            if let modeErr = modeCatalogError {
                HStack(spacing: 6) {
                    Text("模式：\(modeErr)").font(DesignFonts.body(11)).foregroundStyle(palette.errFg.color)
                    Button("重试模式目录") {
                        Task { await loadSettingsCatalogs(forceModels: false, forceModes: true) }
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .disabled(desktop.busy || settingsCatalogLoading)
                }
            }
            if let settingsError, !settingsError.isEmpty {
                Text(settingsError).font(DesignFonts.body(11)).foregroundStyle(palette.errFg.color)
            }
            if let settingsNotice, !settingsNotice.isEmpty {
                Text(settingsNotice).font(DesignFonts.body(11)).foregroundStyle(palette.textMuted.color)
            }

            VStack(alignment: .leading, spacing: 4) {
                Text("模型").font(DesignFonts.body(11, weight: .medium)).foregroundStyle(palette.textSecondary.color)
                Menu {
                    ForEach(modelCatalog) { model in
                        Button(model.name) {
                            settingsNotice = nil
                            applyDesktopSettings(
                                model: model.handle,
                                reasoningEffort: model.defaultReasoningEffort,
                                mode: currentMode
                            )
                        }
                    }
                } label: {
                    HStack {
                        Text(selectedModel?.name ?? desktop.view?.model ?? (settingsCatalogLoading ? "读取模型…" : "没有可用模型"))
                            .font(DesignFonts.body(12))
                        Spacer()
                        LucideIcon(.chevronDown, size: 12)
                    }
                    .padding(.horizontal, 8)
                    .frame(maxWidth: .infinity, minHeight: 30)
                    .background(RoundedRectangle(cornerRadius: 6).fill(palette.surface.color))
                }
                .disabled(desktop.busy || settingsCatalogLoading || modelCatalog.isEmpty)
            }

            if let selectedModel {
                VStack(alignment: .leading, spacing: 4) {
                    Text("思考强度").font(DesignFonts.body(11, weight: .medium)).foregroundStyle(palette.textSecondary.color)
                    Menu {
                        Button("继承原生默认") {
                            settingsNotice = nil
                            applyDesktopSettings(model: selectedModel.handle, reasoningEffort: nil, mode: currentMode)
                        }
                        ForEach(selectedModel.reasoningEfforts, id: \.self) { effort in
                            Button(effort) {
                                settingsNotice = nil
                                applyDesktopSettings(model: selectedModel.handle, reasoningEffort: effort, mode: currentMode)
                            }
                        }
                    } label: {
                        HStack {
                            Text(currentEffort.isEmpty ? "继承原生默认" : currentEffort)
                                .font(DesignFonts.body(12))
                            Spacer()
                            LucideIcon(.chevronDown, size: 12)
                        }
                        .padding(.horizontal, 8)
                        .frame(maxWidth: .infinity, minHeight: 30)
                        .background(RoundedRectangle(cornerRadius: 6).fill(palette.surface.color))
                    }
                    .disabled(desktop.busy || settingsCatalogLoading || selectedModel.reasoningEfforts.isEmpty)
                }
            }

            VStack(alignment: .leading, spacing: 4) {
                Text("模式").font(DesignFonts.body(11, weight: .medium)).foregroundStyle(palette.textSecondary.color)
                HStack(spacing: 6) {
                    ForEach(availableModes.isEmpty ? [
                        DesktopCatalogItem(handle: "default", kind: "mode", name: "默认", description: "", reasoningEfforts: []),
                        DesktopCatalogItem(handle: "plan", kind: "mode", name: "Plan", description: "", reasoningEfforts: []),
                    ] : availableModes) { modeItem in
                        let modeVal: DesktopMode = (modeItem.handle == "plan") ? .plan : .default
                        Button(modeItem.name) {
                            guard let selectedModel else { return }
                            let validEffort: String?
                            if !currentEffort.isEmpty && selectedModel.reasoningEfforts.contains(currentEffort) {
                                validEffort = currentEffort
                                settingsNotice = nil
                            } else {
                                validEffort = nil
                                settingsNotice = currentEffort.isEmpty ? nil : "当前思考强度不可用，将继承原生默认"
                            }
                            applyDesktopSettings(model: selectedModel.handle, reasoningEffort: validEffort, mode: modeVal)
                        }
                        .buttonStyle(currentMode == modeVal ? AnyButtonStyle(PrimaryButtonStyle()) : AnyButtonStyle(SecondaryButtonStyle()))
                        .disabled(desktop.busy || settingsCatalogLoading || selectedModel == nil || availableModes.isEmpty)
                    }
                }
            }
        }
        .padding(14)
        .frame(width: 280)
    }

    private func applyDesktopSettings(model: String, reasoningEffort: String?, mode: DesktopMode) {
        settingsError = nil
        Task {
            do {
                try await desktop.act(.settings(model: model, reasoningEffort: reasoningEffort, mode: mode))
                settingsError = nil
            } catch {
                settingsError = error.localizedDescription
            }
        }
    }

    private func loadSettingsCatalogs(forceModels: Bool = false, forceModes: Bool = false) async {
        settingsCatalogLoading = true
        defer { settingsCatalogLoading = false }
        if forceModels || modelCatalog.isEmpty || modelCatalogError != nil {
            do {
                let models = try await desktop.fetchCatalog(kind: "models", query: "", forceRefetch: forceModels)
                modelCatalog = models.items
                modelCatalogError = nil
            } catch {
                modelCatalogError = error.localizedDescription
            }
        }
        if forceModes || modeCatalog.isEmpty || modeCatalogError != nil {
            do {
                let modes = try await desktop.fetchCatalog(kind: "modes", query: "", forceRefetch: forceModes)
                modeCatalog = modes.items
                modeCatalogError = nil
            } catch {
                modeCatalogError = error.localizedDescription
            }
        }
    }

    private func openCatalogPicker(_ kind: String) {
        catalogPickerKind = kind
        catalogPickerQuery = ""
        selectedOptionIndex = 0
    }

    private var referenceCatalogPickerPanel: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Menu {
                    Button("插件与应用") { catalogPickerKind = "mentions"; catalogPickerQuery = "" }
                    Button("技能") { catalogPickerKind = "skills"; catalogPickerQuery = "" }
                    Button("应用") { catalogPickerKind = "apps"; catalogPickerQuery = "" }
                    Button("插件") { catalogPickerKind = "plugins"; catalogPickerQuery = "" }
                } label: {
                    Text(catalogKindLabel(catalogPickerKind))
                        .font(DesignFonts.body(12, weight: .medium))
                }
                .frame(width: 100)

                IMEGuardedInlineTextField(
                    text: $catalogPickerQuery,
                    placeholder: "搜索名称",
                    onSubmit: {},
                    onCancel: { catalogPickerKind = "" }
                )
                .frame(height: 28)
                .padding(.horizontal, 6)
                .background(RoundedRectangle(cornerRadius: 6).fill(palette.elevated.color))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(palette.borderStrong.color, lineWidth: 1))

                Button {
                    catalogPickerKind = ""
                } label: {
                    LucideIcon(.x, size: 13)
                }
                .buttonStyle(IconButtonStyle(dimension: 26))
                .accessibilityLabel("关闭引用")
            }

            if pickerBusy {
                Text("正在读取…").font(DesignFonts.body(11)).foregroundStyle(palette.textMuted.color)
            }
            ForEach(pickerErrors.keys.sorted(), id: \.self) { srcKind in
                if let msg = pickerErrors[srcKind] {
                    HStack(spacing: 6) {
                        Text("\(srcKind == "apps" ? "应用：" : (srcKind == "plugins" ? "插件：" : ""))\(msg)")
                            .font(DesignFonts.body(11))
                            .foregroundStyle(palette.errFg.color)
                        Button("重试") {
                            Task {
                                pickerBusy = true
                                let batch = await desktop.fetchReferenceCatalog(
                                    kind: catalogPickerKind,
                                    query: catalogPickerQuery,
                                    forceRefetch: true,
                                    retryKind: srcKind
                                )
                                pickerErrors.removeValue(forKey: srcKind)
                                for (k, v) in batch.errors { pickerErrors[k] = v }
                                if !batch.items.isEmpty {
                                    let existing = Set(pickerItems.map(\.handle))
                                    pickerItems.append(contentsOf: batch.items.filter { !existing.contains($0.handle) })
                                }
                                pickerBusy = false
                            }
                        }
                        .buttonStyle(SecondaryButtonStyle())
                    }
                }
            }

            ForEach(pickerItems.prefix(12)) { item in
                Button {
                    commitPickerReference(item.asChosenItem)
                } label: {
                    HStack(spacing: 6) {
                        Text(item.name)
                            .font(DesignFonts.body(12, weight: .semibold))
                            .foregroundStyle(palette.textPrimary.color)
                        Text("\(item.kind == "plugin" ? "插件" : (item.kind == "app" ? "应用" : "技能"))\(item.description.isEmpty ? "" : " · \(item.description)")")
                            .font(DesignFonts.body(11))
                            .foregroundStyle(palette.textMuted.color)
                            .lineLimit(1)
                        Spacer()
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 5)
                    .background(RoundedRectangle(cornerRadius: 6).fill(palette.surface.color))
                    .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())
            }

            if !pickerBusy && pickerErrors.isEmpty && pickerItems.isEmpty {
                Text(catalogEmptyLabel(catalogPickerKind))
                    .font(DesignFonts.body(11))
                    .foregroundStyle(palette.textMuted.color)
            }

            if !pickerNextCursors.isEmpty {
                Button("下一页") {
                    Task {
                        pickerBusy = true
                        let batch = await desktop.fetchReferenceCatalog(
                            kind: catalogPickerKind,
                            query: catalogPickerQuery,
                            cursors: pickerNextCursors,
                            nextPageOnly: true
                        )
                        pickerItems = batch.items
                        pickerNextCursors = batch.nextCursors
                        pickerErrors = batch.errors
                        pickerBusy = false
                    }
                }
                .buttonStyle(SecondaryButtonStyle())
                .disabled(pickerBusy)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 10).fill(palette.elevated.color))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
    }

    private func catalogKindLabel(_ kind: String) -> String {
        switch kind {
        case "skills": "技能"
        case "apps": "应用"
        case "plugins": "插件"
        default: "插件与应用"
        }
    }

    private func catalogEmptyLabel(_ kind: String) -> String {
        switch kind {
        case "apps": "没有可用应用"
        case "plugins": "没有可用插件"
        case "skills": "没有可用技能"
        default: "没有可用引用"
        }
    }

    private func commitPickerReference(_ item: DesktopChosenItem) {
        do {
            let inserted = try DesktopReferencesEngine.insert(draft, item: item, start: caretStart, end: caretEnd)
            draft = inserted.draft
            caretStart = inserted.caret
            caretEnd = inserted.caret
            targetCaretToRestore = inserted.caret
            catalogPickerKind = ""
            dismissedInlineKey = ""
            localError = nil
        } catch {
            localError = error.localizedDescription
        }
    }

    private func commitInlineReference(_ item: DesktopChosenItem) {
        let s = activeInlineQuery?.start ?? caretStart
        let e = activeInlineQuery?.end ?? caretEnd
        do {
            let inserted = try DesktopReferencesEngine.insert(draft, item: item, start: s, end: e)
            draft = inserted.draft
            caretStart = inserted.caret
            caretEnd = inserted.caret
            targetCaretToRestore = inserted.caret
            dismissedInlineKey = ""
            localError = nil
        } catch {
            localError = error.localizedDescription
        }
    }

    private func chooseAndAttachFile() async {
        do {
            guard let item = try await desktop.chooseItem(kind: "file") else { return }
            if draft.references.contains(where: { $0.handle == item.handle }) { return }
            guard draft.references.count < 32 else {
                localError = "引用数量超出限制，本次附件未添加"
                return
            }
            draft.references.append(item)
            localError = nil
        } catch {
            localError = error.localizedDescription
        }
    }

    private func sendDraft(steer: Bool) async {
        localError = nil
        let handles = draft.references.map(\.handle)
        do {
            try CodexDesktopStore.validateOutgoingPayload(text: draft.text, referencesCount: handles.count)
        } catch {
            localError = error.localizedDescription
            return
        }
        let utf8Spans = DesktopReferencesEngine.toUTF8ReferenceSpans(draft)
        let spansOpt = utf8Spans.isEmpty ? nil : utf8Spans
        let payloadKey = "\(draft.text)|\(handles.joined(separator: ","))|\(steer)"
        let messageId: String
        if let existing = idempotentSendState, existing.payloadKey == payloadKey {
            messageId = existing.messageId
        } else {
            let fresh = UUID().uuidString.lowercased()
            idempotentSendState = (payloadKey: payloadKey, messageId: fresh)
            messageId = fresh
        }

        do {
            if steer, let turnId = desktop.view?.activeTurnId {
                try await desktop.act(.steer(
                    messageId: messageId,
                    text: draft.text,
                    references: handles,
                    referenceSpans: spansOpt,
                    turnId: turnId
                ))
            } else {
                try await desktop.act(.send(
                    messageId: messageId,
                    text: draft.text,
                    references: handles,
                    referenceSpans: spansOpt
                ))
            }
            draft = ReferenceDraft()
            caretStart = 0
            caretEnd = 0
            targetCaretToRestore = 0
            idempotentSendState = nil
        } catch {
            // Retain draft and messageId so user retry stays idempotent!
        }
    }
}

// MARK: - Desktop Outbox & Pending Approvals / Secret Questions Subviews

public struct DesktopOutboxListView: View {
    @Environment(\.agentHubPalette) private var palette
    public let queue: [DesktopQueuedMessage]
    public let locked: Bool
    public let onCancelQueued: (String) -> Void
    public let onPromptDismissUnknown: (String) -> Void
    public let onResumeQueue: () -> Void

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("待发送 · \(queue.count)")
                .font(DesignFonts.body(12, weight: .semibold))
                .foregroundStyle(palette.textPrimary.color)
            ForEach(queue) { item in
                HStack(spacing: 8) {
                    StatusChip(outboxStatusLabel(item), tone: item.status == "outcome_unknown" ? .warning : .neutral)
                    Text(item.preview.isEmpty ? "引用内容" : item.preview)
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textSecondary.color)
                        .lineLimit(1)
                    Spacer()
                    if item.status == "outcome_unknown" {
                        Button("核对后移除") { onPromptDismissUnknown(item.id) }
                            .buttonStyle(SecondaryButtonStyle())
                            .disabled(locked)
                    } else if item.status == "queued" || item.status == "held" {
                        Button("取消") { onCancelQueued(item.id) }
                            .buttonStyle(SecondaryButtonStyle())
                            .disabled(locked)
                    }
                }
            }
            if queue.contains(where: { $0.status == "held" && $0.kind == "queue" }) {
                Button("继续发送队列", action: onResumeQueue)
                    .buttonStyle(PrimaryButtonStyle())
                    .disabled(locked)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 10).fill(palette.surface.color))
    }

    private func outboxStatusLabel(_ item: DesktopQueuedMessage) -> String {
        switch item.status {
        case "outcome_unknown": "结果待核对"
        case "held": item.kind == "steer" ? "补充未发送，请取消后重试" : "已暂停"
        case "dispatching": "正在提交到 Codex…"
        default: "等待线程空闲"
        }
    }
}

public struct DesktopRequestsListView: View {
    @Environment(\.agentHubPalette) private var palette
    public let requests: [DesktopPendingRequest]
    public let busy: Bool
    public let onAct: (DesktopAction) -> Void
    public let onOpenCodex: () -> Void

    @State private var expandedKey: String?
    @State private var openDiffFiles: Set<String> = []
    @State private var openExtraKeys: Set<String> = []
    @State private var questionChoices: [String: String] = [:]
    @State private var questionFreeAnswers: [String: String] = [:]

    private func resolvedQuestionAnswer(reqKey: String, questionId: String) -> String {
        let compound = "\(reqKey):\(questionId)"
        let free = questionFreeAnswers[compound] ?? ""
        if !free.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return free
        }
        return questionChoices[compound] ?? ""
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(requests.enumerated()), id: \.element.key) { index, req in
                let shown = (expandedKey == req.key) || (expandedKey == nil && index == 0)
                VStack(alignment: .leading, spacing: 8) {
                    Button {
                        expandedKey = shown ? "" : req.key
                    } label: {
                        HStack {
                            Text(req.title)
                                .font(DesignFonts.body(13, weight: .semibold))
                                .foregroundStyle(palette.textPrimary.color)
                            Spacer()
                            LucideIcon(shown ? .chevronDown : .chevronRight, size: 14)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(PlainHitButtonStyle())

                    if shown {
                        if let review = req.review {
                            approvalReviewView(reqKey: req.key, review: review)
                        } else if !req.body.isEmpty {
                            Text(req.body).font(DesignFonts.mono(12))
                        }

                        if req.canAccept && req.kind == "question" {
                            ForEach(req.questions) { q in
                                let compound = "\(req.key):\(q.id)"
                                DesktopQuestionFieldView(
                                    question: q,
                                    busy: busy,
                                    selectedChoice: questionChoices[compound] ?? "",
                                    freeAnswer: questionFreeAnswers[compound] ?? "",
                                    onSelectChoice: { label in
                                        questionChoices[compound] = label
                                        questionFreeAnswers[compound] = ""
                                    },
                                    onChangeFreeAnswer: { val in
                                        questionFreeAnswers[compound] = String(val.prefix(4096))
                                    }
                                )
                            }
                            Button("提交回答") {
                                let payload = req.questions.map { q in
                                    DesktopAnswer(questionId: q.id, answers: [resolvedQuestionAnswer(reqKey: req.key, questionId: q.id)])
                                }
                                onAct(.answer(requestKey: req.key, answers: payload))
                            }
                            .buttonStyle(PrimaryButtonStyle())
                            .disabled(busy || req.questions.contains { resolvedQuestionAnswer(reqKey: req.key, questionId: $0.id).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty })
                        } else if req.canAccept && req.kind == "elicitation" {
                            DesktopElicitationFormView(request: req, busy: busy, onAct: onAct)
                        } else {
                            HStack(spacing: 8) {
                                if ["command", "file", "permissions"].contains(req.kind) {
                                    Button("拒绝") { onAct(.decide(requestKey: req.key, allow: false, decision: nil)) }
                                        .buttonStyle(DangerButtonStyle(soft: true))
                                        .disabled(busy)
                                    if req.canAccept {
                                        Button("允许本次") { onAct(.decide(requestKey: req.key, allow: true, decision: nil)) }
                                            .buttonStyle(PrimaryButtonStyle())
                                            .disabled(busy)
                                    }
                                    if req.canAccept && ["command", "file"].contains(req.kind) && (req.supportedDecisions?.contains("acceptForSession") == true) {
                                        Button("允许本次会话") { onAct(.decide(requestKey: req.key, allow: true, decision: "acceptForSession")) }
                                            .buttonStyle(SecondaryButtonStyle())
                                            .disabled(busy)
                                    }
                                }
                                if req.kind == "elicitation" {
                                    Button("拒绝") { onAct(.elicitation(requestKey: req.key, accept: false, content: nil)) }
                                        .buttonStyle(DangerButtonStyle(soft: true))
                                        .disabled(busy)
                                }
                                if !req.canAccept {
                                    Text("请在 Codex 中查看完整请求并处理。")
                                        .font(DesignFonts.body(11))
                                        .foregroundStyle(palette.textMuted.color)
                                }
                                Button("在 Codex 中打开", action: onOpenCodex)
                                    .buttonStyle(SecondaryButtonStyle())
                                    .disabled(busy)
                            }
                        }
                    }
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 10).fill(palette.surface.color))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(palette.borderStrong.color, lineWidth: 1))
            }
        }
    }

    @ViewBuilder
    private func approvalReviewView(reqKey: String, review: DesktopReview) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if let cmd = review.command {
                Text(cmd)
                    .font(DesignFonts.mono(12))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .background(RoundedRectangle(cornerRadius: 6).fill(palette.codeBg.color))
            }
            if let cwd = review.cwd {
                HStack(spacing: 4) {
                    Text("工作目录：").font(DesignFonts.body(12)).foregroundStyle(palette.textSecondary.color)
                    Text(cwd).font(DesignFonts.mono(12)).foregroundStyle(palette.textPrimary.color)
                }
            }
            if let reason = review.reason {
                Text(reason).font(DesignFonts.body(12)).foregroundStyle(palette.textSecondary.color)
            }
            ForEach(Array(review.changes.enumerated()), id: \.offset) { fIdx, file in
                let fileKey = "\(reqKey):\(fIdx)"
                let fileOpen = openDiffFiles.contains(fileKey)
                Button {
                    if fileOpen { openDiffFiles.remove(fileKey) }
                    else { openDiffFiles.insert(fileKey) }
                } label: {
                    Text("\(fileOpen ? "▾" : "▸") \(file.path) · \(file.kind)\(file.truncated ? " · 已截断" : "")")
                        .font(DesignFonts.mono(11))
                        .foregroundStyle(palette.textSecondary.color)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())

                if fileOpen {
                    ScrollView(.horizontal) {
                        Text(file.diff)
                            .font(DesignFonts.mono(11))
                            .foregroundStyle(palette.textPrimary.color)
                            .padding(8)
                    }
                    .frame(maxHeight: 160)
                    .background(RoundedRectangle(cornerRadius: 6).fill(palette.codeBg.color))
                }
            }
            if let net = review.permissions?.network {
                Text("网络权限").font(DesignFonts.body(12, weight: .medium)).foregroundStyle(palette.textSecondary.color)
                Text(net).font(DesignFonts.mono(11)).padding(8).frame(maxWidth: .infinity, alignment: .leading).background(RoundedRectangle(cornerRadius: 6).fill(palette.codeBg.color))
            }
            if let fs = review.permissions?.fileSystem {
                Text("文件系统权限").font(DesignFonts.body(12, weight: .medium)).foregroundStyle(palette.textSecondary.color)
                Text(fs).font(DesignFonts.mono(11)).padding(8).frame(maxWidth: .infinity, alignment: .leading).background(RoundedRectangle(cornerRadius: 6).fill(palette.codeBg.color))
            }
            if let extra = review.extra, !extra.isEmpty {
                let extraOpen = openExtraKeys.contains(reqKey)
                Button {
                    if extraOpen { openExtraKeys.remove(reqKey) }
                    else { openExtraKeys.insert(reqKey) }
                } label: {
                    Text("\(extraOpen ? "▾" : "▸") 其他请求信息")
                        .font(DesignFonts.body(12))
                        .foregroundStyle(palette.textSecondary.color)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())
                if extraOpen {
                    Text(extra)
                        .font(DesignFonts.mono(11))
                        .padding(8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 6).fill(palette.codeBg.color))
                }
            }
            if review.truncated {
                Text("内容超出显示上限，请在 Codex 中查看完整请求并处理。")
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.errFg.color)
            }
        }
    }
}

private struct DesktopQuestionFieldView: View {
    @Environment(\.agentHubPalette) private var palette
    let question: DesktopQuestion
    let busy: Bool
    let selectedChoice: String
    let freeAnswer: String
    let onSelectChoice: (String) -> Void
    let onChangeFreeAnswer: (String) -> Void

    @State private var localText: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(question.header.isEmpty ? question.question : question.header)
                .font(DesignFonts.body(12, weight: .semibold))
            if !question.header.isEmpty {
                Text(question.question)
                    .font(DesignFonts.body(12))
                    .foregroundStyle(palette.textSecondary.color)
            }
            ForEach(Array(question.options.enumerated()), id: \.offset) { _, opt in
                let isSelected = (selectedChoice == opt.label) && freeAnswer.isEmpty
                Button {
                    localText = ""
                    onSelectChoice(opt.label)
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: isSelected ? "largecircle.fill.circle" : "circle")
                            .font(.system(size: 12))
                            .foregroundStyle(isSelected ? palette.accent.color : palette.textMuted.color)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(opt.label).font(DesignFonts.body(12, weight: .medium)).foregroundStyle(palette.textPrimary.color)
                            if !opt.description.isEmpty {
                                Text(opt.description).font(DesignFonts.body(11)).foregroundStyle(palette.textMuted.color)
                            }
                        }
                        Spacer()
                    }
                    .padding(.vertical, 3)
                    .contentShape(Rectangle())
                }
                .buttonStyle(PlainHitButtonStyle())
                .disabled(busy)
            }
            Text(question.options.isEmpty ? "回答" : "或填写回答")
                .font(DesignFonts.body(11))
                .foregroundStyle(palette.textSecondary.color)
            Group {
                if question.secret {
                    SecureField("输入密钥或敏感回答", text: $localText)
                        .textFieldStyle(.roundedBorder)
                } else {
                    TextField("回答", text: $localText)
                        .textFieldStyle(.roundedBorder)
                }
            }
            .disabled(busy)
            .onAppear { localText = freeAnswer }
            .onChange(of: freeAnswer) { next in
                if localText != next { localText = next }
            }
            .onChange(of: localText) { next in
                let clamped = String(next.prefix(4096))
                if clamped != localText { localText = clamped }
                onChangeFreeAnswer(clamped)
            }
        }
    }
}

private struct DesktopElicitationFormView: View {
    @Environment(\.agentHubPalette) private var palette
    let request: DesktopPendingRequest
    let busy: Bool
    let onAct: (DesktopAction) -> Void

    @State private var rawValues: [String: String] = [:]
    @State private var formError: String?

    var body: some View {
        let fields = ElicitationSchemaFormEngine.parseFields(from: request.formSchema)
        VStack(alignment: .leading, spacing: 8) {
            ForEach(fields) { field in
                DesktopElicitationFieldRow(
                    field: field,
                    busy: busy,
                    value: rawValues[field.key] ?? "",
                    onChange: { rawValues[field.key] = $0 }
                )
            }
            if let formError, !formError.isEmpty {
                Text(formError).font(DesignFonts.body(12)).foregroundStyle(palette.errFg.color)
            }
            HStack(spacing: 8) {
                Button("拒绝") {
                    onAct(.elicitation(requestKey: request.key, accept: false, content: nil))
                }
                .buttonStyle(DangerButtonStyle(soft: true))
                .disabled(busy)

                Button("提交") {
                    do {
                        let content = try ElicitationSchemaFormEngine.buildSubmissionContent(fields: fields, rawValues: rawValues)
                        formError = nil
                        onAct(.elicitation(requestKey: request.key, accept: true, content: content))
                    } catch {
                        formError = error.localizedDescription
                    }
                }
                .buttonStyle(PrimaryButtonStyle())
                .disabled(busy)
            }
        }
    }
}

private struct DesktopElicitationFieldRow: View {
    @Environment(\.agentHubPalette) private var palette
    let field: ElicitationSchemaFormEngine.FieldSpec
    let busy: Bool
    let value: String
    let onChange: (String) -> Void

    @State private var localText: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text("\(field.title)\(field.required ? " *" : "")")
                .font(DesignFonts.body(12, weight: .medium))
            if let choices = field.choices {
                Menu {
                    Button("请选择") { onChange("") }
                    ForEach(Array(choices.enumerated()), id: \.offset) { _, choice in
                        Button(choice.label) { onChange(choice.encodedJSON) }
                    }
                } label: {
                    let currentLabel = choices.first(where: { $0.encodedJSON == value })?.label ?? "请选择"
                    HStack {
                        Text(currentLabel).font(DesignFonts.body(12))
                        Spacer()
                        LucideIcon(.chevronDown, size: 12)
                    }
                    .padding(.horizontal, 8)
                    .frame(maxWidth: .infinity, minHeight: 28)
                    .background(RoundedRectangle(cornerRadius: 6).fill(palette.elevated.color))
                }
                .disabled(busy)
            } else {
                TextField(field.title, text: $localText)
                    .textFieldStyle(.roundedBorder)
                    .disabled(busy)
                    .onAppear { localText = value }
                    .onChange(of: localText) { next in
                        let clamped = String(next.prefix(field.maxLength))
                        if clamped != localText { localText = clamped }
                        onChange(clamped)
                    }
            }
            if let desc = field.description {
                Text(desc).font(DesignFonts.body(11)).foregroundStyle(palette.textMuted.color)
            }
        }
    }
}
