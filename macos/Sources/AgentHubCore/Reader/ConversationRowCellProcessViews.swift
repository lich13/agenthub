import AppKit
import SwiftUI

enum ProcessRowLayoutMetrics {
    static let compactRowHeight: CGFloat = 30
    static let thoughtDisclosureHeaderHeight: CGFloat = 32
    static let collapsedGroupRowHeight: CGFloat = 34
}

@MainActor
private final class CapsulePillBoxNSView: NSView {
    override func layout() {
        super.layout()
        layer?.cornerRadius = min(bounds.width, bounds.height) * 0.5
    }
}

extension ConversationRowCellView {
    func buildExecutionGroupView(
        group: ExecutionGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil,
        nestedInRail: Bool = false
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let aliases = group.stateAliases
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: group.id,
            aliases: aliases,
            initiallyOpen: group.initiallyOpen
        )
        let header = group.summaryHeader
        let countMeta = "\(group.commands.count) \(group.kind == "tool" ? "项工具" : "条命令")"
            + (group.failedCount > 0 ? " · \(group.failedCount) 条失败" : "")

        let summaryRow = makeProcessSummaryRow(
            gutterIcon: header.icon,
            nestedInRail: nestedInRail,
            title: header.label,
            metaPill: countMeta,
            failureCount: 0,
            extraPills: [],
            running: group.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: group.id, aliases: aliases, currentlyOpen: isOpen)
        }
        destStack.addArrangedSubview(summaryRow)

        // Lazy child creation: do NOT build inner views when collapsed!
        guard isOpen else { return }
        let bodyWidth = max(120, width - 20)
        let cmdStack = makeExecutionCommandsList(
            commands: group.commands,
            width: bodyWidth,
            wrappedInSurfaceCard: false,
            palette: palette,
            coordinator: coordinator
        )
        destStack.addArrangedSubview(makeLeftRailContainer(content: cmdStack, width: width, palette: palette))
    }

    func buildProcessGroupView(
        proc: UnifiedProcessGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: proc.stateKey,
            aliases: proc.stateAliases,
            initiallyOpen: proc.initiallyOpen
        )
        let summary = proc.summaryText
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: proc.iconName,
            title: summary.primary,
            metaPill: summary.meta,
            failureCount: proc.failedCount,
            extraPills: [],
            running: proc.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: proc.stateKey, aliases: proc.stateAliases, currentlyOpen: isOpen)
        }
        containerStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let bodyWidth = max(120, width - 22)
        let wrapThinking = proc.toolCount > 0 || proc.thoughtCount > 1
        let stepsStack = NSStackView()
        stepsStack.orientation = .vertical
        stepsStack.alignment = .leading
        stepsStack.spacing = 8
        stepsStack.translatesAutoresizingMaskIntoConstraints = false

        for step in proc.steps {
            switch step {
            case let .thinking(id, sids, text, stepRunning, _):
                if wrapThinking {
                    stepsStack.addArrangedSubview(
                        makeInnerThinkingDisclosureView(
                            stateKey: "\(id):thinking",
                            aliases: sids.map { "\($0):thinking" },
                            markdown: text,
                            running: stepRunning,
                            width: bodyWidth,
                            palette: palette,
                            coordinator: coordinator
                        )
                    )
                } else {
                    stepsStack.addArrangedSubview(
                        makeThoughtProseView(markdown: text, width: bodyWidth, palette: palette, coordinator: coordinator)
                    )
                }
            case let .execution(group, _):
                stepsStack.addArrangedSubview(
                    makeExecutionCommandsList(
                        commands: group.commands,
                        width: bodyWidth,
                        wrappedInSurfaceCard: true,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            case let .detail(id, sids, title, detail, running, failed, _):
                let detailCmd = ExecutionCommand(
                    id: id,
                    sourceIds: sids,
                    title: title,
                    preview: nil,
                    status: failed ? .failed : (running ? .running : .completed),
                    instructionFile: false,
                    sections: [ExecutionSection(label: "", text: detail)]
                )
                stepsStack.addArrangedSubview(
                    makeExecutionCommandsList(
                        commands: [detailCmd],
                        width: bodyWidth,
                        wrappedInSurfaceCard: false,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            }
        }
        containerStack.addArrangedSubview(makeLeftRailContainer(content: stepsStack, width: width, palette: palette))
    }

    func buildClaudeProcessGroupView(
        proc: ClaudeProcessGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let aliases = proc.sourceIds.map { "claude-process:\(proc.turnId):\($0)" }
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: proc.stateKey,
            aliases: aliases,
            initiallyOpen: false
        )
        var extra: [String] = []
        if proc.needsReply { extra.append("待回复") }
        if proc.hasPermission { extra.append(proc.needsPermission ? "待授权" : "授权记录") }
        if !proc.running && (proc.needsReply || proc.needsPermission) { extra.append("展开查看") }

        let summaryRow = makeProcessSummaryRow(
            gutterIcon: proc.iconName,
            title: proc.labelText,
            metaPill: proc.metaText,
            failureCount: proc.failedCount,
            extraPills: extra,
            running: proc.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: proc.stateKey, aliases: aliases, currentlyOpen: isOpen)
        }
        containerStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let bodyWidth = max(120, width - 22)
        let innerStack = buildClaudeEntriesStack(
            entries: proc.entries,
            width: bodyWidth,
            wrapThinkingDisclosures: proc.wrapsThinkingDisclosures,
            coordinator: coordinator
        )
        containerStack.addArrangedSubview(makeLeftRailContainer(content: innerStack, width: width, palette: palette))
    }

    func buildClaudeMetadataGroupView(
        meta: ClaudeMetadataGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: meta.id,
            aliases: aliases,
            initiallyOpen: false
        )
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: nil,
            title: meta.label,
            metaPill: nil,
            failureCount: 0,
            extraPills: [],
            running: false,
            open: isOpen,
            expandsChevronToTrailing: true,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: meta.id, aliases: aliases, currentlyOpen: isOpen)
        }
        destStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let bodyWidth = max(100, width - 22)
        let innerStack = buildClaudeEntriesStack(
            entries: meta.renderItems,
            width: bodyWidth,
            wrapThinkingDisclosures: false,
            coordinator: coordinator
        )
        destStack.addArrangedSubview(makeLeftRailContainer(content: innerStack, width: width, palette: palette))
    }

    private func buildClaudeEntriesStack(
        entries: [ClaudeEventRenderItem],
        width: CGFloat,
        wrapThinkingDisclosures: Bool,
        coordinator: ConversationTableCoordinator
    ) -> NSStackView {
        let palette = coordinator.palette
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false

        for entry in entries {
            switch entry {
            case let .group(group):
                buildExecutionGroupView(group: group, width: width, coordinator: coordinator, into: stack, nestedInRail: true)
            case let .metadata(meta):
                buildClaudeMetadataGroupView(meta: meta, width: width, coordinator: coordinator, into: stack)
            case let .item(ev, key, sids):
                if ClaudeProcessGroupingEngine.isThoughtOrCommentary(ev) {
                    let markdown = ev.text ?? ""
                    guard !VisibleMarkdownEngine.visibleMarkdown(markdown).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }
                    if wrapThinkingDisclosures {
                        stack.addArrangedSubview(
                            makeInnerThinkingDisclosureView(
                                stateKey: "\(key):thinking",
                                aliases: sids.map { "\($0):thinking" },
                                markdown: markdown,
                                running: ev.kind == "thinking" && ev.status == "running",
                                width: width,
                                palette: palette,
                                coordinator: coordinator
                            )
                        )
                    } else {
                        stack.addArrangedSubview(makeThoughtProseView(markdown: markdown, width: width, palette: palette, coordinator: coordinator))
                    }
                } else if let detail = ev.detail, !detail.isEmpty {
                    let cmd = ExecutionCommand(
                        id: key,
                        sourceIds: sids,
                        title: ev.text ?? ev.role ?? "活动记录",
                        preview: nil,
                        status: ExecutionGroupingEngine.executionStatus(ev.status),
                        instructionFile: false,
                        sections: [ExecutionSection(label: "", text: detail)]
                    )
                    stack.addArrangedSubview(
                        makeExecutionCommandsList(
                            commands: [cmd],
                            width: width,
                            wrappedInSurfaceCard: false,
                            palette: palette,
                            coordinator: coordinator
                        )
                    )
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    stack.addArrangedSubview(makeThoughtProseView(markdown: text, width: width, palette: palette, coordinator: coordinator))
                }
            }
        }
        return stack
    }

    func buildThinkingView(
        id: String,
        aliases: [String],
        title: String,
        text: String,
        running: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let open = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: id,
            aliases: aliases,
            initiallyOpen: false
        )
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: .brain,
            title: running ? "正在思考…" : title,
            metaPill: nil,
            failureCount: 0,
            extraPills: [],
            running: running,
            open: open,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: id, aliases: aliases, currentlyOpen: open)
        }
        containerStack.addArrangedSubview(summaryRow)
        guard open else { return }
        let thoughtView = makeThoughtProseView(markdown: text, width: max(120, width - 22), palette: palette, coordinator: coordinator)
        containerStack.addArrangedSubview(makeLeftRailContainer(content: thoughtView, width: width, palette: palette))
    }

    static func thoughtPreviewSnippet(_ markdown: String) -> String {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(markdown)
        for rawLine in cleaned.components(separatedBy: .newlines) {
            let trimmed = rawLine
                .replacing(MarkdownPatterns.markdownBlockMarkers, with: "")
                .replacing(MarkdownPatterns.whitespace, with: " ")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty {
                let ns = trimmed as NSString
                return ns.length > 140 ? "\(ns.substring(to: 140))…" : trimmed
            }
        }
        return "思考内容"
    }

    private func makeInnerThinkingDisclosureView(
        stateKey: String,
        aliases: [String],
        markdown: String,
        running: Bool,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let open = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: stateKey,
            aliases: aliases,
            initiallyOpen: false
        )
        let itemCol = NSStackView()
        itemCol.orientation = .vertical
        itemCol.alignment = .leading
        itemCol.spacing = 6
        itemCol.translatesAutoresizingMaskIntoConstraints = false
        itemCol.widthAnchor.constraint(equalToConstant: width).isActive = true

        let header = HoverSummaryRowView(
            open: false,
            isEnabled: true,
            hoverFill: palette.hover.nsColor.withAlphaComponent(0.55),
            openFill: .clear,
            focusRingColor: palette.accent.nsColor,
            accessibilityExpanded: open,
            accessibilityLabelText: "思考内容"
        )
        header.onTap = { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: stateKey, aliases: aliases, currentlyOpen: open)
        }
        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 8
        hStack.translatesAutoresizingMaskIntoConstraints = false
        header.addSubview(hStack)

        let brainIcon = NSImageView(
            image: LucideVectorCatalog.nsImage(for: .brain, size: 14, color: palette.accent.nsColor)
        )
        brainIcon.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            brainIcon.widthAnchor.constraint(equalToConstant: 14),
            brainIcon.heightAnchor.constraint(equalToConstant: 14),
        ])
        hStack.addArrangedSubview(brainIcon)

        let titleAttr = NSMutableAttributedString(
            string: running ? "正在思考…" : "思考",
            attributes: [
                .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                .foregroundColor: palette.textPrimary.nsColor,
            ]
        )
        let preview = Self.thoughtPreviewSnippet(markdown)
        if !preview.isEmpty {
            titleAttr.append(
                NSAttributedString(
                    string: " \(preview)",
                    attributes: [
                        .font: DesignFonts.nsBodyFont(size: 12),
                        .foregroundColor: palette.textMuted.nsColor,
                    ]
                )
            )
        }
        let titleLbl = NSTextField(labelWithAttributedString: titleAttr)
        titleLbl.lineBreakMode = .byTruncatingTail
        titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        hStack.addArrangedSubview(titleLbl)

        hStack.addArrangedSubview(NSView())
        if running {
            hStack.addArrangedSubview(
                AppKitRunningRingView(
                    size: 13,
                    trackColor: palette.borderStrong.nsColor,
                    accentColor: palette.accent.nsColor
                )
            )
        }

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
            header.widthAnchor.constraint(equalToConstant: width),
            header.heightAnchor.constraint(equalToConstant: ProcessRowLayoutMetrics.thoughtDisclosureHeaderHeight),
            hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor, constant: 4),
            hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -4),
            hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
        ])
        hStack.addArrangedSubview(chev)
        itemCol.addArrangedSubview(header)

        if open {
            itemCol.addArrangedSubview(
                makeThoughtProseView(markdown: markdown, width: width, palette: palette, coordinator: coordinator)
            )
        }
        return itemCol
    }

    private func makeThoughtProseView(
        markdown: String,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let layout = coordinator.cachedThoughtLayout(markdown: markdown, width: width)
        let attr = layout.attributed
        let h = layout.height
        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: width, height: h))
        tv.applyAttributedText(attr, width: width)
        tv.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            tv.widthAnchor.constraint(equalToConstant: width),
            tv.heightAnchor.constraint(equalToConstant: h),
        ])
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        return tv
    }

    private func makeProcessSummaryRow(
        gutterIcon: LucideIconName?,
        nestedInRail: Bool = false,
        title: String,
        metaPill: String?,
        failureCount: Int,
        extraPills: [String],
        running: Bool,
        open: Bool,
        expandsChevronToTrailing: Bool,
        width: CGFloat,
        palette: DesignPalette,
        action: @escaping () -> Void
    ) -> NSView {
        let row = HoverSummaryRowView(
            open: open,
            isEnabled: true,
            hoverFill: palette.surface.nsColor,
            openFill: palette.surface.nsColor,
            focusRingColor: palette.accent.nsColor,
            accessibilityExpanded: open,
            accessibilityLabelText: title
        )
        row.clipsToBounds = false
        row.onTap = action

        if let gutterIcon, !nestedInRail {
            let iconView = NSImageView(
                image: LucideVectorCatalog.nsImage(for: gutterIcon, size: 15, color: palette.accent.nsColor)
            )
            iconView.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(iconView)
            NSLayoutConstraint.activate([
                iconView.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: -20),
                iconView.centerYAnchor.constraint(equalTo: row.centerYAnchor),
                iconView.widthAnchor.constraint(equalToConstant: 15),
                iconView.heightAnchor.constraint(equalToConstant: 15),
            ])
        }

        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 8
        hStack.translatesAutoresizingMaskIntoConstraints = false
        row.addSubview(hStack)

        if let gutterIcon, nestedInRail {
            let inlineIcon = NSImageView(
                image: LucideVectorCatalog.nsImage(for: gutterIcon, size: 14, color: palette.accent.nsColor)
            )
            inlineIcon.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                inlineIcon.widthAnchor.constraint(equalToConstant: 14),
                inlineIcon.heightAnchor.constraint(equalToConstant: 14),
            ])
            hStack.addArrangedSubview(inlineIcon)
        }

        let titleLbl = NSTextField(labelWithString: title)
        titleLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        titleLbl.textColor = (open ? palette.textPrimary : palette.textSecondary).nsColor
        titleLbl.lineBreakMode = .byTruncatingTail
        titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        hStack.addArrangedSubview(titleLbl)

        if let metaPill, !metaPill.isEmpty {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: metaPill,
                    textColor: palette.textMuted.nsColor,
                    fill: palette.elevated.nsColor,
                    border: palette.borderStrong.nsColor,
                    semibold: false
                )
            )
        }

        if failureCount > 0 {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: "\(failureCount) 项失败",
                    textColor: palette.errFg.nsColor,
                    fill: palette.errBg.nsColor,
                    border: palette.errBorder.nsColor,
                    semibold: true
                )
            )
        }

        for extra in extraPills {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: extra,
                    textColor: palette.textMuted.nsColor,
                    fill: palette.elevated.nsColor,
                    border: palette.borderStrong.nsColor,
                    semibold: false
                )
            )
        }

        if running || expandsChevronToTrailing {
            hStack.addArrangedSubview(NSView())
        }
        if running {
            hStack.addArrangedSubview(
                AppKitRunningRingView(
                    size: 13,
                    trackColor: palette.borderStrong.nsColor,
                    accentColor: palette.accent.nsColor
                )
            )
        }

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
        ])
        hStack.addArrangedSubview(chev)
        if !running && !expandsChevronToTrailing {
            hStack.addArrangedSubview(NSView())
        }

        NSLayoutConstraint.activate([
            row.widthAnchor.constraint(equalToConstant: width),
            row.heightAnchor.constraint(equalToConstant: ProcessRowLayoutMetrics.compactRowHeight),
            hStack.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: open ? 6 : 0),
            hStack.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -8),
            hStack.centerYAnchor.constraint(equalTo: row.centerYAnchor),
        ])
        return row
    }

    private func makeCapsulePill(
        text: String,
        textColor: NSColor,
        fill: NSColor,
        border: NSColor,
        semibold: Bool
    ) -> NSView {
        let box = CapsulePillBoxNSView()
        box.translatesAutoresizingMaskIntoConstraints = false
        box.wantsLayer = true
        box.layer?.cornerRadius = 10
        box.layer?.backgroundColor = fill.cgColor
        box.layer?.borderWidth = 1
        box.layer?.borderColor = border.cgColor

        let lbl = NSTextField(labelWithString: text)
        lbl.font = DesignFonts.nsBodyFont(size: 11, weight: semibold ? .semibold : .regular)
        lbl.textColor = textColor
        lbl.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(lbl)
        NSLayoutConstraint.activate([
            box.heightAnchor.constraint(equalToConstant: 20),
            lbl.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 8),
            lbl.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            lbl.centerYAnchor.constraint(equalTo: box.centerYAnchor),
        ])
        return box
    }

    private func makeLeftRailContainer(
        content: NSView,
        width: CGFloat,
        palette: DesignPalette
    ) -> NSView {
        let wrap = NSView()
        wrap.translatesAutoresizingMaskIntoConstraints = false

        let rail = NSView()
        rail.translatesAutoresizingMaskIntoConstraints = false
        rail.wantsLayer = true
        rail.layer?.backgroundColor = palette.borderStrong.nsColor.cgColor

        content.translatesAutoresizingMaskIntoConstraints = false
        wrap.addSubview(rail)
        wrap.addSubview(content)
        NSLayoutConstraint.activate([
            wrap.widthAnchor.constraint(equalToConstant: width),
            rail.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 2),
            rail.topAnchor.constraint(equalTo: wrap.topAnchor, constant: 4),
            rail.bottomAnchor.constraint(equalTo: wrap.bottomAnchor, constant: -2),
            rail.widthAnchor.constraint(equalToConstant: 2),
            content.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 16),
            content.trailingAnchor.constraint(equalTo: wrap.trailingAnchor, constant: -4),
            content.topAnchor.constraint(equalTo: wrap.topAnchor, constant: 4),
            content.bottomAnchor.constraint(equalTo: wrap.bottomAnchor, constant: -4),
        ])
        return wrap
    }

    private func makeExecutionCommandsList(
        commands: [ExecutionCommand],
        width: CGFloat,
        wrappedInSurfaceCard: Bool,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let listStack = NSStackView()
        listStack.orientation = .vertical
        listStack.alignment = .leading
        listStack.spacing = 0
        listStack.translatesAutoresizingMaskIntoConstraints = false
        let innerW = wrappedInSurfaceCard ? max(100, width - 20) : width

        for (idx, cmd) in commands.enumerated() {
            let cmdOpen = coordinator.disclosureStore.isOpen(
                scope: coordinator.sessionScopeKey,
                stateKey: cmd.id,
                aliases: cmd.sourceIds,
                initiallyOpen: !cmd.instructionFile && (cmd.status == .running || cmd.status == .failed)
            )
            let itemCol = NSStackView()
            itemCol.orientation = .vertical
            itemCol.alignment = .leading
            itemCol.spacing = 6
            itemCol.translatesAutoresizingMaskIntoConstraints = false
            itemCol.widthAnchor.constraint(equalToConstant: innerW).isActive = true

            let header = HoverSummaryRowView(
                open: false,
                isEnabled: true,
                hoverFill: palette.hover.nsColor.withAlphaComponent(0.55),
                openFill: .clear,
                focusRingColor: palette.accent.nsColor,
                accessibilityExpanded: cmdOpen,
                accessibilityLabelText: cmd.title
            )
            header.onTap = { [weak coordinator] in
                coordinator?.toggleDisclosurePreservingAnchor(stateKey: cmd.id, aliases: cmd.sourceIds, currentlyOpen: cmdOpen)
            }
            let hStack = NSStackView()
            hStack.orientation = .horizontal
            hStack.alignment = .centerY
            hStack.spacing = 8
            hStack.translatesAutoresizingMaskIntoConstraints = false
            header.addSubview(hStack)

            let termIcon = NSImageView(
                image: LucideVectorCatalog.nsImage(for: .terminal, size: 15, color: palette.accent.nsColor)
            )
            termIcon.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                termIcon.widthAnchor.constraint(equalToConstant: 15),
                termIcon.heightAnchor.constraint(equalToConstant: 15),
            ])
            hStack.addArrangedSubview(termIcon)

            let titleAttr = NSMutableAttributedString()
            if cmd.instructionFile {
                titleAttr.append(NSAttributedString(string: "AGENTS.md", attributes: [
                    .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                    .foregroundColor: palette.textPrimary.nsColor,
                ]))
            } else {
                titleAttr.append(NSAttributedString(string: VisibleMarkdownEngine.visibleMarkdown(cmd.title), attributes: [
                    .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                    .foregroundColor: palette.textPrimary.nsColor,
                ]))
                if let preview = cmd.preview, !preview.isEmpty {
                    titleAttr.append(NSAttributedString(string: " \(VisibleMarkdownEngine.visibleMarkdown(preview))", attributes: [
                        .font: DesignFonts.nsBodyFont(size: 12),
                        .foregroundColor: palette.textMuted.nsColor,
                    ]))
                }
            }
            let titleLbl = NSTextField(labelWithAttributedString: titleAttr)
            titleLbl.lineBreakMode = .byTruncatingTail
            titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            hStack.addArrangedSubview(titleLbl)

            hStack.addArrangedSubview(NSView())
            if cmd.status == .running {
                hStack.addArrangedSubview(
                    AppKitRunningRingView(
                        size: 13,
                        trackColor: palette.borderStrong.nsColor,
                        accentColor: palette.accent.nsColor
                    )
                )
            }

            let stLbl = NSTextField(labelWithString: cmd.status.label)
            stLbl.font = DesignFonts.nsBodyFont(size: 12)
            stLbl.textColor = (cmd.status == .failed ? palette.errFg : palette.textMuted).nsColor
            hStack.addArrangedSubview(stLbl)

            let chev = NSImageView(
                image: LucideVectorCatalog.nsImage(
                    for: cmdOpen ? .chevronDown : .chevronRight,
                    size: 16,
                    color: (cmdOpen ? palette.accent : palette.textMuted).nsColor
                )
            )
            chev.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                chev.widthAnchor.constraint(equalToConstant: 16),
                chev.heightAnchor.constraint(equalToConstant: 16),
                header.widthAnchor.constraint(equalToConstant: innerW),
                header.heightAnchor.constraint(equalToConstant: ProcessRowLayoutMetrics.compactRowHeight),
                hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor, constant: 4),
                hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -4),
                hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            ])
            hStack.addArrangedSubview(chev)
            itemCol.addArrangedSubview(header)

            if cmdOpen {
                let detail = cmd.sections.map { sec in
                    sec.label.isEmpty ? sec.text : "\(sec.label)\n\(sec.text)"
                }.joined(separator: "\n\n")
                itemCol.addArrangedSubview(
                    makeMonospaceDetailBox(
                        text: detail.isEmpty ? "暂无输出" : detail,
                        width: innerW,
                        palette: palette,
                        coordinator: coordinator,
                        linkifyFiles: true
                    )
                )
            }

            if idx < commands.count - 1 {
                let sep = NSView()
                sep.translatesAutoresizingMaskIntoConstraints = false
                sep.wantsLayer = true
                sep.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                NSLayoutConstraint.activate([
                    sep.widthAnchor.constraint(equalToConstant: innerW),
                    sep.heightAnchor.constraint(equalToConstant: 1),
                ])
                itemCol.addArrangedSubview(sep)
            }
            listStack.addArrangedSubview(itemCol)
        }

        if !wrappedInSurfaceCard {
            return listStack
        }
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 10
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        card.addSubview(listStack)
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            listStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 10),
            listStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -10),
            listStack.topAnchor.constraint(equalTo: card.topAnchor, constant: 2),
            listStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -2),
        ])
        return card
    }

    private func makeMonospaceDetailBox(
        text: String,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator,
        linkifyFiles: Bool
    ) -> NSView {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
        let pStyle = NSMutableParagraphStyle()
        pStyle.minimumLineHeight = 19.2 // 12pt * 1.6
        pStyle.maximumLineHeight = 19.2
        let baseAttrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsMonoFont(size: 12),
            .foregroundColor: palette.textSecondary.nsColor,
            .paragraphStyle: pStyle,
        ]
        let attr = NSMutableAttributedString()
        if linkifyFiles {
            let spans = FileLinksAndPlanExport.fileLinkSpans(cleaned)
            let ns = cleaned as NSString
            var cursor = 0
            for span in spans where span.nsRange.location >= cursor {
                if span.nsRange.location > cursor {
                    let prefix = ns.substring(with: NSRange(location: cursor, length: span.nsRange.location - cursor))
                    attr.append(NSAttributedString(string: prefix, attributes: baseAttrs))
                }
                var linkAttrs = baseAttrs
                linkAttrs[.foregroundColor] = palette.accent.nsColor
                linkAttrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
                linkAttrs[.link] = span.href
                attr.append(NSAttributedString(string: span.label, attributes: linkAttrs))
                cursor = span.nsRange.location + span.nsRange.length
            }
            if cursor < ns.length {
                attr.append(NSAttributedString(string: ns.substring(from: cursor), attributes: baseAttrs))
            }
        } else {
            attr.append(NSAttributedString(string: cleaned, attributes: baseAttrs))
        }

        let boxWidth = max(106, width)
        let innerW = boxWidth - 26
        let contentH = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: innerW))
        let boxH = min(240, max(48, contentH + 22))

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: true)
        scroll.hasVerticalScroller = contentH + 22 > boxH
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = palette.codeBg.nsColor
        scroll.wantsLayer = true
        scroll.layer?.cornerRadius = 10
        scroll.layer?.borderWidth = 1
        scroll.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let tv = ReaderSelectableTextView(
            frame: NSRect(x: 0, y: 0, width: boxWidth, height: max(boxH, contentH + 22))
        )
        tv.textContainerInset = NSSize(width: 13, height: 11)
        tv.applyAttributedText(attr, width: boxWidth)
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        scroll.documentView = tv
        scroll.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: boxWidth),
            scroll.heightAnchor.constraint(equalToConstant: boxH),
        ])
        return scroll
    }
}
